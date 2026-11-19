//! Skraft's viewport renderer (docs/render/display.md, docs/render/look.md).
//!
//! The renderer knows meshes by key, never the document: the caller (the display scene in
//! `tools`) uploads one mesh per definition — the concatenation of its direct solids, each a
//! [`Part`] with its own index range — and draws each definition once, instanced over all its
//! placements. Highlights redraw one part of one placement over it.
//!
//! It draws into its own off-screen target (MSAA colour + depth, resolved into a sampleable
//! texture), so it is independent of the UI and testable without a window. A frame that would
//! draw the same picture as the last one draws nothing: the target keeps it.
//!
//! Precision (docs/render/display.md): the GPU only sees `f32`, so everything is drawn
//! **relative to the camera**. Placements, the view and lines are `f64`; each placement's
//! `view × world × recentre` is computed on the CPU in `f64` and only then made `f32`, and each
//! definition's mesh is stored around its own centre. Depth is **reversed** (1 near, 0 far) in
//! a float depth buffer (with a stencil, for section caps): [`Frame::proj`] must map near to
//! 1, e.g. `Mat4::perspective_infinite_reverse_rh`.
//!
//! Colours (docs/render/look.md): faces take a colour from the frame's **palette** — the face's own
//! (baked in the mesh, [`Part::face_paint`]), else its placement's ([`Placement::material`]),
//! else [`Style::face_front`]. Changing a palette colour re-uploads no vertex.
//!
//! A **ghost** batch (an open part's blank, docs/model/opening-a-part.md) has its edges drawn like any
//! other, but its faces only after everything opaque: their fronts, blended at
//! [`Style::ghost_opacity`], depth tested without writing depth. With
//! [`Batch::faint_edges`], its edges and silhouettes are drawn so too, at
//! [`Style::ghost_edge_opacity`] (what the eraser would take).

use std::hash::{Hash, Hasher};
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::{DMat3, DMat4, DVec3, Mat4, Vec3};
use skraft_kernel::Mesh;
use wgpu::util::DeviceExt;

/// The format the viewport is drawn in: sRGB-encoded bytes, blended in linear.
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// The format [`Renderer::target`] is viewed with: the same bytes, without sRGB decoding,
/// which is what egui's renderer expects of a native texture.
pub const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Depth with a stencil (section caps, docs/render/look.md): 32-bit float when the device has it — reversed
/// depth needs it to stay precise far away — else whatever the device picks for 24 bits (on
/// Apple Silicon, 32-bit float anyway).
fn depth_format(device: &wgpu::Device) -> wgpu::TextureFormat {
    if device.features().contains(wgpu::Features::DEPTH32FLOAT_STENCIL8) {
        wgpu::TextureFormat::Depth32FloatStencil8
    } else {
        wgpu::TextureFormat::Depth24PlusStencil8
    }
}
const SAMPLES: u32 = 4;

/// No palette entry: the colour comes from the next level (placement, then the default).
pub const NO_MATERIAL: u32 = u32::MAX;

/// A texture layer's side (texels): the caller gives every image at this size
/// ([`Renderer::set_textures`]). The shader's `TEXTURE_SIZE`.
pub const TEXTURE_SIZE: u32 = 1024;
/// The mip levels of a layer, down to one texel.
const TEXTURE_MIPS: u32 = TEXTURE_SIZE.ilog2() + 1;

/// A palette entry without texture ([`Style::textures`]).
pub const NO_TEXTURE: [f32; 4] = [-1.0, 0.0, 0.0, 0.0];

/// A palette entry's texture ([`Style::textures`]): the layer of its image
/// ([`Renderer::set_textures`]), its real size (mm), its rotation (radians).
pub fn texture_entry(layer: u32, size: [f64; 2], rotation: f64) -> [f32; 4] {
    [layer as f32, (1.0 / size[0]) as f32, (1.0 / size[1]) as f32, rotation as f32]
}

/// Set in an instance's palette entry: its faces take it over their own paint (a face the
/// paint tool would paint, [`Highlight::face`]). The shader's `FORCED`.
const FORCED: u32 = 1 << 30;

/// One solid of a definition's mesh.
pub struct Part<'a> {
    /// The caller's id for it (the solid's id).
    pub id: u64,
    pub mesh: &'a Mesh,
    /// The palette entry of each B-rep face (by its index in the record), or [`NO_MATERIAL`];
    /// faces past the end have none.
    pub face_paint: &'a [u32],
}

/// A tint mixed into what is drawn: `rgb` (linear) by `a` (0 leaves it as is).
pub type Tint = [f32; 4];

pub const NO_TINT: Tint = [0.0; 4];

/// All placements of one definition's mesh.
pub struct Batch {
    pub key: u64,
    pub placements: Vec<Placement>,
    /// Faces see-through (see the module doc); no section cap.
    pub ghost: bool,
    /// Edges and silhouettes see-through too, drawn with the ghosts' faces.
    pub faint_edges: bool,
    /// The axis of the mesh along its part's length (0, 1, 2: x, y, z): a texture's grain
    /// (docs/render/look.md#textures).
    pub grain: u8,
}

impl Batch {
    pub fn new(key: u64, placements: Vec<Placement>) -> Self {
        Batch { key, placements, ghost: false, faint_edges: false, grain: 0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// From the definition's coordinates to the world's.
    pub transform: DMat4,
    /// Faces and edges tint (e.g. faded outside the context being edited).
    pub tint: Tint,
    /// The palette entry of its faces, or [`NO_MATERIAL`].
    pub material: u32,
    /// In a ghost batch: how much more opaque than [`Style::ghost_opacity`] its faces are (an
    /// open part's machining, a little more than its blanks).
    pub opacity: f32,
}

impl Placement {
    pub fn new(transform: DMat4) -> Self {
        Placement { transform, tint: NO_TINT, material: NO_MATERIAL, opacity: 0.0 }
    }
}

/// One part of one placement drawn again over the scene, tinted (selection, hover).
#[derive(Clone, Copy, Debug)]
pub struct Highlight {
    pub key: u64,
    pub part: u64,
    pub transform: DMat4,
    pub face_tint: Tint,
    pub edge_tint: Tint,
    /// The placement's palette entry (its faces keep their colour under the tint).
    pub material: u32,
    /// A ghost's (an open part's body, docs/model/bodies.md): its faces washed with the face tint over the
    /// ghost's own, see-through; its edges as any highlight's.
    pub ghost: bool,
    /// Only this B-rep face of the part (its index in the record), in `material` over its own
    /// paint, without its edges: the face the paint tool would paint (docs/render/look.md).
    pub face: Option<u32>,
    /// Its batch's [`Batch::grain`].
    pub grain: u8,
}

/// A world-space segment, coloured (`rgb` linear; `a` mixed over the background, or, in the
/// overlay, its opacity over what is drawn under it), plain or dashed, `width` points wide.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub a: DVec3,
    pub b: DVec3,
    pub color: [f32; 4],
    pub dash: Option<Dash>,
    pub width: f32,
}

/// What the last picture drawn cost (the render statistics overlay).
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub draw_calls: u32,
    /// Triangles submitted (lines are quads of two).
    pub triangles: u64,
    /// Placements and highlights.
    pub instances: u32,
    pub lines: u32,
    pub boxes: u32,
    /// Bytes written to the GPU for the picture (instances, lines, boxes, fills), meshes
    /// apart.
    pub upload_bytes: u64,
}

/// A render pass counting what it draws.
struct Counted<'p> {
    pass: wgpu::RenderPass<'p>,
    calls: u32,
    vertices: u64,
}

impl<'p> Counted<'p> {
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.calls += 1;
        self.vertices += vertices.len() as u64 * instances.len() as u64;
        self.pass.draw(vertices, instances);
    }

    fn draw_indexed(&mut self, indices: Range<u32>, base: i32, instances: Range<u32>) {
        self.calls += 1;
        self.vertices += indices.len() as u64 * instances.len() as u64;
        self.pass.draw_indexed(indices, base, instances);
    }
}

impl<'p> std::ops::Deref for Counted<'p> {
    type Target = wgpu::RenderPass<'p>;
    fn deref(&self) -> &Self::Target {
        &self.pass
    }
}

impl std::ops::DerefMut for Counted<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.pass
    }
}

/// A box's outline: the edges of the unit cube `[0, 1]³` placed by `transform` (which may
/// scale it), `width` points wide, depth-tested; dashed (`dash`'s pattern, its phase and
/// anchoring ignored) from every corner. Instanced: a selection of tens of thousands costs
/// one draw.
#[derive(Clone, Copy, Debug)]
pub struct BoxOutline {
    pub transform: DMat4,
    pub color: [f32; 4],
    pub width: f32,
    pub dash: Option<Dash>,
    /// Only its corners: each edge drawn this far (points) from its ends, at most to its middle
    /// (OpenCutList's `Kuix::BoxCornersMotif3d`).
    pub corners: Option<f32>,
    /// The edges of its faces turned to the eye drawn again over the scene, only its back left
    /// hidden behind faces (OpenCutList's `shell_on_top`).
    pub shell_on_top: bool,
    /// Drawn wholly over the scene (OpenCutList's `on_top`).
    pub over: bool,
}

impl BoxOutline {
    /// Plain, depth-tested, `width` points wide.
    pub fn new(transform: DMat4, color: [f32; 4], width: f32) -> Self {
        BoxOutline { transform, color, width, dash: None, corners: None, shell_on_top: false, over: false }
    }
}

/// Translucent triangles (linear RGBA, `a` its opacity), blended: a flat convex polygon (world
/// `corners`, a fan), or, `triangles`, the corners three by three (OpenCutList's `Kuix::Mesh`).
/// Drawn in the scene, hidden behind faces, or `over` everything; triangles whose back is
/// turned to the eye (counter-clockwise seen from their front) left out when `cull_back`.
#[derive(Clone, Debug)]
pub struct Fill {
    pub corners: Vec<DVec3>,
    pub color: [f32; 4],
    pub triangles: bool,
    pub over: bool,
    pub cull_back: bool,
}

impl Fill {
    /// A convex polygon in the scene, both sides drawn.
    pub fn polygon(corners: Vec<DVec3>, color: [f32; 4]) -> Self {
        Fill { corners, color, triangles: false, over: false, cull_back: false }
    }
}

/// The fills drawn by one pipeline: in the scene or over it, both sides or the front only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FillGroup {
    Scene,
    SceneFront,
    Over,
    OverFront,
}

impl FillGroup {
    const ALL: [FillGroup; 4] = [FillGroup::Scene, FillGroup::SceneFront, FillGroup::Over, FillGroup::OverFront];

    fn of(f: &Fill) -> Self {
        match (f.over, f.cull_back) {
            (false, false) => FillGroup::Scene,
            (false, true) => FillGroup::SceneFront,
            (true, false) => FillGroup::Over,
            (true, true) => FillGroup::OverFront,
        }
    }

    fn over(self) -> bool {
        matches!(self, FillGroup::Over | FillGroup::OverFront)
    }

    fn cull_back(self) -> bool {
        matches!(self, FillGroup::SceneFront | FillGroup::OverFront)
    }

    fn label(self) -> &'static str {
        match self {
            FillGroup::Scene => "fills",
            FillGroup::SceneFront => "fills (front faces)",
            FillGroup::Over => "fills (over)",
            FillGroup::OverFront => "fills (over, front faces)",
        }
    }
}

/// A line's dashes: `pattern` in points, dash, gap, dash, gap (zeros left out: a dash and a
/// gap, `[l, l, 0, 0]`, is the plain dashed line), lying as `lay` says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dash {
    pub pattern: [f32; 4],
    pub lay: Lay,
}

/// How a dash pattern lies on its line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lay {
    /// Laid on the line itself in model units, so that its dashes stay where they are on it as
    /// the view orbits or pans: the pattern at this many model units at the line's `a`, its
    /// period the power of two (model units) nearest to its length on screen at the view's
    /// focus ([`Frame::model_per_point`]): one period for every line of the picture, the same
    /// while the view orbits, doubled or halved as it zooms by a factor two; regular on the
    /// line, so shorter on screen as it runs away (fading into a lighter line where they would
    /// be a few pixels long).
    Model(f64),
    /// Laid on the line itself, stretched a little so that a dash starts and another ends the
    /// line on screen. A box's half edges are always anchored.
    Anchored,
    /// On screen, at this many points at the line's `a`: as long on screen everywhere, held
    /// still as the view pans, sliding along the line as it orbits (the guides').
    Screen(f32),
}

impl Dash {
    /// Dashes `length` points long, gaps as long, lying as `lay` says.
    pub const fn even(length: f32, lay: Lay) -> Self {
        Dash { pattern: [length, length, 0.0, 0.0], lay }
    }
}

/// The optional device features the renderer uses when the device has them:
/// `DEPTH_CLIP_CONTROL`, so that depth-tested lines past the parallel projection's depth range
/// (infinite guides and axes) are clamped to it rather than cut.
pub const OPTIONAL_FEATURES: wgpu::Features = wgpu::Features::DEPTH_CLIP_CONTROL;

/// A square grid on the XY plane centred on the origin.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub spacing: f64,
    /// Lines on each side of the axes.
    pub count: u32,
}

/// The section plane, in world coordinates: what is on the side `normal` points to is cut
/// away (docs/render/look.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub origin: DVec3,
    pub normal: DVec3,
}

/// How the model is drawn: colours (linear RGB) and display options.
#[derive(Clone, Copy, Debug)]
pub struct Style<'a> {
    /// The front of a face without a colour, the back of every face.
    pub face_front: [f32; 3],
    pub face_back: [f32; 3],
    /// Edges and silhouettes.
    pub edge: [f32; 3],
    /// The grid's lines (mixed over the background, its tens more than the others).
    pub grid: [f32; 3],
    /// The colours faces and placements refer to (linear RGB; alpha unused).
    pub palette: &'a [[f32; 4]],
    /// Each palette entry's texture ([`texture_entry`], [`NO_TEXTURE`]); entries past its end
    /// have none.
    pub textures: &'a [[f32; 4]],
    /// Draw the outline of curved faces.
    pub silhouettes: bool,
    pub section: Option<Section>,
    /// How much darker than its part a section cap is (0–1).
    pub cap_shade: f32,
    /// How opaque the hatches over a section cap are, in the edges' ink (0–1).
    pub hatch_opacity: f32,
    /// How opaque a ghost batch's faces are (0–1).
    pub ghost_opacity: f32,
    /// How opaque the edges of a batch with [`Batch::faint_edges`] are (0–1).
    pub ghost_edge_opacity: f32,
}

impl Default for Style<'_> {
    fn default() -> Self {
        Style {
            face_front: [0.92, 0.91, 0.87],
            face_back: [0.55, 0.60, 0.72],
            edge: [0.02, 0.02, 0.03],
            grid: [0.3, 0.3, 0.3],
            palette: &[],
            textures: &[],
            silhouettes: true,
            section: None,
            cap_shade: 0.25,
            hatch_opacity: 0.5,
            ghost_opacity: 0.15,
            ghost_edge_opacity: 0.35,
        }
    }
}

/// Everything one frame draws.
pub struct Frame<'a> {
    /// World to camera (`f64`: the camera may be far from the origin).
    pub view: DMat4,
    /// Camera to clip, **reversed depth** (near → 1, far → 0).
    pub proj: Mat4,
    /// Linear RGB.
    pub background: [f32; 3],
    pub style: Style<'a>,
    pub batches: &'a [Batch],
    pub highlights: &'a [Highlight],
    /// Depth-tested lines (inferences, guides).
    pub lines: &'a [Line],
    /// Depth-tested lines blended over what they do not hide, at their alpha, no depth
    /// written (the guides outside the context being edited: the faces under them show through).
    pub faint_lines: &'a [Line],
    /// Depth-tested box outlines (the selection's boxes).
    pub boxes: &'a [BoxOutline],
    /// Lines drawn over everything (tool previews), blended at their alpha.
    pub overlay: &'a [Line],
    /// Lines drawn only where faces hide them, blended, in their own style: the hidden part of
    /// a line drawn in the scene too (a move's line: plain where seen, dashed where hidden).
    pub hidden_lines: &'a [Line],
    /// Translucent polygons and triangles (tool previews).
    pub fills: &'a [Fill],
    pub grid: Option<Grid>,
    /// The axes are drawn (in `lines`): the grid leaves out its lines along them.
    pub axes: bool,
    /// Where the grid is: the frame of the context being edited, to world.
    pub axes_frame: DMat4,
    /// Pixels per point of the screen: line widths are in points, so they look the same on
    /// any display.
    pub pixels_per_point: f32,
    /// The model units a point spans at the view's focus (the orbit centre): sets the period of
    /// the dashes laid in model units ([`Dash`]).
    pub model_per_point: f64,
}

/// Line widths, in points.
pub const EDGE_WIDTH: f32 = 1.5;
pub const LINE_WIDTH: f32 = 1.5;
/// A line drawn bolder (the curve a tool refers to).
pub const BOLD_WIDTH: f32 = 3.0;
/// The space between a section cap's hatches (points): from this to twice as much, as the
/// zoom goes (docs/render/look.md).
pub const HATCH_SPACING: f32 = 8.0;
/// A hatch's width (points).
pub const HATCH_WIDTH: f32 = 1.0;
/// How far apart the points a section's hatches are anchored at may be (mm, in the plane's
/// basis): near the eye, for precision; a power of two, so that the pattern does not change.
const HATCH_ANCHOR_STEP: f64 = 4096.0;
pub const GRID_WIDTH: f32 = 1.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    /// The face's palette entry, or [`NO_MATERIAL`].
    paint: u32,
}

/// One corner of a segment drawn as a screen-space quad (6 per segment, two triangles): both
/// ends, which end this corner is at (`corner.x`: 0 = `a`, 1 = `b`) and on which side
/// (`corner.y`: ±1). The vertex shader widens it to the line width in pixels.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EdgeVertex {
    a: [f32; 3],
    b: [f32; 3],
    corner: [f32; 2],
}

/// A corner of a silhouette candidate's quad: a mesh edge inside a curved surface, with the
/// normals of its two triangles. Drawn only where one faces the eye and the other does not.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SilhouetteVertex {
    a: [f32; 3],
    b: [f32; 3],
    corner: [f32; 2],
    n1: [f32; 3],
    n2: [f32; 3],
}

/// A corner of a coloured line's quad; `corner.z` is its half width in pixels; `pattern`: its
/// dash pattern (pixels, zeros when plain); `dash`: where the pattern is at `a` (pixels), and
/// 1 when it is anchored at both ends.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineVertex {
    a: [f32; 3],
    b: [f32; 3],
    corner: [f32; 3],
    color: [f32; 4],
    pattern: [f32; 4],
    /// How the pattern lies (the shader's `LAY_*`: in model units 0, anchored 1, on screen 2),
    /// then in model units the phase at the nearer end (high and low parts) and the length,
    /// negative when the nearer end is `b`; anchored, 1; on screen, the phase (px) at `a`
    /// (`vs_line`).
    dash: [f32; 4],
}

/// A [`BoxOutline`] as the GPU takes it: the unit cube to camera space, its colour, its dash
/// pattern (pixels, zeros when plain), then half its width, its corners' length (pixels, 0:
/// whole edges), `shell_on_top` and `over` (1 or 0).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BoxRaw {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    pattern: [f32; 4],
    params: [f32; 4],
}

/// A corner of a fill's triangle, in camera coordinates.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FillVertex {
    pos: [f32; 3],
    color: [f32; 4],
}

/// The corners a box outline is drawn with: 24 half edges of 6 (`vs_box`).
const BOX_VERTICES: u32 = 24 * 6;

/// The corners of a segment's quad, in drawing order.
const QUAD: [[f32; 2]; 6] = [[0.0, -1.0], [0.0, 1.0], [1.0, -1.0], [1.0, -1.0], [0.0, 1.0], [1.0, 1.0]];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 3],
    tint: [f32; 4],
    material: u32,
    grain: u32,
}

impl InstanceRaw {
    /// `model_view`: from the mesh's (recentred) coordinates to the camera's, in `f64`;
    /// `opacity`: a ghost's, over the ghosts' (carried in the normal matrix's spare `w`);
    /// `key`: its mesh's, which chooses its section caps' hatch ([`hatch_of`]).
    fn new(model_view: &DMat4, tint: Tint, material: u32, opacity: f32, key: u64, grain: u8) -> Self {
        let linear = DMat3::from_mat4(*model_view);
        let n = linear.inverse().transpose();
        let c = |v: DVec3, w: f64| [v.x as f32, v.y as f32, v.z as f32, w as f32];
        // The winding's sign, for the shader to tell outside from inside under a mirror.
        let mirrored = if linear.determinant() < 0.0 { -1.0 } else { 1.0 };
        InstanceRaw {
            model: model_view.as_mat4().to_cols_array_2d(),
            normal: [c(n.x_axis, mirrored), c(n.y_axis, opacity as f64), c(n.z_axis, hatch_of(key))],
            tint,
            material,
            grain: grain as u32,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    proj: [[f32; 4]; 4],
    /// World +Z and the key light's direction, in camera coordinates.
    up: [f32; 4],
    key: [f32; 4],
    background: [f32; 4],
    /// Target size in pixels, then half the width of edges and of lines, in pixels.
    viewport: [f32; 4],
    face_front: [f32; 4],
    face_back: [f32; 4],
    edge: [f32; 4],
    /// The section plane in camera coordinates (`xyz · p + w > 0` is cut away).
    clip: [f32; 4],
    /// x: section on (1) or off (0); y: how much darker the caps are.
    section: [f32; 4],
    /// The basis of the plane the caps' hatches are measured in, in camera coordinates, and
    /// `w` such that `xyz · p + w` measures a point `p` of the plane along it (mm).
    hatch_u: [f32; 4],
    hatch_v: [f32; 4],
    /// The hatches' spacing and half width (pixels), their opacity.
    hatch: [f32; 4],
}

/// How many directions the section caps' hatches take (the shader's `hatch_direction`).
const HATCH_DIRECTIONS: u64 = 6;

/// The hatch of a mesh's section caps: one of [`HATCH_DIRECTIONS`], from its key (one part,
/// one hatch; neighbours seldom alike).
fn hatch_of(key: u64) -> f64 {
    // splitmix64's finaliser: neighbouring keys spread.
    let mut z = key.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((z ^ (z >> 31)) % HATCH_DIRECTIONS) as f64
}

/// The basis `u`, `v` of a section's plane its hatches are measured in (`u` across world Z,
/// else along world X), and the point of the plane they are measured from: near `eye`,
/// snapped to [`HATCH_ANCHOR_STEP`] in that basis.
fn hatch_frame(section: &Section, eye: DVec3) -> (DVec3, DVec3, DVec3) {
    let n = section.normal.normalize();
    let u = if n.z.abs() < 0.999 { DVec3::Z.cross(n).normalize() } else { (DVec3::X - n * n.x).normalize() };
    let v = n.cross(u);
    let to_eye = eye - section.origin;
    let snap = |x: f64| (x / HATCH_ANCHOR_STEP).floor() * HATCH_ANCHOR_STEP;
    let anchor = section.origin + u * snap(to_eye.dot(u)) + v * snap(to_eye.dot(v));
    (u, v, anchor)
}

struct PartRange {
    id: u64,
    indices: Range<u32>,
    /// Each B-rep face's runs of triangles in `indices` (by face index): a face highlighted
    /// draws only those.
    faces: Vec<(u32, Range<u32>)>,
    edges: Range<u32>,
    silhouettes: Range<u32>,
}

struct GpuMesh {
    revision: u64,
    /// The definition coordinates the vertices are relative to.
    origin: DVec3,
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    index_count: u32,
    edges: Option<wgpu::Buffer>,
    edge_vertex_count: u32,
    silhouettes: Option<wgpu::Buffer>,
    silhouette_vertex_count: u32,
    parts: Vec<PartRange>,
}

struct Target {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolved: wgpu::Texture,
    resolved_view: wgpu::TextureView,
    /// The resolved target seen as plain `Rgba8Unorm`: egui treats texels as sRGB-encoded
    /// values, so it must read the stored bytes, not the sRGB view's decoded (linear) ones.
    display_view: wgpu::TextureView,
}

/// A picture of its own, apart from the viewport's (a component's thumbnail,
/// docs/ui/components-library.md#thumbnails): its resolved texture only, drawn by
/// [`Renderer::render_picture`] with the multisampled colour and depth the renderer keeps for
/// pictures of its size.
pub struct Picture {
    size: [u32; 2],
    resolved: wgpu::Texture,
    resolved_view: wgpu::TextureView,
    display_view: wgpu::TextureView,
}

impl Picture {
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// As [`Renderer::target`]'s: a [`DISPLAY_FORMAT`] view for egui.
    pub fn display_view(&self) -> &wgpu::TextureView {
        &self.display_view
    }

    /// The texture (to copy it out).
    pub fn texture(&self) -> &wgpu::Texture {
        &self.resolved
    }
}

/// The multisampled colour and depth pictures of one size are drawn with.
struct PictureScratch {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
}

/// The textures' images, a layer each in one array ([`Renderer::set_textures`]).
struct TextureLayers {
    keys: Vec<u64>,
    view: wgpu::TextureView,
}

impl TextureLayers {
    /// An array of `count` layers (one at least: a binding needs a texture), every mip level.
    fn create(device: &wgpu::Device, count: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("texture layers"),
            size: wgpu::Extent3d { width: TEXTURE_SIZE, height: TEXTURE_SIZE, depth_or_array_layers: count.max(1) },
            mip_level_count: TEXTURE_MIPS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        (texture, view)
    }

    fn empty(device: &wgpu::Device) -> Self {
        TextureLayers { keys: Vec::new(), view: TextureLayers::create(device, 1).1 }
    }
}

/// The next mip level of a square sRGB image `side` texels wide: each texel the mean of four,
/// averaged in linear light.
fn mip_down(rgba: &[u8], side: u32) -> Vec<u8> {
    let half = (side / 2).max(1);
    let to_linear = |c: u8| (c as f32 / 255.0).powf(2.2);
    let to_srgb = |l: f32| (l.max(0.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8;
    let mut out = vec![0u8; (half * half * 4) as usize];
    for y in 0..half {
        for x in 0..half {
            for c in 0..4 {
                let at = |dx: u32, dy: u32| {
                    let (sx, sy) = ((2 * x + dx).min(side - 1), (2 * y + dy).min(side - 1));
                    rgba[((sy * side + sx) * 4 + c) as usize]
                };
                let px = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
                out[((y * half + x) * 4 + c) as usize] = if c == 3 {
                    (px.iter().map(|v| *v as u32).sum::<u32>() / 4) as u8
                } else {
                    to_srgb(px.iter().map(|v| to_linear(*v)).sum::<f32>() / 4.0)
                };
            }
        }
    }
    out
}

/// A growable buffer rewritten each frame.
struct Dynamic {
    label: &'static str,
    usage: wgpu::BufferUsages,
    buffer: Option<wgpu::Buffer>,
    capacity: u64,
}

impl Dynamic {
    fn new(label: &'static str, usage: wgpu::BufferUsages) -> Self {
        Dynamic { label, usage, buffer: None, capacity: 0 }
    }

    /// Writes `bytes`; true when the buffer was (re)created (a bind group must follow).
    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }
        let len = bytes.len() as u64;
        let mut created = false;
        if self.buffer.is_none() || self.capacity < len {
            // Doubled, short of the device's limit (the caller keeps `bytes` within it).
            let capacity = len.next_power_of_two().max(4096).min(device.limits().max_buffer_size).max(len);
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: capacity,
                usage: self.usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = capacity;
            created = true;
        }
        if let Some(b) = &self.buffer {
            queue.write_buffer(b, 0, bytes);
        }
        created
    }
}

/// The viewport renderer.
pub struct Renderer {
    faces: wgpu::RenderPipeline,
    /// Faces while a section is on: cut, the backs seen through the cut painted as caps.
    faces_cut: wgpu::RenderPipeline,
    /// A ghost batch's faces: their fronts blended, cut by the section, no depth written.
    faces_ghost: wgpu::RenderPipeline,
    /// A ghost highlighted: a wash of its tint over its faces, as see-through.
    faces_ghost_mark: wgpu::RenderPipeline,
    /// Then the caps marked in the stencil (the parity of the faces left along each ray), and
    /// drawn there from the back faces, just behind the plane.
    cap_mark: wgpu::RenderPipeline,
    caps: wgpu::RenderPipeline,
    depth_format: wgpu::TextureFormat,
    edges: wgpu::RenderPipeline,
    silhouettes: wgpu::RenderPipeline,
    /// The same for [`Batch::faint_edges`]: blended, no depth written.
    edges_faint: wgpu::RenderPipeline,
    silhouettes_faint: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    boxes: wgpu::RenderPipeline,
    /// The boxes' edges drawn over the scene ([`BoxOutline::shell_on_top`], `over`).
    boxes_front: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,
    /// The faint lines: depth tested, blended, no depth written.
    faint_lines: wgpu::RenderPipeline,
    /// The hidden lines: where faces are nearer, blended, no depth written.
    hidden: wgpu::RenderPipeline,
    /// The fills by [`FillGroup`]: in the scene or over it, both sides or the front only.
    fills: [wgpu::RenderPipeline; 4],
    globals: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    palette: Dynamic,
    /// The palette entries' textures ([`Style::textures`]).
    texture_entries: Dynamic,
    /// The images of the textures, one layer each ([`Renderer::set_textures`]), and their keys.
    layers: TextureLayers,
    sampler: wgpu::Sampler,
    /// The shown document's meshes, by key.
    meshes: std::collections::HashMap<u64, GpuMesh>,
    /// The documents not shown: their meshes wait here, so switching back uploads nothing
    /// (keys are a document's own: they may be the same in two documents).
    document: u64,
    parked: std::collections::HashMap<u64, std::collections::HashMap<u64, GpuMesh>>,
    instances: Dynamic,
    line_vertices: Dynamic,
    box_instances: Dynamic,
    fill_vertices: Dynamic,
    target: Option<Target>,
    /// Bumped whenever what is uploaded changes (a mesh, the target): part of a frame's
    /// fingerprint.
    generation: u64,
    target_generation: u64,
    /// The fingerprint of the picture in the target.
    drawn: Option<u64>,
    /// What the picture in the target cost.
    stats: Stats,
    /// The multisampled colour and depth [`Picture`]s are drawn with.
    picture_scratch: Option<PictureScratch>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("skraft shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skraft"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let instance_attrs = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4,
            12 => Uint32, 14 => Uint32
        ];
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<InstanceRaw>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attrs,
        };
        let face_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32];
        let face_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &face_attrs,
        };
        let edge_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        let edge_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<EdgeVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &edge_attrs,
        };
        let sil_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x3, 13 => Float32x3];
        let sil_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<SilhouetteVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &sil_attrs,
        };
        let line_attrs =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4];
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &line_attrs,
        };

        let box_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4];
        let box_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<BoxRaw>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &box_attrs,
        };

        let culled = |label: &str,
                      vs: &str,
                      fs: &str,
                      buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                      depth: wgpu::DepthStencilState,
                      write_mask: wgpu::ColorWrites,
                      blend: Option<wgpu::BlendState>,
                      unclipped_depth: bool,
                      cull_mode: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode,
                    unclipped_depth,
                    ..Default::default()
                },
                depth_stencil: Some(depth),
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: TARGET_FORMAT, blend, write_mask })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let blended =
            |label: &str,
             vs: &str,
             fs: &str,
             buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
             depth: wgpu::DepthStencilState,
             write_mask: wgpu::ColorWrites,
             blend: Option<wgpu::BlendState>,
             unclipped_depth: bool| { culled(label, vs, fs, buffers, depth, write_mask, blend, unclipped_depth, None) };
        let pipeline =
            |label: &str,
             vs: &str,
             fs: &str,
             buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
             depth: wgpu::DepthStencilState,
             write_mask: wgpu::ColorWrites| blended(label, vs, fs, buffers, depth, write_mask, None, false);
        let depth_format = depth_format(device);
        let depth = |write: bool, compare: wgpu::CompareFunction, bias: wgpu::DepthBiasState| wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: Default::default(),
            bias,
        };
        // The stencil of the caps: its low bit flipped by `cap_mark`, tested by `caps`.
        let stencil = |compare: wgpu::CompareFunction, pass_op: wgpu::StencilOperation| {
            let face = wgpu::StencilFaceState { compare, fail_op: Keep, depth_fail_op: Keep, pass_op };
            wgpu::StencilState { front: face, back: face, read_mask: 1, write_mask: 1 }
        };
        use wgpu::StencilOperation::Keep;
        let all = wgpu::ColorWrites::ALL;
        // Reversed depth: nearer is greater. Faces are pushed back (towards 0) a little so
        // their edges win the depth test.
        let face_bias = wgpu::DepthBiasState { constant: -2, slope_scale: -1.0, clamp: 0.0 };
        let nearer = wgpu::CompareFunction::GreaterEqual;
        let faces = pipeline(
            "faces",
            "vs_face",
            "fs_face",
            &[Some(face_layout.clone()), Some(instance_layout.clone())],
            depth(true, nearer, face_bias),
            all,
        );
        let faces_cut = pipeline(
            "faces (section)",
            "vs_face",
            "fs_face_cut",
            &[Some(face_layout.clone()), Some(instance_layout.clone())],
            depth(true, nearer, face_bias),
            all,
        );
        let faces_ghost = blended(
            "faces (ghost)",
            "vs_face",
            "fs_face_ghost",
            &[Some(face_layout.clone()), Some(instance_layout.clone())],
            depth(false, nearer, face_bias),
            all,
            Some(wgpu::BlendState::ALPHA_BLENDING),
            false,
        );
        let faces_ghost_mark = blended(
            "faces (ghost highlighted)",
            "vs_face",
            "fs_face_ghost_mark",
            &[Some(face_layout.clone()), Some(instance_layout.clone())],
            depth(false, nearer, face_bias),
            all,
            Some(wgpu::BlendState::ALPHA_BLENDING),
            false,
        );
        // Every fragment left flips the stencil: an odd count, the ray meets the plane inside
        // a part.
        let cap_mark = pipeline(
            "caps (mark)",
            "vs_face",
            "fs_cap_mark",
            &[Some(face_layout.clone()), Some(instance_layout.clone())],
            wgpu::DepthStencilState {
                stencil: stencil(wgpu::CompareFunction::Always, wgpu::StencilOperation::Invert),
                ..depth(false, wgpu::CompareFunction::Always, Default::default())
            },
            wgpu::ColorWrites::empty(),
        );
        // Where the stencil marks a cap, the back faces again, at a depth just behind the plane
        // (their order kept): the nearest one, the cut part's, wins over any front face.
        let caps = pipeline(
            "caps",
            "vs_face",
            "fs_cap",
            &[Some(face_layout), Some(instance_layout.clone())],
            wgpu::DepthStencilState { stencil: stencil(wgpu::CompareFunction::Equal, Keep), ..depth(true, nearer, Default::default()) },
            all,
        );
        let edges = pipeline(
            "edges",
            "vs_edge",
            "fs_edge",
            &[Some(edge_layout.clone()), Some(instance_layout.clone())],
            depth(true, nearer, Default::default()),
            all,
        );
        let silhouettes = pipeline(
            "silhouettes",
            "vs_silhouette",
            "fs_edge",
            &[Some(sil_layout.clone()), Some(instance_layout.clone())],
            depth(true, nearer, Default::default()),
            all,
        );
        let faint = |label: &str, vs: &str, layout: &wgpu::VertexBufferLayout<'_>| {
            blended(
                label,
                vs,
                "fs_edge_faint",
                &[Some(layout.clone()), Some(instance_layout.clone())],
                depth(false, nearer, Default::default()),
                all,
                Some(wgpu::BlendState::ALPHA_BLENDING),
                false,
            )
        };
        let edges_faint = faint("edges (faint)", "vs_edge", &edge_layout);
        let silhouettes_faint = faint("silhouettes (faint)", "vs_silhouette", &sil_layout);
        // Infinite lines (here and in the overlay) reach past the parallel projection's depth
        // range: clamped to it, where the device can.
        let unclipped = device.features().contains(wgpu::Features::DEPTH_CLIP_CONTROL);
        let lines = blended(
            "lines",
            "vs_line",
            "fs_line",
            &[Some(line_layout.clone())],
            depth(true, nearer, Default::default()),
            all,
            None,
            unclipped,
        );
        let boxes = blended(
            "boxes",
            "vs_box",
            "fs_line",
            &[Some(box_layout.clone())],
            depth(true, nearer, Default::default()),
            all,
            None,
            unclipped,
        );
        // Over the scene: no depth test, none written.
        let boxes_front = blended(
            "boxes (front)",
            "vs_box_front",
            "fs_line",
            &[Some(box_layout)],
            depth(false, wgpu::CompareFunction::Always, Default::default()),
            all,
            None,
            unclipped,
        );
        // Hidden behind faces, blended over what they do not hide (no depth written).
        let faint_lines = blended(
            "faint lines",
            "vs_line",
            "fs_overlay",
            &[Some(line_layout.clone())],
            depth(false, nearer, Default::default()),
            all,
            Some(wgpu::BlendState::ALPHA_BLENDING),
            unclipped,
        );
        // Reversed depth: what something nearer hides is less.
        let hidden = blended(
            "hidden lines",
            "vs_line",
            "fs_overlay",
            &[Some(line_layout.clone())],
            depth(false, wgpu::CompareFunction::Less, Default::default()),
            all,
            Some(wgpu::BlendState::ALPHA_BLENDING),
            unclipped,
        );
        let overlay = blended(
            "overlay",
            "vs_line",
            "fs_overlay",
            &[Some(line_layout)],
            depth(false, wgpu::CompareFunction::Always, Default::default()),
            all,
            Some(wgpu::BlendState::ALPHA_BLENDING),
            unclipped,
        );
        let fill_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let fill_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<FillVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &fill_attrs,
        };
        // In the scene, hidden behind faces, or over it; without hiding what lies behind them
        // (no depth written). Both sides, or the front only.
        let fills = FillGroup::ALL.map(|g| {
            let compare = if g.over() { wgpu::CompareFunction::Always } else { nearer };
            culled(
                g.label(),
                "vs_fill",
                "fs_fill",
                &[Some(fill_layout.clone())],
                depth(false, compare, Default::default()),
                all,
                Some(wgpu::BlendState::ALPHA_BLENDING),
                false,
                g.cull_back().then_some(wgpu::Face::Back),
            )
        });

        Renderer {
            faces,
            faces_cut,
            faces_ghost,
            faces_ghost_mark,
            cap_mark,
            caps,
            depth_format,
            edges,
            silhouettes,
            edges_faint,
            silhouettes_faint,
            lines,
            boxes,
            boxes_front,
            overlay,
            faint_lines,
            hidden,
            fills,
            globals,
            layout: bgl,
            bind_group: None,
            palette: Dynamic::new("palette", wgpu::BufferUsages::STORAGE),
            texture_entries: Dynamic::new("texture entries", wgpu::BufferUsages::STORAGE),
            layers: TextureLayers::empty(device),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("textures"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            meshes: Default::default(),
            document: 0,
            parked: Default::default(),
            instances: Dynamic::new("instances", wgpu::BufferUsages::VERTEX),
            line_vertices: Dynamic::new("lines", wgpu::BufferUsages::VERTEX),
            box_instances: Dynamic::new("boxes", wgpu::BufferUsages::VERTEX),
            fill_vertices: Dynamic::new("fills", wgpu::BufferUsages::VERTEX),
            target: None,
            generation: 0,
            target_generation: 0,
            drawn: None,
            stats: Stats::default(),
            picture_scratch: None,
        }
    }

    /// Shows document `doc`: the meshes are then its own (none for a new one), the others'
    /// kept aside until it is shown again or dropped ([`Self::drop_document`]).
    pub fn set_document(&mut self, doc: u64) {
        if doc == self.document {
            return;
        }
        let shown = std::mem::replace(&mut self.meshes, self.parked.remove(&doc).unwrap_or_default());
        if !shown.is_empty() {
            self.parked.insert(self.document, shown);
        }
        self.document = doc;
        self.generation += 1;
    }

    /// Frees document `doc`'s meshes (it was closed).
    pub fn drop_document(&mut self, doc: u64) {
        if doc == self.document {
            if !self.meshes.is_empty() {
                self.meshes.clear();
                self.generation += 1;
            }
        } else {
            self.parked.remove(&doc);
        }
    }

    /// Is the mesh `key` uploaded at `revision`?
    pub fn has_mesh(&self, key: u64, revision: u64) -> bool {
        self.meshes.get(&key).is_some_and(|m| m.revision == revision)
    }

    /// Uploads the mesh of `key` (a definition) as the concatenation of `parts`, unless it is
    /// already there at `revision`.
    pub fn set_mesh(&mut self, device: &wgpu::Device, key: u64, revision: u64, parts: &[Part<'_>]) {
        if self.has_mesh(key, revision) {
            return;
        }
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut edges: Vec<EdgeVertex> = Vec::new();
        let mut silhouettes: Vec<SilhouetteVertex> = Vec::new();
        let mut ranges = Vec::with_capacity(parts.len());
        // The centre of the parts' origins: vertices are stored relative to it.
        let origin = parts
            .iter()
            .map(|p| p.mesh.origin)
            .fold(None, |b: Option<(DVec3, DVec3)>, o| Some(b.map_or((o, o), |(lo, hi)| (lo.min(o), hi.max(o)))))
            .map_or(DVec3::ZERO, |(lo, hi)| (lo + hi) * 0.5);
        for p in parts {
            let shift = (p.mesh.origin - origin).as_vec3();
            let at = |v: &[f32; 3]| (Vec3::from_array(*v) + shift).to_array();
            let base = vertices.len() as u32;
            let (i0, e0, s0) = (indices.len() as u32, edges.len() as u32, silhouettes.len() as u32);
            // A vertex takes its triangles' face colour. A vertex shared by two faces of
            // different colours (rare: the mesher splits vertices along B-rep edges) is
            // duplicated so each triangle keeps its own.
            let mut paint_of: Vec<Option<u32>> = vec![None; p.mesh.positions.len()];
            let mut extra: Vec<Vertex> = Vec::new();
            let mut tri_indices = Vec::with_capacity(p.mesh.indices.len());
            let mut faces: Vec<(u32, Range<u32>)> = Vec::new();
            for (t, tri) in p.mesh.indices.chunks_exact(3).enumerate() {
                let face = p.mesh.tri_face.get(t).copied().unwrap_or(u32::MAX);
                // Its triangle's place in the whole index buffer, added to its face's last run
                // when it follows it (the mesher writes a face's triangles together).
                let first = i0 + tri_indices.len() as u32;
                match faces.last_mut() {
                    Some((f, run)) if *f == face && run.end == first => run.end = first + 3,
                    _ => faces.push((face, first..first + 3)),
                }
                let face = face as usize;
                let paint = p.face_paint.get(face).copied().unwrap_or(NO_MATERIAL);
                for &i in tri {
                    let slot = &mut paint_of[i as usize];
                    match slot {
                        None => {
                            *slot = Some(paint);
                            tri_indices.push(i + base);
                        }
                        Some(q) if *q == paint => tri_indices.push(i + base),
                        Some(_) => {
                            let v = Vertex { pos: at(&p.mesh.positions[i as usize]), normal: p.mesh.normals[i as usize], paint };
                            tri_indices.push(base + p.mesh.positions.len() as u32 + extra.len() as u32);
                            extra.push(v);
                        }
                    }
                }
            }
            vertices.extend(p.mesh.positions.iter().zip(&p.mesh.normals).zip(&paint_of).map(|((pos, n), paint)| Vertex {
                pos: at(pos),
                normal: *n,
                paint: paint.unwrap_or(NO_MATERIAL),
            }));
            vertices.extend(extra);
            indices.extend(tri_indices);
            for poly in &p.mesh.edges {
                for w in poly.windows(2) {
                    let (a, b) = (at(&w[0]), at(&w[1]));
                    edges.extend(QUAD.map(|corner| EdgeVertex { a, b, corner }));
                }
            }
            for s in silhouette_edges(p.mesh) {
                let (a, b) = (at(&s.a), at(&s.b));
                silhouettes.extend(QUAD.map(|corner| SilhouetteVertex { a, b, corner, n1: s.n1, n2: s.n2 }));
            }
            faces.sort_by_key(|(f, _)| *f);
            ranges.push(PartRange {
                id: p.id,
                indices: i0..indices.len() as u32,
                faces,
                edges: e0..edges.len() as u32,
                silhouettes: s0..silhouettes.len() as u32,
            });
        }
        let buffer = |label: &str, bytes: &[u8], usage: wgpu::BufferUsages| {
            (!bytes.is_empty())
                .then(|| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage }))
        };
        let mesh = GpuMesh {
            revision,
            origin,
            vertices: buffer("vertices", bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX),
            indices: buffer("indices", bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX),
            index_count: indices.len() as u32,
            edges: buffer("edges", bytemuck::cast_slice(&edges), wgpu::BufferUsages::VERTEX),
            edge_vertex_count: edges.len() as u32,
            silhouettes: buffer("silhouettes", bytemuck::cast_slice(&silhouettes), wgpu::BufferUsages::VERTEX),
            silhouette_vertex_count: silhouettes.len() as u32,
            parts: ranges,
        };
        self.meshes.insert(key, mesh);
        self.generation += 1;
    }

    /// Drops the meshes whose key `keep` refuses.
    pub fn retain_meshes(&mut self, mut keep: impl FnMut(u64) -> bool) {
        let before = self.meshes.len();
        self.meshes.retain(|k, _| keep(*k));
        if self.meshes.len() != before {
            self.generation += 1;
        }
    }

    /// The resolved target of the last frame, as a [`DISPLAY_FORMAT`] view for egui, and a
    /// number that changes whenever it is recreated (a UI re-registers its texture then).
    pub fn target(&self) -> Option<(&wgpu::TextureView, u64)> {
        self.target.as_ref().map(|t| (&t.display_view, self.target_generation))
    }

    /// The textures' images, in layers ([`texture_entry`]): each `(key, rgba)`, sRGB RGBA bytes
    /// of [`TEXTURE_SIZE`]² texels. Uploaded again, its mip levels made, only when the keys
    /// change.
    pub fn set_textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layers: &[(u64, &[u8])]) {
        let keys: Vec<u64> = layers.iter().map(|(k, _)| *k).collect();
        if keys == self.layers.keys {
            return;
        }
        let (texture, view) = TextureLayers::create(device, layers.len() as u32);
        for (layer, (_, rgba)) in layers.iter().enumerate() {
            if rgba.len() != (TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize {
                continue;
            }
            let mut level = rgba.to_vec();
            let mut side = TEXTURE_SIZE;
            for mip in 0..TEXTURE_MIPS {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: mip,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: layer as u32 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &level,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(side * 4), rows_per_image: Some(side) },
                    wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                );
                if side > 1 {
                    level = mip_down(&level, side);
                    side /= 2;
                }
            }
        }
        self.layers = TextureLayers { keys, view };
        self.bind_group = None;
        self.generation += 1;
    }

    /// The resolved target texture (to copy it out, e.g. for a screenshot).
    pub fn target_texture(&self) -> Option<&wgpu::Texture> {
        self.target.as_ref().map(|t| &t.resolved)
    }

    /// Draws `frame` at `size` pixels into the off-screen target. Returns false, having
    /// encoded nothing, when the target already holds this very picture.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        frame: &Frame<'_>,
    ) -> bool {
        let size = [size[0].max(1), size[1].max(1)];
        self.ensure_target(device, size);
        let print = fingerprint(frame, size, self.generation);
        if self.drawn == Some(print) {
            return false;
        }

        let bg = frame.background;
        let style = &frame.style;
        let dir = |v: DVec3| {
            let v = frame.view.transform_vector3(v).normalize_or_zero();
            [v.x as f32, v.y as f32, v.z as f32, 0.0]
        };
        let rgb = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
        // The section plane, in camera coordinates (in f64: the camera may be far away).
        // Its hatches' directions, measured from a point near the eye.
        let (clip, section_on, hatch_u, hatch_v) = match style.section {
            Some(s) if s.normal.length_squared() > 0.0 => {
                let n = frame.view.transform_vector3(s.normal.normalize()).normalize();
                let o = frame.view.transform_point3(s.origin);
                let (u, v, anchor) = hatch_frame(&s, frame.view.inverse().transform_point3(DVec3::ZERO));
                let anchor = frame.view.transform_point3(anchor);
                let measure = |d: DVec3| {
                    let d = frame.view.transform_vector3(d);
                    [d.x as f32, d.y as f32, d.z as f32, -d.dot(anchor) as f32]
                };
                ([n.x as f32, n.y as f32, n.z as f32, -n.dot(o) as f32], 1.0, measure(u), measure(v))
            }
            _ => ([0.0; 4], 0.0, [0.0; 4], [0.0; 4]),
        };
        let ppp = frame.pixels_per_point;
        queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals {
                proj: frame.proj.to_cols_array_2d(),
                up: dir(DVec3::Z),
                key: dir(DVec3::new(0.35, 0.25, 0.9)),
                background: [bg[0], bg[1], bg[2], 1.0],
                viewport: [size[0] as f32, size[1] as f32, EDGE_WIDTH * frame.pixels_per_point / 2.0, 0.0],
                face_front: rgb(style.face_front),
                face_back: rgb(style.face_back),
                edge: rgb(style.edge),
                clip,
                section: [section_on, style.cap_shade, style.ghost_opacity, style.ghost_edge_opacity],
                hatch_u,
                hatch_v,
                hatch: [HATCH_SPACING * ppp, HATCH_WIDTH * ppp / 2.0, style.hatch_opacity, 0.0],
            }),
        );
        // The palette and its textures: never empty (a binding needs a buffer).
        let palette: Vec<[f32; 4]> = if style.palette.is_empty() { vec![[0.0; 4]] } else { style.palette.to_vec() };
        // One per palette entry: the buffer is larger than what is written, and what lies past
        // it would read as the first layer.
        let mut textures: Vec<[f32; 4]> = style.textures.to_vec();
        textures.resize(palette.len(), NO_TEXTURE);
        let new_palette = self.palette.write(device, queue, bytemuck::cast_slice(&palette));
        let new_textures = self.texture_entries.write(device, queue, bytemuck::cast_slice(&textures));
        if new_palette || new_textures || self.bind_group.is_none() {
            let (Some(pb), Some(tb)) = (&self.palette.buffer, &self.texture_entries.buffer) else { return false };
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("globals"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: pb.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: tb.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.layers.view) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            }));
        }

        let origin = |key: u64| self.meshes.get(&key).map_or(DVec3::ZERO, |m| m.origin);
        let model_view = |t: &DMat4, key: u64| frame.view * *t * DMat4::from_translation(origin(key));

        // Instances: every batch's placements, then two per highlight (faces, edges).
        let mut raw = Vec::new();
        let mut batch_ranges = Vec::with_capacity(frame.batches.len());
        for b in frame.batches {
            let start = raw.len() as u32;
            raw.extend(
                b.placements
                    .iter()
                    .map(|p| InstanceRaw::new(&model_view(&p.transform, b.key), p.tint, p.material, p.opacity, b.key, b.grain)),
            );
            batch_ranges.push(start..raw.len() as u32);
        }
        // Highlights by runs of the same part, each run drawn in one call (a selection can
        // hold tens of thousands): its face instances, then its edge instances. Runs keep the
        // highlights' order.
        let mut runs: Vec<HighlightRun> = Vec::new();
        for chunk in frame.highlights.chunk_by(|a, b| a.key == b.key && a.part == b.part && a.ghost == b.ghost && a.face == b.face) {
            let mvs: Vec<DMat4> = chunk.iter().map(|h| model_view(&h.transform, h.key)).collect();
            let faces = raw.len() as u32;
            // A single face takes its highlight's colour over its own paint.
            let face_material = |h: &Highlight| match h.face {
                None => h.material,
                // No material: an entry past the palette (the default colour), not NO_MATERIAL,
                // which has every bit.
                Some(_) if h.material == NO_MATERIAL => FORCED | (NO_MATERIAL >> 2),
                Some(_) => FORCED | h.material,
            };
            raw.extend(chunk.iter().zip(&mvs).map(|(h, mv)| InstanceRaw::new(mv, h.face_tint, face_material(h), 0.0, h.key, h.grain)));
            let edges = raw.len() as u32;
            raw.extend(chunk.iter().zip(&mvs).map(|(h, mv)| InstanceRaw::new(mv, h.edge_tint, h.material, 0.0, h.key, h.grain)));
            runs.push(HighlightRun {
                key: chunk[0].key,
                part: chunk[0].part,
                ghost: chunk[0].ghost,
                face: chunk[0].face,
                faces: faces..edges,
                edges: edges..raw.len() as u32,
            });
        }
        self.instances.write(device, queue, bytemuck::cast_slice(&raw));

        // Lines: grid and axes, depth-tested lines, the faint lines, the hidden lines, then the
        // overlay.
        let mut lv = Vec::new();
        push_scene_lines(&mut lv, frame);
        let tested = lv.len() as u32;
        for l in frame.faint_lines {
            push_line(&mut lv, &frame.view, l, ppp, frame.model_per_point);
        }
        let faint = lv.len() as u32;
        for l in frame.hidden_lines {
            push_line(&mut lv, &frame.view, l, ppp, frame.model_per_point);
        }
        let hidden = lv.len() as u32;
        for l in frame.overlay {
            push_line(&mut lv, &frame.view, l, ppp, frame.model_per_point);
        }
        // Past the device's buffer limit, the last lines are dropped (whole quads) rather than
        // the device lost.
        let max_lines = device.limits().max_buffer_size as usize / std::mem::size_of::<LineVertex>() / QUAD.len();
        lv.truncate(max_lines * QUAD.len());
        let tested = tested.min(lv.len() as u32);
        let faint = faint.min(lv.len() as u32);
        let hidden = hidden.min(lv.len() as u32);
        self.line_vertices.write(device, queue, bytemuck::cast_slice(&lv));
        // Box outlines: one instance each.
        let mut bx: Vec<BoxRaw> = frame
            .boxes
            .iter()
            .map(|b| BoxRaw {
                model: (frame.view * b.transform).as_mat4().to_cols_array_2d(),
                color: b.color,
                pattern: b.dash.map_or([0.0; 4], |d| d.pattern.map(|l| l * ppp)),
                params: [
                    b.width * ppp / 2.0,
                    b.corners.map_or(0.0, |c| c * ppp),
                    if b.shell_on_top { 1.0 } else { 0.0 },
                    if b.over { 1.0 } else { 0.0 },
                ],
            })
            .collect();
        let fronts = frame.boxes.iter().any(|b| b.shell_on_top || b.over);
        bx.truncate(device.limits().max_buffer_size as usize / std::mem::size_of::<BoxRaw>());
        self.box_instances.write(device, queue, bytemuck::cast_slice(&bx));
        // Fills by group (each drawn by its pipeline), in their order within it: a polygon as a
        // fan of triangles, triangles as they are.
        let mut fv = Vec::new();
        let mut fill_ranges: [Range<u32>; 4] = Default::default();
        for (k, group) in FillGroup::ALL.into_iter().enumerate() {
            let start = fv.len() as u32;
            for f in frame.fills.iter().filter(|f| FillGroup::of(f) == group) {
                let at = |p: DVec3| FillVertex { pos: frame.view.transform_point3(p).as_vec3().to_array(), color: f.color };
                if f.triangles {
                    fv.extend(f.corners.chunks_exact(3).flat_map(|t| t.iter().map(|p| at(*p))));
                } else {
                    for i in 1..f.corners.len().saturating_sub(1) {
                        fv.extend([at(f.corners[0]), at(f.corners[i]), at(f.corners[i + 1])]);
                    }
                }
            }
            fill_ranges[k] = start..fv.len() as u32;
        }
        let max_triangles = device.limits().max_buffer_size as usize / std::mem::size_of::<FillVertex>() / 3;
        fv.truncate(max_triangles * 3);
        let fill_ranges = fill_ranges.map(|r| r.start.min(fv.len() as u32)..r.end.min(fv.len() as u32));
        self.fill_vertices.write(device, queue, bytemuck::cast_slice(&fv));

        let (Some(target), Some(bind_group)) = (&self.target, &self.bind_group) else { return false };
        let mut pass = Counted {
            pass: encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa,
                    depth_slice: None,
                    resolve_target: Some(&target.resolved_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: 1.0 }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }),
            calls: 0,
            vertices: 0,
        };
        pass.set_bind_group(0, bind_group, &[]);

        if let Some(inst) = &self.instances.buffer {
            // Faces.
            pass.set_pipeline(if section_on > 0.0 { &self.faces_cut } else { &self.faces });
            pass.set_vertex_buffer(1, inst.slice(..));
            for (b, r) in frame.batches.iter().zip(&batch_ranges).filter(|(b, _)| !b.ghost) {
                let Some(m) = self.meshes.get(&b.key) else { continue };
                if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                    pass.set_vertex_buffer(0, v.slice(..));
                    pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.index_count, 0, r.clone());
                }
            }
            for run in runs.iter().filter(|r| !r.ghost) {
                let Some((m, part)) = part_of(&self.meshes, run.key, run.part) else { continue };
                if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                    pass.set_vertex_buffer(0, v.slice(..));
                    pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                    for r in run.indices(part) {
                        pass.draw_indexed(r, 0, run.faces.clone());
                    }
                }
            }
            if section_on > 0.0 {
                // The caps: where the faces left along the ray are odd in number, the ray
                // meets the plane inside a part; there, the back faces drawn again just behind
                // the plane, so that the cut part's wins over another part's front face lying
                // on it (parts in contact, a dowel in its hole) and what lies behind the cap
                // stays hidden.
                pass.set_stencil_reference(1);
                pass.set_pipeline(&self.cap_mark);
                for (b, r) in frame.batches.iter().zip(&batch_ranges).filter(|(b, _)| !b.ghost) {
                    let Some(m) = self.meshes.get(&b.key) else { continue };
                    if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                        pass.set_vertex_buffer(0, v.slice(..));
                        pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..m.index_count, 0, r.clone());
                    }
                }
                pass.set_pipeline(&self.caps);
                for (b, r) in frame.batches.iter().zip(&batch_ranges).filter(|(b, _)| !b.ghost) {
                    let Some(m) = self.meshes.get(&b.key) else { continue };
                    if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                        pass.set_vertex_buffer(0, v.slice(..));
                        pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..m.index_count, 0, r.clone());
                    }
                }
                // The highlighted ones over them, tinted (at the same depth).
                for run in runs.iter().filter(|r| !r.ghost) {
                    let Some((m, part)) = part_of(&self.meshes, run.key, run.part) else { continue };
                    if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                        pass.set_vertex_buffer(0, v.slice(..));
                        pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                        for r in run.indices(part) {
                            pass.draw_indexed(r, 0, run.faces.clone());
                        }
                    }
                }
            }
            // Edges, then silhouettes (same ink, same instances).
            pass.set_pipeline(&self.edges);
            for (b, r) in frame.batches.iter().zip(&batch_ranges).filter(|(b, _)| !b.faint_edges) {
                let Some(m) = self.meshes.get(&b.key) else { continue };
                if let Some(e) = &m.edges {
                    pass.set_vertex_buffer(0, e.slice(..));
                    pass.draw(0..m.edge_vertex_count, r.clone());
                }
            }
            for run in runs.iter().filter(|r| r.face.is_none()) {
                let Some((m, part)) = part_of(&self.meshes, run.key, run.part) else { continue };
                if let Some(e) = &m.edges {
                    pass.set_vertex_buffer(0, e.slice(..));
                    pass.draw(part.edges.clone(), run.edges.clone());
                }
            }
            if style.silhouettes {
                pass.set_pipeline(&self.silhouettes);
                for (b, r) in frame.batches.iter().zip(&batch_ranges).filter(|(b, _)| !b.faint_edges) {
                    let Some(m) = self.meshes.get(&b.key) else { continue };
                    if let Some(s) = &m.silhouettes {
                        pass.set_vertex_buffer(0, s.slice(..));
                        pass.draw(0..m.silhouette_vertex_count, r.clone());
                    }
                }
                for run in runs.iter().filter(|r| r.face.is_none()) {
                    let Some((m, part)) = part_of(&self.meshes, run.key, run.part) else { continue };
                    if let Some(s) = &m.silhouettes {
                        pass.set_vertex_buffer(0, s.slice(..));
                        pass.draw(part.silhouettes.clone(), run.edges.clone());
                    }
                }
            }
        }
        // Ghosts' faces, over everything opaque.
        if let Some(inst) = &self.instances.buffer
            && (frame.batches.iter().any(|b| b.ghost) || runs.iter().any(|r| r.ghost))
        {
            pass.set_pipeline(&self.faces_ghost);
            pass.set_vertex_buffer(1, inst.slice(..));
            for (b, r) in frame.batches.iter().zip(&batch_ranges).filter(|(b, _)| b.ghost) {
                let Some(m) = self.meshes.get(&b.key) else { continue };
                if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                    pass.set_vertex_buffer(0, v.slice(..));
                    pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.index_count, 0, r.clone());
                }
            }
            // Ghosts highlighted: a wash of their tint, over the ghost's own faces.
            pass.set_pipeline(&self.faces_ghost_mark);
            for run in runs.iter().filter(|r| r.ghost) {
                let Some((m, part)) = part_of(&self.meshes, run.key, run.part) else { continue };
                if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                    pass.set_vertex_buffer(0, v.slice(..));
                    pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                    for r in run.indices(part) {
                        pass.draw_indexed(r, 0, run.faces.clone());
                    }
                }
            }
        }
        // Faint edges and silhouettes, over the ghosts' faces.
        if let Some(inst) = &self.instances.buffer
            && frame.batches.iter().any(|b| b.faint_edges)
        {
            pass.set_vertex_buffer(1, inst.slice(..));
            pass.set_pipeline(&self.edges_faint);
            for (b, r) in frame.batches.iter().zip(&batch_ranges).filter(|(b, _)| b.faint_edges) {
                let Some(m) = self.meshes.get(&b.key) else { continue };
                if let Some(e) = &m.edges {
                    pass.set_vertex_buffer(0, e.slice(..));
                    pass.draw(0..m.edge_vertex_count, r.clone());
                }
            }
            if style.silhouettes {
                pass.set_pipeline(&self.silhouettes_faint);
                for (b, r) in frame.batches.iter().zip(&batch_ranges).filter(|(b, _)| b.faint_edges) {
                    let Some(m) = self.meshes.get(&b.key) else { continue };
                    if let Some(s) = &m.silhouettes {
                        pass.set_vertex_buffer(0, s.slice(..));
                        pass.draw(0..m.silhouette_vertex_count, r.clone());
                    }
                }
            }
        }
        if let Some(lb) = &self.line_vertices.buffer {
            pass.set_vertex_buffer(0, lb.slice(..));
            if tested > 0 {
                pass.set_pipeline(&self.lines);
                pass.draw(0..tested, 0..1);
            }
        }
        if let Some(bb) = &self.box_instances.buffer
            && !bx.is_empty()
        {
            pass.set_pipeline(&self.boxes);
            pass.set_vertex_buffer(0, bb.slice(..));
            pass.draw(0..BOX_VERTICES, 0..bx.len() as u32);
        }
        // Faint lines over what is drawn (opaque), blended.
        if let Some(lb) = &self.line_vertices.buffer
            && faint > tested
        {
            pass.set_pipeline(&self.faint_lines);
            pass.set_vertex_buffer(0, lb.slice(..));
            pass.draw(tested..faint, 0..1);
        }
        // Fills in the scene over the depth-tested lines (which they tint).
        let draw_fills = |pass: &mut Counted<'_>, over: bool| {
            let Some(fb) = &self.fill_vertices.buffer else { return };
            for (g, r) in FillGroup::ALL.iter().zip(&fill_ranges) {
                if g.over() == over && !r.is_empty() {
                    pass.set_pipeline(&self.fills[*g as usize]);
                    pass.set_vertex_buffer(0, fb.slice(..));
                    pass.draw(r.clone(), 0..1);
                }
            }
        };
        draw_fills(&mut pass, false);
        if let Some(lb) = &self.line_vertices.buffer
            && hidden > faint
        {
            pass.set_vertex_buffer(0, lb.slice(..));
            pass.set_pipeline(&self.hidden);
            pass.draw(faint..hidden, 0..1);
        }
        // Then over everything: the boxes' fronts, the fills, the overlay's lines.
        if let Some(bb) = &self.box_instances.buffer
            && fronts
        {
            pass.set_pipeline(&self.boxes_front);
            pass.set_vertex_buffer(0, bb.slice(..));
            pass.draw(0..BOX_VERTICES, 0..bx.len() as u32);
        }
        draw_fills(&mut pass, true);
        if let Some(lb) = &self.line_vertices.buffer {
            let total = lv.len() as u32;
            if total > hidden {
                pass.set_vertex_buffer(0, lb.slice(..));
                pass.set_pipeline(&self.overlay);
                pass.draw(hidden..total, 0..1);
            }
        }
        self.stats = Stats {
            draw_calls: pass.calls,
            triangles: pass.vertices / 3,
            instances: raw.len() as u32,
            lines: (lv.len() / QUAD.len()) as u32,
            boxes: bx.len() as u32,
            upload_bytes: (std::mem::size_of_val(raw.as_slice())
                + std::mem::size_of_val(lv.as_slice())
                + std::mem::size_of_val(bx.as_slice())
                + std::mem::size_of_val(fv.as_slice())) as u64,
        };
        drop(pass);
        self.drawn = Some(print);
        true
    }

    /// What the picture in the target cost.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The meshes uploaded, every document's, and the bytes they take on the GPU.
    pub fn mesh_memory(&self) -> (usize, u64) {
        let all = || std::iter::once(&self.meshes).chain(self.parked.values()).flat_map(|m| m.values());
        let bytes = all().flat_map(|m| [&m.vertices, &m.indices, &m.edges, &m.silhouettes]).flatten().map(|b| b.size()).sum();
        (all().count(), bytes)
    }

    /// A new [`Picture`] of `size` pixels, to draw with [`Self::render_picture`].
    pub fn new_picture(&self, device: &wgpu::Device, size: [u32; 2]) -> Picture {
        let size = [size[0].max(1), size[1].max(1)];
        let resolved = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("picture"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[DISPLAY_FORMAT],
        });
        Picture {
            size,
            resolved_view: resolved.create_view(&Default::default()),
            display_view: resolved.create_view(&wgpu::TextureViewDescriptor { format: Some(DISPLAY_FORMAT), ..Default::default() }),
            resolved,
        }
    }

    /// Draws `frame` into `picture`, submitted at once (its own encoder: the buffers it writes
    /// are the viewport's too, so a picture is never encoded with the viewport's frame). The
    /// viewport's target, the fingerprint of its picture and its statistics are left as they
    /// were: a still view stays still.
    pub fn render_picture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame<'_>, picture: &Picture) {
        let size = picture.size;
        if self.picture_scratch.as_ref().is_none_or(|s| s.size != size) {
            let texture = |label: &str, format| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: SAMPLES,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            self.picture_scratch =
                Some(PictureScratch { size, msaa: texture("picture msaa", TARGET_FORMAT), depth: texture("picture depth", self.depth_format) });
        }
        let Some(scratch) = &self.picture_scratch else { return };
        let target = Target {
            size,
            msaa: scratch.msaa.clone(),
            depth: scratch.depth.clone(),
            resolved: picture.resolved.clone(),
            resolved_view: picture.resolved_view.clone(),
            display_view: picture.display_view.clone(),
        };
        let kept = (self.target.replace(target), self.drawn.take(), self.stats, self.generation, self.target_generation);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("picture") });
        self.render(device, queue, &mut encoder, size, frame);
        queue.submit([encoder.finish()]);
        (self.target, self.drawn, self.stats, self.generation, self.target_generation) = kept;
    }

    fn ensure_target(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let texture = |label: &str, format, samples, usage, view_formats: &[wgpu::TextureFormat]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats,
            })
        };
        let msaa = texture("viewport msaa", TARGET_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT, &[]);
        let depth = texture("viewport depth", self.depth_format, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT, &[]);
        let resolved = texture(
            "viewport",
            TARGET_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            &[DISPLAY_FORMAT],
        );
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        self.target = Some(Target {
            size,
            msaa: view(&msaa),
            depth: view(&depth),
            resolved_view: view(&resolved),
            display_view: resolved.create_view(&wgpu::TextureViewDescriptor { format: Some(DISPLAY_FORMAT), ..Default::default() }),
            resolved,
        });
        self.target_generation += 1;
        self.generation += 1;
    }
}

fn part_of(meshes: &std::collections::HashMap<u64, GpuMesh>, key: u64, part: u64) -> Option<(&GpuMesh, &PartRange)> {
    let m = meshes.get(&key)?;
    Some((m, m.parts.iter().find(|p| p.id == part)?))
}

/// Consecutive highlights of the same part of the same mesh, and their instances.
struct HighlightRun {
    key: u64,
    part: u64,
    /// Its faces drawn with the ghosts'.
    ghost: bool,
    /// Only this B-rep face, without edges ([`Highlight::face`]).
    face: Option<u32>,
    /// Tinted by their faces' tint, then by their edges'.
    faces: Range<u32>,
    edges: Range<u32>,
}

impl HighlightRun {
    /// The index ranges it draws of `part`: all of it, or its face's runs of triangles.
    fn indices<'a>(&self, part: &'a PartRange) -> Box<dyn Iterator<Item = Range<u32>> + 'a> {
        match self.face {
            None => Box::new(std::iter::once(part.indices.clone())),
            Some(face) => {
                let from = part.faces.partition_point(|(f, _)| *f < face);
                Box::new(part.faces[from..].iter().take_while(move |(f, _)| *f == face).map(|(_, r)| r.clone()))
            }
        }
    }
}

/// Everything that makes the picture of `frame` (docs/render/look.md): equal fingerprints, equal
/// pictures. `generation` stands for what is uploaded (meshes, target).
fn fingerprint(frame: &Frame<'_>, size: [u32; 2], generation: u64) -> u64 {
    struct H(std::collections::hash_map::DefaultHasher);
    impl H {
        fn f(&mut self, v: f64) {
            v.to_bits().hash(&mut self.0);
        }
        fn fs(&mut self, vs: &[f32]) {
            for v in vs {
                v.to_bits().hash(&mut self.0);
            }
        }
        fn m(&mut self, m: &DMat4) {
            for v in m.to_cols_array() {
                self.f(v);
            }
        }
        fn v(&mut self, v: DVec3) {
            self.f(v.x);
            self.f(v.y);
            self.f(v.z);
        }
    }
    let mut h = H(Default::default());
    (size, generation).hash(&mut h.0);
    h.m(&frame.view);
    h.fs(&frame.proj.to_cols_array());
    h.fs(&frame.background);
    let s = &frame.style;
    h.fs(&s.face_front);
    h.fs(&s.face_back);
    h.fs(&s.edge);
    h.fs(&s.grid);
    h.fs(&[s.cap_shade, s.hatch_opacity, s.ghost_opacity, s.ghost_edge_opacity]);
    s.palette.len().hash(&mut h.0);
    for c in s.palette {
        h.fs(c);
    }
    s.textures.len().hash(&mut h.0);
    for t in s.textures {
        h.fs(t);
    }
    s.silhouettes.hash(&mut h.0);
    match s.section {
        Some(sec) => {
            1u8.hash(&mut h.0);
            h.v(sec.origin);
            h.v(sec.normal);
        }
        None => 0u8.hash(&mut h.0),
    }
    frame.batches.len().hash(&mut h.0);
    for b in frame.batches {
        (b.key, b.placements.len(), b.ghost, b.faint_edges).hash(&mut h.0);
        for p in &b.placements {
            h.m(&p.transform);
            h.fs(&p.tint);
            h.fs(&[p.opacity]);
            p.material.hash(&mut h.0);
        }
    }
    frame.highlights.len().hash(&mut h.0);
    for hl in frame.highlights {
        (hl.key, hl.part, hl.material, hl.ghost, hl.face).hash(&mut h.0);
        h.m(&hl.transform);
        h.fs(&hl.face_tint);
        h.fs(&hl.edge_tint);
    }
    let dash = |h: &mut H, d: &Option<Dash>| match d {
        Some(d) => {
            h.fs(&d.pattern);
            match d.lay {
                Lay::Model(phase) => (1u8, phase.to_bits()).hash(&mut h.0),
                Lay::Anchored => 2u8.hash(&mut h.0),
                Lay::Screen(phase) => (3u8, phase.to_bits()).hash(&mut h.0),
            }
        }
        None => 0u8.hash(&mut h.0),
    };
    for lines in [frame.lines, frame.faint_lines, frame.overlay, frame.hidden_lines] {
        lines.len().hash(&mut h.0);
        for l in lines {
            h.v(l.a);
            h.v(l.b);
            h.fs(&l.color);
            h.fs(&[l.width]);
            dash(&mut h, &l.dash);
        }
    }
    frame.boxes.len().hash(&mut h.0);
    for b in frame.boxes {
        h.m(&b.transform);
        h.fs(&b.color);
        h.fs(&[b.width, b.corners.unwrap_or(0.0)]);
        dash(&mut h, &b.dash);
        (b.shell_on_top, b.over).hash(&mut h.0);
    }
    frame.fills.len().hash(&mut h.0);
    for f in frame.fills {
        f.corners.len().hash(&mut h.0);
        for c in &f.corners {
            h.v(*c);
        }
        h.fs(&f.color);
        (f.triangles, f.over, f.cull_back).hash(&mut h.0);
    }
    match frame.grid {
        Some(g) => {
            (1u8, g.count).hash(&mut h.0);
            h.f(g.spacing);
        }
        None => 0u8.hash(&mut h.0),
    }
    frame.axes.hash(&mut h.0);
    h.m(&frame.axes_frame);
    h.fs(&[frame.pixels_per_point]);
    frame.model_per_point.to_bits().hash(&mut h.0);
    h.0.finish()
}

/// A silhouette candidate: a mesh edge between two triangles of a curved surface, with their
/// normals (in the mesh's coordinates, relative to its origin).
#[derive(Clone, Copy, Debug)]
struct SilhouetteEdge {
    a: [f32; 3],
    b: [f32; 3],
    n1: [f32; 3],
    n2: [f32; 3],
}

/// The mesh edges shared by two triangles at an angle that are not B-rep edges (those are
/// drawn anyway): where a curved surface can turn away from the eye (docs/render/look.md). Vertices are
/// welded by position: the mesher may split them along seams.
fn silhouette_edges(m: &Mesh) -> Vec<SilhouetteEdge> {
    use std::collections::HashMap;
    // A position, quantised finely relative to the mesh's size.
    let extent = m.positions.iter().fold(0f32, |e, p| e.max(p[0].abs()).max(p[1].abs()).max(p[2].abs())).max(1e-6);
    let q = 1e-5 * extent;
    let key = |p: &[f32; 3]| [(p[0] / q).round() as i64, (p[1] / q).round() as i64, (p[2] / q).round() as i64];
    let mut weld: HashMap<[i64; 3], u32> = HashMap::new();
    let ids: Vec<u32> = m
        .positions
        .iter()
        .map(|p| {
            let n = weld.len() as u32;
            *weld.entry(key(p)).or_insert(n)
        })
        .collect();
    let pair = |a: u32, b: u32| if a < b { (a, b) } else { (b, a) };
    // Segments of B-rep edges, welded the same way.
    let mut brep = std::collections::HashSet::new();
    for poly in &m.edges {
        for w in poly.windows(2) {
            if let (Some(&a), Some(&b)) = (weld.get(&key(&w[0])), weld.get(&key(&w[1]))) {
                brep.insert(pair(a, b));
            }
        }
    }
    // Each mesh edge: the triangles along it (first vertex index kept for its position).
    // Per welded edge: its triangles' normals, and its two vertex indices.
    type Sides = HashMap<(u32, u32), (Vec<[f32; 3]>, [u32; 2])>;
    let mut sides: Sides = HashMap::new();
    let pos = |i: u32| Vec3::from_array(m.positions[i as usize]);
    for tri in m.indices.chunks_exact(3) {
        let n = (pos(tri[1]) - pos(tri[0])).cross(pos(tri[2]) - pos(tri[0]));
        if n.length_squared() == 0.0 {
            continue;
        }
        let n = n.normalize().to_array();
        for k in 0..3 {
            let (i, j) = (tri[k], tri[(k + 1) % 3]);
            let (a, b) = (ids[i as usize], ids[j as usize]);
            if a == b {
                continue;
            }
            sides.entry(pair(a, b)).or_insert_with(|| (Vec::new(), [i, j])).0.push(n);
        }
    }
    let mut out = Vec::new();
    for (k, (normals, [i, j])) in sides {
        let [n1, n2] = normals.as_slice() else { continue };
        if brep.contains(&k) {
            continue;
        }
        // Flat (inside a plane, or nearly): never an outline.
        if Vec3::from_array(*n1).dot(Vec3::from_array(*n2)) > 0.99999 {
            continue;
        }
        out.push(SilhouetteEdge { a: m.positions[i as usize], b: m.positions[j as usize], n1: *n1, n2: *n2 });
    }
    out
}

/// A world line, in camera coordinates (subtracted in `f64`), as wide as it says.
/// `ppp`: pixels per point (dashes are in points); `mpp`: model units per point at the view's
/// focus (dashes laid in model units).
fn push_line(out: &mut Vec<LineVertex>, view: &DMat4, l: &Line, ppp: f32, mpp: f64) {
    let half = l.width * ppp / 2.0;
    let (a, b) = (view.transform_point3(l.a), view.transform_point3(l.b));
    // `dash.x`: the shader's `LAY_*`.
    let (pattern, dash) = match l.dash {
        Some(Dash { pattern, lay: Lay::Anchored }) => (pattern.map(|l| l * ppp), [1.0, 1.0, 0.0, 0.0]),
        Some(Dash { pattern, lay: Lay::Screen(phase) }) => (pattern.map(|l| l * ppp), [2.0, phase * ppp, 0.0, 0.0]),
        Some(Dash { pattern, lay: Lay::Model(phase) }) => {
            // The pattern in model units, its period the nearest power of two.
            let points: f64 = pattern.iter().map(|l| *l as f64).sum();
            let period = if points > 0.0 && mpp > 0.0 { (points * mpp).log2().round().clamp(-100.0, 100.0).exp2() } else { 0.0 };
            let pattern = pattern.map(|l| if period > 0.0 { (l as f64 * period / points) as f32 } else { 0.0 });
            // From the nearer end (the eye looks down -Z), where a model unit is the most
            // pixels: the float keeps the dashes there; the shader adds its phase exactly (high
            // and low parts).
            let len = l.a.distance(l.b);
            let from_b = b.z > a.z;
            let at = if from_b { phase + len } else { phase };
            let hi = at as f32;
            let lo = (at - hi as f64) as f32;
            (pattern, [0.0, hi, lo, if from_b { -len } else { len } as f32])
        }
        None => ([0.0; 4], [0.0; 4]),
    };
    let (a, b) = (a.as_vec3().to_array(), b.as_vec3().to_array());
    out.extend(QUAD.map(|[end, side]| LineVertex { a, b, corner: [end, side, half], color: l.color, pattern, dash }));
}

fn push_scene_lines(out: &mut Vec<LineVertex>, frame: &Frame<'_>) {
    // The grid in the context's frame (composed in f64, so far frames stay exact).
    let v = &(frame.view * frame.axes_frame);
    let ppp = frame.pixels_per_point;
    if let Some(g) = frame.grid {
        let n = g.count as i32;
        let ext = g.spacing * g.count as f64;
        for i in -n..=n {
            if i == 0 && frame.axes {
                continue;
            }
            let t = i as f64 * g.spacing;
            let alpha = if i % 10 == 0 { 0.30 } else { 0.12 };
            let [r, g, b] = frame.style.grid;
            let c = [r, g, b, alpha];
            push_line(
                out,
                v,
                &Line { a: DVec3::new(-ext, t, 0.0), b: DVec3::new(ext, t, 0.0), color: c, dash: None, width: GRID_WIDTH },
                ppp,
                frame.model_per_point,
            );
            push_line(
                out,
                v,
                &Line { a: DVec3::new(t, -ext, 0.0), b: DVec3::new(t, ext, 0.0), color: c, dash: None, width: GRID_WIDTH },
                ppp,
                frame.model_per_point,
            );
        }
    }
    for l in frame.lines {
        push_line(out, &frame.view, l, ppp, frame.model_per_point);
    }
}

#[cfg(test)]
mod tests;

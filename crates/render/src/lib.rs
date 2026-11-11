//! SolidSketch's viewport renderer (docs/ARCHITECTURE.md §7 bis, §7 quater).
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
//! Precision (ARCHITECTURE.md §7 bis): the GPU only sees `f32`, so everything is drawn
//! **relative to the camera**. Placements, the view and lines are `f64`; each placement's
//! `view × world × recentre` is computed on the CPU in `f64` and only then made `f32`, and each
//! definition's mesh is stored around its own centre. Depth is **reversed** (1 near, 0 far) in
//! a float depth buffer (with a stencil, for section caps): [`Frame::proj`] must map near to
//! 1, e.g. `Mat4::perspective_infinite_reverse_rh`.
//!
//! Colours (§7 quater): faces take a colour from the frame's **palette** — the face's own
//! (baked in the mesh, [`Part::face_paint`]), else its placement's ([`Placement::material`]),
//! else [`Style::face_front`]. Changing a palette colour re-uploads no vertex.
//!
//! A **ghost** batch (an open part's blank, ARCHITECTURE.md §2.9) has its edges drawn like any
//! other, but its faces only after everything opaque: their fronts, blended at
//! [`Style::ghost_opacity`], depth tested without writing depth.

use std::hash::{Hash, Hasher};
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::{DMat3, DMat4, DVec3, Mat4, Vec3};
use solidsketch_kernel::Mesh;
use wgpu::util::DeviceExt;

/// The format the viewport is drawn in: sRGB-encoded bytes, blended in linear.
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// The format [`Renderer::target`] is viewed with: the same bytes, without sRGB decoding,
/// which is what egui's renderer expects of a native texture.
pub const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Depth with a stencil (section caps, §7 quater): 32-bit float when the device has it — reversed
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
}

impl Batch {
    pub fn new(key: u64, placements: Vec<Placement>) -> Self {
        Batch { key, placements, ghost: false }
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
}

impl Placement {
    pub fn new(transform: DMat4) -> Self {
        Placement { transform, tint: NO_TINT, material: NO_MATERIAL }
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
}

/// A world-space segment, coloured (`rgb` linear, `a` mixed over the background), plain or
/// dashed.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub a: DVec3,
    pub b: DVec3,
    pub color: [f32; 4],
    pub dash: Option<Dash>,
}

/// A line's dashes, on screen: a dash of `length` points, a gap as long, the pattern at
/// `phase` points at the line's `a` (so that it can hold still on the line as the view moves).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dash {
    pub length: f32,
    pub phase: f32,
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
/// away (§7 quater).
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
    /// The colours faces and placements refer to (linear RGB; alpha unused).
    pub palette: &'a [[f32; 4]],
    /// Draw the outline of curved faces.
    pub silhouettes: bool,
    pub section: Option<Section>,
    /// How much darker than its part a section cap is (0–1).
    pub cap_shade: f32,
    /// How opaque a ghost batch's faces are (0–1).
    pub ghost_opacity: f32,
}

impl Default for Style<'_> {
    fn default() -> Self {
        Style {
            face_front: [0.92, 0.91, 0.87],
            face_back: [0.55, 0.60, 0.72],
            edge: [0.02, 0.02, 0.03],
            palette: &[],
            silhouettes: true,
            section: None,
            cap_shade: 0.25,
            ghost_opacity: 0.15,
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
    /// Lines drawn over everything (tool previews).
    pub overlay: &'a [Line],
    pub grid: Option<Grid>,
    /// The axes are drawn (in `lines`): the grid leaves out its lines along them.
    pub axes: bool,
    /// Where the grid is: the frame of the context being edited, to world.
    pub axes_frame: DMat4,
    /// Pixels per point of the screen: line widths are in points, so they look the same on
    /// any display.
    pub pixels_per_point: f32,
}

/// Line widths, in points.
pub const EDGE_WIDTH: f32 = 1.5;
pub const LINE_WIDTH: f32 = 1.5;
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

/// A corner of a coloured line's quad; `corner.z` is its half width in pixels; `dash`: the
/// period of its dash pattern (pixels, 0 when plain) and where the pattern is at `a`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineVertex {
    a: [f32; 3],
    b: [f32; 3],
    corner: [f32; 3],
    color: [f32; 4],
    dash: [f32; 2],
}

/// The corners of a segment's quad, in drawing order.
const QUAD: [[f32; 2]; 6] = [[0.0, -1.0], [0.0, 1.0], [1.0, -1.0], [1.0, -1.0], [0.0, 1.0], [1.0, 1.0]];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 3],
    tint: [f32; 4],
    material: u32,
}

impl InstanceRaw {
    /// `model_view`: from the mesh's (recentred) coordinates to the camera's, in `f64`.
    fn new(model_view: &DMat4, tint: Tint, material: u32) -> Self {
        let linear = DMat3::from_mat4(*model_view);
        let n = linear.inverse().transpose();
        let c = |v: DVec3, w: f64| [v.x as f32, v.y as f32, v.z as f32, w as f32];
        // The winding's sign, for the shader to tell outside from inside under a mirror.
        let mirrored = if linear.determinant() < 0.0 { -1.0 } else { 1.0 };
        InstanceRaw {
            model: model_view.as_mat4().to_cols_array_2d(),
            normal: [c(n.x_axis, mirrored), c(n.y_axis, 0.0), c(n.z_axis, 0.0)],
            tint,
            material,
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
}

struct PartRange {
    id: u64,
    indices: Range<u32>,
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
            let capacity = len.next_power_of_two().max(4096);
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
    /// Then the caps marked in the stencil (the parity of the faces left along each ray), and
    /// their depth brought onto the plane.
    cap_mark: wgpu::RenderPipeline,
    cap_depth: wgpu::RenderPipeline,
    depth_format: wgpu::TextureFormat,
    edges: wgpu::RenderPipeline,
    silhouettes: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    palette: Dynamic,
    meshes: std::collections::HashMap<u64, GpuMesh>,
    instances: Dynamic,
    line_vertices: Dynamic,
    target: Option<Target>,
    /// Bumped whenever what is uploaded changes (a mesh, the target): part of a frame's
    /// fingerprint.
    generation: u64,
    target_generation: u64,
    /// The fingerprint of the picture in the target.
    drawn: Option<u64>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("solidsketch shader"),
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
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("solidsketch"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let instance_attrs = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4,
            12 => Uint32
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
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x4, 4 => Float32x2];
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &line_attrs,
        };

        let blended = |label: &str,
                       vs: &str,
                       fs: &str,
                       buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                       depth: wgpu::DepthStencilState,
                       write_mask: wgpu::ColorWrites,
                       blend: Option<wgpu::BlendState>,
                       unclipped_depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
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
        // The stencil of the caps: its low bit flipped by `cap_mark`, tested by `cap_depth`.
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
        // Every fragment left flips the stencil: an odd count, the ray meets the plane inside
        // a part.
        let cap_mark = pipeline(
            "caps (mark)",
            "vs_face",
            "fs_cap_mark",
            &[Some(face_layout), Some(instance_layout.clone())],
            wgpu::DepthStencilState {
                stencil: stencil(wgpu::CompareFunction::Always, wgpu::StencilOperation::Invert),
                ..depth(false, wgpu::CompareFunction::Always, Default::default())
            },
            wgpu::ColorWrites::empty(),
        );
        let cap_depth = pipeline(
            "caps (depth)",
            "vs_screen",
            "fs_cap_depth",
            &[],
            wgpu::DepthStencilState { stencil: stencil(wgpu::CompareFunction::Equal, Keep), ..depth(true, nearer, Default::default()) },
            wgpu::ColorWrites::empty(),
        );
        let edges = pipeline(
            "edges",
            "vs_edge",
            "fs_edge",
            &[Some(edge_layout), Some(instance_layout.clone())],
            depth(true, nearer, Default::default()),
            all,
        );
        let silhouettes = pipeline(
            "silhouettes",
            "vs_silhouette",
            "fs_edge",
            &[Some(sil_layout), Some(instance_layout)],
            depth(true, nearer, Default::default()),
            all,
        );
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
        let overlay = blended(
            "overlay",
            "vs_line",
            "fs_line",
            &[Some(line_layout)],
            depth(false, wgpu::CompareFunction::Always, Default::default()),
            all,
            None,
            unclipped,
        );

        Renderer {
            faces,
            faces_cut,
            faces_ghost,
            cap_mark,
            cap_depth,
            depth_format,
            edges,
            silhouettes,
            lines,
            overlay,
            globals,
            layout: bgl,
            bind_group: None,
            palette: Dynamic::new("palette", wgpu::BufferUsages::STORAGE),
            meshes: Default::default(),
            instances: Dynamic::new("instances", wgpu::BufferUsages::VERTEX),
            line_vertices: Dynamic::new("lines", wgpu::BufferUsages::VERTEX),
            target: None,
            generation: 0,
            target_generation: 0,
            drawn: None,
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
            for (t, tri) in p.mesh.indices.chunks_exact(3).enumerate() {
                let face = p.mesh.tri_face.get(t).copied().unwrap_or(u32::MAX) as usize;
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
            ranges.push(PartRange {
                id: p.id,
                indices: i0..indices.len() as u32,
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
        let (clip, section_on) = match style.section {
            Some(s) if s.normal.length_squared() > 0.0 => {
                let n = frame.view.transform_vector3(s.normal.normalize()).normalize();
                let o = frame.view.transform_point3(s.origin);
                ([n.x as f32, n.y as f32, n.z as f32, -n.dot(o) as f32], 1.0)
            }
            _ => ([0.0; 4], 0.0),
        };
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
                section: [section_on, style.cap_shade, style.ghost_opacity, 0.0],
            }),
        );
        // The palette: never empty (a binding needs a buffer).
        let palette: Vec<[f32; 4]> = if style.palette.is_empty() { vec![[0.0; 4]] } else { style.palette.to_vec() };
        if self.palette.write(device, queue, bytemuck::cast_slice(&palette)) || self.bind_group.is_none() {
            let Some(pb) = &self.palette.buffer else { return false };
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("globals"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: pb.as_entire_binding() },
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
            raw.extend(b.placements.iter().map(|p| InstanceRaw::new(&model_view(&p.transform, b.key), p.tint, p.material)));
            batch_ranges.push(start..raw.len() as u32);
        }
        let highlight_start = raw.len() as u32;
        for h in frame.highlights {
            let mv = model_view(&h.transform, h.key);
            raw.push(InstanceRaw::new(&mv, h.face_tint, h.material));
            raw.push(InstanceRaw::new(&mv, h.edge_tint, h.material));
        }
        self.instances.write(device, queue, bytemuck::cast_slice(&raw));

        // Lines: grid and axes, depth-tested lines, then the overlay.
        let mut lv = Vec::new();
        let ppp = frame.pixels_per_point;
        let line_half = LINE_WIDTH * ppp / 2.0;
        push_scene_lines(&mut lv, frame);
        let tested = lv.len() as u32;
        for l in frame.overlay {
            push_line(&mut lv, &frame.view, l, line_half, ppp);
        }
        self.line_vertices.write(device, queue, bytemuck::cast_slice(&lv));

        let (Some(target), Some(bind_group)) = (&self.target, &self.bind_group) else { return false };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        });
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
            for (k, h) in frame.highlights.iter().enumerate() {
                let Some((m, part)) = part_of(&self.meshes, h) else { continue };
                if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                    let at = highlight_start + 2 * k as u32;
                    pass.set_vertex_buffer(0, v.slice(..));
                    pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(part.indices.clone(), 0, at..at + 1);
                }
            }
            if section_on > 0.0 {
                // The caps: where the faces left along the ray are odd in number, the ray
                // meets the plane inside a part; there, the depth goes onto the plane, so
                // what lies behind the cap stays hidden.
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
                pass.set_pipeline(&self.cap_depth);
                pass.draw(0..3, 0..1);
            }
            // Edges, then silhouettes (same ink, same instances).
            pass.set_pipeline(&self.edges);
            for (b, r) in frame.batches.iter().zip(&batch_ranges) {
                let Some(m) = self.meshes.get(&b.key) else { continue };
                if let Some(e) = &m.edges {
                    pass.set_vertex_buffer(0, e.slice(..));
                    pass.draw(0..m.edge_vertex_count, r.clone());
                }
            }
            for (k, h) in frame.highlights.iter().enumerate() {
                let Some((m, part)) = part_of(&self.meshes, h) else { continue };
                if let Some(e) = &m.edges {
                    let at = highlight_start + 2 * k as u32 + 1;
                    pass.set_vertex_buffer(0, e.slice(..));
                    pass.draw(part.edges.clone(), at..at + 1);
                }
            }
            if style.silhouettes {
                pass.set_pipeline(&self.silhouettes);
                for (b, r) in frame.batches.iter().zip(&batch_ranges) {
                    let Some(m) = self.meshes.get(&b.key) else { continue };
                    if let Some(s) = &m.silhouettes {
                        pass.set_vertex_buffer(0, s.slice(..));
                        pass.draw(0..m.silhouette_vertex_count, r.clone());
                    }
                }
                for (k, h) in frame.highlights.iter().enumerate() {
                    let Some((m, part)) = part_of(&self.meshes, h) else { continue };
                    if let Some(s) = &m.silhouettes {
                        let at = highlight_start + 2 * k as u32 + 1;
                        pass.set_vertex_buffer(0, s.slice(..));
                        pass.draw(part.silhouettes.clone(), at..at + 1);
                    }
                }
            }
        }
        // Ghosts' faces, over everything opaque.
        if let Some(inst) = &self.instances.buffer
            && frame.batches.iter().any(|b| b.ghost)
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
        }
        if let Some(lb) = &self.line_vertices.buffer {
            let total = lv.len() as u32;
            pass.set_vertex_buffer(0, lb.slice(..));
            if tested > 0 {
                pass.set_pipeline(&self.lines);
                pass.draw(0..tested, 0..1);
            }
            if total > tested {
                pass.set_pipeline(&self.overlay);
                pass.draw(tested..total, 0..1);
            }
        }
        drop(pass);
        self.drawn = Some(print);
        true
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

fn part_of<'m>(meshes: &'m std::collections::HashMap<u64, GpuMesh>, h: &Highlight) -> Option<(&'m GpuMesh, &'m PartRange)> {
    let m = meshes.get(&h.key)?;
    Some((m, m.parts.iter().find(|p| p.id == h.part)?))
}

/// Everything that makes the picture of `frame` (§7 quater): equal fingerprints, equal
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
    h.fs(&[s.cap_shade, s.ghost_opacity]);
    s.palette.len().hash(&mut h.0);
    for c in s.palette {
        h.fs(c);
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
        (b.key, b.placements.len(), b.ghost).hash(&mut h.0);
        for p in &b.placements {
            h.m(&p.transform);
            h.fs(&p.tint);
            p.material.hash(&mut h.0);
        }
    }
    frame.highlights.len().hash(&mut h.0);
    for hl in frame.highlights {
        (hl.key, hl.part, hl.material).hash(&mut h.0);
        h.m(&hl.transform);
        h.fs(&hl.face_tint);
        h.fs(&hl.edge_tint);
    }
    for lines in [frame.lines, frame.overlay] {
        lines.len().hash(&mut h.0);
        for l in lines {
            h.v(l.a);
            h.v(l.b);
            h.fs(&l.color);
            match l.dash {
                Some(d) => h.fs(&[1.0, d.length, d.phase]),
                None => 0u8.hash(&mut h.0),
            }
        }
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
/// drawn anyway): where a curved surface can turn away from the eye (§7 quater). Vertices are
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

/// A world line, in camera coordinates (subtracted in `f64`), `half` pixels on each side.
/// `ppp`: pixels per point (dashes are in points).
fn push_line(out: &mut Vec<LineVertex>, view: &DMat4, l: &Line, half: f32, ppp: f32) {
    let a = view.transform_point3(l.a).as_vec3().to_array();
    let b = view.transform_point3(l.b).as_vec3().to_array();
    let dash = l.dash.map_or([0.0, 0.0], |d| [2.0 * d.length * ppp, d.phase * ppp]);
    out.extend(QUAD.map(|[end, side]| LineVertex { a, b, corner: [end, side, half], color: l.color, dash }));
}

/// SketchUp's axis colours (linear), the positive halves'; the negative ones are drawn at
/// [`AXIS_NEGATIVE_ALPHA`].
pub const AXIS_X: [f32; 4] = [0.85, 0.05, 0.05, 1.0];
pub const AXIS_Y: [f32; 4] = [0.05, 0.6, 0.05, 1.0];
pub const AXIS_Z: [f32; 4] = [0.05, 0.1, 0.85, 1.0];
pub const AXIS_NEGATIVE_ALPHA: f32 = 0.35;

fn push_scene_lines(out: &mut Vec<LineVertex>, frame: &Frame<'_>) {
    // The grid in the context's frame (composed in f64, so far frames stay exact).
    let v = &(frame.view * frame.axes_frame);
    let ppp = frame.pixels_per_point;
    let (grid_half, line_half) = (GRID_WIDTH * ppp / 2.0, LINE_WIDTH * ppp / 2.0);
    if let Some(g) = frame.grid {
        let n = g.count as i32;
        let ext = g.spacing * g.count as f64;
        for i in -n..=n {
            if i == 0 && frame.axes {
                continue;
            }
            let t = i as f64 * g.spacing;
            let alpha = if i % 10 == 0 { 0.30 } else { 0.12 };
            let c = [0.3, 0.3, 0.3, alpha];
            push_line(out, v, &Line { a: DVec3::new(-ext, t, 0.0), b: DVec3::new(ext, t, 0.0), color: c, dash: None }, grid_half, ppp);
            push_line(out, v, &Line { a: DVec3::new(t, -ext, 0.0), b: DVec3::new(t, ext, 0.0), color: c, dash: None }, grid_half, ppp);
        }
    }
    for l in frame.lines {
        push_line(out, &frame.view, l, line_half, ppp);
    }
}

#[cfg(test)]
mod tests;

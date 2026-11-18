//! Drawing over the model (docs/ARCHITECTURE.md §7 octies): one [`Drawer`] for everything the
//! tools and the editor draw — lines, fills, boxes, grips, marks, lengths —, after
//! OpenCutList's Kuix 3D motifs (`lib/kuix/entity/3d/motif3d.rb`). Immediate: what is drawn is
//! worked out again at each view, with the camera (what is measured in pixels: arrow heads,
//! corners); the output, a [`Drawing`], is flat, styled lines and polygons — and boxes, which
//! the renderer draws instanced. The app only translates it.

use std::f64::consts::TAU;

use glam::{DAffine3, DVec2, DVec3};
use skraft_model::{GuideShape, Units};

use crate::OrientedBox;
use crate::camera::Camera;
use crate::tools::{PointShape, PreviewLabel, PreviewPoint};
use crate::vcb;

/// Line widths, in points.
pub const NORMAL: f32 = 1.5;
/// A line drawn bolder (the curve a tool refers to, a locked axis).
pub const BOLD: f32 = 3.0;

/// How long an arrow's head is on screen (pixels), and how far its strokes spread, a share of
/// that length on each side.
pub const ARROW_PX: f64 = 10.0;
const ARROW_SPREAD: f64 = 0.45;

/// A length shorter than this (mm) is not written.
const MIN_LENGTH: f64 = 1e-3;
/// Nor one this short on screen (pixels): it would not fit.
const LABEL_PX: f64 = 30.0;

// ---- style ------------------------------------------------------------------------------------

/// A colour: linear RGBA, or one of the theme's, which the app resolves with the palette and
/// mode shown (they change at runtime).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Rgba([f32; 4]),
    Theme(ThemeColor),
}

impl From<[f32; 4]> for Color {
    fn from(c: [f32; 4]) -> Self {
        Color::Rgba(c)
    }
}

/// The theme's colours a drawing may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeColor {
    /// The model's edges.
    Edge,
    /// The guides.
    Guide,
    /// The guides of the other components, faded as their parts are.
    FadedGuide,
    /// What is under the cursor.
    Hover,
    /// What is selected.
    Selected,
    /// The box of the part open or the assembly being edited.
    ContextBox,
}

/// How a line is dashed: Kuix's five stipples (`LINE_STIPPLE_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stipple {
    #[default]
    Solid,
    Dotted,
    ShortDashes,
    /// The guides' dashes (the dashes of every dashed line before the stipples).
    LongDashes,
    DashDot,
}

impl Stipple {
    /// Its pattern in points (dash, gap, dash, gap), none when solid.
    pub fn pattern(self) -> Option<[f32; 4]> {
        let long = crate::guides::DASH_PX as f32;
        match self {
            Stipple::Solid => None,
            Stipple::Dotted => Some([NORMAL, 2.0 * NORMAL, 0.0, 0.0]),
            Stipple::ShortDashes => Some([long / 2.0, long / 2.0, 0.0, 0.0]),
            Stipple::LongDashes => Some([long, long, 0.0, 0.0]),
            Stipple::DashDot => Some([long, long / 2.0, NORMAL, long / 2.0]),
        }
    }
}

/// Where a line or a fill is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Layer {
    /// In the scene: hidden behind faces (Kuix's default).
    #[default]
    Scene,
    /// Over everything (Kuix's `on_top`).
    Over,
}

/// How the part of a line in the scene hidden behind faces is drawn again (Kuix's
/// `show_hidden_lines`): its stipple; its colour and width, the line's when none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hidden {
    pub stipple: Stipple,
    pub color: Option<Color>,
    pub width: Option<f32>,
}

impl Default for Hidden {
    /// Long dashes, as Kuix's.
    fn default() -> Self {
        Hidden { stipple: Stipple::LongDashes, color: None, width: None }
    }
}

/// How a line is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pen {
    pub color: Color,
    /// Points.
    pub width: f32,
    pub stipple: Stipple,
    /// The stipple stretched a little so that a dash starts and another ends the line.
    pub anchored: bool,
    pub layer: Layer,
    /// In the scene, blended over what it does not hide at its alpha, no depth written (the
    /// guides of other components, which take the colour of the faces they lie on); otherwise
    /// its alpha mixes it with the background.
    pub blended: bool,
    /// Its part hidden behind faces drawn again so (in the scene only).
    pub hidden: Option<Hidden>,
}

impl Pen {
    /// A plain line in the scene, [`NORMAL`] wide.
    pub fn new(color: impl Into<Color>) -> Self {
        Pen {
            color: color.into(),
            width: NORMAL,
            stipple: Stipple::Solid,
            anchored: false,
            layer: Layer::Scene,
            blended: false,
            hidden: None,
        }
    }

    pub fn width(self, width: f32) -> Self {
        Pen { width, ..self }
    }

    pub fn bold(self) -> Self {
        self.width(BOLD)
    }

    /// [`BOLD`] when `bold`, else [`NORMAL`].
    pub fn bold_if(self, bold: bool) -> Self {
        self.width(if bold { BOLD } else { NORMAL })
    }

    pub fn stipple(self, stipple: Stipple) -> Self {
        Pen { stipple, ..self }
    }

    /// A guide's: long dashes, held still on it as the view pans (at the phase of each of its
    /// strokes, [`Drawer::guide`]).
    /// Translucent (an alpha below one), blended: over a part it takes its colour.
    pub fn guide(color: impl Into<Color>) -> Self {
        let color = color.into();
        let pen = Pen::new(color).stipple(Stipple::LongDashes);
        match color {
            Color::Rgba(c) if c[3] < 1.0 => pen.blended(),
            _ => pen,
        }
    }

    /// Long dashes, a dash at both ends (a box's edges keep their corners drawn).
    pub fn dashed(self) -> Self {
        Pen { stipple: Stipple::LongDashes, anchored: true, ..self }
    }

    pub fn dotted(self) -> Self {
        self.stipple(Stipple::Dotted)
    }

    pub fn anchored(self) -> Self {
        Pen { anchored: true, ..self }
    }

    pub fn over(self) -> Self {
        Pen { layer: Layer::Over, ..self }
    }

    pub fn blended(self) -> Self {
        Pen { blended: true, ..self }
    }

    /// Its hidden part drawn again in `hidden`.
    pub fn hidden(self, hidden: Hidden) -> Self {
        Pen { hidden: Some(hidden), ..self }
    }

    /// Plain where seen, long dashes where hidden (a move's line: the former x-ray lines).
    pub fn x_ray(self) -> Self {
        self.hidden(Hidden::default())
    }
}

/// How a surface is filled: its colour (the alpha its opacity), its layer, and whether only
/// its front is drawn (the triangles counter-clockwise seen from the eye).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub color: Color,
    pub layer: Layer,
    pub cull_back: bool,
}

impl Brush {
    /// In the scene, both sides.
    pub fn new(color: impl Into<Color>) -> Self {
        Brush { color: color.into(), layer: Layer::Scene, cull_back: false }
    }

    pub fn over(self) -> Self {
        Brush { layer: Layer::Over, ..self }
    }

    pub fn front_only(self) -> Self {
        Brush { cull_back: true, ..self }
    }
}

// ---- output -----------------------------------------------------------------------------------

/// A segment (world), its pen, its stipple at `phase` points at `a` (when not anchored: the
/// guides' dashes hold still on them as the view pans).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub a: DVec3,
    pub b: DVec3,
    pub pen: Pen,
    pub phase: f32,
}

/// Filled triangles (world): a convex polygon (a fan of its `corners`), or, `triangles`, the
/// corners three by three.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    pub corners: Vec<DVec3>,
    pub triangles: bool,
    pub brush: Brush,
}

/// A box's edges, drawn instanced by the renderer: dashed from its corners (the pen's
/// anchoring and hidden style ignored; `Layer::Over`: drawn wholly over the scene).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxEdges {
    pub bounds: OrientedBox,
    pub pen: Pen,
    /// Only its corners: each edge this far (points) from its ends, at most to its middle
    /// (Kuix's `BoxCornersMotif3d`).
    pub corners: Option<f32>,
    /// The edges of its faces turned to the eye drawn again over the scene (Kuix's
    /// `shell_on_top`).
    pub shell_on_top: bool,
}

/// How a box is drawn, besides its pen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxStyle {
    pub corners: Option<f32>,
    pub shell_on_top: bool,
}

impl Default for BoxStyle {
    /// Whole edges, its front over the scene (Kuix's default).
    fn default() -> Self {
        BoxStyle { corners: None, shell_on_top: true }
    }
}

/// Which ends of a line carry an arrow head.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ends {
    #[default]
    None,
    Start,
    End,
    Both,
}

/// What is drawn over the model: a tool's preview, the editor's own drawings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub lines: Vec<Line>,
    pub fills: Vec<Fill>,
    pub boxes: Vec<BoxEdges>,
    /// Points drawn over everything, after the fills (grips).
    pub grips: Vec<PreviewPoint>,
    /// Points marked with a cross (world; the app draws a `+` on the screen), which the
    /// lengths keep clear of.
    pub marks: Vec<DVec3>,
    /// Lengths written over the scene.
    pub lengths: Vec<PreviewLabel>,
}

impl Drawing {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
            && self.fills.is_empty()
            && self.boxes.is_empty()
            && self.grips.is_empty()
            && self.marks.is_empty()
            && self.lengths.is_empty()
    }

    /// `other` drawn after this.
    pub fn append(&mut self, other: Drawing) {
        let Drawing { lines, fills, boxes, grips, marks, lengths } = other;
        self.lines.extend(lines);
        self.fills.extend(fills);
        self.boxes.extend(boxes);
        self.grips.extend(grips);
        self.marks.extend(marks);
        self.lengths.extend(lengths);
    }
}

// ---- the drawer -------------------------------------------------------------------------------

/// Draws into a [`Drawing`], with a current pen and brush, in the axes of a current frame
/// (points are given in them; frames are rigid).
pub struct Drawer<'a> {
    camera: &'a Camera,
    size: DVec2,
    units: &'a Units,
    out: &'a mut Drawing,
    frame: DAffine3,
    pen: Pen,
    brush: Brush,
}

impl<'a> Drawer<'a> {
    /// Drawing for `camera` in a `size` viewport, lengths written in `units`; black pen and
    /// brush, in the world's axes.
    pub fn new(camera: &'a Camera, size: DVec2, units: &'a Units, out: &'a mut Drawing) -> Self {
        let black = [0.0, 0.0, 0.0, 1.0];
        Drawer { camera, size, units, out, frame: DAffine3::IDENTITY, pen: Pen::new(black), brush: Brush::new(black) }
    }

    /// Drawing in the axes `t` places in the world.
    pub fn with_frame(self, t: DAffine3) -> Self {
        Drawer { frame: t, ..self }
    }

    pub fn camera(&self) -> &Camera {
        self.camera
    }

    pub fn size(&self) -> DVec2 {
        self.size
    }

    /// The axes points are given in, to the world.
    pub fn frame(&self) -> DAffine3 {
        self.frame
    }

    /// `f` drawing in the axes `t` places in the current ones (Kuix's transformations).
    pub fn in_frame<R>(&mut self, t: DAffine3, f: impl FnOnce(&mut Self) -> R) -> R {
        let outer = self.frame;
        self.frame = outer * t;
        let r = f(self);
        self.frame = outer;
        r
    }

    /// The lines drawn next drawn so.
    pub fn pen(&mut self, pen: Pen) -> &mut Self {
        self.pen = pen;
        self
    }

    /// The surfaces filled next filled so.
    pub fn brush(&mut self, brush: Brush) -> &mut Self {
        self.brush = brush;
        self
    }

    /// The point `p` (current axes) in the world.
    pub fn world(&self, p: DVec3) -> DVec3 {
        self.frame.transform_point3(p)
    }

    /// The length of `n` pixels at `p` (current axes).
    pub fn px(&self, p: DVec3, n: f64) -> f64 {
        self.camera.world_per_pixel(self.size, self.world(p)) * n
    }

    // ---- lines --------------------------------------------------------------------------------

    pub fn line(&mut self, a: DVec3, b: DVec3) -> &mut Self {
        self.line_phased(a, b, 0.0)
    }

    /// A line whose stipple is at `phase` points at `a`.
    pub fn line_phased(&mut self, a: DVec3, b: DVec3, phase: f32) -> &mut Self {
        let (a, b) = (self.world(a), self.world(b));
        self.out.lines.push(Line { a, b, pen: self.pen, phase });
        self
    }

    /// The guide `shape` (world, whatever the current axes) as the view shows it: an infinite
    /// line across the view, a circle as chords (`crate::guides::strokes`), each stroke's
    /// stipple at its phase.
    pub fn guide(&mut self, shape: &GuideShape) -> &mut Self {
        let pen = self.pen;
        let strokes = crate::guides::strokes(self.camera, self.size, shape);
        self.out.lines.extend(strokes.into_iter().map(|s| Line { a: s.a, b: s.b, pen, phase: s.phase as f32 }));
        self
    }

    /// The lines `a`–`b`.
    pub fn segments(&mut self, pairs: impl IntoIterator<Item = (DVec3, DVec3)>) -> &mut Self {
        for (a, b) in pairs {
            self.line(a, b);
        }
        self
    }

    /// The lines through `points`, in order.
    pub fn polyline(&mut self, points: &[DVec3]) -> &mut Self {
        for w in points.windows(2) {
            self.line(w[0], w[1]);
        }
        self
    }

    /// The lines around `points`, closed.
    pub fn polygon(&mut self, points: &[DVec3]) -> &mut Self {
        self.polyline(points);
        if let (Some(&last), Some(&first)) = (points.last(), points.first())
            && points.len() > 2
        {
            self.line(last, first);
        }
        self
    }

    /// A sampled curve through `points` (closed back to the first when `closed`): its stipple
    /// runs on from chord to chord (each at the phase the previous left it), so that short
    /// chords are dashed as one line.
    pub fn curve(&mut self, points: &[DVec3], closed: bool) -> &mut Self {
        let n = points.len();
        if n < 2 {
            return self;
        }
        let count = if closed && n > 2 { n } else { n - 1 };
        let Some(pattern) = self.pen.stipple.pattern() else {
            for i in 0..count {
                self.line(points[i], points[(i + 1) % n]);
            }
            return self;
        };
        let period: f32 = pattern.iter().sum();
        let pen = self.pen;
        self.pen = Pen { anchored: false, ..pen };
        let mut phase = 0.0f32;
        for i in 0..count {
            let (a, b) = (points[i], points[(i + 1) % n]);
            self.line_phased(a, b, phase);
            let on_screen = match (self.camera.project(self.world(a), self.size), self.camera.project(self.world(b), self.size)) {
                (Some(pa), Some(pb)) => pa.distance(pb) as f32,
                _ => 0.0,
            };
            phase = (phase + on_screen) % period;
        }
        self.pen = pen;
        self
    }

    /// A line with arrow heads at `ends` (Kuix's `EdgeMotif3d`): two strokes [`ARROW_PX`]
    /// long on screen, in the plane facing the eye through the tip, plain whatever the
    /// stipple; none when the line is too short on screen for one.
    pub fn edge(&mut self, a: DVec3, b: DVec3, ends: Ends) -> &mut Self {
        self.line(a, b).arrow_heads(a, b, ends)
    }

    /// Only the arrow heads of [`Self::edge`] (the line drawn otherwise: seen through faces),
    /// with a pen as wide as the line's: a head is as wide as the segment it ends.
    pub fn arrow_heads(&mut self, a: DVec3, b: DVec3, ends: Ends) -> &mut Self {
        let (wa, wb) = (self.world(a), self.world(b));
        let head = Pen { stipple: Stipple::Solid, ..self.pen };
        let mut heads = Vec::new();
        if matches!(ends, Ends::End | Ends::Both) {
            heads.extend(self.arrow_head(wa, wb));
        }
        if matches!(ends, Ends::Start | Ends::Both) {
            heads.extend(self.arrow_head(wb, wa));
        }
        self.out.lines.extend(heads.into_iter().map(|(p, tip)| Line { a: p, b: tip, pen: head, phase: 0.0 }));
        self
    }

    /// The strokes of the head of an arrow from `a` to `b` (world), at `b`.
    fn arrow_head(&self, a: DVec3, b: DVec3) -> Vec<(DVec3, DVec3)> {
        let (Some(pa), Some(pb)) = (self.camera.project(a, self.size), self.camera.project(b, self.size)) else { return Vec::new() };
        let d = pb - pa;
        if d.length() < 2.0 * ARROW_PX {
            return Vec::new();
        }
        let u = d.normalize();
        let n = DVec2::new(-u.y, u.x);
        [n, -n]
            .into_iter()
            .filter_map(|side| {
                self.camera.ray(pb - u * ARROW_PX + side * ARROW_PX * ARROW_SPREAD, self.size).hit_plane(b, self.camera.forward())
            })
            .map(|p| (p, b))
            .collect()
    }

    /// The parallelogram from `origin` along `u` and `v` (Kuix's `RectangleMotif3d`).
    pub fn rectangle(&mut self, origin: DVec3, u: DVec3, v: DVec3) -> &mut Self {
        self.polygon(&[origin, origin + u, origin + u + v, origin + v])
    }

    /// The circle about `center` in the plane of `normal`, as `segments` chords (Kuix's
    /// `CircleMotif3d`: 24).
    pub fn circle(&mut self, center: DVec3, normal: DVec3, radius: f64, segments: usize) -> &mut Self {
        let pts = circle_points(center, normal, radius, segments);
        self.curve(&pts, true)
    }

    /// The arc about `center` from `center + from` turning `angle` (radians, counter-clockwise
    /// about `normal`), as chords turning at most `step` radians each.
    pub fn arc(&mut self, center: DVec3, normal: DVec3, from: DVec3, angle: f64, step: f64) -> &mut Self {
        let pts = arc_points(center, normal, from, angle, step);
        self.curve(&pts, false)
    }

    /// The box `lo`..`hi` (current axes), Kuix's way: its front over the scene.
    pub fn box_(&mut self, lo: DVec3, hi: DVec3) -> &mut Self {
        self.box_styled(lo, hi, BoxStyle::default())
    }

    /// The box `lo`..`hi` (current axes), as `style` says; as twelve lines when the pen says
    /// what the renderer's boxes do not do (blended, a hidden part, a stipple's phase
    /// anchored mid-edge is theirs anyway).
    pub fn box_styled(&mut self, lo: DVec3, hi: DVec3, style: BoxStyle) -> &mut Self {
        if self.pen.blended || self.pen.hidden.is_some() {
            let c: [DVec3; 8] = std::array::from_fn(|i| corner(lo, hi, i));
            for (i, j) in BOX_EDGES {
                self.line(c[i], c[j]);
            }
            return self;
        }
        self.out.boxes.push(BoxEdges {
            bounds: OrientedBox { frame: self.frame, lo, hi },
            pen: self.pen,
            corners: style.corners,
            shell_on_top: style.shell_on_top,
        });
        self
    }

    /// The box `b` (world, whatever the current axes), as `style` says: the selection's, by the
    /// ten thousand.
    pub fn oriented_box(&mut self, b: &OrientedBox, style: BoxStyle) -> &mut Self {
        self.out.boxes.push(BoxEdges { bounds: *b, pen: self.pen, corners: style.corners, shell_on_top: style.shell_on_top });
        self
    }

    /// The boxes `boxes` (world, whatever the current axes), each placed by `place` first, as
    /// `style` says: room made for them at once (a selection's, by the ten thousand).
    pub fn oriented_boxes<'b>(
        &mut self,
        boxes: impl ExactSizeIterator<Item = &'b OrientedBox>,
        place: DAffine3,
        style: BoxStyle,
    ) -> &mut Self {
        let pen = self.pen;
        self.out.boxes.reserve(boxes.len());
        let moved = place != DAffine3::IDENTITY;
        self.out.boxes.extend(boxes.map(|b| BoxEdges {
            bounds: if moved { OrientedBox { frame: place * b.frame, ..*b } } else { *b },
            pen,
            corners: style.corners,
            shell_on_top: style.shell_on_top,
        }));
        self
    }

    /// Kuix's `ArrowMotif3d`: an arrow in the parallelogram from `origin` along `u` (its way)
    /// and `v`.
    pub fn arrow(&mut self, origin: DVec3, u: DVec3, v: DVec3) -> &mut Self {
        let pts = ARROW.map(|[x, y]| origin + u * x + v * y);
        self.polyline(&pts)
    }

    /// Kuix's `ChevronMotif3d`: a chevron in the parallelogram from `origin` along `u` (its
    /// way) and `v`.
    pub fn chevron(&mut self, origin: DVec3, u: DVec3, v: DVec3) -> &mut Self {
        let o = 0.2;
        let pts = [[1.0 / 1.8, o], [1.0 - o, 0.5], [1.0 / 1.8, 1.0 - o]].map(|[x, y]| origin + u * x + v * y);
        self.polyline(&pts)
    }

    // ---- fills --------------------------------------------------------------------------------

    /// The convex polygon `corners`.
    pub fn fill_polygon(&mut self, corners: &[DVec3]) -> &mut Self {
        let corners = corners.iter().map(|p| self.world(*p)).collect();
        self.out.fills.push(Fill { corners, triangles: false, brush: self.brush });
        self
    }

    /// Triangles, the corners three by three (Kuix's `Mesh`): counter-clockwise seen from
    /// their front.
    pub fn fill_triangles(&mut self, corners: &[DVec3]) -> &mut Self {
        let mut corners: Vec<DVec3> = corners.iter().map(|p| self.world(*p)).collect();
        corners.truncate(corners.len() / 3 * 3);
        if self.frame.matrix3.determinant() < 0.0 {
            // A mirroring frame turns them over: their front stays their front.
            corners.chunks_exact_mut(3).for_each(|t| t.swap(1, 2));
        }
        self.out.fills.push(Fill { corners, triangles: true, brush: self.brush });
        self
    }

    /// The parallelogram from `origin` along `u` and `v` (Kuix's `RectangleFillMotif3d`).
    pub fn fill_rectangle(&mut self, origin: DVec3, u: DVec3, v: DVec3) -> &mut Self {
        self.fill_polygon(&[origin, origin + u, origin + u + v, origin + v])
    }

    /// The disc about `center` in the plane of `normal` (Kuix's `CircleFillMotif3d`).
    pub fn fill_disc(&mut self, center: DVec3, normal: DVec3, radius: f64, segments: usize) -> &mut Self {
        let pts = circle_points(center, normal, radius, segments);
        self.fill_polygon(&pts)
    }

    /// The faces of the box `lo`..`hi` turned to the eye (Kuix's `BoxFillMotif3d`): its back
    /// faces left out whatever the brush.
    pub fn fill_box(&mut self, lo: DVec3, hi: DVec3) -> &mut Self {
        let c: [DVec3; 8] = std::array::from_fn(|i| corner(lo, hi, i));
        let tris: Vec<DVec3> = BOX_FACES
            .iter()
            .flat_map(|f| {
                let [a, b, cc, d] = f.map(|i| c[i]);
                [a, b, cc, a, cc, d]
            })
            .collect();
        let brush = self.brush;
        self.brush = Brush { cull_back: true, ..brush };
        self.fill_triangles(&tris);
        self.brush = brush;
        self
    }

    /// Kuix's `ArrowFillMotif3d`: an arrow filled in the parallelogram from `origin` along `u`
    /// (its way) and `v`.
    pub fn fill_arrow(&mut self, origin: DVec3, u: DVec3, v: DVec3) -> &mut Self {
        let o = 0.1;
        let at = |[x, y]: [f64; 2]| origin + u * x + v * y;
        let shaft = [[o, 1.0 / 3.0], [1.0 / 1.8, 1.0 / 3.0], [1.0 / 1.8, 2.0 / 3.0], [o, 2.0 / 3.0]].map(at);
        let head = [[1.0 / 1.8, o], [1.0 - o, 0.5], [1.0 / 1.8, 1.0 - o]].map(at);
        self.fill_polygon(&shaft).fill_polygon(&head)
    }

    /// Kuix's `AxesHelper`: at `origin`, a small cube and three bars along the axes,
    /// `length_px` long and `section_px` thick on screen, filled with `colors` (the cube's,
    /// then X, Y, Z).
    pub fn axes(&mut self, origin: DVec3, length_px: f64, section_px: f64, colors: [Color; 4]) -> &mut Self {
        let (l, s) = (self.px(origin, length_px), self.px(origin, section_px));
        let h = s / 2.0;
        let boxes = [
            (DVec3::splat(-h), DVec3::splat(h)),
            (DVec3::new(h, -h, -h), DVec3::new(h + l, h, h)),
            (DVec3::new(-h, h, -h), DVec3::new(h, h + l, h)),
            (DVec3::new(-h, -h, h), DVec3::new(h, h, h + l)),
        ];
        let brush = self.brush;
        for ((lo, hi), color) in boxes.into_iter().zip(colors) {
            self.brush = Brush { color, ..brush };
            self.fill_box(origin + lo, origin + hi);
        }
        self.brush = brush;
        self
    }

    // ---- points and lengths -------------------------------------------------------------------

    /// A grip at `p`, `radius` pixels, filled and / or ringed (a [`PointShape::Bar`]'s
    /// direction in the current axes).
    pub fn grip(&mut self, p: DVec3, radius: f32, fill: Option<[f32; 4]>, stroke: Option<[f32; 4]>, shape: PointShape) -> &mut Self {
        let shape = match shape {
            PointShape::Bar { along } => PointShape::Bar { along: self.frame.transform_vector3(along) },
            s => s,
        };
        let at = self.world(p);
        self.out.grips.push(PreviewPoint { at, radius, fill, stroke, shape });
        self
    }

    /// A cross at `p`, which the lengths keep clear of.
    pub fn mark(&mut self, p: DVec3) -> &mut Self {
        let p = self.world(p);
        self.out.marks.push(p);
        self
    }

    /// The length of `a`–`b` written along it, in `color`, measuring the value `slot` of the
    /// measurements box if any; too short (on screen), written only while that value is typed.
    pub fn length(&mut self, a: DVec3, b: DVec3, color: [f32; 4], slot: Option<usize>) -> &mut Self {
        let len = (b - a).length();
        let hidden = len < MIN_LENGTH || len <= self.px((a + b) * 0.5, LABEL_PX);
        if !hidden || slot.is_some() {
            let text = vcb::format(len, self.units);
            let (a, b) = (self.world(a), self.world(b));
            self.out.lengths.push(PreviewLabel { a, b, text, color, slot, hidden, ..Default::default() });
        }
        self
    }

    /// The same, written however short it is on screen (hidden only when next to nothing).
    pub fn length_always(&mut self, a: DVec3, b: DVec3, color: [f32; 4], slot: Option<usize>) -> &mut Self {
        let len = (b - a).length();
        let text = vcb::format(len, self.units);
        self.label(a, b, text, color, slot, len < MIN_LENGTH)
    }

    /// `text` written along `a`–`b` (an angle), measuring the value `slot` of the measurements
    /// box if any; `hidden`: written only while that value is typed.
    pub fn label(&mut self, a: DVec3, b: DVec3, text: String, color: [f32; 4], slot: Option<usize>, hidden: bool) -> &mut Self {
        let (a, b) = (self.world(a), self.world(b));
        self.out.lengths.push(PreviewLabel { a, b, text, color, slot, hidden, ..Default::default() });
        self
    }
}

// ---- shapes -----------------------------------------------------------------------------------

/// Corner `i` of the box `lo`..`hi`: bit 0 x, bit 1 y, bit 2 z at `hi`.
fn corner(lo: DVec3, hi: DVec3, i: usize) -> DVec3 {
    DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z })
}

/// A box's edges, by their corners.
const BOX_EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];

/// A box's faces, by their corners, counter-clockwise seen from outside.
const BOX_FACES: [[usize; 4]; 6] = [
    [0, 2, 3, 1], // -z
    [4, 5, 7, 6], // +z
    [0, 4, 6, 2], // -x
    [1, 3, 7, 5], // +x
    [0, 1, 5, 4], // -y
    [2, 6, 7, 3], // +y
];

/// Kuix's arrow, in its unit square.
const ARROW: [[f64; 2]; 8] = [
    [0.1, 1.0 / 3.0],
    [1.0 / 1.8, 1.0 / 3.0],
    [1.0 / 1.8, 0.1],
    [0.9, 0.5],
    [1.0 / 1.8, 0.9],
    [1.0 / 1.8, 2.0 / 3.0],
    [0.1, 2.0 / 3.0],
    [0.1, 1.0 / 3.0],
];

/// Two unit directions across `normal`.
fn across(normal: DVec3) -> (DVec3, DVec3) {
    let n = normal.normalize_or(DVec3::Z);
    let u = n.any_orthonormal_vector();
    (u, n.cross(u))
}

fn circle_points(center: DVec3, normal: DVec3, radius: f64, segments: usize) -> Vec<DVec3> {
    let (u, v) = across(normal);
    let n = segments.max(3);
    (0..n)
        .map(|i| {
            let t = TAU * i as f64 / n as f64;
            center + (u * t.cos() + v * t.sin()) * radius
        })
        .collect()
}

fn arc_points(center: DVec3, normal: DVec3, from: DVec3, angle: f64, step: f64) -> Vec<DVec3> {
    let n = normal.normalize_or(DVec3::Z);
    let v = n.cross(from);
    let count = ((angle.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 10_000);
    (0..=count)
        .map(|i| {
            let t = angle * i as f64 / count as f64;
            center + from * t.cos() + v * t.sin()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;

    fn drawn(f: impl FnOnce(&mut Drawer<'_>)) -> Drawing {
        let camera = Camera::default();
        let units = Units::default();
        let mut out = Drawing::default();
        f(&mut Drawer::new(&camera, DVec2::new(800.0, 600.0), &units, &mut out));
        out
    }

    #[test]
    fn points_are_drawn_in_the_current_frame() {
        let t = DAffine3::from_translation(DVec3::new(10.0, 0.0, 0.0));
        let d = drawn(|d| {
            d.in_frame(t, |d| {
                d.pen(Pen::new([1.0, 0.0, 0.0, 1.0]).bold()).line(DVec3::ZERO, DVec3::Y);
            });
            d.line(DVec3::ZERO, DVec3::X);
        });
        assert_eq!(d.lines[0].a, DVec3::new(10.0, 0.0, 0.0));
        assert_eq!(d.lines[0].pen.width, BOLD);
        // Out of the frame: the world again, the pen kept.
        assert_eq!(d.lines[1].a, DVec3::ZERO);
        assert_eq!(d.lines[1].pen.width, BOLD);
    }

    #[test]
    fn boxes_go_out_as_boxes_but_when_the_renderer_cannot_draw_them() {
        let d = drawn(|d| {
            d.pen(Pen::new([0.0; 4]).dashed()).box_(DVec3::ZERO, DVec3::ONE);
            d.pen(Pen::new([0.0; 4]).x_ray()).box_(DVec3::ZERO, DVec3::ONE);
        });
        assert_eq!(d.boxes.len(), 1);
        assert!(d.boxes[0].shell_on_top);
        assert_eq!(d.lines.len(), 12);
    }

    #[test]
    fn a_filled_box_shows_its_front_only_and_stays_outward_when_mirrored() {
        let mirror = DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0));
        let d = drawn(|d| {
            d.fill_box(DVec3::ZERO, DVec3::ONE);
            d.in_frame(mirror, |d| {
                d.fill_box(DVec3::ZERO, DVec3::ONE);
            });
        });
        for (f, centre) in d.fills.iter().zip([DVec3::splat(0.5), DVec3::new(-0.5, 0.5, 0.5)]) {
            assert!(f.triangles && f.brush.cull_back);
            assert_eq!(f.corners.len(), 36);
            for t in f.corners.chunks_exact(3) {
                let n = (t[1] - t[0]).cross(t[2] - t[0]);
                assert!(n.dot(t[0] - centre) > 0.0, "a triangle turned inwards");
            }
        }
    }

    #[test]
    fn arrows_need_room_on_screen() {
        let d = drawn(|d| {
            d.edge(DVec3::ZERO, DVec3::new(2000.0, 0.0, 0.0), Ends::Both);
            d.edge(DVec3::ZERO, DVec3::new(1e-3, 0.0, 0.0), Ends::Both);
        });
        // The first line, two heads of two strokes; the second line alone.
        assert_eq!(d.lines.len(), 1 + 4 + 1);
        // A head as wide as the segment it ends.
        let bold = drawn(|d| {
            d.pen(Pen::new([0.0; 4]).bold().dashed()).edge(DVec3::ZERO, DVec3::new(2000.0, 0.0, 0.0), Ends::End);
        });
        assert_eq!(bold.lines.len(), 1 + 2);
        assert!(bold.lines.iter().all(|l| l.pen.width == BOLD));
        assert!(bold.lines[1..].iter().all(|l| l.pen.stipple == Stipple::Solid));
    }

    #[test]
    fn short_lengths_are_written_only_when_typed() {
        let d = drawn(|d| {
            d.length(DVec3::ZERO, DVec3::new(1e-4, 0.0, 0.0), [0.0; 4], None);
            d.length(DVec3::ZERO, DVec3::new(1e-4, 0.0, 0.0), [0.0; 4], Some(0));
            d.length(DVec3::ZERO, DVec3::new(500.0, 0.0, 0.0), [0.0; 4], None);
        });
        assert_eq!(d.lengths.len(), 2);
        assert!(d.lengths[0].hidden && !d.lengths[1].hidden);
    }

    #[test]
    fn a_curve_runs_its_dashes_on_from_chord_to_chord() {
        let pts: Vec<DVec3> = (0..=20).map(|i| DVec3::new(i as f64 * 5.0, 0.0, 0.0)).collect();
        let d = drawn(|d| {
            d.pen(Pen::new([0.0; 4]).dashed()).curve(&pts, false);
        });
        assert_eq!(d.lines.len(), 20);
        assert!(d.lines.iter().all(|l| !l.pen.anchored));
        assert_eq!(d.lines[0].phase, 0.0);
        assert!(d.lines[1].phase > 0.0);
    }

    #[test]
    fn stipples_have_their_patterns() {
        assert_eq!(Stipple::Solid.pattern(), None);
        let long = Stipple::LongDashes.pattern().unwrap();
        assert_eq!(long[0], crate::guides::DASH_PX as f32);
        assert!(Stipple::DashDot.pattern().unwrap().iter().all(|l| *l > 0.0));
    }
}

//! Box and Cylinder: click the base, then the height (SketchUp's rectangle then push/pull, in
//! one tool). Values can be typed at each step: `600;400` or `600=` for the base, `@*2` for
//! twice the value so far; a typed length keeps the direction of the gesture. The height can
//! come with the base (`600;400;19`, `600==` a cube; `r;h` for a cylinder): the shape is done.
//!
//! The base lies in the plane of the face under the first click, through the point inferred
//! there (§6 ter), and the height runs along its normal; with no face, on the ground of the
//! axes (the open part's, ARCHITECTURE.md §2.9, else the context's).
//! The arrow keys choose the plane instead: ↑ horizontal, → across the red axis, ← across the
//! green one. A shape aligned with the axes is built as is; any other is built in its own
//! frame and placed.

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::Primitive;

use super::{Command, Cx, INK, Preview, PreviewLine, Tool, ToolInput, ToolKind, cross};
use crate::infer::{InferKind, Inference};
use crate::vcb::{Vcb, VcbKind, VcbValue};

/// The smallest dimension built (mm).
const MIN: f64 = 1e-3;

/// A drawing frame, in the axes' coordinates: origin `o`, in-plane axes `u` and `v`, normal `n`
/// (right-handed, orthonormal).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    o: DVec3,
    u: DVec3,
    v: DVec3,
    n: DVec3,
}

impl Frame {
    /// The frame of the plane through `o` with normal `n`: `u` horizontal (the red axis on a
    /// horizontal plane), `v` = `n` × `u` (up on a wall). A normal within a hair of an axis is
    /// taken as that axis, so a frame on a face of an aligned box is exactly aligned.
    fn new(o: DVec3, n: DVec3) -> Frame {
        let mut n = n.normalize();
        for a in [DVec3::X, DVec3::Y, DVec3::Z] {
            let d = n.dot(a);
            if d.abs() > 1.0 - 1e-9 {
                n = a * d.signum();
            }
        }
        let u = if n.z.abs() > 0.999 { DVec3::X } else { DVec3::Z.cross(n).normalize() };
        Frame { o, u, v: n.cross(u), n }
    }

    /// `p` in the frame's coordinates.
    fn local(&self, p: DVec3) -> DVec3 {
        let d = p - self.o;
        DVec3::new(d.dot(self.u), d.dot(self.v), d.dot(self.n))
    }

    /// The frame's point `l`, in the axes' coordinates.
    fn at(&self, l: DVec3) -> DVec3 {
        self.o + self.u * l.x + self.v * l.y + self.n * l.z
    }

    /// Along the axes themselves?
    fn aligned(&self) -> bool {
        self.u == DVec3::X && self.v == DVec3::Y && self.n == DVec3::Z
    }

    fn placement(&self) -> DAffine3 {
        DAffine3::from_cols(self.u, self.v, self.n, self.o)
    }

    /// `p` brought along the normal onto the plane.
    fn flatten(&self, p: DVec3) -> DVec3 {
        p - self.n * (p - self.o).dot(self.n)
    }
}

/// The frame the first click under `px` gives (§ module doc), and the inference.
fn frame_at(cx: &Cx<'_>, px: DVec2) -> Option<(Frame, Inference)> {
    let inf = cx.infer(px, |_| true, None)?;
    let n = match (cx.axis_lock, &inf.hit) {
        (Some(k), _) => DVec3::AXES[k],
        // A normal goes to the axes by the transpose of axes → world.
        (None, Some(h)) => cx.axes().matrix3.transpose().mul_vec3(h.normal),
        (None, None) => DVec3::Z,
    };
    Some((Frame::new(cx.to_axes(inf.point), n), inf))
}

/// The cursor's point on the frame's plane (the axes' coordinates): a feature inferred anywhere
/// (endpoint, midpoint, edge…) is brought along the normal onto the plane; otherwise the
/// cursor's ray meets the plane.
fn on_plane(cx: &Cx<'_>, px: DVec2, f: &Frame) -> Option<(DVec3, Option<Inference>)> {
    let n_world = cx.axes().matrix3.inverse().transpose().mul_vec3(f.n).normalize();
    if let Some(inf) = cx.infer(px, |_| true, Some((cx.to_world(f.o), n_world))).filter(|i| i.kind.is_feature()) {
        return Some((f.flatten(cx.to_axes(inf.point)), Some(inf)));
    }
    cx.axes_ray(px).hit_plane(f.o, f.n).map(|p| (p, None))
}

/// The height the cursor gives above the plane at `p` (the axes' coordinates): that of a feature
/// inferred, else that of the closest point of the normal through `p` to the cursor's ray.
fn height_at(cx: &Cx<'_>, px: DVec2, f: &Frame, p: DVec3) -> (f64, Option<Inference>) {
    if let Some(inf) = cx.infer(px, |_| true, None).filter(|i| i.kind.is_feature()) {
        return (f.local(cx.to_axes(inf.point)).z, Some(inf));
    }
    (f.local(cx.axes_ray(px).closest_on_line(p, f.n)).z, None)
}

fn line(cx: &Cx<'_>, a: DVec3, b: DVec3) -> PreviewLine {
    PreviewLine { a: cx.to_world(a), b: cx.to_world(b), color: INK }
}

/// Whether the cursor's point is an inferred feature, worth a cross: not on the empty ground
/// (the fallback plane), nor without any inference.
fn snapped(snap: &Option<Inference>) -> bool {
    snap.as_ref().is_some_and(|i| i.kind != InferKind::OnPlane)
}

// ---- Box ---------------------------------------------------------------------------------------

#[derive(Default)]
pub struct BoxTool {
    state: BoxState,
    cursor: Option<DVec3>,
    snap: Option<Inference>,
}

#[derive(Default, Clone, Copy)]
enum BoxState {
    #[default]
    First,
    /// The first corner (`f.o`) is set; the opposite one, `p2`, on the plane, follows the cursor.
    Base { f: Frame, p2: DVec3 },
    /// The base is set; the height follows the cursor.
    Height { f: Frame, p2: DVec3, h: f64 },
}

impl BoxTool {
    fn finish(&mut self, cx: &Cx<'_>, f: Frame, p2: DVec3, h: f64) -> Vec<Command> {
        self.state = BoxState::First;
        let d = f.local(p2);
        if d.x.abs() < MIN || d.y.abs() < MIN || h.abs() < MIN {
            return vec![Command::Say(t!("tool.box.say.empty"))];
        }
        let size = DVec3::new(d.x.abs(), d.y.abs(), h.abs());
        let (primitive, placement) = if f.aligned() {
            // In the axes: corners as clicked, no rounding through a frame.
            let lo = f.o.min(p2);
            (Primitive::Box { origin: DVec3::new(lo.x, lo.y, f.o.z + h.min(0.0)), size }, DAffine3::IDENTITY)
        } else {
            let lo = DVec3::new(d.x.min(0.0), d.y.min(0.0), h.min(0.0));
            (Primitive::Box { origin: lo, size }, f.placement())
        };
        vec![Command::Apply(cx.add_primitive(primitive, placement))]
    }
}

impl Tool for BoxTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Box
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match (input, self.state) {
            (ToolInput::Hover { px }, BoxState::First) => {
                (self.cursor, self.snap) = frame_at(cx, *px).map_or((None, None), |(f, i)| (Some(f.o), Some(i)));
            }
            (ToolInput::Hover { px }, BoxState::Base { f, .. }) => {
                if let Some((p2, snap)) = on_plane(cx, *px, &f) {
                    self.state = BoxState::Base { f, p2 };
                    self.snap = snap;
                }
            }
            (ToolInput::Hover { px }, BoxState::Height { f, p2, .. }) => {
                let (h, snap) = height_at(cx, *px, &f, p2);
                self.state = BoxState::Height { f, p2, h };
                self.snap = snap;
            }
            (ToolInput::Click { px, .. }, BoxState::First) => {
                if let Some((f, _)) = frame_at(cx, *px) {
                    self.state = BoxState::Base { f, p2: f.o };
                }
            }
            (ToolInput::Click { .. }, BoxState::Base { f, p2 }) => {
                let d = f.local(p2);
                if d.x.abs() < MIN || d.y.abs() < MIN {
                    return vec![Command::Say(t!("tool.box.say.empty_base"))];
                }
                self.state = BoxState::Height { f, p2, h: 0.0 };
            }
            (ToolInput::Click { .. }, BoxState::Height { f, p2, h }) => return self.finish(cx, f, p2, h),
            (ToolInput::Value(VcbValue::Lengths(v)), BoxState::Base { f, .. }) => {
                // Signed as the drag (the box's VCB gives the current ones).
                let (w, l) = (v[0], v[1]);
                if w.abs() < MIN || l.abs() < MIN {
                    return vec![Command::Say(t!("tool.box.say.base_lengths"))];
                }
                let p2 = if f.aligned() { f.o + DVec3::new(w, l, 0.0) } else { f.at(DVec3::new(w, l, 0.0)) };
                match v.get(2) {
                    Some(&h) => return self.finish(cx, f, p2, h),
                    None => self.state = BoxState::Height { f, p2, h: 0.0 },
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), BoxState::Height { f, p2, .. }) => {
                return self.finish(cx, f, p2, v[0]);
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let mut lines = Vec::new();
        // The base's corners at height `z`, as edges.
        let rect = |f: &Frame, p2: DVec3, z: f64| {
            let d = f.local(p2);
            let c = [(0.0, 0.0), (d.x, 0.0), (d.x, d.y), (0.0, d.y)].map(|(x, y)| f.at(DVec3::new(x, y, z)));
            (0..4).map(move |i| (c[i], c[(i + 1) % 4]))
        };
        match self.state {
            BoxState::First => {
                if let Some(p) = self.cursor.filter(|_| snapped(&self.snap)) {
                    lines.extend(cross(cx, cx.to_world(p), INK));
                }
            }
            BoxState::Base { f, p2 } => lines.extend(rect(&f, p2, 0.0).map(|(a, b)| line(cx, a, b))),
            BoxState::Height { f, p2, h } => {
                lines.extend(rect(&f, p2, 0.0).map(|(a, b)| line(cx, a, b)));
                lines.extend(rect(&f, p2, h).map(|(a, b)| line(cx, a, b)));
                for (a, _) in rect(&f, p2, 0.0) {
                    lines.push(line(cx, a, a + f.n * h));
                }
            }
        }
        Preview { lines, inference: self.snap.clone(), ..Default::default() }
    }

    fn status(&self) -> String {
        match self.state {
            BoxState::First => t!("tool.box.hint.first"),
            BoxState::Base { .. } => t!("tool.box.hint.base"),
            BoxState::Height { .. } => t!("tool.box.hint.height"),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        match self.state {
            BoxState::First => None,
            BoxState::Base { f, p2 } => {
                let d = f.local(p2);
                Some(Vcb::new(t!("tool.box.vcb.dimensions"), VcbKind::Lengths(vec![d.x, d.y, 0.0])).optional(1))
            }
            BoxState::Height { h, .. } => Some(Vcb::new(t!("tool.box.vcb.height"), VcbKind::Lengths(vec![h]))),
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.state, BoxState::First)
    }

    fn reset(&mut self) {
        self.state = BoxState::First;
        self.snap = None;
    }
}

// ---- Cylinder ----------------------------------------------------------------------------------

#[derive(Default)]
pub struct CylinderTool {
    state: CylState,
    cursor: Option<DVec3>,
    snap: Option<Inference>,
}

#[derive(Default, Clone, Copy)]
enum CylState {
    #[default]
    Center,
    /// The centre (`f.o`) is set; the radius follows the cursor.
    Radius { f: Frame, r: f64 },
    Height { f: Frame, r: f64, h: f64 },
}

impl CylinderTool {
    fn finish(&mut self, cx: &Cx<'_>, f: Frame, r: f64, h: f64) -> Vec<Command> {
        self.state = CylState::Center;
        if r < MIN || h.abs() < MIN {
            return vec![Command::Say(t!("tool.cylinder.say.empty"))];
        }
        let (origin, placement) = if f.aligned() {
            (DVec3::new(f.o.x, f.o.y, f.o.z + h.min(0.0)), DAffine3::IDENTITY)
        } else {
            (DVec3::new(0.0, 0.0, h.min(0.0)), f.placement())
        };
        let primitive = Primitive::Cylinder { origin, radius: r, height: h.abs() };
        vec![Command::Apply(cx.add_primitive(primitive, placement))]
    }
}

impl Tool for CylinderTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Cylinder
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match (input, self.state) {
            (ToolInput::Hover { px }, CylState::Center) => {
                (self.cursor, self.snap) = frame_at(cx, *px).map_or((None, None), |(f, i)| (Some(f.o), Some(i)));
            }
            (ToolInput::Hover { px }, CylState::Radius { f, .. }) => {
                if let Some((p, snap)) = on_plane(cx, *px, &f) {
                    self.state = CylState::Radius { f, r: (p - f.o).length() };
                    self.snap = snap;
                }
            }
            (ToolInput::Hover { px }, CylState::Height { f, r, .. }) => {
                let (h, snap) = height_at(cx, *px, &f, f.at(DVec3::new(r, 0.0, 0.0)));
                self.state = CylState::Height { f, r, h };
                self.snap = snap;
            }
            (ToolInput::Click { px, .. }, CylState::Center) => {
                if let Some((f, _)) = frame_at(cx, *px) {
                    self.state = CylState::Radius { f, r: 0.0 };
                }
            }
            (ToolInput::Click { .. }, CylState::Radius { f, r }) if r >= MIN => {
                self.state = CylState::Height { f, r, h: 0.0 };
            }
            (ToolInput::Click { .. }, CylState::Height { f, r, h }) => return self.finish(cx, f, r, h),
            (ToolInput::Value(VcbValue::Lengths(v)), CylState::Radius { f, .. }) => match (v[0], v.get(1)) {
                (r, _) if r < MIN => return vec![Command::Say(t!("tool.cylinder.say.zero_radius"))],
                (r, Some(&h)) => return self.finish(cx, f, r, h),
                (r, None) => self.state = CylState::Height { f, r, h: 0.0 },
            },
            (ToolInput::Value(VcbValue::Lengths(v)), CylState::Height { f, r, .. }) => return self.finish(cx, f, r, v[0]),
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let circle = |f: Frame, r: f64, z: f64| {
            const N: usize = 48;
            let p = move |i: usize| {
                let a = i as f64 / N as f64 * std::f64::consts::TAU;
                f.at(DVec3::new(a.cos() * r, a.sin() * r, z))
            };
            (0..N).map(move |i| (p(i), p(i + 1)))
        };
        let mut lines = Vec::new();
        match self.state {
            CylState::Center => {
                if let Some(p) = self.cursor.filter(|_| snapped(&self.snap)) {
                    lines.extend(cross(cx, cx.to_world(p), INK));
                }
            }
            CylState::Radius { f, r } => {
                lines.extend(circle(f, r, 0.0).map(|(a, b)| line(cx, a, b)));
                lines.extend(cross(cx, cx.to_world(f.o), INK));
            }
            CylState::Height { f, r, h } => {
                lines.extend(circle(f, r, 0.0).map(|(a, b)| line(cx, a, b)));
                lines.extend(circle(f, r, h).map(|(a, b)| line(cx, a, b)));
                for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
                    let a = f.at(DVec3::new(x * r, y * r, 0.0));
                    lines.push(line(cx, a, a + f.n * h));
                }
            }
        }
        Preview { lines, inference: self.snap.clone(), ..Default::default() }
    }

    fn status(&self) -> String {
        match self.state {
            CylState::Center => t!("tool.cylinder.hint.center"),
            CylState::Radius { .. } => t!("tool.cylinder.hint.radius"),
            CylState::Height { .. } => t!("tool.cylinder.hint.height"),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        match self.state {
            CylState::Center => None,
            CylState::Radius { r, .. } => Some(Vcb::new(t!("tool.cylinder.vcb.radius"), VcbKind::Lengths(vec![r, 0.0])).optional(1)),
            CylState::Height { h, .. } => Some(Vcb::new(t!("tool.cylinder.vcb.height"), VcbKind::Lengths(vec![h]))),
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.state, CylState::Center)
    }

    fn reset(&mut self) {
        self.state = CylState::Center;
        self.snap = None;
    }
}

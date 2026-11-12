//! Box and Cylinder: click the base, then the height (SketchUp's rectangle then push/pull, in
//! one tool). Values can be typed at each step: `600;400` or `600=` for the base, `@*2` for
//! twice the value so far; a typed length keeps the direction of the gesture. The height can
//! come with the base (`600;400;19`, `600==` a cube; `r;h` for a cylinder): the shape is done.
//! Esc goes back one step.
//!
//! The box's opposite corner snaps to the square's near it (not on a feature, nor on a plane
//! seen edge on). The cylinder's radius snaps likewise to the red, green or blue axis through
//! the centre, and is dashed in that axis's colour along it; its "Measure" option (Tab) takes
//! the base's length as the radius or the diameter. The box's "Centred" option (the
//! bar, or a tap of the copy modifier while drawing the base) takes the first click as the
//! centre of the base; typed lengths are then the whole ones.
//!
//! The first point, after OpenCutList: with the copy modifier held over a face, its centroid;
//! Shift tapped then, the whole face is the base (the box: the rectangle around it, along its
//! longest side; the cylinder: a disc's circle) and the height follows. The other modifier
//! tapped (Command on macOS, Alt elsewhere) measures the first point from a corner: along an
//! edge, from its nearer end; across a face, from the two sides at its corner nearest the
//! cursor; the distances are typed (`d` or `d1;d2`).
//!
//! The height takes that of a feature inferred, or the plane of a planar face parallel to the
//! base (not in its plane) the cursor is on. After OpenCutList: Shift held takes the last height again (the cursor's way), as
//! a double-click on the opposite corner does; the copy modifier held runs it up to the first
//! face along the normal from the middle of the base; tapped, it centres the height on the
//! plane (the "Centred height" option, saved), typed heights then being the whole ones.
//!
//! The drawing plane follows what the cursor aims at, after OpenCutList's smart draw tool:
//! - the first click: the plane of the face under it, through the point inferred there
//!   (§6 ter); on an edge, the face it bounds, else the ground, else the upright plane
//!   through it; with nothing, the ground of the axes (the open part's, ARCHITECTURE.md §2.9,
//!   else the context's). A guide line is taken as an edge, a guide circle as its tangent
//!   there (off a face, in its plane). On a straight edge or a guide askew to the axes, the
//!   base's sides run along it;
//! - the box's second corner (a cylinder keeps the plane of its centre) sets the plane again:
//!   a point in the ground, or in the red or green upright plane through the first corner,
//!   takes that plane; an edge, the plane through it and the first corner (its sides along the
//!   edge unless the first click gave them); a guide (a line, a circle's tangent there),
//!   likewise, its sides always along it; a face the first corner lies on, its plane;
//!   another face or a point off the ground, the upright plane through both corners; else the
//!   plane so far is kept.
//!
//! The height runs along the normal, towards the eye. The arrow keys lock the plane instead:
//! ↑ horizontal, → across the red axis, ← across the green one, ↓ the face under the cursor;
//! the same key again frees it. A shape aligned with the axes is built as is; any other is
//! built in its own frame and placed.

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::Primitive;

use super::face::{self, Face};
use super::{
    BLUE, Command, Cx, GREEN, INK, Key, MAGENTA, OptionChoice, OptionValue, Preview, PreviewLabel, PreviewLine, RED, Tool, ToolInput,
    ToolKind, ToolOption, alt_key, copy_key, dashed,
};
use crate::camera::Ray;
use crate::infer::{EdgeRef, InferKind, Inference, LineKind};
use crate::vcb::{self, Vcb, VcbKind, VcbValue};

/// The smallest dimension built (mm).
const MIN: f64 = 1e-3;
/// How far off a plane a point still lies in it (mm).
const ON: f64 = 1e-3;
/// Below this cosine, two unit vectors are perpendicular; above 1 − this, parallel.
const SQUARE: f64 = 1e-9;
/// The size of the shape drawn at the cursor before the first click (pixels).
const GAUGE_PX: f64 = 40.0;
/// The shortest side (pixels) that gets its length written.
const LABEL_PX: f64 = 30.0;

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
    /// The frame of the plane through `o` with normal `n`. Its `u` runs along `dir` when that
    /// lies in the plane askew to the frame's usual sides (an edge aimed at); otherwise `u` is
    /// horizontal (the red axis on a horizontal plane) and `v` = `n` × `u` (up on a wall).
    /// Directions within a hair of an axis are taken as that axis, so a frame on a face of an
    /// aligned box is exactly aligned.
    fn new(o: DVec3, n: DVec3, dir: Option<DVec3>) -> Frame {
        let n = to_axis(n.normalize());
        let mut u = if n.z.abs() > 0.999 { (DVec3::X - n * n.x).normalize() } else { DVec3::Z.cross(n).normalize() };
        if let Some(d) = dir.and_then(DVec3::try_normalize).filter(|d| d.dot(n).abs() < SQUARE) {
            let c = d.dot(u);
            // Along `u` or `v`, the base is the same rectangle: the usual frame is kept.
            if c.abs() > SQUARE && c.abs() < 1.0 - SQUARE {
                u = to_axis(d * c.signum());
            }
        }
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

/// `d` (unit), exactly the axis it is within a hair of, if any.
fn to_axis(d: DVec3) -> DVec3 {
    for a in [DVec3::X, DVec3::Y, DVec3::Z] {
        let c = d.dot(a);
        if c.abs() > 1.0 - SQUARE {
            return a * c.signum();
        }
    }
    d
}

/// `n`, or its opposite, towards the eye (`ray`: the cursor's ray direction).
fn facing(n: DVec3, ray: DVec3) -> DVec3 {
    if n.dot(ray) > 0.0 { -n } else { n }
}

/// A world direction in the axes' coordinates (unit).
fn dir_to_axes(cx: &Cx<'_>, d: DVec3) -> DVec3 {
    cx.axes().matrix3.inverse().mul_vec3(d).normalize()
}

/// A direction of the axes' coordinates in the world (unit).
fn dir_to_world(cx: &Cx<'_>, d: DVec3) -> DVec3 {
    cx.axes().matrix3.mul_vec3(d).normalize()
}

/// The normal the locks give the drawing plane: → red, ← green reversed (so that `u` stays
/// red and `v` up), ↑ blue, ↓ the face's, taken when the key was pressed.
fn locked_normal(cx: &Cx<'_>) -> Option<DVec3> {
    match (cx.axis_lock, cx.face_lock) {
        (Some(0), _) => Some(DVec3::X),
        (Some(1), _) => Some(-DVec3::Y),
        (Some(_), _) => Some(DVec3::Z),
        (None, Some(n)) => Some(dir_to_axes(cx, n)),
        (None, None) => None,
    }
}

/// The direction (axes) of the straight edge `inf` is on, if any: its midpoint or a point along it.
fn edge_dir(cx: &Cx<'_>, inf: &Inference) -> Option<DVec3> {
    match (inf.kind, inf.edge) {
        (InferKind::Midpoint | InferKind::OnEdge, Some(EdgeRef::Straight(d))) => Some(dir_to_axes(cx, d)),
        _ => None,
    }
}

/// The direction (axes) of the guide `inf` is on, if any: a line's, a circle's tangent there.
fn guide_dir(cx: &Cx<'_>, inf: &Inference) -> Option<DVec3> {
    match (inf.kind, inf.line, inf.edge) {
        (InferKind::OnLine(LineKind::Guide), Some((_, d)), _) => Some(dir_to_axes(cx, d)),
        (InferKind::OnGuide, _, Some(EdgeRef::Arc { tangent, .. })) => Some(dir_to_axes(cx, tangent)),
        _ => None,
    }
}

/// The normal (axes) of the guide circle `inf` is on, if any.
fn guide_normal(cx: &Cx<'_>, inf: &Inference) -> Option<DVec3> {
    match (inf.kind, inf.edge) {
        (InferKind::OnGuide, Some(EdgeRef::Arc { tangent, radial, .. })) => {
            radial.cross(tangent).try_normalize().map(|n| dir_to_axes(cx, n))
        }
        _ => None,
    }
}

/// The normal (axes) of the face the cursor's ray hits, and a point of it.
fn hit_face(cx: &Cx<'_>, inf: &Inference) -> Option<(DVec3, DVec3)> {
    inf.hit.as_ref().map(|h| (dir_to_axes(cx, h.normal), cx.to_axes(h.point)))
}

/// The plane through an edge along `d` when no face of it is aimed at: the ground for a level
/// edge, else the upright plane through it (for an upright edge, the one across the red or
/// green axis the eye faces most).
fn edge_plane(d: DVec3, ray: DVec3) -> DVec3 {
    if d.z.abs() < SQUARE {
        return DVec3::Z;
    }
    let across = if ray.x.abs() > ray.y.abs() { DVec3::X } else { DVec3::Y };
    facing(DVec3::Z.cross(d).try_normalize().unwrap_or(across), ray)
}

/// The upright plane through `o` and `q`, unless `q` is right above or below `o`.
fn upright(o: DVec3, q: DVec3, ray: DVec3, dir: Option<DVec3>) -> Option<Frame> {
    let h = DVec3::new(q.x - o.x, q.y - o.y, 0.0);
    if h.length() < ON {
        return None;
    }
    Some(Frame::new(o, facing(DVec3::Z.cross(h).normalize(), ray), dir))
}

/// What the first step finds under the cursor: the frame a click there gives (§ module doc),
/// the direction of the edge it is on, the inference, and the planar face under it.
struct StartAt {
    f: Frame,
    dir: Option<DVec3>,
    inf: Inference,
    face: Option<Face>,
}

/// The first step under `px`. With the copy modifier held over a face, its centroid.
fn start_at(cx: &Cx<'_>, px: DVec2) -> Option<StartAt> {
    let mut inf = cx.infer(px, |_| true, None)?;
    // The face under it, unless the point is on an edge or a corner of it (a centre is not).
    let inside = !matches!(inf.kind, InferKind::Endpoint | InferKind::Midpoint | InferKind::OnEdge | InferKind::OnEdgeAndLine(_));
    let face =
        inf.hit.as_ref().filter(|_| inside).and_then(|h| Face::at(cx.scene, h)).filter(|f| (inf.point - f.point).dot(f.normal).abs() < ON);
    if cx.copy_down
        && let Some(c) = face.as_ref().map(Face::centroid)
    {
        inf = Inference { point: c, kind: InferKind::Center, line: None, line2: None, edge: None, curve: None, crossed: Vec::new(), ..inf };
    }
    let o = cx.to_axes(inf.point);
    // A guide line gives its direction as an edge does.
    let dir = edge_dir(cx, &inf).or_else(|| guide_dir(cx, &inf));
    let ray = cx.axes_ray(px).dir;
    let n = match (locked_normal(cx), dir, hit_face(cx, &inf), guide_normal(cx, &inf)) {
        (Some(n), ..) => n,
        // An edge: the face it bounds, if that is the one aimed at.
        (None, Some(d), Some((n, p)), _) if d.dot(n).abs() < SQUARE && (o - p).dot(n).abs() < ON => n,
        // A guide circle: its plane.
        (None, Some(_), _, Some(n)) => facing(n, ray),
        (None, Some(d), _, None) => edge_plane(d, ray),
        (None, None, Some((n, _)), _) => n,
        (None, None, None, _) => DVec3::Z,
    };
    Some(StartAt { f: Frame::new(o, n, dir), dir, inf, face })
}

/// The first point measured from a corner (OpenCutList's "measure from vertex"), in the axes'
/// coordinates: the corner `v`, the cursor's point `p`, and per side measured from (along an
/// edge, one; across a face, the two sides at its corner nearest the cursor) its unit
/// direction from the corner and the foot of `p` on it.
#[derive(Clone, Debug)]
struct Measure {
    v: DVec3,
    p: DVec3,
    sides: Vec<(DVec3, DVec3)>,
}

impl Measure {
    fn at(cx: &Cx<'_>, s: &StartAt) -> Option<Measure> {
        let p = s.f.o;
        let foot = |v: DVec3, d: DVec3| v + d * (p - v).dot(d);
        if edge_dir(cx, &s.inf).is_some()
            && let Some((a, b)) = face::segment_at(cx.scene, &s.inf)
        {
            let (a, b) = (cx.to_axes(a), cx.to_axes(b));
            let (v, w) = if a.distance(p) <= b.distance(p) { (a, b) } else { (b, a) };
            let d = (w - v).try_normalize()?;
            return Some(Measure { v, p, sides: vec![(d, foot(v, d))] });
        }
        let (v, sides) = s.face.as_ref()?.corner_near(cx.to_world(p))?;
        if sides.len() < 2 {
            return None;
        }
        let v = cx.to_axes(v);
        let sides = sides.iter().take(2).map(|d| dir_to_axes(cx, *d)).map(|d| (d, foot(v, d))).collect();
        Some(Measure { v, p, sides })
    }

    /// The distances: along the edge from the corner; across the face from each side.
    fn lengths(&self) -> Vec<f64> {
        match self.sides.as_slice() {
            [(_, foot)] => vec![foot.distance(self.v)],
            sides => sides.iter().map(|(_, foot)| foot.distance(self.p)).collect(),
        }
    }

    /// The point at the distances `d` (as [`Measure::lengths`] gives them).
    fn point(&self, d: &[f64]) -> Option<DVec3> {
        match (self.sides.as_slice(), d) {
            ([(_, foot)], [d, ..]) => Some(self.v + (*foot - self.v).try_normalize()? * *d),
            // Each side moved towards the cursor by its distance: where the two meet.
            ([(d0, f0), (d1, f1)], [a, b, ..]) => {
                let (n0, n1) = ((self.p - *f0).try_normalize()?, (self.p - *f1).try_normalize()?);
                meet(self.v + n0 * *a, *d0, self.v + n1 * *b, *d1)
            }
            _ => None,
        }
    }
}

/// Where the line through `p` along `a` meets the one through `q` along `b` (unit, coplanar).
fn meet(p: DVec3, a: DVec3, q: DVec3, b: DVec3) -> Option<DVec3> {
    let (w, c) = (p - q, a.dot(b));
    let det = 1.0 - c * c;
    (det > 1e-12).then(|| p + a * ((c * b.dot(w) - a.dot(w)) / det))
}

/// The first step, the same for both tools: what the cursor gives there, and the corner the
/// first point is measured from (the mode [`FROM_CORNER`]).
#[derive(Default)]
struct First {
    at: Option<StartAt>,
    from_corner: bool,
    measure: Option<Measure>,
}

impl First {
    fn hover(&mut self, cx: &Cx<'_>, px: DVec2) {
        self.at = start_at(cx, px);
        self.measure = self.at.as_ref().filter(|_| self.from_corner).and_then(|s| Measure::at(cx, s));
    }

    fn leave(&mut self) {
        self.at = None;
        self.measure = None;
    }

    fn inference(&self) -> Option<Inference> {
        self.at.as_ref().map(|s| s.inf.clone())
    }

    /// The frame of a first point typed as distances from the corner, and its direction.
    fn typed(&self, d: &[f64]) -> Option<(Frame, Option<DVec3>)> {
        let s = self.at.as_ref()?;
        let o = self.measure.as_ref()?.point(d)?;
        Some((Frame { o, ..s.f }, s.dir))
    }

    fn vcb(&self) -> Option<Vcb> {
        self.measure.as_ref().map(|m| Vcb::new(t!("tool.shape.vcb.distances"), VcbKind::Lengths(m.lengths())))
    }

    /// The corner measured from, and the distances, dashed (one along the edge, else one per
    /// side).
    fn preview(&self, cx: &Cx<'_>, lines: &mut Vec<PreviewLine>, labels: &mut Vec<PreviewLabel>, marks: &mut Vec<DVec3>) {
        let Some(m) = &self.measure else { return };
        mark(cx, marks, m.v);
        match m.sides.as_slice() {
            [(_, foot)] => {
                dashed(cx.camera, cx.size, lines, cx.to_world(m.v), cx.to_world(*foot), RED);
                label(cx, labels, m.v, *foot, RED);
            }
            sides => {
                for ((_, foot), color) in sides.iter().zip([RED, GREEN]) {
                    dashed(cx.camera, cx.size, lines, cx.to_world(m.v), cx.to_world(*foot), INK);
                    dashed(cx.camera, cx.size, lines, cx.to_world(*foot), cx.to_world(m.p), color);
                    label(cx, labels, *foot, m.p, color);
                }
            }
        }
    }
}

/// The cursor's point on the frame's plane (the axes' coordinates): a feature inferred anywhere
/// (endpoint, midpoint, edge…) is brought along the normal onto the plane; otherwise the
/// cursor's ray meets the plane.
fn on_plane(cx: &Cx<'_>, px: DVec2, f: &Frame) -> Option<(DVec3, Option<Inference>)> {
    let n_world = dir_to_world(cx, f.n);
    if let Some(inf) = cx.infer(px, |_| true, Some((cx.to_world(f.o), n_world))).filter(|i| i.kind.is_feature()) {
        return Some((f.flatten(cx.to_axes(inf.point)), Some(inf)));
    }
    cx.axes_ray(px).hit_plane(f.o, f.n).map(|p| (p, None))
}

/// The plane the box's second corner under `px` sets (§ module doc), and the corner on it:
/// `f` is the plane so far, `dir` the direction the first click gave.
fn base_at(cx: &Cx<'_>, px: DVec2, f: &Frame, dir: Option<DVec3>) -> Option<(Frame, DVec3, Option<Inference>)> {
    let o = f.o;
    if let Some(n) = locked_normal(cx) {
        let (p2, snap) = on_plane(cx, px, &Frame::new(o, n, dir))?;
        // A guide aimed at: the sides along it.
        let dir = snap.as_ref().and_then(|i| guide_dir(cx, i)).or(dir);
        return Some((Frame::new(o, n, dir), p2, snap));
    }
    let ray = cx.axes_ray(px).dir;
    // Nothing under the cursor: the ground through the first corner.
    let inf = cx.infer(px, |_| true, Some((cx.to_world(o), dir_to_world(cx, DVec3::Z))))?;
    let q = cx.to_axes(inf.point);
    let on = |a: DVec3| (q - o).dot(a).abs() < ON;
    // An axis plane through the first corner, its normal the way the plane so far has it.
    let axis = |a: DVec3| Frame::new(o, if f.n == -a { -a } else { a }, dir);
    let axes = [DVec3::Z, DVec3::X, DVec3::Y];
    // A guide aimed at gives the sides its direction, whatever the first click gave.
    let guide = guide_dir(cx, &inf);
    let dir = guide.or(dir);
    let g = match edge_dir(cx, &inf).or(guide) {
        // An edge or a guide across an axis plane it touches there: that plane; else the plane
        // through it and the first corner (one lying in the ground among them), its sides
        // along it unless the first click gave them (a guide's always).
        Some(d) => match axes.into_iter().find(|&a| on(a) && d.dot(a).abs() > SQUARE) {
            Some(a) => axis(a),
            None => match d.cross(q - o).try_normalize() {
                Some(n) => Frame::new(o, facing(n, ray), dir.or(Some(d))),
                // A guide through the first corner: the plane so far, along it.
                None if guide.is_some() => Frame::new(o, f.n, dir),
                None => *f,
            },
        },
        None if inf.kind.is_point() => axes.into_iter().find(|&a| on(a)).map_or(*f, axis),
        None if inf.kind == InferKind::OnFace => match hit_face(cx, &inf) {
            Some((n, p)) if (o - p).dot(n).abs() < ON => Frame::new(o, n, dir),
            _ => upright(o, q, ray, dir).unwrap_or(*f),
        },
        None if on(DVec3::Z) => axis(DVec3::Z),
        None => upright(o, q, ray, dir).unwrap_or(*f),
    };
    let snap = Some(inf).filter(|i| i.kind != InferKind::OnPlane);
    Some((g, g.flatten(q), snap))
}

/// The height the cursor gives above the plane at `p` (the axes' coordinates): that of a feature
/// inferred, or of a planar face parallel to the plane (not in it) the cursor is on; else that
/// of the closest point of the normal through `p` to the cursor's ray.
fn height_at(cx: &Cx<'_>, px: DVec2, f: &Frame, p: DVec3) -> (f64, Option<Inference>) {
    if let Some(inf) = cx.infer(px, |_| true, None) {
        if inf.kind.is_feature() {
            return (f.local(cx.to_axes(inf.point)).z, Some(inf));
        }
        if inf.kind == InferKind::OnFace
            && let Some((q, n)) = inf.hit.as_ref().and_then(|h| face::plane_at(cx.scene, h))
            && dir_to_axes(cx, n).dot(f.n).abs() > 1.0 - SQUARE
        {
            let h = f.local(cx.to_axes(q)).z;
            if h.abs() >= ON {
                return (h, Some(inf));
            }
        }
    }
    (f.local(cx.axes_ray(px).closest_on_line(p, f.n)).z, None)
}

/// The height step's `h`, the top's height above the plane (centred, the bottom as far below):
/// the cursor's at `p`, or with Shift held, the last height (the cursor's way); with the copy
/// modifier held, up to the first face met along the normal from `mid` (the base's middle).
fn pull_at(cx: &Cx<'_>, px: DVec2, f: &Frame, p: DVec3, mid: DVec3, centered: bool) -> (f64, Option<Inference>) {
    let (h, snap) = height_at(cx, px, f, p);
    let sign = if h < 0.0 { -1.0 } else { 1.0 };
    if cx.shift_down
        && let Some(last) = cx.last_height
    {
        return (sign * last * if centered { 0.5 } else { 1.0 }, None);
    }
    if cx.copy_down {
        let ray = Ray { origin: cx.to_world(mid), dir: dir_to_world(cx, f.n * sign), t_min: ON };
        if let Some(hit) = cx.scene.pick_inferable(&ray, |_| true) {
            return (sign * hit.t, None);
        }
    }
    (h, snap)
}

/// The bottom and the top (heights above the plane) of a shape pulled to `h`.
fn span(h: f64, centered: bool) -> (f64, f64) {
    if centered { (-h.abs(), h.abs()) } else { (h.min(0.0), h.max(0.0)) }
}

/// The height of a shape pulled to `h`, signed as `h` (what the measurements box shows).
fn whole(h: f64, centered: bool) -> f64 {
    if centered { 2.0 * h } else { h }
}

/// The `h` a whole height `w` asks for.
fn pull_of(w: f64, centered: bool) -> f64 {
    if centered { w / 2.0 } else { w }
}

/// The colour of a plane's lines: its normal's axis, magenta for another locked one, else ink.
fn plane_color(cx: &Cx<'_>, f: &Frame) -> [f32; 4] {
    let n = f.n.abs();
    if n.x == 1.0 {
        RED
    } else if n.y == 1.0 {
        GREEN
    } else if n.z == 1.0 {
        BLUE
    } else if locked_normal(cx).is_some() {
        MAGENTA
    } else {
        INK
    }
}

/// Model length of `px` pixels at `p` (the axes' coordinates).
fn pixels(cx: &Cx<'_>, p: DVec3, px: f64) -> f64 {
    cx.camera.world_per_pixel(cx.size, cx.to_world(p)) * px
}

/// The length of the side `a`–`b` written at its middle, when it is long enough on screen.
fn label(cx: &Cx<'_>, out: &mut Vec<PreviewLabel>, a: DVec3, b: DVec3, color: [f32; 4]) {
    let len = (b - a).length();
    if len >= MIN && len > pixels(cx, a, LABEL_PX) {
        out.push(PreviewLabel { a: cx.to_world(a), b: cx.to_world(b), text: vcb::format(len, cx.units()), color });
    }
}

/// A cross at `p` (the axes' coordinates), which the lengths keep clear of.
fn mark(cx: &Cx<'_>, marks: &mut Vec<DVec3>, p: DVec3) {
    marks.push(cx.to_world(p));
}

fn line(cx: &Cx<'_>, a: DVec3, b: DVec3, color: [f32; 4]) -> PreviewLine {
    PreviewLine { a: cx.to_world(a), b: cx.to_world(b), color, bold: false }
}

/// Whether the cursor's point is an inferred feature, worth a cross: not on the empty ground
/// (the fallback plane), nor without any inference.
fn snapped(snap: &Option<Inference>) -> bool {
    snap.as_ref().is_some_and(|i| i.kind != InferKind::OnPlane)
}

// ---- Options shared by both tools --------------------------------------------------------------

/// The height runs as far below the plane as above (saved for the tool).
const PULL_CENTERED: &str = "pull_centered";
/// The first point is measured from a corner (the first step's mode, not saved).
const FROM_CORNER: &str = "from_corner";

/// A toggle drawn as its icon (after OCL's Smart Draw, `get_action_option_btn_child`).
fn toggle(key: &'static str, label: String, tip: String, on: bool, saved: bool, icon: &'static str) -> ToolOption {
    ToolOption { key, label, tip, value: OptionValue::Toggle(on), choices: Vec::new(), saved, icon: Some(icon) }
}

/// The options both tools have: the height centred, the first point measured from a corner.
fn shared_options(cx: &Cx<'_>, first: &First) -> Vec<ToolOption> {
    vec![
        toggle(
            PULL_CENTERED,
            t!("tool.shape.option.pull_centered"),
            t!("tool.shape.option.pull_centered_tip", copy_key = copy_key()),
            cx.toggle(PULL_CENTERED),
            true,
            "centered",
        ),
        toggle(
            FROM_CORNER,
            t!("tool.shape.option.from_corner"),
            t!("tool.shape.option.from_corner_tip", alt_key = alt_key()),
            first.from_corner,
            false,
            "from_corner",
        ),
    ]
}

/// A key tapped at the first step: the other modifier switches the measure from a corner.
fn first_key(key: Key, first: &First) -> Option<Command> {
    (key == Key::Alt).then_some(Command::SetOption { key: FROM_CORNER, value: OptionValue::Toggle(!first.from_corner) })
}

// ---- Box ---------------------------------------------------------------------------------------

/// The box's option: the first click is the centre of the base (saved for the tool).
const CENTERED: &str = "centered";

/// How close (pixels) the opposite corner is drawn to the square's to snap to it (OpenCutList's
/// 20 px aperture, across).
const SQUARE_PX: f64 = 10.0;
/// Below this cosine between the eye's ray and a plane's normal, the plane is seen edge on.
const EDGE_ON: f64 = 0.1;

#[derive(Default)]
pub struct BoxTool {
    state: BoxState,
    first: First,
    snap: Option<Inference>,
    /// The saved options, as the editor last gave them: the first click is the centre of the
    /// base ([`CENTERED`]); the height is centred ([`PULL_CENTERED`]).
    centered: bool,
    pull_centered: bool,
}

#[derive(Default, Clone, Copy)]
enum BoxState {
    #[default]
    First,
    /// The first click (`f.o`) is set, `dir` the direction it gave; the cursor's corner, `p2`,
    /// follows the cursor, and the plane `f` with it.
    Base { f: Frame, dir: Option<DVec3>, p2: DVec3 },
    /// The base is set (`centered` on `f.o` or not); the height follows the cursor.
    Height { f: Frame, dir: Option<DVec3>, p2: DVec3, centered: bool, h: f64 },
}

/// The base from the first click `f.o` to the cursor's corner `p2`, as a frame on its first
/// corner and its opposite one: centred on `f.o`, the base runs as far on the other side.
fn base(f: Frame, p2: DVec3, centered: bool) -> (Frame, DVec3) {
    if !centered {
        return (f, p2);
    }
    let d = f.local(p2);
    (Frame { o: f.at(DVec3::new(-d.x, -d.y, 0.0)), ..f }, p2)
}

/// The cursor's corner snapped to the square's (side the first one, as drawn) when it is
/// near it on screen, for a corner not taken from a feature. Not on a plane seen nearly edge
/// on: every point of it is near the cursor there.
fn square(cx: &Cx<'_>, px: DVec2, f: &Frame, p2: DVec3) -> Option<DVec3> {
    if cx.axes_ray(px).dir.dot(f.n).abs() < EDGE_ON {
        return None;
    }
    let d = f.local(p2);
    let sq = f.at(DVec3::new(d.x, d.x.abs() * if d.y < 0.0 { -1.0 } else { 1.0 }, 0.0));
    let at = cx.camera.project(cx.to_world(sq), cx.size)?;
    (d.x.abs() >= MIN && at.distance(px) <= SQUARE_PX).then_some(sq)
}

/// The base of the face `face` (the world): the rectangle around it, its sides along its
/// longest straight side; as a frame on a corner, the opposite corner and the direction.
fn face_base(cx: &Cx<'_>, face: &Face) -> Option<(Frame, DVec3, Option<DVec3>)> {
    let dir = face.longest_side().map(|d| dir_to_axes(cx, d));
    let f = Frame::new(cx.to_axes(face.point), dir_to_axes(cx, face.normal), dir);
    let (lo, hi) = face.points().map(|p| f.local(cx.to_axes(p))).fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
    let p2 = f.at(DVec3::new(hi.x, hi.y, 0.0));
    let f = Frame { o: f.at(DVec3::new(lo.x, lo.y, 0.0)), ..f };
    let d = f.local(p2);
    (d.x >= MIN && d.y >= MIN).then_some((f, p2, dir))
}

impl BoxTool {
    /// The box on the base `f.o`–`p2` (its first corner and the opposite one), pulled to `h`.
    fn finish(&mut self, cx: &Cx<'_>, f: Frame, p2: DVec3, h: f64) -> Vec<Command> {
        self.state = BoxState::First;
        let d = f.local(p2);
        let (z0, z1) = span(h, self.pull_centered);
        if d.x.abs() < MIN || d.y.abs() < MIN || z1 - z0 < MIN {
            return vec![Command::Say(t!("tool.box.say.empty"))];
        }
        let size = DVec3::new(d.x.abs(), d.y.abs(), z1 - z0);
        let (primitive, placement) = if f.aligned() {
            // In the axes: corners as clicked, no rounding through a frame.
            let lo = f.o.min(p2);
            (Primitive::Box { origin: DVec3::new(lo.x, lo.y, f.o.z + z0), size }, DAffine3::IDENTITY)
        } else {
            let lo = DVec3::new(d.x.min(0.0), d.y.min(0.0), z0);
            (Primitive::Box { origin: lo, size }, f.placement())
        };
        vec![Command::Apply(cx.add_primitive(primitive, placement)), Command::RememberHeight(z1 - z0)]
    }

    /// The height step ends at `h`: the box, or, without a height, a word and the step stays.
    fn pulled(&mut self, cx: &Cx<'_>, h: f64) -> Vec<Command> {
        let BoxState::Height { f, dir, p2, centered, .. } = self.state else { return vec![] };
        if h.abs() < MIN {
            self.state = BoxState::Height { f, dir, p2, centered, h };
            return vec![Command::Say(t!("tool.box.say.empty"))];
        }
        let (f, p2) = base(f, p2, centered);
        self.finish(cx, f, p2, h)
    }
}

impl Tool for BoxTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Box
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        self.centered = cx.toggle(CENTERED);
        self.pull_centered = cx.toggle(PULL_CENTERED);
        match (input, self.state) {
            // The copy modifier tapped: the base centred, or the height.
            (ToolInput::Key { key: Key::Copy, .. }, BoxState::Base { .. }) => {
                return vec![Command::SetOption { key: CENTERED, value: OptionValue::Toggle(!self.centered) }];
            }
            (ToolInput::Key { key: Key::Copy, .. }, BoxState::Height { .. }) => {
                return vec![Command::SetOption { key: PULL_CENTERED, value: OptionValue::Toggle(!self.pull_centered) }];
            }
            // Shift tapped with the copy modifier held over a face: the whole face as the base.
            (ToolInput::Key { key: Key::Shift, .. }, BoxState::First) if cx.copy_down => {
                if let Some((f, p2, dir)) = self.first.at.as_ref().and_then(|s| s.face.as_ref()).and_then(|face| face_base(cx, face)) {
                    self.state = BoxState::Height { f, dir, p2, centered: false, h: 0.0 };
                    self.first.from_corner = false;
                }
            }
            (ToolInput::Key { key, .. }, BoxState::First) => return first_key(*key, &self.first).into_iter().collect(),
            (ToolInput::Hover { px }, BoxState::First) => self.first.hover(cx, *px),
            (ToolInput::Hover { px }, BoxState::Base { f, dir, .. }) => {
                if let Some((f, mut p2, mut snap)) = base_at(cx, *px, &f, dir) {
                    if !snap.as_ref().is_some_and(|i| i.kind.is_feature())
                        && let Some(sq) = square(cx, *px, &f, p2)
                    {
                        (p2, snap) = (sq, None);
                    }
                    self.state = BoxState::Base { f, dir, p2 };
                    self.snap = snap;
                }
            }
            (ToolInput::Hover { px }, BoxState::Height { f, dir, p2, centered, .. }) => {
                let (g, q) = base(f, p2, centered);
                let d = g.local(q);
                let (h, snap) = pull_at(cx, *px, &f, p2, g.at(DVec3::new(d.x / 2.0, d.y / 2.0, 0.0)), self.pull_centered);
                self.state = BoxState::Height { f, dir, p2, centered, h };
                self.snap = snap;
            }
            (ToolInput::Click { px, .. }, BoxState::First) => {
                if let Some(s) = start_at(cx, *px) {
                    self.state = BoxState::Base { f: s.f, dir: s.dir, p2: s.f.o };
                    self.first.from_corner = false;
                }
            }
            (ToolInput::Click { .. }, BoxState::Base { f, dir, p2 }) => {
                let d = f.local(p2);
                if d.x.abs() < MIN || d.y.abs() < MIN {
                    return vec![Command::Say(t!("tool.box.say.empty_base"))];
                }
                self.state = BoxState::Height { f, dir, p2, centered: self.centered, h: 0.0 };
            }
            (ToolInput::Click { .. }, BoxState::Height { h, .. }) => return self.pulled(cx, h),
            // A double-click on the opposite corner: the last height again.
            (ToolInput::DoubleClick { .. }, BoxState::Height { .. }) => {
                if let Some(last) = cx.last_height {
                    return self.pulled(cx, pull_of(last, self.pull_centered));
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), BoxState::First) => {
                if let Some((f, dir)) = self.first.typed(v) {
                    self.state = BoxState::Base { f, dir, p2: f.o };
                    self.first.from_corner = false;
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), BoxState::Base { f, dir, .. }) => {
                // Signed as the drag (the box's VCB gives the current ones); centred, the
                // whole lengths, half on each side.
                let k = if self.centered { 0.5 } else { 1.0 };
                let (w, l) = (v[0], v[1]);
                if w.abs() < MIN || l.abs() < MIN {
                    return vec![Command::Say(t!("tool.box.say.base_lengths"))];
                }
                let p2 = if f.aligned() { f.o + DVec3::new(w * k, l * k, 0.0) } else { f.at(DVec3::new(w * k, l * k, 0.0)) };
                self.state = BoxState::Height { f, dir, p2, centered: self.centered, h: 0.0 };
                if let Some(&h) = v.get(2) {
                    return self.pulled(cx, pull_of(h, self.pull_centered));
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), BoxState::Height { .. }) => return self.pulled(cx, pull_of(v[0], self.pull_centered)),
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let mut lines = Vec::new();
        let mut labels = Vec::new();
        let mut marks = Vec::new();
        // The corners of the base from `f.o` to the local `(x, y)`, at height `z`.
        let corners = |f: &Frame, x: f64, y: f64, z: f64| [(0.0, 0.0), (x, 0.0), (x, y), (0.0, y)].map(|(x, y)| f.at(DVec3::new(x, y, z)));
        let rect = |lines: &mut Vec<PreviewLine>, c: [DVec3; 4], color| {
            lines.extend((0..4).map(|i| line(cx, c[i], c[(i + 1) % 4], color)));
        };
        let mut inference = self.snap.clone();
        match self.state {
            BoxState::First => {
                inference = self.first.inference();
                if let Some(s) = &self.first.at {
                    // A small base at the cursor, in the plane it would take (around it, centred).
                    let f = s.f;
                    let w = pixels(cx, f.o, GAUGE_PX);
                    let o = if self.centered { f.at(DVec3::new(-w / 2.0, -w / 4.0, 0.0)) } else { f.o };
                    rect(&mut lines, corners(&Frame { o, ..f }, w, w / 2.0, 0.0), plane_color(cx, &f));
                    if snapped(&inference) {
                        mark(cx, &mut marks, f.o);
                    }
                }
                self.first.preview(cx, &mut lines, &mut labels, &mut marks);
            }
            BoxState::Base { f, p2, .. } => {
                let color = plane_color(cx, &f);
                let (g, p2) = base(f, p2, self.centered);
                let d = g.local(p2);
                let c = corners(&g, d.x, d.y, 0.0);
                rect(&mut lines, c, color);
                label(cx, &mut labels, c[0], c[1], RED);
                label(cx, &mut labels, c[0], c[3], GREEN);
                // A square base: its diagonal; centred: the centre, and on to the cursor.
                if (d.x.abs() - d.y.abs()).abs() <= 1e-9 * d.x.abs().max(1.0) && d.x.abs() >= MIN {
                    dashed(cx.camera, cx.size, &mut lines, cx.to_world(c[0]), cx.to_world(c[2]), color);
                } else if self.centered {
                    dashed(cx.camera, cx.size, &mut lines, cx.to_world(f.o), cx.to_world(p2), INK);
                }
                if self.centered {
                    mark(cx, &mut marks, f.o);
                }
            }
            BoxState::Height { f, p2, centered, h, .. } => {
                let color = plane_color(cx, &f);
                let (f, p2) = base(f, p2, centered);
                let d = f.local(p2);
                let (z0, z1) = span(h, self.pull_centered);
                let (bottom, top) = (corners(&f, d.x, d.y, z0), corners(&f, d.x, d.y, z1));
                rect(&mut lines, bottom, color);
                rect(&mut lines, top, color);
                lines.extend(bottom.iter().zip(&top).map(|(a, b)| line(cx, *a, *b, color)));
                label(cx, &mut labels, bottom[0], top[0], BLUE);
                // Centred: the diagonal through the cursor (at the opposite corner, height `h`).
                if self.pull_centered {
                    let (a, b) = (f.at(DVec3::new(0.0, 0.0, -h)), f.at(DVec3::new(d.x, d.y, h)));
                    dashed(cx.camera, cx.size, &mut lines, cx.to_world(a), cx.to_world(b), INK);
                }
            }
        }
        Preview { lines, labels, marks, inference, ..Default::default() }
    }

    fn status(&self) -> String {
        match self.state {
            BoxState::First => t!("tool.box.hint.first", copy_key = copy_key(), alt_key = alt_key()),
            BoxState::Base { .. } => t!("tool.box.hint.base", copy_key = copy_key()),
            BoxState::Height { .. } => t!("tool.shape.hint.height", copy_key = copy_key()),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        match self.state {
            BoxState::First => self.first.vcb(),
            BoxState::Base { f, p2, .. } => {
                // The whole lengths, signed as the drag.
                let d = f.local(p2) * if self.centered { 2.0 } else { 1.0 };
                Some(Vcb::new(t!("tool.box.vcb.dimensions"), VcbKind::Lengths(vec![d.x, d.y, 0.0])).optional(1))
            }
            BoxState::Height { h, .. } => Some(Vcb::new(t!("tool.box.vcb.height"), VcbKind::Lengths(vec![whole(h, self.pull_centered)]))),
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.state, BoxState::First)
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let centered = toggle(
            CENTERED,
            t!("tool.box.option.centered"),
            t!("tool.box.option.centered_tip", copy_key = copy_key()),
            cx.toggle(CENTERED),
            true,
            "rectangle_centered",
        );
        std::iter::once(centered).chain(shared_options(cx, &self.first)).collect()
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        match (key, value) {
            (CENTERED, OptionValue::Toggle(on)) => self.centered = *on,
            (PULL_CENTERED, OptionValue::Toggle(on)) => self.pull_centered = *on,
            (FROM_CORNER, OptionValue::Toggle(on)) => self.first.from_corner = *on,
            _ => {}
        }
    }

    fn down_locks_face(&self) -> bool {
        !matches!(self.state, BoxState::Height { .. })
    }

    fn leave(&mut self) {
        self.first.leave();
        self.snap = None;
    }

    fn back(&mut self) {
        self.state = match self.state {
            BoxState::Height { f, dir, p2, .. } => BoxState::Base { f, dir, p2 },
            _ => BoxState::First,
        };
        self.snap = None;
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
    first: First,
    snap: Option<Inference>,
    /// The height is centred ([`PULL_CENTERED`], as the editor last gave it).
    pull_centered: bool,
    /// The base is measured across, not from the centre ([`MEASURE`], as the editor last gave
    /// it).
    diameter: bool,
}

/// The cylinder's option: what the base's length is, the radius or the diameter (saved).
const MEASURE: &str = "measure";
const RADIUS: &str = "radius";
const DIAMETER: &str = "diameter";

#[derive(Default, Clone, Copy)]
enum CylState {
    #[default]
    Center,
    /// The centre (`f.o`) is set; the radius follows the cursor, at `rim` (local, in the plane).
    Radius {
        f: Frame,
        rim: DVec3,
    },
    Height {
        f: Frame,
        rim: DVec3,
        h: f64,
    },
}

/// The frame of a cylinder's centre: the plane the first step gives, its sides the usual ones
/// (a circle has none).
fn center_frame(f: Frame) -> Frame {
    Frame::new(f.o, f.n, None)
}

/// The cursor's rim point `p` snapped onto the red, green or blue axis through the centre
/// (those lying in the plane) when it is drawn near it on screen; not on a plane seen nearly
/// edge on, where every point of it is near the cursor.
fn on_axis(cx: &Cx<'_>, px: DVec2, f: &Frame, p: DVec3) -> Option<DVec3> {
    if cx.axes_ray(px).dir.dot(f.n).abs() < EDGE_ON {
        return None;
    }
    [DVec3::X, DVec3::Y, DVec3::Z]
        .into_iter()
        .filter(|a| a.dot(f.n).abs() < SQUARE)
        .map(|a| f.o + a * (p - f.o).dot(a))
        .filter(|q| (*q - f.o).length() >= MIN)
        .filter_map(|q| Some((q, cx.camera.project(cx.to_world(q), cx.size)?.distance(px))))
        .filter(|&(_, d)| d <= SQUARE_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(q, _)| q)
}

/// The colour of the axis `d` runs along, if it does.
fn axis_color(d: DVec3) -> Option<[f32; 4]> {
    let d = d.try_normalize()?;
    [(DVec3::X, RED), (DVec3::Y, GREEN), (DVec3::Z, BLUE)].into_iter().find(|(a, _)| d.dot(*a).abs() > 1.0 - SQUARE).map(|(_, c)| c)
}

impl CylinderTool {
    fn finish(&mut self, cx: &Cx<'_>, f: Frame, r: f64, h: f64) -> Vec<Command> {
        self.state = CylState::Center;
        let (z0, z1) = span(h, self.pull_centered);
        if r < MIN || z1 - z0 < MIN {
            return vec![Command::Say(t!("tool.cylinder.say.empty"))];
        }
        let (origin, placement) = if f.aligned() {
            (DVec3::new(f.o.x, f.o.y, f.o.z + z0), DAffine3::IDENTITY)
        } else {
            (DVec3::new(0.0, 0.0, z0), f.placement())
        };
        let primitive = Primitive::Cylinder { origin, radius: r, height: z1 - z0 };
        vec![Command::Apply(cx.add_primitive(primitive, placement)), Command::RememberHeight(z1 - z0)]
    }

    /// The height step ends at `h`: the cylinder, or, without a height, a word and the step stays.
    fn pulled(&mut self, cx: &Cx<'_>, h: f64) -> Vec<Command> {
        let CylState::Height { f, rim, .. } = self.state else { return vec![] };
        if h.abs() < MIN {
            self.state = CylState::Height { f, rim, h };
            return vec![Command::Say(t!("tool.cylinder.say.empty"))];
        }
        self.finish(cx, f, rim.length(), h)
    }
}

impl Tool for CylinderTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Cylinder
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        self.pull_centered = cx.toggle(PULL_CENTERED);
        self.diameter = cx.choice(MEASURE) == Some(DIAMETER);
        match (input, self.state) {
            (ToolInput::Key { key: Key::Copy, .. }, CylState::Height { .. }) => {
                return vec![Command::SetOption { key: PULL_CENTERED, value: OptionValue::Toggle(!self.pull_centered) }];
            }
            // Shift tapped with the copy modifier held over a disc: that circle as the base.
            (ToolInput::Key { key: Key::Shift, .. }, CylState::Center) if cx.copy_down => {
                if let Some((c, r, n)) =
                    self.first.at.as_ref().and_then(|s| s.face.as_ref()).and_then(|face| face.disc.map(|(c, r)| (c, r, face.normal)))
                {
                    let f = Frame::new(cx.to_axes(c), dir_to_axes(cx, n), None);
                    self.state = CylState::Height { f, rim: DVec3::X * r, h: 0.0 };
                    self.first.from_corner = false;
                }
            }
            (ToolInput::Key { key, .. }, CylState::Center) => return first_key(*key, &self.first).into_iter().collect(),
            (ToolInput::Hover { px }, CylState::Center) => self.first.hover(cx, *px),
            (ToolInput::Hover { px }, CylState::Radius { f, .. }) => {
                if let Some((p, snap)) = on_plane(cx, *px, &f) {
                    // Not taken from a feature: drawn near an axis through the centre, along it.
                    let p = if snapped(&snap) { p } else { on_axis(cx, *px, &f, p).unwrap_or(p) };
                    self.state = CylState::Radius { f, rim: f.local(p) };
                    self.snap = snap;
                }
            }
            (ToolInput::Hover { px }, CylState::Height { f, rim, .. }) => {
                let (h, snap) = pull_at(cx, *px, &f, f.at(rim), f.o, self.pull_centered);
                self.state = CylState::Height { f, rim, h };
                self.snap = snap;
            }
            (ToolInput::Click { px, .. }, CylState::Center) => {
                if let Some(s) = start_at(cx, *px) {
                    self.state = CylState::Radius { f: center_frame(s.f), rim: DVec3::ZERO };
                    self.first.from_corner = false;
                }
            }
            (ToolInput::Click { .. }, CylState::Radius { f, rim }) if rim.length() >= MIN => {
                self.state = CylState::Height { f, rim, h: 0.0 };
            }
            (ToolInput::Click { .. }, CylState::Height { h, .. }) => return self.pulled(cx, h),
            (ToolInput::DoubleClick { .. }, CylState::Height { .. }) => {
                if let Some(last) = cx.last_height {
                    return self.pulled(cx, pull_of(last, self.pull_centered));
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), CylState::Center) => {
                if let Some((f, _)) = self.first.typed(v) {
                    self.state = CylState::Radius { f: center_frame(f), rim: DVec3::ZERO };
                    self.first.from_corner = false;
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), CylState::Radius { f, rim }) => {
                let r = if self.diameter { v[0] / 2.0 } else { v[0] };
                if r < MIN {
                    return vec![Command::Say(t!("tool.cylinder.say.zero_radius"))];
                }
                // The typed radius, the way the cursor gave.
                self.state = CylState::Height { f, rim: rim.try_normalize().unwrap_or(DVec3::X) * r, h: 0.0 };
                if let Some(&h) = v.get(1) {
                    return self.pulled(cx, pull_of(h, self.pull_centered));
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), CylState::Height { .. }) => return self.pulled(cx, pull_of(v[0], self.pull_centered)),
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
        let mut labels = Vec::new();
        let mut marks = Vec::new();
        let mut inference = self.snap.clone();
        match self.state {
            CylState::Center => {
                inference = self.first.inference();
                if let Some(s) = &self.first.at {
                    let f = s.f;
                    let color = plane_color(cx, &f);
                    lines.extend(circle(f, pixels(cx, f.o, GAUGE_PX) / 2.0, 0.0).map(|(a, b)| line(cx, a, b, color)));
                    if snapped(&inference) {
                        mark(cx, &mut marks, f.o);
                    }
                }
                self.first.preview(cx, &mut lines, &mut labels, &mut marks);
            }
            CylState::Radius { f, rim } => {
                let color = plane_color(cx, &f);
                lines.extend(circle(f, rim.length(), 0.0).map(|(a, b)| line(cx, a, b, color)));
                mark(cx, &mut marks, f.o);
                // Measured across: the whole diameter, through the centre.
                let (o, p) = (if self.diameter { f.at(-rim) } else { f.o }, f.at(rim));
                let color = axis_color(p - o).unwrap_or(INK);
                dashed(cx.camera, cx.size, &mut lines, cx.to_world(o), cx.to_world(p), color);
                label(cx, &mut labels, o, p, color);
            }
            CylState::Height { f, rim, h } => {
                let (r, color) = (rim.length(), plane_color(cx, &f));
                let (z0, z1) = span(h, self.pull_centered);
                lines.extend(circle(f, r, z0).map(|(a, b)| line(cx, a, b, color)));
                lines.extend(circle(f, r, z1).map(|(a, b)| line(cx, a, b, color)));
                for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
                    let a = f.at(DVec3::new(x * r, y * r, z0));
                    lines.push(line(cx, a, a + f.n * (z1 - z0), color));
                }
                label(cx, &mut labels, f.at(rim + DVec3::Z * z0), f.at(rim + DVec3::Z * z1), BLUE);
            }
        }
        Preview { lines, labels, marks, inference, ..Default::default() }
    }

    fn status(&self) -> String {
        match self.state {
            CylState::Center => t!("tool.cylinder.hint.center", copy_key = copy_key(), alt_key = alt_key()),
            CylState::Radius { .. } if self.diameter => t!("tool.cylinder.hint.diameter"),
            CylState::Radius { .. } => t!("tool.cylinder.hint.radius"),
            CylState::Height { .. } => t!("tool.shape.hint.height", copy_key = copy_key()),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        match self.state {
            CylState::Center => self.first.vcb(),
            CylState::Radius { rim, .. } => {
                let (label, len) = if self.diameter {
                    (t!("tool.cylinder.vcb.diameter"), 2.0 * rim.length())
                } else {
                    (t!("tool.cylinder.vcb.radius"), rim.length())
                };
                Some(Vcb::new(label, VcbKind::Lengths(vec![len, 0.0])).optional(1))
            }
            CylState::Height { h, .. } => {
                Some(Vcb::new(t!("tool.cylinder.vcb.height"), VcbKind::Lengths(vec![whole(h, self.pull_centered)])))
            }
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.state, CylState::Center)
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let value = if cx.choice(MEASURE) == Some(DIAMETER) { DIAMETER } else { RADIUS };
        let mut options = vec![ToolOption {
            key: MEASURE,
            label: t!("tool.cylinder.option.measure"),
            tip: t!("tool.cylinder.option.measure_tip"),
            value: OptionValue::Choice(value.into()),
            choices: vec![
                OptionChoice {
                    value: RADIUS,
                    label: t!("tool.cylinder.option.radius"),
                    tip: t!("tool.cylinder.option.radius_tip"),
                    icon: Some("measure_radius"),
                },
                OptionChoice {
                    value: DIAMETER,
                    label: t!("tool.cylinder.option.diameter"),
                    tip: t!("tool.cylinder.option.diameter_tip"),
                    icon: Some("measure_diameter"),
                },
            ],
            saved: true,
            icon: None,
        }];
        options.extend(shared_options(cx, &self.first));
        options
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        match (key, value) {
            (MEASURE, OptionValue::Choice(v)) => self.diameter = v == DIAMETER,
            (PULL_CENTERED, OptionValue::Toggle(on)) => self.pull_centered = *on,
            (FROM_CORNER, OptionValue::Toggle(on)) => self.first.from_corner = *on,
            _ => {}
        }
    }

    fn down_locks_face(&self) -> bool {
        matches!(self.state, CylState::Center)
    }

    fn leave(&mut self) {
        self.first.leave();
        self.snap = None;
    }

    fn back(&mut self) {
        self.state = match self.state {
            CylState::Height { f, rim, .. } => CylState::Radius { f, rim },
            _ => CylState::Center,
        };
        self.snap = None;
    }

    fn reset(&mut self) {
        self.state = CylState::Center;
        self.snap = None;
    }
}

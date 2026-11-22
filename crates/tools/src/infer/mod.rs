//! Inferences (docs/tools/inferences.md): the point a tool takes under the cursor.
//!
//! Candidates come from the solids' exact snap features ([`Snaps`]), placed in the world by
//! each occurrence: endpoints, midpoints, thirds and centres; then — when the tool has a start point —
//! where a line through it crosses an edge, then the lines themselves (the context's red,
//! green and blue axes, parallel or perpendicular to the last straight edge hovered, tangent
//! or radial to the last arc); then points on edges, then the face under the cursor, then a
//! fallback plane. Within an aperture of [`APERTURE_PX`] the best kind wins, then the nearest
//! on screen, then the nearest to the eye; a point hidden behind the scene is skipped.
//!
//! A point hovered a while is **tracked**: its axes are inferred too, and where two lines of
//! different origins cross. The middle of two tracked points is a point to infer.
//!
//! A **lock** (an arrow key, or Shift held on a line) keeps the point on one line: a feature
//! hovered is then projected onto it ("aligned with that corner"), an edge it crosses gives
//! the crossing.

mod crossings;
mod curves;
mod edges;
mod lines;
mod points;
#[cfg(test)]
mod tests;

use glam::{DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{EntityPath, GuideShape};

use crate::camera::{Camera, Projector, Ray};
use crate::scene::{DisplayScene, Hit, Owner, ray_box};
use crossings::crossing;
use curves::kept_guides;
use lines::{Line, lines, on_line, two_lines};
use points::infer_points;

pub use edges::{EdgeHit, EdgesOn, edge_at, tangent_chain};

/// How far from the cursor a feature is inferred (pixels).
pub const APERTURE_PX: f64 = 10.0;

/// How far from the cursor a third of an edge is inferred (pixels): narrower than
/// [`APERTURE_PX`], the edge stays easy to take between its points.
const THIRD_APERTURE_PX: f64 = 6.0;

/// How close (pixels, at the point's depth) a line must pass to an edge to cross it: two lines
/// in space seldom meet exactly.
const CROSSING_PX: f64 = 0.5;

/// Which line: through the start point, or through a tracked point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineKind {
    /// The context's axis `k` (0 red X, 1 green Y, 2 blue Z) through the start point.
    Axis(usize),
    /// The context's axis `k` through a tracked point.
    TrackedAxis(usize),
    /// Parallel to the last straight edge hovered.
    Parallel,
    /// Perpendicular to the last straight edge hovered, in the tool's plane.
    Perpendicular,
    /// Parallel to the tangent of the last arc hovered, where it was hovered.
    Tangent,
    /// Along the radius of the last arc hovered, where it was hovered.
    Radial,
    /// A guide line of the document (docs/tools/tape-measure.md).
    Guide,
    /// The axis `k` of the context's axes, as drawn: through their origin.
    ModelAxis(usize),
    /// The line the tool draws itself (the « Tournage » axis, [`Query::tool_line`]).
    Tool,
}

/// An axis's colour, as its name says it.
fn axis_name(k: usize) -> String {
    match k {
        0 => t!("infer.axis.red"),
        1 => t!("infer.axis.green"),
        _ => t!("infer.axis.blue"),
    }
}

impl LineKind {
    /// A line of the scene itself — a guide, an axis as drawn —, not one a tool's point
    /// gives: it is drawn depth-tested (not inferred where hidden), and it is an edge to refer
    /// to (the tape offsets a guide from it).
    pub fn is_fixed(self) -> bool {
        matches!(self, LineKind::Guide | LineKind::ModelAxis(_) | LineKind::Tool)
    }

    /// The line's name inside a crossing.
    fn name(self) -> String {
        match self {
            LineKind::Axis(k) => t!("infer.line.axis", axis = axis_name(k)),
            LineKind::TrackedAxis(k) => t!("infer.line.tracked_axis", axis = axis_name(k)),
            LineKind::Parallel => t!("infer.line.parallel"),
            LineKind::Perpendicular => t!("infer.line.perpendicular"),
            LineKind::Tangent => t!("infer.line.tangent"),
            LineKind::Radial => t!("infer.line.radial"),
            LineKind::Guide => t!("infer.line.guide"),
            LineKind::ModelAxis(k) => t!("infer.line.axis", axis = axis_name(k)),
            LineKind::Tool => t!("infer.line.tool"),
        }
    }

    fn on_label(self) -> String {
        match self {
            LineKind::Axis(k) => t!("infer.on_line.axis", axis = axis_name(k)),
            LineKind::TrackedAxis(k) => t!("infer.on_line.tracked_axis", axis = axis_name(k)),
            LineKind::Parallel => t!("infer.on_line.parallel"),
            LineKind::Perpendicular => t!("infer.on_line.perpendicular"),
            LineKind::Tangent => t!("infer.on_line.tangent"),
            LineKind::Radial => t!("infer.on_line.radial"),
            LineKind::Guide => t!("infer.on_guide"),
            LineKind::ModelAxis(k) => t!("infer.on_line.axis", axis = axis_name(k)),
            LineKind::Tool => t!("infer.on_line.tool"),
        }
    }

    /// The axis it runs along, if any (its colour).
    pub fn axis(self) -> Option<usize> {
        match self {
            LineKind::Axis(k) | LineKind::TrackedAxis(k) | LineKind::ModelAxis(k) => Some(k),
            _ => None,
        }
    }
}

/// What kind of point was inferred, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InferKind {
    /// A vertex: the end of an edge.
    Endpoint,
    /// The middle of a straight edge or of an arc.
    Midpoint,
    /// A point at one or two thirds of a straight edge or of an arc.
    Third,
    /// The middle of the two tracked points: built from them, it is not tracked itself.
    TrackedMiddle,
    /// A corner of the box of what the tool takes (docs/tools/inferences.md#selection-box).
    BoxCorner,
    /// The centre of an arc, a circle or an ellipse.
    Center,
    /// The origin of the context being edited.
    Origin,
    /// Where a line crosses an edge (a point built from both).
    OnEdgeAndLine(LineKind),
    /// Where two lines of different origins cross.
    OnTwoLines(LineKind, LineKind),
    /// Where a line crosses a guide circle.
    OnGuideAndLine(LineKind),
    /// Where a guide circle crosses an edge.
    OnGuideAndEdge,
    /// Where two guide circles cross.
    OnTwoGuides,
    /// On a line.
    OnLine(LineKind),
    /// On a guide circle (a guide line gives `OnLine(LineKind::Guide)`).
    OnGuide,
    /// A point on an edge.
    OnEdge,
    /// A point on the face under the cursor.
    OnFace,
    /// Nothing under the cursor: a point of the tool's fallback plane.
    OnPlane,
}

/// How many ranks [`InferKind::rank`] gives.
const RANKS: usize = InferKind::OnPlane.rank() + 1;

impl InferKind {
    /// Its priority, best first (0); kinds of one rank are equals.
    const fn rank(self) -> usize {
        match self {
            InferKind::Endpoint => 0,
            InferKind::Midpoint | InferKind::TrackedMiddle | InferKind::BoxCorner | InferKind::Center | InferKind::Origin => 1,
            InferKind::Third
            | InferKind::OnEdgeAndLine(_)
            | InferKind::OnTwoLines(..)
            | InferKind::OnGuideAndLine(_)
            | InferKind::OnGuideAndEdge
            | InferKind::OnTwoGuides => 2,
            InferKind::OnLine(_) | InferKind::OnGuide => 3,
            InferKind::OnEdge => 4,
            InferKind::OnFace => 5,
            InferKind::OnPlane => 6,
        }
    }

    /// A point of the geometry (endpoint, midpoint, third, centre, origin) or the middle of the
    /// tracked points.
    pub fn is_point(self) -> bool {
        self.rank() <= 1 || self == InferKind::Third
    }

    /// A point that can be tracked: the middle of the tracked points is not, it would replace
    /// one of the two it stands on.
    pub fn is_trackable(self) -> bool {
        self.is_point() && self != InferKind::TrackedMiddle
    }

    /// A point the geometry or a line gives, not just somewhere on a face or a plane: what a
    /// tool should take as is, even off its working plane.
    pub fn is_feature(self) -> bool {
        self.rank() <= InferKind::OnEdge.rank()
    }

    /// The (first) line the point is on, if any.
    pub fn line(self) -> Option<LineKind> {
        match self {
            InferKind::OnLine(l) | InferKind::OnEdgeAndLine(l) | InferKind::OnTwoLines(l, _) | InferKind::OnGuideAndLine(l) => Some(l),
            _ => None,
        }
    }

    /// The tooltip (empty: none).
    pub fn label(self) -> String {
        match self {
            InferKind::Endpoint => t!("infer.endpoint"),
            InferKind::Midpoint => t!("infer.midpoint"),
            InferKind::Third => t!("infer.third"),
            InferKind::TrackedMiddle => t!("infer.tracked_middle"),
            InferKind::BoxCorner => t!("infer.box_corner"),
            InferKind::Center => t!("infer.center"),
            InferKind::Origin => t!("infer.origin"),
            InferKind::OnEdgeAndLine(l) => format!("{} × {}", t!("infer.edge"), l.name()),
            InferKind::OnTwoLines(a, b) => {
                let s = format!("{} × {}", a.name(), b.name());
                let mut c = s.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
            }
            InferKind::OnGuideAndLine(l) => format!("{} × {}", t!("infer.guide"), l.name()),
            InferKind::OnGuideAndEdge => format!("{} × {}", t!("infer.guide"), t!("infer.edge").to_lowercase()),
            InferKind::OnTwoGuides => format!("{} × {}", t!("infer.guide"), t!("infer.line.guide")),
            InferKind::OnLine(l) => l.on_label(),
            InferKind::OnGuide => t!("infer.on_guide"),
            InferKind::OnEdge => t!("infer.on_edge"),
            InferKind::OnFace => t!("infer.on_face"),
            InferKind::OnPlane => String::new(),
        }
    }
}

/// The edge a point was inferred on, as a reference for the next lines (world, unit vectors).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeRef {
    /// A straight edge's direction.
    Straight(DVec3),
    /// An arc where the point is: its tangent there, and its radius (from the centre out);
    /// its centre and radius length (a concentric guide's).
    Arc { tangent: DVec3, radial: DVec3, center: DVec3, radius: f64 },
}

/// A curve of the scene a point lies on, as it is drawn (world): what an [`EdgeRef`] is taken
/// from, shown when a tool refers to it (the tape's guide offset from it).
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// A straight edge, end to end.
    Segment(DVec3, DVec3),
    /// An arc (or an ellipse's) as a polyline.
    Arc(Vec<DVec3>),
    /// A guide: drawn as it is, its dashes where they are.
    Guide(GuideShape),
    /// An axis as drawn: the line through `origin` along `dir` (unit).
    Axis { origin: DVec3, dir: DVec3 },
}

/// The point inferred under the cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct Inference {
    /// World point.
    pub point: DVec3,
    pub kind: InferKind,
    /// The solid the point belongs to (none for the origin, a line and the fallback plane).
    pub path: Option<EntityPath>,
    /// What the cursor's ray hits, whatever was inferred: tools orient themselves on its face.
    pub hit: Option<Hit>,
    /// For a point on a line: the line (its origin, unit direction).
    pub line: Option<(DVec3, DVec3)>,
    /// For a point where two lines cross: the second.
    pub line2: Option<(DVec3, DVec3)>,
    /// For a point of a straight edge or of an arc: the edge there (the next parallel,
    /// perpendicular or tangent inference refers to it).
    pub edge: Option<EdgeRef>,
    /// The curve `edge` is taken from, as drawn (an edge, a guide, an axis).
    pub curve: Option<Curve>,
    /// For a point where two curves cross — edges, guides, the axes as drawn —: each of them
    /// there, as `edge` would give it, with the curve itself (the tape's guide from a crossing
    /// picks one). Not a reference for the next lines: `edge` stays empty.
    pub crossed: Vec<(EdgeRef, Curve)>,
}

impl Inference {
    /// A point on no line: of the geometry, of a face or of the fallback plane.
    fn at(point: DVec3, kind: InferKind, path: Option<EntityPath>, hit: Option<Hit>, edge: Option<(EdgeRef, Curve)>) -> Self {
        let (edge, curve) = edge.unzip();
        Inference { point, kind, path, hit, line: None, line2: None, edge, curve, crossed: Vec::new() }
    }

    /// A point of the box of what the tool takes, `hit` being what the ray hits.
    pub(crate) fn on_box(point: DVec3, kind: InferKind, hit: Option<Hit>) -> Self {
        Self::at(point, kind, None, hit, None)
    }

    /// A point on the line `a`, and on `b` where two lines cross (those fixed then crossed).
    fn on_lines(point: DVec3, kind: InferKind, path: Option<EntityPath>, hit: Option<Hit>, a: &Line, b: Option<&Line>) -> Self {
        let crossed = b.map_or_else(Vec::new, |b| [a, b].iter().filter_map(|l| l.as_ref()).collect());
        let (line, line2) = (Some((a.origin, a.dir)), b.map(|b| (b.origin, b.dir)));
        Inference { point, kind, path, hit, line, line2, edge: None, curve: None, crossed }
    }
}

/// A lock: the point stays on this line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lock {
    /// Where the line passes; `None`: the tool's start point.
    pub origin: Option<DVec3>,
    /// Unit direction.
    pub dir: DVec3,
    pub line: LineKind,
}

/// What a tool asks for.
#[derive(Clone, Copy)]
pub struct Query<'a> {
    /// The geometry to infer on (and that may hide others), e.g. all but what is moving.
    pub keep: &'a dyn Fn(&Owner<'_>) -> bool,
    /// Where the point falls when nothing is under the cursor: world point and normal.
    pub plane: (DVec3, DVec3),
    /// A point offered as [`InferKind::Origin`] (world).
    pub origin: Option<DVec3>,
    /// The axes are drawn: their lines through `origin` are inferred on.
    pub model_axes: bool,
    /// The tool's start point: lines are inferred through it.
    pub from: Option<DVec3>,
    /// Points tracked (hovered a while): their axes are inferred too.
    pub tracked: &'a [DVec3],
    /// Only lines lying in `plane` (the rotate tool's protractor).
    pub in_plane: bool,
    /// The context's axes, in the world.
    pub axes: [DVec3; 3],
    /// The last edge hovered (straight or arc).
    pub reference: Option<EdgeRef>,
    /// Keeps the point on this line.
    pub lock: Option<Lock>,
    /// A line the tool draws itself (origin, unit direction; world): inferred on as the axes
    /// as drawn, its origin as an endpoint.
    pub tool_line: Option<(DVec3, DVec3)>,
    /// The corners of the box of what the tool takes (world), offered as [`InferKind::BoxCorner`].
    pub box_corners: &'a [DVec3],
}

/// The point inferred at `px`; `None` only when nothing is under the cursor and the ray
/// misses the fallback plane.
pub fn infer(scene: &DisplayScene, camera: &Camera, size: DVec2, px: DVec2, q: &Query<'_>) -> Option<Inference> {
    let base = infer_points(scene, camera, size, px, q);
    let ray = camera.ray(px, size);
    let screen = camera.projector(size);
    let hit = base.as_ref().and_then(|b| b.hit.clone());

    if let Some(lock) = q.lock
        && let Some(origin) = lock.origin.or(q.from)
    {
        let l = Line { origin, dir: lock.dir, kind: lock.line, source: 0 };
        // An edge the locked line crosses near the cursor: the crossing.
        if let Some(c) = crossing(scene, camera, size, &screen, px, q, &[l]) {
            return Some(c.inference(&l, hit));
        }
        let point = match &base {
            // A feature hovered: the point of the line nearest to it.
            Some(b) if b.kind.is_feature() => origin + l.dir * (b.point - origin).dot(l.dir),
            _ => on_line(&ray, &screen, px, &l, hit.as_ref()),
        };
        return Some(Inference::on_lines(point, InferKind::OnLine(l.kind), None, hit, &l, None));
    }
    // Points of the geometry beat what lines give; a third only ranks with the points they build.
    if base.as_ref().is_some_and(|b| b.kind.is_point() && b.kind != InferKind::Third) {
        return base;
    }
    // A point built by the geometry (a third, a guide circle across an edge or another): as good as one built by
    // lines, the nearer on screen wins; better than any point on a line.
    let built = base.as_ref().filter(|b| b.kind.rank() == InferKind::OnGuideAndEdge.rank());
    let built_dist = built.and_then(|b| screen.project(b.point)).map_or(f64::INFINITY, |s| s.distance(px));
    // A line seen nearly end on is a point on screen: its "nearest point" runs off anywhere.
    let lines: Vec<Line> = lines(q, &kept_guides(scene, q)).into_iter().filter(|l| l.dir.dot(ray.dir).abs() <= 0.995).collect();
    if lines.is_empty() {
        return base;
    }

    // Points built from two things: a line and an edge, or two lines.
    let edge = crossing(scene, camera, size, &screen, px, q, &lines);
    let two = two_lines(camera, size, &screen, px, &lines);
    let edge_dist = edge.as_ref().and_then(|c| screen.project(c.point)).map(|s| s.distance(px));
    let meet = |p, a: &Line, b: &Line, hit| Inference::on_lines(p, InferKind::OnTwoLines(a.kind, b.kind), None, hit, a, Some(b));
    match (edge, two) {
        (Some(_), Some((d2, p, i, j))) if d2 < edge_dist.unwrap_or(f64::INFINITY) && d2 < built_dist => {
            return Some(meet(p, &lines[i], &lines[j], hit));
        }
        (Some(c), _) if edge_dist.unwrap_or(f64::INFINITY) < built_dist => {
            let l = lines[c.line];
            return Some(c.inference(&l, hit));
        }
        (None, Some((d2, p, i, j))) if d2 < built_dist => return Some(meet(p, &lines[i], &lines[j], hit)),
        _ => {}
    }
    if built.is_some() {
        return base;
    }

    let mut best: Option<(f64, DVec3, usize)> = None;
    for (k, l) in lines.iter().enumerate() {
        let p = on_line(&ray, &screen, px, l, hit.as_ref());
        let Some(s) = screen.project(p) else { continue };
        if l.kind.is_fixed() {
            // Drawn depth-tested: not inferred where it is hidden.
            if !visible(scene, camera, size, &screen, p, q) {
                continue;
            }
        } else if screen.project(l.origin).is_some_and(|o| o.distance(s) < APERTURE_PX) {
            // Every line passes through its origin: there, none means anything.
            continue;
        }
        let dist = s.distance(px);
        if dist <= APERTURE_PX && best.as_ref().is_none_or(|b| dist < b.0) {
            best = Some((dist, p, k));
        }
    }
    match best {
        Some((_, p, k)) => {
            let l = &lines[k];
            let mut i = Inference::on_lines(p, InferKind::OnLine(l.kind), None, hit, l, None);
            // A guide or an axis is an edge to refer to (parallel, perpendicular; a guide offset
            // from it).
            if let Some((edge, curve)) = l.as_ref() {
                (i.edge, i.curve) = (Some(edge), Some(curve));
            }
            Some(i)
        }
        None => base,
    }
}

/// Can the bounds `b` (world) hold a feature within the aperture of the ray?
fn near_ray(camera: &Camera, size: DVec2, ray: &Ray, (lo, hi): (DVec3, DVec3)) -> bool {
    // A pixel's size grows with depth: take it at the farthest corner.
    let wpp = (0..8)
        .map(|i| {
            let c = DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            camera.world_per_pixel(size, c)
        })
        .fold(0.0, f64::max);
    let pad = DVec3::splat(wpp * APERTURE_PX * 1.5);
    ray_box(ray.origin, ray.dir, (lo - pad, hi + pad), ray.t_min)
}

/// Is `p` seen from the camera: not cut away by the section, and nothing before it on its
/// own ray — what is drawn see-through hides nothing ([`DisplayScene::see_through`])?
fn visible(scene: &DisplayScene, camera: &Camera, size: DVec2, screen: &Projector, p: DVec3, q: &Query<'_>) -> bool {
    if scene.section().is_some_and(|s| s.cuts(p)) {
        return false;
    }
    let Some(s) = screen.project(p) else { return false };
    let ray = camera.ray(s, size);
    let depth = (p - ray.origin).dot(ray.dir);
    match scene.pick_inferable(&ray, |o| (q.keep)(o) && !scene.see_through(o.piece)) {
        None => true,
        // Points on the surface hit are seen: allow a few pixels (a mesh only approximates
        // curved faces) and a hair for rounding.
        Some(h) => h.t >= depth - camera.world_per_pixel(size, p) * 3.0 - 1e-9 * p.length().max(1.0),
    }
}

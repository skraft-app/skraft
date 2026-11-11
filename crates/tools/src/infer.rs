//! Inferences (docs/ARCHITECTURE.md §6 ter): the point a tool takes under the cursor.
//!
//! Candidates come from the solids' exact snap features ([`Snaps`]), placed in the world by
//! each occurrence: endpoints, midpoints and centres; then — when the tool has a start point —
//! where a line through it crosses an edge, then the lines themselves (the context's red,
//! green and blue axes, parallel or perpendicular to the last straight edge hovered, tangent
//! or radial to the last arc); then points on edges, then the face under the cursor, then a
//! fallback plane. Within an aperture of [`APERTURE_PX`] the best kind wins, then the nearest
//! on screen, then the nearest to the eye; a point hidden behind the scene is skipped.
//!
//! A point hovered a while is **tracked**: its axes are inferred too, and where two lines of
//! different origins cross.
//!
//! A **lock** (an arrow key, or Shift held on a line) keeps the point on one line: a feature
//! hovered is then projected onto it ("aligned with that corner"), an edge it crosses gives
//! the crossing.

use std::f64::consts::TAU;

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_kernel::{SnapCurve, Snaps};
use solidsketch_model::EntityPath;

use crate::camera::{Camera, Projector, Ray};
use crate::scene::{DisplayScene, Hit, Owner, ray_box, transform_bounds};

/// How far from the cursor a feature is inferred (pixels).
pub const APERTURE_PX: f64 = 10.0;

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
    /// The line's name inside a crossing.
    fn name(self) -> String {
        match self {
            LineKind::Axis(k) => t!("infer.line.axis", axis = axis_name(k)),
            LineKind::TrackedAxis(k) => t!("infer.line.tracked_axis", axis = axis_name(k)),
            LineKind::Parallel => t!("infer.line.parallel"),
            LineKind::Perpendicular => t!("infer.line.perpendicular"),
            LineKind::Tangent => t!("infer.line.tangent"),
            LineKind::Radial => t!("infer.line.radial"),
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
        }
    }

    /// The axis it runs along, if any (its colour).
    pub fn axis(self) -> Option<usize> {
        match self {
            LineKind::Axis(k) | LineKind::TrackedAxis(k) => Some(k),
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
    /// The centre of an arc, a circle or an ellipse.
    Center,
    /// The origin of the context being edited.
    Origin,
    /// Where a line crosses an edge (a point built from both).
    OnEdgeAndLine(LineKind),
    /// Where two lines of different origins cross.
    OnTwoLines(LineKind, LineKind),
    /// On a line.
    OnLine(LineKind),
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
            InferKind::Midpoint | InferKind::Center | InferKind::Origin => 1,
            InferKind::OnEdgeAndLine(_) | InferKind::OnTwoLines(..) => 2,
            InferKind::OnLine(_) => 3,
            InferKind::OnEdge => 4,
            InferKind::OnFace => 5,
            InferKind::OnPlane => 6,
        }
    }

    /// A point of the geometry (endpoint, midpoint, centre, origin): what can be tracked.
    pub fn is_point(self) -> bool {
        self.rank() <= 1
    }

    /// A point the geometry or a line gives, not just somewhere on a face or a plane: what a
    /// tool should take as is, even off its working plane.
    pub fn is_feature(self) -> bool {
        self.rank() <= InferKind::OnEdge.rank()
    }

    /// The (first) line the point is on, if any.
    pub fn line(self) -> Option<LineKind> {
        match self {
            InferKind::OnLine(l) | InferKind::OnEdgeAndLine(l) | InferKind::OnTwoLines(l, _) => Some(l),
            _ => None,
        }
    }

    /// The tooltip (empty: none).
    pub fn label(self) -> String {
        match self {
            InferKind::Endpoint => t!("infer.endpoint"),
            InferKind::Midpoint => t!("infer.midpoint"),
            InferKind::Center => t!("infer.center"),
            InferKind::Origin => t!("infer.origin"),
            InferKind::OnEdgeAndLine(l) => format!("{} × {}", t!("infer.edge"), l.name()),
            InferKind::OnTwoLines(a, b) => {
                let s = format!("{} × {}", a.name(), b.name());
                let mut c = s.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
            }
            InferKind::OnLine(l) => l.on_label(),
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
    /// An arc where the point is: its tangent there, and its radius (from the centre out).
    Arc { tangent: DVec3, radial: DVec3 },
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
}

impl Inference {
    /// A point on no line: of the geometry, of a face or of the fallback plane.
    fn at(point: DVec3, kind: InferKind, path: Option<EntityPath>, hit: Option<Hit>, edge: Option<EdgeRef>) -> Self {
        Inference { point, kind, path, hit, line: None, line2: None, edge }
    }

    /// A point on the line `a`, and on `b` where two lines cross.
    fn on_lines(point: DVec3, kind: InferKind, path: Option<EntityPath>, hit: Option<Hit>, a: &Line, b: Option<&Line>) -> Self {
        Inference { point, kind, path, hit, line: Some((a.origin, a.dir)), line2: b.map(|b| (b.origin, b.dir)), edge: None }
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
}

struct Candidate {
    point: DVec3,
    kind: InferKind,
    path: Option<EntityPath>,
    edge: Option<EdgeRef>,
    /// Screen distance to the cursor (pixels).
    dist: f64,
    /// Depth along the cursor's ray.
    depth: f64,
}

/// A line inferred on.
#[derive(Clone, Copy, Debug)]
struct Line {
    origin: DVec3,
    /// Unit.
    dir: DVec3,
    kind: LineKind,
    /// 0 for the start point, 1 + i for tracked point i: lines of one origin cross only there.
    source: usize,
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
            return Some(Inference::on_lines(c.point, InferKind::OnEdgeAndLine(l.kind), c.path, hit, &l, None));
        }
        let point = match &base {
            // A feature hovered: the point of the line nearest to it.
            Some(b) if b.kind.is_feature() => origin + l.dir * (b.point - origin).dot(l.dir),
            _ => on_line(&ray, &screen, px, &l, hit.as_ref()),
        };
        return Some(Inference::on_lines(point, InferKind::OnLine(l.kind), None, hit, &l, None));
    }
    // Points of the geometry beat what lines give.
    if base.as_ref().is_some_and(|b| b.kind.is_point()) {
        return base;
    }
    // A line seen nearly end on is a point on screen: its "nearest point" runs off anywhere.
    let lines: Vec<Line> = lines(q).into_iter().filter(|l| l.dir.dot(ray.dir).abs() <= 0.995).collect();
    if lines.is_empty() {
        return base;
    }

    // Points built from two things: a line and an edge, or two lines.
    let edge = crossing(scene, camera, size, &screen, px, q, &lines);
    let two = two_lines(camera, size, &screen, px, &lines);
    let edge_dist = edge.as_ref().and_then(|c| screen.project(c.point)).map(|s| s.distance(px));
    let meet = |p, a: &Line, b: &Line, hit| Inference::on_lines(p, InferKind::OnTwoLines(a.kind, b.kind), None, hit, a, Some(b));
    match (edge, two) {
        (Some(_), Some((d2, p, i, j))) if d2 < edge_dist.unwrap_or(f64::INFINITY) => {
            return Some(meet(p, &lines[i], &lines[j], hit));
        }
        (Some(c), _) => {
            let l = &lines[c.line];
            return Some(Inference::on_lines(c.point, InferKind::OnEdgeAndLine(l.kind), c.path, hit, l, None));
        }
        (None, Some((_, p, i, j))) => return Some(meet(p, &lines[i], &lines[j], hit)),
        (None, None) => {}
    }

    let mut best: Option<(f64, DVec3, usize)> = None;
    for (k, l) in lines.iter().enumerate() {
        let p = on_line(&ray, &screen, px, l, hit.as_ref());
        let Some(s) = screen.project(p) else { continue };
        // Every line passes through its origin: there, none means anything.
        if screen.project(l.origin).is_some_and(|o| o.distance(s) < APERTURE_PX) {
            continue;
        }
        let dist = s.distance(px);
        if dist <= APERTURE_PX && best.as_ref().is_none_or(|b| dist < b.0) {
            best = Some((dist, p, k));
        }
    }
    match best {
        Some((_, p, k)) => Some(Inference::on_lines(p, InferKind::OnLine(lines[k].kind), None, hit, &lines[k], None)),
        None => base,
    }
}

/// The lines inferred on: through the start point, the axes, then those of the reference edge
/// when not an axis already (parallel and perpendicular — in the tool's plane — to a straight
/// edge, tangent and radial to an arc); through each tracked point, the axes.
fn lines(q: &Query<'_>) -> Vec<Line> {
    let mut out = Vec::new();
    let known = |d: DVec3| q.axes.iter().any(|a| a.dot(d).abs() > 1.0 - 1e-9);
    if let Some(from) = q.from {
        for k in 0..3 {
            out.push(Line { origin: from, dir: q.axes[k], kind: LineKind::Axis(k), source: 0 });
        }
        let refs = match q.reference {
            Some(EdgeRef::Straight(r)) => [(Some(r), LineKind::Parallel), (q.plane.1.cross(r).try_normalize(), LineKind::Perpendicular)],
            Some(EdgeRef::Arc { tangent, radial }) => [(Some(tangent), LineKind::Tangent), (Some(radial), LineKind::Radial)],
            None => [(None, LineKind::Parallel), (None, LineKind::Perpendicular)],
        };
        for (d, kind) in refs {
            if let Some(dir) = d.filter(|d| !known(*d)) {
                out.push(Line { origin: from, dir, kind, source: 0 });
            }
        }
    }
    for (i, t) in q.tracked.iter().enumerate() {
        // The start point itself needs no tracking.
        if q.from.is_some_and(|f| (f - *t).length() < 1e-9) {
            continue;
        }
        for k in 0..3 {
            out.push(Line { origin: *t, dir: q.axes[k], kind: LineKind::TrackedAxis(k), source: 1 + i });
        }
    }
    if q.in_plane {
        out.retain(|l| l.dir.dot(q.plane.1).abs() < 1e-9);
    }
    out
}

/// The point of the line the cursor gives: where it pierces the face under the cursor if that
/// is near the cursor, else the point nearest the cursor's ray.
fn on_line(ray: &Ray, screen: &Projector, px: DVec2, l: &Line, hit: Option<&Hit>) -> DVec3 {
    if let Some(h) = hit {
        let d = l.dir.dot(h.normal);
        if d.abs() > 1e-9 {
            let p = l.origin + l.dir * ((h.point - l.origin).dot(h.normal) / d);
            if screen.project(p).is_some_and(|s| s.distance(px) <= APERTURE_PX) {
                return p;
            }
        }
    }
    ray.closest_on_line(l.origin, l.dir)
}

/// Where two lines of different origins cross near the cursor (within half a pixel of each
/// other at that depth): (screen distance, the point on the first line, both indices). The
/// first line is the start point's when one is: its point stays exactly on it.
fn two_lines(camera: &Camera, size: DVec2, screen: &Projector, px: DVec2, lines: &[Line]) -> Option<(f64, DVec3, usize, usize)> {
    let mut best: Option<(f64, DVec3, usize, usize)> = None;
    for (i, a) in lines.iter().enumerate() {
        for (j, b) in lines.iter().enumerate().skip(i + 1) {
            if a.source == b.source {
                continue;
            }
            let tol = |p: DVec3| camera.world_per_pixel(size, p) * CROSSING_PX;
            let Some(p) = lines_meet(a, b, tol) else { continue };
            // Where one line passes through the other's origin, that point is all they give.
            if [a.origin, b.origin].iter().any(|o| (p - *o).length() <= tol(p)) {
                continue;
            }
            let Some(s) = screen.project(p) else { continue };
            let dist = s.distance(px);
            if dist <= APERTURE_PX && best.as_ref().is_none_or(|x| dist < x.0) {
                best = Some((dist, p, i, j));
            }
        }
    }
    best
}

/// Where the lines `a` and `b` (not parallel) pass within `tol` of each other: the point of `a`.
fn lines_meet(a: &Line, b: &Line, tol: impl Fn(DVec3) -> f64) -> Option<DVec3> {
    let w = a.origin - b.origin;
    let bb = a.dir.dot(b.dir);
    let denom = 1.0 - bb * bb;
    if denom < 1e-12 {
        return None;
    }
    let (d, e) = (a.dir.dot(w), b.dir.dot(w));
    let s = (bb * e - d) / denom;
    let t = (e - bb * d) / denom;
    let (p, q) = (a.origin + a.dir * s, b.origin + b.dir * t);
    ((p - q).length() <= tol(p)).then_some(p)
}

/// A line crossing an edge near the cursor.
struct Crossing {
    /// On the line, exactly.
    point: DVec3,
    /// Index of the line in the list given.
    line: usize,
    /// The solid whose edge it is.
    path: Option<EntityPath>,
}

/// The visible crossing nearest the cursor (within the aperture) of one of `lines` with an
/// edge of the scene; the point is taken on the line.
fn crossing(
    scene: &DisplayScene,
    camera: &Camera,
    size: DVec2,
    screen: &Projector,
    px: DVec2,
    q: &Query<'_>,
    lines: &[Line],
) -> Option<Crossing> {
    let ray = camera.ray(px, size);
    let mut found: Vec<(f64, Crossing)> = Vec::new();
    for (place, part, piece) in scene.inferable() {
        let Some(b) = part.bounds else { continue };
        let path = place.entity_path();
        if !(q.keep)(&Owner { path: &path, piece }) || !near_ray(camera, size, &ray, transform_bounds(&place.transform, b)) {
            continue;
        }
        let t = &place.transform;
        for e in &part.snaps.edges {
            for (k, l) in lines.iter().enumerate() {
                let tol = |p: DVec3| camera.world_per_pixel(size, p) * CROSSING_PX;
                for p in line_meets_curve(l.origin, l.dir, &e.curve, t, tol) {
                    // A line meets the edge its origin lies on there: no crossing.
                    if (p - l.origin).length() <= tol(p) {
                        continue;
                    }
                    let Some(s) = screen.project(p) else { continue };
                    let dist = s.distance(px);
                    if dist <= APERTURE_PX {
                        found.push((dist, Crossing { point: p, line: k, path: Some(path.clone()) }));
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, c)| c).find(|c| visible(scene, camera, size, screen, c.point, q))
}

/// Where the line `from + s·dir` (unit `dir`) meets `curve` placed by `t`, within `tol(p)` of
/// it: points of the line.
fn line_meets_curve(from: DVec3, dir: DVec3, curve: &SnapCurve, t: &DAffine3, tol: impl Fn(DVec3) -> f64) -> Vec<DVec3> {
    match *curve {
        SnapCurve::Segment { a, b } => {
            line_meets_segment(from, dir, t.transform_point3(a), t.transform_point3(b), &tol).into_iter().collect()
        }
        SnapCurve::Arc { center, x, y, radius, sweep } => {
            let arc = PlacedArc::new(t, center, x, y, radius, sweep);
            if !arc.is_circle() {
                // Unevenly scaled: an ellipse. Its samples will do.
                return arc.samples().windows(2).filter_map(|w| line_meets_segment(from, dir, w[0], w[1], &tol)).collect();
            }
            let PlacedArc { c, rx, n, .. } = arc;
            let on_arc = |p: DVec3| {
                let a = arc.angle_of(p);
                let slack = tol(p) / rx;
                a <= sweep + slack || a >= TAU - slack
            };
            let dn = dir.dot(n);
            if dn.abs() > 1e-9 {
                // Through the arc's plane: one point, on the circle or not.
                let p = from + dir * ((c - from).dot(n) / dn);
                return (((p - c).length() - rx).abs() <= tol(p) && on_arc(p)).then_some(p).into_iter().collect();
            }
            // In the arc's plane (or beside it): up to two points.
            let w = from - c;
            if w.dot(n).abs() > tol(c) {
                return vec![];
            }
            let (bh, cc) = (dir.dot(w), w.dot(w) - rx * rx);
            let disc = bh * bh - cc;
            if disc < 0.0 {
                return vec![];
            }
            let r = disc.sqrt();
            [-bh - r, -bh + r].into_iter().map(|s| from + dir * s).filter(|p| on_arc(*p)).collect()
        }
        SnapCurve::Polyline { ref points, .. } => {
            let pts: Vec<DVec3> = points.iter().map(|p| t.transform_point3(*p)).collect();
            pts.windows(2).filter_map(|w| line_meets_segment(from, dir, w[0], w[1], &tol)).collect()
        }
    }
}

/// Where the line `from + s·dir` (unit `dir`) passes within `tol` of the segment `[a, b]`
/// (not parallel to it): the line's point there.
fn line_meets_segment(from: DVec3, dir: DVec3, a: DVec3, b: DVec3, tol: &impl Fn(DVec3) -> f64) -> Option<DVec3> {
    let u = b - a;
    let c = u.dot(u);
    if c < 1e-24 {
        return None;
    }
    let w = from - a;
    let (bb, d, e) = (dir.dot(u), dir.dot(w), u.dot(w));
    let denom = c - bb * bb; // |u|² sin²: zero when parallel
    if denom <= 1e-12 * c {
        return None;
    }
    let s = (bb * e - c * d) / denom;
    let tt = (e - bb * d) / denom;
    let p = from + dir * s;
    let slack = tol(p) / c.sqrt();
    if !(-slack..=1.0 + slack).contains(&tt) {
        return None;
    }
    ((p - (a + u * tt.clamp(0.0, 1.0))).length() <= tol(p)).then_some(p)
}

/// The points of the geometry under `px` (no lines), else the face, else the plane.
fn infer_points(scene: &DisplayScene, camera: &Camera, size: DVec2, px: DVec2, q: &Query<'_>) -> Option<Inference> {
    let ray = camera.ray(px, size);
    let screen = camera.projector(size);
    let hit = scene.pick_inferable(&ray, q.keep);

    let mut cands = Vec::new();
    let mut offer = |point: DVec3, kind: InferKind, path: Option<&EntityPath>, edge: Option<EdgeRef>| {
        let Some(s) = screen.project(point) else { return };
        let dist = (s - px).length();
        if dist <= APERTURE_PX {
            let depth = (point - ray.origin).dot(ray.dir);
            cands.push(Candidate { point, kind, path: path.cloned(), edge, dist, depth });
        }
    };

    if let Some(o) = q.origin {
        offer(o, InferKind::Origin, None, None);
    }
    for (place, part, piece) in scene.inferable() {
        let Some(b) = part.bounds else { continue };
        let path = place.entity_path();
        if !(q.keep)(&Owner { path: &path, piece }) || !near_ray(camera, size, &ray, transform_bounds(&place.transform, b)) {
            continue;
        }
        let t = &place.transform;
        features(&part.snaps, t, &ray, &screen, px, &mut |p, k, e| offer(p, k, Some(&path), e));
    }

    // Best kind first; within a kind, those within a pixel of the nearest are equals (features
    // stacked on screen, as a top and a bottom corner seen from above): the nearest to the eye.
    let mut nearest = [f64::INFINITY; RANKS];
    for c in &cands {
        let r = c.kind.rank();
        nearest[r] = nearest[r].min(c.dist);
    }
    let key = |c: &Candidate| {
        let close = c.dist <= nearest[c.kind.rank()] + 1.0;
        (c.kind.rank(), !close, if close { c.depth } else { c.dist })
    };
    cands.sort_by(|a, b| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal));
    for c in cands {
        if visible(scene, camera, size, &screen, c.point, q) {
            return Some(Inference::at(c.point, c.kind, c.path, hit, c.edge));
        }
    }

    if let Some(h) = hit {
        // On a planar face, the point is put exactly on its plane.
        let point = scene
            .inferable_at(&h)
            .and_then(|(t, part)| {
                let plane = part.snaps.planes.get(h.face as usize).copied().flatten()?;
                Some(t.transform_point3(plane.project(t.inverse().transform_point3(h.point))))
            })
            .unwrap_or(h.point);
        let path = Some(h.path.clone());
        return Some(Inference::at(point, InferKind::OnFace, path, Some(h), None));
    }
    let point = ray.hit_plane(q.plane.0, q.plane.1)?;
    Some(Inference::at(point, InferKind::OnPlane, None, None, None))
}

/// Offers the snap features of one occurrence (definition to world `t`): its vertices, the
/// midpoints and centres of its edges, and the point of each edge nearest the cursor (with the
/// edge there, straight or arc, as the next reference).
fn features(s: &Snaps, t: &DAffine3, ray: &Ray, screen: &Projector, px: DVec2, offer: &mut impl FnMut(DVec3, InferKind, Option<EdgeRef>)) {
    for v in &s.vertices {
        offer(t.transform_point3(*v), InferKind::Endpoint, None);
    }
    for e in &s.edges {
        // The edge at a point of it (world), as a reference.
        let edge_at = |p: DVec3| match e.curve {
            SnapCurve::Segment { a, b } => t.transform_vector3(b - a).try_normalize().map(EdgeRef::Straight),
            SnapCurve::Arc { center, x, y, radius, sweep } => {
                let PlacedArc { c, n, .. } = PlacedArc::new(t, center, x, y, radius, sweep);
                let d = p - c;
                let radial = (d - n * d.dot(n)).try_normalize()?;
                Some(EdgeRef::Arc { tangent: n.cross(radial), radial })
            }
            SnapCurve::Polyline { .. } => None,
        };
        if let Some(m) = e.curve.midpoint() {
            let m = t.transform_point3(m);
            offer(m, InferKind::Midpoint, edge_at(m));
        }
        if let Some(c) = e.curve.center() {
            offer(t.transform_point3(c), InferKind::Center, None);
        }
        if let Some(p) = on_curve(&e.curve, t, ray, screen, px) {
            offer(p, InferKind::OnEdge, edge_at(p));
        }
    }
}

/// The point of `curve` (placed by `t`) nearest the cursor, in the world.
fn on_curve(curve: &SnapCurve, t: &DAffine3, ray: &Ray, screen: &Projector, px: DVec2) -> Option<DVec3> {
    match *curve {
        SnapCurve::Segment { a, b } => Some(on_segment(ray, t.transform_point3(a), t.transform_point3(b))),
        SnapCurve::Arc { center, x, y, radius, sweep } => {
            let arc = PlacedArc::new(t, center, x, y, radius, sweep);
            let dn = ray.dir.dot(arc.n);
            if dn.abs() > 1e-3 && arc.is_circle() {
                // Seen from the side of its plane: where the ray's line meets the plane (a parallel
                // projection may start past it: the line, not the half-line), radially.
                let q = ray.origin + ray.dir * ((arc.c - ray.origin).dot(arc.n) / dn);
                let mut a = arc.angle_of(q);
                if a > sweep {
                    // Past the arc's end: the nearer end.
                    a = if a - sweep < TAU - a { sweep } else { 0.0 };
                }
                return Some(arc.at(a));
            }
            // Edge on (or unevenly scaled): sample it.
            on_polyline(&arc.samples(), ray, screen, px)
        }
        SnapCurve::Polyline { ref points, .. } => {
            let pts: Vec<DVec3> = points.iter().map(|p| t.transform_point3(*p)).collect();
            on_polyline(&pts, ray, screen, px)
        }
    }
}

/// An arc placed in the world: a placement may scale it, unevenly into an ellipse.
#[derive(Clone, Copy)]
struct PlacedArc {
    c: DVec3,
    /// Unit axes of the arc's plane, from the placed `x` and `y`.
    x: DVec3,
    y: DVec3,
    /// Unit normal.
    n: DVec3,
    /// Radius along each axis.
    rx: f64,
    ry: f64,
    sweep: f64,
}

impl PlacedArc {
    /// How many chords stand for an arc that has to be sampled.
    const SAMPLES: usize = 64;

    fn new(t: &DAffine3, center: DVec3, x: DVec3, y: DVec3, radius: f64, sweep: f64) -> Self {
        let (xw, yw) = (t.transform_vector3(x), t.transform_vector3(y));
        let (x, y) = (xw.normalize(), yw.normalize());
        PlacedArc { c: t.transform_point3(center), x, y, n: x.cross(y), rx: xw.length() * radius, ry: yw.length() * radius, sweep }
    }

    /// Still a circle (evenly scaled).
    fn is_circle(&self) -> bool {
        (self.rx - self.ry).abs() <= 1e-9 * self.rx.max(1.0)
    }

    /// The point at angle `a`.
    fn at(&self, a: f64) -> DVec3 {
        self.c + self.x * (self.rx * a.cos()) + self.y * (self.ry * a.sin())
    }

    /// The angle of `p` around the centre, in [0, 2π).
    fn angle_of(&self, p: DVec3) -> f64 {
        let d = p - self.c;
        let a = d.dot(self.y).atan2(d.dot(self.x));
        if a < 0.0 { a + TAU } else { a }
    }

    /// [`Self::SAMPLES`] + 1 points along it, ends included.
    fn samples(&self) -> Vec<DVec3> {
        (0..=Self::SAMPLES).map(|i| self.at(self.sweep * i as f64 / Self::SAMPLES as f64)).collect()
    }
}

fn on_polyline(pts: &[DVec3], ray: &Ray, screen: &Projector, px: DVec2) -> Option<DVec3> {
    pts.windows(2)
        .map(|w| on_segment(ray, w[0], w[1]))
        .filter_map(|p| Some((screen.project(p)?.distance(px), p)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
}

/// The point of the segment `[a, b]` nearest the ray's line.
fn on_segment(ray: &Ray, a: DVec3, b: DVec3) -> DVec3 {
    let u = b - a;
    let len2 = u.length_squared();
    if len2 < 1e-24 {
        return a;
    }
    let w = a - ray.origin;
    let (du, dw, uw) = (ray.dir.dot(u), ray.dir.dot(w), u.dot(w));
    let denom = len2 - du * du; // |u|² (1 - cos²): zero when parallel to the ray
    let s = if denom.abs() < 1e-12 * len2 { 0.0 } else { (du * dw - uw) / denom };
    a + u * s.clamp(0.0, 1.0)
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
/// own ray?
fn visible(scene: &DisplayScene, camera: &Camera, size: DVec2, screen: &Projector, p: DVec3, q: &Query<'_>) -> bool {
    if scene.section().is_some_and(|s| s.cuts(p)) {
        return false;
    }
    let Some(s) = screen.project(p) else { return false };
    let ray = camera.ray(s, size);
    let depth = (p - ray.origin).dot(ray.dir);
    match scene.pick_inferable(&ray, q.keep) {
        None => true,
        // Points on the surface hit are seen: allow a few pixels (a mesh only approximates
        // curved faces) and a hair for rounding.
        Some(h) => h.t >= depth - camera.world_per_pixel(size, p) * 3.0 - 1e-9 * p.length().max(1.0),
    }
}

#[cfg(test)]
mod tests {
    use glam::{DAffine3, DVec2, DVec3};
    use solidsketch_model::{Document, EntityPath, Id, Operation, Primitive};

    use super::*;
    use crate::{Editor, Modifiers, StandardView, ToolInput, ToolKind, ViewArea};

    /// An editor looking straight down (parallel) at ±1 m around `at`.
    fn top_editor(at: DVec3) -> Editor {
        view_editor(StandardView::Top, at)
    }

    fn view_editor(view: StandardView, at: DVec3) -> Editor {
        let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Inline::default()));
        e.set_viewport_size(DVec2::new(1000.0, 1000.0));
        e.set_parallel(true);
        let b = (at - DVec3::splat(1000.0), at + DVec3::splat(1000.0));
        e.camera.set_standard_view(view, Some(b), ViewArea::full(e.viewport_size()));
        e
    }

    fn add(e: &mut Editor, primitive: Primitive) -> Id {
        let root = e.document().root();
        e.apply(Operation::AddPrimitive { context: root, primitive, placement: glam::DAffine3::IDENTITY }).unwrap().expect("done").created
            [0]
    }

    fn add_box(e: &mut Editor, origin: DVec3, size: DVec3) -> Id {
        add(e, Primitive::Box { origin, size })
    }

    fn px(e: &Editor, p: DVec3) -> DVec2 {
        e.camera.project(p, e.viewport_size()).unwrap()
    }

    /// What is inferred with the cursor at `px`, with lines through `from`.
    fn at_px_from(e: &Editor, px: DVec2, from: DVec3, reference: Option<EdgeRef>, lock: Option<(DVec3, LineKind)>) -> Inference {
        let lock = lock.map(|(dir, line)| Lock { origin: None, dir, line });
        let keep = |_: &Owner<'_>| true;
        let q = Query {
            keep: &keep,
            plane: (from, DVec3::Z),
            origin: Some(DVec3::ZERO),
            from: Some(from),
            tracked: &[],
            in_plane: false,
            axes: [DVec3::X, DVec3::Y, DVec3::Z],
            reference,
            lock,
        };
        infer(e.scene(), &e.camera, e.viewport_size(), px, &q).unwrap()
    }

    fn at_from(e: &Editor, p: DVec3, from: DVec3, reference: Option<EdgeRef>, lock: Option<(DVec3, LineKind)>) -> Inference {
        at_px_from(e, px(e, p), from, reference, lock)
    }

    fn near(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-6
    }

    /// For points the cursor gives (pixel → ray → line rounds to about 1e-6 here); features
    /// inferred are exact.
    fn about(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-4
    }

    /// What is inferred with the cursor over the world point `p` (no exclusion, ground z = 0).
    fn at(e: &Editor, p: DVec3) -> Inference {
        let keep = |_: &Owner<'_>| true;
        let q = Query {
            keep: &keep,
            plane: (DVec3::ZERO, DVec3::Z),
            origin: Some(DVec3::ZERO),
            from: None,
            tracked: &[],
            in_plane: false,
            axes: [DVec3::X, DVec3::Y, DVec3::Z],
            reference: None,
            lock: None,
        };
        infer(e.scene(), &e.camera, e.viewport_size(), px(e, p), &q).unwrap()
    }

    #[test]
    fn kinds_on_a_box_seen_from_above() {
        let mut e = top_editor(DVec3::ZERO);
        add_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 150.0));
        // 2 to 3 pixels away from the feature: the feature, exactly.
        let i = at(&e, DVec3::new(305.0, -4.0, 150.0));
        assert_eq!((i.kind, i.point), (InferKind::Endpoint, DVec3::new(300.0, 0.0, 150.0)));
        let i = at(&e, DVec3::new(154.0, -6.0, 150.0));
        assert_eq!((i.kind, i.point), (InferKind::Midpoint, DVec3::new(150.0, 0.0, 150.0)));
        let i = at(&e, DVec3::new(60.0, -4.0, 150.0));
        assert_eq!(i.kind, InferKind::OnEdge);
        assert!((i.point - DVec3::new(60.0, 0.0, 150.0)).length() < 1e-6, "{}", i.point);
        let i = at(&e, DVec3::new(100.0, 100.0, 150.0));
        assert_eq!(i.kind, InferKind::OnFace);
        assert_eq!(i.point.z, 150.0, "exactly on the face's plane");
        let i = at(&e, DVec3::new(-500.0, -500.0, 0.0));
        assert_eq!(i.kind, InferKind::OnPlane);
        assert!(i.point.z.abs() < 1e-9);
        let i = at(&e, DVec3::new(-4.0, 600.0, 0.0));
        assert_eq!(i.kind, InferKind::OnPlane, "the origin is far");
        // The origin, where nothing hides it.
        let mut e = top_editor(DVec3::ZERO);
        add_box(&mut e, DVec3::new(500.0, 500.0, 0.0), DVec3::splat(100.0));
        let i = at(&e, DVec3::new(4.0, -4.0, 0.0));
        assert_eq!((i.kind, i.point), (InferKind::Origin, DVec3::ZERO));
    }

    #[test]
    fn centre_of_a_cylinder() {
        let mut e = top_editor(DVec3::ZERO);
        let c = DVec3::new(200.0, 200.0, 50.0);
        add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 600.0, height: 50.0 });
        let wpp = e.camera.world_per_pixel(e.viewport_size(), c);
        assert!(wpp * APERTURE_PX < 100.0, "{wpp}");
        let i = at(&e, c + DVec3::new(4.0, -3.0, 0.0));
        assert_eq!(i.kind, InferKind::Center);
        assert!((i.point - c).length() < 1e-9, "{}", i.point);
        // Away from the centre and the circle: the top face.
        let d = DVec3::new(1.0, 1.0, 0.0).normalize();
        let i = at(&e, c + d * 300.0);
        assert_eq!(i.kind, InferKind::OnFace);
        // Just outside the circle: a point of the exact circle.
        let i = at(&e, c + d * 603.0);
        assert_eq!(i.kind, InferKind::OnEdge);
        let r = (i.point - c).length();
        assert!((r - 600.0).abs() < 1e-9 && (i.point.z - 50.0).abs() < 1e-9, "{}", i.point);
    }

    #[test]
    fn hidden_features_are_not_inferred() {
        let mut e = top_editor(DVec3::ZERO);
        // A small box under a slab: its top corner is right under the cursor, but hidden.
        add_box(&mut e, DVec3::new(100.0, 100.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
        add_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 400.0, 20.0) + DVec3::new(0.0, 0.0, 100.0));
        let i = at(&e, DVec3::new(100.0, 100.0, 120.0));
        assert_eq!(i.kind, InferKind::OnFace, "{i:?}");
        assert_eq!(i.point.z, 120.0);
    }

    #[test]
    fn exact_in_a_moved_group_and_far_away() {
        for far in [DVec3::ZERO, DVec3::new(1e8, -3e7, 0.0)] {
            let mut e = top_editor(far);
            let a = add_box(&mut e, far, DVec3::new(100.0, 100.0, 37.5));
            e.select(vec![a]);
            e.make_assembly();
            let g = e.selection()[0];
            let root = e.document().root();
            let t = DAffine3::from_translation(DVec3::new(500.0, 250.0, 0.0));
            e.apply(Operation::Transform { context: root, entities: vec![g], transform: t }).unwrap();
            let corner = far + DVec3::new(600.0, 350.0, 37.5);
            let i = at(&e, corner + DVec3::new(4.0, 4.0, 0.0));
            assert_eq!(i.kind, InferKind::Endpoint);
            assert!((i.point - corner).length() < 1e-6, "{} {}", i.point, corner);
            assert_eq!(i.path.unwrap().instances, vec![g]);
        }
    }

    #[test]
    fn move_puts_a_corner_on_a_corner() {
        let mut e = top_editor(DVec3::ZERO);
        let a = add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
        add_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
        e.select(vec![]);
        e.set_tool(ToolKind::Move);
        // Nothing selected and the cursor just off the box: the corner's solid moves.
        let p = px(&e, DVec3::new(104.0, -4.0, 50.0));
        e.input(ToolInput::Hover { px: p });
        e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
        let p = px(&e, DVec3::new(303.0, 5.0, 50.0));
        e.input(ToolInput::Hover { px: p });
        assert_eq!(e.view().inference.map(|i| i.1), Some(InferKind::Endpoint));
        e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
        let (lo, _) = bounds(&e, a);
        assert_eq!(lo, DVec3::new(200.0, 0.0, 0.0));
    }

    #[test]
    fn box_takes_a_corner_and_a_height_from_features() {
        let mut e = top_editor(DVec3::ZERO);
        add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
        e.set_tool(ToolKind::Box);
        let click = |e: &mut Editor, p: DVec3| {
            let p = px(e, p);
            e.input(ToolInput::Hover { px: p });
            e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
        };
        click(&mut e, DVec3::new(300.0, 300.0, 0.0));
        // The opposite corner on the other box's top corner, brought down to the ground.
        click(&mut e, DVec3::new(103.0, 97.0, 50.0));
        // The height of that same corner.
        click(&mut e, DVec3::new(97.0, 103.0, 50.0));
        let id = e.selection()[0];
        // The corners inferred are exact; the first one, clicked on the ground, is not.
        let (lo, hi) = bounds(&e, id);
        assert_eq!(lo, DVec3::new(100.0, 100.0, 0.0));
        assert!((hi - DVec3::new(300.0, 300.0, 50.0)).length() < 1e-4 && hi.z == 50.0, "{hi}");
    }

    #[test]
    fn lines_through_the_start_point() {
        let mut e = top_editor(DVec3::ZERO);
        add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
        let from = DVec3::new(50.0, 50.0, 50.0);
        let i = at_from(&e, DVec3::new(300.0, 53.0, 50.0), from, None, None);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(0)));
        assert!(about(i.point, DVec3::new(300.0, 50.0, 50.0)) && i.point.y == 50.0, "{}", i.point);
        assert_eq!(i.line, Some((from, DVec3::X)));
        let i = at_from(&e, DVec3::new(53.0, -300.0, 50.0), from, None, None);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(1)));
        // Near the start point, no line means anything.
        let i = at_from(&e, DVec3::new(53.0, 52.0, 50.0), from, None, None);
        assert_eq!(i.kind, InferKind::OnFace);
        // Off every line: no line.
        let i = at_from(&e, DVec3::new(300.0, 300.0, 50.0), from, None, None);
        assert_eq!(i.kind, InferKind::OnPlane);

        // Parallel and perpendicular to a diagonal edge hovered before (in the tool's plane).
        let r = DVec3::new(1.0, 2.0, 0.0).normalize();
        let perp = DVec3::Z.cross(r);
        let side = DVec3::new(0.0, 0.0, 0.0) + perp * 3.0;
        let i = at_from(&e, from + r * 400.0 + side, from, Some(EdgeRef::Straight(r)), None);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::Parallel));
        assert!((i.point - from).cross(r).length() < 1e-6);
        let i = at_from(&e, from + perp * 400.0 + r * 3.0, from, Some(EdgeRef::Straight(r)), None);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::Perpendicular));
        // A feature still beats a line.
        let i = at_from(&e, DVec3::new(103.0, 53.0, 50.0), from, None, None);
        assert_eq!(i.kind, InferKind::Midpoint);
    }

    #[test]
    fn a_lock_projects_the_feature_hovered() {
        let mut e = top_editor(DVec3::ZERO);
        add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
        add_box(&mut e, DVec3::new(300.0, 200.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
        let from = DVec3::new(50.0, 50.0, 50.0);
        let lock = Some((DVec3::X, LineKind::Axis(0)));
        // The corner at (300, 200, 50), far from the red line: aligned with it on the line.
        let i = at_from(&e, DVec3::new(302.0, 203.0, 50.0), from, None, lock);
        assert_eq!((i.kind, i.point), (InferKind::OnLine(LineKind::Axis(0)), DVec3::new(300.0, 50.0, 50.0)));
        // Nothing hovered: the line's point nearest the cursor.
        let i = at_from(&e, DVec3::new(-500.0, 600.0, 50.0), from, None, lock);
        assert!(about(i.point, DVec3::new(-500.0, 50.0, 50.0)), "{}", i.point);
    }

    #[test]
    fn a_line_pierces_the_face_under_the_cursor() {
        let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
        add_box(&mut e, DVec3::new(300.0, -200.0, 0.0), DVec3::new(20.0, 500.0, 300.0));
        let from = DVec3::new(50.0, 50.0, 50.0);
        // Whichever side of the wall the camera sees, the red line meets it there.
        let fwd = e.camera.forward();
        let x = if fwd.x > 0.0 { 300.0 } else { 320.0 };
        let p = DVec3::new(x, 50.0, 50.0);
        let i = at_px_from(&e, px(&e, p) + DVec2::new(1.5, 1.0), from, None, None);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(0)), "{i:?}");
        assert!(near(i.point, p), "{} {p}", i.point);
    }

    fn click_at(e: &mut Editor, p: DVec3) {
        let p = px(e, p);
        e.input(ToolInput::Hover { px: p });
        e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
    }

    /// World bounds of the part the root's occurrence `id` places.
    fn bounds(e: &Editor, id: Id) -> (DVec3, DVec3) {
        let (_, part, t) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
        crate::scene::transform_bounds(&t, part.shape().vertex_bounds().unwrap())
    }

    #[test]
    fn a_box_on_a_wall() {
        let mut e = view_editor(StandardView::Front, DVec3::ZERO);
        add_box(&mut e, DVec3::ZERO, DVec3::new(1000.0, 100.0, 800.0));
        // The face seen from the front, whichever way the front looks.
        let (y, out) = if e.camera.forward().y > 0.0 { (0.0, -1.0) } else { (100.0, 1.0) };
        e.set_tool(ToolKind::Box);
        // Far from the wall's edges: points of its face.
        click_at(&mut e, DVec3::new(200.0, y, 150.0));
        click_at(&mut e, DVec3::new(500.0, y, 400.0));
        e.input(ToolInput::Vcb("30".into()));
        let id = e.selection()[0];
        let (lo, hi) = bounds(&e, id);
        let (y0, y1) = if out < 0.0 { (y - 30.0, y) } else { (y, y + 30.0) };
        assert!(about(lo, DVec3::new(200.0, y0, 150.0)) && about(hi, DVec3::new(500.0, y1, 400.0)), "{lo} {hi}");
        // Exactly on the wall's plane, and exactly 30 deep.
        assert!((lo.y - y0).abs() < 1e-9 && (hi.y - y1).abs() < 1e-9, "{lo} {hi}");
    }

    #[test]
    fn arrow_keys_choose_the_drawing_plane() {
        let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
        e.set_tool(ToolKind::Cylinder);
        // → : the plane across the red axis, through the origin.
        e.input(ToolInput::Key { key: crate::Key::Right, mods: Modifiers::default() });
        click_at(&mut e, DVec3::ZERO);
        e.input(ToolInput::Vcb("40".into()));
        e.input(ToolInput::Vcb("-25".into()));
        let id = e.selection()[0];
        // A cylinder's B-rep has a vertex per circle only: both 40 from the red axis, at x 0
        // and -25.
        let (lo, hi) = bounds(&e, id);
        assert!((lo.x + 25.0).abs() < 1e-9 && hi.x.abs() < 1e-9, "{lo} {hi}");
        for p in [lo, hi] {
            assert!((p.y.hypot(p.z) - 40.0).abs() < 1e-9, "{p}");
        }
        // The lock went with the operation: the next one is on the ground.
        e.set_tool(ToolKind::Box);
        assert_eq!(e.axis_lock(), None);
    }

    #[test]
    fn shift_holds_the_line_of_a_move() {
        let mut e = top_editor(DVec3::ZERO);
        let a = add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
        add_box(&mut e, DVec3::new(300.0, 200.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
        e.select(vec![a]);
        e.set_tool(ToolKind::Move);
        click_at(&mut e, DVec3::new(50.0, 50.0, 50.0));
        // Along the red axis, Shift down: held there.
        e.input(ToolInput::Hover { px: px(&e, DVec3::new(250.0, 53.0, 50.0)) });
        assert_eq!(e.view().inference.map(|i| i.1), Some(InferKind::OnLine(LineKind::Axis(0))));
        e.set_shift(true);
        // To the other box's corner: the move stops level with it on the red axis.
        click_at(&mut e, DVec3::new(302.0, 203.0, 50.0));
        e.set_shift(false);
        // The start was a point of the face under the cursor; the end, level with the corner.
        let lo = bounds(&e, a).0;
        assert!(about(lo, DVec3::new(250.0, 0.0, 0.0)) && lo.y == 0.0 && lo.z == 0.0, "{lo}");
    }

    #[test]
    fn a_point_on_an_arc_gives_its_tangent() {
        let mut e = top_editor(DVec3::ZERO);
        let c = DVec3::new(200.0, 200.0, 50.0);
        add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 600.0, height: 50.0 });
        let a = 30f64.to_radians();
        let radial = DVec3::new(a.cos(), a.sin(), 0.0);
        let i = at(&e, c + radial * 603.0);
        assert_eq!(i.kind, InferKind::OnEdge);
        let Some(EdgeRef::Arc { tangent, radial: r }) = i.edge else { panic!("{:?}", i.edge) };
        // The angle comes from the cursor (rounded); the frame at the point found is exact.
        assert!((r - radial).length() < 1e-6 && (tangent - DVec3::new(-a.sin(), a.cos(), 0.0)).length() < 1e-6, "{r} {tangent}");
        assert!((i.point - c).normalize().dot(r) > 1.0 - 1e-12 && tangent.dot(r).abs() < 1e-12 && tangent.z == 0.0);
        // A straight edge gives its direction.
        let mut e = top_editor(DVec3::ZERO);
        add_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 150.0));
        assert_eq!(at(&e, DVec3::new(60.0, -4.0, 150.0)).edge, Some(EdgeRef::Straight(DVec3::X)));
    }

    #[test]
    fn tangent_and_radial_lines() {
        let e = top_editor(DVec3::ZERO);
        let a = 30f64.to_radians();
        let (tangent, radial) = (DVec3::new(-a.sin(), a.cos(), 0.0), DVec3::new(a.cos(), a.sin(), 0.0));
        let arc = Some(EdgeRef::Arc { tangent, radial });
        let from = DVec3::new(-200.0, -100.0, 0.0);
        let i = at_from(&e, from + tangent * 500.0 + radial * 3.0, from, arc, None);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::Tangent));
        assert!((i.point - from).cross(tangent).length() < 1e-6);
        let i = at_from(&e, from + radial * 500.0 + tangent * 3.0, from, arc, None);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::Radial));
        assert!((i.point - from).cross(radial).length() < 1e-6);
        // At 0°, the tangent is the green axis: the axis says it.
        let arc = Some(EdgeRef::Arc { tangent: DVec3::Y, radial: DVec3::X });
        let i = at_from(&e, from + DVec3::Y * 500.0, from, arc, None);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(1)));
    }

    #[test]
    fn move_along_the_tangent_of_an_arc_hovered() {
        let mut e = top_editor(DVec3::ZERO);
        let c = DVec3::new(-400.0, -400.0, 50.0);
        add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 300.0, height: 50.0 });
        let b = add_box(&mut e, DVec3::new(300.0, 300.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
        e.select(vec![b]);
        e.set_tool(ToolKind::Move);
        // Hover the circle at 30°: it becomes the reference.
        let a = 30f64.to_radians();
        let (tangent, radial) = (DVec3::new(-a.sin(), a.cos(), 0.0), DVec3::new(a.cos(), a.sin(), 0.0));
        e.input(ToolInput::Hover { px: px(&e, c + radial * 302.0) });
        assert_eq!(e.view().inference.map(|i| i.1), Some(InferKind::OnEdge));
        // Start at the box's corner; the destination follows the tangent.
        let from = DVec3::new(400.0, 400.0, 50.0);
        let p = px(&e, from + DVec3::new(2.0, -2.0, 0.0));
        e.input(ToolInput::Hover { px: p });
        e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
        e.input(ToolInput::Hover { px: px(&e, from + tangent * 300.0 + radial * 3.0) });
        assert_eq!(e.view().inference.map(|i| i.1), Some(InferKind::OnLine(LineKind::Tangent)));
    }

    #[test]
    fn a_line_crossing_an_edge() {
        let mut e = top_editor(DVec3::ZERO);
        // A wall whose top edges run from y = -200 to 400 (midpoints at y = 100, away from it all).
        add_box(&mut e, DVec3::new(300.0, -200.0, 0.0), DVec3::new(20.0, 600.0, 50.0));
        // The red axis from a point at the wall's height crosses its top edge at x = 300.
        let from = DVec3::new(50.0, 50.0, 50.0);
        let i = at_from(&e, DVec3::new(302.0, 55.0, 50.0), from, None, None);
        assert_eq!(i.kind, InferKind::OnEdgeAndLine(LineKind::Axis(0)), "{i:?}");
        assert_eq!(i.point, DVec3::new(300.0, 50.0, 50.0));
        assert_eq!(i.line, Some((from, DVec3::X)));
        assert!(i.path.is_some());
        // From lower down, the axis only passes under the edge (seen from above they overlap).
        let i = at_from(&e, DVec3::new(302.0, 55.0, 50.0), DVec3::new(50.0, 50.0, 10.0), None, None);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(0)), "{i:?}");
        // Locked on the green axis: the crossing with the wall's other top edge… none (parallel);
        // with the box's near edge, at y = -200.
        let i = at_from(&e, DVec3::new(303.0, -197.0, 50.0), DVec3::new(303.0, 50.0, 50.0), None, Some((DVec3::Y, LineKind::Axis(1))));
        assert_eq!(i.kind, InferKind::OnEdgeAndLine(LineKind::Axis(1)), "{i:?}");
        assert_eq!(i.point, DVec3::new(303.0, -200.0, 50.0));
    }

    #[test]
    fn a_line_crossing_a_circle() {
        let mut e = top_editor(DVec3::ZERO);
        let c = DVec3::new(-400.0, -400.0, 50.0);
        add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 300.0, height: 50.0 });
        // In the circle's plane: the red axis through the centre meets it at ±300.
        let from = DVec3::new(-900.0, -400.0, 50.0);
        let i = at_from(&e, DVec3::new(-697.0, -396.0, 50.0), from, None, None);
        assert_eq!(i.kind, InferKind::OnEdgeAndLine(LineKind::Axis(0)), "{i:?}");
        assert!((i.point - DVec3::new(-700.0, -400.0, 50.0)).length() < 1e-9, "{}", i.point);
        // Through it: the blue axis from a point of the top circle, below, meets it there. Seen
        // from above the blue axis is end on: use a side view.
        let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
        add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 300.0, height: 50.0 });
        let on_circle = c + DVec3::new(0.0, -300.0, 0.0);
        let from = on_circle.with_z(-400.0);
        let i = at_px_from(&e, px(&e, on_circle) + DVec2::new(1.0, 1.5), from, None, None);
        assert_eq!(i.kind, InferKind::OnEdgeAndLine(LineKind::Axis(2)), "{i:?}");
        assert!((i.point - on_circle).length() < 1e-9, "{}", i.point);
    }

    /// What is inferred at the world point `p` with these tracked points (and maybe a start).
    fn tracking(e: &Editor, p: DVec3, from: Option<DVec3>, tracked: &[DVec3]) -> Inference {
        let keep = |_: &Owner<'_>| true;
        let q = Query {
            keep: &keep,
            plane: (DVec3::ZERO, DVec3::Z),
            origin: Some(DVec3::ZERO),
            from,
            tracked,
            in_plane: false,
            axes: [DVec3::X, DVec3::Y, DVec3::Z],
            reference: None,
            lock: None,
        };
        infer(e.scene(), &e.camera, e.viewport_size(), px(e, p), &q).unwrap()
    }

    #[test]
    fn tracked_points_give_lines_and_crossings() {
        let e = top_editor(DVec3::ZERO);
        let t = DVec3::new(400.0, 200.0, 0.0);
        let from = DVec3::new(-500.0, -300.0, 0.0);
        // The start's red axis × the tracked point's green one: exactly there.
        let i = tracking(&e, DVec3::new(403.0, -297.0, 0.0), Some(from), &[t]);
        assert_eq!(i.kind, InferKind::OnTwoLines(LineKind::Axis(0), LineKind::TrackedAxis(1)), "{i:?}");
        assert_eq!(i.point, DVec3::new(400.0, -300.0, 0.0));
        assert_eq!((i.line, i.line2), (Some((from, DVec3::X)), Some((t, DVec3::Y))));
        assert_eq!(i.kind.label(), "Axe rouge × axe vert depuis le point");
        // Along the tracked point's green axis alone.
        let i = tracking(&e, DVec3::new(403.0, 600.0, 0.0), Some(from), &[t]);
        assert_eq!(i.kind, InferKind::OnLine(LineKind::TrackedAxis(1)));
        assert_eq!(i.line, Some((t, DVec3::Y)));
        // Two tracked points and no start point (a tool's first click).
        let u = DVec3::new(-300.0, -100.0, 0.0);
        let i = tracking(&e, DVec3::new(403.0, -97.0, 0.0), None, &[t, u]);
        assert_eq!(i.kind, InferKind::OnTwoLines(LineKind::TrackedAxis(1), LineKind::TrackedAxis(0)), "{i:?}");
        assert_eq!(i.point, DVec3::new(400.0, -100.0, 0.0));
        // At other heights the lines only overlap on screen: no crossing.
        let i = tracking(&e, DVec3::new(403.0, -97.0, 0.0), None, &[t, u.with_z(50.0)]);
        assert!(!matches!(i.kind, InferKind::OnTwoLines(..)), "{i:?}");
    }

    #[test]
    fn a_point_hovered_a_while_is_tracked() {
        let mut e = top_editor(DVec3::ZERO);
        let a = add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
        add_box(&mut e, DVec3::new(300.0, 300.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
        e.select(vec![a]);
        e.set_tool(ToolKind::Move);
        e.tick(10.0);
        let corner = DVec3::new(300.0, 300.0, 50.0);
        e.input(ToolInput::Hover { px: px(&e, corner + DVec3::new(2.0, -2.0, 0.0)) });
        assert!(e.tick(10.3), "waiting");
        assert!(e.tracked().is_empty());
        assert!(!e.tick(10.6));
        assert_eq!(e.tracked(), &[corner]);
        // The move starts at A's corner; the destination: level with the tracked corner on the
        // red axis, exactly.
        click_at(&mut e, DVec3::new(102.0, 2.0, 50.0));
        e.input(ToolInput::Hover { px: px(&e, DVec3::new(303.0, 3.0, 50.0)) });
        let v = e.view();
        assert_eq!(v.inference.map(|i| i.1), Some(InferKind::OnTwoLines(LineKind::Axis(0), LineKind::TrackedAxis(1))));
        assert!(v.overlay.iter().any(|l| l.a == corner), "a guide from the tracked point");
        e.input(ToolInput::Click { px: px(&e, DVec3::new(303.0, 3.0, 50.0)), mods: Modifiers::default() });
        assert_eq!(bounds(&e, a).0, DVec3::new(200.0, 0.0, 0.0));
        // Gone with the operation.
        assert!(e.tracked().is_empty());
    }

    #[test]
    fn two_points_tracked_at_most() {
        let mut e = top_editor(DVec3::ZERO);
        add_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 400.0, 50.0));
        e.set_tool(ToolKind::Move);
        let corners = [DVec3::new(0.0, 0.0, 50.0), DVec3::new(400.0, 0.0, 50.0), DVec3::new(400.0, 400.0, 50.0)];
        let mut t = 0.0;
        for c in corners {
            e.tick(t);
            e.input(ToolInput::Hover { px: px(&e, c) });
            t += 1.0;
            e.tick(t);
            // Moving away (nothing there) ends the dwell.
            e.input(ToolInput::Hover { px: px(&e, DVec3::new(-700.0, 700.0, 0.0)) });
        }
        assert_eq!(e.tracked(), &corners[1..]);
        e.input(ToolInput::Key { key: crate::Key::Escape, mods: Modifiers::default() });
        assert!(e.tracked().is_empty());
    }

    #[test]
    fn a_line_does_not_cross_an_edge_at_its_own_origin() {
        let mut e = top_editor(DVec3::ZERO);
        add_box(&mut e, DVec3::ZERO, DVec3::new(1000.0, 400.0, 50.0));
        // A start point on the top front edge (away from its midpoint): the green axis through it
        // meets that edge right there, which is no crossing.
        let from = DVec3::new(300.0, 0.0, 50.0);
        let i = at_from(&e, DVec3::new(310.0, -15.0, 50.0), from, None, None);
        assert_eq!(i.kind, InferKind::OnEdge, "{i:?}");
        // Nor do two lines meet at the start point: a tracked point level with it on the red axis.
        let i = tracking(&e, DVec3::new(310.0, -15.0, 50.0), Some(from), &[DVec3::new(-400.0, 0.0, 50.0)]);
        assert!(!matches!(i.kind, InferKind::OnTwoLines(..)), "{i:?}");
    }

    /// Timing, not a check (review finding 7): hovers swept over large scenes, iso perspective.
    /// `cargo test --release -p solidsketch-tools infer_timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn infer_timing() {
        use solidsketch_model::{Boolean, Scope};
        use std::time::Instant;

        let iso = |e: &mut Editor| {
            e.set_parallel(false);
            let b = e.scene().bounds();
            e.camera.set_standard_view(StandardView::Iso, b, ViewArea::full(e.viewport_size()));
        };
        let array = |e: &mut Editor, ids: Vec<Id>, step: DVec3, count: u32| -> Vec<Id> {
            let root = e.document().root();
            let op = Operation::Array { context: root, entities: ids.clone(), step: DAffine3::from_translation(step), count };
            let mut all = ids;
            all.extend(e.apply(op).unwrap().expect("done").created);
            all
        };

        // Many parts: n × n occurrences of one board (a cabinet maker's sheet of parts).
        let grid = |n: u32| {
            let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
            let a = add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 60.0, 18.0));
            let row = array(&mut e, vec![a], DVec3::X * 150.0, n - 1);
            array(&mut e, row, DVec3::Y * 100.0, n - 1);
            iso(&mut e);
            e
        };
        // One heavy part: a board drilled with 2^k holes (circles: arcs, a dense mesh).
        let drilled = |k: usize| {
            let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
            let root = e.document().root();
            let board = add_box(&mut e, DVec3::ZERO, DVec3::new(2000.0, 1000.0, 18.0));
            let drill = add(&mut e, Primitive::Cylinder { origin: DVec3::new(50.0, 50.0, -5.0), radius: 4.0, height: 30.0 });
            let target = EntityPath::root(board);
            e.apply(Operation::Boolean {
                target: target.clone(),
                tool: EntityPath::root(drill),
                op: Boolean::Difference,
                scope: Scope::All,
            })
            .unwrap();
            let _ = root;
            for i in 0..k {
                let holes = e.document().resolve_part(&target).unwrap().1.machinings.iter().map(|m| m.id).collect();
                let step = if i < 4 { DVec3::X * (100.0 * (1 << i) as f64) } else { DVec3::Y * (100.0 * (1 << (i - 4)) as f64) };
                e.apply(Operation::DuplicateMachinings {
                    target: target.clone(),
                    machinings: holes,
                    transform: DAffine3::from_translation(step),
                    scope: Scope::All,
                })
                .unwrap();
            }
            assert!(!e.busy());
            iso(&mut e);
            e
        };

        let run = |name: &str, e: &Editor| {
            let (parts, edges, tris) = e
                .scene()
                .inferable()
                .fold((0, 0, 0), |(p, ed, t), (_, m, _)| (p + 1, ed + m.snaps.edges.len(), t + m.mesh.indices.len() / 3));
            let keep = |_: &Owner<'_>| true;
            let tracked = [DVec3::new(10.0, 10.0, 18.0), DVec3::new(300.0, 500.0, 18.0)];
            let diagonal = Some(EdgeRef::Straight(DVec3::new(1.0, 1.0, 0.0).normalize()));
            for (case, from, tracked, reference) in
                [("points", None, &[][..], None), ("11 lines", Some(DVec3::new(0.0, 0.0, 18.0)), &tracked[..], diagonal)]
            {
                let q = Query {
                    keep: &keep,
                    plane: (DVec3::ZERO, DVec3::Z),
                    origin: Some(DVec3::ZERO),
                    from,
                    tracked,
                    in_plane: false,
                    axes: [DVec3::X, DVec3::Y, DVec3::Z],
                    reference,
                    lock: None,
                };
                // Only the hovers over the geometry: elsewhere there is next to nothing to do.
                let mut times = Vec::new();
                for i in 0..100 {
                    for j in 0..100 {
                        let px = DVec2::new(5.0 + 10.0 * i as f64, 5.0 + 10.0 * j as f64);
                        let t = Instant::now();
                        let r = std::hint::black_box(infer(e.scene(), &e.camera, e.viewport_size(), px, &q));
                        let ms = t.elapsed().as_secs_f64() * 1e3;
                        if r.is_some_and(|r| r.hit.is_some()) {
                            times.push(ms);
                        }
                    }
                }
                times.sort_by(f64::total_cmp);
                let mean = times.iter().sum::<f64>() / times.len() as f64;
                let pct = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
                println!(
                    "{name:<18} {parts:>5} parts {edges:>6} edges {tris:>7} tris | {case:<8} {:>5} hovers | mean {mean:6.2} ms  p50 {:6.2}  p95 {:6.2}  max {:6.2}",
                    times.len(),
                    pct(0.5),
                    pct(0.95),
                    pct(1.0)
                );
            }
        };

        for n in [10, 20, 40] {
            run(&format!("grid {n}x{n}"), &grid(n));
        }
        for k in [5, 7] {
            run(&format!("board, {} holes", 1 << k), &drilled(k));
        }
    }
}

//! The lines inferred on: through the start point, tracked points, the axes as drawn and
//! the guide lines; where the cursor falls on one, where two of them cross.

use glam::{DVec2, DVec3};
use solidsketch_model::GuideShape;

use super::{APERTURE_PX, CROSSING_PX, Curve, EdgeRef, LineKind, Query};
use crate::camera::{Camera, Projector, Ray};
use crate::scene::{Hit, SceneGuide};

/// A line inferred on.
#[derive(Clone, Copy, Debug)]
pub(super) struct Line {
    pub(super) origin: DVec3,
    /// Unit.
    pub(super) dir: DVec3,
    pub(super) kind: LineKind,
    /// 0 for the start point, 1 + i for tracked point i, then the drawn axes, then one per
    /// guide line: lines of one origin cross only there.
    pub(super) source: usize,
}

impl Line {
    /// The line as an edge to refer to, when one is drawn (a guide, an axis as drawn), with
    /// the line as drawn.
    pub(super) fn as_ref(&self) -> Option<(EdgeRef, Curve)> {
        let (origin, dir) = (self.origin, self.dir);
        let curve = match self.kind {
            LineKind::Guide => Curve::Guide(GuideShape::Line { origin, dir }),
            LineKind::ModelAxis(_) => Curve::Axis { origin, dir },
            _ => return None,
        };
        Some((EdgeRef::Straight(dir), curve))
    }
}

/// The lines inferred on: through the start point, the axes, then those of the reference edge
/// when not an axis already (parallel and perpendicular — in the tool's plane — to a straight
/// edge, tangent and radial to an arc); through each tracked point, the axes; the axes as
/// drawn; the guide lines of the document.
pub(super) fn lines(q: &Query<'_>, guides: &[&SceneGuide]) -> Vec<Line> {
    let mut out = Vec::new();
    let known = |d: DVec3| q.axes.iter().any(|a| a.dot(d).abs() > 1.0 - 1e-9);
    if let Some(from) = q.from {
        for k in 0..3 {
            out.push(Line { origin: from, dir: q.axes[k], kind: LineKind::Axis(k), source: 0 });
        }
        let refs = match q.reference {
            Some(EdgeRef::Straight(r)) => [(Some(r), LineKind::Parallel), (q.plane.1.cross(r).try_normalize(), LineKind::Perpendicular)],
            Some(EdgeRef::Arc { tangent, radial, .. }) => [(Some(tangent), LineKind::Tangent), (Some(radial), LineKind::Radial)],
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
    // The axes as drawn, but where the start point's run already.
    let first = 1 + q.tracked.len();
    if let Some(o) = q.origin.filter(|o| q.model_axes && q.from.is_none_or(|f| (f - *o).length() >= 1e-9)) {
        for k in 0..3 {
            out.push(Line { origin: o, dir: q.axes[k], kind: LineKind::ModelAxis(k), source: first });
        }
    }
    for (i, g) in guides.iter().enumerate() {
        if let GuideShape::Line { origin, dir } = g.shape {
            out.push(Line { origin, dir, kind: LineKind::Guide, source: first + 1 + i });
        }
    }
    if q.in_plane {
        // A line of the scene, only lying in the plane.
        let (o, n) = q.plane;
        out.retain(|l| l.dir.dot(n).abs() < 1e-9 && (!l.kind.is_fixed() || (l.origin - o).dot(n).abs() < 1e-6));
    }
    out
}

/// The point of the line the cursor gives: where it pierces the face under the cursor if that
/// is near the cursor, else the point nearest the cursor's ray.
pub(super) fn on_line(ray: &Ray, screen: &Projector, px: DVec2, l: &Line, hit: Option<&Hit>) -> DVec3 {
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
pub(super) fn two_lines(camera: &Camera, size: DVec2, screen: &Projector, px: DVec2, lines: &[Line]) -> Option<(f64, DVec3, usize, usize)> {
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
pub(super) fn lines_meet(a: &Line, b: &Line, tol: impl Fn(DVec3) -> f64) -> Option<DVec3> {
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

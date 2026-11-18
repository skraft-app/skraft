//! Where a line crosses an edge or a guide circle, and where a guide circle crosses a curve.

use std::f64::consts::TAU;

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::SnapCurve;
use skraft_model::EntityPath;

use super::curves::{PlacedArc, curve_ref, drawn, guide_arc, guide_arcs, kept_guides};
use super::lines::Line;
use super::{APERTURE_PX, CROSSING_PX, Curve, EdgeRef, InferKind, Inference, Query, near_ray, visible};
use crate::camera::{Camera, Projector};
use crate::scene::{DisplayScene, Hit, Owner};

/// A line crossing an edge near the cursor.
pub(super) struct Crossing {
    /// On the line, exactly.
    pub(super) point: DVec3,
    /// Index of the line in the list given.
    pub(super) line: usize,
    /// The solid whose edge it is.
    pub(super) path: Option<EntityPath>,
    /// A guide circle's, not an edge's.
    pub(super) guide: bool,
    /// The edge or the guide circle there.
    pub(super) curve: Option<(EdgeRef, Curve)>,
}

impl Crossing {
    /// The point inferred where it crosses the line `l` (the one given by its index).
    pub(super) fn inference(self, l: &Line, hit: Option<Hit>) -> Inference {
        let kind = if self.guide { InferKind::OnGuideAndLine(l.kind) } else { InferKind::OnEdgeAndLine(l.kind) };
        let mut i = Inference::on_lines(self.point, kind, self.path, hit, l, None);
        i.crossed = l.as_ref().into_iter().chain(self.curve).collect();
        i
    }
}

/// The visible crossing nearest the cursor (within the aperture) of one of `lines` with an
/// edge of the scene; the point is taken on the line.
pub(super) fn crossing(
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
    for (place, part, piece) in scene.inferable_where(|b| near_ray(camera, size, &ray, *b)) {
        let path = place.entity_path();
        if !(q.keep)(&Owner { path: &path, piece }) {
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
                        let curve = curve_ref(&e.curve, t, p).map(|r| (r, drawn(&e.curve, t)));
                        found.push((dist, Crossing { point: p, line: k, path: Some(path.clone()), guide: false, curve }));
                    }
                }
            }
        }
    }
    for (shape, arc) in guide_arcs(&kept_guides(scene, q)) {
        for (k, l) in lines.iter().enumerate() {
            let tol = |p: DVec3| camera.world_per_pixel(size, p) * CROSSING_PX;
            for p in line_meets_curve(l.origin, l.dir, &arc, &DAffine3::IDENTITY, tol) {
                if (p - l.origin).length() <= tol(p) {
                    continue;
                }
                let Some(s) = screen.project(p) else { continue };
                let dist = s.distance(px);
                if dist <= APERTURE_PX {
                    let curve = curve_ref(&arc, &DAffine3::IDENTITY, p).map(|r| (r, Curve::Guide(shape)));
                    found.push((dist, Crossing { point: p, line: k, path: None, guide: true, curve }));
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, c)| c).find(|c| visible(scene, camera, size, screen, c.point, q))
}

/// Where the line `from + s·dir` (unit `dir`) meets `curve` placed by `t`, within `tol(p)` of
/// it: points of the line.
pub(super) fn line_meets_curve(from: DVec3, dir: DVec3, curve: &SnapCurve, t: &DAffine3, tol: impl Fn(DVec3) -> f64) -> Vec<DVec3> {
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

/// Where the circle about `c` (unit normal `n`, radius `r`, world) meets `curve` placed by
/// `t`, within `tol(p)` of both.
pub(super) fn circle_meets_curve(c: DVec3, n: DVec3, r: f64, curve: &SnapCurve, t: &DAffine3, tol: impl Fn(DVec3) -> f64) -> Vec<DVec3> {
    let circle = guide_arc(c, n, r);
    // A segment: where its line meets the circle, within the segment.
    let on_segment = |a: DVec3, b: DVec3| -> Vec<DVec3> {
        let len = (b - a).length();
        let Some(u) = (b - a).try_normalize() else { return vec![] };
        line_meets_curve(a, u, &circle, &DAffine3::IDENTITY, &tol)
            .into_iter()
            .filter(|p| {
                let s = (*p - a).dot(u);
                s >= -tol(*p) && s <= len + tol(*p)
            })
            .collect()
    };
    match *curve {
        SnapCurve::Segment { a, b } => on_segment(t.transform_point3(a), t.transform_point3(b)),
        SnapCurve::Arc { center, x, y, radius, sweep } => {
            let arc = PlacedArc::new(t, center, x, y, radius, sweep);
            if !arc.is_circle() {
                return arc.samples().windows(2).flat_map(|w| on_segment(w[0], w[1])).collect();
            }
            let PlacedArc { c: c2, n: n2, rx: r2, .. } = arc;
            let on_arc = |p: DVec3| {
                let a = arc.angle_of(p);
                let slack = tol(p) / r2;
                a <= sweep + slack || a >= TAU - slack
            };
            let on_edge = |p: &DVec3| ((*p - c2).length() - r2).abs() <= tol(*p) && on_arc(*p);
            let axis = n.cross(n2);
            if axis.length() < 1e-9 {
                // In parallel planes: in one plane, two circles meet in up to two points.
                if (c2 - c).dot(n).abs() > tol(c) {
                    return vec![];
                }
                let w = (c2 - c) - n * (c2 - c).dot(n);
                let d = w.length();
                if d < 1e-9 || d > r + r2 + tol(c) || d < (r - r2).abs() - tol(c) {
                    return vec![];
                }
                let (u, a) = (w / d, (r * r - r2 * r2 + d * d) / (2.0 * d));
                let h = (r * r - a * a).max(0.0).sqrt();
                let v = n.cross(u);
                let base = c + u * a;
                return [base + v * h, base - v * h].into_iter().filter(on_edge).collect();
            }
            // Else on the line where both planes meet: its points on the guide, kept on the arc.
            let (d1, d2) = (n.dot(c), n2.dot(c2));
            let on_both = (n2.cross(axis) * d1 + axis.cross(n) * d2) / axis.length_squared();
            line_meets_curve(on_both, axis.normalize(), &circle, &DAffine3::IDENTITY, &tol).into_iter().filter(on_edge).collect()
        }
        SnapCurve::Polyline { ref points, .. } => {
            let pts: Vec<DVec3> = points.iter().map(|p| t.transform_point3(*p)).collect();
            pts.windows(2).flat_map(|w| on_segment(w[0], w[1])).collect()
        }
    }
}

/// Where the line `from + s·dir` (unit `dir`) passes within `tol` of the segment `[a, b]`
/// (not parallel to it): the line's point there.
pub(super) fn line_meets_segment(from: DVec3, dir: DVec3, a: DVec3, b: DVec3, tol: &impl Fn(DVec3) -> f64) -> Option<DVec3> {
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

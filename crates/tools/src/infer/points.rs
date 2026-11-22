//! The points of the geometry under the cursor (no lines), else the face, else the plane.

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::{SnapCurve, SnapEdge, Snaps};
use skraft_model::{EntityPath, GuideShape};

use super::crossings::circle_meets_curve;
use super::curves::{curve_ref, drawn, guide_arc, guide_arcs, guide_circle, kept_guides, on_curve};
use super::{APERTURE_PX, CROSSING_PX, Curve, EdgeRef, InferKind, Inference, Query, RANKS, THIRD_APERTURE_PX, near_ray, visible};
use crate::camera::{Camera, Projector, Ray};
use crate::scene::{DisplayScene, Owner};

pub(super) struct Candidate {
    point: DVec3,
    kind: InferKind,
    path: Option<EntityPath>,
    edge: Option<(EdgeRef, Curve)>,
    /// The curves crossing there (see [`Inference::crossed`]).
    crossed: Vec<(EdgeRef, Curve)>,
    /// Screen distance to the cursor (pixels).
    dist: f64,
    /// Depth along the cursor's ray.
    depth: f64,
}

/// The points of the geometry under `px` (no lines), else the face, else the plane.
pub(super) fn infer_points(scene: &DisplayScene, camera: &Camera, size: DVec2, px: DVec2, q: &Query<'_>) -> Option<Inference> {
    let ray = camera.ray(px, size);
    let screen = camera.projector(size);
    let hit = scene.pick_inferable(&ray, q.keep);

    let mut cands = Vec::new();
    let mut offer =
        |point: DVec3, kind: InferKind, path: Option<&EntityPath>, edge: Option<(EdgeRef, Curve)>, crossed: Vec<(EdgeRef, Curve)>| {
            let Some(s) = screen.project(point) else { return };
            let dist = (s - px).length();
            if dist <= APERTURE_PX {
                let depth = (point - ray.origin).dot(ray.dir);
                cands.push(Candidate { point, kind, path: path.cloned(), edge, crossed, dist, depth });
            }
        };

    if let Some(o) = q.origin {
        offer(o, InferKind::Origin, None, None, Vec::new());
    }
    // The tool's own line: its origin, an end.
    if let Some((o, dir)) = q.tool_line {
        offer(o, InferKind::Endpoint, None, Some((EdgeRef::Straight(dir), Curve::Axis { origin: o, dir })), Vec::new());
    }
    let kept = kept_guides(scene, q);
    let circles: Vec<(DVec3, DVec3, f64)> = kept
        .iter()
        .filter_map(|g| match g.shape {
            GuideShape::Circle { center, normal, radius } => Some((center, normal, radius)),
            GuideShape::Line { .. } => None,
        })
        .collect();
    let tol = |p: DVec3| camera.world_per_pixel(size, p) * CROSSING_PX;
    for (place, part, piece) in scene.inferable_where(|b| near_ray(camera, size, &ray, *b)) {
        let path = place.entity_path();
        if !(q.keep)(&Owner { path: &path, piece }) {
            continue;
        }
        let t = &place.transform;
        features(&part.snaps, t, &ray, &screen, px, &mut |p, k, e, x| offer(p, k, Some(&path), e, x));
        // Where a guide circle crosses an edge.
        for &(c, n, r) in &circles {
            for e in &part.snaps.edges {
                for p in circle_meets_curve(c, n, r, &e.curve, t, tol) {
                    let guide = curve_ref(&guide_arc(c, n, r), &DAffine3::IDENTITY, p).map(|g| (g, guide_circle(c, n, r)));
                    let edge = curve_ref(&e.curve, t, p).map(|r| (r, drawn(&e.curve, t)));
                    offer(p, InferKind::OnGuideAndEdge, Some(&path), None, [guide, edge].into_iter().flatten().collect());
                }
            }
        }
    }
    // Where two guide circles cross.
    for (i, &(c, n, r)) in circles.iter().enumerate() {
        for &(c2, n2, r2) in &circles[i + 1..] {
            for p in circle_meets_curve(c, n, r, &guide_arc(c2, n2, r2), &DAffine3::IDENTITY, tol) {
                let crossed = [(c, n, r), (c2, n2, r2)]
                    .map(|(c, n, r)| curve_ref(&guide_arc(c, n, r), &DAffine3::IDENTITY, p).map(|g| (g, guide_circle(c, n, r))));
                offer(p, InferKind::OnTwoGuides, None, None, crossed.into_iter().flatten().collect());
            }
        }
    }
    // The guide circles: their centres, and the point nearest the cursor (an arc to refer to).
    for (shape, arc) in guide_arcs(&kept_guides(scene, q)) {
        let SnapCurve::Arc { center, x, y, radius, .. } = arc else { continue };
        offer(center, InferKind::Center, None, None, Vec::new());
        if let Some(p) = on_curve(&arc, &DAffine3::IDENTITY, &ray, &screen, px)
            && let Some(radial) = (p - center).try_normalize()
        {
            let tangent = x.cross(y).cross(radial);
            offer(p, InferKind::OnGuide, None, Some((EdgeRef::Arc { tangent, radial, center, radius }, Curve::Guide(shape))), Vec::new());
        }
    }
    // The middle of the two tracked points, offered last: a feature at the same place keeps
    // its edge.
    if let [a, b] = q.tracked {
        offer((*a + *b) * 0.5, InferKind::TrackedMiddle, None, None, Vec::new());
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
        // The tracked points' middle is shown over everything: it may lie inside a solid.
        if c.kind == InferKind::TrackedMiddle || visible(scene, camera, size, &screen, c.point, q) {
            let mut i = Inference::at(c.point, c.kind, c.path, hit, c.edge);
            i.crossed = c.crossed;
            return Some(i);
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

/// Offers the snap features of one occurrence (definition to world `t`): its vertices (with
/// the edges ending there as the curves crossing, for a vertex near the cursor), the
/// midpoints, thirds and centres of its edges, and the point of each edge nearest the cursor (with the
/// edge there, straight or arc, as the next reference).
pub(super) fn features(
    s: &Snaps,
    t: &DAffine3,
    ray: &Ray,
    screen: &Projector,
    px: DVec2,
    offer: &mut impl FnMut(DVec3, InferKind, Option<(EdgeRef, Curve)>, Vec<(EdgeRef, Curve)>),
) {
    for v in &s.vertices {
        let p = t.transform_point3(*v);
        let near = screen.project(p).is_some_and(|s| s.distance(px) <= APERTURE_PX);
        let crossed = if near {
            let ends_here = |e: &&SnapEdge| e.curve.ends().iter().any(|end| (*end - *v).length() < 1e-6);
            s.edges.iter().filter(ends_here).filter_map(|e| Some((curve_ref(&e.curve, t, p)?, drawn(&e.curve, t)))).collect()
        } else {
            Vec::new()
        };
        offer(p, InferKind::Endpoint, None, crossed);
    }
    for e in &s.edges {
        let edge_at = |p: DVec3| Some((curve_ref(&e.curve, t, p)?, drawn(&e.curve, t)));
        if let Some(m) = e.curve.midpoint() {
            let m = t.transform_point3(m);
            offer(m, InferKind::Midpoint, edge_at(m), Vec::new());
        }
        // Thirds only where the edge is long enough on screen for each point to be aimed at.
        if let Some(thirds) = e.curve.thirds().map(|p| p.map(|p| t.transform_point3(p)))
            && let (Some(a), Some(b)) = (screen.project(thirds[0]), screen.project(thirds[1]))
            && a.distance(b) >= 2.0 * APERTURE_PX
        {
            for (p, s) in thirds.into_iter().zip([a, b]) {
                if s.distance(px) <= THIRD_APERTURE_PX {
                    offer(p, InferKind::Third, edge_at(p), Vec::new());
                }
            }
        }
        if let Some(c) = e.curve.center() {
            offer(t.transform_point3(c), InferKind::Center, None, Vec::new());
        }
        if let Some(p) = on_curve(&e.curve, t, ray, screen, px) {
            offer(p, InferKind::OnEdge, edge_at(p), Vec::new());
        }
    }
}

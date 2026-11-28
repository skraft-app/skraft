//! The points of the geometry under the cursor (no lines), else the face, else the plane.

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::{SnapCurve, SnapEdge, Snaps};
use skraft_model::{EntityPath, GuideShape};

use super::crossings::{circle_meets_curve, curve_meets_plane, curves_meet};
use super::curves::{curve_ref, drawn, guide_arc, guide_arcs, guide_circle, kept_guides, on_curve};
use super::{
    APERTURE_PX, CROSSING_PX, Curve, EdgeRef, FaceOf, InferKind, Inference, Query, RANKS, THIRD_APERTURE_PX, near_ray, on_face,
    placed_plane, visible,
};
use crate::camera::{Camera, Projector, Ray};
use crate::scene::{DisplayScene, Owner, Piece, transform_bounds};

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
    // The edges passing near the cursor, for their crossings.
    let mut near: Vec<NearEdge<'_>> = Vec::new();
    // The solids near the cursor, for the faces those edges pierce.
    let mut solids: Vec<(EntityPath, Piece, DAffine3, &Snaps, (DVec3, DVec3))> = Vec::new();
    for (place, part, piece) in scene.inferable_where(|b| near_ray(camera, size, &ray, *b)) {
        let path = place.entity_path();
        if !(q.keep)(&Owner { path: &path, piece }) {
            continue;
        }
        let t = &place.transform;
        features(&part.snaps, t, &ray, &screen, px, &mut |p, k, e, x| offer(p, k, Some(&path), e, x), &mut |e| {
            let bounds = curve_bounds(&e.curve, t);
            near.push(NearEdge { path: path.clone(), piece, edge: e, t: *t, bounds });
        });
        if let Some(b) = part.bounds {
            solids.push((path.clone(), piece, *t, &part.snaps, transform_bounds(t, b)));
        }
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
    // Where edges of two solids cross, where an edge pierces a planar face of another (one seen
    // there, near the cursor).
    for (i, a) in near.iter().enumerate() {
        for b in near[i + 1..].iter().filter(|b| !a.same_solid(&b.path, b.piece) && meet(a.bounds, b.bounds)) {
            for p in curves_meet(&a.edge.curve, &a.t, &b.edge.curve, &b.t, tol) {
                if !a.at_an_end(p, tol(p)) && !b.at_an_end(p, tol(p)) {
                    offer(p, InferKind::OnTwoEdges, Some(&a.path), None, [a.at(p), b.at(p)].into_iter().flatten().collect());
                }
            }
        }
    }
    for (path, piece, t, snaps, bounds) in &solids {
        // A point of its faces lies in its box.
        let others: Vec<&NearEdge<'_>> = near.iter().filter(|a| !a.same_solid(path, *piece) && meet(a.bounds, *bounds)).collect();
        if others.is_empty() {
            continue;
        }
        for (face, plane) in snaps.planes.iter().enumerate() {
            let Some((o, n)) = plane.and_then(|p| placed_plane(t, p)) else { continue };
            for a in &others {
                for p in curve_meets_plane(&a.edge.curve, &a.t, o, n, tol) {
                    let seen = screen.project(p).is_some_and(|s| s.distance(px) <= APERTURE_PX);
                    let f = FaceOf { path, piece: *piece, face: face as u32 };
                    if seen && !a.at_an_end(p, tol(p)) && on_face(scene, camera, size, &screen, p, f, q) {
                        offer(p, InferKind::OnEdgeAndFace, Some(&a.path), None, a.at(p).into_iter().collect());
                    }
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
    for &c in q.box_corners {
        offer(c, InferKind::BoxCorner, None, None, Vec::new());
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

/// An edge passing near the cursor, placed in the world by `t`.
struct NearEdge<'a> {
    path: EntityPath,
    piece: Piece,
    edge: &'a SnapEdge,
    t: DAffine3,
    /// Its world box, a little wider ([`curve_bounds`]).
    bounds: (DVec3, DVec3),
}

/// The world box of `curve` placed by `t`, a hair wider for the tolerance of the crossings
/// (edges a fraction of a pixel apart meet).
fn curve_bounds(curve: &SnapCurve, t: &DAffine3) -> (DVec3, DVec3) {
    let pts: Vec<DVec3> = match *curve {
        SnapCurve::Segment { a, b } => vec![t.transform_point3(a), t.transform_point3(b)],
        SnapCurve::Arc { center, x, y, radius, .. } => {
            let (c, r) = (t.transform_point3(center), (t.transform_vector3(x).length().max(t.transform_vector3(y).length())) * radius);
            vec![c - DVec3::splat(r), c + DVec3::splat(r)]
        }
        SnapCurve::Polyline { ref points, .. } => points.iter().map(|p| t.transform_point3(*p)).collect(),
    };
    let lo = pts.iter().copied().fold(DVec3::splat(f64::INFINITY), DVec3::min);
    let hi = pts.iter().copied().fold(DVec3::splat(f64::NEG_INFINITY), DVec3::max);
    let pad = DVec3::splat(1e-3 * (hi - lo).max_element().max(1.0));
    (lo - pad, hi + pad)
}

/// Do the boxes `a` and `b` overlap?
fn meet(a: (DVec3, DVec3), b: (DVec3, DVec3)) -> bool {
    a.0.cmple(b.1).all() && b.0.cmple(a.1).all()
}

impl NearEdge<'_> {
    /// An edge of that solid: they meet only at its vertices.
    fn same_solid(&self, path: &EntityPath, piece: Piece) -> bool {
        self.piece == piece && self.path == *path
    }

    /// Is `p` within `tol` of one of its ends (an endpoint already)?
    fn at_an_end(&self, p: DVec3, tol: f64) -> bool {
        self.edge.curve.ends().iter().any(|e| (self.t.transform_point3(*e) - p).length() <= tol)
    }

    /// The edge at its point `p`, as crossed there.
    fn at(&self, p: DVec3) -> Option<(EdgeRef, Curve)> {
        Some((curve_ref(&self.edge.curve, &self.t, p)?, drawn(&self.edge.curve, &self.t)))
    }
}

/// Offers the snap features of one occurrence (definition to world `t`): its vertices (with
/// the edges ending there as the curves crossing, for a vertex near the cursor), the
/// midpoints, thirds and centres of its edges, and the point of each edge nearest the cursor (with the
/// edge there, straight or arc, as the next reference); gives `near` each edge passing within
/// the aperture.
pub(super) fn features<'a>(
    s: &'a Snaps,
    t: &DAffine3,
    ray: &Ray,
    screen: &Projector,
    px: DVec2,
    offer: &mut impl FnMut(DVec3, InferKind, Option<(EdgeRef, Curve)>, Vec<(EdgeRef, Curve)>),
    near: &mut impl FnMut(&'a SnapEdge),
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
            if screen.project(p).is_some_and(|s| s.distance(px) <= APERTURE_PX) {
                near(e);
            }
            offer(p, InferKind::OnEdge, edge_at(p), Vec::new());
        }
    }
}

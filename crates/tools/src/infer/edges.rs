//! The B-rep edge under the cursor, and the edges running on from it tangentially: what the
//! round and chamfer tools pick (docs/tools/rounds.md, docs/decisions/0042-body-nodes-operators.md).

use glam::{DAffine3, DVec2, DVec3};
use skraft_model::EntityPath;
use skraft_model::kernel::{SnapCurve, Snaps};

use super::curves::{drawn, on_curve};
use super::{APERTURE_PX, Curve, near_ray};
use crate::camera::Camera;
use crate::scene::{DisplayScene, Owner, Piece};

/// An edge of a shape shown: its owner, its index in its solid's record, where it lies.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeHit {
    /// The occurrence (the open part's, for its bodies) and the piece.
    pub path: EntityPath,
    pub piece: Piece,
    /// The edge's index in its solid's record.
    pub edge: u32,
    /// Its solid's axes to the world.
    pub transform: DAffine3,
    /// A point inside it, away from its ends (world): what names it to the kernel.
    pub middle: DVec3,
    /// The edge as drawn (world).
    pub drawn: Vec<DVec3>,
}

/// A point inside `c`, away from its ends: its middle, half way round a whole circle.
fn middle(c: &SnapCurve) -> Option<DVec3> {
    match c {
        SnapCurve::Arc { center, x, y, radius, .. } if c.is_closed_arc() => {
            Some(SnapCurve::arc_point(*center, *x, *y, *radius, std::f64::consts::PI))
        }
        SnapCurve::Polyline { points, .. } if points.len() > 2 => Some(points[points.len() / 2]),
        _ => c.midpoint(),
    }
}

fn points_of(c: &Curve) -> Vec<DVec3> {
    match c {
        Curve::Segment(a, b) => vec![*a, *b],
        Curve::Arc(points) => points.clone(),
        _ => Vec::new(),
    }
}

/// The edge `e` of `snaps` (placed by `t`) as an [`EdgeHit`].
fn hit(path: &EntityPath, piece: Piece, t: &DAffine3, snaps: &Snaps, e: usize) -> Option<EdgeHit> {
    let edge = snaps.edges.get(e)?;
    Some(EdgeHit {
        path: path.clone(),
        piece,
        edge: edge.id,
        transform: *t,
        middle: t.transform_point3(middle(&edge.curve)?),
        drawn: points_of(&drawn(&edge.curve, t)),
    })
}

/// The edge nearest the cursor `px` within the aperture, among the shapes `keep` accepts, and
/// seen: nothing `hide` accepts in front of it (what is shown, `keep`'s at least).
pub fn edge_at(
    scene: &DisplayScene,
    camera: &Camera,
    size: DVec2,
    px: DVec2,
    keep: &dyn Fn(&Owner<'_>) -> bool,
    hide: &dyn Fn(&Owner<'_>) -> bool,
) -> Option<EdgeHit> {
    let ray = camera.ray(px, size);
    let screen = camera.projector(size);
    let mut best: Option<(f64, EdgeHit)> = None;
    for (place, part, piece) in scene.inferable_where(|b| near_ray(camera, size, &ray, *b)) {
        let path = place.entity_path();
        if !keep(&Owner { path: &path, piece }) {
            continue;
        }
        let t = &place.transform;
        for (i, e) in part.snaps.edges.iter().enumerate() {
            let Some(p) = on_curve(&e.curve, t, &ray, &screen, px) else { continue };
            let Some(d) = screen.project(p).map(|s| s.distance(px)) else { continue };
            if d > APERTURE_PX || best.as_ref().is_some_and(|(b, _)| *b <= d) || !seen(scene, camera, size, p, hide) {
                continue;
            }
            if let Some(h) = hit(&path, piece, t, &part.snaps, i) {
                best = Some((d, h));
            }
        }
    }
    best.map(|(_, h)| h)
}

/// Is `p` seen: not cut away by the section, and nothing `keep` accepts before it on its ray?
fn seen(scene: &DisplayScene, camera: &Camera, size: DVec2, p: DVec3, keep: &dyn Fn(&Owner<'_>) -> bool) -> bool {
    if scene.section().is_some_and(|s| s.cuts(p)) {
        return false;
    }
    let Some(s) = camera.projector(size).project(p) else { return false };
    let ray = camera.ray(s, size);
    let depth = (p - ray.origin).dot(ray.dir);
    match scene.pick_inferable(&ray, |o| keep(o) && !scene.see_through(o.piece)) {
        None => true,
        Some(h) => h.t >= depth - camera.world_per_pixel(size, p) * 3.0 - 1e-9 * p.length().max(1.0),
    }
}

/// The unit tangent of `c` at its end `at`, pointing out of the curve.
fn outward(c: &SnapCurve, at: DVec3) -> Option<DVec3> {
    match *c {
        SnapCurve::Segment { a, b } => {
            let d = (b - a).try_normalize()?;
            if at.distance(a) < 1e-6 { Some(-d) } else { Some(d) }
        }
        SnapCurve::Arc { x, y, sweep, center, radius } => {
            let tangent = |t: f64| -x * t.sin() + y * t.cos();
            if at.distance(SnapCurve::arc_point(center, x, y, radius, 0.0)) < 1e-6 { Some(-tangent(0.0)) } else { Some(tangent(sweep)) }
        }
        SnapCurve::Polyline { ref points, .. } => {
            let n = points.len();
            if n < 2 {
                return None;
            }
            if at.distance(points[0]) < 1e-6 {
                (points[0] - points[1]).try_normalize()
            } else {
                (points[n - 1] - points[n - 2]).try_normalize()
            }
        }
    }
}

/// The edges of the solid `of` was hit on that run on from it tangentially, at either end,
/// one after another (the round tools' double click): `of` first.
pub fn tangent_chain(scene: &DisplayScene, of: &EdgeHit) -> Vec<EdgeHit> {
    let Some((_, part)) = scene
        .inferable_where(|_| true)
        .into_iter()
        .find(|(p, _, piece)| p.entity_path() == of.path && *piece == of.piece)
        .map(|(p, m, _)| (p, m))
    else {
        return vec![of.clone()];
    };
    let edges = &part.snaps.edges;
    let Some(start) = edges.iter().position(|e| e.id == of.edge) else { return vec![of.clone()] };
    let mut chain = vec![start];
    let mut ends: Vec<DVec3> = edges[start].curve.ends();
    while let Some(at) = ends.pop() {
        let Some(&from) = chain.iter().find(|&&i| edges[i].curve.ends().iter().any(|e| e.distance(at) < 1e-6)) else { continue };
        let Some(out) = outward(&edges[from].curve, at) else { continue };
        // The edge leaving that end along the same tangent, not taken yet.
        let next = (0..edges.len()).find(|&j| {
            !chain.contains(&j)
                && edges[j].curve.ends().iter().any(|e| e.distance(at) < 1e-6)
                && outward(&edges[j].curve, at).is_some_and(|o| o.dot(out) < -1.0 + 1e-6)
        });
        if let Some(j) = next {
            chain.push(j);
            ends.extend(edges[j].curve.ends().into_iter().filter(|e| e.distance(at) >= 1e-6));
        }
    }
    chain.into_iter().filter_map(|i| hit(&of.path, of.piece, &of.transform, &part.snaps, i)).collect()
}

/// The edges `ids` (record indices) of the piece `piece` of `path` as shown: the open part's
/// body, or a part's shape (a node's edges lit when it is opened, docs/tools/rounds.md).
pub fn edges_of(scene: &DisplayScene, path: &EntityPath, piece: Piece, ids: &[u32]) -> Vec<EdgeHit> {
    let Some((place, part)) = scene
        .inferable_where(|_| true)
        .into_iter()
        .find(|(p, _, pc)| *pc == piece && (piece != Piece::Shape || p.entity_path() == *path))
        .map(|(p, m, _)| (p, m))
    else {
        return Vec::new();
    };
    (0..part.snaps.edges.len())
        .filter(|&i| ids.contains(&part.snaps.edges[i].id))
        .filter_map(|i| hit(path, piece, &place.transform, &part.snaps, i))
        .collect()
}

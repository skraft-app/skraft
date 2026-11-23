//! Parts born oriented (docs/decisions/0054-changing-a-components-axes.md), after OpenCutList's
//! `_get_auto_orient_transformation`: blue across the part's largest faces, red along its longest
//! edges, each axis pointing towards its material.

use glam::{DMat3, DVec3};

/// Two directions closer than this are parallel; a direction this close to square is square.
const SAME_DIRECTION: f64 = 1e-6;

// What decides the axes' signs, the thickness first: the magnitudes only break ties.
const Z_WEIGHT: f64 = 4.0;
const X_WEIGHT: f64 = 2.0;
const Y_WEIGHT: f64 = 1.0;

/// The direction of the heaviest group of parallel directions, the first on a tie.
fn heaviest(list: impl Iterator<Item = (DVec3, f64)>) -> Option<DVec3> {
    let mut groups: Vec<(DVec3, f64)> = Vec::new();
    for (d, weight) in list {
        match groups.iter_mut().find(|g| g.0.cross(d).length() < SAME_DIRECTION) {
            Some(g) => g.1 += weight,
            None => groups.push((d, weight)),
        }
    }
    let mut best: Option<(DVec3, f64)> = None;
    for g in groups {
        if best.is_none_or(|b| g.1 > b.1 + SAME_DIRECTION) {
            best = Some(g);
        }
    }
    best.map(|b| b.0)
}

/// The new axes of a shape, in its current ones: blue on the normal of the parallel `faces`
/// (normal, area) of the largest total area, red on the parallel `edges` (unit direction, length)
/// square to it of the largest total length. None without such a face and edge.
pub fn axes_on(faces: &[(DVec3, f64)], edges: &[(DVec3, f64)], points: &[DVec3]) -> Option<DMat3> {
    let z = heaviest(faces.iter().copied())?;
    let x = heaviest(edges.iter().copied().filter(|(d, _)| d.dot(z).abs() < SAME_DIRECTION))?;
    Some(signed_axes(points, x, z))
}

/// The direct frame on the directions `x` and `z`, their signs towards the shape's `points`. Y
/// follows from them, so of the four choices the one the weights rank first; a symmetric shape
/// keeps the directions given.
pub fn signed_axes(points: &[DVec3], x: DVec3, z: DVec3) -> DMat3 {
    let y = z.cross(x);
    let (ox, oy, oz) = (offset(points, x), offset(points, y), offset(points, z));
    let (mut best, mut score) = ((1.0, 1.0), f64::NEG_INFINITY);
    for sx in [1.0, -1.0] {
        for sz in [1.0, -1.0] {
            let s = Z_WEIGHT * sz * oz + X_WEIGHT * sx * ox + Y_WEIGHT * sx * sz * oy;
            if s > score + 1e-9 {
                (best, score) = ((sx, sz), s);
            }
        }
    }
    let (x, z) = (x * best.0, z * best.1);
    DMat3::from_cols(x, z.cross(x), z)
}

/// Where `points` sit along `axis` from the origin, in [-1, 1]: 1 all on its positive side, 0
/// centred on it.
fn offset(points: &[DVec3], axis: DVec3) -> f64 {
    let (lo, hi) = points.iter().map(|p| p.dot(axis)).fold((f64::MAX, f64::MIN), |(l, h), d| (l.min(d), h.max(d)));
    if points.is_empty() || (hi - lo).abs() < 1e-9 { 0.0 } else { (lo + hi) / (hi - lo) }
}

//! What the shape tools read of the scene's geometry, in the world: a planar face (its plane,
//! centroid, corners, sides, whether it is a disc) and the straight edge an inference is on.
//! From the display mesh (each triangle knows its B-rep face) and the snap features (exact
//! vertices and edges, docs/ARCHITECTURE.md §6 ter).

use glam::{DAffine3, DVec3};
use solidsketch_kernel::SnapCurve;
use solidsketch_model::EntityPath;

use crate::infer::Inference;
use crate::scene::{DisplayScene, Hit};

/// How far apart (mm) a mesh point and an exact vertex still are the same point: the mesh is
/// `f32` around its origin.
const SAME: f64 = 1e-3;

/// A planar face of the scene, in the world.
#[derive(Clone, Debug)]
pub(super) struct Face {
    /// A point of its plane, and its unit normal towards the eye (as the hit's).
    pub point: DVec3,
    pub normal: DVec3,
    /// Its triangles, on the plane.
    tris: Vec<[DVec3; 3]>,
    /// Its exact corners (vertices of the solid on it).
    pub corners: Vec<DVec3>,
    /// Its straight sides: edges of the solid between two of its corners.
    pub sides: Vec<(DVec3, DVec3)>,
    /// When it is a disc (bounded by arcs of one circle only): its centre and radius.
    pub disc: Option<(DVec3, f64)>,
}

impl Face {
    /// The face `hit` is on, when planar.
    pub fn at(scene: &DisplayScene, hit: &Hit) -> Option<Face> {
        let (t, part) = scene.inferable_at(hit)?;
        let (point, normal) = plane_at(scene, hit)?;
        let flat = |p: DVec3| p - normal * (p - point).dot(normal);

        let m = &part.mesh;
        let at = |i: u32| {
            let [x, y, z] = m.positions[i as usize];
            flat(t.transform_point3(m.origin + DVec3::new(x as f64, y as f64, z as f64)))
        };
        let tris: Vec<[DVec3; 3]> = (m.tri_face.iter().enumerate())
            .filter(|(_, f)| **f == hit.face)
            .map(|(k, _)| [at(m.indices[3 * k]), at(m.indices[3 * k + 1]), at(m.indices[3 * k + 2])])
            .collect();
        if tris.is_empty() {
            return None;
        }
        let on_face = |p: DVec3| (p - point).dot(normal).abs() < SAME && tris.iter().flatten().any(|q| q.distance(p) < SAME);

        let corners: Vec<DVec3> = part.snaps.vertices.iter().map(|v| t.transform_point3(*v)).filter(|v| on_face(*v)).collect();
        let is_corner = |p: DVec3| corners.iter().any(|c| c.distance(p) < SAME);
        let mut sides = Vec::new();
        let mut disc: Option<(DVec3, f64)> = None;
        for e in &part.snaps.edges {
            match e.curve {
                SnapCurve::Segment { a, b } => {
                    let (a, b) = (t.transform_point3(a), t.transform_point3(b));
                    if is_corner(a) && is_corner(b) {
                        sides.push((a, b));
                    }
                }
                // An arc of the face's plane (a circle may be cut into several), the face all
                // within it and reaching it: a disc, if no straight side bounds it too.
                SnapCurve::Arc { center, x, y, radius, .. } => {
                    let c = t.transform_point3(center);
                    let axis = t.transform_vector3(x.cross(y)).normalize();
                    let far = tris.iter().flatten().map(|p| p.distance(c)).fold(0.0, f64::max);
                    if (c - point).dot(normal).abs() < SAME && axis.dot(normal).abs() > 1.0 - 1e-9 && (far - radius).abs() < SAME {
                        disc = Some((c, radius));
                    }
                }
                _ => {}
            }
        }
        let disc = disc.filter(|_| sides.is_empty());
        Some(Face { point, normal, tris, corners, sides, disc })
    }

    /// Its centroid (the centre of its area).
    pub fn centroid(&self) -> DVec3 {
        let (mut sum, mut area) = (DVec3::ZERO, 0.0);
        for [a, b, c] in &self.tris {
            let w = (*b - *a).cross(*c - *a).length();
            sum += (*a + *b + *c) / 3.0 * w;
            area += w;
        }
        if area > 0.0 { sum / area } else { self.tris[0][0] }
    }

    /// Every point of its outline (and inside it): its triangles' corners.
    pub fn points(&self) -> impl Iterator<Item = DVec3> + '_ {
        self.tris.iter().flatten().copied()
    }

    /// Its longest straight side's direction, if any.
    pub fn longest_side(&self) -> Option<DVec3> {
        let (a, b) = self.sides.iter().max_by(|x, y| (x.1 - x.0).length().total_cmp(&(y.1 - y.0).length()))?;
        (*b - *a).try_normalize()
    }

    /// Its corner nearest `p`, with its sides there (from it outwards, unit).
    pub fn corner_near(&self, p: DVec3) -> Option<(DVec3, Vec<DVec3>)> {
        let v = *self.corners.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))?;
        let sides: Vec<DVec3> = self
            .sides
            .iter()
            .filter_map(|(a, b)| {
                if a.distance(v) < SAME {
                    (*b - *a).try_normalize()
                } else if b.distance(v) < SAME {
                    (*a - *b).try_normalize()
                } else {
                    None
                }
            })
            .collect();
        Some((v, sides))
    }
}

/// The plane of the face `hit` is on, when planar (in the world): a point of it and its unit
/// normal towards the eye (as the hit's). Cheaper than [`Face::at`].
pub(super) fn plane_at(scene: &DisplayScene, hit: &Hit) -> Option<(DVec3, DVec3)> {
    let (t, part) = scene.inferable_at(hit)?;
    let plane = part.snaps.planes.get(hit.face as usize).copied().flatten()?;
    let mut normal = t.matrix3.inverse().transpose().mul_vec3(plane.normal).normalize();
    if normal.dot(hit.normal) < 0.0 {
        normal = -normal;
    }
    Some((t.transform_point3(plane.point), normal))
}

/// The straight edge `inf` is on (its ends, in the world), looked for on the solid it names.
pub(super) fn segment_at(scene: &DisplayScene, inf: &Inference) -> Option<(DVec3, DVec3)> {
    let path: &EntityPath = inf.path.as_ref()?;
    let p = inf.point;
    let on = |t: &DAffine3, a: DVec3, b: DVec3| {
        let (a, b) = (t.transform_point3(a), t.transform_point3(b));
        let d = b - a;
        let k = ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
        (p.distance(a + d * k) < SAME).then_some((a, b))
    };
    scene.inferable_of(path).into_iter().find_map(|(place, part, _)| {
        part.snaps.edges.iter().find_map(|e| match e.curve {
            SnapCurve::Segment { a, b } => on(&place.transform, a, b),
            _ => None,
        })
    })
}

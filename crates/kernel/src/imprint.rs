//! The tool an imprint cuts (docs/decisions/0055-linked-bodies-through-imprints.md): the source's
//! whole final solid, however it was built. Only its faces standing as walls inside the receiver
//! are moved: the one facing an opening is the **bottom**, made deeper; the others are **cheeks**,
//! given the clearance. Everything else of the source cuts nothing where it is.

use serde::{Deserialize, Serialize};

/// How an imprint's tool differs from its source inside the receiver (mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clearance {
    /// On its cheeks.
    pub around: f64,
    /// On its bottom.
    pub deep: f64,
}

impl Clearance {
    pub fn is_valid(&self) -> bool {
        self.around.is_finite() && self.deep.is_finite()
    }
}

#[cfg(feature = "engine")]
mod build {
    use glam::{DAffine3, DVec3};
    use solidcore_ops::{Body, PointClass};

    use super::Clearance;
    use crate::engine::{guarded, kernel};
    use crate::{BooleanOp, Error, Primitive, Result, Solid};

    const CALL: &str = "imprint";
    /// How far off a face a point is taken to tell what lies beyond it (mm).
    const STEP: f64 = 0.01;
    /// Less material than this in common is no contact (mm³): faces merely touching.
    const NO_CONTACT: f64 = 1e-6;

    fn refused(message: impl ToString) -> Error {
        Error::Kernel { call: CALL.into(), message: message.to_string() }
    }

    /// What a wall of the contact is.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Kind {
        Bottom,
        Cheek,
    }

    /// A wall: a point on it, its outward normal there, what it is.
    #[derive(Clone, Copy, Debug)]
    struct Wall {
        at: DVec3,
        normal: DVec3,
        kind: Kind,
    }

    impl Solid {
        /// What `source`, placed by `placed` in this solid's axes, has in common with it: empty
        /// when they only touch or do not meet.
        fn contact_of(&self, source: &Solid, placed: &DAffine3) -> Result<Solid> {
            if self.is_empty() || source.is_empty() {
                return Ok(Solid::empty());
            }
            let common = match self.boolean(&source.transformed(placed)?, BooleanOp::Intersection) {
                Ok(common) => common,
                Err(Error::Empty(_)) => return Ok(Solid::empty()),
                Err(e) => return Err(e),
            };
            Ok(if common.is_empty() || common.volume()? <= NO_CONTACT { Solid::empty() } else { common })
        }

        /// Does `source`, placed by `placed` in this solid's axes, share material with it?
        pub fn has_contact(&self, source: &Solid, placed: &DAffine3) -> Result<bool> {
            Ok(!self.contact_of(source, placed)?.is_empty())
        }

        /// The tool `source` (placed by `placed` in this receiver's axes) cuts in this receiver,
        /// in the source's axes. No contact: `previous`, as it was, or a refusal. A clearance
        /// that cannot be built is refused, never reduced.
        pub fn imprint_tool(&self, source: &Solid, placed: &DAffine3, clearance: &Clearance, previous: Option<&Solid>) -> Result<Solid> {
            let contact = self.contact_of(source, placed)?;
            if contact.is_empty() {
                return previous.cloned().ok_or_else(|| refused("the parts share no material"));
            }
            grown(source, &contact, self, placed, clearance)
        }
    }

    /// The walls of `contact`: its faces with `receiver`'s material past them, each lying on the
    /// source. One whose way across the contact ends on an opening is the bottom.
    fn walls_of(contact: &Solid, receiver: &Solid) -> Result<Vec<Wall>> {
        let (body, order) = Body::from_record(contact.record(), true).map_err(refused)?;
        let mesh = contact.mesh()?;
        let count = contact.record().faces.len();
        // On each face, its largest triangle's centre and normal: a point well inside it.
        let mut best: Vec<Option<(f64, DVec3, DVec3)>> = vec![None; count];
        for (tri, &f) in mesh.indices.chunks_exact(3).zip(&mesh.tri_face) {
            let at = |k: usize| mesh.origin + DVec3::from_array(mesh.positions[tri[k] as usize].map(f64::from));
            let normal = |k: usize| DVec3::from_array(mesh.normals[tri[k] as usize].map(f64::from));
            let (a, b, c) = (at(0), at(1), at(2));
            let area = (b - a).cross(c - a).length();
            if best[f as usize].is_none_or(|(most, ..)| area > most) {
                let n = (normal(0) + normal(1) + normal(2)).normalize_or_zero();
                best[f as usize] = Some((area, (a + b + c) / 3.0, n));
            }
        }
        let hit = |origin: DVec3, along: DVec3| body.face_hit_by_ray(&order, origin.to_array(), along.to_array()).map_err(refused);
        // The exact point of each face, from just inside it: the mesh's lies on a chord.
        let mut samples = Vec::with_capacity(count);
        for (i, b) in best.iter().enumerate() {
            let (_, p, n) = b.ok_or_else(|| refused(format!("face {i} has no mesh")))?;
            if n == DVec3::ZERO {
                return Err(refused(format!("face {i} has no normal")));
            }
            let from = p - n * STEP;
            let on = hit(from, n)?.filter(|h| h.face == i).map_or(p, |h| DVec3::from_array(h.point));
            samples.push((on, n));
        }
        let (other, _) = Body::from_record(receiver.record(), true).map_err(refused)?;
        let past: Vec<[f64; 3]> = samples.iter().map(|(p, n)| (*p + *n * STEP).to_array()).collect();
        let classes = other.classify_points(&past).map_err(refused)?;
        let inside: Vec<bool> = classes.into_iter().map(|c| matches!(c, Ok(PointClass::Inside))).collect();
        let mut walls = Vec::new();
        for (&(at, normal), _) in samples.iter().zip(&inside).filter(|(_, wall)| **wall) {
            let kind = match hit(at - normal * STEP, -normal)? {
                Some(across) if across.face < inside.len() && !inside[across.face] => Kind::Bottom,
                _ => Kind::Cheek,
            };
            walls.push(Wall { at, normal, kind });
        }
        Ok(walls)
    }

    /// `walls`, in the source's axes, one per face of `source` (its bottom first: a face that is
    /// both is moved once).
    fn on_source(source: &Solid, walls: &[Wall], placed: &DAffine3) -> Result<Vec<Wall>> {
        let (body, order) = Body::from_record(source.record(), true).map_err(refused)?;
        let back = placed.inverse();
        let mut ordered: Vec<Wall> = walls
            .iter()
            .map(|w| Wall { at: back.transform_point3(w.at), normal: back.transform_vector3(w.normal).normalize(), kind: w.kind })
            .collect();
        ordered.sort_by_key(|w| w.kind != Kind::Bottom);
        let mut seen = Vec::with_capacity(ordered.len());
        let mut out = Vec::with_capacity(ordered.len());
        for w in ordered {
            let origin = w.at - w.normal * STEP;
            let found = body.face_hit_by_ray(&order, origin.to_array(), w.normal.to_array()).map_err(refused)?;
            let face = found.ok_or_else(|| refused("a wall of the contact is not on the source"))?.face;
            if !seen.contains(&face) {
                seen.push(face);
                out.push(w);
            }
        }
        Ok(out)
    }

    /// A source that is a box square to its axes, its walls moved side by side: exact.
    fn as_box(source: &Solid, walls: &[Wall], c: &Clearance) -> Option<Primitive> {
        if !source.fills_its_box() {
            return None;
        }
        let (mut lo, mut hi) = source.vertex_bounds()?;
        for w in walls {
            let k = w.normal.abs().max_position();
            if (w.normal[k].abs() - 1.0).abs() > 1e-6 {
                return None;
            }
            let d = if w.kind == Kind::Bottom { c.deep } else { c.around };
            if w.normal[k] > 0.0 {
                hi[k] += d;
            } else {
                lo[k] -= d;
            }
        }
        (hi - lo).min_element().gt(&0.0).then_some(Primitive::Box { origin: lo, size: hi - lo })
    }

    /// `source` with its walls moved kind by kind, in `steps`' order. A move makes the body anew,
    /// so each face is found again by its point, moved with it.
    fn moved_walls(source: &Solid, walls: &[Wall], steps: [(Kind, f64); 2]) -> Result<Solid> {
        let (body, _) = Body::from_record(source.record(), true).map_err(refused)?;
        let mut walls = walls.to_vec();
        for (kind, d) in steps {
            if d == 0.0 || !walls.iter().any(|w| w.kind == kind) {
                continue;
            }
            let (_, order) = body.to_record().map_err(refused)?;
            let mut list = Vec::new();
            for w in walls.iter().filter(|w| w.kind == kind) {
                let origin = w.at - w.normal * STEP;
                let found = body.face_hit_by_ray(&order, origin.to_array(), w.normal.to_array()).map_err(refused)?;
                list.push(found.ok_or_else(|| refused("a wall of the source was lost"))?.face);
            }
            list.sort_unstable();
            list.dedup();
            solidcore_ops::offset_faces(&body, &order, &list, d).map_err(refused)?;
            walls.iter_mut().filter(|w| w.kind == kind).for_each(|w| w.at += w.normal * d);
        }
        let (record, _) = body.to_record().map_err(refused)?;
        Ok(Solid::from_record(record))
    }

    fn grown(source: &Solid, contact: &Solid, receiver: &Solid, placed: &DAffine3, c: &Clearance) -> Result<Solid> {
        if c.around == 0.0 && c.deep == 0.0 {
            return Ok(source.clone());
        }
        // SolidCore's fixes are switched on with its first kernel.
        drop(kernel());
        guarded(CALL, || {
            let walls = on_source(source, &walls_of(contact, receiver)?, placed)?;
            if let Some(tool) = as_box(source, &walls, c) {
                return tool.build();
            }
            let (bottom, cheek) = ((Kind::Bottom, c.deep), (Kind::Cheek, c.around));
            // SolidCore takes one order or the other, depending on how the faces meet.
            moved_walls(source, &walls, [bottom, cheek]).or_else(|_| moved_walls(source, &walls, [cheek, bottom]))
        })
    }
}

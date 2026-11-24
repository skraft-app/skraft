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
    use glam::{DAffine3, DMat3, DVec2, DVec3};
    use solidcore_ops::{Body, PointClass};
    use solidcore_record::brep::{Curve, Loop, Shell, Surface};

    use super::Clearance;
    use crate::engine::{guarded, kernel};
    use crate::profile::{self, Corner};
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

    /// A wall: a point on it, its outward normal there, what it is, and the face of the source
    /// it lies on (once found there: [`on_source`]).
    #[derive(Clone, Copy, Debug)]
    struct Wall {
        at: DVec3,
        normal: DVec3,
        kind: Kind,
        face: Option<usize>,
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
            walls.push(Wall { at, normal, kind, face: None });
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
            .map(|w| Wall { at: back.transform_point3(w.at), normal: back.transform_vector3(w.normal).normalize(), ..*w })
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
                out.push(Wall { face: Some(face), ..w });
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

    /// A source that is a right prism (two planar ends, every other face a plane or a cylinder
    /// along its axis, as a dowel or a filleted tenon), all its sides cheeks and each end the
    /// bottom or no wall: its end's outline offset by the profile and extruded again, the bottom
    /// end run deeper. Exact, and milliseconds where `offset_faces` takes seconds
    /// (docs/decisions/0055-linked-bodies-through-imprints.md, « Clearance »). `None` for any
    /// other source, or an outline that does not give back the source's volume.
    fn as_prism(source: &Solid, walls: &[Wall], c: &Clearance) -> Option<Solid> {
        let r = source.record();
        if source.pieces() != 1 || r.shells.iter().filter(|s| matches!(s, Shell::Faces { .. })).count() != 1 {
            return None;
        }
        let v3 = |a: [f64; 3]| DVec3::from_array(a);
        let axis_of = |f: &solidcore_record::brep::Face| -> Option<(DVec3, bool)> {
            // (direction, true for a plane) of the face's surface.
            match &f.surface {
                Surface::Cylinder { frame, .. } => Some((v3(frame.x_axis).cross(v3(frame.y_axis)).normalize(), false)),
                s => crate::snaps::plane_of(s).map(|p| (p.normal, true)),
            }
        };
        let kinds: Vec<(DVec3, bool)> = r.faces.iter().map(axis_of).collect::<Option<_>>()?;
        const SQUARE: f64 = 1e-9;
        // The axis: a planar face's normal along which every face is an end or a side.
        let (axis, ends) = kinds.iter().filter(|(_, plane)| *plane).find_map(|&(d, _)| {
            let mut ends = Vec::new();
            for (i, &(n, plane)) in kinds.iter().enumerate() {
                let along = n.dot(d).abs();
                if plane && (along - 1.0).abs() <= SQUARE {
                    ends.push(i);
                } else if !(plane && along <= SQUARE || !plane && (along - 1.0).abs() <= SQUARE) {
                    return None;
                }
            }
            (ends.len() == 2).then_some((d, ends))
        })?;
        let height_of = |i: usize| crate::snaps::plane_of(&r.faces[i].surface).map(|p| p.point.dot(axis));
        let (mut low, mut high) = (ends[0], ends[1]);
        let (mut lo, mut hi) = (height_of(low)?, height_of(high)?);
        if lo > hi {
            (low, high, lo, hi) = (high, low, hi, lo);
        }
        // Every side a cheek, each end the bottom or no wall.
        let kind_of = |i: usize| walls.iter().find(|w| w.face == Some(i)).map(|w| w.kind);
        for i in 0..r.faces.len() {
            let kind = kind_of(i);
            let fits = if i == low || i == high { kind != Some(Kind::Cheek) } else { kind == Some(Kind::Cheek) };
            if !fits {
                return None;
            }
        }
        // The low end's outline, in a frame whose third axis is the prism's.
        let [Loop::Edges { edges }] = r.faces[low].loops.as_slice() else { return None };
        let at = v3(r.vertices[r.edges[edges.first()?.edge as usize].vertices[0] as usize].point);
        let u = axis.any_orthonormal_vector();
        let frame = DMat3::from_cols(u, axis.cross(u), axis);
        let flat = |p: DVec3| {
            let q = frame.transpose() * (p - at);
            DVec2::new(q.x, q.y)
        };
        let mut outline = Vec::new();
        for e in edges {
            let edge = &r.edges[e.edge as usize];
            let [a, b] = edge.vertices.map(|k| v3(r.vertices[k as usize].point));
            let from = if e.forward { a } else { b };
            match &edge.curve {
                Curve::Line { .. } => outline.push(Corner { p: flat(from), bulge: 0.0, fillet: 0.0, id: 0 }),
                Curve::Circle { frame: f, .. } => {
                    // The edge runs counter-clockwise about its circle's normal from its start to
                    // its end; a closed one is a whole turn.
                    let (centre, normal) = (v3(f.origin), v3(f.x_axis).cross(v3(f.y_axis)).normalize());
                    if normal.cross(axis).length() > SQUARE {
                        return None;
                    }
                    let (sa, sb) = (a - centre, b - centre);
                    let mut sweep = sa.cross(sb).dot(normal).atan2(sa.dot(sb));
                    if sweep <= 1e-12 {
                        sweep += std::f64::consts::TAU;
                    }
                    let sweep = sweep * normal.dot(axis).signum() * if e.forward { 1.0 } else { -1.0 };
                    // Halves, a side's sweep staying under a half turn.
                    let (c2, f2) = (flat(centre), flat(from));
                    let middle = c2 + DVec2::from_angle(sweep / 2.0).rotate(f2 - c2);
                    let bulge = profile::bulge_of(sweep / 2.0);
                    outline.push(Corner { p: f2, bulge, fillet: 0.0, id: 0 });
                    outline.push(Corner { p: middle, bulge, fillet: 0.0, id: 0 });
                }
                _ => return None,
            }
        }
        // The outline read rightly gives back the source's volume.
        let volume = source.volume().ok()?;
        if ((profile::signed_area2(&outline) / 2.0).abs() * (hi - lo) - volume).abs() > 1e-6 * volume {
            return None;
        }
        let grown = profile::offset(&outline, c.around).ok()?;
        let deeper = |end: usize| if kind_of(end) == Some(Kind::Bottom) { c.deep } else { 0.0 };
        let (from, to) = (lo - deeper(low), hi + deeper(high));
        if to <= from {
            return None;
        }
        let tool = Primitive::Prism { origin: DVec3::ZERO, profile: grown, height: to - from }.build().ok()?;
        tool.transformed(&DAffine3::from_mat3_translation(frame, at + axis * (from - lo))).ok()
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
            if let Some(tool) = as_prism(source, &walls, c) {
                return Ok(tool);
            }
            let (bottom, cheek) = ((Kind::Bottom, c.deep), (Kind::Cheek, c.around));
            // SolidCore takes one order or the other, depending on how the faces meet.
            moved_walls(source, &walls, [bottom, cheek]).or_else(|_| moved_walls(source, &walls, [cheek, bottom]))
        })
    }

    #[cfg(test)]
    mod tests {
        use glam::{DAffine3, DVec2, DVec3};

        use super::*;

        fn walls(receiver: &Solid, source: &Solid) -> Vec<Wall> {
            let contact = receiver.contact_of(source, &DAffine3::IDENTITY).unwrap();
            on_source(source, &walls_of(&contact, receiver).unwrap(), &DAffine3::IDENTITY).unwrap()
        }

        fn prism(points: &[[f64; 2]], fillet: f64, at: DVec3, height: f64) -> Solid {
            let mut outline = profile::polygon(&points.iter().map(|p| DVec2::from_array(*p)).collect::<Vec<_>>());
            profile::settle_ids(&mut outline);
            outline.iter_mut().for_each(|c| c.fillet = fillet);
            Primitive::Prism { origin: at, profile: outline, height }.build().unwrap()
        }

        /// `as_prism`'s tool is `offset_faces`' on each source, in `block`.
        fn same_as_offset_faces(block: &Solid, sources: &[Solid]) {
            let c = Clearance { around: 0.1, deep: 2.0 };
            for (i, source) in sources.iter().enumerate() {
                let walls = walls(block, source);
                let fast = as_prism(source, &walls, &c).unwrap_or_else(|| panic!("source {i}: not read as a prism"));
                let steps = [(Kind::Bottom, c.deep), (Kind::Cheek, c.around)];
                let slow = moved_walls(source, &walls, steps).or_else(|_| moved_walls(source, &walls, [steps[1], steps[0]])).unwrap();
                let (vf, vs) = (fast.volume().unwrap(), slow.volume().unwrap());
                assert!((vf - vs).abs() <= 1e-6 * vs, "source {i}: {vf} against offset_faces' {vs}");
                let both = fast.boolean(&slow, BooleanOp::Intersection).unwrap().volume().unwrap();
                assert!((both - vs).abs() <= 1e-6 * vs, "source {i}: the same volume, elsewhere ({both} in common of {vs})");
            }
        }

        fn block() -> Solid {
            Primitive::Box { origin: DVec3::new(-30.0, -30.0, 0.0), size: DVec3::new(100.0, 70.0, 60.0) }.build().unwrap()
        }

        #[test]
        fn the_profile_gives_the_tool_offset_faces_gives() {
            // Lying along X, through the block's end: its far end no wall.
            let along_x = DAffine3::from_translation(DVec3::new(40.0, 0.0, 20.0)) * DAffine3::from_rotation_y(std::f64::consts::FRAC_PI_2);
            let trapezoid =
                prism(&[[0.0, 0.0], [12.0, 0.0], [12.0, 6.0], [0.0, 9.0]], 0.0, DVec3::ZERO, 50.0).transformed(&along_x).unwrap();
            same_as_offset_faces(&block(), &[trapezoid]);
        }

        /// The same on round sides, which `offset_faces` takes seconds over (run when the
        /// outline's reading changes: `cargo test --workspace --lib -- --ignored imprint`).
        #[test]
        #[ignore = "offset_faces on round sides: about 14 s"]
        fn the_profile_gives_the_tool_offset_faces_gives_on_round_sides() {
            let dowel = Primitive::Cylinder { origin: DVec3::new(0.0, 0.0, 38.0), radius: 4.0, height: 50.0 }.build().unwrap();
            let tenon = prism(&[[0.0, 0.0], [40.0, 0.0], [40.0, 10.0], [0.0, 10.0]], 3.0, DVec3::new(0.0, 0.0, 38.0), 60.0);
            same_as_offset_faces(&block(), &[dowel, tenon]);
        }

        #[test]
        fn a_side_flush_with_the_receiver_is_left_to_offset_faces() {
            // An L tenon, its side x = 0 on the block's: that side is no wall, it does not move.
            let block = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 100.0, 60.0) }.build().unwrap();
            let tenon = prism(
                &[[0.0, 40.0], [30.0, 40.0], [30.0, 50.0], [10.0, 50.0], [10.0, 60.0], [0.0, 60.0]],
                0.0,
                DVec3::new(0.0, 0.0, 38.0),
                60.0,
            );
            let c = Clearance { around: 0.1, deep: 2.0 };
            assert!(as_prism(&tenon, &walls(&block, &tenon), &c).is_none());
            assert!(block.imprint_tool(&tenon, &DAffine3::IDENTITY, &c, None).is_ok());
        }
    }
}

//! The tool an imprint cuts (docs/decisions/0055-linked-bodies-through-imprints.md): a body grown
//! by a clearance around it, deeper at its end, run back past its shoulder. From the cheapest
//! way that holds: a box stays a box, an extrusion along the axis is drawn again from its profile
//! offset, anything else has its faces offset by SolidCore.

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::Solid;
use crate::snaps::{Plane, plane_of};

/// How an imprint differs from the body it is made of (mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clearance {
    /// From the shoulder to the end, in the body's axes; none for a body joined to nothing,
    /// which has neither.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<DVec3>,
    /// All around, the end and the shoulder apart.
    pub around: f64,
    /// Past the end.
    pub deep: f64,
    /// Back past the shoulder.
    pub back: f64,
}

impl Clearance {
    /// Finite, a unit axis, and not run back by a negative length.
    pub fn is_valid(&self) -> bool {
        self.around.is_finite()
            && self.deep.is_finite()
            && self.back.is_finite()
            && self.back >= 0.0
            && self.axis.is_none_or(|a| a.is_finite() && (a.length_squared() - 1.0).abs() < 1e-9)
    }
}

/// A planar face of a solid: its plane, and the vertices of its boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanarFace {
    pub plane: Plane,
    pub vertices: Vec<DVec3>,
}

impl Solid {
    /// Its planar faces, read from the record (no kernel).
    pub fn planar_faces(&self) -> Vec<PlanarFace> {
        use solidcore_record::brep::Loop;
        let r = self.record();
        r.faces
            .iter()
            .filter_map(|f| {
                let plane = plane_of(&f.surface)?;
                let mut ids: Vec<u32> = f
                    .loops
                    .iter()
                    .flat_map(|l| match l {
                        Loop::Edges { edges } => edges.iter().flat_map(|e| r.edges[e.edge as usize].vertices).collect::<Vec<_>>(),
                        Loop::Vertex { vertex } => vec![*vertex],
                    })
                    .collect();
                ids.sort_unstable();
                ids.dedup();
                Some(PlanarFace { plane, vertices: ids.iter().map(|v| DVec3::from_array(r.vertices[*v as usize].point)).collect() })
            })
            .collect()
    }
}

#[cfg(feature = "engine")]
mod build {
    use glam::{DAffine3, DQuat, DVec3};

    use super::Clearance;
    use crate::engine::{guarded, kernel};
    use crate::{Error, Primitive, Result, Solid, profile};

    const CALL: &str = "imprint";
    /// How far from a unit axis a direction may be and still lie along it; how flat a face is.
    const ALONG: f64 = 1e-9;
    const FLAT: f64 = 1e-3;

    fn refused(message: impl ToString) -> Error {
        Error::Kernel { call: CALL.into(), message: message.to_string() }
    }

    impl Solid {
        /// This body as the tool of an imprint. `recipe`: the primitive it is built from, if
        /// any. Refused rather than reduced when the clearance cannot be built.
        pub fn imprinted(&self, recipe: Option<&Primitive>, c: &Clearance) -> Result<Solid> {
            if c.around == 0.0 && (c.axis.is_none() || (c.deep == 0.0 && c.back == 0.0)) {
                return Ok(self.clone());
            }
            if let Some(tool) = as_box(self, c) {
                return tool.build();
            }
            if let Some(tool) = from_profile(recipe, c)? {
                return tool.build();
            }
            by_faces(self, c)
        }
    }

    /// A body filling its box, its axis along one of the box's: the box grown.
    fn as_box(s: &Solid, c: &Clearance) -> Option<Primitive> {
        if !s.fills_its_box() {
            return None;
        }
        let (mut lo, mut hi) = s.vertex_bounds()?;
        let along = match c.axis {
            Some(a) => Some((0..3).find(|k| (a[*k].abs() - 1.0).abs() < ALONG).map(|k| (k, a[k] > 0.0))?),
            None => None,
        };
        for k in 0..3 {
            let (down, up) = match along {
                Some((axis, true)) if axis == k => (c.back, c.deep),
                Some((axis, false)) if axis == k => (c.deep, c.back),
                _ => (c.around, c.around),
            };
            (lo[k], hi[k]) = (lo[k] - down, hi[k] + up);
        }
        (hi - lo).min_element().gt(&0.0).then_some(Primitive::Box { origin: lo, size: hi - lo })
    }

    /// An extrusion along the axis: its profile offset, its height grown.
    fn from_profile(recipe: Option<&Primitive>, c: &Clearance) -> Result<Option<Primitive>> {
        let (Some(Primitive::Prism { origin, profile: outline, height }), Some(axis)) = (recipe, c.axis) else { return Ok(None) };
        if (axis.z.abs() - 1.0).abs() >= ALONG {
            return Ok(None);
        }
        let outline = match c.around {
            0.0 => outline.clone(),
            d => profile::offset(&profile::rounded(outline), d).map_err(|e| refused(format!("{e:?}")))?,
        };
        let (below, above) = if axis.z > 0.0 { (c.back, c.deep) } else { (c.deep, c.back) };
        let height = height + below + above;
        if height <= 0.0 {
            return Err(refused("nothing left of the height"));
        }
        Ok(Some(Primitive::Prism { origin: *origin - DVec3::Z * below, profile: outline, height }))
    }

    /// Each face's box, by its record index, read on the display mesh.
    fn face_boxes(s: &Solid) -> Result<Vec<(DVec3, DVec3)>> {
        let m = s.mesh()?;
        let n = m.tri_face.iter().max().map_or(0, |f| *f as usize + 1);
        let mut out = vec![(DVec3::INFINITY, DVec3::NEG_INFINITY); n];
        for (tri, &f) in m.indices.chunks_exact(3).zip(&m.tri_face) {
            for &i in tri {
                let p = m.origin + DVec3::from_array(m.positions[i as usize].map(f64::from));
                let b = &mut out[f as usize];
                *b = (b.0.min(p), b.1.max(p));
            }
        }
        Ok(out)
    }

    fn offset(s: &Solid, faces: &[usize], d: f64) -> Result<Solid> {
        // SolidCore's fixes are switched on with its first kernel.
        drop(kernel());
        guarded(CALL, || {
            let (body, order) = solidcore_ops::Body::from_record(s.record(), true).map_err(refused)?;
            solidcore_ops::offset_faces(&body, &order, faces, d).map_err(refused)?;
            let (record, _) = body.to_record().map_err(refused)?;
            Ok(Solid::from_record(record))
        })
    }

    fn flat_at(b: &(DVec3, DVec3), z: f64) -> bool {
        (b.0.z - z).abs() < FLAT && (b.1.z - z).abs() < FLAT
    }

    /// The faces at the height `z`, flat there.
    fn faces_at(s: &Solid, z: f64) -> Result<Vec<usize>> {
        Ok(face_boxes(s)?.iter().enumerate().filter(|(_, b)| flat_at(b, z)).map(|(i, _)| i).collect())
    }

    /// Any other body: its faces offset, in axes where the imprint's axis is +Z.
    fn by_faces(s: &Solid, c: &Clearance) -> Result<Solid> {
        let Some(axis) = c.axis else {
            let all: Vec<usize> = (0..s.record().faces.len()).collect();
            return offset(s, &all, c.around);
        };
        let to_z = DAffine3::from_quat(DQuat::from_rotation_arc(axis, DVec3::Z));
        let mut tool = s.transformed(&to_z)?;
        let (lo, hi) = tool.bounds()?;
        if c.around != 0.0 {
            let sides: Vec<usize> =
                face_boxes(&tool)?.iter().enumerate().filter(|(_, b)| !flat_at(b, lo.z) && !flat_at(b, hi.z)).map(|(i, _)| i).collect();
            tool = offset(&tool, &sides, c.around)?;
        }
        for (z, d, what) in [(hi.z, c.deep, "no flat end"), (lo.z, c.back, "no flat shoulder")] {
            if d != 0.0 {
                let faces = faces_at(&tool, z)?;
                if faces.is_empty() {
                    return Err(refused(what));
                }
                tool = offset(&tool, &faces, d)?;
            }
        }
        tool.transformed(&to_z.inverse())
    }
}

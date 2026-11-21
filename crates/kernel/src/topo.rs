//! A solid's topology read from its record, for what points at its vertices and edges (a
//! drawing's dimensions, docs/ui/drawing.md#dimensions): their places, the faces around them,
//! an edge's curve. No kernel: the record alone.

use glam::DVec3;
use solidcore_record::brep::Curve;

use crate::Solid;

/// An edge's shape, in the solid's axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeShape {
    /// A straight edge from `a` to `b`.
    Line { a: DVec3, b: DVec3 },
    /// A circle's arc (or the whole circle: `a == b`), from `a` to `b`.
    Circle { centre: DVec3, normal: DVec3, radius: f64, a: DVec3, b: DVec3 },
    /// Any other curve: its ends.
    Other { a: DVec3, b: DVec3 },
}

impl EdgeShape {
    /// Its ends.
    pub fn ends(&self) -> (DVec3, DVec3) {
        match *self {
            EdgeShape::Line { a, b } | EdgeShape::Circle { a, b, .. } | EdgeShape::Other { a, b } => (a, b),
        }
    }
}

fn v3(p: [f64; 3]) -> DVec3 {
    DVec3::from_array(p)
}

impl Solid {
    /// Its vertices' places.
    pub fn vertex_points(&self) -> Vec<DVec3> {
        self.record.vertices.iter().map(|v| v3(v.point)).collect()
    }

    /// The faces around vertex `v` (of its edges), each once.
    pub fn vertex_faces(&self, v: u32) -> Vec<u32> {
        let mut out: Vec<u32> =
            self.record.edges.iter().filter(|e| e.vertices.contains(&v)).flat_map(|e| e.uses.iter().map(|u| u.face)).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// How many edges it has.
    pub fn edge_count(&self) -> usize {
        self.record.edges.len()
    }

    /// The faces edge `e` lies between (two for a manifold edge).
    pub fn edge_faces(&self, e: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self.record.edges.get(e as usize).map_or_else(Vec::new, |e| e.uses.iter().map(|u| u.face).collect());
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Edge `e`'s shape.
    pub fn edge_shape(&self, e: u32) -> Option<EdgeShape> {
        let edge = self.record.edges.get(e as usize)?;
        let point = |i: u32| self.record.vertices.get(i as usize).map(|v| v3(v.point));
        let (a, b) = (point(edge.vertices[0])?, point(edge.vertices[1])?);
        Some(match &edge.curve {
            Curve::Line { .. } => EdgeShape::Line { a, b },
            Curve::Circle { frame, radius } => {
                let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
                EdgeShape::Circle { centre: v3(frame.origin), normal: x.cross(y).normalize_or_zero(), radius: *radius, a, b }
            }
            _ => EdgeShape::Other { a, b },
        })
    }
}

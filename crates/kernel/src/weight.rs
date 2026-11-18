//! What a solid, a mesh and snaps weigh in memory, roughly: the undo history's budget
//! (docs/model/undo.md#memory-cap) adds up what its past states hold alone.
//!
//! An estimate: the structs' sizes and their vectors' lengths, not the allocator's overhead nor
//! spare capacity. Walking a record is linear in its size, far cheaper than meshing it.

use std::mem::size_of;

use solidcore_record::brep::{Brep, Curve, Curve2, Edge, Face, Loop, Surface, blend::BlendDefinition};

use crate::{Mesh, SnapCurve, Snaps, Solid};

/// The bytes of `v`'s items, `T` sized (their own heap aside).
fn items<T>(v: &[T]) -> u64 {
    std::mem::size_of_val(v) as u64
}

fn curve(c: &Curve) -> u64 {
    match c {
        Curve::Polyline { points } => items(points),
        Curve::Spline { knots, points } => items(knots) + items(points),
        Curve::RationalSpline { knots, homogeneous } => items(knots) + items(homogeneous),
        Curve::SurfaceIntersection { surfaces, leader, .. } => {
            surfaces.iter().map(|s| size_of::<Surface>() as u64 + surface(s)).sum::<u64>() + size_of::<Curve>() as u64 + curve(leader)
        }
        Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. } => 0,
    }
}

fn curve2(c: &Curve2) -> u64 {
    match c {
        Curve2::Spline { knots, points } => items(knots) + items(points),
        Curve2::RationalSpline { knots, homogeneous } => items(knots) + items(homogeneous),
    }
}

/// A surface's heap (its struct counted by its holder).
fn surface(s: &Surface) -> u64 {
    match s {
        Surface::Spline { u_knots, v_knots, points } => {
            items(u_knots) + items(v_knots) + items(points) + points.iter().map(|r| items(r)).sum::<u64>()
        }
        Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
            items(u_knots) + items(v_knots) + items(homogeneous) + homogeneous.iter().map(|r| items(r)).sum::<u64>()
        }
        Surface::PlacedRevolution { profile, .. } => size_of::<Curve>() as u64 + curve(profile),
        // Its supports, leader and seeds are NURBS of their own: a flat guess.
        Surface::Blend { .. } => size_of::<BlendDefinition>() as u64 + 4096,
        Surface::Plane { .. }
        | Surface::PlaneThrough { .. }
        | Surface::Cylinder { .. }
        | Surface::Cone { .. }
        | Surface::Sphere { .. }
        | Surface::Torus { .. }
        | Surface::BallPatch { .. } => 0,
    }
}

fn face(f: &Face) -> u64 {
    let loops = f.loops.iter().map(|l| match l {
        Loop::Edges { edges } => items(edges) + edges.iter().map(|e| curve2(&e.pcurve)).sum::<u64>(),
        Loop::Vertex { .. } => 0,
    });
    surface(&f.surface) + f.nurbs.as_ref().map_or(0, surface) + items(&f.loops) + loops.sum::<u64>()
}

fn edge(e: &Edge) -> u64 {
    curve(&e.curve) + e.nurbs.as_ref().map_or(0, curve) + items(&e.uses)
}

fn brep(b: &Brep) -> u64 {
    let regions = b.regions.iter().map(|r| items(&r.shells)).sum::<u64>();
    let shells = b.shells.iter().map(|s| match s {
        solidcore_record::brep::Shell::Faces { faces, wires } => items(faces) + items(wires),
        solidcore_record::brep::Shell::Wire { edges } => items(edges),
        solidcore_record::brep::Shell::Vertex { .. } => 0,
    });
    size_of::<Brep>() as u64
        + items(&b.regions)
        + regions
        + items(&b.shells)
        + shells.sum::<u64>()
        + items(&b.faces)
        + b.faces.iter().map(face).sum::<u64>()
        + items(&b.edges)
        + b.edges.iter().map(edge).sum::<u64>()
        + items(&b.vertices)
}

impl Solid {
    /// Roughly the bytes it holds: its record.
    pub fn approx_bytes(&self) -> u64 {
        size_of::<Solid>() as u64 + brep(&self.record)
    }
}

impl Mesh {
    /// Roughly the bytes it holds.
    pub fn approx_bytes(&self) -> u64 {
        size_of::<Mesh>() as u64
            + items(&self.positions)
            + items(&self.normals)
            + items(&self.indices)
            + items(&self.tri_face)
            + items(&self.edges)
            + self.edges.iter().map(|e| items(e)).sum::<u64>()
            + items(&self.edge_ids)
    }
}

impl Snaps {
    /// Roughly the bytes it holds.
    pub fn approx_bytes(&self) -> u64 {
        let sampled = self.edges.iter().map(|e| match &e.curve {
            SnapCurve::Polyline { points, .. } => items(points),
            SnapCurve::Segment { .. } | SnapCurve::Arc { .. } => 0,
        });
        size_of::<Snaps>() as u64 + items(&self.vertices) + items(&self.edges) + sampled.sum::<u64>() + items(&self.planes)
    }
}

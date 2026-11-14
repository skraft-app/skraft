//! SolidSketch's geometry engine: SolidCore behind a small, safe API.
//!
//! A [`Solid`] is a SolidCore B-rep **record** (`solidcore_record::brep::Brep`): plain data,
//! cloneable and serialisable, that the model can store, copy (undo) and save as is.
//!
//! The kernel runs **off the UI thread** (docs/ARCHITECTURE.md §7 ter): the model describes
//! what it needs as a [`Job`] (records in, [`Task`]s), a [`KernelService`] runs it — on a
//! thread, in a web worker, or inline for tests — and hands back the records produced with
//! their display meshes. Without the `engine` feature this crate is only those types (and
//! `solidcore-record`): the web UI is built so, the kernel living in its own worker.
//!
//! With `engine`, every call runs on a fresh SolidCore `Kernel` and catches the kernel's
//! panics (on native targets), so a kernel bug fails one task instead of taking the
//! application down.
//!
//! Units are millimetres, Z up, right-handed.

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use solidcore_record::brep::Brep as Record;
use solidsketch_i18n::t;

#[cfg(feature = "engine")]
mod engine;
mod job;
mod polyhedron;
pub mod profile;
mod shared;
mod snaps;
mod step;
mod stretch;

#[cfg(all(feature = "engine", not(target_arch = "wasm32")))]
pub use engine::Pool;
#[cfg(feature = "engine")]
pub use engine::{Context, Inline, parallel_map, run, run_with};

/// The stack every thread running SolidCore gets: it recurses legitimately, and a stack
/// overflow aborts (it is no panic).
pub const KERNEL_STACK: usize = 256 << 20;
pub use job::{Display, EARLIER, Job, JobId, JobResult, KernelService, Output, Progress, Quality, Task, Waker, decode, encode};
pub use polyhedron::{PolyFace, Polyhedron};
pub use shared::{KernelHandle, SharedKernel};
pub use snaps::{Plane, SnapCurve, SnapEdge, Snaps};
pub use step::{StepBody, StepExcluded, StepImport, StepInstance, StepProduct};
pub use stretch::{Stretch, StretchError};

/// Why a kernel call failed. SolidCore's own messages stay as it words them (English).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Error {
    /// SolidCore reported an error.
    Kernel { call: String, message: String },
    /// The record could not go to the kernel, or the kernel's body back to the record.
    Convert { call: String, message: String },
    /// The mesher refused the body.
    Mesh(String),
    /// The operation produced nothing (e.g. the intersection of disjoint solids).
    Empty(String),
    /// SolidCore panicked; the guard caught it.
    Panic { call: String, message: String },
    /// What runs the kernel stopped while running the job (the web worker crashed).
    Crashed(String),
    /// A stretch that cannot be applied to this solid.
    Stretch(StretchError),
    /// The job was cancelled ([`KernelService::cancel`]).
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Error::Kernel { call, message } => t!("error.kernel.failed", call = call, message = message),
            Error::Convert { call, message } => t!("error.kernel.conversion", call = call, message = message),
            Error::Mesh(message) => t!("error.kernel.mesh", message = message),
            Error::Empty(call) => t!("error.kernel.empty", call = call),
            Error::Panic { call, message } => t!("error.kernel.panic", call = call, message = message),
            Error::Crashed(why) => t!("error.kernel.crashed", why = why),
            Error::Stretch(e) => return std::fmt::Display::fmt(e, f),
            Error::Cancelled => t!("error.kernel.cancelled"),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for Error {}

impl From<StretchError> for Error {
    fn from(e: StretchError) -> Self {
        Error::Stretch(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A boolean operation between two solids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BooleanOp {
    Union,
    Difference,
    Intersection,
}

impl BooleanOp {
    #[cfg(feature = "engine")]
    pub(crate) fn call(self) -> &'static str {
        match self {
            BooleanOp::Union => "union",
            BooleanOp::Difference => "difference",
            BooleanOp::Intersection => "intersection",
        }
    }
}

/// A primitive solid, by its parameters (mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Primitive {
    /// One corner at `origin`, extents along X, Y, Z.
    Box {
        origin: DVec3,
        size: DVec3,
    },
    /// Standing on Z = `origin.z`, along +Z.
    Cylinder {
        origin: DVec3,
        radius: f64,
        height: f64,
    },
    Sphere {
        center: DVec3,
        radius: f64,
    },
    Cone {
        origin: DVec3,
        base_radius: f64,
        top_radius: f64,
        height: f64,
    },
    Torus {
        center: DVec3,
        major_radius: f64,
        minor_radius: f64,
    },
    /// The outline `profile` (its corners in the XY plane through `origin`, relative to it, with
    /// the bulge of the side leaving each and their fillets: [`profile`], a simple outline either
    /// way round)
    /// extruded along +Z by `height` (docs/ARCHITECTURE.md §6 nonies).
    Prism {
        origin: DVec3,
        profile: Vec<profile::Corner>,
        height: f64,
    },
}

impl Primitive {
    /// The same primitive at the origin of its own axes, and where that origin was: a part's
    /// blank is built in the part's axes and placed by its occurrence (docs/ARCHITECTURE.md
    /// §2.2). A box's origin is its lowest corner (its extents made positive), a cylinder's or
    /// a cone's the centre of its base, a sphere's or a torus's its centre, a prism's the lowest
    /// corner of its profile's box, on its base.
    pub fn at_origin(&self) -> (Primitive, DVec3) {
        match *self {
            Primitive::Prism { origin, ref profile, height } => {
                let lo = profile::bounds(&profile::rounded(profile)).map_or(DVec2::ZERO, |b| b.0);
                let profile = profile.iter().map(|c| profile::Corner { p: c.p - lo, ..*c }).collect();
                (Primitive::Prism { origin: DVec3::ZERO, profile, height }, origin + lo.extend(0.0))
            }
            Primitive::Box { origin, size } => {
                let lo = origin.min(origin + size);
                (Primitive::Box { origin: DVec3::ZERO, size: size.abs() }, lo)
            }
            Primitive::Cylinder { origin, radius, height } => (Primitive::Cylinder { origin: DVec3::ZERO, radius, height }, origin),
            Primitive::Sphere { center, radius } => (Primitive::Sphere { center: DVec3::ZERO, radius }, center),
            Primitive::Cone { origin, base_radius, top_radius, height } => {
                (Primitive::Cone { origin: DVec3::ZERO, base_radius, top_radius, height }, origin)
            }
            Primitive::Torus { center, major_radius, minor_radius } => {
                (Primitive::Torus { center: DVec3::ZERO, major_radius, minor_radius }, center)
            }
        }
    }
}

/// An exact solid: a SolidCore B-rep record. Serialises as the record itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Solid {
    record: Record,
}

impl Solid {
    /// Wraps a record (e.g. read from a file). It is validated when first used by the kernel.
    pub fn from_record(record: Record) -> Self {
        Solid { record }
    }

    /// A body with nothing in it (a stand-in while the kernel has not answered).
    pub fn empty() -> Self {
        Solid {
            record: Record {
                version: 1,
                tolerance: 0.0,
                regions: Vec::new(),
                shells: Vec::new(),
                faces: Vec::new(),
                edges: Vec::new(),
                vertices: Vec::new(),
            },
        }
    }

    /// The B-rep record.
    pub fn record(&self) -> &Record {
        &self.record
    }

    /// Axis-aligned bounds of the solid's vertices (`None` for a body without vertices).
    pub fn vertex_bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut it = self.record.vertices.iter().map(|v| DVec3::from_array(v.point));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// How many separate lumps of material it holds: its bounded regions that are not voids.
    pub fn pieces(&self) -> usize {
        self.record.regions.iter().skip(1).filter(|r| !r.void).count()
    }

    /// Counts of faces, edges and vertices.
    pub fn topology_counts(&self) -> (usize, usize, usize) {
        (self.record.faces.len(), self.record.edges.len(), self.record.vertices.len())
    }
}

/// A display mesh: what the viewport draws, never what the model measures.
///
/// Positions are `f32` **relative to `origin`** (the centre of the mesh's bounds, in `f64`), so
/// a solid far from the model's origin keeps its precision: the renderer folds `origin` into
/// the placement it computes in `f64` (docs/ARCHITECTURE.md §7 bis).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mesh {
    pub origin: DVec3,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// B-rep face index per triangle.
    pub tri_face: Vec<u32>,
    /// B-rep edges as polylines, relative to `origin` (seams between pieces of one smooth
    /// surface left out).
    pub edges: Vec<Vec<[f32; 3]>>,
    /// Per polyline of `edges`: the index of its edge in the record.
    pub edge_ids: Vec<u32>,
}

impl Mesh {
    /// The twelve edges of the box `lo..hi`, no faces: what stands for a solid whose mesh is
    /// on its way (docs/ARCHITECTURE.md §7 ter).
    pub fn outline(lo: DVec3, hi: DVec3) -> Self {
        let origin = (lo + hi) * 0.5;
        let corner = |i: usize| {
            let c = DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            (c - origin).as_vec3().to_array()
        };
        let positions: Vec<[f32; 3]> = (0..8).map(corner).collect();
        // Corners differing by one bit are joined.
        let edges: Vec<Vec<[f32; 3]>> = (0..8)
            .flat_map(|i| [1, 2, 4].into_iter().filter(move |b| i & b == 0).map(move |b| (i, i | b)))
            .map(|(a, b)| vec![positions[a], positions[b]])
            .collect();
        let edge_ids = vec![u32::MAX; edges.len()];
        Mesh { origin, positions, normals: vec![[0.0, 0.0, 1.0]; 8], indices: Vec::new(), tri_face: Vec::new(), edges, edge_ids }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_has_twelve_edges() {
        let m = Mesh::outline(DVec3::ZERO, DVec3::new(1.0, 2.0, 3.0));
        assert_eq!(m.edges.len(), 12);
        assert!(m.indices.is_empty());
        assert_eq!(m.origin, DVec3::new(0.5, 1.0, 1.5));
    }
}

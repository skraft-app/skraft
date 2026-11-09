//! SolidSketch's geometry engine: SolidCore behind a small, safe API.
//!
//! A [`Solid`] is a SolidCore B-rep **record** (`solidcore_record::brep::Brep`): plain data,
//! cloneable and serialisable, that the model can store, copy (undo) and save as is. An
//! operation converts the records it reads into kernel bodies, runs the kernel, and converts
//! the result back: the kernel's state never outlives a call.
//!
//! Every call runs on a fresh [`Kernel`] and catches the kernel's panics (on native targets),
//! so a kernel bug fails one operation instead of taking the application down.
//!
//! Units are millimetres, Z up, right-handed.

use glam::{DAffine3, DVec3};
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::primitive::api_primitives as prim;
use solidcore_record::brep::Brep as Record;

mod snaps;
pub use snaps::{Plane, SnapCurve, SnapEdge, Snaps};

/// Why a kernel call failed.
#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    /// SolidCore reported an error.
    #[error("{call}: {message}")]
    Kernel { call: &'static str, message: String },
    /// The record could not go to the kernel, or the kernel's body back to the record.
    #[error("{call}: conversion: {message}")]
    Convert { call: &'static str, message: String },
    /// The mesher refused the body.
    #[error("tessellation: {0}")]
    Mesh(String),
    /// The operation produced nothing (e.g. the intersection of disjoint solids).
    #[error("{0}: empty result")]
    Empty(&'static str),
    /// SolidCore panicked; the guard caught it.
    #[error("{call}: kernel panic: {message}")]
    Panic { call: &'static str, message: String },
}

pub type Result<T> = std::result::Result<T, Error>;

/// A boolean operation between two solids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    Difference,
    Intersection,
}

/// An exact solid: a SolidCore B-rep record. Serialises as the record itself.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Solid {
    record: Record,
}

impl Solid {
    /// Wraps a record (e.g. read from a file). It is validated when first used by the kernel.
    pub fn from_record(record: Record) -> Self {
        Solid { record }
    }

    /// The B-rep record.
    pub fn record(&self) -> &Record {
        &self.record
    }

    /// A box with one corner at `origin` and the given extents along X, Y, Z.
    pub fn new_box(origin: DVec3, size: DVec3) -> Result<Self> {
        create("box", |k| prim::create_box(k, p3(origin), size.x, size.y, size.z))
    }

    /// A cylinder standing on the XY plane at `origin`, along +Z.
    pub fn new_cylinder(origin: DVec3, radius: f64, height: f64) -> Result<Self> {
        create("cylinder", |k| prim::create_cylinder(k, p3(origin), radius, height))
    }

    /// A sphere centred at `center`.
    pub fn new_sphere(center: DVec3, radius: f64) -> Result<Self> {
        create("sphere", |k| prim::create_sphere(k, p3(center), radius))
    }

    /// A cone (or truncated cone) standing on the XY plane at `origin`, along +Z.
    pub fn new_cone(origin: DVec3, base_radius: f64, top_radius: f64, height: f64) -> Result<Self> {
        create("cone", |k| prim::create_cone(k, p3(origin), base_radius, top_radius, height))
    }

    /// A torus centred at `center`, around +Z.
    pub fn new_torus(center: DVec3, major_radius: f64, minor_radius: f64) -> Result<Self> {
        create("torus", |k| prim::create_torus(k, p3(center), major_radius, minor_radius))
    }

    /// This solid moved by the rigid (or uniformly scaled) transform `t`.
    pub fn transformed(&self, t: &DAffine3) -> Result<Self> {
        let m = row_major(t);
        guarded("transform", || {
            let mut k = kernel();
            let mut body = to_kernel(&mut k, &self.record, "transform")?;
            solidcore::topology::brep::mirror::transform_matrix(&mut k, &mut body, &m, false)
                .map_err(|e| kernel_err("transform", e))?;
            to_record(&mut k, &mut body, "transform")
        })
    }

    /// `self op other`, both in the same coordinate system.
    pub fn boolean(&self, other: &Solid, op: BooleanOp) -> Result<Self> {
        let call = match op {
            BooleanOp::Union => "union",
            BooleanOp::Difference => "difference",
            BooleanOp::Intersection => "intersection",
        };
        guarded(call, || {
            use solidcore::boolean::api_brep as b;
            let mut k = kernel();
            let a = to_kernel(&mut k, &self.record, call)?;
            let o = to_kernel(&mut k, &other.record, call)?;
            let r = match op {
                BooleanOp::Union => b::boolean_union(&mut k, a, o),
                BooleanOp::Difference => b::boolean_difference(&mut k, a, o),
                BooleanOp::Intersection => b::boolean_intersection(&mut k, a, o),
            };
            let mut body = r.map_err(|e| kernel_err(call, e))?;
            let out = to_record(&mut k, &mut body, call)?;
            if out.record.faces.is_empty() {
                return Err(Error::Empty(call));
            }
            Ok(out)
        })
    }

    /// Axis-aligned bounds of the solid's vertices (`None` for a body without vertices).
    pub fn vertex_bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut it = self.record.vertices.iter().map(|v| DVec3::from_array(v.point));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Counts of faces, edges and vertices.
    pub fn topology_counts(&self) -> (usize, usize, usize) {
        (self.record.faces.len(), self.record.edges.len(), self.record.vertices.len())
    }

    /// A display mesh of the solid, in its own coordinates.
    pub fn mesh(&self) -> Result<Mesh> {
        self.display().map(|(m, _)| m)
    }

    /// The display mesh and the snap features (what tools infer on), in the solid's own
    /// coordinates: one tessellation for both.
    pub fn display(&self) -> Result<(Mesh, Snaps)> {
        let size = self
            .vertex_bounds()
            .map(|(lo, hi)| (hi - lo).length())
            .filter(|s| *s > 1e-6)
            .unwrap_or(100.0);
        guarded("mesh", || {
            let tol = solidcore_tess::MeshTolerance::display_of(size);
            let (plan, _stats) =
                solidcore_tess::plan(&self.record, &tol, size).map_err(|e| Error::Mesh(e.to_string()))?;
            let (m, _refused) = plan.evaluate();
            let visible: Vec<bool> = (0..m.edges.len()).map(|i| edge_drawn(&self.record, &m, i)).collect();
            let polylines: Vec<Vec<DVec3>> =
                m.edges.iter().map(|e| e.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect()).collect();
            Ok((Mesh::from_tess(&m, &visible), Snaps::build(&self.record, &polylines, &visible)))
        })
    }
}

/// A display mesh: what the viewport draws, never what the model measures.
///
/// Positions are `f32` **relative to `origin`** (the centre of the mesh's bounds, in `f64`), so
/// a solid far from the model's origin keeps its precision: the renderer folds `origin` into
/// the placement it computes in `f64` (docs/ARCHITECTURE.md §7 bis).
#[derive(Clone, Debug, Default)]
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

/// Does the viewport draw edge `i` of `record`'s tessellation `m`? Not a seam: the mesher
/// leaves `seams` empty, so they are told from the record.
fn edge_drawn(record: &Record, m: &solidcore_tess::Mesh, i: usize) -> bool {
    m.shaded_edge_visible(i) && !m.seams.get(i).copied().unwrap_or(false) && !is_seam(record, i)
}

/// Is edge `i` a seam: where a face meets itself (a cylinder's side), or between two pieces
/// of one surface?
fn is_seam(record: &Record, i: usize) -> bool {
    let Some(e) = record.edges.get(i) else { return false };
    let [a, b] = match e.uses.as_slice() {
        [a, b] => [a.face, b.face],
        _ => return false,
    };
    if a == b {
        return true;
    }
    match (record.faces.get(a as usize), record.faces.get(b as usize)) {
        (Some(fa), Some(fb)) => fa.surface == fb.surface,
        _ => false,
    }
}

impl Mesh {
    /// `drawn[i]`: is edge `i` drawn.
    fn from_tess(m: &solidcore_tess::Mesh, drawn: &[bool]) -> Self {
        let d = |p: &solidcore_record::Vec3| DVec3::new(p.x, p.y, p.z);
        let origin = m
            .positions
            .iter()
            .map(d)
            .fold(None, |b: Option<(DVec3, DVec3)>, p| Some(b.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p)))))
            .map_or(DVec3::ZERO, |(lo, hi)| (lo + hi) * 0.5);
        let rel = |p: &solidcore_record::Vec3| (d(p) - origin).as_vec3().to_array();
        let dir = |p: &solidcore_record::Vec3| d(p).as_vec3().to_array();
        let (edges, edge_ids) = m
            .edges
            .iter()
            .enumerate()
            .filter(|(i, _)| drawn.get(*i).copied().unwrap_or(false))
            .map(|(i, e)| (e.iter().map(rel).collect::<Vec<_>>(), i as u32))
            .unzip();
        Mesh {
            origin,
            positions: m.positions.iter().map(rel).collect(),
            normals: m.normals.iter().map(dir).collect(),
            indices: m.triangles.iter().flatten().copied().collect(),
            tri_face: m.tri_face.clone(),
            edges,
            edge_ids,
        }
    }
}

// ---------------------------------------------------------------------------------------------

fn kernel() -> Kernel {
    Kernel::new(&KernelOptions::default())
}

fn p3(v: DVec3) -> solidcore::Point3 {
    solidcore::Point3::new(v.x, v.y, v.z)
}

/// SolidCore's matrix convention: row vectors, linear part in rows 0..3, translation in row 3.
fn row_major(t: &DAffine3) -> [[f64; 4]; 4] {
    let (x, y, z, w) = (t.matrix3.x_axis, t.matrix3.y_axis, t.matrix3.z_axis, t.translation);
    [[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [z.x, z.y, z.z, 0.0], [w.x, w.y, w.z, 1.0]]
}

fn kernel_err(call: &'static str, e: solidcore::SolidError) -> Error {
    Error::Kernel { call, message: e.to_string() }
}

fn to_kernel(k: &mut Kernel, r: &Record, call: &'static str) -> Result<solidcore::Brep> {
    solidcore_convert::brep::to_kernel_body(k, r, true)
        .map_err(|e| Error::Convert { call, message: e.to_string() })
}

fn to_record(k: &mut Kernel, b: &mut solidcore::Brep, call: &'static str) -> Result<Solid> {
    solidcore_convert::brep::to_record(k, b)
        .map(|(record, _order)| Solid { record })
        .map_err(|e| Error::Convert { call, message: e.to_string() })
}

fn create(call: &'static str, f: impl FnOnce(&mut Kernel) -> solidcore::Result<solidcore::Brep>) -> Result<Solid> {
    guarded(call, || {
        let mut k = kernel();
        let mut body = f(&mut k).map_err(|e| kernel_err(call, e))?;
        to_record(&mut k, &mut body, call)
    })
}

/// Runs `f`, turning a kernel panic into an error where unwinding is available.
fn guarded<T>(call: &'static str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let message = p
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown".to_owned());
            Err(Error::Panic { call, message })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_meshes() {
        let b = Solid::new_box(DVec3::ZERO, DVec3::new(10.0, 20.0, 30.0)).unwrap();
        assert_eq!(b.topology_counts(), (6, 12, 8));
        let m = b.mesh().unwrap();
        // The mesher refines planar faces too: count nothing but what the model relies on.
        assert_eq!(m.indices.len(), m.tri_face.len() * 3);
        let mut faces: Vec<u32> = m.tri_face.clone();
        faces.sort_unstable();
        faces.dedup();
        assert_eq!(faces, (0..6).collect::<Vec<_>>());
        assert_eq!(m.edges.len(), 12);
        assert_eq!(m.normals.len(), m.positions.len());
        for n in &m.normals {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((l - 1.0).abs() < 1e-4);
        }
        assert!((m.origin - DVec3::new(5.0, 10.0, 15.0)).length() < 1e-9);
    }

    #[test]
    fn far_mesh_keeps_precision() {
        // 100 km from the origin, a 0.01 mm step must survive: positions are relative.
        let at = DVec3::new(1e8, 0.0, 0.0);
        let m = Solid::new_box(at, DVec3::new(0.01, 1.0, 1.0)).unwrap().mesh().unwrap();
        let xs: Vec<f32> = m.positions.iter().map(|p| p[0]).collect();
        let (lo, hi) = xs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
        assert!(((hi - lo) - 0.01).abs() < 1e-6, "{lo} {hi}");
    }

    #[test]
    fn primitives_mesh() {
        for s in [
            Solid::new_cylinder(DVec3::ZERO, 5.0, 10.0),
            Solid::new_sphere(DVec3::ZERO, 5.0),
            Solid::new_cone(DVec3::ZERO, 5.0, 2.0, 10.0),
            Solid::new_torus(DVec3::ZERO, 10.0, 3.0),
        ] {
            let m = s.unwrap().mesh().unwrap();
            assert!(m.indices.len() > 30);
        }
    }

    #[test]
    fn boolean_difference_and_transform() {
        let a = Solid::new_box(DVec3::ZERO, DVec3::splat(20.0)).unwrap();
        let c = Solid::new_cylinder(DVec3::new(10.0, 10.0, -5.0), 4.0, 30.0).unwrap();
        let d = a.boolean(&c, BooleanOp::Difference).unwrap();
        assert!(d.topology_counts().0 >= 7, "{:?}", d.topology_counts());
        let u = a.boolean(&Solid::new_box(DVec3::splat(10.0), DVec3::splat(20.0)).unwrap(), BooleanOp::Union);
        assert!(u.unwrap().topology_counts().0 >= 9);
        let moved = a.transformed(&DAffine3::from_translation(DVec3::new(100.0, 0.0, 0.0))).unwrap();
        let (lo, _) = moved.vertex_bounds().unwrap();
        assert!((lo.x - 100.0).abs() < 1e-9);
        d.mesh().unwrap();
    }

    #[test]
    fn box_snaps() {
        let b = Solid::new_box(DVec3::ZERO, DVec3::new(10.0, 20.0, 30.0)).unwrap();
        let (m, s) = b.display().unwrap();
        assert_eq!(m.edge_ids.len(), m.edges.len());
        assert_eq!(s.vertices.len(), 8);
        assert!(s.vertices.contains(&DVec3::new(10.0, 20.0, 30.0)));
        assert_eq!(s.edges.len(), 12);
        assert!(s.edges.iter().all(|e| matches!(e.curve, SnapCurve::Segment { .. })));
        let mids: Vec<DVec3> = s.edges.iter().filter_map(|e| e.curve.midpoint()).collect();
        assert!(mids.contains(&DVec3::new(5.0, 0.0, 0.0)), "{mids:?}");
        assert_eq!(s.planes.len(), 6);
        assert!(s.planes.iter().all(Option::is_some));
    }

    #[test]
    fn cylinder_snaps() {
        let c = Solid::new_cylinder(DVec3::new(1.0, 2.0, 3.0), 5.0, 10.0).unwrap();
        let (_, s) = c.display().unwrap();
        // Two circles; the side's seam is not drawn, and a circle's vertex is no endpoint.
        assert!(s.vertices.is_empty(), "{:?} {:?}", s.vertices, s.edges);
        assert_eq!(c.mesh().unwrap().edges.len(), 2, "the side's seam is not drawn");
        assert_eq!(s.edges.len(), 2, "{:?}", s.edges);
        let mut centers: Vec<DVec3> = s.edges.iter().filter_map(|e| e.curve.center()).collect();
        centers.sort_by(|a, b| a.z.total_cmp(&b.z));
        assert!((centers[0] - DVec3::new(1.0, 2.0, 3.0)).length() < 1e-9, "{centers:?}");
        assert!((centers[1] - DVec3::new(1.0, 2.0, 13.0)).length() < 1e-9, "{centers:?}");
        assert!(s.edges.iter().all(|e| e.curve.is_closed_arc()));
        assert_eq!(s.planes.iter().filter(|p| p.is_some()).count(), 2);
    }

    #[test]
    fn half_disc_arc_snaps() {
        // A box cut by a cylinder through its side: the cut leaves open arcs.
        let a = Solid::new_box(DVec3::ZERO, DVec3::splat(20.0)).unwrap();
        let c = Solid::new_cylinder(DVec3::new(10.0, 0.0, -5.0), 4.0, 30.0).unwrap();
        let d = a.boolean(&c, BooleanOp::Difference).unwrap();
        let (_, s) = d.display().unwrap();
        let arcs: Vec<&SnapCurve> = s.edges.iter().map(|e| &e.curve).filter(|c| matches!(c, SnapCurve::Arc { .. })).collect();
        assert_eq!(arcs.len(), 2, "{:?}", s.edges);
        for arc in arcs {
            let SnapCurve::Arc { center, sweep, .. } = arc else { unreachable!() };
            assert!((sweep - std::f64::consts::PI).abs() < 1e-6, "{sweep}");
            let m = arc.midpoint().unwrap();
            // The middle of the half circle is the deepest point of the notch.
            assert!((m - (*center + DVec3::new(0.0, 4.0, 0.0))).length() < 1e-9, "{m} {center}");
        }
    }

    #[test]
    fn serde_round_trip() {
        let a = Solid::new_cylinder(DVec3::ZERO, 3.0, 7.0).unwrap();
        let json = serde_json::to_string(&a).unwrap();
        let b: Solid = serde_json::from_str(&json).unwrap();
        assert_eq!(a, b);
        b.mesh().unwrap();
    }
}

//! The display scene (ARCHITECTURE.md §7 bis): the document flattened for drawing and picking.
//!
//! - Level 1: one display mesh per solid, cached by the solid's `Arc` (an unchanged solid is
//!   never tessellated again, across undo and redo too).
//! - Level 2: per definition, the list of its direct solids and a revision that changes when
//!   that list changes: the renderer re-uploads the definition's merged mesh only then.
//! - Placements: every occurrence of every definition in the scene, with its instance path and
//!   world transform.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use solidsketch_kernel::{Mesh, Snaps, Solid};
use solidsketch_model::{Document, Entity, EntityPath, Id, MAX_DEPTH};

use crate::camera::Ray;

/// One solid's display mesh with its bounds (in the definition's coordinates).
#[derive(Clone, Debug)]
pub struct PartMesh {
    pub solid: Id,
    pub mesh: Arc<Mesh>,
    /// What tools infer on (§6 ter), from the same tessellation.
    pub snaps: Arc<Snaps>,
    pub bounds: Option<(DVec3, DVec3)>,
}

/// A definition's direct solids, as the renderer uploads them.
#[derive(Clone, Debug)]
pub struct DefinitionMesh {
    pub definition: Id,
    pub revision: u64,
    pub parts: Vec<PartMesh>,
}

/// One occurrence of a definition in the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// Instances from the root to this occurrence (empty for the root).
    pub path: Vec<Id>,
    /// Definition to world.
    pub transform: DAffine3,
}

/// What a picking ray hit.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The solid hit, by its full path.
    pub path: EntityPath,
    /// The B-rep face (index in the solid's record).
    pub face: u32,
    /// World point and unit normal (facing the ray).
    pub point: DVec3,
    pub normal: DVec3,
    /// Distance along the ray.
    pub t: f64,
}

#[derive(Default)]
pub struct DisplayScene {
    /// Level 1: by `Arc<Solid>` address; the `Arc` is held so the address is not reused.
    solids: HashMap<usize, (Arc<Solid>, PartMesh)>,
    definitions: BTreeMap<Id, DefinitionMesh>,
    placements: BTreeMap<Id, Vec<Placement>>,
    /// Solids the mesher refused, with why.
    errors: Vec<(Id, String)>,
    bounds: Option<(DVec3, DVec3)>,
}

impl DisplayScene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Brings the scene up to date with `doc`. Only solids not seen before are tessellated.
    pub fn sync(&mut self, doc: &Document) {
        let mut seen = HashMap::with_capacity(self.solids.len());
        self.errors.clear();
        self.definitions.clear();
        for def in doc.definitions() {
            let mut parts = Vec::new();
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for e in &def.entities {
                let Entity::Solid(s) = e else { continue };
                let key = Arc::as_ptr(&s.solid) as usize;
                // The same Arc may be in several definitions (a component made unique shares
                // its solids until they change): look in what this sync has seen first.
                let cached = seen.get(&key).cloned().or_else(|| self.solids.remove(&key));
                let entry = match cached {
                    Some(hit) => hit,
                    None => {
                        let (mesh, snaps) = match s.solid.display() {
                            Ok(m) => m,
                            Err(err) => {
                                self.errors.push((s.id, err.to_string()));
                                (Mesh::default(), Snaps::default())
                            }
                        };
                        let bounds = mesh_bounds(&mesh);
                        let part = PartMesh { solid: s.id, mesh: Arc::new(mesh), snaps: Arc::new(snaps), bounds };
                        (s.solid.clone(), part)
                    }
                };
                let mut part = entry.1.clone();
                part.solid = s.id;
                (s.id, key).hash(&mut h);
                parts.push(part);
                seen.insert(key, entry);
            }
            if !parts.is_empty() {
                self.definitions.insert(def.id, DefinitionMesh { definition: def.id, revision: h.finish(), parts });
            }
        }
        self.solids = seen;

        self.placements.clear();
        let mut path = Vec::new();
        self.place(doc, doc.root(), &mut path, DAffine3::IDENTITY, 0);

        self.bounds = None;
        for (def, places) in &self.placements {
            let Some(dm) = self.definitions.get(def) else { continue };
            for part in &dm.parts {
                let Some(b) = part.bounds else { continue };
                for p in places {
                    self.bounds = Some(union(self.bounds, transform_bounds(&p.transform, b)));
                }
            }
        }
    }

    fn place(&mut self, doc: &Document, def: Id, path: &mut Vec<Id>, t: DAffine3, depth: usize) {
        let Ok(d) = doc.definition(def) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        self.placements.entry(def).or_default().push(Placement { path: path.clone(), transform: t });
        for e in &d.entities {
            if let Entity::Instance(i) = e {
                path.push(i.id);
                self.place(doc, i.definition, path, t * i.transform, depth + 1);
                path.pop();
            }
        }
    }

    /// Definitions with solids, as the renderer uploads them.
    pub fn definition_meshes(&self) -> impl Iterator<Item = &DefinitionMesh> {
        self.definitions.values()
    }

    pub fn definition_mesh(&self, def: Id) -> Option<&DefinitionMesh> {
        self.definitions.get(&def)
    }

    /// Every occurrence of every definition that has solids.
    pub fn placements(&self) -> impl Iterator<Item = (Id, &[Placement])> {
        self.placements
            .iter()
            .filter(|(d, _)| self.definitions.contains_key(d))
            .map(|(d, p)| (*d, p.as_slice()))
    }

    pub fn errors(&self) -> &[(Id, String)] {
        &self.errors
    }

    /// World bounds of everything drawn.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        self.bounds
    }

    /// Every solid drawn, once per occurrence: its placement and its part.
    pub fn parts(&self) -> impl Iterator<Item = (&Placement, &PartMesh)> {
        self.placements.iter().flat_map(move |(def, places)| {
            let parts = self.definitions.get(def).map(|d| d.parts.as_slice()).unwrap_or(&[]);
            places.iter().flat_map(move |p| parts.iter().map(move |part| (p, part)))
        })
    }

    /// The occurrence `path` names: its definition-to-world transform and its part.
    pub fn part_at(&self, path: &EntityPath) -> Option<(DAffine3, &PartMesh)> {
        self.parts()
            .find(|(p, part)| part.solid == path.entity && p.path == path.instances)
            .map(|(p, part)| (p.transform, part))
    }

    /// The nearest solid along `ray`, among those `keep` accepts.
    pub fn pick(&self, ray: &Ray, keep: impl Fn(&EntityPath) -> bool) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for (def, places) in &self.placements {
            let Some(dm) = self.definitions.get(def) else { continue };
            for p in places {
                let inv = p.transform.inverse();
                let o = inv.transform_point3(ray.origin);
                let d = inv.transform_vector3(ray.dir);
                for part in &dm.parts {
                    let Some(b) = part.bounds else { continue };
                    if !ray_box(o, d, b, ray.t_min) {
                        continue;
                    }
                    let path = EntityPath::new(p.path.clone(), part.solid);
                    if !keep(&path) {
                        continue;
                    }
                    let Some((t, tri, n_local)) = ray_mesh(o, d, &part.mesh, ray.t_min) else { continue };
                    // t is in units of the local direction: re-measure it in the world.
                    let point = p.transform.transform_point3(o + d * t);
                    let world_t = (point - ray.origin).dot(ray.dir);
                    if best.as_ref().is_some_and(|b| b.t <= world_t) {
                        continue;
                    }
                    let mut normal = p.transform.matrix3.inverse().transpose().mul_vec3(n_local).normalize_or_zero();
                    if normal.dot(ray.dir) > 0.0 {
                        normal = -normal;
                    }
                    let face = part.mesh.tri_face.get(tri).copied().unwrap_or(0);
                    best = Some(Hit { path, face, point, normal, t: world_t });
                }
            }
        }
        best
    }
}

/// Where the solids under `entity` (an entity of the definition `context_path` leads into)
/// are drawn: `(definition holding the solid, solid, world transform)`. For an instance, every
/// solid below it, at any depth.
pub fn solids_under(doc: &Document, context_path: &[Id], entity: Id) -> Vec<(Id, Id, DAffine3)> {
    let mut out = Vec::new();
    let (Ok(ctx), Ok(t)) = (doc.context_of(context_path), doc.transform_of(context_path)) else { return out };
    let Some(e) = doc.definition(ctx).ok().and_then(|d| d.entity(entity)) else { return out };
    fn walk(doc: &Document, def: Id, e: &Entity, t: DAffine3, out: &mut Vec<(Id, Id, DAffine3)>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        match e {
            Entity::Solid(s) => out.push((def, s.id, t)),
            Entity::Instance(i) => {
                let Ok(d) = doc.definition(i.definition) else { return };
                let t = t * i.transform;
                for child in &d.entities {
                    walk(doc, i.definition, child, t, out, depth + 1);
                }
            }
        }
    }
    walk(doc, ctx, e, t, &mut out, 0);
    out
}

fn mesh_bounds(m: &Mesh) -> Option<(DVec3, DVec3)> {
    let mut it = m.positions.iter().map(|p| m.origin + DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
}

fn union(a: Option<(DVec3, DVec3)>, b: (DVec3, DVec3)) -> (DVec3, DVec3) {
    match a {
        Some((lo, hi)) => (lo.min(b.0), hi.max(b.1)),
        None => b,
    }
}

pub(crate) fn transform_bounds(t: &DAffine3, (lo, hi): (DVec3, DVec3)) -> (DVec3, DVec3) {
    let mut out: Option<(DVec3, DVec3)> = None;
    for i in 0..8 {
        let c = DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let p = t.transform_point3(c);
        out = Some(union(out, (p, p)));
    }
    out.unwrap_or((lo, hi))
}

/// Does the ray `o + t·d` (t ≥ t_min) cross the box (slab test, slightly padded)?
pub(crate) fn ray_box(o: DVec3, d: DVec3, (lo, hi): (DVec3, DVec3), t_min: f64) -> bool {
    let pad = ((hi - lo).length() * 1e-6).max(1e-9);
    let (lo, hi) = (lo - pad, hi + pad);
    let (mut t0, mut t1) = (t_min, f64::INFINITY);
    for k in 0..3 {
        if d[k].abs() < 1e-300 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return false;
            }
            continue;
        }
        let (a, b) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// The nearest triangle of `m` along `o + t·d`: (t, triangle, geometric normal).
fn ray_mesh(o: DVec3, d: DVec3, m: &Mesh, t_min: f64) -> Option<(f64, usize, DVec3)> {
    let o = o - m.origin;
    let v = |i: u32| m.positions.get(i as usize).map(|p| DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
    let mut best: Option<(f64, usize, DVec3)> = None;
    for (k, tri) in m.indices.chunks_exact(3).enumerate() {
        let (Some(a), Some(b), Some(c)) = (v(tri[0]), v(tri[1]), v(tri[2])) else { continue };
        // Möller–Trumbore.
        let (e1, e2) = (b - a, c - a);
        let p = d.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-18 {
            continue;
        }
        let inv = 1.0 / det;
        let s = o - a;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = s.cross(e1);
        let w = d.dot(q) * inv;
        if w < 0.0 || u + w > 1.0 {
            continue;
        }
        let t = e2.dot(q) * inv;
        if t < t_min || best.is_some_and(|(bt, _, _)| bt <= t) {
            continue;
        }
        best = Some((t, k, e1.cross(e2)));
    }
    best
}

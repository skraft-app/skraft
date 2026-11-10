//! The display scene (ARCHITECTURE.md §7 bis): the document flattened for drawing and picking.
//!
//! - Level 1: one display mesh per solid, cached by the solid's `Arc` (an unchanged solid is
//!   never tessellated again, across undo and redo too: a mesh is kept while anything — the
//!   undo history — holds its solid). Meshes come from the kernel's service
//!   (docs/ARCHITECTURE.md §7 ter): with an operation's result, or asked for by the editor for
//!   solids that have none ([`DisplayScene::take_missing`]); until then a solid is drawn as the
//!   outline of its bounding box.
//! - Level 2: per part (docs/ARCHITECTURE.md §2), its shape's mesh and a revision that changes
//!   when the shape (or its mesh) does: the renderer re-uploads the part's mesh only then.
//!   Assemblies have no geometry of their own.
//! - Placements: every occurrence of every part in the scene, with its occurrence path and
//!   world transform; a part is drawn once, instanced at all of them.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use solidsketch_kernel::{Display, Mesh, Snaps, Solid};
use solidsketch_model::{ComponentKind, Document, EntityPath, Id, MAX_DEPTH, Section};

use crate::camera::Ray;

/// One part's display mesh with its bounds (in the part's axes).
#[derive(Clone, Debug)]
pub struct PartMesh {
    /// The part.
    pub solid: Id,
    pub mesh: Arc<Mesh>,
    /// What tools infer on (§6 ter), from the same tessellation.
    pub snaps: Arc<Snaps>,
    pub bounds: Option<(DVec3, DVec3)>,
}

/// A part's mesh, as the renderer uploads it (one entry in `parts`).
#[derive(Clone, Debug)]
pub struct DefinitionMesh {
    pub definition: Id,
    /// Changes with the mesh and with the faces' materials.
    pub revision: u64,
    pub parts: Vec<PartMesh>,
    /// The materials of single faces (by face index), over the occurrences' (§7 quater).
    pub faces: BTreeMap<u32, Id>,
}

/// One occurrence of a part in the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// Occurrences from the root to the part's, that one included.
    pub path: Vec<Id>,
    /// Part to world.
    pub transform: DAffine3,
    /// The material its faces take (its occurrence's, else an assembly's above it).
    pub material: Option<Id>,
}

impl Placement {
    /// The path naming this occurrence.
    pub fn entity_path(&self) -> EntityPath {
        EntityPath::from_chain(&self.path).unwrap_or_else(|| EntityPath::root(Id(0)))
    }
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

/// A solid's display, as far as it is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Not meshed yet: drawn as its bounding box; `asked` once handed to the editor.
    Pending { asked: bool },
    Ready,
    /// The mesher refused it: its bounding box stays.
    Failed,
}

struct Cached {
    /// Held so the address (the key) is not reused.
    solid: Arc<Solid>,
    part: PartMesh,
    state: State,
}

impl Cached {
    fn pending(solid: Arc<Solid>) -> Self {
        let mesh = solid.vertex_bounds().map(|(lo, hi)| Mesh::outline(lo, hi)).unwrap_or_default();
        Cached::new(solid, mesh, Snaps::default(), State::Pending { asked: false })
    }

    fn new(solid: Arc<Solid>, mesh: Mesh, snaps: Snaps, state: State) -> Self {
        let bounds = mesh_bounds(&mesh);
        // `part.solid` is set by each part showing it.
        let part = PartMesh { solid: Id(0), mesh: Arc::new(mesh), snaps: Arc::new(snaps), bounds };
        Cached { solid, part, state }
    }
}

#[derive(Default)]
pub struct DisplayScene {
    /// Level 1: by `Arc<Solid>` address.
    solids: HashMap<usize, Cached>,
    definitions: BTreeMap<Id, DefinitionMesh>,
    placements: BTreeMap<Id, Vec<Placement>>,
    /// Solids of the document without a mesh, not handed out yet.
    missing: Vec<Arc<Solid>>,
    bounds: Option<(DVec3, DVec3)>,
    /// The active section: picking ignores what it cuts away.
    section: Option<Section>,
}

impl DisplayScene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Brings the scene up to date with `doc`. Solids not seen before get a stand-in until
    /// their mesh comes ([`DisplayScene::take_missing`], [`DisplayScene::insert`]).
    pub fn sync(&mut self, doc: &Document) {
        let mut seen = std::collections::HashSet::new();
        self.definitions.clear();
        self.missing.clear();
        for c in doc.components() {
            let ComponentKind::Part(p) = &c.kind else { continue };
            let shape = p.shape();
            let key = Arc::as_ptr(shape) as usize;
            let entry = self.solids.entry(key).or_insert_with(|| Cached::pending(shape.clone()));
            // A part made unique shares its shape until it changes: ask for it once.
            if seen.insert(key) && entry.state == (State::Pending { asked: false }) {
                self.missing.push(shape.clone());
            }
            let mut part = entry.part.clone();
            part.solid = c.id;
            // The mesh changes with the state (a stand-in, then the real one).
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (key, entry.state == State::Ready, &p.faces).hash(&mut h);
            self.definitions.insert(
                c.id,
                DefinitionMesh { definition: c.id, revision: h.finish(), parts: vec![part], faces: p.faces.clone() },
            );
        }
        // Keep what the document shows, and what something else holds (undo, a job).
        self.solids.retain(|key, c| seen.contains(key) || Arc::strong_count(&c.solid) > 1);

        self.placements.clear();
        for p in doc.placed_parts() {
            let mut path = p.path.instances;
            path.push(p.path.entity);
            self.placements.entry(p.part).or_default().push(Placement { path, transform: p.transform, material: p.material });
        }
        self.section = doc.settings().display.active_section();

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

    /// Parts, as the renderer uploads them.
    pub fn definition_meshes(&self) -> impl Iterator<Item = &DefinitionMesh> {
        self.definitions.values()
    }

    pub fn definition_mesh(&self, def: Id) -> Option<&DefinitionMesh> {
        self.definitions.get(&def)
    }

    /// Every occurrence of every part.
    pub fn placements(&self) -> impl Iterator<Item = (Id, &[Placement])> {
        self.placements
            .iter()
            .filter(|(d, _)| self.definitions.contains_key(d))
            .map(|(d, p)| (*d, p.as_slice()))
    }

    /// The document's solids that have no mesh and were not handed out before: the caller
    /// asks the kernel for them, then [`DisplayScene::insert`]s each.
    pub fn take_missing(&mut self) -> Vec<Arc<Solid>> {
        let missing = std::mem::take(&mut self.missing);
        for s in &missing {
            if let Some(c) = self.solids.get_mut(&(Arc::as_ptr(s) as usize)) {
                c.state = State::Pending { asked: true };
            }
        }
        missing
    }

    /// The kernel's display of `solid` (from an operation, or asked for): in the cache, for
    /// the next [`DisplayScene::sync`]. A refused one keeps the stand-in.
    pub fn insert(&mut self, solid: &Arc<Solid>, display: Result<Display, String>) {
        let key = Arc::as_ptr(solid) as usize;
        let cached = match display {
            Ok(d) => Cached::new(solid.clone(), d.mesh, d.snaps, State::Ready),
            Err(_) => Cached { state: State::Failed, ..Cached::pending(solid.clone()) },
        };
        self.solids.insert(key, cached);
    }

    /// Is `solid`'s mesh still on its way?
    pub fn is_pending(&self, solid: &Arc<Solid>) -> bool {
        self.solids.get(&(Arc::as_ptr(solid) as usize)).is_some_and(|c| matches!(c.state, State::Pending { .. }))
    }

    /// The active section: what it cuts away is neither picked nor inferred.
    pub fn section(&self) -> Option<Section> {
        self.section
    }

    /// World bounds of everything drawn.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        self.bounds
    }

    /// Every part drawn, once per occurrence: its placement and its mesh.
    pub fn parts(&self) -> impl Iterator<Item = (&Placement, &PartMesh)> {
        self.placements.iter().flat_map(move |(def, places)| {
            let parts = self.definitions.get(def).map(|d| d.parts.as_slice()).unwrap_or(&[]);
            places.iter().flat_map(move |p| parts.iter().map(move |part| (p, part)))
        })
    }

    /// The occurrence `path` names: its part-to-world transform and its mesh.
    pub fn part_at(&self, path: &EntityPath) -> Option<(DAffine3, &PartMesh)> {
        self.parts()
            .find(|(p, _)| p.path.split_last().is_some_and(|(last, rest)| *last == path.entity && rest == path.instances))
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
                    let path = p.entity_path();
                    if !keep(&path) {
                        continue;
                    }
                    // The section, in the part's axes (placements are rigid).
                    let clip = self.section.map(|s| (inv.transform_point3(s.origin), inv.transform_vector3(s.normal)));
                    let Some((t, tri, n_local)) = ray_mesh(o, d, &part.mesh, ray.t_min, clip) else { continue };
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

/// A part drawn under an occurrence ([`solids_under`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Under {
    pub part: Id,
    /// Part to world.
    pub transform: DAffine3,
    /// The material its faces take there.
    pub material: Option<Id>,
}

/// Where the parts under `entity` (an occurrence of the assembly `context_path` leads into)
/// are drawn. For an assembly's occurrence, every part below it, at any depth.
pub fn solids_under(doc: &Document, context_path: &[Id], entity: Id) -> Vec<Under> {
    let mut out = Vec::new();
    let (Ok(ctx), Ok(t)) = (doc.context_of(context_path), doc.transform_of(context_path)) else { return out };
    let Some(o) = doc.assembly(ctx).ok().and_then(|a| a.occurrence(entity)) else { return out };
    // The material the assemblies above the context give.
    let mut inherited = None;
    let mut at = doc.root();
    for &i in context_path {
        let Some(occ) = doc.assembly(at).ok().and_then(|a| a.occurrence(i)) else { break };
        inherited = occ.material.or(inherited);
        at = occ.component;
    }
    fn walk(doc: &Document, component: Id, t: DAffine3, material: Option<Id>, out: &mut Vec<Under>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        match doc.component(component).map(|c| &c.kind) {
            Ok(ComponentKind::Part(_)) => out.push(Under { part: component, transform: t, material }),
            Ok(ComponentKind::Assembly(a)) => {
                for o in &a.occurrences {
                    walk(doc, o.component, t * o.transform, o.material.or(material), out, depth + 1);
                }
            }
            Err(_) => {}
        }
    }
    walk(doc, o.component, t * o.transform, o.material.or(inherited), &mut out, 0);
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

/// The nearest triangle of `m` along `o + t·d`: (t, triangle, geometric normal); a point
/// `clip` (a plane's point and normal) cuts away is not a hit.
fn ray_mesh(o: DVec3, d: DVec3, m: &Mesh, t_min: f64, clip: Option<(DVec3, DVec3)>) -> Option<(f64, usize, DVec3)> {
    let clip = clip.map(|(p, n)| (p - m.origin, n));
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
        if clip.is_some_and(|(p, n)| (o + d * t - p).dot(n) > 0.0) {
            continue;
        }
        best = Some((t, k, e1.cross(e2)));
    }
    best
}

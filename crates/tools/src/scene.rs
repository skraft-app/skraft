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
//! - The recipe of one open part (ARCHITECTURE.md §2.9), when its machinings are shown: its
//!   blank and its tools, meshed like shapes, under keys of their own ([`RecipeMeshes`]); one
//!   mesh per distinct tool solid, a copied machining sharing its original's. Drawn by the
//!   view; for inferences ([`DisplayScene::inferable`]) they stand in for the open
//!   occurrence's shape, each saying what it is ([`Piece`]).

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use solidsketch_kernel::{Display, Mesh, Snaps, Solid};
use solidsketch_model::{ComponentKind, Document, EntityPath, GuideShape, Id, MAX_DEPTH, Section};

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
    /// Which of its geometry ([`DisplayScene::pick_inferable`]; a shape otherwise).
    pub piece: Piece,
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
    Pending {
        asked: bool,
    },
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
        Cached::new(solid, mesh, Snaps::default(), None, State::Pending { asked: false })
    }

    /// `exact`: the solid's bounding box, from the kernel; else the mesh's.
    fn new(solid: Arc<Solid>, mesh: Mesh, snaps: Snaps, exact: Option<(DVec3, DVec3)>, state: State) -> Self {
        let bounds = exact.or_else(|| mesh_bounds(&mesh));
        // `part.solid` is set by each part showing it.
        let part = PartMesh { solid: Id(0), mesh: Arc::new(mesh), snaps: Arc::new(snaps), bounds };
        Cached { solid, part, state }
    }
}

/// The keys of a recipe's meshes, apart from components' ids (small numbers from the
/// document's counter).
const RECIPE_KEY: u64 = 1 << 63;
const TOOL_KEY: u64 = 1 << 62;
/// The key of the first mesh a tool previews in a part's stead (a part stretched, ARCHITECTURE.md
/// §6 sexies); the others follow it ([`preview_key`]).
pub const PREVIEW_KEY: Id = Id(1 << 61);

/// The key of the `i`-th mesh a tool previews.
pub fn preview_key(i: usize) -> Id {
    Id(PREVIEW_KEY.0 + i as u64)
}

/// What a piece of geometry of the scene is, for inferences: an occurrence's shape, or — an
/// open part's recipe shown — its blank or the tool of one of its machinings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Piece {
    #[default]
    Shape,
    Blank,
    Tool(Id),
    /// A guide (§6 quinquies); its owner's path names its component's occurrences then the
    /// guide's id.
    Guide(Id),
}

/// Whose geometry an inference is asked to consider (a tool's filter).
#[derive(Clone, Copy, Debug)]
pub struct Owner<'a> {
    /// The occurrence (the open part's, for its blank and tools).
    pub path: &'a EntityPath,
    pub piece: Piece,
}

/// One machining of the recipe shown.
#[derive(Clone, Debug, PartialEq)]
pub struct RecipeTool {
    pub machining: Id,
    /// The mesh of its tool.
    pub key: Id,
    /// Its tool in the part's axes.
    pub placement: DAffine3,
    pub failed: bool,
}

/// The meshes of the recipe shown: the blank's and the tools', in the part's axes.
#[derive(Clone, Debug, PartialEq)]
pub struct RecipeMeshes {
    pub part: Id,
    pub blank: Id,
    pub tools: Vec<RecipeTool>,
}

/// A guide placed in the world (ARCHITECTURE.md §6 quinquies): drawn and inferred on.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneGuide {
    /// The occurrences from the root to its component (empty: the model's).
    pub path: Vec<Id>,
    pub id: Id,
    /// World.
    pub shape: GuideShape,
}

#[derive(Default)]
pub struct DisplayScene {
    /// Level 1: by `Arc<Solid>` address.
    solids: HashMap<usize, Cached>,
    definitions: BTreeMap<Id, DefinitionMesh>,
    /// The meshes a tool previews, under [`preview_key`]s.
    preview: Vec<DefinitionMesh>,
    placements: BTreeMap<Id, Vec<Placement>>,
    /// Solids of the document without a mesh, not handed out yet.
    missing: Vec<Arc<Solid>>,
    bounds: Option<(DVec3, DVec3)>,
    /// The active section: picking ignores what it cuts away.
    section: Option<Section>,
    /// The open occurrence whose recipe is shown, its meshes, and their placements in the
    /// world (the blank's, then each tool's).
    recipe_path: Option<EntityPath>,
    /// Drawn and inferred on (else only meshed: the entity info reads its tools' sizes).
    recipe_shown: bool,
    recipe: Option<RecipeMeshes>,
    recipe_places: Vec<(Placement, Id, Piece)>,
    guides: Vec<SceneGuide>,
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
            let dm = self.mesh_of(p.shape(), c.id, &p.faces, &mut seen);
            self.definitions.insert(c.id, dm);
            // A machined part's blank too, for its exact size (its dimensions).
            if !p.machinings.is_empty() {
                self.mesh_of(&p.blank, c.id, &BTreeMap::new(), &mut seen);
            }
        }
        self.recipe = None;
        let shown = self.recipe_path.as_ref().and_then(|path| doc.resolve_part(path).ok());
        if let Some((part, p, _)) = shown {
            let blank = Id(RECIPE_KEY | part.0);
            let dm = self.mesh_of(&p.blank, blank, &BTreeMap::new(), &mut seen);
            self.definitions.insert(blank, dm);
            // One mesh per distinct tool solid, keyed by the first machining using it.
            let mut keys: HashMap<usize, Id> = HashMap::new();
            let mut tools = Vec::with_capacity(p.machinings.len());
            for m in &p.machinings {
                let at = Arc::as_ptr(&m.tool) as usize;
                let key = match keys.get(&at) {
                    Some(k) => *k,
                    None => {
                        let key = Id(RECIPE_KEY | TOOL_KEY | m.id.0);
                        let dm = self.mesh_of(&m.tool, key, &BTreeMap::new(), &mut seen);
                        self.definitions.insert(key, dm);
                        keys.insert(at, key);
                        key
                    }
                };
                tools.push(RecipeTool { machining: m.id, key, placement: m.placement, failed: p.is_failed(m.id) });
            }
            self.recipe = Some(RecipeMeshes { part, blank, tools });
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
        // Hidden, they are neither drawn, inferred on nor picked.
        self.guides = if doc.settings().display.guides {
            doc.placed_guides()
                .into_iter()
                .map(|g| SceneGuide { path: g.path, id: g.guide.id, shape: g.guide.shape.transformed(&g.transform) })
                .collect()
        } else {
            Vec::new()
        };

        // Where the recipe shown stands: at its occurrence.
        self.recipe_places.clear();
        if let (Some(r), Some(path), true) = (&self.recipe, &self.recipe_path, self.recipe_shown) {
            let chain: Vec<Id> = path.instances.iter().copied().chain([path.entity]).collect();
            if let Some(open) = self.placements.get(&r.part).and_then(|ps| ps.iter().find(|p| p.path == chain)).cloned() {
                self.recipe_places.push((open.clone(), r.blank, Piece::Blank));
                for t in &r.tools {
                    let place = Placement { transform: open.transform * t.placement, material: None, ..open.clone() };
                    self.recipe_places.push((place, t.key, Piece::Tool(t.machining)));
                }
            }
        }

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

    /// The mesh of `solid` under the key `key` (a part, or a recipe's solid): cached, or a
    /// stand-in asked for once.
    fn mesh_of(
        &mut self,
        solid: &Arc<Solid>,
        key: Id,
        faces: &BTreeMap<u32, Id>,
        seen: &mut std::collections::HashSet<usize>,
    ) -> DefinitionMesh {
        let at = Arc::as_ptr(solid) as usize;
        let entry = self.solids.entry(at).or_insert_with(|| Cached::pending(solid.clone()));
        // A part made unique shares its shape until it changes: ask for it once.
        if seen.insert(at) && entry.state == (State::Pending { asked: false }) {
            self.missing.push(solid.clone());
        }
        let mut part = entry.part.clone();
        part.solid = key;
        // The mesh changes with the state (a stand-in, then the real one).
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (at, entry.state == State::Ready, faces).hash(&mut h);
        DefinitionMesh { definition: key, revision: h.finish(), parts: vec![part], faces: faces.clone() }
    }

    /// Meshes the recipe of the part the occurrence `path` places (its blank and tools) from
    /// the next [`DisplayScene::sync`], or none; `shown`: drawn and inferred on instead of
    /// the occurrence's shape.
    pub fn set_recipe(&mut self, path: Option<EntityPath>, shown: bool) {
        self.recipe_path = path;
        self.recipe_shown = shown;
    }

    /// Is the recipe drawn and inferred on?
    pub fn recipe_shown(&self) -> bool {
        self.recipe_shown && self.recipe.is_some()
    }

    /// The exact bounding box of `solid` in its own axes, once the kernel has given its
    /// display (the scene's solids, the open part's recipe, machined parts' blanks).
    pub fn bounds_of(&self, solid: &Arc<Solid>) -> Option<(DVec3, DVec3)> {
        let c = self.solids.get(&(Arc::as_ptr(solid) as usize))?;
        (c.state == State::Ready).then_some(c.part.bounds).flatten()
    }

    /// The geometry inferences see, once per occurrence: every shape, but the open part's
    /// while its recipe is shown — its blank and its tools instead.
    pub fn inferable(&self) -> impl Iterator<Item = (&Placement, &PartMesh, Piece)> {
        let open = self.recipe_places.first().map(|(p, _, _)| p.path.clone());
        let shapes = self.parts().filter(move |(p, _)| open.as_ref() != Some(&p.path)).map(|(p, m)| (p, m, Piece::Shape));
        let recipe = self
            .recipe_places
            .iter()
            .filter_map(|(p, key, piece)| self.definitions.get(key).and_then(|d| d.parts.first()).map(|m| (p, m, *piece)));
        shapes.chain(recipe)
    }

    /// The mesh a hit of [`DisplayScene::pick_inferable`] is on, and its placement.
    pub fn inferable_at(&self, hit: &Hit) -> Option<(DAffine3, &PartMesh)> {
        let chain: Vec<Id> = hit.path.instances.iter().copied().chain([hit.path.entity]).collect();
        self.inferable().find(|(p, _, piece)| p.path == chain && *piece == hit.piece).map(|(p, m, _)| (p.transform, m))
    }

    /// The recipe shown, as meshed.
    pub fn recipe(&self) -> Option<&RecipeMeshes> {
        self.recipe.as_ref()
    }

    /// Parts, as the renderer uploads them (with the meshes a tool previews).
    pub fn definition_meshes(&self) -> impl Iterator<Item = &DefinitionMesh> {
        self.definitions.values().chain(&self.preview)
    }

    pub fn definition_mesh(&self, def: Id) -> Option<&DefinitionMesh> {
        if def.0 >= PREVIEW_KEY.0 && def.0 < TOOL_KEY {
            return self.preview.iter().find(|m| m.definition == def);
        }
        self.definitions.get(&def)
    }

    /// The meshes a tool previews in parts' stead, under [`preview_key`]s (none: dropped).
    pub fn set_preview(&mut self, meshes: Vec<DefinitionMesh>) {
        self.preview = meshes;
    }

    /// Every occurrence of every part.
    pub fn placements(&self) -> impl Iterator<Item = (Id, &[Placement])> {
        self.placements.iter().filter(|(d, _)| self.definitions.contains_key(d)).map(|(d, p)| (*d, p.as_slice()))
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
            Ok(d) => Cached::new(solid.clone(), d.mesh, d.snaps, d.bounds, State::Ready),
            Err(_) => Cached { state: State::Failed, ..Cached::pending(solid.clone()) },
        };
        self.solids.insert(key, cached);
    }

    /// Is `solid`'s mesh still on its way?
    pub fn is_pending(&self, solid: &Arc<Solid>) -> bool {
        self.solids.get(&(Arc::as_ptr(solid) as usize)).is_some_and(|c| matches!(c.state, State::Pending { .. }))
    }

    /// The active section: what it cuts away is neither picked nor inferred.
    /// Every guide of the document, placed in the world (none while they are hidden).
    pub fn guides(&self) -> &[SceneGuide] {
        &self.guides
    }

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

    /// The nearest shape along `ray`, among those `keep` accepts.
    pub fn pick(&self, ray: &Ray, keep: impl Fn(&EntityPath) -> bool) -> Option<Hit> {
        self.pick_among(ray, self.parts().map(|(p, m)| (p, m, Piece::Shape)), |o| keep(o.path))
    }

    /// The nearest geometry inferences see along `ray` ([`DisplayScene::inferable`]), among
    /// what `keep` accepts.
    pub fn pick_inferable(&self, ray: &Ray, keep: impl Fn(&Owner<'_>) -> bool) -> Option<Hit> {
        self.pick_among(ray, self.inferable(), keep)
    }

    fn pick_among<'s>(
        &'s self,
        ray: &Ray,
        items: impl Iterator<Item = (&'s Placement, &'s PartMesh, Piece)>,
        keep: impl Fn(&Owner<'_>) -> bool,
    ) -> Option<Hit> {
        // The best hit, with what breaks a tie with it: does the ray enter the solid there,
        // and the volume of the solid's box.
        let mut best: Option<(Hit, bool, f64)> = None;
        for (p, part, piece) in items {
            let Some(b) = part.bounds else { continue };
            let inv = p.transform.inverse();
            let o = inv.transform_point3(ray.origin);
            let d = inv.transform_vector3(ray.dir);
            if !ray_box(o, d, b, ray.t_min) {
                continue;
            }
            let path = p.entity_path();
            if !keep(&Owner { path: &path, piece }) {
                continue;
            }
            // The section, in the part's axes (placements are rigid).
            let clip = self.section.map(|s| (inv.transform_point3(s.origin), inv.transform_vector3(s.normal)));
            let Some((t, tri, n_local)) = ray_mesh(o, d, &part.mesh, ray.t_min, clip) else { continue };
            // t is in units of the local direction: re-measure it in the world.
            let point = p.transform.transform_point3(o + d * t);
            let world_t = (point - ray.origin).dot(ray.dir);
            let entering = n_local.dot(d) < 0.0;
            let volume = (b.1 - b.0).max(DVec3::ZERO).element_product();
            if best.as_ref().is_some_and(|(b, b_entering, b_volume)| {
                if (world_t - b.t).abs() > tie_tolerance(b.t) {
                    return b.t < world_t;
                }
                // Coincident faces: the solid the ray enters, rather than one it leaves (seen
                // through a section, from inside), then the smaller one, then the scene order.
                match (entering, *b_entering) {
                    (true, false) => false,
                    (false, true) => true,
                    _ => *b_volume <= volume,
                }
            }) {
                continue;
            }
            let mut normal = p.transform.matrix3.inverse().transpose().mul_vec3(n_local).normalize_or_zero();
            if normal.dot(ray.dir) > 0.0 {
                normal = -normal;
            }
            let face = part.mesh.tri_face.get(tri).copied().unwrap_or(0);
            best = Some((Hit { path, piece, face, point, normal, t: world_t }, entering, volume));
        }
        best.map(|(hit, _, _)| hit)
    }
}

impl DisplayScene {
    /// The machining of the recipe shown whose tool `ray` meets first, the part drawn by
    /// `part_to_world`, with the distance along the ray.
    pub fn pick_tool(&self, ray: &Ray, part_to_world: DAffine3) -> Option<(Id, f64)> {
        let recipe = self.recipe.as_ref()?;
        let mut best: Option<(Id, f64)> = None;
        for tool in &recipe.tools {
            let Some(part) = self.definitions.get(&tool.key).and_then(|d| d.parts.first()) else { continue };
            let Some(b) = part.bounds else { continue };
            let t = part_to_world * tool.placement;
            let inv = t.inverse();
            let (o, d) = (inv.transform_point3(ray.origin), inv.transform_vector3(ray.dir));
            if !ray_box(o, d, b, ray.t_min) {
                continue;
            }
            let clip = self.section.map(|s| (inv.transform_point3(s.origin), inv.transform_vector3(s.normal)));
            let Some((k, _, _)) = ray_mesh(o, d, &part.mesh, ray.t_min, clip) else { continue };
            let world_t = (t.transform_point3(o + d * k) - ray.origin).dot(ray.dir);
            if best.is_none_or(|(_, b)| world_t < b) {
                best = Some((tool.machining, world_t));
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
        let c =
            DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
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

/// How far apart (mm, along the ray) two hits at about `t` may be and still be on coincident
/// faces: meshes are single precision, placed by double transforms.
fn tie_tolerance(t: f64) -> f64 {
    1e-3 + 1e-6 * t.abs()
}

/// The nearest triangle of `m` along `o + t·d`: (t, triangle, geometric normal, pointing out
/// of the solid as its vertex normals do); a point `clip` (a plane's point and normal) cuts
/// away is not a hit.
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
        let mut n = e1.cross(e2);
        let vn = |i: u32| m.normals.get(i as usize).map_or(DVec3::ZERO, |n| DVec3::new(n[0] as f64, n[1] as f64, n[2] as f64));
        if n.dot(vn(tri[0]) + vn(tri[1]) + vn(tri[2])) < 0.0 {
            n = -n;
        }
        best = Some((t, k, n));
    }
    best
}

/// `mesh` stretched (ARCHITECTURE.md §6 sexies), by a continuous map along the axis: where a
/// section holds matter wholly (`spans`, as [`solidsketch_kernel::Stretch::section_spans`]
/// gives them) the section's move, in between a linear blend. The B-rep's vertices all lie in
/// that matter, so they go where the stretch puts them; the mesh's other points lie on what
/// crosses a cutter — planes and cylinders along the axis, which any increasing map along it
/// keeps. The map being affine only between its knots, the triangles are first cut at them:
/// none then turns over, however hard the compression (moving each vertex by its section, or
/// a long triangle by a map bent under it, did).
pub fn stretched_mesh(mesh: &Mesh, stretch: &solidsketch_kernel::Stretch, spans: &[Option<(f64, f64)>]) -> Mesh {
    let a = stretch.axis();
    let knots: Vec<(f64, f64)> =
        spans.iter().zip(stretch.moves()).filter_map(|(s, m)| s.map(|(lo, hi)| [(lo, *m), (hi, *m)])).flatten().collect();
    let shift = |x: f64| -> f64 {
        let (Some(first), Some(last)) = (knots.first(), knots.last()) else { return stretch.move_at(x) };
        if x <= first.0 {
            return first.1;
        }
        if x >= last.0 {
            return last.1;
        }
        let i = knots.partition_point(|k| k.0 <= x);
        let ((x0, m0), (x1, m1)) = (knots[i - 1], knots[i]);
        if x1 - x0 <= 0.0 { m1 } else { m0 + (m1 - m0) * (x - x0) / (x1 - x0) }
    };
    // Where the map bends: the knots between which the move changes.
    let mut bends: Vec<f64> = knots.windows(2).filter(|w| (w[0].1 - w[1].1).abs() > 1e-9).flat_map(|w| [w[0].0, w[1].0]).collect();
    bends.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    let cut = cut_at(mesh, a, &bends);
    let moved = |p: &[f32; 3]| -> [f32; 3] {
        let w = cut.origin + DVec3::from_array(p.map(f64::from));
        (w + a * shift(a.dot(w)) - cut.origin).as_vec3().to_array()
    };
    Mesh {
        positions: cut.positions.iter().map(moved).collect(),
        edges: cut.edges.iter().map(|e| e.iter().map(moved).collect()).collect(),
        ..cut
    }
}

/// `mesh` with its triangles cut by the planes `a · p = x` for each `x` of `at` (the pieces
/// fanned into triangles, keeping their face); its edges as they are.
fn cut_at(mesh: &Mesh, a: DVec3, at: &[f64]) -> Mesh {
    if at.is_empty() {
        return mesh.clone();
    }
    let o = mesh.origin;
    let point = |i: u32| DVec3::from_array(mesh.positions[i as usize].map(f64::from));
    let normal = |i: u32| DVec3::from_array(mesh.normals.get(i as usize).copied().unwrap_or([0.0, 0.0, 1.0]).map(f64::from));
    let (mut positions, mut normals, mut indices, mut tri_face) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (t, tri) in mesh.indices.chunks(3).enumerate() {
        let face = mesh.tri_face.get(t).copied().unwrap_or(0);
        let mut pieces: Vec<Vec<(DVec3, DVec3)>> = vec![tri.iter().map(|&i| (point(i), normal(i))).collect()];
        for &x in at {
            let level = |p: DVec3| a.dot(o + p) - x;
            let mut next = Vec::with_capacity(pieces.len() + 1);
            for poly in pieces {
                let (lo, hi) = poly.iter().fold((f64::MAX, f64::MIN), |(l, h), (p, _)| (l.min(level(*p)), h.max(level(*p))));
                if lo >= -1e-9 || hi <= 1e-9 {
                    next.push(poly);
                    continue;
                }
                // Both sides of the plane: each keeps its corners and the crossings.
                let (mut below, mut above) = (Vec::new(), Vec::new());
                for k in 0..poly.len() {
                    let ((p, n), (q, m)) = (poly[k], poly[(k + 1) % poly.len()]);
                    let (lp, lq) = (level(p), level(q));
                    if lp <= 0.0 {
                        below.push((p, n));
                    }
                    if lp >= 0.0 {
                        above.push((p, n));
                    }
                    if (lp < 0.0 && lq > 0.0) || (lp > 0.0 && lq < 0.0) {
                        let s = lp / (lp - lq);
                        let c = (p.lerp(q, s), n.lerp(m, s).normalize_or_zero());
                        below.push(c);
                        above.push(c);
                    }
                }
                next.extend([below, above].into_iter().filter(|p| p.len() >= 3));
            }
            pieces = next;
        }
        for poly in pieces {
            let base = positions.len() as u32;
            for (p, n) in &poly {
                positions.push(p.as_vec3().to_array());
                normals.push(n.as_vec3().to_array());
            }
            for k in 1..poly.len() as u32 - 1 {
                indices.extend([base, base + k, base + k + 1]);
                tri_face.push(face);
            }
        }
    }
    Mesh { positions, normals, indices, tri_face, ..mesh.clone() }
}

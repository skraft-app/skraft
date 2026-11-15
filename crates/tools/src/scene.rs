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
//! - The recipe of one open part (ARCHITECTURE.md §2.9, §2.11), when its machinings are shown:
//!   its positive bodies (the blank) and its negative ones (the tools), meshed like shapes,
//!   under keys of their own ([`RecipeMeshes`]); one mesh per distinct solid, a copied body
//!   sharing its original's. Drawn by the
//!   view; for inferences ([`DisplayScene::inferable`]) they stand in for the open
//!   occurrence's shape, each saying what it is ([`Piece`]).

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use solidsketch_kernel::{Display, Mesh, Quality, Snaps, Solid};
use solidsketch_model::{
    Assembly, ComponentKind, Document, EntityPath, FastMap, FastSet, GuideShape, Id, MAX_DEPTH, Occurrence, Section, Sign,
};

use crate::bvh::{Aabb, Bvh, ray_box_entry};
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
    /// Hidden by the user: it, or an occurrence above it, out of the active path (what is
    /// being edited shows).
    pub hidden: bool,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum State {
    /// Not meshed yet: drawn as its bounding box; `asked` once handed to the editor.
    Pending {
        asked: bool,
    },
    /// Drawn by its coarse preview mesh, the final one on its way.
    Preview,
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
    /// A positive body of the open part (§2.11).
    Blank(Id),
    /// A negative body of the open part: a machining's tool.
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

/// One body of the recipe shown.
#[derive(Clone, Debug, PartialEq)]
pub struct RecipeTool {
    pub body: Id,
    /// The mesh of its solid.
    pub key: Id,
    /// Its solid in the part's axes.
    pub placement: DAffine3,
    pub failed: bool,
}

/// The meshes of the recipe shown, in the part's axes: its positive bodies (`blanks`) and its
/// negative ones (`tools`, the machinings).
#[derive(Clone, Debug, PartialEq)]
pub struct RecipeMeshes {
    pub part: Id,
    pub blanks: Vec<RecipeTool>,
    pub tools: Vec<RecipeTool>,
}

impl RecipeMeshes {
    /// Every body: the positive ones, then the negative ones.
    pub fn bodies(&self) -> impl Iterator<Item = &RecipeTool> {
        self.blanks.iter().chain(&self.tools)
    }

    /// The body `id`.
    pub fn body(&self, id: Id) -> Option<&RecipeTool> {
        self.bodies().find(|b| b.body == id)
    }

    /// Is the body `id` drawn as a ghost (a blank, or a machining that did not fail)?
    pub fn is_ghost(&self, id: Id) -> bool {
        self.blanks.iter().any(|b| b.body == id) || self.tools.iter().any(|b| b.body == id && !b.failed)
    }
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
    solids: FastMap<usize, Cached>,
    definitions: BTreeMap<Id, DefinitionMesh>,
    /// The meshes a tool previews, under [`preview_key`]s.
    preview: Vec<DefinitionMesh>,
    placements: BTreeMap<Id, Vec<Placement>>,
    /// Solids of the document without a mesh, not handed out yet.
    missing: Vec<Arc<Solid>>,
    bounds: Option<(DVec3, DVec3)>,
    /// The active section: picking ignores what it cuts away.
    section: Option<Section>,
    /// The chain (root first) of the part open or the assembly being edited: the occurrences
    /// hidden along it show.
    active: Vec<Id>,
    /// While the rest of the model is hidden, `active`: only what lies under it is picked,
    /// inferred on and framed (empty: everything).
    shown_under: Vec<Id>,
    /// The occurrences hidden are shown (picked, inferred on and framed).
    show_hidden: bool,
    /// The open occurrence whose recipe is shown, its meshes, and their placements in the
    /// world (the blank's, then each tool's).
    recipe_path: Option<EntityPath>,
    /// Drawn and inferred on (else only meshed: the entity info reads its tools' sizes).
    recipe_shown: bool,
    recipe: Option<RecipeMeshes>,
    recipe_places: Vec<(Placement, Id, Piece)>,
    /// X-ray: everything drawn see-through, hiding nothing from inferences.
    xray: bool,
    guides: Vec<SceneGuide>,
    /// The placed meshes, indexed for picking and regions.
    index: SceneIndex,
    /// Counts the syncs and the meshes taken in: what was worked out from the scene holds
    /// while it stays.
    generation: u64,
    /// By solid (its key in `solids`), the definitions drawing it: what a mesh coming in
    /// changes ([`DisplayScene::refresh_meshes`]).
    users: FastMap<usize, Vec<Id>>,
    /// The solids whose display came since the last sync or refresh.
    arrived: Vec<usize>,
}

/// The world bounds of the `entries` shown ([`is_shown`]): zoom extents frames the context
/// alone while the rest of the model is hidden, and never what the user hid.
fn shown_bounds(
    placements: &BTreeMap<Id, Vec<Placement>>,
    shown_under: &[Id],
    show_hidden: bool,
    entries: &[Entry],
) -> Option<(DVec3, DVec3)> {
    let shown = |e: &&Entry| is_shown(&placements[&e.def][e.place as usize], shown_under, show_hidden);
    entries.iter().filter(shown).filter_map(|e| e.bounds).reduce(|a, b| union(Some(a), b))
}

/// Is `p` shown: under `shown_under` (the context, while the rest of the model is hidden),
/// and not hidden by the user (unless `show_hidden`)?
fn is_shown(p: &Placement, shown_under: &[Id], show_hidden: bool) -> bool {
    (show_hidden || !p.hidden) && p.path.starts_with(shown_under)
}

/// Is a path (`path`, root first) whose deepest hidden occurrence is at `hidden_at` hidden,
/// `active` being edited? The occurrences hidden along the active path show.
fn hidden_off(path: &[Id], hidden_at: Option<u32>, active: &[Id]) -> bool {
    let along = path.iter().zip(active).take_while(|(a, b)| a == b).count();
    hidden_at.is_some_and(|d| d as usize >= along)
}

/// Is the occurrence `chain` names (root first, itself last) hidden by the user — it, or an
/// occurrence above it, out of the `active` path ([`hidden_off`])?
pub(crate) fn is_chain_hidden(doc: &Document, chain: &[Id], active: &[Id]) -> bool {
    let mut at = doc.root();
    let mut hidden_at = None;
    for (k, &id) in chain.iter().enumerate() {
        let Some(o) = doc.assembly(at).ok().and_then(|a| a.occurrence(id)) else { return false };
        if o.hidden {
            hidden_at = Some(k as u32);
        }
        at = o.component;
    }
    hidden_off(chain, hidden_at, active)
}

/// One part mesh at one placement ([`DisplayScene::parts`] yields them in this order).
#[derive(Clone, Copy, Debug)]
struct Entry {
    def: Id,
    /// Its placement, in `placements[def]`.
    place: u32,
    /// Its mesh, in `definitions[def].parts`.
    part: u32,
    /// World (none for an empty mesh).
    bounds: Option<Aabb>,
}

/// The best hit so far, with what breaks a tie with it: does the ray enter the solid there,
/// the volume of the solid's box, its place in the scene's order.
struct Best {
    hit: Hit,
    entering: bool,
    volume: f64,
    rank: usize,
}

/// What a ray or a region meets without testing every placement (rebuilt by each
/// [`DisplayScene::sync`]): the entries in the scene's order, the same sorted by path (an
/// occurrence's parts, at any depth, form a run), and a hierarchy of their world boxes.
#[derive(Default)]
struct SceneIndex {
    entries: Vec<Entry>,
    /// Each definition's run of `entries` (they are made definition by definition).
    runs: FastMap<Id, std::ops::Range<u32>>,
    by_path: Vec<u32>,
    bvh: Bvh,
}

impl DisplayScene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Brings the scene up to date with `doc`. Solids not seen before get a stand-in until
    /// their mesh comes ([`DisplayScene::take_missing`], [`DisplayScene::insert`]).
    pub fn sync(&mut self, doc: &Document) {
        self.generation += 1;
        let mut seen = FastSet::default();
        self.definitions.clear();
        self.users.clear();
        self.arrived.clear();
        self.missing.clear();
        for c in doc.components() {
            let ComponentKind::Part(p) = &c.kind else { continue };
            self.define(p.shape(), c.id, &p.faces, &mut seen);
        }
        self.recipe = None;
        let shown = self.recipe_path.as_ref().and_then(|path| doc.resolve_part(path).ok());
        if let Some((part, p, _)) = shown {
            // One mesh per definition (its copies share it), keyed by it.
            for d in &p.defs {
                self.define(&d.solid, Id(RECIPE_KEY | TOOL_KEY | d.id.0), &BTreeMap::new(), &mut seen);
            }
            let (mut blanks, mut tools) = (Vec::new(), Vec::with_capacity(p.bodies.len()));
            for b in &p.bodies {
                let key = Id(RECIPE_KEY | TOOL_KEY | b.def.0);
                let t = RecipeTool { body: b.id, key, placement: b.placement, failed: p.is_failed(b.id) };
                if b.sign == Sign::Positive { blanks.push(t) } else { tools.push(t) }
            }
            self.recipe = Some(RecipeMeshes { part, blanks, tools });
        }
        // Keep what the document shows, and what something else holds (undo, a job).
        self.solids.retain(|key, c| seen.contains(key) || Arc::strong_count(&c.solid) > 1);

        self.placements.clear();
        for p in doc.placed_parts() {
            let mut path = p.path.instances;
            path.push(p.path.entity);
            let hidden = hidden_off(&path, p.hidden_at, &self.active);
            self.placements.entry(p.part).or_default().push(Placement { path, transform: p.transform, material: p.material, hidden });
        }
        self.section = doc.settings().display.active_section();
        self.show_hidden = doc.settings().display.show_hidden;
        // Hidden, they are neither drawn, inferred on nor picked.
        self.guides = if doc.settings().display.guides {
            doc.placed_guides()
                .into_iter()
                .filter(|g| g.path.starts_with(&self.shown_under) && (self.show_hidden || !hidden_off(&g.path, g.hidden_at, &self.active)))
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
                for b in &r.blanks {
                    let place = Placement { transform: open.transform * b.placement, ..open.clone() };
                    self.recipe_places.push((place, b.key, Piece::Blank(b.body)));
                }
                for t in &r.tools {
                    let place = Placement { transform: open.transform * t.placement, material: None, ..open.clone() };
                    self.recipe_places.push((place, t.key, Piece::Tool(t.body)));
                }
            }
        }

        self.reindex();
    }

    /// The index of what `placements` places (and the scene's bounds, from the same boxes).
    fn reindex(&mut self) {
        let mut entries = Vec::with_capacity(self.index.entries.len());
        let mut paths: Vec<(&[Id], u32)> = Vec::with_capacity(self.index.entries.len());
        let mut runs = FastMap::default();
        for (def, places) in &self.placements {
            let Some(dm) = self.definitions.get(def) else { continue };
            let start = entries.len() as u32;
            for (place, p) in places.iter().enumerate() {
                for (part, m) in dm.parts.iter().enumerate() {
                    let bounds = m.bounds.map(|b| transform_bounds(&p.transform, b));
                    paths.push((&p.path, entries.len() as u32));
                    entries.push(Entry { def: *def, place: place as u32, part: part as u32, bounds });
                }
            }
            runs.insert(*def, start..entries.len() as u32);
        }
        paths.sort_unstable();
        let by_path = paths.into_iter().map(|(_, i)| i).collect();
        self.bounds = shown_bounds(&self.placements, &self.shown_under, self.show_hidden, &entries);
        // The same entries as before but for their boxes (occurrences moved, materials or
        // visibility changed): the hierarchy refit, unless it grew loose.
        let old = &self.index.entries;
        let same = entries.len() == old.len()
            && entries
                .iter()
                .zip(old)
                .all(|(a, b)| (a.def, a.place, a.part, a.bounds.is_some()) == (b.def, b.place, b.part, b.bounds.is_some()));
        let mut bvh = std::mem::take(&mut self.index.bvh);
        if !same || !bvh.refit(|i| entries[i as usize].bounds.expect("only boxes are in the hierarchy")) {
            bvh = Bvh::build(entries.iter().enumerate().filter_map(|(i, e)| e.bounds.map(|b| (i as u32, b))));
        }
        self.index = SceneIndex { entries, runs, by_path, bvh };
    }

    /// An entry's placement and mesh.
    fn entry(&self, i: u32) -> (&Placement, &PartMesh) {
        let e = &self.index.entries[i as usize];
        (&self.placements[&e.def][e.place as usize], &self.definitions[&e.def].parts[e.part as usize])
    }

    /// The entries placed at `chain` or below it (an occurrence's path, root first), in the
    /// scene's order.
    fn entries_under(&self, chain: &[Id]) -> &[u32] {
        let path = |i: &u32| {
            let e = &self.index.entries[*i as usize];
            self.placements[&e.def][e.place as usize].path.as_slice()
        };
        let by = &self.index.by_path;
        let start = by.partition_point(|i| path(i) < chain);
        let len = by[start..].partition_point(|i| path(i).starts_with(chain));
        &by[start..start + len]
    }

    /// The open occurrence whose shape the recipe stands in for, inferences-wise.
    fn recipe_open(&self) -> Option<&[Id]> {
        self.recipe_places.first().map(|(p, _, _)| p.path.as_slice())
    }

    /// The recipe's pieces placed, with their meshes.
    fn recipe_pieces(&self) -> impl Iterator<Item = (&Placement, &PartMesh, Piece)> {
        self.recipe_places
            .iter()
            .filter_map(|(p, key, piece)| self.definitions.get(key).and_then(|d| d.parts.first()).map(|m| (p, m, *piece)))
    }

    /// The geometry inferences see ([`DisplayScene::inferable`]) whose world box `accept`s, in
    /// the same order: `accept` must hold of a box whenever it holds of one inside it.
    pub fn inferable_where(&self, accept: impl Fn(&(DVec3, DVec3)) -> bool) -> Vec<(&Placement, &PartMesh, Piece)> {
        let open = self.recipe_open();
        let mut hits = Vec::new();
        self.index.bvh.visit(&accept, |i| hits.push(i));
        hits.sort_unstable();
        let shapes = hits
            .into_iter()
            .map(|i| self.entry(i))
            .filter(|(p, _)| open != Some(p.path.as_slice()) && self.is_shown(p))
            .map(|(p, m)| (p, m, Piece::Shape));
        let recipe = self.recipe_pieces().filter(|(p, m, _)| m.bounds.is_some_and(|b| accept(&transform_bounds(&p.transform, b))));
        shapes.chain(recipe).collect()
    }

    /// Every part shown whose world box `accept`s, as [`DisplayScene::parts`] yields them,
    /// with its rank there: `accept` must hold of a box whenever it holds of one inside it.
    pub fn parts_where(&self, accept: impl Fn(&(DVec3, DVec3)) -> bool) -> Vec<(usize, &Placement, &PartMesh)> {
        let mut hits = Vec::new();
        self.index.bvh.visit(&accept, |i| hits.push(i));
        hits.sort_unstable();
        hits.into_iter()
            .map(|i| {
                let (p, m) = self.entry(i);
                (i as usize, p, m)
            })
            .filter(|(_, p, _)| self.is_shown(p))
            .collect()
    }

    /// How many parts are shown at `chain` or below (an occurrence's path, root first), and
    /// the first one's rank in [`DisplayScene::parts`].
    pub fn parts_under(&self, chain: &[Id]) -> (usize, Option<usize>) {
        let (mut n, mut first) = (0, None);
        for &i in self.entries_under(chain) {
            if self.is_shown(self.entry(i).0) {
                n += 1;
                first = Some(first.map_or(i, |f: u32| f.min(i)));
            }
        }
        (n, first.map(|i| i as usize))
    }

    /// The geometry inferences see on the occurrence `path` (its shape's meshes, or the
    /// recipe's while it is the open one).
    pub fn inferable_of(&self, path: &EntityPath) -> Vec<(&Placement, &PartMesh, Piece)> {
        let chain: Vec<Id> = path.instances.iter().copied().chain([path.entity]).collect();
        if self.recipe_open() == Some(chain.as_slice()) {
            return self.recipe_pieces().collect();
        }
        self.entries_under(&chain)
            .iter()
            .map(|i| self.entry(*i))
            .filter(|(p, _)| p.path == chain)
            .map(|(p, m)| (p, m, Piece::Shape))
            .collect()
    }

    /// The mesh of `solid` under the key `key` (a part, or a recipe's solid): cached, or a
    /// stand-in asked for once.
    fn mesh_of(&mut self, solid: &Arc<Solid>, key: Id, faces: &BTreeMap<u32, Id>, seen: &mut FastSet<usize>) -> DefinitionMesh {
        let at = Arc::as_ptr(solid) as usize;
        let entry = self.solids.entry(at).or_insert_with(|| Cached::pending(solid.clone()));
        // A part made unique shares its shape until it changes: ask for it once.
        if seen.insert(at) && entry.state == (State::Pending { asked: false }) {
            self.missing.push(solid.clone());
        }
        let mut part = entry.part.clone();
        part.solid = key;
        DefinitionMesh { definition: key, revision: revision(at, entry.state, faces), parts: vec![part], faces: faces.clone() }
    }

    /// The definition `key` drawn from `solid` ([`DisplayScene::mesh_of`]), recorded as one
    /// of its users.
    fn define(&mut self, solid: &Arc<Solid>, key: Id, faces: &BTreeMap<u32, Id>, seen: &mut FastSet<usize>) {
        let dm = self.mesh_of(solid, key, faces, seen);
        self.users.entry(Arc::as_ptr(solid) as usize).or_default().push(key);
        self.definitions.insert(key, dm);
    }

    /// Takes in the displays come since the last sync, the document the same: only the
    /// definitions drawing them change, and the boxes of their placements — the placements,
    /// the guides and the recipe stay. Cheaper than a [`DisplayScene::sync`] by far for a
    /// model of many parts, whose meshes come a few at a time.
    pub fn refresh_meshes(&mut self) {
        let arrived = std::mem::take(&mut self.arrived);
        let mut changed = Vec::new();
        for at in arrived {
            let (Some(keys), Some(c)) = (self.users.get(&at), self.solids.get(&at)) else { continue };
            for &key in keys {
                let Some(old) = self.definitions.get_mut(&key) else { continue };
                let mut part = c.part.clone();
                part.solid = key;
                old.revision = revision(at, c.state, &old.faces);
                old.parts = vec![part];
                changed.push(key);
            }
        }
        if changed.is_empty() {
            return;
        }
        self.generation += 1;
        // The boxes of their placements; the hierarchy refit, or made again when a mesh
        // gained or lost its box (it holds only those with one).
        let mut rebuild = false;
        for def in &changed {
            let Some(run) = self.index.runs.get(def).cloned() else { continue };
            for i in run {
                let e = self.index.entries[i as usize];
                let place = &self.placements[&e.def][e.place as usize];
                let bounds = self.definitions[&e.def]
                    .parts
                    .get(e.part as usize)
                    .and_then(|m| m.bounds)
                    .map(|b| transform_bounds(&place.transform, b));
                rebuild |= bounds.is_some() != e.bounds.is_some();
                self.index.entries[i as usize].bounds = bounds;
            }
        }
        let entries = &self.index.entries;
        if rebuild || !self.index.bvh.refit(|i| entries[i as usize].bounds.expect("only boxes are in the hierarchy")) {
            self.index.bvh = Bvh::build(entries.iter().enumerate().filter_map(|(i, e)| e.bounds.map(|b| (i as u32, b))));
        }
        self.bounds = shown_bounds(&self.placements, &self.shown_under, self.show_hidden, entries);
    }

    /// Meshes the recipe of the part the occurrence `path` places (its blank and tools) from
    /// the next [`DisplayScene::sync`], or none; `shown`: drawn and inferred on instead of
    /// the occurrence's shape.
    pub fn set_recipe(&mut self, path: Option<EntityPath>, shown: bool) {
        self.recipe_path = path;
        self.recipe_shown = shown;
    }

    /// The part open or the assembly being edited (`chain`, root first; empty: the model),
    /// from the next [`DisplayScene::sync`]: the occurrences hidden along it show, and with
    /// `hide_rest_of_model`, the rest of the model is hidden from picking, inferences and the
    /// bounds.
    pub fn set_active(&mut self, chain: Vec<Id>, hide_rest_of_model: bool) {
        self.shown_under = if hide_rest_of_model { chain.clone() } else { Vec::new() };
        self.active = chain;
    }

    /// Is `p` shown: neither in the rest of the model, hidden, nor hidden by the user (but
    /// with the hidden shown)?
    pub fn is_shown(&self, p: &Placement) -> bool {
        is_shown(p, &self.shown_under, self.show_hidden)
    }

    /// Is the recipe drawn and inferred on?
    pub fn recipe_shown(&self) -> bool {
        self.recipe_shown && self.recipe.is_some()
    }

    /// The exact bounding box of `solid` in its own axes, once the kernel has given its
    /// display (the scene's solids, the open part's recipe, machined parts' blanks).
    pub fn bounds_of(&self, solid: &Arc<Solid>) -> Option<(DVec3, DVec3)> {
        let c = self.solids.get(&(Arc::as_ptr(solid) as usize))?;
        matches!(c.state, State::Ready | State::Preview).then_some(c.part.bounds).flatten()
    }

    /// The geometry inferences see, once per occurrence: every shape, but the open part's
    /// while its recipe is shown — its blank and its tools instead.
    pub fn inferable(&self) -> impl Iterator<Item = (&Placement, &PartMesh, Piece)> {
        let open = self.recipe_open();
        let shapes =
            self.parts().filter(move |(p, _)| open != Some(p.path.as_slice()) && self.is_shown(p)).map(|(p, m)| (p, m, Piece::Shape));
        shapes.chain(self.recipe_pieces())
    }

    /// The mesh a hit of [`DisplayScene::pick_inferable`] is on, and its placement.
    pub fn inferable_at(&self, hit: &Hit) -> Option<(DAffine3, &PartMesh)> {
        self.inferable_of(&hit.path).into_iter().find(|(_, _, piece)| *piece == hit.piece).map(|(p, m, _)| (p.transform, m))
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

    /// Are solids of the document without a mesh not handed out yet ([`DisplayScene::take_missing`])?
    pub fn has_missing(&self) -> bool {
        !self.missing.is_empty()
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
        self.insert_at(solid, display, Quality::Final);
    }

    /// [`DisplayScene::insert`] of a mesh at `quality`: a preview drawn until the final one
    /// comes (one coming after the final is dropped); a refused preview changes nothing.
    pub fn insert_at(&mut self, solid: &Arc<Solid>, display: Result<Display, String>, quality: Quality) {
        let key = Arc::as_ptr(solid) as usize;
        let now = self.solids.get(&key).map(|c| c.state);
        if quality == Quality::Preview && (display.is_err() || matches!(now, Some(State::Ready | State::Failed))) {
            return;
        }
        // The final mesh refused: the preview stays drawn.
        if display.is_err()
            && now == Some(State::Preview)
            && let Some(c) = self.solids.get_mut(&key)
        {
            c.state = State::Failed;
            self.arrived.push(key);
            return;
        }
        let cached = match display {
            Ok(d) => {
                let state = if quality == Quality::Preview { State::Preview } else { State::Ready };
                Cached::new(solid.clone(), d.mesh, d.snaps, d.bounds, state)
            }
            Err(_) => Cached { state: State::Failed, ..Cached::pending(solid.clone()) },
        };
        self.solids.insert(key, cached);
        self.arrived.push(key);
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

    /// Changes with each [`DisplayScene::sync`] (the document, or a mesh come).
    pub fn generation(&self) -> u64 {
        self.generation
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
        let chain: Vec<Id> = path.instances.iter().copied().chain([path.entity]).collect();
        self.entries_under(&chain).iter().map(|i| self.entry(*i)).find(|(p, _)| p.path == chain).map(|(p, part)| (p.transform, part))
    }

    /// The nearest shape along `ray`, among those `keep` accepts.
    pub fn pick(&self, ray: &Ray, keep: impl Fn(&EntityPath) -> bool) -> Option<Hit> {
        self.pick_among(ray, false, |o| keep(o.path))
    }

    /// X-ray: everything drawn see-through, hiding nothing from inferences.
    pub fn set_xray(&mut self, on: bool) {
        self.xray = on;
    }

    /// Is `piece` drawn see-through, hiding nothing from inferences: anything in X-ray, else
    /// an open part's body shown as a ghost (a blank, a machining that did not fail, §2.11)?
    pub fn see_through(&self, piece: Piece) -> bool {
        match (piece, &self.recipe) {
            _ if self.xray => true,
            (Piece::Blank(_), Some(_)) => true,
            (Piece::Tool(id), Some(r)) => r.tools.iter().any(|t| t.body == id && !t.failed),
            _ => false,
        }
    }

    /// The nearest geometry inferences see along `ray` ([`DisplayScene::inferable`]), among
    /// what `keep` accepts.
    pub fn pick_inferable(&self, ray: &Ray, keep: impl Fn(&Owner<'_>) -> bool) -> Option<Hit> {
        self.pick_among(ray, true, keep)
    }

    /// The nearest of the shapes (`inferable`: what inferences see instead) along `ray`: the
    /// index's boxes nearest first, those beyond the best hit left out.
    fn pick_among(&self, ray: &Ray, inferable: bool, keep: impl Fn(&Owner<'_>) -> bool) -> Option<Hit> {
        let mut best: Option<Best> = None;
        // The recipe's pieces come after every shape in the scene's order.
        if inferable {
            let after = self.index.entries.len();
            for (k, (p, part, piece)) in self.recipe_pieces().enumerate() {
                self.try_hit(ray, p, part, piece, after + k, &keep, &mut best);
            }
        }
        let open = if inferable { self.recipe_open() } else { None };
        self.index.bvh.along_ray(ray.origin, ray.dir, ray.t_min, |i| {
            let (p, part) = self.entry(i);
            if open != Some(p.path.as_slice()) {
                self.try_hit(ray, p, part, Piece::Shape, i as usize, &keep, &mut best);
            }
            // A hit further than the best one by more than a tie never wins.
            best.as_ref().map_or(f64::INFINITY, |b| b.hit.t + tie_tolerance(b.hit.t))
        });
        best.map(|b| b.hit)
    }

    /// [`DisplayScene::pick`] (`inferable`: [`DisplayScene::pick_inferable`]) testing every
    /// placed mesh, as the index must find.
    #[cfg(test)]
    pub(crate) fn pick_by_scan(&self, ray: &Ray, inferable: bool) -> Option<Hit> {
        let mut best: Option<Best> = None;
        let open = if inferable { self.recipe_open() } else { None };
        let keep = |_: &Owner<'_>| true;
        for i in 0..self.index.entries.len() as u32 {
            let (p, part) = self.entry(i);
            if open != Some(p.path.as_slice()) {
                self.try_hit(ray, p, part, Piece::Shape, i as usize, &keep, &mut best);
            }
        }
        if inferable {
            let after = self.index.entries.len();
            for (k, (p, part, piece)) in self.recipe_pieces().enumerate() {
                self.try_hit(ray, p, part, piece, after + k, &keep, &mut best);
            }
        }
        best.map(|b| b.hit)
    }

    /// `ray` against one placed mesh (`rank`: its place in the scene's order): kept in `best`
    /// when it hits nearer, or as near and better.
    #[allow(clippy::too_many_arguments)]
    fn try_hit(
        &self,
        ray: &Ray,
        p: &Placement,
        part: &PartMesh,
        piece: Piece,
        rank: usize,
        keep: &impl Fn(&Owner<'_>) -> bool,
        best: &mut Option<Best>,
    ) {
        let Some(b) = part.bounds else { return };
        if !self.is_shown(p) {
            return;
        }
        let inv = p.transform.inverse();
        let o = inv.transform_point3(ray.origin);
        let d = inv.transform_vector3(ray.dir);
        if !ray_box(o, d, b, ray.t_min) {
            return;
        }
        let path = p.entity_path();
        if !keep(&Owner { path: &path, piece }) {
            return;
        }
        // The section, in the part's axes (placements are rigid).
        let clip = self.section.map(|s| (inv.transform_point3(s.origin), inv.transform_vector3(s.normal)));
        let Some((t, tri, n_local)) = ray_mesh(o, d, &part.mesh, ray.t_min, clip) else { return };
        // t is in units of the local direction: re-measure it in the world.
        let point = p.transform.transform_point3(o + d * t);
        let world_t = (point - ray.origin).dot(ray.dir);
        let entering = n_local.dot(d) < 0.0;
        let volume = (b.1 - b.0).max(DVec3::ZERO).element_product();
        if best.as_ref().is_some_and(|b| {
            if (world_t - b.hit.t).abs() > tie_tolerance(b.hit.t) {
                return b.hit.t < world_t;
            }
            // Coincident faces: the solid the ray enters, rather than one it leaves (seen
            // through a section, from inside), then the smaller one, then the scene order.
            match (entering, b.entering) {
                (true, false) => false,
                (false, true) => true,
                _ => (b.volume, b.rank) <= (volume, rank),
            }
        }) {
            return;
        }
        let mut normal = p.transform.matrix3.inverse().transpose().mul_vec3(n_local).normalize_or_zero();
        if normal.dot(ray.dir) > 0.0 {
            normal = -normal;
        }
        let face = part.mesh.tri_face.get(tri).copied().unwrap_or(0);
        *best = Some(Best { hit: Hit { path, piece, face, point, normal, t: world_t }, entering, volume, rank });
    }
}

impl DisplayScene {
    /// The body of the recipe shown that `ray` meets first, the part drawn by
    /// `part_to_world`, with the distance along the ray: a negative one (a machining) first,
    /// whatever the depth — they lie inside the positive ones, drawn as ghosts —, else a
    /// positive one.
    pub fn pick_body(&self, ray: &Ray, part_to_world: DAffine3) -> Option<(Id, f64)> {
        let recipe = self.recipe.as_ref()?;
        self.pick_in(&recipe.tools, ray, part_to_world).or_else(|| self.pick_in(&recipe.blanks, ray, part_to_world))
    }

    /// The body of `bodies` that `ray` meets first, and where.
    fn pick_in(&self, bodies: &[RecipeTool], ray: &Ray, part_to_world: DAffine3) -> Option<(Id, f64)> {
        let mut best: Option<(Id, f64)> = None;
        for tool in bodies {
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
                best = Some((tool.body, world_t));
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

/// The occurrences of the assembly a context path leads into, indexed by id, with what
/// they inherit from above: built once, it answers many occurrences (a selection of tens of
/// thousands) without searching the assembly for each.
pub struct ContextIndex<'d> {
    doc: &'d Document,
    /// Context to world.
    transform: DAffine3,
    /// The material the assemblies above the context give.
    inherited: Option<Id>,
    /// Searched by id through its own index (`Assembly::occurrence`).
    assembly: &'d Assembly,
    /// The occurrences hidden by the user are shown.
    show_hidden: bool,
}

impl<'d> ContextIndex<'d> {
    pub fn new(doc: &'d Document, context_path: &[Id]) -> Option<Self> {
        let ctx = doc.context_of(context_path).ok()?;
        let transform = doc.transform_of(context_path).ok()?;
        let mut inherited = None;
        let mut at = doc.root();
        for &i in context_path {
            let Some(occ) = doc.assembly(at).ok().and_then(|a| a.occurrence(i)) else { break };
            inherited = occ.material.or(inherited);
            at = occ.component;
        }
        let assembly = doc.assembly(ctx).ok()?;
        Some(ContextIndex { doc, transform, inherited, assembly, show_hidden: doc.settings().display.show_hidden })
    }

    /// Context to world.
    pub fn transform(&self) -> DAffine3 {
        self.transform
    }

    pub fn occurrence(&self, id: Id) -> Option<&'d Occurrence> {
        self.assembly.occurrence(id)
    }

    /// Where the parts under the occurrence `entity` are drawn. For an assembly's
    /// occurrence, every part below it, at any depth — but those the user hid below it
    /// (unless the hidden are shown): `entity` itself is there, hidden or not.
    pub fn solids_under(&self, entity: Id) -> Vec<Under> {
        let mut out = Vec::new();
        self.solids_under_into(entity, &mut out);
        out
    }

    /// [`ContextIndex::solids_under`], added to `out` (one buffer for many occurrences).
    pub fn solids_under_into(&self, entity: Id, out: &mut Vec<Under>) {
        fn walk(doc: &Document, show_hidden: bool, component: Id, t: DAffine3, material: Option<Id>, out: &mut Vec<Under>, depth: usize) {
            if depth > MAX_DEPTH {
                return;
            }
            match doc.component(component).map(|c| &c.kind) {
                Ok(ComponentKind::Part(_)) => out.push(Under { part: component, transform: t, material }),
                Ok(ComponentKind::Assembly(a)) => {
                    for o in a.occurrences().iter().filter(|o| show_hidden || !o.hidden) {
                        walk(doc, show_hidden, o.component, t * o.transform, o.material.or(material), out, depth + 1);
                    }
                }
                Err(_) => {}
            }
        }
        if let Some(o) = self.occurrence(entity) {
            walk(self.doc, self.show_hidden, o.component, self.transform * o.transform, o.material.or(self.inherited), out, 0);
        }
    }
}

/// A definition's revision: changes with its solid, its state (a stand-in, then the real
/// mesh) and its faces' materials.
fn revision(at: usize, state: State, faces: &BTreeMap<u32, Id>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (at, matches!(state, State::Pending { .. } | State::Failed), state == State::Ready, faces).hash(&mut h);
    h.finish()
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
pub(crate) fn ray_box(o: DVec3, d: DVec3, b: (DVec3, DVec3), t_min: f64) -> bool {
    ray_box_entry(o, d, &b, t_min).is_some()
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

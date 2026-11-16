//! The document: components — parts and assemblies — placed by occurrences
//! (docs/ARCHITECTURE.md §2).
//!
//! The document is immutable piece by piece: components and solids sit behind `Arc`, so a
//! clone shares everything and an operation copies only what it changes (`Arc::make_mut`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use solidsketch_i18n::t;

use crate::settings::Settings;
use solidsketch_kernel::Solid;

use crate::{Error, FastMap, Result};

/// A stable identifier, from one counter: unique across a document for components, within
/// their assembly for occurrences and within their part for bodies (a copy of a component
/// keeps them, docs/ARCHITECTURE.md §2.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub u64);

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A shared, named definition: a part or an assembly (§2.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: Id,
    pub name: String,
    /// Named by the user: kept while unused (in the model's library); otherwise dropped with
    /// its last occurrence.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub named: bool,
    pub kind: ComponentKind,
    /// Construction guides, in the component's axes (ARCHITECTURE.md §6 quinquies).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// How it is stretched: its cutters and locked axes (ARCHITECTURE.md §6 sexies).
    #[serde(default, skip_serializing_if = "Behavior::is_default")]
    pub behavior: Behavior,
}

/// How a component behaves when stretched (ARCHITECTURE.md §6 sexies, after SketchUp's
/// `Behavior`): the axes it never stretches along, and where its cutters stand. Both follow
/// the component: its copies keep them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Behavior {
    /// The axes (its own) it never stretches along.
    #[serde(default, skip_serializing_if = "StretchMask::is_empty")]
    pub no_stretch: StretchMask,
    /// The cutters along each of its axes, as increasing ratios of its box in ]0, 1[; none:
    /// one in the middle. A locked axis has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutters: Option<[Vec<f64>; 3]>,
}

impl Behavior {
    pub fn is_default(&self) -> bool {
        *self == Behavior::default()
    }

    /// A known mask, and cutters finite, inside ]0, 1[ and increasing.
    pub fn is_valid(&self) -> bool {
        let increasing = |r: &Vec<f64>| r.iter().all(|x| x.is_finite() && *x > 0.0 && *x < 1.0) && r.windows(2).all(|w| w[0] < w[1]);
        self.no_stretch.0 <= StretchMask::ALL && self.cutters.as_ref().is_none_or(|c| c.iter().all(increasing))
    }

    /// The same, the cutters of its locked axes dropped (the mask is the one truth).
    pub fn normalized(mut self) -> Behavior {
        if let Some(c) = &mut self.cutters {
            for (k, r) in c.iter_mut().enumerate() {
                if self.no_stretch.locks(k) {
                    r.clear();
                }
            }
        }
        self
    }
}

/// The axes a component never stretches along, in its own axes: SketchUp's `no_scale_mask`
/// bits — X 1, Y 2, Z 4; the planes' and the uniform scale's (8 to 64) kept, unused (there is
/// no scaling: stretching is ours).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StretchMask(pub u8);

impl StretchMask {
    /// Every bit SketchUp knows.
    pub const ALL: u8 = 0b111_1111;

    pub fn is_empty(&self) -> bool {
        self.0 & 0b111 == 0
    }

    /// Is the axis `k` (0 X, 1 Y, 2 Z) locked?
    pub fn locks(self, k: usize) -> bool {
        k < 3 && self.0 & (1 << k) != 0
    }

    /// The same, the axis `k` locked or not.
    pub fn with(self, k: usize, locked: bool) -> StretchMask {
        if locked { StretchMask(self.0 | (1 << k)) } else { StretchMask(self.0 & !(1 << k)) }
    }

    /// Does it lock a stretch along `axis` (unit, in the component's axes)? As soon as the
    /// axis has a component along a locked one.
    pub fn locks_along(self, axis: DVec3) -> bool {
        (0..3).any(|k| self.locks(k) && axis[k].abs() > 1e-9)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Part(Part),
    Assembly(Assembly),
}

impl Component {
    pub fn as_part(&self) -> Option<&Part> {
        match &self.kind {
            ComponentKind::Part(p) => Some(p),
            ComponentKind::Assembly(_) => None,
        }
    }

    pub fn as_assembly(&self) -> Option<&Assembly> {
        match &self.kind {
            ComponentKind::Assembly(a) => Some(a),
            ComponentKind::Part(_) => None,
        }
    }

    pub fn is_part(&self) -> bool {
        self.as_part().is_some()
    }

    pub fn guide(&self, id: Id) -> Option<&Guide> {
        self.guides.iter().find(|g| g.id == id)
    }
}

/// A construction guide of a component (§6 quinquies): drawn and inferred, never cut.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub id: Id,
    pub shape: GuideShape,
}

/// A guide's geometry, in its component's axes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuideShape {
    /// The infinite line through `origin` along `dir` (unit).
    Line { origin: DVec3, dir: DVec3 },
    /// The whole circle of `radius` about `center`, in the plane of `normal` (unit).
    Circle { center: DVec3, normal: DVec3, radius: f64 },
}

impl GuideShape {
    /// The shape moved by the rigid `t`, its directions kept unit.
    pub fn transformed(&self, t: &DAffine3) -> GuideShape {
        match *self {
            GuideShape::Line { origin, dir } => {
                GuideShape::Line { origin: t.transform_point3(origin), dir: t.transform_vector3(dir).normalize_or_zero() }
            }
            GuideShape::Circle { center, normal, radius } => {
                GuideShape::Circle { center: t.transform_point3(center), normal: t.transform_vector3(normal).normalize_or_zero(), radius }
            }
        }
    }

    /// Finite values, unit directions, a positive radius.
    pub fn is_valid(&self) -> bool {
        let unit = |v: DVec3| v.is_finite() && (v.length() - 1.0).abs() < 1e-6;
        match *self {
            GuideShape::Line { origin, dir } => origin.is_finite() && unit(dir),
            GuideShape::Circle { center, normal, radius } => center.is_finite() && unit(normal) && radius.is_finite() && radius > 0.0,
        }
    }
}

/// A part: the union of its positive bodies minus the union of its negative ones, every body
/// placed in the part's own axes (docs/ARCHITECTURE.md §2.3, §2.4, §2.11). A body is an
/// occurrence of one of the part's body definitions: copies share theirs, a change to its shape
/// shows in every one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// The shapes of the bodies, each used by one body at least.
    pub defs: Vec<BodyDef>,
    pub bodies: Vec<Body>,
    /// The part as last computed from its bodies; absent when it is its single positive body,
    /// unplaced (no kernel). A cache saved with the file: the bodies are the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) shape: Option<Arc<Solid>>,
    /// The bodies whose boolean failed when the shape was computed: skipped, the others
    /// computed (§2.4). Part of the same cache.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) failed: BTreeSet<Id>,
    /// The material of single faces of the shape, by their index in its record (§7 quater):
    /// over the occurrence's. A boolean renumbers faces: an index may then name another
    /// face, or none (ignored).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub faces: BTreeMap<u32, Id>,
}

impl Part {
    /// A part of the single positive body `solid`, unplaced: its definition `def`, its body
    /// `id`.
    pub fn new(def: Id, id: Id, solid: Arc<Solid>) -> Self {
        Part::of_body(BodyDef::new(def, solid), Body::new(id, def, Sign::Positive, DAffine3::IDENTITY))
    }

    /// A part of the single positive body `body`, of `def`; computed when it is placed.
    pub(crate) fn of_body(def: BodyDef, body: Body) -> Self {
        Part { defs: vec![def], bodies: vec![body], shape: None, failed: BTreeSet::new(), faces: BTreeMap::new() }
    }

    /// The part as it is drawn and used: its bodies combined.
    pub fn shape(&self) -> &Arc<Solid> {
        match &self.shape {
            Some(s) => s,
            // The single positive body is the shape (`is_plain`).
            None => self.solid_of(&self.bodies[0]),
        }
    }

    pub fn def(&self, id: Id) -> Option<&BodyDef> {
        self.defs.iter().find(|d| d.id == id)
    }

    /// The definition of `b`, a body of this part (a part read or changed keeps every body's).
    pub fn def_of(&self, b: &Body) -> &BodyDef {
        self.def(b.def).expect("a body's definition is in its part")
    }

    /// The solid of `b`, in its own axes.
    pub fn solid_of(&self, b: &Body) -> &Arc<Solid> {
        &self.def_of(b).solid
    }

    /// The profile `b`'s solid is built from, if any.
    pub fn profile_of(&self, b: &Body) -> Option<&crate::BodyProfile> {
        self.def_of(b).profile.as_ref()
    }

    /// How many bodies use the definition `def`, those of groups included (a group's children
    /// counted once, however many copies of the group there are).
    pub fn copies_of(&self, def: Id) -> usize {
        self.all_bodies().filter(|b| b.def == def).count()
    }

    /// Every body of the part: its own, then those of the groups they lead to, at any depth
    /// (each group's once, however many copies of it there are).
    pub fn all_bodies(&self) -> impl Iterator<Item = &Body> {
        let used = used_defs(&self.defs, &self.bodies);
        self.bodies.iter().chain(self.defs.iter().filter(move |d| used.contains(&d.id)).flat_map(|d| d.children()))
    }

    /// The bodies of the group `def` uses (none for a leaf or an unknown definition).
    pub fn children_of(&self, def: Id) -> &[Body] {
        self.def(def).map_or(&[], |d| d.children())
    }

    /// The group definitions the bodies `group` lead through (the part's own body first, each
    /// a group's, the next one among its children), and the last one's placement in the part's
    /// axes.
    pub fn group_chain(&self, group: &[Id]) -> crate::Result<(Vec<Id>, DAffine3)> {
        let mut chain = Vec::with_capacity(group.len());
        let mut placed = DAffine3::IDENTITY;
        let mut bodies: &[Body] = &self.bodies;
        for id in group {
            let b = bodies.iter().find(|b| b.id == *id).ok_or(Error::NotFound(*id))?;
            let d = self.def(b.def).filter(|d| d.is_group()).ok_or(Error::NotFound(*id))?;
            placed *= b.placement;
            chain.push(d.id);
            bodies = d.children();
        }
        Ok((chain, placed))
    }

    /// The group `def` (one of this part's) seen as a part: its children as the bodies, its
    /// result as the shape, every definition of the part.
    pub(crate) fn group_as_part(&self, def: &BodyDef) -> Part {
        let g = def.group.clone().unwrap_or_default();
        let mut p = Part { defs: self.defs.clone(), bodies: g.children, shape: None, failed: g.failed, faces: BTreeMap::new() };
        if !p.bodies.is_empty() && !p.is_plain() {
            p.shape = Some(def.solid.clone());
        }
        p
    }

    /// Drops the definitions no body uses, through groups.
    pub(crate) fn drop_unused_defs(&mut self) {
        let used = used_defs(&self.defs, &self.bodies);
        self.defs.retain(|d| used.contains(&d.id));
    }

    /// Is the part its single positive body, unplaced: its own shape, with no kernel?
    pub fn is_plain(&self) -> bool {
        matches!(&self.bodies[..], [b] if b.sign == Sign::Positive && is_identity(&b.placement))
    }

    pub fn body(&self, id: Id) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id)
    }

    pub fn positives(&self) -> impl Iterator<Item = &Body> {
        self.bodies.iter().filter(|b| b.sign == Sign::Positive)
    }

    pub fn negatives(&self) -> impl Iterator<Item = &Body> {
        self.bodies.iter().filter(|b| b.sign == Sign::Negative)
    }

    /// The first positive body with a profile: the one the Profile tool edits for now.
    pub fn profiled(&self) -> Option<&Body> {
        self.positives().find(|b| self.profile_of(b).is_some())
    }

    /// Did the body `id` fail, the last time the shape (or its group) was computed?
    pub fn is_failed(&self, id: Id) -> bool {
        self.failed.contains(&id) || self.defs.iter().any(|d| d.group.as_ref().is_some_and(|g| g.failed.contains(&id)))
    }

    /// How many separate lumps of material the shape holds: a part is one piece of wood, more
    /// is an error the user has to correct (§2.11).
    pub fn pieces(&self) -> usize {
        self.shape().pieces()
    }
}

/// The definitions `bodies` use, through groups (`defs` the part's).
pub(crate) fn used_defs(defs: &[BodyDef], bodies: &[Body]) -> BTreeSet<Id> {
    let mut used = BTreeSet::new();
    let mut stack: Vec<Id> = bodies.iter().map(|b| b.def).collect();
    while let Some(id) = stack.pop() {
        if used.insert(id)
            && let Some(d) = defs.iter().find(|d| d.id == id)
        {
            stack.extend(d.children().iter().map(|b| b.def));
        }
    }
    used
}

/// `t` within a hair of the identity.
pub(crate) fn is_identity(t: &DAffine3) -> bool {
    t.abs_diff_eq(DAffine3::IDENTITY, 1e-12)
}

/// Whether a body adds material to its part or takes it away (§2.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sign {
    Positive,
    Negative,
}

/// The shape of bodies of a part (§2.11): `solid`, in its own axes — a body's. A leaf holds its
/// shape (a free one, or built from its profile); a group (§2.11 « Body groups ») holds bodies,
/// `solid` then only their result cached: `(∪ positive children) − (∪ negative children)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyDef {
    pub id: Id,
    /// Its name, given by the user (§2.11 « Body names »): every body of it shows it. None:
    /// the interface names it by its nature and rank (« Solide 3 »). Kept when its shape
    /// changes (a stretch, a boolean, a new profile) and by its copies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub solid: Arc<Solid>,
    /// The profile `solid` is built from (§6 nonies), in its axes: the solid is then its
    /// cache. None for a free shape (imported, a boolean's result, stretched askew) and a group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<crate::BodyProfile>,
    #[serde(default)]
    pub source: BodySource,
    /// The bodies a group is made of, placed in its axes; none for a leaf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<BodyGroup>,
}

impl BodyDef {
    /// A free shape (no profile), local.
    pub fn new(id: Id, solid: Arc<Solid>) -> Self {
        BodyDef { id, name: None, solid, profile: None, source: BodySource::Local, group: None }
    }

    /// A group of `children`, `solid` their result, `failed` those whose boolean failed.
    pub fn group(id: Id, solid: Arc<Solid>, children: Vec<Body>, failed: BTreeSet<Id>) -> Self {
        BodyDef { group: Some(BodyGroup { children, failed }), ..BodyDef::new(id, solid) }
    }

    /// The bodies of a group, none for a leaf.
    pub fn children(&self) -> &[Body] {
        self.group.as_ref().map_or(&[], |g| &g.children)
    }

    pub fn is_group(&self) -> bool {
        self.group.is_some()
    }
}

/// The content of a group of bodies (§2.11 « Body groups »): its bodies, each an occurrence of a
/// definition of the same part, placed in the group's axes. Its negatives cut its positives
/// only. Body ids are unique in the whole part, groups included.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyGroup {
    pub children: Vec<Body>,
    /// The children whose boolean failed when the group was computed (skipped).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub failed: BTreeSet<Id>,
}

/// A solid of a part: an occurrence of the definition `def`, placed by `placement` in the part's
/// axes, adding material or taking it away (`sign`; a negative body is a machining, §2.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub id: Id,
    pub def: Id,
    pub sign: Sign,
    pub placement: DAffine3,
}

impl Body {
    pub fn new(id: Id, def: Id, sign: Sign, placement: DAffine3) -> Self {
        Body { id, def, sign, placement }
    }
}

/// Where a body comes from. Only local ones for now; linked ones (machinings of fixed
/// hardware) will come (§2.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodySource {
    #[default]
    Local,
}

/// An assembly: occurrences of other components.
///
/// Found by id through an index (id → position), dropped by any change to the list and built
/// again only once searched a few times since (a copy made to change one occurrence is not
/// indexed for a search or two; a clone keeps the index): a search per occurrence of a large
/// assembly stays linear in all, not quadratic. An occurrence's id never changes.
#[derive(Default, Serialize, Deserialize)]
pub struct Assembly {
    occurrences: Vec<Occurrence>,
    #[serde(skip)]
    index: OnceLock<FastMap<Id, u32>>,
    /// Searches by scanning since the index was dropped.
    #[serde(skip)]
    scans: AtomicU32,
}

/// Below this many occurrences, a search scans them (cheaper than hashing).
const INDEXED_FROM: usize = 16;
/// Searches by scanning before the index is built.
const SCANS_BEFORE_INDEX: u32 = 4;

impl Assembly {
    pub fn new(occurrences: Vec<Occurrence>) -> Self {
        Assembly { occurrences, index: OnceLock::new(), scans: AtomicU32::new(0) }
    }

    pub fn occurrences(&self) -> &[Occurrence] {
        &self.occurrences
    }

    /// The list, to change: the index is dropped.
    pub(crate) fn occurrences_mut(&mut self) -> &mut Vec<Occurrence> {
        self.index = OnceLock::new();
        self.scans = AtomicU32::new(0);
        &mut self.occurrences
    }

    pub fn occurrence(&self, id: Id) -> Option<&Occurrence> {
        self.position(id).map(|i| &self.occurrences[i])
    }

    /// The occurrence at `i` (its [`Assembly::position`]), to change but for its id: the
    /// index stays.
    pub(crate) fn occurrence_at_mut(&mut self, i: usize) -> &mut Occurrence {
        &mut self.occurrences[i]
    }

    pub(crate) fn occurrence_mut(&mut self, id: Id) -> Option<&mut Occurrence> {
        let i = self.position(id)?;
        Some(&mut self.occurrences[i])
    }

    pub(crate) fn position(&self, id: Id) -> Option<usize> {
        let scan = || self.occurrences.iter().position(|o| o.id == id);
        if self.occurrences.len() < INDEXED_FROM {
            return scan();
        }
        if self.index.get().is_none() && self.scans.fetch_add(1, Ordering::Relaxed) < SCANS_BEFORE_INDEX {
            return scan();
        }
        let index = self.index.get_or_init(|| self.occurrences.iter().enumerate().map(|(i, o)| (o.id, i as u32)).collect());
        index.get(&id).map(|i| *i as usize).filter(|i| self.occurrences[*i].id == id)
    }
}

impl Clone for Assembly {
    fn clone(&self) -> Self {
        Assembly {
            occurrences: self.occurrences.clone(),
            index: self.index.clone(),
            scans: AtomicU32::new(self.scans.load(Ordering::Relaxed)),
        }
    }
}

impl PartialEq for Assembly {
    fn eq(&self, other: &Self) -> bool {
        self.occurrences == other.occurrences
    }
}

impl std::fmt::Debug for Assembly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Assembly").field("occurrences", &self.occurrences).finish()
    }
}

/// A component placed: one copy of a part or an assembly (§2.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Occurrence {
    pub id: Id,
    /// The copy's own name ("Gauche", "Droite"); may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub component: Id,
    /// From the component's axes to its assembly's: a move, a rotation, maybe a mirror; never
    /// a scale.
    pub transform: DAffine3,
    /// The copy's material (its faces without one of their own take it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Id>,
    /// Hidden by the user (ARCHITECTURE.md §6): neither drawn, picked, inferred on nor framed, nor is
    /// anything under it — but while it is being edited, or with the hidden shown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// A material: a name and a colour (§7 quater). Wood species, panels… come later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// sRGB.
    pub color: [u8; 3],
}

/// An occurrence in the scene: the occurrences of assemblies leading to it from the root,
/// then the occurrence itself. Two occurrences of one assembly make two paths to the same part.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityPath {
    /// Occurrence ids from the root, outermost first; empty for an occurrence of the root.
    pub instances: Vec<Id>,
    pub entity: Id,
    /// In the part `entity` places, the group of bodies open (§2.11 « Body groups »): the
    /// bodies leading to it, the part's own first, each a group's. Empty: the part itself. Only
    /// the operations on bodies, and [`Document::resolve_container`], read it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group: Vec<Id>,
}

impl EntityPath {
    pub fn new(instances: Vec<Id>, entity: Id) -> Self {
        EntityPath { instances, entity, group: Vec::new() }
    }

    pub fn root(entity: Id) -> Self {
        EntityPath::new(Vec::new(), entity)
    }

    /// This path into the group of bodies `group` of its part.
    pub fn in_group(mut self, group: Vec<Id>) -> Self {
        self.group = group;
        self
    }

    /// This path to the part itself, out of any group.
    pub fn part_path(&self) -> EntityPath {
        EntityPath::new(self.instances.clone(), self.entity)
    }

    /// The path `chain` (occurrences from the root, the last one included) names.
    pub fn from_chain(chain: &[Id]) -> Option<Self> {
        let (last, rest) = chain.split_last()?;
        Some(EntityPath::new(rest.to_vec(), *last))
    }
}

/// [`Document::placed_changes`]'s walk: what it compares with, and what it found.
struct ChangesWalk<'a> {
    before: &'a Document,
    touched: &'a [Id],
    /// By component: does it differ from `before`'s, itself or below?
    dirty: FastMap<Id, bool>,
    out: Vec<(PlacedPart, Id, DAffine3)>,
}

/// A part placed in the scene, by [`Document::placed_parts`].
#[derive(Clone, Debug)]
pub struct PlacedPart {
    pub path: EntityPath,
    pub part: Id,
    /// Part to world.
    pub transform: DAffine3,
    pub shape: Arc<Solid>,
    /// The material its faces take: the occurrence's own, else the nearest assembly
    /// occurrence's above it that has one (an assembly painted paints its parts).
    pub material: Option<Id>,
    /// The deepest hidden occurrence of its path, by its index in the path (the occurrence
    /// itself last): none when nothing along it is hidden.
    pub hidden_at: Option<u32>,
}

/// A guide placed in the scene, by [`Document::placed_guides`].
#[derive(Clone, Debug)]
pub struct PlacedGuide {
    /// The occurrences from the root to the component holding it (empty: the root's).
    pub path: Vec<Id>,
    pub component: Id,
    /// The component's axes to world.
    pub transform: DAffine3,
    pub guide: Guide,
    /// The deepest hidden occurrence of `path`, by its index there: none when nothing along
    /// it is hidden.
    pub hidden_at: Option<u32>,
}

/// The whole model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub(crate) components: BTreeMap<Id, Arc<Component>>,
    /// The root assembly: the model itself, never placed nor counted.
    pub(crate) root: Id,
    pub(crate) next_id: u64,
    /// Display style and units, saved with the model.
    #[serde(default)]
    pub(crate) settings: Settings,
    /// The materials parts and occurrences are painted with, by id (ids from `next_id`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) materials: BTreeMap<Id, Material>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// An empty model.
    pub fn new() -> Self {
        Self::with_settings(Settings::default())
    }

    /// An empty model with these settings (the app's defaults for new models).
    pub fn with_settings(settings: Settings) -> Self {
        let root = Id(1);
        let kind = ComponentKind::Assembly(Assembly::default());
        let c =
            Component { id: root, name: t!("default_name.model"), named: true, kind, guides: Vec::new(), behavior: Behavior::default() };
        Document { components: BTreeMap::from([(root, Arc::new(c))]), root, next_id: 2, settings, materials: BTreeMap::new() }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Every material, in id order.
    pub fn materials(&self) -> impl Iterator<Item = (Id, &Material)> {
        self.materials.iter().map(|(id, m)| (*id, m))
    }

    pub fn material(&self, id: Id) -> Result<&Material> {
        self.materials.get(&id).ok_or(Error::NotFound(id))
    }

    pub fn root(&self) -> Id {
        self.root
    }

    /// The id the next thing made will take. It goes back with an undo: the ids of the steps
    /// undone are handed out again.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    pub fn root_assembly(&self) -> &Assembly {
        // The root is always an assembly: `new` makes it, files are checked, no operation
        // changes it.
        self.assembly(self.root).expect("the root is an assembly")
    }

    pub fn component(&self, id: Id) -> Result<&Component> {
        self.components.get(&id).map(|c| &**c).ok_or(Error::NotFound(id))
    }

    pub fn assembly(&self, id: Id) -> Result<&Assembly> {
        self.component(id)?.as_assembly().ok_or(Error::NotAnAssembly(id))
    }

    pub fn part(&self, id: Id) -> Result<&Part> {
        self.component(id)?.as_part().ok_or(Error::NotAPart(id))
    }

    /// Every component but the root, in id order.
    pub fn components(&self) -> impl Iterator<Item = &Component> {
        self.components.values().map(|c| &**c).filter(|c| c.id != self.root)
    }

    /// How many occurrences of `component` there are, in all assemblies (not multiplied by
    /// the occurrences of those assemblies).
    pub fn occurrence_count(&self, component: Id) -> usize {
        self.components.values().filter_map(|c| c.as_assembly()).flat_map(|a| &a.occurrences).filter(|o| o.component == component).count()
    }

    /// The assembly an occurrence path leads into (the root for an empty path).
    pub fn context_of(&self, instances: &[Id]) -> Result<Id> {
        let mut at = self.root;
        for &i in instances {
            let o = self.assembly(at)?.occurrence(i).ok_or(Error::NotFound(i))?;
            self.assembly(o.component)?;
            at = o.component;
        }
        Ok(at)
    }

    /// From the axes of the assembly `instances` leads into to the root's.
    pub fn transform_of(&self, instances: &[Id]) -> Result<DAffine3> {
        let mut at = self.root;
        let mut t = DAffine3::IDENTITY;
        for &i in instances {
            let o = self.assembly(at)?.occurrence(i).ok_or(Error::NotFound(i))?;
            t *= o.transform;
            at = o.component;
        }
        Ok(t)
    }

    /// The occurrence a path names.
    pub fn resolve(&self, path: &EntityPath) -> Result<&Occurrence> {
        let ctx = self.context_of(&path.instances)?;
        self.assembly(ctx)?.occurrence(path.entity).ok_or(Error::NotFound(path.entity))
    }

    /// The part a path names, and its part-to-world transform.
    pub fn resolve_part(&self, path: &EntityPath) -> Result<(Id, &Part, DAffine3)> {
        let o = self.resolve(path)?;
        let part = self.part(o.component)?;
        Ok((o.component, part, self.transform_of(&path.instances)? * o.transform))
    }

    /// What `path` leads into: the part it places — its id, the part, part to world — or,
    /// when it names a group of bodies (`path.group`), that group seen as a part (§2.11 « Body
    /// groups »): its children as the bodies, its result as the shape, every definition of
    /// the part, and group to world.
    pub fn resolve_container(&self, path: &EntityPath) -> Result<(Id, std::borrow::Cow<'_, Part>, DAffine3)> {
        let (id, p, t) = self.resolve_part(path)?;
        if path.group.is_empty() {
            return Ok((id, std::borrow::Cow::Borrowed(p), t));
        }
        let (chain, placed) = p.group_chain(&path.group)?;
        let def = p.def(*chain.last().expect("a group path is not empty")).ok_or(Error::NotFound(path.entity))?;
        Ok((id, std::borrow::Cow::Owned(p.group_as_part(def)), t * placed))
    }

    /// Is the occurrence `id` of the assembly `context` an assembly's (can it be entered)?
    pub fn is_assembly_occurrence(&self, context: Id, id: Id) -> bool {
        self.assembly(context).ok().and_then(|a| a.occurrence(id)).is_some_and(|o| self.assembly(o.component).is_ok())
    }

    /// Every part of the scene, placed: a part of an assembly with three occurrences appears
    /// three times.
    pub fn placed_parts(&self) -> Vec<PlacedPart> {
        let mut out = Vec::new();
        self.walk(self.root, &mut Vec::new(), DAffine3::IDENTITY, (None, None), &mut out, 0);
        out
    }

    /// The parts `self` places otherwise than `before` does at the same path — another part,
    /// the same one changed (its component not `before`'s, or one of `touched`), or elsewhere —
    /// as `self` places them, each with what `before` placed there (part and part-to-world).
    /// Paths `before` lacks are left out. What both share (the same components, unchanged down
    /// the tree, placed alike) is skipped whole: a change to a few occurrences of a large model
    /// costs a pass over the assemblies holding them, not a walk of every placement.
    pub fn placed_changes(&self, before: &Document, touched: &[Id]) -> Vec<(PlacedPart, Id, DAffine3)> {
        let mut walk = ChangesWalk { before, touched, dirty: FastMap::default(), out: Vec::new() };
        let t = DAffine3::IDENTITY;
        self.walk_changes(&mut walk, (self.root, before.root), &mut Vec::new(), (t, t), (None, None), 0);
        walk.out
    }

    /// Does `id` differ from `before`'s, or hold (at any depth) a component that does?
    fn dirty(&self, walk: &mut ChangesWalk<'_>, id: Id, depth: usize) -> bool {
        if let Some(d) = walk.dirty.get(&id) {
            return *d;
        }
        let d = depth > MAX_DEPTH
            || walk.touched.contains(&id)
            || match (self.components.get(&id), walk.before.components.get(&id)) {
                (Some(c), Some(cb)) if Arc::ptr_eq(c, cb) => match &c.kind {
                    ComponentKind::Part(_) => false,
                    ComponentKind::Assembly(a) => a.occurrences.iter().any(|o| self.dirty(walk, o.component, depth + 1)),
                },
                _ => true,
            };
        walk.dirty.insert(id, d);
        d
    }

    fn walk_changes(
        &self,
        walk: &mut ChangesWalk<'_>,
        (assembly, was): (Id, Id),
        path: &mut Vec<Id>,
        (t, tb): (DAffine3, DAffine3),
        (material, hidden_at): (Option<Id>, Option<u32>),
        depth: usize,
    ) {
        let before = walk.before;
        let (Ok(a), Ok(b)) = (self.assembly(assembly), before.assembly(was)) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        // Occurrences keep their order: matched by place, else (some added or removed) by id.
        let mut by_id: Option<FastMap<Id, &Occurrence>> = None;
        // Placed alike above: an occurrence left as it was is placed alike (no product to
        // work out). Neighbours often share their component: the last one found unchanged.
        let same_above = t == tb;
        let mut unchanged: Option<Id> = None;
        for (i, o) in a.occurrences.iter().enumerate() {
            let ob = match b.occurrences.get(i) {
                Some(ob) if ob.id == o.id => ob,
                _ => match by_id.get_or_insert_with(|| b.occurrences.iter().map(|o| (o.id, o)).collect()).get(&o.id) {
                    Some(ob) => *ob,
                    None => continue,
                },
            };
            let mut clean = |c: Id| {
                if unchanged == Some(c) {
                    return true;
                }
                let clean = !self.dirty(walk, c, depth + 1);
                if clean {
                    unchanged = Some(c);
                }
                clean
            };
            if o.component == ob.component && same_above && o.transform == ob.transform && clean(o.component) {
                continue;
            }
            let (t, tb) = (t * o.transform, tb * ob.transform);
            if o.component == ob.component && t.abs_diff_eq(tb, 1e-9) && clean(o.component) {
                continue;
            }
            let (Ok(c), Ok(cb)) = (self.component(o.component), before.component(ob.component)) else { continue };
            let hidden_at = if o.hidden { Some(depth as u32) } else { hidden_at };
            match (&c.kind, &cb.kind) {
                (ComponentKind::Part(p), ComponentKind::Part(_)) => walk.out.push((
                    PlacedPart {
                        path: EntityPath::new(path.clone(), o.id),
                        part: c.id,
                        transform: t,
                        shape: p.shape().clone(),
                        material: o.material.or(material),
                        hidden_at,
                    },
                    cb.id,
                    tb,
                )),
                (ComponentKind::Assembly(_), ComponentKind::Assembly(_)) => {
                    path.push(o.id);
                    self.walk_changes(walk, (c.id, cb.id), path, (t, tb), (o.material.or(material), hidden_at), depth + 1);
                    path.pop();
                }
                _ => {}
            }
        }
    }

    /// `above`: the material and the deepest hidden occurrence ([`PlacedPart::hidden_at`])
    /// coming from the occurrences above.
    fn walk(
        &self,
        assembly: Id,
        path: &mut Vec<Id>,
        t: DAffine3,
        above: (Option<Id>, Option<u32>),
        out: &mut Vec<PlacedPart>,
        depth: usize,
    ) {
        let Ok(a) = self.assembly(assembly) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        let (material, hidden_at) = above;
        for o in &a.occurrences {
            let Ok(c) = self.component(o.component) else { continue };
            let t = t * o.transform;
            let hidden_at = if o.hidden { Some(depth as u32) } else { hidden_at };
            match &c.kind {
                ComponentKind::Part(p) => out.push(PlacedPart {
                    path: EntityPath::new(path.clone(), o.id),
                    part: c.id,
                    transform: t,
                    shape: p.shape().clone(),
                    material: o.material.or(material),
                    hidden_at,
                }),
                ComponentKind::Assembly(_) => {
                    path.push(o.id);
                    self.walk(c.id, path, t, (o.material.or(material), hidden_at), out, depth + 1);
                    path.pop();
                }
            }
        }
    }

    /// Every component's guides as the scene shows them: the root's, then each occurrence's
    /// down the tree, a part's in each of its occurrences.
    pub fn placed_guides(&self) -> Vec<PlacedGuide> {
        // Most models have none: no walk of every occurrence for them.
        if self.components.values().all(|c| c.guides.is_empty()) {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.walk_guides(self.root, &mut Vec::new(), DAffine3::IDENTITY, None, &mut out, 0);
        out
    }

    fn walk_guides(
        &self,
        component: Id,
        path: &mut Vec<Id>,
        t: DAffine3,
        hidden_at: Option<u32>,
        out: &mut Vec<PlacedGuide>,
        depth: usize,
    ) {
        let Ok(c) = self.component(component) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        for g in &c.guides {
            out.push(PlacedGuide { path: path.clone(), component, transform: t, guide: g.clone(), hidden_at });
        }
        if let ComponentKind::Assembly(a) = &c.kind {
            for o in &a.occurrences {
                path.push(o.id);
                let hidden_at = if o.hidden { Some(depth as u32) } else { hidden_at };
                self.walk_guides(o.component, path, t * o.transform, hidden_at, out, depth + 1);
                path.pop();
            }
        }
    }

    /// Does `component` contain an occurrence of `target`, at any depth (or is it `target`)?
    pub fn contains_component(&self, component: Id, target: Id) -> bool {
        fn go(doc: &Document, c: Id, target: Id, depth: usize) -> bool {
            if c == target {
                return true;
            }
            if depth > MAX_DEPTH {
                return true; // treat a runaway as a cycle
            }
            doc.assembly(c).is_ok_and(|a| a.occurrences.iter().any(|o| go(doc, o.component, target, depth + 1)))
        }
        go(self, component, target, 0)
    }

    // --- mutation helpers, for operations ---------------------------------------------------

    pub(crate) fn fresh_id(&mut self) -> Id {
        let id = Id(self.next_id);
        self.next_id += 1;
        id
    }

    /// A part of the single positive body `solid`, unplaced, its definition and body named
    /// with fresh ids.
    pub(crate) fn plain_part(&mut self, solid: Arc<Solid>) -> Part {
        let def = self.fresh_id();
        Part::new(def, self.fresh_id(), solid)
    }

    pub(crate) fn component_mut(&mut self, id: Id) -> Result<&mut Component> {
        self.components.get_mut(&id).map(Arc::make_mut).ok_or(Error::NotFound(id))
    }

    pub(crate) fn assembly_mut(&mut self, id: Id) -> Result<&mut Assembly> {
        match &mut self.component_mut(id)?.kind {
            ComponentKind::Assembly(a) => Ok(a),
            ComponentKind::Part(_) => Err(Error::NotAnAssembly(id)),
        }
    }

    pub(crate) fn part_mut(&mut self, id: Id) -> Result<&mut Part> {
        match &mut self.component_mut(id)?.kind {
            ComponentKind::Part(p) => Ok(p),
            ComponentKind::Assembly(_) => Err(Error::NotAPart(id)),
        }
    }

    /// Adds a component, under a name no other component has (`base`, else `base#2`, …).
    pub(crate) fn insert_component(&mut self, base: &str, named: bool, kind: ComponentKind) -> Id {
        let id = self.fresh_id();
        let name = self.unique_name(base);
        self.components.insert(id, Arc::new(Component { id, name, named, kind, guides: Vec::new(), behavior: Behavior::default() }));
        id
    }

    /// Adds a copy of the component `src` (its recipe or its occurrences, its guides — ids
    /// kept — and its behavior), under a name of its own.
    pub(crate) fn copy_component(&mut self, src: Id) -> Result<Id> {
        let src = self.component(src)?.clone();
        let copy = self.insert_component(&src.name, src.named, src.kind);
        let c = self.component_mut(copy)?;
        (c.guides, c.behavior) = (src.guides, src.behavior);
        Ok(copy)
    }

    /// A name not used by another component: `base`, else `base#2`, `base#3`, …
    pub(crate) fn unique_name(&self, base: &str) -> String {
        let taken = |n: &str| self.components.values().any(|c| c.name == n);
        if !taken(base) {
            return base.to_owned();
        }
        (2..).map(|i| format!("{base}#{i}")).find(|n| !taken(n)).unwrap_or_default()
    }

    /// Drops the components no occurrence uses that the user did not name (§2.1), and so on
    /// down: an unnamed assembly dropped may leave its parts unused.
    pub(crate) fn purge(&mut self) {
        loop {
            let mut used = crate::FastSet::default();
            for c in self.components.values() {
                if let ComponentKind::Assembly(a) = &c.kind {
                    used.extend(a.occurrences.iter().map(|o| o.component));
                }
            }
            let unused: Vec<Id> =
                self.components.values().filter(|c| c.id != self.root && !c.named && !used.contains(&c.id)).map(|c| c.id).collect();
            if unused.is_empty() {
                return;
            }
            for id in unused {
                self.components.remove(&id);
            }
        }
    }

    /// Copies the components and materials of `fragment` (another document) into this one,
    /// every id drawn afresh, names made unique; a material equal to one already here (same
    /// name and colour) is that one. Returns the fragment root's occurrences, renumbered too,
    /// for the caller to place.
    ///
    /// With `reuse` (the fragment was copied from this document, whose ids still name the same
    /// things, §6 decies), the components and materials still here are used as they are; only
    /// the others are copied.
    pub(crate) fn graft(&mut self, fragment: &Document, reuse: bool) -> Result<Vec<Occurrence>> {
        // Decided before anything is added: a fresh id may be one of the fragment's.
        let here: crate::FastSet<Id> = if reuse {
            fragment.materials.keys().filter(|id| self.materials.contains_key(id)).copied().collect()
        } else {
            crate::FastSet::default()
        };
        let mut materials: HashMap<Id, Id> = HashMap::new();
        for (id, m) in &fragment.materials {
            let same = if here.contains(id) { Some(*id) } else { self.materials.iter().find(|(_, here)| *here == m).map(|(k, _)| *k) };
            let to = match same {
                Some(k) => k,
                None => {
                    let k = self.fresh_id();
                    self.materials.insert(k, m.clone());
                    k
                }
            };
            materials.insert(*id, to);
        }
        // Decided before anything is added: a fresh id may be one of the fragment's.
        let kept: crate::FastSet<Id> = if reuse {
            fragment
                .components
                .keys()
                .filter(|id| **id != fragment.root && **id != self.root && self.components.contains_key(id))
                .copied()
                .collect()
        } else {
            crate::FastSet::default()
        };
        let components: HashMap<Id, Id> = fragment
            .components
            .keys()
            .filter(|id| **id != fragment.root)
            .map(|id| (*id, if kept.contains(id) { *id } else { self.fresh_id() }))
            .collect();
        let occurrences = |doc: &mut Document, list: &[Occurrence]| -> Result<Vec<Occurrence>> {
            list.iter()
                .map(|o| {
                    let component = *components.get(&o.component).ok_or(Error::NotFound(o.component))?;
                    let material = o.material.and_then(|m| materials.get(&m).copied());
                    Ok(Occurrence {
                        id: doc.fresh_id(),
                        name: o.name.clone(),
                        component,
                        transform: o.transform,
                        material,
                        hidden: o.hidden,
                    })
                })
                .collect()
        };
        // In the fragment's order: ids and names come out the same each time.
        for (old, c) in fragment.components.iter().filter(|(id, _)| **id != fragment.root && !kept.contains(*id)) {
            let new = &components[old];
            let kind = match &c.kind {
                ComponentKind::Assembly(a) => ComponentKind::Assembly(Assembly::new(occurrences(self, &a.occurrences)?)),
                ComponentKind::Part(p) => {
                    let mut p = p.clone();
                    let mut bodies: HashMap<Id, Id> = HashMap::new();
                    for b in &mut p.bodies {
                        let id = self.fresh_id();
                        bodies.insert(b.id, id);
                        b.id = id;
                    }
                    p.failed = p.failed.iter().filter_map(|id| bodies.get(id).copied()).collect();
                    for g in p.defs.iter_mut().filter_map(|d| d.group.as_mut()) {
                        let mut own: HashMap<Id, Id> = HashMap::new();
                        for b in &mut g.children {
                            let id = self.fresh_id();
                            own.insert(b.id, id);
                            b.id = id;
                        }
                        g.failed = g.failed.iter().filter_map(|id| own.get(id).copied()).collect();
                    }
                    p.faces = p.faces.iter().filter_map(|(f, m)| materials.get(m).map(|m| (*f, *m))).collect();
                    ComponentKind::Part(p)
                }
            };
            let guides = c.guides.iter().map(|g| Guide { id: self.fresh_id(), shape: g.shape }).collect();
            let name = self.unique_name(&c.name);
            self.components
                .insert(*new, Arc::new(Component { id: *new, name, named: c.named, kind, guides, behavior: c.behavior.clone() }));
        }
        occurrences(self, &fragment.root_assembly().occurrences)
    }
}

/// The deepest nesting walked; deeper is treated as a cycle.
pub const MAX_DEPTH: usize = 64;

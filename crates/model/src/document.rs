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

use crate::{Error, Result};

/// A stable identifier, from one counter: unique across a document for components, within
/// their assembly for occurrences and within their part for machinings (a copy of a component
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

/// A part: its blank minus its machinings, in the part's own axes (§2.3, §2.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub blank: Arc<Solid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub machinings: Vec<Machining>,
    /// The blank minus the machinings, as last computed; absent without machinings (the
    /// blank is the shape). A cache saved with the file: the recipe is the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) shape: Option<Arc<Solid>>,
    /// The machinings that failed to cut when the shape was computed: skipped, the others
    /// cut (§2.4). Part of the same cache.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) failed: BTreeSet<Id>,
    /// The material of single faces of the shape, by their index in its record (§7 quater):
    /// over the occurrence's. A boolean renumbers faces: an index may then name another
    /// face, or none (ignored).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub faces: BTreeMap<u32, Id>,
}

impl Part {
    pub fn new(blank: Arc<Solid>) -> Self {
        Part { blank, machinings: Vec::new(), shape: None, failed: BTreeSet::new(), faces: BTreeMap::new() }
    }

    /// The part as it is drawn and used: the blank with its machinings cut.
    pub fn shape(&self) -> &Arc<Solid> {
        self.shape.as_ref().unwrap_or(&self.blank)
    }

    pub fn machining(&self, id: Id) -> Option<&Machining> {
        self.machinings.iter().find(|m| m.id == id)
    }

    /// Did the machining `id` fail to cut, the last time the shape was computed?
    pub fn is_failed(&self, id: Id) -> bool {
        self.failed.contains(&id)
    }
}

/// Material taken out of a part: `tool`, placed by `placement` in the part's axes (§2.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Machining {
    pub id: Id,
    pub tool: Arc<Solid>,
    pub placement: DAffine3,
    #[serde(default)]
    pub source: MachiningSource,
}

/// Where a machining comes from. Only local ones for now; linked ones (from fixed hardware)
/// will come (§2.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachiningSource {
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
    index: OnceLock<HashMap<Id, u32>>,
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
}

impl EntityPath {
    pub fn new(instances: Vec<Id>, entity: Id) -> Self {
        EntityPath { instances, entity }
    }

    pub fn root(entity: Id) -> Self {
        EntityPath { instances: Vec::new(), entity }
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
    dirty: HashMap<Id, bool>,
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

    /// Is the occurrence `id` of the assembly `context` an assembly's (can it be entered)?
    pub fn is_assembly_occurrence(&self, context: Id, id: Id) -> bool {
        self.assembly(context).ok().and_then(|a| a.occurrence(id)).is_some_and(|o| self.assembly(o.component).is_ok())
    }

    /// Every part of the scene, placed: a part of an assembly with three occurrences appears
    /// three times.
    pub fn placed_parts(&self) -> Vec<PlacedPart> {
        let mut out = Vec::new();
        self.walk(self.root, &mut Vec::new(), DAffine3::IDENTITY, None, &mut out, 0);
        out
    }

    /// The parts `self` places otherwise than `before` does at the same path — another part,
    /// the same one changed (its component not `before`'s, or one of `touched`), or elsewhere —
    /// as `self` places them, each with what `before` placed there (part and part-to-world).
    /// Paths `before` lacks are left out. What both share (the same components, unchanged down
    /// the tree, placed alike) is skipped whole: a change to a few occurrences of a large model
    /// costs a pass over the assemblies holding them, not a walk of every placement.
    pub fn placed_changes(&self, before: &Document, touched: &[Id]) -> Vec<(PlacedPart, Id, DAffine3)> {
        let mut walk = ChangesWalk { before, touched, dirty: HashMap::new(), out: Vec::new() };
        let t = DAffine3::IDENTITY;
        self.walk_changes(&mut walk, (self.root, before.root), &mut Vec::new(), (t, t), None, 0);
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
        material: Option<Id>,
        depth: usize,
    ) {
        let before = walk.before;
        let (Ok(a), Ok(b)) = (self.assembly(assembly), before.assembly(was)) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        // Occurrences keep their order: matched by place, else (some added or removed) by id.
        let mut by_id: Option<HashMap<Id, &Occurrence>> = None;
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
            match (&c.kind, &cb.kind) {
                (ComponentKind::Part(p), ComponentKind::Part(_)) => walk.out.push((
                    PlacedPart {
                        path: EntityPath::new(path.clone(), o.id),
                        part: c.id,
                        transform: t,
                        shape: p.shape().clone(),
                        material: o.material.or(material),
                    },
                    cb.id,
                    tb,
                )),
                (ComponentKind::Assembly(_), ComponentKind::Assembly(_)) => {
                    path.push(o.id);
                    self.walk_changes(walk, (c.id, cb.id), path, (t, tb), o.material.or(material), depth + 1);
                    path.pop();
                }
                _ => {}
            }
        }
    }

    fn walk(&self, assembly: Id, path: &mut Vec<Id>, t: DAffine3, material: Option<Id>, out: &mut Vec<PlacedPart>, depth: usize) {
        let Ok(a) = self.assembly(assembly) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        for o in &a.occurrences {
            let Ok(c) = self.component(o.component) else { continue };
            let t = t * o.transform;
            match &c.kind {
                ComponentKind::Part(p) => out.push(PlacedPart {
                    path: EntityPath::new(path.clone(), o.id),
                    part: c.id,
                    transform: t,
                    shape: p.shape().clone(),
                    material: o.material.or(material),
                }),
                ComponentKind::Assembly(_) => {
                    path.push(o.id);
                    self.walk(c.id, path, t, o.material.or(material), out, depth + 1);
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
        self.walk_guides(self.root, &mut Vec::new(), DAffine3::IDENTITY, &mut out, 0);
        out
    }

    fn walk_guides(&self, component: Id, path: &mut Vec<Id>, t: DAffine3, out: &mut Vec<PlacedGuide>, depth: usize) {
        let Ok(c) = self.component(component) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        for g in &c.guides {
            out.push(PlacedGuide { path: path.clone(), component, transform: t, guide: g.clone() });
        }
        if let ComponentKind::Assembly(a) = &c.kind {
            for o in &a.occurrences {
                path.push(o.id);
                self.walk_guides(o.component, path, t * o.transform, out, depth + 1);
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
            let mut used = std::collections::HashSet::new();
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
    pub(crate) fn graft(&mut self, fragment: &Document) -> Result<Vec<Occurrence>> {
        let mut materials: HashMap<Id, Id> = HashMap::new();
        for (id, m) in &fragment.materials {
            let same = self.materials.iter().find(|(_, here)| *here == m).map(|(k, _)| *k);
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
        let components: HashMap<Id, Id> =
            fragment.components.keys().filter(|id| **id != fragment.root).map(|id| (*id, self.fresh_id())).collect();
        let occurrences = |doc: &mut Document, list: &[Occurrence]| -> Result<Vec<Occurrence>> {
            list.iter()
                .map(|o| {
                    let component = *components.get(&o.component).ok_or(Error::NotFound(o.component))?;
                    let material = o.material.and_then(|m| materials.get(&m).copied());
                    Ok(Occurrence { id: doc.fresh_id(), name: o.name.clone(), component, transform: o.transform, material })
                })
                .collect()
        };
        // In the fragment's order: ids and names come out the same each time.
        for (old, c) in fragment.components.iter().filter(|(id, _)| **id != fragment.root) {
            let new = &components[old];
            let kind = match &c.kind {
                ComponentKind::Assembly(a) => ComponentKind::Assembly(Assembly::new(occurrences(self, &a.occurrences)?)),
                ComponentKind::Part(p) => {
                    let mut p = p.clone();
                    let mut machinings: HashMap<Id, Id> = HashMap::new();
                    for m in &mut p.machinings {
                        let id = self.fresh_id();
                        machinings.insert(m.id, id);
                        m.id = id;
                    }
                    p.failed = p.failed.iter().filter_map(|id| machinings.get(id).copied()).collect();
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

//! The document: components — parts and assemblies — placed by occurrences
//! (docs/ARCHITECTURE.md §2).
//!
//! The document is immutable piece by piece: components and solids sit behind `Arc`, so a
//! clone shares everything and an operation copies only what it changes (`Arc::make_mut`).

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::DAffine3;
use serde::{Deserialize, Serialize};
use solidsketch_i18n::t;

use crate::settings::Settings;
use solidsketch_kernel::Solid;

use crate::{Error, Result};

/// A stable identifier, from one counter: unique across a document for components and
/// machinings, within their assembly for occurrences (docs/ARCHITECTURE.md §2.6).
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
    /// The material of single faces of the shape, by their index in its record (§7 quater):
    /// over the occurrence's. A boolean renumbers faces: an index may then name another
    /// face, or none (ignored).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub faces: BTreeMap<u32, Id>,
}

impl Part {
    pub fn new(blank: Arc<Solid>) -> Self {
        Part { blank, machinings: Vec::new(), shape: None, faces: BTreeMap::new() }
    }

    /// The part as it is drawn and used: the blank with its machinings cut.
    pub fn shape(&self) -> &Arc<Solid> {
        self.shape.as_ref().unwrap_or(&self.blank)
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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Assembly {
    pub occurrences: Vec<Occurrence>,
}

impl Assembly {
    pub fn occurrence(&self, id: Id) -> Option<&Occurrence> {
        self.occurrences.iter().find(|o| o.id == id)
    }

    pub(crate) fn occurrence_mut(&mut self, id: Id) -> Option<&mut Occurrence> {
        self.occurrences.iter_mut().find(|o| o.id == id)
    }

    pub(crate) fn position(&self, id: Id) -> Option<usize> {
        self.occurrences.iter().position(|o| o.id == id)
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
        let c = Component { id: root, name: t!("default_name.model"), named: true, kind: ComponentKind::Assembly(Assembly::default()) };
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
        self.components
            .values()
            .filter_map(|c| c.as_assembly())
            .flat_map(|a| &a.occurrences)
            .filter(|o| o.component == component)
            .count()
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
        self.assembly(context)
            .ok()
            .and_then(|a| a.occurrence(id))
            .is_some_and(|o| self.assembly(o.component).is_ok())
    }

    /// Every part of the scene, placed: a part of an assembly with three occurrences appears
    /// three times.
    pub fn placed_parts(&self) -> Vec<PlacedPart> {
        let mut out = Vec::new();
        self.walk(self.root, &mut Vec::new(), DAffine3::IDENTITY, None, &mut out, 0);
        out
    }

    fn walk(
        &self,
        assembly: Id,
        path: &mut Vec<Id>,
        t: DAffine3,
        material: Option<Id>,
        out: &mut Vec<PlacedPart>,
        depth: usize,
    ) {
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
        self.components.insert(id, Arc::new(Component { id, name, named, kind }));
        id
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
}

/// The deepest nesting walked; deeper is treated as a cycle.
pub const MAX_DEPTH: usize = 64;

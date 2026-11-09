//! The document: definitions holding solids and instances (docs/ARCHITECTURE.md §2).
//!
//! The document is immutable piece by piece: definitions and solids sit behind `Arc`, so a
//! clone shares everything and an operation copies only what it changes (`Arc::make_mut`).

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::DAffine3;
use serde::{Deserialize, Serialize};

use crate::settings::Settings;
use solidsketch_kernel::Solid;

use crate::{Error, Result};

/// A stable identifier, unique across a document (definitions, solids and instances alike).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub u64);

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// What a definition is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefinitionKind {
    /// The model itself: the top of the tree, never instanced.
    Root,
    /// A group: a definition with a single instance, dropped when that instance goes.
    Group,
    /// A component: shareable by any number of instances, kept while unused.
    Component,
}

/// A container of entities: the model, a group or a component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub id: Id,
    pub name: String,
    pub kind: DefinitionKind,
    pub entities: Vec<Entity>,
}

impl Definition {
    pub fn entity(&self, id: Id) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id() == id)
    }

    pub(crate) fn entity_mut(&mut self, id: Id) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.id() == id)
    }

    pub(crate) fn position(&self, id: Id) -> Option<usize> {
        self.entities.iter().position(|e| e.id() == id)
    }
}

/// What a definition holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Entity {
    Solid(SolidEntity),
    Instance(Instance),
}

impl Entity {
    pub fn id(&self) -> Id {
        match self {
            Entity::Solid(s) => s.id,
            Entity::Instance(i) => i.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Entity::Solid(s) => &s.name,
            Entity::Instance(i) => &i.name,
        }
    }

    pub(crate) fn set_name(&mut self, name: String) {
        match self {
            Entity::Solid(s) => s.name = name,
            Entity::Instance(i) => i.name = name,
        }
    }

    pub fn as_solid(&self) -> Option<&SolidEntity> {
        match self {
            Entity::Solid(s) => Some(s),
            Entity::Instance(_) => None,
        }
    }

    pub fn as_instance(&self) -> Option<&Instance> {
        match self {
            Entity::Instance(i) => Some(i),
            Entity::Solid(_) => None,
        }
    }
}

/// A solid in its definition's coordinates (it has no transform of its own).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolidEntity {
    pub id: Id,
    pub name: String,
    pub solid: Arc<Solid>,
}

/// A placed use of a group's or component's definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub id: Id,
    /// The instance's own name (SketchUp's "instance name"); may be empty.
    pub name: String,
    pub definition: Id,
    /// From the definition's coordinates to its container's.
    pub transform: DAffine3,
}

/// An entity in the scene: the instances leading to it from the root, then the entity itself.
/// Two instances of one component make two paths to the same solid.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityPath {
    /// Instance ids from the root, outermost first; empty for an entity of the root.
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
}

/// The whole model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub(crate) definitions: BTreeMap<Id, Arc<Definition>>,
    pub(crate) root: Id,
    pub(crate) next_id: u64,
    /// Display style and units, saved with the model (absent from older files: defaults).
    #[serde(default)]
    pub(crate) settings: Settings,
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
        let def = Definition { id: root, name: "Modèle".into(), kind: DefinitionKind::Root, entities: Vec::new() };
        Document { definitions: BTreeMap::from([(root, Arc::new(def))]), root, next_id: 2, settings }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn root(&self) -> Id {
        self.root
    }

    pub fn root_definition(&self) -> &Definition {
        // The root is always present: `new` creates it and no operation removes it.
        &self.definitions[&self.root]
    }

    pub fn definition(&self, id: Id) -> Result<&Definition> {
        self.definitions.get(&id).map(|d| &**d).ok_or(Error::NotFound(id))
    }

    pub fn definitions(&self) -> impl Iterator<Item = &Definition> {
        self.definitions.values().map(|d| &**d)
    }

    /// Components, in id order (the "In model" library).
    pub fn components(&self) -> impl Iterator<Item = &Definition> {
        self.definitions().filter(|d| d.kind == DefinitionKind::Component)
    }

    /// How many instances use `definition`, anywhere.
    pub fn instance_count(&self, definition: Id) -> usize {
        self.definitions()
            .flat_map(|d| &d.entities)
            .filter(|e| e.as_instance().is_some_and(|i| i.definition == definition))
            .count()
    }

    /// The definition an instance path leads into (the root for an empty path).
    pub fn context_of(&self, instances: &[Id]) -> Result<Id> {
        let mut def = self.root;
        for &i in instances {
            let inst = self.definition(def)?.entity(i).and_then(Entity::as_instance).ok_or(Error::NotFound(i))?;
            def = inst.definition;
        }
        Ok(def)
    }

    /// The transform from the coordinates of the definition `instances` leads into to the root's.
    pub fn transform_of(&self, instances: &[Id]) -> Result<DAffine3> {
        let mut def = self.root;
        let mut t = DAffine3::IDENTITY;
        for &i in instances {
            let inst = self.definition(def)?.entity(i).and_then(Entity::as_instance).ok_or(Error::NotFound(i))?;
            t *= inst.transform;
            def = inst.definition;
        }
        Ok(t)
    }

    /// The entity a path names.
    pub fn resolve(&self, path: &EntityPath) -> Result<&Entity> {
        let ctx = self.context_of(&path.instances)?;
        self.definition(ctx)?.entity(path.entity).ok_or(Error::NotFound(path.entity))
    }

    /// Every solid of the scene, placed: its path and its transform to the root. A solid in a
    /// component with three instances appears three times.
    pub fn placed_solids(&self) -> Vec<(EntityPath, DAffine3, Arc<Solid>)> {
        let mut out = Vec::new();
        self.walk(self.root, &mut Vec::new(), DAffine3::IDENTITY, &mut out, 0);
        out
    }

    fn walk(&self, def: Id, path: &mut Vec<Id>, t: DAffine3, out: &mut Vec<(EntityPath, DAffine3, Arc<Solid>)>, depth: usize) {
        let Some(d) = self.definitions.get(&def) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        for e in &d.entities {
            match e {
                Entity::Solid(s) => out.push((EntityPath::new(path.clone(), s.id), t, s.solid.clone())),
                Entity::Instance(i) => {
                    path.push(i.id);
                    self.walk(i.definition, path, t * i.transform, out, depth + 1);
                    path.pop();
                }
            }
        }
    }

    /// Does `def` contain an instance of `target`, at any depth (or is it `target`)?
    pub fn contains_definition(&self, def: Id, target: Id) -> bool {
        fn go(doc: &Document, def: Id, target: Id, depth: usize) -> bool {
            if def == target {
                return true;
            }
            if depth > MAX_DEPTH {
                return true; // treat a runaway as a cycle
            }
            doc.definitions.get(&def).is_some_and(|d| {
                d.entities.iter().filter_map(Entity::as_instance).any(|i| go(doc, i.definition, target, depth + 1))
            })
        }
        go(self, def, target, 0)
    }

    // --- mutation helpers, for operations ---------------------------------------------------

    pub(crate) fn fresh_id(&mut self) -> Id {
        let id = Id(self.next_id);
        self.next_id += 1;
        id
    }

    pub(crate) fn definition_mut(&mut self, id: Id) -> Result<&mut Definition> {
        self.definitions.get_mut(&id).map(Arc::make_mut).ok_or(Error::NotFound(id))
    }

    pub(crate) fn insert_definition(&mut self, def: Definition) {
        self.definitions.insert(def.id, Arc::new(def));
    }

    /// A name not used by another definition: `base`, else `base#2`, `base#3`, …
    pub(crate) fn unique_definition_name(&self, base: &str) -> String {
        let taken = |n: &str| self.definitions().any(|d| d.name == n);
        if !taken(base) {
            return base.to_owned();
        }
        (2..).map(|i| format!("{base}#{i}")).find(|n| !taken(n)).unwrap_or_default()
    }

    /// Drops groups no instance uses any more (components stay, as SketchUp's library does).
    pub(crate) fn purge_orphan_groups(&mut self) {
        loop {
            let orphans: Vec<Id> = self
                .definitions()
                .filter(|d| d.kind == DefinitionKind::Group && self.instance_count(d.id) == 0)
                .map(|d| d.id)
                .collect();
            if orphans.is_empty() {
                return;
            }
            for id in orphans {
                self.definitions.remove(&id);
            }
        }
    }

    /// Copies of `entities` with fresh ids. A copied component instance keeps its definition;
    /// a copied group instance gets a copy of its definition (a group has one instance), deeply.
    pub(crate) fn copy_entities(&mut self, entities: &[Entity]) -> Result<Vec<Entity>> {
        let mut out = Vec::with_capacity(entities.len());
        for e in entities {
            let mut e = e.clone();
            let id = self.fresh_id();
            match &mut e {
                Entity::Solid(s) => s.id = id,
                Entity::Instance(i) => {
                    i.id = id;
                    if self.definition(i.definition)?.kind == DefinitionKind::Group {
                        i.definition = self.copy_definition(i.definition, DefinitionKind::Group)?;
                    }
                }
            }
            out.push(e);
        }
        Ok(out)
    }

    /// A new definition of `kind` with a copy of `def`'s contents ([`Self::copy_entities`]).
    pub(crate) fn copy_definition(&mut self, def: Id, kind: DefinitionKind) -> Result<Id> {
        let src = self.definition(def)?.clone();
        let entities = self.copy_entities(&src.entities)?;
        let id = self.fresh_id();
        let name = match kind {
            DefinitionKind::Component => self.unique_definition_name(&src.name),
            _ => src.name.clone(),
        };
        self.insert_definition(Definition { id, name, kind, entities });
        Ok(id)
    }
}

/// The deepest nesting walked; deeper is treated as a cycle.
pub const MAX_DEPTH: usize = 64;

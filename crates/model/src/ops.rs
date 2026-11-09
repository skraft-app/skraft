//! Operations: every change to a document (docs/ARCHITECTURE.md §1).
//!
//! An operation is plain, serialisable data. [`Document::apply`] never changes the document it
//! is called on: it returns a new one that shares everything the operation left alone.

use std::sync::Arc;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use solidsketch_kernel::{BooleanOp, Solid};

use crate::document::{Definition, DefinitionKind, Document, Entity, EntityPath, Id, Instance, SolidEntity};
use crate::settings::Settings;
use crate::{Error, Result};

/// A primitive solid, by its parameters (mm), in its context's coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Primitive {
    /// One corner at `origin`, extents along X, Y, Z.
    Box { origin: DVec3, size: DVec3 },
    /// Standing on Z = `origin.z`, along +Z.
    Cylinder { origin: DVec3, radius: f64, height: f64 },
    Sphere { center: DVec3, radius: f64 },
    Cone { origin: DVec3, base_radius: f64, top_radius: f64, height: f64 },
    Torus { center: DVec3, major_radius: f64, minor_radius: f64 },
}

impl Primitive {
    pub fn build(&self) -> Result<Solid> {
        Ok(match *self {
            Primitive::Box { origin, size } => Solid::new_box(origin, size)?,
            Primitive::Cylinder { origin, radius, height } => Solid::new_cylinder(origin, radius, height)?,
            Primitive::Sphere { center, radius } => Solid::new_sphere(center, radius)?,
            Primitive::Cone { origin, base_radius, top_radius, height } => {
                Solid::new_cone(origin, base_radius, top_radius, height)?
            }
            Primitive::Torus { center, major_radius, minor_radius } => {
                Solid::new_torus(center, major_radius, minor_radius)?
            }
        })
    }

    /// The default name of a solid built from this primitive.
    pub fn noun(&self) -> &'static str {
        match self {
            Primitive::Box { .. } => "Boîte",
            Primitive::Cylinder { .. } => "Cylindre",
            Primitive::Sphere { .. } => "Sphère",
            Primitive::Cone { .. } => "Cône",
            Primitive::Torus { .. } => "Tore",
        }
    }
}

/// A boolean operation, serialisable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Boolean {
    Union,
    Difference,
    Intersection,
}

impl From<Boolean> for BooleanOp {
    fn from(b: Boolean) -> Self {
        match b {
            Boolean::Union => BooleanOp::Union,
            Boolean::Difference => BooleanOp::Difference,
            Boolean::Intersection => BooleanOp::Intersection,
        }
    }
}

fn identity() -> DAffine3 {
    DAffine3::IDENTITY
}

/// Every change to a document. `context` is always the id of the definition being edited.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Operation {
    /// Adds a primitive solid to `context`, built in its own frame then placed by the rigid
    /// `placement` (a box drawn on a wall, §6 ter); identity for an axis-aligned one.
    AddPrimitive {
        context: Id,
        primitive: Primitive,
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        placement: DAffine3,
    },
    /// Moves (rotates, scales) entities of `context` by `transform`, in `context`'s coordinates:
    /// an instance's transform is premultiplied, a solid's B-rep is transformed.
    Transform { context: Id, entities: Vec<Id>, transform: DAffine3 },
    /// Copies entities of `context`, the copies moved by `transform`.
    Duplicate { context: Id, entities: Vec<Id>, transform: DAffine3 },
    /// `count` copies of entities of `context`, the k-th moved by `step` applied k times (a
    /// linear array for a translation, a circular one for a rotation).
    Array { context: Id, entities: Vec<Id>, step: DAffine3, count: u32 },
    /// Removes entities from `context`.
    Delete { context: Id, entities: Vec<Id> },
    /// Renames an entity of `context` (an instance's own name, or a solid's).
    RenameEntity { context: Id, entity: Id, name: String },
    /// Renames a group's or component's definition.
    RenameDefinition { definition: Id, name: String },
    /// Moves entities of `context` into a new group or component, placed by one new instance.
    MakeContainer { context: Id, entities: Vec<Id>, kind: DefinitionKind, name: Option<String> },
    /// Replaces an instance by its definition's contents, placed by its transform.
    Explode { context: Id, instance: Id },
    /// Gives one instance of a shared component a definition of its own.
    MakeUnique { context: Id, instance: Id },
    /// Turns a group into a component or a component with at most one instance into a group.
    SetKind { definition: Id, kind: DefinitionKind },
    /// Places a new instance of a component in `context`.
    PlaceInstance { context: Id, definition: Id, transform: DAffine3 },
    /// `target op tool`. The target is replaced in its own context; the tool is brought into
    /// the target's coordinates, then removed unless `keep_tool` (ARCHITECTURE.md §4).
    Boolean { target: EntityPath, tool: EntityPath, op: Boolean, keep_tool: bool },
    /// Replaces the document's settings (display style, units).
    SetSettings(Settings),
}

impl Operation {
    /// A short label for menus and the undo history.
    pub fn label(&self) -> &'static str {
        match self {
            Operation::AddPrimitive { primitive, .. } => primitive.noun(),
            Operation::Transform { .. } => "Déplacer",
            Operation::Duplicate { .. } => "Copier",
            Operation::Array { .. } => "Réseau",
            Operation::Delete { .. } => "Effacer",
            Operation::RenameEntity { .. } | Operation::RenameDefinition { .. } => "Renommer",
            Operation::MakeContainer { kind: DefinitionKind::Component, .. } => "Créer un composant",
            Operation::MakeContainer { .. } => "Créer un groupe",
            Operation::Explode { .. } => "Éclater",
            Operation::MakeUnique { .. } => "Rendre unique",
            Operation::SetKind { .. } => "Changer de type",
            Operation::PlaceInstance { .. } => "Placer un composant",
            Operation::Boolean { op: Boolean::Union, .. } => "Union",
            Operation::Boolean { op: Boolean::Difference, .. } => "Soustraction",
            Operation::Boolean { op: Boolean::Intersection, .. } => "Intersection",
            Operation::SetSettings(_) => "Réglages du modèle",
        }
    }

    /// Does this step fold into the previous one `prev` in the undo history? (Settings
    /// changed in a row, e.g. while dragging a colour, undo in one go.)
    pub fn merges_into(&self, prev: &Operation) -> bool {
        matches!((self, prev), (Operation::SetSettings(_), Operation::SetSettings(_)))
    }
}

/// The most copies an `Operation::Array` makes.
pub const MAX_ARRAY: u32 = 1000;

/// What an operation did, for the caller to update its selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Entities created, in the operation's context (the target's, for a boolean).
    pub created: Vec<Id>,
}

impl Document {
    /// Applies `op` to a copy of this document. On error the document is unchanged.
    pub fn apply(&self, op: &Operation) -> Result<(Document, Outcome)> {
        let mut doc = self.clone();
        let outcome = doc.apply_in_place(op)?;
        doc.purge_orphan_groups();
        Ok((doc, outcome))
    }

    fn apply_in_place(&mut self, op: &Operation) -> Result<Outcome> {
        match op {
            Operation::SetSettings(settings) => {
                self.settings = settings.clone();
                Ok(Outcome::default())
            }
            Operation::AddPrimitive { context, primitive, placement } => {
                self.definition(*context)?;
                let mut solid = primitive.build()?;
                if !is_identity(placement) {
                    solid = solid.transformed(placement)?;
                }
                let id = self.fresh_id();
                let name = primitive.noun().to_owned();
                self.definition_mut(*context)?.entities.push(Entity::Solid(SolidEntity { id, name, solid: Arc::new(solid) }));
                Ok(Outcome { created: vec![id] })
            }
            Operation::Transform { context, entities, transform } => {
                let def = self.definition(*context)?;
                let mut moved = Vec::with_capacity(entities.len());
                for &id in entities {
                    let e = def.entity(id).ok_or(Error::NotFound(id))?;
                    moved.push(transformed(e.clone(), transform)?);
                }
                let def = self.definition_mut(*context)?;
                for e in moved {
                    if let Some(slot) = def.entity_mut(e.id()) {
                        *slot = e;
                    }
                }
                Ok(Outcome::default())
            }
            Operation::Duplicate { context, entities, transform } => {
                let sources = self.entities_of(*context, entities)?;
                let copies = self.copy_entities(&sources)?;
                let copies = copies.into_iter().map(|e| transformed(e, transform)).collect::<Result<Vec<_>>>()?;
                let created = copies.iter().map(Entity::id).collect();
                self.definition_mut(*context)?.entities.extend(copies);
                Ok(Outcome { created })
            }
            Operation::Array { context, entities, step, count } => {
                if *count == 0 || *count > MAX_ARRAY {
                    return Err(Error::Invalid("array count out of range"));
                }
                let sources = self.entities_of(*context, entities)?;
                let mut at = DAffine3::IDENTITY;
                let mut created = Vec::new();
                for _ in 0..*count {
                    at = *step * at;
                    let copies = self.copy_entities(&sources)?;
                    let copies = copies.into_iter().map(|e| transformed(e, &at)).collect::<Result<Vec<_>>>()?;
                    created.extend(copies.iter().map(Entity::id));
                    self.definition_mut(*context)?.entities.extend(copies);
                }
                Ok(Outcome { created })
            }
            Operation::Delete { context, entities } => {
                self.entities_of(*context, entities)?;
                self.definition_mut(*context)?.entities.retain(|e| !entities.contains(&e.id()));
                Ok(Outcome::default())
            }
            Operation::RenameEntity { context, entity, name } => {
                self.definition(*context)?.entity(*entity).ok_or(Error::NotFound(*entity))?;
                if let Some(e) = self.definition_mut(*context)?.entity_mut(*entity) {
                    e.set_name(name.clone());
                }
                Ok(Outcome::default())
            }
            Operation::RenameDefinition { definition, name } => {
                if self.definition(*definition)?.kind == DefinitionKind::Root {
                    return Err(Error::Invalid("the model is not renamed"));
                }
                if self.definitions().any(|d| d.id != *definition && d.name == *name) {
                    return Err(Error::Invalid("another definition has that name"));
                }
                self.definition_mut(*definition)?.name = name.clone();
                Ok(Outcome::default())
            }
            Operation::MakeContainer { context, entities, kind, name } => {
                if *kind == DefinitionKind::Root {
                    return Err(Error::Invalid("a container is a group or a component"));
                }
                if entities.is_empty() {
                    return Err(Error::Invalid("nothing to put in the container"));
                }
                let moved = self.entities_of(*context, entities)?;
                let def_id = self.fresh_id();
                let base = match (name, kind) {
                    (Some(n), _) => n.clone(),
                    (None, DefinitionKind::Component) => "Composant".to_owned(),
                    (None, _) => "Groupe".to_owned(),
                };
                let name = self.unique_definition_name(&base);
                self.insert_definition(Definition { id: def_id, name, kind: *kind, entities: moved });
                let inst = self.fresh_id();
                let ctx = self.definition_mut(*context)?;
                ctx.entities.retain(|e| !entities.contains(&e.id()));
                ctx.entities.push(Entity::Instance(Instance {
                    id: inst,
                    name: String::new(),
                    definition: def_id,
                    transform: DAffine3::IDENTITY,
                }));
                Ok(Outcome { created: vec![inst] })
            }
            Operation::Explode { context, instance } => {
                let inst = self.instance_of(*context, *instance)?;
                let contents = self.definition(inst.definition)?.entities.clone();
                let copies = self.copy_entities(&contents)?;
                let copies =
                    copies.into_iter().map(|e| transformed(e, &inst.transform)).collect::<Result<Vec<_>>>()?;
                let created = copies.iter().map(Entity::id).collect();
                let ctx = self.definition_mut(*context)?;
                let at = ctx.position(*instance).ok_or(Error::NotFound(*instance))?;
                ctx.entities.splice(at..=at, copies);
                Ok(Outcome { created })
            }
            Operation::MakeUnique { context, instance } => {
                let inst = self.instance_of(*context, *instance)?;
                if self.definition(inst.definition)?.kind != DefinitionKind::Component
                    || self.instance_count(inst.definition) < 2
                {
                    return Ok(Outcome::default());
                }
                let def = self.copy_definition(inst.definition, DefinitionKind::Component)?;
                if let Some(Entity::Instance(i)) = self.definition_mut(*context)?.entity_mut(*instance) {
                    i.definition = def;
                }
                Ok(Outcome::default())
            }
            Operation::SetKind { definition, kind } => {
                let current = self.definition(*definition)?.kind;
                if current == DefinitionKind::Root || *kind == DefinitionKind::Root {
                    return Err(Error::Invalid("the model is neither a group nor a component"));
                }
                if *kind == DefinitionKind::Group && self.instance_count(*definition) > 1 {
                    return Err(Error::Invalid("a component with several instances cannot become a group"));
                }
                if current != *kind {
                    let base = self.definition(*definition)?.name.clone();
                    let d = self.definition_mut(*definition)?;
                    d.kind = *kind;
                    // keep names unique among components
                    if *kind == DefinitionKind::Component {
                        d.name = String::new();
                        let name = self.unique_definition_name(&base);
                        self.definition_mut(*definition)?.name = name;
                    }
                }
                Ok(Outcome::default())
            }
            Operation::PlaceInstance { context, definition, transform } => {
                if self.definition(*definition)?.kind != DefinitionKind::Component {
                    return Err(Error::Invalid("only a component is placed again"));
                }
                self.definition(*context)?;
                if self.contains_definition(*definition, *context) {
                    return Err(Error::Cycle);
                }
                let id = self.fresh_id();
                self.definition_mut(*context)?.entities.push(Entity::Instance(Instance {
                    id,
                    name: String::new(),
                    definition: *definition,
                    transform: *transform,
                }));
                Ok(Outcome { created: vec![id] })
            }
            Operation::Boolean { target, tool, op, keep_tool } => self.boolean(target, tool, *op, *keep_tool),
        }
    }

    fn boolean(&mut self, target: &EntityPath, tool: &EntityPath, op: Boolean, keep_tool: bool) -> Result<Outcome> {
        let target_ctx = self.context_of(&target.instances)?;
        let tool_ctx = self.context_of(&tool.instances)?;
        if target_ctx == tool_ctx && target.entity == tool.entity {
            return Err(Error::Invalid("a solid is not combined with itself"));
        }
        let a = self.resolve(target)?.as_solid().ok_or(Error::NotASolid(target.entity))?.solid.clone();
        let b = self.resolve(tool)?.as_solid().ok_or(Error::NotASolid(tool.entity))?.solid.clone();
        // The tool, in the target's coordinates.
        let relative = self.transform_of(&target.instances)?.inverse() * self.transform_of(&tool.instances)?;
        let b = if is_identity(&relative) { b } else { Arc::new(b.transformed(&relative)?) };
        let result = a.boolean(&b, op.into())?;
        if let Some(Entity::Solid(s)) = self.definition_mut(target_ctx)?.entity_mut(target.entity) {
            s.solid = Arc::new(result);
        }
        if !keep_tool {
            self.definition_mut(tool_ctx)?.entities.retain(|e| e.id() != tool.entity);
        }
        Ok(Outcome { created: vec![target.entity] })
    }

    /// Clones of the entities `ids` of `context`, in the context's order; every id must be there.
    fn entities_of(&self, context: Id, ids: &[Id]) -> Result<Vec<Entity>> {
        let def = self.definition(context)?;
        if let Some(&missing) = ids.iter().find(|id| def.entity(**id).is_none()) {
            return Err(Error::NotFound(missing));
        }
        Ok(def.entities.iter().filter(|e| ids.contains(&e.id())).cloned().collect())
    }

    fn instance_of(&self, context: Id, id: Id) -> Result<Instance> {
        self.definition(context)?
            .entity(id)
            .ok_or(Error::NotFound(id))?
            .as_instance()
            .cloned()
            .ok_or(Error::NotAnInstance(id))
    }
}

/// `e` moved by `t`: a solid's B-rep is transformed, an instance's transform premultiplied.
fn transformed(e: Entity, t: &DAffine3) -> Result<Entity> {
    if is_identity(t) {
        return Ok(e);
    }
    Ok(match e {
        Entity::Solid(mut s) => {
            s.solid = Arc::new(s.solid.transformed(t)?);
            Entity::Solid(s)
        }
        Entity::Instance(mut i) => {
            i.transform = *t * i.transform;
            Entity::Instance(i)
        }
    })
}

fn is_identity(t: &DAffine3) -> bool {
    t.abs_diff_eq(DAffine3::IDENTITY, 1e-12)
}

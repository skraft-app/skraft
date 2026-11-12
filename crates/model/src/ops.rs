//! Operations: every change to a document (docs/ARCHITECTURE.md §1).
//!
//! An operation is plain, serialisable data. Applying it never changes the document it is
//! called on: it gives a new one that shares everything the operation left alone.
//!
//! The kernel runs elsewhere (docs/ARCHITECTURE.md §7 ter), so an operation applies in two
//! steps: [`Document::prepare`] validates it and gathers the kernel calls it needs into a
//! [`Job`] — or, needing none, is already done; [`Document::commit`] applies it with the job's
//! outputs. Both run the same code (`apply_in_place`) over a [`Solver`]: one records the calls,
//! the other hands out their results in order.
//!
//! Moving, copying, arraying and grouping never need the kernel: they only change occurrences
//! (§2.2, §2.5). Building a part and cutting it do.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use solidsketch_i18n::t;
use solidsketch_kernel::{BooleanOp, Display, EARLIER, Job, Output, Primitive, Solid, Stretch, Task};

use crate::document::{
    Assembly, ComponentKind, Document, EntityPath, Guide, GuideShape, Id, Machining, MachiningSource, Material, Occurrence, Part,
};
use crate::settings::Settings;
use crate::{Error, Refusal, Result};

/// What a change to a part touches (ARCHITECTURE.md §2.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    /// The part itself: every occurrence of it shows the change.
    #[default]
    All,
    /// Only the occurrence picked: the assemblies on its way shown elsewhere, then the part,
    /// are made unique first.
    This,
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

/// Every change to a document. `context` is always the id of the assembly being edited, and
/// `entities` occurrences of it — or its guides (§6 quinquies), which removing, moving,
/// copying and grouping take too, as the operations on machinings take the part's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Operation {
    /// A new part built from a primitive, and its first occurrence in `context`: the blank in
    /// the part's own axes, the occurrence placed by the rigid `placement` (a box drawn on a
    /// wall, §6 ter; identity for an axis-aligned one) and where the primitive stood.
    AddPrimitive {
        context: Id,
        primitive: Primitive,
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        placement: DAffine3,
    },
    /// Moves (rotates, mirrors) occurrences of `context` by `transform`, in `context`'s axes.
    /// Rigid only: a scale changes a part, not its place (§2.2).
    Transform { context: Id, entities: Vec<Id>, transform: DAffine3 },
    /// Copies occurrences of `context`, moved by `transform`: more copies of the same parts.
    Duplicate { context: Id, entities: Vec<Id>, transform: DAffine3 },
    /// `count` copies of occurrences of `context`, the k-th moved by `step` applied k times (a
    /// linear array for a translation, a circular one for a rotation).
    Array { context: Id, entities: Vec<Id>, step: DAffine3, count: u32 },
    /// Removes occurrences from `context`.
    Delete { context: Id, entities: Vec<Id> },
    /// Names one occurrence of `context` ("Gauche"); empty to clear.
    RenameOccurrence { context: Id, occurrence: Id, name: String },
    /// Names a part or an assembly; a named component is kept while unused.
    RenameComponent { component: Id, name: String },
    /// Moves occurrences of `context` into a new assembly, placed by one new occurrence.
    MakeAssembly { context: Id, entities: Vec<Id>, name: Option<String> },
    /// Replaces an occurrence of an assembly by the assembly's occurrences, placed by it.
    Explode { context: Id, occurrence: Id },
    /// Gives one occurrence a component of its own, a copy of the one it shared ("faire une
    /// pièce à part").
    MakeUnique { context: Id, occurrence: Id },
    /// Places one more occurrence of a component in `context`.
    PlaceOccurrence { context: Id, component: Id, transform: DAffine3 },
    /// `target op tool`, on the target's part, the tool brought into its axes
    /// (ARCHITECTURE.md §4): on every occurrence of the part, or only the one picked
    /// (`scope`, §2.5). A difference adds a machining; a union or an intersection makes a new
    /// blank. The tool's occurrence leaves the model.
    Boolean {
        target: EntityPath,
        tool: EntityPath,
        op: Boolean,
        #[serde(default)]
        scope: Scope,
    },
    /// A primitive cut from the part `target` places, as a new machining (drawn in an open
    /// part, ARCHITECTURE.md §2.9): built in its own axes, placed by the rigid `placement` in
    /// the part's, as [`Operation::AddPrimitive`] places a blank in its context.
    AddMachining {
        target: EntityPath,
        primitive: Primitive,
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        placement: DAffine3,
        #[serde(default)]
        scope: Scope,
    },
    /// Removes machinings from the part `target` places; the part is computed again from its
    /// blank.
    DeleteMachinings {
        target: EntityPath,
        machinings: Vec<Id>,
        #[serde(default)]
        scope: Scope,
    },
    /// Moves (rotates, mirrors) machinings of the part `target` places by `transform`, in the
    /// part's axes; the part is computed again from its blank.
    TransformMachinings {
        target: EntityPath,
        machinings: Vec<Id>,
        transform: DAffine3,
        #[serde(default)]
        scope: Scope,
    },
    /// Copies machinings of the part `target` places, moved by `transform` in its axes: the
    /// copies are cut from the part as it is.
    DuplicateMachinings {
        target: EntityPath,
        machinings: Vec<Id>,
        transform: DAffine3,
        #[serde(default)]
        scope: Scope,
    },
    /// `count` copies of machinings of the part `target` places, the k-th moved by `step`
    /// applied k times, in its axes.
    ArrayMachinings {
        target: EntityPath,
        machinings: Vec<Id>,
        step: DAffine3,
        count: u32,
        #[serde(default)]
        scope: Scope,
    },
    /// Stretches what the occurrences `occurrences` of the assembly `context` leads into place
    /// — parts or assemblies — along `axis`, in the axes `frame` places in the context's (its
    /// own, the context's or the model's, ARCHITECTURE.md §6 sexies): the cutters at `cuts`
    /// (increasing positions along the axis, as `axis · p`) split them into sections, section
    /// `i` moved by `moves[i]` along the axis. What lies in one section is moved; a part across
    /// a cutter has its blank stretched, a machining in one section moved with it, one across a
    /// cutter its tool stretched, its guides following their section, then is computed again
    /// from its blank; an assembly across a cutter is walked, each component stretched in its
    /// own axes (`stretch.rs`).
    Stretch {
        context: Vec<Id>,
        occurrences: Vec<Id>,
        frame: DAffine3,
        axis: DVec3,
        cuts: Vec<f64>,
        moves: Vec<f64>,
        #[serde(default)]
        scope: Scope,
    },
    /// Replaces the document's settings (display style, units).
    SetSettings(Settings),
    /// A new material (§7 quater).
    AddMaterial { name: String, color: [u8; 3] },
    /// Renames or recolours a material.
    EditMaterial { material: Id, name: String, color: [u8; 3] },
    /// Removes a material: what was painted with it is not any more.
    DeleteMaterial { material: Id },
    /// Paints occurrences of `context` (`None`: their material is removed).
    PaintOccurrences { context: Id, entities: Vec<Id>, material: Option<Id> },
    /// Paints faces of a part's shape, by their index in its record (every occurrence of the
    /// part shows it; `None`: they take the occurrence's material again).
    PaintFaces { part: Id, faces: Vec<u32>, material: Option<Id> },
    /// A construction guide added to `component` (a part or an assembly), in its axes
    /// (ARCHITECTURE.md §6 quinquies). Every occurrence shows it; no scope.
    AddGuide { component: Id, shape: GuideShape },
}

impl Operation {
    /// A short label for menus and the undo history.
    pub fn label(&self) -> String {
        match self {
            Operation::AddPrimitive { .. } => t!("operation.add_part"),
            Operation::Transform { .. } => t!("operation.transform"),
            Operation::Duplicate { .. } => t!("operation.duplicate"),
            Operation::Array { .. } => t!("operation.array"),
            Operation::Delete { .. } => t!("operation.delete"),
            Operation::RenameOccurrence { .. } | Operation::RenameComponent { .. } => t!("operation.rename"),
            Operation::MakeAssembly { .. } => t!("operation.make_assembly"),
            Operation::Explode { .. } => t!("operation.explode"),
            Operation::MakeUnique { .. } => t!("operation.make_unique"),
            Operation::PlaceOccurrence { .. } => t!("operation.place"),
            Operation::Boolean { op: Boolean::Union, .. } => t!("operation.union"),
            Operation::Boolean { op: Boolean::Difference, .. } => t!("operation.difference"),
            Operation::Boolean { op: Boolean::Intersection, .. } => t!("operation.intersection"),
            Operation::SetSettings(_) => t!("operation.settings"),
            Operation::AddMaterial { .. } => t!("operation.add_material"),
            Operation::EditMaterial { .. } => t!("operation.edit_material"),
            Operation::DeleteMaterial { .. } => t!("operation.delete_material"),
            Operation::PaintOccurrences { .. } | Operation::PaintFaces { .. } => t!("operation.paint"),
            Operation::AddMachining { .. } => t!("operation.add_machining"),
            Operation::DeleteMachinings { .. } => t!("operation.delete"),
            Operation::TransformMachinings { .. } => t!("operation.transform"),
            Operation::DuplicateMachinings { .. } => t!("operation.duplicate"),
            Operation::ArrayMachinings { .. } => t!("operation.array"),
            Operation::AddGuide { .. } => t!("operation.add_guide"),
            Operation::Stretch { .. } => t!("operation.stretch"),
        }
    }

    /// Does this step fold into the previous one `prev` in the undo history? (Settings
    /// changed in a row, e.g. while dragging a colour, undo in one go.)
    pub fn merges_into(&self, prev: &Operation) -> bool {
        match (self, prev) {
            (Operation::SetSettings(_), Operation::SetSettings(_)) => true,
            (Operation::EditMaterial { material: a, .. }, Operation::EditMaterial { material: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// The most copies an `Operation::Array` makes.
pub const MAX_ARRAY: u32 = 1000;

/// What an operation did, for the caller to update its selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Occurrences created, in the operation's context (the target's, for a boolean).
    pub created: Vec<Id>,
    /// Machinings created, in the target's part (as it is after the operation).
    pub machinings: Vec<Id>,
    /// The material created.
    pub material: Option<Id>,
    /// The guide created, in the operation's component.
    pub guide: Option<Id>,
    /// The parts a stretch changed, and how (§6 sexies).
    pub stretched: Vec<crate::StretchedPart>,
}

/// An operation applied: the new document, what it created, and the displays the kernel made
/// of the solids it produced (to feed the display scene's cache).
#[derive(Clone, Debug)]
pub struct Applied {
    pub document: Document,
    pub outcome: Outcome,
    pub displays: Vec<(Arc<Solid>, std::result::Result<Display, String>)>,
}

/// The first step of applying an operation.
#[derive(Clone, Debug)]
pub enum Prepared {
    /// The operation needs no kernel: here is the result (boxed: far larger than a job).
    Done(Box<Applied>),
    /// Run this job, then [`Document::commit`] its outputs.
    Kernel(Job),
}

impl Document {
    /// Validates `op` and gathers the kernel calls it needs. On error the document is unchanged.
    pub fn prepare(&self, op: &Operation) -> Result<Prepared> {
        let mut recorder = Recorder::default();
        let mut doc = self.clone();
        let outcome = doc.apply_in_place(op, &mut recorder)?;
        if recorder.job.tasks.is_empty() {
            doc.purge();
            return Ok(Prepared::Done(Box::new(Applied { document: doc, outcome, displays: Vec::new() })));
        }
        Ok(Prepared::Kernel(recorder.job))
    }

    /// Applies `op` with the outputs of the job [`Document::prepare`] gave for it, on this same
    /// document.
    pub fn commit(&self, op: &Operation, outputs: Vec<Output>) -> Result<Applied> {
        let mut replayer = Replayer { outputs: outputs.into_iter(), displays: Vec::new() };
        let mut doc = self.clone();
        let outcome = doc.apply_in_place(op, &mut replayer)?;
        if replayer.outputs.next().is_some() {
            return Err(Error::Invalid(Refusal::OutOfStep));
        }
        doc.purge();
        Ok(Applied { document: doc, outcome, displays: replayer.displays })
    }

    /// Applies `op` to a copy of this document, running the kernel here. On error the
    /// document is unchanged.
    #[cfg(any(test, feature = "engine"))]
    pub fn apply(&self, op: &Operation) -> Result<(Document, Outcome)> {
        let applied = match self.prepare(op)? {
            Prepared::Done(a) => *a,
            Prepared::Kernel(job) => self.commit(op, solidsketch_kernel::run(&job)?)?,
        };
        Ok((applied.document, applied.outcome))
    }

    fn apply_in_place(&mut self, op: &Operation, solver: &mut dyn Solver) -> Result<Outcome> {
        match op {
            Operation::SetSettings(settings) => {
                self.settings = settings.clone();
                Ok(Outcome::default())
            }
            Operation::AddMaterial { name, color } => {
                let id = self.fresh_id();
                self.materials.insert(id, Material { name: name.clone(), color: *color });
                Ok(Outcome { material: Some(id), ..Outcome::default() })
            }
            Operation::EditMaterial { material, name, color } => {
                let m = self.materials.get_mut(material).ok_or(Error::NotFound(*material))?;
                *m = Material { name: name.clone(), color: *color };
                Ok(Outcome::default())
            }
            Operation::DeleteMaterial { material } => {
                self.materials.remove(material).ok_or(Error::NotFound(*material))?;
                // Unpaint what used it, copying only the components that did.
                let users: Vec<Id> = self
                    .components
                    .values()
                    .filter(|c| match &c.kind {
                        ComponentKind::Part(p) => p.faces.values().any(|m| m == material),
                        ComponentKind::Assembly(a) => a.occurrences.iter().any(|o| o.material == Some(*material)),
                    })
                    .map(|c| c.id)
                    .collect();
                for id in users {
                    match &mut self.component_mut(id)?.kind {
                        ComponentKind::Part(p) => p.faces.retain(|_, m| m != material),
                        ComponentKind::Assembly(a) => {
                            for o in &mut a.occurrences {
                                if o.material == Some(*material) {
                                    o.material = None;
                                }
                            }
                        }
                    }
                }
                Ok(Outcome::default())
            }
            Operation::PaintOccurrences { context, entities, material } => {
                if let Some(m) = material {
                    self.material(*m)?;
                }
                // Guides take no paint.
                let (_, entities) = &self.split_guides(*context, entities)?;
                self.occurrences_of(*context, entities)?;
                for o in &mut self.assembly_mut(*context)?.occurrences {
                    if entities.contains(&o.id) {
                        o.material = *material;
                    }
                }
                Ok(Outcome::default())
            }
            Operation::PaintFaces { part, faces, material } => {
                if let Some(m) = material {
                    self.material(*m)?;
                }
                let count = self.part(*part)?.shape().record().faces.len() as u32;
                if faces.is_empty() || faces.iter().any(|f| *f >= count) {
                    return Err(Error::Invalid(Refusal::NoSuchFace));
                }
                let p = self.part_mut(*part)?;
                for f in faces {
                    match material {
                        Some(m) => p.faces.insert(*f, *m),
                        None => p.faces.remove(f),
                    };
                }
                Ok(Outcome::default())
            }
            Operation::AddGuide { component, shape } => {
                self.component(*component)?;
                if !shape.is_valid() {
                    return Err(Error::Invalid(Refusal::BadGuide));
                }
                let id = self.fresh_id();
                self.component_mut(*component)?.guides.push(Guide { id, shape: *shape });
                Ok(Outcome { guide: Some(id), ..Outcome::default() })
            }
            Operation::AddPrimitive { context, primitive, placement } => {
                self.assembly(*context)?;
                if !is_rigid(placement) {
                    return Err(Error::Invalid(Refusal::PlacementNotRigid));
                }
                let (local, at) = primitive.at_origin();
                let blank = solver.primitive(&local)?;
                let part = self.insert_component(&t!("default_name.part"), false, ComponentKind::Part(Part::new(blank)));
                let id = self.fresh_id();
                let transform = *placement * DAffine3::from_translation(at);
                self.assembly_mut(*context)?.occurrences.push(Occurrence {
                    id,
                    name: String::new(),
                    component: part,
                    transform,
                    material: None,
                });
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::Transform { context, entities, transform } => {
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (guides, entities) = &self.split_guides(*context, entities)?;
                self.occurrences_of(*context, entities)?;
                for o in &mut self.assembly_mut(*context)?.occurrences {
                    if entities.contains(&o.id) {
                        o.transform = *transform * o.transform;
                    }
                }
                self.move_guides(*context, guides, transform)?;
                Ok(Outcome::default())
            }
            Operation::Duplicate { context, entities, transform } => {
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (guides, entities) = &self.split_guides(*context, entities)?;
                let sources = self.occurrences_of(*context, entities)?;
                let mut created = self.add_copies(*context, &sources, transform)?;
                created.extend(self.copy_guides(*context, guides, &[*transform])?);
                Ok(Outcome { created, ..Outcome::default() })
            }
            Operation::Array { context, entities, step, count } => {
                if *count == 0 || *count > MAX_ARRAY {
                    return Err(Error::Invalid(Refusal::ArrayCount));
                }
                if !is_rigid(step) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (guides, entities) = &self.split_guides(*context, entities)?;
                let sources = self.occurrences_of(*context, entities)?;
                let mut at = DAffine3::IDENTITY;
                let mut created = Vec::new();
                let mut steps = Vec::new();
                for _ in 0..*count {
                    at = *step * at;
                    created.extend(self.add_copies(*context, &sources, &at)?);
                    steps.push(at);
                }
                created.extend(self.copy_guides(*context, guides, &steps)?);
                Ok(Outcome { created, ..Outcome::default() })
            }
            Operation::Delete { context, entities } => {
                let (guides, entities) = &self.split_guides(*context, entities)?;
                self.occurrences_of(*context, entities)?;
                self.assembly_mut(*context)?.occurrences.retain(|o| !entities.contains(&o.id));
                self.component_mut(*context)?.guides.retain(|g| !guides.contains(&g.id));
                Ok(Outcome::default())
            }
            Operation::RenameOccurrence { context, occurrence, name } => {
                self.occurrences_of(*context, &[*occurrence])?;
                if let Some(o) = self.assembly_mut(*context)?.occurrence_mut(*occurrence) {
                    o.name = name.clone();
                }
                Ok(Outcome::default())
            }
            Operation::RenameComponent { component, name } => {
                if *component == self.root {
                    return Err(Error::Invalid(Refusal::RootRenamed));
                }
                self.component(*component)?;
                if self.components().any(|c| c.id != *component && c.name == *name) {
                    return Err(Error::Invalid(Refusal::NameTaken));
                }
                let c = self.component_mut(*component)?;
                c.name = name.clone();
                c.named = true;
                Ok(Outcome::default())
            }
            Operation::MakeAssembly { context, entities, name } => {
                // The guides go along, where they stand (the new occurrence is not moved).
                let (guides, entities) = &self.split_guides(*context, entities)?;
                if entities.is_empty() {
                    return Err(Error::Invalid(Refusal::EmptyAssembly));
                }
                let moved = self.occurrences_of(*context, entities)?;
                let base = name.clone().unwrap_or_else(|| t!("default_name.assembly"));
                let assembly = self.insert_component(&base, name.is_some(), ComponentKind::Assembly(Assembly { occurrences: moved }));
                let taken: Vec<Guide> = self.component(*context)?.guides.iter().filter(|g| guides.contains(&g.id)).cloned().collect();
                self.component_mut(*context)?.guides.retain(|g| !guides.contains(&g.id));
                self.component_mut(assembly)?.guides = taken;
                let id = self.fresh_id();
                let ctx = self.assembly_mut(*context)?;
                // The new occurrence stands where the first moved one stood, in the tree.
                let at = ctx.occurrences.iter().position(|o| entities.contains(&o.id)).unwrap_or(ctx.occurrences.len());
                ctx.occurrences.retain(|o| !entities.contains(&o.id));
                let at = at.min(ctx.occurrences.len());
                ctx.occurrences
                    .insert(at, Occurrence { id, name: String::new(), component: assembly, transform: DAffine3::IDENTITY, material: None });
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::Explode { context, occurrence } => {
                let o = self.occurrences_of(*context, &[*occurrence])?.remove(0);
                let contents = self.assembly(o.component).map_err(|_| Error::Invalid(Refusal::NotExploded))?.occurrences.clone();
                let mut copies = Vec::with_capacity(contents.len());
                for c in contents {
                    let id = self.fresh_id();
                    copies.push(Occurrence { id, transform: o.transform * c.transform, ..c });
                }
                let created = copies.iter().map(|c| c.id).collect();
                // Its guides stay where they were, in the context.
                let guides = self.component(o.component)?.guides.clone();
                let guides: Vec<Guide> =
                    guides.iter().map(|g| Guide { id: self.fresh_id(), shape: g.shape.transformed(&o.transform) }).collect();
                self.component_mut(*context)?.guides.extend(guides);
                let ctx = self.assembly_mut(*context)?;
                let at = ctx.position(*occurrence).ok_or(Error::NotFound(*occurrence))?;
                ctx.occurrences.splice(at..=at, copies);
                Ok(Outcome { created, ..Outcome::default() })
            }
            Operation::MakeUnique { context, occurrence } => {
                let o = self.occurrences_of(*context, &[*occurrence])?.remove(0);
                if self.occurrence_count(o.component) < 2 {
                    return Ok(Outcome::default());
                }
                let copy = self.copy_component(o.component)?;
                if let Some(o) = self.assembly_mut(*context)?.occurrence_mut(*occurrence) {
                    o.component = copy;
                }
                Ok(Outcome::default())
            }
            Operation::PlaceOccurrence { context, component, transform } => {
                if *component == self.root {
                    return Err(Error::Invalid(Refusal::RootPlaced));
                }
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                self.component(*component)?;
                self.assembly(*context)?;
                if self.contains_component(*component, *context) {
                    return Err(Error::Cycle);
                }
                let id = self.fresh_id();
                self.assembly_mut(*context)?.occurrences.push(Occurrence {
                    id,
                    name: String::new(),
                    component: *component,
                    transform: *transform,
                    material: None,
                });
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::Boolean { target, tool, op, scope } => {
                if *scope == Scope::This {
                    self.separate(target)?;
                }
                self.boolean(target, tool, *op, solver)
            }
            Operation::AddMachining { target, primitive, placement, scope } => {
                if !is_rigid(placement) {
                    return Err(Error::Invalid(Refusal::PlacementNotRigid));
                }
                self.resolve_part(target)?;
                if *scope == Scope::This {
                    self.separate(target)?;
                }
                let (part, _, _) = self.resolve_part(target)?;
                let (local, at) = primitive.at_origin();
                let tool = solver.primitive(&local)?;
                let id = self.fresh_id();
                let placement = *placement * DAffine3::from_translation(at);
                self.part_mut(part)?.machinings.push(Machining { id, tool, placement, source: MachiningSource::Local });
                self.recompute(part, Some(&[id]), solver)?;
                Ok(Outcome { machinings: vec![id], ..Outcome::default() })
            }
            Operation::DeleteMachinings { target, machinings, scope } => {
                let (part, guides, machinings) = &self.machined_part(target, machinings, *scope)?;
                self.component_mut(*part)?.guides.retain(|g| !guides.contains(&g.id));
                if !machinings.is_empty() {
                    self.part_mut(*part)?.machinings.retain(|m| !machinings.contains(&m.id));
                    self.recompute(*part, None, solver)?;
                }
                Ok(Outcome::default())
            }
            Operation::TransformMachinings { target, machinings, transform, scope } => {
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (part, guides, machinings) = self.machined_part(target, machinings, *scope)?;
                self.move_guides(part, &guides, transform)?;
                if !machinings.is_empty() {
                    for m in &mut self.part_mut(part)?.machinings {
                        if machinings.contains(&m.id) {
                            m.placement = *transform * m.placement;
                        }
                    }
                    self.recompute(part, None, solver)?;
                }
                Ok(Outcome::default())
            }
            Operation::DuplicateMachinings { target, machinings, transform, scope } => {
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (part, guides, machinings) = self.machined_part(target, machinings, *scope)?;
                let mut created = self.copy_machinings(part, &machinings, &[*transform])?;
                if !created.is_empty() {
                    self.recompute(part, Some(&created), solver)?;
                }
                created.extend(self.copy_guides(part, &guides, &[*transform])?);
                Ok(Outcome { machinings: created, ..Outcome::default() })
            }
            Operation::ArrayMachinings { target, machinings, step, count, scope } => {
                if *count == 0 || *count > MAX_ARRAY {
                    return Err(Error::Invalid(Refusal::ArrayCount));
                }
                if !is_rigid(step) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (part, guides, machinings) = self.machined_part(target, machinings, *scope)?;
                let steps: Vec<DAffine3> = std::iter::successors(Some(*step), |at| Some(*step * *at)).take(*count as usize).collect();
                let mut created = self.copy_machinings(part, &machinings, &steps)?;
                if !created.is_empty() {
                    self.recompute(part, Some(&created), solver)?;
                }
                created.extend(self.copy_guides(part, &guides, &steps)?);
                Ok(Outcome { machinings: created, ..Outcome::default() })
            }
            Operation::Stretch { context, occurrences, frame, axis, cuts, moves, scope } => {
                let stretch = Stretch::new(*axis, cuts.clone(), moves.clone()).map_err(solidsketch_kernel::Error::from)?;
                let stretched = self
                    .stretch_tree(context, occurrences, frame, &stretch, *scope, &mut |doc, part, s| doc.stretch_part(part, s, solver))?;
                Ok(Outcome { stretched, ..Outcome::default() })
            }
        }
    }

    /// Stretches `part` (§6 sexies): its blank, its machinings — moved when in one section,
    /// their tool stretched in its own frame across a cutter — and its guides; then computes it
    /// again from the blank.
    fn stretch_part(&mut self, part: Id, stretch: &Stretch, solver: &mut dyn Solver) -> Result<()> {
        let p = self.part(part)?.clone();
        let blank = solver.stretch(&p.blank, stretch)?;
        let mut machinings = Vec::with_capacity(p.machinings.len());
        for m in &p.machinings {
            let local = stretch.in_frame(&m.placement);
            let rigid = local.extent(&m.tool).map_or(Some(0.0), |(lo, hi)| local.rigid_move(lo, hi));
            machinings.push(match rigid {
                Some(d) => Machining { placement: DAffine3::from_translation(stretch.axis() * d) * m.placement, ..m.clone() },
                None => Machining { tool: solver.stretch(&m.tool, &local)?, ..m.clone() },
            });
        }
        self.stretch_guides(part, stretch)?;
        let p = self.part_mut(part)?;
        p.blank = blank;
        p.machinings = machinings;
        self.recompute(part, None, solver)
    }

    /// The part `target` places, `ids` (at least one) split into its guides and its
    /// machinings, every one in it; made its own first under the scope "only this one" when
    /// machinings change (they and the guides keep their ids, §2.6) — guides alone ignore the
    /// scope (§6 quinquies).
    fn machined_part(&mut self, target: &EntityPath, ids: &[Id], scope: Scope) -> Result<(Id, Vec<Id>, Vec<Id>)> {
        if ids.is_empty() {
            return Err(Error::Invalid(Refusal::NoMachinings));
        }
        let (part, p, _) = self.resolve_part(target)?;
        let p = p.clone();
        let (guides, machinings) = self.split_guides(part, ids)?;
        if let Some(&missing) = machinings.iter().find(|id| p.machining(**id).is_none()) {
            return Err(Error::NotFound(missing));
        }
        if scope == Scope::This && !machinings.is_empty() {
            self.separate(target)?;
            return Ok((self.resolve_part(target)?.0, guides, machinings));
        }
        Ok((part, guides, machinings))
    }

    /// `ids` split into the guides of `component` and the rest, each in the order given.
    fn split_guides(&self, component: Id, ids: &[Id]) -> Result<(Vec<Id>, Vec<Id>)> {
        let c = self.component(component)?;
        Ok(ids.iter().partition(|id| c.guide(**id).is_some()))
    }

    /// Moves the guides `ids` of `component` by the rigid `t`, in its axes.
    fn move_guides(&mut self, component: Id, ids: &[Id], t: &DAffine3) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        for g in &mut self.component_mut(component)?.guides {
            if ids.contains(&g.id) {
                g.shape = g.shape.transformed(t);
            }
        }
        Ok(())
    }

    /// Adds to `component`, for each transform of `steps`, a copy of its guides `ids` moved by
    /// it, with fresh ids; returns them in order.
    fn copy_guides(&mut self, component: Id, ids: &[Id], steps: &[DAffine3]) -> Result<Vec<Id>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let sources: Vec<Guide> = self.component(component)?.guides.iter().filter(|g| ids.contains(&g.id)).cloned().collect();
        let mut copies = Vec::with_capacity(sources.len() * steps.len());
        for t in steps {
            for g in &sources {
                copies.push(Guide { id: self.fresh_id(), shape: g.shape.transformed(t) });
            }
        }
        let created = copies.iter().map(|g| g.id).collect();
        self.component_mut(component)?.guides.extend(copies);
        Ok(created)
    }

    /// Adds to `part`, for each transform of `steps`, a copy of its machinings `ids` moved by
    /// it (in the part's axes), with fresh ids; returns them in order.
    fn copy_machinings(&mut self, part: Id, ids: &[Id], steps: &[DAffine3]) -> Result<Vec<Id>> {
        let sources: Vec<Machining> = self.part(part)?.machinings.iter().filter(|m| ids.contains(&m.id)).cloned().collect();
        let mut copies = Vec::with_capacity(sources.len() * steps.len());
        for t in steps {
            for m in &sources {
                copies.push(Machining { id: self.fresh_id(), placement: *t * m.placement, ..m.clone() });
            }
        }
        let created = copies.iter().map(|m| m.id).collect();
        self.part_mut(part)?.machinings.extend(copies);
        Ok(created)
    }

    /// Computes `part`'s shape again from its recipe (§2.4): only the machinings `added` cut
    /// from the shape as it is, when given (they were just added); else all of them from the
    /// blank. Without machinings, the blank is the shape: no kernel.
    fn recompute(&mut self, part: Id, added: Option<&[Id]>, solver: &mut dyn Solver) -> Result<()> {
        let p = self.part(part)?;
        if p.machinings.is_empty() {
            let p = self.part_mut(part)?;
            p.shape = None;
            p.failed.clear();
            return Ok(());
        }
        let (start, cut): (Arc<Solid>, Vec<&Machining>) = match added {
            Some(ids) => (p.shape().clone(), p.machinings.iter().filter(|m| ids.contains(&m.id)).collect()),
            None => (p.blank.clone(), p.machinings.iter().collect()),
        };
        let tools: Vec<(Arc<Solid>, DAffine3)> = cut.iter().map(|m| (m.tool.clone(), m.placement)).collect();
        let ids: Vec<Id> = cut.iter().map(|m| m.id).collect();
        let (shape, failed) = solver.recipe(&start, &tools)?;
        let p = self.part_mut(part)?;
        // From the blank, the failures are all new; else those of the machinings added join.
        if added.is_none() {
            p.failed.clear();
        }
        p.failed.extend(failed.iter().filter_map(|k| ids.get(*k).copied()));
        p.shape = Some(shape);
        Ok(())
    }

    fn boolean(&mut self, target: &EntityPath, tool: &EntityPath, op: Boolean, solver: &mut dyn Solver) -> Result<Outcome> {
        if target == tool {
            return Err(Error::Invalid(Refusal::SelfBoolean));
        }
        let (part, p, t_target) = self.resolve_part(target)?;
        let p = p.clone();
        let (_, q, t_tool) = self.resolve_part(tool)?;
        let tool_shape = q.shape().clone();
        let tool_ctx = self.context_of(&tool.instances)?;
        // The tool, in the target part's axes.
        let relative = t_target.inverse() * t_tool;
        let moved = if is_identity(&relative) { None } else { Some(relative) };
        let result = solver.boolean(p.shape(), &tool_shape, moved, op.into())?;
        let next = match op {
            Boolean::Difference => {
                let id = self.fresh_id();
                let mut next = p;
                next.machinings.push(Machining { id, tool: tool_shape, placement: relative, source: MachiningSource::Local });
                next.shape = Some(result);
                next
            }
            // A new blank, the machinings of both in it.
            Boolean::Union | Boolean::Intersection => Part::new(result),
        };
        *self.part_mut(part)? = next;
        self.assembly_mut(tool_ctx)?.occurrences.retain(|o| o.id != tool.entity);
        Ok(Outcome { created: vec![target.entity], ..Outcome::default() })
    }

    /// Makes what `path` names its own (§2.5, scope "only this one"): top down, every
    /// component on the way shown by other occurrences too gets a copy of its own, the
    /// occurrence repointed to it. The copies keep their occurrences' ids (unique within an
    /// assembly, §2.6): `path` stays valid, and names the same thing.
    pub(crate) fn separate(&mut self, path: &EntityPath) -> Result<()> {
        let mut at = self.root;
        for &i in path.instances.iter().chain(std::iter::once(&path.entity)) {
            let o = self.assembly(at)?.occurrence(i).ok_or(Error::NotFound(i))?.component;
            let own = if self.occurrence_count(o) > 1 {
                let copy = self.copy_component(o)?;
                if let Some(occ) = self.assembly_mut(at)?.occurrence_mut(i) {
                    occ.component = copy;
                }
                copy
            } else {
                o
            };
            at = own;
        }
        Ok(())
    }

    /// Clones of the occurrences `ids` of `context`, in the context's order; every id must be
    /// there.
    fn occurrences_of(&self, context: Id, ids: &[Id]) -> Result<Vec<Occurrence>> {
        let a = self.assembly(context)?;
        if let Some(&missing) = ids.iter().find(|id| a.occurrence(**id).is_none()) {
            return Err(Error::NotFound(missing));
        }
        Ok(a.occurrences.iter().filter(|o| ids.contains(&o.id)).cloned().collect())
    }

    /// Adds copies of `sources` (fresh ids, same components) to `context`, moved by `t`.
    fn add_copies(&mut self, context: Id, sources: &[Occurrence], t: &DAffine3) -> Result<Vec<Id>> {
        let mut copies = Vec::with_capacity(sources.len());
        for s in sources {
            let id = self.fresh_id();
            copies.push(Occurrence { id, transform: *t * s.transform, ..s.clone() });
        }
        let created = copies.iter().map(|c| c.id).collect();
        self.assembly_mut(context)?.occurrences.extend(copies);
        Ok(created)
    }
}

fn is_identity(t: &DAffine3) -> bool {
    t.abs_diff_eq(DAffine3::IDENTITY, 1e-12)
}

/// A move, a rotation and maybe a mirror: unit, square axes (§2.2).
pub(crate) fn is_rigid(t: &DAffine3) -> bool {
    let m = t.matrix3;
    let unit = |v: DVec3| (v.length_squared() - 1.0).abs() < 1e-9;
    unit(m.x_axis)
        && unit(m.y_axis)
        && unit(m.z_axis)
        && m.x_axis.dot(m.y_axis).abs() < 1e-9
        && m.y_axis.dot(m.z_axis).abs() < 1e-9
        && m.z_axis.dot(m.x_axis).abs() < 1e-9
        && t.translation.is_finite()
}

// ---- the kernel calls of an operation -------------------------------------------------------

/// The kernel, as an operation sees it.
trait Solver {
    fn primitive(&mut self, primitive: &Primitive) -> Result<Arc<Solid>>;
    /// `target op tool`, the tool first moved by `tool_transform`.
    fn boolean(&mut self, target: &Arc<Solid>, tool: &Arc<Solid>, tool_transform: Option<DAffine3>, op: BooleanOp) -> Result<Arc<Solid>>;
    /// `blank` minus each placed tool in turn; the solid, and the tools (by index) that failed.
    fn recipe(&mut self, blank: &Arc<Solid>, tools: &[(Arc<Solid>, DAffine3)]) -> Result<(Arc<Solid>, Vec<usize>)>;
    /// `solid` stretched.
    fn stretch(&mut self, solid: &Arc<Solid>, stretch: &Stretch) -> Result<Arc<Solid>>;
}

/// Records the calls into a job and answers with stand-ins (the operation only stores them,
/// or hands them to a later call: they then name that task's output, [`EARLIER`]).
#[derive(Default)]
struct Recorder {
    job: Job,
    /// Inputs already in the job, and the stand-ins handed out (as `EARLIER | task`), by
    /// `Arc` address.
    inputs: HashMap<*const Solid, u32>,
    /// Held so their addresses stay theirs.
    stand_ins: Vec<Arc<Solid>>,
}

impl Recorder {
    fn input(&mut self, s: &Arc<Solid>) -> u32 {
        *self.inputs.entry(Arc::as_ptr(s)).or_insert_with(|| {
            self.job.inputs.push(s.clone());
            (self.job.inputs.len() - 1) as u32
        })
    }

    fn task(&mut self, task: Task) -> Result<Arc<Solid>> {
        let at = self.job.tasks.len() as u32;
        self.job.tasks.push(task);
        let stand_in = Arc::new(Solid::empty());
        self.inputs.insert(Arc::as_ptr(&stand_in), EARLIER | at);
        self.stand_ins.push(stand_in.clone());
        Ok(stand_in)
    }
}

impl Solver for Recorder {
    fn primitive(&mut self, primitive: &Primitive) -> Result<Arc<Solid>> {
        self.task(Task::Primitive { primitive: primitive.clone(), placement: DAffine3::IDENTITY })
    }

    fn boolean(&mut self, target: &Arc<Solid>, tool: &Arc<Solid>, tool_transform: Option<DAffine3>, op: BooleanOp) -> Result<Arc<Solid>> {
        let (target, tool) = (self.input(target), self.input(tool));
        self.task(Task::Boolean { target, tool, tool_transform, op })
    }

    fn recipe(&mut self, blank: &Arc<Solid>, tools: &[(Arc<Solid>, DAffine3)]) -> Result<(Arc<Solid>, Vec<usize>)> {
        let blank = self.input(blank);
        let tools = tools.iter().map(|(t, p)| (self.input(t), *p)).collect();
        Ok((self.task(Task::Recipe { blank, tools })?, Vec::new()))
    }

    /// Refused here already when the record cannot be stretched: the operation is refused when
    /// prepared, not when the kernel answers.
    fn stretch(&mut self, solid: &Arc<Solid>, stretch: &Stretch) -> Result<Arc<Solid>> {
        stretch.apply(solid).map_err(solidsketch_kernel::Error::from)?;
        let solid = self.input(solid);
        self.task(Task::Stretch { solid, stretch: stretch.clone() })
    }
}

/// Hands out the job's outputs, in the order the calls come (the order they were recorded).
struct Replayer {
    outputs: std::vec::IntoIter<Output>,
    displays: Vec<(Arc<Solid>, std::result::Result<Display, String>)>,
}

impl Replayer {
    fn next(&mut self) -> Result<Arc<Solid>> {
        match self.outputs.next() {
            Some(Output::Solid { solid, display }) => {
                let solid = Arc::new(solid);
                self.displays.push((solid.clone(), display));
                Ok(solid)
            }
            _ => Err(Error::Invalid(Refusal::OutOfStep)),
        }
    }
}

impl Solver for Replayer {
    fn primitive(&mut self, _: &Primitive) -> Result<Arc<Solid>> {
        self.next()
    }

    fn boolean(&mut self, _: &Arc<Solid>, _: &Arc<Solid>, _: Option<DAffine3>, _: BooleanOp) -> Result<Arc<Solid>> {
        self.next()
    }

    fn recipe(&mut self, _: &Arc<Solid>, _: &[(Arc<Solid>, DAffine3)]) -> Result<(Arc<Solid>, Vec<usize>)> {
        match self.outputs.next() {
            Some(Output::Recipe { solid, display, failed }) => {
                let solid = Arc::new(solid);
                self.displays.push((solid.clone(), display));
                Ok((solid, failed.into_iter().map(|k| k as usize).collect()))
            }
            _ => Err(Error::Invalid(Refusal::OutOfStep)),
        }
    }

    fn stretch(&mut self, _: &Arc<Solid>, _: &Stretch) -> Result<Arc<Solid>> {
        self.next()
    }
}

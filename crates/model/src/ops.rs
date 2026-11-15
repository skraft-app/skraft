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

use crate::clipboard::BodyFragment;
use crate::document::{
    Assembly, Behavior, Body, BodyDef, ComponentKind, Document, EntityPath, Guide, GuideShape, Id, Material, Occurrence, Part, Sign,
    is_identity,
};
use crate::settings::Settings;
use crate::stretch::BodyStretch;
use crate::{Error, FastSet, Refusal, Result};

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
/// copying and grouping take too, as the operations on bodies take the part's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Operation {
    /// A new part built from a primitive, and its first occurrence in `context`: its positive
    /// body in the part's own axes, the occurrence placed by the rigid `placement` (a box drawn on a
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
    /// Copies of occurrences of `context` (or its guides), one set per placement: each copy at
    /// `placement · T`, `T` the source's as it was; the sources themselves moved by `moved`
    /// (identity but for Distribute, which moves them to the first slot). Copy along a line or
    /// a grid, Distribute (ARCHITECTURE.md §6 duodecies). Rigid placements only.
    Copies {
        context: Id,
        entities: Vec<Id>,
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        moved: DAffine3,
        placements: Vec<DAffine3>,
    },
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
    /// (`scope`, §2.5). A difference adds the tool's shape as a negative body; a union gives
    /// the target all the tool's bodies (§2.11); an intersection makes a new part of one
    /// positive body. The tool's occurrence leaves the model.
    Boolean {
        target: EntityPath,
        tool: EntityPath,
        op: Boolean,
        #[serde(default)]
        scope: Scope,
    },
    /// A primitive added to the part `target` places, as a new body of `sign` (drawn in an
    /// open part, ARCHITECTURE.md §2.9, §2.11 — a negative one is a machining): built in its
    /// own axes, placed by the rigid `placement` in the part's, as [`Operation::AddPrimitive`]
    /// places a part in its context.
    AddBody {
        target: EntityPath,
        primitive: Primitive,
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        placement: DAffine3,
        sign: Sign,
        #[serde(default)]
        scope: Scope,
    },
    /// Removes bodies from the part `target` places (a positive one is left); the part is
    /// computed again.
    DeleteBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        #[serde(default)]
        scope: Scope,
    },
    /// Gives each of the bodies of the part `target` places that shares its definition a copy of
    /// its own (§2.11): a change to its shape no longer shows in the others.
    MakeBodiesUnique {
        target: EntityPath,
        bodies: Vec<Id>,
        #[serde(default)]
        scope: Scope,
    },
    /// Gives bodies of the part `target` places the sign `sign` (a blank becomes a machining,
    /// or the reverse; a positive one is left); the part is computed again.
    SetSign {
        target: EntityPath,
        bodies: Vec<Id>,
        sign: Sign,
        #[serde(default)]
        scope: Scope,
    },
    /// Moves (rotates, mirrors) bodies of the part `target` places by `transform`, in the
    /// part's axes; the part is computed again.
    TransformBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        transform: DAffine3,
        #[serde(default)]
        scope: Scope,
    },
    /// Copies bodies of the part `target` places, moved by `transform` in its axes: the
    /// copies are added to the part as it is.
    DuplicateBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        transform: DAffine3,
        #[serde(default)]
        scope: Scope,
    },
    /// `count` copies of bodies of the part `target` places, the k-th moved by `step`
    /// applied k times, in its axes.
    ArrayBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        step: DAffine3,
        count: u32,
        #[serde(default)]
        scope: Scope,
    },
    /// `Copies` for bodies of the part `target` places (or its guides), in its axes: one copy
    /// per placement, sharing its definition, the sources moved by `moved`.
    CopyBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        moved: DAffine3,
        placements: Vec<DAffine3>,
        #[serde(default)]
        scope: Scope,
    },
    /// Stretches what the occurrences `occurrences` of the assembly `context` leads into place
    /// — parts or assemblies — along `axis`, in the axes `frame` places in the context's (its
    /// own, the context's or the model's, ARCHITECTURE.md §6 sexies): the cutters at `cuts`
    /// (increasing positions along the axis, as `axis · p`) split them into sections, section
    /// `i` moved by `moves[i]` along the axis. What lies in one section is moved; a part across
    /// a cutter has each body in one section moved with it, each one across a cutter stretched
    /// in its own axes, its guides following their section, then is computed again; an
    /// assembly across a cutter is walked, each component stretched in its
    /// own axes (`stretch.rs`).
    ///
    /// `cutters`, with one occurrence: the cutters its component (as it is after the stretch)
    /// takes, in its own axes ([`Behavior::cutters`]) — the tool's, set again after the
    /// stretch; its locks are kept.
    Stretch {
        context: Vec<Id>,
        occurrences: Vec<Id>,
        frame: DAffine3,
        axis: DVec3,
        cuts: Vec<f64>,
        moves: Vec<f64>,
        #[serde(default)]
        scope: Scope,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cutters: Option<Box<[Vec<f64>; 3]>>,
    },
    /// Stretches bodies of the part `target` places, each in its own axes (§2.11, §6 sexies),
    /// and its guides among `bodies`: along `axis`, in the axes `frame` places in the part's,
    /// the cutters at `cuts` splitting them into sections moved by `moves` (as
    /// [`Operation::Stretch`]). A body in one section is moved, one across a cutter stretched
    /// (its profile kept when it stays one); then the part is computed again.
    StretchBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        frame: DAffine3,
        axis: DVec3,
        cuts: Vec<f64>,
        moves: Vec<f64>,
        #[serde(default)]
        scope: Scope,
    },
    /// Gives the body `body` of the part `target` places a new profile (§6 nonies, §2.11), in
    /// the body's axes: its solid is built again from it, then the part; under the scope,
    /// "only this one" making it a separate part. The body must have a profile already.
    SetProfile {
        target: EntityPath,
        body: Id,
        profile: crate::BodyProfile,
        #[serde(default)]
        scope: Scope,
    },
    /// Sets how `component` is stretched: its locked axes and its cutters (ARCHITECTURE.md
    /// §6 sexies). Every occurrence shows it; no scope. Changes in a row to one component undo
    /// in one go.
    SetBehavior { component: Id, behavior: Behavior },
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
    /// Hides occurrences of `context`, or shows them again (ARCHITECTURE.md §6). An assembly's occurrence
    /// is hidden in every occurrence of the assembly.
    SetHidden { context: Id, entities: Vec<Id>, hidden: bool },
    /// Shows every occurrence hidden, in every assembly.
    ShowAll,
    /// Paints faces of a part's shape, by their index in its record (every occurrence of the
    /// part shows it; `None`: they take the occurrence's material again).
    PaintFaces { part: Id, faces: Vec<u32>, material: Option<Id> },
    /// A construction guide added to `component` (a part or an assembly), in its axes
    /// (ARCHITECTURE.md §6 quinquies). Every occurrence shows it; no scope.
    AddGuide { component: Id, shape: GuideShape },
    /// Places the content of another document in `context` (an imported file,
    /// ARCHITECTURE.md §8 ter): its components and materials copied, ids drawn afresh. What
    /// its root holds comes as one occurrence placed by the rigid `transform`: the root's
    /// single occurrence itself, else a new assembly named as the root.
    Insert { context: Id, fragment: Arc<Document>, transform: DAffine3 },
    /// Places what was copied ([`Document::entities_fragment`], §6 decies) in `context`, moved
    /// by the rigid `transform` from the fragment's world axes into the context's: its
    /// occurrences each on their own, its guides in the context. With `reuse` (pasting into
    /// the document it was copied from, whose ids still name the same things), the components
    /// still here are used as they are — the copies are occurrences of the same parts (§2.5);
    /// the others are grafted as from another document ([`Operation::Insert`]).
    Paste { context: Id, fragment: Arc<Document>, transform: DAffine3, reuse: bool },
    /// Adds to the part `target` places the bodies and guides copied from an open part
    /// ([`Document::bodies_fragment`]), moved by the rigid `transform` in the part's axes;
    /// the copies are added to the part as it is.
    PasteBodies {
        target: EntityPath,
        fragment: Arc<BodyFragment>,
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        transform: DAffine3,
        #[serde(default)]
        scope: Scope,
    },
}

impl Operation {
    /// A short label for menus and the undo history.
    pub fn label(&self) -> String {
        match self {
            Operation::AddPrimitive { .. } => t!("operation.add_part"),
            Operation::Transform { .. } => t!("operation.transform"),
            Operation::Duplicate { .. } => t!("operation.duplicate"),
            Operation::Array { .. } => t!("operation.array"),
            Operation::Copies { moved, .. } | Operation::CopyBodies { moved, .. } if !is_identity(moved) => {
                t!("operation.distribute")
            }
            Operation::Copies { .. } | Operation::CopyBodies { .. } => t!("operation.copies"),
            Operation::Delete { .. } => t!("operation.delete"),
            Operation::RenameOccurrence { .. } | Operation::RenameComponent { .. } => t!("operation.rename"),
            Operation::MakeAssembly { .. } => t!("operation.make_assembly"),
            Operation::Explode { .. } => t!("operation.explode"),
            Operation::MakeUnique { .. } | Operation::MakeBodiesUnique { .. } => t!("operation.make_unique"),
            Operation::PlaceOccurrence { .. } => t!("operation.place"),
            Operation::Boolean { op: Boolean::Union, .. } => t!("operation.union"),
            Operation::Boolean { op: Boolean::Difference, .. } => t!("operation.difference"),
            Operation::Boolean { op: Boolean::Intersection, .. } => t!("operation.intersection"),
            Operation::SetSettings(_) => t!("operation.settings"),
            Operation::AddMaterial { .. } => t!("operation.add_material"),
            Operation::EditMaterial { .. } => t!("operation.edit_material"),
            Operation::DeleteMaterial { .. } => t!("operation.delete_material"),
            Operation::PaintOccurrences { .. } | Operation::PaintFaces { .. } => t!("operation.paint"),
            Operation::SetHidden { hidden: true, .. } => t!("operation.hide"),
            Operation::SetHidden { hidden: false, .. } => t!("operation.show"),
            Operation::ShowAll => t!("operation.show_all"),
            Operation::AddBody { sign: Sign::Negative, .. } => t!("operation.add_machining"),
            Operation::AddBody { sign: Sign::Positive, .. } => t!("operation.add_body"),
            Operation::DeleteBodies { .. } => t!("operation.delete"),
            Operation::SetSign { sign: Sign::Positive, .. } => t!("operation.set_sign_positive"),
            Operation::SetSign { sign: Sign::Negative, .. } => t!("operation.set_sign_negative"),
            Operation::TransformBodies { .. } => t!("operation.transform"),
            Operation::DuplicateBodies { .. } => t!("operation.duplicate"),
            Operation::ArrayBodies { .. } => t!("operation.array"),
            Operation::AddGuide { .. } => t!("operation.add_guide"),
            Operation::Insert { .. } => t!("operation.import"),
            Operation::Paste { .. } | Operation::PasteBodies { .. } => t!("operation.paste"),
            Operation::Stretch { .. } | Operation::StretchBodies { .. } => t!("operation.stretch"),
            Operation::SetBehavior { .. } => t!("operation.stretch_behavior"),
            Operation::SetProfile { .. } => t!("operation.set_profile"),
        }
    }

    /// Does this step fold into the previous one `prev` in the undo history? (Settings
    /// changed in a row, e.g. while dragging a colour, undo in one go.)
    pub fn merges_into(&self, prev: &Operation) -> bool {
        match (self, prev) {
            (Operation::SetSettings(_), Operation::SetSettings(_)) => true,
            (Operation::EditMaterial { material: a, .. }, Operation::EditMaterial { material: b, .. }) => a == b,
            (Operation::SetBehavior { component: a, .. }, Operation::SetBehavior { component: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// The most copies an `Operation::Array` makes, the most placements of an `Operation::Copies`.
pub const MAX_ARRAY: u32 = 1000;

/// What an operation did, for the caller to update its selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Occurrences created, in the operation's context (the target's, for a boolean).
    pub created: Vec<Id>,
    /// Bodies (and guides, pasted with them) created, in the target's part (as it is after
    /// the operation).
    pub bodies: Vec<Id>,
    /// The material created.
    pub material: Option<Id>,
    /// The guide created, in the operation's component.
    pub guide: Option<Id>,
    /// The parts a stretch changed, and how (§6 sexies).
    pub stretched: Vec<crate::StretchedPart>,
    /// Done, but the user should know something about it (the status bar says it).
    pub warning: Option<Warning>,
}

/// What an operation done has to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    /// A union whose tool's machinings (negative bodies) cut the target too (§2.11).
    UnionCut,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Warning::UnionCut => t!("warning.union_cut"),
        })
    }
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
                        ComponentKind::Assembly(a) => a.occurrences().iter().any(|o| o.material == Some(*material)),
                    })
                    .map(|c| c.id)
                    .collect();
                for id in users {
                    match &mut self.component_mut(id)?.kind {
                        ComponentKind::Part(p) => p.faces.retain(|_, m| m != material),
                        ComponentKind::Assembly(a) => {
                            for o in a.occurrences_mut() {
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
                let entities = id_set(entities);
                for o in self.assembly_mut(*context)?.occurrences_mut() {
                    if entities.contains(&o.id) {
                        o.material = *material;
                    }
                }
                Ok(Outcome::default())
            }
            Operation::SetHidden { context, entities, hidden } => {
                // Guides are not hidden one by one.
                let (_, entities) = &self.split_guides(*context, entities)?;
                self.occurrences_of(*context, entities)?;
                let entities = id_set(entities);
                // The assembly copied only when something changes.
                if self.assembly(*context)?.occurrences().iter().any(|o| entities.contains(&o.id) && o.hidden != *hidden) {
                    for o in self.assembly_mut(*context)?.occurrences_mut() {
                        if entities.contains(&o.id) {
                            o.hidden = *hidden;
                        }
                    }
                }
                Ok(Outcome::default())
            }
            Operation::ShowAll => {
                let holding: Vec<Id> = self
                    .components
                    .values()
                    .filter(|c| c.as_assembly().is_some_and(|a| a.occurrences().iter().any(|o| o.hidden)))
                    .map(|c| c.id)
                    .collect();
                for id in holding {
                    for o in self.assembly_mut(id)?.occurrences_mut() {
                        o.hidden = false;
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
            Operation::Insert { context, fragment, transform } => {
                self.assembly(*context)?;
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::PlacementNotRigid));
                }
                let mut top = self.graft(fragment, false)?;
                let placed = match top.len() {
                    0 => return Err(Error::Invalid(Refusal::EmptyAssembly)),
                    1 => {
                        let mut o = top.remove(0);
                        o.transform = *transform * o.transform;
                        o
                    }
                    _ => {
                        let name = fragment.component(fragment.root)?.name.clone();
                        let assembly = self.insert_component(&name, true, ComponentKind::Assembly(Assembly::new(top)));
                        Occurrence {
                            id: self.fresh_id(),
                            name: String::new(),
                            component: assembly,
                            transform: *transform,
                            material: None,
                            hidden: false,
                        }
                    }
                };
                let id = placed.id;
                self.assembly_mut(*context)?.occurrences_mut().push(placed);
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::Paste { context, fragment, transform, reuse } => {
                self.assembly(*context)?;
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let shapes: Vec<GuideShape> = fragment.component(fragment.root)?.guides.iter().map(|g| g.shape).collect();
                let top = self.graft(fragment, *reuse)?;
                if top.is_empty() && shapes.is_empty() {
                    return Err(Error::Invalid(Refusal::NothingPasted));
                }
                // Reused, an assembly may be the context or hold it.
                if top.iter().any(|o| self.contains_component(o.component, *context)) {
                    return Err(Error::Cycle);
                }
                let mut created: Vec<Id> = top.iter().map(|o| o.id).collect();
                let placed = top.into_iter().map(|o| Occurrence { transform: *transform * o.transform, ..o });
                self.assembly_mut(*context)?.occurrences_mut().extend(placed);
                let guides: Vec<Guide> = shapes.iter().map(|s| Guide { id: self.fresh_id(), shape: s.transformed(transform) }).collect();
                created.extend(guides.iter().map(|g| g.id));
                self.component_mut(*context)?.guides.extend(guides);
                Ok(Outcome { created, ..Outcome::default() })
            }
            Operation::PasteBodies { target, fragment, transform, scope } => {
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                if fragment.is_empty() {
                    return Err(Error::Invalid(Refusal::NothingPasted));
                }
                self.resolve_part(target)?;
                if *scope == Scope::This && !fragment.bodies.is_empty() {
                    self.separate(target)?;
                }
                let (part, p, _) = self.resolve_part(target)?;
                // Into the part copied from, its definitions as they are: the bodies pasted are
                // copies of theirs; elsewhere, copies of the definitions.
                let here = |b: &Body| {
                    let from = fragment.defs.iter().find(|d| d.id == b.def);
                    p.def(b.def).is_some_and(|d| from.is_some_and(|f| Arc::ptr_eq(&f.solid, &d.solid) && f.profile == d.profile))
                };
                let (same, other): (Vec<&Body>, Vec<&Body>) = fragment.bodies.iter().partition(|b| here(b));
                let other: Vec<Body> = other.into_iter().cloned().collect();
                let mut copies: Vec<Body> = Vec::with_capacity(fragment.bodies.len());
                for b in same {
                    copies.push(Body { id: self.fresh_id(), placement: *transform * b.placement, ..b.clone() });
                }
                let (defs, rest) = self.copies_of_bodies(&fragment.defs, &other, transform);
                copies.extend(rest);
                let mut created: Vec<Id> = copies.iter().map(|b| b.id).collect();
                if !copies.is_empty() {
                    let p = self.part_mut(part)?;
                    p.defs.extend(defs);
                    p.bodies.extend(copies);
                    self.recompute(part, Some(&created), solver)?;
                }
                let guides: Vec<Guide> =
                    fragment.guides.iter().map(|s| Guide { id: self.fresh_id(), shape: s.transformed(transform) }).collect();
                created.extend(guides.iter().map(|g| g.id));
                self.component_mut(part)?.guides.extend(guides);
                Ok(Outcome { bodies: created, ..Outcome::default() })
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
                let solid = solver.primitive(&local)?;
                // The shapes drawn keep their profile (§6 nonies, step 3).
                let def = BodyDef { profile: crate::BodyProfile::of(&local), ..BodyDef::new(self.fresh_id(), solid) };
                let body = Body::new(self.fresh_id(), def.id, Sign::Positive, DAffine3::IDENTITY);
                let part = Part::of_body(def, body);
                let part = self.insert_component(&t!("default_name.part"), false, ComponentKind::Part(part));
                let id = self.fresh_id();
                let transform = *placement * DAffine3::from_translation(at);
                self.assembly_mut(*context)?.occurrences_mut().push(Occurrence {
                    id,
                    name: String::new(),
                    component: part,
                    transform,
                    material: None,
                    hidden: false,
                });
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::Transform { context, entities, transform } => {
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (guides, entities) = &self.split_guides(*context, entities)?;
                // Changed in place, by position: the assembly's index stays.
                let at = self.positions_of(*context, entities)?;
                let a = self.assembly_mut(*context)?;
                for i in at {
                    let o = a.occurrence_at_mut(i);
                    o.transform = *transform * o.transform;
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
            Operation::Copies { context, entities, moved, placements } => {
                check_copies(moved, placements)?;
                let (guides, entities) = &self.split_guides(*context, entities)?;
                let sources = self.occurrences_of(*context, entities)?;
                let mut created = Vec::new();
                for t in placements {
                    created.extend(self.add_copies(*context, &sources, t)?);
                }
                created.extend(self.copy_guides(*context, guides, placements)?);
                if !is_identity(moved) {
                    let at = self.positions_of(*context, entities)?;
                    let a = self.assembly_mut(*context)?;
                    for i in at {
                        let o = a.occurrence_at_mut(i);
                        o.transform = *moved * o.transform;
                    }
                    self.move_guides(*context, guides, moved)?;
                }
                Ok(Outcome { created, ..Outcome::default() })
            }
            Operation::Delete { context, entities } => {
                let (guides, entities) = &self.split_guides(*context, entities)?;
                self.occurrences_of(*context, entities)?;
                let entities = id_set(entities);
                self.assembly_mut(*context)?.occurrences_mut().retain(|o| !entities.contains(&o.id));
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
                // The guides go along, where they stand.
                let (guides, entities) = &self.split_guides(*context, entities)?;
                if entities.is_empty() {
                    return Err(Error::Invalid(Refusal::EmptyAssembly));
                }
                let moved = self.occurrences_of(*context, entities)?;
                let base = name.clone().unwrap_or_else(|| t!("default_name.assembly"));
                let assembly = self.insert_component(&base, name.is_some(), ComponentKind::Assembly(Assembly::new(moved)));
                // Its axes have their origin at the low corner of its content's bounds: the
                // content is moved by the opposite of that corner, the new occurrence by it, so
                // nothing moves in the context.
                let corner = self.component_bounds(assembly).map_or(DVec3::ZERO, |(lo, _)| lo);
                let to_axes = DAffine3::from_translation(-corner);
                for o in self.assembly_mut(assembly)?.occurrences_mut() {
                    o.transform = to_axes * o.transform;
                }
                let taken: Vec<Guide> = self
                    .component(*context)?
                    .guides
                    .iter()
                    .filter(|g| guides.contains(&g.id))
                    .map(|g| Guide { id: g.id, shape: g.shape.transformed(&to_axes) })
                    .collect();
                self.component_mut(*context)?.guides.retain(|g| !guides.contains(&g.id));
                self.component_mut(assembly)?.guides = taken;
                let id = self.fresh_id();
                let entities = id_set(entities);
                let ctx = self.assembly_mut(*context)?;
                // The new occurrence stands where the first moved one stood, in the tree.
                let at = ctx.occurrences().iter().position(|o| entities.contains(&o.id)).unwrap_or(ctx.occurrences().len());
                ctx.occurrences_mut().retain(|o| !entities.contains(&o.id));
                let at = at.min(ctx.occurrences().len());
                ctx.occurrences_mut().insert(
                    at,
                    Occurrence {
                        id,
                        name: String::new(),
                        component: assembly,
                        transform: DAffine3::from_translation(corner),
                        material: None,
                        hidden: false,
                    },
                );
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::Explode { context, occurrence } => {
                let o = self.occurrences_of(*context, &[*occurrence])?.remove(0);
                let contents = self.assembly(o.component).map_err(|_| Error::Invalid(Refusal::NotExploded))?.occurrences().to_vec();
                let mut copies = Vec::with_capacity(contents.len());
                for c in contents {
                    let id = self.fresh_id();
                    // An assembly hidden leaves its content hidden.
                    copies.push(Occurrence { id, transform: o.transform * c.transform, hidden: c.hidden || o.hidden, ..c });
                }
                let created = copies.iter().map(|c| c.id).collect();
                // Its guides stay where they were, in the context.
                let guides = self.component(o.component)?.guides.clone();
                let guides: Vec<Guide> =
                    guides.iter().map(|g| Guide { id: self.fresh_id(), shape: g.shape.transformed(&o.transform) }).collect();
                self.component_mut(*context)?.guides.extend(guides);
                let ctx = self.assembly_mut(*context)?;
                let at = ctx.position(*occurrence).ok_or(Error::NotFound(*occurrence))?;
                ctx.occurrences_mut().splice(at..=at, copies);
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
                self.assembly_mut(*context)?.occurrences_mut().push(Occurrence {
                    id,
                    name: String::new(),
                    component: *component,
                    transform: *transform,
                    material: None,
                    hidden: false,
                });
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::Boolean { target, tool, op, scope } => {
                if *scope == Scope::This {
                    self.separate(target)?;
                }
                self.boolean(target, tool, *op, solver)
            }
            Operation::AddBody { target, primitive, placement, sign, scope } => {
                if !is_rigid(placement) {
                    return Err(Error::Invalid(Refusal::PlacementNotRigid));
                }
                self.resolve_part(target)?;
                if *scope == Scope::This {
                    self.separate(target)?;
                }
                let (part, _, _) = self.resolve_part(target)?;
                let (local, at) = primitive.at_origin();
                let solid = solver.primitive(&local)?;
                let id = self.fresh_id();
                let placement = *placement * DAffine3::from_translation(at);
                // Drawn, it keeps its profile (§6 nonies).
                let def = BodyDef { profile: crate::BodyProfile::of(&local), ..BodyDef::new(self.fresh_id(), solid) };
                let p = self.part_mut(part)?;
                p.bodies.push(Body::new(id, def.id, *sign, placement));
                p.defs.push(def);
                self.recompute(part, Some(&[id]), solver)?;
                Ok(Outcome { bodies: vec![id], ..Outcome::default() })
            }
            Operation::DeleteBodies { target, bodies, scope } => {
                let (part, guides, bodies) = &self.bodies_of(target, bodies, *scope)?;
                if !bodies.is_empty() && self.part(*part)?.positives().all(|b| bodies.contains(&b.id)) {
                    return Err(Error::Invalid(Refusal::LastPositive));
                }
                self.component_mut(*part)?.guides.retain(|g| !guides.contains(&g.id));
                if !bodies.is_empty() {
                    let p = self.part_mut(*part)?;
                    p.bodies.retain(|b| !bodies.contains(&b.id));
                    p.drop_unused_defs();
                    self.recompute(*part, None, solver)?;
                }
                Ok(Outcome::default())
            }
            Operation::MakeBodiesUnique { target, bodies, scope } => {
                let (part, _, bodies) = self.bodies_of(target, bodies, *scope)?;
                for id in bodies {
                    let p = self.part(part)?;
                    let Some(b) = p.body(id) else { continue };
                    if p.copies_of(b.def) < 2 {
                        continue;
                    }
                    let def = p.def_of(b).clone();
                    let copy = BodyDef { id: self.fresh_id(), ..def };
                    let p = self.part_mut(part)?;
                    if let Some(b) = p.bodies.iter_mut().find(|b| b.id == id) {
                        b.def = copy.id;
                    }
                    p.defs.push(copy);
                }
                Ok(Outcome::default())
            }
            Operation::SetSign { target, bodies, sign, scope } => {
                let (part, _, bodies) = self.bodies_of(target, bodies, *scope)?;
                let p = self.part(part)?;
                let flipped: Vec<Id> = p.bodies.iter().filter(|b| bodies.contains(&b.id) && b.sign != *sign).map(|b| b.id).collect();
                if flipped.is_empty() {
                    return Ok(Outcome::default());
                }
                if *sign == Sign::Negative && p.positives().all(|b| flipped.contains(&b.id)) {
                    return Err(Error::Invalid(Refusal::LastPositive));
                }
                for b in &mut self.part_mut(part)?.bodies {
                    if flipped.contains(&b.id) {
                        b.sign = *sign;
                    }
                }
                self.recompute(part, None, solver)?;
                Ok(Outcome::default())
            }
            Operation::TransformBodies { target, bodies, transform, scope } => {
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (part, guides, bodies) = self.bodies_of(target, bodies, *scope)?;
                self.move_guides(part, &guides, transform)?;
                if !bodies.is_empty() {
                    for b in &mut self.part_mut(part)?.bodies {
                        if bodies.contains(&b.id) {
                            b.placement = *transform * b.placement;
                        }
                    }
                    self.recompute(part, None, solver)?;
                }
                Ok(Outcome::default())
            }
            Operation::DuplicateBodies { target, bodies, transform, scope } => {
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (part, guides, bodies) = self.bodies_of(target, bodies, *scope)?;
                let mut created = self.copy_bodies(part, &bodies, &[*transform])?;
                if !created.is_empty() {
                    self.recompute(part, Some(&created), solver)?;
                }
                created.extend(self.copy_guides(part, &guides, &[*transform])?);
                Ok(Outcome { bodies: created, ..Outcome::default() })
            }
            Operation::ArrayBodies { target, bodies, step, count, scope } => {
                if *count == 0 || *count > MAX_ARRAY {
                    return Err(Error::Invalid(Refusal::ArrayCount));
                }
                if !is_rigid(step) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let (part, guides, bodies) = self.bodies_of(target, bodies, *scope)?;
                let steps: Vec<DAffine3> = std::iter::successors(Some(*step), |at| Some(*step * *at)).take(*count as usize).collect();
                let mut created = self.copy_bodies(part, &bodies, &steps)?;
                if !created.is_empty() {
                    self.recompute(part, Some(&created), solver)?;
                }
                created.extend(self.copy_guides(part, &guides, &steps)?);
                Ok(Outcome { bodies: created, ..Outcome::default() })
            }
            Operation::CopyBodies { target, bodies, moved, placements, scope } => {
                check_copies(moved, placements)?;
                let (part, guides, bodies) = self.bodies_of(target, bodies, *scope)?;
                let mut created = self.copy_bodies(part, &bodies, placements)?;
                let shifted = !is_identity(moved);
                if shifted {
                    for b in &mut self.part_mut(part)?.bodies {
                        if bodies.contains(&b.id) {
                            b.placement = *moved * b.placement;
                        }
                    }
                }
                if shifted && !bodies.is_empty() {
                    // A source moved: the part from its bodies again.
                    self.recompute(part, None, solver)?;
                } else if !created.is_empty() {
                    self.recompute(part, Some(&created), solver)?;
                }
                created.extend(self.copy_guides(part, &guides, placements)?);
                self.move_guides(part, &guides, moved)?;
                Ok(Outcome { bodies: created, ..Outcome::default() })
            }
            Operation::Stretch { context, occurrences, frame, axis, cuts, moves, scope, cutters } => {
                let stretch = Stretch::new(*axis, cuts.clone(), moves.clone()).map_err(solidsketch_kernel::Error::from)?;
                let stretched = self
                    .stretch_tree(context, occurrences, frame, &stretch, *scope, &mut |doc, part, s| doc.stretch_part(part, s, solver))?;
                if let (Some(cutters), [o]) = (cutters, &occurrences[..]) {
                    let component = self.assembly(self.context_of(context)?)?.occurrence(*o).ok_or(Error::NotFound(*o))?.component;
                    let mut behavior = self.component(component)?.behavior.clone();
                    behavior.cutters = Some((**cutters).clone());
                    let behavior = behavior.normalized();
                    if !behavior.is_valid() {
                        return Err(Error::Invalid(Refusal::BadBehavior));
                    }
                    self.component_mut(component)?.behavior = behavior;
                }
                Ok(Outcome { stretched, ..Outcome::default() })
            }
            Operation::StretchBodies { target, bodies, frame, axis, cuts, moves, scope } => {
                if !is_rigid(frame) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let stretch = Stretch::new(*axis, cuts.clone(), moves.clone()).map_err(solidsketch_kernel::Error::from)?;
                // In the part's axes.
                let stretch = stretch.in_frame(&frame.inverse());
                let (part, guides, ids) = self.bodies_of(target, bodies, *scope)?;
                self.stretch_guides(part, &stretch, Some(&guides))?;
                if !ids.is_empty() {
                    self.stretch_bodies(part, Some(&ids), &stretch, solver)?;
                    self.recompute(part, None, solver)?;
                }
                Ok(Outcome::default())
            }
            Operation::SetProfile { target, body, profile, scope } => {
                let (_, p, _) = self.resolve_part(target)?;
                let b = p.body(*body).ok_or(Error::NotFound(*body))?;
                if p.profile_of(b).is_none() || !profile.is_valid() {
                    return Err(Error::Invalid(Refusal::BadProfile));
                }
                let def = b.def;
                if *scope == Scope::This {
                    self.separate(target)?;
                }
                let (part, _, _) = self.resolve_part(target)?;
                let solid = solver.primitive(&profile.primitive())?;
                // Its definition's: every copy of the body shows it.
                if let Some(d) = self.part_mut(part)?.defs.iter_mut().find(|d| d.id == def) {
                    d.solid = solid;
                    d.profile = Some(profile.clone());
                }
                self.recompute(part, None, solver)?;
                Ok(Outcome::default())
            }
            Operation::SetBehavior { component, behavior } => {
                let behavior = behavior.clone().normalized();
                if !behavior.is_valid() {
                    return Err(Error::Invalid(Refusal::BadBehavior));
                }
                self.component(*component)?;
                self.component_mut(*component)?.behavior = behavior;
                Ok(Outcome::default())
            }
        }
    }

    /// Stretches `part` (§6 sexies): each body moved when in one section, stretched in its own
    /// axes across a cutter (its profile kept when it stays one, §6 nonies), and its guides;
    /// then computes it again.
    fn stretch_part(&mut self, part: Id, stretch: &Stretch, solver: &mut dyn Solver) -> Result<()> {
        self.stretch_bodies(part, None, stretch, solver)?;
        self.stretch_guides(part, stretch, None)?;
        self.recompute(part, None, solver)
    }

    /// Stretches the bodies `ids` of `part` (all of them: `None`) by `stretch`, in the part's
    /// axes (§2.11, after §6 sexies' components): a body lying in one section is moved; one
    /// across a cutter changes its definition, the stretch taken in its own axes (its profile
    /// kept when it stays one, §6 nonies). Bodies of one definition stretched alike keep one;
    /// the change most of them take is made to it in place — the bodies not reached show it
    /// too —, the others go to copies; all of them do when a body reached keeps the shape (only
    /// moved). The part is not computed again.
    fn stretch_bodies(&mut self, part: Id, ids: Option<&[Id]>, stretch: &Stretch, solver: &mut dyn Solver) -> Result<()> {
        let p = self.part(part)?.clone();
        let BodyStretch { moved, changes } = p.body_stretch(ids, stretch);
        let mut defs = p.defs.clone();
        let mut bodies = p.bodies.clone();
        for c in &changes {
            let def = p.def(c.def).ok_or(Error::NotFound(c.def))?;
            let solid = solver.stretch(&def.solid, &c.local)?;
            let profile = def.profile.as_ref().and_then(|f| f.stretched(&c.local));
            if c.in_place {
                if let Some(d) = defs.iter_mut().find(|d| d.id == c.def) {
                    (d.solid, d.profile) = (solid, profile);
                }
            } else {
                let copy = BodyDef { id: self.fresh_id(), solid, profile, ..def.clone() };
                for b in bodies.iter_mut().filter(|b| c.bodies.contains(&b.id)) {
                    b.def = copy.id;
                }
                defs.push(copy);
            }
        }
        for b in &mut bodies {
            if let Some((_, t)) = moved.iter().find(|(id, _)| *id == b.id) {
                b.placement = *t;
            }
        }
        let p = self.part_mut(part)?;
        (p.defs, p.bodies) = (defs, bodies);
        p.drop_unused_defs();
        Ok(())
    }

    /// The part `target` places, `ids` (at least one) split into its guides and its bodies,
    /// every one in it; made its own first under the scope "only this one" when bodies change
    /// (they and the guides keep their ids, §2.6) — guides alone ignore the scope (§6
    /// quinquies).
    fn bodies_of(&mut self, target: &EntityPath, ids: &[Id], scope: Scope) -> Result<(Id, Vec<Id>, Vec<Id>)> {
        if ids.is_empty() {
            return Err(Error::Invalid(Refusal::NoBodies));
        }
        let (part, p, _) = self.resolve_part(target)?;
        let p = p.clone();
        let (guides, bodies) = self.split_guides(part, ids)?;
        if let Some(&missing) = bodies.iter().find(|id| p.body(**id).is_none()) {
            return Err(Error::NotFound(missing));
        }
        if scope == Scope::This && !bodies.is_empty() {
            self.separate(target)?;
            return Ok((self.resolve_part(target)?.0, guides, bodies));
        }
        Ok((part, guides, bodies))
    }

    /// `ids` split into the guides of `component` and the rest, each in the order given.
    pub(crate) fn split_guides(&self, component: Id, ids: &[Id]) -> Result<(Vec<Id>, Vec<Id>)> {
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

    /// Copies of `bodies` moved by `t`, and of the definitions `defs` they use, all with fresh
    /// ids — bodies sharing a definition still share its copy: what another part receives.
    fn copies_of_bodies(&mut self, defs: &[BodyDef], bodies: &[Body], t: &DAffine3) -> (Vec<BodyDef>, Vec<Body>) {
        let mut renamed: Vec<(Id, BodyDef)> = Vec::new();
        let mut out = Vec::with_capacity(bodies.len());
        for b in bodies {
            let def = match renamed.iter().find(|(from, _)| *from == b.def) {
                Some((_, d)) => d.id,
                None => {
                    let Some(d) = defs.iter().find(|d| d.id == b.def) else { continue };
                    let copy = BodyDef { id: self.fresh_id(), ..d.clone() };
                    let id = copy.id;
                    renamed.push((b.def, copy));
                    id
                }
            };
            out.push(Body { id: self.fresh_id(), def, placement: *t * b.placement, ..b.clone() });
        }
        (renamed.into_iter().map(|(_, d)| d).collect(), out)
    }

    /// Adds to `part`, for each transform of `steps`, a copy of its bodies `ids` moved by it (in
    /// the part's axes), with fresh ids; returns them in order.
    fn copy_bodies(&mut self, part: Id, ids: &[Id], steps: &[DAffine3]) -> Result<Vec<Id>> {
        let sources: Vec<Body> = self.part(part)?.bodies.iter().filter(|b| ids.contains(&b.id)).cloned().collect();
        let mut copies = Vec::with_capacity(sources.len() * steps.len());
        for t in steps {
            for b in &sources {
                copies.push(Body { id: self.fresh_id(), placement: *t * b.placement, ..b.clone() });
            }
        }
        let created = copies.iter().map(|b| b.id).collect();
        self.part_mut(part)?.bodies.extend(copies);
        Ok(created)
    }

    /// Computes `part`'s shape again from its bodies (§2.4, §2.11): `(∪ positives) − (∪
    /// negatives)`. When `added` is given (bodies just added) and all of one sign, the part as
    /// it is is kept: added negatives cut it, added positives are cut by every negative then
    /// joined to it (`(A ∪ B) − N = (A − N) ∪ (B − N)`); else everything is computed again. A
    /// plain part (a single positive body, unplaced) is its body: no kernel.
    fn recompute(&mut self, part: Id, added: Option<&[Id]>, solver: &mut dyn Solver) -> Result<()> {
        let p = self.part(part)?;
        if p.is_plain() {
            let p = self.part_mut(part)?;
            p.shape = None;
            p.failed.clear();
            return Ok(());
        }
        let placed = |b: &Body| (p.solid_of(b).clone(), b.placement);
        let is_added = |b: &Body| added.is_some_and(|ids| ids.contains(&b.id));
        let new: Vec<&Body> = p.bodies.iter().filter(|b| is_added(b)).collect();
        let current = p.shape().clone();
        // The bodies each index of the task's failures names: its positives, then its negatives.
        let (base, positives, negatives): (Option<Arc<Solid>>, Vec<&Body>, Vec<&Body>) =
            if !new.is_empty() && new.iter().all(|b| b.sign == Sign::Negative) {
                (None, Vec::new(), new)
            } else if !new.is_empty() && new.iter().all(|b| b.sign == Sign::Positive) {
                (Some(current.clone()), new, p.negatives().collect())
            } else {
                (None, p.positives().collect(), p.negatives().collect())
            };
        let incremental = base.is_some() || positives.is_empty();
        let mut named: Vec<Id> = positives.iter().chain(&negatives).map(|b| b.id).collect();
        let mut pos: Vec<(Arc<Solid>, DAffine3)> = positives.iter().map(|b| placed(b)).collect();
        if positives.is_empty() {
            // Negatives added: they cut the part as it is.
            pos.push((current, DAffine3::IDENTITY));
            named.insert(0, Id(u64::MAX));
        }
        let neg: Vec<(Arc<Solid>, DAffine3)> = negatives.iter().map(|b| placed(b)).collect();
        // Added positives are cut by every negative: only their failures are new.
        let fresh: Vec<Id> = if base.is_some() { positives.iter().map(|b| b.id).collect() } else { named.clone() };
        let (shape, failed) = solver.recipe(base.as_ref(), &pos, &neg)?;
        let p = self.part_mut(part)?;
        if !incremental {
            p.failed.clear();
        }
        p.failed.extend(failed.iter().filter_map(|k| named.get(*k).copied()).filter(|id| fresh.contains(id)));
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
        let q = q.clone();
        let tool_ctx = self.context_of(&tool.instances)?;
        // The tool, in the target part's axes.
        let relative = t_target.inverse() * t_tool;
        match op {
            // The tool's shape, a negative body of the target, cut from it as it is. A tool
            // that is its single positive body (a shape drawn) is taken whole, its profile
            // with it: still editable (§2.11).
            Boolean::Difference => {
                let id = self.fresh_id();
                let moved = if is_identity(&relative) { None } else { Some(relative) };
                let result = solver.boolean(p.shape(), q.shape(), moved, op.into())?;
                let (def, body) = match q.is_plain() {
                    true => {
                        let def = BodyDef { id: self.fresh_id(), ..q.def_of(&q.bodies[0]).clone() };
                        let body = Body::new(id, def.id, Sign::Negative, relative * q.bodies[0].placement);
                        (def, body)
                    }
                    false => {
                        let def = BodyDef::new(self.fresh_id(), q.shape().clone());
                        let body = Body::new(id, def.id, Sign::Negative, relative);
                        (def, body)
                    }
                };
                let mut next = p;
                next.defs.push(def);
                next.bodies.push(body);
                next.shape = Some(result);
                *self.part_mut(part)? = next;
            }
            // Every body of the tool, positive and negative, joins the target's (§2.11). Its
            // machinings cut the target too: said when they do, the result then holding less
            // than both shapes joined.
            Boolean::Union => {
                let (defs, bodies) = self.copies_of_bodies(&q.defs, &q.bodies, &relative);
                let check = q.negatives().next().is_some();
                let both = if check {
                    solver.union_volume(&[(p.shape().clone(), DAffine3::IDENTITY), (q.shape().clone(), relative)])?
                } else {
                    None
                };
                let p = self.part_mut(part)?;
                p.defs.extend(defs);
                p.bodies.extend(bodies);
                self.recompute(part, None, solver)?;
                let after = if check { solver.union_volume(&[(self.part(part)?.shape().clone(), DAffine3::IDENTITY)])? } else { None };
                let cut = matches!((both, after), (Some(b), Some(a)) if a < b * (1.0 - 1e-5));
                self.assembly_mut(tool_ctx)?.occurrences_mut().retain(|o| o.id != tool.entity);
                let warning = cut.then_some(Warning::UnionCut);
                return Ok(Outcome { created: vec![target.entity], warning, ..Outcome::default() });
            }
            // A new part of one positive body: the common volume.
            Boolean::Intersection => {
                let moved = if is_identity(&relative) { None } else { Some(relative) };
                let result = solver.boolean(p.shape(), q.shape(), moved, op.into())?;
                *self.part_mut(part)? = self.plain_part(result);
            }
        }
        self.assembly_mut(tool_ctx)?.occurrences_mut().retain(|o| o.id != tool.entity);
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

    /// Where the occurrences `ids` stand in `context`, in the context's order, each once;
    /// every id must be there.
    fn positions_of(&self, context: Id, ids: &[Id]) -> Result<Vec<usize>> {
        let a = self.assembly(context)?;
        let mut at = if ids.len() * 8 <= a.occurrences().len() {
            // A few among many: looked up one by one, by the assembly's index.
            ids.iter().map(|id| a.position(*id).ok_or(Error::NotFound(*id))).collect::<Result<Vec<usize>>>()?
        } else {
            // Sets both ways: a selection can hold tens of thousands of occurrences.
            let wanted = id_set(ids);
            let at: Vec<usize> = a.occurrences().iter().enumerate().filter(|(_, o)| wanted.contains(&o.id)).map(|(i, _)| i).collect();
            if at.len() < wanted.len() {
                let present: FastSet<Id> = at.iter().map(|i| a.occurrences()[*i].id).collect();
                if let Some(&missing) = ids.iter().find(|id| !present.contains(id)) {
                    return Err(Error::NotFound(missing));
                }
            }
            at
        };
        at.sort_unstable();
        at.dedup();
        Ok(at)
    }

    /// Clones of the occurrences `ids` of `context`, in the context's order; every id must be
    /// there.
    pub(crate) fn occurrences_of(&self, context: Id, ids: &[Id]) -> Result<Vec<Occurrence>> {
        let at = self.positions_of(context, ids)?;
        let occurrences = self.assembly(context)?.occurrences();
        Ok(at.into_iter().map(|i| occurrences[i].clone()).collect())
    }

    /// Adds copies of `sources` (fresh ids, same components) to `context`, moved by `t`.
    fn add_copies(&mut self, context: Id, sources: &[Occurrence], t: &DAffine3) -> Result<Vec<Id>> {
        let mut copies = Vec::with_capacity(sources.len());
        for s in sources {
            let id = self.fresh_id();
            copies.push(Occurrence { id, transform: *t * s.transform, ..s.clone() });
        }
        let created = copies.iter().map(|c| c.id).collect();
        self.assembly_mut(context)?.occurrences_mut().extend(copies);
        Ok(created)
    }
}

/// `ids` as a set, for lookups over a whole assembly.
fn id_set(ids: &[Id]) -> FastSet<Id> {
    ids.iter().copied().collect()
}

/// Refuses `Copies` / `CopyBodies` doing nothing or too much, or not rigid.
fn check_copies(moved: &DAffine3, placements: &[DAffine3]) -> Result<()> {
    if placements.len() > MAX_ARRAY as usize || (placements.is_empty() && is_identity(moved)) {
        return Err(Error::Invalid(Refusal::ArrayCount));
    }
    if !is_rigid(moved) || !placements.iter().all(is_rigid) {
        return Err(Error::Invalid(Refusal::NotRigid));
    }
    Ok(())
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
    /// The union of the placed `positives` minus each placed negative, joined to `base` when
    /// given ([`Task::Recipe`]); the solid, and the bodies (by index, positives then
    /// negatives) that failed.
    fn recipe(
        &mut self,
        base: Option<&Arc<Solid>>,
        positives: &[(Arc<Solid>, DAffine3)],
        negatives: &[(Arc<Solid>, DAffine3)],
    ) -> Result<(Arc<Solid>, Vec<usize>)>;
    /// `solid` stretched.
    fn stretch(&mut self, solid: &Arc<Solid>, stretch: &Stretch) -> Result<Arc<Solid>>;
    /// The volume of the union of the placed `solids`, when it can be computed (always none
    /// while the job is recorded).
    fn union_volume(&mut self, solids: &[(Arc<Solid>, DAffine3)]) -> Result<Option<f64>>;
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

    fn recipe(
        &mut self,
        base: Option<&Arc<Solid>>,
        positives: &[(Arc<Solid>, DAffine3)],
        negatives: &[(Arc<Solid>, DAffine3)],
    ) -> Result<(Arc<Solid>, Vec<usize>)> {
        let base = base.map(|b| self.input(b));
        let positives = positives.iter().map(|(s, p)| (self.input(s), *p)).collect();
        let negatives = negatives.iter().map(|(s, p)| (self.input(s), *p)).collect();
        Ok((self.task(Task::Recipe { base, positives, negatives })?, Vec::new()))
    }

    /// Refused here already when the record cannot be stretched: the operation is refused when
    /// prepared, not when the kernel answers.
    fn stretch(&mut self, solid: &Arc<Solid>, stretch: &Stretch) -> Result<Arc<Solid>> {
        stretch.apply(solid).map_err(solidsketch_kernel::Error::from)?;
        let solid = self.input(solid);
        self.task(Task::Stretch { solid, stretch: stretch.clone() })
    }

    fn union_volume(&mut self, solids: &[(Arc<Solid>, DAffine3)]) -> Result<Option<f64>> {
        let solids = solids.iter().map(|(s, t)| (self.input(s), *t)).collect();
        self.job.tasks.push(Task::UnionVolume { solids });
        Ok(None)
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

    fn recipe(
        &mut self,
        _: Option<&Arc<Solid>>,
        _: &[(Arc<Solid>, DAffine3)],
        _: &[(Arc<Solid>, DAffine3)],
    ) -> Result<(Arc<Solid>, Vec<usize>)> {
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

    fn union_volume(&mut self, _: &[(Arc<Solid>, DAffine3)]) -> Result<Option<f64>> {
        match self.outputs.next() {
            Some(Output::Volume(v)) => Ok(v),
            _ => Err(Error::Invalid(Refusal::OutOfStep)),
        }
    }
}

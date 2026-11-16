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

use std::collections::BTreeSet;

use crate::clipboard::BodyFragment;
use crate::document::{
    Assembly, Behavior, Body, BodyDef, ComponentKind, Document, EntityPath, Guide, GuideShape, Id, Material, Occurrence, Part, Sign,
    is_identity,
};
use crate::settings::Settings;
use crate::stretch::BodyStretch;
use crate::{Error, FastMap, FastSet, Refusal, Result};

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
    /// Groups bodies of the part `target` places (§2.11 « Body groups »): one body of a new
    /// group definition takes their place, the group's axes the part's. Their negatives then
    /// cut their positives only. Negatives alone make a negative group of positive children
    /// (a compound machining); otherwise the group is positive, the signs kept.
    GroupBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        #[serde(default)]
        scope: Scope,
    },
    /// Flattens groups among bodies of the part `target` places: each one's children brought
    /// up into the part, their placements and signs composed with the group's — the reach of
    /// their negatives widened to the part. A negative group holding a negative is refused
    /// (it would add material).
    UngroupBodies {
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
    /// Adds to the part `target` places the parts copied ([`Document::entities_fragment`], in
    /// world axes) as bodies ([`Document::parts_as_bodies`]): one positive body per part, a
    /// group of its bodies unless it is its single body, moved by the rigid `transform` from
    /// world axes into the part's (§6 decies, across the part's boundary).
    PasteParts {
        target: EntityPath,
        fragment: Arc<Document>,
        transform: DAffine3,
        #[serde(default)]
        scope: Scope,
    },
    /// Places in `context` a new part of the bodies copied from an open part
    /// ([`Document::bodies_fragment`]): in the axes of the part copied from, the occurrence
    /// moved by the rigid `transform` from world axes into the context's. Signs kept, unless
    /// no body is positive: then they all are.
    PasteAsPart { context: Id, fragment: Arc<BodyFragment>, transform: DAffine3 },
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
            Operation::GroupBodies { .. } => t!("operation.group"),
            Operation::UngroupBodies { .. } => t!("operation.ungroup"),
            Operation::SetSign { sign: Sign::Positive, .. } => t!("operation.set_sign_positive"),
            Operation::SetSign { sign: Sign::Negative, .. } => t!("operation.set_sign_negative"),
            Operation::TransformBodies { .. } => t!("operation.transform"),
            Operation::DuplicateBodies { .. } => t!("operation.duplicate"),
            Operation::ArrayBodies { .. } => t!("operation.array"),
            Operation::AddGuide { .. } => t!("operation.add_guide"),
            Operation::Insert { .. } => t!("operation.import"),
            Operation::Paste { .. } | Operation::PasteBodies { .. } | Operation::PasteParts { .. } | Operation::PasteAsPart { .. } => {
                t!("operation.paste")
            }
            Operation::Stretch { .. } | Operation::StretchBodies { .. } => t!("operation.stretch"),
            Operation::SetBehavior { .. } => t!("operation.stretch_behavior"),
            Operation::SetProfile { .. } => t!("operation.set_profile"),
        }
    }

    /// The part (or group of bodies, §2.11 « Body groups ») an operation on bodies changes,
    /// and what it reaches; none for the other operations.
    fn body_target_mut(&mut self) -> Option<(&mut EntityPath, Scope)> {
        match self {
            Operation::AddBody { target, scope, .. }
            | Operation::DeleteBodies { target, scope, .. }
            | Operation::MakeBodiesUnique { target, scope, .. }
            | Operation::SetSign { target, scope, .. }
            | Operation::TransformBodies { target, scope, .. }
            | Operation::DuplicateBodies { target, scope, .. }
            | Operation::ArrayBodies { target, scope, .. }
            | Operation::CopyBodies { target, scope, .. }
            | Operation::StretchBodies { target, scope, .. }
            | Operation::SetProfile { target, scope, .. }
            | Operation::PasteBodies { target, scope, .. }
            | Operation::PasteParts { target, scope, .. }
            | Operation::GroupBodies { target, scope, .. }
            | Operation::UngroupBodies { target, scope, .. } => Some((target, *scope)),
            _ => None,
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
    /// The faces put onto others after it (§2.11's welding), for the journal.
    pub welds: Vec<crate::Weld>,
}

/// What an operation done has to say. None for now: a union's tool, and a part pasted into a
/// part, now come as groups (§2.11 « Body groups »), with nothing lost to tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {}

impl std::fmt::Display for Warning {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {}
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
        let mut outcome = doc.apply_in_place(op, &mut recorder)?;
        outcome.welds = doc.weld(self, &mut recorder)?;
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
        let mut outcome = doc.apply_in_place(op, &mut replayer)?;
        outcome.welds = doc.weld(self, &mut replayer)?;
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
        let mut inner = op.clone();
        if let Some((target, scope)) = inner.body_target_mut()
            && !target.group.is_empty()
        {
            let group = std::mem::take(&mut target.group);
            let path = target.clone();
            return self.apply_in_group(&path, &group, scope, &inner, solver);
        }
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
            Operation::PasteBodies { target, fragment, transform, scope } => self.paste_bodies(target, fragment, transform, *scope, solver),
            Operation::PasteParts { target, fragment, transform, scope } => {
                let parts = fragment.parts_as_bodies()?;
                self.paste_bodies(target, &parts, transform, *scope, solver)
            }
            Operation::PasteAsPart { context, fragment, transform } => {
                self.assembly(*context)?;
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                if fragment.bodies.is_empty() {
                    return Err(Error::Invalid(Refusal::NothingPasted));
                }
                let (defs, mut bodies) = self.copies_of_bodies(&fragment.defs, &fragment.bodies, &DAffine3::IDENTITY);
                if bodies.iter().all(|b| b.sign == Sign::Negative) {
                    bodies.iter_mut().for_each(|b| b.sign = Sign::Positive);
                }
                let part = Part { defs, bodies, shape: None, failed: Default::default(), faces: Default::default() };
                let part = self.insert_component(&t!("default_name.part"), false, ComponentKind::Part(part));
                self.component_mut(part)?.guides = fragment.guides.iter().map(|s| Guide { id: self.fresh_id(), shape: *s }).collect();
                self.recompute(part, None, solver)?;
                let id = self.fresh_id();
                self.assembly_mut(*context)?.occurrences_mut().push(Occurrence {
                    id,
                    name: String::new(),
                    component: part,
                    transform: *transform * fragment.world,
                    material: None,
                    hidden: false,
                });
                Ok(Outcome { created: vec![id], ..Outcome::default() })
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
                // The last positive ones too: the part is then empty, an error shown (§2.11
                // « Empty parts »).
                let (part, guides, bodies) = &self.bodies_of(target, bodies, *scope)?;
                self.component_mut(*part)?.guides.retain(|g| !guides.contains(&g.id));
                if !bodies.is_empty() {
                    let p = self.part_mut(*part)?;
                    p.bodies.retain(|b| !bodies.contains(&b.id));
                    p.drop_unused_defs();
                    self.recompute(*part, None, solver)?;
                }
                Ok(Outcome::default())
            }
            Operation::GroupBodies { target, bodies, scope } => {
                let (part, _, bodies) = self.bodies_of(target, bodies, *scope)?;
                if bodies.is_empty() {
                    return Err(Error::Invalid(Refusal::NoBodies));
                }
                let id = self.group_bodies(part, &bodies, solver)?;
                Ok(Outcome { bodies: vec![id], ..Outcome::default() })
            }
            Operation::UngroupBodies { target, bodies, scope } => {
                let (part, _, bodies) = self.bodies_of(target, bodies, *scope)?;
                let created = self.ungroup_bodies(part, &bodies, solver)?;
                Ok(Outcome { bodies: created, ..Outcome::default() })
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
                    let fresh = self.fresh_id();
                    let copy = self.renamed_children(BodyDef { id: fresh, ..def });
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
                // The last positive ones too: the part is then empty (§2.11 « Empty parts »).
                if flipped.is_empty() {
                    return Ok(Outcome::default());
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
            let (stretched, inner) = self.stretched_def(&p.defs, def, &c.local, solver)?;
            defs.extend(inner);
            if c.in_place {
                if let Some(d) = defs.iter_mut().find(|d| d.id == c.def) {
                    *d = stretched;
                }
            } else {
                let id = self.fresh_id();
                let copy = self.renamed_children(BodyDef { id, ..stretched });
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

    /// The definition `def` (of the definitions `defs`) stretched by `local`, in its axes, under
    /// its id; and the definitions it now needs that `defs` lacks. A leaf has its solid
    /// stretched (its profile kept when it stays one, §6 nonies); a group has its children
    /// stretched in turn — each in one section moved, each across a cutter given a definition
    /// of its own, stretched in its axes — then is computed again.
    fn stretched_def(
        &mut self,
        defs: &[BodyDef],
        def: &BodyDef,
        local: &Stretch,
        solver: &mut dyn Solver,
    ) -> Result<(BodyDef, Vec<BodyDef>)> {
        let Some(g) = &def.group else {
            let profile = def.profile.as_ref().and_then(|f| f.stretched(local));
            // A turned body: along its axis only, rebuilt from its half profile (§6 undecies).
            let solid = match (&def.profile, &profile) {
                (Some(f), None) if f.is_turned() => return Err(Error::Invalid(Refusal::TurnedAcross)),
                (_, Some(f)) if f.is_turned() => solver.primitive(&f.primitive())?,
                _ => solver.stretch(&def.solid, local)?,
            };
            return Ok((BodyDef { solid, profile, ..def.clone() }, Vec::new()));
        };
        let BodyStretch { moved, changes } = crate::stretch::bodies_stretch(defs, &g.children, None, local);
        let mut children = g.children.clone();
        let mut extra: Vec<BodyDef> = Vec::new();
        for c in &changes {
            let inner = defs.iter().find(|d| d.id == c.def).ok_or(Error::NotFound(c.def))?;
            let (stretched, more) = self.stretched_def(defs, inner, &c.local, solver)?;
            let id = self.fresh_id();
            let copy = self.renamed_children(BodyDef { id, ..stretched });
            for b in children.iter_mut().filter(|b| c.bodies.contains(&b.id)) {
                b.def = copy.id;
            }
            extra.extend(more);
            extra.push(copy);
        }
        for b in &mut children {
            if let Some((_, t)) = moved.iter().find(|(id, _)| *id == b.id) {
                b.placement = *t;
            }
        }
        let all: Vec<BodyDef> = defs.iter().chain(&extra).cloned().collect();
        let (solid, failed) = Self::group_result(&all, &children, solver)?;
        let group = crate::BodyGroup { children, failed };
        Ok((BodyDef { solid, group: Some(group), ..def.clone() }, extra))
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

    /// Adds to the part `target` places the bodies and guides of `fragment`, moved by `transform`
    /// in the part's axes ([`Operation::PasteBodies`]).
    fn paste_bodies(
        &mut self,
        target: &EntityPath,
        fragment: &BodyFragment,
        transform: &DAffine3,
        scope: Scope,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        if !is_rigid(transform) {
            return Err(Error::Invalid(Refusal::NotRigid));
        }
        if fragment.is_empty() {
            return Err(Error::Invalid(Refusal::NothingPasted));
        }
        self.resolve_part(target)?;
        if scope == Scope::This && !fragment.bodies.is_empty() {
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
        let guides: Vec<Guide> = fragment.guides.iter().map(|s| Guide { id: self.fresh_id(), shape: s.transformed(transform) }).collect();
        created.extend(guides.iter().map(|g| g.id));
        self.component_mut(part)?.guides.extend(guides);
        Ok(Outcome { bodies: created, ..Outcome::default() })
    }

    /// Copies of `bodies` moved by `t`, and of the definitions `defs` they use through groups,
    /// all with fresh ids — bodies sharing a definition still share its copy: what another part
    /// receives.
    fn copies_of_bodies(&mut self, defs: &[BodyDef], bodies: &[Body], t: &DAffine3) -> (Vec<BodyDef>, Vec<Body>) {
        let used = crate::document::used_defs(defs, bodies);
        let kept: Vec<&BodyDef> = defs.iter().filter(|d| used.contains(&d.id)).collect();
        let renamed: FastMap<Id, Id> = kept.iter().map(|d| (d.id, self.fresh_id())).collect();
        let mut out_defs = Vec::with_capacity(kept.len());
        for d in kept {
            let mut copy = BodyDef { id: renamed[&d.id], ..d.clone() };
            if let Some(g) = &mut copy.group {
                g.children.retain(|c| renamed.contains_key(&c.def));
                g.children.iter_mut().for_each(|c| c.def = renamed[&c.def]);
            }
            out_defs.push(self.renamed_children(copy));
        }
        let mut out = Vec::with_capacity(bodies.len());
        for b in bodies {
            let Some(&def) = renamed.get(&b.def) else { continue };
            out.push(Body { id: self.fresh_id(), def, placement: *t * b.placement, ..b.clone() });
        }
        (out_defs, out)
    }

    /// `def` with fresh ids for a group's children, its failures following them: a copy of a
    /// group holds bodies of its own (body ids are unique in a part, groups included).
    pub(crate) fn renamed_children(&mut self, mut def: BodyDef) -> BodyDef {
        if let Some(g) = &mut def.group {
            let mut map: Vec<(Id, Id)> = Vec::with_capacity(g.children.len());
            for c in &mut g.children {
                let id = self.fresh_id();
                map.push((c.id, id));
                c.id = id;
            }
            g.failed = g.failed.iter().filter_map(|f| map.iter().find(|(old, _)| old == f).map(|(_, new)| *new)).collect();
        }
        def
    }

    /// The result of a group of `children` (`defs` the part's): `(∪ positives) − (∪
    /// negatives)`, and the children whose boolean failed. A single positive child, unplaced,
    /// is the result: no kernel.
    pub(crate) fn group_result(defs: &[BodyDef], children: &[Body], solver: &mut dyn Solver) -> Result<(Arc<Solid>, BTreeSet<Id>)> {
        let solid = |b: &Body| defs.iter().find(|d| d.id == b.def).map(|d| d.solid.clone()).ok_or(Error::NotFound(b.def));
        if let [c] = children
            && c.sign == Sign::Positive
            && is_identity(&c.placement)
        {
            return Ok((solid(c)?, BTreeSet::new()));
        }
        let (pos, neg): (Vec<&Body>, Vec<&Body>) = children.iter().partition(|b| b.sign == Sign::Positive);
        let placed = |list: &[&Body]| list.iter().map(|b| Ok((solid(b)?, b.placement))).collect::<Result<Vec<_>>>();
        let (shape, failed) = solver.recipe(None, &placed(&pos)?, &placed(&neg)?)?;
        let named: Vec<Id> = pos.iter().chain(&neg).map(|b| b.id).collect();
        Ok((shape, failed.iter().filter_map(|k| named.get(*k).copied()).collect()))
    }

    /// [`Operation::GroupBodies`]: the bodies `ids` of `part` (each one of its own) replaced
    /// by one body of a new group of them, where the first of them stood; returns its id.
    fn group_bodies(&mut self, part: Id, ids: &[Id], solver: &mut dyn Solver) -> Result<Id> {
        let p = self.part(part)?;
        let mut children: Vec<Body> = p.bodies.iter().filter(|b| ids.contains(&b.id)).cloned().collect();
        let negative = children.iter().all(|b| b.sign == Sign::Negative);
        // Negatives alone: a compound machining, its children the shape it takes away.
        if negative {
            children.iter_mut().for_each(|c| c.sign = Sign::Positive);
        }
        let mixed = children.iter().any(|b| b.sign == Sign::Negative);
        let (solid, failed) = Self::group_result(&p.defs, &children, solver)?;
        let def = BodyDef::group(self.fresh_id(), solid, children, failed);
        let sign = if negative { Sign::Negative } else { Sign::Positive };
        let body = Body::new(self.fresh_id(), def.id, sign, DAffine3::IDENTITY);
        let id = body.id;
        let p = self.part_mut(part)?;
        let at = p.bodies.iter().position(|b| ids.contains(&b.id)).unwrap_or(p.bodies.len());
        p.bodies.retain(|b| !ids.contains(&b.id));
        p.bodies.insert(at.min(p.bodies.len()), body);
        p.defs.push(def);
        p.failed.retain(|f| !ids.contains(f));
        // The same bodies of one sign: the part is the same, unless it is now its group alone.
        // Mixed, the group's negatives no longer cut the positives outside it.
        if mixed || self.part(part)?.is_plain() {
            self.recompute(part, None, solver)?;
        }
        Ok(id)
    }

    /// [`Operation::UngroupBodies`]: the groups among the bodies `ids` of `part` flattened in
    /// place — their children brought up, placements and signs composed (fresh ids when the
    /// group is shared); returns the bodies brought up.
    fn ungroup_bodies(&mut self, part: Id, ids: &[Id], solver: &mut dyn Solver) -> Result<Vec<Id>> {
        let p = self.part(part)?.clone();
        let is_group = |b: &Body| ids.contains(&b.id) && p.def_of(b).is_group();
        if !p.bodies.iter().any(is_group) {
            return Ok(Vec::new());
        }
        // `parent − (P − N)`: no body of the parent's takes away P but leaves N.
        if p.bodies
            .iter()
            .filter(|b| is_group(b))
            .any(|b| b.sign == Sign::Negative && p.def_of(b).children().iter().any(|c| c.sign == Sign::Negative))
        {
            return Err(Error::Invalid(Refusal::UngroupNegative));
        }
        let mut bodies = Vec::with_capacity(p.bodies.len());
        let mut created = Vec::new();
        for b in &p.bodies {
            if !is_group(b) {
                bodies.push(b.clone());
                continue;
            }
            let shared = p.copies_of(b.def) > 1;
            for c in p.def_of(b).children() {
                let id = if shared { self.fresh_id() } else { c.id };
                // A negative group holds positives only (above): each one a machining.
                let sign = if b.sign == Sign::Negative { Sign::Negative } else { c.sign };
                bodies.push(Body { id, def: c.def, sign, placement: b.placement * c.placement });
                created.push(id);
            }
        }
        let p = self.part_mut(part)?;
        p.bodies = bodies;
        p.drop_unused_defs();
        self.recompute(part, None, solver)?;
        Ok(created)
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
    /// plain part (a single positive body, unplaced) is its body, a part without bodies empty:
    /// no kernel. Without material (no positive, a machining swallowing it) the shape is empty
    /// (§2.11 « Empty parts »): the kernel says so.
    pub(crate) fn recompute(&mut self, part: Id, added: Option<&[Id]>, solver: &mut dyn Solver) -> Result<()> {
        let p = self.part(part)?;
        if p.is_plain() || p.bodies.is_empty() {
            let empty = p.bodies.is_empty();
            let p = self.part_mut(part)?;
            p.shape = empty.then(|| Arc::new(Solid::empty()));
            p.failed.clear();
            return Ok(());
        }
        let placed = |b: &Body| (p.solid_of(b).clone(), b.placement);
        let is_added = |b: &Body| added.is_some_and(|ids| ids.contains(&b.id));
        let new: Vec<&Body> = p.bodies.iter().filter(|b| is_added(b)).collect();
        let current = p.shape().clone();
        // The bodies each index of the task's failures names: its positives, then its negatives.
        let cut_current = !new.is_empty() && new.iter().all(|b| b.sign == Sign::Negative);
        let (base, positives, negatives): (Option<Arc<Solid>>, Vec<&Body>, Vec<&Body>) =
            if cut_current {
                (None, Vec::new(), new)
            } else if !new.is_empty() && new.iter().all(|b| b.sign == Sign::Positive) {
                (Some(current.clone()), new, p.negatives().collect())
            } else {
                (None, p.positives().collect(), p.negatives().collect())
            };
        let incremental = base.is_some() || cut_current;
        let mut named: Vec<Id> = positives.iter().chain(&negatives).map(|b| b.id).collect();
        let mut pos: Vec<(Arc<Solid>, DAffine3)> = positives.iter().map(|b| placed(b)).collect();
        if cut_current {
            // Negatives added: they cut the part as it is (reported failed when it is empty: no
            // body of the part's).
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
        p.failed.extend(failed.iter().filter_map(|k| named.get(*k).copied()).filter(|id| fresh.contains(id) && *id != Id(u64::MAX)));
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
            // with it: still editable; any other comes as a negative group of copies of its
            // bodies, its shape the group's (§2.11 « Body groups »).
            Boolean::Difference => {
                let moved = if is_identity(&relative) { None } else { Some(relative) };
                let result = solver.boolean(p.shape(), q.shape(), moved, op.into())?;
                let (defs, mut body) = self.tool_as_body(&q, &relative);
                body.sign = Sign::Negative;
                let mut next = p;
                next.defs.extend(defs);
                next.bodies.push(body);
                next.shape = Some(result);
                *self.part_mut(part)? = next;
            }
            // The tool joins the target as a positive body: its single body, or a group of
            // copies of its bodies, its machinings cutting that group only (§2.11 « Body
            // groups »). Added as it is: cut by the target's machinings, joined to its shape.
            Boolean::Union => {
                let (defs, body) = self.tool_as_body(&q, &relative);
                let id = body.id;
                let p = self.part_mut(part)?;
                p.defs.extend(defs);
                p.bodies.push(body);
                self.recompute(part, Some(&[id]), solver)?;
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

    /// `op`, an operation on the bodies of the part `path` places (no group in it), applied
    /// inside its group of bodies `group` (§2.11 « Body groups »): the group's definition is
    /// put in the part's place as a part of its own — its children the bodies, its result the
    /// shape —, `op` applied to it as to any part (in the group's axes), then the definition
    /// takes the result: every copy of the group shows it. The groups above it, then the part,
    /// are computed again. A group made to hold itself, at any depth, is refused.
    fn apply_in_group(
        &mut self,
        path: &EntityPath,
        group: &[Id],
        scope: Scope,
        op: &Operation,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        self.resolve_part(path)?;
        if scope == Scope::This {
            self.separate(path)?;
        }
        let (part, p, _) = self.resolve_part(path)?;
        let original = p.clone();
        let (chain, placed) = original.group_chain(group)?;
        let last = original.def(*chain.last().expect("a group path is not empty")).ok_or(Error::NotFound(path.entity))?;
        *self.part_mut(part)? = original.group_as_part(last);
        // The part's guides, in the group's axes while the operation runs (moved, copied with
        // bodies in them), then back in the part's: those it left alone as they were, exactly.
        let guides = self.component(part)?.guides.clone();
        let into = placed.inverse();
        let inside: Vec<Guide> = guides.iter().map(|g| Guide { id: g.id, shape: g.shape.transformed(&into) }).collect();
        self.component_mut(part)?.guides = inside.clone();
        let outcome = self.apply_in_place(op, solver)?;
        let back: Vec<Guide> = self
            .component(part)?
            .guides
            .iter()
            .map(|g| match inside.iter().position(|i| i.id == g.id && i.shape == g.shape) {
                Some(k) => guides[k].clone(),
                None => Guide { id: g.id, shape: g.shape.transformed(&placed) },
            })
            .collect();
        self.component_mut(part)?.guides = back;
        let edited = self.part(part)?.clone();
        // The definitions: the group's part's, then those it never saw used elsewhere.
        let mut defs = edited.defs.clone();
        for d in &original.defs {
            if !defs.iter().any(|e| e.id == d.id) {
                defs.push(d.clone());
            }
        }
        let shape = edited.shape().clone();
        if let Some(d) = defs.iter_mut().find(|d| d.id == *chain.last().expect("a group path is not empty")) {
            d.solid = shape;
            d.group = Some(crate::BodyGroup { children: edited.bodies, failed: edited.failed });
        }
        for d in defs.iter().filter(|d| d.is_group()) {
            if crate::document::used_defs(&defs, d.children()).contains(&d.id) {
                return Err(Error::Cycle);
            }
        }
        // The groups above, innermost first.
        for id in chain.iter().rev().skip(1) {
            let Some(d) = defs.iter().find(|d| d.id == *id) else { continue };
            let children = d.children().to_vec();
            let (solid, failed) = Self::group_result(&defs, &children, solver)?;
            if let Some(d) = defs.iter_mut().find(|d| d.id == *id) {
                d.solid = solid;
                d.group = Some(crate::BodyGroup { children, failed });
            }
        }
        let mut next = original;
        next.defs = defs;
        next.drop_unused_defs();
        *self.part_mut(part)? = next;
        self.recompute(part, None, solver)?;
        Ok(outcome)
    }

    /// The part `q` as one positive body of another part, placed by `relative` in its axes,
    /// with the definitions it needs (fresh ids): its single positive body when it is plain
    /// (its profile kept), else a group of copies of its bodies, the group's result `q`'s shape.
    fn tool_as_body(&mut self, q: &Part, relative: &DAffine3) -> (Vec<BodyDef>, Body) {
        if q.is_plain() {
            let (defs, mut bodies) = self.copies_of_bodies(&q.defs, &q.bodies, relative);
            return (defs, bodies.remove(0));
        }
        let (mut defs, children) = self.copies_of_bodies(&q.defs, &q.bodies, &DAffine3::IDENTITY);
        let failed = q.bodies.iter().zip(&children).filter(|(b, _)| q.failed.contains(&b.id)).map(|(_, c)| c.id).collect();
        let group = BodyDef::group(self.fresh_id(), q.shape().clone(), children, failed);
        let body = Body::new(self.fresh_id(), group.id, Sign::Positive, *relative);
        defs.push(group);
        (defs, body)
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
pub(crate) trait Solver {
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
}

//! Operations: every change to a document (docs/README.md).
//!
//! An operation is plain, serialisable data. Applying it never changes the document it is
//! called on: it gives a new one that shares everything the operation left alone.
//!
//! The kernel runs elsewhere (docs/model/kernel.md), so an operation applies in two
//! steps: [`Document::prepare`] validates it and gathers the kernel calls it needs into a
//! [`Job`] — or, needing none, is already done; [`Document::commit`] applies it with the job's
//! outputs. Both run the same code (`apply_in_place`) over a [`Solver`]: one records the calls,
//! the other hands out their results in order.
//!
//! Moving, copying, arraying and grouping never need the kernel: they only change occurrences
//! (docs/model/data-model.md, docs/model/data-model.md). Building a part and cutting it do.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use skraft_i18n::t;
use skraft_kernel::names::{Blend, FaceMap, InputNames, NodeEdges, NodeOp, NodeRefusal, RecipeNames, TreeBody, TreeNode};
use skraft_kernel::{BooleanOp, Display, EARLIER, Job, Output, Primitive, Solid, Stretch, Task};

use std::collections::{BTreeMap, BTreeSet};

use crate::assets::Asset;
use crate::clipboard::BodyFragment;
use crate::document::{
    Assembly, Behavior, BlendMode, Body, BodyDef, ComponentKind, Document, EntityPath, Family, Guide, GuideShape, Id, Material,
    MaterialKind, Occurrence, Part, Sign, Texture, is_identity,
};
use crate::drawing::{Border, Container, ElementKind, FieldOn, Paper, Projection};
use crate::props::{PropDef, PropTarget, Props, Value};
use crate::settings::Settings;
use crate::stretch::BodyStretch;
use crate::{Error, FastMap, FastSet, Refusal, Result};

/// What a change to a part touches (docs/model/data-model.md).
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

fn is_false(b: &bool) -> bool {
    !*b
}

fn identity() -> DAffine3 {
    DAffine3::IDENTITY
}

/// Every change to a document. `context` is always the id of the assembly being edited, and
/// `entities` occurrences of it — or its guides (docs/tools/tape-measure.md), which removing, moving,
/// copying and grouping take too, as the operations on bodies take the part's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Operation {
    /// A new part built from a primitive, and its first occurrence in `context`: its positive
    /// body in the part's own axes, the occurrence placed by the rigid `placement` (a box drawn on a
    /// wall, docs/tools/inferences.md; identity for an axis-aligned one) and where the primitive stood. The
    /// part painted with `material` (« Pipette », docs/tools/draw-options.md).
    AddPrimitive {
        context: Id,
        primitive: Primitive,
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        placement: DAffine3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        material: Option<Id>,
    },
    /// A part drawn by a tool: [`Operation::AddPrimitive`], the part born oriented, its longest
    /// dimension on red, its shortest on blue (docs/decisions/0054-changing-a-components-axes.md).
    DrawPrimitive {
        context: Id,
        primitive: Primitive,
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        placement: DAffine3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        material: Option<Id>,
    },
    /// Gives `component` new axes, `axes` going from them to the current ones (a direct rigid
    /// transform): nothing moves in the world (docs/decisions/0054-changing-a-components-axes.md).
    ChangeAxes { component: Id, axes: DAffine3 },
    /// « Empreindre »: each source of `pairs` (from `context`) cuts its final solid in its receiver,
    /// its walls there moved by `around` (cheeks) and `deep` (bottom); the pairs are those the
    /// kernel found sharing material (docs/decisions/0055-linked-bodies-through-imprints.md).
    Imprint { context: Id, pairs: Vec<crate::ImprintPair>, around: f64, deep: f64 },
    /// A linked body's clearance: on its cheeks, on its bottom.
    SetClearance { part: Id, body: Id, around: f64, deep: f64 },
    /// « Replacer »: each source goes back on its seats, when they agree
    /// (docs/decisions/0055-linked-bodies-through-imprints.md).
    ReseatSources { sources: Vec<crate::LinkSource> },
    /// « Séparer »: the parts receiving these sources become parts of their own, each linked
    /// body then back under the sources it has left.
    SeparateReceivers { sources: Vec<crate::LinkSource> },
    /// « Amener l'usinage ici »: the linked bodies of this source's links off, brought under it.
    BringLinkedBody { source: crate::LinkSource },
    /// « Rompre le lien »: the linked bodies `bodies` of `part` become plain bodies.
    Unlink { part: Id, bodies: Vec<Id> },
    /// « Retirer »: these linked bodies, each by its part and id, taken away with their links.
    RemoveLinked { bodies: Vec<(Id, Id)> },
    /// Moves (rotates, mirrors) occurrences of `context` by `transform`, in `context`'s axes.
    /// Rigid only: a scale changes a part, not its place (docs/model/data-model.md).
    Transform { context: Id, entities: Vec<Id>, transform: DAffine3 },
    /// Copies occurrences of `context`, moved by `transform`: more copies of the same parts.
    Duplicate { context: Id, entities: Vec<Id>, transform: DAffine3 },
    /// `count` copies of occurrences of `context`, the k-th moved by `step` applied k times (a
    /// linear array for a translation, a circular one for a rotation).
    Array { context: Id, entities: Vec<Id>, step: DAffine3, count: u32 },
    /// Copies of occurrences of `context` (or its guides), one set per placement: each copy at
    /// `placement · T`, `T` the source's as it was; the sources themselves moved by `moved`
    /// (identity but for Distribute, which moves them to the first slot). Copy along a line or
    /// a grid, Distribute (docs/tools/copy-array.md). Rigid placements only (isometries:
    /// a mirror is one, see `is_rigid`).
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
    /// Names a part or an assembly (no more automatic: docs/model/bodies.md#automatic-names).
    RenameComponent { component: Id, name: String },
    /// Names the body definition `def` of the part `part` (docs/model/bodies.md#body-names): every body
    /// of it, copies included, shows the name; empty to clear (named automatically again,
    /// by its nature: « Extrusion#2 »). Names given need not be unique. No scope: the shape is left alone.
    RenameBodyDef { part: Id, def: Id, name: String },
    /// Names the body `body` of the part `part` (its own, or a group's) by itself, as an
    /// occurrence (« Haut <Trou#1> », docs/model/bodies.md#body-names); trimmed, empty to clear.
    RenameBody { part: Id, body: Id, name: String },
    /// Moves occurrences of `context` into a new assembly, placed by one new occurrence.
    MakeAssembly { context: Id, entities: Vec<Id>, name: Option<String> },
    /// Replaces an occurrence of an assembly by the assembly's occurrences, placed by it.
    Explode { context: Id, occurrence: Id },
    /// Gives one occurrence a component of its own, a copy of the one it shared ("faire une
    /// pièce à part").
    MakeUnique { context: Id, occurrence: Id },
    /// Places one more occurrence of a component in `context`.
    PlaceOccurrence { context: Id, component: Id, transform: DAffine3 },
    /// Makes the occurrences `entities` of `context` occurrences of `component`, each one
    /// keeping its placement, its name and its visibility (« Remplacer la
    /// sélection », docs/ui/components-library.md#managing). The guides among them are left.
    ReplaceOccurrences { context: Id, entities: Vec<Id>, component: Id },
    /// Removes a part or an assembly and every occurrence of it, at any depth
    /// (docs/ui/components-library.md#managing).
    DeleteComponent { component: Id },
    /// Drops every component not in the model (placed nowhere under the root), the named ones
    /// too (« Purger les inutilisés », docs/ui/components-library.md#managing).
    PurgeUnused,
    /// `target op (∪ tools)` on each target's part, the tools brought into its axes
    /// (docs/model/kernel.md#booleans-across-contexts): on every occurrence of the part, or only the one picked
    /// (`scope`, docs/model/data-model.md). A difference adds each tool reaching the target as a negative
    /// body; a union (one target) gives it all the tools' bodies (docs/model/bodies.md); an intersection
    /// makes each target a new part of one positive body. The tools' occurrences leave the model
    /// unless `keep_tools`; `keep_targets` puts each result in a new part beside its target, left
    /// as it was; `drop_unused` drops the tools' parts left without occurrence, even named.
    Boolean {
        targets: Vec<EntityPath>,
        tools: Vec<EntityPath>,
        op: Boolean,
        #[serde(default)]
        scope: Scope,
        #[serde(default, skip_serializing_if = "is_false")]
        keep_targets: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        keep_tools: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        drop_unused: bool,
    },
    /// A primitive added to the part `target` places, as a new body of `sign` (drawn in an
    /// open part, docs/model/opening-a-part.md, docs/model/bodies.md — a negative one is a machining): built in its
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
    /// Moves (or copies, `copy`) the bodies `bodies` of the level `from` names — the part's
    /// root, or a group or body node in it (`from.group`) — to `to`, their world place kept
    /// (docs/model/bodies.md#moving-bodies, decision 0044): a body keeps its id and definition
    /// inside the part, the names of the nodes above follow it; a shared group left or entered
    /// is made unique first; each body keeps what it does (adds or takes away).
    MoveBodies {
        from: EntityPath,
        bodies: Vec<Id>,
        to: BodiesTo,
        #[serde(default, skip_serializing_if = "is_false")]
        copy: bool,
        #[serde(default)]
        scope: Scope,
    },
    /// Moves (or copies, `copy`) the occurrences `occurrences` of the assembly `from` leads
    /// to into the part `to` (at the level its path names), as bodies — parts pasted into a part
    /// (docs/tools/copy-paste.md, decision 0044) —, their world place kept.
    MoveParts {
        from: Vec<Id>,
        occurrences: Vec<Id>,
        to: EntityPath,
        #[serde(default, skip_serializing_if = "is_false")]
        copy: bool,
    },
    /// Moves (or copies, `copy`) the occurrences `occurrences` of the assembly `from` leads
    /// to into the assembly `to` leads to (empty: the model), their world place kept (decision
    /// 0044): that assembly's definition changes, every copy of it. Into itself, or into an
    /// assembly one of them holds, refused.
    MoveOccurrences {
        from: Vec<Id>,
        occurrences: Vec<Id>,
        to: Vec<Id>,
        #[serde(default, skip_serializing_if = "is_false")]
        copy: bool,
    },
    /// Gives each of the bodies of the part `target` places that shares its definition a copy of
    /// its own (docs/model/bodies.md): a change to its shape no longer shows in the others.
    MakeBodiesUnique {
        target: EntityPath,
        bodies: Vec<Id>,
        #[serde(default)]
        scope: Scope,
    },
    /// Groups bodies of the part `target` places (docs/model/bodies.md#body-groups): one body of a new
    /// group definition takes their place, the group's axes the part's. Their negatives then
    /// cut their positives only. Negatives alone make a negative group of positive children
    /// (a compound machining); otherwise the group is positive, the signs kept.
    GroupBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        #[serde(default)]
        scope: Scope,
    },
    /// Explodes groups among bodies of the part `target` places: each one's children brought
    /// up into the part, their placements and signs composed with the group's — the reach of
    /// their negatives widened to the part. A negative group holding a negative is refused
    /// (it would add material).
    UngroupBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        #[serde(default)]
        scope: Scope,
    },
    /// Undoes the body node `body` of the level `target` names, its last edge let go
    /// (docs/tools/rounds.md): every copy of it in the part, at any level, gives way to its
    /// children, as [`Operation::UngroupBodies`] brings them up. Refused whole if one cannot be.
    DissolveNode {
        target: EntityPath,
        body: Id,
        #[serde(default)]
        scope: Scope,
    },
    /// Flattens groups among bodies of the part `target` places: each one becomes a free shape,
    /// its result, without its children (docs/model/bodies.md#flattening). Only the copies
    /// given: the others of a shared group keep their tree.
    FlattenBodies {
        target: EntityPath,
        bodies: Vec<Id>,
        #[serde(default)]
        scope: Scope,
    },
    /// Flattens the part `target` places: one free shape, its result, in place of its bodies.
    FlattenPart {
        target: EntityPath,
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
    /// own, the context's or the model's, docs/tools/stretch.md): the cutters at `cuts`
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
    /// Stretches bodies of the part `target` places, each in its own axes (docs/model/bodies.md, docs/tools/stretch.md),
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
    /// Gives the body `body` of the part `target` places a new profile (docs/tools/prism.md, docs/model/bodies.md), in
    /// the body's axes: its solid is built again from it, then the part; under the scope,
    /// "only this one" making it a separate part. The body must have a profile already.
    SetProfile {
        target: EntityPath,
        body: Id,
        profile: crate::BodyProfile,
        #[serde(default)]
        scope: Scope,
    },
    /// Rounds or chamfers edges of the bodies `bodies` of the part `target` places
    /// (docs/decisions/0042-body-nodes-operators.md): a body node takes their place, where the
    /// first of them stood, they its children; its edges those nearest `picks` (points in the
    /// part's axes, on its children's result), named in its children. Negatives alone make a
    /// negative node of positive children (a machining rounded). Refused when the kernel does
    /// not build it (a size too large: [`Error::Blend`] says the largest).
    Blend {
        target: EntityPath,
        bodies: Vec<Id>,
        blend: Blend,
        size: f64,
        picks: Vec<DVec3>,
        #[serde(default)]
        scope: Scope,
    },
    /// Changes the body node `body` of the part `target` places: its kind and size, and with
    /// `picks` (not empty) its edges, taken again nearest them. Every copy of the node shows it.
    SetBlend {
        target: EntityPath,
        body: Id,
        blend: Blend,
        size: f64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        picks: Vec<DVec3>,
        #[serde(default)]
        scope: Scope,
    },
    /// Sets how `component` is stretched: its locked axes and its cutters (docs/tools/stretch.md
    /// ). Every occurrence shows it; no scope. Changes in a row to one component undo
    /// in one go.
    SetBehavior { component: Id, behavior: Behavior },
    /// Replaces the document's settings (display style, units).
    SetSettings(Settings),
    /// A new item of the catalog (docs/render/look.md): a material or a covering of `kind`, a
    /// coloration without one.
    AddMaterial {
        name: String,
        color: [u8; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<MaterialKind>,
    },
    /// Changes the type of a material or a covering, within its family
    /// (docs/decisions/0057-materials-on-the-definition-typed-sources.md).
    SetMaterialKind { material: Id, kind: MaterialKind },
    /// Renames or recolours a material.
    EditMaterial { material: Id, name: String, color: [u8; 3] },
    /// Sets a coat's blend and intensity (0 to 1); changes in a row to one coat undo in one go
    /// (docs/decisions/0057-materials-on-the-definition-typed-sources.md).
    SetCoat { material: Id, blend: BlendMode, intensity: f32 },
    /// Sets the coats a scheme lays, from the bottom up: coats of the catalog, never schemes.
    SetCoats { material: Id, coats: Vec<Id> },
    /// Removes a material: what was painted with it is not any more.
    DeleteMaterial { material: Id },
    /// Drops the materials nothing refers to (docs/ui/catalog.md).
    PurgeMaterials,
    /// Gives `material` the texture `texture` (none: its colour alone, docs/render/look.md#textures),
    /// its image among the document's — `image` added to them first, when given.
    SetTexture {
        material: Id,
        texture: Option<Texture>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image: Option<Asset>,
    },
    /// Adds the materials of another document — a `.skraftmat` ([`Document::materials_fragment`])
    /// —, their images and properties' definitions: one equal to a material here is that one.
    ImportMaterials { fragment: Arc<Document> },
    /// Sets properties of the entity `target` (docs/model/properties.md): each key to its
    /// value, or removes it (`None`). Changes in a row to the same keys of one entity undo in
    /// one go.
    SetProps { target: PropTarget, props: BTreeMap<String, Option<Value>> },
    /// Defines the property `key` — the kind of its values, its label, choices, default and
    /// where it applies —, or removes its definition (`None`: its values are kept).
    SetPropDef { key: String, def: Option<PropDef> },
    /// Paints the parts `targets` lead to from the root — an assembly's every part below it —
    /// with `material` (`None`: removed). A material is a part's, on its definition: under
    /// « only this one » (`scope`), the parts some of whose placements are not among the
    /// targets are made unique first (docs/decisions/0057-materials-on-the-definition-typed-sources.md).
    PaintParts { targets: Vec<EntityPath>, layer: Family, material: Option<Id>, scope: Scope },
    /// Hides occurrences of `context`, or shows them again (docs/tools/interaction.md). An assembly's occurrence
    /// is hidden in every occurrence of the assembly.
    SetHidden { context: Id, entities: Vec<Id>, hidden: bool },
    /// Shows every occurrence hidden, in every assembly.
    ShowAll,
    /// Paints faces of a part's shape, by their index in its record, on `layer` — their veneer
    /// or edge, or their coloration — (every occurrence of the part shows it; `None`: they show
    /// what is under again).
    PaintFaces {
        part: Id,
        faces: Vec<u32>,
        #[serde(default)]
        layer: Family,
        material: Option<Id>,
    },
    /// A construction guide added to `component` (a part or an assembly), in its axes
    /// (docs/tools/tape-measure.md). Every occurrence shows it; no scope.
    AddGuide { component: Id, shape: GuideShape },
    /// Places the content of another document in `context` (an imported file,
    /// docs/io/step-import.md): its components and materials copied, ids drawn afresh. What
    /// its root holds comes as one occurrence placed by the rigid `transform`: the root's
    /// single occurrence itself, else a new assembly named as the root.
    Insert { context: Id, fragment: Arc<Document>, transform: DAffine3 },
    /// Places what was copied ([`Document::entities_fragment`], docs/tools/copy-paste.md) in `context`, moved
    /// by the rigid `transform` from the fragment's world axes into the context's: its
    /// occurrences each on their own, its guides in the context. With `reuse` (pasting into
    /// the document it was copied from, whose ids still name the same things), the components
    /// still here are used as they are — the copies are occurrences of the same parts (docs/model/data-model.md);
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
    /// world axes into the part's (docs/tools/copy-paste.md, across the part's boundary).
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
    /// `op` on the assembly the occurrences `parent` lead to from the root: its `context` is
    /// that assembly's definition as it is when applied (docs/tools/selection.md, parts
    /// picked at any depth). With `unique`, each assembly along `parent` placed more than once
    /// first gets a definition of its own (outermost first), as « make unique » does: only the
    /// copy reached by `parent` changes. Takes the operations on occurrences of a context
    /// ([`Operation::Transform`], [`Operation::Duplicate`], [`Operation::Array`],
    /// [`Operation::Copies`], [`Operation::Delete`], [`Operation::SetHidden`],
    /// [`Operation::ReplaceOccurrences`]), and the one making a part there
    /// ([`Operation::AddPrimitive`], « Dessiner dans », docs/tools/draw-options.md).
    At { parent: Vec<Id>, unique: bool, op: Box<Operation> },
    /// A presentation added last (docs/ui/drawing.md), « Présentation n » when `name` is none,
    /// with a first sheet of `paper` if given.
    AddPresentation { name: Option<String>, paper: Option<Paper> },
    /// A presentation added from a template, its sheets keeping their names.
    AddPresentationFrom { template: Arc<Document>, name: Option<String> },
    /// A presentation renamed, or where its projected views go changed.
    SetPresentation { presentation: Id, name: String, projection: Projection },
    /// A presentation deleted, with its sheets.
    DeletePresentation { presentation: Id },
    /// A presentation moved to `index` in their order.
    MovePresentation { presentation: Id, index: usize },
    /// A sheet added last to `presentation` (none: the first, made if there is none); named
    /// « Feuille n » when `name` is none.
    AddSheet { presentation: Option<Id>, name: Option<String>, paper: Paper },
    /// A sheet renamed, or its paper, grid or border changed.
    SetSheet {
        sheet: Id,
        name: String,
        paper: Paper,
        /// Its grid's step (paper millimetres; 0: none, at most 100).
        #[serde(default)]
        grid: f64,
        /// Its frame.
        #[serde(default)]
        border: Option<Border>,
    },
    /// A sheet deleted, with its elements.
    DeleteSheet { sheet: Id },
    /// A sheet moved to `index` of presentation `presentation` (none: its own).
    MoveSheet { sheet: Id, presentation: Option<Id>, index: usize },
    /// A copy of a sheet, with its elements, put at `index` of `presentation`.
    CopySheet { sheet: Id, presentation: Id, index: usize },
    /// An element added on top of a sheet or a composition.
    AddElement { to: Container, element: ElementKind },
    /// An element changed (moved, its view turned…), of the same kind.
    SetElement { element: Id, kind: ElementKind },
    /// Elements deleted, and what hangs on them (projected views, dimensions).
    DeleteElements { elements: Vec<Id> },
    /// Elements hidden or shown, locked or not (`None`: left as it is).
    SetElementState { elements: Vec<Id>, hidden: Option<bool>, locked: Option<bool> },
    /// An element moved to `index` in its container's order (bottom first).
    MoveElement { element: Id, index: usize },
    /// A sheet added from a template's sheet (none: its first), its compositions brought in once.
    AddSheetFrom { template: Arc<Document>, sheet: Option<Id>, presentation: Option<Id>, name: Option<String> },
    /// Elements of one container gathered into a composition placed there (decision 0049), « Groupe#n »
    /// when `name` is none.
    MakeComposition { elements: Vec<Id>, name: Option<String> },
    /// A fragment of a drawing (`Document::elements_fragment`) pasted on top of `to`, moved by
    /// `offset`; `linked` when copied from this document in a state it still knows.
    PasteElements { fragment: Arc<Document>, to: Container, offset: glam::DVec2, linked: bool },
    /// Elements of a sheet, with what hangs on them, moved (or copied) onto sheet `to`.
    MoveElements { elements: Vec<Id>, to: Id, offset: glam::DVec2, copy: bool },
    /// An occurrence replaced by copies of what its composition holds.
    ExplodeOccurrence { occurrence: Id },
    /// An occurrence given a composition of its own, a copy of the one it shares.
    MakeOccurrenceUnique { occurrence: Id },
    /// A composition named.
    RenameComposition { composition: Id, name: String },
    /// A composition deleted, with its occurrences.
    DeleteComposition { composition: Id },
    /// The compositions shown on no sheet deleted.
    PurgeCompositions,
    /// A field's value set on a presentation, a sheet or an occurrence, or cleared.
    SetField { on: FieldOn, key: String, value: Option<crate::Value> },
    /// Several operations in one step, applied in order: their outcomes put together.
    Batch(Vec<Operation>),
}

impl Operation {
    /// `target op tool`, the scope's, nothing kept: the boolean of one target and one tool.
    pub fn boolean(target: EntityPath, tool: EntityPath, op: Boolean, scope: Scope) -> Operation {
        Operation::Boolean {
            targets: vec![target],
            tools: vec![tool],
            op,
            scope,
            keep_targets: false,
            keep_tools: false,
            drop_unused: false,
        }
    }

    /// A short label for menus and the undo history.
    pub fn label(&self) -> String {
        match self {
            Operation::AddPrimitive { .. } | Operation::DrawPrimitive { .. } => t!("operation.add_part"),
            Operation::ChangeAxes { .. } => t!("operation.change_axes"),
            Operation::Transform { .. } => t!("operation.transform"),
            Operation::Duplicate { .. } => t!("operation.duplicate"),
            Operation::Array { .. } => t!("operation.array"),
            Operation::Copies { moved, .. } | Operation::CopyBodies { moved, .. } if !is_identity(moved) => {
                t!("operation.distribute")
            }
            Operation::Copies { .. } | Operation::CopyBodies { .. } => t!("operation.copies"),
            Operation::Delete { .. } => t!("operation.delete"),
            Operation::RenameOccurrence { .. }
            | Operation::RenameComponent { .. }
            | Operation::RenameBodyDef { .. }
            | Operation::RenameBody { .. } => {
                t!("operation.rename")
            }
            Operation::MakeAssembly { .. } => t!("operation.make_assembly"),
            Operation::Explode { .. } => t!("operation.explode"),
            Operation::MakeUnique { .. } | Operation::MakeBodiesUnique { .. } => t!("operation.make_unique"),
            Operation::PlaceOccurrence { .. } => t!("operation.place"),
            Operation::ReplaceOccurrences { .. } => t!("operation.replace"),
            Operation::DeleteComponent { .. } => t!("operation.delete_component"),
            Operation::PurgeUnused => t!("operation.purge_unused"),
            Operation::Boolean { op: Boolean::Union, .. } => t!("operation.union"),
            Operation::Boolean { op: Boolean::Difference, .. } => t!("operation.difference"),
            Operation::Boolean { op: Boolean::Intersection, .. } => t!("operation.intersection"),
            Operation::SetSettings(_) => t!("operation.settings"),
            Operation::AddSheet { .. } => t!("operation.add_sheet"),
            Operation::SetSheet { .. } => t!("operation.set_sheet"),
            Operation::DeleteSheet { .. } => t!("operation.delete_sheet"),
            Operation::MoveSheet { .. } => t!("operation.move_sheet"),
            Operation::CopySheet { .. } => t!("operation.copy_sheet"),
            Operation::AddElement { element: ElementKind::View(_), .. } => t!("operation.add_view"),
            Operation::AddElement { .. } => t!("operation.add_element"),
            Operation::SetElement { .. } => t!("operation.set_element"),
            Operation::DeleteElements { .. } => t!("operation.delete"),
            Operation::AddSheetFrom { .. } => t!("operation.add_sheet"),
            Operation::AddPresentation { .. } | Operation::AddPresentationFrom { .. } => t!("operation.add_presentation"),
            Operation::SetPresentation { .. } | Operation::MovePresentation { .. } => t!("operation.set_presentation"),
            Operation::DeletePresentation { .. } => t!("operation.delete_presentation"),
            Operation::SetElementState { .. } | Operation::MoveElement { .. } => t!("operation.set_element"),
            Operation::MakeComposition { .. } => t!("operation.make_composition"),
            Operation::ExplodeOccurrence { .. } => t!("operation.explode_occurrence"),
            Operation::MakeOccurrenceUnique { .. } => t!("operation.make_occurrence_unique"),
            Operation::RenameComposition { .. } => t!("operation.rename_composition"),
            Operation::DeleteComposition { .. } => t!("operation.delete_composition"),
            Operation::PurgeCompositions => t!("operation.purge_compositions"),
            Operation::SetField { .. } => t!("operation.set_field"),
            Operation::PasteElements { .. } => t!("operation.paste_elements"),
            Operation::MoveElements { .. } => t!("operation.move_elements"),
            Operation::AddMaterial { .. } => t!("operation.add_material"),
            Operation::EditMaterial { .. } => t!("operation.edit_material"),
            Operation::SetMaterialKind { .. } => t!("operation.set_material_kind"),
            Operation::SetCoat { .. } | Operation::SetCoats { .. } => t!("operation.edit_material"),
            Operation::DeleteMaterial { .. } => t!("operation.delete_material"),
            Operation::PurgeMaterials => t!("operation.purge_materials"),
            Operation::SetProps { .. } => t!("operation.set_props"),
            Operation::SetTexture { .. } => t!("operation.set_texture"),
            Operation::ImportMaterials { .. } => t!("operation.import_materials"),
            Operation::SetPropDef { .. } => t!("operation.set_prop_def"),
            Operation::PaintParts { .. } | Operation::PaintFaces { .. } => t!("operation.paint"),
            Operation::SetHidden { hidden: true, .. } => t!("operation.hide"),
            Operation::SetHidden { hidden: false, .. } => t!("operation.show"),
            Operation::ShowAll => t!("operation.show_all"),
            Operation::AddBody { sign: Sign::Negative, .. } => t!("operation.add_machining"),
            Operation::AddBody { sign: Sign::Positive, .. } => t!("operation.add_body"),
            Operation::AddBody { sign: Sign::Intersect, .. } => t!("operation.add_intersect"),
            Operation::Imprint { .. } => t!("operation.add_link"),
            Operation::SetClearance { .. } => t!("operation.set_clearance"),
            Operation::ReseatSources { .. } => t!("operation.reseat_sources"),
            Operation::SeparateReceivers { .. } => t!("operation.separate_receivers"),
            Operation::BringLinkedBody { .. } => t!("operation.bring_linked_body"),
            Operation::Unlink { .. } => t!("operation.unlink"),
            Operation::RemoveLinked { .. } => t!("operation.remove_linked"),
            Operation::DeleteBodies { .. } => t!("operation.delete"),
            Operation::GroupBodies { .. } => t!("operation.group"),
            Operation::MoveBodies { copy: true, .. } => t!("operation.copy_bodies_to"),
            Operation::MoveBodies { .. } => t!("operation.move_bodies_to"),
            Operation::MoveParts { copy: true, .. } => t!("operation.copy_parts_into"),
            Operation::MoveParts { .. } => t!("operation.move_parts_into"),
            Operation::MoveOccurrences { copy: true, .. } => t!("operation.copy_occurrences_into"),
            Operation::MoveOccurrences { .. } => t!("operation.move_occurrences_into"),
            Operation::UngroupBodies { .. } | Operation::DissolveNode { .. } => t!("operation.ungroup"),
            Operation::FlattenBodies { .. } | Operation::FlattenPart { .. } => t!("common.flatten"),
            Operation::SetSign { sign: Sign::Positive, .. } => t!("operation.set_sign_positive"),
            Operation::SetSign { sign: Sign::Negative, .. } => t!("operation.set_sign_negative"),
            Operation::SetSign { sign: Sign::Intersect, .. } => t!("operation.set_sign_intersect"),
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
            Operation::Blend { blend: Blend::Round, .. } => t!("operation.round"),
            Operation::Blend { blend: Blend::Chamfer, .. } => t!("operation.chamfer"),
            Operation::SetBlend { .. } => t!("operation.set_blend"),
            Operation::At { op, .. } => op.label(),
            Operation::Batch(ops) => ops.first().map_or_else(|| t!("operation.batch"), |op| op.label()),
        }
    }

    /// The context an operation on occurrences of a context changes, to set; none for the
    /// others.
    fn context_mut(&mut self) -> Option<&mut Id> {
        match self {
            Operation::Transform { context, .. }
            | Operation::Duplicate { context, .. }
            | Operation::Array { context, .. }
            | Operation::Copies { context, .. }
            | Operation::Delete { context, .. }
            | Operation::SetHidden { context, .. }
            | Operation::MakeAssembly { context, .. }
            | Operation::Explode { context, .. }
            | Operation::MakeUnique { context, .. }
            | Operation::ReplaceOccurrences { context, .. }
            | Operation::AddPrimitive { context, .. }
            | Operation::DrawPrimitive { context, .. } => Some(context),
            _ => None,
        }
    }

    /// The part (or group of bodies, docs/model/bodies.md#body-groups) an operation on bodies changes,
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
            | Operation::Blend { target, scope, .. }
            | Operation::SetBlend { target, scope, .. }
            | Operation::PasteBodies { target, scope, .. }
            | Operation::PasteParts { target, scope, .. }
            | Operation::GroupBodies { target, scope, .. }
            | Operation::UngroupBodies { target, scope, .. }
            | Operation::FlattenBodies { target, scope, .. } => Some((target, *scope)),
            _ => None,
        }
    }

    /// Does this step fold into the previous one `prev` in the undo history? (Settings
    /// changed in a row, e.g. while dragging a colour, undo in one go.)
    pub fn merges_into(&self, prev: &Operation) -> bool {
        match (self, prev) {
            (Operation::SetSettings(_), Operation::SetSettings(_)) => true,
            (Operation::EditMaterial { material: a, .. }, Operation::EditMaterial { material: b, .. }) => a == b,
            (Operation::SetCoat { material: a, .. }, Operation::SetCoat { material: b, .. }) => a == b,
            (Operation::SetElement { element: a, .. }, Operation::SetElement { element: b, .. }) => a == b,
            (Operation::SetBehavior { component: a, .. }, Operation::SetBehavior { component: b, .. }) => a == b,
            (Operation::SetProps { target: a, props: pa }, Operation::SetProps { target: b, props: pb }) => {
                a == b && pa.keys().eq(pb.keys())
            }
            _ => false,
        }
    }
}

/// Where [`Operation::MoveBodies`] puts the bodies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodiesTo {
    /// A level of the same part: a group or body node, by the bodies leading to it from the
    /// part's root (empty: its root).
    Level(Vec<Id>),
    /// Out of the part: a new part in the assembly the occurrences `occurrences` lead to (empty:
    /// the model; the part's own context: « Sortir de la pièce »), as bodies pasted out of a
    /// part (docs/tools/copy-paste.md).
    NewPart(Vec<Id>),
    /// Into another part, at the level its path names (its `group`), as bodies pasted into it;
    /// the part's other occurrence (the same part): its level, as [`BodiesTo::Level`].
    Part(EntityPath),
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
    /// The parts a stretch changed, and how (docs/tools/stretch.md).
    pub stretched: Vec<crate::StretchedPart>,
    /// Done, but the user should know something about it (the status bar says it).
    pub warning: Option<Warning>,
    /// The faces put onto others after it (docs/model/bodies.md's welding), for the journal.
    pub welds: Vec<crate::Weld>,
    /// Bodies moved in the tree of the bodies it edited, by their paths from there, old →
    /// new (grouped, ungrouped, wrapped by a node, renamed in a copy): the names of the nodes
    /// above follow them (docs/decisions/0042-body-nodes-operators.md).
    pub relocated: Vec<(Vec<Id>, Vec<Id>)>,
    /// The sheet added (docs/ui/drawing.md).
    pub sheet: Option<Id>,
    /// The element added to a sheet or a composition, or the occurrence a group became.
    pub element: Option<Id>,
    /// The presentation added.
    pub presentation: Option<Id>,
    /// The composition an occurrence was given.
    pub composition: Option<Id>,
    /// The elements an occurrence exploded into.
    pub elements: Vec<Id>,
    /// What the linked bodies did after it (docs/decisions/0055-linked-bodies-through-imprints.md).
    pub links: crate::Followed,
    /// « Empreindre »: the machinings it added, the links it made.
    pub imprinted: (usize, usize),
}

/// What an operation done has to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// Properties pasted or imported renamed, old → new: here, their key was defined otherwise
    /// (docs/model/properties.md).
    PropsRenamed(Vec<(String, String)>),
    /// Bodies moved in the tree (decision 0044): the shared groups made unique on the way, the
    /// nodes left in error (their edges on a body gone), by name.
    Moved { made_unique: Vec<String>, in_error: Vec<String> },
    /// Sources not put back: their links ask for places `gap` apart
    /// (docs/decisions/0055-linked-bodies-through-imprints.md).
    NotReseated { names: Vec<String>, gap: String },
}

impl Warning {
    /// The warning of keys `renamed`, if any.
    fn renamed(renamed: BTreeMap<String, String>) -> Option<Warning> {
        (!renamed.is_empty()).then(|| Warning::PropsRenamed(renamed.into_iter().collect()))
    }
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Warning::PropsRenamed(keys) => {
                let keys: Vec<String> = keys.iter().map(|(old, new)| format!("{old} → {new}")).collect();
                f.write_str(&t!("warning.props_renamed", keys = keys.join(", ")))
            }
            Warning::Moved { made_unique, in_error } => {
                let mut said = Vec::new();
                if !made_unique.is_empty() {
                    said.push(t!("warning.made_unique", names = made_unique.join(", ")));
                }
                if !in_error.is_empty() {
                    said.push(t!("warning.nodes_lost_edges", names = in_error.join(", ")));
                }
                f.write_str(&said.join(" · "))
            }
            Warning::NotReseated { names, gap } => f.write_str(&t!("warning.not_reseated", names = names.join(", "), gap = gap)),
        }
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

/// What [`Outcome::relocated`] holds.
type Relocated = Vec<(Vec<Id>, Vec<Id>)>;

impl Document {
    /// Validates `op` and gathers the kernel calls it needs. On error the document is unchanged.
    pub fn prepare(&self, op: &Operation) -> Result<Prepared> {
        let mut recorder = Recorder::default();
        let mut doc = self.clone();
        let mut outcome = doc.apply_in_place(op, &mut recorder)?;
        outcome.welds = doc.weld(self, &mut recorder)?;
        doc.update_imprints(self, &mut recorder)?;
        outcome.links = doc.follow_links(self, &mut recorder)?;
        doc.drop_dead_links();
        doc.settle_link_names();
        if recorder.job.tasks.is_empty() {
            doc.settle_names(Some(self));
            doc.drop_unused_empty_assemblies();
            return Ok(Prepared::Done(Box::new(Applied { document: doc, outcome, displays: Vec::new() })));
        }
        Ok(Prepared::Kernel(recorder.job))
    }

    /// The kernel job previewing the round or chamfer `op` ([`Operation::Blend`], or a node
    /// changed, [`Operation::SetBlend`]) — the node's
    /// task alone, not the part computed again around it: its output is the node's solid, or
    /// why the kernel refuses it (docs/tools/rounds.md).
    pub fn blend_job(&self, op: &Operation) -> Result<Job> {
        let (Operation::Blend { target, .. } | Operation::SetBlend { target, .. }) = op else {
            return Err(Error::Invalid(Refusal::NotANode));
        };
        let (_, p, _) = self.resolve_container(target)?;
        let (children, node, picks, group) = match op {
            Operation::Blend { bodies, blend, size, picks, .. } => {
                let mut children: Vec<Body> = p.bodies.iter().filter(|b| bodies.contains(&b.id)).cloned().collect();
                // As the operation makes it: negatives alone, a machining rounded (intersections alone, an intersection).
                group_sign(&mut children);
                (children, NodeOp::blend_of(*blend, *size, Vec::new()), picks, None)
            }
            // A node changed: its children as they are, its edges kept unless picked again —
            // from their result cached, when it holds.
            Operation::SetBlend { body, blend, size, picks, .. } => {
                let b = p.body(*body).ok_or(Error::NotFound(*body))?;
                let g = p.def(b.def).and_then(|d| d.group.as_ref()).ok_or(Error::Invalid(Refusal::NotANode))?;
                let edges = match (&g.op, picks.is_empty()) {
                    (Some(op), _) if op.blend().is_none() => return Err(Error::Invalid(Refusal::NotANode)),
                    (Some(op), true) => op.edges().to_vec(),
                    (Some(_), false) => Vec::new(),
                    (None, _) => return Err(Error::Invalid(Refusal::NotANode)),
                };
                (g.children.clone(), NodeOp::blend_of(*blend, *size, edges), picks, Some(g))
            }
            _ => return Err(Error::Invalid(Refusal::NotANode)),
        };
        if children.is_empty() {
            return Err(Error::Invalid(Refusal::NoBodies));
        }
        let mut leaves = Vec::new();
        let children = tree(&p.defs, &children, &mut leaves)?;
        let (children, leaves) = match group {
            Some(g) => {
                let (children, leaves, _) = node_input(g, &children, &leaves);
                (children, leaves)
            }
            None => (children, leaves),
        };
        let mut recorder = Recorder::default();
        let picks: Vec<[f64; 3]> = picks.iter().map(|p| p.to_array()).collect();
        recorder.node(&leaves, &children, &node, &picks)?;
        Ok(recorder.job)
    }

    /// The children's result of the body node `node` of what `target` leads into, where its
    /// edges lie (docs/tools/rounds.md): the node's own cache or `known` when one holds for its
    /// children as they are, else the job computing it (the node's task with no edge to take:
    /// its output is that result, its face map and its display).
    pub fn node_raw(&self, target: &EntityPath, node: Id, known: Option<&Arc<crate::RawCache>>) -> Result<NodeRaw> {
        let (_, p, _) = self.resolve_container(target)?;
        let b = p.body(node).ok_or(Error::NotFound(node))?;
        let g = p.def(b.def).and_then(|d| d.group.as_ref()).ok_or(Error::Invalid(Refusal::NotANode))?;
        let op = g.op.as_ref().ok_or(Error::Invalid(Refusal::NotANode))?;
        let mut leaves = Vec::new();
        let tree = tree(&p.defs, &g.children, &mut leaves)?;
        if let Some(c) = g.raw.0.iter().chain(known).find(|c| c.holds(&tree, &leaves)) {
            return Ok(NodeRaw::Ready(c.clone()));
        }
        let (task, task_leaves, _) = node_input(g, &tree, &leaves);
        let mut recorder = Recorder::default();
        recorder.node(&task_leaves, &task, &op.clone().with_edges(Vec::new()), &[])?;
        Ok(NodeRaw::Ask(RawAsk { tree, leaves, job: recorder.job }))
    }

    /// Applies `op` with the outputs of the job [`Document::prepare`] gave for it, on this same
    /// document.
    pub fn commit(&self, op: &Operation, outputs: Vec<Output>) -> Result<Applied> {
        let mut replayer = Replayer { outputs: outputs.into_iter(), displays: Vec::new() };
        let mut doc = self.clone();
        let mut outcome = doc.apply_in_place(op, &mut replayer)?;
        outcome.welds = doc.weld(self, &mut replayer)?;
        doc.update_imprints(self, &mut replayer)?;
        outcome.links = doc.follow_links(self, &mut replayer)?;
        doc.drop_dead_links();
        doc.settle_link_names();
        if replayer.outputs.next().is_some() {
            return Err(Error::Invalid(Refusal::OutOfStep));
        }
        doc.settle_names(Some(self));
        doc.drop_unused_empty_assemblies();
        Ok(Applied { document: doc, outcome, displays: replayer.displays })
    }

    /// Applies `op` to a copy of this document, running the kernel here. On error the
    /// document is unchanged.
    #[cfg(any(test, feature = "engine"))]
    pub fn apply(&self, op: &Operation) -> Result<(Document, Outcome)> {
        let applied = match self.prepare(op)? {
            Prepared::Done(a) => *a,
            Prepared::Kernel(job) => self.commit(op, skraft_kernel::run(&job)?)?,
        };
        Ok((applied.document, applied.outcome))
    }

    pub(crate) fn apply_in_place(&mut self, op: &Operation, solver: &mut dyn Solver) -> Result<Outcome> {
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
            Operation::AddPresentation { name, paper } => {
                let id = self.add_presentation(name.as_deref())?;
                let sheet = paper.map(|p| self.add_sheet(Some(id), None, p)).transpose()?;
                Ok(Outcome { presentation: Some(id), sheet, ..Outcome::default() })
            }
            Operation::AddPresentationFrom { template, name } => {
                let id = self.add_presentation_from(template, name.as_deref())?;
                Ok(Outcome { presentation: Some(id), ..Outcome::default() })
            }
            Operation::SetPresentation { presentation, name, projection } => {
                self.set_presentation(*presentation, name, *projection).map(|_| Outcome::default())
            }
            Operation::DeletePresentation { presentation } => self.delete_presentation(*presentation).map(|_| Outcome::default()),
            Operation::MovePresentation { presentation, index } => {
                self.move_presentation(*presentation, *index).map(|_| Outcome::default())
            }
            Operation::AddSheet { presentation, name, paper } => {
                let id = self.add_sheet(*presentation, name.as_deref(), *paper)?;
                Ok(Outcome { sheet: Some(id), ..Outcome::default() })
            }
            Operation::SetSheet { sheet, name, paper, grid, border } => {
                self.set_sheet(*sheet, name, *paper, *grid, *border).map(|_| Outcome::default())
            }
            Operation::DeleteSheet { sheet } => self.delete_sheet(*sheet).map(|_| Outcome::default()),
            Operation::MoveSheet { sheet, presentation, index } => {
                self.move_sheet(*sheet, *presentation, *index).map(|_| Outcome::default())
            }
            Operation::CopySheet { sheet, presentation, index } => {
                let id = self.copy_sheet(*sheet, *presentation, *index)?;
                Ok(Outcome { sheet: Some(id), ..Outcome::default() })
            }
            Operation::AddElement { to, element } => {
                let id = self.add_element(*to, element)?;
                Ok(Outcome { element: Some(id), ..Outcome::default() })
            }
            Operation::SetElement { element, kind } => self.set_element(*element, kind).map(|_| Outcome::default()),
            Operation::DeleteElements { elements } => self.delete_elements(elements).map(|_| Outcome::default()),
            Operation::SetElementState { elements, hidden, locked } => {
                self.set_element_state(elements, *hidden, *locked).map(|_| Outcome::default())
            }
            Operation::MoveElement { element, index } => self.move_element(*element, *index).map(|_| Outcome::default()),
            Operation::AddSheetFrom { template, sheet, presentation, name } => {
                let id = self.add_sheet_from(template, *sheet, *presentation, name.as_deref())?;
                Ok(Outcome { sheet: Some(id), ..Outcome::default() })
            }
            Operation::MakeComposition { elements, name } => {
                let id = self.make_composition(elements, name.as_deref())?;
                Ok(Outcome { element: Some(id), ..Outcome::default() })
            }
            Operation::PasteElements { fragment, to, offset, linked } => {
                let elements = self.paste_elements(fragment, *to, *offset, *linked)?;
                Ok(Outcome { elements, ..Outcome::default() })
            }
            Operation::MoveElements { elements, to, offset, copy } => {
                let elements = self.move_elements(elements, *to, *offset, *copy)?;
                Ok(Outcome { elements, ..Outcome::default() })
            }
            Operation::ExplodeOccurrence { occurrence } => {
                let elements = self.explode_occurrence(*occurrence)?;
                Ok(Outcome { elements, ..Outcome::default() })
            }
            Operation::MakeOccurrenceUnique { occurrence } => {
                let composition = self.make_occurrence_unique(*occurrence)?;
                Ok(Outcome { composition: Some(composition), ..Outcome::default() })
            }
            Operation::RenameComposition { composition, name } => self.rename_composition(*composition, name).map(|_| Outcome::default()),
            Operation::DeleteComposition { composition } => self.delete_composition(*composition).map(|_| Outcome::default()),
            Operation::PurgeCompositions => {
                self.purge_compositions();
                Ok(Outcome::default())
            }
            Operation::SetField { on, key, value } => self.set_field(*on, key, value.as_ref()).map(|_| Outcome::default()),
            Operation::AddMaterial { name, color, kind } => {
                let id = self.fresh_id();
                self.materials.insert(id, Material::of_kind(name.clone(), *color, *kind));
                Ok(Outcome { material: Some(id), ..Outcome::default() })
            }
            Operation::SetMaterialKind { material, kind } => {
                let m = self.materials.get_mut(material).ok_or(Error::NotFound(*material))?;
                // Within its family only: an item never changes family.
                if m.family() != kind.family() {
                    return Err(Error::Invalid(Refusal::MaterialLayer));
                }
                m.kind = Some(*kind);
                Ok(Outcome::default())
            }
            Operation::EditMaterial { material, name, color } => {
                let m = self.materials.get_mut(material).ok_or(Error::NotFound(*material))?;
                (m.name, m.color) = (name.clone(), *color);
                Ok(Outcome::default())
            }
            Operation::SetProps { target, props } => {
                self.set_props(target, props)?;
                Ok(Outcome::default())
            }
            Operation::SetTexture { material, texture, image } => {
                self.material(*material)?;
                if let Some(a) = image {
                    self.assets.insert(a.id(), a.clone());
                }
                if let Some(t) = texture
                    && (!t.is_valid() || !self.assets.contains_key(&t.image))
                {
                    return Err(Error::Invalid(Refusal::BadTexture));
                }
                self.materials.get_mut(material).ok_or(Error::NotFound(*material))?.texture = texture.clone();
                self.prune_assets();
                Ok(Outcome::default())
            }
            Operation::ImportMaterials { fragment } => {
                if fragment.materials.is_empty() {
                    return Err(Error::Invalid(Refusal::NothingPasted));
                }
                let (_, renamed) = self.graft(&fragment.materials_fragment(&[])?, false)?;
                Ok(Outcome { warning: Warning::renamed(renamed), ..Outcome::default() })
            }
            Operation::SetPropDef { key, def } => {
                self.set_prop_def(key, def.as_ref())?;
                Ok(Outcome::default())
            }
            Operation::PurgeMaterials => {
                let referenced = self.materials_referenced();
                self.materials.retain(|id, _| referenced.contains(id));
                self.prune_assets();
                Ok(Outcome::default())
            }
            Operation::SetCoat { material, blend, intensity } => {
                let m = self.materials.get_mut(material).ok_or(Error::NotFound(*material))?;
                if m.kind != Some(MaterialKind::Coat) || !(0.0..=1.0).contains(intensity) {
                    return Err(Error::Invalid(Refusal::MaterialLayer));
                }
                (m.blend, m.intensity) = (*blend, *intensity);
                Ok(Outcome::default())
            }
            Operation::SetCoats { material, coats } => {
                let is_coat = |id: &Id| self.materials.get(id).is_some_and(|m| m.kind == Some(MaterialKind::Coat));
                if !coats.iter().all(is_coat) {
                    return Err(Error::Invalid(Refusal::MaterialLayer));
                }
                let m = self.materials.get_mut(material).ok_or(Error::NotFound(*material))?;
                if !m.is_scheme() {
                    return Err(Error::Invalid(Refusal::MaterialLayer));
                }
                m.coats = coats.clone();
                Ok(Outcome::default())
            }
            Operation::DeleteMaterial { material } => {
                self.materials.remove(material).ok_or(Error::NotFound(*material))?;
                // A coat leaves the schemes laying it (the interface said how many first).
                for scheme in self.schemes_with(*material) {
                    if let Some(m) = self.materials.get_mut(&scheme) {
                        m.coats.retain(|c| c != material);
                    }
                }
                // Unpaint what used it, copying only the parts that did.
                let users: Vec<Id> = self
                    .components
                    .values()
                    .filter(|c| c.as_part().is_some_and(|p| p.materials_held().any(|m| m == *material)))
                    .map(|c| c.id)
                    .collect();
                for id in users {
                    let p = self.part_mut(id)?;
                    p.faces.retain(|_, m| m != material);
                    p.coverings.retain(|_, m| m != material);
                    for slot in [&mut p.material, &mut p.coloration] {
                        if *slot == Some(*material) {
                            *slot = None;
                        }
                    }
                }
                self.prune_assets();
                Ok(Outcome::default())
            }
            Operation::PaintParts { targets, layer, material, scope } => {
                self.check_layer(*material, *layer, Family::Covering)?;
                self.paint_parts(targets, *layer, *material, *scope)?;
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
            Operation::PaintFaces { part, faces, layer, material } => {
                self.check_layer(*material, *layer, Family::Material)?;
                let count = self.part(*part)?.shape().record().faces.len() as u32;
                if faces.is_empty() || faces.iter().any(|f| *f >= count) {
                    return Err(Error::Invalid(Refusal::NoSuchFace));
                }
                let slots = self.part_mut(*part)?.faces_mut(*layer).ok_or(Error::Invalid(Refusal::MaterialLayer))?;
                for f in faces {
                    match material {
                        Some(m) => slots.insert(*f, *m),
                        None => slots.remove(f),
                    };
                }
                Ok(Outcome::default())
            }
            Operation::Insert { context, fragment, transform } => {
                self.assembly(*context)?;
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::PlacementNotRigid));
                }
                let (mut top, renamed) = self.graft(fragment, false)?;
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
                            hidden: false,
                            props: Props::default(),
                        }
                    }
                };
                let id = placed.id;
                self.assembly_mut(*context)?.occurrences_mut().push(placed);
                Ok(Outcome { created: vec![id], warning: Warning::renamed(renamed), ..Outcome::default() })
            }
            Operation::Paste { context, fragment, transform, reuse } => {
                self.assembly(*context)?;
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                let sources: Vec<Guide> = fragment.component(fragment.root)?.guides.clone();
                let (top, renamed) = self.graft(fragment, *reuse)?;
                if top.is_empty() && sources.is_empty() {
                    return Err(Error::Invalid(Refusal::NothingPasted));
                }
                // Reused, an assembly may be the context or hold it.
                if top.iter().any(|o| self.contains_component(o.component, *context)) {
                    return Err(Error::Cycle);
                }
                let mut created: Vec<Id> = top.iter().map(|o| o.id).collect();
                let placed = top.into_iter().map(|o| Occurrence { transform: *transform * o.transform, ..o });
                self.assembly_mut(*context)?.occurrences_mut().extend(placed);
                let guides: Vec<Guide> = sources.iter().map(|g| g.moved(self.fresh_id(), transform)).collect();
                created.extend(guides.iter().map(|g| g.id));
                self.component_mut(*context)?.guides.extend(guides);
                Ok(Outcome { created, warning: Warning::renamed(renamed), ..Outcome::default() })
            }
            Operation::PasteBodies { target, fragment, transform, scope } => {
                let (fragment, renamed) = self.adopt_fragment_props(fragment);
                let outcome = self.paste_bodies(target, &fragment, transform, *scope, solver)?;
                Ok(Outcome { warning: Warning::renamed(renamed), ..outcome })
            }
            Operation::PasteParts { target, fragment, transform, scope } => {
                let parts = fragment.parts_as_bodies()?;
                let (parts, renamed) = self.adopt_fragment_props(&parts);
                let outcome = self.paste_bodies(target, &parts, transform, *scope, solver)?;
                Ok(Outcome { warning: Warning::renamed(renamed), ..outcome })
            }
            Operation::PasteAsPart { context, fragment, transform } => {
                self.assembly(*context)?;
                if !is_rigid(transform) {
                    return Err(Error::Invalid(Refusal::NotRigid));
                }
                if fragment.bodies.is_empty() {
                    return Err(Error::Invalid(Refusal::NothingPasted));
                }
                let (fragment, renamed) = self.adopt_fragment_props(fragment);
                let (defs, mut bodies) = self.copies_of_bodies(&fragment.defs, &fragment.bodies, &DAffine3::IDENTITY);
                // No positive among them (machinings, intersections): all positive.
                if bodies.iter().all(|b| b.sign != Sign::Positive) {
                    bodies.iter_mut().for_each(|b| b.sign = Sign::Positive);
                }
                // A single body named: the new part takes its name (docs/model/bodies.md#body-names).
                let base = match &bodies[..] {
                    [b] => defs.iter().find(|d| d.id == b.def).filter(|d| d.named).map(|d| d.name.clone()),
                    _ => None,
                };
                let base = base.unwrap_or_else(|| t!("default_name.part"));
                // Its own name, the occurrence's now.
                let occurrence_name = match &mut bodies[..] {
                    [b] => std::mem::take(&mut b.name),
                    _ => String::new(),
                };
                let part = Part {
                    defs,
                    bodies,
                    shape: None,
                    failed: Default::default(),
                    material: None,
                    coloration: None,
                    coverings: Default::default(),
                    faces: Default::default(),
                };
                let part = self.insert_component(&base, false, ComponentKind::Part(part));
                self.component_mut(part)?.guides = fragment.guides.iter().map(|g| Guide { id: self.fresh_id(), ..g.clone() }).collect();
                self.recompute(part, None, solver)?;
                let id = self.fresh_id();
                self.assembly_mut(*context)?.occurrences_mut().push(Occurrence {
                    id,
                    name: occurrence_name,
                    component: part,
                    transform: *transform * fragment.world,
                    hidden: false,
                    props: Props::default(),
                });
                Ok(Outcome { created: vec![id], warning: Warning::renamed(renamed), ..Outcome::default() })
            }
            Operation::AddGuide { component, shape } => {
                self.component(*component)?;
                if !shape.is_valid() {
                    return Err(Error::Invalid(Refusal::BadGuide));
                }
                let id = self.fresh_id();
                self.component_mut(*component)?.guides.push(Guide { id, shape: *shape, props: Props::default() });
                Ok(Outcome { guide: Some(id), ..Outcome::default() })
            }
            Operation::AddPrimitive { context, primitive, placement, material } => {
                self.assembly(*context)?;
                if !is_rigid(placement) {
                    return Err(Error::Invalid(Refusal::PlacementNotRigid));
                }
                // A material where its kind says, on the part (a face's, nowhere).
                let layer = match material {
                    Some(m) => Some(self.material(*m)?.family()),
                    None => None,
                };
                let (local, at) = primitive.at_origin();
                let solid = solver.primitive(&local)?;
                // The shapes drawn keep their profile (docs/tools/prism.md, step 3).
                let def = BodyDef { profile: crate::BodyProfile::of(&local), ..BodyDef::new(self.fresh_id(), solid) };
                let body = Body::new(self.fresh_id(), def.id, Sign::Positive, DAffine3::IDENTITY);
                let mut part = Part::of_body(def, body);
                if let Some(slot) = layer.and_then(|l| part.slot_mut(l)) {
                    *slot = *material;
                }
                let part = self.insert_component(&t!("default_name.part"), false, ComponentKind::Part(part));
                let id = self.fresh_id();
                let transform = *placement * DAffine3::from_translation(at);
                self.assembly_mut(*context)?.occurrences_mut().push(Occurrence {
                    id,
                    name: String::new(),
                    component: part,
                    transform,
                    hidden: false,
                    props: Props::default(),
                });
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::DrawPrimitive { context, primitive, placement, material } => {
                let born = crate::axes::oriented(primitive);
                let add = Operation::AddPrimitive {
                    context: *context,
                    primitive: born.primitive,
                    placement: *placement * born.placed,
                    material: *material,
                };
                let outcome = self.apply_in_place(&add, solver)?;
                if let Some(axes) = born.axes {
                    let made = self.assembly(*context)?.occurrence(outcome.created[0]).map(|o| o.component);
                    self.change_axes(made.ok_or(Error::NotFound(outcome.created[0]))?, &axes, solver)?;
                }
                Ok(outcome)
            }
            Operation::ChangeAxes { component, axes } => self.change_axes(*component, axes, solver).map(|_| Outcome::default()),
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
            Operation::RenameBodyDef { part, def, name } => {
                let p = self.part_mut(*part)?;
                let d = p.defs.iter_mut().find(|d| d.id == *def).ok_or(Error::NotFound(*def))?;
                // Named after its source.
                if d.source == crate::BodySource::Linked {
                    return Err(Error::Invalid(Refusal::LinkedBody));
                }
                let name = name.trim();
                d.set_name((!name.is_empty()).then(|| name.to_owned()));
                Ok(Outcome::default())
            }
            Operation::RenameBody { part, body, name } => {
                let b = self.part_mut(*part)?.any_body_mut(*body).ok_or(Error::NotFound(*body))?;
                b.name = name.trim().to_owned();
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
                let taken: Vec<Guide> =
                    self.component(*context)?.guides.iter().filter(|g| guides.contains(&g.id)).map(|g| g.moved(g.id, &to_axes)).collect();
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
                        hidden: false,
                        props: Props::default(),
                    },
                );
                let gathered: Vec<Id> = entities.iter().copied().collect();
                self.links_gathered(*context, &gathered, id);
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::Explode { context, occurrence } => {
                let o = self.occurrences_of(*context, &[*occurrence])?.remove(0);
                let contents = self.assembly(o.component).map_err(|_| Error::Invalid(Refusal::NotExploded))?.occurrences().to_vec();
                let mut copies = Vec::with_capacity(contents.len());
                let mut copied = Vec::with_capacity(contents.len());
                for c in contents {
                    let id = self.fresh_id();
                    copied.push((c.id, id));
                    // An assembly hidden leaves its content hidden.
                    copies.push(Occurrence { id, transform: o.transform * c.transform, hidden: c.hidden || o.hidden, ..c });
                }
                let created = copies.iter().map(|c| c.id).collect();
                // Its guides stay where they were, in the context.
                let guides = self.component(o.component)?.guides.clone();
                let guides: Vec<Guide> = guides.iter().map(|g| g.moved(self.fresh_id(), &o.transform)).collect();
                self.component_mut(*context)?.guides.extend(guides);
                let ctx = self.assembly_mut(*context)?;
                let at = ctx.position(*occurrence).ok_or(Error::NotFound(*occurrence))?;
                ctx.occurrences_mut().splice(at..=at, copies);
                self.links_exploded(*context, *occurrence, o.component, &copied)?;
                Ok(Outcome { created, ..Outcome::default() })
            }
            Operation::At { parent, unique, op } => {
                if *unique {
                    for k in 0..parent.len() {
                        let context = self.context_of(&parent[..k])?;
                        self.apply_in_place(&Operation::MakeUnique { context, occurrence: parent[k] }, solver)?;
                    }
                }
                let mut op = (**op).clone();
                *op.context_mut().ok_or(Error::Invalid(Refusal::NotAtPath))? = self.context_of(parent)?;
                self.apply_in_place(&op, solver)
            }
            Operation::Batch(ops) => {
                let mut all = Outcome::default();
                for op in ops {
                    let o = self.apply_in_place(op, solver)?;
                    all.created.extend(o.created);
                    all.bodies.extend(o.bodies);
                    all.material = all.material.or(o.material);
                    all.guide = all.guide.or(o.guide);
                    all.stretched.extend(o.stretched);
                    all.warning = all.warning.or(o.warning);
                    all.welds.extend(o.welds);
                    all.relocated.extend(o.relocated);
                    all.sheet = all.sheet.or(o.sheet);
                    all.element = all.element.or(o.element);
                    all.presentation = all.presentation.or(o.presentation);
                    all.composition = all.composition.or(o.composition);
                    all.elements.extend(o.elements);
                }
                Ok(all)
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
                    hidden: false,
                    props: Props::default(),
                });
                Ok(Outcome { created: vec![id], ..Outcome::default() })
            }
            Operation::ReplaceOccurrences { context, entities, component } => {
                if *component == self.root {
                    return Err(Error::Invalid(Refusal::RootPlaced));
                }
                self.component(*component)?;
                let (_, entities) = &self.split_guides(*context, entities)?;
                self.occurrences_of(*context, entities)?;
                if self.contains_component(*component, *context) {
                    return Err(Error::Cycle);
                }
                let entities = id_set(entities);
                // The assembly copied only when something changes.
                if self.assembly(*context)?.occurrences().iter().any(|o| entities.contains(&o.id) && o.component != *component) {
                    for o in self.assembly_mut(*context)?.occurrences_mut() {
                        if entities.contains(&o.id) {
                            o.component = *component;
                        }
                    }
                }
                Ok(Outcome::default())
            }
            Operation::DeleteComponent { component } => {
                if *component == self.root {
                    return Err(Error::Invalid(Refusal::RootDeleted));
                }
                self.component(*component)?;
                let holding: Vec<Id> = self
                    .components
                    .values()
                    .filter(|c| c.as_assembly().is_some_and(|a| a.occurrences().iter().any(|o| o.component == *component)))
                    .map(|c| c.id)
                    .collect();
                for id in holding {
                    self.assembly_mut(id)?.occurrences_mut().retain(|o| o.component != *component);
                }
                self.components.remove(component);
                Ok(Outcome::default())
            }
            Operation::PurgeUnused => {
                let counts = self.counts_in_model();
                self.components.retain(|id, _| counts.contains_key(id));
                Ok(Outcome::default())
            }
            Operation::Boolean { targets, tools, op, scope, keep_targets, keep_tools, drop_unused } => {
                let keep = crate::boolean::Keep { targets: *keep_targets, tools: *keep_tools, drop_unused: *drop_unused };
                self.booleans(targets, tools, *op, *scope, keep, solver)
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
                // Drawn, it keeps its profile (docs/tools/prism.md).
                let def = BodyDef { profile: crate::BodyProfile::of(&local), ..BodyDef::new(self.fresh_id(), solid) };
                let p = self.part_mut(part)?;
                p.bodies.push(Body::new(id, def.id, *sign, placement));
                p.defs.push(def);
                self.recompute(part, Some(&[id]), solver)?;
                Ok(Outcome { bodies: vec![id], ..Outcome::default() })
            }
            Operation::Imprint { context, pairs, around, deep } => self.imprint(*context, pairs, *around, *deep, solver),
            Operation::SetClearance { part, body, around, deep } => self.set_clearance(*part, *body, (*around, *deep), solver),
            Operation::ReseatSources { sources } => self.reseat_sources(sources),
            Operation::SeparateReceivers { sources } => self.separate_receivers(sources, solver),
            Operation::BringLinkedBody { source } => self.bring_linked_body(*source, solver),
            Operation::Unlink { part, bodies } => self.unlink_bodies(*part, bodies),
            Operation::RemoveLinked { bodies } => self.remove_linked(bodies, solver),
            Operation::DeleteBodies { target, bodies, scope } => {
                // The last positive ones too: the part is then empty, an error shown (docs/model/bodies.md
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
            Operation::MoveBodies { from, bodies, to, copy, scope } => {
                let bodies = if *copy { bodies.clone() } else { self.editable(self.resolve_part(from)?.0, bodies.clone())? };
                self.move_bodies(from, &bodies, to, *copy, *scope, solver)
            }
            Operation::MoveParts { from, occurrences, to, copy } => self.move_parts(from, occurrences, to, *copy, solver),
            Operation::MoveOccurrences { from, occurrences, to, copy } => self.move_occurrences(from, occurrences, to, *copy),
            Operation::GroupBodies { target, bodies, scope } => {
                let (part, _, bodies) = self.bodies_of(target, bodies, *scope)?;
                let bodies = self.editable(part, bodies)?;
                if bodies.is_empty() {
                    return Err(Error::Invalid(Refusal::NoBodies));
                }
                let id = self.group_bodies(part, &bodies, solver)?;
                let relocated = bodies.iter().map(|&b| (vec![b], vec![id, b])).collect();
                Ok(Outcome { bodies: vec![id], relocated, ..Outcome::default() })
            }
            Operation::UngroupBodies { target, bodies, scope } => {
                let (part, _, bodies) = self.bodies_of(target, bodies, *scope)?;
                let (created, relocated) = self.ungroup_bodies(part, &bodies, solver)?;
                Ok(Outcome { bodies: created, relocated, ..Outcome::default() })
            }
            Operation::DissolveNode { target, body, scope } => {
                self.check_dissolve_node(target, *body)?;
                let def = self.resolve_container(target)?.1.body(*body).ok_or(Error::NotFound(*body))?.def;
                let whole = EntityPath { group: Vec::new(), ..target.clone() };
                if *scope == Scope::This {
                    self.separate(&whole)?;
                }
                let part = self.resolve_part(&whole)?.0;
                let level = target.group.last().map(|_| self.resolve_part(&whole).and_then(|(_, p, _)| p.group_chain(&target.group)));
                let level = level.transpose()?.and_then(|(chain, _)| chain.last().copied());
                let moves = self.dissolve_node(part, def, solver)?;
                let created = moves.get(&level).into_iter().flatten().filter(|(old, _)| old[0] == *body).map(|(_, new)| new[0]).collect();
                Ok(Outcome { created, ..Outcome::default() })
            }
            Operation::FlattenBodies { target, bodies, scope } => {
                let (part, _, bodies) = self.bodies_of(target, bodies, *scope)?;
                let flat = self.flatten_bodies(part, &bodies, solver)?;
                Ok(Outcome { bodies: flat, ..Outcome::default() })
            }
            Operation::FlattenPart { target, scope } => {
                let ids: Vec<Id> = self.resolve_part(target)?.1.bodies.iter().map(|b| b.id).collect();
                let (part, _, _) = self.bodies_of(target, &ids, *scope)?;
                let p = self.part(part)?;
                if p.bodies.iter().any(|b| !p.is_sound(b)) || p.shape().is_empty() {
                    return Err(Error::Invalid(Refusal::FlattenFailed));
                }
                if !p.has_tree() {
                    return Ok(Outcome::default());
                }
                // The shape as it is: the materials of its faces, by their index, still hold.
                let shape = p.shape().clone();
                let (def, id) = (self.fresh_id(), self.fresh_id());
                let p = self.part_mut(part)?;
                p.defs = vec![BodyDef::new(def, shape)];
                p.bodies = vec![Body::new(id, def, Sign::Positive, DAffine3::IDENTITY)];
                p.shape = None;
                p.failed.clear();
                Ok(Outcome { bodies: vec![id], ..Outcome::default() })
            }
            Operation::MakeBodiesUnique { target, bodies, scope } => {
                let (part, _, bodies) = self.bodies_of(target, bodies, *scope)?;
                let mut relocated = Vec::new();
                for id in bodies {
                    let p = self.part(part)?;
                    let Some(b) = p.body(id) else { continue };
                    if p.copies_of(b.def) < 2 {
                        continue;
                    }
                    let def = p.def_of(b).clone();
                    let fresh = self.fresh_id();
                    let (copy, renamed) = self.renamed_children_map(BodyDef { id: fresh, ..def });
                    relocated.extend(renamed.into_iter().map(|(old, new)| (vec![id, old], vec![id, new])));
                    let p = self.part_mut(part)?;
                    if let Some(b) = p.bodies.iter_mut().find(|b| b.id == id) {
                        b.def = copy.id;
                    }
                    p.defs.push(copy);
                }
                Ok(Outcome { relocated, ..Outcome::default() })
            }
            Operation::SetSign { target, bodies, sign, scope } => {
                let (part, _, bodies) = self.bodies_of(target, bodies, *scope)?;
                let bodies = self.editable(part, bodies)?;
                let p = self.part(part)?;
                let flipped: Vec<Id> = p.bodies.iter().filter(|b| bodies.contains(&b.id) && b.sign != *sign).map(|b| b.id).collect();
                // The last positive ones too: the part is then empty (docs/model/bodies.md#empty-parts).
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
                let bodies = self.editable(part, bodies)?;
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
                let bodies = self.editable(part, bodies)?;
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
                let bodies = self.editable(part, bodies)?;
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
                let bodies = self.editable(part, bodies)?;
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
                let stretch = Stretch::new(*axis, cuts.clone(), moves.clone()).map_err(skraft_kernel::Error::from)?;
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
                let stretch = Stretch::new(*axis, cuts.clone(), moves.clone()).map_err(skraft_kernel::Error::from)?;
                // In the part's axes.
                let stretch = stretch.in_frame(&frame.inverse());
                let (part, guides, ids) = self.bodies_of(target, bodies, *scope)?;
                let ids = self.editable(part, ids)?;
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
                    // A corner the edit inserted gets its id: its side's name.
                    d.profile = Some(profile.clone().settled());
                }
                self.recompute(part, None, solver)?;
                Ok(Outcome::default())
            }
            Operation::Blend { target, bodies, blend, size, picks, scope } => {
                let (part, _, ids) = self.bodies_of(target, bodies, *scope)?;
                let ids = self.editable(part, ids)?;
                if ids.is_empty() || !(size.is_finite() && *size > 0.0) {
                    return Err(Error::Invalid(Refusal::NoBodies));
                }
                let p = self.part(part)?;
                let defs = p.defs.clone();
                let mut children: Vec<Body> = p.bodies.iter().filter(|b| ids.contains(&b.id)).cloned().collect();
                // Negatives alone: a machining rounded, its children the shape it takes away
                // (intersections alone, an intersection rounded).
                let sign = group_sign(&mut children);
                let op = NodeOp::blend_of(*blend, *size, Vec::new());
                let group = crate::BodyGroup { op: Some(op), ..crate::BodyGroup::plain(children, BTreeSet::new()) };
                let picks: Vec<[f64; 3]> = picks.iter().map(|p| p.to_array()).collect();
                let (solid, group, inner) = Self::computed_group(&defs, group, &picks, solver)?;
                if let Some(r) = &group.refused {
                    return Err(Error::Blend(r.clone()));
                }
                note_refused(&mut self.part_mut(part)?.defs, inner);
                let def = BodyDef { group: Some(group), ..BodyDef::new(self.fresh_id(), solid) };
                let body = Body::new(self.fresh_id(), def.id, sign, DAffine3::IDENTITY);
                let id = body.id;
                let p = self.part_mut(part)?;
                let at = p.bodies.iter().position(|b| ids.contains(&b.id)).unwrap_or(p.bodies.len());
                p.bodies.retain(|b| !ids.contains(&b.id));
                p.bodies.insert(at.min(p.bodies.len()), body);
                p.defs.push(def);
                p.failed.retain(|f| !ids.contains(f));
                self.recompute(part, None, solver)?;
                let relocated = ids.iter().map(|&b| (vec![b], vec![id, b])).collect();
                Ok(Outcome { bodies: vec![id], relocated, ..Outcome::default() })
            }
            Operation::SetBlend { target, body, blend, size, picks, scope } => {
                if !(size.is_finite() && *size > 0.0) {
                    return Err(Error::Invalid(Refusal::NotANode));
                }
                let (_, p, _) = self.resolve_part(target)?;
                let b = p.body(*body).ok_or(Error::NotFound(*body))?;
                let def = b.def;
                if p.def(def).and_then(|d| d.op()).and_then(|o| o.blend()).is_none() {
                    return Err(Error::Invalid(Refusal::NotANode));
                }
                if *scope == Scope::This {
                    self.separate(target)?;
                }
                let (part, p, _) = self.resolve_part(target)?;
                let defs = p.defs.clone();
                let mut group = defs.iter().find(|d| d.id == def).and_then(|d| d.group.clone()).ok_or(Error::NotFound(def))?;
                if let Some(op) = group.op.as_mut().and_then(|o| o.blend_mut()) {
                    op.kind = *blend;
                    op.size = *size;
                    if !picks.is_empty() {
                        op.edges.clear();
                    }
                }
                let picks: Vec<[f64; 3]> = picks.iter().map(|p| p.to_array()).collect();
                let (solid, group, inner) = Self::computed_group(&defs, group, &picks, solver)?;
                if let Some(r) = &group.refused {
                    return Err(Error::Blend(r.clone()));
                }
                note_refused(&mut self.part_mut(part)?.defs, inner);
                // Its definition's: every copy of the node shows it.
                if let Some(d) = self.part_mut(part)?.defs.iter_mut().find(|d| d.id == def) {
                    d.solid = solid;
                    d.group = Some(group);
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

    /// Stretches `part` (docs/tools/stretch.md): each body moved when in one section, stretched in its own
    /// axes across a cutter (its profile kept when it stays one, docs/tools/prism.md), and its guides;
    /// then computes it again.
    fn stretch_part(&mut self, part: Id, stretch: &Stretch, solver: &mut dyn Solver) -> Result<()> {
        self.stretch_bodies(part, None, stretch, solver)?;
        self.stretch_guides(part, stretch, None)?;
        self.recompute(part, None, solver)
    }

    /// Stretches the bodies `ids` of `part` (all of them: `None`) by `stretch`, in the part's
    /// axes (docs/model/bodies.md, after docs/tools/stretch.md' components): a body lying in one section is moved; one
    /// across a cutter changes its definition, the stretch taken in its own axes (its profile
    /// kept when it stays one, docs/tools/prism.md). Bodies of one definition stretched alike keep one;
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
    /// stretched (its profile kept when it stays one, docs/tools/prism.md); a group has its children
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
            // A turned body: along its axis only, rebuilt from its half profile (docs/tools/turned-bodies.md).
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
        let group = crate::BodyGroup { children, ..def.group.clone().unwrap_or_default() };
        // Its children stretched first (`stretched_def`), each computed on its own: no node
        // inside is computed with it.
        let (solid, group, _) = Self::computed_group(&all, group, &[], solver)?;
        Ok((BodyDef { solid, group: Some(group), ..def.clone() }, extra))
    }

    /// The part `target` places, `ids` (at least one) split into its guides and its bodies,
    /// every one in it; made its own first under the scope "only this one" when bodies change
    /// (they and the guides keep their ids, docs/model/data-model.md) — guides alone ignore the scope (docs/tools/tape-measure.md).
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
                copies.push(g.moved(self.fresh_id(), t));
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
        let guides: Vec<Guide> = fragment.guides.iter().map(|g| g.moved(self.fresh_id(), transform)).collect();
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
            // In another part it is a shape of its own: its link stays with the body it was, and
            // an imprint is no longer made of a body there.
            let mut copy = BodyDef { id: renamed[&d.id], source: crate::BodySource::Local, imprint: None, ..d.clone() };
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
    pub(crate) fn renamed_children(&mut self, def: BodyDef) -> BodyDef {
        self.renamed_children_map(def).0
    }

    /// [`Self::renamed_children`], with its children's ids, old → new.
    pub(crate) fn renamed_children_map(&mut self, mut def: BodyDef) -> (BodyDef, Vec<(Id, Id)>) {
        let mut renamed_ids = Vec::new();
        if let Some(g) = &mut def.group {
            let mut map: Vec<(Id, Id)> = Vec::with_capacity(g.children.len());
            for c in &mut g.children {
                let id = self.fresh_id();
                map.push((c.id, id));
                c.id = id;
            }
            g.failed = g.failed.iter().filter_map(|f| map.iter().find(|(old, _)| old == f).map(|(_, new)| *new)).collect();
            // The names its node and its face map give start with a child's id: theirs too, and
            // those its own made faces are named by (docs/decisions/0042-body-nodes-operators.md).
            let moves: Vec<(Vec<Id>, Vec<Id>)> = map.iter().map(|&(old, new)| (vec![old], vec![new])).collect();
            relocate(g, &[], &moves);
            renamed_ids = map;
        }
        (def, renamed_ids)
    }

    /// The result of a group of `children` (`defs` the part's): `(∪ positives) − (∪
    /// negatives)`, and the children whose boolean failed. A single positive child, unplaced,
    /// is the result: no kernel.
    /// With its face map, when every child's faces are named (a leaf, or a group with its own).
    pub(crate) fn group_result(
        defs: &[BodyDef],
        children: &[Body],
        solver: &mut dyn Solver,
    ) -> Result<(Arc<Solid>, BTreeSet<Id>, Option<FaceMap>)> {
        let def = |b: &Body| defs.iter().find(|d| d.id == b.def).ok_or(Error::NotFound(b.def));
        let solid = |b: &Body| def(b).map(|d| d.solid.clone());
        // How a child's faces are named, if they are.
        let names = |b: &Body| -> Result<Option<InputNames>> {
            let d = def(b)?;
            Ok(match (d.face_map(), &d.group) {
                (Some(m), _) => Some(InputNames::Map(m.clone())),
                (None, None) => Some(InputNames::Recipe(d.profile.as_ref().map(|p| p.primitive()))),
                (None, Some(_)) => None,
            })
        };
        if let [c] = children
            && c.sign == Sign::Positive
            && is_identity(&c.placement)
        {
            // Its result, and its faces' names from it.
            let d = def(c)?;
            let map = match names(c)? {
                Some(InputNames::Recipe(r)) => Some(skraft_kernel::names::leaf_map(d.solid.record(), r.as_ref(), c.id.0)),
                Some(InputNames::Map(m)) => {
                    Some(m.iter().map(|tags| tags.iter().map(|t| skraft_kernel::names::under(&[c.id.0], t)).collect()).collect())
                }
                None => None,
            };
            return Ok((solid(c)?, BTreeSet::new(), map));
        }
        let of = |s: Sign| children.iter().filter(move |b| b.sign == s).collect::<Vec<&Body>>();
        let (pos, neg, lim) = (of(Sign::Positive), of(Sign::Negative), of(Sign::Intersect));
        let placed = |list: &[&Body]| list.iter().map(|b| Ok((solid(b)?, b.placement))).collect::<Result<Vec<_>>>();
        let named_list = |list: &[&Body]| list.iter().map(|b| Ok(names(b)?.map(|n| (b.id.0, n)))).collect::<Result<Option<Vec<_>>>>();
        let names = match (named_list(&pos)?, named_list(&neg)?, named_list(&lim)?) {
            (Some(positives), Some(negatives), Some(intersects)) => Some(RecipeNames { positives, negatives, intersects }),
            _ => None,
        };
        let (shape, failed, faces) = solver.named_recipe(&placed(&pos)?, &placed(&neg)?, &placed(&lim)?, names)?;
        let named: Vec<Id> = pos.iter().chain(&neg).chain(&lim).map(|b| b.id).collect();
        Ok((shape, failed.iter().filter_map(|k| named.get(*k).copied()).collect(), faces))
    }

    /// `group` computed again (`defs` the part's): a plain group's result, or a body node's —
    /// its children computed from their leaves, then its operation, its edges by their names
    /// or (`picks` given) nearest those points, named (docs/decisions/0042-body-nodes-operators.md).
    /// The group returned with its failures, its node's edges and why the node was refused.
    pub(crate) fn computed_group(
        defs: &[BodyDef],
        group: crate::BodyGroup,
        picks: &[[f64; 3]],
        solver: &mut dyn Solver,
    ) -> Result<(Arc<Solid>, crate::BodyGroup, InnerRefusals)> {
        let Some(op) = group.op.clone() else {
            let (solid, failed, faces) = Self::group_result(defs, &group.children, solver)?;
            let group = crate::BodyGroup { failed, refused: None, ..group }.with_faces(&solid, faces);
            return Ok((solid, group, Vec::new()));
        };
        let mut leaves = Vec::new();
        let children = tree(defs, &group.children, &mut leaves)?;
        // From its children's result when it holds: their booleans not done again.
        let (task, task_leaves, cached) = node_input(&group, &children, &leaves);
        let out = solver.node(&task_leaves, &task, &op, picks)?;
        let mut failed: BTreeSet<Id> = out.failed.iter().filter_map(|p| p.first()).map(|&i| Id(i)).collect();
        if cached {
            // Those before the cutters added failed as they did.
            failed.extend(group.failed.iter().copied().filter(|f| group.children.iter().any(|c| c.id == *f)));
        }
        // Its children's result kept: the one the kernel gives, or (it was given it as it is,
        // and gives none back) the one it had.
        let raw = match out.raw {
            Some((solid, faces)) => {
                crate::RawCell(Some(Arc::new(crate::RawCache { tree: children.clone(), leaves: leaves.clone(), solid, faces })))
            }
            None if cached && task.len() == 1 => group.raw.clone(),
            None => crate::RawCell::default(),
        };
        // A node inside refused: its definition, down its path of bodies.
        let def_at = |path: &[u64]| {
            let mut def = None;
            let mut bodies: &[Body] = &group.children;
            for id in path {
                let d = defs.iter().find(|d| bodies.iter().any(|b| b.id.0 == *id && b.def == d.id))?;
                def = Some(d.id);
                bodies = d.children();
            }
            def
        };
        let inner = out.inner.into_iter().filter_map(|(path, why)| Some((def_at(&path)?, why))).collect();
        let op = op.with_edges(out.edges);
        let node = crate::BodyGroup { failed, op: Some(op), refused: out.refusal, raw, ..group }.with_faces(&out.solid, Some(out.faces));
        Ok((out.solid, node, inner))
    }

    /// [`Operation::GroupBodies`]: the bodies `ids` of `part` (each one of its own) replaced
    /// by one body of a new group of them, where the first of them stood; returns its id.
    fn group_bodies(&mut self, part: Id, ids: &[Id], solver: &mut dyn Solver) -> Result<Id> {
        let p = self.part(part)?;
        let mut children: Vec<Body> = p.bodies.iter().filter(|b| ids.contains(&b.id)).cloned().collect();
        // Several intersections alone: their union would limit, not each (docs/decisions/0045-intersection-a-third-sign.md).
        if children.len() > 1 && children.iter().all(|b| b.sign == Sign::Intersect) {
            return Err(Error::Invalid(Refusal::IntersectsRegrouped));
        }
        // Negatives alone: a compound machining, its children the shape it takes away; an intersection
        // alone, the shape it limits by.
        let sign = group_sign(&mut children);
        // Negatives or intersections beside positives: they no longer act on the positives outside it.
        let mixed = children.iter().any(|b| b.sign != Sign::Positive);
        let (solid, failed, faces) = Self::group_result(&p.defs, &children, solver)?;
        let mut def = BodyDef::group(self.fresh_id(), solid, children, failed);
        def.group = def.group.take().map(|g| g.with_faces(&def.solid, faces));
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

    /// [`Operation::FlattenBodies`]: the groups among the bodies `ids` of `part` made free
    /// shapes of their results, their faces' names kept for the nodes above: the part's shape
    /// is the same, nothing to compute. A definition some copy not given still uses is left to
    /// it. Returns the bodies flattened.
    fn flatten_bodies(&mut self, part: Id, ids: &[Id], solver: &mut dyn Solver) -> Result<Vec<Id>> {
        let p = self.part(part)?.clone();
        let groups: Vec<&Body> = p.bodies.iter().filter(|b| ids.contains(&b.id) && p.def_of(b).is_group()).collect();
        if groups.iter().any(|b| !p.is_sound(b) || p.solid_of(b).is_empty()) {
            return Err(Error::Invalid(Refusal::FlattenFailed));
        }
        let mut made: BTreeMap<Id, BodyDef> = BTreeMap::new();
        for b in &groups {
            if made.contains_key(&b.def) {
                continue;
            }
            let def = p.def_of(b);
            let group = def.group.clone().expect("a group");
            let (solid, kept_faces, kept_faces_of) = match def.face_map() {
                Some(map) => (def.solid.clone(), map.clone(), group.faces_of),
                // No map yet (its solid came from elsewhere): computed, for the names.
                None => {
                    let (solid, group, _) = Self::computed_group(&p.defs, group, &[], solver)?;
                    (solid, group.faces, group.faces_of)
                }
            };
            let alone = p.all_bodies().filter(|x| x.def == b.def).all(|x| ids.contains(&x.id));
            let id = if alone { b.def } else { self.fresh_id() };
            made.insert(b.def, BodyDef { id, solid, group: None, profile: None, kept_faces, kept_faces_of, ..def.clone() });
        }
        let flat: Vec<Id> = groups.iter().map(|b| b.id).collect();
        let q = self.part_mut(part)?;
        for b in q.bodies.iter_mut().filter(|b| flat.contains(&b.id)) {
            b.def = made[&b.def].id;
        }
        for def in made.into_values() {
            match q.defs.iter_mut().find(|d| d.id == def.id) {
                Some(d) => *d = def,
                None => q.defs.push(def),
            }
        }
        q.drop_unused_defs();
        Ok(flat)
    }

    /// [`Operation::UngroupBodies`]: the groups among the bodies `ids` of `part` exploded in
    /// place — their children brought up, placements and signs composed (fresh ids when the
    /// group is shared); returns the bodies brought up.
    fn ungroup_bodies(&mut self, part: Id, ids: &[Id], solver: &mut dyn Solver) -> Result<(Vec<Id>, Relocated)> {
        let p = self.part(part)?.clone();
        let is_group = |b: &Body| ids.contains(&b.id) && p.def_of(b).is_group();
        if !p.bodies.iter().any(is_group) {
            return Ok((Vec::new(), Vec::new()));
        }
        if let Some(r) = p.bodies.iter().filter(|b| is_group(b)).find_map(|b| ungroup_refusal(&p, b)) {
            return Err(Error::Invalid(r));
        }
        let mut bodies = Vec::with_capacity(p.bodies.len());
        let mut created = Vec::new();
        let mut relocated = Vec::new();
        for b in &p.bodies {
            if !is_group(b) {
                bodies.push(b.clone());
                continue;
            }
            let shared = p.copies_of(b.def) > 1;
            for c in p.def_of(b).children() {
                let id = if shared { self.fresh_id() } else { c.id };
                bodies.push(brought_up(b, c, id));
                created.push(id);
                relocated.push((vec![b.id, c.id], vec![id]));
            }
        }
        let p = self.part_mut(part)?;
        p.bodies = bodies;
        p.drop_unused_defs();
        self.recompute(part, None, solver)?;
        Ok((created, relocated))
    }

    /// Can the node `body` of the level `target` names be undone, every copy of it in the part?
    pub fn check_dissolve_node(&self, target: &EntityPath, body: Id) -> Result<()> {
        let def = self.resolve_container(target)?.1.body(body).ok_or(Error::NotFound(body))?.def;
        let (_, p, _) = self.resolve_part(target)?;
        if p.def(def).and_then(|d| d.op()).is_none() {
            return Err(Error::Invalid(Refusal::NotANode));
        }
        match p.all_bodies().filter(|b| b.def == def).find_map(|b| ungroup_refusal(p, b)) {
            Some(r) => Err(Error::Invalid(r)),
            None => Ok(()),
        }
    }

    /// [`Operation::DissolveNode`]: every body of definition `def`, at every level of `part`,
    /// replaced by its children; the names of the nodes above follow them, the groups that
    /// held it computed again from the inside out. Returns the moves by level (`None`: the
    /// part's own bodies, else the group definition).
    fn dissolve_node(&mut self, part: Id, def: Id, solver: &mut dyn Solver) -> Result<FastMap<Option<Id>, Relocated>> {
        let mut p = self.part(part)?.clone();
        let before = p.defs.clone();
        let children = p.children_of(def).to_vec();
        // One copy keeps its children's ids; several take fresh ones, ids being unique in a part.
        let shared = p.copies_of(def) > 1;
        let mut moves: FastMap<Option<Id>, Relocated> = FastMap::default();
        let (bodies, moved) = self.bodies_brought_up(&p.bodies, def, &children, shared);
        p.bodies = bodies;
        moves.insert(None, moved);
        for d in p.defs.iter_mut().filter(|d| d.id != def) {
            let Some(g) = d.group.as_mut() else { continue };
            if g.children.iter().any(|b| b.def == def) {
                let (bodies, moved) = self.bodies_brought_up(&g.children, def, &children, shared);
                g.children = bodies;
                moves.insert(Some(d.id), moved);
            }
        }
        for d in p.defs.iter_mut().filter(|d| d.id != def) {
            if let Some(g) = d.group.as_mut() {
                follow_moves(&before, d.id, &mut Vec::new(), &moves, g);
            }
        }
        p.drop_unused_defs();
        // The groups that held it, each after those inside it.
        let mut order = Vec::new();
        for d in &before {
            held_first(&before, d.id, &mut order);
        }
        for id in order {
            let held = before.iter().find(|d| d.id == id).is_some_and(|d| crate::document::used_defs(&before, d.children()).contains(&def));
            if id == def || !held {
                continue;
            }
            let Some(group) = p.def(id).and_then(|d| d.group.clone()) else { continue };
            let (solid, group, inner) = Self::computed_group(&p.defs, group, &[], solver)?;
            note_refused(&mut p.defs, inner);
            if let Some(d) = p.defs.iter_mut().find(|d| d.id == id) {
                d.solid = solid;
                d.group = Some(group);
            }
        }
        *self.part_mut(part)? = p;
        self.recompute(part, None, solver)?;
        Ok(moves)
    }

    /// `bodies`, those of definition `def` replaced by `children` brought up, with the moves.
    fn bodies_brought_up(&mut self, bodies: &[Body], def: Id, children: &[Body], shared: bool) -> (Vec<Body>, Relocated) {
        let mut out = Vec::with_capacity(bodies.len() + children.len());
        let mut moved = Vec::new();
        for b in bodies {
            if b.def != def {
                out.push(b.clone());
                continue;
            }
            for c in children {
                let id = if shared { self.fresh_id() } else { c.id };
                out.push(brought_up(b, c, id));
                moved.push((vec![b.id, c.id], vec![id]));
            }
        }
        (out, moved)
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

    /// Computes `part`'s shape again from its bodies (docs/model/data-model.md, docs/model/bodies.md): `(∪ positives) − (∪
    /// negatives)`. When `added` is given (bodies just added) and all of one sign, the part as
    /// it is is kept: added negatives cut it, added positives are cut by every negative then
    /// joined to it (`(A ∪ B) − N = (A − N) ∪ (B − N)`); else everything is computed again. A
    /// plain part (a single positive body, unplaced) is its body, a part without bodies empty:
    /// no kernel. Without material (no positive, a machining swallowing it) the shape is empty
    /// (docs/model/bodies.md#empty-parts): the kernel says so.
    pub(crate) fn recompute(&mut self, part: Id, added: Option<&[Id]>, solver: &mut dyn Solver) -> Result<()> {
        let p = self.part(part)?;
        // Before the deferred case: no kernel here, and a part without bodies needs its shape.
        if p.is_plain() || p.bodies.is_empty() {
            let empty = p.bodies.is_empty();
            let p = self.part_mut(part)?;
            p.shape = empty.then(|| Arc::new(Solid::empty()));
            p.failed.clear();
            return Ok(());
        }
        if self.deferred == Some(part) {
            return Ok(());
        }
        let placed = |b: &Body| (p.solid_of(b).clone(), b.placement);
        let is_added = |b: &Body| added.is_some_and(|ids| ids.contains(&b.id));
        let new: Vec<&Body> = p.bodies.iter().filter(|b| is_added(b)).collect();
        let current = p.shape().clone();
        // The bodies each index of the task's failures names: its positives, its negatives, then
        // its intersections (docs/decisions/0045-intersection-a-third-sign.md).
        let all = |s: Sign| !new.is_empty() && new.iter().all(|b| b.sign == s);
        let on_current = all(Sign::Negative) || all(Sign::Intersect);
        type Lists<'a> = (Option<Arc<Solid>>, Vec<&'a Body>, Vec<&'a Body>, Vec<&'a Body>);
        let (base, positives, negatives, intersects): Lists = if all(Sign::Negative) {
            (None, Vec::new(), new, Vec::new())
        } else if all(Sign::Intersect) {
            (None, Vec::new(), Vec::new(), new)
        } else if all(Sign::Positive) {
            (Some(current.clone()), new, p.negatives().collect(), p.intersects().collect())
        } else {
            (None, p.positives().collect(), p.negatives().collect(), p.intersects().collect())
        };
        let incremental = base.is_some() || on_current;
        let mut named: Vec<Id> = positives.iter().chain(&negatives).chain(&intersects).map(|b| b.id).collect();
        let mut pos: Vec<(Arc<Solid>, DAffine3)> = positives.iter().map(|b| placed(b)).collect();
        if on_current {
            // Negatives or intersections added: they act on the part as it is (reported failed when it
            // is empty: no body of the part's).
            pos.push((current, DAffine3::IDENTITY));
            named.insert(0, Id(u64::MAX));
        }
        let neg: Vec<(Arc<Solid>, DAffine3)> = negatives.iter().map(|b| placed(b)).collect();
        let lim: Vec<(Arc<Solid>, DAffine3)> = intersects.iter().map(|b| placed(b)).collect();
        // Added positives are cut by every negative: only their failures are new.
        let fresh: Vec<Id> = if base.is_some() { positives.iter().map(|b| b.id).collect() } else { named.clone() };
        let (shape, failed) = solver.recipe(base.as_ref(), &pos, &neg, &lim)?;
        let p = self.part_mut(part)?;
        if !incremental {
            p.failed.clear();
        }
        p.failed.extend(failed.iter().filter_map(|k| named.get(*k).copied()).filter(|id| fresh.contains(id) && *id != Id(u64::MAX)));
        p.shape = Some(shape);
        Ok(())
    }

    /// `op`, an operation on the bodies of the part `path` places (no group in it), applied
    /// inside its group of bodies `group` (docs/model/bodies.md#body-groups): the group's definition is
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
        let inside: Vec<Guide> = guides.iter().map(|g| g.moved(g.id, &into)).collect();
        self.component_mut(part)?.guides = inside.clone();
        // In a node, or a group under one: the node computes the group from its children
        // afterwards, the part standing for it is not computed meanwhile.
        let in_node = chain.iter().any(|id| original.def(*id).is_some_and(|d| d.op().is_some()));
        self.deferred = in_node.then_some(part);
        let done = self.apply_in_place(op, solver);
        self.deferred = None;
        let mut outcome = done?;
        let back: Vec<Guide> = self
            .component(part)?
            .guides
            .iter()
            .map(|g| match inside.iter().position(|i| i.id == g.id && i.shape == g.shape) {
                Some(k) => guides[k].clone(),
                None => g.moved(g.id, &placed),
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
        // Bodies moved in the tree under the group: the names of the nodes on the way (the
        // group and those above it) follow them, from each node down.
        let relocated = std::mem::take(&mut outcome.relocated);
        if !relocated.is_empty() {
            for (k, id) in chain.iter().enumerate() {
                if let Some(g) = defs.iter_mut().find(|d| d.id == *id).and_then(|d| d.group.as_mut()) {
                    relocate(g, &group[k + 1..], &relocated);
                }
            }
        }
        let last = *chain.last().expect("a group path is not empty");
        let edited_shape = edited.shape().clone();
        let edited_group = crate::BodyGroup {
            children: edited.bodies,
            failed: edited.failed,
            ..defs.iter().find(|d| d.id == last).and_then(|d| d.group.clone()).unwrap_or_default()
        };
        // A node: its operation applied to its children's result (computed again from their
        // leaves, for the names).
        // Under a node too, a plain group: computed again with its face map, for the node not to
        // compute it from its leaves (docs/decisions/0042-body-nodes-operators.md).
        let under_node = chain.iter().any(|id| defs.iter().any(|d| d.id == *id && d.op().is_some()));
        let (shape, edited_group, inner) = match edited_group.op {
            Some(_) => Self::computed_group(&defs, edited_group, &[], solver)?,
            None if under_node => Self::computed_group(&defs, edited_group, &[], solver)?,
            None => {
                let group = edited_group.with_faces(&edited_shape, None);
                (edited_shape, group, Vec::new())
            }
        };
        note_refused(&mut defs, inner);
        if let Some(d) = defs.iter_mut().find(|d| d.id == last) {
            d.solid = shape;
            d.group = Some(edited_group);
        }
        for d in defs.iter().filter(|d| d.is_group()) {
            if crate::document::used_defs(&defs, d.children()).contains(&d.id) {
                return Err(Error::Cycle);
            }
        }
        // The groups above, innermost first.
        for id in chain.iter().rev().skip(1) {
            let Some(group) = defs.iter().find(|d| d.id == *id).and_then(|d| d.group.clone()) else { continue };
            let (solid, group, inner) = Self::computed_group(&defs, group, &[], solver)?;
            note_refused(&mut defs, inner);
            if let Some(d) = defs.iter_mut().find(|d| d.id == *id) {
                d.solid = solid;
                d.group = Some(group);
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
    /// `named`: the name of `q`'s part, given by the user, for the body's definition unless it
    /// has one.
    pub(crate) fn tool_as_body(&mut self, q: &Part, relative: &DAffine3, named: Option<String>) -> (Vec<BodyDef>, Body) {
        if q.is_one_body() {
            let (mut defs, mut bodies) = self.copies_of_bodies(&q.defs, &q.bodies, relative);
            let body = bodies.remove(0);
            if let Some(d) = defs.iter_mut().find(|d| d.id == body.def)
                && !d.named
                && named.is_some()
            {
                d.set_name(named);
            }
            return (defs, body);
        }
        let (mut defs, children) = self.copies_of_bodies(&q.defs, &q.bodies, &DAffine3::IDENTITY);
        let failed = q.bodies.iter().zip(&children).filter(|(b, _)| q.failed.contains(&b.id)).map(|(_, c)| c.id).collect();
        let mut group = BodyDef::group(self.fresh_id(), q.shape().clone(), children, failed);
        if named.is_some() {
            group.set_name(named);
        }
        let body = Body::new(self.fresh_id(), group.id, Sign::Positive, *relative);
        defs.push(group);
        (defs, body)
    }

    /// Makes what `path` names its own (docs/model/data-model.md, scope "only this one"): top down, every
    /// component on the way shown by other occurrences too gets a copy of its own, the
    /// occurrence repointed to it. The copies keep their occurrences' ids (unique within an
    /// assembly, docs/model/data-model.md): `path` stays valid, and names the same thing.
    pub(crate) fn separate(&mut self, path: &EntityPath) -> Result<()> {
        let mut at = self.root;
        for &i in path.occurrences.iter().chain(std::iter::once(&path.entity)) {
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

/// A move, a rotation and maybe a mirror: unit, square axes (docs/model/data-model.md).
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

/// Why the group body `b` of `p` cannot be exploded, if so.
fn ungroup_refusal(p: &Part, b: &Body) -> Option<Refusal> {
    let children = p.def_of(b).children();
    // `parent − (P − N)`: no body of the parent's takes away P but leaves N.
    if b.sign == Sign::Negative && children.iter().any(|c| c.sign == Sign::Negative) {
        return Some(Refusal::UngroupNegative);
    }
    // An intersection of several bodies: each would limit, not their union.
    if b.sign == Sign::Intersect && !matches!(children, [c] if c.sign == Sign::Positive) {
        return Some(Refusal::IntersectsRegrouped);
    }
    None
}

/// The child `c` of the group body `b` brought up to `b`'s level under the id `id`: placement
/// composed; a negative group's or an intersection's children each what the group was.
fn brought_up(b: &Body, c: &Body, id: Id) -> Body {
    let sign = if b.sign == Sign::Positive { c.sign } else { b.sign };
    Body { id, name: c.name.clone(), def: c.def, sign, placement: b.placement * c.placement, props: c.props.clone() }
}

/// [`relocate`] of `group` for the moves of every level under it, reached through the tree as
/// it was (`defs`): `prefix` is the way from `group` to the level `at`.
fn follow_moves(defs: &[BodyDef], at: Id, prefix: &mut Vec<Id>, moves: &FastMap<Option<Id>, Relocated>, group: &mut crate::BodyGroup) {
    if let Some(m) = moves.get(&Some(at)) {
        relocate(group, prefix, m);
    }
    let Some(d) = defs.iter().find(|d| d.id == at) else { return };
    for c in d.children() {
        if defs.iter().any(|x| x.id == c.def && x.is_group()) {
            prefix.push(c.id);
            follow_moves(defs, c.def, prefix, moves, group);
            prefix.pop();
        }
    }
}

/// Pushes `id` onto `order` after the group definitions it holds, each once.
fn held_first(defs: &[BodyDef], id: Id, order: &mut Vec<Id>) {
    if order.contains(&id) {
        return;
    }
    if let Some(d) = defs.iter().find(|d| d.id == id) {
        for c in d.children() {
            held_first(defs, c.def, order);
        }
        if d.is_group() {
            order.push(id);
        }
    }
}

/// The names of `group`'s node and its face map following bodies moved in the tree under it
/// (docs/decisions/0042-body-nodes-operators.md): `moves`, old path → new path, from the bodies
/// `prefix` leads to (`group`'s descendants), in their order — a tag whose path starts with
/// `prefix` + old starts with `prefix` + new.
pub(crate) fn relocate(group: &mut crate::BodyGroup, prefix: &[Id], moves: &[(Vec<Id>, Vec<Id>)]) {
    let prefix: Vec<u64> = prefix.iter().map(|i| i.0).collect();
    let moves: Vec<(Vec<u64>, Vec<u64>)> =
        moves.iter().map(|(old, new)| (old.iter().map(|i| i.0).collect(), new.iter().map(|i| i.0).collect())).collect();
    let mut follow = |t: &mut skraft_kernel::names::FaceTag| relocate_tag(t, &prefix, &moves);
    if let Some(op) = &mut group.op {
        op.tags_mut().for_each(&mut follow);
    }
    group.faces.iter_mut().flatten().for_each(&mut follow);
}

/// [`relocate`] for one tag: its path, and for a face a node on the way made (a round's, a
/// corner's), the tags it is named by — relative to that node, `prefix` from it down.
fn relocate_tag(t: &mut skraft_kernel::names::FaceTag, prefix: &[u64], moves: &[(Vec<u64>, Vec<u64>)]) {
    if let Some(below) = prefix.strip_prefix(t.path.as_slice()) {
        match &mut t.face {
            skraft_kernel::names::FaceName::Made(between) => between.iter_mut().for_each(|b| relocate_tag(b, below, moves)),
            skraft_kernel::names::FaceName::Corner(around) => around.iter_mut().for_each(|b| relocate_tag(b, below, moves)),
            _ => {}
        }
    }
    for (old, new) in moves {
        let n = prefix.len() + old.len();
        if t.path.len() >= n && t.path.starts_with(prefix) && t.path[prefix.len()..n] == old[..] {
            t.path.splice(prefix.len()..n, new.iter().copied());
        }
    }
}

/// The sign of a group (or node) made of `children`, them made what it holds: negatives alone a
/// compound machining, intersections alone a compound intersection — their children then positive, the
/// shape it takes away or limits by —; else positive, as they are.
fn group_sign(children: &mut [Body]) -> Sign {
    let alone = |s: Sign| children.iter().all(|b| b.sign == s);
    let sign = if alone(Sign::Negative) {
        Sign::Negative
    } else if alone(Sign::Intersect) {
        Sign::Intersect
    } else {
        Sign::Positive
    };
    if sign != Sign::Positive {
        children.iter_mut().for_each(|c| c.sign = Sign::Positive);
    }
    sign
}

fn tree(defs: &[BodyDef], children: &[Body], leaves: &mut Vec<Arc<Solid>>) -> Result<Vec<TreeBody>> {
    children
        .iter()
        .map(|b| {
            let d = defs.iter().find(|d| d.id == b.def).ok_or(Error::NotFound(b.def))?;
            let node = match &d.group {
                // Computed already, its faces named (a flattened group's too): as it is, not from
                // its leaves.
                _ if d.face_map().is_some() => {
                    leaves.push(d.solid.clone());
                    TreeNode::Mapped { solid: (leaves.len() - 1) as u32, faces: d.face_map().cloned().unwrap_or_default() }
                }
                Some(g) => TreeNode::Group { children: tree(defs, &g.children, leaves)?, op: g.op.clone() },
                None => {
                    leaves.push(d.solid.clone());
                    TreeNode::Leaf { solid: (leaves.len() - 1) as u32, recipe: d.profile.as_ref().map(|p| p.primitive()) }
                }
            };
            let (negative, intersect) = (b.sign == Sign::Negative, b.sign == Sign::Intersect);
            Ok(TreeBody { id: b.id.0, negative, intersect, placement: b.placement, node })
        })
        .collect()
}

/// A body node's children's result, as [`Document::node_raw`] finds it.
#[derive(Debug)]
pub enum NodeRaw {
    Ready(Arc<crate::RawCache>),
    Ask(RawAsk),
}

/// The job computing a node's children's result, and the children it is for.
#[derive(Debug)]
pub struct RawAsk {
    pub tree: Vec<TreeBody>,
    pub leaves: Vec<Arc<Solid>>,
    pub job: Job,
}

impl RawAsk {
    /// Is it for the same children as `other`?
    pub fn same(&self, other: &RawAsk) -> bool {
        self.tree == other.tree
            && self.leaves.len() == other.leaves.len()
            && self.leaves.iter().zip(&other.leaves).all(|(a, b)| Arc::ptr_eq(a, b))
    }
}

/// What the kernel gives for a body node ([`Task::Node`]).
pub(crate) struct NodeOutcome {
    pub solid: Arc<Solid>,
    pub faces: FaceMap,
    pub edges: Vec<NodeEdges>,
    pub failed: Vec<Vec<u64>>,
    pub refusal: Option<NodeRefusal>,
    /// The nodes inside computed with it that were refused (by path), and why.
    pub inner: Vec<(Vec<u64>, NodeRefusal)>,
    /// Its children's result before its operation, and its face map (none from a recording).
    pub raw: Option<(Arc<Solid>, FaceMap)>,
}

/// The children a node's task computes and their leaves: from its children's result cached
/// (`group.raw`), when it holds for `tree` (leaves `leaves`, the node's children as they are)
/// or only lacks cutters added since — then that result and those cutters —, else `tree`
/// itself. And whether it came from the cache.
fn node_input(group: &crate::BodyGroup, tree: &[TreeBody], leaves: &[Arc<Solid>]) -> (Vec<TreeBody>, Vec<Arc<Solid>>, bool) {
    let base = |solid: u32, c: &crate::RawCache| TreeBody {
        id: 0,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Base { solid, faces: c.faces.clone() },
    };
    match group.raw.0.as_deref() {
        Some(c) if c.holds(tree, leaves) => (vec![base(0, c)], vec![c.solid.clone()], true),
        Some(c) => match c.cutters(tree, leaves) {
            Some(added) => {
                let mut leaves = leaves.to_vec();
                leaves.push(c.solid.clone());
                let children = std::iter::once(base((leaves.len() - 1) as u32, c)).chain(added.iter().cloned()).collect();
                (children, leaves, true)
            }
            None => (tree.to_vec(), leaves.to_vec(), false),
        },
        None => (tree.to_vec(), leaves.to_vec(), false),
    }
}

/// The nodes inside a group computed with it that were refused: their definitions, and why
/// ([`Document::computed_group`]), for [`note_refused`].
pub(crate) type InnerRefusals = Vec<(Id, NodeRefusal)>;

/// The refusals of the nodes inside a group computed with it, on their definitions (`defs`):
/// computed from their leaves with their parent (no face map of their own), they were not
/// on their own, and would else show their last state.
pub(crate) fn note_refused(defs: &mut [BodyDef], inner: InnerRefusals) {
    for (def, why) in inner {
        if let Some(g) = defs.iter_mut().find(|d| d.id == def).and_then(|d| d.group.as_mut()) {
            g.refused = Some(why);
        }
    }
}

/// The kernel, as an operation sees it.
pub(crate) trait Solver {
    fn primitive(&mut self, primitive: &Primitive) -> Result<Arc<Solid>>;
    /// The union of the placed `positives` minus each placed negative, intersected with each
    /// placed intersection, joined to `base` when given ([`Task::Recipe`]); the solid, and the bodies
    /// (by index, positives, negatives then intersections) that failed.
    fn recipe(
        &mut self,
        base: Option<&Arc<Solid>>,
        positives: &[(Arc<Solid>, DAffine3)],
        negatives: &[(Arc<Solid>, DAffine3)],
        intersects: &[(Arc<Solid>, DAffine3)],
    ) -> Result<(Arc<Solid>, Vec<usize>)>;
    /// A group's recipe, its inputs named ([`Task::Recipe`]'s `names`): the solid, the bodies
    /// that failed, its face map when the kernel gives one.
    fn named_recipe(
        &mut self,
        positives: &[(Arc<Solid>, DAffine3)],
        negatives: &[(Arc<Solid>, DAffine3)],
        intersects: &[(Arc<Solid>, DAffine3)],
        names: Option<RecipeNames>,
    ) -> Result<(Arc<Solid>, Vec<usize>, Option<FaceMap>)>;
    /// `solid` stretched.
    fn stretch(&mut self, solid: &Arc<Solid>, stretch: &Stretch) -> Result<Arc<Solid>>;
    /// `solid` moved by the rigid `transform`, its faces in the same order.
    fn transform(&mut self, solid: &Arc<Solid>, transform: &DAffine3) -> Result<Arc<Solid>>;
    /// The tool `source`, placed by `placed` in `receiver`'s axes, cuts in it, in the axes `frame`
    /// takes them to; `previous` when they no longer meet ([`Task::Imprint`]).
    fn imprint(
        &mut self,
        source: &Arc<Solid>,
        receiver: &Arc<Solid>,
        placed: &DAffine3,
        previous: Option<&Arc<Solid>>,
        clearance: &skraft_kernel::Clearance,
    ) -> Result<Arc<Solid>>;
    /// A body node: `children` (their leaves `leaves[i]`) computed, then `op` ([`Task::Node`]).
    fn node(&mut self, leaves: &[Arc<Solid>], children: &[TreeBody], op: &NodeOp, picks: &[[f64; 3]]) -> Result<NodeOutcome>;
}

/// Records the calls into a job and answers with stand-ins (the operation only stores them,
/// or hands them to a later call: they then name that task's output, [`EARLIER`]).
#[derive(Default)]
pub(crate) struct Recorder {
    job: Job,
    /// Inputs already in the job, and the stand-ins handed out (as `EARLIER | task`), by
    /// `Arc` address.
    inputs: HashMap<*const Solid, u32>,
    /// Held so their addresses stay theirs.
    stand_ins: Vec<Arc<Solid>>,
}

impl Recorder {
    /// Asks whether `source`, placed by `placed`, and `receiver` share material: the index of
    /// the task answering ([`Task::Contact`]).
    pub(crate) fn contact(&mut self, source: &Arc<Solid>, receiver: &Arc<Solid>, placed: DAffine3) -> usize {
        let (source, receiver) = (self.input(source), self.input(receiver));
        self.job.tasks.push(Task::Contact { source, receiver, placed });
        self.job.tasks.len() - 1
    }

    /// The job recorded.
    pub(crate) fn into_job(self) -> Job {
        self.job
    }

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

    fn recipe(
        &mut self,
        base: Option<&Arc<Solid>>,
        positives: &[(Arc<Solid>, DAffine3)],
        negatives: &[(Arc<Solid>, DAffine3)],
        intersects: &[(Arc<Solid>, DAffine3)],
    ) -> Result<(Arc<Solid>, Vec<usize>)> {
        let base = base.map(|b| self.input(b));
        let positives = positives.iter().map(|(s, p)| (self.input(s), *p)).collect();
        let negatives = negatives.iter().map(|(s, p)| (self.input(s), *p)).collect();
        let intersects = intersects.iter().map(|(s, p)| (self.input(s), *p)).collect();
        Ok((self.task(Task::Recipe { base, positives, negatives, intersects, names: None })?, Vec::new()))
    }

    fn named_recipe(
        &mut self,
        positives: &[(Arc<Solid>, DAffine3)],
        negatives: &[(Arc<Solid>, DAffine3)],
        intersects: &[(Arc<Solid>, DAffine3)],
        names: Option<RecipeNames>,
    ) -> Result<(Arc<Solid>, Vec<usize>, Option<FaceMap>)> {
        let positives = positives.iter().map(|(s, p)| (self.input(s), *p)).collect();
        let negatives = negatives.iter().map(|(s, p)| (self.input(s), *p)).collect();
        let intersects = intersects.iter().map(|(s, p)| (self.input(s), *p)).collect();
        Ok((self.task(Task::Recipe { base: None, positives, negatives, intersects, names })?, Vec::new(), None))
    }

    /// Refused here already when the record cannot be stretched: the operation is refused when
    /// prepared, not when the kernel answers.
    fn stretch(&mut self, solid: &Arc<Solid>, stretch: &Stretch) -> Result<Arc<Solid>> {
        stretch.apply(solid).map_err(skraft_kernel::Error::from)?;
        let solid = self.input(solid);
        self.task(Task::Stretch { solid, stretch: stretch.clone() })
    }

    fn transform(&mut self, solid: &Arc<Solid>, transform: &DAffine3) -> Result<Arc<Solid>> {
        let solid = self.input(solid);
        self.task(Task::Transform { solid, transform: *transform })
    }

    fn imprint(
        &mut self,
        source: &Arc<Solid>,
        receiver: &Arc<Solid>,
        placed: &DAffine3,
        previous: Option<&Arc<Solid>>,
        clearance: &skraft_kernel::Clearance,
    ) -> Result<Arc<Solid>> {
        let (source, receiver) = (self.input(source), self.input(receiver));
        let previous = previous.map(|p| self.input(p));
        self.task(Task::Imprint { source, receiver, placed: *placed, previous, clearance: *clearance })
    }

    fn node(&mut self, leaves: &[Arc<Solid>], children: &[TreeBody], op: &NodeOp, picks: &[[f64; 3]]) -> Result<NodeOutcome> {
        let inputs: Vec<u32> = leaves.iter().map(|s| self.input(s)).collect();
        let mut children = children.to_vec();
        skraft_kernel::names::map_leaves(&mut children, &mut |i| inputs[i as usize]);
        let solid = self.task(Task::Node { children, op: op.clone(), picks: picks.to_vec() })?;
        Ok(NodeOutcome {
            solid,
            faces: Vec::new(),
            edges: op.edges().to_vec(),
            failed: Vec::new(),
            refusal: None,
            inner: Vec::new(),
            raw: None,
        })
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

    fn recipe(
        &mut self,
        _: Option<&Arc<Solid>>,
        _: &[(Arc<Solid>, DAffine3)],
        _: &[(Arc<Solid>, DAffine3)],
        _: &[(Arc<Solid>, DAffine3)],
    ) -> Result<(Arc<Solid>, Vec<usize>)> {
        match self.outputs.next() {
            Some(Output::Recipe { solid, display, failed, .. }) => {
                let solid = Arc::new(solid);
                self.displays.push((solid.clone(), display));
                Ok((solid, failed.into_iter().map(|k| k as usize).collect()))
            }
            _ => Err(Error::Invalid(Refusal::OutOfStep)),
        }
    }

    fn named_recipe(
        &mut self,
        _: &[(Arc<Solid>, DAffine3)],
        _: &[(Arc<Solid>, DAffine3)],
        _: &[(Arc<Solid>, DAffine3)],
        _: Option<RecipeNames>,
    ) -> Result<(Arc<Solid>, Vec<usize>, Option<FaceMap>)> {
        match self.outputs.next() {
            Some(Output::Recipe { solid, display, failed, faces }) => {
                let solid = Arc::new(solid);
                self.displays.push((solid.clone(), display));
                Ok((solid, failed.into_iter().map(|k| k as usize).collect(), faces))
            }
            _ => Err(Error::Invalid(Refusal::OutOfStep)),
        }
    }

    fn stretch(&mut self, _: &Arc<Solid>, _: &Stretch) -> Result<Arc<Solid>> {
        self.next()
    }

    fn imprint(
        &mut self,
        _: &Arc<Solid>,
        _: &Arc<Solid>,
        _: &DAffine3,
        _: Option<&Arc<Solid>>,
        _: &skraft_kernel::Clearance,
    ) -> Result<Arc<Solid>> {
        self.next()
    }

    fn transform(&mut self, _: &Arc<Solid>, _: &DAffine3) -> Result<Arc<Solid>> {
        self.next()
    }

    fn node(&mut self, _: &[Arc<Solid>], _: &[TreeBody], _: &NodeOp, _: &[[f64; 3]]) -> Result<NodeOutcome> {
        match self.outputs.next() {
            Some(Output::Node { solid, display, faces, edges, failed, refusal, inner, raw }) => {
                let solid = Arc::new(solid);
                self.displays.push((solid.clone(), display));
                let raw = raw.map(|(s, f)| (Arc::new(s), f));
                Ok(NodeOutcome { solid, faces, edges, failed, refusal, inner, raw })
            }
            _ => Err(Error::Invalid(Refusal::OutOfStep)),
        }
    }
}

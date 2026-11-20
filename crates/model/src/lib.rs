//! Skraft's document: components — parts (positive and negative bodies) and assemblies —
//! placed by occurrences, the operations that change it, its undo history and its file
//! format. See docs/model/data-model.md–docs/model/undo.md and docs/io/file-format.md.

mod assets;
mod boolean;
mod clipboard;
mod document;
pub mod expr;
mod fast_hash;
mod file;
mod history;
mod import;
pub mod measure;
mod moves;
mod ops;
mod profile;
pub mod props;
mod settings;
mod step;
mod stretch;
mod weld;

#[cfg(test)]
mod moves_tests;
#[cfg(test)]
mod node_tests;
#[cfg(test)]
mod tests;

pub use assets::{Asset, ImageKind};
pub use clipboard::BodyFragment;
pub use document::{
    Assembly, Behavior, Body, BodyDef, BodyGroup, BodySource, Component, ComponentKind, Document, EntityPath, Guide, GuidePath, GuideShape,
    Id, MAX_DEPTH, Material, Nature, Occurrence, Part, PlacedGuide, PlacedPart, RawCache, RawCell, Sign, StretchMask, Texture, name_base,
};
pub use fast_hash::{FastBuild, FastHasher, FastMap, FastSet};
pub use file::{Content, FORMAT, VERSION};
pub use history::History;
pub use import::ImportReport;
pub use ops::{Applied, BodiesTo, Boolean, MAX_ARRAY, Operation, Outcome, Prepared, Scope, Warning};
pub use profile::BodyProfile;
pub use props::{EntityKind, PropDef, PropTarget, Props, Value, ValueKind};
pub use settings::{APPROX, Display, LengthUnit, Section, Settings, Units};
use skraft_i18n::t;
pub use skraft_kernel as kernel;
pub use skraft_kernel::Primitive;
pub use step::StepReport;
pub use stretch::StretchedPart;
pub use stretch::{BodyStretch, DefChange};
pub use weld::{WELD, Weld};

/// Why an operation (or a file) was refused. Shown to the user: each kind has its text in
/// the catalog (`error.model.*`, `error.file.*`).
#[derive(Clone, Debug)]
pub enum Error {
    NotFound(Id),
    NotAPart(Id),
    NotAnAssembly(Id),
    /// An assembly would contain itself.
    Cycle,
    Invalid(Refusal),
    Kernel(kernel::Error),
    File(FileError),
    /// Refused in this part (by its name), of an assembly being changed.
    InPart {
        part: String,
        error: Box<Error>,
    },
    /// A round or a chamfer the kernel does not build (docs/decisions/0042-body-nodes-operators.md).
    Blend(kernel::names::NodeRefusal),
}

/// An operation that cannot be done as asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The kernel's outputs do not match the operation.
    OutOfStep,
    /// A placement is a move and a rotation.
    PlacementNotRigid,
    /// Only moves, rotations and mirrors place an occurrence.
    NotRigid,
    ArrayCount,
    RootRenamed,
    /// Another part or assembly has that name.
    NameTaken,
    /// Nothing to put in the assembly.
    EmptyAssembly,
    /// Only an assembly is exploded.
    NotExploded,
    RootPlaced,
    RootDeleted,
    /// A part is not combined with itself.
    SelfBoolean,
    /// A boolean names no target or no tool.
    NoOperand,
    /// A union joins its tools to one target.
    UnionTargets,
    /// No tool of a difference or an intersection reaches a target.
    NothingReached,
    /// A face index past the part's faces (docs/render/look.md).
    NoSuchFace,
    /// A change to bodies names none.
    NoBodies,
    /// A guide with no length or direction, or not finite.
    BadGuide,
    /// A stretch names no occurrence.
    NothingStretched,
    /// A behavior with an unknown mask, or cutters not increasing inside ]0, 1[.
    BadBehavior,
    /// A body's profile that crosses itself or has no height, or a body without one.
    BadProfile,
    /// A paste with nothing in it.
    NothingPasted,
    /// A negative group holding a negative body flattened: it would add material.
    UngroupNegative,
    /// A turned body stretched across its axis, or across an arc of its half profile (docs/tools/turned-bodies.md).
    TurnedAcross,
    /// Only an operation on occurrences of a context is applied at a path.
    NotAtPath,
    /// A property's key not `<namespace>.<name>` (docs/model/properties.md).
    BadPropKey,
    /// A property's value not well formed, or not as its definition says.
    BadPropValue,
    /// A property's definition not well formed.
    BadPropDef,
    /// A property's definition that values of it do not fit.
    PropDefContradicted,
    /// A texture without its image, or of a size not positive (docs/render/look.md#textures).
    BadTexture,
    /// A change to a body node's operation on a body that is none, or a size not positive.
    NotANode,
}

/// A file that cannot be opened.
#[derive(Clone, Debug, PartialEq)]
pub enum FileError {
    /// Not JSON, or not the document's shape: the parser's message.
    Unreadable(String),
    /// Not a ZIP (docs/io/file-format.md).
    NotZip,
    /// An entry the file must have, missing (its path in the ZIP).
    MissingEntry(String),
    /// Another `format` than Skraft's.
    NotSkraft(String),
    TooNew(u32),
    RootNotAssembly,
    BadComponentId(Id),
    BadBodyId(Id),
    BadOccurrenceId(Id),
    /// A guide's id taken twice in its component, past the counter, or a guide not well formed.
    BadGuide(Id),
    /// A component filed under another id than its own.
    Misfiled {
        id: Id,
        key: Id,
    },
    /// A part without a body, whose shape is missing when needed, or failed bodies unknown.
    ShapeOutOfStep(Id),
    DuplicateOccurrence {
        occurrence: Id,
        assembly: Id,
    },
    BadMaterialId(Id),
    /// Something painted with a material the document does not have.
    UnknownMaterial(Id),
    /// A component's behavior not well formed.
    BadBehavior(Id),
    /// A property, or its definition, not well formed, or a value not as its definition says
    /// (its key).
    BadProp(String),
    /// A material's texture without its image, or not well formed.
    BadTexture(Id),
    /// Materials alone (a `.skraftmat`): imported into a model, not opened as one.
    MaterialsOnly,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Error::NotFound(id) => t!("error.model.not_found", id = id),
            Error::NotAPart(id) => t!("error.model.not_a_part", id = id),
            Error::NotAnAssembly(id) => t!("error.model.not_an_assembly", id = id),
            Error::Cycle => t!("error.model.cycle"),
            Error::Invalid(r) => r.to_string(),
            Error::Kernel(e) => e.to_string(),
            Error::File(e) => e.to_string(),
            Error::InPart { part, error } => t!("error.model.in_part", part = part, error = error),
            Error::Blend(r) => blend_refusal(r),
        };
        f.write_str(&text)
    }
}

/// Why a body node was refused, for the user.
pub fn blend_refusal(r: &kernel::names::NodeRefusal) -> String {
    use kernel::names::NodeRefusal as R;
    match r {
        R::Lost(names) => skraft_i18n::tp!("error.model.blend_lost", names.len()),
        R::NoEdge => t!("error.model.blend_no_edge"),
        R::TooLarge { size: Some(size) } => {
            let size = format!("{size:.2}");
            let size = size.trim_end_matches('0').trim_end_matches('.');
            t!("error.model.blend_too_large", size = skraft_i18n::number(size))
        }
        R::TooLarge { size: None } => t!("error.model.blend_no_room"),
        R::Kernel(e) => t!("error.model.blend_failed", error = e),
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Refusal::OutOfStep => t!("error.model.out_of_step"),
            Refusal::PlacementNotRigid => t!("error.model.placement_not_rigid"),
            Refusal::NotRigid => t!("error.model.not_rigid"),
            Refusal::ArrayCount => t!("error.model.array_count"),
            Refusal::RootRenamed => t!("error.model.root_renamed"),
            Refusal::NameTaken => t!("error.model.name_taken"),
            Refusal::EmptyAssembly => t!("error.model.empty_assembly"),
            Refusal::NotExploded => t!("error.model.not_exploded"),
            Refusal::RootPlaced => t!("error.model.root_placed"),
            Refusal::RootDeleted => t!("error.model.root_deleted"),
            Refusal::SelfBoolean => t!("error.model.self_boolean"),
            Refusal::NoOperand => t!("error.model.no_operand"),
            Refusal::UnionTargets => t!("error.model.union_targets"),
            Refusal::NothingReached => t!("error.model.nothing_reached"),
            Refusal::NoSuchFace => t!("error.model.no_such_face"),
            Refusal::NoBodies => t!("error.model.no_bodies"),
            Refusal::BadGuide => t!("error.model.bad_guide"),
            Refusal::NothingStretched => t!("error.model.nothing_stretched"),
            Refusal::BadBehavior => t!("error.model.bad_behavior"),
            Refusal::BadProfile => t!("error.model.bad_profile"),
            Refusal::NothingPasted => t!("error.model.nothing_pasted"),
            Refusal::TurnedAcross => t!("error.model.turned_across"),
            Refusal::UngroupNegative => t!("error.model.ungroup_negative"),
            Refusal::NotAtPath => t!("error.model.not_at_path"),
            Refusal::BadPropKey => t!("error.model.bad_prop_key"),
            Refusal::BadPropValue => t!("error.model.bad_prop_value"),
            Refusal::BadPropDef => t!("error.model.bad_prop_def"),
            Refusal::PropDefContradicted => t!("error.model.prop_def_contradicted"),
            Refusal::BadTexture => t!("error.model.bad_texture"),
            Refusal::NotANode => t!("error.model.not_a_node"),
        })
    }
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            FileError::Unreadable(e) => t!("error.file.unreadable", error = e),
            FileError::NotZip => t!("error.file.not_zip"),
            FileError::MissingEntry(entry) => t!("error.file.missing_entry", entry = entry),
            FileError::NotSkraft(format) => t!("error.file.not_skraft", format = format),
            FileError::TooNew(version) => t!("error.file.too_new", version = version, supported = file::VERSION),
            FileError::RootNotAssembly => t!("error.file.root_not_assembly"),
            FileError::BadComponentId(id) => t!("error.file.bad_component_id", id = id),
            FileError::BadBodyId(id) => t!("error.file.bad_body_id", id = id),
            FileError::BadOccurrenceId(id) => t!("error.file.bad_occurrence_id", id = id),
            FileError::BadGuide(id) => t!("error.file.bad_guide", id = id),
            FileError::Misfiled { id, key } => t!("error.file.misfiled", id = id, key = key),
            FileError::ShapeOutOfStep(id) => t!("error.file.shape_out_of_step", id = id),
            FileError::BadMaterialId(id) => t!("error.file.bad_material_id", id = id),
            FileError::UnknownMaterial(id) => t!("error.file.unknown_material", id = id),
            FileError::BadBehavior(id) => t!("error.file.bad_behavior", id = id),
            FileError::BadProp(key) => t!("error.file.bad_prop", key = key),
            FileError::BadTexture(id) => t!("error.file.bad_texture", id = id),
            FileError::MaterialsOnly => t!("error.file.materials_only"),
            FileError::DuplicateOccurrence { occurrence, assembly } => {
                t!("error.file.duplicate_occurrence", occurrence = occurrence, assembly = assembly)
            }
        })
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Kernel(e) => Some(e),
            _ => None,
        }
    }
}

impl From<kernel::Error> for Error {
    fn from(e: kernel::Error) -> Self {
        Error::Kernel(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

//! SolidSketch's document: components — parts (a blank and its machinings) and assemblies —
//! placed by occurrences, the operations that change it, its undo history and its file
//! format. See docs/ARCHITECTURE.md §2–§5 and §8.

mod document;
pub mod expr;
mod file;
mod history;
mod import;
pub mod measure;
mod ops;
mod settings;
mod stretch;

#[cfg(test)]
mod tests;

pub use document::{
    Assembly, Behavior, Component, ComponentKind, Document, EntityPath, Guide, GuideShape, Id, MAX_DEPTH, Machining, MachiningSource,
    Material, Occurrence, Part, PlacedGuide, PlacedPart, StretchMask,
};
pub use file::{FORMAT, VERSION};
pub use history::History;
pub use import::{ImportReport, SKETCHUP_FORMAT, SKETCHUP_VERSION};
pub use ops::{Applied, Boolean, MAX_ARRAY, Operation, Outcome, Prepared, Scope};
pub use settings::{Display, LengthUnit, Section, Settings, Units};
use solidsketch_i18n::t;
pub use solidsketch_kernel as kernel;
pub use solidsketch_kernel::Primitive;
pub use stretch::StretchedPart;

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
    /// A part is not combined with itself.
    SelfBoolean,
    /// A face index past the part's faces (§7 quater).
    NoSuchFace,
    /// A change to machinings names none.
    NoMachinings,
    /// A guide with no length or direction, or not finite.
    BadGuide,
    /// A stretch names no occurrence.
    NothingStretched,
    /// A behavior with an unknown mask, or cutters not increasing inside ]0, 1[.
    BadBehavior,
}

/// A file that cannot be opened.
#[derive(Clone, Debug, PartialEq)]
pub enum FileError {
    /// Not JSON, or not the document's shape: the parser's message.
    Unreadable(String),
    /// Another `format` than SolidSketch's.
    NotSolidSketch(String),
    TooNew(u32),
    RootNotAssembly,
    BadComponentId(Id),
    BadMachiningId(Id),
    BadOccurrenceId(Id),
    /// A guide's id taken twice in its component, past the counter, or a guide not well formed.
    BadGuide(Id),
    /// A component filed under another id than its own.
    Misfiled {
        id: Id,
        key: Id,
    },
    /// A part whose shape is there without machinings, or missing with some.
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
        };
        f.write_str(&text)
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
            Refusal::SelfBoolean => t!("error.model.self_boolean"),
            Refusal::NoSuchFace => t!("error.model.no_such_face"),
            Refusal::NoMachinings => t!("error.model.no_machinings"),
            Refusal::BadGuide => t!("error.model.bad_guide"),
            Refusal::NothingStretched => t!("error.model.nothing_stretched"),
            Refusal::BadBehavior => t!("error.model.bad_behavior"),
        })
    }
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            FileError::Unreadable(e) => t!("error.file.unreadable", error = e),
            FileError::NotSolidSketch(format) => t!("error.file.not_solidsketch", format = format),
            FileError::TooNew(version) => t!("error.file.too_new", version = version, supported = file::VERSION),
            FileError::RootNotAssembly => t!("error.file.root_not_assembly"),
            FileError::BadComponentId(id) => t!("error.file.bad_component_id", id = id),
            FileError::BadMachiningId(id) => t!("error.file.bad_machining_id", id = id),
            FileError::BadOccurrenceId(id) => t!("error.file.bad_occurrence_id", id = id),
            FileError::BadGuide(id) => t!("error.file.bad_guide", id = id),
            FileError::Misfiled { id, key } => t!("error.file.misfiled", id = id, key = key),
            FileError::ShapeOutOfStep(id) => t!("error.file.shape_out_of_step", id = id),
            FileError::BadMaterialId(id) => t!("error.file.bad_material_id", id = id),
            FileError::UnknownMaterial(id) => t!("error.file.unknown_material", id = id),
            FileError::BadBehavior(id) => t!("error.file.bad_behavior", id = id),
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

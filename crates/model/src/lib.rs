//! SolidSketch's document: components — parts (a blank and its machinings) and assemblies —
//! placed by occurrences, the operations that change it, its undo history and its file
//! format. See docs/ARCHITECTURE.md §2–§5 and §8.

mod document;
pub mod expr;
mod file;
mod history;
pub mod measure;
mod ops;
mod settings;

#[cfg(test)]
mod tests;

pub use document::{
    Assembly, Component, ComponentKind, Document, EntityPath, Id, MAX_DEPTH, Machining, MachiningSource, Occurrence, Part,
    PlacedPart,
};
pub use file::{FORMAT, VERSION};
pub use history::History;
pub use ops::{Applied, Boolean, MAX_ARRAY, Operation, Outcome, Prepared, Scope};
pub use solidsketch_kernel::Primitive;
pub use settings::{Display, LengthUnit, Settings, Units};
pub use solidsketch_kernel as kernel;

/// Why an operation (or a file) was refused.
#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} not found")]
    NotFound(Id),
    #[error("{0} is not a part")]
    NotAPart(Id),
    #[error("{0} is not an assembly")]
    NotAnAssembly(Id),
    #[error("an assembly would contain itself")]
    Cycle,
    #[error("{0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Kernel(#[from] kernel::Error),
    #[error("file: {0}")]
    File(String),
}

pub type Result<T> = std::result::Result<T, Error>;

//! SolidSketch's document: definitions (the model, groups, components) holding solids and
//! instances, the operations that change it, its undo history and its file format.
//! See docs/ARCHITECTURE.md §2–§5 and §8.

mod document;
mod file;
mod history;
mod ops;
mod settings;

#[cfg(test)]
mod tests;

pub use document::{Definition, DefinitionKind, Document, Entity, EntityPath, Id, Instance, MAX_DEPTH, SolidEntity};
pub use file::{FORMAT, VERSION};
pub use history::History;
pub use ops::{Boolean, MAX_ARRAY, Operation, Outcome, Primitive};
pub use settings::{Display, LengthUnit, Settings, Units};
pub use solidsketch_kernel as kernel;

/// Why an operation (or a file) was refused.
#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} not found")]
    NotFound(Id),
    #[error("{0} is not a solid")]
    NotASolid(Id),
    #[error("{0} is not an instance")]
    NotAnInstance(Id),
    #[error("a component would contain itself")]
    Cycle,
    #[error("{0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Kernel(#[from] kernel::Error),
    #[error("file: {0}")]
    File(String),
}

pub type Result<T> = std::result::Result<T, Error>;

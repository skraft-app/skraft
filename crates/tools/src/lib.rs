//! SolidSketch's interaction layer: the camera, the display scene (mesh caches, placements,
//! picking), the editing context and selection, and the interactive tools, all gathered in an
//! [`Editor`]. No UI and no GPU here: the app forwards input and draws the [`View`].
//! See docs/ARCHITECTURE.md §6, §7, §7 bis.

pub mod camera;
mod editor;
pub mod infer;
pub mod scene;
pub mod tools;
pub mod vcb;

#[cfg(test)]
mod tests;

pub use camera::{Camera, Projection, Ray, StandardView, ViewArea};
pub use editor::{Editor, Look, Mark, View, ViewHighlight, ViewPlacement};
pub use infer::{EdgeRef, InferKind, Inference, LineKind};
pub use scene::{DefinitionMesh, DisplayScene, Hit, Owner, PartMesh, Piece, Placement, RecipeMeshes, RecipeTool};
pub use tools::{Key, Marquee, Modifiers, OptionChoice, OptionValue, OptionValues, PreviewLine, ToolInput, ToolKind, ToolOption};

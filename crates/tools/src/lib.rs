//! SolidSketch's interaction layer: the camera, the display scene (mesh caches, placements,
//! picking), the editing context and selection, and the interactive tools, all gathered in an
//! [`Editor`]. No UI and no GPU here: the app forwards input and draws the [`View`].
//! See docs/ARCHITECTURE.md §6, §7, §7 bis.

mod bvh;
pub mod camera;
pub mod draw;
mod editor;
pub mod guides;
pub mod infer;
pub mod navigation;
pub mod scene;
pub mod section_cut;
pub mod tools;
pub mod vcb;

#[cfg(test)]
mod bench;
#[cfg(test)]
mod tests;

pub use camera::{Camera, Projection, Ray, StandardView, ViewArea};
pub use editor::journal;
pub use editor::{
    Clip, Content, Editor, LoadEnd, LoadKind, Loading, Look, Mark, Naming, Notice, NoticeAction, NoticeKind, OrientedBox, SectionCut, Tree,
    TreeRow, View, ViewHighlight, ViewPlacement,
};
pub use infer::{Curve, EdgeRef, InferKind, Inference, LineKind};
pub use scene::{DefinitionMesh, DisplayScene, Hit, Owner, PartMesh, Piece, Placement, RecipeMeshes, RecipeTool, SceneGuide};
pub use tools::{
    Key, Marquee, Modifiers, OptionChoice, OptionValue, OptionValues, PointShape, PreviewLabel, ToolInput, ToolKind, ToolOption,
};

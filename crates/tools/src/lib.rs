//! Skraft's interaction layer: the camera, the display scene (mesh caches, placements,
//! picking), the editing context and selection, and the interactive tools, all gathered in an
//! [`Editor`]. No UI and no GPU here: the app forwards input and draws the [`View`].
//! See docs/tools/interaction.md, docs/README.md, docs/render/display.md.

mod bvh;
pub mod camera;
pub mod draw;
mod editor;
pub mod guides;
pub mod hidden_lines;
pub mod infer;
pub mod navigation;
pub mod scene;
pub mod search;
pub mod section_cut;
pub mod segment;
pub mod tools;
pub mod vcb;

#[cfg(test)]
mod bench;
#[cfg(test)]
mod tests;

pub use camera::{Camera, Projection, Ray, StandardView, ViewArea};
pub use editor::journal;
pub use editor::{
    Clip, Content, Editor, LinkAct, LoadEnd, LoadKind, Loading, Look, Mark, NameTarget, Naming, Notice, NoticeAction, NoticeKind,
    OrientedBox, SectionCut, Tree, TreeRow, View, ViewHighlight, ViewPlacement, cap_histories,
};
pub use infer::{Curve, EdgeRef, InferKind, Inference, LineKind};
pub use scene::{DefinitionMesh, DisplayScene, Hit, Owner, PartMesh, Piece, Placement, RecipeMeshes, RecipeTool, SceneGuide};
pub use tools::{
    Cursor, Key, Marquee, Modifiers, OptionChoice, OptionValue, OptionValues, PointShape, PreviewLabel, Steps, ToolInput, ToolKind,
    ToolOption,
};

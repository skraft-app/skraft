//! Interactive tools (docs/tools/interaction.md): each is a state machine fed with pointer and key
//! input; it previews without touching the document and, when done, emits an `Operation`.

mod blend;
mod boolean;
mod box_grips;
mod copy_grid;
mod copy_line;
mod copying;
mod distribute;
mod erase;
mod face;
pub(crate) mod handle;
pub mod imprint;
mod measure;
mod mover;
mod paint;
mod pick;
pub(crate) mod profile;
mod reorient;
mod rotate;
mod sample;
mod section;
mod select;
mod shapes;
mod stretch;

pub use blend::{BlendPreview, BlendTool};
pub use boolean::BooleanTool;
pub use copy_grid::CopyGridTool;
pub use copy_line::CopyLineTool;
pub use distribute::DistributeTool;
pub use erase::EraseTool;
pub use measure::MeasureTool;
pub use mover::MoveTool;
pub use paint::PaintTool;
pub use profile::ProfileTool;
pub use reorient::ReorientTool;
pub use rotate::RotateTool;
pub use sample::SampleTool;
pub use section::SectionTool;
pub use select::SelectTool;
pub use shapes::{BoxTool, CylinderTool, PrismTool, synced_with};
pub use stretch::StretchTool;

use std::collections::BTreeMap;

use glam::{DAffine3, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use skraft_i18n::t;
use skraft_model::{Boolean, Document, EntityPath, GuidePath, Id, Operation, Primitive, Scope, Section, Sign, Units};

use crate::camera::{Camera, Ray};
use crate::infer::{self, EdgeRef, InferKind, Inference, LineKind, Lock, Query};
use crate::scene::{DisplayScene, Hit, Owner, Piece, RecipeMeshes};
use crate::vcb::{Repeat, Vcb, VcbValue};

/// Keyboard modifiers, as the app maps them. `copy` is SketchUp's copy toggle (Ctrl on
/// Windows/Linux, Option on macOS); `toggle` adds to / removes from the selection (Shift).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub toggle: bool,
    pub copy: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Delete,
    Enter,
    Left,
    Right,
    Up,
    Down,
    /// The copy modifier tapped alone (Option on macOS, Ctrl elsewhere): toggles copying.
    Copy,
    /// Shift tapped alone (with the copy modifier held, a shape tool takes the whole face).
    Shift,
    /// The other modifier tapped alone (Command on macOS, Alt elsewhere): a shape tool measures
    /// its first point from a corner.
    Alt,
    /// Turns the tool's first choice over to its next value (the booleans' scope).
    Tab,
    /// Shift with `+`: one copy more (the copy tools, OCL's SmartHandle), when the tool
    /// [takes counts](Tool::takes_counts).
    More,
    /// Shift with `−`: one copy fewer, never below one.
    Fewer,
}

/// Primary-button and keyboard input for the active tool (navigation is the editor's). Pixel
/// positions are in the viewport, origin top left.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolInput {
    Hover {
        px: DVec2,
    },
    Click {
        px: DVec2,
        mods: Modifiers,
    },
    DoubleClick {
        px: DVec2,
        mods: Modifiers,
    },
    /// The primary button pressed at `px` and dragged away (past the click distance).
    DragStart {
        px: DVec2,
        mods: Modifiers,
    },
    /// The cursor moved while dragging.
    Drag {
        px: DVec2,
    },
    /// The primary button released at `px`, ending a drag.
    DragEnd {
        px: DVec2,
        mods: Modifiers,
    },
    Key {
        key: Key,
        mods: Modifiers,
    },
    /// A value typed in the measurements box and validated with Enter (the editor reads it
    /// for the tool's [`Tool::vcb`] and gives it a [`ToolInput::Value`]).
    Vcb(String),
    /// What was typed, read for the tool's [`Tool::vcb`].
    Value(VcbValue),
}

/// What a tool asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)] // Transient: made and taken at once, an operation most often.
pub enum Command {
    Apply(Operation),
    /// Replaces the selection (entities of the active context).
    Select(Vec<Id>),
    /// Replaces the selection by parts and guides at any depth under the active context
    /// (« Parts », docs/tools/selection.md).
    SelectPaths(Vec<EntityPath>, Vec<GuidePath>),
    /// Enters the assembly the occurrence `Id` of the active context places, or opens its
    /// part.
    Enter(Id),
    /// Edits the profile of the open part's body `Id` (its double-click, docs/model/bodies.md): the Profile
    /// tool on it.
    EditProfile(Id),
    /// Closes the open part, else leaves the active context for its parent.
    Exit,
    /// A click on nothing of the active context, at this pixel: off the grey box of the part
    /// open or the assembly being edited, it leaves it ([`Command::Exit`]); inside, it clears
    /// the selection.
    ClickOff(DVec2),
    /// A message for the status bar (e.g. why a value was refused).
    Say(String),
    /// The height (whole) of the shape just drawn: the next one can take it again (Shift,
    /// double-click).
    RememberHeight(f64),
    /// Sets an option of the active tool, as a click in the bar does (a key that toggles it).
    SetOption {
        key: &'static str,
        value: OptionValue,
    },
    /// Drops the arrow keys' and Shift's locks (a step of the gesture done: the prism tool's
    /// corner placed); the points tracked stay. The tool is then given the cursor again.
    ClearLocks,
    /// Keeps only the latest point tracked, so that neither the older one nor their middle stays
    /// on screen (a rotation's preview starting).
    KeepLastTracked,
    /// The material the paint tool paints with from now on (none: the paint removed).
    PaintWith(Option<Id>),
    /// Holds the tool `ToolKind` over the active one, which waits under it as it is, for as long
    /// as the other modifier is held (the tool stack, docs/tools/interaction.md#tool-stack).
    Hold(ToolKind),
    /// Starts the tool `ToolKind`, as choosing it does.
    Start(ToolKind),
    /// Undoes the last step, which must be `previous`, and applies `op` instead (a copy
    /// turned into an array by a typed `5x`).
    Replace {
        previous: Box<Operation>,
        op: Box<Operation>,
    },
    /// Asks the kernel what `sources` imprint in `receivers` and the copies placed alike (paths
    /// of occurrences from the model): « Empreindre »'s confirmation.
    Imprint {
        sources: Vec<Vec<Id>>,
        receivers: Vec<Vec<Id>>,
    },
}

/// A length a tool writes by what it measures (a side being drawn): the side `a`–`b` (world),
/// written at its middle, or slid along it or off it to keep clear of what matters (the app
/// places it).
///
/// A length that measures one of the values the measurements box takes ([`Tool::vcb`]) says
/// which: while a value is typed, it shows what is typed for it instead (vcb.rs, [`crate::vcb::typed`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreviewLabel {
    pub a: DVec3,
    pub b: DVec3,
    pub text: String,
    /// Its text and frame (linear RGBA): the axis it runs along.
    pub color: [f32; 4],
    /// While its value is typed (the editor sets it), `text` is what is typed, and this what it
    /// reads as when that says more: written after it, greyed, as `= <result>`.
    pub result: Option<String>,
    /// The index of the value of the measurements box it measures, if any.
    pub slot: Option<usize>,
    /// Shown only while its value is typed (too short to write otherwise).
    pub hidden: bool,
    /// Its value is being typed (the editor sets it): framed bolder, with a caret.
    pub active: bool,
}

/// What a tool shows while it works.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    /// The entity of the active context under the cursor.
    pub hover: Option<Id>,
    /// A solid anywhere in the scene under the cursor (tools that reach across contexts).
    pub hover_solid: Option<EntityPath>,
    /// The guide at any depth under the cursor (« Parts », docs/tools/selection.md).
    pub hover_guide: Option<GuidePath>,
    /// The solids picked (the solid tools' targets, A).
    pub picked_solids: Vec<EntityPath>,
    /// The solids picked as tools (the solid tools' B), drawn lighter.
    pub picked_tools: Vec<EntityPath>,
    /// The selection drawn moved by this world transform (the move tool's ghost).
    pub ghost: Option<DAffine3>,
    /// The selection carried, off the view: drawn neither where it is nor moved (a component
    /// placed from the library while the pointer is not over the view,
    /// docs/ui/components-library.md#placing).
    pub away: bool,
    /// The selection drawn copied at each of these world transforms, the original staying (the
    /// copy tools' ghosts, docs/tools/copy-array.md).
    pub copies: Vec<DAffine3>,
    /// The point the tool infers under the cursor (marker and tooltip).
    pub inference: Option<Inference>,
    /// More lines for the inference's tooltip (the tape: the length of the edge hovered); with
    /// no inference, the tooltip by the cursor (the pipette: the material under it).
    pub tip: Option<String>,
    /// The selection rectangle being dragged.
    pub marquee: Option<Marquee>,
    /// The occurrence a change is aimed at, when it reaches every occurrence of its part: the
    /// others are marked too (docs/model/data-model.md).
    pub propagate: Option<EntityPath>,
    /// A section being placed, drawn instead of the document's (docs/render/look.md).
    pub section: Option<Section>,
    /// A part drawn stretched (docs/tools/stretch.md).
    pub stretch: Option<StretchPreview>,
    /// The round or chamfer to preview: the editor has the kernel compute it
    /// (docs/tools/rounds.md).
    pub blend: Option<BlendPreview>,
    /// The selected occurrences drawn as any others: the tool shows what it took its own way
    /// (the stretch tool's grips and cutters, whose colours the selection's would blur).
    pub hide_selection: bool,
    /// What the tool draws, through a [`crate::draw::Drawer`] (docs/render/drawer.md): its lines, fills,
    /// boxes, grips, marks and lengths.
    pub drawing: crate::draw::Drawing,
    /// Entities of the active context drawn as ghosts: what the eraser would take.
    pub erasing: Vec<Id>,
    /// The parts at any depth the eraser would take (« Parts », docs/tools/selection.md).
    pub erasing_paths: Vec<EntityPath>,
    /// The guides at any depth the eraser would take (« Parts »).
    pub erasing_guides: Vec<GuidePath>,
    /// What the paint tool would paint, drawn in the colour it would take (docs/render/look.md).
    pub paint: Option<PaintPreview>,
}

/// What the paint tool would paint, and with what (docs/render/look.md).
#[derive(Clone, Debug, PartialEq)]
pub struct PaintPreview {
    /// The material it paints with (none: the paint removed).
    pub material: Option<Id>,
    pub target: PaintTarget,
}

/// What a click of the paint tool paints.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintTarget {
    /// Occurrences of the active context.
    Occurrences(Vec<Id>),
    /// Parts or assemblies at any depth under it, each in its own assembly (« Parts »).
    Paths(Vec<EntityPath>),
    /// One face of a part, by the path of the occurrence it is seen on: every occurrence of the
    /// part shows it.
    Face { path: EntityPath, face: u32 },
}

/// A point a tool draws over everything (world; its radius in pixels), filled and / or ringed
/// (linear RGBA).
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewPoint {
    pub at: DVec3,
    pub radius: f32,
    pub fill: Option<[f32; 4]>,
    pub stroke: Option<[f32; 4]>,
    pub shape: PointShape,
}

/// How a [`PreviewPoint`] is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointShape {
    Round,
    /// A square on the screen, its radius half its side (a corner kept on its sides' lines).
    Square,
    /// A rectangle along the world direction `along` as the screen shows it, its radius half
    /// its width (a side moved across itself); a square where `along` is seen end on.
    Bar {
        along: DVec3,
    },
    /// A point taken from the geometry.
    Diamond,
    /// A closed padlock: something locked, a click unlocks it (the stretch's locked axes).
    Padlock,
    /// The mirror's badge (OpenCutList's): a square in `fill`, two triangles facing a dashed
    /// axis in `stroke` (the copy tools' mirror, docs/tools/copy-array.md).
    Mirror,
}

/// What a tool previews stretched: the occurrences `occurrences` of the context `context`, and
/// with them every occurrence of what they place when `all` (the scope); the stretch in the
/// axes `frame` places in the context's, as `Operation::Stretch` takes it.
#[derive(Clone, Debug, PartialEq)]
pub struct StretchPreview {
    pub context: Vec<Id>,
    pub occurrences: Vec<Id>,
    /// Bodies of the part the one occurrence places, stretched alone (`frame` then in the
    /// part's axes, docs/model/bodies.md).
    pub bodies: Option<Vec<Id>>,
    pub frame: DAffine3,
    pub all: bool,
    pub axis: DVec3,
    pub cuts: Vec<f64>,
    pub moves: Vec<f64>,
}

// ---- options (docs/ui/interface.md, the tool's option bar) ---------------------------------

/// The value of a tool option.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionValue {
    /// One of a choice's values.
    Choice(String),
    Toggle(bool),
    /// A length (mm), shown in the document's units and typed in place (or `<length>x` in the
    /// measurements box, docs/ui/interface.md).
    Length(f64),
}

/// One of a choice's values, as the bar shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct OptionChoice {
    pub value: &'static str,
    pub label: String,
    pub tip: String,
    /// The name of its icon, which the app draws instead of the label (then in the tip).
    pub icon: Option<&'static str>,
}

/// The steps a tool goes through, as the steps bar at the bottom of the view shows them
/// (docs/ui/interface.md#steps-bar, OpenCutList's `SmartTool#set_steps_bar`): `count` numbers,
/// the one at `index` (from 0) the current, then what to do at it; no number when `count` is 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Steps {
    pub count: usize,
    pub index: usize,
    /// The step's main sentence, without the keys (the status bar keeps them).
    pub text: String,
}

/// An option of the active tool, as the bar shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolOption {
    pub key: &'static str,
    /// The option's name (the boolean's scope), shown before a choice, on a toggle.
    pub label: String,
    pub tip: String,
    pub value: OptionValue,
    /// A choice's values; empty for a toggle.
    pub choices: Vec<OptionChoice>,
    /// A setting, kept per tool in the user's preferences (read back in [`Cx::options`]); else
    /// the gesture's own state (copying), which the tool keeps.
    pub saved: bool,
    /// A toggle's icon, by name, which the app draws instead of the label (then in the tip).
    pub icon: Option<&'static str>,
    /// A colour (sRGB) filling the toggle's button while it is on: « Pipette »'s material
    /// (docs/tools/draw-options.md).
    pub swatch: Option<[u8; 3]>,
}

/// The creation tools' toggle « Nommage des pièces » (OCL's `ask_name`): the part's name asked
/// as soon as it is made (docs/tools/quick-tools.md).
pub const ASK_NAME: &str = "ask_name";

/// The part creation tools' toggles « Dessiner dans » and « Pipette » (OCL's `draw_in` and
/// `pick_material`, docs/tools/draw-options.md): the part made goes into the assembly of
/// the part picked before the first click, takes its material.
pub const DRAW_IN: &str = "draw_in";
pub const PICK_MATERIAL: &str = "pick_material";

/// The selection's scope (docs/tools/selection.md), shared by Select and every tool's
/// picking: what a click takes, the active context's child or the part at any depth. Saved
/// once for all the tools, under [`SELECTION_SETTINGS`].
pub const PICK_SCOPE: &str = "pick_scope";
/// The values of [`PICK_SCOPE`].
pub const PICK_CONTEXT: &str = "context";
pub const PICK_PARTS: &str = "parts";
/// Where the settings shared by the tools are kept, as a tool's are under its id.
pub const SELECTION_SETTINGS: &str = "selection";

/// One tool's saved settings, by option key.
pub type OptionValues = BTreeMap<String, OptionValue>;

/// A selection rectangle (pixels, origin top left), as the select tool drags it.
#[derive(Clone, Debug, PartialEq)]
pub struct Marquee {
    pub from: DVec2,
    pub to: DVec2,
    /// Drawn right to left (SketchUp): what it touches is taken, not only what it encloses.
    pub crossing: bool,
    /// The entities of the active context it would take now.
    pub hits: Vec<Id>,
    /// The parts at any depth it would take now (« Parts », docs/tools/selection.md).
    pub paths: Vec<EntityPath>,
    /// The guides at any depth it would take now (« Parts »).
    pub guides: Vec<GuidePath>,
}

impl Marquee {
    pub fn new(from: DVec2, to: DVec2) -> Self {
        Marquee { from, to, crossing: to.x < from.x, hits: Vec::new(), paths: Vec::new(), guides: Vec::new() }
    }
}

/// Which tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Select,
    Box,
    Cylinder,
    /// Draws an outline, then pulls it into a prism (docs/tools/prism.md).
    Prism,
    /// Sets an axis, draws a half profile, then turns it about the axis (docs/tools/turned-bodies.md).
    Turn,
    Move,
    Rotate,
    Boolean(Boolean),
    /// Erases (or hides) what it is clicked or dragged over.
    Erase,
    /// Paints occurrences or faces (docs/render/look.md).
    Paint,
    /// Takes the material under the cursor for the paint tool (« Prélever », docs/render/look.md).
    Sample,
    /// Places the section plane (docs/render/look.md).
    Section,
    /// Measures, and draws guides (docs/tools/tape-measure.md).
    Measure,
    /// Stretches a part along one of its axes (docs/tools/stretch.md).
    Stretch,
    /// Copies along a line (docs/tools/copy-array.md).
    CopyLine,
    /// Copies along a grid (docs/tools/copy-array.md).
    CopyGrid,
    /// Spreads copies between two points (docs/tools/copy-array.md).
    Distribute,
    /// Edits the profile of a body of the open part (docs/tools/prism.md, docs/model/bodies.md).
    Profile,
    /// Rounds edges (docs/tools/rounds.md).
    Round,
    /// Chamfers edges (docs/tools/rounds.md).
    Chamfer,
    /// Gives a part or an assembly new axes (docs/tools/reorient.md).
    Reorient,
    /// Imprints a part in the parts it goes into (docs/decisions/0055-linked-bodies-through-imprints.md).
    Imprint,
}

impl ToolKind {
    /// Every tool.
    pub const ALL: [ToolKind; 24] = [
        ToolKind::Select,
        ToolKind::Box,
        ToolKind::Cylinder,
        ToolKind::Prism,
        ToolKind::Turn,
        ToolKind::Move,
        ToolKind::Rotate,
        ToolKind::Boolean(Boolean::Union),
        ToolKind::Boolean(Boolean::Difference),
        ToolKind::Boolean(Boolean::Intersection),
        ToolKind::Erase,
        ToolKind::Paint,
        ToolKind::Sample,
        ToolKind::Section,
        ToolKind::Measure,
        ToolKind::Stretch,
        ToolKind::CopyLine,
        ToolKind::CopyGrid,
        ToolKind::Distribute,
        ToolKind::Profile,
        ToolKind::Round,
        ToolKind::Chamfer,
        ToolKind::Reorient,
        ToolKind::Imprint,
    ];

    /// The tool named `id` ([`ToolKind::id`]).
    pub fn from_id(id: &str) -> Option<ToolKind> {
        ToolKind::ALL.into_iter().find(|k| k.id() == id)
    }

    /// A stable name, for the settings kept per tool.
    pub fn id(self) -> &'static str {
        match self {
            ToolKind::Select => "select",
            ToolKind::Box => "box",
            ToolKind::Cylinder => "cylinder",
            ToolKind::Prism => "prism",
            ToolKind::Turn => "turn",
            ToolKind::Move => "move",
            ToolKind::Rotate => "rotate",
            ToolKind::Boolean(Boolean::Union) => "union",
            ToolKind::Boolean(Boolean::Difference) => "difference",
            ToolKind::Boolean(Boolean::Intersection) => "intersection",
            ToolKind::Erase => "erase",
            ToolKind::Paint => "paint",
            ToolKind::Sample => "sample",
            ToolKind::Section => "section",
            ToolKind::Measure => "measure",
            ToolKind::Stretch => "stretch",
            ToolKind::CopyLine => "copy_line",
            ToolKind::CopyGrid => "copy_grid",
            ToolKind::Distribute => "distribute",
            ToolKind::Profile => "profile",
            ToolKind::Round => "round",
            ToolKind::Chamfer => "chamfer",
            ToolKind::Reorient => "reorient",
            ToolKind::Imprint => "imprint",
        }
    }

    /// Its name in the option bar and the menus.
    pub fn label(self) -> String {
        match self {
            ToolKind::Select => t!("tool.select.name"),
            ToolKind::Box => t!("tool.box.name"),
            ToolKind::Cylinder => t!("tool.cylinder.name"),
            ToolKind::Prism => t!("tool.prism.name"),
            ToolKind::Turn => t!("tool.turn.name"),
            ToolKind::Move => t!("tool.move.name"),
            ToolKind::Rotate => t!("tool.rotate.name"),
            ToolKind::Boolean(Boolean::Union) => t!("tool.boolean.union.name"),
            ToolKind::Boolean(Boolean::Difference) => t!("tool.boolean.difference.name"),
            ToolKind::Boolean(Boolean::Intersection) => t!("tool.boolean.intersection.name"),
            ToolKind::Erase => t!("tool.erase.name"),
            ToolKind::Paint => t!("tool.paint.name"),
            ToolKind::Sample => t!("tool.sample.name"),
            ToolKind::Section => t!("tool.section.name"),
            ToolKind::Measure => t!("tool.measure.name"),
            ToolKind::Stretch => t!("tool.stretch.name"),
            ToolKind::CopyLine => t!("tool.copy_line.name"),
            ToolKind::CopyGrid => t!("tool.copy_grid.name"),
            ToolKind::Distribute => t!("tool.distribute.name"),
            ToolKind::Profile => t!("tool.profile.name"),
            ToolKind::Round => t!("tool.round.name"),
            ToolKind::Chamfer => t!("tool.chamfer.name"),
            ToolKind::Reorient => t!("tool.reorient.name"),
            ToolKind::Imprint => t!("tool.imprint.name"),
        }
    }
}

/// What a tool sees of the editor.
#[derive(Clone)]
pub struct Cx<'a> {
    /// The time now (seconds, the app's clock).
    pub now: f64,
    pub doc: &'a Document,
    pub scene: &'a DisplayScene,
    pub camera: &'a Camera,
    pub size: DVec2,
    /// Occurrence path of the context being edited (empty: the model).
    pub context: &'a [Id],
    /// The definition being edited.
    pub context_def: Id,
    /// Definition to world, for the context: operations are expressed in it.
    pub context_transform: DAffine3,
    /// The part open in the context, by its occurrence there (docs/model/opening-a-part.md).
    pub open_part: Option<Id>,
    /// The group of bodies open in the open part (docs/model/bodies.md#body-groups): the bodies leading
    /// to it; empty, the part itself.
    pub open_group: &'a [Id],
    /// The open part's axes in the context's (identity without one): its guides' frame.
    pub part_in_context: DAffine3,
    /// The axes of what is open in the context's — the open part's, or its group's (identity
    /// without one): tools draw and infer in these axes (docs/model/data-model.md), see [`Cx::axes`].
    pub axes_in_context: DAffine3,
    /// The open part's recipe, while its machinings are shown: the selection is machinings
    /// then, and the tools act on them (docs/model/opening-a-part.md).
    pub recipe: Option<&'a RecipeMeshes>,
    /// What a change to the open part reaches (docs/model/data-model.md).
    pub part_scope: Scope,
    /// Whether a shape drawn in the open part adds material or takes it away (docs/model/bodies.md).
    pub body_sign: Sign,
    pub selection: &'a [Id],
    /// The parts selected at any depth (« Parts », docs/tools/selection.md), by their paths from the root.
    pub deep: &'a [EntityPath],
    /// The guides selected at any depth with them (« Parts »), by their paths from the root.
    pub deep_guides: &'a [GuidePath],
    /// Does a click take parts at any depth (« Parts », no part open)?
    pub parts: bool,
    /// The context axis the arrow keys lock (→ 0 red, ← 1 green, ↑ 2 blue; ↓ frees it).
    pub axis_lock: Option<usize>,
    /// The normal ↓ locks for a tool that takes it ([`Tool::down_locks_face`]): the face's
    /// under the cursor when the key was pressed (world, unit, towards the eye).
    pub face_lock: Option<DVec3>,
    /// Shift held, and the copy modifier (Option on macOS, Ctrl elsewhere).
    pub shift_down: bool,
    pub copy_down: bool,
    /// The other modifier held (Command on macOS, Alt elsewhere).
    pub alt_down: bool,
    /// The active tool is held over another for as long as the other modifier is (the tool
    /// stack, docs/tools/interaction.md#tool-stack): it hands back when the key is released.
    pub held: bool,
    /// The height (whole) of the last shape drawn, if any.
    pub last_height: Option<f64>,
    /// The line Shift holds.
    pub shift_lock: Option<Lock>,
    /// Points tracked (hovered a while), world.
    pub tracked: &'a [DVec3],
    /// The last edge hovered, straight or arc (world).
    pub reference: Option<EdgeRef>,
    /// The active tool's saved settings.
    pub options: &'a OptionValues,
    /// The material the paint tool paints with (none: it removes paint).
    pub material: Option<Id>,
    /// The occurrences the tool's preview draws otherwise — stretched or moved (docs/tools/stretch.md) —
    /// by path: not where they are drawn, so not to infer on.
    pub previewed: &'a std::collections::HashMap<EntityPath, (Id, DAffine3)>,
    /// A line the tool draws itself and infers on ([`Tool::infer_line`]), in the axes.
    pub tool_line: Option<(DVec3, DVec3)>,
}

impl Cx<'_> {
    pub fn ray(&self, px: DVec2) -> Ray {
        self.camera.ray(px, self.size)
    }

    /// The frame tools draw and infer in, to world: the open part's axes, else the context's.
    pub fn axes(&self) -> DAffine3 {
        self.context_transform * self.axes_in_context
    }

    /// The ray in the axes' coordinates.
    pub fn axes_ray(&self, px: DVec2) -> Ray {
        let r = self.ray(px);
        let inv = self.axes().inverse();
        Ray { origin: inv.transform_point3(r.origin), dir: inv.transform_vector3(r.dir).normalize(), t_min: r.t_min }
    }

    /// The open part's occurrence path.
    pub fn open_part_path(&self) -> Option<EntityPath> {
        self.open_part.map(|id| EntityPath::new(self.context.to_vec(), id).in_group(self.open_group.to_vec()))
    }

    /// Are the open part's blanks and machinings shown (the selection being its bodies)?
    pub fn machinings_shown(&self) -> bool {
        self.recipe.is_some() && self.open_part.is_some()
    }

    /// The body of the open part under `px` (a machining before a blank), while they are shown.
    pub fn pick_body(&self, px: DVec2) -> Option<Id> {
        if !self.machinings_shown() {
            return None;
        }
        self.scene.pick_body(&self.ray(px), self.axes()).map(|(id, _)| id)
    }

    /// The body of the open part under `px` that has a profile to edit, while they are shown.
    pub fn profiled_body_at(&self, px: DVec2) -> Option<Id> {
        let id = self.pick_body(px)?;
        let (_, part, _) = self.doc.resolve_container(&self.open_part_path()?).ok()?;
        part.body(id).filter(|b| part.profile_of(b).is_some()).map(|b| b.id)
    }

    /// The group of bodies of the open part (or group) under `px`, while they are shown: what a
    /// double-click opens (docs/model/bodies.md#body-groups).
    pub fn group_body_at(&self, px: DVec2) -> Option<Id> {
        let id = self.pick_body(px)?;
        let (_, part, _) = self.doc.resolve_container(&self.open_part_path()?).ok()?;
        part.body(id).filter(|b| part.def_of(b).is_group()).map(|b| b.id)
    }

    /// The occurrences from the root to the component guides are drawn in now: the open
    /// part's, else the context's (docs/tools/tape-measure.md).
    pub fn guide_path(&self) -> Vec<Id> {
        self.context.iter().copied().chain(self.open_part).collect()
    }

    /// The guide of the active context under `px` (within [`GUIDE_PICK_PX`], not hidden by the
    /// model), the nearest on screen.
    pub fn pick_guide(&self, px: DVec2) -> Option<Id> {
        let path = self.guide_path();
        self.nearest_guide(px, |g| g.path == path).map(|g| g.id)
    }

    /// The guide at any depth under the active context near `px` (« Parts »,
    /// docs/tools/selection.md), by its path from the root; none in an open part.
    pub fn guide_at(&self, px: DVec2) -> Option<GuidePath> {
        if self.open_part.is_some() {
            return None;
        }
        self.nearest_guide(px, |g| g.path.starts_with(self.context)).map(|g| GuidePath::new(g.path.clone(), g.id))
    }

    /// The guide `keep` keeps near `px` (within [`GUIDE_PICK_PX`], not hidden by the model),
    /// the nearest on screen.
    fn nearest_guide(&self, px: DVec2, keep: impl Fn(&crate::scene::SceneGuide) -> bool) -> Option<&crate::scene::SceneGuide> {
        let ray = self.ray(px);
        let mut best: Option<(f64, &crate::scene::SceneGuide)> = None;
        for g in self.scene.guides().iter().filter(|g| keep(g)) {
            let Some((dist, p)) = crate::guides::distance(self.camera, self.size, &g.shape, px) else { continue };
            if dist > GUIDE_PICK_PX || best.is_some_and(|b| dist >= b.0) {
                continue;
            }
            // Behind the model where the cursor is: hidden (a few pixels of slack).
            let depth = (p - ray.origin).dot(ray.dir);
            let slack = self.camera.world_per_pixel(self.size, p) * 3.0;
            if self.scene.pick(&ray, |_| true).is_some_and(|h| h.t < depth - slack) {
                continue;
            }
            best = Some((dist, g));
        }
        best.map(|b| b.1)
    }

    /// The entity of the active context a click of Select takes at `px`: a guide near it,
    /// else the blank or machining under it while they are shown, else the context's occurrence that
    /// leads to the solid under it.
    pub fn entity_at(&self, px: DVec2) -> Option<Id> {
        self.pick_guide(px)
            .or_else(|| if self.machinings_shown() { self.pick_body(px) } else { self.pick(px).and_then(|h| self.selectable(&h.path)) })
    }

    /// The part under `px` at any depth under the active context (« Parts », docs/tools/selection.md), by
    /// its path from the root; none in an open part.
    pub fn part_at(&self, px: DVec2) -> Option<EntityPath> {
        if self.open_part.is_some() {
            return None;
        }
        self.pick(px).map(|h| h.path).filter(|p| p.instances.starts_with(self.context))
    }

    /// Is `id` an occurrence of the context (not a guide, nor a machining)?
    pub fn is_occurrence(&self, id: Id) -> bool {
        self.open_part.is_none() && self.doc.assembly(self.context_def).is_ok_and(|a| a.occurrence(id).is_some())
    }

    /// The operation erasing entities of the active context: in an open part, its machinings
    /// and guides; else the context's occurrences and guides.
    pub fn erase(&self, entities: Vec<Id>) -> Operation {
        match self.open_part_path() {
            Some(target) => Operation::DeleteBodies { target, bodies: entities, scope: self.part_scope },
            None => Operation::Delete { context: self.context_def, entities },
        }
    }

    /// What moving the selection changes, and in which frame: in an open part, its
    /// machinings and guides; else the context's occurrences and guides.
    pub fn moved(&self) -> Moved {
        match self.open_part_path() {
            Some(target) => Moved::Machinings { target, scope: self.part_scope, frame: self.axes() },
            None => Moved::Occurrences { context: self.context_def, frame: self.context_transform },
        }
    }

    /// What moving the parts `paths` and the guides `guides` at any depth changes (« Parts »,
    /// docs/tools/selection.md): grouped per parent assembly — a guide of an assembly with its
    /// occurrences, a part's guides per part —, each in its axes; under « only this one », the
    /// assemblies along made unique when they are placed more than once (not for a part's
    /// guides, which ignore the scope: docs/tools/tape-measure.md). A guide reached twice (two
    /// occurrences of its component) moves once, unless made unique.
    pub fn moved_deep(&self, paths: &[EntityPath], guides: &[GuidePath]) -> Moved {
        let unique = self.part_scope == Scope::This
            && (paths.iter().any(|p| self.doc.shared_along(&p.instances))
                || guides.iter().any(|g| !self.is_part_guide(g) && self.doc.shared_along(&g.instances)));
        let mut groups: Vec<DeepGroup> = Vec::new();
        let mut add = |parent: &[Id], id: Id, part: Option<EntityPath>, frame: &dyn Fn() -> DAffine3| match groups
            .iter_mut()
            .find(|g| g.parent == parent)
        {
            Some(g) if !g.entities.contains(&id) => g.entities.push(id),
            Some(_) => {}
            None => groups.push(DeepGroup { parent: parent.to_vec(), frame: frame(), entities: vec![id], part }),
        };
        for p in paths {
            add(&p.instances, p.entity, None, &|| self.doc.transform_of(&p.instances).unwrap_or(DAffine3::IDENTITY));
        }
        let mut seen: Vec<(Id, Id)> = Vec::new();
        for g in guides {
            let Ok((component, frame)) = self.doc.component_at(&g.instances) else { continue };
            let part = self.is_part_guide(g);
            // The same guide through another occurrence of its component: once.
            if (part || !unique) && seen.contains(&(component, g.guide)) {
                continue;
            }
            seen.push((component, g.guide));
            let target = g.instances.split_last().filter(|_| part).map(|(&last, parents)| EntityPath::new(parents.to_vec(), last));
            add(&g.instances, g.guide, target, &|| frame);
        }
        Moved::Deep { groups, unique }
    }

    /// Does the guide `g` belong to a part (else to an assembly)?
    fn is_part_guide(&self, g: &GuidePath) -> bool {
        self.doc.component_at(&g.instances).is_ok_and(|(c, _)| self.doc.part(c).is_ok())
    }

    /// The operation erasing the parts `paths` and the guides `guides` at any depth (« Parts »,
    /// docs/tools/selection.md): one at each parent assembly (and part), in one step.
    pub fn erase_deep(&self, paths: &[EntityPath], guides: &[GuidePath]) -> Operation {
        let Moved::Deep { groups, unique } = self.moved_deep(paths, guides) else { unreachable!() };
        Moved::at_each(&groups, unique, |_, entities| Operation::Delete { context: Id(0), entities })
    }

    /// `op` of the occurrences selected in each parent assembly of the parts `paths`, applied
    /// at it (« Parts », docs/tools/selection.md), in one step.
    pub fn deep_op(&self, paths: &[EntityPath], op: impl Fn(Vec<Id>) -> Operation) -> Operation {
        let Moved::Deep { groups, unique } = self.moved_deep(paths, &[]) else { unreachable!() };
        Moved::at_each(&groups, unique, |_, entities| op(entities))
    }

    /// Is `o` under one of the parts `paths` at any depth (« Parts »)? What a move or a
    /// rotation of them leaves out of inferences.
    pub fn is_moving_deep(&self, o: &Owner<'_>, paths: &[EntityPath]) -> bool {
        let chain = |p: &EntityPath| p.instances.iter().copied().chain([p.entity]).collect::<Vec<Id>>();
        let under = chain(o.path);
        paths.iter().any(|p| under.starts_with(&chain(p)))
    }

    /// Is `o` one of the guides `guides` at any depth (« Parts »)?
    pub fn is_moving_guide(&self, o: &Owner<'_>, guides: &[GuidePath]) -> bool {
        matches!(o.piece, Piece::Guide(id) if guides.iter().any(|g| g.guide == id && g.instances == o.path.instances))
    }

    /// The operation making `primitive`, drawn in the axes and placed by `placement` there:
    /// in an open part, a body of it — a machining, or material added, as the bar says (docs/model/opening-a-part.md,
    /// docs/model/bodies.md); else a new part of the context.
    pub fn add_primitive(&self, primitive: Primitive, placement: DAffine3) -> Operation {
        match self.open_part_path() {
            Some(target) => Operation::AddBody { target, primitive, placement, sign: self.body_sign, scope: self.part_scope },
            None => Operation::DrawPrimitive {
                context: self.context_def,
                primitive,
                placement: self.axes_in_context * placement,
                material: None,
            },
        }
    }

    /// The part a creation tool's first click picks (docs/tools/draw-options.md): the one under `px`, at
    /// any depth under the context, while « Dessiner dans » or « Pipette » is on.
    pub fn creation_pick(&self, px: DVec2) -> Option<EntityPath> {
        (self.toggle(DRAW_IN) || self.toggle(PICK_MATERIAL)).then(|| self.part_at(px).map(|p| p.part_path())).flatten()
    }

    /// The assembly « Dessiner dans » makes a part in, for the part `picked`: the one holding
    /// it, by its occurrences from the root, unless it is the context (or the option is off).
    pub fn draw_in<'p>(&self, picked: Option<&'p EntityPath>) -> Option<&'p [Id]> {
        let parent = picked.filter(|_| self.open_part.is_none() && self.toggle(DRAW_IN))?.instances.as_slice();
        (parent != self.context && self.doc.context_of(parent).is_ok()).then_some(parent)
    }

    /// The material « Pipette » gives a part made, from the part `picked`: its occurrence's own,
    /// else the one it inherits from its assemblies (as the view paints it).
    pub fn picked_material(&self, picked: Option<&EntityPath>) -> Option<Id> {
        let p = picked.filter(|_| self.open_part.is_none() && self.toggle(PICK_MATERIAL))?;
        self.occurrence_material(p)
    }

    /// The material the occurrence `path` (from the root) paints its parts with: its own, else
    /// the one it inherits from its assemblies.
    pub fn occurrence_material(&self, path: &EntityPath) -> Option<Id> {
        let mut at = self.doc.root();
        let mut material = None;
        for &i in path.instances.iter().chain([&path.entity]) {
            let o = self.doc.assembly(at).ok()?.occurrence(i)?;
            material = o.material.or(material);
            at = o.component;
        }
        material.filter(|m| self.doc.material(*m).is_ok())
    }

    /// The material shown on `face` of the part `path` leads to (the paint tool's pipette):
    /// the face's own, else its occurrence's (docs/render/look.md).
    pub fn material_shown(&self, path: &EntityPath, face: u32) -> Option<Id> {
        let own = self.doc.resolve_part(path).ok().and_then(|(_, part, _)| part.faces.get(&face).copied());
        own.filter(|m| self.doc.material(*m).is_ok()).or_else(|| self.occurrence_material(&path.part_path()))
    }

    /// The operation a part creation tool applies (as [`Cx::add_primitive`]), « Dessiner dans »
    /// and « Pipette » taken from the part `picked` before the first click (docs/tools/draw-options.md):
    /// in another assembly than the context, placed in its axes, its copies made unique first
    /// under « only this one » when it is placed more than once (as parts moved at any depth).
    pub fn add_part(&self, primitive: Primitive, placement: DAffine3, picked: Option<&EntityPath>) -> Operation {
        if self.open_part.is_some() {
            return self.add_primitive(primitive, placement);
        }
        let material = self.picked_material(picked);
        match self.draw_in(picked) {
            Some(parent) => {
                let frame = self.doc.transform_of(parent).unwrap_or(DAffine3::IDENTITY);
                let placement = frame.inverse() * self.context_transform * self.axes_in_context * placement;
                let unique = self.part_scope == Scope::This && self.doc.shared_along(parent);
                let op = Operation::DrawPrimitive { context: self.context_def, primitive, placement, material };
                Operation::At { parent: parent.to_vec(), unique, op: Box::new(op) }
            }
            None => {
                Operation::DrawPrimitive { context: self.context_def, primitive, placement: self.axes_in_context * placement, material }
            }
        }
    }

    /// The component a guide drawn now belongs to (docs/tools/tape-measure.md) — the open part, else the
    /// context — and its axes to world.
    pub fn guide_owner(&self) -> (Id, DAffine3) {
        let part = self.open_part_path().and_then(|p| self.doc.resolve(&p).ok()).map(|o| o.component);
        // Its guides stay the part's, in its axes, a group open or not.
        match part {
            Some(part) => (part, self.context_transform * self.part_in_context),
            None => (self.context_def, self.axes()),
        }
    }

    /// Is `o` one of `moving` (occurrences of the context, or machinings of the open part, and
    /// their guides)?
    /// What a move or a rotation leaves out of inferences.
    pub fn is_moving(&self, o: &Owner<'_>, moving: &[Id]) -> bool {
        match o.piece {
            Piece::Tool(id) | Piece::Blank(id) => moving.contains(&id),
            Piece::Shape => self.selectable(o.path).is_some_and(|id| moving.contains(&id)),
            Piece::Guide(id) => moving.contains(&id) && o.path.instances == self.guide_path(),
        }
    }

    /// The nearest solid under `px`, anywhere.
    pub fn pick(&self, px: DVec2) -> Option<Hit> {
        self.scene.pick(&self.ray(px), |_| true)
    }

    /// The entity of the active context a hit belongs to: the solid itself if it is in the
    /// context, else the context's occurrence that leads to it; `None` outside the context,
    /// and while a part is open (its machinings are picked otherwise).
    pub fn selectable(&self, path: &EntityPath) -> Option<Id> {
        if self.open_part.is_some() {
            return None;
        }
        let rest = path.instances.strip_prefix(self.context)?;
        Some(rest.first().copied().unwrap_or(path.entity))
    }

    /// Is `path` under one of the selected entities?
    pub fn in_selection(&self, path: &EntityPath) -> bool {
        let chain = |p: &EntityPath| p.instances.iter().copied().chain([p.entity]).collect::<Vec<Id>>();
        self.selectable(path).is_some_and(|id| self.selection.contains(&id))
            || (!self.deep.is_empty() && {
                let under = chain(path);
                self.deep.iter().any(|p| under.starts_with(&chain(p)))
            })
    }

    /// A drawer into `out` for this view (docs/render/drawer.md), in the world's axes.
    pub fn drawer<'d>(&'d self, out: &'d mut crate::draw::Drawing) -> crate::draw::Drawer<'d> {
        crate::draw::Drawer::new(self.camera, self.size, self.units(), out)
    }

    /// The same, in the axes' coordinates (those of the context, [`Self::to_world`]).
    pub fn axes_drawer<'d>(&'d self, out: &'d mut crate::draw::Drawing) -> crate::draw::Drawer<'d> {
        self.drawer(out).with_frame(self.axes())
    }

    /// The document's units (lengths typed in the measurements box).
    pub fn units(&self) -> &Units {
        &self.doc.settings().units
    }

    /// A point of the axes' coordinates, in the world.
    pub fn to_world(&self, p: DVec3) -> DVec3 {
        self.axes().transform_point3(p)
    }

    /// A world point, in the axes' coordinates.
    pub fn to_axes(&self, p: DVec3) -> DVec3 {
        self.axes().inverse().transform_point3(p)
    }

    /// The axis `k` (0 red X, 1 green Y, 2 blue Z) of the axes, in the world.
    pub fn axis(&self, k: usize) -> DVec3 {
        self.axes().matrix3.col(k).normalize()
    }

    /// The ink of the axis (of the axes) the world direction `d` runs along ([`ink_along`]).
    pub(crate) fn ink_along(&self, d: DVec3, tolerance: f64) -> Option<[f32; 4]> {
        ink_along([self.axis(0), self.axis(1), self.axis(2)], d, tolerance)
    }

    /// The ground of the axes: their origin and Z axis, in the world.
    pub fn ground(&self) -> (DVec3, DVec3) {
        (self.to_world(DVec3::ZERO), self.axis(2))
    }

    /// The point inferred under `px` (docs/tools/inferences.md), on the solids `keep` accepts; `plane` (world
    /// point and normal) when nothing is under the cursor, by default the context's ground.
    pub fn infer(&self, px: DVec2, keep: impl Fn(&Owner<'_>) -> bool, plane: Option<(DVec3, DVec3)>) -> Option<Inference> {
        self.query(px, &keep, plane, None, false, None, &[])
    }

    /// The same on everything, the `corners` of the box of what the tool takes among the
    /// points (docs/tools/inferences.md#selection-box).
    pub fn infer_on_box(&self, px: DVec2, corners: &[DVec3]) -> Option<Inference> {
        self.query(px, &|_| true, None, None, false, None, corners)
    }

    /// The same, with lines inferred through the start point `from` (world), kept on `lock`.
    pub fn infer_from(
        &self,
        px: DVec2,
        keep: impl Fn(&Owner<'_>) -> bool,
        plane: Option<(DVec3, DVec3)>,
        from: DVec3,
        lock: Option<Lock>,
    ) -> Option<Inference> {
        self.query(px, &keep, plane, Some(from), false, lock, &[])
    }

    /// Lines through `from` that lie in `plane` only (a protractor's), never locked.
    pub fn infer_in_plane(&self, px: DVec2, keep: impl Fn(&Owner<'_>) -> bool, plane: (DVec3, DVec3), from: DVec3) -> Option<Inference> {
        self.query(px, &keep, Some(plane), Some(from), true, None, &[])
    }

    #[allow(clippy::too_many_arguments)]
    fn query(
        &self,
        px: DVec2,
        keep: &dyn Fn(&Owner<'_>) -> bool,
        plane: Option<(DVec3, DVec3)>,
        from: Option<DVec3>,
        in_plane: bool,
        lock: Option<Lock>,
        box_corners: &[DVec3],
    ) -> Option<Inference> {
        let q = Query {
            keep,
            plane: plane.unwrap_or_else(|| self.ground()),
            origin: Some(self.to_world(DVec3::ZERO)),
            // As the app draws them: shown, or an open part's.
            model_axes: self.doc.settings().display.axes || self.open_part.is_some(),
            from,
            tracked: self.tracked,
            in_plane,
            axes: [self.axis(0), self.axis(1), self.axis(2)],
            reference: self.reference,
            lock,
            tool_line: self.tool_line.map(|(o, d)| (self.to_world(o), self.axes().matrix3.mul_vec3(d).normalize())),
            box_corners,
        };
        infer::infer(self.scene, self.camera, self.size, px, &q)
    }

    /// The saved value of the toggle `key` (off when never set).
    pub fn toggle(&self, key: &str) -> bool {
        matches!(self.options.get(key), Some(OptionValue::Toggle(true)))
    }

    /// The saved value of the choice `key`, if any.
    pub fn choice(&self, key: &str) -> Option<&str> {
        match self.options.get(key) {
            Some(OptionValue::Choice(v)) => Some(v),
            _ => None,
        }
    }

    /// The saved value of the length `key` (mm; zero when never set).
    pub fn length(&self, key: &str) -> f64 {
        match self.options.get(key) {
            Some(OptionValue::Length(v)) => *v,
            _ => 0.0,
        }
    }

    /// How many occurrences of the part `path` names the scene shows (through copies of
    /// assemblies too).
    pub fn copies_shown(&self, path: &EntityPath) -> usize {
        let Ok((part, _, _)) = self.doc.resolve_part(path) else { return 0 };
        self.scene.placements().find(|(p, _)| *p == part).map_or(0, |(_, places)| places.len())
    }

    /// The line a lock holds, if any: Shift's, else the arrow keys' axis.
    pub fn lock(&self) -> Option<Lock> {
        self.shift_lock.or_else(|| self.axis_lock.map(|k| Lock { origin: None, dir: self.axis(k), line: LineKind::Axis(k) }))
    }
}

/// The colour of a guide for `kind`: an axis's own, magenta for an edge's parallel or
/// perpendicular, else ink (a guide of the document too).
pub fn guide_color(kind: InferKind) -> [f32; 4] {
    if kind == InferKind::OnGuide || kind.line() == Some(LineKind::Guide) {
        return INK;
    }
    match kind.line().map(|l| l.axis()) {
        Some(Some(0)) => RED,
        Some(Some(1)) => GREEN,
        Some(Some(_)) => BLUE,
        Some(None) => MAGENTA,
        None => INK,
    }
}

/// What a drag of the primary button is to a tool (docs/tools/interaction.md, OpenCutList's SmartDraw).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragClicks {
    /// A drag: [`ToolInput::DragStart`], [`ToolInput::Drag`], [`ToolInput::DragEnd`].
    No,
    /// Hovers, then a click where the button is released.
    Release,
    /// A click where the button went down, hovers, then a click where it is released (unless
    /// released near the press).
    PressAndRelease,
}

/// A mouse cursor of the app, as OpenCutList's (`SmartCursorManager`): the tool's at its step,
/// the app drawing it (docs/ui/cursors.md). None: the system's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cursor {
    /// The arrow (Select, a step that picks with no cursor of its own).
    Select,
    /// The pick step of a tool acting on parts (OCL's `STATE_SELECT`).
    SelectPart,
    /// Shift held while picking: adding or removing (`STATE_SELECT_MULTIPLE`).
    SelectPlusMinus,
    /// A selection rectangle dragged (`STATE_SELECT_RECT`).
    SelectRect,
    SelectCopyLine,
    SelectCopyGrid,
    SelectDistribute,
    SelectStretch,
    /// The booleans: the target (a), then the tool part (b).
    SelectUniteA,
    SelectUniteB,
    SelectSubtractA,
    SelectSubtractB,
    SelectIntersectA,
    SelectIntersectB,
    /// Moving (Move), copying (Move with its copy toggle).
    Move,
    MoveCopy,
    /// The shape tools' first steps, then their height (OCL's `STATE_PULL`).
    PencilRectangle,
    PencilCircle,
    PencilPolygon,
    Pull,
    /// Paint: occurrences, faces; its pipette (docs/render/look.md).
    PaintPart,
    PaintFace,
    Picker,
}

impl Cursor {
    /// Every cursor.
    pub const ALL: [Cursor; 23] = [
        Cursor::Select,
        Cursor::SelectPart,
        Cursor::SelectPlusMinus,
        Cursor::SelectRect,
        Cursor::SelectCopyLine,
        Cursor::SelectCopyGrid,
        Cursor::SelectDistribute,
        Cursor::SelectStretch,
        Cursor::SelectUniteA,
        Cursor::SelectUniteB,
        Cursor::SelectSubtractA,
        Cursor::SelectSubtractB,
        Cursor::SelectIntersectA,
        Cursor::SelectIntersectB,
        Cursor::Move,
        Cursor::MoveCopy,
        Cursor::PencilRectangle,
        Cursor::PencilCircle,
        Cursor::PencilPolygon,
        Cursor::Pull,
        Cursor::PaintPart,
        Cursor::PaintFace,
        Cursor::Picker,
    ];
}

/// An interactive tool.
pub trait Tool {
    fn kind(&self) -> ToolKind;
    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command>;
    fn preview(&self, cx: &Cx<'_>) -> Preview;
    /// A hint for the status bar.
    fn status(&self) -> String;
    /// What the measurements box takes now, with the current values, if anything.
    fn vcb(&self) -> Option<Vcb> {
        None
    }
    /// Does the tool copy rather than move (the cursor shows a « + »)?
    fn copying(&self) -> bool {
        false
    }
    /// The cursor at the tool's step now (docs/ui/cursors.md); none: the system's.
    fn cursor(&self, _cx: &Cx<'_>) -> Option<Cursor> {
        None
    }
    /// Is an operation under way (Esc cancels it rather than clearing the selection)?
    fn busy(&self) -> bool;
    /// The body of the part (or group) open whose profile the tool edits, if any.
    fn profile_body(&self, _cx: &Cx<'_>) -> Option<Id> {
        None
    }
    /// The options the bar shows (docs/ui/interface.md), labels as they read now.
    fn options(&self, _cx: &Cx<'_>) -> Vec<ToolOption> {
        Vec::new()
    }
    /// An option of the gesture changed (a saved one is stored by the editor, read in
    /// [`Cx::options`]).
    fn set_option(&mut self, _key: &str, _value: &OptionValue) {}
    /// Made the active tool, its saved options read: what it takes from where it starts.
    fn started(&mut self, _cx: &Cx<'_>) {}
    /// Does ↓ lock the normal of the face under the cursor now, rather than free the axis
    /// (the shape tools' drawing plane)?
    fn down_locks_face(&self) -> bool {
        false
    }
    /// A line the tool draws and infers on, as the axes as drawn, its origin as an endpoint
    /// (the « Tournage » axis): origin and unit direction, in the axes.
    fn infer_line(&self) -> Option<(DVec3, DVec3)> {
        None
    }
    /// What a drag starting now is to the tool: a drag, or clicks (the shape tools).
    fn drag_clicks(&self) -> DragClicks {
        DragClicks::No
    }
    /// The cursor left the view (off it, or over a panel): what follows it is hidden.
    fn leave(&mut self) {}
    /// Is the tool waiting on the time, the cursor resting (the prism tool's switch to tangent
    /// arcs)? The editor then gives it the cursor again as time goes by.
    fn waiting(&self) -> bool {
        false
    }
    /// Does Delete go to the tool now rather than erase the selection (the profile's corner
    /// under the cursor)?
    fn takes_delete(&self) -> bool {
        false
    }
    /// Do Shift with `+` / `−` change the count of copies now ([`Key::More`], [`Key::Fewer`])
    /// rather than start a typed value?
    fn takes_counts(&self) -> bool {
        false
    }
    /// The length option a value typed with this suffix sets (the prism tool's `r`, its fillet);
    /// `x` is the first length option's otherwise (the offset).
    fn length_suffix(&self, _suffix: char) -> Option<&'static str> {
        None
    }
    /// The commands for the level `path` chosen in the tree drop-up (docs/tools/selection.md), if the tool
    /// takes it its own way (Paint paints it); none: it is selected.
    fn tree_pick(&self, _path: &EntityPath, _cx: &Cx<'_>) -> Option<Vec<Command>> {
        None
    }
    /// Does the tool take parts at any depth under « Parts » (docs/tools/selection.md)?
    /// Its bar then shows the selection's scope.
    fn picks_parts(&self) -> bool {
        false
    }
    /// Does the tool pick what it acts on first (docs/tools/selection.md)? Esc at its
    /// first step then lets the selection go, the tool picking again.
    fn picks_first(&self) -> bool {
        false
    }
    /// Is the tool at its pick step now (what it acts on not taken yet, or a click taking
    /// another, a part clicked by a boolean)? The cursor is then the arrow, as Select's.
    fn picking(&self) -> bool {
        false
    }
    /// Has its last step back dropped what it acts on (the copy tools' target)? Esc then lets
    /// the selection go too, the tool picking again (docs/tools/selection.md).
    fn dropped(&self) -> bool {
        false
    }
    /// Its steps, for the steps bar (none: no bar).
    fn steps(&self) -> Option<Steps> {
        None
    }
    /// Does it wait for a validation now (Enter goes on, Esc steps back)? The validation
    /// buttons then show (docs/ui/interface.md#steps-bar, OpenCutList's `show_validation`).
    fn awaits_validation(&self) -> bool {
        false
    }
    /// One step back (Esc while busy); by default, back to the first.
    fn back(&mut self) {
        self.reset();
    }
    /// Back to the first step.
    fn reset(&mut self);
}

/// How a copy was moved, in the world, as a function of a fraction of the way (for arrays).
#[derive(Clone, Copy, Debug)]
pub enum Motion {
    Translate(DVec3),
    /// A rotation about the line through `center` along `axis`.
    Rotate {
        center: DVec3,
        axis: DVec3,
        angle: f64,
    },
}

impl Motion {
    /// `fraction` of the motion, in the world.
    pub fn part(&self, fraction: f64) -> DAffine3 {
        match *self {
            Motion::Translate(v) => DAffine3::from_translation(v * fraction),
            Motion::Rotate { center, axis, angle } => rotate::rotation_about(center, axis, angle * fraction),
        }
    }
}

/// What the move and rotate tools change: occurrences of the context, or machinings of the
/// open part (docs/model/opening-a-part.md); each in its own frame (to world), which operations are expressed in.
#[derive(Clone, Debug)]
pub enum Moved {
    Occurrences {
        context: Id,
        frame: DAffine3,
    },
    Machinings {
        target: EntityPath,
        scope: Scope,
        frame: DAffine3,
    },
    /// Parts at any depth (« Parts », docs/tools/selection.md), per parent assembly; with
    /// `unique`, the assemblies along each parent's path made unique first (« only this
    /// one »). The entities given to the methods are left out: the groups hold them. One
    /// operation at each parent, in one step.
    Deep {
        groups: Vec<DeepGroup>,
        unique: bool,
    },
}

/// The parts selected at any depth in one assembly (docs/tools/selection.md).
#[derive(Clone, Debug)]
pub struct DeepGroup {
    /// The occurrences from the root to the assembly.
    pub parent: Vec<Id>,
    /// The assembly's axes to world.
    pub frame: DAffine3,
    /// Its occurrences (and guides) selected.
    pub entities: Vec<Id>,
    /// A part's guides instead: the part `parent` leads into, placed by this path; `frame` its
    /// axes to world, `entities` its guides.
    pub part: Option<EntityPath>,
}

/// `op` on occurrences of a context made on the bodies (here, guides) of the part `target`
/// places, in its axes; none for an operation that has no such form.
fn on_bodies(op: Operation, target: EntityPath) -> Option<Operation> {
    let scope = Scope::All;
    Some(match op {
        Operation::Transform { entities, transform, .. } => Operation::TransformBodies { target, bodies: entities, transform, scope },
        Operation::Duplicate { entities, transform, .. } => Operation::DuplicateBodies { target, bodies: entities, transform, scope },
        Operation::Array { entities, step, count, .. } => Operation::ArrayBodies { target, bodies: entities, step, count, scope },
        Operation::Copies { entities, moved, placements, .. } => {
            Operation::CopyBodies { target, bodies: entities, moved, placements, scope }
        }
        Operation::Delete { entities, .. } => Operation::DeleteBodies { target, bodies: entities, scope },
        _ => return None,
    })
}

/// The world motion `world`, in the frame `frame` (to world).
fn local_in(frame: &DAffine3, world: DAffine3) -> DAffine3 {
    if world.matrix3 == glam::DMat3::IDENTITY {
        // A translation stays one exactly.
        return DAffine3::from_translation(frame.matrix3.inverse().mul_vec3(world.translation));
    }
    frame.inverse() * world * *frame
}

impl Moved {
    /// The world motion `world`, in the frame (the root's for parts at any depth: each group
    /// takes its own, [`Moved::at_each`]).
    fn local(&self, world: DAffine3) -> DAffine3 {
        match self {
            Moved::Occurrences { frame, .. } | Moved::Machinings { frame, .. } => local_in(frame, world),
            Moved::Deep { .. } => world,
        }
    }

    /// Parts at any depth: `op` made at each group from its frame and occurrences, applied
    /// at its parent, all in one step.
    /// A part's guides take the operation on its bodies instead, at the part (the scope
    /// ignored); those that have none (hiding, painting) leave them.
    fn at_each(groups: &[DeepGroup], unique: bool, op: impl Fn(&DAffine3, Vec<Id>) -> Operation) -> Operation {
        Operation::Batch(
            groups
                .iter()
                .filter_map(|g| {
                    let o = op(&g.frame, g.entities.clone());
                    match &g.part {
                        Some(target) => on_bodies(o, target.clone()),
                        None => Some(Operation::At { parent: g.parent.clone(), unique, op: Box::new(o) }),
                    }
                })
                .collect(),
        )
    }

    pub fn transform(&self, entities: Vec<Id>, world: DAffine3) -> Operation {
        if let Moved::Deep { groups, unique } = self {
            return Self::at_each(groups, *unique, |f, entities| Operation::Transform {
                context: Id(0),
                entities,
                transform: local_in(f, world),
            });
        }
        let transform = self.local(world);
        match self {
            Moved::Occurrences { context, .. } => Operation::Transform { context: *context, entities, transform },
            Moved::Machinings { target, scope, .. } => {
                Operation::TransformBodies { target: target.clone(), bodies: entities, transform, scope: *scope }
            }
            Moved::Deep { .. } => unreachable!(),
        }
    }

    /// A paste (docs/tools/copy-paste.md) placed `world` further: its transformation, from what was copied
    /// to the frame, moved by `world` in the frame. Any other operation is kept as it is.
    pub fn paste(&self, op: &Operation, world: DAffine3) -> Operation {
        let local = self.local(world);
        match op.clone() {
            Operation::Paste { context, fragment, transform, reuse } => {
                Operation::Paste { context, fragment, transform: local * transform, reuse }
            }
            Operation::PasteBodies { target, fragment, transform, scope } => {
                Operation::PasteBodies { target, fragment, transform: local * transform, scope }
            }
            Operation::PasteParts { target, fragment, transform, scope } => {
                Operation::PasteParts { target, fragment, transform: local * transform, scope }
            }
            Operation::PasteAsPart { context, fragment, transform } => {
                Operation::PasteAsPart { context, fragment, transform: local * transform }
            }
            // A component placed from the library (docs/ui/components-library.md#placing).
            Operation::PlaceOccurrence { context, component, transform } => {
                Operation::PlaceOccurrence { context, component, transform: local * transform }
            }
            other => other,
        }
    }

    pub fn duplicate(&self, entities: Vec<Id>, world: DAffine3) -> Operation {
        let transform = self.local(world);
        match self {
            Moved::Occurrences { context, .. } => Operation::Duplicate { context: *context, entities, transform },
            Moved::Machinings { target, scope, .. } => {
                Operation::DuplicateBodies { target: target.clone(), bodies: entities, transform, scope: *scope }
            }
            Moved::Deep { groups, unique } => Self::at_each(groups, *unique, |f, entities| Operation::Duplicate {
                context: Id(0),
                entities,
                transform: local_in(f, world),
            }),
        }
    }

    /// Copies at the world transforms `placements`, the sources moved by `moved` (world): the
    /// copy tools' operation (docs/tools/copy-array.md).
    pub fn copies(&self, entities: Vec<Id>, moved: DAffine3, placements: &[DAffine3]) -> Operation {
        let moved = self.local(moved);
        let placements = placements.iter().map(|w| self.local(*w)).collect();
        match self {
            Moved::Occurrences { context, .. } => Operation::Copies { context: *context, entities, moved, placements },
            Moved::Machinings { target, scope, .. } => {
                Operation::CopyBodies { target: target.clone(), bodies: entities, moved, placements, scope: *scope }
            }
            Moved::Deep { groups, unique } => Self::at_each(groups, *unique, |f, entities| Operation::Copies {
                context: Id(0),
                entities,
                moved: local_in(f, moved),
                placements: placements.iter().map(|w| local_in(f, *w)).collect(),
            }),
        }
    }

    pub fn array(&self, entities: Vec<Id>, world_step: DAffine3, count: u32) -> Operation {
        let step = self.local(world_step);
        match self {
            Moved::Occurrences { context, .. } => Operation::Array { context: *context, entities, step, count },
            Moved::Machinings { target, scope, .. } => {
                Operation::ArrayBodies { target: target.clone(), bodies: entities, step, count, scope: *scope }
            }
            Moved::Deep { groups, unique } => Self::at_each(groups, *unique, |f, entities| Operation::Array {
                context: Id(0),
                entities,
                step: local_in(f, world_step),
                count,
            }),
        }
    }
}

/// A copy just made: typing `5x` (or `*5`) right after makes it an array of 5 at the same
/// step, `/5` 5 copies spread over the same way; retyping corrects (SketchUp).
#[derive(Clone, Debug)]
pub struct LastCopy {
    pub moved: Moved,
    pub entities: Vec<Id>,
    pub motion: Motion,
    /// The operation applied last for it (the one a new count replaces).
    pub op: Operation,
}

impl LastCopy {
    /// The operation for the copy itself.
    pub fn new(moved: Moved, entities: Vec<Id>, motion: Motion) -> Self {
        let op = moved.duplicate(entities.clone(), motion.part(1.0));
        LastCopy { moved, entities, motion, op }
    }

    /// A typed count: the commands replacing the copy (or the last array) by the new array.
    pub fn retype(&mut self, repeat: Repeat) -> Vec<Command> {
        let (fraction, count) = match repeat {
            Repeat::Times(n) => (1.0, n),
            Repeat::Divide(n) => (1.0 / n as f64, n),
        };
        let op = self.moved.array(self.entities.clone(), self.motion.part(fraction), count);
        let previous = std::mem::replace(&mut self.op, op.clone());
        vec![Command::Replace { previous: Box::new(previous), op: Box::new(op) }]
    }
}

pub fn make(kind: ToolKind) -> Box<dyn Tool> {
    match kind {
        ToolKind::Select => Box::new(SelectTool::default()),
        ToolKind::Box => Box::new(BoxTool::default()),
        ToolKind::Cylinder => Box::new(CylinderTool::default()),
        ToolKind::Prism => Box::new(PrismTool::default()),
        ToolKind::Turn => Box::new(PrismTool::turning()),
        ToolKind::Move => Box::new(MoveTool::default()),
        ToolKind::Rotate => Box::new(RotateTool::default()),
        ToolKind::Boolean(op) => Box::new(BooleanTool::new(op)),
        ToolKind::Erase => Box::new(EraseTool::default()),
        ToolKind::Paint => Box::new(PaintTool::default()),
        ToolKind::Sample => Box::new(SampleTool::default()),
        ToolKind::Section => Box::new(SectionTool::default()),
        ToolKind::Measure => Box::new(MeasureTool::default()),
        ToolKind::Stretch => Box::new(StretchTool::default()),
        ToolKind::Reorient => Box::new(ReorientTool::default()),
        ToolKind::Imprint => Box::new(imprint::ImprintTool::default()),
        ToolKind::CopyLine => Box::new(CopyLineTool::default()),
        ToolKind::CopyGrid => Box::new(CopyGridTool::default()),
        ToolKind::Distribute => Box::new(DistributeTool::default()),
        ToolKind::Profile => Box::new(ProfileTool::default()),
        ToolKind::Round => Box::new(BlendTool::new(skraft_model::kernel::names::Blend::Round)),
        ToolKind::Chamfer => Box::new(BlendTool::new(skraft_model::kernel::names::Blend::Chamfer)),
    }
}

/// How near a guide the cursor picks it (pixels).
pub const GUIDE_PICK_PX: f64 = 5.0;

/// The copy modifier's name for hints (SketchUp: Option on macOS, Ctrl elsewhere).
pub fn copy_key() -> String {
    if skraft_i18n::mac() { t!("key.option") } else { t!("key.ctrl") }
}

/// The other modifier's name for hints (Command on macOS, Alt elsewhere).
pub fn alt_key() -> String {
    if skraft_i18n::mac() { t!("key.command") } else { t!("key.alt") }
}

/// `hint`, said to be copying when it is.
fn with_copying(hint: String, copying: bool) -> String {
    if copying { format!("{hint} {}", t!("tool.copying")) } else { hint }
}

// The tools' inks, named by the light mode's: the app draws each in the mode's
// (`Theme::ink`, docs/render/look.md#view-background).
/// Neutral colour of construction lines.
pub const INK: [f32; 4] = skraft_theme::GUIDE_INK;
/// Axis colours (SketchUp's red, green, blue).
pub const RED: [f32; 4] = skraft_theme::AXIS_X;
pub const GREEN: [f32; 4] = skraft_theme::AXIS_Y;
pub const BLUE: [f32; 4] = skraft_theme::AXIS_Z;
/// Parallel and perpendicular guides (SketchUp's magenta).
pub const MAGENTA: [f32; 4] = skraft_theme::FREE_LINE;
/// The highlight colour of picked things.
pub const PICK: [f32; 4] = skraft_theme::PICK;
// The same in either mode.
pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
pub const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// What is inactive or about to go: an axis, a cutter, the legs of a distribute.
pub const GREY: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
/// How far from 1 the cosine of a direction and an axis it runs along exactly may be.
pub(crate) const ALONG: f64 = 1e-9;

/// The ink of the axis `k` (0 red X, 1 green Y, 2 blue Z).
pub fn axis_ink(k: usize) -> [f32; 4] {
    [RED, GREEN, BLUE][k]
}

/// The ink of the one of `axes` (unit) that `d` runs along, either way, within `tolerance` of
/// the cosine; none along no axis, or for no direction.
pub(crate) fn ink_along(axes: [DVec3; 3], d: DVec3, tolerance: f64) -> Option<[f32; 4]> {
    let d = d.try_normalize()?;
    (0..3).find(|&k| axes[k].dot(d).abs() > 1.0 - tolerance).map(axis_ink)
}

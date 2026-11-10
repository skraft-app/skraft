//! Interactive tools (ARCHITECTURE.md §6): each is a state machine fed with pointer and key
//! input; it previews without touching the document and, when done, emits an `Operation`.

mod boolean;
mod mover;
mod paint;
mod rotate;
mod section;
mod select;
mod shapes;

pub use boolean::BooleanTool;
pub use mover::MoveTool;
pub use paint::PaintTool;
pub use rotate::RotateTool;
pub use section::SectionTool;
pub use select::SelectTool;
pub use shapes::{BoxTool, CylinderTool};

use std::collections::BTreeMap;

use glam::{DAffine3, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use solidsketch_i18n::t;
use solidsketch_model::{Boolean, Document, EntityPath, Id, Operation, Primitive, Scope, Section, Units};

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
    /// Turns the tool's first choice over to its next value (the booleans' scope).
    Tab,
}

/// Primary-button and keyboard input for the active tool (navigation is the editor's). Pixel
/// positions are in the viewport, origin top left.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolInput {
    Hover { px: DVec2 },
    Click { px: DVec2, mods: Modifiers },
    DoubleClick { px: DVec2, mods: Modifiers },
    /// The primary button pressed at `px` and dragged away (past the click distance).
    DragStart { px: DVec2, mods: Modifiers },
    /// The cursor moved while dragging.
    Drag { px: DVec2 },
    /// The primary button released at `px`, ending a drag.
    DragEnd { px: DVec2, mods: Modifiers },
    Key { key: Key, mods: Modifiers },
    /// A value typed in the measurements box and validated with Enter (the editor reads it
    /// for the tool's [`Tool::vcb`] and gives it a [`ToolInput::Value`]).
    Vcb(String),
    /// What was typed, read for the tool's [`Tool::vcb`].
    Value(VcbValue),
}

/// What a tool asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Apply(Operation),
    /// Replaces the selection (entities of the active context).
    Select(Vec<Id>),
    /// Enters the assembly the occurrence `Id` of the active context places, or opens its
    /// part.
    Enter(Id),
    /// Closes the open part, else leaves the active context for its parent.
    Exit,
    /// A click on nothing of the active context, at this pixel: off the grey box of the part
    /// open or the assembly being edited, it leaves it ([`Command::Exit`]); inside, it clears
    /// the selection.
    ClickOff(DVec2),
    /// A message for the status bar (e.g. why a value was refused).
    Say(String),
    /// Undoes the last step, which must be `previous`, and applies `op` instead (a copy
    /// turned into an array by a typed `5x`).
    Replace { previous: Operation, op: Operation },
}

/// A line a tool previews, in world coordinates (linear RGBA).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewLine {
    pub a: DVec3,
    pub b: DVec3,
    pub color: [f32; 4],
}

/// What a tool shows while it works.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    /// Lines drawn over the scene.
    pub lines: Vec<PreviewLine>,
    /// The entity of the active context under the cursor.
    pub hover: Option<Id>,
    /// A solid anywhere in the scene under the cursor (tools that reach across contexts).
    pub hover_solid: Option<EntityPath>,
    /// A solid already picked (the boolean tool's target).
    pub picked_solid: Option<EntityPath>,
    /// The selection drawn moved by this world transform (the move tool's ghost).
    pub ghost: Option<DAffine3>,
    /// The point the tool infers under the cursor (marker and tooltip).
    pub inference: Option<Inference>,
    /// The selection rectangle being dragged.
    pub marquee: Option<Marquee>,
    /// The occurrence a change is aimed at, when it reaches every occurrence of its part: the
    /// others are marked too (ARCHITECTURE.md §2.5).
    pub propagate: Option<EntityPath>,
    /// A section being placed, drawn instead of the document's (§7 quater).
    pub section: Option<Section>,
}

// ---- options (ARCHITECTURE.md §6 bis, the tool's option bar) ---------------------------------

/// The value of a tool option.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionValue {
    /// One of a choice's values.
    Choice(String),
    Toggle(bool),
}

/// One of a choice's values, as the bar shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct OptionChoice {
    pub value: &'static str,
    pub label: String,
    pub tip: String,
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
}

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
}

impl Marquee {
    pub fn new(from: DVec2, to: DVec2) -> Self {
        Marquee { from, to, crossing: to.x < from.x, hits: Vec::new() }
    }
}

/// Which tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Select,
    Box,
    Cylinder,
    Move,
    Rotate,
    Boolean(Boolean),
    /// Paints occurrences or faces (§7 quater).
    Paint,
    /// Places the section plane (§7 quater).
    Section,
}

impl ToolKind {
    /// A stable name, for the settings kept per tool.
    pub fn id(self) -> &'static str {
        match self {
            ToolKind::Select => "select",
            ToolKind::Box => "box",
            ToolKind::Cylinder => "cylinder",
            ToolKind::Move => "move",
            ToolKind::Rotate => "rotate",
            ToolKind::Boolean(Boolean::Union) => "union",
            ToolKind::Boolean(Boolean::Difference) => "difference",
            ToolKind::Boolean(Boolean::Intersection) => "intersection",
            ToolKind::Paint => "paint",
            ToolKind::Section => "section",
        }
    }

    /// Its name in the option bar and the menus.
    pub fn label(self) -> String {
        match self {
            ToolKind::Select => t!("tool.select.name"),
            ToolKind::Box => t!("tool.box.name"),
            ToolKind::Cylinder => t!("tool.cylinder.name"),
            ToolKind::Move => t!("tool.move.name"),
            ToolKind::Rotate => t!("tool.rotate.name"),
            ToolKind::Boolean(Boolean::Union) => t!("tool.boolean.union.name"),
            ToolKind::Boolean(Boolean::Difference) => t!("tool.boolean.difference.name"),
            ToolKind::Boolean(Boolean::Intersection) => t!("tool.boolean.intersection.name"),
            ToolKind::Paint => t!("tool.paint.name"),
            ToolKind::Section => t!("tool.section.name"),
        }
    }
}

/// What a tool sees of the editor.
pub struct Cx<'a> {
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
    /// The part open in the context, by its occurrence there (ARCHITECTURE.md §2.9).
    pub open_part: Option<Id>,
    /// The open part's axes in the context's (identity without one): tools draw and infer
    /// in these axes (§2.3), see [`Cx::axes`].
    pub axes_in_context: DAffine3,
    /// The open part's recipe, while its machinings are shown: the selection is machinings
    /// then, and the tools act on them (§2.9).
    pub recipe: Option<&'a RecipeMeshes>,
    /// What a change to the open part reaches (§2.5).
    pub part_scope: Scope,
    pub selection: &'a [Id],
    /// The context axis the arrow keys lock (→ 0 red, ← 1 green, ↑ 2 blue; ↓ frees it).
    pub axis_lock: Option<usize>,
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
        self.open_part.map(|id| EntityPath::new(self.context.to_vec(), id))
    }

    /// Are the open part's machinings shown (the selection being machinings)?
    pub fn machinings_shown(&self) -> bool {
        self.recipe.is_some() && self.open_part.is_some()
    }

    /// The machining whose tool is under `px`, while they are shown.
    pub fn pick_machining(&self, px: DVec2) -> Option<Id> {
        if !self.machinings_shown() {
            return None;
        }
        self.scene.pick_tool(&self.ray(px), self.axes()).map(|(id, _)| id)
    }

    /// What a click at `px` takes when nothing is selected (move, rotate): the machining
    /// under it while they are shown, else the entity of the context `inf` is on.
    pub fn entity_under(&self, px: DVec2, inf: Option<&Inference>) -> Option<Id> {
        if self.machinings_shown() {
            return self.pick_machining(px);
        }
        let under = inf.and_then(|i| i.hit.as_ref().map(|h| &h.path).or(i.path.as_ref()));
        under.and_then(|p| self.selectable(p))
    }

    /// What moving the selection changes, and in which frame.
    pub fn moved(&self) -> Moved {
        match self.open_part_path().filter(|_| self.machinings_shown()) {
            Some(target) => Moved::Machinings { target, scope: self.part_scope, frame: self.axes() },
            None => Moved::Occurrences { context: self.context_def, frame: self.context_transform },
        }
    }

    /// The operation making `primitive`, drawn in the axes and placed by `placement` there:
    /// in an open part, a machining of it (§2.9); else a new part of the context.
    pub fn add_primitive(&self, primitive: Primitive, placement: DAffine3) -> Operation {
        match self.open_part_path() {
            Some(target) => Operation::AddMachining { target, primitive, placement, scope: self.part_scope },
            None => Operation::AddPrimitive { context: self.context_def, primitive, placement: self.axes_in_context * placement },
        }
    }

    /// Is `o` one of `moving` (occurrences of the context, or machinings of the open part)?
    /// What a move or a rotation leaves out of inferences.
    pub fn is_moving(&self, o: &Owner<'_>, moving: &[Id]) -> bool {
        match o.piece {
            Piece::Tool(id) => moving.contains(&id),
            Piece::Blank => false,
            Piece::Shape => self.selectable(o.path).is_some_and(|id| moving.contains(&id)),
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
        self.selectable(path).is_some_and(|id| self.selection.contains(&id))
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

    /// The ground of the axes: their origin and Z axis, in the world.
    pub fn ground(&self) -> (DVec3, DVec3) {
        (self.to_world(DVec3::ZERO), self.axis(2))
    }

    /// The point inferred under `px` (§6 ter), on the solids `keep` accepts; `plane` (world
    /// point and normal) when nothing is under the cursor, by default the context's ground.
    pub fn infer(&self, px: DVec2, keep: impl Fn(&Owner<'_>) -> bool, plane: Option<(DVec3, DVec3)>) -> Option<Inference> {
        self.query(px, &keep, plane, None, false, None)
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
        self.query(px, &keep, plane, Some(from), false, lock)
    }

    /// Lines through `from` that lie in `plane` only (a protractor's), never locked.
    pub fn infer_in_plane(&self, px: DVec2, keep: impl Fn(&Owner<'_>) -> bool, plane: (DVec3, DVec3), from: DVec3) -> Option<Inference> {
        self.query(px, &keep, Some(plane), Some(from), true, None)
    }

    fn query(
        &self,
        px: DVec2,
        keep: &dyn Fn(&Owner<'_>) -> bool,
        plane: Option<(DVec3, DVec3)>,
        from: Option<DVec3>,
        in_plane: bool,
        lock: Option<Lock>,
    ) -> Option<Inference> {
        let q = Query {
            keep,
            plane: plane.unwrap_or_else(|| self.ground()),
            origin: Some(self.to_world(DVec3::ZERO)),
            from,
            tracked: self.tracked,
            in_plane,
            axes: [self.axis(0), self.axis(1), self.axis(2)],
            reference: self.reference,
            lock,
        };
        infer::infer(self.scene, self.camera, self.size, px, &q)
    }

    /// The saved value of the choice `key`, if any.
    pub fn choice(&self, key: &str) -> Option<&str> {
        match self.options.get(key) {
            Some(OptionValue::Choice(v)) => Some(v),
            _ => None,
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
/// perpendicular, else ink.
pub fn guide_color(kind: InferKind) -> [f32; 4] {
    match kind.line().map(|l| l.axis()) {
        Some(Some(0)) => RED,
        Some(Some(1)) => GREEN,
        Some(Some(_)) => BLUE,
        Some(None) => MAGENTA,
        None => INK,
    }
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
    /// Is an operation under way (Esc cancels it rather than clearing the selection)?
    fn busy(&self) -> bool;
    /// The options the bar shows (ARCHITECTURE.md §6 bis), labels as they read now.
    fn options(&self, _cx: &Cx<'_>) -> Vec<ToolOption> {
        Vec::new()
    }
    /// An option of the gesture changed (a saved one is stored by the editor, read in
    /// [`Cx::options`]).
    fn set_option(&mut self, _key: &str, _value: &OptionValue) {}
    /// Back to the first step.
    fn reset(&mut self);
}

/// How a copy was moved, in the world, as a function of a fraction of the way (for arrays).
#[derive(Clone, Copy, Debug)]
pub enum Motion {
    Translate(DVec3),
    /// A rotation about the line through `center` along `axis`.
    Rotate { center: DVec3, axis: DVec3, angle: f64 },
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
/// open part (§2.9); each in its own frame (to world), which operations are expressed in.
#[derive(Clone, Debug)]
pub enum Moved {
    Occurrences { context: Id, frame: DAffine3 },
    Machinings { target: EntityPath, scope: Scope, frame: DAffine3 },
}

impl Moved {
    /// The world motion `world`, in the frame.
    fn local(&self, world: DAffine3) -> DAffine3 {
        let (Moved::Occurrences { frame, .. } | Moved::Machinings { frame, .. }) = self;
        if world.matrix3 == glam::DMat3::IDENTITY {
            // A translation stays one exactly.
            return DAffine3::from_translation(frame.matrix3.inverse().mul_vec3(world.translation));
        }
        frame.inverse() * world * *frame
    }

    pub fn transform(&self, entities: Vec<Id>, world: DAffine3) -> Operation {
        let transform = self.local(world);
        match self {
            Moved::Occurrences { context, .. } => Operation::Transform { context: *context, entities, transform },
            Moved::Machinings { target, scope, .. } => {
                Operation::TransformMachinings { target: target.clone(), machinings: entities, transform, scope: *scope }
            }
        }
    }

    pub fn duplicate(&self, entities: Vec<Id>, world: DAffine3) -> Operation {
        let transform = self.local(world);
        match self {
            Moved::Occurrences { context, .. } => Operation::Duplicate { context: *context, entities, transform },
            Moved::Machinings { target, scope, .. } => {
                Operation::DuplicateMachinings { target: target.clone(), machinings: entities, transform, scope: *scope }
            }
        }
    }

    pub fn array(&self, entities: Vec<Id>, world_step: DAffine3, count: u32) -> Operation {
        let step = self.local(world_step);
        match self {
            Moved::Occurrences { context, .. } => Operation::Array { context: *context, entities, step, count },
            Moved::Machinings { target, scope, .. } => {
                Operation::ArrayMachinings { target: target.clone(), machinings: entities, step, count, scope: *scope }
            }
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
        vec![Command::Replace { previous, op }]
    }
}

pub fn make(kind: ToolKind) -> Box<dyn Tool> {
    match kind {
        ToolKind::Select => Box::new(SelectTool::default()),
        ToolKind::Box => Box::new(BoxTool::default()),
        ToolKind::Cylinder => Box::new(CylinderTool::default()),
        ToolKind::Move => Box::new(MoveTool::default()),
        ToolKind::Rotate => Box::new(RotateTool::default()),
        ToolKind::Boolean(op) => Box::new(BooleanTool::new(op)),
        ToolKind::Paint => Box::new(PaintTool::default()),
        ToolKind::Section => Box::new(SectionTool::default()),
    }
}

/// The copy modifier's name for hints (SketchUp: Option on macOS, Ctrl elsewhere).
pub fn copy_key() -> String {
    if solidsketch_i18n::mac() { t!("key.option") } else { t!("key.ctrl") }
}

/// `hint`, said to be copying when it is.
fn with_copying(hint: String, copying: bool) -> String {
    if copying { format!("{hint} {}", t!("tool.copying")) } else { hint }
}

/// Neutral colour of construction lines (the theme's tokens, §7 quater).
pub const INK: [f32; 4] = solidsketch_theme::GUIDE_INK;
/// Axis colours (SketchUp's red, green, blue).
pub const RED: [f32; 4] = solidsketch_theme::AXIS_X;
pub const GREEN: [f32; 4] = solidsketch_theme::AXIS_Y;
pub const BLUE: [f32; 4] = solidsketch_theme::AXIS_Z;
/// Parallel and perpendicular guides (SketchUp's magenta).
pub const MAGENTA: [f32; 4] = solidsketch_theme::FREE_LINE;
/// The highlight colour of picked things.
pub const PICK: [f32; 4] = solidsketch_theme::PICK;

/// A small cross marking a point, sized for the screen.
pub fn cross(cx: &Cx<'_>, p: DVec3, color: [f32; 4]) -> Vec<PreviewLine> {
    let s = cx.camera.world_per_pixel(cx.size, p) * 6.0;
    [DVec3::X, DVec3::Y, DVec3::Z]
        .into_iter()
        .map(|d| PreviewLine { a: p - d * s, b: p + d * s, color })
        .collect()
}

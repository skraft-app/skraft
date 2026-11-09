//! Interactive tools (ARCHITECTURE.md §6): each is a state machine fed with pointer and key
//! input; it previews without touching the document and, when done, emits an `Operation`.

mod boolean;
mod mover;
mod rotate;
mod select;
mod shapes;

pub use boolean::BooleanTool;
pub use mover::MoveTool;
pub use rotate::RotateTool;
pub use select::SelectTool;
pub use shapes::{BoxTool, CylinderTool};

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_model::{Boolean, Document, EntityPath, Id, Operation, Units};

use crate::camera::{Camera, Ray};
use crate::infer::{self, EdgeRef, InferKind, Inference, LineKind, Lock, Query};
use crate::scene::{DisplayScene, Hit};

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
}

/// Primary-button and keyboard input for the active tool (navigation is the editor's). Pixel
/// positions are in the viewport, origin top left.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolInput {
    Hover { px: DVec2 },
    Click { px: DVec2, mods: Modifiers },
    DoubleClick { px: DVec2, mods: Modifiers },
    Key { key: Key, mods: Modifiers },
    /// A value typed in the measurements box and validated with Enter.
    Vcb(String),
}

/// What a tool asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Apply(Operation),
    /// Replaces the selection (entities of the active context).
    Select(Vec<Id>),
    /// Enters the group or component instance `Id` of the active context.
    Enter(Id),
    /// Leaves the active context for its parent.
    Exit,
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
}

/// What a tool sees of the editor.
pub struct Cx<'a> {
    pub doc: &'a Document,
    pub scene: &'a DisplayScene,
    pub camera: &'a Camera,
    pub size: DVec2,
    /// Instance path of the context being edited (empty: the model).
    pub context: &'a [Id],
    /// The definition being edited.
    pub context_def: Id,
    /// Definition to world, for the context.
    pub context_transform: DAffine3,
    pub selection: &'a [Id],
    /// The context axis the arrow keys lock (→ 0 red, ← 1 green, ↑ 2 blue; ↓ frees it).
    pub axis_lock: Option<usize>,
    /// The line Shift holds.
    pub shift_lock: Option<Lock>,
    /// Points tracked (hovered a while), world.
    pub tracked: &'a [DVec3],
    /// The last edge hovered, straight or arc (world).
    pub reference: Option<EdgeRef>,
}

impl Cx<'_> {
    pub fn ray(&self, px: DVec2) -> Ray {
        self.camera.ray(px, self.size)
    }

    /// The ray in the context's coordinates.
    pub fn context_ray(&self, px: DVec2) -> Ray {
        let r = self.ray(px);
        let inv = self.context_transform.inverse();
        Ray { origin: inv.transform_point3(r.origin), dir: inv.transform_vector3(r.dir).normalize(), t_min: r.t_min }
    }

    /// The nearest solid under `px`, anywhere.
    pub fn pick(&self, px: DVec2) -> Option<Hit> {
        self.scene.pick(&self.ray(px), |_| true)
    }

    /// The entity of the active context a hit belongs to: the solid itself if it is in the
    /// context, else the context's instance that leads to it; `None` outside the context.
    pub fn selectable(&self, path: &EntityPath) -> Option<Id> {
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

    pub fn to_world(&self, p: DVec3) -> DVec3 {
        self.context_transform.transform_point3(p)
    }

    pub fn to_context(&self, p: DVec3) -> DVec3 {
        self.context_transform.inverse().transform_point3(p)
    }

    /// The context's axis `k` (0 red X, 1 green Y, 2 blue Z), in the world.
    pub fn axis(&self, k: usize) -> DVec3 {
        self.context_transform.matrix3.col(k).normalize()
    }

    /// The context's ground: its origin and Z axis, in the world.
    pub fn ground(&self) -> (DVec3, DVec3) {
        (self.to_world(DVec3::ZERO), self.axis(2))
    }

    /// The point inferred under `px` (§6 ter), on the solids `keep` accepts; `plane` (world
    /// point and normal) when nothing is under the cursor, by default the context's ground.
    pub fn infer(&self, px: DVec2, keep: impl Fn(&EntityPath) -> bool, plane: Option<(DVec3, DVec3)>) -> Option<Inference> {
        self.query(px, &keep, plane, None, false, None)
    }

    /// The same, with lines inferred through the start point `from` (world), kept on `lock`.
    pub fn infer_from(
        &self,
        px: DVec2,
        keep: impl Fn(&EntityPath) -> bool,
        plane: Option<(DVec3, DVec3)>,
        from: DVec3,
        lock: Option<Lock>,
    ) -> Option<Inference> {
        self.query(px, &keep, plane, Some(from), false, lock)
    }

    /// Lines through `from` that lie in `plane` only (a protractor's), never locked.
    pub fn infer_in_plane(&self, px: DVec2, keep: impl Fn(&EntityPath) -> bool, plane: (DVec3, DVec3), from: DVec3) -> Option<Inference> {
        self.query(px, &keep, Some(plane), Some(from), true, None)
    }

    fn query(
        &self,
        px: DVec2,
        keep: &dyn Fn(&EntityPath) -> bool,
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
    /// The measurements box's label, when the tool takes a value now.
    fn vcb_label(&self) -> Option<&'static str> {
        None
    }
    /// The measurements box's current value, lengths in `units`.
    fn vcb_value(&self, _units: &Units) -> String {
        String::new()
    }
    /// Does the tool copy rather than move (the cursor shows a « + »)?
    fn copying(&self) -> bool {
        false
    }
    /// Is an operation under way (Esc cancels it rather than clearing the selection)?
    fn busy(&self) -> bool;
    /// Back to the first step.
    fn reset(&mut self);
}

/// How a copy was moved, as a function of a fraction of the way (for arrays).
#[derive(Clone, Copy, Debug)]
pub enum Motion {
    /// A translation, in the context's coordinates.
    Translate(DVec3),
    /// A rotation about the line through `center` along `axis` (world), the context's frame
    /// being `frame` (to world).
    Rotate { center: DVec3, axis: DVec3, angle: f64, frame: DAffine3 },
}

impl Motion {
    /// `fraction` of the motion, in the context's coordinates.
    pub fn part(&self, fraction: f64) -> DAffine3 {
        match *self {
            Motion::Translate(v) => DAffine3::from_translation(v * fraction),
            Motion::Rotate { center, axis, angle, frame } => {
                frame.inverse() * rotate::rotation_about(center, axis, angle * fraction) * frame
            }
        }
    }
}

/// A copy just made: typing `5x` (or `*5`) right after makes it an array of 5 at the same
/// step, `/5` 5 copies spread over the same way; retyping corrects (SketchUp).
#[derive(Clone, Debug)]
pub struct LastCopy {
    pub context: Id,
    pub entities: Vec<Id>,
    pub motion: Motion,
    /// The operation applied last for it (the one a new count replaces).
    pub op: Operation,
}

impl LastCopy {
    /// The operation for the copy itself.
    pub fn new(context: Id, entities: Vec<Id>, motion: Motion) -> Self {
        let op = Operation::Duplicate { context, entities: entities.clone(), transform: motion.part(1.0) };
        LastCopy { context, entities, motion, op }
    }

    /// A typed count: the commands replacing the copy (or the last array) by the new array,
    /// `None` if `text` is not a count.
    pub fn retype(&mut self, text: &str) -> Option<Vec<Command>> {
        let (fraction, count) = match crate::vcb::repeat(text)? {
            crate::vcb::Repeat::Times(n) => (1.0, n),
            crate::vcb::Repeat::Divide(n) => (1.0 / n as f64, n),
        };
        let op = Operation::Array { context: self.context, entities: self.entities.clone(), step: self.motion.part(fraction), count };
        let previous = std::mem::replace(&mut self.op, op.clone());
        Some(vec![Command::Replace { previous, op }])
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
    }
}

/// The copy modifier's name for hints (SketchUp: Option on macOS, Ctrl elsewhere).
pub const COPY_KEY: &str = if cfg!(target_os = "macos") { "Option" } else { "Ctrl" };

/// Neutral colour of construction lines.
pub const INK: [f32; 4] = [0.05, 0.05, 0.08, 1.0];
/// Axis colours (SketchUp's red, green, blue).
pub const RED: [f32; 4] = [0.85, 0.05, 0.05, 1.0];
pub const GREEN: [f32; 4] = [0.05, 0.6, 0.05, 1.0];
pub const BLUE: [f32; 4] = [0.05, 0.1, 0.85, 1.0];
/// Parallel and perpendicular guides (SketchUp's magenta).
pub const MAGENTA: [f32; 4] = [0.8, 0.05, 0.75, 1.0];
/// The highlight colour of picked things.
pub const PICK: [f32; 4] = [0.85, 0.35, 0.0, 1.0];

/// A small cross marking a point, sized for the screen.
pub fn cross(cx: &Cx<'_>, p: DVec3, color: [f32; 4]) -> Vec<PreviewLine> {
    let s = cx.camera.world_per_pixel(cx.size, p) * 6.0;
    [DVec3::X, DVec3::Y, DVec3::Z]
        .into_iter()
        .map(|d| PreviewLine { a: p - d * s, b: p + d * s, color })
        .collect()
}

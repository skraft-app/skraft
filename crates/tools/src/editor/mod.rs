//! The editor: the document's history, its display scene, the camera, the context being edited,
//! the selection and the active tool. The app forwards input to it and draws what [`View`]
//! says; everything here runs without a UI or a GPU.
//!
//! The kernel runs behind a [`KernelService`] (docs/ARCHITECTURE.md §7 ter): an operation that
//! needs it is *pending* until [`Editor::poll`] gets its result. Meanwhile only navigation and
//! hovering work; Esc abandons it. Meshes the scene lacks are asked for the same way.
//!
//! Here: the state, the tools and their input. The rest, by concern: [`document`] (operations
//! and the kernel, undo, opening), [`selection`] (selection and context), [`navigation`],
//! [`view`] (what to draw).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::kernel::{JobId, KernelService, Solid};
use solidsketch_model::{Document, EntityPath, History, Id, Operation, Outcome, Scope};

use crate::camera::{Camera, ViewArea};
use crate::infer::{EdgeRef, InferKind, Lock};
use crate::scene::DisplayScene;
use crate::vcb;
use crate::tools::{self, Command, Cx, Key, OptionValue, OptionValues, Preview, Tool, ToolInput, ToolKind, ToolOption};

mod document;
mod navigation;
mod selection;
mod view;

pub use view::{Look, Mark, View, ViewHighlight, ViewPlacement};
use document::{After, Pending, Probe};

pub struct Editor {
    history: History,
    kernel: Box<dyn KernelService>,
    next_job: JobId,
    pending: Option<Pending>,
    /// Operations of one action (explode several assemblies), applied one after the other.
    queue: VecDeque<(Operation, After)>,
    collected: Vec<Id>,
    /// Meshes asked for, by job.
    meshing: HashMap<JobId, Arc<Solid>>,
    /// The jobs of [`Editor::crash_kernel`] still out, and what came back of them.
    probes: HashMap<JobId, Probe>,
    probe_report: Vec<String>,
    /// How the last operation started or polled ended (for [`Editor::apply`]).
    last: Option<Result<Outcome, String>>,
    scene: DisplayScene,
    pub camera: Camera,
    size: DVec2,
    /// Occurrence path of the context being edited.
    context: Vec<Id>,
    /// The part open in the context, by its occurrence there (ARCHITECTURE.md §2.9).
    open_part: Option<Id>,
    /// The "Machinings" switch: an open part shows its blank as a ghost and its tools.
    machinings_shown: bool,
    /// What a change to the open part reaches (§2.5): every occurrence, until said otherwise
    /// for this opening.
    part_scope: Scope,
    /// The machining hovered in the entity info's list (drawn hovered in the view).
    listed_hover: Option<Id>,
    selection: Vec<Id>,
    tool: Box<dyn Tool>,
    pivot: Option<DVec3>,
    message: Option<String>,
    /// The part of the viewport no panel covers, if not all of it (pixels).
    free: Option<(DVec2, DVec2)>,
    /// Inference locks (§6 ter): the arrow keys' axis, and the line Shift holds.
    axis_lock: Option<usize>,
    shift: bool,
    shift_lock: Option<Lock>,
    /// Points tracked (§6 ter, step 3): hovered [`TRACK_AFTER`] seconds; the latest
    /// [`MAX_TRACKED`].
    tracked: Vec<DVec3>,
    /// The point under the cursor and since when (seconds).
    dwell: Option<(DVec3, f64)>,
    /// The time the app last gave (seconds).
    now: f64,
    /// The last edge hovered (parallel / perpendicular / tangent inferences).
    reference: Option<EdgeRef>,
    /// Where the cursor last was: the tool follows it again when a lock changes.
    last_hover: Option<DVec2>,
    /// The tools' saved settings (the option bar's, ARCHITECTURE.md §6 bis), by tool id.
    tool_settings: BTreeMap<String, OptionValues>,
    /// The material the paint tool paints with (§7 quater).
    paint: Option<Id>,
}

/// The settings of a tool that has none saved.
static NO_SETTINGS: OptionValues = BTreeMap::new();

/// How long a point is hovered to be tracked (seconds).
pub const TRACK_AFTER: f64 = 0.5;
/// How many points are tracked at once.
pub const MAX_TRACKED: usize = 2;

impl Editor {
    /// An editor of `doc`, its kernel run by `kernel`.
    pub fn new(doc: Document, kernel: Box<dyn KernelService>) -> Self {
        let mut e = Editor {
            history: History::new(doc),
            kernel,
            next_job: 1,
            pending: None,
            queue: VecDeque::new(),
            collected: Vec::new(),
            meshing: HashMap::new(),
            probes: HashMap::new(),
            probe_report: Vec::new(),
            last: None,
            scene: DisplayScene::new(),
            camera: Camera::default(),
            size: DVec2::new(800.0, 600.0),
            context: Vec::new(),
            open_part: None,
            machinings_shown: false,
            part_scope: Scope::All,
            listed_hover: None,
            selection: Vec::new(),
            tool: tools::make(ToolKind::Select),
            pivot: None,
            message: None,
            free: None,
            axis_lock: None,
            shift: false,
            shift_lock: None,
            tracked: Vec::new(),
            dwell: None,
            now: 0.0,
            reference: None,
            last_hover: None,
            tool_settings: BTreeMap::new(),
            paint: None,
        };
        e.after_change();
        e.zoom_extents();
        e
    }

    // ---- state ---------------------------------------------------------------------------------

    pub fn document(&self) -> &Document {
        self.history.document()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn scene(&self) -> &DisplayScene {
        &self.scene
    }

    /// Occurrence path of the context being edited (empty: the model).
    pub fn context(&self) -> &[Id] {
        &self.context
    }

    /// The definition being edited.
    pub fn context_definition(&self) -> Id {
        self.document().context_of(&self.context).unwrap_or(self.document().root())
    }

    /// The frame of the context being edited, to world (identity for the model).
    pub fn context_transform(&self) -> DAffine3 {
        self.document().transform_of(&self.context).unwrap_or(DAffine3::IDENTITY)
    }

    /// The part open in the context, by its occurrence there (ARCHITECTURE.md §2.9).
    pub fn open_part(&self) -> Option<Id> {
        self.open_part
    }

    /// The path of the open part's occurrence.
    pub fn open_part_path(&self) -> Option<EntityPath> {
        self.open_part.map(|id| EntityPath::new(self.context.clone(), id))
    }

    /// The "Machinings" switch (a preference: it holds for every part opened).
    pub fn machinings_switch(&self) -> bool {
        self.machinings_shown
    }

    pub fn set_machinings_switch(&mut self, on: bool) {
        if on != self.machinings_shown {
            self.machinings_shown = on;
            // The selection was machinings, or would become them.
            if self.open_part.is_some() {
                self.tool.reset();
                self.selection.clear();
            }
            self.sync_scene();
        }
    }

    /// How many occurrences of the open part the scene shows (through copies of assemblies
    /// too): what a change reaches under the scope "all".
    pub fn open_part_copies(&self) -> usize {
        self.open_part_path().map_or(0, |p| self.cx().copies_shown(&p))
    }

    /// The machining of the open part hovered: in the entity info's list, else in the view
    /// (its tool under the cursor).
    pub fn hovered_machining(&self) -> Option<Id> {
        if !self.showing_machinings() {
            return None;
        }
        self.listed_hover.or_else(|| self.preview().hover)
    }

    /// The machining hovered in the entity info's list, if any.
    pub fn set_listed_hover(&mut self, machining: Option<Id>) {
        self.listed_hover = machining;
    }

    /// What a change to the open part reaches.
    pub fn part_scope(&self) -> Scope {
        self.part_scope
    }

    pub fn set_part_scope(&mut self, scope: Scope) {
        self.part_scope = scope;
    }

    /// Are an open part's machinings shown now (its blank a ghost)?
    pub fn showing_machinings(&self) -> bool {
        self.machinings_shown && self.open_part.is_some()
    }



    /// Brings the display scene up to date (the document, the recipe shown) and asks for the
    /// meshes it lacks.
    pub(super) fn sync_scene(&mut self) {
        self.scene.set_recipe(self.open_part_path(), self.machinings_shown);
        self.scene.sync(self.history.document());
        self.request_meshes();
    }

    /// The open part's axes in the context's (its occurrence's T); identity without one.
    fn axes_in_context(&self) -> DAffine3 {
        let doc = self.document();
        self.open_part
            .and_then(|id| doc.assembly(self.context_definition()).ok()?.occurrence(id).map(|o| o.transform))
            .unwrap_or(DAffine3::IDENTITY)
    }

    /// The frame tools draw and infer in, to world: the open part's axes, else the context's
    /// (§2.3). The axes drawn are these.
    pub fn axes_transform(&self) -> DAffine3 {
        self.context_transform() * self.axes_in_context()
    }

    /// Selected entities, in the active context.
    pub fn selection(&self) -> &[Id] {
        &self.selection
    }

    /// Does the active tool copy (the cursor shows a « + »)?
    pub fn tool_copying(&self) -> bool {
        self.tool.copying()
    }

    pub fn tool_kind(&self) -> ToolKind {
        self.tool.kind()
    }

    /// The status bar's text: the kernel at work, else the last message, else the tool's hint.
    pub fn status(&self) -> String {
        if let Some(p) = &self.pending {
            return format!("{} {}", solidsketch_i18n::ellipsis(t!("status.computing", op = p.op.label())), t!("status.computing_cancel"));
        }
        self.message.clone().unwrap_or_else(|| self.tool.status())
    }

    /// Is an operation waiting for the kernel (or queued)? Then only navigation works.
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.queue.is_empty()
    }

    /// The measurements box's label and the value it shows while nothing is typed, when the
    /// tool takes one.
    pub fn vcb(&self) -> Option<(String, String)> {
        self.tool.vcb().map(|v| {
            let shown = vcb::display(&v, &self.document().settings().units);
            (v.label, shown)
        })
    }

    pub fn set_viewport_size(&mut self, size: DVec2) {
        self.size = size.max(DVec2::ONE);
    }

    pub fn viewport_size(&self) -> DVec2 {
        self.size
    }

    /// The part of the viewport floating panels leave visible (pixels, origin top left): what
    /// zoom extents frames.
    pub fn set_free_area(&mut self, min: DVec2, max: DVec2) {
        self.free = Some((min, max));
    }

    fn view_area(&self) -> ViewArea {
        let full = ViewArea::full(self.size);
        match self.free {
            Some((lo, hi)) => {
                let (lo, hi) = (lo.clamp(DVec2::ZERO, self.size), hi.clamp(DVec2::ZERO, self.size));
                // Panels covering nearly everything: frame the whole viewport instead.
                if (hi - lo).min_element() < 100.0 { full } else { ViewArea { free_min: lo, free_max: hi, ..full } }
            }
            None => full,
        }
    }

    fn cx(&self) -> Cx<'_> {
        self.cx_for(self.tool.kind())
    }

    /// What the tool `kind` sees (its settings among it): the active tool's, also while it is
    /// taken out for a call.
    fn cx_for(&self, kind: ToolKind) -> Cx<'_> {
        let doc = self.history.document();
        Cx {
            doc,
            scene: &self.scene,
            camera: &self.camera,
            size: self.size,
            context: &self.context,
            context_def: doc.context_of(&self.context).unwrap_or(doc.root()),
            context_transform: doc.transform_of(&self.context).unwrap_or(DAffine3::IDENTITY),
            open_part: self.open_part,
            axes_in_context: self.axes_in_context(),
            recipe: self.scene.recipe().filter(|_| self.showing_machinings()),
            part_scope: self.part_scope,
            selection: &self.selection,
            axis_lock: self.axis_lock,
            shift_lock: self.shift_lock,
            tracked: &self.tracked,
            reference: self.reference,
            options: self.tool_settings.get(kind.id()).unwrap_or(&NO_SETTINGS),
            material: self.paint_material(),
        }
    }

    /// The material the paint tool paints with: none, or one of the document's.
    pub fn paint_material(&self) -> Option<Id> {
        self.paint.filter(|m| self.document().material(*m).is_ok())
    }

    pub fn set_paint_material(&mut self, material: Option<Id>) {
        self.paint = material;
    }

    // ---- the tool's options (the option bar) -------------------------------------------------

    /// The active tool's options, as the bar shows them now.
    pub fn tool_options(&self) -> Vec<ToolOption> {
        self.tool.options(&self.cx())
    }

    /// Sets an option of the active tool (a click in the bar): a setting is kept for the tool,
    /// a gesture's state goes to the tool.
    pub fn set_tool_option(&mut self, key: &str, value: OptionValue) {
        let Some(option) = self.tool_options().into_iter().find(|o| o.key == key) else { return };
        if option.saved {
            self.tool_settings.entry(self.tool.kind().id().to_owned()).or_default().insert(key.to_owned(), value.clone());
        }
        self.tool.set_option(key, &value);
        self.rehover();
    }

    /// Turns the active tool's first choice over to its next value (Tab).
    fn next_choice(&mut self) {
        let Some(option) = self.tool_options().into_iter().find(|o| !o.choices.is_empty()) else { return };
        let OptionValue::Choice(current) = &option.value else { return };
        let at = option.choices.iter().position(|c| c.value == current).map_or(0, |i| (i + 1) % option.choices.len());
        self.set_tool_option(option.key, OptionValue::Choice(option.choices[at].value.to_owned()));
    }

    /// Every tool's saved settings (for the user's preferences).
    pub fn tool_settings(&self) -> &BTreeMap<String, OptionValues> {
        &self.tool_settings
    }

    /// Takes the tools' settings back (from the user's preferences).
    pub fn set_tool_settings(&mut self, settings: BTreeMap<String, OptionValues>) {
        self.tool_settings = settings;
    }

    /// The axis the arrow keys lock (0 red, 1 green, 2 blue).
    pub fn axis_lock(&self) -> Option<usize> {
        self.axis_lock
    }

    /// Shift pressed or released (the app tells every frame): pressed on a linear inference,
    /// it holds that line until released.
    pub fn set_shift(&mut self, down: bool) {
        if down == self.shift {
            return;
        }
        self.shift = down;
        self.shift_lock = if down {
            self.preview().inference.and_then(|i| {
                let (origin, dir) = i.line?;
                Some(Lock { origin: Some(origin), dir, line: i.kind.line()? })
            })
        } else {
            None
        };
        self.rehover();
    }

    /// Locks and tracked points go with each operation, Esc, a tool change, undo and redo.
    fn clear_locks(&mut self) {
        self.axis_lock = None;
        self.shift_lock = None;
        self.tracked.clear();
        self.dwell = None;
    }

    /// The points tracked (world): marked on screen.
    pub fn tracked(&self) -> &[DVec3] {
        &self.tracked
    }

    /// The time now (seconds, the app's clock): a point hovered long enough becomes tracked.
    /// Returns whether a point is being hovered towards that (the app should call again soon,
    /// even without input).
    pub fn tick(&mut self, now: f64) -> bool {
        self.now = now;
        let Some((p, since)) = self.dwell else { return false };
        if self.tracked.contains(&p) {
            return false;
        }
        if now - since < TRACK_AFTER {
            return true;
        }
        self.tracked.push(p);
        if self.tracked.len() > MAX_TRACKED {
            self.tracked.remove(0);
        }
        self.dwell = None;
        self.rehover();
        false
    }

    /// Feeds the tool the last cursor position again (a lock changed).
    fn rehover(&mut self) {
        if let Some(px) = self.last_hover {
            self.input(ToolInput::Hover { px });
        }
    }

    // ---- tools ---------------------------------------------------------------------------------

    pub fn set_tool(&mut self, kind: ToolKind) {
        self.tool = tools::make(kind);
        self.message = None;
        self.clear_locks();
        self.reference = None;
    }

    /// Feeds the active tool. Esc cancels the tool's step, else clears the selection; Delete
    /// erases the selection.
    pub fn input(&mut self, input: ToolInput) {
        if self.busy() {
            match input {
                ToolInput::Key { key: Key::Escape, .. } => self.abandon(),
                ToolInput::Hover { px } => self.last_hover = Some(px),
                _ => {}
            }
            return;
        }
        if !matches!(input, ToolInput::Hover { .. }) {
            self.message = None;
        }
        // A typed value is read for what the tool takes now (vcb.rs).
        let input = match input {
            ToolInput::Vcb(text) => {
                let Some(spec) = self.tool.vcb() else { return };
                match vcb::read(&spec, &text, &self.document().settings().units, &self.axes_transform()) {
                    Ok(v) => ToolInput::Value(v),
                    Err(message) => {
                        self.message = Some(message);
                        return;
                    }
                }
            }
            input => input,
        };
        match &input {
            ToolInput::Hover { px } => self.last_hover = Some(*px),
            // The arrow keys lock an axis for every tool: → red, ← green, ↑ blue, ↓ free.
            ToolInput::Key { key: key @ (Key::Right | Key::Left | Key::Up | Key::Down), .. } => {
                let k = match key {
                    Key::Right => Some(0),
                    Key::Left => Some(1),
                    Key::Up => Some(2),
                    _ => None,
                };
                self.axis_lock = if k == self.axis_lock { None } else { k };
                self.rehover();
                return;
            }
            ToolInput::Key { key: Key::Escape, .. } => {
                self.clear_locks();
                if self.tool.busy() {
                    self.tool.reset();
                } else if self.tool.kind() != ToolKind::Select {
                    self.set_tool(ToolKind::Select);
                } else if self.selection.is_empty() && self.open_part.is_some() {
                    self.exit();
                } else {
                    self.selection.clear();
                }
                return;
            }
            ToolInput::Key { key: Key::Delete, .. } if !self.tool.busy() => {
                self.delete_selection();
                return;
            }
            ToolInput::Key { key: Key::Tab, .. } => {
                self.next_choice();
                return;
            }
            _ => {}
        }
        // The tool reads the editor while it runs: take it out for the call.
        let mut tool = std::mem::replace(&mut self.tool, tools::make(ToolKind::Select));
        let commands = tool.input(&input, &self.cx_for(tool.kind()));
        self.tool = tool;
        if matches!(input, ToolInput::Hover { .. }) {
            let inference = self.preview().inference;
            if let Some(edge) = inference.as_ref().and_then(|i| i.edge) {
                self.reference = Some(edge);
            }
            // A point of the geometry under the cursor starts (or goes on) dwelling.
            let point = inference.filter(|i| i.kind.is_point()).map(|i| i.point);
            self.dwell = match (point, self.dwell) {
                (Some(p), Some((q, since))) if p == q => Some((q, since)),
                (Some(p), _) => Some((p, self.now)),
                (None, _) => None,
            };
        }
        for c in commands {
            self.run(c);
        }
    }

    pub(crate) fn run(&mut self, c: Command) {
        match c {
            Command::Apply(op) => {
                self.clear_locks();
                let after = if matches!(op, Operation::Boolean { .. }) { After::Nothing } else { After::Select };
                let _ = self.start(op, after, false);
            }
            Command::Select(ids) => self.selection = ids,
            Command::Enter(id) => self.enter(id),
            Command::Exit => self.exit(),
            Command::ClickOff(px) => match self.context_bounds() {
                Some(b) if !b.hit_by(&self.camera.ray(px, self.size)) => self.exit(),
                _ => self.selection.clear(),
            },
            Command::Say(s) => self.message = Some(s),
            Command::Replace { previous, op } => {
                if self.history.undo_op() == Some(&previous) && self.history.undo() {
                    self.after_change();
                    self.clear_locks();
                    let _ = self.start(op, After::Select, true);
                } else {
                    self.message = Some(t!("status.copy_not_last"));
                }
            }
        }
    }

    pub fn preview(&self) -> Preview {
        self.tool.preview(&self.cx())
    }

    /// The point the active tool infers under the cursor, with its kind (marker and tooltip).
    pub fn inference(&self) -> Option<(DVec3, InferKind)> {
        self.preview().inference.map(|i| (i.point, i.kind))
    }

    /// The selection rectangle being dragged, if any (the app draws it).
    pub fn marquee(&self) -> Option<tools::Marquee> {
        self.preview().marquee
    }
}

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
use solidsketch_model::kernel::{JobId, KernelService, Quality, Solid};
use solidsketch_model::{Document, EntityPath, History, Id, Operation, Outcome, Scope, measure};

use crate::camera::{Camera, ViewArea};
use crate::infer::{EdgeRef, InferKind, Lock};
use crate::scene::DisplayScene;
use crate::tools::{self, Command, Cx, Key, OptionValue, OptionValues, Preview, Tool, ToolInput, ToolKind, ToolOption};
use crate::vcb;

mod document;
mod loading;
mod navigation;
mod selection;
mod view;

use document::{After, Pending, Probe};
pub use loading::{LoadEnd, LoadKind, Loading};
pub use view::{Look, Mark, OrientedBox, View, ViewHighlight, ViewPlacement};

pub struct Editor {
    history: History,
    kernel: Box<dyn KernelService>,
    next_job: JobId,
    pending: Option<Pending>,
    /// Operations of one action (explode several assemblies), applied one after the other.
    queue: VecDeque<(Operation, After)>,
    collected: Vec<Id>,
    /// Meshes asked for, by job: the solid, and how fine.
    meshing: HashMap<JobId, (Arc<Solid>, Quality)>,
    /// The load under way (§7 sexies): a file opened or imported, until its last mesh.
    load: Option<loading::Load>,
    /// How the last load ended, until the app takes it.
    load_end: Option<LoadEnd>,
    /// Files being read by the kernel (STEP), by job: their names (ARCHITECTURE.md §8 ter).
    importing: HashMap<JobId, String>,
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
    /// The occurrence hovered in the outliner (drawn hovered in the view).
    outliner_hover: Option<EntityPath>,
    /// The stretch the scene's preview mesh was made for, and the revisions handed out.
    stretch_meshed: Option<tools::StretchPreview>,
    preview_revision: u64,
    /// A stretch applied, drawn as previewed until the kernel's job gives the part its new
    /// shape (else the part would show as it was for a frame or more).
    held_stretch: Option<(JobId, tools::StretchPreview)>,
    /// The occurrences a stretch previewed draws otherwise, by path: the mesh, the transform.
    stretch_swaps: HashMap<EntityPath, (Id, DAffine3)>,
    /// How the selection is drawn, worked out once for the frames that follow (§7 bis).
    selection_drawing: std::cell::RefCell<Option<view::SelectionDrawing>>,
    /// Is anything hidden, at the history's revision ([`Editor::has_hidden`]: asked at every
    /// frame, found by a pass over every assembly).
    has_hidden: std::cell::Cell<Option<(u64, bool)>>,
    selection: Vec<Id>,
    tool: Box<dyn Tool>,
    pivot: Option<DVec3>,
    message: Option<String>,
    /// The part of the viewport no panel covers, if not all of it (pixels).
    free: Option<(DVec2, DVec2)>,
    /// Inference locks (§6 ter): the arrow keys' axis, and the line Shift holds.
    axis_lock: Option<usize>,
    /// The normal ↓ locks for the shape tools (world): the face's under the cursor.
    face_lock: Option<DVec3>,
    shift: bool,
    /// The copy modifier held (Option on macOS, Ctrl elsewhere).
    copy: bool,
    /// The other modifier held (Command on macOS, Alt elsewhere).
    alt: bool,
    shift_lock: Option<Lock>,
    /// The height (whole) of the last shape drawn: the next can take it again.
    last_height: Option<f64>,
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
            load: Some(loading::Load::new(LoadKind::Open)),
            load_end: None,
            importing: HashMap::new(),
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
            outliner_hover: None,
            stretch_meshed: None,
            preview_revision: 0,
            held_stretch: None,
            stretch_swaps: HashMap::new(),
            selection_drawing: std::cell::RefCell::new(None),
            has_hidden: std::cell::Cell::new(None),
            selection: Vec::new(),
            tool: tools::make(ToolKind::Select),
            pivot: None,
            message: None,
            free: None,
            axis_lock: None,
            face_lock: None,
            shift: false,
            copy: false,
            alt: false,
            shift_lock: None,
            last_height: None,
            tracked: Vec::new(),
            dwell: None,
            now: 0.0,
            reference: None,
            last_hover: None,
            tool_settings: BTreeMap::new(),
            paint: None,
        };
        // Opened: its meshes asked as a load (none for an empty document: ended at once).
        e.after_change();
        e.settle_load();
        e.load_end = None;
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

    /// Has the open part a profile to edit (§6 nonies, step 3)?
    pub fn open_part_profile(&self) -> bool {
        self.open_part_path().and_then(|p| self.document().resolve_part(&p).ok()).is_some_and(|(_, part, _)| part.profile.is_some())
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

    /// The occurrence hovered in the outliner, if any: drawn as if hovered in the view. Did it
    /// change (the view to draw again)?
    pub fn set_outliner_hover(&mut self, path: Option<EntityPath>) -> bool {
        let changed = self.outliner_hover != path;
        self.outliner_hover = path;
        changed
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
        let active = self.context.iter().copied().chain(self.open_part).collect();
        self.scene.set_active(active, self.history.document().settings().display.hide_rest_of_model);
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

    /// Does the measurements box take a typed value now: what the tool takes at this step, or
    /// a length option's `<length>x`?
    pub fn takes_typing(&self) -> bool {
        self.tool.vcb().is_some() || self.tool_options().iter().any(|o| matches!(o.value, OptionValue::Length(_)))
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

    /// The area set by [`Self::set_free_area`], if any.
    pub fn free_area(&self) -> Option<(DVec2, DVec2)> {
        self.free
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

    pub(crate) fn cx(&self) -> Cx<'_> {
        self.cx_for(self.tool.kind())
    }

    /// What the tool `kind` sees (its settings among it): the active tool's, also while it is
    /// taken out for a call.
    fn cx_for(&self, kind: ToolKind) -> Cx<'_> {
        let doc = self.history.document();
        Cx {
            now: self.now,
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
            face_lock: self.face_lock,
            shift_down: self.shift,
            copy_down: self.copy,
            alt_down: self.alt,
            last_height: self.last_height,
            shift_lock: self.shift_lock,
            tracked: &self.tracked,
            reference: self.reference,
            options: self.tool_settings.get(kind.id()).unwrap_or(&NO_SETTINGS),
            material: self.paint_material(),
            previewed: &self.stretch_swaps,
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

    /// Reads `text` for the active tool's length option `key` (typed in its field of the bar,
    /// or `<length>x` in the measurements box), `@` its current value: the option is set, or
    /// the status bar says why not.
    pub fn type_tool_length(&mut self, key: &str, text: &str) {
        let Some(option) = self.tool_options().into_iter().find(|o| o.key == key) else { return };
        let OptionValue::Length(current) = option.value else { return };
        match measure::coordinate(text, &self.document().settings().units, current) {
            Ok(v) => self.set_tool_option(option.key, OptionValue::Length(v)),
            Err(e) => self.message = Some(t!("tool.vcb.refused", label = option.label, text = text.trim(), error = e)),
        }
    }

    /// The active tool's first length option, if `text` is meant for it: a text ending in `x`
    /// (OpenCutList's offset), unless the measurements box takes a count of copies now.
    fn length_typed<'t>(&self, text: &'t str) -> Option<(&'static str, &'t str)> {
        if matches!(self.tool.vcb(), Some(vcb::Vcb { kind: vcb::VcbKind::Count, .. })) {
            return None;
        }
        // A suffix the tool gives one of its length options (the prism tool's `5r`, its fillet).
        let text = text.trim();
        if let Some(last) = text.chars().last()
            && let Some(key) = self.tool.length_suffix(last.to_ascii_lowercase())
        {
            return Some((key, &text[..text.len() - last.len_utf8()]));
        }
        let len = text.strip_suffix(['x', 'X'])?;
        let option = self.tool_options().into_iter().find(|o| matches!(o.value, OptionValue::Length(_)))?;
        Some((option.key, len))
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

    /// The normal ↓ locks for the shape tools (world).
    pub fn face_lock(&self) -> Option<DVec3> {
        self.face_lock
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

    /// The copy modifier pressed or released (the app tells every frame): the tool follows the
    /// cursor again with it.
    pub fn set_copy(&mut self, down: bool) {
        if down != self.copy {
            self.copy = down;
            self.rehover();
        }
    }

    /// The other modifier pressed or released (the app tells every frame): the tool follows
    /// the cursor again with it.
    pub fn set_alt(&mut self, down: bool) {
        if down != self.alt {
            self.alt = down;
            self.rehover();
        }
    }

    /// Locks and tracked points go with each operation, Esc, a tool change, undo and redo.
    fn clear_locks(&mut self) {
        self.axis_lock = None;
        self.face_lock = None;
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
        // A tool waiting on the time (the prism tool's rest on its last corner) follows the cursor
        // again, which hasn't moved.
        if self.tool.waiting() {
            self.rehover();
            if self.tool.waiting() {
                return true;
            }
        }
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

    /// The cursor left the view (off it, or over a panel): the tool hides what follows it,
    /// and nothing follows it again until it comes back.
    pub fn leave(&mut self) {
        if self.last_hover.take().is_some() {
            self.dwell = None;
            self.tool.leave();
        }
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

    /// Feeds the active tool. Esc goes back one step of the tool; at its first step, it does
    /// nothing more, but with Select, where it clears the selection (or closes the open part).
    /// Delete erases the selection.
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
                if let Some((key, len)) = self.length_typed(&text) {
                    let len = len.to_owned();
                    self.type_tool_length(key, &len);
                    return;
                }
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
            // The arrow keys lock an axis for every tool: → red, ← green, ↑ blue, ↓ free; for
            // the shape tools, ↓ locks the normal of the face under the cursor (the same face
            // again, or none: free).
            ToolInput::Key { key: Key::Down, .. } if self.tool.down_locks_face() => {
                let face = self.last_hover.and_then(|px| self.cx().pick(px)).map(|h| h.normal);
                self.face_lock = match (face, self.face_lock) {
                    (Some(n), Some(l)) if n.dot(l) > 1.0 - 1e-9 => None,
                    (face, _) => face,
                };
                self.axis_lock = None;
                self.rehover();
                return;
            }
            ToolInput::Key { key: key @ (Key::Right | Key::Left | Key::Up | Key::Down), .. } => {
                let k = match key {
                    Key::Right => Some(0),
                    Key::Left => Some(1),
                    Key::Up => Some(2),
                    _ => None,
                };
                self.axis_lock = if k == self.axis_lock { None } else { k };
                self.face_lock = None;
                self.rehover();
                return;
            }
            ToolInput::Key { key: Key::Escape, .. } => {
                self.clear_locks();
                // A tool at its first step stays active (only Space goes back to Select): Esc
                // only drops the locks.
                if self.tool.busy() {
                    self.tool.back();
                    self.rehover();
                } else if self.tool.kind() == ToolKind::Select {
                    if self.selection.is_empty() && self.open_part.is_some() {
                        self.exit();
                    } else {
                        self.selection.clear();
                    }
                }
                return;
            }
            ToolInput::Key { key: Key::Delete, .. } if !self.tool.busy() && !self.tool.takes_delete() => {
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
        self.update_preview_mesh();
    }

    /// What the tool previews stretched, in the scene (§6 sexies): the document as the stretch
    /// would leave it, sketched without its geometry; each part it stretches meshed from its
    /// display mesh, each vertex moved by its section; and, per occurrence it changes, what to
    /// draw instead (`stretch_swaps`). Dropped when the tool shows none.
    pub(super) fn update_preview_mesh(&mut self) {
        let wanted = self.preview().stretch;
        if wanted == self.stretch_meshed {
            return;
        }
        let mut meshes = Vec::new();
        let mut swaps = HashMap::new();
        if let Some(s) = &wanted {
            let doc = self.document().clone();
            let scope = if s.all { Scope::All } else { Scope::This };
            if let Ok((sketch, stretched)) = doc.stretch_sketch(&s.context, &s.occurrences, &s.frame, s.axis, &s.cuts, &s.moves, scope) {
                let mut keys = HashMap::new();
                for sp in &stretched {
                    let (Ok(from), Some(dm)) = (doc.part(sp.from), self.scene.definition_mesh(sp.from)) else { continue };
                    let key = crate::scene::preview_key(meshes.len());
                    let spans = sp.stretch.section_spans(from.shape());
                    let parts = dm
                        .parts
                        .iter()
                        .map(|p| crate::scene::PartMesh {
                            solid: key,
                            mesh: Arc::new(crate::scene::stretched_mesh(&p.mesh, &sp.stretch, &spans)),
                            snaps: Arc::default(),
                            bounds: None,
                        })
                        .collect();
                    self.preview_revision += 1;
                    let faces = dm.faces.clone();
                    meshes.push(crate::scene::DefinitionMesh { definition: key, revision: self.preview_revision, parts, faces });
                    keys.insert(sp.part, key);
                }
                // Each occurrence drawn otherwise: stretched, moved, or both — only those the
                // sketch changed are visited (a large model mostly shared, untouched).
                // The parts stretched are not in the sketch (meshed here): named as changed.
                let touched: Vec<Id> = stretched.iter().map(|sp| sp.part).collect();
                for (placed, part, t) in sketch.placed_changes(&doc, &touched) {
                    let key = keys.get(&placed.part).copied().unwrap_or(placed.part);
                    if key != part || !t.abs_diff_eq(placed.transform, 1e-9) {
                        swaps.insert(placed.path, (key, placed.transform));
                    }
                }
            }
        }
        self.scene.set_preview(meshes);
        self.stretch_swaps = swaps;
        self.stretch_meshed = wanted;
    }

    pub(crate) fn run(&mut self, c: Command) {
        match c {
            Command::Apply(op) => {
                // A tool's setting kept on a component (the stretch's cutters) is no new gesture.
                if !matches!(op, Operation::SetBehavior { .. }) {
                    self.clear_locks();
                }
                let held = matches!(op, Operation::Stretch { .. }).then(|| self.stretch_meshed.clone()).flatten();
                // What a boolean or a drawing tool makes is not selected: the next shape is
                // drawn straight away.
                let after = match op {
                    Operation::Boolean { .. } | Operation::AddPrimitive { .. } | Operation::AddMachining { .. } | Operation::SetProfile { .. } => After::Nothing,
                    _ => After::Select,
                };
                // Hidden (the eraser with Shift), they leave the selection, unless the hidden
                // are shown.
                let hidden = match &op {
                    Operation::SetHidden { entities, hidden: true, .. } if !self.document().settings().display.show_hidden => {
                        entities.clone()
                    }
                    _ => Vec::new(),
                };
                if self.start(op, after, false).is_ok() {
                    self.selection.retain(|id| !hidden.contains(id));
                }
                self.held_stretch = held.zip(self.pending.as_ref()).map(|(s, p)| (p.job, s));
            }
            Command::Select(ids) => self.selection = ids,
            Command::SetOption { key, value } => self.set_tool_option(key, value),
            Command::RememberHeight(h) => self.last_height = Some(h),
            Command::Enter(id) => self.enter(id),
            Command::Exit => self.exit(),
            Command::ClickOff(px) => match self.context_bounds() {
                Some(b) if !b.hit_by(&self.camera.ray(px, self.size)) => self.exit(),
                _ => self.selection.clear(),
            },
            Command::Say(s) => self.message = Some(s),
            Command::ClearLocks => {
                self.axis_lock = None;
                self.face_lock = None;
                self.shift_lock = None;
                // The tool's next step follows the cursor at once, not at its next move.
                self.rehover();
            }
            Command::Replace { previous, op } => {
                if self.history.undo_op() == Some(&*previous) && self.history.undo() {
                    self.after_change();
                    self.clear_locks();
                    let _ = self.start(*op, After::Select, true);
                } else {
                    self.message = Some(t!("status.copy_not_last"));
                }
            }
        }
    }

    /// What the tool shows; while a stretch it applied is computed, that stretch alone.
    pub fn preview(&self) -> Preview {
        if let Some((job, s)) = &self.held_stretch
            && self.pending.as_ref().is_some_and(|p| p.job == *job)
        {
            return Preview { stretch: Some(s.clone()), ..Preview::default() };
        }
        self.tool.preview(&self.cx())
    }

    /// The point the active tool infers under the cursor, with its kind (marker and tooltip).
    pub fn inference(&self) -> Option<(DVec3, InferKind)> {
        self.preview().inference.map(|i| (i.point, i.kind))
    }

    /// More lines the active tool adds to the inference's tooltip, if any.
    pub fn inference_tip(&self) -> Option<String> {
        self.preview().tip
    }

    /// The selection rectangle being dragged, if any (the app draws it).
    pub fn marquee(&self) -> Option<tools::Marquee> {
        self.preview().marquee
    }

    /// The lengths the tool writes over the scene (the app draws them).
    pub fn labels(&self) -> Vec<tools::PreviewLabel> {
        self.preview().labels
    }

    /// The points the tool marks with a cross (world), which the lengths keep clear of.
    pub fn marks(&self) -> Vec<DVec3> {
        self.preview().marks
    }

    /// The round points the tool draws over everything (the app draws them).
    pub fn overlay_points(&self) -> Vec<tools::PreviewPoint> {
        self.preview().points
    }
}

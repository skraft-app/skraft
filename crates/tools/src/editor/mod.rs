//! The editor: the document's history, its display scene, the camera, the context being edited,
//! the selection and the active tool. The app forwards input to it and draws what [`View`]
//! says; everything here runs without a UI or a GPU.
//!
//! The kernel runs behind a [`KernelService`] (docs/model/kernel.md): an operation that
//! needs it is *pending* until [`Editor::poll`] gets its result. Meanwhile only navigation and
//! hovering work; Esc abandons it. Meshes the scene lacks are asked for the same way.
//!
//! Here: the state, the tools and their input. The rest, by concern: [`document`] (operations
//! and the kernel, undo, opening), [`selection`] (selection and context), [`navigation`],
//! [`view`] (what to draw).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::kernel::{JobId, KernelService, Quality, Solid};
use skraft_model::{Document, EntityPath, GuidePath, History, Id, Operation, Outcome, Scope, Sign, measure};

use crate::camera::{Camera, ViewArea};
use crate::infer::{EdgeRef, InferKind, Lock};
use crate::scene::DisplayScene;
use crate::tools::{self, Command, Cx, DragClicks, Key, OptionValue, OptionValues, Preview, Tool, ToolInput, ToolKind, ToolOption};
use crate::vcb;

mod clipboard;
mod document;
pub mod journal;
mod library;
mod links;
mod loading;
mod naming;
mod navigation;
mod quick;
mod selection;
mod stack;
mod tree;
mod view;

pub use clipboard::{Clip, Content};
pub use document::cap_histories;
use document::{After, Pending, Probe};
pub use links::LinkAct;
pub use loading::{LoadEnd, LoadKind, Loading};
pub use naming::{NameTarget, Naming};
pub use tree::{Tree, TreeRow};
pub use view::{Look, Mark, OrientedBox, SectionCut, View, ViewHighlight, ViewPlacement};

/// A message for the app's toasts (docs/ui/interface.md#toasts): an operation that failed, a
/// file imported… The status bar keeps the tools' hints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub title: String,
    /// What went wrong, or more about it (the kernel's or the system's words).
    pub detail: Option<String>,
    /// What the toast's button does, if it has one.
    pub action: Option<NoticeAction>,
    /// A second button, after it.
    pub also: Option<NoticeAction>,
}

/// A notice's button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    /// Opens the naming dialog on this part, assembly or body definition, just made
    /// (docs/tools/quick-tools.md#naming-what-was-just-made, [`Editor::naming_of`]).
    Rename(NameTarget),
    /// Acts on what an operation left of linked bodies
    /// (docs/decisions/0055-linked-bodies-through-imprints.md).
    Link(LinkAct),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Success,
    /// What an operation done has to say (`Outcome::warning`).
    Warning,
    Error,
}

/// The most notices kept for the app: an editor nobody takes them from (headless) keeps the last.
const NOTICES: usize = 32;

pub struct Editor {
    /// What the links need the user to see, as of the current document.
    link_status: skraft_model::LinkStatus,
    /// Who shapes whom through the links, as of the current document.
    link_map: skraft_model::LinkMap,
    /// What the last toast about linked bodies offers.
    link_offer: Option<links::LinkOffer>,
    /// « Imprimer » waiting for the kernel's answer, and its plan once answered.
    imprint_ask: Option<links::ImprintAsk>,
    imprint_ready: Option<skraft_model::ImprintPlan>,
    /// The parts seated in one part that overlap, as the kernel last answered.
    link_overlaps: skraft_model::LinkOverlaps,
    overlap_memo: skraft_model::OverlapMemo,
    overlap_ask: Option<links::OverlapAsk>,
    /// An operation is waiting for the check: what it makes overlap gets a toast.
    overlap_tell: bool,
    /// The last check failed, and said so.
    overlap_failed: bool,
    history: History,
    kernel: Box<dyn KernelService>,
    next_job: JobId,
    pending: Option<Pending>,
    /// Operations of one action (explode several assemblies), applied one after the other.
    queue: VecDeque<(Operation, After)>,
    collected: Vec<Id>,
    /// What the operations of « Rendre unique » gave their own definition, offered to be named
    /// once they are all done (docs/tools/quick-tools.md#naming-what-was-just-made).
    made_unique: Vec<NameTarget>,
    /// Meshes asked for, by job: the solid, and how fine.
    meshing: HashMap<JobId, (Arc<Solid>, Quality)>,
    /// The load under way (docs/ui/loading.md): a file opened or imported, until its last mesh.
    load: Option<loading::Load>,
    /// How the last load ended, until the app takes it.
    load_end: Option<LoadEnd>,
    /// Files being read by the kernel (STEP), by job: their names (docs/io/step-import.md).
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
    /// The part open in the context, by its occurrence there (docs/model/opening-a-part.md).
    open_part: Option<Id>,
    /// The group of bodies open in the open part (docs/model/bodies.md#body-groups): the bodies leading
    /// to it, the part's own first; empty, the part itself.
    open_group: Vec<Id>,
    /// What a change to the open part reaches (docs/model/data-model.md): every occurrence, until said otherwise
    /// for this opening.
    part_scope: Scope,
    /// Whether a shape drawn in the open part takes material away (a machining) or adds it
    /// (docs/model/bodies.md): taking away, until said otherwise for this opening.
    body_sign: Sign,
    /// The machining hovered in the entity info's list (drawn hovered in the view).
    listed_hover: Option<Id>,
    /// The parts selected at any depth under « Parts » (docs/tools/selection.md), by their
    /// paths from the root, all under the active context; `selection` then empty.
    deep: Vec<EntityPath>,
    /// The guides selected at any depth under « Parts », with `deep`: paths from the root, all
    /// under the active context.
    deep_guides: Vec<GuidePath>,
    /// OCL's tree drop-up, while the copy modifier is held over a part (docs/tools/selection.md).
    tree: Option<tree::Tree>,
    /// The tree drop-up was opened since the copy modifier was pressed: the tap its release
    /// makes is not the tool's.
    tree_tap: bool,
    /// The model chosen (the navigator's first row clicked), until anything else is: selected,
    /// entered, opened, or a click in the view.
    model_selected: bool,
    /// The component chosen in the library (docs/ui/components-library.md#managing), until
    /// anything else is: [`Editor::chosen_component`].
    chosen: Option<Id>,
    /// The material chosen in its card (its entity info shown): [`Editor::chosen_material`].
    chosen_material: Option<library::ChosenMaterial>,
    /// The occurrence hovered in the navigator (drawn hovered in the view).
    navigator_hover: Option<EntityPath>,
    /// The stretch the scene's preview mesh was made for, and the revisions handed out.
    stretch_meshed: Option<tools::StretchPreview>,
    preview_revision: u64,
    /// A stretch, move or rotation applied, drawn as previewed until the kernel's job gives
    /// the part its new shape (else it would show as it was for a frame or more): the
    /// stretch, or the selection's ghost (copies left behind: as a copy).
    held: Option<(JobId, Preview)>,
    /// The occurrences a stretch previewed draws otherwise, by path: the mesh, the transform.
    stretch_swaps: HashMap<EntityPath, (Id, DAffine3)>,
    /// The bodies of the open part a stretch previewed draws otherwise (docs/model/bodies.md): the mesh, the
    /// placement in the part's axes.
    pub(super) body_swaps: HashMap<Id, (Id, DAffine3)>,
    /// The round or chamfer previewed (docs/tools/rounds.md): what the kernel was asked, its
    /// job while it runs, why it refused it, and whether its result is in the scene's preview.
    blend_asked: Option<tools::BlendPreview>,
    blend_job: Option<JobId>,
    blend_note: Option<String>,
    blend_shown: bool,
    /// The open body node's children's result (docs/tools/rounds.md): the last one known, the
    /// job computing it when its cache does not hold, and the children that job is for.
    node_raw: Option<Arc<skraft_model::RawCache>>,
    node_raw_job: Option<JobId>,
    node_raw_asked: Option<skraft_model::RawAsk>,
    /// How the selection is drawn, worked out once for the frames that follow (docs/render/display.md).
    selection_drawing: std::cell::RefCell<Option<view::SelectionDrawing>>,
    /// The section's cut lines, kept while the plane and the meshes stay (docs/render/look.md).
    section_cuts: crate::section_cut::SectionCuts,
    /// Is anything hidden, at the history's revision ([`Editor::has_hidden`]: asked at every
    /// frame, found by a pass over every assembly).
    has_hidden: std::cell::Cell<Option<(u64, bool)>>,
    selection: Vec<Id>,
    tool: Box<dyn Tool>,
    pivot: Option<DVec3>,
    /// The views gone through, for previous / next view (docs/ui/navigation-styles.md), and the one shown.
    views: Vec<Camera>,
    view_index: usize,
    message: Option<String>,
    /// What the app shows in its toasts, oldest first, till it takes them ([`Editor::take_notices`]).
    notices: Vec<Notice>,
    /// The part of the viewport no panel covers, if not all of it (pixels).
    free: Option<(DVec2, DVec2)>,
    /// Inference locks (docs/tools/inferences.md): the arrow keys' axis, and the line Shift holds.
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
    /// Points tracked (docs/tools/inferences.md, step 3): hovered [`TRACK_AFTER`] seconds; the latest
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
    /// The tools' saved settings (the option bar's, docs/ui/interface.md), by tool id.
    tool_settings: BTreeMap<String, OptionValues>,
    /// The material the paint tool paints with (docs/render/look.md).
    paint: Option<Id>,
    /// A paste carried at the cursor (docs/tools/copy-paste.md).
    carry: Option<clipboard::Carry>,
    /// The tools waiting under the active one, the nearest last (the tool stack,
    /// docs/tools/interaction.md#tool-stack).
    stack: Vec<stack::Suspended>,
    /// The tool that handed back to it: what is typed before the next shape goes to it.
    aside: Option<Box<dyn Tool>>,
    /// The quick tools' card, offered on what a creation tool just made.
    card: Option<quick::Card>,
    /// The naming dialog the app is to open, on a part just made (docs/tools/quick-tools.md).
    naming: Option<Naming>,
    /// A drag the editor turns into clicks for the tool (docs/tools/interaction.md, [`Tool::drag_clicks`]).
    drag_click: Option<DragClick>,
    /// What it went through: journal, kernel failures, recording (docs/control-protocol.md).
    journal: journal::Journal,
    /// How deep in the editor's own calls: a public method it calls itself is not the outside's
    /// (not recorded).
    depth: u32,
}

/// A drag turned into clicks, while the button is down.
#[derive(Clone, Copy, Debug)]
enum DragClick {
    /// The release clicks.
    Release,
    /// The press clicked here; the release clicks unless it is nearer than [`DRAG_CLICK_MIN`].
    Pressed(DVec2),
    /// Given up (Esc, another tool): the release does nothing.
    Dropped,
}

/// How far from the press a release clicks again (pixels; OpenCutList's 20).
const DRAG_CLICK_MIN: f64 = 20.0;

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
            link_status: doc.link_status(),
            link_map: doc.link_map(),
            link_offer: None,
            imprint_ask: None,
            imprint_ready: None,
            link_overlaps: Default::default(),
            overlap_memo: Default::default(),
            overlap_ask: None,
            overlap_tell: false,
            overlap_failed: false,
            history: History::new(doc),
            kernel,
            next_job: 1,
            pending: None,
            queue: VecDeque::new(),
            collected: Vec::new(),
            made_unique: Vec::new(),
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
            open_group: Vec::new(),
            part_scope: Scope::All,
            body_sign: Sign::Negative,
            listed_hover: None,
            model_selected: false,
            chosen: None,
            chosen_material: None,
            deep: Vec::new(),
            deep_guides: Vec::new(),
            tree: None,
            tree_tap: false,
            navigator_hover: None,
            stretch_meshed: None,
            preview_revision: 0,
            held: None,
            stretch_swaps: HashMap::new(),
            body_swaps: HashMap::new(),
            blend_asked: None,
            blend_job: None,
            node_raw: None,
            node_raw_job: None,
            node_raw_asked: None,
            blend_note: None,
            blend_shown: false,
            selection_drawing: std::cell::RefCell::new(None),
            section_cuts: Default::default(),
            has_hidden: std::cell::Cell::new(None),
            selection: Vec::new(),
            tool: tools::make(ToolKind::Select),
            pivot: None,
            views: Vec::new(),
            view_index: 0,
            message: None,
            notices: Vec::new(),
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
            carry: None,
            stack: Vec::new(),
            aside: None,
            card: None,
            naming: None,
            drag_click: None,
            journal: journal::Journal::default(),
            depth: 0,
        };
        // Opened: its meshes asked as a load (none for an empty document: ended at once).
        e.after_change();
        e.settle_load();
        e.load_end = None;
        e.zoom_extents();
        e.views.clear();
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

    /// The part open in the context, by its occurrence there (docs/model/opening-a-part.md).
    pub fn open_part(&self) -> Option<Id> {
        self.open_part
    }

    /// The group of bodies open in the open part: the bodies leading to it (docs/model/bodies.md « Body
    /// groups »); empty, the part itself.
    pub fn open_group(&self) -> &[Id] {
        &self.open_group
    }

    /// The body of the part (or group) open whose profile is edited (the tool « Profil »), if
    /// any: the body open, the deepest level of the editing (docs/model/bodies.md#edit-levels).
    pub fn profile_body(&self) -> Option<Id> {
        self.tool.profile_body(&self.cx())
    }

    /// Leaves the body open (its profile edited) for the part or group holding it, the body
    /// selected: was one open?
    fn leave_body(&mut self) -> bool {
        if self.tool.kind() != ToolKind::Profile {
            return false;
        }
        let body = self.profile_body();
        self.set_tool(ToolKind::Select);
        self.set_selection(body.into_iter().collect());
        self.listed_hover = None;
        true
    }

    /// The path of the open part's occurrence, into its group open (docs/model/bodies.md#body-groups).
    pub fn open_part_path(&self) -> Option<EntityPath> {
        self.open_part.map(|id| EntityPath::new(self.context.clone(), id).in_group(self.open_group.clone()))
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

    /// The occurrence hovered in the navigator, if any: drawn as if hovered in the view. Did it
    /// change (the view to draw again)?
    pub fn set_navigator_hover(&mut self, path: Option<EntityPath>) -> bool {
        let changed = self.navigator_hover != path;
        self.navigator_hover = path;
        changed
    }

    /// X-ray: every part drawn see-through, its edges all seen, and nothing hiding a point
    /// from inferences (the document's setting `display.xray`, docs/ui/view-bar.md).
    pub fn xray(&self) -> bool {
        self.history.document().settings().display.xray
    }

    /// Turns X-ray on or off: a change of the document's settings (undone as the others).
    pub fn set_xray(&mut self, on: bool) {
        if on != self.xray() {
            let mut s = self.history.document().settings().clone();
            s.display.xray = on;
            let _ = self.apply(Operation::SetSettings(s));
        }
    }

    /// What a change to the open part reaches.
    pub fn part_scope(&self) -> Scope {
        self.part_scope
    }

    pub fn set_part_scope(&mut self, scope: Scope) {
        self.part_scope = scope;
    }

    /// Whether a shape drawn in the open part adds material or takes it away (docs/model/bodies.md).
    pub fn body_sign(&self) -> Sign {
        self.body_sign
    }

    pub fn set_body_sign(&mut self, sign: Sign) {
        self.body_sign = sign;
    }

    /// Are an open part's bodies shown (blanks ghosts, machinings solids): is a part open?
    pub fn showing_machinings(&self) -> bool {
        self.open_part.is_some()
    }

    /// Brings the display scene up to date (the document, the recipe shown) and asks for the
    /// meshes it lacks.
    pub(super) fn sync_scene(&mut self) {
        self.scene.set_recipe(self.open_part_path());
        let active = self.context.iter().copied().chain(self.open_part).collect();
        let display = &self.history.document().settings().display;
        self.scene.set_active(active, display.hide_rest_of_model);
        self.scene.set_xray(display.xray);
        self.update_node_raw();
        self.scene.sync(self.history.document());
        self.request_meshes();
    }

    /// Gives the scene the open body node's children's result, where its edges lie, or asks
    /// the kernel for it when its cache does not hold (a file just read:
    /// docs/tools/rounds.md).
    fn update_node_raw(&mut self) {
        use skraft_model::NodeRaw;
        let wanted = self.open_group.split_last().and_then(|(&node, above)| {
            let parent = EntityPath::new(self.context.clone(), self.open_part?).in_group(above.to_vec());
            self.document().node_raw(&parent, node, self.node_raw.as_ref()).ok()
        });
        match wanted {
            Some(NodeRaw::Ask(mut ask)) => {
                self.scene.set_node_raw(None);
                if self.node_raw_asked.as_ref().is_none_or(|a| !a.same(&ask)) {
                    self.drop_node_raw_job();
                    let id = self.job_id();
                    self.kernel.submit(id, std::mem::take(&mut ask.job));
                    self.node_raw_job = Some(id);
                    self.node_raw_asked = Some(ask);
                }
            }
            Some(NodeRaw::Ready(raw)) => {
                self.drop_node_raw_job();
                self.scene.set_node_raw(Some(raw.clone()));
                self.node_raw = Some(raw);
            }
            None => {
                self.drop_node_raw_job();
                self.scene.set_node_raw(None);
                self.node_raw = None;
            }
        }
    }

    fn drop_node_raw_job(&mut self) {
        if let Some(j) = self.node_raw_job.take() {
            self.kernel.cancel(j);
        }
        self.node_raw_asked = None;
    }

    /// The kernel's answer for the open node's children's result: its task took no edge, so
    /// its output is that result. Refused, it is not asked again for the same children.
    pub(super) fn node_raw_done(&mut self, result: skraft_model::kernel::JobResult) {
        let Ok(mut outputs) = result else { return };
        let Some(skraft_model::kernel::Output::Node { solid, display, faces, .. }) = outputs.pop() else { return };
        if solid.is_empty() {
            return;
        }
        let Some(asked) = self.node_raw_asked.take() else { return };
        let solid = Arc::new(solid);
        self.scene.insert_at(&solid, display, Quality::Final);
        self.node_raw = Some(Arc::new(skraft_model::RawCache { tree: asked.tree, leaves: asked.leaves, solid, faces }));
        self.sync_scene();
    }

    /// The open part's axes in the context's (its occurrence's T); identity without one.
    fn part_in_context(&self) -> DAffine3 {
        let doc = self.document();
        self.open_part
            .and_then(|id| doc.assembly(self.context_definition()).ok()?.occurrence(id).map(|o| o.transform))
            .unwrap_or(DAffine3::IDENTITY)
    }

    /// The axes of what is open in the context's: the open part's, or its group open's
    /// (docs/model/bodies.md#body-groups); identity without one.
    fn axes_in_context(&self) -> DAffine3 {
        let doc = self.document();
        let group = self
            .open_part
            .filter(|_| !self.open_group.is_empty())
            .and_then(|id| doc.assembly(self.context_definition()).ok()?.occurrence(id).and_then(|o| doc.part(o.component).ok()))
            .and_then(|p| p.group_chain(&self.open_group).ok())
            .map_or(DAffine3::IDENTITY, |(_, t)| t);
        self.part_in_context() * group
    }

    /// The frame tools draw and infer in, to world: the open part's axes, else the context's
    /// (docs/model/data-model.md). The axes drawn are these.
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

    /// The active tool's cursor at its step now (docs/ui/cursors.md); none: the system's.
    pub fn tool_cursor(&self) -> Option<crate::tools::Cursor> {
        self.tool.cursor(&self.cx())
    }

    /// The active tool's steps, for the steps bar.
    pub fn tool_steps(&self) -> Option<crate::tools::Steps> {
        self.tool.steps()
    }

    /// Does the active tool wait for a validation (the ✓ / ✗ buttons, which give it Enter and
    /// Esc)? Not while the kernel computes: only Esc is taken then.
    pub fn tool_awaits_validation(&self) -> bool {
        !self.busy() && self.tool.awaits_validation()
    }

    /// Is the active tool at its pick step (the cursor is then the arrow)?
    pub fn tool_picking(&self) -> bool {
        self.tool.picking()
    }

    pub fn tool_kind(&self) -> ToolKind {
        self.tool.kind()
    }

    /// The status bar's text: the kernel at work, else the last message, else the tool's hint.
    /// The notices since the last call, oldest first: the app shows them as toasts.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    pub(super) fn notify(&mut self, kind: NoticeKind, title: String, detail: Option<String>) {
        self.push_notice(Notice { kind, title, detail, action: None, also: None });
    }

    pub(super) fn push_notice(&mut self, notice: Notice) {
        if self.notices.len() == NOTICES {
            self.notices.remove(0);
        }
        self.notices.push(notice);
    }

    pub fn status(&self) -> String {
        if let Some(p) = &self.pending {
            return format!("{} {}", skraft_i18n::ellipsis(t!("status.computing", op = p.op.label())), t!("status.computing_cancel"));
        }
        if self.tree.is_some() {
            return t!("status.tree");
        }
        self.message.clone().unwrap_or_else(|| {
            let mut s = self.tool.status();
            // Why the round previewed is refused (« 25 mm au plus »).
            if let Some(note) = &self.blend_note {
                s = format!("{s} · {note}");
            }
            // A node open, another tool on: how its edges are changed (docs/tools/rounds.md), first —
            // the tool's hint after it may not fit.
            if !self.blending()
                && let Some((_, name, _)) = self.open_node()
            {
                s = format!("{} · {s}", t!("status.node_open", name = name));
            }
            if let Some(tab) = self.tab_hint() {
                s = format!("{s} · {tab}");
            }
            // Under « Parts », the tree drop-up (docs/tools/selection.md).
            if self.tree_allowed() {
                s = format!("{s} · {}", t!("status.tree_key", key = tools::copy_key()));
            }
            s
        })
    }

    /// What Tab does now, for the status bar: it switches the tool's first choice to its next
    /// value ([`Editor::next_choice`]) — said only while there is one, its name and that value.
    fn tab_hint(&self) -> Option<String> {
        let option = self.tool_options().into_iter().find(|o| !o.choices.is_empty())?;
        let OptionValue::Choice(current) = &option.value else { return None };
        let at = option.choices.iter().position(|c| c.value == current).map_or(0, |i| (i + 1) % option.choices.len());
        Some(t!("status.tab", option = option.label, next = option.choices[at].label))
    }

    /// Is an operation waiting for the kernel (or queued)? Then only navigation works.
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.queue.is_empty()
    }

    /// The measurements box's label and the value it shows while nothing is typed, when the
    /// tool takes one.
    pub fn vcb(&self) -> Option<(String, String)> {
        self.aside_vcb().or_else(|| self.tool.vcb()).map(|v| {
            let shown = vcb::display(&v, &self.document().settings().units);
            (v.label, shown)
        })
    }

    /// Does the measurements box take a typed value now: what the tool takes at this step, or
    /// a length option's `<length>x`?
    pub fn takes_typing(&self) -> bool {
        self.aside_vcb().is_some()
            || self.tool.vcb().is_some()
            || self.tool_options().iter().any(|o| matches!(o.value, OptionValue::Length(_)))
    }

    /// Do Shift with `+` / `−` go to the tool now (the copies' count) rather than to the
    /// measurements box?
    pub fn takes_counts(&self) -> bool {
        self.tool.takes_counts()
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

    /// The body node open (the group open is one, docs/tools/rounds.md): its id, its name, its
    /// operation.
    pub fn open_node(&self) -> Option<(Id, String, skraft_model::kernel::names::BlendOp)> {
        let (parent, node, op) = tools::BlendTool::node(&self.cx())?;
        let (_, p, _) = self.document().resolve_container(&parent).ok()?;
        let name = p.body(node).map(|b| p.def_of(b).name.clone())?;
        Some((node, name, op))
    }

    /// Is the active tool a node's (« Arrondir », « Chanfreiner »)?
    pub fn blending(&self) -> bool {
        matches!(self.tool.kind(), ToolKind::Round | ToolKind::Chamfer)
    }

    pub(crate) fn cx(&self) -> Cx<'_> {
        self.cx_for(&*self.tool)
    }

    /// What `tool` sees (its settings among it, the line it infers on): the active tool's, also
    /// while it is taken out for a call.
    fn cx_for(&self, tool: &dyn Tool) -> Cx<'_> {
        let kind = tool.kind();
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
            open_group: &self.open_group,
            part_in_context: self.part_in_context(),
            axes_in_context: self.axes_in_context(),
            recipe: self.scene.recipe().filter(|_| self.showing_machinings()),
            part_scope: self.part_scope,
            body_sign: self.body_sign,
            selection: &self.selection,
            deep: &self.deep,
            deep_guides: &self.deep_guides,
            parts: self.picks_parts_now(),
            axis_lock: self.axis_lock,
            face_lock: self.face_lock,
            shift_down: self.shift,
            copy_down: self.copy,
            alt_down: self.alt,
            held: self.tool_held(),
            last_height: self.last_height,
            shift_lock: self.shift_lock,
            tracked: &self.tracked,
            reference: self.reference,
            options: self.tool_settings.get(kind.id()).unwrap_or(&NO_SETTINGS),
            material: self.paint_material(),
            previewed: &self.stretch_swaps,
            tool_line: tool.infer_line(),
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

    /// The active tool's options, as the bar shows them now: the selection's scope last
    /// (docs/tools/selection.md), for a tool that takes parts at any depth, no part open.
    pub fn tool_options(&self) -> Vec<ToolOption> {
        let mut options = self.tool.options(&self.cx());
        if self.tool.picks_parts() && self.open_part.is_none() {
            let parts = self.pick_parts();
            let choice = |value: &'static str, label: String, tip: String, icon: &'static str| tools::OptionChoice {
                value,
                label,
                tip,
                icon: Some(icon),
            };
            options.push(ToolOption {
                key: tools::PICK_SCOPE,
                label: t!("selection_scope.label"),
                tip: t!("selection_scope.tip"),
                value: OptionValue::Choice(if parts { tools::PICK_PARTS } else { tools::PICK_CONTEXT }.into()),
                choices: vec![
                    choice(tools::PICK_CONTEXT, t!("selection_scope.context"), t!("selection_scope.context_tip"), "select_context"),
                    choice(tools::PICK_PARTS, t!("selection_scope.parts"), t!("selection_scope.parts_tip"), "select_parts"),
                ],
                saved: true,
                icon: None,
                swatch: None,
            });
        }
        options
    }

    /// Is the selection's scope « Parts » (docs/tools/selection.md)?
    pub fn pick_parts(&self) -> bool {
        self.tool_settings
            .get(tools::SELECTION_SETTINGS)
            .and_then(|s| s.get(tools::PICK_SCOPE))
            .is_some_and(|v| matches!(v, OptionValue::Choice(c) if c == tools::PICK_PARTS))
    }

    /// Does a click of the active tool take parts at any depth now?
    fn picks_parts_now(&self) -> bool {
        self.pick_parts() && self.open_part.is_none() && self.tool.picks_parts()
    }

    /// Sets an option of the active tool (a click in the bar): a setting is kept for the tool,
    /// a gesture's state goes to the tool.
    pub fn set_tool_option(&mut self, key: &str, value: OptionValue) {
        let event = journal::Event::Option { key: key.to_owned(), value: value.clone() };
        self.note(event, Some(journal::Step::Option { key: key.to_owned(), value: value.clone() }));
        self.inside(|e| e.change_tool_option(key, value));
    }

    fn change_tool_option(&mut self, key: &str, value: OptionValue) {
        let Some(option) = self.tool_options().into_iter().find(|o| o.key == key) else { return };
        // The selection's scope, shared by the tools: what was selected the other way let go.
        if key == tools::PICK_SCOPE {
            if option.value != value {
                self.tool_settings.entry(tools::SELECTION_SETTINGS.to_owned()).or_default().insert(key.to_owned(), value);
                self.clear_selection();
                self.tool.reset();
                self.rehover();
            }
            return;
        }
        if option.saved {
            // Shared by the part creation tools that have it, as OCL's (docs/tools/draw-options.md).
            let kind = self.tool.kind();
            for k in std::iter::once(kind).chain(tools::synced_with(key).iter().copied().filter(|k| *k != kind)) {
                self.tool_settings.entry(k.id().to_owned()).or_default().insert(key.to_owned(), value.clone());
            }
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
        if self.tool.vcb().is_some_and(|v| v.kind == vcb::VcbKind::Count || v.counts > 0) {
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

    /// Turns the active tool's first choice over to its next value (Tab; [`Editor::tab_hint`]
    /// says it).
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
        if self.depth == 0 {
            self.journal.record(journal::Step::Shift(down), &self.camera, self.size);
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
            if self.depth == 0 {
                self.journal.record(journal::Step::Copy(down), &self.camera, self.size);
            }
            self.copy = down;
            if down {
                self.open_tree();
            } else {
                self.close_tree();
            }
            self.rehover();
        }
    }

    /// The other modifier pressed or released (the app tells every frame): the tool follows
    /// the cursor again with it.
    pub fn set_alt(&mut self, down: bool) {
        if down != self.alt {
            if self.depth == 0 {
                self.journal.record(journal::Step::Alt(down), &self.camera, self.size);
            }
            self.alt = down;
            // Released: the tool it held hands back (docs/tools/interaction.md#tool-stack).
            if !down && self.tool_held() {
                self.pop_tool(true);
            }
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
            self.inside(|e| e.input(ToolInput::Hover { px }));
        }
    }

    // ---- journal (docs/control-protocol.md) ------------------------------------------------------------------

    /// What it went through: the journal, the kernel's failures, the recording under way.
    pub fn journal(&self) -> &journal::Journal {
        &self.journal
    }

    /// Who causes what it goes through from now on (the journal's entries): the person at
    /// the window, or a client of the control channel.
    pub fn set_origin(&mut self, origin: journal::Origin) {
        self.journal.set_origin(origin);
    }

    /// Starts recording the inputs from the document and the view as they are now (again: from
    /// now), or stops: the recording, if one was under way.
    pub fn record(&mut self, on: bool) -> Option<journal::Recording> {
        let done = self.journal.stop_recording();
        if on {
            let document = self.document().to_json().unwrap_or_default();
            self.journal.start_recording(document, self.tool.kind());
        }
        done
    }

    /// Notes `event` in the journal and, called from outside (not from the editor's own
    /// calls), `step` in the recording.
    fn note(&mut self, event: journal::Event, step: Option<journal::Step>) {
        self.journal.log(event);
        if self.depth == 0
            && self.journal.is_recording()
            && let Some(step) = step
        {
            self.journal.record(step, &self.camera, self.size);
        }
    }

    /// Runs `f` as one of the editor's own calls: the public methods it calls are not recorded.
    fn inside<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.depth += 1;
        let r = f(self);
        self.depth -= 1;
        r
    }

    // ---- tools ---------------------------------------------------------------------------------

    pub fn set_tool(&mut self, kind: ToolKind) {
        self.note(journal::Event::Tool(kind), Some(journal::Step::Tool(kind)));
        self.inside(|e| e.switch_tool(kind));
    }

    fn switch_tool(&mut self, kind: ToolKind) {
        // Another tool gives up a paste carried.
        self.settle_carry(true);
        let mut tool = tools::make(kind);
        // Its saved options, read at once: what its status says before any input.
        for option in tool.options(&self.cx_for(&*tool)) {
            tool.set_option(option.key, &option.value);
        }
        let cx = self.cx_for(&*tool);
        tool.started(&cx);
        self.tool = tool;
        // Erase lets the selection go when it starts: it erases what its clicks take, never
        // the selection (docs/tools/selection.md).
        if kind == ToolKind::Erase {
            self.clear_selection();
        }
        self.stack.clear();
        self.aside = None;
        self.card = None;
        self.message = None;
        self.clear_locks();
        self.reference = None;
        if self.drag_click.is_some() {
            self.drag_click = Some(DragClick::Dropped);
        }
    }

    /// Feeds the active tool. Esc goes back one step of the tool; at its first step, it does
    /// nothing more, but with Select, where it clears the selection (or closes the open part),
    /// and in a body open (its profile edited), which it leaves.
    /// Delete erases the selection.
    pub fn input(&mut self, input: ToolInput) {
        // A click in the view, or Esc, chooses something else than the model.
        if matches!(
            input,
            ToolInput::Click { .. } | ToolInput::DoubleClick { .. } | ToolInput::DragStart { .. } | ToolInput::Key { key: Key::Escape, .. }
        ) {
            self.model_selected = false;
            self.chosen = None;
            // The paint tool's clicks keep it: what they paint with stays shown.
            if self.tool.kind() != ToolKind::Paint || matches!(input, ToolInput::Key { .. }) {
                self.chosen_material = None;
            }
        }
        self.note(journal::Event::Input { input: input.clone(), count: 1 }, Some(journal::Step::Input(input.clone())));
        self.inside(|e| {
            e.input_tool(input);
            // A paste carried put down or given up (Esc): back to the tool before.
            e.settle_carry(false);
        });
    }

    fn input_tool(&mut self, input: ToolInput) {
        if self.busy() {
            match input {
                ToolInput::Key { key: Key::Escape, .. } => self.abandon(),
                ToolInput::Hover { px } => self.last_hover = Some(px),
                _ => {}
            }
            return;
        }
        // The copy modifier's tap after the tree drop-up was open: the list's, not the tool's.
        if let ToolInput::Key { key: Key::Copy, .. } = input
            && std::mem::take(&mut self.tree_tap)
        {
            return;
        }
        if let Some(inputs) = self.drag_clicks(&input) {
            for input in inputs {
                self.input_tool(input);
            }
            return;
        }
        if !matches!(input, ToolInput::Hover { .. }) {
            self.message = None;
        }
        // A typed value is read for what the tool takes now (vcb.rs).
        let input = match input {
            ToolInput::Vcb(text) => {
                if self.type_aside(&text) {
                    return;
                }
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
        if let ToolInput::Key { key, .. } = &input
            && Self::card_key(*key)
        {
            self.card = None;
        }
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
                if self.drag_click.is_some() {
                    self.drag_click = Some(DragClick::Dropped);
                }
                self.clear_locks();
                // A tool at its first step stays active (only Space goes back to Select): Esc
                // only drops the locks.
                if self.tool.busy() {
                    self.tool.back();
                    // Back at its first step, a tool the quick tools started hands back; one
                    // that picks first lets the selection go, and picks again (docs/tools/selection.md).
                    if !self.tool.busy() {
                        if !self.quick_on_top() && self.tool.dropped() {
                            self.clear_selection();
                        }
                        self.hand_back(false);
                    }
                    self.rehover();
                } else if !self.stack.is_empty() {
                    // At its first step, a tool over another hands back (docs/tools/interaction.md#tool-stack).
                    self.pop_tool(false);
                    self.rehover();
                } else if self.aside.take().is_some() {
                    // The tool aside let go first.
                } else if self.tool.picks_first() && self.anything_selected() {
                    // What the tool would act on let go: it picks again (docs/tools/selection.md).
                    self.clear_selection();
                    self.rehover();
                } else if self.tool.kind() == ToolKind::Profile {
                    // The body open left for its part (docs/model/bodies.md#edit-levels).
                    self.exit();
                } else if self.tool.kind() == ToolKind::Select {
                    if self.selection.is_empty() && self.open_part.is_some() {
                        self.exit();
                    } else {
                        self.clear_selection();
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
        let cx = self.cx_for(&*tool);
        // Where the tool draws the selection moved, as the input may apply it: held while
        // computed (the tool is back at its first step by then).
        let ghost = match input {
            ToolInput::Hover { .. } => None,
            _ => tool.preview(&cx).ghost.map(|g| (g, tool.copying())),
        };
        let commands = tool.input(&input, &cx);
        self.tool = tool;
        let job_before = self.pending.as_ref().map(|p| p.job);
        if matches!(input, ToolInput::Hover { .. }) {
            let inference = self.preview().inference;
            if let Some(edge) = inference.as_ref().and_then(|i| i.edge) {
                self.reference = Some(edge);
            }
            // A point of the geometry under the cursor starts (or goes on) dwelling.
            let point = inference.filter(|i| i.kind.is_trackable()).map(|i| i.point);
            self.dwell = match (point, self.dwell) {
                (Some(p), Some((q, since))) if p == q => Some((q, since)),
                (Some(p), _) => Some((p, self.now)),
                (None, _) => None,
            };
        }
        let acted = Self::acts(&commands);
        let selection = (self.selection.clone(), self.deep.clone(), self.deep_guides.clone());
        for c in commands {
            self.run(c);
        }
        // What a tool that picks first just took (or let go) is what it acts on from now: it
        // sees it at once, not at the next move of the cursor (docs/tools/selection.md).
        if self.tool.picks_first()
            && (&self.selection, &self.deep, &self.deep_guides) != (&selection.0, &selection.1, &selection.2)
            && let Some(px) = self.last_hover
        {
            let mut tool = std::mem::replace(&mut self.tool, tools::make(ToolKind::Select));
            let cx = self.cx_for(&*tool);
            let commands = tool.input(&ToolInput::Hover { px }, &cx);
            self.tool = tool;
            for c in commands {
                self.run(c);
            }
        }
        if let (Some((g, copying)), Some(p)) = (ghost, &self.pending)
            && Some(p.job) != job_before
            && self.held.is_none()
        {
            let held =
                if copying { Preview { copies: vec![g], ..Preview::default() } } else { Preview { ghost: Some(g), ..Preview::default() } };
            self.held = Some((p.job, held));
        }
        if acted {
            self.hand_back(true);
        }
        // The next shape started: the tool aside takes nothing more, the card goes.
        if self.tool.busy() {
            self.aside = None;
            self.card = None;
        }
        self.update_preview_mesh();
        self.update_blend_preview();
    }

    /// A drag as clicks, for a tool that asks for it (docs/tools/interaction.md, OpenCutList's SmartDraw): the press
    /// clicks at the tool's first step, the cursor hovers, the release clicks (not when nearer
    /// than [`DRAG_CLICK_MIN`] to a press that clicked, nor when that press left the tool at its
    /// first step). None: the input goes as it is.
    fn drag_clicks(&mut self, input: &ToolInput) -> Option<Vec<ToolInput>> {
        match *input {
            ToolInput::DragStart { px, mods } => match self.tool.drag_clicks() {
                DragClicks::No => {
                    self.drag_click = None;
                    None
                }
                DragClicks::Release => {
                    self.drag_click = Some(DragClick::Release);
                    Some(vec![ToolInput::Hover { px }])
                }
                DragClicks::PressAndRelease => {
                    self.drag_click = Some(DragClick::Pressed(px));
                    Some(vec![ToolInput::Hover { px }, ToolInput::Click { px, mods }])
                }
            },
            ToolInput::Drag { px } if self.drag_click.is_some() => Some(vec![ToolInput::Hover { px }]),
            ToolInput::DragEnd { px, mods } => {
                let hover = ToolInput::Hover { px };
                match self.drag_click.take()? {
                    DragClick::Dropped => Some(vec![hover]),
                    DragClick::Pressed(from) if from.distance(px) < DRAG_CLICK_MIN => Some(vec![hover]),
                    // The press took nothing (the tool is still at its first step): nor does the release.
                    DragClick::Pressed(_) if self.tool.drag_clicks() == DragClicks::PressAndRelease => Some(vec![hover]),
                    DragClick::Pressed(_) | DragClick::Release => Some(vec![hover, ToolInput::Click { px, mods }]),
                }
            }
            _ => None,
        }
    }

    /// What the tool previews stretched, in the scene (docs/tools/stretch.md): the document as the stretch
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
        let mut body_swaps = HashMap::new();
        if let Some(s) = wanted.as_ref().filter(|s| s.bodies.is_some()) {
            // Bodies of the open part, each from its recipe mesh, in its own axes (docs/model/bodies.md).
            let path = EntityPath::new(s.context.clone(), s.occurrences[0]).in_group(self.open_group.clone());
            let stretch = skraft_model::kernel::Stretch::new(s.axis, s.cuts.clone(), s.moves.clone());
            let doc = self.document().clone();
            let recipe = self.scene.recipe().cloned();
            if let (Ok((_, part, _)), Ok(stretch), Some(r)) = (doc.resolve_container(&path), stretch, recipe) {
                let stretch = stretch.in_frame(&s.frame.inverse());
                // As the operation splits them: moved, or a definition changed — shown by the
                // copies it reaches, and made in place, by those it does not (docs/model/bodies.md).
                let plan = part.body_stretch(s.bodies.as_deref(), &stretch);
                for (id, t) in &plan.moved {
                    if let Some(rb) = r.body(*id) {
                        body_swaps.insert(*id, (rb.key, *t));
                    }
                }
                for c in &plan.changes {
                    let Some(def) = part.def(c.def) else { continue };
                    let Some(rb) = c.bodies.first().and_then(|id| r.body(*id)) else { continue };
                    let local = &c.local;
                    let Some(dm) = self.scene.definition_mesh(rb.key) else { continue };
                    let key = crate::scene::preview_key(meshes.len());
                    let spans = local.section_spans(&def.solid);
                    let parts = dm
                        .parts
                        .iter()
                        .map(|p| crate::scene::PartMesh {
                            solid: key,
                            mesh: Arc::new(crate::scene::stretched_mesh(&p.mesh, local, &spans)),
                            snaps: Arc::default(),
                            bounds: None,
                        })
                        .collect();
                    self.preview_revision += 1;
                    let faces = dm.faces.clone();
                    meshes.push(crate::scene::DefinitionMesh { definition: key, revision: self.preview_revision, parts, faces });
                    for id in part.shown_by(c, &plan) {
                        if let Some(b) = part.body(id) {
                            body_swaps.insert(id, (key, b.placement));
                        }
                    }
                }
            }
        } else if let Some(s) = &wanted {
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
        self.body_swaps = body_swaps;
        self.stretch_meshed = wanted;
    }

    /// Asks the kernel for the round or chamfer the tool previews, when it changed: the job
    /// before it dropped, the result shown in the part's (or the body's) stead when it comes
    /// ([`Editor::blend_done`]), docs/tools/rounds.md.
    pub(super) fn update_blend_preview(&mut self) {
        let wanted = self.preview().blend;
        if wanted == self.blend_asked {
            return;
        }
        if let Some(j) = self.blend_job.take() {
            self.kernel.cancel(j);
        }
        self.blend_note = None;
        if self.blend_shown {
            self.scene.set_preview(Vec::new());
            self.stretch_swaps.clear();
            self.body_swaps.clear();
            self.blend_shown = false;
        }
        if let Some(w) = &wanted {
            match self.document().blend_job(&w.op) {
                Ok(job) => {
                    let id = self.job_id();
                    self.kernel.submit(id, job);
                    self.blend_job = Some(id);
                }
                Err(e) => self.blend_note = Some(e.to_string()),
            }
        }
        self.blend_asked = wanted;
    }

    /// The kernel's preview of the round asked: drawn in the part's or the body's stead, or
    /// why it is refused, for the status bar.
    pub(super) fn blend_done(&mut self, result: skraft_model::kernel::JobResult) {
        let Some(w) = self.blend_asked.clone() else { return };
        let out = match result {
            Ok(mut outputs) => outputs.pop(),
            Err(e) => {
                self.blend_note = Some(e.to_string());
                return;
            }
        };
        let Some(skraft_model::kernel::Output::Node { display, refusal, .. }) = out else { return };
        if let Some(r) = refusal {
            self.blend_note = Some(skraft_model::blend_refusal(&r));
            return;
        }
        let Ok(d) = display else { return };
        let key = crate::scene::preview_key(0);
        self.preview_revision += 1;
        let part = crate::scene::PartMesh { solid: key, mesh: Arc::new(d.mesh), snaps: Arc::new(d.snaps), bounds: d.bounds };
        let mesh =
            crate::scene::DefinitionMesh { definition: key, revision: self.preview_revision, parts: vec![part], faces: Default::default() };
        self.scene.set_preview(vec![mesh]);
        match w.body {
            Some(b) => {
                self.body_swaps.insert(b, (key, DAffine3::IDENTITY));
            }
            None => {
                self.stretch_swaps.insert(w.path.clone(), (key, w.world));
            }
        }
        self.blend_shown = true;
    }

    pub(crate) fn run(&mut self, c: Command) {
        match c {
            Command::Apply(op) => {
                // A tool's setting kept on a component (the stretch's cutters) is no new gesture.
                if !matches!(op, Operation::SetBehavior { .. }) {
                    self.clear_locks();
                }
                let stretching = matches!(op, Operation::Stretch { .. } | Operation::StretchBodies { .. });
                let held = stretching.then(|| self.stretch_meshed.clone()).flatten();
                // A round applied: its preview drawn until the kernel gives the part its shape.
                let blending = matches!(op, Operation::Blend { .. } | Operation::SetBlend { .. }) && self.blend_shown;
                let held_blend = blending.then(|| self.blend_asked.clone()).flatten();
                // What a boolean or a drawing tool makes is not selected: the next shape is
                // drawn straight away.
                // What a drawing tool makes is offered the quick tools' card (docs/tools/quick-tools.md); a
                // part made, its name asked or offered (docs/tools/quick-tools.md).
                let after = match op {
                    Operation::AddPrimitive { .. } | Operation::DrawPrimitive { .. } => {
                        After::Card { ask_name: Some(self.cx().toggle(tools::ASK_NAME)) }
                    }
                    // A part made in another assembly (« Dessiner dans », docs/tools/draw-options.md).
                    Operation::At { op: ref made, .. }
                        if matches!(**made, Operation::AddPrimitive { .. } | Operation::DrawPrimitive { .. }) =>
                    {
                        After::Card { ask_name: Some(self.cx().toggle(tools::ASK_NAME)) }
                    }
                    Operation::AddBody { .. } => After::Card { ask_name: None },
                    Operation::Boolean { .. } | Operation::SetProfile { .. } => After::Nothing,
                    // At any depth (docs/tools/selection.md): what it makes is not in the active context.
                    Operation::At { .. } | Operation::Batch(_) => After::Nothing,
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
                self.held = held.zip(self.pending.as_ref()).map(|(s, p)| (p.job, Preview { stretch: Some(s), ..Preview::default() }));
                if let (Some(b), Some(p)) = (held_blend, self.pending.as_ref()) {
                    self.held = Some((p.job, Preview { blend: Some(b), ..Preview::default() }));
                }
            }
            Command::Select(ids) => {
                // Let go right after an operation still computed (a tool picking again,
                // docs/tools/selection.md): once it lands, what it moved drawn where it goes until then.
                if ids.is_empty() {
                    let pending = self.pending.as_mut().map(|p| &mut p.after);
                    let mut waiting = false;
                    for after in pending.into_iter().chain(self.queue.iter_mut().map(|(_, a)| a)) {
                        if matches!(after, After::Select | After::Nothing) {
                            *after = After::LetGo;
                            waiting = true;
                        }
                    }
                    if waiting {
                        return;
                    }
                }
                self.set_selection(ids);
            }
            Command::SelectPaths(paths, guides) => self.select_parts_and_guides(paths, guides),
            Command::SetOption { key, value } => self.set_tool_option(key, value),
            Command::RememberHeight(h) => self.last_height = Some(h),
            Command::Enter(id) => self.enter(id),
            Command::EditProfile(body) => {
                self.set_tool(ToolKind::Profile);
                self.tool = Box::new(tools::profile::ProfileTool::of_body(body));
                self.clear_selection();
            }
            Command::Exit => self.exit(),
            Command::ClickOff(px) => match self.context_bounds() {
                Some(b) if !b.hit_by(&self.camera.ray(px, self.size)) => self.exit(),
                _ => self.clear_selection(),
            },
            Command::Say(s) => self.message = Some(s),
            Command::PaintWith(m) => self.paint = m,
            Command::Hold(kind) => {
                self.push_tool(kind, stack::Over::Held);
                self.rehover();
            }
            Command::Start(kind) => {
                self.switch_tool(kind);
                self.rehover();
            }
            Command::ClearLocks => {
                self.axis_lock = None;
                self.face_lock = None;
                self.shift_lock = None;
                // The tool's next step follows the cursor at once, not at its next move.
                self.rehover();
            }
            Command::KeepLastTracked => {
                let older = self.tracked.len().saturating_sub(1);
                self.tracked.drain(..older);
                self.dwell = None;
                self.rehover();
            }
            Command::Replace { previous, op } => {
                if self.history.undo_op() == Some(&*previous) && self.history.undo() {
                    self.after_change();
                    self.clear_locks();
                    let after = if matches!(*op, Operation::At { .. } | Operation::Batch(_)) { After::Nothing } else { After::Select };
                    let _ = self.start(*op, after, true);
                } else {
                    self.message = Some(t!("status.copy_not_last"));
                }
            }
        }
    }

    /// What the tool shows; while a stretch, move or rotation it applied is computed, that
    /// alone.
    pub fn preview(&self) -> Preview {
        if let Some((job, held)) = &self.held
            && self.pending.as_ref().is_some_and(|p| p.job == *job)
        {
            return held.clone();
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

    /// The lengths the tool writes over the scene (the app draws them), those measuring a value
    /// of the measurements box showing what `typed` in it gives that value (vcb.rs).
    pub fn labels(&self, typed: &str) -> Vec<tools::PreviewLabel> {
        let mut labels = self.preview().drawing.lengths;
        let values = match self.tool.vcb() {
            Some(v) if !typed.trim().is_empty() && self.length_typed(typed).is_none() => {
                vcb::typed(&v, typed, &self.document().settings().units)
            }
            _ => Vec::new(),
        };
        labels.retain_mut(|l| match l.slot.and_then(|s| values.get(s).cloned().flatten()) {
            Some(t) => {
                l.text = t.text;
                l.result = t.result;
                l.active = t.active;
                true
            }
            None => !l.hidden,
        });
        labels
    }

    /// The points the tool marks with a cross (world), which the lengths keep clear of.
    pub fn marks(&self) -> Vec<DVec3> {
        self.preview().drawing.marks
    }

    /// The round points the tool draws over everything (the app draws them).
    pub fn overlay_points(&self) -> Vec<tools::PreviewPoint> {
        self.preview().drawing.grips
    }
}

//! The editor: the document's history, its display scene, the camera, the context being edited,
//! the selection and the active tool. The app forwards input to it and draws what [`View`]
//! says; everything here runs without a UI or a GPU.

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_model::{DefinitionKind, Document, Entity, EntityPath, History, Id, Operation, Outcome};

use crate::camera::{Camera, StandardView, ViewArea};
use crate::infer::{EdgeRef, InferKind, Lock};
use crate::scene::{DisplayScene, solids_under};
use crate::tools::{self, Command, Cx, Key, Preview, PreviewLine, Tool, ToolInput, ToolKind};

/// How a highlighted solid is marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Selected,
    Hover,
    /// Picked by a tool (the boolean's target).
    Picked,
    /// The selection where the move tool would put it.
    Ghost,
}

/// One occurrence of a definition to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPlacement {
    pub transform: DAffine3,
    /// Outside the context being edited (drawn faded).
    pub faded: bool,
}

/// One solid of one occurrence drawn again, marked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewHighlight {
    /// The definition whose mesh holds the solid.
    pub definition: Id,
    pub solid: Id,
    pub transform: DAffine3,
    pub mark: Mark,
}

/// What the viewport draws this frame (meshes come from [`Editor::scene`]).
#[derive(Clone, Debug, Default)]
pub struct View {
    pub batches: Vec<(Id, Vec<ViewPlacement>)>,
    pub highlights: Vec<ViewHighlight>,
    pub overlay: Vec<PreviewLine>,
    /// The inferred point to mark, with its kind (the app draws the marker and the tooltip).
    pub inference: Option<(DVec3, InferKind)>,
}

pub struct Editor {
    history: History,
    scene: DisplayScene,
    pub camera: Camera,
    size: DVec2,
    /// Instance path of the context being edited.
    context: Vec<Id>,
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
}

/// How long a point is hovered to be tracked (seconds).
pub const TRACK_AFTER: f64 = 0.5;
/// How many points are tracked at once.
pub const MAX_TRACKED: usize = 2;

impl Default for Editor {
    fn default() -> Self {
        Self::new(Document::new())
    }
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let mut scene = DisplayScene::new();
        scene.sync(&doc);
        let mut e = Editor {
            history: History::new(doc),
            scene,
            camera: Camera::default(),
            size: DVec2::new(800.0, 600.0),
            context: Vec::new(),
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
        };
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

    /// Instance path of the context being edited (empty: the model).
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

    /// The status bar's text: the last message, else the tool's hint.
    pub fn status(&self) -> String {
        self.message.clone().unwrap_or_else(|| self.tool.status())
    }

    pub fn vcb(&self) -> Option<(&'static str, String)> {
        self.tool.vcb_label().map(|l| (l, self.tool.vcb_value(&self.document().settings().units)))
    }

    pub fn set_viewport_size(&mut self, size: DVec2) {
        self.size = size.max(DVec2::ONE);
    }

    pub fn viewport_size(&self) -> DVec2 {
        self.size
    }

    /// The part of the viewport floating panels leave visible (pixels, origin top left): what
    /// « zoom étendu » frames.
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
        let doc = self.history.document();
        Cx {
            doc,
            scene: &self.scene,
            camera: &self.camera,
            size: self.size,
            context: &self.context,
            context_def: doc.context_of(&self.context).unwrap_or(doc.root()),
            context_transform: doc.transform_of(&self.context).unwrap_or(DAffine3::IDENTITY),
            selection: &self.selection,
            axis_lock: self.axis_lock,
            shift_lock: self.shift_lock,
            tracked: &self.tracked,
            reference: self.reference,
        }
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
        if !matches!(input, ToolInput::Hover { .. }) {
            self.message = None;
        }
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
                } else {
                    self.selection.clear();
                }
                return;
            }
            ToolInput::Key { key: Key::Delete, .. } if !self.tool.busy() => {
                self.delete_selection();
                return;
            }
            _ => {}
        }
        // The tool reads the editor while it runs: take it out for the call.
        let mut tool = std::mem::replace(&mut self.tool, tools::make(ToolKind::Select));
        let commands = tool.input(&input, &self.cx());
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

    fn run(&mut self, c: Command) {
        match c {
            Command::Apply(op) => {
                self.clear_locks();
                let keeps_selection = matches!(op, Operation::Boolean { .. });
                if let Ok(out) = self.apply(op)
                    && !out.created.is_empty()
                    && !keeps_selection
                {
                    self.selection = out.created;
                }
            }
            Command::Select(ids) => self.selection = ids,
            Command::Enter(id) => self.enter(id),
            Command::Exit => self.exit(),
            Command::Say(s) => self.message = Some(s),
            Command::Replace { previous, op } => {
                if self.history.undo_op() == Some(&previous) && self.history.undo() {
                    self.after_change();
                    self.run(Command::Apply(op));
                } else {
                    self.message = Some("Le dernier pas n'est plus cette copie".into());
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

    // ---- document ------------------------------------------------------------------------------

    /// Applies `op`; on failure the status bar says why and nothing changes.
    pub fn apply(&mut self, op: Operation) -> Result<Outcome, String> {
        let label = op.label();
        match self.history.apply(op) {
            Ok(out) => {
                self.after_change();
                Ok(out)
            }
            Err(e) => {
                let msg = format!("{label} : {e}");
                self.message = Some(msg.clone());
                Err(msg)
            }
        }
    }

    pub fn undo(&mut self) {
        self.tool.reset();
        self.clear_locks();
        if self.history.undo() {
            self.after_change();
        }
    }

    pub fn redo(&mut self) {
        self.tool.reset();
        self.clear_locks();
        if self.history.redo() {
            self.after_change();
        }
    }

    /// Replaces the document (a file opened).
    pub fn open(&mut self, doc: Document) {
        self.history = History::new(doc);
        self.context.clear();
        self.selection.clear();
        self.tool.reset();
        self.after_change();
        self.zoom_extents();
    }

    /// Keeps the context and the selection valid, and the scene in step.
    fn after_change(&mut self) {
        let doc = self.history.document();
        while !self.context.is_empty() && doc.context_of(&self.context).is_err() {
            self.context.pop();
        }
        let ctx = doc.context_of(&self.context).unwrap_or(doc.root());
        if let Ok(def) = doc.definition(ctx) {
            self.selection.retain(|id| def.entity(*id).is_some());
        }
        self.scene.sync(doc);
        for (id, err) in self.scene.errors() {
            self.message = Some(format!("Affichage de {id} impossible : {err}"));
        }
    }

    pub fn select(&mut self, ids: Vec<Id>) {
        self.selection = ids;
    }

    pub fn select_all(&mut self) {
        let ctx = self.context_definition();
        self.selection = self.document().definition(ctx).map(|d| d.entities.iter().map(Entity::id).collect()).unwrap_or_default();
    }

    /// Selects the entity `path` names, editing the context it is in (the outliner's click).
    pub fn select_path(&mut self, path: &EntityPath) {
        if self.document().resolve(path).is_err() {
            return;
        }
        if self.context != path.instances {
            self.tool.reset();
            self.context = path.instances.clone();
        }
        self.selection = vec![path.entity];
    }

    /// Edits the instance `id` of the active context.
    pub fn enter(&mut self, id: Id) {
        let is_instance = self.document().resolve(&EntityPath::new(self.context.clone(), id)).is_ok_and(|e| e.as_instance().is_some());
        if is_instance {
            self.tool.reset();
            self.context.push(id);
            self.selection.clear();
        }
    }

    /// Goes back to the parent context, the instance left selected.
    pub fn exit(&mut self) {
        if let Some(id) = self.context.pop() {
            self.tool.reset();
            self.selection = vec![id];
        }
    }

    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let entities = std::mem::take(&mut self.selection);
        let _ = self.apply(Operation::Delete { context: self.context_definition(), entities });
    }

    /// Puts the selection in a new group or component.
    pub fn make_container(&mut self, kind: DefinitionKind) {
        if self.selection.is_empty() {
            self.message = Some("Sélectionner d'abord ce qu'il faut regrouper".into());
            return;
        }
        let op = Operation::MakeContainer { context: self.context_definition(), entities: self.selection.clone(), kind, name: None };
        if let Ok(out) = self.apply(op) {
            self.selection = out.created;
        }
    }

    /// The selected instances of the active context.
    fn selected_instances(&self) -> Vec<Id> {
        let Ok(def) = self.document().definition(self.context_definition()) else { return vec![] };
        self.selection.iter().copied().filter(|id| def.entity(*id).is_some_and(|e| e.as_instance().is_some())).collect()
    }

    pub fn explode(&mut self) {
        let context = self.context_definition();
        let mut created = Vec::new();
        for instance in self.selected_instances() {
            if let Ok(out) = self.apply(Operation::Explode { context, instance }) {
                created.extend(out.created);
            }
        }
        if !created.is_empty() {
            self.selection = created;
        }
    }

    pub fn make_unique(&mut self) {
        let context = self.context_definition();
        for instance in self.selected_instances() {
            let _ = self.apply(Operation::MakeUnique { context, instance });
        }
    }

    // ---- navigation (SketchUp) -----------------------------------------------------------------

    /// Starts an orbit or a pan at `px`: the point under the cursor becomes the pivot.
    pub fn nav_begin(&mut self, px: DVec2) {
        let ray = self.camera.ray(px, self.size);
        self.pivot = Some(
            self.scene
                .pick(&ray, |_| true)
                .map(|h| h.point)
                .unwrap_or_else(|| self.camera.eye + self.camera.forward() * self.camera.focus),
        );
    }

    fn pivot(&self) -> DVec3 {
        self.pivot.unwrap_or_else(|| self.camera.eye + self.camera.forward() * self.camera.focus)
    }

    pub fn orbit(&mut self, delta: DVec2) {
        let p = self.pivot();
        self.camera.orbit(delta, p);
    }

    pub fn pan(&mut self, delta: DVec2) {
        let p = self.pivot();
        self.camera.pan(delta, self.size, p);
    }

    /// Zooms by `steps` (positive: closer) towards the scene under `px`.
    pub fn zoom(&mut self, px: DVec2, steps: f64) {
        let ray = self.camera.ray(px, self.size);
        let target = self.scene.pick(&ray, |_| true).map(|h| h.point);
        self.camera.zoom(steps, px, self.size, target);
    }

    /// Centres the view on the scene point under `px` (SketchUp's middle double-click).
    pub fn recenter(&mut self, px: DVec2) {
        let ray = self.camera.ray(px, self.size);
        if let Some(h) = self.scene.pick(&ray, |_| true) {
            let d = (h.point - self.camera.eye).dot(self.camera.forward());
            self.camera.eye = h.point - self.camera.forward() * d;
            self.camera.focus = d.abs().max(1e-3);
        }
    }

    pub fn zoom_extents(&mut self) {
        let b = self.scene.bounds().unwrap_or((DVec3::splat(-1000.0), DVec3::splat(1000.0)));
        self.camera.zoom_extents(b, self.view_area());
    }

    pub fn standard_view(&mut self, v: StandardView) {
        let b = self.scene.bounds().or(Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))));
        self.camera.set_standard_view(v, b, self.view_area());
    }

    pub fn set_parallel(&mut self, parallel: bool) {
        self.camera.set_parallel(parallel);
    }

    // ---- drawing -------------------------------------------------------------------------------

    /// A dashed line from `a` to `b` (dashes of a few pixels at their depth).
    fn dashed(&self, out: &mut Vec<PreviewLine>, a: DVec3, b: DVec3, color: [f32; 4]) {
        let len = (b - a).length();
        let step = self.camera.world_per_pixel(self.size, (a + b) * 0.5) * 10.0;
        if len < 1e-12 || step <= 0.0 {
            return;
        }
        let n = ((len / step).ceil() as usize).min(400);
        for k in (0..n).step_by(2) {
            let (t0, t1) = (k as f64 / n as f64, ((k + 1) as f64 / n as f64).min(1.0));
            out.push(PreviewLine { a: a.lerp(b, t0), b: a.lerp(b, t1), color });
        }
    }

    /// What to draw now.
    pub fn view(&self) -> View {
        let mut v = View::default();
        let doc = self.document();
        for (def, places) in self.scene.placements() {
            let ps = places
                .iter()
                .map(|p| ViewPlacement { transform: p.transform, faded: !p.path.starts_with(&self.context) })
                .collect();
            v.batches.push((def, ps));
        }
        let mark = |out: &mut Vec<ViewHighlight>, ctx: &[Id], id: Id, m: Mark, extra: DAffine3| {
            for (definition, solid, t) in solids_under(doc, ctx, id) {
                out.push(ViewHighlight { definition, solid, transform: extra * t, mark: m });
            }
        };
        let preview = self.preview();
        for id in &self.selection {
            mark(&mut v.highlights, &self.context, *id, Mark::Selected, DAffine3::IDENTITY);
        }
        if let Some(h) = preview.hover.filter(|h| !self.selection.contains(h)) {
            mark(&mut v.highlights, &self.context, h, Mark::Hover, DAffine3::IDENTITY);
        }
        for (path, m) in [(&preview.hover_solid, Mark::Hover), (&preview.picked_solid, Mark::Picked)] {
            if let Some(p) = path {
                mark(&mut v.highlights, &p.instances, p.entity, m, DAffine3::IDENTITY);
            }
        }
        if let Some(g) = preview.ghost {
            for id in &self.selection {
                mark(&mut v.highlights, &self.context, *id, Mark::Ghost, g);
            }
        }
        v.overlay = preview.lines;
        // Guides from tracked points, dashed (the tools draw those from their start point).
        if let Some(i) = &preview.inference {
            let color = tools::guide_color(i.kind);
            for (origin, _) in [i.line, i.line2].into_iter().flatten() {
                if self.tracked.contains(&origin) {
                    self.dashed(&mut v.overlay, origin, i.point, color);
                }
            }
        }
        v.inference = preview.inference.map(|i| (i.point, i.kind));
        v
    }
}

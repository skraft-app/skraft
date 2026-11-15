//! The application window: title bar (top), workspace card and ribbon across the top of the
//! view (§6 septies), the tool option bar centered under them, entity info, materials and
//! outliner (cards docked left or right, under the ribbon), status bar with the measurements
//! box (bottom), viewport (centre). Every action goes through the command registry (docs/ARCHITECTURE.md §7 quater):
//! menus (native on macOS, ☰ elsewhere), title bar, tool bar, keyboard (SketchUp's shortcuts,
//! changeable), the command palette and the control channel.

use solidsketch_i18n::{ellipsis, t};
use solidsketch_model::kernel::{KernelService, Progress, SharedKernel};
use solidsketch_model::{Boolean, Document, LengthUnit, Operation, Settings};
use solidsketch_theme::{Choice, Mode, Palette as UiPalette, Theme};
use solidsketch_tools::{Editor, Key, LoadEnd, LoadKind, StandardView, ToolInput, ToolKind};

use crate::clipboard::Clipboard;
use crate::commands::{self, COMMANDS, Keymap, Menu, Shortcut};
use crate::dock::{self, Dock, PanelId};
use crate::docs::{DocId, Docs, file_name};
use crate::file::{self, Inbox};
#[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
use crate::native_menu::NativeMenu;
use crate::panels::{self, Action, CommandView};
use crate::prefs::{Navigation, Preferences};
use crate::ribbon;
use crate::tabs::{MenuRow, Tab, TabAction};
use crate::titlebar;
use crate::viewport::{self, Viewport};

/// A side panel dragged by its header to another place.
#[derive(Clone, Copy)]
struct PanelDrag {
    id: PanelId,
    /// The pointer from the card's top left corner.
    offset: egui::Vec2,
    size: egui::Vec2,
}

pub struct SolidSketchApp {
    ctx: egui::Context,
    /// The documents open, one per file tab (docs/ARCHITECTURE.md §6 octies); none shown: the
    /// start page.
    docs: Docs,
    /// The document shown at the last frame: the tools' settings go to a newly shown one.
    last_shown: Option<DocId>,
    /// Tabs being closed (or the app quitting), asking about the modified documents.
    closing: Option<Closing>,
    /// Every modified document was asked about: the window may close now.
    quit_ok: bool,
    /// A document is modified: the page asks before it is left (web).
    #[cfg(target_arch = "wasm32")]
    unsaved: std::rc::Rc<std::cell::Cell<bool>>,
    viewport: Viewport,
    inbox: Inbox,
    message: Option<String>,
    prefs: Preferences,
    /// The preferences as last stored: they are written again when they differ.
    stored_prefs: Preferences,
    prefs_window: Option<PrefsTab>,
    /// The side panel being dragged to another place.
    panel_drag: Option<PanelDrag>,
    /// The shortcuts in use (the defaults with the user's changes).
    keymap: Keymap,
    /// The command whose shortcut the Raccourcis tab waits for.
    capturing: Option<&'static str>,
    palette: Option<Palette>,
    /// The ribbon's bottom this frame (`None`: no document shown): the command palette opens
    /// below it.
    ribbon_bottom: Option<f32>,
    theme: Theme,
    /// The palette egui's styles were installed for (`None`: not yet).
    styled: Option<UiPalette>,
    /// The copy modifier, Shift and the other modifier (Command on macOS, Alt elsewhere), each
    /// followed to tell a tap of it alone ([`Tap`]).
    taps: [Tap; 3],
    /// Shows the Débogage menu ([`debug_menu_wanted`]).
    debug_menu: bool,
    /// The control channel, switched on and off while the app runs (desktop).
    #[cfg(not(target_arch = "wasm32"))]
    remote: crate::control::Remote,
    /// The macOS menu bar (`None` if it could not be built: the ☰ menu then).
    #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
    native_menu: Option<NativeMenu>,
    /// Keys given back to egui on the next frame ([`Self::native_command`]).
    pending_keys: Vec<egui::Event>,
    /// The documents loading, and since when (egui's time): the window comes after
    /// [`LOAD_WINDOW_AFTER`].
    loads_seen: std::collections::HashMap<DocId, f64>,
    /// What was copied, for every document (docs/ARCHITECTURE.md §6 decies), and how many
    /// copies were made (each names its token).
    clipboard: Option<Clipboard>,
    copies: u64,
}

/// How long a load lasts before its window shows (seconds).
const LOAD_WINDOW_AFTER: f64 = 0.3;

/// Closing documents one after the other (docs/ARCHITECTURE.md §6 octies): each modified one
/// is shown and asked about (Save, Don't save, Cancel) before it closes. Quitting asks the same
/// about every modified document, but closes none: Cancel leaves them all open.
struct Closing {
    /// The documents still to go through, in order.
    queue: Vec<DocId>,
    /// The app quits once they are gone through.
    quit: bool,
    /// The one asked about now (the question shown)…
    asking: Option<DocId>,
    /// …or being saved: it closes once saved.
    saving: Option<DocId>,
}

/// An answer to « Enregistrer les modifications ? ».
#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
    Save,
    DontSave,
    Cancel,
}

/// The command palette (Cmd/Ctrl+K): what is typed, and the line picked.
#[derive(Default)]
struct Palette {
    query: String,
    picked: usize,
    /// The list was brought back to its top for this query. egui keeps a scroll offset across
    /// launches: without it, the list would open where it was last left.
    at_top: bool,
}

/// The Débogage menu is asked for at launch, in any build (no dev build needed): `?debug` in
/// the page's address on the web, `SOLIDSKETCH_DEBUG` set in the environment on the desktop.
fn debug_menu_wanted() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.location().search().ok())
            .is_some_and(|q| q.trim_start_matches('?').split('&').any(|p| p == "debug" || p.starts_with("debug=")))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var_os("SOLIDSKETCH_DEBUG").is_some()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrefsTab {
    General,
    Model,
    Navigation,
    Interface,
    Shortcuts,
    /// The control channel (desktop).
    Debug,
}

/// Where the kernel runs (docs/ARCHITECTURE.md §7 ter): its own thread on the desktop, a web
/// worker in the browser. Either wakes the UI when a result is ready.
fn kernel_service(ctx: &egui::Context) -> Box<dyn KernelService> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let ctx = ctx.clone();
        Box::new(solidsketch_model::kernel::Pool::new(Some(std::sync::Arc::new(move || ctx.request_repaint()))))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let ctx = ctx.clone();
        Box::new(crate::worker::WebKernel::new(std::rc::Rc::new(move || ctx.request_repaint())))
    }
}

/// The tool a `tool.*` command starts.
pub(crate) fn tool_of(id: &str) -> Option<ToolKind> {
    Some(match id {
        "tool.select" => ToolKind::Select,
        "tool.erase" => ToolKind::Erase,
        "tool.box" => ToolKind::Box,
        "tool.cylinder" => ToolKind::Cylinder,
        "tool.prism" => ToolKind::Prism,
        "tool.turn" => ToolKind::Turn,
        "tool.move" => ToolKind::Move,
        "tool.rotate" => ToolKind::Rotate,
        "tool.paint" => ToolKind::Paint,
        "tool.section" => ToolKind::Section,
        "tool.measure" => ToolKind::Measure,
        "tool.stretch" => ToolKind::Stretch,
        "tool.copy_line" => ToolKind::CopyLine,
        "tool.copy_grid" => ToolKind::CopyGrid,
        "tool.distribute" => ToolKind::Distribute,
        "tool.union" => ToolKind::Boolean(Boolean::Union),
        "tool.difference" => ToolKind::Boolean(Boolean::Difference),
        "tool.intersection" => ToolKind::Boolean(Boolean::Intersection),
        _ => return None,
    })
}

fn view_of(id: &str) -> Option<StandardView> {
    Some(match id {
        "view.top" => StandardView::Top,
        "view.front" => StandardView::Front,
        "view.right" => StandardView::Right,
        "view.back" => StandardView::Back,
        "view.left" => StandardView::Left,
        "view.iso" => StandardView::Iso,
        _ => return None,
    })
}

impl SolidSketchApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        crate::style::install_fonts(&cc.egui_ctx);
        let (prefs, error) = Preferences::load();
        // The texts name keys as the platform does (in the browser too).
        solidsketch_i18n::set_mac(cc.egui_ctx.os() == egui::os::OperatingSystem::Mac);
        solidsketch_i18n::set_locale(&prefs.locale);
        let docs = Docs::new(SharedKernel::new(kernel_service(&cc.egui_ctx)));
        #[cfg(not(target_arch = "wasm32"))]
        let (remote, error) = {
            let ctx = cc.egui_ctx.clone();
            let mut remote = crate::control::Remote::new(crate::control::env_port().unwrap_or(prefs.remote_port), move || ctx.request_repaint());
            // The preference, or the environment for this launch only.
            let error = match prefs.remote_control || crate::control::env_wanted() {
                true => remote.set(true).err().map(|e| t!("status.control_failed", error = e)).or(error),
                false => error,
            };
            (remote, error)
        };
        // Its native menu is added below, on macOS.
        #[cfg_attr(not(all(target_os = "macos", not(target_arch = "wasm32"))), allow(unused_mut))]
        let mut app = SolidSketchApp {
            ctx: cc.egui_ctx.clone(),
            docs,
            last_shown: None,
            closing: None,
            quit_ok: false,
            #[cfg(target_arch = "wasm32")]
            unsaved: Default::default(),
            viewport: Viewport::new(),
            inbox: Inbox::default(),
            message: error,
            keymap: Keymap::new(&prefs.shortcuts),
            panel_drag: None,
            stored_prefs: prefs.clone(),
            prefs,
            prefs_window: None,
            capturing: None,
            palette: None,
            ribbon_bottom: None,
            theme: Theme::light(),
            styled: None,
            taps: Default::default(),
            debug_menu: debug_menu_wanted(),
            #[cfg(not(target_arch = "wasm32"))]
            remote,
            #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
            native_menu: None,
            pending_keys: Vec::new(),
            loads_seen: std::collections::HashMap::new(),
            clipboard: None,
            copies: 0,
        };
        #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
        {
            let views = app.command_views();
            app.native_menu =
                NativeMenu::new(&cc.egui_ctx, &views, &app.keymap).inspect_err(|e| log::warn!("no native menu bar: {e}")).ok();
        }
        #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
        crate::app_drags_window(cc);
        #[cfg(not(target_arch = "wasm32"))]
        app.reopen_documents();
        #[cfg(target_arch = "wasm32")]
        app.ask_before_leaving();
        app
    }

    /// Opens again the documents open when the app last quit (desktop, if the preference
    /// says so); none left: the start page.
    #[cfg(not(target_arch = "wasm32"))]
    fn reopen_documents(&mut self) {
        if !self.prefs.reopen_documents {
            return;
        }
        let (files, shown) = (self.prefs.open_files.clone(), self.prefs.shown_file);
        let mut failed = Vec::new();
        let mut tabs = Vec::new();
        for path in &files {
            match std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| Document::from_json(&t).map_err(|e| e.to_string()))
            {
                Ok(doc) => {
                    self.open_document(doc, Some(path.clone()));
                    tabs.push(self.docs.active_index());
                }
                Err(e) => {
                    tabs.push(None);
                    failed.push(t!("status.open_failed", file = file_name(path), error = e));
                }
            }
        }
        if let Some(i) = shown.and_then(|k| tabs.get(k).copied().flatten()) {
            self.docs.select(i);
        }
        if let Some(m) = failed.into_iter().next() {
            self.message = Some(m);
        }
    }

    /// The page asks before it is left while a document is modified (web).
    #[cfg(target_arch = "wasm32")]
    fn ask_before_leaving(&self) {
        use wasm_bindgen::JsCast;
        let unsaved = self.unsaved.clone();
        let handler = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(move |e: web_sys::BeforeUnloadEvent| {
            if unsaved.get() {
                e.prevent_default();
                e.set_return_value("");
            }
        });
        if let Some(w) = web_sys::window() {
            w.set_onbeforeunload(Some(handler.as_ref().unchecked_ref()));
        }
        // For as long as the page lives.
        handler.forget();
    }

    /// Readies a document's editor: the tools' settings and the machinings' switch, the
    /// user's.
    fn setup(prefs: &Preferences) -> impl FnOnce(&mut Editor) + use<> {
        let (tools, machinings) = (prefs.tool_options.clone(), prefs.show_machinings);
        move |e: &mut Editor| {
            e.set_tool_settings(tools);
            e.set_machinings_switch(machinings);
        }
    }

    /// Opens `doc` (read from `path`) and shows it: its tab if that file is open already, the
    /// blank document's place, or a new tab.
    fn open_document(&mut self, doc: Document, path: Option<String>) {
        if let Some(i) = path.as_deref().and_then(|p| self.docs.find_path(p)) {
            self.docs.select(i);
            return;
        }
        match self.docs.open(doc, path, Self::setup(&self.prefs)) {
            Ok((_, Some(replaced))) => self.viewport.drop_document(replaced),
            Ok(_) => {}
            Err(_) => self.message = Some(t!("status.too_many_documents", max = crate::docs::MAX_OPEN)),
        }
    }

    /// Closes tab `i` now, without asking.
    fn close_now(&mut self, i: usize) {
        if let Some(doc) = self.docs.close(i) {
            self.viewport.drop_document(doc.id);
        }
    }

    /// Closes the documents `ids`, asking about the modified ones (docs/ARCHITECTURE.md §6
    /// octies). Nothing while tabs are being closed already.
    fn close_documents(&mut self, ids: Vec<DocId>) {
        if self.closing.is_none() {
            self.closing = Some(Closing { queue: ids, quit: false, asking: None, saving: None });
            self.advance_closing();
        }
    }

    /// Quits: asks about each modified document first (none closes meanwhile: Cancel leaves
    /// them as they were). The documents open are kept to be opened again at launch.
    fn request_quit(&mut self) {
        if self.closing.as_ref().is_some_and(|c| c.quit) {
            return;
        }
        let queue = self.docs.iter().filter(|d| d.modified()).map(|d| d.id).collect();
        self.closing = Some(Closing { queue, quit: true, asking: None, saving: None });
        self.advance_closing();
    }

    /// Goes on with the documents being closed: an unmodified one closes, a modified one is
    /// shown and asked about; when none is left, the app quits if it was quitting.
    fn advance_closing(&mut self) {
        loop {
            let Some(c) = &mut self.closing else { return };
            if c.asking.is_some() || c.saving.is_some() {
                return;
            }
            let Some(&id) = c.queue.first() else {
                let quit = c.quit;
                self.closing = None;
                if quit {
                    self.quit_ok = true;
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                return;
            };
            let quit = c.quit;
            match self.docs.index_of(id) {
                Some(i) if self.docs.get(i).is_some_and(|d| d.modified()) => {
                    c.asking = Some(id);
                    self.docs.select(i);
                    return;
                }
                found => {
                    c.queue.remove(0);
                    if let (Some(i), false) = (found, quit) {
                        self.close_now(i);
                    }
                }
            }
        }
    }

    /// The answer to the question about the document being closed.
    fn answer(&mut self, answer: Answer) {
        let Some(c) = &mut self.closing else { return };
        let Some(id) = c.asking.take() else { return };
        match answer {
            Answer::Cancel => self.closing = None,
            Answer::Save => {
                c.saving = Some(id);
                let ask = self.docs.index_of(id).and_then(|i| self.docs.get(i)).is_some_and(|d| d.path.is_none());
                self.save_document(id, ask);
            }
            Answer::DontSave => {
                c.queue.retain(|q| *q != id);
                if !c.quit
                    && let Some(i) = self.docs.index_of(id)
                {
                    self.close_now(i);
                }
            }
        }
        self.advance_closing();
    }

    /// Document `doc` was saved (`ok`) or not: one being closed then closes, if it is not
    /// modified again; else the closing stops.
    fn closing_saved(&mut self, doc: DocId, ok: bool) {
        let Some(c) = &mut self.closing else { return };
        if c.saving != Some(doc) {
            return;
        }
        c.saving = None;
        if !ok {
            self.closing = None;
            return;
        }
        // Modified since the save started: asked about again.
        if self.docs.index_of(doc).and_then(|i| self.docs.get(i)).is_some_and(|d| d.modified()) {
            self.advance_closing();
            return;
        }
        c.queue.retain(|q| *q != doc);
        if !c.quit
            && let Some(i) = self.docs.index_of(doc)
        {
            self.close_now(i);
        }
        self.advance_closing();
    }

    /// « Enregistrer les modifications de « X » ? », while a modified document is closing.
    fn save_prompt(&mut self, ctx: &egui::Context) {
        let Some(id) = self.closing.as_ref().and_then(|c| c.asking) else { return };
        let Some(name) = self.docs.index_of(id).and_then(|i| self.docs.get(i)).map(|d| d.name()) else {
            self.closing = None;
            return;
        };
        let mut chosen = None;
        let modal = egui::Modal::new(egui::Id::unique("save_prompt")).frame(modal_frame(ctx)).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.spacing_mut().button_padding = MODAL_BUTTON_PADDING;
            ui.heading(t!("save_prompt.title"));
            ui.add_space(6.0);
            ui.label(t!("save_prompt.question", name = name));
            ui.weak(t!("save_prompt.note"));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(t!("save_prompt.dont_save")).clicked() {
                    chosen = Some(Answer::DontSave);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = ui.add(egui::Button::new(t!("save_prompt.save")).fill(ui.visuals().selection.bg_fill));
                    if save.clicked() {
                        chosen = Some(Answer::Save);
                    }
                    if ui.button(t!("save_prompt.cancel")).clicked() {
                        chosen = Some(Answer::Cancel);
                    }
                });
            });
        });
        if ui_key(ctx, egui::Key::Enter) {
            chosen = chosen.or(Some(Answer::Save));
        }
        if modal.should_close() {
            chosen = chosen.or(Some(Answer::Cancel));
        }
        if let Some(a) = chosen {
            self.answer(a);
        }
    }

    /// The window over a document being loaded (docs/ARCHITECTURE.md §7 sexies): the file
    /// read, its preview meshes, its final meshes, each with how far it is, until the last final
    /// mesh; « Annuler » (or Esc) stops it all. Shown once a load has lasted [`LOAD_WINDOW_AFTER`]
    /// (a small file would only flash it).
    fn loading_window(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let loading: Vec<DocId> = self.docs.iter_mut().filter(|d| d.editor.loading().is_some()).map(|d| d.id).collect();
        self.loads_seen.retain(|id, _| loading.contains(id));
        for &id in &loading {
            self.loads_seen.entry(id).or_insert(now);
        }
        let Some(d) = self.docs.active() else { return };
        let (Some(load), Some(&since)) = (d.editor.loading().cloned(), self.loads_seen.get(&d.id)) else { return };
        if now - since < LOAD_WINDOW_AFTER {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(LOAD_WINDOW_AFTER - (now - since)));
            return;
        }
        let title = match &load.kind {
            LoadKind::Open => t!("loading.open", name = d.name()),
            LoadKind::Import { name } => t!("loading.import", name = name),
        };
        let row = |ui: &mut egui::Ui, label: String, done: usize, total: usize, current: bool| {
            ui.label(label);
            let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
            let bar = egui::ProgressBar::new(fraction).text(format!("{done} / {total}")).animate(current && done < total);
            ui.add(bar);
            ui.end_row();
        };
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("loading")).frame(modal_frame(ctx)).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.spacing_mut().button_padding = MODAL_BUTTON_PADDING;
            ui.heading(title);
            ui.add_space(8.0);
            egui::Grid::new("loading_stages").num_columns(2).spacing([12.0, 8.0]).min_col_width(90.0).show(ui, |ui| {
                match load.reading {
                    Some(Progress::Bodies { done, total }) => row(ui, t!("loading.bodies"), done as usize, total as usize, true),
                    Some(Progress::Reading) => {
                        ui.label(t!("loading.reading"));
                        ui.add(egui::Spinner::new());
                        ui.end_row();
                    }
                    None => {}
                }
                let (pd, pt) = load.previews;
                let (fd, ft) = load.finals;
                if load.reading.is_none() || pt > 0 {
                    row(ui, t!("loading.previews"), pd, pt, pd < pt);
                    row(ui, t!("loading.finals"), fd, ft, pd >= pt);
                }
            });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t!("loading.cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if modal.should_close() || ui_key(ctx, egui::Key::Escape) {
            cancel = true;
        }
        if cancel && let Some(d) = self.docs.active_mut() {
            d.editor.cancel_loading();
        }
    }

    /// The tabs' actions (docs/ARCHITECTURE.md §6 octies).
    fn tab_action(&mut self, a: TabAction) {
        let id = |docs: &Docs, i: usize| docs.get(i).map(|d| d.id);
        match a {
            TabAction::Select(i) => self.docs.select(i),
            TabAction::New => self.new_document(),
            TabAction::Reorder { from, to } => self.docs.move_tab(from, to),
            TabAction::Close(i) | TabAction::Menu(i, MenuRow::Close) => {
                if let Some(d) = id(&self.docs, i) {
                    self.close_documents(vec![d]);
                }
            }
            TabAction::Menu(i, MenuRow::CloseOthers) => {
                let keep = id(&self.docs, i);
                let others = self.docs.iter().map(|d| d.id).filter(|d| Some(*d) != keep).collect();
                self.close_documents(others);
            }
            TabAction::Menu(_, MenuRow::CloseAll) => {
                let all = self.docs.iter().map(|d| d.id).collect();
                self.close_documents(all);
            }
            TabAction::Menu(i, MenuRow::Reveal) => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(p) = self.docs.get(i).and_then(|d| d.path.clone()) {
                    reveal(&p);
                }
                #[cfg(target_arch = "wasm32")]
                let _ = i;
            }
        }
    }

    /// The tabs as the title bar draws them.
    fn tabs(&self) -> Vec<Tab> {
        self.docs.iter().map(|d| Tab { id: d.id, name: d.name(), path: d.path.clone(), modified: d.modified() }).collect()
    }

    /// The menus are the native ones (macOS desktop).
    fn native_menus(&self) -> bool {
        #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
        return self.native_menu.is_some();
        #[cfg(not(all(target_os = "macos", not(target_arch = "wasm32"))))]
        false
    }

    /// A command chosen in the native menu bar. AppKit takes its ⌘ shortcut before egui sees
    /// the key, so it goes back to egui as that key while a shortcut is being captured.
    #[cfg_attr(not(all(target_os = "macos", not(target_arch = "wasm32"))), allow(dead_code))]
    fn native_command(&mut self, id: &'static str, actions: &mut Vec<Action>) {
        if let Some(s) = self.keymap.get(id)
            && self.capturing.is_some()
        {
            let modifiers = egui::Modifiers { alt: s.alt, shift: s.shift, mac_cmd: s.command, command: s.command, ctrl: false };
            for pressed in [true, false] {
                self.pending_keys.push(egui::Event::Key { key: s.key, physical_key: None, pressed, repeat: false, modifiers });
            }
            self.ctx.request_repaint();
        } else {
            actions.push(Action::Command(id));
        }
    }

    /// Changes the shown document's settings (an undoable step).
    fn set_settings(&mut self, settings: Settings) {
        if let Some(d) = self.docs.active_mut()
            && &settings != d.editor.document().settings()
        {
            let _ = d.editor.apply(Operation::SetSettings(settings));
        }
    }

    // ---- commands ------------------------------------------------------------------------------

    /// Can `id` run now?
    fn enabled(&self, id: &str) -> bool {
        // Without a document (the start page): only what opens one, and the app's own.
        let Some(d) = self.docs.active() else {
            return match id {
                "file.new" | "file.open" | "file.import" | "app.command_palette" | "app.preferences" | "view.render_stats" => true,
                "app.quit" | "app.remote_control" => !cfg!(target_arch = "wasm32"),
                _ if let Some(k) = commands::recent_slot(id) => k < self.prefs.recent_files.len(),
                _ => PanelId::from_command(id).is_some(),
            };
        };
        let e = &d.editor;
        let has_sel = !e.selection().is_empty();
        let section = e.document().settings().display.section.is_some();
        match id {
            "edit.undo" => e.history().can_undo(),
            "edit.redo" => e.history().can_redo(),
            "edit.erase" | "edit.make_assembly" | "edit.explode" | "edit.make_unique" => has_sel,
            "edit.cut" | "edit.copy" => e.can_copy(),
            "edit.paste" | "edit.paste_in_place" => self.clipboard.as_ref().is_some_and(|c| e.can_paste(&c.clip)),
            "view.section" | "view.section_clear" => section,
            "view.show_all" => e.has_hidden(),
            "edit.delete_guides" => e.document().component(e.guide_component()).is_ok_and(|c| !c.guides.is_empty()),
            "debug.crash_kernel" => self.debug_menu,
            "view.next_tab" | "view.previous_tab" => self.docs.len() > 1,
            "app.quit" | "app.remote_control" => !cfg!(target_arch = "wasm32"),
            _ if let Some(k) = commands::recent_slot(id) => k < self.prefs.recent_files.len(),
            _ => commands::find(id).is_some(),
        }
    }

    /// A toggle's state, or which tool is on.
    fn checked(&self, id: &str) -> Option<bool> {
        match id {
            id if let Some(p) = PanelId::from_command(id) => return Some(self.prefs.panels.is_shown(p)),
            "view.render_stats" => return Some(self.prefs.show_render_stats),
            #[cfg(not(target_arch = "wasm32"))]
            "app.remote_control" => return Some(self.remote.is_on()),
            _ => {}
        }
        let editor = &self.docs.active()?.editor;
        let d = &editor.document().settings().display;
        match id {
            "view.parallel" => Some(editor.camera.is_parallel()),
            "view.grid" => Some(d.grid),
            "view.axes" => Some(d.axes),
            "view.guides" => Some(d.guides),
            "view.silhouettes" => Some(d.silhouettes),
            "view.xray" => Some(editor.xray()),
            "view.hide_rest_of_model" => Some(d.hide_rest_of_model),
            "view.show_hidden" => Some(d.show_hidden),
            "view.section" => Some(d.section_active),
            _ => tool_of(id).map(|k| editor.tool_kind() == k),
        }
    }

    /// Its label now (« Annuler Déplacer »).
    fn label(&self, c: &commands::Command) -> String {
        let h = self.docs.active().map(|d| d.editor.history());
        let h = h.as_ref();
        match c.id {
            "edit.undo" => h.and_then(|h| h.undo_op()).map_or_else(|| (c.label)(), |o| t!("menu.edit.undo_op", op = o.label())),
            "edit.redo" => h.and_then(|h| h.redo_op()).map_or_else(|| (c.label)(), |o| t!("menu.edit.redo_op", op = o.label())),
            id if let Some(f) = commands::recent_slot(id).and_then(|k| self.prefs.recent_files.get(k)) => file_name(f).to_owned(),
            // In the macOS app menu: « Quitter SolidSketch ».
            "app.quit" if self.native_menus() => t!("menu.app.quit", app = "SolidSketch"),
            _ => (c.label)(),
        }
    }

    fn mac(&self) -> bool {
        self.ctx.os() == egui::os::OperatingSystem::Mac
    }

    /// Every command as the menus, the tool bar and the palette show it now.
    fn command_views(&self) -> Vec<CommandView> {
        let mac = self.mac();
        COMMANDS
            .iter()
            // An empty recent file slot is not shown; nor Quit on the web (the page is closed).
            .filter(|c| commands::recent_slot(c.id).is_none_or(|k| k < self.prefs.recent_files.len()))
            .filter(|c| c.id != "app.quit" || !cfg!(target_arch = "wasm32"))
            .map(|c| CommandView {
                id: c.id,
                label: self.label(c),
                menu: c.menu,
                group: c.group,
                enabled: self.enabled(c.id),
                checked: self.checked(c.id),
                shortcut: self.keymap.get(c.id).map(|s| s.display(mac)),
            })
            .collect()
    }

    /// Runs the command `id`.
    fn run(&mut self, id: &str) -> Result<(), String> {
        if commands::find(id).is_none() {
            return Err(t!("status.command_unknown", id = id));
        }
        if !self.enabled(id) {
            return Err(t!("status.command_unavailable", id = id));
        }
        if let Some(k) = commands::recent_slot(id) {
            return self.open_recent(k);
        }
        // What needs no document.
        let done = match id {
            "file.new" => {
                self.new_document();
                true
            }
            "file.open" => {
                file::open(&self.inbox, &self.ctx);
                true
            }
            "file.import" => {
                file::import(&self.inbox, &self.ctx);
                true
            }
            "file.save" | "file.save_as" => {
                self.save(id == "file.save_as");
                true
            }
            "file.close" => {
                let shown = self.docs.active().map(|d| d.id);
                self.close_documents(shown.into_iter().collect());
                true
            }
            "app.quit" => {
                self.request_quit();
                true
            }
            "view.next_tab" | "view.previous_tab" => {
                self.docs.cycle(id == "view.next_tab");
                true
            }
            "app.command_palette" => {
                self.palette = Some(Palette::default());
                true
            }
            "app.preferences" => {
                let first = if self.docs.active().is_some() { PrefsTab::Model } else { PrefsTab::General };
                self.prefs_window = Some(self.prefs_window.unwrap_or(first));
                true
            }
            id if let Some(p) = PanelId::from_command(id) => {
                self.prefs.panels.toggle(p);
                true
            }
            "view.render_stats" => {
                self.prefs.show_render_stats = !self.prefs.show_render_stats;
                true
            }
            #[cfg(not(target_arch = "wasm32"))]
            "app.remote_control" => {
                self.set_remote(!self.remote.is_on());
                true
            }
            "edit.cut" | "edit.copy" => {
                self.copy(id == "edit.cut");
                true
            }
            "edit.paste" | "edit.paste_in_place" => {
                self.paste(id == "edit.paste_in_place");
                true
            }
            _ => false,
        };
        if done {
            return Ok(());
        }
        let Some(doc) = self.docs.active_mut() else {
            return Err(t!("status.command_unavailable", id = id));
        };
        if let Some(k) = tool_of(id) {
            doc.vcb.clear();
            doc.editor.set_tool(k);
            return Ok(());
        }
        if let Some(v) = view_of(id) {
            doc.editor.standard_view(v);
            return Ok(());
        }
        let mut s = doc.editor.document().settings().clone();
        let e = &mut doc.editor;
        match id {
            "edit.undo" => e.undo(),
            "edit.redo" => e.redo(),
            "edit.select_all" => e.select_all(),
            "edit.erase" => e.delete_selection(),
            "edit.make_assembly" => e.make_assembly(),
            "edit.explode" => e.explode(),
            "edit.make_unique" => e.make_unique(),
            "edit.delete_guides" => e.delete_guides(),
            "view.parallel" => {
                let p = !e.camera.is_parallel();
                e.set_parallel(p);
            }
            "view.xray" => e.set_xray(!e.xray()),
            "view.grid"
            | "view.axes"
            | "view.guides"
            | "view.silhouettes"
            | "view.hide_rest_of_model"
            | "view.show_hidden"
            | "view.section"
            | "view.section_clear" => {
                let d = &mut s.display;
                match id {
                    "view.grid" => d.grid = !d.grid,
                    "view.guides" => d.guides = !d.guides,
                    "view.axes" => d.axes = !d.axes,
                    "view.silhouettes" => d.silhouettes = !d.silhouettes,
                    "view.hide_rest_of_model" => d.hide_rest_of_model = !d.hide_rest_of_model,
                    "view.show_hidden" => d.show_hidden = !d.show_hidden,
                    "view.section" => d.section_active = !d.section_active,
                    _ => {
                        d.section = None;
                        d.section_active = false;
                    }
                }
                self.set_settings(s);
            }
            "view.zoom_extents" => e.zoom_extents(),
            "view.show_all" => e.show_all(),
            "debug.crash_kernel" => e.crash_kernel(),
            _ => return Err(t!("status.command_unknown", id = id)),
        }
        Ok(())
    }

    /// Copies the shown document's selection (and erases it, for `cut`) into the app's
    /// clipboard; the system clipboard gets its token.
    fn copy(&mut self, cut: bool) {
        let Some(d) = self.docs.active_mut() else { return };
        // The state copied from: the one before the erase, for a cut.
        let revision = d.editor.history().revision();
        let Some(clip) = (if cut { d.editor.cut() } else { d.editor.copy() }) else { return };
        self.copies += 1;
        let c = Clipboard::new(clip, d.id, revision, self.copies);
        self.ctx.copy_text(c.token.clone());
        self.clipboard = Some(c);
    }

    /// Pastes the app's clipboard into the shown document: at the cursor, or `in_place`
    /// (where it was copied from).
    fn paste(&mut self, in_place: bool) {
        let (Some(c), Some(d)) = (&self.clipboard, self.docs.active_mut()) else { return };
        let reuse = c.reuse(d.id, |r| d.editor.history().holds(r));
        d.vcb.clear();
        if in_place {
            d.editor.paste_in_place(&c.clip, reuse);
        } else {
            d.editor.paste(&c.clip, reuse);
        }
    }

    fn act(&mut self, a: Action) {
        match a {
            Action::Command(id) => {
                if let Err(m) = self.run(id) {
                    self.message = Some(m);
                }
                return;
            }
            Action::Tab(t) => return self.tab_action(t),
            #[cfg(not(target_arch = "wasm32"))]
            Action::RemoteRetry => return self.set_remote(true),
            #[cfg(target_arch = "wasm32")]
            Action::RemoteRetry => return,
            Action::Ribbon(change) => return self.prefs.ribbon.apply(change),
            Action::QuickPin(id) => return crate::quick::toggle(&mut self.prefs.quick_tools, id),
            Action::QuickTool(id) => {
                if let (Some(k), Some(d)) = (tool_of(id), self.docs.active_mut()) {
                    d.vcb.clear();
                    d.editor.start_quick_tool(k);
                }
                return;
            }
            Action::ToggleMachinings => {
                self.prefs.show_machinings = !self.prefs.show_machinings;
                for d in self.docs.iter_mut() {
                    d.editor.set_machinings_switch(self.prefs.show_machinings);
                }
                return;
            }
            _ => {}
        }
        let Some(doc) = self.docs.active_mut() else { return };
        let e = &mut doc.editor;
        match a {
            Action::Command(_)
            | Action::Tab(_)
            | Action::Ribbon(_)
            | Action::QuickPin(_)
            | Action::QuickTool(_)
            | Action::ToggleMachinings
            | Action::RemoteRetry => {}
            Action::SelectPath(p, add) => e.toggle_path(&p, add),
            Action::SetHidden(p, hidden) => e.set_hidden(&p, hidden),
            Action::Apply(op) => {
                let _ = e.apply(op);
            }
            Action::ToolOption(key, value) => e.set_tool_option(key, value),
            Action::ToggleProfile => {
                if e.tool_kind() == ToolKind::Profile {
                    e.set_tool(ToolKind::Select);
                } else {
                    e.edit_profile();
                }
            }
            Action::ToolLength(key, text) => e.type_tool_length(key, &text),
            Action::PartScope(s) => e.set_part_scope(s),
            Action::BodySign(s) => e.set_body_sign(s),
            Action::SelectMachinings(ids) => e.select(ids),
            Action::HoverMachining(m) => e.set_listed_hover(m),
            // The view is drawn before the panels: again, for the hover to show this frame.
            Action::HoverOutliner(p) => {
                if e.set_outliner_hover(p) {
                    self.ctx.request_repaint();
                }
            }
            Action::PaintWith(m) => {
                e.set_paint_material(m);
                if e.tool_kind() != ToolKind::Paint {
                    doc.vcb.clear();
                    e.set_tool(ToolKind::Paint);
                }
            }
        }
    }

    /// A new document, in a tab of its own (or the blank one's place).
    fn new_document(&mut self) {
        self.open_document(Document::with_settings(self.prefs.new_model.clone()), None);
    }

    /// Opens the recent file `k`; one that cannot be read leaves the list.
    fn open_recent(&mut self, k: usize) -> Result<(), String> {
        let Some(path) = self.prefs.recent_files.get(k).cloned() else {
            return Err(t!("status.command_unavailable", id = format!("file.recent.{}", k + 1)));
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(e) = file::open_path(&path, &self.inbox) {
            self.prefs.recent_files.retain(|p| *p != path);
            return Err(t!("status.open_failed", file = file_name(&path), error = e));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = path;
        Ok(())
    }

    /// Remembers `path` in the recent files (desktop: the browser gives only a name).
    fn add_recent_file(&mut self, path: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        self.prefs.add_recent_file(path);
        #[cfg(target_arch = "wasm32")]
        let _ = path;
    }

    /// Saves the shown document (`ask`: where, whatever its file).
    fn save(&mut self, ask: bool) {
        if let Some(id) = self.docs.active().map(|d| d.id) {
            self.save_document(id, ask);
        }
    }

    /// Saves document `id`: to its file, else (or if `ask`) where the user says.
    fn save_document(&mut self, id: DocId, ask: bool) {
        let Some(d) = self.docs.iter().find(|d| d.id == id) else { return };
        let path = if ask { None } else { d.path.clone() };
        // The name offered: its file's, else the default one.
        let name = d.path.as_deref().map(file_name).map(|n| n.trim_end_matches(&format!(".{}", file::EXTENSION)).to_owned());
        let name = name.unwrap_or_else(|| t!("default_name.file"));
        file::save(id, d.editor.document(), d.editor.history().revision(), path.as_deref(), &name, &self.inbox, &self.ctx);
    }

    /// A modifier tapped alone goes to the tool: the copy modifier (Option on macOS, Ctrl
    /// elsewhere) toggles copying (SketchUp; the box: its base centred, or its height), Shift
    /// (with the copy modifier held, a shape tool takes the whole face) and the other modifier
    /// (Command on macOS, Alt elsewhere: a shape tool measures from a corner). Told on release,
    /// and only if no key, click or scroll, nor another modifier pressed, came in between: Ctrl+Z
    /// and the like are left alone, and so is the copy modifier held while Shift is tapped.
    fn modifier_taps(&mut self, ctx: &egui::Context) {
        let (mods, events) = ctx.input(|i| (i.modifiers, i.events.iter().any(interrupts_tap)));
        let other_key = if ctx.os() == egui::os::OperatingSystem::Mac { mods.mac_cmd } else { mods.alt };
        let down = [viewport::modifiers(ctx, mods).copy, mods.shift, other_key];
        let pressed: Vec<bool> = down.iter().zip(&self.taps).map(|(d, t)| *d && !t.down).collect();
        for (k, key) in [Key::Copy, Key::Shift, Key::Alt].into_iter().enumerate() {
            let other = events || pressed.iter().enumerate().any(|(j, p)| j != k && *p);
            if self.taps[k].feed(down[k], other)
                && !ctx.egui_wants_keyboard_input()
                && let Some(d) = self.docs.active_mut()
            {
                d.editor.input(ToolInput::Key { key, mods: Default::default() });
            }
        }
    }

    /// Keyboard: the tool's keys, the measurements box fed from anywhere (SketchUp), then the
    /// commands' shortcuts.
    fn keyboard(&mut self, ctx: &egui::Context) {
        self.modifier_taps(ctx);
        if ctx.egui_wants_keyboard_input() || self.capturing.is_some() || self.palette.is_some() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            if let Some(id) = self.key_event(ctx, ev) {
                self.act(Action::Command(id));
            }
        }
    }

    /// One keyboard event: to the shown document's tool or measurements box, or the command its
    /// shortcut runs (given back).
    fn key_event(&mut self, ctx: &egui::Context, ev: egui::Event) -> Option<&'static str> {
        // The clipboard's keys come as these (desktop and web alike), not as shortcuts; done
        // only when they can be, without a word otherwise. ⌘V pastes only while the system
        // clipboard still holds the app's token (§6 decies).
        let clipboard = match &ev {
            egui::Event::Cut => Some("edit.cut"),
            egui::Event::Copy => Some("edit.copy"),
            egui::Event::Paste(text) if self.clipboard.as_ref().is_some_and(|c| c.is_on_system_clipboard(text)) => Some("edit.paste"),
            _ => None,
        };
        if let Some(id) = clipboard {
            return self.enabled(id).then_some(id);
        }
        let egui::Event::Key { key, pressed: true, modifiers, .. } = ev else {
            if let egui::Event::Text(t) = ev
                && let Some(d) = self.docs.active_mut()
            {
                // `*5` and `/5` (arrays), `@` (the current value), `(`, and points `[x;y;z]`,
                // `<dx;dy;dz>` start a value too.
                let starts_value = t.starts_with(|c: char| {
                    c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ',' | '*' | '/' | '@' | '(' | '[' | '<' | ';')
                });
                if d.editor.takes_typing() && (!d.vcb.is_empty() || starts_value) {
                    d.vcb.push_str(&t);
                }
            }
            return None;
        };
        use egui::Key as K;
        // The tabs' fixed keys, whatever the shortcuts (docs/ARCHITECTURE.md §6 octies): Ctrl+Tab
        // (Ctrl on macOS too) and ⌘T / Ctrl+T beside ⌘N.
        if key == K::Tab && modifiers.ctrl {
            return Some(if modifiers.shift { "view.previous_tab" } else { "view.next_tab" });
        }
        if key == K::T && modifiers.command && !modifiers.shift && !modifiers.alt {
            return Some("file.new");
        }
        // The web sends these as keys too, before its clipboard events: done by those.
        if modifiers.command_only() && matches!(key, K::X | K::C | K::V) {
            return None;
        }
        let command = || self.keymap.command_for(key, modifiers);
        let Some(d) = self.docs.active_mut() else { return command() };
        let typing = !d.vcb.is_empty();
        let m = viewport::modifiers(ctx, modifiers);
        let tool_key = |k: Key| ToolInput::Key { key: k, mods: m };
        let plain = !(modifiers.command || modifiers.alt || modifiers.ctrl);
        let e = &mut d.editor;
        match key {
            K::Enter if typing => {
                let text = std::mem::take(&mut d.vcb);
                e.input(ToolInput::Vcb(text));
            }
            K::Backspace if typing => {
                d.vcb.pop();
            }
            K::Escape if typing => d.vcb.clear(),
            _ if typing => {}
            K::Escape => e.input(tool_key(Key::Escape)),
            K::Delete | K::Backspace if plain => e.input(tool_key(Key::Delete)),
            K::ArrowLeft => e.input(tool_key(Key::Left)),
            K::ArrowRight => e.input(tool_key(Key::Right)),
            K::ArrowUp => e.input(tool_key(Key::Up)),
            K::ArrowDown => e.input(tool_key(Key::Down)),
            K::Tab => e.input(tool_key(Key::Tab)),
            // Enter with nothing typed: the tool's (the prism tool closes its outline).
            K::Enter => e.input(tool_key(Key::Enter)),
            _ => return self.keymap.command_for(key, modifiers),
        }
        None
    }

    /// The command palette (Cmd/Ctrl+K): type to filter, ↑↓ to pick, Entrée to run.
    fn palette_window(&mut self, ctx: &egui::Context, views: &[CommandView], actions: &mut Vec<Action>) {
        let Some(p) = &mut self.palette else { return };
        let found: Vec<&CommandView> = views
            .iter()
            .filter(|v| v.enabled && (v.menu != Menu::Debug || self.debug_menu))
            .filter(|v| commands::find(v.id).is_some_and(|c| commands::matches(&p.query, c)))
            .collect();
        // ↑↓ taken before the query field sees them: they pick a row, never move its cursor.
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if down {
            p.picked = (p.picked + 1).min(found.len().saturating_sub(1));
        }
        if up {
            p.picked = p.picked.saturating_sub(1);
        }
        p.picked = p.picked.min(found.len().saturating_sub(1));
        let mut run = None;
        let mut open = !escape;
        let top = self.ribbon_bottom.map_or(80.0, |b| (b - ctx.content_rect().top() + 8.0).max(80.0));
        // A fixed id: the title changes with the language.
        egui::Window::new(t!("palette.title"))
            .id(egui::Id::unique("command_palette"))
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .fixed_size([600.0, 0.0])
            // Below the ribbon when a document is shown, never over it.
            .anchor(egui::Align2::CENTER_TOP, [0.0, top])
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut p.query)
                        .hint_text(ellipsis(t!("palette.hint")))
                        .margin(egui::vec2(8.0, 6.0))
                        .desired_width(f32::INFINITY),
                );
                // Only when lost: asked every frame, it would reset the field's key filter (the
                // arrows would then move the focus to a row for a frame) and break dead keys.
                if !r.has_focus() {
                    r.request_focus();
                }
                if r.changed() {
                    p.picked = 0;
                    p.at_top = false;
                }
                ui.separator();
                let mut list = egui::ScrollArea::vertical().max_height(480.0).min_scrolled_height(480.0);
                if !std::mem::replace(&mut p.at_top, true) {
                    list = list.vertical_scroll_offset(0.0);
                }
                list.show(ui, |ui| {
                    // In menu order, each menu's commands under its name.
                    let mut header = None;
                    for (k, v) in found.iter().enumerate() {
                        if k == 0 || found[k - 1].menu != v.menu {
                            if k > 0 {
                                ui.add_space(6.0);
                            }
                            header = Some(palette_header(ui, v.menu));
                        }
                        let row = palette_row(ui, v, k == p.picked).on_hover_text(v.id);
                        if k == p.picked && (up || down) {
                            // A section's first row brings its name along.
                            let first = k == 0 || found[k - 1].menu != v.menu;
                            let rect = match header {
                                Some(h) if first => h.union(row.rect),
                                _ => row.rect,
                            };
                            ui.scroll_to_rect(rect, None);
                        }
                        if row.clicked() {
                            run = Some(v.id);
                        }
                    }
                    if found.is_empty() {
                        ui.weak(t!("palette.none"));
                    }
                });
            });
        if enter {
            run = run.or_else(|| found.get(p.picked).map(|v| v.id));
        }
        if let Some(id) = run {
            open = false;
            actions.push(Action::Command(id));
        }
        if !open {
            self.palette = None;
        }
    }

    /// The preferences (⌘,): the language; the model's display and units, saved with it,
    /// which can become the defaults of new models; the navigation, the interface and the
    /// shortcuts, the user's own.
    fn preferences_window(&mut self, ctx: &egui::Context) {
        let Some(mut tab) = self.prefs_window else { return };
        let mut open = true;
        // A fixed id: the title changes with the language.
        egui::Window::new(t!("prefs.title"))
            .id(egui::Id::unique("preferences"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            // Pinned by its title bar: changing tabs moves only its bottom edge.
            .pivot(egui::Align2::CENTER_TOP)
            .default_pos({
                let r = ctx.content_rect();
                egui::pos2(r.center().x, r.top() + r.height() * 0.15)
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut tab, PrefsTab::General, t!("prefs.tab.general"));
                    ui.selectable_value(&mut tab, PrefsTab::Model, t!("prefs.tab.model"));
                    ui.selectable_value(&mut tab, PrefsTab::Navigation, t!("prefs.tab.navigation"));
                    ui.selectable_value(&mut tab, PrefsTab::Interface, t!("prefs.tab.interface"));
                    ui.selectable_value(&mut tab, PrefsTab::Shortcuts, t!("prefs.tab.shortcuts"));
                    if !cfg!(target_arch = "wasm32") {
                        ui.selectable_value(&mut tab, PrefsTab::Debug, t!("prefs.tab.debug"));
                    }
                });
                ui.separator();
                match tab {
                    PrefsTab::General => general_settings(ui, &mut self.prefs),
                    #[cfg(not(target_arch = "wasm32"))]
                    PrefsTab::Debug => self.debug_settings(ui),
                    #[cfg(target_arch = "wasm32")]
                    PrefsTab::Debug => {}
                    PrefsTab::Model => self.model_settings(ui),
                    PrefsTab::Navigation => navigation_settings(ui, &mut self.prefs.navigation),
                    PrefsTab::Interface => interface_settings(ui, &mut self.prefs),
                    PrefsTab::Shortcuts => self.shortcut_settings(ui),
                }
            });
        if tab != PrefsTab::Shortcuts || !open {
            self.capturing = None;
        }
        self.prefs_window = open.then_some(tab);
    }

    /// The control channel: its switch, showing it on whatever started it, and its port, set
    /// once typed or dragged to its value (not at each step).
    #[cfg(not(target_arch = "wasm32"))]
    fn debug_settings(&mut self, ui: &mut egui::Ui) {
        let mut on = self.remote.is_on();
        let mut port_set = false;
        egui::Grid::new("debug").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(t!("prefs.debug.remote_control")).on_hover_text(t!("prefs.debug.remote_control_tip"));
            ui.checkbox(&mut on, "");
            ui.end_row();
            ui.label(t!("prefs.debug.remote_port"));
            let r = ui.add(egui::DragValue::new(&mut self.prefs.remote_port).range(1024..=65535).speed(1.0));
            port_set = r.drag_stopped() || r.lost_focus();
            ui.end_row();
        });
        if on != self.remote.is_on() {
            self.set_remote(on);
        }
        if port_set && let Err(e) = self.remote.set_port(self.prefs.remote_port) {
            self.message = Some(t!("status.control_failed", error = e));
        }
    }

    fn model_settings(&mut self, ui: &mut egui::Ui) {
        let Some(current) = self.docs.active().map(|d| d.editor.document().settings().clone()) else {
            ui.weak(t!("prefs.model.no_document"));
            return;
        };
        let mut s = current.clone();
        ui.weak(t!("prefs.model.intro"));
        ui.add_space(4.0);
        egui::Grid::new("model_settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.strong(t!("prefs.model.display"));
            ui.end_row();
            ui.label(t!("prefs.model.grid"));
            ui.checkbox(&mut s.display.grid, "");
            ui.end_row();
            ui.label(t!("prefs.model.axes"));
            ui.checkbox(&mut s.display.axes, "");
            ui.end_row();
            ui.label(t!("prefs.model.guides"));
            ui.checkbox(&mut s.display.guides, "");
            ui.end_row();
            ui.label(t!("prefs.model.silhouettes"));
            ui.checkbox(&mut s.display.silhouettes, "").on_hover_text(t!("prefs.model.silhouettes_tip"));
            ui.end_row();
            ui.label(t!("prefs.model.hide_rest_of_model"));
            ui.checkbox(&mut s.display.hide_rest_of_model, "").on_hover_text(t!("prefs.model.hide_rest_of_model_tip"));
            ui.end_row();
            ui.strong(t!("prefs.model.units"));
            ui.end_row();
            ui.label(t!("prefs.model.lengths"));
            egui::ComboBox::from_id_salt("length_unit").selected_text(s.units.length.name()).show_ui(ui, |ui| {
                for u in LengthUnit::ALL {
                    ui.selectable_value(&mut s.units.length, u, u.name());
                }
            });
            ui.end_row();
            ui.label(t!("prefs.model.decimals"));
            ui.add(egui::Slider::new(&mut s.units.decimals, 0..=6));
            ui.end_row();
            ui.label(t!("prefs.model.unit_symbol"));
            ui.checkbox(&mut s.units.symbol, "").on_hover_text(t!("prefs.model.unit_symbol_tip"));
            ui.end_row();
            ui.label(t!("prefs.model.example"));
            ui.weak(s.units.format(1234.5678));
            ui.end_row();
        });
        if s != current {
            self.set_settings(s.clone());
        }
        ui.add_space(8.0);
        // The section is the model's, not a default for new ones.
        let without_section = |mut s: Settings| {
            s.display.section = None;
            s.display.section_active = false;
            s
        };
        let defaults = without_section(s.clone()) == self.prefs.new_model;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!defaults, egui::Button::new(t!("prefs.model.use_for_new")))
                .on_hover_text(t!("prefs.model.use_for_new_tip"))
                .clicked()
            {
                self.prefs.new_model = without_section(s.clone());
            }
            if ui
                .add_enabled(!defaults, egui::Button::new(t!("prefs.model.restore")))
                .on_hover_text(t!("prefs.model.restore_tip"))
                .clicked()
            {
                let mut d = self.prefs.new_model.clone();
                d.display.section = s.display.section;
                d.display.section_active = s.display.section_active;
                self.set_settings(d);
            }
        });
        if defaults {
            ui.weak(t!("prefs.model.are_defaults"));
        }
    }

    /// Raccourcis: every command with its shortcut; « Modifier » waits for the next key
    /// (Échap cancels, Retour arrière removes it).
    fn shortcut_settings(&mut self, ui: &mut egui::Ui) {
        let mac = self.mac();
        if let Some(id) = self.capturing {
            let pressed = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            match pressed {
                Some((egui::Key::Escape, _)) => self.capturing = None,
                Some((egui::Key::Backspace | egui::Key::Delete, m)) if !m.any() => {
                    self.prefs.shortcuts.insert(id.to_owned(), String::new());
                    self.capturing = None;
                }
                Some((key, m)) => {
                    let s = Shortcut { key, command: m.command, shift: m.shift, alt: m.alt };
                    self.prefs.shortcuts.insert(id.to_owned(), s.text());
                    self.capturing = None;
                }
                None => {}
            }
        }
        ui.weak(t!("prefs.shortcuts.intro"));
        ui.add_space(4.0);
        let mut reset = None;
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            egui::Grid::new("shortcuts").num_columns(3).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
                for c in COMMANDS.iter().filter(|c| c.menu != Menu::Debug) {
                    ui.label((c.label)()).on_hover_text(c.id);
                    let current = self.keymap.get(c.id);
                    let mut text = current.map_or("—".to_owned(), |s| s.display(mac));
                    if let Some(s) = current
                        && !self.keymap.conflicts(c.id, s).is_empty()
                    {
                        text.push_str("  ⚠");
                    }
                    if self.capturing == Some(c.id) {
                        ui.strong("…");
                    } else {
                        ui.label(text);
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button(t!("prefs.shortcuts.edit")).clicked() {
                            self.capturing = Some(c.id);
                        }
                        if self.prefs.shortcuts.contains_key(c.id) && ui.small_button(t!("prefs.shortcuts.default")).clicked() {
                            reset = Some(c.id);
                        }
                    });
                    ui.end_row();
                }
            });
        });
        if let Some(id) = reset {
            self.prefs.shortcuts.remove(id);
        }
    }

    /// The side panels, as cards in their columns ([`dock`]), the one dragged by its header
    /// following the pointer. Gives the view's span left free between the columns (x).
    /// The docked cards, in columns from `top` (under the ribbon) to the bottom of the view.
    fn side_panels(&mut self, ctx: &egui::Context, view: egui::Rect, top: f32, margin: f32, actions: &mut Vec<Action>) -> egui::Rangef {
        let style = ctx.global_style();
        let frame = egui::Frame::window(&style).inner_margin(6.0).total_margin().sum();
        let header = style.spacing.interact_size.y;
        let metrics = dock::Metrics { header: header + frame.y, min_height: header + frame.y + 60.0, gap: margin };
        let max_width = (view.width() * 0.4).max(dock::COLUMN_MIN_WIDTH);
        let pointer = ctx.pointer_latest_pos();
        let dragged = self.panel_drag.map(|d| d.id);
        let room = view.bottom() - margin - top;
        let mut event = None;
        let mut outliner_shown = false;
        let mut columns = Vec::new();
        for side in [Dock::Left, Dock::Right] {
            let width = self.prefs.panels.column(side).width.clamp(dock::COLUMN_MIN_WIDTH, max_width);
            let x = match side {
                Dock::Left => view.left() + margin,
                Dock::Right => view.right() - margin - width,
            };
            let placed = dock::place(self.prefs.panels.column(side), top, room, metrics, dragged);
            for p in &placed {
                let rect = egui::Rect::from_min_size(egui::pos2(x, p.top), egui::vec2(width, p.height));
                // Every card but the one filling the column has a bottom edge to drag.
                let edges = Edges { side: Some(side), bottom: !p.fills, max_width, min_height: metrics.min_height };
                if let Some(e) = self.panel_card(ctx, p.id, rect, header, edges, &mut outliner_shown, actions) {
                    event = Some(e);
                }
            }
            columns.push((side, x, width, placed));
        }
        if let (Some(d), Some(p)) = (self.panel_drag, pointer) {
            let rect = egui::Rect::from_min_size(p - d.offset, d.size);
            let edges = Edges { side: None, bottom: false, max_width, min_height: metrics.min_height };
            if let Some(e) = self.panel_card(ctx, d.id, rect, header, edges, &mut outliner_shown, actions) {
                event = Some(e);
            }
        }
        if !outliner_shown {
            actions.push(Action::HoverOutliner(None));
        }
        // Where a dragged card goes: the column of the nearer edge, above the first card whose
        // middle is below the pointer.
        let target = |p: egui::Pos2| {
            let side = if p.x < view.center().x { Dock::Left } else { Dock::Right };
            let (_, x, width, placed) = columns.iter().find(|c| c.0 == side).expect("both columns");
            let before = placed.iter().find(|q| q.top + q.height / 2.0 > p.y);
            (side, *x, *width, placed, before)
        };
        match event {
            Some(CardEvent::Grabbed(drag)) => self.panel_drag = Some(drag),
            Some(CardEvent::Dropped) => {
                if let (Some(d), Some(p)) = (self.panel_drag.take(), pointer) {
                    let (side, .., before) = target(p);
                    self.prefs.panels.move_card(d.id, side, before.map(|q| q.id));
                }
            }
            None => {
                if let (Some(_), Some(p)) = (self.panel_drag, pointer) {
                    // Shown under the dragged card: the whole column if empty, else a bar
                    // where the card will be inserted.
                    let (_, x, width, placed, before) = target(p);
                    let accent = style.visuals.selection.bg_fill;
                    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("panel_drop")));
                    let x_range = egui::Rangef::new(x, x + width);
                    match (before, placed.last()) {
                        (_, None) => {
                            let r = egui::Rect::from_x_y_ranges(x_range, top..=view.bottom() - margin);
                            painter.rect(r, 6.0, accent.gamma_multiply(0.25), egui::Stroke::new(1.5, accent), egui::StrokeKind::Inside);
                        }
                        (b, Some(last)) => {
                            let y = b.map_or(last.top + last.height + margin / 2.0, |q| q.top - margin / 2.0);
                            let r = egui::Rect::from_x_y_ranges(x_range, y - 2.0..=y + 2.0);
                            painter.rect_filled(r, 2.0, accent);
                        }
                    }
                }
            }
        }
        // A dragged card still holds its room in its column for the rest of the layout.
        let left = if self.prefs.panels.occupied(Dock::Left, None) { columns[0].1 + columns[0].2 + margin } else { view.left() };
        let right = if self.prefs.panels.occupied(Dock::Right, None) { columns[1].1 - margin } else { view.right() };
        egui::Rangef::new(left, right)
    }

    /// One side panel in `rect`: its header (click: fold, drag: move it) and its content, and
    /// the edges `edges` gives to drag (width of its column, its height).
    #[allow(clippy::too_many_arguments)]
    fn panel_card(
        &mut self,
        ctx: &egui::Context,
        id: PanelId,
        rect: egui::Rect,
        header: f32,
        edges: Edges,
        outliner_shown: &mut bool,
        actions: &mut Vec<Action>,
    ) -> Option<CardEvent> {
        let folded = self.prefs.panels.card(id).is_some_and(|c| c.folded);
        let floating = edges.side.is_none();
        let mut event = None;
        // Drawn only with a document shown.
        let i = self.docs.active_index()?;
        let docs = &mut self.docs;
        egui::Area::new(egui::Id::unique(("panel", id)))
            .fixed_pos(rect.min)
            .order(if floating { egui::Order::Foreground } else { egui::Order::Middle })
            .show(ctx, |ui| {
                let frame = egui::Frame::window(ui.style()).inner_margin(6.0);
                let inner = rect.size() - frame.total_margin().sum();
                frame.show(ui, |ui| {
                    ui.set_min_size(inner);
                    ui.set_max_size(inner);
                    let (head_rect, _) = ui.allocate_exact_size(egui::vec2(inner.x, header), egui::Sense::hover());
                    let head = ui.interact(head_rect, egui::Id::unique(("panel_head", id)), egui::Sense::click_and_drag());
                    let painter = ui.painter();
                    let color = ui.visuals().strong_text_color();
                    let c = egui::pos2(head_rect.left() + 6.0, head_rect.center().y);
                    let arrow = if folded {
                        vec![c + egui::vec2(-2.5, -4.0), c + egui::vec2(3.5, 0.0), c + egui::vec2(-2.5, 4.0)]
                    } else {
                        vec![c + egui::vec2(-4.0, -2.5), c + egui::vec2(4.0, -2.5), c + egui::vec2(0.0, 3.5)]
                    };
                    painter.add(egui::Shape::convex_polygon(arrow, color, egui::Stroke::NONE));
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    painter.text(
                        egui::pos2(head_rect.left() + 18.0, head_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        panel_title(id),
                        font,
                        color,
                    );
                    if head.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    } else if head.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    if head.clicked()
                        && let Some(c) = self.prefs.panels.card_mut(id)
                    {
                        c.folded = !c.folded;
                    }
                    if head.drag_started()
                        && let Some(p) = head.interact_pointer_pos()
                    {
                        event = Some(CardEvent::Grabbed(PanelDrag { id, offset: p - rect.min, size: rect.size() }));
                    }
                    if head.drag_stopped() {
                        event = Some(CardEvent::Dropped);
                    }
                    if folded {
                        return;
                    }
                    let Some(doc) = docs.iter_mut().nth(i) else { return };
                    let body = (inner.y - header - ui.spacing().item_spacing.y).max(0.0);
                    let area = egui::ScrollArea::vertical().auto_shrink(false).max_height(body).min_scrolled_height(body);
                    match id {
                        PanelId::EntityInfo => {
                            area.id_salt("entity_info").show(ui, |ui| panels::info(ui, &doc.editor, &mut doc.info, actions));
                        }
                        PanelId::Materials => {
                            area.id_salt("materials").show(ui, |ui| panels::materials(ui, &doc.editor, &mut doc.info, actions));
                        }
                        PanelId::Outliner => {
                            *outliner_shown = true;
                            panels::outliner(ui, &doc.editor, actions, area.id_salt("outliner"));
                        }
                    }
                });
                let Some(side) = edges.side else { return };
                // The edge facing the view widens or narrows the column (double click: its
                // default width).
                let (x, sign) = match side {
                    Dock::Left => (rect.right(), 1.0),
                    Dock::Right => (rect.left(), -1.0),
                };
                let grip = egui::Rect::from_x_y_ranges(x - 3.0..=x + 3.0, rect.y_range());
                let edge = ui.interact(grip, egui::Id::unique(("panel_width", id)), egui::Sense::click_and_drag());
                if edge.hovered() || edge.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                let column = self.prefs.panels.column_mut(side);
                if edge.dragged() {
                    column.width = (rect.width() + sign * edge.drag_delta().x).clamp(dock::COLUMN_MIN_WIDTH, edges.max_width);
                }
                if edge.double_clicked() {
                    column.width = dock::COLUMN_WIDTH;
                }
                // The bottom edge sets its height (the card filling the column takes the rest).
                if !edges.bottom || folded {
                    return;
                }
                let grip = egui::Rect::from_x_y_ranges(rect.x_range(), rect.bottom() - 3.0..=rect.bottom() + 3.0);
                let edge = ui.interact(grip, egui::Id::unique(("panel_height", id)), egui::Sense::drag());
                if edge.hovered() || edge.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                if edge.dragged()
                    && let Some(c) = self.prefs.panels.card_mut(id)
                {
                    c.height = (rect.height() + edge.drag_delta().y).max(edges.min_height);
                }
            });
        event
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let Some(d) = self.docs.active_mut() else {
                // The start page: the app's message only.
                ui.label(self.message.clone().unwrap_or_default());
                return;
            };
            let status = match &self.message {
                Some(m) if !d.editor.busy() => m.clone(),
                _ => d.editor.status(),
            };
            ui.label(status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, value) = d.editor.vcb().unwrap_or_else(|| (t!("status.measurements"), String::new()));
                let mut text = if d.vcb.is_empty() { value } else { d.vcb.clone() };
                let r = ui.add_enabled(d.editor.takes_typing(), egui::TextEdit::singleline(&mut text).desired_width(160.0));
                if r.changed() {
                    d.vcb = text.clone();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !d.vcb.is_empty() {
                    let t = std::mem::take(&mut d.vcb);
                    d.editor.input(ToolInput::Vcb(t));
                }
                ui.label(label);
            });
        });
    }

    /// The start page (docs/ARCHITECTURE.md §6 octies): no document open.
    fn start_page(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(24.0));
            // The app's mark. Its tile fills 824 of the icon's 1024 units: drawn larger so the
            // tile is MARK wide (as in the title bar).
            const MARK: f32 = 72.0;
            const TILE: f32 = 824.0 / 1024.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(MARK, MARK), egui::Sense::hover());
            egui::Image::new(crate::icons::APP).paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(MARK / TILE)));
            ui.add_space(16.0);
            ui.heading(egui::RichText::new(t!("start.title")).size(28.0).strong());
            ui.add_space(20.0);
            let width = 260.0;
            for (id, label) in [("file.new", t!("start.new")), ("file.open", t!("start.open")), ("file.import", t!("start.import"))] {
                if ui.add_sized([width, 32.0], egui::Button::new(label)).clicked() {
                    actions.push(Action::Command(id));
                }
            }
            if cfg!(target_arch = "wasm32") {
                return;
            }
            ui.add_space(24.0);
            ui.strong(t!("start.recent"));
            ui.add_space(4.0);
            if self.prefs.recent_files.is_empty() {
                ui.weak(t!("start.no_recent"));
            }
            for (k, path) in self.prefs.recent_files.iter().enumerate().take(commands::RECENT_FILES) {
                let id = commands::COMMANDS.iter().find(|c| commands::recent_slot(c.id) == Some(k)).map(|c| c.id);
                let r = ui.add_sized([width, 24.0], egui::Button::new(file_name(path)).frame(false)).on_hover_text(path);
                if let (true, Some(id)) = (r.clicked(), id) {
                    actions.push(Action::Command(id));
                }
            }
        });
    }

    /// The theme the preferences ask for, followed by egui (§7 quater).
    fn apply_theme(&mut self, ctx: &egui::Context) {
        ctx.set_theme(match self.prefs.theme {
            Choice::System => egui::ThemePreference::System,
            Choice::Light => egui::ThemePreference::Light,
            Choice::Dark => egui::ThemePreference::Dark,
        });
        // egui's two styles come from the palette's tokens, installed when it changes.
        if self.styled != Some(self.prefs.palette) {
            crate::style::install_palette(ctx, self.prefs.palette);
            self.styled = Some(self.prefs.palette);
        }
        let mode = if ctx.theme() == egui::Theme::Dark { Mode::Dark } else { Mode::Light };
        self.theme = Theme::of(self.prefs.palette, mode);
    }

    /// Switches the control channel on or off, and remembers it (the preference): from the
    /// command, the preferences, the title bar's mark.
    #[cfg(not(target_arch = "wasm32"))]
    fn set_remote(&mut self, on: bool) {
        self.prefs.remote_control = on;
        if let Err(e) = self.remote.set(on) {
            self.message = Some(t!("status.control_failed", error = e));
        }
    }

    /// Answers the control channel's requests (desktop, when it is on).
    #[cfg(not(target_arch = "wasm32"))]
    fn serve_control(&mut self, frame: &eframe::Frame) {
        use crate::control::{Reply, Request};
        use serde_json::json;
        let Some(pending) = self.remote.control().map(|c| c.take()) else { return };
        for p in pending {
            let reply = match &p.request {
                Request::Commands => {
                    let list: Vec<_> = self
                        .command_views()
                        .into_iter()
                        .map(
                            |v| json!({ "id": v.id, "label": v.label, "enabled": v.enabled, "checked": v.checked, "shortcut": v.shortcut }),
                        )
                        .collect();
                    Reply::json(200, json!(list))
                }
                Request::Run(id) => match self.run(id) {
                    Ok(()) => Reply::json(200, json!({ "ok": true, "status": self.docs.active().map(|d| d.editor.status()) })),
                    Err(e) => Reply::json(400, json!({ "ok": false, "error": e })),
                },
                Request::Docs => {
                    let list: Vec<_> = self
                        .docs
                        .iter()
                        .map(|d| json!({ "id": d.id, "name": d.name(), "path": d.path, "modified": d.modified() }))
                        .collect();
                    let asking = self.closing.as_ref().and_then(|c| c.asking);
                    Reply::json(200, json!({ "docs": list, "shown": self.docs.active_index(), "asking": asking }))
                }
                Request::DocsSwitch(i) if *i < self.docs.len() => {
                    self.docs.select(*i);
                    Reply::json(200, json!({ "ok": true }))
                }
                Request::DocsClose { index, force } if *index < self.docs.len() => {
                    if *force {
                        self.close_now(*index);
                    } else {
                        self.tab_action(TabAction::Close(*index));
                    }
                    Reply::json(200, json!({ "ok": true, "asking": self.closing.as_ref().and_then(|c| c.asking) }))
                }
                Request::DocsSwitch(_) | Request::DocsClose { .. } => Reply::json(400, json!({ "ok": false, "error": "no such tab" })),
                Request::DocsNew => {
                    self.new_document();
                    Reply::json(200, json!({ "ok": true, "shown": self.docs.active_index() }))
                }
                Request::State => {
                    let Some(d) = self.docs.active() else {
                        p.answer(Reply::json(200, json!({ "document": null })));
                        continue;
                    };
                    let e = &d.editor;
                    Reply::json(
                        200,
                        json!({
                            "tool": e.tool_kind().id(),
                            "context": e.context().iter().map(|i| i.0).collect::<Vec<_>>(),
                            "open_part": e.open_part().map(|i| i.0),
                            "selection": e.selection().iter().map(|i| i.0).collect::<Vec<_>>(),
                            "status": e.status(),
                            "busy": e.busy(),
                            "revision": e.history().revision(),
                            "modified": d.modified(),
                            "path": d.path,
                            "document": d.id,
                        }),
                    )
                }
                Request::Screenshot => {
                    let shot = frame.wgpu_render_state().and_then(|s| self.viewport.capture(s));
                    match shot.map(|(w, h, px)| crate::control::png(w, h, &px)) {
                        Some(Ok(png)) => Reply { status: 200, content_type: "image/png", body: png },
                        Some(Err(e)) => Reply::json(500, json!({ "ok": false, "error": e })),
                        None => Reply::json(503, json!({ "ok": false, "error": "nothing drawn yet" })),
                    }
                }
            };
            p.answer(reply);
        }
    }
}

impl SolidSketchApp {
    /// The title bar's mark of the control channel, while it is on; frames follow while it
    /// flashes.
    fn remote_mark(&mut self, ctx: &egui::Context) -> Option<titlebar::RemoteMark> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = ctx;
            None
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::control::Status;
            match self.remote.status() {
                Status::Off => None,
                Status::On { port, served, flash } => {
                    if flash > 0.0 {
                        ctx.request_repaint();
                    }
                    Some(titlebar::RemoteMark { port, served, flash, error: None })
                }
                Status::Failed { port, error } => Some(titlebar::RemoteMark { port, served: 0, flash: 0.0, error: Some(error) }),
            }
        }
    }
}

impl eframe::App for SolidSketchApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.append(&mut self.pending_keys);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        // What the kernel finished since the last frame (it wakes the UI when it does), for
        // every document: one in the background goes on computing.
        for d in self.docs.iter_mut() {
            d.editor.poll();
        }
        // A file whose opening was cancelled is closed (an import cancelled is undone already).
        let cancelled: Vec<DocId> = self
            .docs
            .iter_mut()
            .filter_map(|d| matches!(d.editor.take_load_end(), Some(LoadEnd::Cancelled(LoadKind::Open))).then_some(d.id))
            .collect();
        for id in cancelled {
            self.loads_seen.remove(&id);
            if let Some(i) = self.docs.index_of(id) {
                self.close_now(i);
            }
        }
        // Closing the window (its button, the OS) asks about the modified documents first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_ok && self.docs.any_modified() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_quit();
        }
        // What the file dialogs ended with (they never block the event loop).
        for outcome in self.inbox.take() {
            match outcome {
                file::Outcome::Opened { name, text } => match Document::from_json(&text) {
                    Ok(doc) => {
                        self.add_recent_file(&name);
                        self.open_document(doc, Some(name));
                    }
                    Err(e) => self.message = Some(t!("status.open_failed", file = name, error = e)),
                },
                file::Outcome::Saved { doc, path, revision } => {
                    self.add_recent_file(&path);
                    if let Some(d) = self.docs.by_id_mut(doc) {
                        d.path = Some(path);
                        d.saved_at = revision;
                        self.message = Some(t!("status.saved"));
                    }
                    self.closing_saved(doc, true);
                }
                file::Outcome::SaveFailed { doc, error } => {
                    self.message = Some(t!("status.save_failed", error = error));
                    self.closing_saved(doc, false);
                }
                file::Outcome::SaveCancelled { doc } => self.closing_saved(doc, false),
                file::Outcome::Imported { name, bytes } => {
                    // Into the shown document; from the start page, a new one.
                    if self.docs.active().is_none() {
                        self.new_document();
                    }
                    if let Some(d) = self.docs.active_mut() {
                        d.editor.import_step(name, bytes);
                    }
                }
            }
        }
        // Any input clears the last app message.
        if ctx.input(|i| !i.events.is_empty() && i.pointer.any_pressed()) {
            self.message = None;
        }
        if self.closing.as_ref().is_none_or(|c| c.asking.is_none()) {
            self.keyboard(&ctx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.serve_control(frame);

        // Still the window's title where the OS shows it (Window menu, Mission Control).
        let title = match self.docs.active() {
            Some(d) => format!("SolidSketch — {}{}", d.name(), if d.modified() { " •" } else { "" }),
            None => "SolidSketch".to_owned(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        let views = self.command_views();
        let mut actions = Vec::new();
        #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
        if let Some(menu) = &mut self.native_menu {
            for id in menu.update(&views, &self.keymap) {
                self.native_command(id, &mut actions);
            }
        }
        let tabs = self.tabs();
        let bar = titlebar::Bar {
            tabs: &tabs,
            shown: self.docs.active_index(),
            reveal: !cfg!(target_arch = "wasm32"),
            integrated: crate::INTEGRATED_TITLEBAR,
            ground: self.docs.active().map(|_| (viewport::color(self.theme.ui.view), viewport::color(self.theme.ui.view_text))),
            remote: self.remote_mark(&ctx),
        };
        titlebar::show(ui, &bar, &views, &mut actions);
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        self.ribbon_bottom = None;
        if self.docs.active().is_some() {
            self.document_ui(ui, &ctx, frame, &views, &mut actions);
        } else {
            egui::CentralPanel::default().show(ui, |ui| self.start_page(ui, &mut actions));
        }
        self.preferences_window(&ctx);
        self.palette_window(&ctx, &views, &mut actions);
        self.save_prompt(&ctx);
        self.loading_window(&ctx);
        for a in actions {
            self.act(a);
        }
        self.follow_documents();
        // Not while dragging (a panel's edge…): written once released.
        if self.prefs != self.stored_prefs && !ctx.input(|i| i.pointer.any_down()) {
            if self.prefs.shortcuts != self.stored_prefs.shortcuts {
                self.keymap = Keymap::new(&self.prefs.shortcuts);
            }
            if let Err(e) = self.prefs.save() {
                self.message = Some(t!("status.prefs_not_saved", error = e));
            }
            self.stored_prefs = self.prefs.clone();
        }
    }
}

impl SolidSketchApp {
    /// After the frame's actions: the tools' settings between the documents and the
    /// preferences, the documents to open again at launch, the page's question (web).
    fn follow_documents(&mut self) {
        let shown = self.docs.active().map(|d| d.id);
        if shown != self.last_shown {
            // A newly shown document takes the tools' settings as they are now.
            if let Some(d) = self.docs.active_mut() {
                d.editor.set_tool_settings(self.prefs.tool_options.clone());
            }
            self.last_shown = shown;
        } else if let Some(d) = self.docs.active()
            && self.prefs.tool_options != *d.editor.tool_settings()
        {
            // A tool's option changed (in the bar, or by Tab): kept with the preferences.
            self.prefs.tool_options = d.editor.tool_settings().clone();
        }
        // While quitting the documents stay as they were open.
        #[cfg(not(target_arch = "wasm32"))]
        if !self.closing.as_ref().is_some_and(|c| c.quit) && !self.quit_ok {
            let saved: Vec<(usize, &String)> = self.docs.iter().enumerate().filter_map(|(i, d)| Some((i, d.path.as_ref()?))).collect();
            let files: Vec<String> = saved.iter().map(|(_, p)| (*p).clone()).collect();
            let shown_file = self.docs.active_index().and_then(|a| saved.iter().position(|(i, _)| *i == a));
            if files != self.prefs.open_files || shown_file != self.prefs.shown_file {
                self.prefs.open_files = files;
                self.prefs.shown_file = shown_file;
            }
        }
        #[cfg(target_arch = "wasm32")]
        self.unsaved.set(self.docs.any_modified());
    }

    /// The shown document: the view, the ribbon, the docked cards and the tool's options.
    fn document_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        views: &[CommandView],
        actions: &mut Vec<Action>,
    ) {
        // The 3D view fills everything between the title bar and the status bar; the tools (with
        // the active tool's option bar as their footer) and the side panel float over it, so
        // opening or resizing them never resizes the view.
        let theme = self.theme;
        let Some(i) = self.docs.active_index() else { return };
        let (viewport, docs, prefs) = (&mut self.viewport, &mut self.docs, &self.prefs);
        let view = egui::CentralPanel::no_frame()
            .show(ui, |ui| {
                if let Some(d) = docs.iter_mut().nth(i) {
                    viewport.ui(ui, frame, d.id, &mut d.editor, &d.vcb, &prefs.navigation, &theme, prefs.show_render_stats);
                }
            })
            .response
            .rect;
        let margin = 8.0;
        let card = |style: &egui::Style| egui::Frame::window(style).inner_margin(6.0);
        // The workspace card and the ribbon across the top (ARCHITECTURE.md §6 septies); the
        // docked cards below them.
        let ribbon_view = ribbon::View { prefs: &self.prefs.ribbon, views };
        // As wide as its content, centered on the view; never over the workspace card nor past
        // the right margin: a narrower window narrows it ([`ribbon::fit`] then gives way).
        let frame_x = card(&ctx.global_style()).total_margin().sum().x;
        let min_left = view.left() + margin + ribbon::WORKSPACE_WIDTH + frame_x + margin;
        let max_right = view.right() - margin;
        let ribbon_width = ribbon::natural_width(ctx, &ribbon_view).min((max_right - min_left - frame_x).max(0.0));
        let ribbon_left = (view.center().x - (ribbon_width + frame_x) / 2.0).min(max_right - ribbon_width - frame_x).max(min_left);
        let ribbon_area = egui::Area::new(egui::Id::unique("ribbon"))
            .fixed_pos(egui::pos2(ribbon_left, view.top() + margin))
            .constrain_to(view)
            .sense(egui::Sense::click())
            .show(ctx, |ui| {
                let card = card(ui.style());
                let inset = card.inner_margin;
                card.show(ui, |ui| ribbon::ribbon(ui, &ribbon_view, ribbon_width, inset, actions))
            });
        egui::Popup::context_menu(&ribbon_area.response)
            .style(crate::style::menu)
            .show(|ui| ribbon::ribbon_menu(ui, &self.prefs.ribbon, actions));
        let ribbon_rect = ribbon_area.response.rect;
        let inner_height = ribbon_rect.height() - card(&ctx.global_style()).total_margin().sum().y;
        egui::Area::new(egui::Id::unique("workspace"))
            .fixed_pos(egui::pos2(view.left() + margin, view.top() + margin))
            .constrain_to(view)
            .show(ctx, |ui| card(ui.style()).show(ui, |ui| ribbon::workspace_card(ui, &ribbon_view, inner_height, actions)));
        let below_ribbon = ribbon_rect.bottom() + margin;
        self.ribbon_bottom = Some(ribbon_rect.bottom());

        let free = self.side_panels(ctx, view, below_ribbon, margin, actions);
        let (free_left, free_right) = (free.min, free.max);
        // The active tool's options: a card centered under the ribbon, between the columns, its
        // width known before drawing it (as last measured: a pass whose width changed is
        // discarded, so the card is never shown misplaced while it settles). No option, no card.
        let options_id = egui::Id::unique("tool_options");
        let width_id = options_id.with("width");
        let mut free_top = below_ribbon;
        let Some(doc) = self.docs.active_mut() else { return };
        if panels::has_tool_options(&doc.editor) {
            let content_width = ctx.data(|d| d.get_temp::<f32>(width_id)).unwrap_or(0.0);
            let options_width = content_width + card(&ctx.global_style()).total_margin().sum().x;
            let options_left = (view.center().x - options_width / 2.0).min(free_right - options_width).max(free_left);
            let options = egui::Area::new(options_id)
                .fixed_pos(egui::pos2(options_left, below_ribbon))
                .constrain_to(view)
                .show(ctx, |ui| {
                    card(ui.style()).show(ui, |ui| {
                        ui.set_width(content_width);
                        centered_row(ui, width_id, |ui| panels::tool_options(ui, &doc.editor, actions));
                    });
                })
                .response
                .rect;
            free_top = options.bottom() + margin;
        }
        // The quick tools' card, at the centre of what was just made (§6 terdecies).
        if let Some(at) = doc.editor.quick_card() {
            let at = view.min + egui::vec2(at.x as f32, at.y as f32);
            crate::quick::card(ctx, view, at, &self.prefs.quick_tools, views, actions);
        }
        // What zoom extents frames: below the ribbon (and the options), beside the side panels,
        // in viewport pixels.
        let free_min = egui::vec2(free_left - view.left(), free_top - view.top());
        let free_max = egui::vec2(free_right - view.left(), view.height());
        doc.editor
            .set_free_area(glam::DVec2::new(free_min.x as f64, free_min.y as f64), glam::DVec2::new(free_max.x as f64, free_max.y as f64));
    }
}

/// The language: every text changes at once.
fn general_settings(ui: &mut egui::Ui, prefs: &mut Preferences) {
    egui::Grid::new("general").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(t!("prefs.general.language"));
        let before = prefs.locale.clone();
        egui::ComboBox::from_id_salt("locale").selected_text(solidsketch_i18n::locale_name(&prefs.locale)).show_ui(ui, |ui| {
            for l in solidsketch_i18n::available_locales() {
                let name = solidsketch_i18n::locale_name(&l);
                ui.selectable_value(&mut prefs.locale, l, name);
            }
        });
        if prefs.locale != before {
            solidsketch_i18n::set_locale(&prefs.locale);
        }
        ui.end_row();
        if !cfg!(target_arch = "wasm32") {
            ui.label(t!("prefs.general.reopen"));
            ui.checkbox(&mut prefs.reopen_documents, "");
            ui.end_row();
        }
    });
}

/// A section's name in the command palette: its menu's, small and dimmed. Its rect.
fn palette_header(ui: &mut egui::Ui, menu: Menu) -> egui::Rect {
    const PAD: f32 = 8.0;
    let mut font = egui::TextStyle::Small.resolve(ui.style());
    font.size = font.size.max(11.0);
    let weak = ui.visuals().weak_text_color();
    let g = ui.painter().layout_no_wrap(menu.label().to_uppercase(), font, weak);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), g.size().y + 8.0), egui::Sense::hover());
    ui.painter().galley(egui::pos2(rect.left() + PAD, rect.bottom() - g.size().y - 2.0), g, weak);
    rect
}

/// A command palette's row: the whole width, the command's label on the left and its shortcut
/// (dimmed) on the right; filled when picked or hovered.
fn palette_row(ui: &mut egui::Ui, v: &CommandView, picked: bool) -> egui::Response {
    const HEIGHT: f32 = 30.0;
    const PAD: f32 = 8.0;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), HEIGHT), egui::Sense::click());
    let visuals = ui.visuals();
    let fill = if picked {
        Some(visuals.selection.bg_fill)
    } else if r.hovered() {
        Some(visuals.widgets.hovered.weak_bg_fill)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, visuals.widgets.hovered.corner_radius, fill);
    }
    let text = if picked { visuals.selection.stroke.color } else { visuals.text_color() };
    let weak = if picked { text } else { visuals.weak_text_color() };
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut right = rect.right() - PAD;
    if let Some(s) = &v.shortcut {
        let g = ui.painter().layout_no_wrap(s.clone(), font.clone(), weak);
        right -= g.size().x;
        ui.painter().galley(egui::pos2(right, rect.center().y - g.size().y / 2.0), g, weak);
        right -= 2.0 * PAD;
    }
    let left = rect.left() + PAD;
    let label = egui::WidgetText::from(v.label.as_str()).color(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (right - left).max(0.0),
        font,
    );
    ui.painter().galley(egui::pos2(left, rect.center().y - label.size().y / 2.0), label, text);
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A palette's name in the preferences.
fn palette_name(palette: UiPalette) -> String {
    match palette {
        UiPalette::Neutral => t!("prefs.interface.palettes.neutral"),
        UiPalette::Warm => t!("prefs.interface.palettes.warm"),
        UiPalette::Graphite => t!("prefs.interface.palettes.graphite"),
    }
}

fn interface_settings(ui: &mut egui::Ui, prefs: &mut Preferences) {
    egui::Grid::new("interface").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(t!("prefs.interface.theme"));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut prefs.theme, Choice::System, t!("prefs.interface.system"));
            ui.selectable_value(&mut prefs.theme, Choice::Light, t!("prefs.interface.light"));
            ui.selectable_value(&mut prefs.theme, Choice::Dark, t!("prefs.interface.dark"));
        });
        ui.end_row();
        ui.label(t!("prefs.interface.palette"));
        // The cards show the variant the theme draws now.
        let mode = if ui.ctx().theme() == egui::Theme::Dark { Mode::Dark } else { Mode::Light };
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for palette in UiPalette::ALL {
                let name = palette_name(palette);
                if crate::style::palette_card(ui, palette, mode, &name, prefs.palette == palette).clicked() {
                    prefs.palette = palette;
                }
            }
        });
        ui.end_row();
        ui.label(t!("prefs.interface.ribbon_labels"));
        ui.checkbox(&mut prefs.ribbon.labels, "");
        ui.end_row();
        ui.label(t!("prefs.interface.panels"));
        ui.vertical(|ui| {
            for id in PanelId::ALL {
                let mut shown = prefs.panels.is_shown(id);
                if ui.checkbox(&mut shown, panel_title(id)).changed() {
                    prefs.panels.toggle(id);
                }
            }
        });
        ui.end_row();
    });
}

fn navigation_settings(ui: &mut egui::Ui, nav: &mut Navigation) {
    egui::Grid::new("navigation").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.strong(t!("prefs.navigation.orbit.title"));
        ui.end_row();
        ui.label(t!("prefs.navigation.orbit.speed"));
        ui.add(egui::Slider::new(&mut nav.orbit_speed, 0.25..=4.0).logarithmic(true).fixed_decimals(2));
        ui.end_row();
        ui.label(t!("prefs.navigation.orbit.mouse"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut nav.mouse_invert_x, t!("prefs.navigation.orbit.invert_x"));
            ui.checkbox(&mut nav.mouse_invert_y, t!("prefs.navigation.orbit.invert_y"));
        });
        ui.end_row();
        ui.label(t!("prefs.navigation.orbit.trackpad"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut nav.trackpad_invert_x, t!("prefs.navigation.orbit.invert_x"));
            ui.checkbox(&mut nav.trackpad_invert_y, t!("prefs.navigation.orbit.invert_y"));
        });
        ui.end_row();
        ui.strong(t!("prefs.navigation.zoom.title"));
        ui.end_row();
        ui.label(t!("prefs.navigation.zoom.speed"));
        ui.add(egui::Slider::new(&mut nav.zoom_speed, 0.25..=4.0).logarithmic(true).fixed_decimals(2));
        ui.end_row();
        ui.label(t!("prefs.navigation.zoom.wheel"));
        ui.checkbox(&mut nav.wheel_invert, t!("prefs.navigation.zoom.wheel_invert"));
        ui.end_row();
        ui.label(t!("prefs.navigation.zoom.target"));
        ui.checkbox(&mut nav.zoom_to_cursor, t!("prefs.navigation.zoom.to_cursor"));
        ui.end_row();
    });
    ui.add_space(8.0);
    if ui.add_enabled(*nav != Navigation::default(), egui::Button::new(t!("prefs.navigation.defaults"))).clicked() {
        *nav = Navigation::default();
    }
}

/// Does `e` make a held copy modifier something else than a tap (a shortcut, a click, a
/// scroll)? The modifier keys themselves come as key events too: they do not.
/// A modifier followed to tell a tap of it alone: down, then up, with nothing else in between.
#[derive(Clone, Copy, Debug, Default)]
struct Tap {
    down: bool,
    /// Down, and nothing else happened since it went down.
    tap: bool,
}

impl Tap {
    /// This frame: the modifier down or not, and whether something `other` happened; true on
    /// the release that ends a tap.
    fn feed(&mut self, down: bool, other: bool) -> bool {
        if down && (!self.down || other) {
            self.tap = !other;
        }
        let tapped = !down && self.down && self.tap;
        if !down {
            self.tap = false;
        }
        self.down = down;
        tapped
    }
}

fn interrupts_tap(e: &egui::Event) -> bool {
    use egui::Key as K;
    let modifier = |k: &K| {
        matches!(
            k,
            K::ShiftLeft | K::ShiftRight | K::ControlLeft | K::ControlRight | K::AltLeft | K::AltRight | K::SuperLeft | K::SuperRight
        )
    };
    match e {
        egui::Event::Key { key, physical_key, .. } => !(modifier(key) || physical_key.as_ref().is_some_and(modifier)),
        egui::Event::PointerButton { .. } | egui::Event::MouseWheel { .. } => true,
        _ => false,
    }
}

/// The frame of the app's modal boxes: egui's popup frame, with room around the content.
fn modal_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(MODAL_MARGIN))
}

/// The room between a modal box's border and its content (points).
const MODAL_MARGIN: i8 = 20;

/// The padding of a modal box's buttons (points): roomier than the app's compact buttons.
const MODAL_BUTTON_PADDING: egui::Vec2 = egui::vec2(14.0, 5.0);

/// Was `key` pressed this frame?
fn ui_key(ctx: &egui::Context, key: egui::Key) -> bool {
    ctx.input(|i| i.key_pressed(key))
}

/// Shows the file at `path` in the Finder, the Explorer or the file manager.
#[cfg(not(target_arch = "wasm32"))]
fn reveal(path: &str) {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(path).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn()
    } else {
        let folder = std::path::Path::new(path).parent().map_or_else(|| ".".into(), |p| p.to_path_buf());
        std::process::Command::new("xdg-open").arg(folder).spawn()
    };
    if let Err(e) = result {
        log::warn!("cannot show {path}: {e}");
    }
}

/// A row centered in `ui`: egui lays a row out from the left, so it is shifted by half the room
/// it leaves, from its width as last measured (kept under `width_id`). A new width discards
/// the pass: the next one, laid out with it, is the one shown.
fn centered_row(ui: &mut egui::Ui, width_id: egui::Id, add_contents: impl FnOnce(&mut egui::Ui)) {
    let width = ui.data(|d| d.get_temp::<f32>(width_id)).unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        let used = ui.scope(add_contents).response.rect.width();
        if used != width {
            ui.data_mut(|d| d.insert_temp(width_id, used));
            // Past egui's passes for this frame, the discard is refused: the next frame then.
            ui.ctx().request_discard("tool bar row resized");
            ui.ctx().request_repaint();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_keys_do_not_interrupt_a_tap() {
        let key = |key: egui::Key| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::ALT,
        };
        assert!(!interrupts_tap(&key(egui::Key::AltLeft)));
        assert!(!interrupts_tap(&key(egui::Key::ControlRight)));
        assert!(interrupts_tap(&key(egui::Key::Z)));
    }

    #[test]
    fn a_tap_is_a_press_and_a_release_with_nothing_between() {
        let mut t = Tap::default();
        assert!(!t.feed(true, false) && t.feed(false, false));
        // Something else while it is down: no tap.
        assert!(!t.feed(true, false) && !t.feed(true, true) && !t.feed(false, false));
        // Held, then released later, still nothing else: a tap.
        assert!(!t.feed(true, false) && !t.feed(true, false) && t.feed(false, false));
    }

    #[test]
    fn every_tool_and_view_command_is_wired() {
        for c in COMMANDS {
            if c.id.starts_with("tool.") {
                assert!(tool_of(c.id).is_some(), "{}", c.id);
            }
        }
        for id in ["view.top", "view.front", "view.right", "view.back", "view.left", "view.iso"] {
            assert!(view_of(id).is_some(), "{id}");
        }
    }
}

/// The edges of a side panel that can be dragged.
#[derive(Clone, Copy)]
struct Edges {
    /// Its column (`None`: dragged, out of any column).
    side: Option<Dock>,
    /// Its bottom edge sets its height.
    bottom: bool,
    max_width: f32,
    min_height: f32,
}

/// What a side panel's header did.
enum CardEvent {
    /// Its drag started.
    Grabbed(PanelDrag),
    /// Released.
    Dropped,
}

fn panel_title(id: PanelId) -> String {
    match id {
        PanelId::EntityInfo => t!("panel.entity_info.title"),
        PanelId::Materials => t!("panel.materials.title"),
        PanelId::Outliner => t!("panel.outliner.title"),
    }
}

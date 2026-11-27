//! The application window: title bar (top), workspace card and ribbon across the top of the
//! view (docs/ui/ribbon.md), the tool option bar centered under them, entity info, materials and
//! navigator (cards docked left or right, under the ribbon), status bar with the measurements
//! box (bottom), viewport (centre). Every action goes through the command registry (docs/ui/commands.md):
//! the native menu (macOS, File only), title bar, ribbon, view bar, keyboard (SketchUp's shortcuts,
//! changeable), the command palette and the control channel.

use skraft_i18n::{ellipsis, t, tp};
use skraft_model::kernel::{KernelService, Progress, SharedKernel};
use skraft_model::{Boolean, Document, FileView, LengthUnit, Operation, Settings};
use skraft_theme::{Choice, Mode, Palette as UiPalette, Theme};
use skraft_tools::{Editor, Key, LoadEnd, LoadKind, StandardView, ToolInput, ToolKind};

use crate::commands::{self, COMMANDS, Keymap, Menu, Shortcut};
use crate::commands::{Action, CommandView};
use crate::docs::{DocId, Docs, file_name};
use crate::kit::style::{CARD_MARGIN, Tip, card_frame, option_card};
use crate::prefs::Preferences;
use crate::system::clipboard::{Clipboard, Copied};
use crate::system::file::{self, Inbox};
use crate::window::dock::{self, Dock, PanelId};
#[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
use crate::window::native_menu::NativeMenu;
use crate::window::ribbon;
use crate::window::tabs::{MenuRow, Tab, TabAction};
use crate::window::titlebar;
use crate::window::toasts::{Act, Note, Toasts};
use crate::workspaces::common::cards;
use crate::workspaces::common::viewport::options;
use crate::workspaces::common::viewport::{self, Viewport};
use crate::workspaces::{self, Stage, Workspace};

pub struct SkraftApp {
    ctx: egui::Context,
    /// What the window's form is read from, and the form of this frame (docs/ui/touch.md).
    probe: crate::window::form::Probe,
    form: crate::window::form::Form,
    compact: compact::Compact,
    /// The measurements box's keypad is open (docs/ui/touch.md#the-keypad).
    keypad: bool,
    /// The page's colours around the app, as last given (web, docs/ui/touch.md#safe-areas).
    #[cfg(target_arch = "wasm32")]
    page_colors: Option<(egui::Color32, egui::Color32, egui::Color32)>,
    /// The GPU the window draws with: a saved file's picture of its model is drawn with it
    /// (docs/io/file-format.md).
    #[cfg(not(target_arch = "wasm32"))]
    render_state: Option<egui_wgpu::RenderState>,
    /// The start page's pictures of the recent files' models.
    start: crate::window::start::Pictures,
    /// The documents open, one per file tab (docs/ui/file-tabs.md); none shown: the
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
    /// The app's messages (a file not opened, « Enregistré »…); the status bar keeps the
    /// tools' hints.
    toasts: Toasts,
    /// The interface's zoom, shown a moment when it changes.
    zoom_label: crate::window::zoom_label::ZoomLabel,
    /// The zoom egui was last seen or set at (`None`: not yet, the preference's goes first).
    zoom_applied: Option<f32>,
    /// The dialog naming a part just made (docs/tools/quick-tools.md).
    naming: Option<crate::workspaces::common::dialogs::naming::Dialog>,
    /// The confirmation of « Empreindre » (decision 0055).
    imprint: Option<crate::workspaces::modelling::dialogs::imprint::Dialog>,
    /// A coat to delete that sequences lay, waiting for its confirmation: its document, its id,
    /// how many sequences (decision 0057).
    deleting_coat: Option<(u64, skraft_model::Id, usize)>,
    prefs: Preferences,
    /// The preferences as last stored: they are written again when they differ.
    stored_prefs: Preferences,
    prefs_window: Option<PrefsTab>,
    about_open: bool,
    /// The check for a new version (docs/ui/updates.md).
    #[cfg(not(target_arch = "wasm32"))]
    updates: crate::dialogs::update::Updates,
    /// The journal window, while open (`app.journal`).
    journal_window: Option<crate::dialogs::journal_window::JournalWindow>,
    /// The kit's gallery, while open (`debug.kit_gallery`).
    kit_gallery: Option<crate::dialogs::kit_gallery::KitGallery>,
    /// The side panel being dragged to another place.
    panel_drag: Option<PanelDrag>,
    /// The shortcuts in use (the defaults with the user's changes).
    keymap: Keymap,
    /// The command whose shortcut the Raccourcis tab waits for.
    capturing: Option<&'static str>,
    /// The Raccourcis tab's search.
    shortcut_search: String,
    palette: Option<Palette>,
    /// The ribbon's bottom this frame (`None`: no document shown): the command palette opens
    /// below it.
    ribbon_bottom: Option<f32>,
    /// The ribbon's bottom when last drawn, kept across frames: a sheet is framed below it
    /// (the stage is drawn before the ribbon).
    ribbon_last_bottom: Option<f32>,
    /// « Exporter en PDF » asked for, by which document and when: done at a next frame, where
    /// the GPU is at hand, once its textures are ready (docs/ui/drawing.md#pdf).
    pdf_asked: Option<(DocId, f64)>,
    /// The top of the steps bar and the validation this frame, if shown: the toasts stand above.
    steps_top: Option<f32>,
    theme: Theme,
    /// The palette egui's styles were installed for (`None`: not yet).
    styled: Option<UiPalette>,
    /// The copy modifier, Shift and the other modifier (Command on macOS, Alt elsewhere), each
    /// followed to tell a tap of it alone ([`Tap`]).
    taps: [Tap; 3],
    /// Shows the Débogage menu ([`debug_menu_wanted`]).
    debug_menu: bool,
    /// The control protocol's requests waiting for a frame, real inputs, messages (docs/control-protocol.md).
    driving: crate::system::driving::Driving,
    /// The control channel, switched on and off while the app runs (desktop).
    #[cfg(not(target_arch = "wasm32"))]
    remote: crate::system::control::Remote,
    /// This app in the registry of running apps, its channel's port kept up to date (desktop;
    /// none if it could not be written).
    #[cfg(not(target_arch = "wasm32"))]
    registration: Option<skraft_control::instances::Registration>,
    /// The Claude Code session that launched this dev build, watched for its name (desktop).
    #[cfg(not(target_arch = "wasm32"))]
    session: Option<crate::system::identity::Session>,
    /// The macOS menu bar (`None` if it could not be built: the ☰ menu then).
    #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
    native_menu: Option<NativeMenu>,
    /// Keys given back to egui on the next frame ([`Self::native_command`]).
    pending_keys: Vec<egui::Event>,
    /// Tab and Shift+Tab taken from egui this frame, for the tool ([`Self::raw_input_hook`]).
    tabs: Vec<egui::Event>,
    /// The documents loading, and since when (egui's time): the window comes after
    /// [`LOAD_WINDOW_AFTER`].
    loads_seen: std::collections::HashMap<DocId, (f64, LoadKind)>,
    /// The paint tool was active at the last look ([`Self::reveal_materials`]).
    painting: bool,
    /// What was copied, for every document (docs/tools/copy-paste.md), and how many
    /// copies were made (each names its token).
    clipboard: Option<Clipboard>,
    copies: u64,
}

/// How long a load lasts before its window shows (seconds).
const LOAD_WINDOW_AFTER: f64 = 0.3;

/// Closing documents one after the other (docs/ui/file-tabs.md): each modified one
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
/// the page's address on the web, `SKRAFT_DEBUG` set in the environment on the desktop.
fn debug_menu_wanted() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.location().search().ok())
            .is_some_and(|q| q.trim_start_matches('?').split('&').any(|p| p == "debug" || p.starts_with("debug=")))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var_os("SKRAFT_DEBUG").is_some()
    }
}

/// Where the kernel runs (docs/model/kernel.md): its own thread on the desktop, a web
/// worker in the browser. Either wakes the UI when a result is ready.
fn kernel_service(ctx: &egui::Context) -> Box<dyn KernelService> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let ctx = ctx.clone();
        Box::new(skraft_model::kernel::Pool::new(Some(std::sync::Arc::new(move || ctx.request_repaint()))))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let ctx = ctx.clone();
        Box::new(crate::system::worker::WebKernel::new(std::rc::Rc::new(move || ctx.request_repaint())))
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
        "tool.sample" => ToolKind::Sample,
        "tool.section" => ToolKind::Section,
        "tool.measure" => ToolKind::Measure,
        "tool.stretch" => ToolKind::Stretch,
        "tool.reorient" => ToolKind::Reorient,
        "tool.round" => ToolKind::Round,
        "tool.chamfer" => ToolKind::Chamfer,
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

/// The preferences window's id (its place kept at launch, [`prune_egui_memory`]).
fn prefs_window_id() -> egui::Id {
    egui::Id::unique("preferences")
}

/// What egui keeps on disk (`app.ron`) and never drops, dropped at launch: the text fields'
/// states (cursor, undo), many keyed by model ids (renames, positions); the places of every
/// area ever shown — each tooltip's, each menu's —, but the windows' the user moves (the
/// preferences, the journal). The other areas of the app are placed every frame.
fn prune_egui_memory(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove_by_type::<egui::text_edit::TextEditState>());
    let windows: Vec<_> = [prefs_window_id(), crate::dialogs::journal_window::window_id()]
        .into_iter()
        .filter_map(|id| egui::AreaState::load(ctx, id).map(|s| (id, s)))
        .collect();
    ctx.memory_mut(|m| {
        let areas = m.areas_mut();
        *areas = Default::default();
        for (id, state) in windows {
            areas.set_state(egui::LayerId::new(egui::Order::Middle, id), state);
        }
    });
}

impl SkraftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        // A phone's arrangement tried on the desktop takes a phone's size, whatever the window
        // had last (docs/ui/touch.md).
        let probe = crate::window::form::Probe::read();
        #[cfg(not(target_arch = "wasm32"))]
        if probe.forced_compact() {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(390.0, 844.0)));
        }
        crate::kit::style::install_fonts(&cc.egui_ctx);
        prune_egui_memory(&cc.egui_ctx);
        // The page's command API asks for a frame when called (docs/control-protocol.md).
        #[cfg(target_arch = "wasm32")]
        crate::system::web::set_context(cc.egui_ctx.clone());
        let (prefs, error) = Preferences::load();
        // The texts name keys as the platform does (in the browser too).
        skraft_i18n::set_mac(cc.egui_ctx.os() == egui::os::OperatingSystem::Mac);
        crate::system::separators::read();
        crate::system::language::apply(prefs.locale.as_deref());
        let docs = Docs::new(SharedKernel::new(kernel_service(&cc.egui_ctx)));
        let mut toasts = Toasts::default();
        if let Some(e) = error {
            toasts.push(Note::error(t!("toast.prefs_not_loaded"), e));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let remote = {
            let ctx = cc.egui_ctx.clone();
            let mut remote = crate::system::control::Remote::new(
                crate::system::control::env_port().unwrap_or(prefs.remote_port),
                skraft_control::server::default_dir(),
                move || ctx.request_repaint(),
            );
            // The preference, or the environment for this launch only.
            if (prefs.remote_control || crate::system::control::env_wanted())
                && let Err(e) = remote.set(true)
            {
                toasts.push(Note::error(t!("toast.control_failed"), e));
            }
            remote
        };
        #[cfg(not(target_arch = "wasm32"))]
        let registration = skraft_control::server::default_dir().and_then(|dir| {
            let here = skraft_control::instances::Instance::here(env!("CARGO_PKG_VERSION"), crate::system::identity::build());
            skraft_control::instances::Registration::register(&dir, here)
                .inspect_err(|e| log::warn!("not in the registry of running apps: {e}"))
                .ok()
        });
        // Its native menu is added below, on macOS.
        #[cfg_attr(not(all(target_os = "macos", not(target_arch = "wasm32"))), allow(unused_mut))]
        let mut app = SkraftApp {
            ctx: cc.egui_ctx.clone(),
            probe,
            form: Default::default(),
            compact: Default::default(),
            keypad: false,
            #[cfg(target_arch = "wasm32")]
            page_colors: None,
            #[cfg(not(target_arch = "wasm32"))]
            render_state: cc.wgpu_render_state.clone(),
            start: Default::default(),
            docs,
            last_shown: None,
            closing: None,
            naming: None,
            imprint: None,
            deleting_coat: None,
            quit_ok: false,
            #[cfg(target_arch = "wasm32")]
            unsaved: Default::default(),
            viewport: Viewport::new(),
            inbox: Inbox::default(),
            toasts,
            zoom_label: Default::default(),
            zoom_applied: None,
            keymap: Keymap::new(&prefs.shortcuts),
            panel_drag: None,
            stored_prefs: prefs.clone(),
            prefs,
            prefs_window: None,
            about_open: false,
            #[cfg(not(target_arch = "wasm32"))]
            updates: crate::dialogs::update::Updates::new(&cc.egui_ctx),
            journal_window: None,
            kit_gallery: None,
            capturing: None,
            shortcut_search: String::new(),
            palette: None,
            ribbon_bottom: None,
            ribbon_last_bottom: None,
            pdf_asked: None,
            steps_top: None,
            theme: Theme::light(),
            styled: None,
            taps: Default::default(),
            debug_menu: debug_menu_wanted(),
            driving: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            remote,
            #[cfg(not(target_arch = "wasm32"))]
            registration,
            #[cfg(not(target_arch = "wasm32"))]
            session: crate::system::identity::Session::watch(cc.egui_ctx.clone()),
            #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
            native_menu: None,
            pending_keys: Vec::new(),
            tabs: Vec::new(),
            loads_seen: std::collections::HashMap::new(),
            painting: false,
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
        #[cfg(not(target_arch = "wasm32"))]
        app.updates.start(&app.prefs.updates);
        #[cfg(not(target_arch = "wasm32"))]
        crate::system::opened::set_context(&cc.egui_ctx);
        #[cfg(target_arch = "wasm32")]
        app.ask_before_leaving();
        app
    }

    /// Readies a document's editor: the tools' settings, the user's.
    fn setup(prefs: &Preferences) -> impl FnOnce(&mut Editor) + use<> {
        let tools = prefs.tool_options.clone();
        move |e: &mut Editor| e.set_tool_settings(tools)
    }

    // ---- commands ------------------------------------------------------------------------------

    /// The theme the preferences ask for, followed by egui (docs/render/look.md).
    fn apply_theme(&mut self, ctx: &egui::Context) {
        ctx.set_theme(match self.prefs.theme {
            Choice::System => egui::ThemePreference::System,
            Choice::Light => egui::ThemePreference::Light,
            Choice::Dark => egui::ThemePreference::Dark,
        });
        // egui's two styles come from the palette's tokens, installed when it changes.
        if self.styled != Some(self.prefs.palette) {
            crate::kit::style::install_palette(ctx, self.prefs.palette);
            self.styled = Some(self.prefs.palette);
        }
        let mode = if ctx.theme() == egui::Theme::Dark { Mode::Dark } else { Mode::Light };
        self.theme = Theme::of(self.prefs.palette, mode);
        // The page around the app takes the title bar's colour above, the panels' below.
        #[cfg(target_arch = "wasm32")]
        {
            let visuals = &ctx.global_style().visuals;
            let colors = (viewport::color(self.theme.ui.title_bar.ground), visuals.panel_fill, visuals.strong_text_color());
            if self.page_colors != Some(colors) {
                crate::system::web::set_page_colors(colors.0, colors.1, colors.2);
                self.page_colors = Some(colors);
            }
        }
    }

    /// The interface's zoom and its preference kept together (docs/ui/interface.md#interface-zoom):
    /// the preference's at launch, egui's when a shortcut changed it, the preference's when the
    /// Preferences changed it. egui takes a new zoom at the next frame.
    fn apply_zoom(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        match self.zoom_applied {
            Some(applied) if zoom != applied => self.prefs.zoom = zoom,
            Some(applied) if self.prefs.zoom == applied => return,
            _ => {
                if !(self.prefs.zoom.is_finite() && (ZOOM_MIN..=ZOOM_MAX).contains(&self.prefs.zoom)) {
                    self.prefs.zoom = 1.0;
                }
                ctx.set_zoom_factor(self.prefs.zoom);
            }
        }
        self.zoom_applied = Some(self.prefs.zoom);
    }
}

/// The interface's zoom a preferences file may set; egui's shortcuts stay between them.
const ZOOM_MIN: f32 = 0.2;
const ZOOM_MAX: f32 = 5.0;

impl eframe::App for SkraftApp {
    /// The control channel stops with the app: its token and discovery files go.
    fn on_exit(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = self.remote.set(false);
            self.registration = None;
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.append(&mut self.pending_keys);
        // Real input asked over the control channel: one frame's events (docs/control-protocol.md).
        self.driving.feed(raw_input);
        // Tab and Shift+Tab are the tools' (the booleans' scope, Stretch…) while no text field
        // has the focus: egui would give the focus to the first widget (or the last) instead —
        // its keyboard navigation — and from then on keep every key from the tool. In a text
        // field, they go from field to field; a shortcut being captured, or a modal dialog,
        // takes them as they are.
        if self.keys_to_tools() && !ctx.text_edit_focused() && self.capturing.is_none() {
            let tabs = &mut self.tabs;
            raw_input.events.retain(|e| {
                let tab =
                    matches!(e, egui::Event::Key { key: egui::Key::Tab, modifiers, .. } if !modifiers.any() || modifiers.shift_only());
                if tab {
                    tabs.push(e.clone());
                }
                !tab
            });
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::kit::handles::frame_start();
        self.driving.frame_start();
        // A press held still is a long press after LONG_PRESS: under a finger, egui is told, and
        // a frame comes while a finger is down though nothing moves, to see it.
        if self.probe.coarse {
            ctx.options_mut(|o| {
                o.input_options.max_click_duration = crate::window::form::LONG_PRESS;
                o.input_options.max_click_dist = crate::window::form::TAP_SLOP;
                o.input_options.max_double_click_delay = crate::window::form::DOUBLE_TAP;
            });
        }
        if matches!(self.driving.origin(), skraft_tools::journal::Origin::User) {
            self.probe.watch_keyboard(&ctx);
        }
        crate::kit::touch::watch_pointer(&ctx, self.probe.coarse);
        if ctx.input(|i| i.any_touches()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.form = self.probe.form(ctx.content_rect().size());
        // This frame's input: the user's, or a client's real events (`ui.*`).
        let origin = self.driving.origin();
        self.set_origin(&origin);
        self.apply_theme(&ctx);
        self.apply_zoom(&ctx);
        crate::kit::widgets::color::sync_favorites(&ctx, &mut self.prefs.favorite_colors);
        self.poll_documents();
        self.take_notices(&ctx);
        self.end_cancelled_loads();
        // Closing the window (its button, the OS) asks about the modified documents first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_ok && self.docs.any_modified() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_quit();
        }
        self.take_dropped_files(&ctx);
        self.take_file_outcomes();
        if self.keys_to_tools() {
            self.keyboard(&ctx);
        }
        self.serve_control(&ctx, frame);
        self.reveal_materials();
        self.show_window_title(&ctx);
        let views = self.command_views();
        let mut actions = Vec::new();
        #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
        if let Some(menu) = &mut self.native_menu {
            for id in menu.update(&views, &self.keymap) {
                self.user_acted();
                self.native_command(id, &mut actions);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(note) = self.updates.poll(&mut self.prefs.updates) {
            self.toasts.push(note);
        }
        // A window too small for the app shows only that, the documents open behind
        // (docs/ui/interface.md#too-small).
        let room = ctx.content_rect().size();
        if crate::window::too_small::too_small(room) {
            let sideways = self.probe.coarse && room.x > room.y;
            egui::CentralPanel::default().show(ui, |ui| crate::window::too_small::show(ui, sideways));
            for a in actions {
                self.act(a);
            }
            self.take_notices(&ctx);
            self.follow_documents();
            self.store_preferences(&ctx);
            crate::kit::cursors::apply(&ctx);
            return;
        }
        let bar_room = self.show_title_bar(ui, &ctx, &views, &mut actions);
        // The start page has no status bar: the toasts stand above the window's bottom.
        let compact = self.form == crate::window::form::Form::Compact;
        let mut status_top = match self.docs.active() {
            // The compact arrangement's bars instead of the status bar (docs/ui/touch.md).
            Some(_) if compact => self.compact_bars(ui, bar_room, &views, &mut actions),
            Some(_) => egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui)).response.rect.top(),
            None => ctx.content_rect().bottom(),
        };
        self.ribbon_bottom = None;
        self.steps_top = None;
        self.export_pdf_when_ready(&ctx, frame);
        if self.docs.active().is_some() {
            self.document_ui(ui, &ctx, frame, &views, &mut actions);
        } else {
            egui::CentralPanel::default()
                .show(ui, |ui| crate::window::start::show(ui, &self.theme, &self.prefs.recent_files, &mut self.start, &mut actions));
        }
        // The toasts above a card risen from the bottom.
        if let Some(top) = self.compact.top.take() {
            status_top = status_top.min(top);
        }
        self.secondary_windows(&ctx, &views, &mut actions);
        drop_hint(&ctx);
        self.show_toasts(&ctx, status_top);
        self.zoom_label.show(&ctx, self.prefs.zoom);
        self.naming_dialog(&ctx);
        self.imprint_dialog(&ctx);
        self.view_picker_dialog(&ctx);
        for a in actions {
            self.act(a);
        }
        // The paint tool started by them: its panel at the next frame.
        if self.reveal_materials() {
            ctx.request_repaint();
        }
        // What the frame's actions made the editors say: shown at the next frame.
        self.take_notices(&ctx);
        self.follow_documents();
        self.store_preferences(&ctx);
        // Last: every widget has said its cursor (docs/ui/cursors.md).
        crate::kit::cursors::apply(&ctx);
    }
}

impl SkraftApp {
    /// The editors' notices (an operation failed, a file imported), every document's, as toasts.
    fn take_notices(&mut self, ctx: &egui::Context) {
        let notes: Vec<Note> =
            self.docs.iter_mut().flat_map(|d| d.editor.take_notices().into_iter().map(|n| Note::of_notice(n, d.id))).collect();
        if !notes.is_empty() {
            ctx.request_repaint();
        }
        for n in notes {
            self.toasts.push(n);
        }
    }

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
}

/// A section's name in the command palette: its menu's, small and dimmed. Its rect.
/// An image's size in pixels, read from its header.
fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

/// The mean colour of an image (sRGB), averaged in linear light on a small copy of it.
fn mean_color(bytes: &[u8]) -> Option<[u8; 3]> {
    let small = image::load_from_memory(bytes).ok()?.thumbnail(64, 64).to_rgb8();
    let linear = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let table: Vec<f64> = (0..=255u8).map(linear).collect();
    let mut sum = [0.0; 3];
    for p in small.pixels() {
        for k in 0..3 {
            sum[k] += table[p.0[k] as usize];
        }
    }
    let n = (small.width() * small.height()).max(1) as f64;
    let srgb = |l: f64| {
        let c = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
        (c.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    Some(sum.map(|s| srgb(s / n)))
}

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

/// Opens the file at `path` with the system's application for its kind.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn open_file(path: &str) {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(path).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("explorer").arg(path).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path).spawn()
    };
    if let Err(e) = result {
        log::warn!("cannot open {path}: {e}");
    }
}

/// Opens `url` in the system's browser, by its own command: egui's `open_url` needs eframe's
/// `links` feature on the desktop, and the 22 crates it brings. On Windows, no `cmd /c start`,
/// whose shell would read the `&` of a URL.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn open_url(_ctx: &egui::Context, url: String) {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&url).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &url]).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&url).spawn()
    };
    if let Err(e) = result {
        log::warn!("cannot open {url}: {e}");
    }
}

/// Opens `url` in a new tab: the page holds the unsaved work.
#[cfg(target_arch = "wasm32")]
pub(crate) fn open_url(ctx: &egui::Context, url: String) {
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_mean_colour_is_averaged_in_linear_light() {
        // Half black, half white: a mid grey of light, 188, not 128.
        let img = image::RgbImage::from_fn(64, 64, |x, _| if x < 32 { image::Rgb([0, 0, 0]) } else { image::Rgb([255, 255, 255]) });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert_eq!(mean_color(&png), Some([188, 188, 188]));
        assert_eq!(mean_color(b"not an image"), None);
    }

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

mod command_run;
mod compact;
mod dialogs;
mod documents;
mod frame;
mod keys;
mod layout;
mod remote;
mod settings;

pub(crate) use dialogs::*;
use keys::*;
use layout::*;
use settings::*;

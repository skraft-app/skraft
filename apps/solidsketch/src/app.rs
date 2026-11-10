//! The application window: title bar (top), tool bar with the tool option bar as its footer
//! (centered at the top), entity info, materials and outliner (left), status bar with the
//! measurements box (bottom), viewport (centre). Every action goes through the command registry (docs/ARCHITECTURE.md §7 quater):
//! menus (native on macOS, ☰ elsewhere), title bar, tool bar, keyboard (SketchUp's shortcuts,
//! changeable), the command palette and the control channel.

use solidsketch_i18n::{ellipsis, t};
use solidsketch_model::kernel::KernelService;
use solidsketch_model::{Boolean, Document, LengthUnit, Operation, Settings};
use solidsketch_theme::{Choice, Mode, Palette as UiPalette, Theme};
use solidsketch_tools::{Editor, Key, StandardView, ToolInput, ToolKind};

use crate::commands::{self, COMMANDS, Keymap, Menu, Shortcut};
#[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
use crate::native_menu::NativeMenu;
use crate::file::{self, Inbox};
use crate::panels::{self, Action, CommandView, InfoState};
use crate::prefs::{Navigation, Preferences};
use crate::titlebar;
use crate::viewport::{self, Viewport};

/// Width of the floating side panel (outliner and entity info), in points.
const SIDE_WIDTH: f32 = 270.0;

pub struct SolidSketchApp {
    ctx: egui::Context,
    editor: Editor,
    viewport: Viewport,
    info: InfoState,
    /// What is being typed into the measurements box.
    vcb: String,
    /// Where the document was opened from or saved to.
    path: Option<String>,
    /// The history revision when last saved or opened (the document is modified if it differs).
    saved_at: u64,
    inbox: Inbox,
    message: Option<String>,
    prefs: Preferences,
    /// The preferences as last stored: they are written again when they differ.
    stored_prefs: Preferences,
    prefs_window: Option<PrefsTab>,
    /// The shortcuts in use (the defaults with the user's changes).
    keymap: Keymap,
    /// The command whose shortcut the Raccourcis tab waits for.
    capturing: Option<&'static str>,
    palette: Option<Palette>,
    theme: Theme,
    /// The palette egui's styles were installed for (`None`: not yet).
    styled: Option<UiPalette>,
    /// The copy modifier is down, and nothing else happened since it went down: released, it
    /// is a tap, which toggles copying (SketchUp).
    copy_down: bool,
    copy_tap: bool,
    /// Shows the Débogage menu ([`debug_menu_wanted`]).
    debug_menu: bool,
    #[cfg(not(target_arch = "wasm32"))]
    control: Option<crate::control::Control>,
    /// The macOS menu bar (`None` if it could not be built: the ☰ menu then).
    #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
    native_menu: Option<NativeMenu>,
    /// Keys given back to egui on the next frame ([`Self::native_command`]).
    pending_keys: Vec<egui::Event>,
}

/// The command palette (Cmd/Ctrl+K): what is typed, and the line picked.
#[derive(Default)]
struct Palette {
    query: String,
    picked: usize,
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
}

/// Where the kernel runs (docs/ARCHITECTURE.md §7 ter): its own thread on the desktop, a web
/// worker in the browser. Either wakes the UI when a result is ready.
fn kernel_service(ctx: &egui::Context) -> Box<dyn KernelService> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let ctx = ctx.clone();
        Box::new(solidsketch_model::kernel::Threaded::new(Some(std::sync::Arc::new(move || ctx.request_repaint()))))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let ctx = ctx.clone();
        Box::new(crate::worker::WebKernel::new(std::rc::Rc::new(move || ctx.request_repaint())))
    }
}

/// The tool a `tool.*` command starts.
fn tool_of(id: &str) -> Option<ToolKind> {
    Some(match id {
        "tool.select" => ToolKind::Select,
        "tool.box" => ToolKind::Box,
        "tool.cylinder" => ToolKind::Cylinder,
        "tool.move" => ToolKind::Move,
        "tool.rotate" => ToolKind::Rotate,
        "tool.paint" => ToolKind::Paint,
        "tool.section" => ToolKind::Section,
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
        let mut editor = Editor::new(Document::with_settings(prefs.new_model.clone()), kernel_service(&cc.egui_ctx));
        editor.set_tool_settings(prefs.tool_options.clone());
        editor.set_machinings_switch(prefs.show_machinings);
        #[cfg(not(target_arch = "wasm32"))]
        let (control, error) = {
            let ctx = cc.egui_ctx.clone();
            match crate::control::Control::from_env(move || ctx.request_repaint()) {
                Some(Ok(c)) => {
                    log::info!("control channel on 127.0.0.1:{}", c.port);
                    (Some(c), error)
                }
                Some(Err(e)) => (None, Some(t!("status.control_failed", error = e))),
                None => (None, error),
            }
        };
        // Its native menu is added below, on macOS.
        #[cfg_attr(not(all(target_os = "macos", not(target_arch = "wasm32"))), allow(unused_mut))]
        let mut app = SolidSketchApp {
            ctx: cc.egui_ctx.clone(),
            editor,
            viewport: Viewport::new(),
            info: InfoState::default(),
            vcb: String::new(),
            path: None,
            saved_at: 0,
            inbox: Inbox::default(),
            message: error,
            keymap: Keymap::new(&prefs.shortcuts),
            stored_prefs: prefs.clone(),
            prefs,
            prefs_window: None,
            capturing: None,
            palette: None,
            theme: Theme::light(),
            styled: None,
            copy_down: false,
            copy_tap: false,
            debug_menu: debug_menu_wanted(),
            #[cfg(not(target_arch = "wasm32"))]
            control,
            #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
            native_menu: None,
            pending_keys: Vec::new(),
        };
        #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
        {
            let views = app.command_views();
            app.native_menu = NativeMenu::new(&cc.egui_ctx, &views, &app.keymap, app.debug_menu)
                .inspect_err(|e| log::warn!("no native menu bar: {e}"))
                .ok();
        }
        app
    }

    /// The menus are the native ones (macOS desktop).
    fn native_menus(&self) -> bool {
        #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
        return self.native_menu.is_some();
        #[cfg(not(all(target_os = "macos", not(target_arch = "wasm32"))))]
        false
    }

    /// A command chosen in the native menu bar. AppKit takes its ⌘ shortcut before egui sees
    /// the key, so it goes back to egui as that key when it is egui's: while a shortcut is
    /// being captured, and for undo, redo and select all in a text field.
    #[cfg_attr(not(all(target_os = "macos", not(target_arch = "wasm32"))), allow(dead_code))]
    fn native_command(&mut self, id: &'static str, actions: &mut Vec<Action>) {
        let text = self.ctx.egui_wants_keyboard_input() && matches!(id, "edit.undo" | "edit.redo" | "edit.select_all");
        if let Some(s) = self.keymap.get(id)
            && (self.capturing.is_some() || text)
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

    /// Changes the document's settings (an undoable step).
    fn set_settings(&mut self, settings: Settings) {
        if &settings != self.editor.document().settings() {
            let _ = self.editor.apply(Operation::SetSettings(settings));
        }
    }

    fn modified(&self) -> bool {
        self.editor.history().revision() != self.saved_at
    }

    // ---- commands ------------------------------------------------------------------------------

    /// Can `id` run now?
    fn enabled(&self, id: &str) -> bool {
        let e = &self.editor;
        let has_sel = !e.selection().is_empty();
        let section = e.document().settings().display.section.is_some();
        match id {
            "edit.undo" => e.history().can_undo(),
            "edit.redo" => e.history().can_redo(),
            "edit.erase" | "edit.make_assembly" | "edit.explode" | "edit.make_unique" => has_sel,
            "view.section" | "view.section_clear" => section,
            "debug.crash_kernel" => self.debug_menu,
            _ => commands::find(id).is_some(),
        }
    }

    /// A toggle's state, or which tool is on.
    fn checked(&self, id: &str) -> Option<bool> {
        let d = &self.editor.document().settings().display;
        match id {
            "view.parallel" => Some(self.editor.camera.is_parallel()),
            "view.grid" => Some(d.grid),
            "view.axes" => Some(d.axes),
            "view.silhouettes" => Some(d.silhouettes),
            "view.section" => Some(d.section_active),
            "view.materials" => Some(self.prefs.show_materials),
            _ => tool_of(id).map(|k| self.editor.tool_kind() == k),
        }
    }

    /// Its label now (« Annuler Déplacer »).
    fn label(&self, c: &commands::Command) -> String {
        let h = self.editor.history();
        match c.id {
            "edit.undo" => h.undo_op().map_or_else(|| (c.label)(), |o| t!("menu.edit.undo_op", op = o.label())),
            "edit.redo" => h.redo_op().map_or_else(|| (c.label)(), |o| t!("menu.edit.redo_op", op = o.label())),
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
        if let Some(k) = tool_of(id) {
            self.vcb.clear();
            self.editor.set_tool(k);
            return Ok(());
        }
        if let Some(v) = view_of(id) {
            self.editor.standard_view(v);
            return Ok(());
        }
        let mut s = self.editor.document().settings().clone();
        let e = &mut self.editor;
        match id {
            "file.new" => self.new_document(),
            "file.open" => file::open(&self.inbox, &self.ctx),
            "file.save" => self.save(false),
            "file.save_as" => self.save(true),
            "edit.undo" => e.undo(),
            "edit.redo" => e.redo(),
            "edit.select_all" => e.select_all(),
            "edit.erase" => e.delete_selection(),
            "edit.make_assembly" => e.make_assembly(),
            "edit.explode" => e.explode(),
            "edit.make_unique" => e.make_unique(),
            "app.command_palette" => self.palette = Some(Palette::default()),
            "app.preferences" => self.prefs_window = Some(self.prefs_window.unwrap_or(PrefsTab::Model)),
            "view.parallel" => {
                let p = !e.camera.is_parallel();
                e.set_parallel(p);
            }
            "view.grid" | "view.axes" | "view.silhouettes" | "view.section" | "view.section_clear" => {
                let d = &mut s.display;
                match id {
                    "view.grid" => d.grid = !d.grid,
                    "view.axes" => d.axes = !d.axes,
                    "view.silhouettes" => d.silhouettes = !d.silhouettes,
                    "view.section" => d.section_active = !d.section_active,
                    _ => {
                        d.section = None;
                        d.section_active = false;
                    }
                }
                self.set_settings(s);
            }
            "view.materials" => self.prefs.show_materials = !self.prefs.show_materials,
            "view.zoom_extents" => e.zoom_extents(),
            "debug.crash_kernel" => e.crash_kernel(),
            _ => return Err(t!("status.command_unknown", id = id)),
        }
        Ok(())
    }

    fn act(&mut self, a: Action) {
        let e = &mut self.editor;
        match a {
            Action::Command(id) => {
                if let Err(m) = self.run(id) {
                    self.message = Some(m);
                }
            }
            Action::SelectPath(p) => e.select_path(&p),
            Action::Apply(op) => {
                let _ = e.apply(op);
            }
            Action::ToolOption(key, value) => e.set_tool_option(key, value),
            Action::PartScope(s) => e.set_part_scope(s),
            Action::SelectMachinings(ids) => e.select(ids),
            Action::HoverMachining(m) => e.set_listed_hover(m),
            Action::ToggleMachinings => {
                self.prefs.show_machinings = !self.prefs.show_machinings;
                e.set_machinings_switch(self.prefs.show_machinings);
            }
            Action::PaintWith(m) => {
                e.set_paint_material(m);
                if e.tool_kind() != ToolKind::Paint {
                    self.vcb.clear();
                    e.set_tool(ToolKind::Paint);
                }
            }
        }
    }

    fn new_document(&mut self) {
        self.editor.open(Document::with_settings(self.prefs.new_model.clone()));
        self.path = None;
        self.saved_at = 0;
    }

    fn save(&mut self, ask: bool) {
        let path = if ask { None } else { self.path.clone() };
        match file::save(self.editor.document(), path.as_deref()) {
            Ok(Some(p)) => {
                self.path = Some(p);
                self.saved_at = self.editor.history().revision();
                self.message = Some(t!("status.saved"));
            }
            Ok(None) => {}
            Err(e) => self.message = Some(t!("status.save_failed", error = e)),
        }
    }

    /// A tap of the copy modifier alone (Option on macOS, Ctrl elsewhere) toggles the tool's
    /// copying. Toggling on release, and only if no key, click or scroll came in between,
    /// leaves Ctrl+Z and the like alone.
    fn copy_modifier(&mut self, ctx: &egui::Context) {
        let (mods, other) = ctx.input(|i| (i.modifiers, i.events.iter().any(interrupts_tap)));
        let down = viewport::modifiers(ctx, mods).copy;
        if down && (!self.copy_down || other) {
            self.copy_tap = !other;
        }
        if !down && self.copy_down && self.copy_tap && !ctx.egui_wants_keyboard_input() {
            self.editor.input(ToolInput::Key { key: Key::Copy, mods: Default::default() });
        }
        if !down {
            self.copy_tap = false;
        }
        self.copy_down = down;
    }

    /// Keyboard: the tool's keys, the measurements box fed from anywhere (SketchUp), then the
    /// commands' shortcuts.
    fn keyboard(&mut self, ctx: &egui::Context) {
        self.copy_modifier(ctx);
        if ctx.egui_wants_keyboard_input() || self.capturing.is_some() || self.palette.is_some() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let vcb_open = self.editor.vcb().is_some();
        for ev in events {
            match ev {
                egui::Event::Text(t) => {
                    // `*5` and `/5` (arrays), `@` (the current value), `(`, and points `[x;y;z]`,
                    // `<dx;dy;dz>` start a value too.
                    let starts_value = t.starts_with(|c: char| {
                        c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ',' | '*' | '/' | '@' | '(' | '[' | '<' | ';')
                    });
                    if vcb_open && (!self.vcb.is_empty() || starts_value) {
                        self.vcb.push_str(&t);
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let typing = !self.vcb.is_empty();
                    let m = viewport::modifiers(ctx, modifiers);
                    let tool_key = |k: Key| ToolInput::Key { key: k, mods: m };
                    use egui::Key as K;
                    let plain = !(modifiers.command || modifiers.alt || modifiers.ctrl);
                    match key {
                        K::Enter if typing => {
                            let text = std::mem::take(&mut self.vcb);
                            self.editor.input(ToolInput::Vcb(text));
                        }
                        K::Backspace if typing => {
                            self.vcb.pop();
                        }
                        K::Escape if typing => self.vcb.clear(),
                        _ if typing => {}
                        K::Escape => self.editor.input(tool_key(Key::Escape)),
                        K::Delete | K::Backspace if plain => self.editor.input(tool_key(Key::Delete)),
                        K::ArrowLeft => self.editor.input(tool_key(Key::Left)),
                        K::ArrowRight => self.editor.input(tool_key(Key::Right)),
                        K::ArrowUp => self.editor.input(tool_key(Key::Up)),
                        K::ArrowDown => self.editor.input(tool_key(Key::Down)),
                        K::Tab => self.editor.input(tool_key(Key::Tab)),
                        _ => {
                            if let Some(id) = self.keymap.command_for(key, modifiers) {
                                self.act(Action::Command(id));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// The command palette (Cmd/Ctrl+K): type to filter, ↑↓ to pick, Entrée to run.
    fn palette_window(&mut self, ctx: &egui::Context, views: &[CommandView], actions: &mut Vec<Action>) {
        let Some(p) = &mut self.palette else { return };
        let found: Vec<&CommandView> = views
            .iter()
            .filter(|v| v.enabled && (v.menu != Menu::Debug || self.debug_menu))
            .filter(|v| commands::find(v.id).is_some_and(|c| commands::matches(&p.query, c)))
            .collect();
        let (up, down, enter, escape) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
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
        // A fixed id: the title changes with the language.
        egui::Window::new(t!("palette.title"))
            .id(egui::Id::unique("command_palette"))
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .fixed_size([420.0, 0.0])
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut p.query).hint_text(ellipsis(t!("palette.hint"))).desired_width(f32::INFINITY));
                r.request_focus();
                if r.changed() {
                    p.picked = 0;
                }
                ui.separator();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (k, v) in found.iter().enumerate() {
                        let text = match &v.shortcut {
                            Some(s) => format!("{}    {s}", v.label),
                            None => v.label.clone(),
                        };
                        let row = ui.selectable_label(k == p.picked, text).on_hover_text(v.id);
                        if k == p.picked && (up || down) {
                            row.scroll_to_me(None);
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
                });
                ui.separator();
                match tab {
                    PrefsTab::General => general_settings(ui, &mut self.prefs),
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

    fn model_settings(&mut self, ui: &mut egui::Ui) {
        let current = self.editor.document().settings().clone();
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
            ui.label(t!("prefs.model.silhouettes"));
            ui.checkbox(&mut s.display.silhouettes, "").on_hover_text(t!("prefs.model.silhouettes_tip"));
            ui.end_row();
            ui.label(t!("prefs.model.background"));
            ui.color_edit_button_srgb(&mut s.display.background);
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

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let status = match &self.message {
                Some(m) if !self.editor.busy() => m.clone(),
                _ => self.editor.status(),
            };
            ui.label(status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, value) = self.editor.vcb().unwrap_or_else(|| (t!("status.measurements"), String::new()));
                let mut text = if self.vcb.is_empty() { value } else { self.vcb.clone() };
                let r = ui.add_enabled(
                    self.editor.vcb().is_some(),
                    egui::TextEdit::singleline(&mut text).desired_width(160.0),
                );
                if r.changed() {
                    self.vcb = text.clone();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.vcb.is_empty() {
                    let t = std::mem::take(&mut self.vcb);
                    self.editor.input(ToolInput::Vcb(t));
                }
                ui.label(label);
            });
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

    /// Answers the control channel's requests (desktop, `SOLIDSKETCH_CONTROL=1`).
    #[cfg(not(target_arch = "wasm32"))]
    fn serve_control(&mut self, frame: &eframe::Frame) {
        use crate::control::{Reply, Request};
        use serde_json::json;
        let Some(control) = &self.control else { return };
        for p in control.take() {
            let reply = match &p.request {
                Request::Commands => {
                    let list: Vec<_> = self
                        .command_views()
                        .into_iter()
                        .map(|v| json!({ "id": v.id, "label": v.label, "enabled": v.enabled, "checked": v.checked, "shortcut": v.shortcut }))
                        .collect();
                    Reply::json(200, json!(list))
                }
                Request::Run(id) => match self.run(id) {
                    Ok(()) => Reply::json(200, json!({ "ok": true, "status": self.editor.status() })),
                    Err(e) => Reply::json(400, json!({ "ok": false, "error": e })),
                },
                Request::State => {
                    let e = &self.editor;
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
                            "modified": self.modified(),
                            "path": self.path,
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

impl eframe::App for SolidSketchApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.append(&mut self.pending_keys);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        // What the kernel finished since the last frame (it wakes the UI when it does).
        self.editor.poll();
        if let Some((name, text)) = self.inbox.take() {
            match Document::from_json(&text) {
                Ok(doc) => {
                    self.editor.open(doc);
                    self.path = Some(name);
                    self.saved_at = 0;
                }
                Err(e) => self.message = Some(t!("status.open_failed", file = name, error = e)),
            }
        }
        // Any input clears the last app message.
        if ctx.input(|i| !i.events.is_empty() && i.pointer.any_pressed()) {
            self.message = None;
        }
        self.keyboard(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.serve_control(frame);

        let name = self.path.as_deref().and_then(|p| p.rsplit(['/', '\\']).next()).map(str::to_owned).unwrap_or_else(|| t!("window.untitled"));
        let modified = self.modified();
        // Still the window's title where the OS shows it (Window menu, Mission Control).
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("SolidSketch — {name}{}", if modified { " •" } else { "" })));

        let views = self.command_views();
        let mut actions = Vec::new();
        #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
        if let Some(menu) = &mut self.native_menu {
            for id in menu.update(&views, &self.keymap, self.debug_menu) {
                self.native_command(id, &mut actions);
            }
        }
        let bar = titlebar::Bar {
            name: &name,
            path: self.path.as_deref(),
            modified,
            integrated: crate::INTEGRATED_TITLEBAR,
            native_menus: self.native_menus(),
            debug_menu: self.debug_menu,
        };
        titlebar::show(ui, &bar, &views, &mut actions);
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        // The 3D view fills everything between the title bar and the status bar; the tools (with
        // the active tool's option bar as their footer) and the side panel float over it, so
        // opening or resizing them never resizes the view.
        let theme = self.theme;
        let view = egui::CentralPanel::no_frame()
            .show(ui, |ui| self.viewport.ui(ui, frame, &mut self.editor, &self.prefs.navigation, &theme))
            .response
            .rect;
        let margin = 8.0;
        let card = |ui: &egui::Ui| egui::Frame::window(ui.style()).inner_margin(6.0);
        let show_materials = self.prefs.show_materials;
        let side = egui::Area::new(egui::Id::unique("side"))
            .fixed_pos(view.left_top() + egui::vec2(margin, margin))
            .constrain_to(view)
            .show(&ctx, |ui| {
                card(ui).show(ui, |ui| {
                    ui.set_width(SIDE_WIDTH);
                    // Fixed ids: the titles change with the language.
                    let title = |key: String| egui::RichText::new(key).strong();
                    egui::CollapsingHeader::new(title(t!("panel.entity_info.title"))).id_salt("entity_info").default_open(true).show(ui, |ui| {
                        panels::info(ui, &self.editor, &mut self.info, &mut actions);
                    });
                    if show_materials {
                        ui.separator();
                        egui::CollapsingHeader::new(title(t!("panel.materials.title"))).id_salt("materials").default_open(true).show(ui, |ui| {
                            panels::materials(ui, &self.editor, &mut self.info, &mut actions);
                        });
                    }
                    ui.separator();
                    egui::CollapsingHeader::new(title(t!("panel.outliner.title"))).id_salt("outliner_header").default_open(true).show(ui, |ui| {
                        // Grows with its content down to the bottom of the view, then scrolls.
                        // An `Area` only offers last frame's size as available room, so the
                        // height is forced (`min_scrolled_height`) and auto-shrink trims it.
                        let room = (view.bottom() - margin - 12.0 - ui.cursor().top()).max(80.0);
                        egui::ScrollArea::vertical().id_salt("outliner").max_height(room).min_scrolled_height(room).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            panels::outliner(ui, &self.editor, &mut actions);
                        });
                    });
                });
            })
            .response
            .rect;
        // Centered over the view, but never over the side panel (its width is last frame's).
        let tools_id = egui::Id::unique("tools");
        let tools_width = ctx.memory(|m| m.area_rect(tools_id)).map_or(0.0, |r| r.width());
        let tools_left = (view.center().x - tools_width / 2.0).max(side.right() + margin);
        let tools = egui::Area::new(tools_id)
            .fixed_pos(egui::pos2(tools_left, view.top() + margin))
            .constrain_to(view)
            .show(&ctx, |ui| {
                card(ui).show(ui, |ui| {
                    // Scrolls only when the window is too narrow for all the buttons.
                    let room = view.right() - margin - 12.0 - tools_left;
                    egui::ScrollArea::horizontal().max_width(room).min_scrolled_width(room).show(ui, |ui| {
                        ui.horizontal(|ui| panels::toolbar(ui, &views, &mut actions));
                    });
                    ui.separator();
                    // Centered under the buttons: egui lays a row out from the left, so the
                    // row is shifted by half the room it left last frame.
                    let width_id = tools_id.with("options_width");
                    let width = ui.data(|d| d.get_temp::<f32>(width_id)).unwrap_or(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                        let used = ui.scope(|ui| panels::tool_options(ui, &self.editor, &mut actions)).response.rect.width();
                        if used != width {
                            ui.data_mut(|d| d.insert_temp(width_id, used));
                            ui.ctx().request_repaint();
                        }
                    });
                });
            })
            .response
            .rect;
        // What zoom extents frames: below the tool bar, right of the side panel, in viewport pixels.
        let free_min = egui::vec2(side.right() + margin - view.left(), tools.bottom() + margin - view.top());
        let free_max = egui::vec2(view.width(), view.height());
        self.editor.set_free_area(
            glam::DVec2::new(free_min.x as f64, free_min.y as f64),
            glam::DVec2::new(free_max.x as f64, free_max.y as f64),
        );

        self.preferences_window(&ctx);
        self.palette_window(&ctx, &views, &mut actions);
        for a in actions {
            self.act(a);
        }
        // A tool's option changed (in the bar, or by Tab): kept with the preferences.
        if self.prefs.tool_options != *self.editor.tool_settings() {
            self.prefs.tool_options = self.editor.tool_settings().clone();
        }
        if self.prefs != self.stored_prefs {
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
    });
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
        ui.label(t!("prefs.interface.panels"));
        ui.checkbox(&mut prefs.show_materials, t!("panel.materials.title"));
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.weak(t!("prefs.interface.background_note"));
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
fn interrupts_tap(e: &egui::Event) -> bool {
    use egui::Key as K;
    let modifier = |k: &K| {
        matches!(k, K::ShiftLeft | K::ShiftRight | K::ControlLeft | K::ControlRight | K::AltLeft | K::AltRight | K::SuperLeft | K::SuperRight)
    };
    match e {
        egui::Event::Key { key, physical_key, .. } => !(modifier(key) || physical_key.as_ref().is_some_and(modifier)),
        egui::Event::PointerButton { .. } | egui::Event::MouseWheel { .. } => true,
        _ => false,
    }
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

//! The application window: menus, tool bar (left), outliner and entity info (right), status
//! bar with the measurements box (bottom), viewport (centre). Keyboard: SketchUp's shortcuts.

use solidsketch_model::kernel::KernelService;
use solidsketch_model::{Boolean, Document, LengthUnit, Operation, Settings};
use solidsketch_tools::{Editor, Key, StandardView, ToolInput, ToolKind};

use crate::file::{self, Inbox};
use crate::panels::{self, Action, InfoState};
use crate::prefs::{Navigation, Preferences};
use crate::viewport::{self, Viewport};

/// Width of the floating side panel (outliner and entity info), in points.
const SIDE_WIDTH: f32 = 270.0;

pub struct SolidSketchApp {
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
    /// The copy modifier is down, and nothing else happened since it went down: released, it
    /// is a tap, which toggles copying (SketchUp).
    copy_down: bool,
    copy_tap: bool,
    /// Shows the Débogage menu ([`debug_menu_wanted`]).
    debug_menu: bool,
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
    Model,
    Navigation,
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

impl SolidSketchApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        cc.egui_ctx.set_theme(egui::Theme::Light);
        let (prefs, error) = Preferences::load();
        let mut editor = Editor::new(Document::with_settings(prefs.new_model.clone()), kernel_service(&cc.egui_ctx));
        editor.set_tool_settings(prefs.tool_options.clone());
        SolidSketchApp {
            editor,
            viewport: Viewport::new(),
            info: InfoState::default(),
            vcb: String::new(),
            path: None,
            saved_at: 0,
            inbox: Inbox::default(),
            message: error,
            stored_prefs: prefs.clone(),
            prefs,
            prefs_window: None,
            copy_down: false,
            copy_tap: false,
            debug_menu: debug_menu_wanted(),
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

    fn act(&mut self, a: Action) {
        let e = &mut self.editor;
        match a {
            Action::Tool(k) => {
                self.vcb.clear();
                e.set_tool(k);
            }
            Action::Undo => e.undo(),
            Action::Redo => e.redo(),
            Action::Erase => e.delete_selection(),
            Action::MakeAssembly => e.make_assembly(),
            Action::Explode => e.explode(),
            Action::MakeUnique => e.make_unique(),
            Action::ZoomExtents => e.zoom_extents(),
            Action::ToggleParallel => {
                let p = !e.camera.is_parallel();
                e.set_parallel(p);
            }
            Action::SelectPath(p) => e.select_path(&p),
            Action::Apply(op) => {
                let _ = e.apply(op);
            }
            Action::CrashKernel => e.crash_kernel(),
            Action::ToolOption(key, value) => e.set_tool_option(key, value),
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
                self.message = Some("Enregistré".into());
            }
            Ok(None) => {}
            Err(e) => self.message = Some(format!("Enregistrement impossible : {e}")),
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

    /// Keyboard: shortcuts, and the measurements box fed from anywhere (SketchUp).
    fn keyboard(&mut self, ctx: &egui::Context) {
        self.copy_modifier(ctx);
        if ctx.egui_wants_keyboard_input() {
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
                        K::Delete | K::Backspace => self.editor.input(tool_key(Key::Delete)),
                        K::ArrowLeft => self.editor.input(tool_key(Key::Left)),
                        K::ArrowRight => self.editor.input(tool_key(Key::Right)),
                        K::ArrowUp => self.editor.input(tool_key(Key::Up)),
                        K::ArrowDown => self.editor.input(tool_key(Key::Down)),
                        K::Tab => self.editor.input(tool_key(Key::Tab)),
                        K::Z if modifiers.command && modifiers.shift => self.editor.redo(),
                        K::Z if modifiers.command => self.editor.undo(),
                        K::Y if modifiers.command => self.editor.redo(),
                        K::A if modifiers.command => self.editor.select_all(),
                        K::S if modifiers.command => self.save(modifiers.shift),
                        K::O if modifiers.command => file::open(&self.inbox, ctx),
                        K::N if modifiers.command => self.new_document(),
                        K::Num1 if modifiers.command => self.editor.standard_view(StandardView::Top),
                        K::Num2 if modifiers.command => self.editor.standard_view(StandardView::Front),
                        K::Num3 if modifiers.command => self.editor.standard_view(StandardView::Right),
                        K::Num4 if modifiers.command => self.editor.standard_view(StandardView::Back),
                        K::Num5 if modifiers.command => self.editor.standard_view(StandardView::Left),
                        K::Num6 if modifiers.command => self.editor.standard_view(StandardView::Iso),
                        K::Comma if modifiers.command => self.prefs_window = Some(self.prefs_window.unwrap_or(PrefsTab::Model)),
                        _ if modifiers.command || modifiers.alt => {}
                        K::Z if modifiers.shift => self.editor.zoom_extents(),
                        K::Space => self.act(Action::Tool(ToolKind::Select)),
                        K::R => self.act(Action::Tool(ToolKind::Box)),
                        K::C => self.act(Action::Tool(ToolKind::Cylinder)),
                        K::M => self.act(Action::Tool(ToolKind::Move)),
                        K::Q => self.act(Action::Tool(ToolKind::Rotate)),
                        K::G => self.editor.make_assembly(),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn menus(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Fichier", |ui| {
                if ui.button("Nouveau").clicked() {
                    self.new_document();
                }
                if ui.button("Ouvrir…").clicked() {
                    file::open(&self.inbox, &ctx);
                }
                if ui.button("Enregistrer").clicked() {
                    self.save(false);
                }
                if ui.button("Enregistrer sous…").clicked() {
                    self.save(true);
                }
            });
            ui.menu_button("Édition", |ui| {
                let e = &self.editor;
                let has_sel = !e.selection().is_empty();
                let undo = e.history().undo_op().map(|o| format!("Annuler {}", o.label())).unwrap_or("Annuler".into());
                let redo = e.history().redo_op().map(|o| format!("Rétablir {}", o.label())).unwrap_or("Rétablir".into());
                if ui.add_enabled(e.history().can_undo(), egui::Button::new(undo)).clicked() {
                    actions.push(Action::Undo);
                }
                if ui.add_enabled(e.history().can_redo(), egui::Button::new(redo)).clicked() {
                    actions.push(Action::Redo);
                }
                ui.separator();
                if ui.add_enabled(has_sel, egui::Button::new("Effacer")).clicked() {
                    actions.push(Action::Erase);
                }
                ui.separator();
                if ui.add_enabled(has_sel, egui::Button::new("Créer un assemblage").shortcut_text("G")).clicked() {
                    actions.push(Action::MakeAssembly);
                }
                if ui.add_enabled(has_sel, egui::Button::new("Éclater")).clicked() {
                    actions.push(Action::Explode);
                }
                if ui.add_enabled(has_sel, egui::Button::new("Faire une pièce à part")).clicked() {
                    actions.push(Action::MakeUnique);
                }
                ui.separator();
                if ui.add(egui::Button::new("Préférences…").shortcut_text("⌘,")).clicked() {
                    self.prefs_window = Some(PrefsTab::Model);
                }
            });
            ui.menu_button("Vue", |ui| {
                let mut parallel = self.editor.camera.is_parallel();
                if ui.checkbox(&mut parallel, "Projection parallèle").changed() {
                    actions.push(Action::ToggleParallel);
                }
                let mut s = self.editor.document().settings().clone();
                let grid = ui.checkbox(&mut s.display.grid, "Grille").changed();
                let axes = ui.checkbox(&mut s.display.axes, "Axes").changed();
                if grid || axes {
                    self.set_settings(s);
                }
                ui.separator();
                for (label, v) in [
                    ("Dessus", StandardView::Top),
                    ("Face", StandardView::Front),
                    ("Droite", StandardView::Right),
                    ("Arrière", StandardView::Back),
                    ("Gauche", StandardView::Left),
                    ("Iso", StandardView::Iso),
                ] {
                    if ui.button(label).clicked() {
                        self.editor.standard_view(v);
                    }
                }
                ui.separator();
                if ui.button("Zoom étendu").clicked() {
                    actions.push(Action::ZoomExtents);
                }
            });
            ui.menu_button("Outils", |ui| {
                for (label, k) in [
                    ("Sélection", ToolKind::Select),
                    ("Boîte", ToolKind::Box),
                    ("Cylindre", ToolKind::Cylinder),
                    ("Déplacer", ToolKind::Move),
                    ("Faire pivoter", ToolKind::Rotate),
                ] {
                    if ui.button(label).clicked() {
                        actions.push(Action::Tool(k));
                    }
                }
            });
            ui.menu_button("Solides", |ui| {
                for (label, op) in [
                    ("Union", Boolean::Union),
                    ("Soustraire", Boolean::Difference),
                    ("Intersecter", Boolean::Intersection),
                ] {
                    if ui.button(label).clicked() {
                        actions.push(Action::Tool(ToolKind::Boolean(op)));
                    }
                }
            });
            if self.debug_menu {
                ui.menu_button("Débogage", |ui| {
                    if ui.button("Faire planter le noyau").on_hover_text("Teste la reprise après une panique du noyau").clicked() {
                        actions.push(Action::CrashKernel);
                    }
                });
            }
        });
    }

    /// Préférences (⌘,): the model's display and units, saved with it, which can become the
    /// defaults of new models; and the navigation, the user's own.
    fn preferences_window(&mut self, ctx: &egui::Context) {
        let Some(mut tab) = self.prefs_window else { return };
        let mut open = true;
        egui::Window::new("Préférences")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut tab, PrefsTab::Model, "Modèle");
                    ui.selectable_value(&mut tab, PrefsTab::Navigation, "Navigation");
                });
                ui.separator();
                match tab {
                    PrefsTab::Model => self.model_settings(ui),
                    PrefsTab::Navigation => navigation_settings(ui, &mut self.prefs.navigation),
                }
            });
        self.prefs_window = open.then_some(tab);
    }

    fn model_settings(&mut self, ui: &mut egui::Ui) {
        let current = self.editor.document().settings().clone();
        let mut s = current.clone();
        ui.weak("Réglages de ce modèle, enregistrés avec lui.");
        ui.add_space(4.0);
        egui::Grid::new("model_settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.strong("Affichage");
            ui.end_row();
            ui.label("Grille");
            ui.checkbox(&mut s.display.grid, "");
            ui.end_row();
            ui.label("Axes");
            ui.checkbox(&mut s.display.axes, "");
            ui.end_row();
            ui.label("Fond");
            ui.color_edit_button_srgb(&mut s.display.background);
            ui.end_row();
            ui.strong("Unités");
            ui.end_row();
            ui.label("Longueurs");
            egui::ComboBox::from_id_salt("length_unit").selected_text(s.units.length.name()).show_ui(ui, |ui| {
                for u in LengthUnit::ALL {
                    ui.selectable_value(&mut s.units.length, u, u.name());
                }
            });
            ui.end_row();
            ui.label("Décimales");
            ui.add(egui::Slider::new(&mut s.units.decimals, 0..=6));
            ui.end_row();
            ui.label("Exemple");
            ui.weak(s.units.format(1234.5678));
            ui.end_row();
        });
        if s != current {
            self.set_settings(s.clone());
        }
        ui.add_space(8.0);
        let defaults = s == self.prefs.new_model;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!defaults, egui::Button::new("Utiliser pour les nouveaux modèles"))
                .on_hover_text("Ces réglages deviennent ceux des prochains modèles (Fichier › Nouveau)")
                .clicked()
            {
                self.prefs.new_model = s.clone();
            }
            if ui
                .add_enabled(!defaults, egui::Button::new("Reprendre les réglages par défaut"))
                .on_hover_text("Applique à ce modèle les réglages des nouveaux modèles")
                .clicked()
            {
                self.set_settings(self.prefs.new_model.clone());
            }
        });
        if defaults {
            ui.weak("Ce sont les réglages des nouveaux modèles.");
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
                let (label, value) = self.editor.vcb().unwrap_or(("Mesures", String::new()));
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
}

impl eframe::App for SolidSketchApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // What the kernel finished since the last frame (it wakes the UI when it does).
        self.editor.poll();
        if let Some((name, text)) = self.inbox.take() {
            match Document::from_json(&text) {
                Ok(doc) => {
                    self.editor.open(doc);
                    self.path = Some(name);
                    self.saved_at = 0;
                }
                Err(e) => self.message = Some(format!("{name} : {e}")),
            }
        }
        // Any input clears the last app message.
        if ctx.input(|i| !i.events.is_empty() && i.pointer.any_pressed()) {
            self.message = None;
        }
        self.keyboard(&ctx);

        let title = format!(
            "SolidSketch — {}{}",
            self.path.as_deref().and_then(|p| p.rsplit(['/', '\\']).next()).unwrap_or("Sans titre"),
            if self.modified() { " •" } else { "" }
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        let mut actions = Vec::new();
        egui::Panel::top("menu").show(ui, |ui| self.menus(ui, &mut actions));
        egui::Panel::top("tool_options").show(ui, |ui| panels::tool_options(ui, &self.editor, &mut actions));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        // The 3D view fills everything between the menu and the status bar; the tools and the
        // side panel float over it, so opening or resizing them never resizes the view.
        let view = egui::CentralPanel::no_frame().show(ui, |ui| self.viewport.ui(ui, frame, &mut self.editor, &self.prefs.navigation)).response.rect;
        let margin = 8.0;
        let card = |ui: &egui::Ui| egui::Frame::window(ui.style()).inner_margin(6.0);
        let tools = egui::Area::new(egui::Id::unique("tools"))
            .fixed_pos(view.left_top() + egui::vec2(margin, margin))
            .constrain_to(view)
            .show(&ctx, |ui| {
                card(ui).show(ui, |ui| {
                    // Scrolls only when the window is too short for all the buttons.
                    let room = view.height() - 2.0 * margin - 12.0;
                    egui::ScrollArea::vertical().max_height(room).min_scrolled_height(room).show(ui, |ui| {
                        ui.vertical_centered(|ui| panels::toolbar(ui, &self.editor, &mut actions));
                    });
                });
            })
            .response
            .rect;
        let side = egui::Area::new(egui::Id::unique("side"))
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(view.right_top() + egui::vec2(-margin, margin))
            .constrain_to(view)
            .show(&ctx, |ui| {
                card(ui).show(ui, |ui| {
                    ui.set_width(SIDE_WIDTH);
                    egui::CollapsingHeader::new(egui::RichText::new("Infos sur l'entité").strong()).default_open(true).show(ui, |ui| {
                        panels::info(ui, &self.editor, &mut self.info, &mut actions);
                    });
                    ui.separator();
                    egui::CollapsingHeader::new(egui::RichText::new("Arborescence").strong()).default_open(true).show(ui, |ui| {
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
        // What « zoom étendu » frames: between the two cards, in viewport pixels.
        let free_min = egui::vec2(tools.right() + margin - view.left(), 0.0);
        let free_max = egui::vec2(side.left() - margin - view.left(), view.height());
        self.editor.set_free_area(
            glam::DVec2::new(free_min.x as f64, free_min.y as f64),
            glam::DVec2::new(free_max.x as f64, free_max.y as f64),
        );

        self.preferences_window(&ctx);
        for a in actions {
            self.act(a);
        }
        // A tool's option changed (in the bar, or by Tab): kept with the preferences.
        if self.prefs.tool_options != *self.editor.tool_settings() {
            self.prefs.tool_options = self.editor.tool_settings().clone();
        }
        if self.prefs != self.stored_prefs {
            if let Err(e) = self.prefs.save() {
                self.message = Some(format!("Préférences non enregistrées : {e}"));
            }
            self.stored_prefs = self.prefs.clone();
        }
    }
}

fn navigation_settings(ui: &mut egui::Ui, nav: &mut Navigation) {
    egui::Grid::new("navigation").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.strong("Orbite");
        ui.end_row();
        ui.label("Vitesse");
        ui.add(egui::Slider::new(&mut nav.orbit_speed, 0.25..=4.0).logarithmic(true).fixed_decimals(2));
        ui.end_row();
        ui.label("Souris (bouton du milieu)");
        ui.horizontal(|ui| {
            ui.checkbox(&mut nav.mouse_invert_x, "inverser ↔");
            ui.checkbox(&mut nav.mouse_invert_y, "inverser ↕");
        });
        ui.end_row();
        ui.label("Trackpad (deux doigts)");
        ui.horizontal(|ui| {
            ui.checkbox(&mut nav.trackpad_invert_x, "inverser ↔");
            ui.checkbox(&mut nav.trackpad_invert_y, "inverser ↕");
        });
        ui.end_row();
        ui.strong("Zoom");
        ui.end_row();
        ui.label("Vitesse");
        ui.add(egui::Slider::new(&mut nav.zoom_speed, 0.25..=4.0).logarithmic(true).fixed_decimals(2));
        ui.end_row();
        ui.label("Molette");
        ui.checkbox(&mut nav.wheel_invert, "inverser le sens");
        ui.end_row();
        ui.label("Cible");
        ui.checkbox(&mut nav.zoom_to_cursor, "vers le curseur");
        ui.end_row();
    });
    ui.add_space(8.0);
    if ui.add_enabled(*nav != Navigation::default(), egui::Button::new("Valeurs par défaut")).clicked() {
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
}

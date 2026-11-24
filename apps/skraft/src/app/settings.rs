//! The preferences window.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PrefsTab {
    General,
    Model,
    Navigation,
    Interface,
    Shortcuts,
    /// The control channel (desktop).
    Debug,
}

impl SkraftApp {
    /// The preferences (⌘,): the language; the model's display and units, saved with it,
    /// which can become the defaults of new models; the navigation, the interface and the
    /// shortcuts, the user's own.
    pub(super) fn preferences_window(&mut self, ctx: &egui::Context) {
        let Some(mut tab) = self.prefs_window else { return };
        let mut open = true;
        // A fixed id: the title changes with the language.
        egui::Window::new(t!("prefs.title"))
            .id(prefs_window_id())
            .title_frame(crate::kit::style::window_title_frame(ctx))
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
                let workspace = self.workspace().id;
                // The ribbon's tabs (as the journal window's), their band across the window
                // under its title bar.
                let mut tabs = vec![
                    (PrefsTab::General, t!("prefs.tab.general")),
                    (PrefsTab::Model, t!("prefs.tab.model")),
                    (PrefsTab::Navigation, t!("prefs.tab.navigation")),
                    (PrefsTab::Interface, t!("prefs.tab.interface")),
                    (PrefsTab::Shortcuts, t!("prefs.tab.shortcuts")),
                ];
                if !cfg!(target_arch = "wasm32") {
                    tabs.push((PrefsTab::Debug, t!("prefs.tab.debug")));
                }
                let labels: Vec<String> = tabs.iter().map(|(_, l)| l.clone()).collect();
                let ids: Vec<egui::Id> = tabs.iter().map(|(k, _)| egui::Id::unique(("prefs_tab", *k as u8))).collect();
                let shown = tabs.iter().position(|(k, _)| *k == tab);
                // Up to the title bar's line: the window's margin and a spacing above the content.
                let m = ui.style().spacing.window_margin;
                let inset = egui::Margin { top: m.top + ui.spacing().item_spacing.y as i8, ..m };
                // The window as wide as its tabs at least (it takes its content's width).
                let width = ui.available_width().max(crate::window::ribbon::tab_strip_width(ui.painter(), &labels));
                if let Some(k) =
                    crate::window::ribbon::tab_strip(ui, &labels, &ids, shown, width, inset, crate::window::ribbon::Corners::Square, &[])
                {
                    tab = tabs[k].0;
                }
                ui.add_space(4.0);
                match tab {
                    PrefsTab::General => general_settings(ui, &mut self.prefs),
                    #[cfg(not(target_arch = "wasm32"))]
                    PrefsTab::Debug => self.debug_settings(ui),
                    #[cfg(target_arch = "wasm32")]
                    PrefsTab::Debug => {}
                    PrefsTab::Model => self.model_settings(ui),
                    PrefsTab::Navigation => crate::dialogs::nav_prefs::navigation_settings(ui, &mut self.prefs.navigation),
                    PrefsTab::Interface => interface_settings(ui, &mut self.prefs, workspace),
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
            ui.label(t!("prefs.debug.remote_control")).tip(t!("prefs.debug.remote_control_tip"));
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
            self.toasts.push(Note::error(t!("toast.control_failed"), e));
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
        // The settings window's sections: a semibold title over a line, the same labels' column.
        crate::kit::style::settings_section(ui, &t!("prefs.model.display"));
        crate::kit::style::settings_grid(ui, "model_display", |ui| {
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
            ui.checkbox(&mut s.display.silhouettes, "").tip(t!("prefs.model.silhouettes_tip"));
            ui.end_row();
            ui.label(t!("prefs.model.hide_rest_of_model"));
            ui.checkbox(&mut s.display.hide_rest_of_model, "").tip(t!("prefs.model.hide_rest_of_model_tip"));
            ui.end_row();
        });
        crate::kit::style::settings_section(ui, &t!("prefs.model.units"));
        crate::kit::style::settings_grid(ui, "model_units", |ui| {
            ui.label(t!("prefs.model.lengths"));
            egui::ComboBox::from_id_salt("length_unit")
                .icon(crate::kit::icons::combo_chevron)
                .selected_text(s.units.length.name())
                .show_ui(ui, |ui| {
                    for u in LengthUnit::ALL {
                        ui.selectable_value(&mut s.units.length, u, u.name());
                    }
                });
            ui.end_row();
            ui.label(t!("prefs.model.decimals"));
            ui.add(egui::Slider::new(&mut s.units.decimals, 0..=6));
            ui.end_row();
            ui.label(t!("prefs.model.unit_symbol"));
            ui.checkbox(&mut s.units.symbol, "").tip(t!("prefs.model.unit_symbol_tip"));
            ui.end_row();
            ui.label(t!("prefs.model.example"));
            ui.weak(s.units.format(1234.567891));
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
            if ui.add_enabled(!defaults, egui::Button::new(t!("prefs.model.use_for_new"))).tip(t!("prefs.model.use_for_new_tip")).clicked()
            {
                self.prefs.new_model = without_section(s.clone());
            }
            if ui.add_enabled(!defaults, egui::Button::new(t!("prefs.model.restore"))).tip(t!("prefs.model.restore_tip")).clicked() {
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
                    ui.label((c.label)()).tip(c.id);
                    let current = self.keymap.get(c.id);
                    let text = current.map_or("—".to_owned(), |s| s.display(mac));
                    let conflict = current.is_some_and(|s| !self.keymap.conflicts(c.id, s, workspace::share).is_empty());
                    if self.capturing == Some(c.id) {
                        ui.strong("…");
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(text);
                            if conflict {
                                ui.add(crate::kit::widgets::warning(ui));
                            }
                        });
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
}

/// The language (every text changes at once), reopening the documents, the undo histories'
/// memory.
fn general_settings(ui: &mut egui::Ui, prefs: &mut Preferences) {
    egui::Grid::new("general").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(t!("prefs.general.language"));
        let before = prefs.locale.clone();
        let system = t!("prefs.general.language_system", language = skraft_i18n::locale_name(crate::language::system()));
        egui::ComboBox::from_id_salt("locale")
            .icon(crate::kit::icons::combo_chevron)
            .selected_text(prefs.locale.as_deref().map_or_else(|| system.clone(), skraft_i18n::locale_name))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut prefs.locale, None, system);
                ui.separator();
                for l in skraft_i18n::available_locales() {
                    let name = skraft_i18n::locale_name(&l);
                    ui.selectable_value(&mut prefs.locale, Some(l), name);
                }
            });
        if prefs.locale != before {
            crate::language::apply(prefs.locale.as_deref());
        }
        ui.end_row();
        if !cfg!(target_arch = "wasm32") {
            ui.label(t!("prefs.general.reopen"));
            ui.checkbox(&mut prefs.reopen_documents, "");
            ui.end_row();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.label(t!("prefs.general.check_updates")).tip(t!("prefs.general.check_updates_tip"));
            ui.checkbox(&mut prefs.updates.check, "");
            ui.end_row();
        }
        ui.label(t!("prefs.general.history_memory")).tip(t!("prefs.general.history_memory_tip"));
        let unit = format!(" {}", t!("prefs.general.history_memory_unit"));
        ui.add(egui::DragValue::new(&mut prefs.history_memory).range(64..=65536).speed(16.0).suffix(unit));
        ui.end_row();
    });
}

pub(super) fn palette_header(ui: &mut egui::Ui, menu: Menu) -> egui::Rect {
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
pub(super) fn palette_row(ui: &mut egui::Ui, v: &CommandView, picked: bool) -> egui::Response {
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
        UiPalette::Navy => t!("prefs.interface.palettes.navy"),
        UiPalette::Warm => t!("prefs.interface.palettes.warm"),
        UiPalette::Graphite => t!("prefs.interface.palettes.graphite"),
    }
}

/// `workspace`: the shown document's, whose panels it shows or closes.
fn interface_settings(ui: &mut egui::Ui, prefs: &mut Preferences, workspace: &str) {
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
                if crate::kit::style::palette_card(ui, palette, mode, &name, prefs.palette == palette).clicked() {
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
            for id in PanelId::ALL.into_iter().filter(|id| workspace::find(Some(workspace)).has(id.command())) {
                let mut shown = prefs.layout(workspace).is_shown(id);
                if ui.checkbox(&mut shown, panel_title(id)).changed() {
                    prefs.layout_mut(workspace).toggle(id);
                }
            }
        });
        ui.end_row();
    });
}

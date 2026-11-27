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
        let r = ctx.content_rect();
        // Pinned by its title bar: changing tabs moves only its bottom edge.
        let at = (egui::Align2::CENTER_TOP, egui::pos2(r.center().x, r.top() + r.height() * 0.15));
        let window = crate::kit::style::app_window(ctx, t!("prefs.title"), prefs_window_id(), &mut open, 420.0, Some(at));
        window.show(ctx, |ui| {
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
            // The window as wide as its tabs (it takes its content's width) when the screen leaves
            // room for them; else (a phone's) as wide as it is, the tabs scrolled. Out of the
            // body's scroll, their band reaches the title bar and the window's sides.
            let tabs_width = crate::window::ribbon::tab_strip_width(ui.painter(), &labels);
            let fits = tabs_width <= crate::kit::style::window_width(ui.ctx(), f32::INFINITY);
            let width = if fits { ui.available_width().max(tabs_width) } else { ui.available_width() };
            if let Some(k) =
                crate::window::ribbon::tab_strip(ui, &labels, &ids, shown, width, inset, crate::window::ribbon::Corners::Square, &[])
            {
                tab = tabs[k].0;
            }
            ui.add_space(4.0);
            crate::kit::style::window_body(ui, |ui| match tab {
                PrefsTab::General => general_settings(ui, &mut self.prefs),
                #[cfg(not(target_arch = "wasm32"))]
                PrefsTab::Debug => self.debug_settings(ui),
                #[cfg(target_arch = "wasm32")]
                PrefsTab::Debug => {}
                PrefsTab::Model => self.model_settings(ui),
                PrefsTab::Navigation => crate::dialogs::nav_prefs::navigation_settings(ui, &mut self.prefs.navigation),
                PrefsTab::Interface => interface_settings(ui, &mut self.prefs, workspace),
                PrefsTab::Shortcuts => self.shortcut_settings(ui),
            })
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
            crate::kit::widgets::ChoiceList::new("length_unit", s.units.length.name()).show(ui, |list| {
                for u in LengthUnit::ALL {
                    list.value(&mut s.units.length, u, u.name());
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

    /// Raccourcis (docs/ui/interface.md#shortcuts): a search line, then every command under
    /// its menu's name, a row each across the window — its icon, its label, its shortcut as
    /// keys —; a click on a row waits for the next key (Échap cancels, Retour arrière removes
    /// it, a click on the row again lets go); ↺ puts back a changed one, ⚠ names the commands
    /// sharing it.
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
                    self.set_shortcut(id, None);
                    self.capturing = None;
                }
                Some((key, m)) => {
                    self.set_shortcut(id, Some(Shortcut { key, command: m.command, shift: m.shift, alt: m.alt }));
                    self.capturing = None;
                }
                None => {}
            }
        }
        ui.add(egui::Label::new(egui::RichText::new(t!("prefs.shortcuts.intro")).weak()).wrap());
        ui.add_space(crate::kit::search::GAP);
        let searched = self.shortcut_search.clone();
        crate::kit::search::field(ui, egui::Id::unique("prefs_shortcuts"), "prefs.shortcuts", &mut self.shortcut_search, |_, _| {});
        ui.add_space(crate::kit::search::GAP);
        let found: Vec<&crate::commands::Command> =
            COMMANDS.iter().filter(|c| c.menu != Menu::Debug && crate::commands::matches(&self.shortcut_search, c)).collect();
        // As high whatever the search lets through: the window does not jump as it is typed.
        let height = (ui.ctx().content_rect().height() * 0.5).clamp(160.0, 460.0);
        let mut act = None;
        let mut list =
            egui::ScrollArea::vertical().id_salt("shortcuts").auto_shrink([false, false]).min_scrolled_height(height).max_height(height);
        // A new search shows what it finds from the top (as the command palette).
        if self.shortcut_search != searched {
            list = list.vertical_scroll_offset(0.0);
        }
        list.show(ui, |ui| {
            for (k, c) in found.iter().enumerate() {
                if k == 0 || found[k - 1].menu != c.menu {
                    if k > 0 {
                        ui.add_space(6.0);
                    }
                    palette_header(ui, c.menu);
                }
                let current = self.keymap.get(c.id);
                let row = ShortcutRow {
                    command: c,
                    keys: current.map(|s| s.keys(mac)),
                    fixed: crate::commands::fixed_key(c.id),
                    capturing: self.capturing == Some(c.id),
                    changed: self.prefs.shortcuts.contains_key(c.id),
                    default: c.shortcut.and_then(Shortcut::parse).map(|s| s.display(mac)),
                    shared: current
                        .map(|s| self.keymap.conflicts(c.id, s, workspaces::share))
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|id| crate::commands::find(id).map(|c| (c.label)()))
                        .collect(),
                };
                if let Some(a) = shortcut_row(ui, &row) {
                    act = Some((c.id, a));
                }
            }
            if found.is_empty() {
                ui.weak(t!("palette.none"));
            }
        });
        match act {
            Some((id, RowAct::Capture)) => self.capturing = if self.capturing == Some(id) { None } else { Some(id) },
            Some((id, RowAct::Reset)) => {
                self.prefs.shortcuts.remove(id);
                if self.capturing == Some(id) {
                    self.capturing = None;
                }
            }
            None => {}
        }
    }

    /// Gives the command `id` the shortcut `s` (`None`: none); its default kept as no change.
    fn set_shortcut(&mut self, id: &'static str, s: Option<Shortcut>) {
        let default = crate::commands::find(id).and_then(|c| c.shortcut).and_then(Shortcut::parse);
        if s == default {
            self.prefs.shortcuts.remove(id);
        } else {
            self.prefs.shortcuts.insert(id.to_owned(), s.map_or_else(String::new, |s| s.text()));
        }
    }
}

/// What a row of the Raccourcis tab shows.
struct ShortcutRow<'a> {
    command: &'a crate::commands::Command,
    /// Its shortcut's keys, none without one.
    keys: Option<Vec<String>>,
    /// The key it answers outside the shortcuts (Delete for « Effacer »): shown, not changed.
    fixed: Option<String>,
    /// Waiting for the key to give it.
    capturing: bool,
    /// The user's, not the default.
    changed: bool,
    /// The default shortcut, as shown.
    default: Option<String>,
    /// The commands having the same shortcut in a workspace that has this one.
    shared: Vec<String>,
}

/// What a click on a row asks.
enum RowAct {
    /// Waits for the key (or lets go).
    Capture,
    /// Puts the default back.
    Reset,
}

/// A row of the Raccourcis tab, the window's width (as the command palette's rows): the
/// command's icon and label on the left; on the right its keys (lit while waiting for one, a
/// dash for none), before them ↺ when changed and ⚠ when shared; filled when hovered. A command
/// with a fixed key (« Effacer ») shows it hollow, its row neither clicked nor filled, its tip
/// saying so.
fn shortcut_row(ui: &mut egui::Ui, row: &ShortcutRow) -> Option<RowAct> {
    use crate::kit::widgets::{Keycaps, keycaps};
    const HEIGHT: f32 = 30.0;
    const PAD: f32 = 8.0;
    const ICON: f32 = 16.0;
    const BUTTON: f32 = 22.0;
    let c = row.command;
    let sense = if row.fixed.is_some() { egui::Sense::hover() } else { egui::Sense::click() };
    let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), HEIGHT), sense);
    crate::kit::handles::publish(|| format!("prefs.shortcut:{}", c.id), rect);
    let visuals = ui.visuals().clone();
    let fill = if row.capturing {
        Some(visuals.selection.bg_fill.gamma_multiply(0.5))
    } else if r.hovered() && row.fixed.is_none() {
        Some(visuals.widgets.hovered.weak_bg_fill)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, visuals.widgets.hovered.corner_radius, fill);
    }
    let y = rect.center().y;
    let mut act = None;
    // Right to left: the keys, ⚠, ↺.
    let mut right = rect.right() - PAD;
    if let Some(key) = &row.fixed {
        right = keycaps(ui, std::slice::from_ref(key), egui::pos2(right, y), Keycaps::Fixed).left();
    } else if row.capturing {
        right = keycaps(ui, &[t!("prefs.shortcuts.capture")], egui::pos2(right, y), Keycaps::Lit).left();
    } else if let Some(keys) = &row.keys {
        right = keycaps(ui, keys, egui::pos2(right, y), Keycaps::Plain).left();
    } else {
        let g = ui.painter().layout_no_wrap("—".to_owned(), egui::TextStyle::Body.resolve(ui.style()), visuals.weak_text_color());
        right -= g.size().x;
        ui.painter().galley(egui::pos2(right, y - g.size().y / 2.0), g, visuals.weak_text_color());
    }
    right -= PAD;
    if !row.shared.is_empty() && !row.capturing {
        let side = ui.text_style_height(&egui::TextStyle::Body);
        let at = egui::Rect::from_center_size(egui::pos2(right - side / 2.0, y), egui::Vec2::splat(side));
        crate::kit::widgets::warning(ui).paint_at(ui, at);
        ui.interact(at, r.id.with("shared"), egui::Sense::hover()).tip(t!("prefs.shortcuts.shared", commands = row.shared.join(", ")));
        right = at.left() - PAD / 2.0;
    }
    if row.changed {
        let at = egui::Rect::from_center_size(egui::pos2(right - BUTTON / 2.0, y), egui::Vec2::splat(BUTTON));
        let tip = match &row.default {
            Some(s) => t!("prefs.shortcuts.reset", shortcut = s),
            None => t!("prefs.shortcuts.reset_none"),
        };
        let b = crate::kit::widgets::IconButton::new(crate::kit::icons::UNDO, BUTTON, 14.0)
            .ink(visuals.weak_text_color())
            .show_at(ui, at, r.id.with("reset"))
            .tip(tip);
        if b.clicked() {
            act = Some(RowAct::Reset);
        }
        right = at.left() - PAD / 2.0;
    }
    let mut left = rect.left() + PAD;
    let ink = visuals.text_color();
    if let Some(icon) = crate::kit::icons::command(c.id) {
        let at = egui::Rect::from_center_size(egui::pos2(left + ICON / 2.0, y), egui::Vec2::splat(ICON));
        egui::Image::new(icon).tint(visuals.weak_text_color()).paint_at(ui, at);
    }
    left += ICON + PAD;
    let label = egui::WidgetText::from((c.label)()).color(ink).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (right - left).max(0.0),
        egui::TextStyle::Body,
    );
    ui.painter().galley(egui::pos2(left, y - label.size().y / 2.0), label, ink);
    if let Some(key) = &row.fixed {
        // Its only tip: no ↺ nor ⚠ on such a row.
        r.tip(t!("prefs.shortcuts.fixed", key = key));
        return None;
    }
    // No tip of its own: egui shows one at a time, and the row's would hide ↺'s and ⚠'s.
    if act.is_none() && r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        act = Some(RowAct::Capture);
    }
    act
}

/// The language (every text changes at once), reopening the documents, the undo histories'
/// memory.
fn general_settings(ui: &mut egui::Ui, prefs: &mut Preferences) {
    egui::Grid::new("general").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(t!("prefs.general.language"));
        let before = prefs.locale.clone();
        let system = t!("prefs.general.language_system", language = skraft_i18n::locale_name(crate::system::language::system()));
        let shown = prefs.locale.as_deref().map_or_else(|| system.clone(), skraft_i18n::locale_name);
        crate::kit::widgets::ChoiceList::new("locale", shown).show(ui, |list| {
            list.value(&mut prefs.locale, None, system);
            list.separator();
            for l in skraft_i18n::available_locales() {
                let name = skraft_i18n::locale_name(&l);
                list.value(&mut prefs.locale, Some(l), name);
            }
        });
        if prefs.locale != before {
            crate::system::language::apply(prefs.locale.as_deref());
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
/// The interface's zooms offered in the Preferences; ⌘+ / ⌘− may set others.
const ZOOMS: [f32; 8] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

fn interface_settings(ui: &mut egui::Ui, prefs: &mut Preferences, workspace: &str) {
    egui::Grid::new("interface").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(t!("prefs.interface.theme"));
        // The three values side by side (docs/ui/interface.md#widgets).
        let themes = [
            (Choice::System, t!("prefs.interface.system"), "system"),
            (Choice::Light, t!("prefs.interface.light"), "light"),
            (Choice::Dark, t!("prefs.interface.dark"), "dark"),
        ];
        let segments: Vec<_> = themes.iter().map(|(c, l, _)| crate::kit::widgets::Segment::new(l, "", None, prefs.theme == *c)).collect();
        let id = ui.make_persistent_id("prefs_theme");
        if let Some(i) = crate::kit::widgets::segmented(ui, id, &segments, |i| format!("prefs.theme:{}", themes[i].2)) {
            prefs.theme = themes[i].0;
        }
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
        ui.label(t!("prefs.interface.zoom"));
        let percent = |zoom: f32| t!("window.zoom", percent = (zoom * 100.0).round() as i32);
        crate::kit::widgets::ChoiceList::new("ui_zoom", percent(prefs.zoom)).show(ui, |list| {
            for zoom in ZOOMS {
                list.value(&mut prefs.zoom, zoom, percent(zoom));
            }
        });
        ui.end_row();
        ui.label(t!("prefs.interface.ribbon_labels"));
        ui.checkbox(&mut prefs.ribbon.labels, "");
        ui.end_row();
        ui.label(t!("prefs.interface.panels"));
        ui.vertical(|ui| {
            for id in PanelId::ALL.into_iter().filter(|id| workspaces::find(Some(workspace)).has(id.command())) {
                let mut shown = prefs.layout(workspace).is_shown(id);
                if ui.checkbox(&mut shown, panel_title(id)).changed() {
                    prefs.layout_mut(workspace).toggle(id);
                }
            }
        });
        ui.end_row();
    });
}

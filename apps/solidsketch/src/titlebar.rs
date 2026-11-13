//! The title bar (docs/ARCHITECTURE.md §7 quinquies): one band at the top of the window with
//! undo and redo, the file tabs (§6 octies, [`crate::tabs`]), the view's toggles, the command search and the preferences,
//! every button a command of the registry.
//!
//! On the macOS desktop it is the window's title bar: the OS bar is hidden, the content runs
//! under it ([`native_options`](crate::native_options)), room is left for the traffic lights,
//! and its empty space moves the window (a double-click zooms it); the menus are the native
//! ones ([`crate::native_menu`]). Elsewhere (web, Windows, Linux) the OS frame is kept and the
//! menus are behind the ☰ button.

use solidsketch_i18n::{ellipsis, t};

use crate::commands::Menu;
use crate::icons;
use crate::panels::{self, Action, CommandView};
use crate::tabs::{self, Tab};

/// The bar's height, in points.
pub const HEIGHT: f32 = 34.0;
/// Room for the macOS traffic lights at the bar's left end.
const TRAFFIC_LIGHTS: f32 = 76.0;
/// The command search's widest.
const SEARCH_WIDTH: f32 = 240.0;
/// Below this width, the search is only its icon.
const SEARCH_MIN: f32 = 120.0;

/// What the bar shows besides the commands.
pub struct Bar<'a> {
    /// The file tabs, and the one shown (none: the start page).
    pub tabs: &'a [Tab],
    pub shown: Option<usize>,
    /// A tab's menu offers to show its file (desktop).
    pub reveal: bool,
    /// The bar is the window's title bar (macOS desktop): room for the traffic lights, and
    /// its empty space moves the window.
    pub integrated: bool,
    /// The menus are the native ones: no ☰ button.
    pub native_menus: bool,
    /// The Débogage menu is shown.
    pub debug_menu: bool,
}

/// The bar's panel, at the top of the window.
pub fn show(ui: &mut egui::Ui, bar: &Bar, views: &[CommandView], actions: &mut Vec<Action>) {
    let frame = egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 0));
    egui::Panel::top("title_bar").exact_size(HEIGHT).resizable(false).frame(frame).show(ui, |ui| {
        if bar.integrated {
            move_window(ui);
        }
        ui.horizontal_centered(|ui| contents(ui, bar, views, actions));
    });
}

/// The bar's empty space moves the window, a double-click zooms it. Registered before the
/// bar's widgets, which take the pointer where they are.
fn move_window(ui: &egui::Ui) {
    let r = ui.interact(ui.max_rect(), egui::Id::unique("title_bar_drag"), egui::Sense::click_and_drag());
    if r.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if r.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

/// The view's toggles, after the standard views' menu.
const TOGGLES: [&str; 7] = ["view.parallel", "view.grid", "view.axes", "view.guides", "view.silhouettes", "view.section", "view.materials"];
/// Room kept for the tabs before the search field narrows.
const TABS_MIN: f32 = 160.0;

fn contents(ui: &mut egui::Ui, bar: &Bar, views: &[CommandView], actions: &mut Vec<Action>) {
    let find = |id: &str| views.iter().find(|v| v.id == id);
    // In full screen the traffic lights are gone.
    if bar.integrated && !ui.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        ui.add_space(TRAFFIC_LIGHTS - 8.0);
    }
    if !bar.native_menus {
        menus(ui, bar, views, actions);
        ui.add_space(4.0);
    }
    for id in ["edit.undo", "edit.redo"] {
        if let Some(v) = find(id) {
            command_button(ui, id_icon(id), v, v.checked == Some(true), &v.tip(), actions);
        }
    }
    ui.separator();
    // The right end first (right to left): the preferences, the search, the view's toggles;
    // the tabs take what is left between.
    let row = ui.available_rect_before_wrap();
    let right = ui
        .scope_builder(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            if let Some(v) = find("app.preferences") {
                command_button(ui, icons::SETTINGS, v, false, &v.tip(), actions);
            }
            // The toggles' width, as egui spaces the buttons.
            let toggles = (TOGGLES.len() + 1) as f32 * (26.0 + ui.spacing().item_spacing.x) + 12.0;
            if let Some(v) = find("app.command_palette") {
                let room = ui.available_width() - toggles - TABS_MIN;
                if room >= SEARCH_MIN {
                    if search(ui, v, room.min(SEARCH_WIDTH)) {
                        actions.push(Action::Command(v.id));
                    }
                } else {
                    command_button(ui, icons::SEARCH, v, false, &v.tip(), actions);
                }
            }
            ui.separator();
            for id in TOGGLES.iter().rev() {
                let Some(v) = find(id) else { continue };
                // The perspective button is lit in perspective (the command toggles parallel).
                let (lit, tip) = if *id == "view.parallel" {
                    (v.checked == Some(false), t!("toolbar.perspective"))
                } else {
                    (v.checked == Some(true), v.tip())
                };
                command_button(ui, id_icon(id), v, lit, &tip, actions);
            }
            views_menu(ui, views, actions);
            ui.separator();
            ui.min_rect()
        })
        .inner;
    let room = egui::Rect::from_x_y_ranges(row.left()..=(right.left() - 4.0).max(row.left()), row.y_range());
    let window_moves = bar.integrated;
    for a in tabs::show(ui, room, bar.tabs, bar.shown, window_moves, bar.reveal) {
        actions.push(Action::Tab(a));
    }
}

/// The icon of a bar button.
fn id_icon(id: &str) -> egui::ImageSource<'static> {
    match id {
        "edit.undo" => icons::UNDO,
        "edit.redo" => icons::REDO,
        "view.parallel" => icons::PERSPECTIVE,
        "view.grid" => icons::GRID,
        "view.guides" => icons::GUIDES,
        "view.axes" => icons::AXES,
        "view.silhouettes" => icons::SILHOUETTES,
        "view.section" => icons::SECTION,
        _ => icons::MATERIALS,
    }
}

/// A bar button: smaller than the tool bar's.
fn bar_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, lit: bool, enabled: bool, tip: &str) -> egui::Response {
    let tint = ui.visuals().text_color();
    let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(18.0, 18.0)).tint(tint);
    ui.add_enabled(enabled, egui::Button::image(img).selected(lit).min_size(egui::vec2(26.0, 26.0))).on_hover_text(tip)
}

fn command_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, v: &CommandView, lit: bool, tip: &str, actions: &mut Vec<Action>) {
    if bar_button(ui, icon, lit, v.enabled, tip).clicked() {
        actions.push(Action::Command(v.id));
    }
}

/// The ☰ button: every menu of the registry, as submenus.
fn menus(ui: &mut egui::Ui, bar: &Bar, views: &[CommandView], actions: &mut Vec<Action>) {
    let img = egui::Image::new(icons::MENU).fit_to_exact_size(egui::vec2(18.0, 18.0)).tint(ui.visuals().text_color());
    let button = egui::Button::image(img).min_size(egui::vec2(26.0, 26.0));
    let (r, _) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        for menu in Menu::BAR {
            if menu == Menu::Debug && !bar.debug_menu {
                continue;
            }
            ui.menu_button(menu.label(), |ui| menu_items(ui, views.iter().filter(|v| v.menu == menu), actions));
        }
    });
    r.on_hover_text(t!("titlebar.menu"));
}

/// A menu's items, a separator before each group.
fn menu_items<'a>(ui: &mut egui::Ui, items: impl Iterator<Item = &'a CommandView>, actions: &mut Vec<Action>) {
    let mut first = true;
    for v in items {
        if v.group && !first {
            ui.separator();
        }
        first = false;
        if panels::command_item(ui, v) {
            actions.push(Action::Command(v.id));
        }
    }
}

/// The standard views, in a menu.
fn views_menu(ui: &mut egui::Ui, views: &[CommandView], actions: &mut Vec<Action>) {
    const IDS: [&str; 6] = ["view.top", "view.front", "view.right", "view.back", "view.left", "view.iso"];
    let img = egui::Image::new(icons::VIEWS).fit_to_exact_size(egui::vec2(18.0, 18.0)).tint(ui.visuals().text_color());
    let button = egui::Button::image(img).min_size(egui::vec2(26.0, 26.0));
    let (r, _) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        // Not a group: no separator before the first.
        menu_items(ui, views.iter().filter(|v| IDS.contains(&v.id)), actions);
    });
    r.on_hover_text(t!("titlebar.views"));
}

/// The command search: looks like a search field, opens the command palette. True when
/// clicked.
fn search(ui: &mut egui::Ui, v: &CommandView, width: f32) -> bool {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click());
    let visuals = ui.visuals();
    let stroke = if r.hovered() { visuals.widgets.hovered.bg_stroke } else { visuals.widgets.inactive.bg_stroke };
    let p = ui.painter();
    p.rect(rect, 6.0, visuals.extreme_bg_color, stroke, egui::StrokeKind::Inside);
    let weak = visuals.weak_text_color();
    let icon = egui::Rect::from_center_size(rect.left_center() + egui::vec2(14.0, 0.0), egui::vec2(14.0, 14.0));
    egui::Image::new(icons::SEARCH).tint(weak).paint_at(ui, icon);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut right = rect.right() - 8.0;
    if let Some(s) = &v.shortcut {
        let g = p.layout_no_wrap(s.clone(), font.clone(), weak);
        right -= g.size().x;
        p.galley(egui::pos2(right, rect.center().y - g.size().y / 2.0), g, weak);
        right -= 8.0;
    }
    let left = icon.right() + 6.0;
    let hint = egui::WidgetText::from(ellipsis(t!("titlebar.search"))).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (right - left).max(0.0),
        font,
    );
    ui.painter().galley(egui::pos2(left, rect.center().y - hint.size().y / 2.0), hint, weak);
    r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

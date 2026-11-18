//! The view bar (docs/ui/view-bar.md): a column of buttons over the left edge of
//! the view, the camera's and the display's commands; the user pins those it shows, as the
//! ribbon's and the quick tools'.

use egui::{Popup, Rect, RectAlign, Sense, StrokeKind, vec2};
use skraft_i18n::t;

use crate::dock::Dock;
use crate::icons;
use crate::panels::{Action, CommandView};

/// The standard views' button: not a command of the registry, it opens their menu.
pub const VIEWS: &str = "view.views";
/// The standard views, in its menu.
const STANDARD_VIEWS: [&str; 6] = ["view.top", "view.front", "view.right", "view.back", "view.left", "view.iso"];

/// The commands the bar can show, in its order.
pub const COMMANDS: &[&str] = &[
    VIEWS,
    "view.zoom_extents",
    "view.parallel",
    "view.xray",
    "view.hide_rest_of_model",
    "view.section",
    "view.grid",
    "view.axes",
    "view.guides",
    "view.silhouettes",
    "view.show_hidden",
];

/// Pinned by default.
pub const DEFAULT: &[&str] = &[VIEWS, "view.zoom_extents", "view.parallel", "view.xray", "view.hide_rest_of_model", "view.section"];

/// A button's side and its icon's.
const BUTTON: f32 = 30.0;
const ICON: f32 = 18.0;
/// The ▾ button's height.
const MORE: f32 = 16.0;
/// The grip's height, at the bar's head.
const GRIP: f32 = 10.0;
/// The bar's background opacity, at rest and under the pointer (the quick tools' card's).
const RESTING: f32 = 0.35;
const HOVERED: f32 = 0.92;

pub fn pinned(pins: &Option<Vec<String>>) -> Vec<&'static str> {
    crate::pins::pinned(COMMANDS, DEFAULT, pins)
}

pub fn toggle(pins: &mut Option<Vec<String>>, id: &'static str) {
    crate::pins::toggle(COMMANDS, DEFAULT, pins, id);
}

pub fn move_to(pins: &mut Option<Vec<String>>, id: &'static str, before: Option<&'static str>) {
    crate::pins::move_to(COMMANDS, DEFAULT, pins, id, before);
}

/// The bar's width, frame included, as [`bar`] draws it.
pub fn width(style: &egui::Style) -> f32 {
    BUTTON + frame(style).total_margin().sum().x
}

fn frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style).inner_margin(3.0)
}

/// Where the bar goes: the side it is docked to, and the top left corner of each slot (left,
/// right), as the side panels' two columns.
pub struct Place {
    pub side: Dock,
    pub slots: [egui::Pos2; 2],
}

fn slot(side: Dock) -> usize {
    match side {
        Dock::Left => 0,
        Dock::Right => 1,
    }
}

/// The bar in its slot, kept inside `view`; its grip dragged, it follows the pointer, the slot
/// it would go to lit, and is put in the one on the pointer's side when let go.
pub fn bar(ctx: &egui::Context, view: Rect, place: &Place, pins: &Option<Vec<String>>, views: &[CommandView], actions: &mut Vec<Action>) {
    let pinned = pinned(pins);
    // While its grip is dragged: the pointer from the bar's corner.
    let grab_id = egui::Id::unique("view_bar_grab");
    let grab = ctx.data(|d| d.get_temp::<egui::Vec2>(grab_id));
    let pointer = ctx.pointer_latest_pos();
    let home = place.slots[slot(place.side)];
    let at = match (grab, pointer) {
        (Some(g), Some(p)) => p - g,
        _ => home,
    };
    let target = pointer.map_or(place.side, |p| if p.x < view.center().x { Dock::Left } else { Dock::Right });
    if grab.is_some() {
        // The slot it goes to, as a docked card's empty column.
        let size = ctx.data(|d| d.get_temp::<egui::Vec2>(grab_id.with("size"))).unwrap_or(vec2(BUTTON, BUTTON));
        let r = Rect::from_min_size(place.slots[slot(target)], size);
        let accent = ctx.global_style().visuals.selection.bg_fill;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, grab_id.with("drop")));
        painter.rect(r, 6.0, accent.gamma_multiply(0.25), egui::Stroke::new(1.5, accent), StrokeKind::Inside);
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let side = place.side;
    // Hovered on the last frame (or a menu of it open): the bar is drawn before egui knows
    // this frame's.
    let hovered_id = egui::Id::unique("view_bar_hovered");
    let hovered = ctx.data(|d| d.get_temp::<bool>(hovered_id)).unwrap_or(false);
    let mut menu_open = false;
    let response = egui::Area::new(egui::Id::unique("view_bar")).fixed_pos(at).constrain_to(view).show(ctx, |ui| {
        let mut frame = frame(ui.style());
        let alpha = if hovered { HOVERED } else { RESTING };
        frame.fill = frame.fill.gamma_multiply(alpha);
        frame.stroke.color = frame.stroke.color.gamma_multiply(alpha);
        frame.shadow.color = frame.shadow.color.gamma_multiply(alpha * alpha);
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.vertical(|ui| {
                let g = grip(ui);
                if g.drag_started()
                    && let Some(p) = pointer
                {
                    ctx.data_mut(|d| d.insert_temp(grab_id, p - at));
                }
                if g.drag_stopped() && grab.is_some() {
                    ctx.data_mut(|d| d.remove::<egui::Vec2>(grab_id));
                    if target != side {
                        actions.push(Action::ViewBarSide(target));
                    }
                }
                let mut items = Vec::new();
                for &id in &pinned {
                    if id == VIEWS {
                        let (item, open) = views_button(ui, side, views, actions);
                        menu_open |= open;
                        items.push(item);
                    } else if let Some(c) = views.iter().find(|v| v.id == id) {
                        items.extend(button(ui, c, actions));
                    }
                }
                // A button dragged along the column moves it.
                if let Some((id, before)) = crate::pins::reorder(ui, egui::Id::unique("view_bar_drag"), true, &items) {
                    actions.push(Action::ViewMove(id, before));
                }
                menu_open |= more(ui, side, &pinned, views, actions);
            });
        });
    });
    ctx.data_mut(|d| d.insert_temp(grab_id.with("size"), response.response.rect.size()));
    let now = response.response.contains_pointer() || menu_open;
    if now != hovered {
        ctx.data_mut(|d| d.insert_temp(hovered_id, now));
        ctx.request_repaint();
    }
}

/// A button's rect and response, its background when hovered or `lit`, its icon.
fn icon_button(ui: &mut egui::Ui, handle: &str, icon: egui::ImageSource<'static>, lit: bool, enabled: bool) -> egui::Response {
    // Dragged even when it cannot run: a drag moves it.
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click_and_drag());
    crate::handles::publish_enabled(|| format!("view_bar:{handle}"), rect, enabled);
    let visuals = ui.style().interact(&response);
    if lit {
        ui.painter().rect_filled(rect, visuals.corner_radius, ui.visuals().selection.bg_fill);
    } else if enabled && response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    }
    let mut ink = if lit { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
    if !enabled {
        ink = ink.gamma_multiply(0.35);
    }
    egui::Image::new(icon).tint(ink).paint_at(ui, Rect::from_center_size(rect.center(), vec2(ICON, ICON)));
    response
}

/// A command's button: lit while on (perspective: while not parallel), its name in the tip;
/// right click unpins it, a drag moves it.
fn button(ui: &mut egui::Ui, c: &CommandView, actions: &mut Vec<Action>) -> Option<crate::pins::Item> {
    let icon = icons::command(c.id)?;
    let tip = if c.id == "view.parallel" { t!("toolbar.perspective") } else { c.tip() };
    let response = icon_button(ui, c.id, icon.clone(), lit(c), c.enabled).on_hover_text(tip);
    if response.clicked() && c.enabled {
        actions.push(Action::Command(c.id));
    }
    unpin_menu(&response, c.id, actions);
    Some(crate::pins::Item { id: c.id, rect: response.rect, response, icon })
}

/// A toggle's button is lit while it is on; perspective / parallel's while in perspective
/// (the command toggles parallel).
fn lit(c: &CommandView) -> bool {
    if c.id == "view.parallel" { c.checked == Some(false) } else { c.checked == Some(true) }
}

/// Right click on a button: Unpin.
fn unpin_menu(response: &egui::Response, id: &'static str, actions: &mut Vec<Action>) {
    Popup::context_menu(response).style(crate::style::menu).show(|ui| {
        if ui.button(t!("view_bar.unpin")).clicked() {
            actions.push(Action::ViewPin(id));
        }
    });
}

/// The grip at the bar's head: dragged, it moves the bar to the other side.
fn grip(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON, GRIP), Sense::drag());
    crate::handles::publish(|| "view_bar:grip".into(), rect);
    let ink = if response.hovered() || response.dragged() { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    // Two rows of three dots.
    for row in [-1.5, 1.5] {
        for col in [-4.0, 0.0, 4.0] {
            ui.painter().circle_filled(rect.center() + vec2(col, row), 1.0, ink);
        }
    }
    response.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text(t!("view_bar.grip"))
}

/// Its menus open towards the view, their top at their button's: on the right of a bar on
/// the left, and the other way (egui flips them up when there is no room below).
fn inward(side: Dock) -> RectAlign {
    match side {
        Dock::Left => RectAlign::RIGHT_START,
        Dock::Right => RectAlign::LEFT_START,
    }
}

/// The standard views' button and its menu, opening towards the view; whether it is open.
fn views_button(ui: &mut egui::Ui, side: Dock, views: &[CommandView], actions: &mut Vec<Action>) -> (crate::pins::Item, bool) {
    let open = Popup::is_id_open(ui.ctx(), egui::Id::unique("view_bar_views_menu"));
    let response = icon_button(ui, VIEWS, icons::VIEWS, open, true);
    let response = if open { response } else { response.on_hover_text(t!("view_bar.views")) };
    Popup::menu(&response).id(egui::Id::unique("view_bar_views_menu")).align(inward(side)).style(crate::style::menu).show(|ui| {
        // Each view with its icon, as the ▾ menu's rows.
        let ink = ui.visuals().text_color();
        for v in views.iter().filter(|v| STANDARD_VIEWS.contains(&v.id)) {
            let mut b = match icons::command(v.id) {
                Some(icon) => egui::Button::new((egui::Image::new(icon).fit_to_exact_size(vec2(16.0, 16.0)).tint(ink), v.label.as_str())),
                None => egui::Button::new(v.label.as_str()),
            };
            if let Some(s) = &v.shortcut {
                b = b.shortcut_text(s.as_str());
            }
            let r = ui.add_enabled(v.enabled, b);
            crate::handles::publish_enabled(|| format!("menu:{}", v.id), r.rect, v.enabled);
            if r.clicked() {
                actions.push(Action::Command(v.id));
            }
        }
    });
    unpin_menu(&response, VIEWS, actions);
    (crate::pins::Item { id: VIEWS, rect: response.rect, response, icon: icons::VIEWS }, open)
}

/// A command's row in the ▾ menu: its label, whether it can run, a toggle's state (its check
/// box, as in the View menu), its shortcut.
fn row(id: &str, views: &[CommandView]) -> Option<(String, bool, Option<bool>, Option<String>)> {
    if id == VIEWS {
        return Some((t!("view_bar.views"), true, None, None));
    }
    let c = views.iter().find(|v| v.id == id)?;
    Some((c.label.clone(), c.enabled, c.checked, c.shortcut.clone()))
}

/// The ▾ button: every command the bar can show, each with its pin. True while it is open.
fn more(ui: &mut egui::Ui, side: Dock, pinned: &[&'static str], views: &[CommandView], actions: &mut Vec<Action>) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON, MORE), Sense::click());
    crate::handles::publish(|| "view_bar:more".into(), rect);
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    }
    crate::ribbon::chevron(ui, rect.center(), ui.visuals().text_color());
    let id = egui::Id::unique("view_bar_more_menu");
    let open = Popup::is_id_open(ui.ctx(), id);
    let response = if open { response } else { response.on_hover_text(t!("view_bar.choose")) };
    // A pin clicked leaves it open, as the ribbon's group menus; a row run closes it.
    Popup::menu(&response)
        .id(id)
        .align(inward(side))
        .style(crate::style::menu)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let ink = ui.visuals().text_color();
            for &id in COMMANDS {
                let Some((name, enabled, check, shortcut)) = row(id, views) else { continue };
                let icon = if id == VIEWS { Some(icons::VIEWS) } else { icons::command(id) };
                ui.horizontal(|ui| {
                    // A toggle's check box in a column every row keeps, then the icon and the label.
                    let check = crate::style::menu_check(check);
                    let mut b = match icon {
                        Some(icon) => {
                            egui::Button::new((check, egui::Image::new(icon).fit_to_exact_size(vec2(16.0, 16.0)).tint(ink), name))
                        }
                        None => egui::Button::new((check, name)),
                    };
                    // Its shortcut on the right; none, an empty one still pushes the label to the
                    // left (else centred).
                    b = b.min_size(vec2(240.0, 22.0)).shortcut_text(shortcut.unwrap_or_default());
                    // A click on a row runs it, but the views' row: its menu is the button's.
                    if ui.add_enabled(enabled, b).clicked() && id != VIEWS {
                        actions.push(Action::Command(id));
                        ui.close();
                    }
                    let on = pinned.contains(&id);
                    let (icon, tint) = if on {
                        (icons::PIN_FILLED, ui.visuals().selection.stroke.color)
                    } else {
                        (icons::PIN, ui.visuals().weak_text_color().gamma_multiply(0.55))
                    };
                    let pin = egui::Button::image(egui::Image::new(icon).fit_to_exact_size(vec2(14.0, 14.0)).tint(tint)).frame(false);
                    let tip = if on { t!("view_bar.unpin") } else { t!("view_bar.pin") };
                    if ui.add(pin).on_hover_text(tip).clicked() {
                        actions.push(Action::ViewPin(id));
                    }
                });
            }
            // The note wraps within the rows' width instead of widening the menu.
            let width = ui.min_rect().width();
            ui.separator();
            ui.set_max_width(width);
            ui.add(egui::Label::new(egui::RichText::new(t!("view_bar.menu_note")).small().weak()).wrap());
        });
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands;

    #[test]
    fn the_view_bar_s_commands_are_commands_with_an_icon() {
        for id in COMMANDS.iter().filter(|id| **id != VIEWS) {
            assert!(commands::find(id).is_some(), "{id}");
            assert!(icons::command(id).is_some(), "{id}");
        }
        for id in STANDARD_VIEWS {
            assert!(commands::find(id).is_some(), "{id}");
        }
        assert!(DEFAULT.iter().all(|d| COMMANDS.contains(d)));
    }

    #[test]
    fn pins_go_at_the_end_and_back_to_the_defaults() {
        let mut pins = None;
        assert_eq!(pinned(&pins), DEFAULT);
        toggle(&mut pins, "view.grid");
        assert_eq!(pinned(&pins).last(), Some(&"view.grid"));
        move_to(&mut pins, "view.grid", Some(VIEWS));
        assert_eq!(pinned(&pins).first(), Some(&"view.grid"));
        toggle(&mut pins, "view.grid");
        assert_eq!(pins, None, "the defaults again: nothing kept");
    }
}

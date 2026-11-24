//! The quick tools' card (docs/tools/quick-tools.md): after a creation, a row of
//! buttons at the centre of what was made, each starting a tool on it; the user pins the
//! commands it shows, as the ribbon's.

use egui::{Popup, Rect, Sense, StrokeKind, vec2};
use skraft_i18n::t;

use crate::cards::panels::{Action, CommandView};
use crate::kit::icons;
use crate::kit::style::Tip;
use crate::kit::widgets::{IconButton, Look, PinRow, pin_row};

/// The commands the card can show, in its order.
pub const COMMANDS: &[&str] =
    &["tool.copy_line", "tool.copy_grid", "tool.distribute", "tool.move", "tool.rotate", "tool.stretch", "tool.paint"];

/// Pinned by default: OCL's copy tools.
pub const DEFAULT: &[&str] = &["tool.copy_line", "tool.copy_grid", "tool.distribute"];

// A button's side and its icon's, and the card's frame: the view bar's, the floating bars of the
// view all as thick (docs/tools/quick-tools.md).
use crate::window::view_bar::{BUTTON, ICON};
/// The ▾ button's width.
const MORE: f32 = 16.0;

/// The pinned commands, in the user's order: `pins` as they left them (unknown ids ignored),
/// else the defaults.
pub fn pinned(pins: &Option<Vec<String>>) -> Vec<&'static str> {
    crate::window::pins::pinned(COMMANDS, DEFAULT, pins)
}

/// Pins `id` (at the end), or unpins it; back to the defaults, nothing kept.
pub fn toggle(pins: &mut Option<Vec<String>>, id: &'static str) {
    crate::window::pins::toggle(COMMANDS, DEFAULT, pins, id);
}

/// Moves the pinned `id` before `before` (none: at the end).
pub fn move_to(pins: &mut Option<Vec<String>>, id: &'static str, before: Option<&'static str>) {
    crate::window::pins::move_to(COMMANDS, DEFAULT, pins, id, before);
}

/// The card, centred on `at` (screen points), kept inside `view`.
pub fn card(ctx: &egui::Context, view: Rect, at: egui::Pos2, pins: &Option<Vec<String>>, views: &[CommandView], actions: &mut Vec<Action>) {
    let pinned = pinned(pins);
    // Hovered on the last frame: the card is drawn before egui knows this frame's.
    let hovered_id = egui::Id::unique("quick_tools_hovered");
    let hovered = ctx.data(|d| d.get_temp::<bool>(hovered_id)).unwrap_or(false);
    let response = egui::Area::new(egui::Id::unique("quick_tools"))
        .fixed_pos(at)
        .pivot(egui::Align2::CENTER_CENTER)
        .constrain_to(view)
        .show(ctx, |ui| {
            // Translucent, as OCL's: the part stays visible behind it; nearly opaque under the
            // pointer, to read and click.
            crate::kit::style::translucent(crate::window::view_bar::frame(ui.style()), hovered).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.horizontal(|ui| {
                    let items: Vec<_> =
                        pinned.iter().filter_map(|&id| views.iter().find(|v| v.id == id)).filter_map(|c| button(ui, c, actions)).collect();
                    // A button dragged along the row moves it (docs/tools/quick-tools.md).
                    if let Some((id, before)) = crate::window::pins::reorder(ui, egui::Id::unique("quick_tools_drag"), false, &items) {
                        actions.push(Action::QuickMove(id, before));
                    }
                    more(ui, &pinned, views, actions);
                });
            });
        });
    let now = response.response.contains_pointer();
    if now != hovered {
        ctx.data_mut(|d| d.insert_temp(hovered_id, now));
        ctx.request_repaint();
    }
}

/// A command's button: its icon, its name in the tip; right click unpins it, a drag moves it.
fn button(ui: &mut egui::Ui, c: &CommandView, actions: &mut Vec<Action>) -> Option<crate::window::pins::Item> {
    let icon = icons::command(c.id)?;
    let response = IconButton::new(icon.clone(), BUTTON, ICON).look(Look::Bar).enabled(c.enabled).show(ui);
    let rect = response.rect;
    crate::kit::handles::publish_enabled(|| format!("quick:{}", c.id), rect, c.enabled);
    let response = response.tip(&c.label);
    if response.clicked() && c.enabled {
        actions.push(Action::QuickTool(c.id));
    }
    Popup::context_menu(&response).style(crate::kit::style::menu).show(|ui| {
        if ui.button(t!("quick_tools.unpin")).clicked() {
            actions.push(Action::QuickPin(c.id));
        }
    });
    Some(crate::window::pins::Item { id: c.id, rect, response, icon })
}

/// The ▾ button: every command the card can show, each with its pin.
fn more(ui: &mut egui::Ui, pinned: &[&'static str], views: &[CommandView], actions: &mut Vec<Action>) {
    let (rect, response) = ui.allocate_exact_size(vec2(MORE, BUTTON), Sense::click());
    crate::kit::handles::publish(|| "quick:more".into(), rect);
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    }
    crate::window::ribbon::chevron(ui, rect.center(), ui.visuals().text_color());
    let response = response.tip(t!("quick_tools.choose"));
    // A pin clicked leaves it open, as the ribbon's group menus; a row run closes it.
    Popup::menu(&response).style(crate::kit::style::menu).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        for &id in COMMANDS {
            let Some(c) = views.iter().find(|v| v.id == id) else { continue };
            let on = pinned.contains(&id);
            let (run, pin) = pin_row(
                ui,
                PinRow {
                    icon: icons::command(id),
                    check: None,
                    label: &c.label,
                    shortcut: c.shortcut.as_deref(),
                    enabled: c.enabled,
                    selected: false,
                    width: 190.0,
                    pinned: on,
                    pin_tip: if on { t!("quick_tools.unpin") } else { t!("quick_tools.pin") },
                },
            );
            if run.clicked() {
                actions.push(Action::QuickTool(id));
                ui.close();
            }
            if pin {
                actions.push(Action::QuickPin(id));
            }
        }
        // The note wraps within the rows' width instead of widening the menu.
        let width = ui.min_rect().width();
        ui.separator();
        ui.set_max_width(width);
        ui.add(egui::Label::new(egui::RichText::new(t!("quick_tools.menu_note")).small().weak()).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands;

    #[test]
    fn the_quick_tools_are_commands_with_an_icon_and_a_tool() {
        for id in COMMANDS {
            assert!(commands::find(id).is_some(), "{id}");
            assert!(icons::command(id).is_some(), "{id}");
            assert!(crate::app::tool_of(id).is_some(), "{id}");
        }
        assert!(DEFAULT.iter().all(|d| COMMANDS.contains(d)));
    }

    #[test]
    fn pins_go_at_the_end_and_back_to_the_defaults() {
        let mut pins = None;
        assert_eq!(pinned(&pins), DEFAULT);
        toggle(&mut pins, "tool.move");
        assert_eq!(pinned(&pins), ["tool.copy_line", "tool.copy_grid", "tool.distribute", "tool.move"]);
        move_to(&mut pins, "tool.move", Some("tool.copy_line"));
        assert_eq!(pinned(&pins), ["tool.move", "tool.copy_line", "tool.copy_grid", "tool.distribute"]);
        toggle(&mut pins, "tool.move");
        assert_eq!(pins, None, "the defaults again: nothing kept");
    }
}

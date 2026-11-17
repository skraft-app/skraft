//! The quick tools' card (docs/ARCHITECTURE.md §6 terdecies): after a creation, a row of
//! buttons at the centre of what was made, each starting a tool on it; the user pins the
//! commands it shows, as the ribbon's.

use egui::{Popup, Rect, Sense, StrokeKind, vec2};
use solidsketch_i18n::t;

use crate::icons;
use crate::panels::{Action, CommandView};

/// The commands the card can show, in its order.
pub const COMMANDS: &[&str] =
    &["tool.copy_line", "tool.copy_grid", "tool.distribute", "tool.move", "tool.rotate", "tool.stretch", "tool.paint"];

/// Pinned by default: OCL's copy tools.
pub const DEFAULT: &[&str] = &["tool.copy_line", "tool.copy_grid", "tool.distribute"];

/// A button's side and its icon's.
const BUTTON: f32 = 34.0;
const ICON: f32 = 22.0;
/// The ▾ button's width.
const MORE: f32 = 16.0;
/// The card's background opacity, at rest and under the pointer.
const RESTING: f32 = 0.35;
const HOVERED: f32 = 0.92;

/// The pinned commands, in the user's order: `pins` as they left them (unknown ids ignored),
/// else the defaults.
pub fn pinned(pins: &Option<Vec<String>>) -> Vec<&'static str> {
    crate::pins::pinned(COMMANDS, DEFAULT, pins)
}

/// Pins `id` (at the end), or unpins it; back to the defaults, nothing kept.
pub fn toggle(pins: &mut Option<Vec<String>>, id: &'static str) {
    crate::pins::toggle(COMMANDS, DEFAULT, pins, id);
}

/// Moves the pinned `id` before `before` (none: at the end).
pub fn move_to(pins: &mut Option<Vec<String>>, id: &'static str, before: Option<&'static str>) {
    crate::pins::move_to(COMMANDS, DEFAULT, pins, id, before);
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
            let mut frame = egui::Frame::window(ui.style()).inner_margin(4.0);
            // Translucent, as OCL's: the part stays visible behind it; nearly opaque under the
            // pointer, to read and click.
            let alpha = if hovered { HOVERED } else { RESTING };
            frame.fill = frame.fill.gamma_multiply(alpha);
            frame.stroke.color = frame.stroke.color.gamma_multiply(alpha);
            frame.shadow.color = frame.shadow.color.gamma_multiply(alpha * alpha);
            frame.show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.horizontal(|ui| {
                    let items: Vec<_> =
                        pinned.iter().filter_map(|&id| views.iter().find(|v| v.id == id)).filter_map(|c| button(ui, c, actions)).collect();
                    // A button dragged along the row moves it (§6 terdecies).
                    if let Some((id, before)) = crate::pins::reorder(ui, egui::Id::unique("quick_tools_drag"), false, &items) {
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
fn button(ui: &mut egui::Ui, c: &CommandView, actions: &mut Vec<Action>) -> Option<crate::pins::Item> {
    let icon = icons::command(c.id)?;
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click_and_drag());
    crate::handles::publish_enabled(|| format!("quick:{}", c.id), rect, c.enabled);
    let visuals = ui.style().interact(&response);
    if c.enabled && response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    }
    let mut ink = ui.visuals().text_color();
    if !c.enabled {
        ink = ink.gamma_multiply(0.35);
    }
    egui::Image::new(icon.clone()).tint(ink).paint_at(ui, Rect::from_center_size(rect.center(), vec2(ICON, ICON)));
    let response = response.on_hover_text(&c.label);
    if response.clicked() && c.enabled {
        actions.push(Action::QuickTool(c.id));
    }
    Popup::context_menu(&response).style(crate::style::menu).show(|ui| {
        if ui.button(t!("quick_tools.unpin")).clicked() {
            actions.push(Action::QuickPin(c.id));
        }
    });
    Some(crate::pins::Item { id: c.id, rect, response, icon })
}

/// The ▾ button: every command the card can show, each with its pin.
fn more(ui: &mut egui::Ui, pinned: &[&'static str], views: &[CommandView], actions: &mut Vec<Action>) {
    let (rect, response) = ui.allocate_exact_size(vec2(MORE, BUTTON), Sense::click());
    crate::handles::publish(|| "quick:more".into(), rect);
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    }
    crate::ribbon::chevron(ui, rect.center(), ui.visuals().text_color());
    let response = response.on_hover_text(t!("quick_tools.choose"));
    // A pin clicked leaves it open, as the ribbon's group menus; a row run closes it.
    Popup::menu(&response).style(crate::style::menu).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let ink = ui.visuals().text_color();
        for &id in COMMANDS {
            let Some(c) = views.iter().find(|v| v.id == id) else { continue };
            ui.horizontal(|ui| {
                let mut b = match icons::command(id) {
                    Some(icon) => {
                        egui::Button::image_and_text(egui::Image::new(icon).fit_to_exact_size(vec2(16.0, 16.0)).tint(ink), &c.label)
                    }
                    None => egui::Button::new(&c.label),
                };
                // Its shortcut on the right; none, an empty one still pushes the label to the
                // left (else centred).
                b = b.min_size(vec2(190.0, 22.0)).shortcut_text(c.shortcut.clone().unwrap_or_default());
                if ui.add_enabled(c.enabled, b).clicked() {
                    actions.push(Action::QuickTool(id));
                    ui.close();
                }
                let on = pinned.contains(&id);
                let (icon, tint) = if on {
                    (icons::PIN_FILLED, ui.visuals().selection.stroke.color)
                } else {
                    (icons::PIN, ui.visuals().weak_text_color().gamma_multiply(0.55))
                };
                let pin = egui::Button::image(egui::Image::new(icon).fit_to_exact_size(vec2(14.0, 14.0)).tint(tint)).frame(false);
                let tip = if on { t!("quick_tools.unpin") } else { t!("quick_tools.pin") };
                if ui.add(pin).on_hover_text(tip).clicked() {
                    actions.push(Action::QuickPin(id));
                }
            });
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

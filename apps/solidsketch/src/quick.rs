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

/// The pinned commands, in the card's order: `pins` as the user left them (unknown ids
/// ignored), else the defaults.
pub fn pinned(pins: &Option<Vec<String>>) -> Vec<&'static str> {
    match pins {
        Some(ids) => COMMANDS.iter().copied().filter(|c| ids.iter().any(|i| i == c)).collect(),
        None => DEFAULT.to_vec(),
    }
}

/// Pins `id`, or unpins it; back to the defaults, nothing kept.
pub fn toggle(pins: &mut Option<Vec<String>>, id: &'static str) {
    let mut now = pinned(pins);
    if now.contains(&id) {
        now.retain(|c| *c != id);
    } else {
        now = COMMANDS.iter().copied().filter(|c| *c == id || now.contains(c)).collect();
    }
    *pins = (now != DEFAULT).then(|| now.into_iter().map(str::to_owned).collect());
}

/// The card, centred on `at` (screen points), kept inside `view`.
pub fn card(ctx: &egui::Context, view: Rect, at: egui::Pos2, pins: &Option<Vec<String>>, views: &[CommandView], actions: &mut Vec<Action>) {
    let pinned = pinned(pins);
    egui::Area::new(egui::Id::unique("quick_tools")).fixed_pos(at).pivot(egui::Align2::CENTER_CENTER).constrain_to(view).show(ctx, |ui| {
        let mut frame = egui::Frame::window(ui.style()).inner_margin(4.0);
        // Translucent, as OCL's: the part stays visible behind it.
        frame.fill = frame.fill.gamma_multiply(0.85);
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for &id in &pinned {
                    if let Some(c) = views.iter().find(|v| v.id == id) {
                        button(ui, c, actions);
                    }
                }
                more(ui, &pinned, views, actions);
            });
        });
    });
}

/// A command's button: its icon, its name in the tip; right click unpins it.
fn button(ui: &mut egui::Ui, c: &CommandView, actions: &mut Vec<Action>) {
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), if c.enabled { Sense::click() } else { Sense::hover() });
    crate::handles::publish_enabled(|| format!("quick:{}", c.id), rect, c.enabled);
    let visuals = ui.style().interact(&response);
    if c.enabled && response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    }
    let mut ink = ui.visuals().text_color();
    if !c.enabled {
        ink = ink.gamma_multiply(0.35);
    }
    if let Some(icon) = icons::command(c.id) {
        egui::Image::new(icon).tint(ink).paint_at(ui, Rect::from_center_size(rect.center(), vec2(ICON, ICON)));
    }
    let response = response.on_hover_text(&c.label);
    if response.clicked() {
        actions.push(Action::QuickTool(c.id));
    }
    Popup::context_menu(&response).style(crate::style::menu).show(|ui| {
        if ui.button(t!("quick_tools.unpin")).clicked() {
            actions.push(Action::QuickPin(c.id));
        }
    });
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
    Popup::menu(&response).style(crate::style::menu).show(|ui| {
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
                // An empty shortcut pushes the label to the left (else centred).
                b = b.min_size(vec2(190.0, 22.0)).shortcut_text("");
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
        ui.separator();
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
    fn pins_toggle_in_the_card_s_order_and_go_back_to_the_defaults() {
        let mut pins = None;
        assert_eq!(pinned(&pins), DEFAULT);
        toggle(&mut pins, "tool.move");
        assert_eq!(pinned(&pins), ["tool.copy_line", "tool.copy_grid", "tool.distribute", "tool.move"]);
        toggle(&mut pins, "tool.copy_line");
        assert_eq!(pinned(&pins), ["tool.copy_grid", "tool.distribute", "tool.move"]);
        toggle(&mut pins, "tool.copy_line");
        toggle(&mut pins, "tool.move");
        assert_eq!(pins, None, "the defaults again: nothing kept");
        // Everything unpinned: kept empty (only the ▾ button); unknown ids ignored.
        let mut pins = Some(vec!["tool.nope".to_owned()]);
        assert!(pinned(&pins).is_empty());
        toggle(&mut pins, "tool.paint");
        assert_eq!(pinned(&pins), ["tool.paint"]);
    }
}

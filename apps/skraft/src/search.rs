//! The search field of the « Composants » card and of the navigator
//! (docs/ui/components-library.md#search); the matching itself is `skraft_tools::search`'s.

use skraft_i18n::t;

use crate::icons;

/// The search field on a line of its own, under the key `salt`, its handle `<handle>:search`: a
/// magnifier, the field over the width left by `trailing` (buttons laid at the line's right end,
/// right to left, given the line's height: [`line_height`]), a ✕ inside its right end to clear
/// it; Esc in it clears it too.
pub fn field(ui: &mut egui::Ui, salt: egui::Id, handle: &'static str, text: &mut String, trailing: impl FnOnce(&mut egui::Ui, f32)) {
    let height = line_height(ui);
    let weak = ui.visuals().weak_text_color();
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // One gap on the line (egui adds it after each item), the one above it in the
        // « Composants » card too. No scope around `trailing`: an empty one would still take a
        // gap, the navigator's field then short of the card's margin.
        ui.spacing_mut().item_spacing.x = GAP;
        trailing(ui, height);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(egui::Image::new(icons::SEARCH).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(weak));
            // The whole width left, at the line's height; the ✕ inside its right end, the text
            // kept clear of it.
            let clear = 16.0;
            let edit = egui::TextEdit::singleline(text)
                .id(salt.with("text"))
                .hint_text(t!("search.hint"))
                .margin(egui::Margin { left: 4, right: clear as i8 + 2, top: 2, bottom: 2 })
                .align(egui::Align2::LEFT_CENTER)
                .min_size(egui::vec2(0.0, height))
                .desired_width(ui.available_width().max(40.0));
            let r = ui.add(edit);
            crate::handles::publish(|| format!("{handle}:search"), r.rect);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                text.clear();
            }
            if !text.is_empty() {
                // Drawn at the field's centre line, not laid out as a button (whose padding put
                // it lower); the buttons' pointer, not the field's text cursor.
                let at = egui::Rect::from_center_size(egui::pos2(r.rect.right() - clear / 2.0 - 1.0, r.rect.center().y), egui::Vec2::splat(clear));
                let b = ui.interact(at, salt.with("clear"), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::Default);
                let tint = if b.hovered() { ui.visuals().text_color() } else { weak };
                egui::Image::new(icons::CLOSE).tint(tint).paint_at(ui, egui::Rect::from_center_size(at.center(), egui::vec2(10.0, 10.0)));
                if b.on_hover_text(t!("search.clear")).clicked() {
                    text.clear();
                }
            }
        });
    });
}

/// The gap around the search field (docs/ui/components-library.md#search): between the line's
/// items — the magnifier, the field, the buttons after it —, from the title bar's line or the
/// tabs above it, to the list under it.
pub const GAP: f32 = 6.0;

/// The search line's height: the field's (its text's row and its margins of 2 pt, rounded as
/// egui rounds the field's), at least a widget's.
pub fn line_height(ui: &egui::Ui) -> f32 {
    ui.spacing().interact_size.y.max((ui.text_style_height(&egui::TextStyle::Body) + 4.0).round())
}

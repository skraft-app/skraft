//! The search of the « Composants » card and of the navigator (docs/ui/components-library.md#search):
//! the text field ([`field`]), the card's natures and material ([`filters`]); the matching itself
//! is `skraft_tools::search`'s.

use skraft_i18n::t;
use skraft_model::Document;
use skraft_tools::search::Filter;

use crate::icons;

/// The search field on a line of its own, under the key `salt`, its handle `<handle>:search`: a
/// magnifier, the field over the width left by `trailing` (buttons laid at the line's right end,
/// right to left), a ✕ inside its right end to clear it; Esc in it clears it too.
pub fn field(ui: &mut egui::Ui, salt: egui::Id, handle: &'static str, text: &mut String, trailing: impl FnOnce(&mut egui::Ui)) {
    let height = ui.spacing().interact_size.y;
    let weak = ui.visuals().weak_text_color();
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        trailing(ui);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(egui::Image::new(icons::SEARCH).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(weak));
            // The whole width left; the ✕ inside its right end, the text kept clear of it.
            let clear = 16.0;
            let edit = egui::TextEdit::singleline(text)
                .id(salt.with("text"))
                .hint_text(t!("search.hint"))
                .margin(egui::Margin { left: 4, right: clear as i8 + 2, top: 2, bottom: 2 })
                .desired_width(ui.available_width().max(40.0));
            let r = ui.add(edit);
            crate::handles::publish(|| format!("{handle}:search"), r.rect);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                text.clear();
            }
            if !text.is_empty() {
                let at = egui::Rect::from_center_size(egui::pos2(r.rect.right() - clear / 2.0 - 1.0, r.rect.center().y), egui::Vec2::splat(clear));
                let img = egui::Image::new(icons::CLOSE).fit_to_exact_size(egui::vec2(10.0, 10.0)).tint(weak);
                if ui.put(at, egui::Button::image(img).frame(false)).on_hover_text(t!("search.clear")).clicked() {
                    text.clear();
                }
            }
        });
    });
}

/// The « Composants » card's filters on a line: the natures' toggles (parts, assemblies; at least
/// one stays lit), then the material menu. Handles `<handle>:parts`, `:assemblies`, `:material`.
pub fn filters(ui: &mut egui::Ui, salt: egui::Id, handle: &'static str, filter: &mut Filter, doc: &Document) {
    let height = ui.spacing().interact_size.y;
    let tint = ui.visuals().text_color();
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let lit = [filter.parts, filter.assemblies].iter().filter(|on| **on).count();
        let toggle = |ui: &mut egui::Ui, on: &mut bool, icon, tip: String, name: &'static str| {
            let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint);
            let b = ui.add(egui::Button::image(img).selected(*on)).on_hover_text(tip);
            crate::handles::publish(|| format!("{handle}:{name}"), b.rect);
            if b.clicked() && !(*on && lit == 1) {
                *on = !*on;
            }
        };
        toggle(ui, &mut filter.parts, icons::BOX, t!("search.parts"), "parts");
        toggle(ui, &mut filter.assemblies, icons::COMPONENT, t!("search.assemblies"), "assemblies");
        ui.add_space(6.0);
        material(ui, salt, handle, filter, doc);
    });
}

/// The material menu, after the materials' icon (what it filters by): every material of the
/// document, « Toutes » first; greyed while the document has none.
fn material(ui: &mut egui::Ui, salt: egui::Id, handle: &'static str, filter: &mut Filter, doc: &Document) {
    // A material gone from the document filters nothing any more.
    if filter.material.is_some_and(|m| doc.material(m).is_err()) {
        filter.material = None;
    }
    let any = doc.materials().next().is_some();
    let tip = if any { t!("search.material_tip") } else { t!("search.no_materials") };
    let tint = if any { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    let icon = ui.add(egui::Image::new(icons::MATERIALS).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
    icon.on_hover_text(&tip);
    let shown = filter.material.and_then(|m| doc.material(m).ok());
    let text = shown.map_or_else(|| t!("search.all_materials"), |m| m.name.clone());
    ui.add_enabled_ui(any, |ui| {
        let r = egui::ComboBox::from_id_salt(salt.with("material")).width(84.0).truncate().selected_text(text).show_ui(ui, |ui| {
            ui.selectable_value(&mut filter.material, None, t!("search.all_materials"));
            for (id, m) in doc.materials() {
                ui.horizontal(|ui| {
                    crate::panels::swatch(ui, Some(m.color), 12.0);
                    ui.selectable_value(&mut filter.material, Some(id), &m.name);
                });
            }
        });
        crate::handles::publish(|| format!("{handle}:material"), r.response.rect);
        r.response.on_hover_text(tip).on_disabled_hover_text(t!("search.no_materials"));
    });
}

//! The widgets the interface's screens share (docs/ui/interface.md#widgets): a screen draws
//! its buttons, fields and rows with these rather than its own.

use crate::kit::icons;

mod button;
mod choice;
pub(crate) mod color;
mod field;
mod form;
mod keypad;
mod menu;
mod segmented;
mod tree;

pub(crate) use button::{
    Confirm, IconButton, LabelButton, Look, MenuFace, OPTION_HEIGHT, TextButton, VEIL_RADIUS, bar_menu, confirm_buttons, menu_open,
    option_button, veil,
};
pub(crate) use choice::{ChoiceList, choice_field, choice_item};
pub(crate) use field::{Field, FieldLook, cut_label, field_stroke, framed_field, live_field, rename_field};
pub(crate) use form::{FormField, form_message, form_row, warning_box};
pub(crate) use keypad::{KeypadAct, keypad};
pub(crate) use menu::{MenuEntry, PinEvent, PinItem, PinMenu, PinTexts, command_item, is_toggle, pin_list, pin_menu, unpin_menu};
pub(crate) use segmented::{Segment, segmented, segmented_sized};
pub(crate) use tree::{
    Fold, RENAMING, Tick, TreeLine, band_color, hidden_color, paint_drag_ghost, set_drag_ghost, start_rename, tick, tree_line, tree_row,
};

/// How [`keycaps`] draws its keys.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Keycaps {
    /// The field's fill, a thicker edge at its bottom.
    Plain,
    /// Outlined and written in the selection's ink: a shortcut being typed.
    Lit,
    /// Hollow and dimmed: a key that cannot be changed.
    Fixed,
}

/// A shortcut drawn as the keys to press (`⇧` `⌘` `S`), each a key cap, as `look` says. Laid
/// right to left from `right` (the right end of its middle line); the rect it covers.
pub fn keycaps(ui: &egui::Ui, keys: &[String], right: egui::Pos2, look: Keycaps) -> egui::Rect {
    const GAP: f32 = 3.0;
    const PAD: f32 = 5.0;
    let visuals = ui.visuals();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let height = (ui.text_style_height(&egui::TextStyle::Body) + 4.0).round();
    let (ink, edge, fill) = match look {
        Keycaps::Plain => (visuals.text_color(), visuals.widgets.noninteractive.bg_stroke.color, visuals.widgets.inactive.weak_bg_fill),
        Keycaps::Lit => (visuals.selection.stroke.color, visuals.selection.stroke.color, visuals.selection.bg_fill),
        Keycaps::Fixed => (visuals.weak_text_color(), visuals.widgets.noninteractive.bg_stroke.color, egui::Color32::TRANSPARENT),
    };
    let radius = egui::CornerRadius::same(4);
    let mut x = right.x;
    let mut covered = egui::Rect::NOTHING;
    for key in keys.iter().rev() {
        let g = ui.painter().layout_no_wrap(key.clone(), font.clone(), ink);
        let width = (g.size().x + 2.0 * PAD).max(height);
        let cap = egui::Rect::from_min_max(egui::pos2(x - width, right.y - height / 2.0), egui::pos2(x, right.y + height / 2.0));
        // The edge, one point thicker at the bottom: a key, not a field (a fixed one flat).
        if look != Keycaps::Fixed {
            ui.painter().rect_filled(cap.translate(egui::vec2(0.0, 1.0)), radius, edge);
        }
        ui.painter().rect(cap, radius, fill, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        ui.painter().galley(cap.center() - g.size() / 2.0, g, ink);
        covered = covered.union(cap);
        x -= width + GAP;
    }
    covered
}

/// The warning sign before an error to correct (a part in several pieces, a shortcut's
/// conflict), in the error colour, as high as the text.
pub fn warning(ui: &egui::Ui) -> egui::Image<'static> {
    let side = ui.text_style_height(&egui::TextStyle::Body);
    egui::Image::new(icons::WARNING).fit_to_exact_size(egui::vec2(side, side)).tint(ui.visuals().error_fg_color)
}

/// The mark of the two ends of a link: a part shaping others, a body shaped by one; in `ink`,
/// its row's (the selection's on a selected row).
pub fn linked_mark(ui: &egui::Ui, ink: egui::Color32) -> egui::Image<'static> {
    let side = ui.text_style_height(&egui::TextStyle::Body) - 2.0;
    egui::Image::new(icons::LINKED).fit_to_exact_size(egui::vec2(side, side)).tint(ink)
}

/// The same chain, small, on the bottom right corner of an icon at `on`: a body shaped by a link,
/// on its sign. A disc of `ground`, its row's background, cuts the icon under it.
pub fn linked_badge(ui: &egui::Ui, on: egui::Rect, ink: egui::Color32, ground: egui::Color32) {
    const SIDE: f32 = 9.0;
    let center = on.right_bottom() - egui::vec2(SIDE / 2.0 - 2.0, SIDE / 2.0 - 2.0);
    badge(ui, egui::Rect::from_center_size(center, egui::Vec2::splat(SIDE + 2.0)), icons::LINKED, SIDE, ink, ground);
}

/// A badge: `icon`, `glyph` points wide, in `ink`, on a disc of `ground` filling `disc`, which
/// cuts what is under it (a linked body's sign, a locked colour).
pub fn badge(ui: &egui::Ui, disc: egui::Rect, icon: egui::ImageSource<'static>, glyph: f32, ink: egui::Color32, ground: egui::Color32) {
    ui.painter().circle_filled(disc.center(), disc.width() / 2.0, ground);
    egui::Image::new(icon).tint(ink).paint_at(ui, egui::Rect::from_center_size(disc.center(), egui::Vec2::splat(glyph)));
}

/// The side of a badge over a picture: a colour box's, a texture's (points).
pub const BADGE: f32 = 18.0;

/// A [`BADGE`] square in `on`'s corner `corner`, a little inside it.
pub fn badge_rect(on: egui::Rect, corner: egui::Align2) -> egui::Rect {
    corner.align_size_within_rect(egui::Vec2::splat(BADGE), on.shrink(3.0))
}

/// A badge's ground over a picture: the panel's, a little see-through, readable over any colour
/// or image.
pub fn picture_ground(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().panel_fill.gamma_multiply(0.85)
}

/// A badge in `on`'s corner `corner` showing `icon` (a lock).
pub fn picture_badge(ui: &egui::Ui, on: egui::Rect, corner: egui::Align2, icon: egui::ImageSource<'static>, glyph: f32) {
    badge(ui, badge_rect(on, corner), icon, glyph, ui.visuals().text_color(), picture_ground(ui));
}

/// A button as a badge in `on`'s corner `corner`, on its disc (a texture's ✕ and ↻), under the
/// key `id`.
pub fn badge_button(
    ui: &mut egui::Ui,
    on: egui::Rect,
    corner: egui::Align2,
    icon: egui::ImageSource<'static>,
    glyph: f32,
    id: egui::Id,
) -> egui::Response {
    let rect = badge_rect(on, corner);
    ui.painter().circle_filled(rect.center(), BADGE / 2.0, picture_ground(ui));
    IconButton::new(icon, BADGE, glyph).show_at(ui, rect, id)
}

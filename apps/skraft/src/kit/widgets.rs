//! The widgets the interface's screens share (docs/ui/interface.md#widgets): a screen draws
//! its buttons, fields and rows with these rather than its own.

use crate::kit::icons;

mod button;
mod field;
mod form;
mod keypad;
mod menu;
mod segmented;
mod tree;

pub(crate) use button::{
    Confirm, IconButton, LabelButton, Look, OPTION_HEIGHT, VEIL_RADIUS, backed, confirm_buttons, option_button, unframed, veil,
};
pub(crate) use field::{Field, FieldLook, cut_label, field_stroke, framed_field, live_field, rename_field};
pub(crate) use form::{FormField, form_message, form_row, warning_box};
pub(crate) use keypad::{KeypadAct, keypad};
pub(crate) use menu::{PinEvent, PinItem, PinMenu, PinTexts, command_item, is_toggle, pin_list, pin_menu, unpin_menu};
pub(crate) use segmented::{Segment, segmented, segmented_sized};
pub(crate) use tree::{
    Fold, RENAMING, Tick, TreeLine, band_color, hidden_color, paint_drag_ghost, set_drag_ghost, start_rename, tick, tree_line, tree_row,
};

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
    ui.painter().circle_filled(center, SIDE / 2.0 + 1.0, ground);
    let rect = egui::Rect::from_center_size(center, egui::vec2(SIDE, SIDE));
    egui::Image::new(icons::LINKED).tint(ink).paint_at(ui, rect);
}

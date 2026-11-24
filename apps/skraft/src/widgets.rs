//! The widgets the interface's screens share (docs/ui/interface.md#widgets): a screen draws
//! its buttons, fields and rows with these rather than its own.

use crate::icons;

mod button;
mod field;
mod menu;
mod segmented;
mod tree;

pub(crate) use button::{Confirm, IconButton, Look, OPTION_HEIGHT, VEIL_RADIUS, backed, confirm_buttons, option_button, unframed, veil};
pub(crate) use field::{Field, FieldLook, cut_label, field_stroke, framed_field, live_field, rename_field};
pub(crate) use menu::{PinRow, command_item, is_toggle, pin_row};
pub(crate) use segmented::{Segment, segmented, segmented_sized};
pub(crate) use tree::{
    Fold, RENAMING, TreeLine, band_color, hidden_color, paint_drag_ghost, set_drag_ghost, start_rename, tree_line, tree_row,
};

/// The warning sign before an error to correct (a part in several pieces, a shortcut's
/// conflict), in the error colour, as high as the text.
pub fn warning(ui: &egui::Ui) -> egui::Image<'static> {
    let side = ui.text_style_height(&egui::TextStyle::Body);
    egui::Image::new(icons::WARNING).fit_to_exact_size(egui::vec2(side, side)).tint(ui.visuals().error_fg_color)
}

/// The mark of the two ends of a link: a part shaping others, a body shaped by one.
pub fn linked_mark(ui: &egui::Ui) -> egui::Image<'static> {
    let side = ui.text_style_height(&egui::TextStyle::Body) - 2.0;
    egui::Image::new(icons::LINKED).fit_to_exact_size(egui::vec2(side, side)).tint(ui.visuals().weak_text_color())
}

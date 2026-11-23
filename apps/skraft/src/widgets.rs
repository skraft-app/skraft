//! The widgets the interface's screens share (docs/ui/interface.md#widgets): a screen draws
//! its buttons, fields and rows with these rather than its own.

use crate::icons;

mod button;
mod field;
mod menu;
mod segmented;
mod tree;

pub(crate) use button::{OPTION_HEIGHT, option_button};
pub(crate) use field::{Field, cut_label, field_stroke, framed_field, rename_field};
pub(crate) use menu::{command_item, is_toggle};
pub(crate) use segmented::{Segment, segmented, segmented_sized};
pub(crate) use tree::{Fold, RENAMING, band_color, fold_toggle, hidden_color, paint_drag_ghost, set_drag_ghost, start_rename, tree_row};

/// The warning sign before an error to correct (a part in several pieces, a shortcut's
/// conflict), in the error colour, as high as the text.
pub fn warning(ui: &egui::Ui) -> egui::Image<'static> {
    let side = ui.text_style_height(&egui::TextStyle::Body);
    egui::Image::new(icons::WARNING).fit_to_exact_size(egui::vec2(side, side)).tint(ui.visuals().error_fg_color)
}

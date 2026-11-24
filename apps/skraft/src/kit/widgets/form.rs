//! A modal box's form (docs/ui/interface.md#widgets): its fields in rows, the labels on the left,
//! the fields of one width against the right edge, each followed by its unit; the line saying what
//! is wrong; a warning in a tinted box.

use super::button::OPTION_HEIGHT;
use super::field::{FieldLook, live_field};

/// A form field's width (points): a length, a count.
pub(crate) const FORM_FIELD_WIDTH: f32 = 72.0;
/// The room after a field for its unit (points).
const UNIT_WIDTH: f32 = 26.0;

/// One row of a form.
pub(crate) struct FormField<'a> {
    pub(crate) label: &'a str,
    /// After the field, faint (`mm`).
    pub(crate) unit: &'a str,
    /// Its value being typed, read in a figure beside the form: the label in semibold, in the
    /// accent's ink, as the figure's leader to it.
    pub(crate) lit: bool,
    /// What is typed cannot be read: the field outlined in the error colour.
    pub(crate) bad: bool,
    /// Its name among the named handles (docs/control-protocol.md).
    pub(crate) handle: &'a str,
}

/// Draws a row: the label on the left, the field (`text`, in the egui field `id`, checked as it is
/// typed) and its unit against the right edge. The field's response.
pub(crate) fn form_row(ui: &mut egui::Ui, id: egui::Id, text: &mut String, f: FormField) -> egui::Response {
    let row = egui::vec2(ui.available_width(), OPTION_HEIGHT);
    ui.allocate_ui_with_layout(row, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        if f.lit {
            let size = egui::TextStyle::Body.resolve(ui.style()).size;
            let ink = ui.visuals().selection.stroke.color;
            ui.label(egui::RichText::new(f.label).font(crate::kit::style::semibold(size)).color(ink));
        } else {
            ui.label(f.label);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(UNIT_WIDTH, OPTION_HEIGHT), egui::Sense::hover());
            let font = egui::TextStyle::Body.resolve(ui.style());
            ui.painter().text(rect.right_center(), egui::Align2::RIGHT_CENTER, f.unit, font, ui.visuals().weak_text_color());
            let look = FieldLook {
                hint: None,
                tip: None,
                width: FORM_FIELD_WIDTH,
                align: egui::Align2::RIGHT_CENTER,
                lit: false,
                bad: f.bad,
                addon: None,
                handle: Some(f.handle.into()),
            };
            live_field(ui, id, text, look)
        })
        .inner
    })
    .inner
}

/// The line saying what is wrong, small, in the error colour, right-aligned under the fields;
/// kept when there is nothing to say: the box does not jump.
pub(crate) fn form_message(ui: &mut egui::Ui, text: Option<&str>) {
    let height = ui.text_style_height(&egui::TextStyle::Small);
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(text) = text {
            ui.label(egui::RichText::new(text).small().color(ui.visuals().error_fg_color));
        }
    });
}

/// A warning in a box tinted with the error colour, its sign before it, wrapped to the width.
pub(crate) fn warning_box(ui: &mut egui::Ui, text: &str) {
    let error = ui.visuals().error_fg_color;
    egui::Frame::new()
        .fill(error.gamma_multiply(0.08))
        .stroke(egui::Stroke::new(1.0, error.gamma_multiply(0.35)))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.add(super::warning(ui));
                ui.add(egui::Label::new(text).wrap());
            });
        });
}

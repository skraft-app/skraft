//! The rows of the menus (docs/ui/interface.md#widgets).

use crate::cards::panels::CommandView;
use crate::kit::icons;
use crate::kit::style::Tip;

/// Whether a command is shown in a menu as a check box (a display toggle).
pub fn is_toggle(v: &CommandView) -> bool {
    v.checked.is_some() && v.id.starts_with("view.")
}

/// A menu item: a check box for a toggle, else a button with its shortcut; no hover text.
/// `column` when the menu has check boxes: the label after their column. True when clicked.
pub fn command_item(ui: &mut egui::Ui, v: &CommandView, column: bool) -> bool {
    let mut button = if is_toggle(v) {
        crate::kit::style::menu_item(&v.label, v.checked, column)
    } else {
        crate::kit::style::menu_item(&v.label, None, column).selected(v.checked == Some(true))
    };
    if let Some(s) = &v.shortcut {
        button = button.shortcut_text(s);
    }
    let r = ui.add_enabled(v.enabled, button);
    crate::kit::handles::publish_enabled(|| format!("menu:{}", v.id), r.rect, v.enabled);
    r.clicked()
}

/// A row of a menu listing commands to pin (the ribbon's groups, the view bar's and the quick
/// tools' ▾).
pub(crate) struct PinRow<'a> {
    pub(crate) icon: Option<egui::ImageSource<'static>>,
    /// `Some`: a column of check boxes every row keeps, this row's state in it (none: empty).
    pub(crate) check: Option<Option<bool>>,
    pub(crate) label: &'a str,
    pub(crate) shortcut: Option<&'a str>,
    pub(crate) enabled: bool,
    pub(crate) selected: bool,
    /// The least width of the row's button.
    pub(crate) width: f32,
    pub(crate) pinned: bool,
    pub(crate) pin_tip: String,
}

/// A command's row and its pin, side by side: the row's response, and whether the pin was clicked.
pub(crate) fn pin_row(ui: &mut egui::Ui, row: PinRow<'_>) -> (egui::Response, bool) {
    ui.horizontal(|ui| {
        let ink = ui.visuals().text_color();
        let image = row.icon.map(|icon| egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0)).tint(ink));
        let button = match (row.check, image) {
            (Some(check), Some(image)) => egui::Button::new((crate::kit::style::menu_check(check), image, row.label)),
            (Some(check), None) => egui::Button::new((crate::kit::style::menu_check(check), row.label)),
            (None, Some(image)) => egui::Button::image_and_text(image, row.label),
            (None, None) => egui::Button::new(row.label),
        };
        // Always a shortcut, if empty: it pushes the label to the left (else centred).
        let button = button.selected(row.selected).min_size(egui::vec2(row.width, 22.0)).shortcut_text(row.shortcut.unwrap_or(""));
        let run = ui.add_enabled(row.enabled, button);
        // Pinned: a filled pin in the accent; unpinned: a faint outline.
        let (icon, tint) = if row.pinned {
            (icons::PIN_FILLED, ui.visuals().selection.stroke.color)
        } else {
            (icons::PIN, ui.visuals().weak_text_color().gamma_multiply(0.55))
        };
        let pin = egui::Button::image(egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint)).frame(false);
        let pin = ui.add(pin).tip(row.pin_tip).clicked();
        (run, pin)
    })
    .inner
}

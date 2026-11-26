//! What the window shows instead of the app when it is too small for it
//! (docs/ui/interface.md#too-small): the app's mark over the start page's grid, and a line —
//! « turn the device » for a touch screen on its side. The documents stay open behind it.

use skraft_i18n::t;

use crate::kit::icons;
use crate::window::start::{MARK, MARK_TILE};

/// The least room (points, each way) the app is drawn in: the compact arrangement's bars and a
/// view still worth working in. An iPhone SE's width.
pub const MIN_SIDE: f32 = 320.0;

const TEXT_SIZE: f32 = 17.0;

/// Whether a window of `size` (points) is too small for the app.
pub fn too_small(size: egui::Vec2) -> bool {
    size.min_elem() < MIN_SIDE
}

/// The screen in `ui`; `sideways`: a touch screen on its side, which may turn.
pub(crate) fn show(ui: &mut egui::Ui, sideways: bool) {
    crate::window::start::grid(ui, ui.max_rect());
    let text = if sideways { t!("too_small.rotate") } else { t!("too_small.room") };
    let font = crate::kit::style::semibold(TEXT_SIZE);
    let galley = ui.painter().layout(text.to_string(), font, ui.visuals().strong_text_color(), ui.max_rect().width() - 32.0);
    let gap = 16.0;
    let height = MARK + gap + galley.size().y;
    let center = ui.max_rect().center();
    let top = center.y - height / 2.0;
    let mark = egui::Rect::from_center_size(egui::pos2(center.x, top + MARK / 2.0), egui::Vec2::splat(MARK / MARK_TILE));
    egui::Image::new(icons::APP).paint_at(ui, mark);
    let at = egui::pos2(center.x - galley.size().x / 2.0, top + MARK + gap);
    ui.painter().galley(at, galley, egui::Color32::PLACEHOLDER);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_upright_fits_and_on_its_side_under_safari_bars_does_not() {
        assert!(!too_small(egui::vec2(320.0, 568.0)));
        assert!(too_small(egui::vec2(852.0, 297.0)));
        assert!(too_small(egui::vec2(300.0, 600.0)));
    }
}

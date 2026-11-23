//! The buttons of the interface (docs/ui/interface.md#widgets).

use crate::icons;
use crate::style::Tip;

/// The height of the option bar's buttons; an icon's button is a square of it.
pub(crate) const OPTION_HEIGHT: f32 = crate::style::BAR_BUTTON;
pub(crate) const OPTION_ICON: f32 = crate::style::BAR_ICON;
/// The side margins of a text button.
const OPTION_PAD: f32 = 8.0;

/// What a button of the option bar shows: its icon (its label then in the tip), else its label.
pub(super) enum Face {
    Icon(egui::ImageSource<'static>),
    Text(std::sync::Arc<egui::Galley>),
}

impl Face {
    pub(super) fn new(ui: &egui::Ui, label: &str, icon: Option<egui::ImageSource<'static>>) -> Self {
        match icon {
            Some(icon) => Face::Icon(icon),
            None => Face::Text(ui.painter().layout_no_wrap(
                label.to_owned(),
                egui::TextStyle::Button.resolve(ui.style()),
                egui::Color32::PLACEHOLDER,
            )),
        }
    }

    /// Its button's width at `height`: a square for an icon.
    pub(super) fn width(&self, height: f32) -> f32 {
        match self {
            Face::Icon(_) => height,
            Face::Text(g) => g.size().x + 2.0 * OPTION_PAD,
        }
    }

    /// Painted in `rect`, an icon `icon` points wide.
    pub(super) fn paint(&self, ui: &egui::Ui, rect: egui::Rect, icon: f32, ink: egui::Color32) {
        match self {
            Face::Icon(image) => {
                egui::Image::new(image.clone()).tint(ink).paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(icon)))
            }
            Face::Text(g) => ui.painter().galley(rect.center() - g.size() / 2.0, g.clone(), ink),
        }
    }
}

/// The tip of a button, and what it adds smaller under it: an icon's is its label, then its tip.
pub(super) fn face_tip<'a>(face: &Face, label: &'a str, tip: &'a str) -> (&'a str, &'a str) {
    match face {
        Face::Icon(_) => (label, tip),
        Face::Text(_) => (tip, ""),
    }
}

/// A toggle of the option bar, drawn like a ribbon button: no frame until hovered, the
/// selection's fill and outline when on — faded with its card while it is translucent
/// (`style::fill_alpha`, docs/ui/interface.md#look), its ink not. True when clicked.
pub(crate) fn option_button(
    ui: &mut egui::Ui,
    handle: &str,
    selected: bool,
    label: &str,
    tip: &str,
    icon: Option<&str>,
    swatch: Option<[u8; 3]>,
) -> bool {
    let face = Face::new(ui, label, icon.and_then(icons::option));
    let (rect, response) = ui.allocate_exact_size(egui::vec2(face.width(OPTION_HEIGHT), OPTION_HEIGHT), egui::Sense::click());
    crate::handles::publish(|| handle.to_owned(), rect);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let visuals = ui.style().interact_selectable(&response, selected);
    let alpha = crate::style::fill_alpha(ui);
    let stroke = faded(visuals.bg_stroke, alpha);
    // On, filled with its colour (« Pipette »'s material, docs/tools/draw-options.md), as OCL's
    // button: the toggle's value, not faded.
    let swatch = swatch.filter(|_| selected).map(|[r, g, b]| egui::Color32::from_rgb(r, g, b));
    if let Some(fill) = swatch {
        ui.painter().rect(rect, visuals.corner_radius, fill, stroke, egui::StrokeKind::Inside);
    } else if selected || response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill.gamma_multiply(alpha), stroke, egui::StrokeKind::Inside);
    }
    let ink = match swatch {
        Some(fill) => ink_on(fill),
        None if selected => visuals.text_color(),
        None => ui.visuals().text_color(),
    };
    face.paint(ui, rect, OPTION_ICON, ink);
    let (text, more) = face_tip(&face, label, tip);
    response.tip_more(text, more).clicked()
}

/// `stroke` faded by `alpha`.
pub(super) fn faded(mut stroke: egui::Stroke, alpha: f32) -> egui::Stroke {
    stroke.color = stroke.color.gamma_multiply(alpha);
    stroke
}

/// The ink readable on `fill`: black on a light colour, white on a dark one (OCL's
/// `ColorUtils.color_is_dark?`).
fn ink_on(fill: egui::Color32) -> egui::Color32 {
    let l = egui::Rgba::from(fill);
    let luminance = 0.2126 * l.r() + 0.7152 * l.g() + 0.0722 * l.b();
    if luminance > 0.18 { egui::Color32::BLACK } else { egui::Color32::WHITE }
}

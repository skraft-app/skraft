//! The buttons of the interface (docs/ui/interface.md#widgets).

use crate::kit::icons;
use crate::kit::style::Tip;

/// A frameless button's corner radius.
pub const VEIL_RADIUS: f32 = 5.0;

/// A frameless button's background: a veil of the text's colour when hovered, darker when
/// pressed — seen on any ground, in either theme. None at rest.
pub fn veil(ui: &egui::Ui, resp: &egui::Response) -> Option<egui::Color32> {
    veil_of(ui.visuals().text_color(), resp)
}

/// A [`veil`] of `ink`.
fn veil_of(ink: egui::Color32, resp: &egui::Response) -> Option<egui::Color32> {
    let alpha = if resp.is_pointer_button_down_on() {
        0.22
    } else if resp.hovered() {
        0.12
    } else {
        return None;
    };
    Some(ink.gamma_multiply(alpha))
}

/// Which of a modal box's [`confirm_buttons`] was clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Confirm {
    Ok,
    Cancel,
}

/// The buttons closing a modal box, from the right edge of the row they end: `ok`, filled with
/// the selection's colour and clickable while `enabled`, then `cancel`. Published as
/// `<handle>.ok` and `<handle>.cancel`. Enter and Esc stay the box's to answer.
pub(crate) fn confirm_buttons(ui: &mut egui::Ui, handle: &str, ok: String, enabled: bool, cancel: String) -> Option<Confirm> {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let fill = ui.visuals().selection.bg_fill;
        let ok = ui.add_enabled(enabled, egui::Button::new(ok).fill(fill));
        crate::kit::handles::publish_enabled(|| format!("{handle}.ok"), ok.rect, enabled);
        let cancel = ui.button(cancel);
        crate::kit::handles::publish(|| format!("{handle}.cancel"), cancel.rect);
        if ok.clicked() {
            Some(Confirm::Ok)
        } else if cancel.clicked() {
            Some(Confirm::Cancel)
        } else {
            None
        }
    })
    .inner
}

/// A [`LabelButton`]'s side margins, its icon's width and the room between icon and label.
const LABEL_PAD: f32 = 18.0;
const LABEL_ICON: f32 = 16.0;
const LABEL_GAP: f32 = 8.0;
const LABEL_TEXT: f32 = 14.0;

/// A large button, an icon before its label (the start page's). The main one is filled and
/// outlined with the selection's colours, as [`confirm_buttons`]' ok; the others are framed as
/// a field. Hovered, pressed or focused, outlined in the accent, as every widget. Measured
/// first ([`LabelButton::width`]), then drawn where its caller lays it out.
pub(crate) struct LabelButton {
    icon: egui::ImageSource<'static>,
    label: std::sync::Arc<egui::Galley>,
    main: bool,
}

impl LabelButton {
    pub(crate) fn new(ui: &egui::Ui, icon: egui::ImageSource<'static>, label: String, main: bool) -> Self {
        let font = if main { crate::kit::style::semibold(LABEL_TEXT) } else { egui::FontId::proportional(LABEL_TEXT) };
        let label = ui.painter().layout_no_wrap(label, font, egui::Color32::PLACEHOLDER);
        LabelButton { icon, label, main }
    }

    /// Its width at rest: its margins, its icon and its label.
    pub(crate) fn width(&self) -> f32 {
        2.0 * LABEL_PAD + LABEL_ICON + LABEL_GAP + self.label.size().x
    }

    /// Drawn in `rect` (wider than [`width`](Self::width): its content centred), published as
    /// `handle`.
    pub(crate) fn show_at(self, ui: &mut egui::Ui, rect: egui::Rect, handle: &str) -> egui::Response {
        let r = ui.interact(rect, ui.scope_id().with(("label_button", handle)), egui::Sense::click());
        crate::kit::handles::publish(|| handle.to_owned(), rect);
        let text = self.label.text().to_owned();
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &text));
        let v = ui.visuals();
        let pressed = r.is_pointer_button_down_on();
        let (mut fill, mut stroke) =
            if self.main { (v.selection.bg_fill, v.selection.stroke) } else { (v.extreme_bg_color, v.widgets.noninteractive.bg_stroke) };
        if r.hovered() || pressed || r.has_focus() {
            stroke.color = v.widgets.hovered.bg_stroke.color;
        }
        if pressed {
            if self.main {
                stroke.width += 1.0;
            } else {
                fill = v.widgets.active.weak_bg_fill;
            }
        }
        let ink = v.text_color();
        ui.painter().rect(rect, v.window_corner_radius, fill, stroke, egui::StrokeKind::Inside);
        let size = self.label.size();
        let left = rect.center().x - (LABEL_ICON + LABEL_GAP + size.x) / 2.0;
        let icon = egui::Rect::from_center_size(egui::pos2(left + LABEL_ICON / 2.0, rect.center().y), egui::Vec2::splat(LABEL_ICON));
        egui::Image::new(self.icon).tint(ink).paint_at(ui, icon);
        ui.painter().galley(egui::pos2(left + LABEL_ICON + LABEL_GAP, rect.center().y - size.y / 2.0), self.label, ink);
        r
    }
}

/// A button whose frame is not drawn ([`backed`] draws its background) but keeps its margins.
pub(crate) fn unframed(button: egui::Button) -> egui::Button {
    button.fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE)
}

/// Draws a frameless button with `add`, and behind it the tabs' + button's background: none
/// at rest, the [`veil`] when hovered or pressed, the selection's when `lit` (asked once
/// the button is drawn: a menu's button is lit while its menu is open).
pub(crate) fn backed(
    ui: &mut egui::Ui,
    lit: impl FnOnce(&egui::Response) -> bool,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let behind = ui.painter().add(egui::Shape::Noop);
    let r = add(ui);
    let fill = if lit(&r) { Some(ui.visuals().selection.bg_fill) } else { veil(ui, &r) };
    if let Some(fill) = fill {
        ui.painter().set(behind, egui::Shape::rect_filled(r.rect, VEIL_RADIUS, fill));
    }
    r
}

/// How an [`IconButton`] shows itself.
#[derive(Clone, Copy)]
pub(crate) enum Look {
    /// No frame at all (a navigator row's).
    Plain,
    /// egui's button frame (the entity info's).
    Framed,
    /// A [`veil`] when hovered or pressed, the selection's fill when lit (the title bar, the tabs).
    Veil,
    /// A floating bar's: the widgets' frame when hovered, the selection's fill when lit. It
    /// senses drags even when it cannot run (a drag moves it), so it is dimmed, not disabled.
    Bar,
}

/// The one icon button of the interface: `side` points square, its icon `glyph` wide.
pub(crate) struct IconButton {
    icon: egui::ImageSource<'static>,
    side: f32,
    glyph: f32,
    look: Look,
    ink: Option<egui::Color32>,
    lit: bool,
    enabled: bool,
}

impl IconButton {
    pub(crate) fn new(icon: egui::ImageSource<'static>, side: f32, glyph: f32) -> Self {
        IconButton { icon, side, glyph, look: Look::Plain, ink: None, lit: false, enabled: true }
    }

    pub(crate) fn look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }

    /// Its icon's colour; by default the text's.
    pub(crate) fn ink(mut self, ink: egui::Color32) -> Self {
        self.ink = Some(ink);
        self
    }

    pub(crate) fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Laid out in `ui`.
    pub(crate) fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let (lit, enabled) = (self.lit, self.enabled);
        match self.look {
            Look::Plain => {
                let button = self.button(ui).frame(false);
                ui.add_enabled(enabled, button)
            }
            Look::Framed => {
                let button = self.button(ui);
                ui.add_enabled(enabled, button)
            }
            Look::Veil => {
                let button = unframed(self.button(ui));
                backed(ui, |_| lit, |ui| ui.add_enabled(enabled, button))
            }
            Look::Bar => self.bar(ui),
        }
    }

    /// At `rect`, under the key `id`, with a [`veil`] of its ink: a button of a strip laid out by
    /// hand (the tabs'), which keeps its key as the others move.
    pub(crate) fn show_at(self, ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id) -> egui::Response {
        let response = ui.interact(rect, id, egui::Sense::click());
        let ink = self.ink.unwrap_or_else(|| ui.visuals().text_color());
        if let Some(fill) = veil_of(ink, &response) {
            ui.painter().rect_filled(rect, VEIL_RADIUS, fill);
        }
        egui::Image::new(self.icon).tint(ink).paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(self.glyph)));
        response
    }

    /// As an egui button, its margins kept.
    fn button(self, ui: &egui::Ui) -> egui::Button<'static> {
        let ink = self.ink.unwrap_or_else(|| ui.visuals().text_color());
        let image = egui::Image::new(self.icon).fit_to_exact_size(egui::Vec2::splat(self.glyph)).tint(ink);
        egui::Button::image(image).min_size(egui::Vec2::splat(self.side))
    }

    fn bar(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(self.side), egui::Sense::click_and_drag());
        let visuals = ui.style().interact(&response);
        if self.lit {
            ui.painter().rect_filled(rect, visuals.corner_radius, ui.visuals().selection.bg_fill);
        } else if self.enabled && response.hovered() {
            ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
        }
        let mut ink = self.ink.unwrap_or_else(|| if self.lit { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() });
        if !self.enabled {
            ink = ink.gamma_multiply(0.35);
        }
        egui::Image::new(self.icon).tint(ink).paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(self.glyph)));
        response
    }
}

/// The height of the option bar's buttons; an icon's button is a square of it.
pub(crate) const OPTION_HEIGHT: f32 = crate::kit::style::BAR_BUTTON;
pub(crate) const OPTION_ICON: f32 = crate::kit::style::BAR_ICON;
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
    crate::kit::handles::publish(|| handle.to_owned(), rect);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let visuals = ui.style().interact_selectable(&response, selected);
    let alpha = crate::kit::style::fill_alpha(ui);
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

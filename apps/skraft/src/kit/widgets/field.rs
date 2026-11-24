//! The text fields of the interface (docs/ui/interface.md#widgets).

use super::button::{OPTION_HEIGHT, faded};
use crate::kit::style::Tip;

/// A value of the entity info cut short (…) at `width` rather than widening the panel; whole
/// in its tool tip (egui's, when cut).
///
/// One line high, not a `ui.scope`: in a `Grid` cell a scope spans the whole height the row had
/// the frame before (its centred row layout), so the row never shrank back — the « Dimensions »
/// row kept the three position fields' height of the row it replaced (an assembly's).
pub(crate) fn cut_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, width: f32) {
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add(egui::Label::new(text).truncate());
    });
}

/// How a framed field looks, whatever it does with what is typed ([`framed_field`],
/// [`live_field`]).
pub(crate) struct FieldLook<'a> {
    /// Shown faint while it is empty (a value it would inherit).
    pub(crate) hint: Option<&'a str>,
    pub(crate) tip: Option<&'a str>,
    /// The whole width, its add-on's included.
    pub(crate) width: f32,
    /// A number's to the right, a name's to the left.
    pub(crate) align: egui::Align2,
    /// Outlined with the selection's colour (a tool option's value set).
    pub(crate) lit: bool,
    /// A letter in a colour, in a box on the left (an axis' `X`).
    pub(crate) addon: Option<(&'a str, egui::Color32)>,
    /// Its name among the named handles (docs/control-protocol.md), if published.
    pub(crate) handle: Option<String>,
}

/// How a [`framed_field`] looks and applies.
pub(crate) struct Field<'a> {
    /// Shown faint while it is empty (a value it would inherit).
    pub(crate) hint: Option<&'a str>,
    pub(crate) tip: Option<&'a str>,
    /// The whole width, its add-on's included.
    pub(crate) width: f32,
    /// A number's to the right, a name's to the left.
    pub(crate) align: egui::Align2,
    /// Outlined with the selection's colour (a tool option's value set).
    pub(crate) lit: bool,
    /// A letter in a colour, in a box on the left (an axis' `X`).
    pub(crate) addon: Option<(&'a str, egui::Color32)>,
    /// A click away applies what was typed (Entity Info), else puts the value back (the tool
    /// options, applied by Enter only).
    pub(crate) blur_applies: bool,
    /// An empty text applies too (a body's name cleared, docs/model/bodies.md#body-names), else it puts the
    /// value back.
    pub(crate) empty_applies: bool,
    /// Its name among the named handles (docs/control-protocol.md), if published.
    pub(crate) handle: Option<String>,
}

/// The outline of a [`framed_field`] (and of a [`material_choice`]): the border's colour at rest,
/// the hovered ink's when hovered, the selection's when lit, twice as thick when focused (or open).
pub(crate) fn field_stroke(visuals: &egui::Visuals, focused: bool, lit: bool, hovered: bool) -> egui::Stroke {
    if focused {
        egui::Stroke::new(2.0, visuals.selection.stroke.color)
    } else if lit {
        egui::Stroke::new(1.0, visuals.selection.stroke.color)
    } else if hovered {
        egui::Stroke::new(1.0, visuals.widgets.hovered.fg_stroke.color)
    } else {
        visuals.widgets.noninteractive.bg_stroke
    }
}

/// The one text field of the interface, as it is drawn: always framed, the fields' ground, an
/// outline telling its state — the border's colour at rest, the hovered ink's when hovered, the
/// selection's when lit, twice as thick when focused. `text` in the egui field `field_id`.
fn draw_field(ui: &mut egui::Ui, field_id: egui::Id, text: &mut String, f: FieldLook) -> egui::Response {
    // The outline from the state of the last frame (the field's is only known once drawn).
    let focused = ui.memory(|m| m.has_focus(field_id));
    let hovered = ui.ctx().read_response(field_id).is_some_and(|r| r.hovered());
    let visuals = ui.visuals();
    // Faded with its card while it is translucent (the option bar's, docs/ui/interface.md#look),
    // its text not.
    let alpha = crate::kit::style::fill_alpha(ui);
    let border = faded(visuals.widgets.noninteractive.bg_stroke, alpha);
    let stroke = faded(field_stroke(visuals, focused, f.lit, hovered), alpha);
    let radius = visuals.widgets.inactive.corner_radius;
    let addon = f.addon.map(|_| OPTION_HEIGHT).unwrap_or(0.0);
    let mut corners = radius;
    if addon > 0.0 {
        (corners.nw, corners.sw) = (0, 0);
    }
    // The margin keeps room for the thicker outline: focusing does not move the text.
    let frame = egui::Frame::new()
        .fill(visuals.extreme_bg_color.gamma_multiply(alpha))
        .stroke(stroke)
        .corner_radius(corners)
        .inner_margin(egui::Margin::symmetric(6, 2) - egui::Margin::from(stroke.width));
    let (ground, ink) = (visuals.widgets.inactive.bg_fill, visuals.text_color());

    // Its row as high as the field from the start: `ui.horizontal` starts one at
    // `interact_size.y`, centered in the parent's row, and grows it downwards to the field's
    // height — the field lower than its neighbours, the parent's row taller for those after it.
    let row = egui::vec2(f.width, OPTION_HEIGHT);
    let r = ui
        .allocate_ui_with_layout(row, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            // The add-on's border and the field's are one line: the field's, drawn over it.
            ui.spacing_mut().item_spacing.x = -border.width;
            if let Some((letter, color)) = f.addon {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(OPTION_HEIGHT), egui::Sense::hover());
                let p = ui.painter();
                let rect_corners = egui::CornerRadius { nw: radius.nw, sw: radius.sw, ne: 0, se: 0 };
                p.rect(rect, rect_corners, ground.lerp_to_gamma(ink, 0.04).gamma_multiply(alpha), border, egui::StrokeKind::Inside);
                let font = egui::TextStyle::Button.resolve(ui.style());
                p.text(rect.center(), egui::Align2::CENTER_CENTER, letter, font, color);
            }
            let width = f.width - addon + if addon > 0.0 { border.width } else { 0.0 };
            let field = egui::TextEdit::singleline(text)
                .id(field_id)
                .frame(frame)
                .desired_width(width)
                .min_size(egui::vec2(width, OPTION_HEIGHT))
                .align(f.align);
            let field = match f.hint {
                Some(hint) => field.hint_text(hint),
                None => field,
            };
            let r = ui.add(field);
            match f.tip {
                Some(tip) => r.tip(tip),
                None => r,
            }
        })
        .inner;
    if let Some(name) = f.handle {
        crate::kit::handles::publish(|| name, r.rect);
    }
    r
}

/// All of the field `r`'s text selected, to be typed over.
fn select_all(ui: &egui::Ui, r: &egui::Response, text: &str) {
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), r.id) {
        ui.ctx().request_repaint();
        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
        state.cursor.set_char_range(Some(all));
        state.store(ui.ctx(), r.id);
    }
}

/// The field of a value applied once typed (the tool options', Entity Info's, the materials'):
/// [`draw_field`]'s look. Focused, all of it is selected to be typed over: the `exact` text, as
/// shown as `shown` (`~ 12` edited as `12,346`), Enter on it unchanged applying nothing; Esc puts
/// the value back. The text typed, when applied.
pub(crate) fn framed_field(ui: &mut egui::Ui, id: egui::Id, shown: &str, exact: &str, f: Field) -> Option<String> {
    let field_id = id.with("field");
    let focused = ui.memory(|m| m.has_focus(field_id));
    // While focused, the text typed (kept across frames); else the value.
    let mut text: String = focused.then(|| ui.data(|d| d.get_temp(id))).flatten().unwrap_or_else(|| shown.to_owned());
    let look = FieldLook { hint: f.hint, tip: f.tip, width: f.width, align: f.align, lit: f.lit, addon: f.addon, handle: f.handle };
    let r = draw_field(ui, field_id, &mut text, look);
    // Just focused (clicked, tabbed into): the real value, all of it selected, to be typed over.
    if r.gained_focus() {
        text = exact.to_owned();
        select_all(ui, &r, &text);
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        let applied =
            ui.input(|i| if i.key_pressed(egui::Key::Escape) { false } else { f.blur_applies || i.key_pressed(egui::Key::Enter) });
        let unchanged = text.trim() == exact || text.trim() == shown;
        return (applied && (f.empty_applies || !text.trim().is_empty()) && !unchanged).then(|| text.trim().to_owned());
    }
    // egui's focus, not `Response::has_focus`: that one is also false while the window is not
    // the system's focused one, and the text typed then (control channel, docs/control-protocol.md) was lost.
    if ui.memory(|m| m.has_focus(r.id)) {
        ui.data_mut(|d| d.insert_temp(id, text));
    }
    None
}

/// The field of a text checked as it is typed (a modal box's name, its lengths): [`draw_field`]'s
/// look, `text` changed at each keystroke, in the egui field `id`. Focused, all of it is selected
/// to be typed over. Enter and Esc are the box's.
pub(crate) fn live_field(ui: &mut egui::Ui, id: egui::Id, text: &mut String, look: FieldLook) -> egui::Response {
    let r = draw_field(ui, id, text, look);
    if r.gained_focus() {
        select_all(ui, &r, text);
    }
    r
}

/// A row's name (a body's, a component's) edited in place in its navigator row (docs/model/bodies.md#body-names), under the key
/// `id`: the name `current` wholly selected as it opens, to be typed over. Enter or a click away
/// gives the text typed — empty: the name cleared —, Esc nothing: `Some` once done, `Some(None)`
/// when nothing changes; None while it is edited.
pub(crate) fn rename_field(ui: &mut egui::Ui, id: egui::Id, current: &str) -> Option<Option<String>> {
    let (text_id, started) = (id.with("text"), id.with("started"));
    let first = !ui.data(|d| d.get_temp::<bool>(started).unwrap_or(false));
    let mut text = ui.data(|d| d.get_temp::<String>(text_id)).unwrap_or_else(|| current.to_owned());
    // Centred in the row as the name it replaces: egui's theme gives the field the row's height
    // (`interact_size`), taller than a line of text, and would keep the text at its top.
    let edit = egui::TextEdit::singleline(&mut text)
        .id(id.with("edit"))
        .desired_width(ui.available_width())
        .margin(egui::vec2(2.0, 0.0))
        .align(egui::Align2::LEFT_CENTER);
    let r = ui.add(edit);
    let done = |ui: &mut egui::Ui| ui.data_mut(|d| (d.remove::<String>(text_id), d.remove::<bool>(started)));
    // Focused and all of it selected as it opens (and whenever focus comes back to it).
    if first || r.gained_focus() {
        if first {
            r.request_focus();
            ui.data_mut(|d| d.insert_temp(started, true));
        }
        let mut state = egui::TextEdit::load_state(ui.ctx(), r.id).unwrap_or_default();
        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
        state.cursor.set_char_range(Some(all));
        state.store(ui.ctx(), r.id);
        ui.ctx().request_repaint();
    }
    // egui's focus, as `framed_field`'s (the window not the system's focused one).
    let focused = ui.memory(|m| m.has_focus(r.id));
    if r.lost_focus() || (!first && !focused && !r.gained_focus()) {
        done(ui);
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let name = text.trim();
        return Some((!escaped && name != current).then(|| name.to_owned()));
    }
    ui.data_mut(|d| d.insert_temp(text_id, text));
    None
}

//! The steps bar and the validation buttons (docs/ui/interface.md#steps-bar), after OpenCutList's
//! `SmartTool#set_steps_bar` and `#show_validation`, what any tool tells
//! ([`skraft_tools::Steps`], `Tool::awaits_validation`): at the bottom centre of the view, the
//! steps' numbers, the current one filled, its sentence framed below (the toasts above); at the
//! top centre, under the tool's options, ✗ and ✓, which give the tool Esc and Enter.

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use skraft_i18n::t;
use skraft_tools::{Key, Steps};

use crate::commands::Action;
use crate::kit::style::Tip;
use crate::window::toasts::Level;

/// A step number's square, and the link between two.
const NUMBER: f32 = 22.0;
const LINK: f32 = 24.0;
/// A validation button's side, its motif's, and the gap between the two (OCL's `unit * 4`).
const BUTTON: f32 = 40.0;
const MOTIF: f32 = 16.0;
const BUTTON_GAP: f32 = 24.0;
const GAP: f32 = 8.0;

/// Draws the steps bar above `bottom` (the view's bottom), centred on `center` (the view's), kept
/// between `left` and `right` (the side panels); its top, none when the tool has no steps.
pub fn bar(ctx: &egui::Context, center: f32, (left, right): (f32, f32), bottom: f32, steps: Option<&Steps>) -> Option<f32> {
    let s = steps?;
    // The room about the centre between the panels, as wide on both sides (the nearer panel's):
    // the sentence wraps only past it.
    let room = (2.0 * (center - left).min(right - center) - 2.0 * GAP).max(NUMBER);
    let area = egui::Area::new(egui::Id::unique("steps_bar"))
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(center, bottom - GAP))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = GAP;
                if s.count > 0 {
                    stepper(ui, s);
                }
                let v = ui.visuals();
                egui::Frame::new()
                    .fill(v.window_fill.gamma_multiply(0.85))
                    .stroke(v.window_stroke)
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(14, 7))
                    .show(ui, |ui| sentence(ui, &s.text, room - 28.0));
            });
        });
    Some(area.response.rect.top())
}

/// Draws ✗ and ✓ below `top` (under the tool's options), centred on `center` (the view's).
pub fn validation(ctx: &egui::Context, center: f32, top: f32, actions: &mut Vec<Action>) {
    egui::Area::new(egui::Id::unique("validation"))
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(pos2(center, top))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| buttons(ui, true, actions));
}

/// ✗ alone, where the validation goes: under a finger, Esc while a tool's operation is under way
/// (docs/ui/touch.md#modifiers).
pub fn step_back(ctx: &egui::Context, center: f32, top: f32, actions: &mut Vec<Action>) {
    egui::Area::new(egui::Id::unique("validation"))
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(pos2(center, top))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| buttons(ui, false, actions));
}

/// The step's sentence on its own lines (`\n` only), unless it is wider than `room`: then
/// wrapped to it. Measured, not left to the area's width (which is last frame's: a longer
/// sentence would wrap to the shorter one's).
fn sentence(ui: &mut egui::Ui, text: &str, room: f32) {
    let text = egui::RichText::new(text).strong();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width =
        text.text().lines().map(|l| ui.painter().layout_no_wrap(l.to_owned(), font.clone(), Color32::WHITE).size().x).fold(0.0, f32::max);
    if width <= room {
        ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
    } else {
        // All the room, not the area's width as last drawn.
        ui.set_width(room);
        ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Wrap));
    }
}

/// The numbers, linked: the current one filled, those passed in the text's colour, the next ones
/// dimmed.
fn stepper(ui: &mut egui::Ui, s: &Steps) {
    let width = s.count as f32 * NUMBER + s.count.saturating_sub(1) as f32 * LINK;
    let (rect, _) = ui.allocate_exact_size(vec2(width, NUMBER), Sense::hover());
    let v = ui.visuals();
    let (strong, dim, ground) = (v.strong_text_color(), v.weak_text_color(), v.window_fill);
    let painter = ui.painter();
    let font = egui::FontId::proportional(13.0);
    for i in 0..s.count {
        let x = rect.left() + i as f32 * (NUMBER + LINK);
        if i > 0 {
            let y = rect.center().y;
            let color = if i <= s.index { strong } else { dim };
            painter.line_segment([pos2(x - LINK, y), pos2(x, y)], Stroke::new(2.0, color));
        }
        let square = Rect::from_min_size(pos2(x, rect.top()), vec2(NUMBER, NUMBER));
        let (fill, line, text) = match i.cmp(&s.index) {
            std::cmp::Ordering::Equal => (strong, strong, ground),
            std::cmp::Ordering::Less => (ground, strong, strong),
            std::cmp::Ordering::Greater => (ground, dim, dim),
        };
        let font = if i == s.index { crate::kit::style::semibold(13.0) } else { font.clone() };
        number(painter, square, &s.name(i), (fill, line, text), font);
    }
}

/// A step's name (`Steps::name`) centred on `at`, drawn as a step passed: on a part the step
/// took, where it was clicked (docs/ui/interface.md#steps-bar). Its square, for what is placed
/// after to keep clear of it.
pub fn badge(painter: &egui::Painter, visuals: &egui::Visuals, at: egui::Pos2, name: &str) -> Rect {
    let square = Rect::from_center_size(at, vec2(NUMBER, NUMBER));
    let (strong, ground) = (visuals.strong_text_color(), visuals.window_fill);
    number(painter, square, name, (ground, strong, strong), crate::kit::style::semibold(13.0));
    square
}

/// A step's name in its square: its fill, frame and text colours.
fn number(painter: &egui::Painter, square: Rect, name: &str, (fill, line, text): (Color32, Color32, Color32), font: egui::FontId) {
    painter.rect(square, 3.0, fill, Stroke::new(1.5, line), StrokeKind::Inside);
    painter.text(square.center(), Align2::CENTER_CENTER, name, font, text);
}

/// ✗ (Esc) and ✓ (Enter), OCL's red and green squares.
/// ✗ and, with `enter`, ✓.
fn buttons(ui: &mut egui::Ui, enter: bool, actions: &mut Vec<Action>) {
    let width = if enter { 2.0 * BUTTON + BUTTON_GAP } else { BUTTON };
    let (rect, _) = ui.allocate_exact_size(vec2(width, BUTTON), Sense::hover());
    let cancel = Rect::from_min_size(rect.min, vec2(BUTTON, BUTTON));
    let ok = Rect::from_min_size(pos2(rect.right() - BUTTON, rect.top()), vec2(BUTTON, BUTTON));
    let both = [
        (cancel, Level::Error, Key::Escape, "validation:cancel", t!("steps.cancel")),
        (ok, Level::Success, Key::Enter, "validation:ok", t!("steps.validate")),
    ];
    for (r, level, key, name, tip) in both.into_iter().take(if enter { 2 } else { 1 }) {
        let response = ui.interact(r, ui.scope_id().with(name), Sense::click()).tip(tip);
        crate::kit::handles::publish(|| name.into(), r);
        let dark = ui.visuals().dark_mode;
        let (hue, fill, _) = crate::window::toasts::colors(ui.visuals(), level, dark);
        // Hovered, the colours swap (OCL's `:hover` style).
        let (ground, ink) = if response.hovered() { (hue, fill) } else { (fill, hue) };
        let painter = ui.painter();
        painter.rect(r, 4.0, ground, Stroke::new(2.0, hue), StrokeKind::Inside);
        motif(painter, Rect::from_center_size(r.center(), vec2(MOTIF, MOTIF)), key == Key::Enter, ink);
        if response.clicked() {
            actions.push(Action::ToolKey(key));
        }
    }
}

/// OCL's motifs in the unit square: ✓ `M0,0.5 L0.375,1 L1,0`, ✗ `M0,0 L1,1 M0,1 L1,0` (y down).
fn motif(painter: &egui::Painter, r: Rect, check: bool, color: Color32) {
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let stroke = Stroke::new(2.5, color);
    if check {
        painter.add(egui::Shape::line(vec![at(0.0, 0.55), at(0.375, 0.95), at(1.0, 0.05)], stroke));
    } else {
        painter.line_segment([at(0.0, 0.0), at(1.0, 1.0)], stroke);
        painter.line_segment([at(0.0, 1.0), at(1.0, 0.0)], stroke);
    }
}

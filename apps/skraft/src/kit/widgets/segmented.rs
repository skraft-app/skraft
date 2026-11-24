//! The segmented control (docs/ui/interface.md#widgets).

use super::button::{Face, OPTION_HEIGHT, OPTION_ICON, face_tip, faded};
use crate::kit::icons;
use crate::kit::style::Tip;

/// How long a segmented control's mark takes to slide to the value chosen, in seconds.
const SLIDE_TIME: f64 = 0.18;

/// A value of a choice in its segmented control.
pub struct Segment {
    label: String,
    tip: String,
    icon: Option<egui::ImageSource<'static>>,
    selected: bool,
}

impl Segment {
    /// `icon`: an option's icon by its name (`icons::option`).
    pub(crate) fn new(label: &str, tip: &str, icon: Option<&'static str>, selected: bool) -> Self {
        Segment { label: label.to_owned(), tip: tip.to_owned(), icon: icon.and_then(icons::option), selected }
    }

    /// A value shown by `icon`, `label` in its tip.
    pub fn image(label: &str, tip: &str, icon: egui::ImageSource<'static>, selected: bool) -> Self {
        Segment { label: label.to_owned(), tip: tip.to_owned(), icon: Some(icon), selected }
    }
}

/// A choice as a segmented control: its values side by side, drawn like ribbon buttons, the
/// chosen one's mark (a selected ribbon button's fill and outline) sliding to the value chosen.
/// The index of a value clicked other than the chosen one.
/// Each segment is published as the handle `handle(index)` (docs/control-protocol.md).
pub(crate) fn segmented(ui: &mut egui::Ui, id: egui::Id, segments: &[Segment], handle: impl Fn(usize) -> String) -> Option<usize> {
    segmented_sized(ui, id, segments, (OPTION_HEIGHT, OPTION_ICON), handle)
}

/// A [`segmented`] control `size.0` points high, its icons `size.1` wide (an icon's segment a
/// square): the option bar's elsewhere (the library's grid / list toggle).
pub fn segmented_sized(
    ui: &mut egui::Ui,
    id: egui::Id,
    segments: &[Segment],
    (height, icon): (f32, f32),
    handle: impl Fn(usize) -> String,
) -> Option<usize> {
    let faces: Vec<Face> = segments.iter().map(|s| Face::new(ui, &s.label, s.icon.clone())).collect();
    let widths: Vec<f32> = faces.iter().map(|f| f.width(height)).collect();
    let (row, _) = ui.allocate_exact_size(egui::vec2(widths.iter().sum(), height), egui::Sense::hover());
    let radius = ui.visuals().widgets.inactive.corner_radius;
    // The marks faded with their card while it is translucent (docs/ui/interface.md#look).
    let alpha = crate::kit::style::fill_alpha(ui);

    // The segments' places along the row, from its left.
    let lefts: Vec<f32> = widths.iter().scan(0.0, |x, w| Some(std::mem::replace(x, *x + w))).collect();
    let place = |i: usize| egui::Rect::from_min_size(row.min + egui::vec2(lefts[i], 0.0), egui::vec2(widths[i], height));

    let responses: Vec<egui::Response> = segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let response = ui.interact(place(i), id.with(i), egui::Sense::click());
            crate::kit::handles::publish(|| handle(i), place(i));
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, s.selected, &s.label));
            if response.hovered() && !s.selected {
                let visuals = ui.style().interact(&response);
                ui.painter().rect_filled(place(i), radius, visuals.weak_bg_fill.gamma_multiply(alpha));
            }
            response
        })
        .collect();

    if let Some(c) = segments.iter().position(|s| s.selected) {
        let (left, right) = slide(ui, id.with("slide"), (lefts[c], lefts[c] + widths[c]));
        let mark = egui::Rect::from_x_y_ranges(row.left() + left..=row.left() + right, row.y_range());
        let selected = ui.style().interact_selectable(&responses[c], true);
        ui.painter().rect(
            mark,
            radius,
            selected.weak_bg_fill.gamma_multiply(alpha),
            faded(selected.bg_stroke, alpha),
            egui::StrokeKind::Inside,
        );
    }

    let mut clicked = None;
    for (i, (s, response)) in segments.iter().zip(responses).enumerate() {
        let ink = if s.selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
        faces[i].paint(ui, place(i), icon, ink);
        let (text, more) = face_tip(&faces[i], &s.label, &s.tip);
        if response.tip_more(text, more).clicked() && !s.selected {
            clicked = Some(i);
        }
    }
    clicked
}

/// A segmented control's mark on its way to `to` (its left and right, from the row's left: a
/// moving bar does not slide it), easing out over [`SLIDE_TIME`] from wherever it was.
fn slide(ui: &egui::Ui, id: egui::Id, to: (f32, f32)) -> (f32, f32) {
    #[derive(Clone, Copy)]
    struct Slide {
        from: (f32, f32),
        to: (f32, f32),
        start: f64,
    }
    impl Slide {
        fn at(&self, now: f64) -> (f32, f32) {
            let t = ((now - self.start) / SLIDE_TIME).clamp(0.0, 1.0) as f32;
            let e = 1.0 - (1.0 - t).powi(3);
            (egui::lerp(self.from.0..=self.to.0, e), egui::lerp(self.from.1..=self.to.1, e))
        }
    }
    let now = ui.input(|i| i.time);
    let kept = ui.data(|d| d.get_temp::<Slide>(id));
    let s = match kept {
        Some(s) if s.to == to => s,
        // Shown at once the first time, else from where it is now.
        _ => {
            let s = match kept {
                Some(k) => Slide { from: k.at(now), to, start: now },
                None => Slide { from: to, to, start: now - SLIDE_TIME },
            };
            ui.data_mut(|d| d.insert_temp(id, s));
            s
        }
    };
    if now - s.start < SLIDE_TIME {
        ui.ctx().request_repaint();
    }
    s.at(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mark's place at `time` (seconds), heading to `to`.
    fn slide_at(ctx: &egui::Context, time: f64, to: (f32, f32)) -> (f32, f32) {
        let mut at = (f32::NAN, f32::NAN);
        let input = egui::RawInput { time: Some(time), ..Default::default() };
        ctx.run_ui(input, |ui| at = slide(ui, egui::Id::unique("test_slide"), to)).textures_delta.clear();
        at
    }

    #[test]
    fn the_mark_is_shown_at_once_then_slides_to_the_value_chosen() {
        let ctx = egui::Context::default();
        let (a, b) = ((0.0, 26.0), (52.0, 78.0));
        assert_eq!(slide_at(&ctx, 1.0, a), a, "no slide the first time");
        assert_eq!(slide_at(&ctx, 1.1, a), a);
        let start = slide_at(&ctx, 1.1 + SLIDE_TIME / 2.0, b);
        assert_eq!(start, a, "starts where it was");
        let half = slide_at(&ctx, 1.1 + SLIDE_TIME, b);
        assert!(a.0 < half.0 && half.0 < b.0 && half.1 - half.0 == 26.0, "on its way: {half:?}");
        assert_eq!(slide_at(&ctx, 1.1 + 2.0 * SLIDE_TIME, b), b);
    }
}

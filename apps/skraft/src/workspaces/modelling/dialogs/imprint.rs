//! The confirmation of « Empreindre » (docs/decisions/0055-linked-bodies-through-imprints.md): what
//! it will cut and where, with the clearance around the tool and how much deeper it goes. Enter
//! or the button imprints, Esc or Cancel changes nothing.

use skraft_i18n::{t, tp};
use skraft_model::measure;
use skraft_model::{Document, ImprintPlan, Units};

use crate::docs::DocId;
use crate::kit::widgets::{Confirm, FormField, OPTION_HEIGHT, confirm_buttons, form_message, form_row, warning_box};

/// The box's width (points).
const WIDTH: f32 = 470.0;
/// The figure's size (points).
const FIGURE: egui::Vec2 = egui::vec2(116.0, 92.0);
/// The room between the figure and the fields, for the leaders (points).
const LEAD: f32 = 40.0;
/// A leader's shelf, level with its label (points).
const SHELF: f32 = 10.0;

/// A tenon in its mortise, cut through: the part B above, its tenon down in the part A, the
/// clearance on the cheeks and the one at the bottom left empty — the one whose field has the
/// focus (`focus`: around, deep) tinted with the accent. Drawn in the interface's inks. Where the
/// leaders start: on a cheek's clearance, on the bottom's.
fn figure(ui: &mut egui::Ui, focus: (bool, bool)) -> (egui::Pos2, egui::Pos2) {
    use egui::{Rect, Stroke};
    let (rect, _) = ui.allocate_exact_size(FIGURE, egui::Sense::hover());
    let v = ui.visuals();
    let p = ui.painter();
    let at = |x: f32, y: f32| rect.min + egui::vec2(x, y);
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    let ink = v.text_color();
    let ground = v.extreme_bg_color;
    let (a_fill, b_fill) = (ground.lerp_to_gamma(ink, 0.10), ground.lerp_to_gamma(ink, 0.28));
    // The part A, its mortise cut from the top.
    for q in [r(4.0, 36.0, 34.0, 88.0), r(82.0, 36.0, 112.0, 88.0), r(34.0, 74.0, 82.0, 88.0)] {
        p.rect_filled(q, 0.0, a_fill);
    }
    // The clearance being typed: on the cheeks, or at the bottom.
    let lit = v.selection.bg_fill;
    if focus.0 {
        for q in [r(34.0, 36.0, 43.0, 74.0), r(73.0, 36.0, 82.0, 74.0)] {
            p.rect_filled(q, 0.0, lit);
        }
    }
    if focus.1 {
        p.rect_filled(r(43.0, 62.0, 73.0, 74.0), 0.0, lit);
    }
    // The part B, its tenon down in the mortise.
    p.rect_filled(r(16.0, 6.0, 100.0, 36.0), 0.0, b_fill);
    p.rect_filled(r(43.0, 35.0, 73.0, 62.0), 0.0, b_fill);
    let line = Stroke::new(1.0, ink);
    let outline = |points: &[(f32, f32)]| {
        let mut path: Vec<egui::Pos2> = points.iter().map(|(x, y)| at(*x, *y)).collect();
        path.push(path[0]);
        p.line(path, line);
    };
    outline(&[(4.0, 36.0), (34.0, 36.0), (34.0, 74.0), (82.0, 74.0), (82.0, 36.0), (112.0, 36.0), (112.0, 88.0), (4.0, 88.0)]);
    outline(&[(16.0, 6.0), (100.0, 6.0), (100.0, 36.0), (73.0, 36.0), (73.0, 62.0), (43.0, 62.0), (43.0, 36.0), (16.0, 36.0)]);
    // Each part named as its step (the badges on the parts picked).
    let font = egui::TextStyle::Small.resolve(ui.style());
    for (name, (x, y)) in [(t!("steps.b"), (24.0, 21.0)), (t!("steps.a"), (12.0, 80.0))] {
        p.text(at(x, y), egui::Align2::CENTER_CENTER, name, font.clone(), v.weak_text_color());
    }
    (at(77.5, 46.0), at(64.0, 68.0))
}

/// The dialog open, on a plan of a document's editor.
pub struct Dialog {
    pub doc: DocId,
    /// What it will do.
    pub plan: ImprintPlan,
    summary: String,
    into: String,
    /// Copies of a part that would show a machining with nothing in it.
    partial: Option<String>,
    around: String,
    deep: String,
}

/// What the user did in the dialog this frame.
pub enum Answer {
    Open,
    Cancel,
    /// The clearance around the tool and past its end (mm).
    Imprint(f64, f64),
}

fn names(doc: &Document, parts: &[skraft_model::Id]) -> String {
    parts.iter().filter_map(|p| doc.component(*p).ok().map(|c| c.name.clone())).collect::<Vec<_>>().join(", ")
}

impl Dialog {
    pub fn new(id: DocId, doc: &Document, plan: ImprintPlan) -> Dialog {
        let units = &doc.settings().units;
        let mut summary = format!("{} · {}", tp!("imprint.machinings", plan.machining_count()), tp!("imprint.links", plan.link_count()));
        if plan.by_copies() > 0 {
            summary = format!("{summary}, {}", tp!("imprint.by_copies", plan.by_copies()));
        }
        let partial = (!plan.partial.is_empty()).then(|| t!("imprint.partial", parts = names(doc, &plan.partial)));
        Dialog {
            doc: id,
            summary,
            into: t!("imprint.into", parts = names(doc, plan.receivers())),
            partial,
            around: units.format(0.0),
            deep: units.format(0.0),
            plan,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, units: &Units) -> Answer {
        let mut answer = Answer::Open;
        let read = |text: &str| measure::length(text, units, 0.0).ok().filter(|v| v.is_finite());
        let (around, deep) = (read(&self.around), read(&self.deep));
        let values = around.zip(deep);
        let ids = (egui::Id::unique("imprint.around"), egui::Id::unique("imprint.deep"));
        let modal = crate::app::modal(ctx, egui::Id::unique("imprint"), WIDTH, t!("imprint.title"), |ui| {
            // What will be done, then what is set.
            ui.label(egui::RichText::new(&self.summary).strong());
            ui.label(egui::RichText::new(&self.into).color(ui.visuals().weak_text_color()));
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                let focus = ui.memory(|m| (m.has_focus(ids.0), m.has_focus(ids.1)));
                let anchors = figure(ui, focus);
                ui.add_space(LEAD);
                // Where each row's label starts: its leader's end.
                let mut ends = Vec::new();
                ui.vertical(|ui| {
                    let unit = units.length.symbol();
                    for (id, text, label, handle, lit, value) in [
                        (ids.0, &mut self.around, t!("imprint.around"), "imprint.around", focus.0, around),
                        (ids.1, &mut self.deep, t!("imprint.deep"), "imprint.deep", focus.1, deep),
                    ] {
                        let at = ui.cursor().min;
                        ends.push(egui::pos2(at.x - 4.0, at.y + OPTION_HEIGHT / 2.0));
                        form_row(ui, id, text, FormField { label: &label, unit, lit, bad: value.is_none(), handle });
                        ui.add_space(4.0);
                    }
                    form_message(ui, values.is_none().then(|| t!("imprint.bad_value")).as_deref());
                });
                let v = ui.visuals();
                for ((anchor, end), lit) in [anchors.0, anchors.1].into_iter().zip(ends).zip([focus.0, focus.1]) {
                    let stroke =
                        if lit { egui::Stroke::new(1.5, v.selection.stroke.color) } else { egui::Stroke::new(1.0, v.weak_text_color()) };
                    crate::kit::overlay::leader(ui.painter(), anchor, end, SHELF, stroke);
                }
            });
            if let Some(partial) = &self.partial {
                ui.add_space(10.0);
                warning_box(ui, partial);
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| match confirm_buttons(ui, "imprint", t!("imprint.ok"), values.is_some(), t!("naming.cancel")) {
                Some(Confirm::Ok) => {
                    if let Some((around, deep)) = values {
                        answer = Answer::Imprint(around, deep);
                    }
                }
                Some(Confirm::Cancel) => answer = Answer::Cancel,
                None => {}
            });
        });
        if matches!(answer, Answer::Open)
            && let Some((around, deep)) = values
            && ctx.input(|i| i.key_pressed(egui::Key::Enter))
        {
            answer = Answer::Imprint(around, deep);
        }
        if matches!(answer, Answer::Open) && modal.should_close() {
            answer = Answer::Cancel;
        }
        answer
    }
}

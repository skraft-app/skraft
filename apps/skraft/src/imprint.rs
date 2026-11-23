//! The confirmation of « Imprimer » (docs/decisions/0055-linked-bodies-through-imprints.md): what
//! it will cut and where, with the clearance around the tool and how much deeper it goes. Enter
//! or the button imprints, Esc or Cancel changes nothing.

use skraft_i18n::{t, tp};
use skraft_model::measure;
use skraft_model::{Document, ImprintPlan, Units};

use crate::docs::DocId;

/// The depth a mortise takes past its tenon, by default (mm).
const DEEP: f64 = 2.0;

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
        let summary = format!("{} · {}", tp!("imprint.machinings", plan.machining_count()), tp!("imprint.links", plan.link_count()));
        let partial = (!plan.partial.is_empty()).then(|| t!("imprint.partial", parts = names(doc, &plan.partial)));
        Dialog {
            doc: id,
            summary,
            into: t!("imprint.into", parts = names(doc, plan.receivers())),
            partial,
            around: units.format(0.0),
            deep: units.format(DEEP),
            plan,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, units: &Units) -> Answer {
        let mut answer = Answer::Open;
        let read = |text: &str, current: f64| measure::length(text, units, current).ok().filter(|v| v.is_finite());
        let values = read(&self.around, 0.0).zip(read(&self.deep, DEEP));
        let modal = egui::Modal::new(egui::Id::unique("imprint")).frame(crate::app::modal_frame(ctx)).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.spacing_mut().button_padding = crate::app::MODAL_BUTTON_PADDING;
            ui.heading(t!("imprint.title"));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(&self.summary).strong());
            ui.label(&self.into);
            ui.add_space(8.0);
            egui::Grid::new("imprint.fields").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                for (label, text, handle) in
                    [(t!("imprint.around"), &mut self.around, "imprint.around"), (t!("imprint.deep"), &mut self.deep, "imprint.deep")]
                {
                    ui.label(label);
                    let field = ui.add(egui::TextEdit::singleline(text).desired_width(110.0).margin(egui::vec2(6.0, 5.0)));
                    crate::handles::publish(|| handle.into(), field.rect);
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            // The line saying what is wrong, kept even when empty: the dialog does not jump.
            let bad = if values.is_none() { t!("imprint.bad_value") } else { String::new() };
            ui.label(egui::RichText::new(bad).small().color(ui.visuals().error_fg_color));
            if let Some(partial) = &self.partial {
                ui.horizontal_top(|ui| {
                    ui.add(crate::panels::warning(ui));
                    ui.add(egui::Label::new(partial).wrap());
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let fill = ui.visuals().selection.bg_fill;
                    let ok = ui.add_enabled(values.is_some(), egui::Button::new(t!("imprint.ok")).fill(fill));
                    crate::handles::publish_enabled(|| "imprint.ok".into(), ok.rect, values.is_some());
                    if ok.clicked()
                        && let Some((around, deep)) = values
                    {
                        answer = Answer::Imprint(around, deep);
                    }
                    let cancel = ui.button(t!("naming.cancel"));
                    crate::handles::publish(|| "imprint.cancel".into(), cancel.rect);
                    if cancel.clicked() {
                        answer = Answer::Cancel;
                    }
                });
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

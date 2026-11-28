//! The export box (docs/io/step-export.md, docs/decisions/0059-exporters-step-exact-skp-meshed.md):
//! the format (STEP; SKP to come), what goes out — the whole model or the selection —, and
//! how many parts that makes; « Exporter » then asks where to save the file.

use skraft_i18n::{t, tp};

use crate::docs::DocId;
use crate::kit::widgets::{Confirm, Segment, confirm_buttons, form_choice};

/// The box open on a document, the parts each content would write counted when it opened.
pub struct Dialog {
    pub doc: DocId,
    /// The selection rather than the whole model.
    pub selection: bool,
    /// The parts the whole model writes; none: no visible solid.
    model: Option<usize>,
    /// The parts the selection writes; none: nothing selected, or no solid in it.
    selected: Option<usize>,
}

/// What the user did in the box this frame.
pub enum Answer {
    Open,
    Cancel,
    /// Export, the selection or the whole model.
    Export {
        selection: bool,
    },
}

impl Dialog {
    /// The box for `doc`, the parts of each content counted: the selection chosen when it holds
    /// something.
    pub fn new(doc: DocId, model: Option<usize>, selected: Option<usize>) -> Dialog {
        Dialog { doc, selection: selected.is_some(), model, selected }
    }

    fn parts(&self) -> Option<usize> {
        if self.selection { self.selected } else { self.model }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Answer {
        let mut answer = Answer::Open;
        let parts = self.parts();
        let modal = crate::app::modal(ctx, egui::Id::unique("export"), 380.0, t!("export.title"), |ui| {
            let formats = [
                Segment::new(&t!("export.step"), &t!("export.step_tip"), None, true),
                Segment::new(&t!("export.skp"), &t!("export.skp_tip"), None, false).enabled(false),
            ];
            form_choice(ui, egui::Id::unique("export.format"), &t!("export.format"), &formats, |i| format!("export.format:{i}"));
            ui.add_space(4.0);
            let selection_tip = if self.selected.is_some() { t!("export.selection_tip") } else { t!("export.selection_none") };
            let contents = [
                Segment::new(&t!("export.model"), &t!("export.model_tip"), None, !self.selection).enabled(self.model.is_some()),
                Segment::new(&t!("export.selection"), &selection_tip, None, self.selection).enabled(self.selected.is_some()),
            ];
            if let Some(i) =
                form_choice(ui, egui::Id::unique("export.content"), &t!("export.content"), &contents, |i| format!("export.content:{i}"))
            {
                self.selection = i == 1;
            }
            ui.add_space(6.0);
            let summary = match self.parts() {
                Some(count) => tp!("export.parts", count),
                None => t!("export.nothing"),
            };
            // One line high: a right-to-left layout alone would take all the height left.
            let line = egui::vec2(ui.available_width(), ui.text_style_height(&egui::TextStyle::Small));
            ui.allocate_ui_with_layout(line, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(crate::kit::style::caption(summary))
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| match confirm_buttons(ui, "export", t!("export.export"), parts.is_some(), t!("export.cancel")) {
                Some(Confirm::Ok) => answer = Answer::Export { selection: self.selection },
                Some(Confirm::Cancel) => answer = Answer::Cancel,
                None => {}
            });
        });
        if matches!(answer, Answer::Open) && parts.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            answer = Answer::Export { selection: self.selection };
        }
        if matches!(answer, Answer::Open) && modal.should_close() {
            answer = Answer::Cancel;
        }
        answer
    }
}

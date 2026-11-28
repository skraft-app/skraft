//! The export box (docs/io/export.md, docs/decisions/0059-exporters-step-exact-skp-meshed.md):
//! the format — STEP, or SKP with its release and how fine curved faces are cut —, what goes
//! out — the whole model or the selection —, and how many parts that makes; « Exporter » then
//! asks where to save the file. The choices hold for the session ([`Choice`]).

use skraft_i18n::{t, tp};
use skraft_model::ExportFormat;
use skraft_model::kernel::{Facets, SkpRelease};

use crate::docs::DocId;
use crate::kit::widgets::{Confirm, Segment, confirm_buttons, form_choice};

/// The format and its options last chosen: the next box opens on them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub skp: bool,
    pub release: SkpRelease,
    pub facets: Facets,
}

impl Choice {
    pub fn format(self) -> ExportFormat {
        if self.skp { ExportFormat::Skp { release: self.release, facets: self.facets } } else { ExportFormat::Step }
    }
}

/// The box open on a document, the parts each content would write counted when it opened.
pub struct Dialog {
    pub doc: DocId,
    pub choice: Choice,
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
    /// Export, the selection or the whole model, as the choice says.
    Export {
        selection: bool,
        choice: Choice,
    },
}

impl Dialog {
    /// The box for `doc` on the last `choice`, the parts of each content counted: the selection
    /// chosen when it holds something.
    pub fn new(doc: DocId, choice: Choice, model: Option<usize>, selected: Option<usize>) -> Dialog {
        Dialog { doc, choice, selection: selected.is_some(), model, selected }
    }

    fn parts(&self) -> Option<usize> {
        if self.selection { self.selected } else { self.model }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Answer {
        let mut answer = Answer::Open;
        let parts = self.parts();
        let export = |d: &Dialog| Answer::Export { selection: d.selection, choice: d.choice };
        let modal = crate::app::modal(ctx, egui::Id::unique("export"), 380.0, t!("export.title"), |ui| {
            let c = &mut self.choice;
            let formats = [
                Segment::new(&t!("export.step"), &t!("export.step_tip"), None, !c.skp),
                Segment::new(&t!("export.skp"), &t!("export.skp_tip"), None, c.skp),
            ];
            if let Some(i) =
                form_choice(ui, egui::Id::unique("export.format"), &t!("export.format"), &formats, |i| format!("export.format:{i}"))
            {
                c.skp = i == 1;
            }
            if c.skp {
                ui.add_space(4.0);
                let releases = [
                    Segment::new(&t!("export.r2017"), &t!("export.r2017_tip"), None, c.release == SkpRelease::R2017),
                    Segment::new(&t!("export.r2026"), &t!("export.r2026_tip"), None, c.release == SkpRelease::R2026),
                ];
                if let Some(i) =
                    form_choice(ui, egui::Id::unique("export.release"), &t!("export.release"), &releases, |i| format!("export.release:{i}"))
                {
                    c.release = [SkpRelease::R2017, SkpRelease::R2026][i];
                }
                ui.add_space(4.0);
                const FACETS: [Facets; 3] = [Facets::Coarse, Facets::Normal, Facets::Fine];
                let labels = [t!("export.coarse"), t!("export.normal"), t!("export.fine")];
                let facets: Vec<Segment> =
                    FACETS.iter().zip(&labels).map(|(f, l)| Segment::new(l, &t!("export.facets_tip"), None, c.facets == *f)).collect();
                if let Some(i) =
                    form_choice(ui, egui::Id::unique("export.facets"), &t!("export.facets"), &facets, |i| format!("export.facets:{i}"))
                {
                    c.facets = FACETS[i];
                }
            }
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
                Some(Confirm::Ok) => answer = export(self),
                Some(Confirm::Cancel) => answer = Answer::Cancel,
                None => {}
            });
        });
        if matches!(answer, Answer::Open) && parts.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            answer = export(self);
        }
        if matches!(answer, Answer::Open) && modal.should_close() {
            answer = Answer::Cancel;
        }
        answer
    }
}

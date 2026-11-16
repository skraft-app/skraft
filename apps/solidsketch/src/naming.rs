//! The dialog naming a part just made (docs/ARCHITECTURE.md §6 quattuordecies), OpenCutList's
//! `UI.inputbox` after a Smart Draw creation: opened by itself (the creation tool's option
//! « Nommage des pièces », [`solidsketch_tools::Editor::take_naming`]) or by the « Renommer »
//! button of the toast « Nouvelle pièce créée ». One field, the part's name, filled with it and
//! selected; Enter or OK names it, Esc or Cancel keeps it. A name the editor refuses (empty,
//! another component's) greys OK and says why under the field.

use solidsketch_i18n::t;
use solidsketch_tools::Naming;

use crate::docs::DocId;

/// The dialog open: on a part of a document.
pub struct Dialog {
    pub doc: DocId,
    pub naming: Naming,
    text: String,
    /// The field's text is selected as a whole, once it has the focus (asked till then).
    selected: bool,
}

/// What the user did in the dialog this frame.
pub enum Answer {
    /// Still typing.
    Open,
    Cancel,
    /// This name, accepted by the editor's check.
    Name(String),
}

impl Dialog {
    pub fn new(doc: DocId, naming: Naming) -> Dialog {
        let text = naming.name.clone();
        Dialog { doc, naming, text, selected: false }
    }

    /// The field's text (the control channel's `app.state`).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Draws the dialog; `refusal` tells why a name cannot be given (the editor's check).
    pub fn show(&mut self, ctx: &egui::Context, refusal: impl Fn(&str) -> Option<String>) -> Answer {
        let mut answer = Answer::Open;
        let why = refusal(&self.text);
        let modal = egui::Modal::new(egui::Id::unique("naming")).frame(crate::app::modal_frame(ctx)).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.spacing_mut().button_padding = crate::app::MODAL_BUTTON_PADDING;
            ui.heading(t!("naming.title"));
            ui.add_space(8.0);
            let id = egui::Id::unique("naming.field");
            // Its own margin: not the modal's roomy button padding, which it would take.
            let edit = egui::TextEdit::singleline(&mut self.text).id(id).desired_width(f32::INFINITY).margin(egui::vec2(6.0, 5.0));
            let field = ui.add(edit);
            crate::handles::publish(|| "naming.field".into(), field.rect);
            if !self.selected {
                // The whole name selected, typing replaces it (OCL's inputbox): once the field
                // has the focus (egui places the cursor when it takes it). The memory's focus,
                // not the response's, false while the window is not the frontmost one.
                if ui.memory(|m| m.has_focus(id)) {
                    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
                    let all =
                        egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(self.text.chars().count()));
                    state.cursor.set_char_range(Some(all));
                    state.store(ctx, id);
                    self.selected = true;
                } else {
                    field.request_focus();
                }
                ctx.request_repaint();
            }
            // The line saying why, kept even when empty: the dialog does not jump.
            let line = why.clone().unwrap_or_default();
            ui.add_space(2.0);
            ui.label(egui::RichText::new(line).small().color(ui.visuals().error_fg_color));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let fill = ui.visuals().selection.bg_fill;
                    let ok = ui.add_enabled(why.is_none(), egui::Button::new(t!("naming.ok")).fill(fill));
                    crate::handles::publish_enabled(|| "naming.ok".into(), ok.rect, why.is_none());
                    if ok.clicked() {
                        answer = Answer::Name(self.text.clone());
                    }
                    let cancel = ui.button(t!("naming.cancel"));
                    crate::handles::publish(|| "naming.cancel".into(), cancel.rect);
                    if cancel.clicked() {
                        answer = Answer::Cancel;
                    }
                });
            });
        });
        if matches!(answer, Answer::Open) && why.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            answer = Answer::Name(self.text.clone());
        }
        if matches!(answer, Answer::Open) && modal.should_close() {
            answer = Answer::Cancel;
        }
        answer
    }
}

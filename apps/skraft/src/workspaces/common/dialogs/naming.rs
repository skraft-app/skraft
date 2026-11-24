//! The dialog naming what was just made (docs/tools/quick-tools.md), OpenCutList's
//! `UI.inputbox` after a Smart Draw creation: opened by itself (the creation tool's option
//! « Nommage des pièces », [`skraft_tools::Editor::take_naming`]) or by the « Renommer »
//! button of a toast « Nouvelle pièce créée », « Nouvel assemblage créé »… One field, the name,
//! filled with it and selected; Enter or OK names it, Esc or Cancel keeps it. A name the editor
//! refuses (empty, another component's) greys OK and says why under the field.

use skraft_i18n::t;
use skraft_tools::Naming;

use crate::docs::DocId;
use crate::kit::widgets::{Confirm, FieldLook, confirm_buttons, live_field};

/// The dialog open: on what a document's editor names.
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
        let modal = crate::app::modal(ctx, egui::Id::unique("naming"), 340.0, self.naming.title.clone(), |ui| {
            let id = egui::Id::unique("naming.field");
            let look = FieldLook {
                hint: None,
                tip: None,
                width: ui.available_width(),
                align: egui::Align2::LEFT_CENTER,
                lit: false,
                bad: why.is_some(),
                addon: None,
                handle: Some("naming.field".into()),
            };
            let field = live_field(ui, id, &mut self.text, look);
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
            ui.add_space(2.0);
            crate::kit::widgets::form_message(ui, why.as_deref());
            ui.add_space(8.0);
            ui.horizontal(|ui| match confirm_buttons(ui, "naming", t!("naming.ok"), why.is_none(), t!("naming.cancel")) {
                Some(Confirm::Ok) => answer = Answer::Name(self.text.clone()),
                Some(Confirm::Cancel) => answer = Answer::Cancel,
                None => {}
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

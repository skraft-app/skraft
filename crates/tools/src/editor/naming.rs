//! Naming a part just made (docs/tools/quick-tools.md), after OpenCutList's Smart
//! Draw `fn_ask_name`: once a creation tool has made a part, the app's naming dialog opens at
//! once (the tool's option « Nommage des pièces ») or a success toast offers a « Renommer »
//! button that opens it. The dialog is the app's; the editor says when, checks the name and
//! names the part — within the creation's step when the dialog opened by itself.

use skraft_i18n::t;
use skraft_model::{Id, Operation, Outcome, Prepared};

use super::journal::Event;
use super::{Editor, Notice, NoticeAction, NoticeKind};

/// A part to name, for the app's dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Naming {
    pub component: Id,
    /// Its name now, the field's text to start with.
    pub name: String,
    /// The history's revision right after its creation, when the dialog opened by itself: named
    /// while it is still the last state, the name joins the creation's step.
    revision: Option<u64>,
}

impl Editor {
    /// After a creation tool's operation: the part it made (`AddPrimitive`, one occurrence of
    /// a new part in the context, or in the assembly `made_in` leads to, docs/tools/draw-options.md) asked
    /// its name, or offered to be named by a toast.
    pub(super) fn offer_naming(&mut self, outcome: &Outcome, made_in: Option<&[Id]>, ask: bool) {
        let doc = self.document();
        let Some(component) = doc
            .assembly(doc.context_of(made_in.unwrap_or(&self.context)).unwrap_or(doc.root()))
            .ok()
            .and_then(|a| outcome.created.first().and_then(|id| a.occurrence(*id)))
            .map(|o| o.component)
        else {
            return;
        };
        let Ok(name) = doc.component(component).map(|c| c.name.clone()) else { return };
        if ask {
            self.naming = Some(Naming { component, name, revision: Some(self.history.revision()) });
        } else {
            let title = t!("notice.part_created", name = name);
            self.push_notice(Notice { kind: NoticeKind::Success, title, detail: None, action: Some(NoticeAction::Rename(component)) });
        }
    }

    /// The naming dialog to open now, if a part was just made with « Nommage des pièces » on;
    /// taken once.
    pub fn take_naming(&mut self) -> Option<Naming> {
        self.naming.take()
    }

    /// The part `component` to name (a toast's « Renommer »), if it still exists: its name, as
    /// a step of its own.
    pub fn naming_of(&self, component: Id) -> Option<Naming> {
        let c = self.document().component(component).ok()?;
        Some(Naming { component, name: c.name.clone(), revision: None })
    }

    /// Why `name` cannot be given to the part of `naming`: empty, or another component's.
    /// `None` when it can (its own name too: nothing changes).
    pub fn name_refusal(&self, naming: &Naming, name: &str) -> Option<String> {
        let name = name.trim();
        if name.is_empty() {
            return Some(t!("naming.empty"));
        }
        let taken = self.document().components().any(|c| c.id != naming.component && c.name == name);
        taken.then(|| t!("error.model.name_taken"))
    }

    /// Names the part of `naming` (trimmed); its own name again changes nothing. Within the
    /// creation's step when the dialog opened by itself and nothing happened since, else a
    /// step of its own. On failure (the part gone, the name refused, the kernel busy), why.
    pub fn name_part(&mut self, naming: &Naming, name: &str) -> Result<(), String> {
        if let Some(why) = self.name_refusal(naming, name) {
            return Err(why);
        }
        let name = name.trim().to_owned();
        let c = self.document().component(naming.component).map_err(|e| e.to_string())?;
        if c.name == name {
            return Ok(());
        }
        let op = Operation::RenameComponent { component: naming.component, name };
        let joins = naming.revision == Some(self.history.revision()) && self.pending.is_none();
        if joins && let Ok(Prepared::Done(applied)) = self.document().prepare(&op) {
            let before = self.history.revision();
            if self.history.amend(applied.document) {
                self.journal.log(Event::Applied { op: op.label(), revision: self.history.revision() });
                // The quick tools' card belongs to the creation's state: still it.
                if let Some(card) = &mut self.card {
                    card.follow(before, self.history.revision());
                }
                self.after_change();
                return Ok(());
            }
        }
        self.start(op, super::After::Nothing, false).map(|_| ())
    }
}

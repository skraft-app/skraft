//! Naming what was just made (docs/tools/quick-tools.md#naming-what-was-just-made), after
//! OpenCutList's Smart Draw `fn_ask_name`: once a creation tool has made a part, the app's
//! naming dialog opens at once (the tool's option « Nommage des pièces ») or a success toast
//! offers a « Renommer » button that opens it; an assembly or a group of bodies made, a part,
//! an assembly or a body made unique, the toast only. The dialog is the app's; the editor says
//! when, checks the name and names the component or the body definition — within the creation's
//! step when the dialog opened by itself.

use skraft_i18n::t;
use skraft_model::{Document, Id, Operation, Outcome, Prepared};

use super::journal::Event;
use super::{Editor, Notice, NoticeAction, NoticeKind};

/// What a naming names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameTarget {
    /// A part or an assembly (`RenameComponent`); its name unique in the document.
    Component(Id),
    /// A body definition of the part `part` (`RenameBodyDef`), every body of it showing the
    /// name; names need not be unique (docs/model/bodies.md#body-names).
    BodyDef { part: Id, def: Id },
}

/// Something to name, for the app's dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Naming {
    pub target: NameTarget,
    /// Its name now, the field's text to start with.
    pub name: String,
    /// The dialog's title: « Nom de la pièce », « Nom de l'assemblage », « Nom du groupe »…
    pub title: String,
    /// The history's revision right after its creation, when the dialog opened by itself: named
    /// while it is still the last state, the name joins the creation's step.
    revision: Option<u64>,
}

/// What a naming names, as the dialog's title and the toasts say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nature {
    Part,
    Assembly,
    Group,
    Body,
}

impl Nature {
    fn title(self) -> String {
        match self {
            Nature::Part => t!("naming.title_part"),
            Nature::Assembly => t!("naming.title_assembly"),
            Nature::Group => t!("naming.title_group"),
            Nature::Body => t!("naming.title_body"),
        }
    }

    /// The toast of one made unique.
    fn made_unique(self, name: &str) -> String {
        match self {
            Nature::Part => t!("notice.part_made_unique", name = name),
            Nature::Assembly => t!("notice.assembly_made_unique", name = name),
            Nature::Group => t!("notice.group_made_unique", name = name),
            Nature::Body => t!("notice.body_made_unique", name = name),
        }
    }
}

/// What `target` is called in `doc`, and what it is: none when it is no more.
fn named(doc: &Document, target: NameTarget) -> Option<(String, Nature)> {
    match target {
        NameTarget::Component(id) => {
            let c = doc.component(id).ok()?;
            let nature = if doc.assembly(id).is_ok() { Nature::Assembly } else { Nature::Part };
            Some((c.name.clone(), nature))
        }
        NameTarget::BodyDef { part, def } => {
            let d = doc.part(part).ok()?.def(def)?;
            Some((d.name.clone(), if d.is_group() { Nature::Group } else { Nature::Body }))
        }
    }
}

/// What `op` (`before` → `after`, its `outcome`) made that the user may name: the assembly
/// `MakeAssembly` made, the group `GroupBodies` made, the components `MakeUnique` and the body
/// definitions `MakeBodiesUnique` gave their own (none when already unique).
pub(super) fn made_to_name(before: &Document, after: &Document, op: &Operation, outcome: &Outcome) -> Vec<NameTarget> {
    let component_of = |doc: &Document, context: Id, occurrence: Id| {
        doc.assembly(context).ok().and_then(|a| a.occurrence(occurrence)).map(|o| o.component)
    };
    let def_of = |doc: &Document, part: Id, body: Id| doc.part(part).ok().and_then(|p| p.any_body(body)).map(|b| b.def);
    match op {
        // « Créer un assemblage » under « Pièces »: in the parts' parent assembly.
        Operation::At { parent, op, .. } if matches!(**op, Operation::MakeAssembly { .. }) => {
            let context = after.context_of(parent).ok();
            outcome
                .created
                .first()
                .zip(context)
                .and_then(|(o, c)| component_of(after, c, *o))
                .map(NameTarget::Component)
                .into_iter()
                .collect()
        }
        Operation::MakeAssembly { context, .. } => {
            outcome.created.first().and_then(|o| component_of(after, *context, *o)).map(NameTarget::Component).into_iter().collect()
        }
        Operation::GroupBodies { target, .. } => {
            let Ok((part, _, _)) = after.resolve_part(target) else { return Vec::new() };
            outcome.bodies.first().and_then(|b| def_of(after, part, *b)).map(|def| NameTarget::BodyDef { part, def }).into_iter().collect()
        }
        Operation::MakeUnique { context, occurrence } => {
            let now = component_of(after, *context, *occurrence);
            now.filter(|_| now != component_of(before, *context, *occurrence)).map(NameTarget::Component).into_iter().collect()
        }
        Operation::MakeBodiesUnique { target, bodies, .. } => {
            let Ok((part, _, _)) = after.resolve_part(target) else { return Vec::new() };
            bodies
                .iter()
                .filter_map(|b| {
                    let now = def_of(after, part, *b)?;
                    (Some(now) != def_of(before, part, *b)).then_some(NameTarget::BodyDef { part, def: now })
                })
                .collect()
        }
        _ => Vec::new(),
    }
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
        let target = NameTarget::Component(component);
        let Some(naming) = self.naming_of(target) else { return };
        if ask {
            self.naming = Some(Naming { revision: Some(self.history.revision()), ..naming });
        } else {
            let title = t!("notice.part_created", name = naming.name);
            self.push_notice(Notice { kind: NoticeKind::Success, title, detail: None, action: Some(NoticeAction::Rename(target)) });
        }
    }

    /// After « Créer un assemblage » or « Grouper les corps »: the assembly or the group made
    /// offered to be named by a toast.
    pub(super) fn offer_renaming(&mut self, made: &[NameTarget]) {
        let Some(&target) = made.first() else { return };
        let Some((name, nature)) = named(self.document(), target) else { return };
        let title = match nature {
            Nature::Assembly => t!("notice.assembly_created", name = name),
            _ => t!("notice.group_created", name = name),
        };
        self.push_notice(Notice { kind: NoticeKind::Success, title, detail: None, action: Some(NoticeAction::Rename(target)) });
    }

    /// Once « Rendre unique » is done (all its operations): the one made unique offered to be
    /// named by a toast; several, a toast that says how many.
    pub(super) fn offer_unique_naming(&mut self) {
        let made = std::mem::take(&mut self.made_unique);
        let live: Vec<(NameTarget, String, Nature)> =
            made.into_iter().filter_map(|t| named(self.document(), t).map(|(n, k)| (t, n, k))).collect();
        match &live[..] {
            [] => {}
            [(target, name, nature)] => {
                let title = nature.made_unique(name);
                self.push_notice(Notice { kind: NoticeKind::Success, title, detail: None, action: Some(NoticeAction::Rename(*target)) });
            }
            many => self.notify(NoticeKind::Success, t!("notice.many_made_unique", count = many.len()), None),
        }
    }

    /// The naming dialog to open now, if a part was just made with « Nommage des pièces » on;
    /// taken once.
    pub fn take_naming(&mut self) -> Option<Naming> {
        self.naming.take()
    }

    /// `target` to name (a toast's « Renommer »), if it still exists: its name, as a step of
    /// its own.
    pub fn naming_of(&self, target: NameTarget) -> Option<Naming> {
        let (name, nature) = named(self.document(), target)?;
        Some(Naming { target, name, title: nature.title(), revision: None })
    }

    /// Why `name` cannot be given to what `naming` names: empty, or (a component's) another
    /// component's. `None` when it can (its own name too: nothing changes).
    pub fn name_refusal(&self, naming: &Naming, name: &str) -> Option<String> {
        let name = name.trim();
        if name.is_empty() {
            return Some(t!("naming.empty"));
        }
        match naming.target {
            NameTarget::Component(id) => {
                let taken = self.document().components().any(|c| c.id != id && c.name == name);
                taken.then(|| t!("error.model.name_taken"))
            }
            NameTarget::BodyDef { .. } => None,
        }
    }

    /// Gives `name` (trimmed) to what `naming` names; its own name again changes nothing. Within
    /// the creation's step when the dialog opened by itself and nothing happened since, else a
    /// step of its own. On failure (it is gone, the name refused, the kernel busy), why.
    pub fn give_name(&mut self, naming: &Naming, name: &str) -> Result<(), String> {
        if let Some(why) = self.name_refusal(naming, name) {
            return Err(why);
        }
        let name = name.trim().to_owned();
        let Some((now, _)) = named(self.document(), naming.target) else { return Err(t!("naming.gone")) };
        if now == name {
            return Ok(());
        }
        let op = match naming.target {
            NameTarget::Component(component) => Operation::RenameComponent { component, name },
            NameTarget::BodyDef { part, def } => Operation::RenameBodyDef { part, def, name },
        };
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

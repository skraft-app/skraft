//! The quick tools after a creation (docs/ARCHITECTURE.md §6 terdecies): a tool started from
//! the card hands back to the creation tool once its action is done; the tool that handed back
//! is kept aside, and what is typed before the next shape starts goes to it (OCL's
//! `callback_action_handler` and `previous_action_handler`). The card itself is shown by the
//! app where [`Editor::quick_card`] says.

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_model::{Id, Operation, Outcome};

use super::Editor;
use crate::tools::{self, Command, Key, Tool, ToolInput, ToolKind, handle};
use crate::vcb;

/// The tool a quick tool hands back to, and what to put back if it is given up.
pub(super) struct Return {
    to: ToolKind,
    /// The card it was started from, offered again if nothing was done.
    card: Option<Card>,
    /// The selection before it started.
    selection: Vec<Id>,
}

/// What a creation tool just made, the quick tools' card offered on it.
pub(super) struct Card {
    /// The occurrences made in the context, or the bodies in the open part.
    made: Vec<Id>,
    /// The centre of their box (world): where the card is drawn.
    anchor: DVec3,
    /// The history's revision and the context it was made at: any other step, an undo or
    /// another context, and the card is gone.
    revision: u64,
    context: Vec<Id>,
    open_part: Option<Id>,
}

impl Card {
    /// The state it belongs to changed in place (`before` → `now`, a name joining the
    /// creation's step): still its own.
    pub(super) fn follow(&mut self, before: u64, now: u64) {
        if self.revision == before {
            self.revision = now;
        }
    }
}

impl Editor {
    /// Where the quick tools' card is drawn now (pixels, its centre), if it is shown: after a
    /// creation, while the creation tool waits at its first step.
    pub fn quick_card(&self) -> Option<DVec2> {
        let c = self.card.as_ref().filter(|c| self.card_valid(c))?;
        if self.busy() || self.tool.busy() || self.carry.is_some() {
            return None;
        }
        self.camera.project(c.anchor, self.size)
    }

    fn card_valid(&self, c: &Card) -> bool {
        c.revision == self.history.revision() && c.context == self.context && c.open_part == self.open_part
    }

    /// Lets the card go.
    pub fn dismiss_quick_card(&mut self) {
        self.card = None;
    }

    /// After a creation tool's operation: the card offered on what it made, unless what it made
    /// cannot be selected (bodies of an open part whose machinings are not shown).
    pub(super) fn offer_card(&mut self, outcome: &Outcome) {
        self.card = None;
        let made = if self.open_part.is_some() { &outcome.bodies } else { &outcome.created };
        if made.is_empty() || (self.open_part.is_some() && !self.showing_machinings()) {
            return;
        }
        let Some(anchor) = self.made_centre(made) else { return };
        self.card = Some(Card {
            made: made.clone(),
            anchor,
            revision: self.history.revision(),
            context: self.context.clone(),
            open_part: self.open_part,
        });
    }

    /// The centre (world) of the box of `made`: occurrences of the context, or bodies of the
    /// open part.
    fn made_centre(&self, made: &[Id]) -> Option<DVec3> {
        let cx = self.cx();
        let doc = self.document();
        let (lo, hi, to_world) = match self.open_part_path() {
            Some(path) => {
                let (_, part, t) = doc.resolve_container(&path).ok()?;
                let bodies: Vec<_> = made.iter().filter_map(|id| part.body(*id)).collect();
                let (lo, hi) = handle::bodies_bounds(&cx, &part, &bodies, DAffine3::IDENTITY)?;
                (lo, hi, t)
            }
            None => {
                let assembly = doc.assembly(doc.context_of(&self.context).ok()?).ok()?;
                let occs: Vec<_> = assembly.occurrences().iter().filter(|o| made.contains(&o.id)).collect();
                let (lo, hi) = handle::occurrences_bounds(&cx, &self.context, &occs, DAffine3::IDENTITY)?;
                (lo, hi, doc.transform_of(&self.context).ok()?)
            }
        };
        Some(to_world.transform_point3((lo + hi) * 0.5))
    }

    /// A key that lets the card go, as OCL's (Alt / Command, Esc).
    pub(super) fn card_key(key: Key) -> bool {
        matches!(key, Key::Alt | Key::Escape)
    }

    /// Starts `kind` on what the card was offered on (else on the selection), as the quick
    /// tools' card does: once its action is done (its first operation applied), or given up
    /// (Esc back at its first step), the tool `self` runs now comes back. Given up, the card
    /// and the selection are put back as they were; done, the card is not offered again.
    pub fn start_quick_tool(&mut self, kind: ToolKind) {
        let to = self.tool.kind();
        let card = self.card.take().filter(|c| self.card_valid(c));
        let selection = self.selection.clone();
        // Selected first: the tool starts on it (Erase erases it, §6 septendecies).
        if let Some(c) = &card {
            self.set_selection(c.made.clone());
        }
        self.set_tool(kind);
        self.quick_return = Some(Return { to, card, selection });
    }

    /// Is the active tool one the quick tools started (it will hand back)?
    pub fn quick_tool_running(&self) -> bool {
        self.quick_return.is_some()
    }

    /// Is a tool kept aside, taking what is typed before the next shape starts?
    pub fn quick_tool_aside(&self) -> Option<ToolKind> {
        self.aside.as_ref().map(|t| t.kind())
    }

    /// Back to the creation tool; the tool that ran kept aside when it did something.
    pub(super) fn hand_back(&mut self, done: bool) {
        let Some(r) = self.quick_return.take() else { return };
        let ran = std::mem::replace(&mut self.tool, tools::make(r.to));
        self.aside = done.then_some(ran);
        if !done {
            self.card = r.card;
            self.set_selection(r.selection);
        }
        self.clear_locks();
        self.reference = None;
    }

    /// Does `commands` do the action a quick tool is started for (an operation applied, not a
    /// setting kept on a component)?
    pub(super) fn acts(commands: &[Command]) -> bool {
        commands.iter().any(|c| match c {
            Command::Apply(op) => !matches!(op, Operation::SetBehavior { .. }),
            Command::Replace { .. } => true,
            _ => false,
        })
    }

    /// The tool aside, if what is typed goes to it now: the active tool at its first step.
    fn aside_taking(&self) -> Option<&dyn Tool> {
        self.aside.as_deref().filter(|_| !self.tool.busy())
    }

    /// `text` typed for the tool aside, if it takes it now: read for what it takes, then given
    /// to it (its copies redone). `false`: the active tool reads it.
    pub(super) fn type_aside(&mut self, text: &str) -> bool {
        let Some(spec) = self.aside_taking().and_then(|t| t.vcb()) else { return false };
        let value = match vcb::read(&spec, text, &self.document().settings().units, &self.axes_transform()) {
            Ok(v) => v,
            Err(message) => {
                self.message = Some(message);
                return true;
            }
        };
        let Some(mut aside) = self.aside.take() else { return false };
        let cx = self.cx_for(&*aside);
        let commands = aside.input(&ToolInput::Value(value), &cx);
        self.aside = Some(aside);
        for c in commands {
            self.run(c);
        }
        true
    }

    /// The measurements box of the tool aside, while it takes what is typed.
    pub(super) fn aside_vcb(&self) -> Option<vcb::Vcb> {
        self.aside_taking().and_then(|t| t.vcb())
    }
}

//! The tool stack (docs/tools/interaction.md#tool-stack, SketchUp's `push_tool` / `pop_tool`):
//! a tool started over the active one suspends it, as it is — its step, its hover, its
//! options —, and hands back to it when it is done. Two ways onto it:
//! - a tool **held** by the other modifier (« Prélever » over « Peindre »): it hands back when
//!   the key is released, or with Esc;
//! - a **quick tool** started from the card after a creation (docs/tools/quick-tools.md): it
//!   hands back once its action is done — the tool that ran kept aside, taking what is typed
//!   until the next shape — or given up (Esc at its first step), the card and the selection
//!   put back as they were.
//!
//! Choosing a tool (the ribbon, a key, a command) empties it.

use skraft_model::Id;

use super::{Editor, journal, quick};
use crate::tools::{self, Tool, ToolKind};

/// A tool waiting under the active one, as it was.
pub(super) struct Suspended {
    tool: Box<dyn Tool>,
    over: Over,
}

/// What the tool over a suspended one is there for: when it hands back, and what that puts back.
pub(super) enum Over {
    /// Held by the other modifier: back when the key is released.
    Held,
    /// A quick tool started from the card: the card it was started from and the selection before
    /// it, put back if it is given up.
    Quick { card: Option<quick::Card>, selection: Vec<Id> },
}

impl Editor {
    /// Starts `kind` over the active tool, which waits under it as it is (`over`: when it
    /// comes back).
    pub(super) fn push_tool(&mut self, kind: ToolKind, over: Over) {
        self.note(journal::Event::Tool(kind), Some(journal::Step::Tool(kind)));
        // A paste carried is given up, as by any other tool.
        self.settle_carry(true);
        let mut tool = tools::make(kind);
        // Its saved options, read at once (as `switch_tool`).
        for option in tool.options(&self.cx_for(&*tool)) {
            tool.set_option(option.key, &option.value);
        }
        let mut under = std::mem::replace(&mut self.tool, tool);
        under.leave();
        self.stack.push(Suspended { tool: under, over });
        self.message = None;
        self.clear_locks();
        self.reference = None;
    }

    /// The tool waiting under the active one back, as it was. `done`: the tool over it did its
    /// action (a quick tool is then kept aside); else it was given up.
    pub(super) fn pop_tool(&mut self, done: bool) {
        let Some(s) = self.stack.pop() else { return };
        let ran = std::mem::replace(&mut self.tool, s.tool);
        match s.over {
            Over::Held => {}
            Over::Quick { card, selection } => {
                if done {
                    self.aside = Some(ran);
                } else {
                    self.card = card;
                    self.set_selection(selection);
                }
            }
        }
        self.message = None;
        self.clear_locks();
        self.reference = None;
    }

    /// Is the active tool held over another by the other modifier?
    pub(super) fn tool_held(&self) -> bool {
        matches!(self.stack.last(), Some(Suspended { over: Over::Held, .. }))
    }

    /// Is the active tool one the quick tools started (it will hand back)?
    pub(super) fn quick_on_top(&self) -> bool {
        matches!(self.stack.last(), Some(Suspended { over: Over::Quick { .. }, .. }))
    }

    /// The tools waiting under the active one, the nearest last (the ribbon marks them).
    pub fn suspended_tools(&self) -> Vec<ToolKind> {
        self.stack.iter().map(|s| s.tool.kind()).collect()
    }
}

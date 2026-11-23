//! What the copy tools (docs/tools/copy-array.md) share around their handle: what is picked and
//! aimed at, the copies last made and the value typed that makes them again, the mirror's key.
//! Each tool keeps where its copies go and how it shows them.

use skraft_i18n::t;
use skraft_model::Operation;

use super::handle::{MIRROR, Memo, Target};
use super::pick::Picker;
use super::{Command, Cursor, Cx, Key, OptionValue, ToolInput};
use crate::infer::Inference;
use crate::vcb::Repeat;

/// A copy tool's picking and its last copies, `L` what it keeps of them.
#[derive(Clone)]
pub(super) struct Copying<L> {
    pub picker: Picker,
    memo: Memo,
    pub snap: Option<Inference>,
    /// The copies just made: a value typed then makes them again.
    pub last: Option<L>,
    /// There was something to copy at the last input (for the hints).
    pub aimed: bool,
}

impl<L> Default for Copying<L> {
    fn default() -> Self {
        Copying { picker: Picker::default(), memo: Memo::default(), snap: None, last: None, aimed: false }
    }
}

impl<L> Copying<L> {
    /// What is copied, once it is taken.
    pub fn target(&self, cx: &Cx<'_>) -> Option<Target> {
        if !self.picker.aims() {
            return None;
        }
        self.memo.target(cx)
    }

    /// Shift released: the selection made is taken, the last copies let go.
    pub fn shift_released(&mut self, cx: &Cx<'_>) {
        if self.picker.shift_released(cx) {
            self.last = None;
        }
    }

    /// The target at this input.
    pub fn aim(&mut self, cx: &Cx<'_>) -> Option<Target> {
        let target = self.target(cx);
        self.aimed = target.is_some();
        target
    }

    /// An input while nothing is aimed at: it picks.
    pub fn pick(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        let (out, took) = self.picker.input(input, cx);
        if took {
            self.last = None;
        }
        out
    }

    /// The copies made (OCL lets them go: the next click picks something else).
    pub fn made(&mut self, last: L, op: Operation) -> Vec<Command> {
        self.last = Some(last);
        self.picker.drop();
        self.snap = None;
        vec![Command::Apply(op), Command::Select(Vec::new())]
    }

    /// Everything let go: nothing aimed at any more, no longer busy (a quick tool then hands
    /// back).
    pub fn forget(&mut self) {
        self.picker.drop();
        self.snap = None;
        self.last = None;
        self.aimed = false;
    }

    pub fn leave(&mut self) {
        self.snap = None;
        self.picker.leave();
    }

    pub fn busy(&self) -> bool {
        self.aimed || self.picker.busy() || self.last.is_some()
    }

    /// The cursor: `pick`'s while picking, then `placing`.
    pub fn cursor(&self, cx: &Cx<'_>, pick: Cursor, placing: Cursor) -> Option<Cursor> {
        Some(if self.aimed { placing } else { self.picker.cursor(cx, pick) })
    }
}

/// The methods of [`Tool`](super::Tool) a copy tool answers from its `base`, a [`Copying`].
macro_rules! copying_tool {
    () => {
        fn busy(&self) -> bool {
            self.base.busy()
        }

        fn picks_first(&self) -> bool {
            true
        }

        /// Nothing aimed at: a click picks what to copy (also once copies are made).
        fn picking(&self) -> bool {
            !self.base.aimed
        }

        fn picks_parts(&self) -> bool {
            true
        }

        fn dropped(&self) -> bool {
            self.base.picker.dropped()
        }
    };
}
pub(super) use copying_tool;

/// The copies a count asks for.
pub(super) fn count(r: Repeat) -> u32 {
    match r {
        Repeat::Times(n) | Repeat::Divide(n) => n,
    }
}

/// The copies made again by `op`, which replaces `last` in the history.
pub(super) fn redone(last: &mut Operation, op: Operation) -> Vec<Command> {
    let previous = std::mem::replace(last, op.clone());
    vec![Command::Replace { previous: Box::new(previous), op: Box::new(op) }, Command::Select(Vec::new())]
}

/// The copy modifier tapped toggles the mirror (OCL's Alt / Command), which warns when it would
/// make `several` copies.
pub(super) fn mirror_key(input: &ToolInput, cx: &Cx<'_>, several: bool) -> Option<Vec<Command>> {
    let ToolInput::Key { key: Key::Copy, .. } = input else { return None };
    let on = !cx.toggle(MIRROR);
    let mut out = vec![Command::SetOption { key: MIRROR, value: OptionValue::Toggle(on) }];
    if on && several {
        out.push(Command::Say(t!("tool.handle.say.mirror_one")));
    }
    Some(out)
}

/// OCL's warning: the mirror makes one copy only.
pub(super) fn mirror_warning(cx: &Cx<'_>, several: bool) -> Option<Command> {
    (cx.toggle(MIRROR) && several).then(|| Command::Say(t!("tool.handle.say.mirror_one")))
}

//! Undo and redo by shared snapshots (docs/ARCHITECTURE.md §5), with the journal of the
//! operations applied.

use crate::document::Document;
use crate::ops::Operation;
#[cfg(any(test, feature = "engine"))]
use crate::{Result, ops::Outcome};

/// The current document, the documents before it and those undone after it.
///
/// Every document state carries a revision, unique within the history: undo and redo bring
/// back the revision of the state they restore, any applied operation (merged or not) makes a
/// new one. Comparing revisions tells whether the document is the one saved.
#[derive(Clone, Debug, Default)]
pub struct History {
    current: Document,
    revision: u64,
    /// The last revision handed out.
    last_revision: u64,
    /// Documents before the current one, each with its revision and the operation that left it.
    past: Vec<(Document, u64, Operation)>,
    /// Undone steps, most recently undone last.
    future: Vec<(Document, u64, Operation)>,
}

impl History {
    pub fn new(doc: Document) -> Self {
        History { current: doc, revision: 0, last_revision: 0, past: Vec::new(), future: Vec::new() }
    }

    pub fn document(&self) -> &Document {
        &self.current
    }

    /// The current document's revision (0 for the one the history started with).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Applies `op`, running the kernel here (see [`History::push`]).
    #[cfg(any(test, feature = "engine"))]
    pub fn apply(&mut self, op: Operation) -> Result<Outcome> {
        let (next, outcome) = self.current.apply(&op)?;
        self.push(next, op);
        Ok(outcome)
    }

    /// Makes `next`, the current document with `op` applied, the last step; the redo steps are
    /// dropped. An operation that merges into the last step (`Operation::merges_into`) replaces
    /// it.
    pub fn push(&mut self, next: Document, op: Operation) {
        let prev = std::mem::replace(&mut self.current, next);
        self.last_revision += 1;
        let prev_revision = std::mem::replace(&mut self.revision, self.last_revision);
        match self.past.last_mut() {
            Some(last) if self.future.is_empty() && op.merges_into(&last.2) => last.2 = op,
            _ => self.past.push((prev, prev_revision, op)),
        }
        self.future.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// The operation an undo would revert.
    pub fn undo_op(&self) -> Option<&Operation> {
        self.past.last().map(|(_, _, op)| op)
    }

    /// The operation a redo would apply again.
    pub fn redo_op(&self) -> Option<&Operation> {
        self.future.last().map(|(_, _, op)| op)
    }

    pub fn undo(&mut self) -> bool {
        let Some((prev, revision, op)) = self.past.pop() else { return false };
        let undone = std::mem::replace(&mut self.current, prev);
        let undone_revision = std::mem::replace(&mut self.revision, revision);
        self.future.push((undone, undone_revision, op));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((next, revision, op)) = self.future.pop() else { return false };
        let prev = std::mem::replace(&mut self.current, next);
        let prev_revision = std::mem::replace(&mut self.revision, revision);
        self.past.push((prev, prev_revision, op));
        true
    }

    /// Is the state of `revision` still in this history (before, current or undone)? Ids only
    /// grow from a state to the next: while it is here, an id it had still names the same
    /// thing (§6 decies).
    pub fn holds(&self, revision: u64) -> bool {
        self.revision == revision || self.past.iter().chain(&self.future).any(|(_, r, _)| *r == revision)
    }

    /// The operations that led to the current document, oldest first.
    pub fn journal(&self) -> impl Iterator<Item = &Operation> {
        self.past.iter().map(|(_, _, op)| op)
    }
}

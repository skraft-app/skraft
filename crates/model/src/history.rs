//! Undo and redo by shared snapshots (docs/model/undo.md), with the journal of the
//! operations applied, and what the states kept weigh (docs/model/undo.md#memory-cap).

use std::cmp::Ordering;
use std::mem::size_of;
use std::sync::Arc;

use crate::FastSet;
use crate::document::{Component, ComponentKind, Document, Occurrence};
use crate::kernel::Solid;
use crate::ops::Operation;
#[cfg(any(test, feature = "engine"))]
use crate::{Result, ops::Outcome};

/// What a document's own map of components spends on one entry, roughly (each state has its
/// own map; the components themselves are shared).
const MAP_ENTRY: u64 = 32;

/// The bytes two neighbouring states hold that the other does not: `older`'s, `newer`'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Weight {
    older: u64,
    newer: u64,
}

/// A state kept besides the current one: the document, its revision, the operation between it
/// and its neighbour (the one that left it, for a step before; that led to it, for one undone)
/// and the weight of the pair it makes with its neighbour towards the current state (none:
/// not weighed yet, [`History::weigh`]).
#[derive(Clone, Debug)]
struct Step {
    doc: Document,
    revision: u64,
    op: Operation,
    weight: Option<Weight>,
}

/// The current document, the documents before it and those undone after it.
///
/// Every document state carries a revision, unique within the history: undo and redo bring
/// back the revision of the state they restore, any applied operation (merged or not) makes a
/// new one. Comparing revisions tells whether the document is the one saved.
///
/// The states kept cost memory (their changed B-reps, and the meshes the scene keeps for them):
/// [`History::held_bytes`] tells how much, [`History::forget`] drops the farthest state.
#[derive(Clone, Debug, Default)]
pub struct History {
    current: Document,
    revision: u64,
    /// The last revision handed out.
    last_revision: u64,
    /// Documents before the current one, the oldest first; each weighed with the next.
    past: Vec<Step>,
    /// Undone steps, most recently undone last; each weighed with the one before it.
    future: Vec<Step>,
    /// States forgotten so far.
    forgotten: u64,
}

impl History {
    pub fn new(doc: Document) -> Self {
        History { current: doc, ..Default::default() }
    }

    pub fn document(&self) -> &Document {
        &self.current
    }

    /// The current document's revision (0 for the one the history started with).
    /// The document before the last step, if any.
    pub fn previous(&self) -> Option<&Document> {
        self.past.last().map(|s| &s.doc)
    }

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
            Some(last) if self.future.is_empty() && op.merges_into(&last.op) => {
                last.op = op;
                last.weight = None;
            }
            _ => self.past.push(Step { doc: prev, revision: prev_revision, op, weight: None }),
        }
        self.future.clear();
    }

    /// Makes `next` the current document within the last step, which keeps its operation (its
    /// undo label): a change that rides on it, as a part's name asked as soon as it is made
    /// (docs/tools/quick-tools.md). A new revision. False, nothing done, without a step to join or with
    /// steps undone.
    pub fn amend(&mut self, next: Document) -> bool {
        let Some(last) = self.past.last_mut().filter(|_| self.future.is_empty()) else { return false };
        last.weight = None;
        self.current = next;
        self.last_revision += 1;
        self.revision = self.last_revision;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// The operation an undo would revert.
    pub fn undo_op(&self) -> Option<&Operation> {
        self.past.last().map(|s| &s.op)
    }

    /// The operation a redo would apply again.
    pub fn redo_op(&self) -> Option<&Operation> {
        self.future.last().map(|s| &s.op)
    }

    /// The pair the step popped made with the current state is the one the state leaving
    /// makes with it: its weight goes along.
    pub fn undo(&mut self) -> bool {
        let Some(Step { doc, revision, op, weight }) = self.past.pop() else { return false };
        let undone = std::mem::replace(&mut self.current, doc);
        let undone_revision = std::mem::replace(&mut self.revision, revision);
        self.future.push(Step { doc: undone, revision: undone_revision, op, weight });
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(Step { doc, revision, op, weight }) = self.future.pop() else { return false };
        let prev = std::mem::replace(&mut self.current, doc);
        let prev_revision = std::mem::replace(&mut self.revision, revision);
        self.past.push(Step { doc: prev, revision: prev_revision, op, weight });
        true
    }

    /// Is the state of `revision` still in this history (before, current or undone)? Ids only
    /// grow from a state to the next: while it is here, an id it had still names the same
    /// thing (docs/tools/copy-paste.md).
    pub fn holds(&self, revision: u64) -> bool {
        self.revision == revision || self.past.iter().chain(&self.future).any(|s| s.revision == revision)
    }

    /// The operations that led to the current document, oldest first (the ones forgotten
    /// aside).
    pub fn journal(&self) -> impl Iterator<Item = &Operation> {
        self.past.iter().map(|s| &s.op)
    }

    /// Weighs the pairs of states not weighed yet (after an operation, an amend): `solid`
    /// tells what a solid holds — its record, and its meshes where they are kept. Once
    /// weighed, a pair keeps its weight.
    pub fn weigh(&mut self, solid: &dyn Fn(&Arc<Solid>) -> u64) {
        for k in 0..self.past.len() {
            if self.past[k].weight.is_none() {
                let newer = self.past.get(k + 1).map_or(&self.current, |s| &s.doc);
                let (older, newer) = exclusive_bytes(&self.past[k].doc, newer, solid);
                self.past[k].weight = Some(Weight { older, newer });
            }
        }
        for k in 0..self.future.len() {
            if self.future[k].weight.is_none() {
                let older = self.future.get(k + 1).map_or(&self.current, |s| &s.doc);
                let (older, newer) = exclusive_bytes(older, &self.future[k].doc, solid);
                self.future[k].weight = Some(Weight { older, newer });
            }
        }
    }

    /// What the states kept besides the current one hold that it does not, as last weighed
    /// ([`History::weigh`]; a pair not weighed counts for nothing).
    pub fn held_bytes(&self) -> u64 {
        let past = self.past.iter().filter_map(|s| s.weight).map(|w| w.older);
        let future = self.future.iter().filter_map(|s| s.weight).map(|w| w.newer);
        past.chain(future).sum()
    }

    /// Can a state be forgotten ([`History::forget`])?
    pub fn can_forget(&self) -> bool {
        self.past.len() > 1 || !self.future.is_empty()
    }

    /// Forgets the state farthest from the current one — the oldest step before it or the
    /// last one to redo, whichever is more steps away (the one before, on a tie). The last
    /// step to undo always stays: an operation abandoned, a paste carried, an array replacing
    /// a copy undo it. What it frees, as weighed (none: nothing forgotten).
    pub fn forget(&mut self) -> Option<u64> {
        let freed = if self.past.len() > 1 && self.past.len() >= self.future.len() {
            self.past.remove(0).weight.map_or(0, |w| w.older)
        } else if !self.future.is_empty() {
            self.future.remove(0).weight.map_or(0, |w| w.newer)
        } else {
            return None;
        };
        self.forgotten += 1;
        Some(freed)
    }

    /// The states forgotten so far.
    pub fn forgotten(&self) -> u64 {
        self.forgotten
    }

    /// [`History::held_bytes`] with every pair weighed again.
    #[cfg(test)]
    pub(crate) fn reweighed(&self, solid: &dyn Fn(&Arc<Solid>) -> u64) -> u64 {
        let mut h = self.clone();
        h.past.iter_mut().chain(&mut h.future).for_each(|s| s.weight = None);
        h.weigh(solid);
        h.held_bytes()
    }
}

/// The bytes `a` holds that `b` does not, and the other way round: their own maps of
/// components, and the components that differ — whole, but for the solids both versions of a
/// component share (by `solid`). A solid one keeps in a changed component and the other in an
/// unchanged one (a part made unique, a part pasted back) is counted though shared: an
/// estimate on the high side. Linear in the components; solids are weighed only in the ones
/// that changed.
pub(crate) fn exclusive_bytes(a: &Document, b: &Document, solid: &dyn Fn(&Arc<Solid>) -> u64) -> (u64, u64) {
    let own = |d: &Document| d.components.len() as u64 * MAP_ENTRY + d.materials.len() as u64 * MAP_ENTRY;
    let (mut in_a, mut in_b) = (own(a), own(b));
    let (mut x, mut y) = (a.components.iter().peekable(), b.components.iter().peekable());
    loop {
        let order = match (x.peek(), y.peek()) {
            (None, None) => break,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some((i, _)), Some((j, _))) => i.cmp(j),
        };
        match order {
            Ordering::Less => in_a += component_bytes(x.next().unwrap().1, None, solid),
            Ordering::Greater => in_b += component_bytes(y.next().unwrap().1, None, solid),
            Ordering::Equal => {
                let (ca, cb) = (x.next().unwrap().1, y.next().unwrap().1);
                if !Arc::ptr_eq(ca, cb) {
                    in_a += component_bytes(ca, Some(cb), solid);
                    in_b += component_bytes(cb, Some(ca), solid);
                }
            }
        }
    }
    (in_a, in_b)
}

/// The solids a component holds: a part's shape and its bodies' definitions.
fn solids_of(c: &Component) -> impl Iterator<Item = &Arc<Solid>> {
    let part = c.as_part();
    part.and_then(|p| p.shape.as_ref()).into_iter().chain(part.into_iter().flat_map(|p| p.defs.iter().map(|d| &d.solid)))
}

/// What `c` holds, but for the solids `other` (another version of it) holds too.
fn component_bytes(c: &Component, other: Option<&Component>, solid: &dyn Fn(&Arc<Solid>) -> u64) -> u64 {
    let shared: FastSet<*const Solid> = other.map_or_else(FastSet::default, |o| solids_of(o).map(Arc::as_ptr).collect());
    let solids = solids_of(c).filter(|s| !shared.contains(&Arc::as_ptr(s))).map(solid).sum::<u64>();
    let own = match &c.kind {
        ComponentKind::Part(p) => {
            let names = p.defs.iter().map(|d| d.name.len()).chain(p.bodies.iter().map(|b| b.name.len())).sum::<usize>();
            std::mem::size_of_val(p.defs.as_slice())
                + std::mem::size_of_val(p.bodies.as_slice())
                + names
                + (p.faces.len() + p.failed.len()) * MAP_ENTRY as usize
        }
        // Its occurrences and their index by id, once built; their names (mostly none) left
        // out: the root's tens of thousands would be walked at each move.
        ComponentKind::Assembly(a) => a.occurrences().len() * (size_of::<Occurrence>() + 16),
    };
    (size_of::<Component>() + c.name.len() + std::mem::size_of_val(c.guides.as_slice()) + own) as u64 + solids
}

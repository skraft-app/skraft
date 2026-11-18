---
status: built
code: [crates/model/src/history.rs, crates/kernel/src/weight.rs, crates/tools/src/scene.rs, crates/tools/src/editor/document.rs, apps/skraft/src/docs.rs]
---

# Undo / redo: shared snapshots

Undo and redo keep whole document states, which share everything that did not change, so no
operation needs an inverse. What the states kept weigh is accounted exactly and capped by a
memory budget. For anyone touching `History` or adding something the document holds.

## Shared snapshots

- The document is **immutable piece by piece** (`Arc` on definitions and B-reps). Applying an
  operation produces a new `Document` that shares everything that did not change.
- Undo = going back to the previous document; redo = going forward. No inverse to write.
- The operation applied is kept with each state (for debugging, and for parametric modelling
  if it comes).

## Memory cap

A history kept whole grows without end: each changed B-rep stays, and the scene keeps the mesh
of every solid the history still holds ([Asynchronous kernel](kernel.md#asynchronous-kernel)),
so a few heavy booleans or an import pin hundreds of MB. The cap is on **memory**, not on a
number of steps. **Why:** a rename weighs nothing, a STEP import hundreds of MB.

Decision record: [The undo history is capped by memory](../decisions/0033-history-memory-cap.md).

- **One budget for every document open** (preference « Mémoire de l'historique », in MiB:
  1 GiB on the desktop, 512 MiB in the browser). What counts is what the states kept hold
  *besides the current documents*: the current state is never weighed.
- **Exact and incremental accounting.** A version of a component (its `Arc`) lives in a
  contiguous run of states. So, for each pair of neighbouring states, `History` keeps the bytes
  the older holds that the newer does not, and the other way round: what the states kept hold
  is the sum of the first number over the steps before the current one and of the second over
  the steps undone; forgetting the oldest state frees exactly its first number, the last one to
  redo its second. A pair is weighed once (`History::weigh`), after the operation, the merge or
  the amend that made it: a parallel walk of the two maps of components comparing `Arc`s, the
  components that differ weighed whole but for the solids both versions share. Undo and redo
  carry the weights along.
- **The weigher.** `model` does not know meshes: the editor weighs a solid as its record
  (`Solid::approx_bytes`, a walk of the record) plus the mesh and snaps the scene keeps for it
  (`DisplayScene::solid_bytes`). A solid whose mesh had not come when its pair was weighed (a
  file still meshing) counts for its record alone. Estimates: struct sizes and vector lengths,
  not the allocator's overhead; a solid shared between a changed component and an unchanged one
  (a part made unique) counts, on the high side.
- **Forgetting.** Every frame, after the kernel's results are in, the app adds up the
  histories (`Docs::cap_histories`); beyond the budget it forgets, document after document, the
  ones shown least recently first, the state farthest from the current one (the oldest step, or
  the last one to redo, whichever is more steps away). Each history keeps its last step to undo
  (an operation abandoned, a paste carried, an array replacing a copy undo it), and one computing
  an operation is left alone (the array's copy may come back by a redo). The meshes of the
  solids forgotten leave the scene at once (`DisplayScene::prune`); a revision forgotten is no
  longer « held » (`History::holds`): a paste from it grafts its parts anew.
- **Seen in the journal**: an entry « Historique » (`Event::Forgotten`: steps, bytes freed),
  nothing else on screen; undo simply stops earlier.

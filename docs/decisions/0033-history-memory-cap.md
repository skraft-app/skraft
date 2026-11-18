# The undo history is capped by memory

- Status: accepted (2026-11-18)
- Design: [Undo / redo](../model/undo.md#memory-cap)

## Context

Decided on 2026-11-18 (questions / answers). A history kept whole grows without end: each
changed B-rep stays, and the scene keeps the mesh of every solid the history still holds, so
a few heavy booleans or an import pin hundreds of MB.

## Decision

- The cap is on **memory**: one budget for every document open (preference « Mémoire de
  l'historique », 1 GiB on the desktop, 512 MiB in the browser), counting what the states kept
  hold besides the current documents.
- Exact and incremental accounting: for each pair of neighbouring states, the bytes each holds
  that the other does not, weighed once after the operation that made the pair.
- Beyond the budget, the app forgets, the documents shown least recently first, the state
  farthest from the current one; each history keeps its last step to undo.

## Alternatives considered

- **A cap on a number of steps**: a rename weighs nothing, a STEP import hundreds of MB.

## Consequences

- `model` does not know meshes: the editor weighs a solid as its record plus the mesh and
  snaps the scene keeps for it.
- The meshes of the solids forgotten leave the scene at once; a revision forgotten is no
  longer « held »: a paste from it grafts its parts anew.
- Seen only in the journal (« Historique »); undo simply stops earlier.

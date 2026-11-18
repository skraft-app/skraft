# Selection scope « Parts »: a second selection, as paths

- Status: accepted (2026-11-17)
- Design: [Select first, and a selection scope](../tools/selection.md)

## Context

The selection takes the children of the active context: to act on a part inside an assembly,
the user enters it. OpenCutList picks the part under the cursor at any depth, and goes up its
path with its tree drop-up. The operations take one `context` and ids in it.

## Decision

Decided in principle on 2026-11-17 with select first, built after it; settled the same day
(questions / answers, when it started):

- a shared setting « Context » / « Parts », a choice option in the bar of Select and of every
  tool's picking phase, saved once, Tab toggling it;
- under « Parts », the leaf part only; going up by OCL's tree drop-up (Option held over a
  part);
- **the selection becomes paths** (`EntityPath` from the root) under « Parts »: the context's
  selection stays ids of the active context (read in many places), « Parts » keeps its own
  list of paths (`Editor::deep`), the two exclusive;
- operations are grouped per parent assembly (`Operation::At`), several in one undo step
  (`Operation::Batch`), a world transform brought into each parent's axes;
- making unique is the « Portée » option, shown only when a selected path goes through an
  assembly placed more than once; « only this one » makes the assemblies along it unique when
  the tool acts, in the same undo step (OCL's `_make_unique_groups_in_path`);
- the highlight goes per path (view and navigator); the rectangle takes parts at any depth;
- the navigator's click is left as it is (it selects in its context, entering it);
- two steps: A1 — the setting, the selection as paths, the highlight, Select, Move, Rotate,
  Delete / Erase, the batch operation, making unique; A2 — the copy tools, Distribute, the
  Stretch, Paint, the clipboard, OCL's Option drop-up. Both built on 2026-11-17.

## Alternatives considered

- Turning the whole selection into paths: not done — the ids of the active context are read in
  many places; « Context » is the case where every path starts with the active context.

## Consequences

- Option has two meanings by state: the drop-up while picking under « Parts », copy or mirror
  once picking is over (as in OCL).
- The Stretch takes parts at any depth of one assembly only.
- Inside an open part, the selection stays the bodies' (no scope).

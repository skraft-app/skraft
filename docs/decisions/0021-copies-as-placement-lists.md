# Copies as a list of placements computed by the tool

- Status: accepted (2026-11-15)
- Design: [Copy along a line, copy along a grid, distribute](../tools/copy-array.md)

## Context

Decided on 2026-11-15 (questions / answers), when porting OpenCutList's Smart Handle tool (Copy
along a line, Copy along a grid, Distribute). The model already had `Array` / `ArrayBodies` (one
step applied k times) and the copies made by Move and Rotate.

## Decision

- Three tools, three commands, sharing one module (`tools/handle.rs`); OCL has them as actions of
  one tool, here a command is a tool.
- The model gets a list of placements, one per copy, computed by the tool — the model only puts
  them down — and a move of the sources: `Operation::Copies { context, entities, moved,
  placements }` and its twin for bodies, `Operation::CopyBodies { target, bodies, moved,
  placements, scope }`. One operation, one undo step.
- `Array`, `ArrayBodies` and Move / Rotate are left as they are.
- Built in four steps on 2026-11-15: the model; the common module and Copy along a line;
  Distribute; Copy along a grid.

## Alternatives considered

- Building on `Array` (one step applied k times): it can express neither a grid (two steps),
  OCL's mirrored copies, nor a moved source (Distribute moves the source to the first slot).

## Consequences

- Placements are rigid, mirror allowed, at most `MAX_ARRAY`.
- Typing a count or a distance again after the click redoes the copies by replacing the
  operation (`Command::Replace`), as OCL's `STATE_HANDLE_COPIES`.
- OCL's twins (picking the other occurrences of the part, same parent, to replace them —
  re-spacing an existing row) are not part of it, to discuss again after use; nor is OCL's Move
  along line.

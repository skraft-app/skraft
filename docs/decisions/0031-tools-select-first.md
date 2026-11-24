# Tools pick first, as OpenCutList's

- Status: accepted (2026-11-17)
- Design: [Select first, and a selection scope](../tools/selection.md)

## Context

The tools acting on entities each took them their own way: Move and Rotate's first click took
the entity under the cursor and started (SketchUp's way, `Cx::entity_under`), the copy tools
had their own `Picker` (`handle.rs`), the Stretch its own picking. A selection scope taking
parts at any depth was to come next, and needed one picking phase to plug into.

## Decision

Decided on 2026-11-17 (questions / answers), after OpenCutList's `SmartSelectActionHandler`
(`tool/smart_tool.rb`); the first of two pieces of work, the selection scope « Parts » after
it ([Selection scope « Parts » as paths](0029-selection-scope-parts-as-paths.md)).

- **A definitive switch, not a preference**: one behaviour, OCL's. A tool acting on entities
  takes the selection when it starts; else it starts with a picking phase (click, Shift
  clicks, rectangle), and only then does its action.
- **One picking module** (`tools/pick.rs`): the copy tools' `Picker` moved out, the Stretch's
  picking brought onto it, shaped so that the scope plugs in (the scope it picks with, a state
  for the tree drop-up).
- Move and Rotate pick first, then take the base point or centre; once done the selection is
  let go. *Amended on 2026-11-24: once moved or turned, the selection is kept and taken again
  (Esc lets it go); only a copy lets it go ([Select first](../tools/selection.md#select-first)).*
  Erase and Paint keep the click that acts; booleans ignore the selection; the shape
  tools, Measure, Section and Profile are not concerned.

## Alternatives considered

- SketchUp's « the first click takes the entity and starts » for Move and Rotate (one click
  fewer).
- A preference between both behaviours: rejected (« a definitive switch, not a preference »).

## Consequences

- One click more than SketchUp in Move and Rotate.
- `Cx::entity_under` is gone; Esc at a tool's first step lets the selection go and picks again.
- The selection scope « Parts » plugs into the one `Picker`.

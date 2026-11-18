# Cutters and locks as a typed Behavior on the component

- Status: accepted (2026-11-12)
- Design: [Stretch](../tools/stretch.md)

## Context

The stretch's cutters were the tool's state. They describe how a component is cut when
stretched, and should follow its definition, as OCL keeps them on it (its `stretch_cutters`
attribute); SketchUp has `Sketchup::Behavior` and its `no_scale_mask` for the axes a component
never scales along.

## Decision

- A typed `Behavior` on the `Component` (a part or an assembly) holds the cutters (ratios of
  its box along each own axis) and the axes it never stretches along (`StretchMask`, SketchUp's
  `no_scale_mask` bit layout, saved as an integer).
- The mask is the one truth: a locked axis has no cutters. Removing the last cutter of an axis
  locks it — it was « one in the middle again » before.
- Cutters are saved by any gesture on them through `Operation::SetBehavior`, pure model;
  consecutive ones on one component merge into one undo step.

## Alternatives considered

- **A generic attribute dictionary** (weighed the same day): rejected — a typed field is
  validated when read, and the rule of each field on copy is written where it means something.
  A dictionary may come later only to carry foreign attributes through a SketchUp import /
  export.
- **One undo step per gesture on the cutters**: too many steps.
- **Cutter changes outside the history**: undoing the previous operation would bring back a
  snapshot without them, silently.

## Consequences

- Copies (`MakeUnique`, `separate`, the walk's copies) keep the behavior; the stretch's target
  takes the tool's cutters after it, in the stretch's own operation.
- A component locked along the stretch axis is a block in a walk: moved with the section
  holding its origin, never stretched (after OCL, which does it for the full mask).
- The file gains a `behavior` field on the component, absent when default.

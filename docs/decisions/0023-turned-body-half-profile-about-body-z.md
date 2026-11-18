# A turned body is a half profile turned a whole turn about the body's Z

- Status: accepted (2026-11-15)
- Design: [Turned bodies (revolution)](../tools/turned-bodies.md)

## Context

Discussed on 2026-11-15 (Q&A), branch `feature/profile-edit`. Turned legs, dowels, knobs and
rings were wanted. SolidCore turns a planar profile of lines and arcs
(`solidcore_ops::sweep::revolve`); the kernel already read surfaces of revolution
(`Surface::PlacedRevolution`, STEP import). Bodies already had a profile (an extrusion) and a
placement in their part ([Bodies](../model/bodies.md)).

## Decision

- **The profile**: a body's profile is either the extrusion or a revolution — a half profile in
  the body's XZ plane (`x`, the radius, never negative), turned a whole turn about the body's Z.
  The axis is the body's (its placement says where it is): no axis of its own stored, no partial
  angle for now. Same corners as an extrusion's, arcs and fillets alike.
- **Created by a tool of its own**, « Tourner »: the axis (two points), then the half profile
  drawn as the Prism's outline, on one side of the axis, in the plane through the axis and the
  profile's first point. **Closed by hand**: no automatic closing along the axis. For the user a
  separate tool, in the code the Prism tool in a turning mode.
- **Edited by the Profile tool**, with two more rules: a corner stopped on the axis, a side lying
  on the axis kept on it.
- **Stretch**: along the axis, rebuilt from the profile (exact); across the axis refused, the
  body's X and Y locked, the diameter changed by the Profile tool only.
- **Cut list**: nothing particular, the box of the final B-rep.

Details settled with the user the same day: only the axis's origin marked and no length shown
(only its way matters); the axis and its origin inferred by the profile's points; the plane
decided by the first click (over nothing, the horizontal plane through the axis's start), left
free while every corner lies on the axis; the half profile edited where it lies (an extrusion's
outline likewise on its bottom cap), not turned to face the eye as first built.

## Alternatives considered

- Automatic closing along the axis: rejected, the profile is closed by hand.
- A stored axis of its own, or a partial angle: not now (a quarter round can come later as an
  angle).

## Consequences

- The body's placement says where the axis is; as a prism drawn on a slanted face takes the
  face's frame, the part made takes the body's axes (its Z along the axis).
- The Stretch tool locks for good the axes a turned body spans whole across its axis
  (`Target::fixed`), and the model refuses a stretch across one (`Refusal::TurnedAcross`).
- A « turned » mark may come with the cut list itself.

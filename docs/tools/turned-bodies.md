---
status: built
code: [crates/model/src/profile.rs, crates/kernel/src/engine.rs, crates/kernel/src/profile.rs, crates/tools/src/tools/shapes/prism.rs, crates/tools/src/tools/profile.rs, crates/tools/src/tools/stretch.rs, crates/tools/src/editor/view.rs, crates/model/src/ops.rs]
---

# Turned bodies (revolution)

A turned leg, a dowel, a knob, a ring: a body made by turning a half profile a whole turn about
an axis. It is drawn by the « Tourner » tool, edited by the Profile tool as an extrusion's
outline is, and stretched along its axis only. SolidCore turns a planar profile of lines and arcs
(`solidcore_ops::sweep::revolve`); the kernel also reads surfaces of revolution
(`Surface::PlacedRevolution`, STEP import; non-rigid for the stretch).

## The profile

A body's profile ([Prism tool and profiled blanks](prism.md#the-profile-in-the-part)) is either an
extrusion (outline in the body's XY plane, `z0`, `height`) or a **revolution**: a half profile in
the body's XZ plane — its points `(x, z)` with `x`, the radius, never negative —, turned a
**whole turn about the body's Z**. The axis is the body's, so its placement
([Bodies](../model/bodies.md)) says where the axis is; no axis of its own stored, no partial
angle. Same corners as an extrusion's (`{ p, bulge, fillet }`): arcs and fillets alike. A
profile that does not touch the axis gives a ring or a bowl.

- `BodyProfile { profile, sweep }` with `Sweep::Extrusion { z0, height }` or
  `Sweep::Revolution { revolution: Turn::Whole }` (saved `"revolution": "whole"`);
  `BodyProfile::turned`, `is_turned`.
- `Primitive::Revolution { origin, profile }`, built by `Solid::new_revolution` (SolidCore's
  `sweep::revolve`, arcs either way, a point a rounding off the axis put on it);
  `profile::is_turnable` (all on the side of positive X, touching the axis or not).
- `BodyProfile::stretched` follows the heights along Z and drops the profile across the axis.

Decision record: [A turned body is a half profile turned about the body's Z](../decisions/0023-turned-body-half-profile-about-body-z.md)

## The Turn tool

« Tourner » (`tool.turn`, `ToolKind::Turn`, L, icon `turn`, ribbon group Create after the prism):
a separate tool for the user, the Prism's code in a turning mode (`PrismTool::turning`).

- **The axis** first, two points, inferred as a line's (a typed length for the second); only its
  origin is marked and no length is shown: only its way matters. It is drawn as an endless guide
  line in its axis colour.
- **The half profile** then, drawn point by point as the Prism's outline (its arcs, fillets,
  typed lengths and inferences — the axis among them, as a guide line, its origin as an
  endpoint, through `Tool::infer_line`), on one side of the axis, in the plane through the axis
  and the profile's first point. The plane is decided by that click: a point inferred on a face
  or an edge; over nothing, taken on the horizontal plane through the axis's start, that plane
  seen edge on giving the plane through the axis facing the eye. A first point on the axis
  decides nothing: the plane follows the cursor (through the axis and its point, inferred as the
  first one's) as long as every corner placed lies on the axis, the first corner or arc off it
  deciding it (Esc back to corners all on the axis frees it again); locked for the rest of the
  outline.
- **Previews**: before the first point, its radius from the axis dashed with its length, and the
  circle it turns along as a faint guide (preview guides not opaque go with the faint lines,
  which blend); while drawing, the outline's mirror across the axis dashed, closed as the outline
  is (its side to the cursor and back to the first corner).
- **Closed by hand**, back on its first point (no automatic closing along the axis), the side
  along the axis drawn as any other; turned at once when closed — no height. A profile crossing
  the axis or itself is refused.
- **Made**: the body's Z is the axis (from the first point to the second), its X towards the
  profile; as a prism drawn on a slanted face takes the face's frame, the part made takes the
  body's axes (its Z along the axis, its origin on the axis at the profile's lowest height,
  `Primitive::at_origin`); in an open part it is a body of it ([Bodies](../model/bodies.md),
  positive or negative as the drawing tools'), placed in the part's axes.
- **Options**: the fillet, the trace (« Tracé »), reversed; no offset.

## Editing the half profile

The Profile tool ([Editing the profile](prism.md#editing-the-profile)) works in the frame of
the profile (`edit_frame`): a turned body's XZ plane, where the half profile lies, not turned to
face the eye (an extrusion's outline likewise on its bottom cap, whatever the view). Same grips
(squares, bars, free, arcs, fillets), plus these rules: a corner on the axis slides along it, a
side lying on the axis stays on it (no grip unless free), a move across the axis stops on it
(bisection along the move), never beyond; an arc bulging across the axis is refused at the drop.

## Display

The bold outline of an open part's profiled bodies (their bottom cap for an extrusion) is the
half profile for a turned body, in its XZ plane, with its axis in long dashes, 10 % past the
profile at each end — drawn by the editor through the Drawer into `View::drawing`
(`editor/view.rs`, [Drawer](../render/drawer.md)).

## Stretch

Along the axis, the body is rebuilt from its stretched half profile (`stretch_bodies`), the
cutters moving the half profile's heights (exact). **Across the axis it is refused**
(`Refusal::TurnedAcross`: across the axis, or across an arc); the diameter is changed by the
Profile tool only. The Stretch tool locks for good (`Target::fixed`, a padlock that does not
unlock, said in the status bar) the axes a turned body spans whole across its axis — any cutter
would cut it —, without saving that lock in the part's cutters; a turned pin inside a longer
part leaves the part's axes free, a cutter through the pin then refused by the model.

## Cut list

Nothing particular: the box of the final B-rep in the part's axes, as any part (a Ø 40 × 700 leg
reads 40 × 40 × 700).

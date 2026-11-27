# Stretch: what crosses a cutter curved is scaled along the axis

- Status: accepted (2026-11-27; prototype)
- Design: [Stretch](../tools/stretch.md)
- Amends [0009](0009-stretch-rewrites-brep-record.md): curved geometry across a cutter is no
  longer refused, it is scaled.

## Context

A stretch translates each section by its move and lengthens what crosses a cutter, provided it
is invariant along the axis: lines along it, planes containing it, cylinders and extruded nets
along it. Anything else across a cutter — an arc of a profile, a round, a hole, a slanted face —
refused the stretch, and the user had to find a cutter position clear of every curve. On a
user's bracket (a straight leg, then a concave arc widening into its head), no cutter could go
through the arc, and a whole-circle span even refused cutters beside it (fixed separately).

Three rules were compared on manipulable profiles (the bracket, a corner round, a round meeting
a slanted face, a rounded end, a round section, a drilled board):

- **A, straight matter inserted**: cut at the cutter, the far side moved, the gap filled by the
  section extruded along the axis (what OpenCutList does to a SketchUp mesh). An arc becomes
  half arc + straight + half arc, with a kink wherever the cutter meets it off its tangent along
  the axis; compression only where the slab taken out has a constant section.
- **B, a proportional zone**: a zone between two cutters scaled as a whole. Holes in the zone go
  oval; the stretch would no longer be a translation of sections.
- **B′, each crossing element scaled between its ends**: what crosses a cutter is scaled along
  the axis, its ends following their sections. An arc becomes an arc of an ellipse, tangent
  where it met a line along or across the axis; the rest is translated as before.

Decided on 2026-11-27 (questions / answers, then the prototypes): B′, closed curves included as
ellipses rather than oblongs — what happens to a round is then what happens to an arc.

## Decision

- **What crosses a cutter curved is scaled along the axis.** A face crossing a cutter that is
  not invariant along the axis (a cylinder across it, a cone, a sphere, a NURBS face, a plane
  not containing it), and the faces it touches through an edge that are such too, form a
  **group**, scaled by one affine map along the axis, `x ↦ lo + m(lo) + (x − lo) · k`: its
  extent `[lo, hi]` along the axis, each end moved as its section, `k` set by them. Its edges
  and vertices go with it. A circle becomes an ellipse, a cylinder an elliptic cylinder, a sphere
  an ellipsoid, a round across the axis an elliptic round; a hole crossed by a cutter goes oval.
  Compression too, down to the 1 mm the tool keeps between sections.
- **Exact, on the NURBS form**: SolidCore holds every analytic curve and surface of a part
  with its NURBS form; the map moves its control points, the parametrisation is kept, so the
  pcurves on the group's faces stand as they are. The scaled faces become NURBS faces; their
  edges say they are ellipses. A plane or line in a group is mapped as such. A face without a
  NURBS form, or an intersection curve, still refuses.
- **The faces around a group take it as they took a lengthened edge**: a plane containing the
  axis, a cylinder or extruded net along it have the group's edges' pcurves mapped in their own
  (u, v), where the map is affine too. A vertex of a group where a face in one section meets it
  must move as that section does: the group's extent ends hold the faces of their sections,
  and anything else refuses (a group whose vertex lies inside its extent, with matter of one
  section there).
- **Still invariant faces lengthen as before**: what crosses a cutter along the axis is not a
  group; a board stretched along its length is unchanged.
- **Default cutters avoid closed curves**: when the tool puts its cutter in the middle (no
  cutter saved), it moves it to the nearest position clear of every hole and round section
  crossing the axis, when there is one. A hole crossed by a cutter goes oval only when the user
  put the cutter there; the preview shows it before the click.
- **The preview** moves the display mesh face by face: a scaled face by its group's map, a face
  in one section by its section's, a lengthened face by a blend along the axis, its points on a
  group's edge by that group's map (a tube's two circles, each its own ellipse).

## Alternatives considered

- **A, straight matter inserted**: kept as the rule of OpenCutList, but a kink at every arc the
  cutter does not meet square, no compression across a curve, and it needs kernel operations
  (cut, extrude, union) where B′ rewrites the record.
- **B, a proportional zone**: holes go oval even when no cutter crosses them.
- **B′ with closed curves refused, or as oblongs**: refused leaves a round leg or a dowel across
  the axis unstretchable; oblongs are tangent only when the cutter goes through the centre and
  cannot compress. The ellipse is what the arc does.
- **Stretching the recipe** (a profile's lines lengthened, its arcs keeping their radius; a
  round node computed again on its stretched child): what a part built in Skraft should do,
  later and alongside; B′ is for what has no recipe (imports, flattened bodies) and what the
  recipe does not reach.

## Consequences

- A group gives its faces and edges new geometry, not a translation: a round across the axis is
  no longer a circle for the inferences (no centre), nor for what reads circles (a turned body's
  profile, a hole's diameter).
- A round meeting a slanted face kinks where they meet: the slanted tangent does not follow the
  scale. The preview shows it.
- A rounded end cut in its bulge stretches into a half ellipse (its ends, in one section, do not
  move; its tip follows the far section).
- Two groups touching at a vertex, with different extents, refuse.

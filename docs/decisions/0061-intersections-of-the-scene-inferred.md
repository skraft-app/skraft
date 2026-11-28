# Intersections of the scene inferred: edge × edge, edge × face, line × face

- Status: accepted (2026-11-28; built)
- Design: [Inferences](../tools/inferences.md#crossings)

## Context

The engine ([0002](0002-central-inference-engine.md)) built points from a line and something
else — a line × an edge, two lines (guides among them), a line × a guide circle — and from the
guide circles, but not from the solids alone: where the edges of two parts cross (a cross-lap,
a board overhanging another), where an edge pierces a face (a leg through a top) nothing was
offered. A line through the face under the cursor was taken there, but as a mere point on the
line, with no marker of its own. And no built point could be tracked, even those that hang on
nothing but the scene (two guides crossing).

## Decision

- **Three kinds, ranked with the built points** (rank 2, the thirds' and crossings'):
  `OnTwoEdges` (edges of two different solids — within one solid edges meet only at its
  vertices), `OnEdgeAndFace` (an edge through a planar face of another solid), `OnFaceAndLine`
  (a line through the planar face under the cursor). Within half a pixel brought to depth, as
  the other crossings; a point at an edge's end is its endpoint, not a crossing.
- **Edges near the cursor only**: those passing within the aperture, paired; their boxes
  must meet (the solid's box, for its faces) before anything is computed.
- **The faces an edge pierces are those of the solids near the cursor**, not only the face
  under it: by the edge, the cursor is as often on the piercing solid's own sides. A point is
  in the face (not only on its plane) when the ray through it meets that face of that solid
  first, at its depth — that solid alone, the edge's solid meeting the ray there too —, and
  nothing else hides it.
- **Drawn as an ×** (`overlay::Marker::Intersection`): the on-edge marker's colour for edge × edge and edge × face, the
  line's colour for line × face.
- **A crossing of fixed things can be tracked**: edges, faces, guides, the axes as drawn —
  not a line through the start point or a tracked point, which moves with the tool.

## Alternatives considered

- **The face under the cursor only, for edge × face**: no pick per candidate, but the point
  was lost whenever the cursor stood on the piercing solid's side of the edge.
- **Apparent crossings** (edges crossing on screen, skew in space): SketchUp does not infer
  them, and the point has no one place in 3D.
- **Curved faces**: their surfaces are not in `Snaps`; through their mesh later if missed.

## Consequences

- The hover costs a little more in dense scenes seen from afar (`infer_timing`, 1 600 boards:
  0.05 → 0.06 ms), nothing on one heavy part.
- A third or a midpoint at the very place of a crossing still comes first (same rank, nearer
  or nearer the eye): the point is the same.

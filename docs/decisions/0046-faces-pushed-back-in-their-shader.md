# Faces pushed back by their slope, in their shader

- Status: accepted (2026-11-20)
- Design: [Display](../render/display.md#depth-of-lines)

## Context

Lines are quads widened on screen, carrying the depth of their centre line: a pixel away from
it, a neighbouring face can be nearer (a concave corner, a face seen at a glance) and would bury
the widened part. Until 2026-11-20 every line was pulled towards the eye by 3 half widths
measured at its depth: in perspective a constant share of the depth (about 0.5 % with the 35°
field of view on a high-density screen), in parallel a few pixels' worth of model units. Zoomed
out, it outgrew a panel's thickness and the gaps between parts: the edges of the parts behind
showed through the faces, whole, or as tapering slivers where a pulled edge crossed a slanted
face (the "staircase" of a curve's segments). In the user's models, the concealed hinges inside
the doors (5 mm behind their faces) showed their rings at a room's distance.

## Decision

- Lines at their own depth; the faces pushed back from the eye by their own slope over 7/8 of an
  edge's half width (SolidCraft's rule, its `FACE_BIAS`, issue #1434), **worked out in the
  faces' vertex shader**: each vertex moved along its ray by the size of a pixel there times the
  tangent of the angle between the normal and the ray (capped at 50). The pipelines add 8 depth
  steps only, for the rounding; the section caps, which write their depth, push it back as much
  in their shader.
- An edge whose two faces turn away is not drawn (but for a ghost): its closed solid hides it.
- The tools' lines wider than an edge are lifted by what the faces' push does not cover.
- The edges' segments stored once per mesh in a storage buffer (64 bytes: ends, the two
  triangles' outward normals), each a quad of 4 corners indexed by one shared index buffer.

## Alternatives considered

- **A smaller pull**: still a share of the depth, the same defect a little farther away.
- **The pipelines' slope bias** (SolidCraft's way; built and tried on 2026-11-20): the bias is
  the GPU's own — Apple's is several times WebGPU's (wgpu passes it unchanged to Metal): 0.25 px
  of it let a hinge 5 mm behind a door through at 12 m.
- **Edges laid on their own faces** (built and tried on 2026-11-20): each half of an edge's quad
  on the plane of the triangle it covers, never nearer than that triangle reaches, else sinking
  with the other side's face; outlines drawn outwards, sinking with their last facet. It hid the
  hinges even at a glance, but each case needed its rule, and each rule its counter-case: an
  edge against another part (a box standing on a board) lost the half of its width over the
  board, and an outline's width changed along a cylinder with its facets' angles. A curved
  surface's outline could not both stay whole and never show through a face just in front of
  the part.
- **Deciding an edge's visibility on its centre line** (reading the faces' depth while drawing
  the edges): exact, but the depth cannot be read in the pass that writes it; splitting the pass
  stores and loads the multisampled targets, some 6–12 ms a frame at 3024 × 1964.

## Consequences

- What lies behind a face shows only where it is nearer than about (7/8 + 1) half widths of the
  face's slope, measured on screen: a hinge 5 mm behind a door is hidden at 12.7 m seen 25° off
  the door's normal; farther, or more at a glance, it shows. A line on a face seen at a glance
  may lose an eighth of its half width on that side.
- Edges and outlines are drawn as before: as wide on every side, against any part.
- Cost (Cruise, Apple M1, 3024 × 1964): 23.5 ms a frame instead of 30.3 ms; the meshes 64.5 MB
  instead of 206.8 MB on the GPU.
- The depth buffer is asked for in 32-bit float (`DEPTH32FLOAT_STENCIL8`), which the renderer
  never requested before.

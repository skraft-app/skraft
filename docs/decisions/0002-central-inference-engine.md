# One inference engine, on the exact B-rep

- Status: accepted (2026-11-09)
- Design: [Inferences](../tools/inferences.md)

## Context

Each tool chose its points itself (`base_point`, `target`, `center_at`…). SketchUp-style
drawing needs the same inferences everywhere — endpoints, midpoints, centres, lines along the
axes and edges, their crossings, tracked points — with the same markers, tooltips and locks.

## Decision

- **A central engine** (`tools/src/infer/`, no UI, no GPU, no state) replaces the point choices
  of each tool: a tool asks the `Cx`, which fills a `Query` with what the editor holds and calls
  `infer::infer`. Locks are held by the editor for every tool.
- **Exact geometry**: candidates come from the B-rep record (`f64` vertices, typed curves:
  exact midpoint of a line, exact centre of a circle), not from the mesh; the mesher's polyline
  only as a fallback for splines and intersection curves.
- Delivered in steps: (1) points — endpoint, midpoint, centre, on edge, on face, ground — with
  markers, tooltip and wiring into Box, Cylinder, Move, Rotate, and (2) linear — axes from the
  start point, parallel / perpendicular, Shift lock, arrows shared — together; then (3)
  relative inferences, decided the same day, delivered progressively, each part tested before
  the next: (a) tangent to arcs, (b) line × edge, (c) point tracking and line × line.

## Alternatives considered

- Keeping per-tool point choices (what was there before).
- Inferring on the display mesh: its vertices and polylines are not the exact geometry.

## Consequences

- Every tool infers the same way; the guides, the axes as drawn and the tools' own lines
  plug in as more lines.
- A snap index per solid (`kernel::Snaps`) is built with the mesh and cached like it;
  `Mesh::edges` keeps the record's edge index for the fallback.
- Seams had to be recognised on the record (the mesher does not fill `seams`): until then a
  cylinder's seam was drawn.

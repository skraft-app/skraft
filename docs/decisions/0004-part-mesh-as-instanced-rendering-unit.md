# Render a part's final B-rep as one instanced mesh

- Status: accepted (2026-11-09)
- Design: [Display](../render/display.md)

## Context

A component may be placed many times (occurrences, nested in assemblies). With the parts /
assemblies model ([Data model](../model/data-model.md)), only a part has geometry, its final
B-rep; an assembly has none. The design review had flagged, as its risk 2, the cost of sending
geometry to the GPU when the model changes.

## Decision

A component is an instanced mesh: the display follows the model. The rendering unit is a part's
final B-rep — one GPU buffer per part (the concatenation of the meshes of its direct solids,
with the index range of each), drawn by one instanced draw with the world matrices of all its
occurrences in the scene, nesting included. Tessellation is cached per solid on the CPU: an
unchanged solid is never tessellated again. A mirrored occurrence passes the sign of its
determinant to the shader, which derives the front faces from it.

## Alternatives considered

None recorded.

## Consequences

- Changing a part sends only its own triangles to the GPU (the end of risk 2 of the review).
- `render` does not know the document: it receives meshes by key with a revision and batches
  `(key, matrices)`; the `tools` crate's display scene flattens the document.
- A selected solid along one path is drawn again with its index range and that occurrence's
  matrix.

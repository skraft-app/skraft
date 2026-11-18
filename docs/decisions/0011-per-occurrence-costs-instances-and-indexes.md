# Per-occurrence costs: one GPU instance, lookups through indexes

- Status: accepted (2026-11-12)
- Design: [Display](../render/display.md)

## Context

On 2026-11-12, large models made the app slow or froze it, each time through a cost paid per
occurrence:

- a 200 × 200 array froze the app (draw calls and geometry built on the CPU per occurrence);
- a stretch among 10 000 cubes fell to 30 FPS (queries per mouse move testing every placement);
- a model of 10 000 distinct parts spent 13 ms per frame while its meshes came in (a full
  sync of the scene for each mesh);
- 40 000 occurrences selected cost 12 ms per frame (the selection's drawing worked out again
  each frame);
- the stretch tool took 300 ms per hover with 10 000 occurrences selected (searches by id
  scanning the occurrence list).

## Decision

- What is drawn or listed per occurrence costs one GPU instance, never draw calls or CPU-built
  geometry: runs of highlights in one instanced call, box outlines as instances of the unit
  cube dashed by the shader, the outliner laying out only the rows in view; buffers kept within
  the device's limit.
- No query per mouse move tests every placement: a spatial index of the placed meshes (a linear
  BVH over their world boxes, refit when only boxes change) and the same entries sorted by
  occurrence path, rebuilt at each `DisplayScene::sync`.
- Meshes coming from the kernel, the document unchanged, are taken in by
  `DisplayScene::refresh_meshes` (only the definitions drawing that solid, a BVH refit), not a
  sync.
- The selection's drawing (`SelectionDrawing`) is worked out once and kept while what it
  depends on stays.
- A search by id among an assembly's occurrences goes through an index (id → position) built
  after a few searches; maps and sets keyed by ids hash with `FastMap` / `FastSet`.

## Alternatives considered

None recorded beyond the former brute-force scans, kept as the reference the indexed answers
are tested against (same results, same order).

## Consequences

- `Assembly::occurrences` is private, so that every search goes through the index.
- Tests compare each indexed query with a scan (`the_scene_index_picks_as_a_scan_does`,
  `the_rectangle_takes_what_a_scan_takes`, `the_meshes_near_the_ray_are_those_a_scan_finds`,
  `meshes_taken_in_leave_the_scene_a_sync_would`, `the_selection_drawn_follows_what_it_hangs_on`).

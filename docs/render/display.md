---
status: built
code: [crates/tools/src/scene.rs, crates/tools/src/bvh.rs, crates/tools/src/editor/view.rs, crates/render/src/lib.rs, crates/render/src/shader.wgsl, crates/model/src/fast_hash.rs, apps/skraft/src/workspaces/common/viewport.rs, apps/skraft/src/lib.rs]
---

# Display

How the document becomes pixels: the unit the GPU draws (a part's mesh, instanced per
occurrence), how the display scene indexes what is placed so that picking and drawing stay
cheap on large models, how precision is kept far from the origin, and how lines (edges, guides,
previews) are drawn wide and dashed. The colours, section and x-ray are in
[Rendering and look](look.md); what tools draw over the model, in [Drawing helpers](drawer.md).

## The definition as the rendering unit

A component is an instanced mesh: the display follows the model. With the model of
[Data model](../model/data-model.md), only a part has geometry (its final B-rep), an assembly
has none. The rendering unit is therefore **a part's final B-rep** — one buffer per part, drawn
by instancing with the matrices of all its occurrences. Changing a part sends only its own
triangles to the GPU. Below, "a definition" thus reads "a part". **Mirror**: a mirrored
occurrence flips the winding of the triangles; the sign of the determinant is passed to the
shader (`n0.w`), which derives the front faces from it (test
`a_mirrored_placement_shows_its_outside`).

```
Level 1 (CPU, tools)   Arc<Solid> → Arc<Mesh>      tessellation cached per solid: an unchanged
                                                   solid is never tessellated again
Level 2 (GPU, render)  Definition → GpuMesh        concatenation of the meshes of ITS direct
                       + ranges [solid → indices]  solids; merged again (copy) if it changes
Drawing                1 instanced draw per definition, with the N world matrices of all its
                       occurrences in the scene (nesting included)
```

- A definition's buffer only holds its **direct solids**; its subgroups have their own, drawn
  with the composed transformations.

Decision record: [Render a part's final B-rep as one instanced mesh](../decisions/0004-part-mesh-as-instanced-rendering-unit.md).

## Highlight and boxes

A selected solid is selected along a path (a single occurrence); it is drawn again on top with
its index range and that occurrence's matrix. A selected occurrence (**part or assembly**) also
shows the bounding box of its content, in its own axes, dashed in the selection's colour
(`ThemeColor::Selected`) and depth-tested, drawn by the editor through the drawer
(`Drawer::oriented_boxes` into `View.drawing`, [Drawing helpers](drawer.md)); it follows the
move tool's ghost, and is not drawn with the stretch, copy along a line, copy as a grid and
distribute tools (which draw their own boxes). The part open, else the assembly being edited,
shows the same box in grey (`ThemeColor::ContextBox`), a little larger.

## Scale

What is drawn or listed per occurrence costs one GPU instance, never draw calls or CPU-built
geometry. Runs of highlights of the same part are drawn in one instanced call; a box outline is
one instance of the unit cube (`Frame::boxes`), its edges dashed by the shader from every corner
(each half edge anchored, its dashes laid on it); the outliner only lays out the rows in view.
Lookups over a selection or an assembly go through sets or an index (`ContextIndex`), never a
search per occurrence. The renderer keeps every buffer within the device's limit (what does not
fit is dropped, the device is not lost).

**Why:** models of tens of thousands of occurrences (arrays, imports) must stay interactive:
any cost per occurrence, per frame or per mouse move, multiplies by that count.

Decision record: [Per-occurrence costs: instances and indexes](../decisions/0011-per-occurrence-costs-instances-and-indexes.md).

### Spatial index

No query per mouse move tests every placement. Each `DisplayScene::sync` rebuilds an index of
the placed meshes (`scene.rs`, `SceneIndex`): their world boxes in a linear BVH (`bvh.rs`:
Morton order of the centres, one scale on the three axes, cut where the highest differing bit
of the codes turns on, down to 4 items per leaf, built in O(n log n) by one integer sort; the
leaves keep their items' boxes and hand out only those that pass the test; when only boxes
changed — occurrences moved, meshes come in — it is refit instead, and built again once its
nodes' areas grow half again what they were at build), and the same entries sorted by
occurrence path (an occurrence's parts, at any depth, form a run).

- Picking walks the BVH nearest box first and skips boxes beyond the best hit (plus its tie
  tolerance); inferences take the meshes near the ray (`inferable_where`), the rectangle
  selection those whose box seen meets the rectangle (`parts_where`; a window takes an entity
  only when every part under it, counted by `parts_under`, lies inside); lookups by path
  (`inferable_at`, `part_at`, `inferable_of`) search the sorted run. Results keep the scene's
  order (ties, candidates), as a brute-force scan gives them (tests
  `the_scene_index_picks_as_a_scan_does`, `the_rectangle_takes_what_a_scan_takes`,
  `the_meshes_near_the_ray_are_those_a_scan_finds`).
- The stretch's preview compares the document it sketches with the current one by
  `Document::placed_changes`, which skips what both share; it counts placements over the
  distinct assemblies (`count_placed`), not per path; the stretch tool keeps its target while
  the scene's generation, the occurrences and the axes stay.
- The maps and sets keyed by ids or addresses on these paths (an assembly's index, the
  selection, the scene's) hash with `FastMap` / `FastSet` (model, `fast_hash.rs`): a
  multiplicative hash, several times cheaper than SipHash, whose resistance to chosen keys a
  desktop modeller does not need.
- An operation on a few occurrences among many finds them by the assembly's index
  (`positions_of`), and a move changes them in place, keeping it.

### Meshes coming in

The document unchanged, a display coming from the kernel is taken in by
`DisplayScene::refresh_meshes`, not a sync — only the definitions drawing that solid change (the
scene keeps, by solid, the definitions using it), then the boxes of their placements (each
definition's run of index entries), the BVH refit (rebuilt only when a mesh gains or loses its
box) and the scene's bounds; the placements, guides and recipe stay (test
`meshes_taken_in_leave_the_scene_a_sync_would`). `placed_guides` returns at once for a model
without guides.

### The selection drawn

Its marks, its occurrences' boxes (in their own axes, one per component) and the set of its ids
are worked out once (`SelectionDrawing`) and kept while the scene's generation, the context, the
selection and the recipe shown stay; a move's ghost is the same moved (test
`the_selection_drawn_follows_what_it_hangs_on`). `ContextIndex` searches the assembly's own
index instead of building one.

### Occurrences by id

`Assembly::occurrences` is private; a search by id (`occurrence`, `position`) goes through an
index (id → position) built after a few searches since the last change to the list
(`occurrences_mut` drops it; a clone keeps it), so that a search per occurrence stays linear in
all. Below 16 occurrences, a scan.

## Coincident faces

Picking takes the nearest hit, but hits within `tie_tolerance` along the ray (1 µm + 1 ppm of
the distance) are a tie, broken in a fixed order: the solid the ray enters (outward normal
against the ray, from the vertex normals) before one it leaves (seen through a section, from
inside), then the smaller bounding box, then the scene order.

**Why:** the choice must not flicker from one pixel to the next, and the same rule serves every
tool (selection, inferences, paint, booleans).

Not yet: reaching the other solid of a tie (cycling, a list under the cursor).

## Renderer and document

- **`render` does not know the document**: it receives meshes by key (the definition's id) with
  a revision, and a list of batches `(key, matrices)` + highlights + preview lines. It is
  `tools` (the display scene) that flattens the document and decides the revisions.
- `render` draws into **its own off-screen target** (MSAA ×4 colour + depth, resolved into a
  texture): the app shows it as an egui image. Rendering is thus independent of egui and
  testable without a window.

## Web

On the web, **WebGPU only**: wgpu is built without its WebGL backend (eframe's
`wgpu_no_default_features`, egui-wgpu without its default `wgpu/webgl`), and a browser without
`navigator.gpu` gets a message in place of the app (`apps/skraft/src/lib.rs`, `start`), as does
a start that fails (no adapter).
**Why:** the shader reads its palette and texture entries from storage buffers, which WebGL2
does not have, so the app could not draw there anyway; the backend only weighed on the UI's
wasm (naga's WGSL → GLSL translation, wgpu-core, the GLES hal: ~1.7 MB of code).

## Precision

The GPU only has `f32`; neither WebGPU, WebGL2 nor Metal offer `f64`.

- **Camera-relative rendering**: view, placements and lines are in `f64`; each
  `view × world × recentring` is computed in `f64` on the CPU and only then converted to `f32`
  (the instance buffer is rewritten every frame anyway); the shader only applies the
  projection.
- **Recentred meshes**: the kernel's `Mesh` is in `f32` relative to its centre (`origin` in
  `f64`), and a definition's GPU mesh is relative to the centre of its solids.
- **Reversed depth** (near → 1, far → 0) in a `Depth32Float`: nearly uniform precision; the
  projection given must be reversed (`glam::camera::rh::proj::directx::perspective_infinite_reverse`).
- Checked by a test: the same scene 100 km from the origin gives the same image.

## Wide lines

Hardware lines are one pixel wide (half a point on a high-density screen), and WebGPU has no
line width (Metal has none at all). Edges, axes, grid and tool previews are therefore quads (two
triangles per segment) widened on screen by the vertex shader, after clipping the segment in
front of the eye. Widths are in points (`EDGE_WIDTH` 1.5; `LINE_WIDTH` 1.5; `BOLD_WIDTH` 3;
`GRID_WIDTH` 1) and multiplied by the screen's pixel density (`Frame::pixels_per_point`): the
same apparent thickness everywhere.

### Dashes

A `Line` may be **dashed** (`Dash { pattern, lay }`: a pattern in points — dash, gap, dash,
gap — and how it lies, `Lay`); the fragment shader discards the gaps. All but the guides' are
**laid on the segment itself**: the vertex shader passes a distance along it with
perspective-correct interpolation (`LineOut::laid`). Three ways:

- **Anchored** (`Lay::Anchored`: a tool's dashed lines; a box's half edges, always): a dash at
  both ends, the number of periods set by the length on screen, the pattern stretched to fit;
  dashes evenly spread on the segment, shorter on screen as it runs away; they jump when the
  count of periods changes, a period's worth of zoom apart.
- **In model units** (`Lay::Model(phase)`: curves, hidden parts but an anchored line's in its
  own stipple, every other dashed line): the
  pattern at `phase` model units at `a`, its period the power of two (model units) nearest to
  the pattern's length **at the view's focus** (`Frame::model_per_point`, the orbit centre's
  scale): one period for the whole picture, so the dashes are regular on the line, shorter on
  screen as it runs away (as the anchored ones), unchanged as the view orbits or pans, doubled
  or halved as it zooms, a factor two apart. Where they would be a few pixels long (towards a
  vanishing point) they fade, from 6 to 3 pixels a period (the screen derivatives of the laid
  distance), into a line inked as much as they ink it, rather than shimmer. The distance is
  laid from the segment's nearer end (where a model unit is the most pixels, so the float keeps
  the dashes there), the phase there passed in two parts (high, low; computed in `f64`) and
  taken modulo the period exactly (powers of two: `wrap`), so a line far from its phase's
  origin keeps its dashes.
- **On screen** (`Lay::Screen(phase)`: the guides, `Pen::guide`): the distance along the
  segment on screen, interpolated linearly on screen, from a phase in points at `a`: dashes as
  long on screen everywhere, held still as the view pans, sliding along the guide as it orbits.

**Why:** dashes spread evenly on screen slide along a receding line as the view orbits
("swimming"); laid on the segment, they hold still on it. One period for the picture, rather
than one chosen pixel by pixel, keeps a receding line's dashes one size along it. The guides are
the exception by the user's choice: as long on screen, swimming, rather than shrinking as they
run away.

Decision record: [Dashes laid on the segment](../decisions/0032-dashes-laid-on-the-segment.md).

### Depth of lines

- Lines that run past a parallel projection's depth range (infinite guides and axes) are
  clamped to it, not cut: the line pipeline uses `unclipped_depth` where the device has
  `DEPTH_CLIP_CONTROL` (`render::OPTIONAL_FEATURES`, which the app asks egui's device for).
- A quad has the depth of its centre line, while a pixel away a neighbouring face can be
  nearer (inner corners, faces seen at a glance): the widened part would sink into it. Lines are
  drawn **at their own depth** and the faces are **pushed back by their own slope**, in their
  vertex shader (`pushed_back`): each vertex moved away from the eye along its ray (its pixel
  kept) by the depth its face gains over 7/8 of an edge's half width (`FACE_PUSH`) — the size of
  a pixel there times the tangent of the angle between the face's normal and the ray, capped
  at 50 where it is seen edge on —, and by 8 depth steps in their pipelines (`FACE_STEPS`: the
  `f32` rounding between a face and what lies on it). Measured on screen, the same at every
  zoom, in perspective as in parallel. In perspective the push is a share of the vertex's
  position, so a corner behind the eye stays behind it and the near plane cuts its triangles.
  - An edge on a face draws (nearly) whole over it at any angle; what lies behind a face shows
    where it is nearer than about (7/8 + 1) half widths of the face's slope — the face recedes
    across the edge's quad too. A concealed hinge 5 mm behind a door's face is hidden at 12.7 m
    seen 25° off the door's normal; farther, or more at a glance, it shows.
  - An edge's quad runs half a width past each end **along the edge's own depth** (linear on
    screen), else its depth would be stretched over the segment and its nearer end would sink
    into the face beside it. Not for a segment under 4 half widths on screen: seen end on, its
    tip would rise through what hides it.
  - The **model's axes** have their own pass, after the edges and the tools' lines: tested
    against the depth a hair nearer (`AXIS_LIFT`, 10⁻⁵ of the depth), so an axis always wins over
    an edge it lies on and stays hidden behind any face in front of it; blended at its alpha, no
    depth written, so a negative half takes the colour of what it covers. Cut where its depth
    doubles (`push_axis`): one quad from next to the eye to far away would round its far part
    by more than the lift.
  - An **edge whose two faces turn away** is not drawn: its closed solid hides it, however close
    behind its faces (drawn for a ghost, seen through) — a small part's far edges.
  - The **section caps** write their own depth, which the pipelines' steps do not reach: their
    shader pushes it back as much (`pushed`, the slope from the depth's screen derivatives).
  - A **line wider than an edge** (`BOLD_WIDTH`) is lifted towards the eye by the pixels of its
    half width the faces' push does not cover, measured at its depth (scaling towards the eye in
    perspective, translation along the view axis in parallel: the pixel does not move).
  - Tests: the edges of a pocket are not buried; a panel's edges 2 mm behind another, seen from
    afar, stay hidden; a hinge's cup 5 mm behind a door changes no pixel from 12.7 m (25°) to
    0.6 m (75°); every B-rep edge segment finds its two triangles.
- **Storage**: a mesh's segments (B-rep edges, outline candidates) in a storage buffer, 64 bytes
  each — the ends, the outward normals of the two triangles along it (`Segment`) —, read by the
  vertex shader by its vertex index, each a quad of 4 corners indexed by one buffer shared by
  every mesh (`SegmentIndices`), so the GPU shades the corners its triangles share once.
- The depth buffer is `Depth32FloatStencil8` when the device has it (`OPTIONAL_FEATURES`): with
  reversed depth and the near plane a thousandth of the distance, a 24-bit one steps by about
  2 mm at 30 m.

**Why:** lines used to be pulled towards the eye by 3 half widths measured at their depth — in
perspective a constant share of the depth (about 0.5 %), in parallel a few pixels' worth of
model units. Zoomed out, that outgrew a panel's thickness and the gaps between parts: the edges
behind came through the faces, whole or as slivers where they crossed a slanted face (the
"staircase" of a curve's segments). Pushing the faces back by their own slope is measured on
screen (SolidCraft's `FACE_BIAS`, its issue #1434), but worked out in the shader: a GPU's slope
bias is its own (Apple's is several times WebGPU's: 0.25 px of it let a hinge 5 mm behind a door
through at 12 m).

**Cost** (`bench_render_step`, `crates/tools/src/bench.rs`: Cruise_Assembly.step, 2 789
placements, 0.15 M edge segments, Apple M1, 3024 × 1964): 23.5 ms a frame (30.3 ms with the
quads' corners as vertices), 11.5 ms without outlines; the meshes 64.5 MB on the GPU (206.8 MB
before: a corner's vertex of 56 bytes, six per outline candidate).

Decision record: [Faces pushed back by their slope, in their shader](../decisions/0046-faces-pushed-back-in-their-shader.md).

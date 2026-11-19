---
status: built
code: [crates/tools/src/tools/handle.rs, crates/tools/src/tools/copy_line.rs, crates/tools/src/tools/copy_grid.rs, crates/tools/src/tools/distribute.rs, crates/model/src/ops.rs]
---

# Copy along a line, copy along a grid, distribute

Three tools that copy or spread the selected parts (or bodies of an open part) from the centre
of their box, after OpenCutList's Smart Handle tool (`tool/smart_handle_tool.rb`:
`SmartHandleActionHandler`, `SmartHandleCopyLineActionHandler`,
`SmartHandleCopyGridActionHandler`, `SmartHandleDistributeActionHandler`). The tool computes the
placements; the model only puts them down, in one undo step.

## Three tools

Three commands — `tool.copy_line` (« Copier en ligne »), `tool.copy_grid` (« Copier en grille »)
and `tool.distribute` (« Répartir ») — sharing one module, `tools/handle.rs` (picking, box and its
centre, axes option, snapping on the nearest axis, arrow locks, counts typed, measure, preview);
the tools are `tools/copy_line.rs`, `tools/copy_grid.rs`, `tools/distribute.rs`. OCL has them as
actions of one tool; here a command is a tool. In the ribbon's **Handle** group of Build and
Assemble, after Rotate; **no default shortcut**. Each tool keeps its own options (OCL shares
them between its copy actions); Distribute's measure defaults to « between the parts », as OCL's
preset.

**Targets**: the active context's occurrences (parts and assemblies, with the context's guides
selected), or under « Parts » the parts at any depth (the guides picked with them copied along,
out of the box), and in an open part its bodies — as Move and Rotate copy.

## Operations

A list of placements, one per copy, computed by the tool — the model only puts them down — and a
move of the sources:

- `Operation::Copies { context, entities, moved: DAffine3, placements: Vec<DAffine3> }`: the
  sources moved to `moved · T` (the identity for the line and the grid; Distribute moves them to
  the first slot, as OCL sets the source instance's transformation), each copy of an occurrence
  placed at `placement · T` (rigid, mirror allowed, [Data model](../model/data-model.md); at most
  `MAX_ARRAY` = 1000 placements; `T` the source's before the move);
- its twin for bodies, `Operation::CopyBodies { target, bodies, moved, placements, scope }`
  (copies sharing their body definitions, [Bodies](../model/bodies.md)).

One operation, one undo step (labelled « Répartir » when the sources move). `Array`,
`ArrayBodies` and Move / Rotate stay as they are.

**Why**: `Array` (one step applied k times) can express neither a grid (two steps), OCL's
mirrored copies, nor a moved source.

Decision record: [Copies as a list of placements computed by the tool](../decisions/0021-copies-as-placement-lists.md)

## Copy along a line and along a grid

As OCL's, one step: no first click.

- **Picking** as the Stretch ([Stretch](stretch.md)): the selection when the tool starts, else a
  click, a rectangle or Shift clicks; once there is a target, the handle starts at **the centre
  of its box** in the chosen axes.
- **Options**: *measure* — edge to edge (outside), centre to centre, between the parts (inside);
  *axes* — active, context, entity (saved, as the Stretch's); *mirror* — toggled by a quick tap
  of Option (Alt elsewhere), for a single copy only (OCL's warning `copies_disable_mirror`
  otherwise). Option does not toggle copying as in Move: these tools always copy.
- **Line**: the cursor projected on the axis nearest on screen, or — on a face — the axis of
  the box face crossed towards the cursor (`_snap_handle`); → ← ↑ lock X, Y, Z.
- **Grid**: the cursor projected on the plane normal to Z, or to the axis locked by the arrows;
  its axes from the active X projected on that plane (`_get_handle_axes`).
- **Minimum distance** by measure (`_get_move_def`'s `dlmin` / `dmin`): the cursor stops at it,
  a smaller typed value is refused (`lt_min_distance`).
- **Typing**: a distance (grid: `dx; dy`), a count `x5`, `*5` or `/5` (grid: `x3; /4`, an empty
  slot = 1 copy, `x3=` both ways); Shift with `+` / `−` changes the count ([Counts with
  Shift](#counts-with-shift)); default `/1`.
- **After the click**, typing a distance or a count again redoes the copies (`Command::Replace`:
  one undo step), as OCL's `STATE_HANDLE_COPIES`.
- **Mirror** (one copy): line — the copy mirrored across the plane through its centre normal to
  the move (OCL's scaling by −1 then half turn about the move: that reflection); grid — the
  corner copy turned half a turn about the normal, the two side copies mirrored as on a line
  (OCL's transforms).
- **The grid's drawing**: its boxes are drawn at its four corners only (OCL's), its sides'
  lengths along the rectangle's sides from its low corner, each in the colour of the axis it
  runs along.

## Distribute

As OCL's, two steps.

- **Picking** as above; then a **first click** sets the start point, a **second** the end point
  (e.g. the inner faces of two uprights); Esc at the second goes back to the first. Both snap as
  the line's (nearest axis on screen or the box face crossed; → ← ↑ lock X, Y, Z), the measure
  line held on the line through the box's centre.
- **The start point** is the point inferred (what is distributed left out, as OCL hides it), else
  on the ground; on the geometry the end runs along the axis the way from the start mostly goes.
  Changing the axes takes the points again (OCL keeps the start).
- **The sources move**: `number` pieces in all, the source taking the first slot, its copies the
  next ones, spread evenly between the two points; while the tool runs, the sources are hidden
  and drawn by the preview (OCL `_hide_instances`): what is distributed shows as a ghost in place
  before the end point, then at its slot (`Preview::ghost`, the copies at the others).
- **Measure**: centre to centre or between the parts (no edge to edge); the mirror option does
  not apply.
- **Typing**: a count `x5` (5 pieces in all) or `/5` (5 intervals: 4 pieces, `/1` refused); Shift
  with `+` / `−` changes it; a distance sets the end point along the line; a **list of spacings**
  `30; 50` (two items at least, `30=` is two) fixes the first gaps, empty slots first (`;; 30`)
  fix the last ones, the rest spread (`_get_move_def`'s start / end spacings); after the click,
  typing a count or spacings again redoes it (`Command::Replace`).
- **Option held**: a ray from the start point along the line; the end point snaps to the first
  thing it hits (`DisplayScene::pick` with any ray), drawn as an arrow (OCL's raytest), so as to
  spread up to the next part without aiming at it. The ray follows the next cursor move.
- **Double-click** repeats the last distribute (count, spacings, both points) on the new target.

## Preview

A deviation from OCL, which draws the copies' edges in black: the copies as Move's ghosts while
copying, the boxes dotted in black (the source's and the copies' alike) and in the
scene, hidden behind faces (no front drawn over them: OCL's `shell_on_top` off), none when what
is copied is one part (or body) that is its box exactly (`Solid::fills_its_box`, square to the
axes chosen: the box would only redraw its edges), the measure
line or rectangle dashed in the scene, translucent where faces hide it (bold while an axis is
locked; `Pen::dimension`, [Dimensions](../render/drawer.md#dimensions)), in the axis colour, with
its VCB bubbles (one per side for the grid), and OCL's mirror badge (`PointShape::Mirror`, 40 px
past the mirrored copy: in the axis colour for the line, black for the grid). The copies' ghosts
are the selection's marks drawn at each copy (`Preview::copies`), so what is copied is the
selection (a click selects it, as Move); once the copies are made the selection is let go (OCL's
restart).

## Counts with Shift

Shift with `+` / `−` (OCL's `onToolKeyDown`): one copy more or fewer (the grid: on both sides;
Distribute: one piece), never below one, `x` or `/` kept; only while placing (line, grid) or at
the second point (Distribute), as OCL's `STATE_HANDLE`, the mirror's warning when going past one
copy. The app reads them from the typed text (`+`; `-`, `_` — Shift with `−` on most layouts — or
`−`) with Shift held and nothing typed yet, when the tool takes them (`Tool::takes_counts`): then
`Key::More` / `Key::Fewer` (control protocol `more` / `fewer`) instead of starting a value; the
numeric keypad's keys too.

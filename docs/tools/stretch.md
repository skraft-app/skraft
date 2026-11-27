---
status: built
code: [crates/tools/src/tools/stretch.rs, crates/model/src/stretch.rs, crates/model/src/ops.rs, crates/model/src/document.rs, crates/kernel/src/stretch.rs, crates/tools/src/scene.rs, crates/tools/src/editor/mod.rs]
---

# Stretch

The Stretch tool lengthens or shortens parts, assemblies and bodies along one of their axes without
deforming what should keep its size: the tenon at each end of a rail, the holes at each end of a
bracket keep their length while the part grows. It is OpenCutList's Smart Reshape stretch
(`tool/smart_reshape_tool.rb`, `SmartReshapeStretchActionHandler`; the analysis in
`worker/common/common_stretch_split_worker.rb` and `model/stretch/`, the application in
`common_stretch_apply_worker.rb`), ported as faithfully as the app allows. It works on the recipe
(bodies, components), and on the B-rep record only to rewrite a solid across a cutter.

Decision records: [Stretch after OpenCutList's Smart Reshape](../decisions/0008-stretch-after-ocl-smart-reshape.md),
[Stretch rewrites the B-rep record](../decisions/0009-stretch-rewrites-brep-record.md),
[Cutters and locks as a typed Behavior](../decisions/0013-stretch-behavior-on-component.md),
[What crosses a cutter curved is scaled along the axis](../decisions/0058-stretch-scales-curves-across-a-cutter.md).

## What it does

As in OCL: the box has a grip on each of its 6 faces; pulling one stretches along that axis,
the opposite face anchored. **Cutters** — planes across the axis, one in the middle by default
(moved to the nearest place clear of the target's holes and round sections, when there is one),
added, moved or removed by the user — split the target into **sections**: each section is
translated by its share of the move, the matter crossing a cutter is lengthened or shortened.
Options: **centred** (both ends move), **measure** as the overall dimension or as the offset;
the length is typed (VCB, expressions, [Interaction](interaction.md)); compression stops 1 mm
before two sections meet; double-click repeats the last measure. Interior handles stretch one
section and compress the others, the overall dimension kept; one whose section holds no matter
wholly (two cutters across one arc, scaled by its ends) is refused: it would move nothing.

## Target

- **Occurrences of one context, of parts or of assemblies**, stretched in the chosen axes
  ([Data model](../model/data-model.md): a part's box is the exact bounds of its shape in its
  frame; an assembly's, its parts' boxes brought into its axes).
- **Taking them**, after OCL's `SmartSelectActionHandler` (`active_selection_path` +
  `active_selection_instances`): the open part, else the selection when the tool starts
  (several ids of the active context); else the tool picks ([Select first](selection.md#select-first),
  the shared `pick::Picker`, keeping the context's occurrences: `Picker::keeping`) — a plain
  click takes one, a rectangle (left to right a window, right to left a crossing, as the Select
  tool, `select::marquee_hits`, its guides left out) and Shift held with clicks toggling
  occurrences, the set taken when Shift is released (OCL's `STATE_SELECT_MULTIPLE`). All go
  through the selection (`Command::Select`), drawn as such, as OCL fills the model's selection.
  A click on nothing, or Esc, lets it go.
- **Parts at any depth** (selection scope « Parts », [Selection scope](selection.md#selection-scope)):
  parts **of one assembly** are the target (`Operation::Stretch` takes a context path); of
  several, the status says so (`tool.stretch.hint.one_assembly`) and a click picks again.
- **Bodies**: in an open part showing its bodies, the bodies selected are stretched alone
  (`Operation::StretchBodies`), their cutters kept for the session; with none selected, the
  open part whole ([Bodies](../model/bodies.md)).
- **The axes option** (OCL's three, saved; default **entity**, as OCL's stretch preset):
  *active* — the model's axes, those of the root; *context* — the active context's axes;
  *entity* — the occurrence's own when there is one, the context's when there are several. In
  an open part: active the root's, context its assembly's, entity the part's. Only at the root
  do the three meet. Changing it goes back to the grips (OCL's `onToolActionOptionStored`).
- **The box** is common: the bounds, in the chosen axes, of every part placed under the
  occurrences — its exact box when it lies square to the axes (a part occurrence read from the
  document, there before its mesh), else its display mesh's vertices (its box's corners would
  swell it). Its axes with no thickness are not offered. The preview (`StretchPreview`) and
  `Document::stretch_bounds` / `stretch_spans` take the same context, occurrences and frame.
- **Locked for good**: the axes a turned body spans whole across its axis (`Target::fixed`,
  [Turned bodies](turned-bodies.md)).

## The operation

`Operation::Stretch { context, occurrences, frame, axis, cuts, moves, scope, cutters }`: the
stretch in the axes `frame` places in the context's (the chosen axes; `frame` rigid), the
occurrences of the context's component it applies to (none named:
`Refusal::NothingStretched`). `axis` is the axis in those axes, `cuts` the cutters' positions
along it (`axis · p`), `moves` each section's displacement (OCL's `edvs`) — a
`kernel::Stretch`. `cutters` (one occurrence) are the cutters written on its component after
the stretch (« Behavior » below). One undo step, one kernel job for every part. A part refusing
names itself (`Error::InPart`). `Command::Replace` boxes its two operations (the operation is
large). `Outcome::stretched` lists the parts changed (the part after, the part before, its
stretch). `Document::stretch_spans` gives the tool its limits (the matter wholly in each
section, whole occurrences and parts' edges), `Document::stretch_bounds` an assembly's box from
the extents.

When prepared, each solid across a cutter is stretched on the spot (a refusal refuses the
operation then, before any kernel call); the job then holds a `Task::Stretch` per solid — the
same rewrite, read back by the kernel, which checks it and meshes it — and the part's recipe
task from the new bodies. Guides follow their section when they have one (a line square to the
axis, a circle about it); a line along the axis stays, an oblique one too. The scope is that of
every change to a part ([Data model](../model/data-model.md)).

## The walk

After OCL's split and apply workers (`root_split`, `_get_edit_transformation`;
`crates/model/src/stretch.rs`). It starts at the context's component, restricted to those
occurrences, each taking the stretch through `frame⁻¹ · its transform`; the stretch is brought
into each component's axes (`Stretch::in_frame`). The context's component itself is changed in
place — as any edit in an open context. Each occurrence below is:

- **moved** when its content lies in sections that move alike (its extent along the axis,
  `Stretch::extent`, conservative for curved faces): only its placement changes, its component
  is not touched — a leg at the end of a frame;
- **stretched** otherwise: a part has its bodies stretched (below), an assembly has its
  occurrences walked in turn (its guides follow their sections).

### Anchors

Each component is stretched in its own axes, **its origin following its anchor** (after OCL):
the anchor is the section holding the end of its extent nearest its origin along the axis
(OCL's `section_def` of a `SPLIT` container: the nearest end, not the section holding the
origin — they differ only for an origin within the matter, in a middle section). The occurrence
moves as its anchor, its content by the sections' moves less the anchor's (`relative`, OCL's
`edv -= container_edv`), and below, each component relative to its own. A component changed in
place moves its occurrences the walk does not reach as much, in their own axes
(`move_unreached`, OCL's back translation of extern instances): on screen nothing differs from
no anchor, only where the part's axes end up in its matter.

### What a component becomes

What the stretch does to each of its pieces (OCL's md5 of section deltas, `ContainerDef#compute_md5`):
the move of each vertex, of each end of each edge's and face's extent (`Stretch::piece_moves`),
of its bodies, of each occurrence moved whole, of each guide — along its axis turned the
canonical way (its first component not negative, OCL's `_axis_sign`), taken from the first
piece's move. Wherever the cutters fall between the same pieces, and for a copy turned half
round or mirrored stretched alike, it is one change: occurrences taking the same keep one
component, the one made by a stretch differing by a move along the axis moved by that move (its
placement); different changes make separate components ([Data model](../model/data-model.md)).

- Under « all occurrences », the change most of the tree's occurrences take is made in place
  (shown wherever the component is), the others go to copies — unless the tree also reaches the
  component unchanged (an occurrence only moved, or anything below one): then every change goes
  to a copy and the component keeps its shape (OCL counts moved instances among the stretched
  ones, `make_unique_c`: two occurrences of one part in one assembly, the cutter across one
  only, would both change).
- **Scope « only these »** (OCL's `make_unique`): the context's path is made its own once
  (`separate`); then the occurrences that take one change share one copy (OCL's
  `make_unique_d`: one new definition per deformation, not per instance), a change goes to a
  copy whenever its component is placed elsewhere too (counted over every path of the
  document). Copies keep their ids ([Data model](../model/data-model.md)), so paths stay valid.

A **linked body** is its source's: Stretch neither moves nor stretches it and leaves it out of
its measures. It is then built again from its source ([Links](../model/bodies.md#links)).

### Blocks and refusals

- A component below the target that is locked along the axis is a block (« Behavior » below),
  never stretched. A part below the target that cannot be stretched refuses the whole stretch,
  named (`Error::InPart`).
- In the context's or the model's axes, a part turned in them that crosses a cutter has no face
  along the axis: its slanted faces are scaled (« B-rep stretch » below), its corners moved as
  their sections. A part that still cannot be (a compression flattening what it scales) refuses,
  named (`Error::InPart`).
- The preview (`stretch_sketch`, `stretch_swaps`) follows the same walk.

## Behavior: cutters and locks

After SketchUp's `Sketchup::Behavior` and OCL's `stretch_cutters` attribute. The cutters are
not the tool's state but how a component is cut when stretched: they follow its definition, as
OCL keeps them on it. With them, the axes it never stretches along. A typed `Behavior` on the
`Component` (a part or an assembly) holds both:

```rust
pub struct Behavior {
    /// The axes (its own) it never stretches along.
    pub no_stretch: StretchMask,
    /// The cutters along each of its axes, as increasing ratios of its box in ]0, 1[; none:
    /// one in the middle. A locked axis has none.
    pub cutters: Option<[Vec<f64>; 3]>,
}
```

**Why** typed rather than a generic attribute dictionary: validated when read (`is_valid`),
and the rule of each field on copy written where it means something.

- **`StretchMask(u8)`**: SketchUp's `no_scale_mask` bit layout — X = 1, Y = 2, Z = 4; bits 3–6
  (XY, XZ, YZ, uniform) reserved, unused while there is no scale tool (occurrences are never
  scaled: stretching is our scaling). Saved as an integer, mapping one-to-one on a SketchUp
  import / export. OCL's stretch reads only the full mask (127, its imported hardware); here
  each axis counts.
- **The mask is the one truth**: a locked axis has no cutters (those saved for it are ignored
  and dropped, `Behavior::normalized`). The tool's `sanitized` does not put one back in the
  middle on a locked axis.
- **Copies**: `MakeUnique`, `separate` and the walk's copies keep the behavior as it is
  (`copy_component`, `copy_from`). The target of a stretch takes the tool's cutters, set again
  after it (`kept_cutters`), through `Operation::Stretch::cutters` (one occurrence; written on
  its component as it is after the stretch, a copy maybe, its locks kept); the components below
  it keep theirs as they were — ratios of a box that changed, still valid.
- **Cutters are saved** by any gesture on them, not only by a stretch: `Operation::SetBehavior
  { component, behavior }`, pure model (no kernel). Consecutive ones on one component merge into
  one undo step (`merges_into`, as `SetSettings` and `EditMaterial`): a run of adjustments is one
  step, « Cutters », the stretch after it another. A stretch writes the new cutters in its own
  operation.
- **Locking, in the tool**: removing the last cutter of an axis (dragged out of the box, or the
  remove modifier) locks it. The locked axis is drawn grey, without grips or cutters, with a
  **closed padlock** at its end (the grip of its positive side: at the box's centre, two locked
  axes would meet); a click on it unlocks the axis, one cutter in the middle. No open padlock on
  free axes (too much on screen): removing the last cutter is the only way to lock. Both are
  `SetBehavior`, merged as above; they do not drop the arrow keys' axis lock as other
  operations do. A stretch leaving no gap between matter puts one cutter in the middle: not a
  lock.
- **Where the tool keeps them** (`Keep`): on the component when the stretch's axes map onto its
  own, each axis one of its own maybe turned over (ratios then reversed) — one occurrence, the
  axes option *entity*, or *context* / *active* with the component square to them; else for
  the session (several occurrences under one key, one occurrence turned obliquely under its
  component), never locked. Several occurrences share one set, the last used, for the session
  (OCL's `@@last_cutters_data`; the store's `None` key): it belongs to no component, and is
  never saved. The tool reads them again whenever the document's differ from what it last read
  or wrote (another target, an undo). The padlock is `PointShape::Padlock`, drawn by the app
  over the view as the grips.
- **A locked component is a block in a walk** (after OCL, which does it for the full mask, a
  locked instance, a glued or camera-facing one): below the target, an occurrence whose
  component is locked along the stretch axis — the axis brought into its own axes; for one
  turned, as soon as that axis has a component on one of its locked axes — is never stretched:
  it is **moved with the section holding its origin**, even across a cutter, and its bounds are
  added to that section's for the compression limit (OCL's `section_def.bounds.add`). Not a
  refusal. The preview follows the same rule.
- An occurrence lock (OCL's `locked?`, on the instance) is not supported.
- **File**: a `behavior` field on the component, absent when default ([File format](../io/file-format.md)).

## The tool

`crates/tools/src/tools/stretch.rs`; in the ribbon's « Remodeler » group, before the booleans
([Workspaces and ribbon](../ui/ribbon.md)), the Solids menu; shortcut S. Built on OCL's own
interaction (`SmartReshapeStretchActionHandler`: its states, `_snap_*` and `_preview_*`).

### Grips and cutters

OCL's `STATE_STRETCH_START`:

- A grip at the centre of each face of the box (not drawn), a round point drawn over
  everything and taken near it on screen, seen or not — 30 px for the active axis's
  (`GRIP_PX`), 10 for the others' (the nearer the eye first when two coincide). Hovering a grip
  makes its axis the active one (the arrow keys lock one); the active axis is dashed between
  its grips in its colour, the others in grey.
- The active axis's cutters are translucent planes across the box, 50 px wider than it, drawn
  in the scene (hidden behind faces, as OCL's `view.draw`; **why:** over everything they were
  unreadable), taken inside on screen or near their outline (`CUTTER_PX`): pressed and dragged
  along the axis, to an end of the box to remove one; the copy modifier held, the cutter to add
  follows the cursor and a click adds it; the other modifier held (Command on macOS, Alt
  elsewhere), a click removes the one under the cursor; none left, the axis is locked
  (« Behavior » above; one in the middle again where cutters are kept for the session).
- The box dotted in ink, in the scene (hidden behind faces), as the copy tools'
  ([Copy along a line, a grid, distribute](copy-array.md) « Preview »; OCL's
  `_preview_stretch_start`): one for the target, however many occurrences; none when it is one
  part (or body) that is its box exactly (`Solid::fills_its_box`, square to the axes).
- A click on a grip starts the stretch, or press, drag and release.

### Interior handles

OCL's (`_get_interior_handle_defs`): with two cutters or more on the active axis, each section
between two of them (not the end ones, which anchor) has a handle — its stretch of the line
between the grips, taken anywhere along it within 20 px (`HANDLE_PX`; after the grips, before
the cutters), drawn when hovered only: bold, an arrow at each end, over everything. It is
grabbed where an inferred point that tells a place along the axis is, projected on it (a
diamond: `PointShape::Diamond`), else at its point nearest the cursor's ray (a round point).
Shift held over it keeps it: its start is then taken anywhere, projected on its line, a dashed
line from where it is taken. Moved by `d` along the axis, the sections take OCL's
`interior_t_coefs` (`interior_shares`): 0 at the first end, rising evenly to 1 at the handle's,
falling evenly to 0 at the other end — the overall length stays, so the measure is always the
offset (typed: the way the cursor takes it), « centred » does not apply, and the compression
limit holds both ways (`move_range`). After it, the axis stays active to chain.
`Operation::Stretch` takes a move per section, so nothing else is needed below the tool.

### Stretching

OCL's `STATE_STRETCH`:

- The target drawn stretched (no cutters), its box dotted where it was — a part that is its box
  too, its edges no longer on it (a deviation from OCL, which draws none) —, the measure line
  dashed with its ends and its length — from the opposite face for the overall dimension, from the grip for the
  offset.
- The cursor within 40 px of the start snaps back to it; else an inferred point that tells a
  place along the axis — a point, an edge across it, a face facing it, not the opposite face's
  plane, not the part itself — projected on it; else the closest point of the axis to the
  cursor's ray.
- Shift holds the last measure (its value; the way the cursor's), a double-click takes it again
  — kept per kind of measure; the copy modifier tapped centres, the other one tapped switches
  the scope (OCL's "make unique"). A typed value ends it; out of range, it is said (the minimal
  length, the maximal compression). Then back to the grips on the same target, to chain.
- **The sections' moves** follow OCL's profile: from 0 at the anchored end to the full move at
  the grip, proportional to their rank; centred, from minus to plus the move. Compression stops
  1 mm before the matter wholly in two sections meets (`Stretch::section_spans`, OCL's section
  bounds).
- **Cutters after the stretch** are set again as OCL does (`StretchDef#cutter_ratios`,
  `kept_cutters`): one in the middle of each gap between the matter of two sections that hold
  some, once moved — each stays between the same vertices, however hard the compression. A run
  of empty sections is one gap, one cutter; matter overlapping across a cutter leaves none
  there; none left, one in the middle. **Why:** put back at the mean of its sections' moves, a
  cutter could end in a section's matter after a strong compression.

### Options and refusals

- **Options**: the measure — overall or offset — and centred (saved), the axes (above), and the
  scope like the booleans' when no part is open (an open part has its own).
- **Refused at the click on a grip**: the operation for a 1 mm pull is prepared; a refusal
  (something curved across a cutter) is said, the stretch does not start.
- The session store of cutters is thread-local (`CUTTERS`): the tools run on one thread, and
  each test on its own (a shared one let tests see each other's cutters, part ids being alike).

## Where the stretch applies

On the part's recipe — its bodies ([Bodies](../model/bodies.md)) —, not on its final B-rep:

- A body whose shape lies in a single section (its definition's extent in its axes) is
  **translated** (its placement): exact, nothing deformed.
- A body **across a cutter** has its definition stretched in the body's own axes — the cutters
  and the axis brought in by the inverse of its placement (a groove along the rail grows with
  it); its profile kept when it stays one ([Prism tool and profiled blanks](prism.md)), a turned
  body rebuilt from its half profile along its axis and refused across it
  (`Refusal::TurnedAcross`, [Turned bodies](turned-bodies.md)), a group's children stretched in
  turn (`stretched_def`). Bodies of one definition stretched alike share the new shape; the
  change most of them take is made in place, the others go to new definitions
  (`Part::body_stretch`, [Bodies](../model/bodies.md) « Body definitions »). If it cannot be
  stretched, the stretch is refused.
- The part is then computed again from its bodies (the recipe task, asynchronous).
- The scope ([Data model](../model/data-model.md)): all occurrences, or only this one (a
  separate part).

## B-rep stretch

Rewriting the record: a pure function on `solidcore_record::brep::Brep` (no kernel), a
**piecewise translation** along the axis, what crosses a cutter lengthened or scaled, in `kernel::Stretch` (`crates/kernel/src/stretch.rs`:
`apply`, `in_frame` — a solid in its own frame —, `extent` — along the axis: exact for lines,
arcs of circles and ellipses and ruled faces (planes, cylinders, cones, a net of one straight
segment along a parameter: a cylinder once scaled, a prism's arc side, whose net's hull can lie
far outside it), conservative for other curved faces —, `section_spans`,
`piece_moves`). An arc counts only along its own angles, not its whole circle's: a cutter
beside a concave arc, where only its circle would pass, crosses straight matter alone. The
arc is the edge's interval (the circle's angle), or, when the edge is parameterised on a NURBS
form, the side of its ends where that form's first inner control point lies.
`apply` checks the rewritten record as data (parameter ranges, references, topology), as the
kernel does before reading it: a fault is the stretch's, never the solid's, and refuses it
naming the edge or face at fault (`StretchError::Unreadable`). `Solid::stretched` then reads
the record back into the kernel, which checks its geometry; a refusal cancels the stretch.

- An element (vertex, edge, face) entirely in one section is **translated** with it: points,
  curves and surfaces with their NURBS form; pcurves and domains unchanged.
- An element crossing a cutter must be **invariant along the axis**, and is lengthened:
  - an edge: a **line parallel to the axis** — its ends follow their vertices, its interval and
    its pcurves' knots grow;
  - a face: a **plane containing the axis direction** — the surface stays, each pcurve point
    shifts by `(Δ·u, Δ·v)` of its section (planes are affine in mm), the domain is taken again
    from the pcurves; or a **cylinder whose axis is parallel** — its NURBS net (SMLib's form,
    v = the height in mm) has its rows past the cutter moved, its v knots, domain and pcurves
    grown; or a **NURBS net extruded along the axis** (same form, as the face's own surface:
    SolidCore writes a prism's arc sides — fillets included — so), stretched the same way;
  - the kernel writes a mirrored solid's lines as degree-1 splines over ±1234567 and its
    cylinders as cones of half angle 0, their NURBS nets sometimes running along u rather than
    v (the height then u): all taken as what they are; a prism's upright edges are
    degree-1 splines on their own length: a spline (or a two-point polyline) across a cutter has
    its knots and points grown with its interval, else it is no longer read back ("parameter
    interval outside the natural domain") — a filleted prism stretched along its height.
- Any other face crossing a cutter (a cylinder across the axis, a cone, a sphere, a torus, a
  NURBS face, a plane not containing the axis) is **scaled along the axis**
  ([0058](../decisions/0058-stretch-scales-curves-across-a-cutter.md), `Stretch::plan`):
  - such faces joined through an edge make a **group**, mapped by one affine map along the axis,
    `x ↦ lo + m(lo) + (x − lo) · k`, over the group's extent `[lo, hi]`, each end moved as its
    section; its edges and vertices go with it. A circle becomes an ellipse, a cylinder an
    elliptic one, a sphere an ellipsoid; a hole crossed goes oval; compression too;
  - exact on the NURBS form SolidCore keeps with every analytic curve and surface: its control
    points are mapped, its parameters kept, so the pcurves on the group's faces stand; the face
    takes the mapped NURBS as its surface, the edge says it is an ellipse (`scaled_conic`, on its
    principal axes, the second from the normal: SolidCore reads a frame square only within
    ~3e-16). A line or a net of the group is mapped as such; a plane is written again on square
    unit axes, which its parametrisation no longer has and the kernel's plane cannot hold, its
    domain and pcurves mapped by the same affine map of (u, v) (`square_plane`) — a chamfer
    across a cutter along the thickness;
  - the faces lengthened around a group have its edges' pcurves mapped in their (u, v), affine
    there too (`FaceMap::along_at`);
  - a vertex moves as its group; where that differs from its section's move, every other face
    there must be lengthened, else `StretchError::Meets` (a group meeting matter that only
    moves, two groups meeting at a vertex). A conic edge or analytic face without its NURBS
    form, an intersection curve, still refuse.
- `section_spans` leaves a group's edges out and counts its two ends in their sections: the
  compression stops before a group flattens, and the preview blends between its ends as the
  group is scaled.
- A compression bringing the matter of two sections that hold some onto each other (a hole near
  an end, compressed past it) is refused, `StretchError::Overlaps`, for any caller: what lies
  wholly in a section only moves, the two would run through each other and the record be no
  solid. The tool stops 1 mm before.
- **Checked at random** (`crates/kernel/tests/stretch_fuzz.rs`): shapes (boards, fillets, arc
  sides, holes, bores, carved arcs, cylinders, spheres, rounds, chamfers) placed anyhow,
  stretched along their own axes (axis noise included) or any, as placed or in their own frame,
  numbers drawn: each result either refused or read back, its volume moved as the moves, its mesh
  like its volume, its length grown by them, still an operand, and the opposite stretch giving
  the original back. Compressions drawn within the room between sections or past it. Seeds
  replayable, cases dumped as jobs; the failures of known families
  are counted, not failed (listed in the test).
- **What the kernel's records hold** (checked on boxes and cylinders stretched and compressed,
  read back, then used as a blank and as a tool in a subtraction): the box's planes carry no
  NURBS form and linear pcurves in mm; its lines run t ∈ [0, 1] from start to end, a
  cylinder's seam is parametrised in mm; a compressed cylinder needs its domain shrunk with its
  knots ("parameter interval outside the natural domain" otherwise).

## Preview

No kernel while dragging.

- The tool describes the stretch (`Preview::stretch`, a `StretchPreview`: the occurrences, the
  scope, the axis, cuts and moves); the editor deforms the part's display mesh on the CPU
  (`scene::stretched_mesh`) and keeps it in the scene under a key of its own (`PREVIEW_KEY`, a
  new revision at each change: the app uploads it like any mesh).
- The map is continuous along the axis: each section's move where it holds matter wholly, a
  linear blend in between — the B-rep's vertices all lie in that matter, the mesh's other points
  on what crosses a cutter, which any increasing map keeps. A scaled group's ends count as its
  sections' matter. Face by face (`Stretch::mesh_moves`, the mesh's faces and edges by their
  record index): a face scaled moves by its group's map, a face in one section by its section's,
  a face lengthened by the blend but its points on a group's edge by that group's map — so
  groups overlapping along the axis (a tube's two circles) each keep their own. Only the
  triangles the blend moves are cut: one with a corner on a group's edge keeps its corners'
  own moves, its inner points would otherwise take the blend's and stand out of the shape. The triangles are first cut where
  the map bends, so that it is affine on each piece: none turns over, however hard the
  compression (moving each vertex by its own section turns long triangles over, as does the
  continuous map on uncut ones — test `a_compressed_preview_turns_no_triangle_over`).
- For an assembly, the editor sketches the document the stretch would make without its geometry
  (`Document::stretch_sketch`: the same walk, parts left as they are): each part it stretches
  gets a preview mesh (`scene::preview_key(i)`, the first `PREVIEW_KEY`), and each occurrence it
  changes — stretched, moved, copied — is drawn from its mesh or at its new place
  (`Editor::stretch_swaps`, by path: ids are kept), its highlights moved with it. Those
  occurrences are not inferred on while stretching (`Cx::previewed`): they are no longer where
  they were. Bodies are drawn stretched from their recipe meshes (`Editor::body_swaps`,
  [Bodies](../model/bodies.md)).
- An open part showing its machinings is drawn as its shape, stretched, during the gesture.
- The grips and the measure's ends are drawn by the app over the view (`Drawing::grips`); the
  cutter planes and their outlines in the scene, depth tested (`Drawing::fills`, blended without
  writing depth, and depth-tested lines; [Drawing helpers](../render/drawer.md)).
- Once applied, the stretch stays drawn as previewed while the kernel computes it (`Editor::held`,
  the tool's grips hidden meanwhile): else the part shows as it was
  for a frame or more before its new shape comes. Test
  `a_stretch_stays_drawn_stretched_while_computed`.

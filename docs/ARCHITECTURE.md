# SolidSketch — architecture

A prototype of a solid-based 3D CAD tool, 100 % Rust, native and WebAssembly. Geometry engine:
[SolidCore](https://github.com/tangency-labs/solidcore) (exact B-rep, a Rust port of SMLib), the
one behind [SolidCraft](https://github.com/tangency-labs/solidcraft). Modelling close to
SketchUp, designed for woodworking: **parts** (machined B-reps) and **assemblies**, both
**components**, in a tree. Long-term goal: a cut list and a cutting diagram like
[OpenCutList](https://github.com/lairdubois/lairdubois-opencutlist-sketchup-extension).

Decisions made on 2026-11-09, and on 2026-11-10 for the parts / assemblies model (§2),
**partly implemented**: what is left is listed in §2.8 and in `docs/REVIEW.md`.

## 1. Paradigm: direct modelling, open to parametric

- Modelling is **direct**: push/pull, move, booleans applied at once. The B-rep *is* the
  state; there is no rebuild history.
- To keep the door open to parametric modelling, **every change to the document goes through
  a typed, serialisable `Operation`** (`AddPrimitive`, `Transform`, `Boolean`, `PushPull`,
  `MakeAssembly`, …), never through direct mutation. Today operations feed undo and a log;
  tomorrow a timeline could replay them (the blocker then being persistent face naming).

## 2. Data model: parts, assemblies, machinings

Decided on 2026-11-10, in place since the same day (except §2.8); the former model is summed
up in §2.7.

```
Document
├─ components : Id → Arc<Component>
├─ root       : Id of the root assembly (the model, never counted itself)
└─ next_id

Component { id, name, props, kind }
  props   : type (solid wood, panel, hardware…), excluded from the cut list, …
  kind    = Part     { blank: Arc<Brep>, machinings: Vec<Machining> }      — a part
          | Assembly { occurrences: Vec<Occurrence> }                      — an assembly
Occurrence { id, component: Id, transform: T, material, name }            — an occurrence
Machining  { id, tool: Arc<Brep>, placement: T, source: Local }            — a machining
            (later: source = Linked { … }, §2.4)
```

### 2.1 Components: parts and assemblies

- **A single sharing mechanism**: a component is a shared, named definition, placed by
  occurrences. A **part** holds a B-rep (its blank and its machinings); an **assembly** holds
  occurrences of other components. "Component" is the common term, which the interface seldom
  uses: it speaks of parts and assemblies.
- **No more groups**, no more loose geometry: everything in the model is an occurrence.
  Drawing a box creates a part and its first occurrence; gathering occurrences creates an
  assembly (named "Assembly N" until the user renames it).
- A part is edited **directly** (no "entering" it); an assembly is opened like a SketchUp
  component (active context, §2.6).
- **Properties** (type, exclusion from the cut list…) go on any component: a hinge modelled in
  several parts is an assembly of type hardware, counted as one item without detailing its
  content.
- **Name**: the component's name is the part's name; the **occurrence name** names one
  occurrence of that part ("Left", "Right"), as in OpenCutList.
- **Purge**: a part with no occurrence disappears, unless the user named it (it then stays in
  the model's library).

### 2.2 Occurrences and transformation

- An occurrence carries **T = translation + rotation + optional mirror** (determinant ±1), its
  **material** and its occurrence name. Moving or rotating only changes T: instant, no kernel,
  no remeshing, no drift of the geometry.
- **T is never baked into the B-rep.** Booleans and (later) push/pull work in the part's frame,
  the other operand brought in by the relative transformation; export applies T on the fly.
- **No scale in T.** Scaling (uniformly) changes the dimensions, hence the part: it is a
  geometry change (§2.5). **Non-uniform scaling is forbidden** everywhere: it breaks circles,
  inferences and dimensions.
- The **mirror stays in T**: a part and its mirror image remain the same part (the cut list
  tells them apart, unless the part is declared symmetric). Checked on 2026-11-10
  (`crates/kernel/tests/part_model.rs`): SolidCore applies a mirror, alone or with a rotation,
  to every geometry (planes, cylinders, spheres, cones, tori, intersection curves), flips the
  faces correctly, and the mirrored solid then serves as target and as tool alike; cutting then
  mirroring = mirroring then cutting.

### 2.3 A part's axes

- The B-rep's frame **is** the part's: the length runs along the red axis, and **the grain is
  always the length** (no separate property). The cut-list dimensions are read on the blank's
  bounding box in this frame.
- Tools working on a part use its axes; elsewhere, the context's.
- Changing a part's axes = re-expressing its B-rep in another frame and compensating the T of
  each of its occurrences.

### 2.4 Machinings

- A part is **a recipe**: **blank** − (tool₁ ∪ tool₂ ∪ …). The **final B-rep** is computed by
  the kernel (asynchronous, §7 ter) and cached, identified by the recipe; it is never the
  source.
- **Always material removal**: the order of machinings does not matter (a set, for the user),
  the computation can be incremental, and a failing machining is isolated and reported without
  blocking the others.
- **Computation: successive, incremental subtractions.** Measured (board − n holes, dev build):
  joining the tools first then cutting once is not faster (50 holes: ~0.4–0.8 s either way).
  Adding a machining = one subtraction on the cached final; removing one = starting again from
  the blank (or from an intermediate final kept in cache).
- **Measured failures**: a tool outside the blank, flush with a face, touching an edge or
  tangent does nothing abnormal; a tool that swallows the whole blank gives a conversion error
  (to be reported as "empty result").
- Low level: the tool is **any B-rep**, placed in the part's frame, anchored at its origin.
  **Machining types** (hole, groove, mortise…) will come on top and generate that tool from
  their parameters.
- **Links, later**: each machining has a stable id and a `source`. A fitted piece of hardware
  could carry its tools and produce *linked* machinings in the part, placed by
  `T_part⁻¹ · T_hardware · T_tool`, recomputed when it moves. The format has the field from now
  on (always `Local` for the time being).

### 2.5 Copies and propagation

- **Copying an occurrence creates an occurrence of the same part**: "Leg ×4", counted without
  guessing anything, without thinking of "making a component" before copying.
- A **change to the part** (geometry, machinings, properties) affects **all its occurrences**
  or **only this one**: the **scope**, an option of the tool (option bar, §6 bis), "all
  occurrences" by default, remembered per tool. Decided on 2026-11-10, after the solid
  operations of Smart Reshape (OpenCutList, `common_solid_boolean_apply_worker.rb`):
  - **all occurrences** (OCL's `make_unique: false`): the part is changed in place, and all its
    occurrences — including those seen through copies of an assembly — show the result. The
    operation is computed in the frame of the clicked occurrence;
  - **only this one** (`make_unique`): only the clicked object changes. Along its path, top
    down, each assembly shown elsewhere becomes a separate assembly, then the part becomes a
    separate part; the other occurrences are never touched;
  - when an operation has **several targets** (later): occurrences of one part that the
    operation leaves identical in their own frame keep a single part; if they diverge, they
    become separate parts even under "all occurrences", since the part cannot hold two results
    (OCL's rule);
  - **locks** (later): a locked occurrence is never changed by propagation, as in OCL.
- **Propagation must be visible**: the number of occurrences affected in the option bar ("All
  occurrences ×4"), the affected occurrences highlighted during a change. "Make unique" becomes
  **"Make a separate part"**, with a deep version for an assembly.
- The **cut list** may merge parts with the same dimensions and material (a display option);
  the model, the cutting diagram and the exports always keep them apart (same dimensions does
  not mean same shape).

### 2.6 Ids, paths, context

- **Stable `u64` ids**, drawn from one counter, for components, occurrences and machinings.
  Those of components and machinings are unique in the document; those of **occurrences** are
  unique **within their assembly**: a copy of an assembly ("make a separate part", scope "only
  this one") keeps the ids of its occurrences, so that paths, the context and the selection
  stay valid. A path is always resolved from the root. An operation designates a face by
  `(part id, face index in the final B-rep)` — enough in direct modelling, since the operation
  applies at once.
- **Occurrence path**: an entity of the scene is designated by the path of occurrences from
  the root; its world transformation is the product of the T along the path.
- **Active context**: the assembly being edited, and the path leading to it.

### 2.7 Former model (version 1 files)

```
Definition { id, name, kind: Root | Group | Component, entities: Vec<Entity> }
Entity      = Solid    { id, name, brep: Arc<Brep> }             — in its definition's coordinates
            | Instance { id, name, definition: Id, transform }
```

Loose solids in any definition, with no transformation of their own (moving them rewrote their
B-rep); groups = definitions with a single instance. When a v1 file is opened (`file.rs`,
module `v1`): each definition becomes an assembly (named for a component, not for a group),
each instance an occurrence, and each loose solid a part — its axes those of its definition,
so nothing moves and the kernel is not called — with its occurrence, which keeps the solid's
id.

### 2.8 Not in place yet

- **Recipe recomputed**: today a subtraction cuts the cached final and the machining is added
  to the list; there is no removing, editing or "integrating" a machining yet, hence no
  recomputation from the blank.
- **"All occurrences / only this one"** and the automatic separate part: a geometry change
  affects all the part's occurrences today.
- **Material per occurrence, properties** (type, exclusion from the cut list): not in the
  format yet; they will enter it with a default value (v2 files will stay readable).
- **Tools in the part's axes** (§2.3), highlighting of occurrences during a change, list of
  machinings in the interface.
- "Make a separate part" on an assembly copies only its first level (not in depth).

## 3. Storing solids: the SolidCore record

- A solid is a **`solidcore_record::brep::Brep`**: plain data, `Clone` + `serde`, shared by
  `Arc`. The kernel (`solidcore::Kernel` + `solidcore::Brep`) is instantiated only for the
  duration of an operation: record → kernel → operation → record.
- After each operation, the record is **squared up** (`square_frames`, `engine.rs`): a
  rotation about an oblique axis leaves plane axes ~1e-15 off a right angle, which SolidCore
  then refuses to read back (tolerance ~3e-16) — the next operation used to fail.
- Benefits: nearly free undo snapshots, direct saving, no kernel state to maintain. Cost: one
  conversion per operation. If it becomes a bottleneck, a lazy cache of kernel bodies will be
  added, invalidated on every change.

## 4. Booleans across contexts

- The **target is changed in its own frame**: the tool is brought into the target part's frame
  by the relative transformation `inv(world(target)) · world(tool)`.
- **Subtract** (model of §2): the tool occurrence becomes a **machining** of the target part,
  without destroying its geometry; it leaves the model, the other occurrences of its part stay.
  **Integrate** applies a machining to the blank and removes it from the list.
- **Union and intersection** are destructive: a new blank, made of the final shapes, into which
  the machinings of both parts are integrated.
- The change affects all occurrences of the target part or only this one (§2.5).
- No "keep the tool" option: the tool always leaves the model (it lives in the machining).

## 5. Undo / redo: shared snapshots

- The document is **immutable piece by piece** (`Arc` on definitions and B-reps). Applying an
  operation produces a new `Document` that shares everything that did not change.
- Undo = going back to the previous document; redo = going forward. No inverse to write.
- The log of applied operations is kept alongside (for future parametric modelling and for
  debugging).

## 6. Interaction: SketchUp-style tools

- An **active tool** (Select, Box, Cylinder, Move, Rotate, Push/pull, Orbit…) receives the
  viewport's events (hover, click, drag, keys) and produces, at the end, an `Operation`. During
  the interaction it exposes a **preview** (ghost geometry) without touching the document.
- **SketchUp navigation** (decided on 2026-11-09): middle button = orbit around the point
  aimed at, Shift+middle = pan, wheel = zoom towards the cursor, middle double-click = recentre;
  trackpad: two fingers = orbit, Shift+two fingers = pan, pinch = zoom. Z stays vertical.
  Perspective projection by default, switch to parallel, standard views (top, front, right,
  back, left, iso) and zoom extents.
- **SketchUp selection**: a click selects the top-level entity of the active context,
  Shift+click adds/removes, double-click on a group/component enters it, double-click in empty
  space leaves it, Esc cancels the tool then clears the selection. Dragging draws a selection
  rectangle: left to right (solid line, "window"), it takes the entities it fully contains;
  right to left (dashed, "crossing"), those it touches; Shift adds/removes. The test is done on
  screen, on the projected vertices and triangles of the display meshes. Outside the active
  context, the model is faded.
- **Booleans = a two-click tool** (target, then tool), which designates solids of any context:
  this is how the rule of §4 is expressed.
- **Typed dimensions** (VCB; decided on 2026-11-10, taken from OpenCutList, `UserTextHelper` +
  `LengthExpressionUtils`): during a tool, typing a value sets the dimension. One can type:
  - expressions: `600-2*19`, `2m*(2+3)`, `min(@; 300)`, `floor(@ - 5; 12; 15; 20)`;
  - `@`, the gesture's current value: `@/2`, `@+19`;
  - lists separated by `;` (empty slot = current value: `;400`); `=` repeats a value (`600=`
    = `600;600`);
  - a shape's height right with the first input, optional: `600;400;19`, `50==` (a cube),
    `radius;height` for the cylinder; the shape is then done;
  - points: `[x;y;z]` in the context's axes, `<dx;dy;dz>` from the start point (Move).

  A bare number is in the document's unit: the computation is done in it, so `@*3` and `@-2`
  both read. Units are checked strictly (`2m*3m` and `@/@` are refused). A typed length keeps
  the direction of the gesture. `,` and `.` are both decimal separators, so `;` is the only
  list separator, and it only splits outside parentheses. The evaluator lives in `model`
  (`expr`, `measure`), to serve the fields too (Position in the entity info) and later the
  parts' dimensions. Each tool declares what it takes (`Tool::vcb`: lengths, angle, number of
  copies, whether a point is accepted) with its current values; the editor reads the input and
  passes it a `ToolInput::Value`, or shows the error.
- **SketchUp copies and arrays** (Move, Rotate; decided on 2026-11-09): a short tap of the copy
  modifier (Option on macOS, Ctrl elsewhere), alone, toggles copying, before or during the
  gesture; the cursor carries a "+" and copying drops once done. Right after a copy, typing
  `5x` (or `*5`) makes it an array of 5 with the same step, `/5` spreads 5 copies over the same
  way; one can retype to correct. The array is an operation (`Operation::Array`: `count`
  copies, the k-th transformed k times by `step`), which replaces the copy in the history
  (`Command::Replace`): a single undo step. Translation as well as rotation (circular array).
- Inferences: see §6 ter.

## 6 bis. Application interface

Decided on 2026-11-09.

- **Layout**: title bar at the top (§7 quinquies), status bar at the bottom with the measurements box (VCB) on the
  right; in between, the **3D view takes the whole width** and the panels **float over it**
  (anchored cards): **horizontal tool bar centered at the top** (never over the side panel),
  the tool option bar centered as its footer; on the left, **Entity info** above the **Outliner**,
  collapsible (layout changed on 2026-11-10). Opening or resizing a panel therefore never resizes the view; zoom
  extents frames the area left free.
- **Original SVG icons** (drawn for the project), white and tinted by the theme, loaded by
  `egui_extras` (`svg`): no build step, no binary asset.
- **SketchUp shortcuts**: Space = Select, M = Move, Q = Rotate, G = Make assembly, Delete =
  Erase, Cmd/Ctrl+Z / Shift+Cmd+Z = Undo / Redo, Shift+Z = Zoom extents; ours: R = Box,
  C = Cylinder, B = Paint, Cmd/Ctrl+K = command palette, Cmd/Ctrl+1…6 = standard views,
  Cmd/Ctrl+, = Preferences. They all come from the command registry (§7 quater) and can be
  changed in the preferences. Once a digit is typed
  (or `@ ( [ < + - * / ;`), keystrokes go to the VCB, confirmed by Enter. Menus and tooltips
  write shortcuts as the platform does (§6 quater).
- The viewport is the image of `render`'s off-screen target, registered as an egui texture;
  rendering is submitted during `update`, before egui paints.
- Wheel ("line" units) = zoom; trackpad scroll ("point" units) = orbit, Shift = pan; pinch =
  zoom. Copy modifier: Option on macOS, Ctrl elsewhere (short tap = toggle, §6).
- **Two levels of settings** (decided on 2026-11-09, after SketchUp's styles):
  - **document settings** (`model::Settings`): display (grid — hidden by default —, axes,
    background colour) and units (length unit, decimals). Saved in the file; changing them is
    an operation (`Operation::SetSettings`), hence undoable, and successive changes (dragging a
    colour) merge into a single step. Lengths stay stored in mm: the unit only serves for input
    (a bare number is in the document's unit) and display;
  - **user preferences** (`apps/solidsketch/src/prefs.rs`): language (§6 quater), navigation
    (orbit speed and direction, mouse and trackpad separately; zoom speed, direction and
    target), the tools' settings and the settings of new models. A versioned JSON file of our
    own, `preferences.json` in the OS's configuration folder
    (`~/Library/Application Support/SolidSketch/` on macOS), `localStorage` in the browser;
    rewritten as soon as a preference changes.
  - `#[serde(default)]` everywhere: an older or partial file loads, a new setting needs no
    migration.
  - **Preferences** window (Cmd/Ctrl+,): *General* tab (language), *Model* tab (document
    settings, with "Use for new models" / "Restore the default settings") and *Navigation* tab.
    *View › Grid / Axes* changes the document too.
- **Tool option bar** (decided on 2026-11-10, after OCL's tools): the footer of the tool
  bar's card (at first a fixed strip under the title bar, moved there the same day), with the
  tool's name and its options — exclusive choices and toggles. Every choice is visible and clickable there; a shortcut is only a
  complement (Tab cycles the scope). The status bar keeps the guidance for the current step.
  - **Tools describe their options, the app draws them**: `Tool::options` returns a list of
    `ToolOption` (key, label — which may depend on what is hovered, "×4" —, tooltip, value,
    choices), `Tool::set_option` receives a change. `tools` stays without an interface: a test
    changes an option like a click does.
  - **Two kinds of options**: **settings** (the scope…), remembered per tool in the user
    preferences (`tool_options`), read by the tool from its `Cx`; and the **gesture's state**
    (copy, for Move and Rotate), which the tool holds itself and which is not remembered.
  - First options: the booleans' scope (all occurrences / this one); copy (Move, Rotate).
    Later: axes (model / part, §2.3), keep a boolean's tool (OCL's `keep_cuts`), options of
    push/pull and of machinings.

## 6 ter. Inferences

Decided on 2026-11-09.

- **A central engine** (`tools/src/infer.rs`, no UI, no GPU) replaces the point choices of each
  tool (`base_point`, `target`, `center_at`…). A tool asks
  `cx.infer(px, &Query { from, lock, exclude })` and gets an `Inference { point, kind, guides,
  path }`: world point, kind of inference, guide lines and the supporting solid if any.
- **Exact geometry**: candidates come from the **B-rep record** (`f64` vertices, typed curves:
  exact midpoint of a line, exact centre of a circle), not from the mesh. A `SnapIndex` per
  solid (vertices, edges and their curve, bounding box) is cached per `Arc<Solid>` like the
  meshes; it is transformed by each placement. Fallback on the mesher's polyline for splines
  and intersection curves (`Mesh::edges` must keep the record's edge index).
- **Priorities** (at equal priority, the closest to the cursor on screen, tolerance ≈ 10 px):
  endpoint > midpoint / centre / origin > intersection (linear constraint × edge or face) >
  linear (red / green / blue axes from `from`, parallel / perpendicular to an edge) > on edge >
  on face (projected exactly if the face is planar) > ground plane or plane through `from`.
- **Occlusion**: a vertex or an edge behind the surface the ray hits is not a candidate (up to
  a tolerance); the edges of the hovered face stay reachable. Inference covers the whole
  visible scene, including outside the active context.
- **Locks**, held by the editor for every tool: arrows → / ← / ↑ = red / green / blue axis, ↓ =
  free; holding Shift locks the current linear inference. A point hovered during a lock is
  projected onto the line (alignment). Locks drop on every applied operation, on Esc, on a
  change of tool and on undo. Depending on the tool, the axis lock gives the direction (Move),
  the rotation axis (Rotate) or the normal of the drawing plane (Box, Cylinder: ↑ horizontal,
  → perpendicular to red, ← to green).
- **Lines**: through the tool's start point, the context's axes and, for the straight edge
  hovered last, parallel and perpendicular (in the tool's plane). A line almost aligned with
  the view direction is ignored (it is only a point on screen). If it pierces the face under
  the cursor near it, that point is taken.
- **Visual feedback**: guide lines (axis colour) through the existing overlay; markers coloured
  by kind and a tooltip ("Endpoint", "Midpoint", "On red axis"…: §6 quater) painted by egui over
  the viewport (`camera.project`): `render` does not change.
- **Drawing plane**: Box and Cylinder are drawn in the plane of the face hovered at the first
  click (height along its normal), like SketchUp; otherwise on the context's ground.
  `Operation::AddPrimitive` takes a rigid frame for that (`placement: DAffine3`, identity by
  default in serde to read existing files back); the primitive is built axis-aligned then
  transformed.
- **Performance**: brute force with culling by bounding boxes against the picking cone; a BVH
  only if measurements call for it.
- **Steps**: (1) points — endpoint, midpoint, centre, on edge, on face, ground — with markers,
  tooltip and wiring into Box, Cylinder, Move, Rotate; (2) linear — axes from `from`, parallel /
  perpendicular, Shift lock, arrows shared. **Delivered together.** Later: (3) point tracking
  (temporary references, intersection of two constraints), tangent to arcs.
- Tests without display, fixed camera: each kind, priorities, occlusion, inference inside a
  transformed component, scene 100 km from the origin.
- **Status (2026-11-09)**: steps 1 and 2 done — `kernel::Snaps` (built with the mesh by
  `Solid::display()`, cached per solid in the scene), `tools::infer`, lines and locks, markers
  and tooltip in the app, wiring into Box, Cylinder, Move, Rotate; Box and Cylinder oriented on
  the face (`AddPrimitive::placement`). Step 3 is left: point tracking, intersection of a line
  with an edge, tangent to arcs. **Seams** (an edge between a face and itself, or between two
  faces of one surface) are recognised on the record: the mesher does not fill `seams`, so a
  cylinder's seam was drawn until then.

### Step 3 — relative inferences (decided on 2026-11-09)

- **Point tracking**: a point (endpoint, midpoint, centre, origin) hovered for **about 0.5 s**
  becomes a **temporary reference**, with no click or key, as in SketchUp. The app passes the
  time to the editor with each hover. **Two** are kept, the most recent; each stays marked on
  screen. They drop when an operation is applied, on Esc and on a change of tool.
- **Lines from the references**: besides those from the start point, the engine infers the
  red / green / blue axes through each reference ("From point, on red axis", a dashed guide
  from the reference).
- **Intersections** (rank "constructed point", right after midpoints and centres): line × line
  (two axes from different sources) and line × edge. Two 3D lines seldom meet, so the
  intersection is accepted when their minimal distance is below a fraction of a pixel brought
  to depth; the point kept is taken on the strongest line (an axis), to stay exactly on it.
  Line × face: already done.
- **Tangent to arcs**: a point on an arc gives its exact tangent as the reference edge; hence
  "Tangent to arc" (parallel to the tangent through the start point) and "Perpendicular to
  arc" (towards the centre).
- **Progressive delivery**, each part tested before the next: (a) tangent to arcs, (b) line ×
  edge, (c) point tracking and line × line.
- **Status**: (a) done — a point on an arc (or its midpoint) becomes the reference
  `EdgeRef::Arc { tangent, radial }` at the exact point; a straight edge, `EdgeRef::Straight`.
  A tangent or a radius that coincides with an axis is announced as the axis.
  (b) done — the linear kinds are grouped in `LineKind` (axis, parallel, perpendicular,
  tangent, radius): `InferKind::OnLine(l)` on a line, `InferKind::OnEdgeAndLine(l)` where it
  crosses an edge (segment, arc — in its plane or through it —, polyline), within half a pixel
  brought to depth; the point is taken on the line. Under a lock, the edge crossed near the
  cursor gives that crossing before any projection. Marker: the edge's square, in the line's
  colour; tooltip "Edge × red axis"…
  (c) done — the editor gets the time from the app (`Editor::tick`, called every frame; it asks
  for a repaint while a point is waiting); a point of the geometry (endpoint, midpoint, centre,
  origin) hovered for `TRACK_AFTER` = 0.5 s is tracked, `MAX_TRACKED` = 2. Its axes are
  `LineKind::TrackedAxis(k)` lines ("From point, on red axis"); two lines of different origins
  crossing within half a pixel give `InferKind::OnTwoLines(a, b)`, the point taken on the first
  (the start point's, if any). Strict 3D crossing: two lines at different heights do not cross,
  even when they overlap on screen. The lock (`Lock`) remembers its line's origin: Shift also
  holds a line from a tracked point. Tracked points are marked with a ring; the guides from
  them are dashed. They drop with the locks.

## 6 quater. Texts and languages

Every text the user sees is in a YAML catalog, one file per locale in `/locales` (`fr.yml`
is the reference; English comes next). The `solidsketch-i18n` crate compiles them in with
rust-i18n and is the only way to them: `t!("menu.file.open")`, `t!("status.save_failed",
error = e)`. The code holds no user-visible string.

- **Keys** are paths that follow where the text shows: `menu.*`, `toolbar.*`, `option_bar.*`,
  `prefs.*`, `panel.entity_info.*`, `panel.outliner.*`, `status.*`, then per tool
  `tool.<tool>.{name, hint.*, vcb.*, option.*, say.*}`, `infer.*`, `operation.*` (Undo /
  Redo), `unit.*`, `default_name.*` and `error.{expr, model, file, kernel, worker,
  preferences}.*`. A text shown in two places has two keys — which may share their words
  through `common`.
- **Shared terms**: `common.*` holds generic terms (Undo, Erase, Copy, Grid…), named by meaning
  (`common.undo`, not after the French « Annuler », which is also a dialog's Cancel). A text
  takes one in with **`$t(key)`** — `menu.edit.undo_op: "$t(common.undo) %{op}"` — resolved at
  lookup (`solidsketch_i18n::resolve`), nested references too, before the placeholders are
  filled. Any key can be referred to; a term is taken as is (no change of case), so one used
  inside a sentence is written in the sentence. Each locale chooses its own references.
- **Values** are named, `%{name}`, and few: a sentence is one key, but a label built from
  parts stays built by the code ("Edge × red axis" is `infer.edge`, "×", `infer.line.axis`).
- **No ellipsis** (…) in a catalog: the code adds it (`solidsketch_i18n::ellipsis`), the same
  for every language — "Open…" is `menu.file.open` + `…`. A sentence it would cut in the middle
  is two keys (`status.computing`, `status.computing_cancel`). A test checks it.
- **Plurals**: a key and its `_plural` form (OpenCutList's convention), picked by `tp!(key,
  count)` with the locale's rule (French: 0 and 1 singular). Languages with more forms would
  need more.
- **Errors** are typed (`model::Error`, `Refusal`, `FileError`, `ExprError`, `kernel::Error`):
  their `Display` looks up one key per kind. SolidCore's own messages stay in English inside.
- **Platform**: hints and shortcuts name keys as the platform does (Option / Ctrl, `⇧⌘Z` /
  `Ctrl+Shift+Z`); the app tells `solidsketch-i18n` whether it runs on a Mac (`ctx.os()`, right
  in the browser too).
- **Numbers** are written with the locale's decimal separator (`locale.decimal_separator`);
  typed values read `.` and `,` alike and always separate a list with `;`, in any language.
- **Locale**: in the preferences (General › Language), applied at once — egui draws every frame
  and nothing caches a text. Texts already in the status bar stay as they were. egui ids never
  come from a text (fixed `id` / `id_salt`), so a window or a section keeps its state across a
  change of language.
- **Checked by the tests** (`crates/i18n`): every key the sources use is in `fr.yml` with the
  placeholders the call fills (references resolved), every key there is used by the code or by
  a reference, every `$t(…)` leads to a key of its locale without a cycle, and every other
  locale has the same keys and placeholders. Keys are therefore always literals.

## 7. Crates

```
                   ┌──────────────┐
                   │    kernel    │  SolidCore behind a safe API
                   └──────┬───────┘
              ┌───────────┴───────────┐
       ┌──────┴──────┐          ┌─────┴──────┐
       │    model    │          │   render   │  wgpu, no egui
       └──────┬──────┘          └─────┬──────┘
       ┌──────┴──────┐                │
       │    tools    │                │
       └──────┬──────┘                │
              └──────────┬────────────┘
                  ┌──────┴──────┐
                  │ app (egui)  │  wiring, panels, viewport
                  └─────────────┘
        (i18n: under every crate but render)
```

| Crate | Role | Depends on |
|---|---|---|
| `crates/i18n` — `solidsketch-i18n` | The texts (§6 quater): the `/locales` catalogs compiled in, `t!` / `tp!`, the current locale, plurals, decimal separator, platform key names. | `rust-i18n` |
| `crates/theme` — `solidsketch-theme` | The colour tokens (§7 quater) of the viewport and the interface, light and dark themes, sRGB ↔ linear. No UI toolkit. | `serde` |
| `crates/kernel` — `solidsketch-kernel` | `Solid` (record), primitives, booleans, transformation, face offset, display mesh (`Mesh`: positions, normals, indices, B-rep face per triangle, edges). Runs each call on a fresh `Kernel` and catches its panics. | `solidcore`, `solidcore-record`, `solidcore-convert`, `solidcore-tess`, `glam`, `i18n` |
| `crates/model` — `solidsketch-model` | `Document`, components, occurrences, ids, paths, `Operation` + `apply`, undo history, versioned JSON file, reading typed values (expressions, `@`, lists, points). No UI dependency; testable without display. | `kernel`, `i18n`, `serde` |
| `crates/render` — `solidsketch-render` | wgpu pipelines (shaded faces, edges, grid, axes, highlight, previews), GPU cache of meshes per part, drawing of a list of `DrawItem { mesh, transform, style }` with a view-projection matrix. Knows neither egui nor the document. Usable off screen. | `kernel` (for `Mesh`), `wgpu`, `glam` |
| `crates/tools` — `solidsketch-tools` | Camera (orbit/pan/zoom, ray from the screen), display scene (the document flattened into placed solids + mesh cache), picking by ray (CPU, triangle → B-rep face), the tools' state machines, typed dimensions, inferences. Testable without UI or GPU. | `model`, `kernel`, `i18n`, `glam` |
| `apps/solidsketch` | eframe: tool bar, outliner, entity info, status bar + VCB, viewport (`render`'s off-screen target shown as an egui texture), kernel service (native thread or Web Worker), translation of egui events into tool events, opening/saving. Native and wasm. | all of the above, `eframe`, `egui-wgpu` |

## 7 bis. Display: the definition as the rendering unit

Decided on 2026-11-09. A component is an instanced mesh: the display follows the model.

**With the model of §2** (in place): only a part has geometry (its final B-rep), an assembly
has none. The rendering unit is therefore **a part's final B-rep** — one buffer per part, drawn
by instancing with the matrices of all its occurrences. Changing a part sends only its own
triangles to the GPU (the end of risk 2 of the review). Below, "a definition" thus reads "a
part". **Mirror**: a mirrored occurrence flips the winding of the triangles; the sign of the
determinant is passed to the shader (`n0.w`), which derives the front faces from it (test
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
- **Highlight**: a selected solid is selected along a path (a single occurrence); it is drawn
  again on top with its index range and that occurrence's matrix. A selected **assembly** also
  shows the bounding box of its content, in its own axes, dashed in the selection's colour and
  depth-tested (`View::selection_boxes`); it follows the move tool's ghost. The assembly being
  edited shows the same box in grey, grown by a few pixels (`View::context_box`).
- **`render` does not know the document**: it receives meshes by key (the definition's id) with
  a revision, and a list of batches `(key, matrices)` + highlights + preview lines. It is
  `tools` (the display scene) that flattens the document and decides the revisions.
- `render` draws into **its own off-screen target** (MSAA ×4 colour + depth, resolved into a
  texture): the app shows it as an egui image. Rendering is thus independent of egui and
  testable without a window.
- **Precision** (the GPU only has `f32`; neither WebGPU, WebGL2 nor Metal offer `f64`):
  - **camera-relative rendering**: view, placements and lines are in `f64`; each
    `view × world × recentring` is computed in `f64` on the CPU and only then converted to
    `f32` (the instance buffer is rewritten every frame anyway); the shader only applies the
    projection;
  - **recentred meshes**: the kernel's `Mesh` is in `f32` relative to its centre (`origin` in
    `f64`), and a definition's GPU mesh is relative to the centre of its solids;
  - **reversed depth** (near → 1, far → 0) in a `Depth32Float`: nearly uniform precision; the
    projection given must be reversed (`Mat4::perspective_infinite_reverse_rh`).
  - Checked by a test: the same scene 100 km from the origin gives the same image.
- **Wide lines, in points**: hardware lines are one pixel wide (half a point on a high-density
  screen). Edges, axes, grid and tool previews are therefore quads (two triangles per segment)
  widened on screen by the vertex shader, after clipping the segment in front of the eye. Widths
  are in points (`EDGE_WIDTH` 1.5; `LINE_WIDTH` 1.5; `GRID_WIDTH` 1) and multiplied by the
  screen's pixel density (`Frame::pixels_per_point`): the same apparent thickness everywhere.
  The quad has the depth of its centre line, while a pixel away a neighbouring face can be
  closer (inner corners, grazing faces): lines are therefore **pulled towards the eye** by 3
  half-widths measured at their depth (scaling towards the eye in perspective, translation along
  the view axis in parallel: the pixel does not move). Test: the edges of a pocket are not
  buried. WebGPU has no line width (Metal has none at all).

## 7 ter. Asynchronous kernel

Decided on 2026-11-09 (questions/answers). The kernel no longer runs on the UI thread: a heavy
boolean no longer freezes the interface and, in the browser, a kernel panic (`panic = abort` in
wasm) only kills the worker.

- **Pure kernel tasks.** An `Operation` applies in two steps:
  1. `Document::prepare` (UI thread) runs the operation with a *recorder* in place of the
     kernel: it validates everything (ids, cycles…), gathers the kernel calls into a `Job`
     (`kernel::Job`: deduplicated input records + `Task`s — primitive, transformation, boolean,
     display) and returns placeholder solids. An operation with no kernel call (rename, group,
     move…) is **done at this step**, with no round trip;
  2. the `Job` goes to the kernel service, which returns for each solid produced its record
     **and its display** (mesh + snaps), in a single round trip;
  3. `Document::commit` replays the operation on the same document, handing out the results in
     order; `History::push` records it.

  The document, the history and the scene stay on the UI thread; the service is stateless.
- **One service, three executions** (`kernel::KernelService`: `submit(id, job)`, `poll()`):
  `Inline` (synchronous, tests), `Threaded` (a thread, desktop), Web Worker (browser, in the
  app). Each result wakes the UI (`request_repaint`).
- **During a computation**: one operation at a time; navigation and hovering stay active, tool
  clicks, editing shortcuts, undo / redo are ignored; the status bar says it is computing and
  the cursor waits. Esc abandons: the result will be dropped. Compound actions (exploding
  several instances) go through an internal queue, one operation at a time. Replacing a copy by
  an array (`Command::Replace`) restores the copy if the array fails or is abandoned.
- **Meshes**: the scene keeps the mesh of every solid still held by the history (undo / redo do
  not remesh). A solid without a mesh (an opened file) is asked from the service (one task per
  solid), drawn in the meantime by the edges of its bounding box (computed on the record,
  without the kernel); the document stays usable.
- **Web**: two wasm. `apps/kernel-worker` (the kernel alone) runs in a module Web Worker
  (`web/kernel-worker.js`); the UI's wasm no longer holds SolidCore: `kernel` has an `engine`
  feature (execution: `solidcore`, `-convert`, `-tess`); without it, it only exposes the types
  (`Solid`, `Mesh`, `Snaps`, `Job`…) and `solidcore-record`. Messages in **MessagePack**
  (`rmp-serde`, named fields: the record has tagged enums, postcard or bincode do not fit), in
  transferred `ArrayBuffer`s. If the worker crashes, its current work fails (a message in the
  status bar), it is restarted and the other tasks in flight are sent to it again.
- **Testing the recovery**: **Debug › Crash the kernel** menu (task `Task::Panic`, followed by a
  normal task that must succeed). The menu is turned on at launch, in any build: `?debug` in the
  page's address, `SOLIDSKETCH_DEBUG` in the environment on the desktop.

## 7 quater. Taken from SolidCraft: rendering and interface

Decided on 2026-11-10 (questions / answers), after the comparison in `docs/REVIEW.md`.

### Still image

- `Renderer::render` computes a **fingerprint** of everything the picture depends on:
  `Frame` (view, projection, background, style, batches, highlights, lines, grid, axes,
  pixel density, section), the target size, and a generation counter bumped by every mesh
  upload and target change. Same fingerprint as the last picture: nothing is encoded, the
  target keeps it, `render` returns `false`. A still view costs no GPU work.

### Theme

- **`crates/theme`** (no egui, like `render`): named **tokens** in linear RGBA — default front
  and back faces, edge ink, highlights (selected, hover, picked, ghost, other occurrences),
  fading outside the context, section cap shade, axes, selection rectangle, inference markers,
  tooltip. Two themes, **light** and **dark**; the app sets egui's theme from the user's
  `theme` preference (system by default, light, dark). The background colour stays a
  **document setting** (its style, as in SketchUp).
- **Interface tokens** (`UiTokens`, decided on 2026-11-10): panel, header, field, border,
  text, dim text, hover, accent and soft accent, corner radii, spacing.
- **Palettes** (decided on 2026-11-10): a `Palette` (user preference `palette`) gives the
  interface's colours, **each in a light and a dark variant**; the `theme` preference
  (system, light, dark) picks the variant, so the two settings are independent. Built in
  code, no theme files: *Neutral* (SolidCraft's: neutral greys, blue accent, the default),
  *Warm* (warm greys, amber), *Graphite* (cool, deeper greys, teal). A palette changes the
  interface only: the model's and the overlays' tokens stay the theme's. A test holds every
  variant to text ≥ 7:1 and dim text ≥ 3:1 on its grounds.
- `apps/solidsketch/src/style.rs` turns a palette's tokens into egui's visuals and spacing for
  **both** egui styles (light and dark) when it is chosen (at start-up, then on a change);
  egui switches with its theme, nothing is restyled per frame. *Preferences › Interface*
  shows each palette as a card drawn in its colours (the variant shown now); a click applies
  it at once.
- **Fonts**: IBM Plex Sans (variable) and IBM Plex Mono (regular), SIL OFL
  (`apps/solidsketch/assets/fonts/`), embedded in the binary (≈ 670 kB): Sans regular for
  text, semibold for headings, Mono for monospace; then the **key symbols**, then egui's
  fonts (symbols, emoji). Neither Plex nor egui's fonts have the macOS key symbols (⌘ ⌥ ⌃ ⌫ ⌦
  ⏎ ⇥): a subset of DejaVu Sans (Bitstream Vera licence, `assets/fonts/dejavu-sans/`, ≈ 18 kB),
  its arrows and technical symbols blocks only (U+2190–21FF, U+2300–23FF), fills them in. The
  arrow keys are written ← ↑ → ↓ (Plex's), not with egui's triangles (⏴), which no font here
  has.
- `render` keeps no colour of its own: faces and ink come in `Frame::style`.

### Palette and materials

- **Model**: `Document.materials: Id → Material { name, color }` (sRGB);
  `Occurrence.material: Option<Id>` (the copy's material, as in OpenCutList; an assembly's
  occurrence paints the parts below it that have none); `Part.faces: BTreeMap<u32, Id>`, the
  material of single **faces** of the part's final B-rep (so of all its occurrences).
  Priority: face > occurrence > the theme's default; back faces keep the back colour.
  **Accepted limit**: a face is named by its index in the record, which a boolean renumbers
  (risk 5); an index past the solid's faces is ignored.
- Operations: `AddMaterial`, `EditMaterial` (name, colour; dragging a colour merges into one
  step), `DeleteMaterial` (unpaints what used it), `PaintOccurrences`, `PaintFaces`. None
  calls the kernel. A file painting with an unknown material is refused.
- **Rendering**: a **palette** (a storage buffer of linear RGBA); each instance carries a
  palette index, each vertex the index of its painted face (or none). Changing a material's
  colour rewrites only the palette; painting an occurrence only the instances (rewritten each
  frame); painting a face re-uploads that one part's mesh. The app gives each material a
  palette slot for the session, never reused, so baked face indices stay valid.
- **Interface**: the **Paint** tool (B, as in SketchUp), option « Peindre : l'exemplaire / la
  face » in its bar (Tab switches); the **Materials** panel (list, colour, name, add, delete,
  the current material — picking one starts the paint tool); the occurrence's material in
  the entity info.

### Silhouettes

- **Geometric, in the shader**: when a mesh is uploaded, `render` collects every mesh edge
  shared by two triangles (vertices welded by position) that is not a B-rep edge and whose
  normals differ, with both normals. The lines' vertex shader draws it only where one
  triangle faces the eye and the other does not: the outline of a curved surface (a
  cylinder's sides). Same wide lines as edges. Document setting `display.silhouettes` (on by
  default).

### Section view

- **One plane** in the document settings: `display.section: Option<Section { origin, normal
  }>` in world coordinates, and `display.section_active`. What lies on the side the normal
  points to is cut away. Changing it is a `SetSettings` (undoable).
- **Cut** in the fragment shader (faces, edges, silhouettes); the plane is passed in camera
  coordinates (computed in `f64`).
- **Caps: colour from the back faces, extent from a stencil.** Our parts are closed B-reps,
  so through the cut only their back shows: a back-facing fragment, section on, is painted
  flat and darker in its part's colour (a colour per part), at its **true depth** with the
  faces' usual bias (edges keep winning). Then, while a section is on: (1) the faces left are
  drawn again without depth test nor colour, each fragment flipping the stencil's low bit —
  an odd count means the ray meets the plane inside a part; (2) a full-screen triangle, where
  the bit is set and the plane is nearer than what is drawn, writes the plane's depth along
  the pixel's ray (no colour), so what lies behind the cap (the part's far edges) is hidden.
  (Writing the plane's depth on every back face instead, without knowing whether the ray is
  inside the part there, painted every kept part behind the plane as a cap and hid its
  edges.) The depth buffer is `Depth32FloatStencil8` when the device has it, else
  `Depth24PlusStencil8`. Hatches later.
- **Picking and inferences** ignore what is cut away.
- **Section plane tool**: click a face → the plane lies on it, normal outwards; move the
  cursor to sink it along the normal, a second click (or a typed depth) places it.
  *View › Show section* toggles it, *View › Remove section* removes it.

### Command registry and external driving

- `apps/solidsketch/src/commands.rs`: every command has a **stable id** (`edit.undo`,
  `tool.move`, `view.top`…), a label (a catalog text), its menu and a default shortcut. The
  app says whether it is enabled (or checked) and runs it. Menus, tool bar and keyboard come
  from it (the active tool's own keys — Esc, arrows, Tab, Delete, typed values — stay apart).
- **Changeable shortcuts**: preference `shortcuts: id → shortcut` (absent = default, empty
  = none), *Shortcuts* tab of the preferences (conflicts flagged). **Command palette**:
  Cmd/Ctrl+K, searches labels and ids (accents ignored), Enter runs.
- **External driving** (desktop): an HTTP server on the **loopback** interface
  (127.0.0.1:7858, `SOLIDSKETCH_CONTROL_PORT`), **off by default**, started by
  `SOLIDSKETCH_CONTROL=1`. The header `X-SolidSketch: 1` is required (CSRF guard, as the
  Claude Bridge). `GET /ping`, `GET /commands` (ids, labels, enabled, checked, shortcut),
  `POST /run` (body: an id), `GET /state` (tool, context, selection, status, revision,
  modified), `GET /screenshot` (the view, as a PNG). Each request is handed to the UI thread,
  which answers at its next frame.

## 7 quinquies. Title bar and native menus

Decided on 2026-11-10, after SolidCraft (`ui-kit/src/window_chrome.rs`, `app_bar.rs`). The
menu strip is gone: its room goes to one bar holding the document and the commands used most.

- **Title bar** (`apps/solidsketch/src/titlebar.rs`, 34 pt): the document's name (its path as a
  tooltip, a dot when modified), Undo / Redo, the standard views (a menu), the view's toggles
  (perspective, grid, axes, silhouettes, section, Materials panel), and at the right the
  command search (opens the palette, shows its shortcut; only its icon when the window is
  narrow) and the preferences. Every button is a registry command, its state and tip from it.
  The tool bar keeps the tools, the booleans, the assembly commands and zoom extents.
- **macOS desktop**: the bar *is* the window's title bar — the OS one is hidden and the content
  runs under it (`with_fullsize_content_view`, `titlebar_shown(false)`, `title_shown(false)`,
  `native_options` in `lib.rs`), 76 pt are left for the traffic lights (none in full screen),
  the bar's empty space moves the window (`ViewportCommand::StartDrag`) and a double-click
  zooms it. The window title is still set (Window menu, Mission Control).
- **Native menus on macOS** (`native_menu.rs`, `muda`): the app menu (About, Preferences,
  Services, Hide, Quit), the registry's menus, the system's Window menu. The items follow the
  app every frame (enabled, checked, « Annuler Déplacer »); the bar is rebuilt when the
  language or the shortcuts change. A chosen item is its command's id, run as everywhere else.
  - Only **⌘ shortcuts** become key equivalents: AppKit takes them before the window sees the
    key, so single letters (R, M, Space…) would be lost to the VCB and the text fields — they
    stay with the app's keymap, and the menu does not show them.
  - For the same reason, a ⌘ command chosen while egui owns the key goes back to egui as that
    key (`raw_input_hook`): while a shortcut is captured in the preferences, and for Undo,
    Redo and Select all in a text field.
- **Web, Windows, Linux**: the OS frame is kept (no caption buttons of our own: resizing,
  snapping and the move loop are the OS's), and the menus are behind a **☰** button at the
  bar's start, the registry's menus as submenus.

## 8. File

Versioned JSON (`{"format": "solidsketch", "version": 1, …}`): serde serialisation of the
document, B-reps included (SolidCore's record format), and its settings (`settings`, missing
from the first files: default values). In the browser: download and file upload.

**Version 2** (written since 2026-11-10): components, occurrences, the parts' recipes (blank +
machinings, with their `source`). The final B-rep is not the source: it is saved as a cache
(`shape`), present exactly when the part has machinings, to open without recomputing. Version
1 files are converted when opened (§2.7). **Reference files**:
`crates/model/tests/files/v1.solidsketch` and `v2.solidsketch`, read back by the test
`reference_files_still_open` — a format change that would break old files fails there. They
are only regenerated for a new version (`write_reference_files`).

## 9. Toolchain and dependencies

- Rust **1.97.1** pinned by `rust-toolchain.toml` (the SolidCore crates ask for ≥ 1.88), with
  the `wasm32-unknown-unknown` target.
- `solidcore` is on crates.io, but `solidcore-record`, `-convert` and `-tess` are not: all four
  come from the git mirror `tangency-labs/solidcore`, **pinned to one commit**.
- wasm: an 8 MiB stack (`.cargo/config.toml`) — SolidCore recurses legitimately and wasm has no
  guard page. Kernel panics cannot be caught in wasm (`panic = abort`): they are confined to the
  kernel's Web Worker (§7 ter).
- `rust-i18n` reads the `/locales` catalogs at compile time; `crates/i18n/build.rs` makes a
  changed catalog rebuild it.

## 10. Scope of the v0 prototype, in steps

1. **Foundation**: `kernel` (done, to be finished), `model` (document, operations, undo, JSON),
   `render` + app: viewport (orbit/pan/zoom, grid, axes), outliner, selection, primitives,
   move/rotate, undo/redo; native + wasm build.
2. **Groups & components**: create from the selection, explode, enter/leave editing, make
   unique, multiple instances. *(Done, on the former model; replaced by §2.)*
2 bis. **Parts, assemblies, machinings** (§2): see the plan in `docs/REVIEW.md`.
3. **Booleans**: union, difference, intersection, across contexts too (§4).
4. **Push/pull**: picking a B-rep face, offset (the kernel's `offset_faces`, or extrusion +
   boolean). The riskiest on the kernel side: last.

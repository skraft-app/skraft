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

Decided on 2026-11-10, in place since the same day (except §2.8).

```
Document
├─ components : Id → Arc<Component>
├─ root       : Id of the root assembly (the model, never counted itself)
└─ next_id

Component { id, name, props, kind, guides }
  props   : excluded from the cut list, … (the part's type comes from its material, §2.2)
  guides  : construction lines and circles in the component's axes (§6 quinquies)
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
- **Properties** (exclusion from the cut list…) go on any component. The part's **type**
  (solid wood, sheet good, dimensional, edge, veneer, hardware…) is not a component property:
  it comes from the **material** applied to the occurrence, as in OpenCutList
  (`MaterialAttributes.type`). A hinge modelled in several parts is an assembly whose
  occurrence is painted with a hardware material: counted as one item, without detailing its
  content (OCL does not descend into an instance painted with a hardware material).
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
  bounding box in this frame — its **exact** box, which only the kernel computes
  (`Solid::bounds`): the record's vertices miss curved faces (a cylinder has one vertex per
  circle, a torus none). The worker gives it with each display (`Display::bounds`), the scene
  keeps it per solid (`DisplayScene::bounds_of`), for the shapes, the blanks of machined parts
  and the open part's tools. SolidCore's tight box is taken within its loose one: the tight
  box of a mirrored cylinder read back from a record is wrong along its axis (1234567,
  `solidcore_quirks.rs`).
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
  tangent does nothing abnormal; a tool that swallows the whole blank gives "empty result".
- **The recipe task** (`Task::Recipe`, 2026-11-10): a solid minus placed tools, one after the
  other; a subtraction that fails is skipped and reported by its index, the others still cut —
  the task never fails on a tool. Removing or moving machinings computes the part again from
  the blank; copying them cuts only the copies from the shape as it is. The failed machinings
  are kept with the shape (`Part::failed`, the same cache, saved with the file) for the
  interface to show. In the web worker a kernel panic still aborts the whole job (§7 ter): only
  failures SolidCore reports are isolated there.
- Low level: the tool is **any B-rep**, placed in the part's frame, anchored at its origin.
  **Machining types** (hole, groove, mortise…) will come on top and generate that tool from
  their parameters.
- **Links, later**: each machining has a stable id and a `source`. A fitted piece of hardware
  could carry its tools and produce *linked* machinings in the part, placed by
  `T_part⁻¹ · T_hardware · T_tool`, recomputed when it moves. The format has the field from now
  on (always `Local` for the time being).

### 2.5 Copies and propagation

- **Copying an occurrence creates an occurrence of the same part**: "Leg ×4" (the count is in
  the entity info, not in the outliner), counted without guessing anything, without thinking of "making a component" before copying.
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
  Those of components are unique in the document; those of **occurrences** are unique
  **within their assembly**, those of **machinings within their part**: a copy of a component
  ("make a separate part", scope "only this one") keeps the ids of its occurrences and of its
  machinings, so that paths, the context and the selection stay valid. A machining is named
  by `(part, machining id)`. A path is always resolved from the root. An operation designates a face by
  `(part id, face index in the final B-rep)` — enough in direct modelling, since the operation
  applies at once.
- **Occurrence path**: an entity of the scene is designated by the path of occurrences from
  the root; its world transformation is the product of the T along the path.
- **Active context**: the assembly being edited, and the path leading to it.

### 2.7 Former model

Dropped on 2026-11-12 with its file converter: the app is not published, the format stays at
version 1 until it is (§8).

### 2.8 Not in place yet

- **Machinings**: no "integrating" a machining into the blank yet, nor a cache of
  intermediate shapes; no marquee selection of machinings.
- **"All occurrences / only this one"** and the automatic separate part: a geometry change
  affects all the part's occurrences today.
- **Material type** (§2.1), **properties** (exclusion from the cut list): not in the
  format yet; they will enter it with a default value (v2 files will stay readable).
- **Tools in the part's axes** (§2.3), highlighting of occurrences during a change, list of
  machinings in the interface.
- "Make a separate part" on an assembly copies only its first level (not in depth).

### 2.9 Opening a part: its axes and its machinings

Decided on 2026-11-10; built in steps (REVIEW, "Work: opening a part").

- **Double-clicking a part's occurrence opens the part**, as double-clicking an assembly's
  enters it: the editor keeps the assembly context and adds the occurrence open in it
  (`Editor::open_part`). Esc (Select, nothing selected), a click off its grey box or a click in
  the outliner close it, the occurrence left selected; an undo that removes it closes it.
  While it is open nothing else is selectable.
- **What surrounds it is faded**, as outside any context; the part's **other occurrences** are
  marked `Related`: whatever is changed reaches them, under the scope (§2.5). A **grey dashed
  box**, a few pixels off its blank, says the part is open, as for an assembly entered.
- **The part's axes** replace the drawn axes, as an assembly's do when it is entered, and show
  even when the axes are hidden: its origin, through the occurrence's T (a mirrored occurrence
  shows its left-handed frame). In an open part, **tools draw and infer in these axes** (§2.3):
  axis locks, inferred axes, the ground, typed points, the frame of a box drawn. The tools see
  two frames: the context's, which operations are expressed in, and the axes
  (`Cx::axes`, `Cx::axes_in_context` = the occurrence's T). **Drawing in an open part adds a
  machining** to it (`Operation::AddMachining`): the shape drawn, in the part's axes, is a
  tool, cut at once; the part stays open.
- **Shown by default: the shape**, as anywhere else. A **"Machinings" switch** — a button at
  the head of the option bar while a part is open, remembered in the preferences; neither a
  menu command nor a shortcut — shows instead **the blank as a ghost** (very light faces, sharp edges and
  silhouettes, drawn after the opaque pass, depth tested without writing) and **each
  machining's tool as a solid**, in a colour of the theme, another for those that failed.
  The tools are picked first; the ghost blank is not picked but keeps its snaps.
  Built (step 3): the display scene meshes the recipe of the part shown — its blank and one
  mesh per distinct tool solid (a copied machining shares its original's) — under keys apart
  from components' ids (`RecipeMeshes`); the view gives each placement a `Look` (shape,
  blank, tool, failed tool) and leaves out the open occurrence's shape; the renderer draws a
  **ghost batch**'s edges like any other, then, after everything opaque, its front faces
  blended at the theme's `ghost_opacity`, depth tested without writing depth (no cap in a
  section); the tools take two palette entries the viewport adds after the materials.
- **Inferences see what is drawn**: while the machinings show, the blank and the tools stand
  in for the open occurrence's shape (`DisplayScene::inferable`, `pick_inferable`), with their
  snaps — a tool's axis, the centre of its end, its edges out of the blank. Each piece says
  what it is (`Piece`: shape, blank, tool of a machining); a tool's filter sees it
  (`Owner`), so a move or a rotation leaves the machinings it moves out (`Cx::is_moving`).
  Picking for the other tools (paint, booleans, section, navigation) stays on the shapes.
- **Machinings are edited there**, while they are shown: the selection is then the open
  part's machinings (a click on a tool, Select all), erased (Delete), moved (copy and array
  too) and turned; while dragging, the moved tool is a ghost where it goes, not drawn where it
  was, and the part is computed again when the change is applied (asynchronous, §7 ter). Move
  and Rotate say what they change through `Cx::moved` (`Moved`: occurrences of the context, or
  machinings of the open part, each in its frame), copies and arrays through `LastCopy`.
- **The scope** of every change to the open part (drawing, editing machinings) is a choice of
  the option bar while it is open, next to the switch: "All occurrences ×N" / "Only this
  one", **back to "all" at each opening** (a change to one occurrence only makes it a
  separate part: better chosen each time). The `Related` marks follow it.
- **The entity info of an open part lists its machinings** ("Machining 3 · 8 × 8 × 30 mm",
  in the part's order; a failed one in the error colour, said so): hovering a row draws its
  tool hovered, hovering a tool in the view makes its row bold; a click selects it (Shift adds
  or removes), first showing the machinings if they are not.
- Later: integrating a machining into the blank, machining types, and changing a part's axes
  (§2.3) from there.

### 2.10 Future track: procedural parts (not decided)

Discussed on 2026-11-11 and set aside: nothing here is decided nor built.

- **A part is already procedural**: a recipe (§2.4) whose final B-rep is only a cache. The
  track is to push this further without falling into a general node graph or a timeline:
  `Part = Blank(params) − Σ Machining(type + params, anchor)`, a two-level CSG tree with a
  fixed shape and parametrised leaves.
- **Parametric blank and anchors**: a "board" blank (L × W × T in the part's axes), and
  machinings anchored on its faces ("face +Z, 37 mm from face −X"): resizing the board, the
  holes follow. Rule: **anchor only on the blank** (its 6 faces and 12 edges, named stably),
  **never on a face of the final B-rep**. Since machinings only remove material and their order
  does not matter, there is no history to replay: this sidesteps persistent face naming, the
  blocker of parametric modelling (REVIEW, risk 5). A free blank (imported B-rep, boolean
  result, curved part) stays direct, its machinings placed in the part's axes as today.
- **Machining types** (hole, groove, mortise…) are the procedural generators of the tools
  (§2.4).
- **Shared tool definitions**, rather than "a machining is a solid that has machinings":
  a tool is composed by **union** (a counterbore = two cylinders, a countersink = cylinder +
  cone), seldom by subtraction, and it is not a part (no material, type, grain, nor place in
  the cut list). What is worth having is sharing: a "hole ø8 × 30" defined once, placed many
  times, changed everywhere — the component / occurrence mechanism applied to tools, and what
  `source: Linked` (§2.4) prepares for hardware. Needs: a tool kind of its own in the document
  (not a `Part`), cycles forbidden, a dependency graph to invalidate the parts using a changed
  tool.
- **Changing a part's axes** (§2.3) with a parametric blank:
  - **quarter turns** (re-orienting the grain, standing up a part drawn lying): a signed
    permutation of the box's axes (24 rotations). L, W, T are permuted, the named faces
    relabelled by the same permutation, the machinings' parameters unchanged, the occurrences'
    T compensated — exact, no kernel. Anchors are stored **geometrically** (±X, ±Y, ±Z of the
    box), the labels end / edge / face **derived** from the current axes: stored semantically,
    re-orienting the grain would move the holes;
  - **origin**: a convention (e.g. the min corner), not movable; T absorbs translations;
  - **oblique rotation**: meaningless for a board (its box no longer gives the cut-list
    dimensions): only through an explicit "Convert to free shape" (parameters dropped, anchors
    frozen into placements). A free blank keeps §2.3: any rotation, B-rep and tool placements
    re-expressed.
- Possible order: machining types → parametric board blank and anchors → shared tool
  definitions and hardware-linked machinings. Not: recursive machinings on tools.
- Open questions: is anchoring on the blank only acceptable (a hole in a chamfered face is
  then expressed against the box)? Can a non-rectangular blank be parametric? Should the axes
  tool snap to the 24 orientations on a parametric part, or stay free and offer the conversion
  only on an oblique angle?

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
  Shift+click adds/removes, double-click on a group/component enters it, a click off the grey box
  of the one being edited (void or another entity) leaves it, one level per click; inside the
  box, a click in the void only clears the selection; Esc goes back one step of the active tool; at its
  first step, it does nothing more for a tool other than Select (the tool stays: only Space —
  the `tool.select` command — goes back to Select; decided on 2026-11-11), and clears the
  selection with Select. Dragging draws a selection
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
  C = Cylinder, B = Paint, S = Stretch, Cmd/Ctrl+K = command palette, Cmd/Ctrl+1…6 = standard views,
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
  tool's options — exclusive choices and toggles; no tool name, and no footer at all for a tool
  without options (nor open part). Every choice is visible and clickable there; a shortcut is only a
  complement (Tab cycles the scope). The status bar keeps the guidance for the current step.
  - **Tools describe their options, the app draws them**: `Tool::options` returns a list of
    `ToolOption` (key, label — which may depend on what is hovered, "×4" —, tooltip, value,
    choices), `Tool::set_option` receives a change. `tools` stays without an interface: a test
    changes an option like a click does.
  - **Icons** (2026-11-12): a choice's value or a toggle may name an icon (`icon`, a name the
    app maps to `assets/icons/options/<name>.svg`, `icons::option`); the bar then draws the
    icon, its label going into the tip. Drawn after OCL's motifs
    (`get_action_option_btn_child` of Smart Reshape and Smart Draw): Stretch, Box, Cylinder;
    the radius and "from a corner" have none in OCL, drawn in the same style. The option's name stays as text
    before a choice. Both rows of the card (buttons, options) are centered in it.
  - **Two kinds of options**: **settings** (the scope…), remembered per tool in the user
    preferences (`tool_options`), read by the tool from its `Cx`; and the **gesture's state**
    (copy, for Move and Rotate), which the tool holds itself and which is not remembered.
  - First options: the booleans' scope (all occurrences / this one); copy (Move, Rotate).
    Later: axes (model / part, §2.3), keep a boolean's tool (OCL's `keep_cuts`), options of
    push/pull and of machinings.

## 6 ter. Inferences

Decided on 2026-11-09.

- **A central engine** (`tools/src/infer/`, no UI, no GPU, no state) replaces the point
  choices of each tool (`base_point`, `target`, `center_at`…). A tool calls `cx.infer(px, keep,
  plane)`, `cx.infer_from(…)` or `cx.infer_in_plane(…)`; the `Cx` fills a `Query` with what the
  editor holds and calls `infer::infer(scene, camera, size, px, &query)`.
  - `Query`: `keep` (the geometry to infer on, and that may hide others: all but what is
    moving), `plane` (fallback plane, world point and normal), `origin` (a point offered as
    `Origin`), `from` (the tool's start point), `tracked` (the tracked points), `in_plane`
    (only lines lying in `plane`: the rotate tool's protractor), `axes` (the context's, in the
    world), `reference` (the last edge hovered, `EdgeRef`), `lock`.
  - `Inference`: `point` (world), `kind` (`InferKind`), `path` (the solid it belongs to, if
    any), `hit` (what the ray hits, whatever was inferred: tools orient themselves on its
    face), `line` / `line2` (the line(s) the point is on: origin and unit direction; the app
    draws the guides from them), `edge` (the edge at the point, the next reference).
- **Exact geometry**: candidates come from the **B-rep record** (`f64` vertices, typed curves:
  exact midpoint of a line, exact centre of a circle), not from the mesh. A `SnapIndex` per
  solid (vertices, edges and their curve, bounding box) is cached per `Arc<Solid>` like the
  meshes; it is transformed by each placement. Fallback on the mesher's polyline for splines
  and intersection curves (`Mesh::edges` must keep the record's edge index).
- **Priorities** (`InferKind::rank`; at equal rank, the closest to the cursor on screen within
  `APERTURE_PX` = 10 px, then the nearest to the eye): endpoint > midpoint / centre / origin >
  constructed point (a line × an edge, two lines of different origins) > on a line (axes from
  `from` or from a tracked point, parallel / perpendicular to an edge, tangent / radial to an
  arc) > on edge > on face (projected exactly if the face is planar) > fallback plane. A lock
  comes before everything: the point stays on its line.
- **Occlusion**: a vertex or an edge behind the surface the ray hits is not a candidate (up to
  a tolerance); the edges of the hovered face stay reachable. Inference covers the whole
  visible scene, including outside the active context.
- **Locks**, held by the editor for every tool: arrows → / ← / ↑ = red / green / blue axis, ↓ =
  free; holding Shift locks the current linear inference. A point hovered during a lock is
  projected onto the line (alignment). Locks drop on every applied operation, on Esc, on a
  change of tool and on undo. Depending on the tool, the axis lock gives the direction (Move),
  the rotation axis (Rotate) or the normal of the drawing plane (Box, Cylinder: ↑ horizontal,
  → perpendicular to red, ← to green, reversed so that the plane's second side points up).
  For the shape tools (`Tool::down_locks_face`), ↓ locks the normal of the face under the
  cursor instead (`face_lock`, taken when pressed); the same face again, or none, frees it
  (decided on 2026-11-11, after OpenCutList's smart draw tool).
- **Lines**: through the tool's start point, the context's axes and, for the straight edge
  hovered last, parallel and perpendicular (in the tool's plane). A line almost aligned with
  the view direction is ignored (it is only a point on screen). If it pierces the face under
  the cursor near it, that point is taken.
- **Visual feedback**: guide lines (axis colour) through the existing overlay; markers coloured
  by kind and a tooltip ("Endpoint", "Midpoint", "On red axis"…: §6 quater) painted by egui over
  the viewport (`camera.project`): `render` does not change.
- **Drawing plane** (revised on 2026-11-11, after OpenCutList's `SmartDrawShapeActionHandler`;
  the rules in `tools/shapes.rs`): the first click takes the plane of the face hovered (on an
  edge, the face it bounds, else the ground or the upright plane through the edge), otherwise
  the context's ground; a straight edge askew to the axes there turns the base's sides along
  it (a guide line likewise, taken as an edge; a guide circle by its tangent there, in its
  plane off a face). The box's second corner sets the plane again: an axis plane through the first corner it
  lies in, the plane through an edge aimed at and the first corner, the plane of a face the
  first corner lies on, else the upright plane through both corners; a guide aimed at (a
  line, a circle by its tangent there) acts as an edge and always turns the base's sides
  along it (even locked plane). The cylinder keeps the
  plane of its centre. The height runs along the normal, towards the eye. Esc goes back one
  step. Before the first click a small shape at the cursor shows the plane, the lines take the
  colour of its normal's axis (magenta for another locked one) and the sides being drawn get
  their lengths written (`Preview::labels`, painted by egui like the tooltip, last, above
  everything). A length is written at the middle of its side; when that covers the cursor, a
  marker, the inference tooltip, a cross (`Preview::marks`, a centre or a start, drawn by the app as a flat `+`) or a length placed before, it
  slides along its side, then off it on either side (the first spot free, else the middle).
  The box's opposite corner snaps to the square's within 10 px (not on a feature, nor on a
  plane seen edge on), its diagonal then drawn; its saved option "Centred" (the bar, or a tap
  of the copy modifier, through `Command::SetOption`) takes the first click as the centre of
  the base, typed lengths being the whole ones.
  The cylinder's radius snaps likewise to the red, green or blue axis through the centre, and
  is dashed in that axis's colour when along it. Its saved option "Measure" (Radius or
  Diameter, Tab to turn it over) sets the length shown and typed for the base; the diameter
  is drawn across, through the centre.
  The first point (decided on 2026-11-11): the copy modifier held over a face takes its centroid
  and, with Shift tapped, the whole face as the base (OpenCutList does this for its polygon
  only; here the box takes the rectangle around the face along its longest side, the cylinder
  a disc's circle); the other modifier tapped (Command on macOS, Alt elsewhere) measures it
  from a corner, typed as `d` (along an edge) or `d1;d2` (across a face). The height: Shift
  held, or a double-click on the opposite corner, takes the last height again (kept by the
  editor across tools); the copy modifier held runs it to the first face along the normal from
  the middle of the base; tapped, it centres the height ("Centred height", saved). The app
  tells a modifier tapped alone (`Key::Copy`, `Key::Shift`, `Key::Alt`) when nothing else, nor
  another modifier pressed, came in between, and tells the copy modifier held every frame
  (`Editor::set_copy`), as Shift (`Editor::set_shift`).
  What a shape tool makes is not selected (decided on 2026-11-11), nor what a boolean makes:
  the selection stays as it was and the next shape follows at once.
  `Operation::AddPrimitive` takes a rigid frame for that (`placement: DAffine3`, identity by
  default in serde to read existing files back); the primitive is built axis-aligned then
  transformed.
- **Performance**: culling by bounding boxes against the picking cone, through the scene's
  BVH (§7 bis, *Spatial index*).
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

## 6 quinquies. Tape measure and guides

Decided on 2026-11-11, after SketchUp's Tape Measure tool. Built in three steps: (1) the
model, the file, the display and the tool; (2) inferences on guides; (3) managing them.

- **Guides are document entities**: `Component::guides`, a list of `Guide { id, shape }` in the
  component's own axes, saved with the file (**version 3**; v2 files read with none) and
  undone like everything else (snapshots, §5). `GuideShape` is an infinite `Line { origin, dir }`
  or a whole `Circle { center, normal, radius }`; a finite `Segment` (a guide a shape tool would
  leave, e.g. a box's diagonal) will come as one more variant.
- **They belong to the active context**: the open part when one is (§2.9), else the assembly
  being edited (the model at the top). A part's guide shows in each of its occurrences and
  moves with them; changing a part's guides ignores the scope (§2.5): it changes no geometry,
  so no part is made separate for it. A guide drawn with no part open belongs to the assembly
  and does not follow a part moved later. Copying a component (separate part) copies its
  guides; exploding an assembly hands its guides to the context, placed as it was.
- **Ids**: from the document's counter, unique within their component, never equal to an
  occurrence's (assembly) or a machining's (part) of the same component: the selection (step
  3) holds them with those, and `Delete`, `Transform`, `Duplicate` will take them.
- **Operation**: `AddGuide { component, shape }`, the shape in the component's axes; no kernel.
- **Display**: dashed, depth-tested lines (`View::guides`, `guides::Stroke`). A line is
  drawn **as far as the view shows it** (`Camera::visible_segment`, 2026-11-11): clipped to
  the view's sides (a few pixels past them), kept off the eye in perspective, and an end
  running off to infinity stops within a quarter of a pixel of its vanishing point; seen end
  on in parallel (within 1 mrad: the standard views stop short of the zenith), it is not
  drawn. A circle is a polyline. The renderer dashes them **on screen** (`render::Dash`):
  dashes and gaps of 6 points, the pattern anchored on the line's origin (when in front of
  the eye; else its first point drawn) and running on along a circle's chords, so it holds
  still as the view pans. An infinite line is never inside a selection rectangle: only a
  crossing one takes it. The axes are drawn the same way, across the view (their negative
  halves faded). Those of the active context in the theme's `guide`,
  the others (outside it, or in another occurrence of the open part) in `guide_faded`. Step 3
  adds a "Guides" toggle to the display (with it off, they are not inferred either).
- **The tool** (`ToolKind::Measure`, `T`, `tools/measure.rs`):
  - the first click takes the inferred point. With the saved option **"Create guides"** on
    (the default; a tap of the copy modifier turns it over, as SketchUp's Ctrl), it starts a
    guide of the kind the saved choice **"Guide"** says (decided on 2026-11-11; Tab turns it
    over, also between the two clicks — the first click is kept and starts again):
    - **Auto** (the default): **on an edge** (`OnEdge`, not an endpoint nor a midpoint),
      **parallel** to a straight edge — the line through the point hovered, along the edge;
      the length shown is the distance between the two lines — or **concentric** to an arc or
      a circle — the circle in its plane through the point hovered (projected on that plane);
      the length shown is the difference of the radii; anywhere else, a measure (below);
    - **Line**: parallel to a straight edge; **tangent** to an arc — a line along its tangent
      at the point clicked, offset along its radius in its plane; anywhere else, the line
      **through the point clicked and the next one** (the length between them shown, a length
      typed doing nothing);
    - **Circle**: concentric to an arc; anywhere else, **centred on the point clicked** — in the
      face clicked, else level —, the cursor giving the radius (shown and typed as "Radius"):
      the concentric case of a circle of radius zero, so a guide crossing two others (a
      fillet's centre) takes its circle.

    **Where curves cross** (edges, guides, the axes as drawn: `Inference::crossed`, each curve
    at the point, set on `OnEdgeAndLine`, `OnGuideAndLine`, `OnTwoLines`, `OnGuideAndEdge` and
    `OnTwoGuides`; not a reference for the next lines) **and at a vertex** (`Endpoint`: the
    edges ending there, `SnapCurve::ends`, looked for only at a vertex near the cursor — so
    the end of an arc gives its tangent), each gives its guide of the kind chosen, and the
    fallback above one more (decided on 2026-11-11). The first click takes, in Line and
    Circle, the one from the edge last hovered (`Cx::reference`) when it is one of them, else
    the fallback; in Auto always the fallback (a point is measured from, as before). The
    other modifier tapped (Command on macOS, Alt elsewhere) turns them over, the status bar
    naming the guide ("Tangent guide", "Parallel guide"…). The last edge hovered alone could
    not choose: coming along a circle to where it crosses a line, the cursor is within the
    line's aperture before the crossing's, so the line is always last.
  - **The reference is marked**: the curve the guide refers to is drawn in magenta as it is
    drawn itself, from the first click on; before it, at a point where several cross, the one
    the click would take. The inference hands it over with its `EdgeRef` (`Inference::curve`,
    and each of `Inference::crossed`, an `infer::Curve`): an edge, end to end, and an arc as
    sampled, bolder (`PreviewLine::bold`, `render::BOLD_WIDTH`); a guide by its very strokes
    (`Preview::guides`), so the magenta dashes fall on its own; an axis as drawn, across the
    view (decided on 2026-11-11: an infinite line through the point clicked hid what it
    was taken from, and its dashes fell between a guide's).

    The offset is drawn dashed — in the colour of the axis it runs along, else ink — from
    the point clicked to the guide,
    square to the edge (for an arc, along the radius through the cursor). Pointing at nothing,
    the cursor gives a point of the horizontal plane (the axes') through the point clicked —
    for an upright edge, of the face clicked, else of the upright plane through it facing the
    eye; the perpendicular is inferred in that plane. The second click, or a typed length (the offset, towards the
    cursor's side), adds the guide. A guide may be offset by zero (along the edge, the tangent
    itself, or the whole circle of an arc); a circle shrunk to nothing adds nothing (said in
    the status bar);
  - **anywhere else** in Auto, or with the option off, it measures: the length from the first point
    to the point hovered, written on the dashed segment between them (`PreviewLabel`) and in
    the measurements box; the second click ends the measure. A length typed then does nothing
    (no scaling of the model, meaningless for cut-list parts);
  - lines are inferred from the first point (axes, parallel and perpendicular to the edge
    clicked, tangent and radial to the arc), so that a guide is offset square to its edge.
    Esc goes back to the first click. `EdgeRef::Arc` carries the arc's centre and radius for
    this.
- **Step 2 — inferences** (done on 2026-11-11): the display scene keeps every guide placed in
  the world (`DisplayScene::guides`, `SceneGuide`, rebuilt with the document), which the
  view draws and the engine infers on, for every tool. A guide line is one more line
  (`LineKind::Guide`, one source each): "On the guide" (rank of the lines; the guide becomes
  the reference edge, so the tape offsets a guide from a guide), "Edge × guide", "Guide ×
  guide" and crossings with the tool's lines; a point of it hidden behind the model is not
  inferred (it is drawn depth-tested), and only one lying in the protractor's plane counts
  for the rotate tool. A guide circle is a whole arc: "On the guide" (`InferKind::OnGuide`,
  its arc as the reference), its centre (`Center`, tracked like any) and where a line crosses
  it (`InferKind::OnGuideAndLine`), and where it crosses an edge (`InferKind::OnGuideAndEdge`,
  exact: a segment, an arc in its plane — two circles —, or across it — on the line where both
  planes meet; a polyline by its segments). Such a point built by the geometry ranks with
  those built by lines (the nearer on screen wins) and beats any point on a line; where two
  guide circles cross likewise (`InferKind::OnTwoGuides`, "Guide × guide").
- **The axes as drawn are lines too** (2026-11-11, for every tool, as in SketchUp): while
  they show (`Display::axes`, or a part open), the context's three axes through its origin
  (`LineKind::ModelAxis`, "On red axis") — crossings with edges, guides and the tool's
  lines; not inferred where hidden; an edge to refer to, so the tape draws a guide from an
  axis as from an edge.
- **Step 3 — managing** (done on 2026-11-11):
  - **Picking**: a guide of the active context within `GUIDE_PICK_PX` (5 px) of the cursor
    and not hidden by the model (`Cx::pick_guide`) comes before the solid under it, for the
    select tool's click and hover and for what Move and Rotate take with nothing selected.
    The marquee takes a guide like a solid: a window one lying wholly inside it (a circle), a
    crossing one any of it touches. Select all takes them. In an open part, its guides are
    selectable, its machinings shown or not; the part's guides are not, from outside it.
  - **Operations**: no new ones. `Delete`, `Transform`, `Duplicate`, `Array` and
    `MakeAssembly` take guide ids of their context among their entities (the guides go into
    the new assembly; guides alone make none; `PaintOccurrences` leaves them out). The new
    assembly's axes have their origin at the low corner of its content's bounds (in the
    context's axes): the content and its guides are brought into them, the new occurrence
    placed there, so nothing moves in the context. The
    operations on machinings take the part's guides — with guides alone, no kernel and no
    separate part, whatever the scope. In an open part, Move, Rotate and Delete always aim at
    the part (`Cx::moved`), its machinings shown or not.
  - **Inference**: a guide is an `Owner` with `Piece::Guide(id)` (its path: the component's
    occurrences, then its id): the guides being moved are left out (`Cx::is_moving`).
  - **Display**: the selected guides in the selection's colour, those hovered (or in the
    marquee) in the hover's; while moving, the selected ones are drawn where they go, and
    where they were only when copying. `Display::guides` (on by default; View › Guides, its
    title-bar button, the model's preferences) hides them: not drawn, inferred nor picked,
    dropped from the selection. Edit › "Delete guides" erases those of the active context,
    shown or not. The entity info of one guide says its shape (a circle's radius).

## 6 sexies. Stretch

Decided on 2026-11-11, after OpenCutList's Smart Reshape stretch
(`tool/smart_reshape_tool.rb`, `SmartReshapeStretchActionHandler`; the analysis in
`worker/common/common_stretch_split_worker.rb` and `model/stretch/`, the application in
`common_stretch_apply_worker.rb`). Not built yet (REVIEW, "Work: stretch"). Comes **before
push/pull** (§10): more useful in woodworking, and safer on the kernel side.

**What it does** (as in OCL): the part's box has a grip on each of its 6 faces; pulling one
stretches the part along that axis, the opposite face anchored. **Cutters** — planes across the
axis, one in the middle by default, added, moved or removed by the user — split the part into
**sections**: each section is translated by its share of the move, the matter crossing a cutter
is lengthened or shortened. A tenon at each end of a rail keeps its length while the rail
grows. Options: **centred** (both ends move), **measure** as the overall dimension or as the
offset; the length is typed (VCB, expressions, §6); compression stops 1 mm before two sections
meet; double-click repeats the last measure. Later: OCL's interior handles (one side stretched,
the other compressed, overall dimension kept).

**Target: one occurrence, of a part or of an assembly** (assemblies built 2026-11-11), stretched
**in its component's axes** (§2.3: a part's box is the exact bounds of its shape in its frame;
an assembly's, its parts' boxes brought into its axes). The tool takes it as the selection
does: the open part, the one occurrence selected, else the active context's occurrence under
the cursor (a part, or the assembly holding it — open the assembly to aim at a part in it).
Later, maybe OCL's way of picking (a click takes the part, the copy key goes up to its parent).

**Several occurrences of one context** (decided on 2026-11-11, after OCL's
`active_selection_path` + `active_selection_instances`, `root_split`, `_get_edit_transformation`;
built the same day):
- **Taking them** as OCL's `SmartSelectActionHandler`: the selection when the tool starts
  (several ids of the active context); in the tool, a rectangle — left to right a window, right
  to left a crossing, as the Select tool (`select::marquee_hits`, its guides left out) — and
  Shift held with clicks toggling occurrences, the set taken when Shift is released
  (`STATE_SELECT_MULTIPLE`: `StretchTool::gathering`, the selection ignored while Shift is
  down); a plain click takes one, as before. Both go through the selection (`Command::Select`),
  drawn as such, as OCL fills the model's selection.
- **The axes option** (OCL's three, saved; default **entity**, as OCL's stretch preset):
  *active* — the model's axes, those of the root; *context* — the active context's axes;
  *entity* — the occurrence's own when there is one, the context's when there are several. In
  an open part: active the root's, context its assembly's, entity the part's. Only at the root
  do the three meet.
- **The box** is common: the bounds, in the chosen axes, of every part placed under the
  occurrences — its exact box when it lies square to the axes (a part occurrence read from the
  document, there before its mesh), else its display mesh's vertices (its box's corners would
  swell it). Its axes with no thickness are not offered. The preview (`StretchPreview`) and
  `Document::stretch_bounds` / `stretch_spans` take the same context, occurrences and frame.
- **The operation** is `Operation::Stretch { context, occurrences, frame, axis, cuts,
  moves, scope, cutters }` (`cutters`: « Behavior » below) (none named: `Refusal::NothingStretched`; `frame` rigid): the stretch in the axes `frame` places in the context's (the chosen axes),
  the occurrences of the context's component it applies to. One occurrence in its own axes is
  the case built first (`frame` its transform). A part refusing names itself
  (`Error::InPart`) when several are stretched. (The operation grew: `Command::Replace` boxes
  its two.)
- **The walk** starts at the context's component, restricted to those occurrences, each taking
  the stretch through `frame⁻¹ · its transform`; below, nothing changes (moved or stretched,
  one component per distinct change, the most common one in place under « all »). The
  context's component itself is changed in place — as any edit in an open context.
- **Scope « only these »** (OCL's `make_unique`): the context's path is made its own once
  (`separate`); then the occurrences that take one change share one copy (OCL's
  `make_unique_d`: one new definition per deformation, not per instance), a component placed
  elsewhere too is left as it is. « All » is unchanged.
- **Cutters**: one occurrence takes its component's (its `Behavior`, below); several share one
  set, the last used, for the session (OCL's `@@last_cutters_data`; the store's `None` key):
  it belongs to no component, and is never saved.
- Changing the axes option goes back to the grips (OCL's `onToolActionOptionStored`).
- **Limit** (the kernel's, not OCL's): in the context's or the model's axes, a part turned in
  them that crosses a cutter has no face along the axis — the stretch is refused, the part
  named (`Error::InPart`); wholly in a section, it is only moved. OCL deforms any geometry.
- The preview (`stretch_sketch`, `stretch_swaps`) follows the same walk unchanged.

**Assemblies** (`crates/model/src/stretch.rs`, after OCL's split and apply workers). Walking
down from the target, the stretch brought into each component's axes (`Stretch::in_frame`),
each occurrence is:
- **moved** when its content lies in sections that move alike (its extent along the axis,
  `Stretch::extent`, conservative for curved faces): only its placement changes, its component
  is not touched — a leg at the end of a frame;
- **stretched** otherwise: a part has its recipe stretched as above, an assembly has its
  occurrences walked in turn (its guides follow their sections). Each component is stretched
  in its own axes, **its origin following its anchor** (changed 2026-11-12, after OCL; it was
  "no anchor" before): the anchor is the section holding the end of its extent nearest its
  origin along the axis (OCL's `section_def` of a `SPLIT` container: the nearest end, not the
  section holding the origin — they differ only for an origin within the matter, in a middle
  section). The occurrence moves as its anchor, its content by the sections' moves less the
  anchor's (`relative`, OCL's `edv -= container_edv`), and below, each component relative to
  its own. A component changed in place moves its occurrences the walk does not reach as much,
  in their own axes (`move_unreached`, OCL's back translation of extern instances): on screen
  nothing differs from no anchor, only where the part's axes end up in its matter.
- **What a component becomes** is what the stretch does to each of its pieces (OCL's md5 of
  section deltas): the move of each vertex, of each end of each edge's and face's extent
  (`Stretch::piece_moves`), of its machinings' tools, of each occurrence moved whole, of each
  guide — along its axis turned the canonical way (its first component not negative, OCL's
  `_axis_sign`), taken from the first piece's move. Wherever the cutters fall between the same
  pieces, and for a copy turned half round or mirrored stretched alike, it is one change:
  occurrences taking the same keep one component, the one made by a stretch differing by a
  move along the axis moved by that move (its placement); different changes make separate
  components (§2.5). (First built on the stretch itself, cuts and moves: a copy turned half
  round, stretched alike, became a separate part — fixed 2026-11-11.) Under « all occurrences », the change most of the tree's
  occurrences take is made in place (shown wherever the component is), the others go to
  copies; under « only this one », the path is made the target's own (`separate`), then a
  change goes to a copy whenever its component is placed elsewhere too (counted over every
  path of the document). Copies keep their ids (§2.6), so paths stay valid.
- A component below the target that is locked along the axis is a block (see « Behavior »
  below), never stretched. A part below the target that cannot be stretched refuses the whole
  stretch, named (`Error::InPart`); `Operation::Stretch` takes any occurrence, one undo step, one job for
  every part. `Outcome::stretched` lists the parts changed (the part after, the part before,
  its stretch). `Document::stretch_spans` gives the tool its limits (the matter wholly in each
  section, whole occurrences and parts' edges), `Document::stretch_bounds` an assembly's box
  from the extents.

**Behavior: cutters and locks** (decided and built on 2026-11-12; after SketchUp's
`Sketchup::Behavior` and OCL's `stretch_cutters` attribute). The cutters are not the tool's
state but how a component is cut when stretched: they follow its definition, as OCL keeps
them on it. With them, the axes it never stretches along. A typed `Behavior` on the
`Component` (a part or an assembly) holds both — not a generic attribute dictionary (weighed
the same day): typed, validated when read, and the rule of each field on copy written where
it means something; a dictionary may come later only to carry foreign attributes through a
SketchUp import / export.
```rust
pub struct Behavior {
    /// Axes (own) along which the component never stretches.
    pub no_stretch: StretchMask,
    /// The cutters along each own axis, as ratios of the box; none: one in the middle.
    pub cutters: Option<[Vec<f64>; 3]>,
}
```
- **`StretchMask(u8)`**: SketchUp's `no_scale_mask` bit layout — X = 1, Y = 2, Z = 4; bits 3–6
  (XY, XZ, YZ, uniform) reserved, unused while there is no scale tool (occurrences are never
  scaled: stretching is our scaling). Saved as an integer, mapping one-to-one on a SketchUp
  import / export. OCL's stretch reads only the full mask (127, its imported hardware); here
  each axis counts.
- **The mask is the one truth**: a locked axis has no cutters (those saved for it are ignored
  and dropped). `sanitized` no longer puts one back in the middle on a locked axis.
- **Copies**: `MakeUnique`, `separate` and the walk's copies keep the behavior as it is
  (`copy_component`, `copy_from`). The target of a stretch takes the tool's cutters, set again
  after it (`kept_cutters`), through `Operation::Stretch::cutters` (one occurrence; written on
  its component as it is after the stretch, a copy maybe, its locks kept); the components
  below it keep theirs as they were — ratios of a box that changed, still valid.
- **Cutters are saved** by any gesture on them, not only by a stretch: `Operation::SetBehavior
  { component, behavior }`, pure model (no kernel). Consecutive ones on one component merge
  into one undo step (`merges_into`, as `SetSettings` and `EditMaterial`): a run of
  adjustments is one step, « Cutters », the stretch after it another. Not one step per gesture
  (too many), not outside the history (undoing the previous operation would bring back a
  snapshot without them, silently). A stretch writes the new cutters in its own operation.
- **Locking, in the tool**: removing the last cutter of an axis (dragged out of the box, or
  the remove modifier) locks it — it was « one in the middle again ». The locked axis is drawn
  grey, without grips or cutters, with a **closed padlock** at its end (the grip of its
  positive side: at the box's centre, two locked axes would meet); a click on it unlocks the
  axis, one cutter in the middle. No open padlock on free axes (too much on screen): removing
  the last cutter is the only way to lock. Both are `SetBehavior`, merged as above; they do
  not drop the arrow keys' axis lock as other operations do. A stretch leaving no gap between
  matter puts one cutter in the middle: not a lock.
- **Where the tool keeps them** (`Keep`): on the component when the stretch's axes map onto
  its own, each axis one of its own maybe turned over (ratios then reversed) — one
  occurrence, the axes option *entity*, or *context* / *active* with the component square to
  them; else for the session (several occurrences under one key, one occurrence turned
  obliquely under its component), never locked. The tool reads them again whenever the
  document's differ from what it last read or wrote (another target, an undo). The padlock
  is `PointShape::Padlock`, drawn by the app over the view as the grips.
- **A locked component is a block in a walk** (after OCL, which does it for the full mask, a
  locked instance, a glued or camera-facing one): below the target, an occurrence whose
  component is locked along the stretch axis — the axis brought into its own axes; for one
  turned, as soon as that axis has a component on one of its locked axes — is never stretched:
  it is **moved with the section holding its origin**, even across a cutter, and its bounds
  are added to that section's for the compression limit (OCL's `section_def.bounds.add`). Not
  a refusal. The preview follows the same rule.
- An occurrence lock (OCL's `locked?`, on the instance) is another thing, left for later.
- **File**: a `behavior` field on the component, absent when default; no version change (§8).

**The tool** (`crates/tools/src/tools/stretch.rs`; in the Solids group of the tool bar, after
the booleans; shortcut S). Rebuilt on 2026-11-11 on OCL's own interaction
(`SmartReshapeStretchActionHandler`: its states, `_snap_*` and `_preview_*`):
- **Pick**: the open part, else the one part occurrence selected, else the part clicked (the
  one under the cursor highlighted until then). A click on nothing, or Esc, lets it go.
- **Grips and cutters** (OCL's `STATE_STRETCH_START`): a grip at the centre of each face of
  the part's box (not drawn), a round point drawn over everything and taken near it on screen, seen
  or not — 30 px for the active axis's, 10 for the others' (the nearer the eye first when two
  coincide). Hovering a grip makes its axis the active one (the arrow keys lock one); the active
  axis is dashed between its grips in its colour, the others in grey. The active axis's cutters
  are translucent planes across the box, 50 px wider than it, drawn in the scene (hidden
  behind faces, as OCL's `view.draw`: over everything they were unreadable), taken inside on
  screen or near
  their outline: pressed and dragged along the axis, to an end of the box to remove one; the
  copy modifier held, the cutter to add follows the cursor and a click adds it; the other
  modifier held (Command on macOS, Alt elsewhere), a click removes the one under the cursor;
  none left, the axis is locked (« Behavior » above; one in the middle again where cutters are
  kept for the session). A click on a grip starts the stretch, or press, drag and
  release.
- **Interior handles** (OCL's, built 2026-11-11): with two cutters or more on the active axis,
  each section between two of them (not the end ones, which anchor) has a handle — its stretch
  of the line between the grips, taken anywhere along it within 20 px (after the grips, before
  the cutters), drawn when hovered only: bold, an arrow at each end, over everything. It is
  grabbed where an inferred point that tells a place along the axis is, projected on it (a
  diamond: `PreviewPoint::diamond`), else at its point nearest the cursor's ray (a round
  point). Shift held over it keeps it: its start is then taken anywhere, projected on its
  line, a dashed line from where it is taken. Moved by `d` along the axis, the sections take
  OCL's `interior_t_coefs` (`interior_shares`): 0 at the first end, rising evenly to 1 at the
  handle's, falling evenly to 0 at the other end — the overall length stays, so the measure is
  always the offset (typed: the way the cursor takes it), « centred » does not apply, and the
  compression limit holds both ways (`move_range`, unchanged). After it, the axis stays active
  to chain. Nothing new below the tool: `Operation::Stretch` already takes a move per section.
- **Stretching** (OCL's `STATE_STRETCH`): the part alone, drawn stretched (no box, no cutters),
  the measure line dashed with its ends and its length — from the opposite face for the
  overall dimension, from the grip for the offset. The cursor within 40 px of the start snaps
  back to it; else an inferred point that tells a place along the axis — a point, an edge across
  it, a face facing it, not the opposite face's plane, not the part itself — projected on it;
  else the closest point of the axis to the cursor's ray. Shift holds the last measure (its
  value; the way the cursor's), a double-click takes it again — kept per kind of measure; the
  copy modifier tapped centres, the other one tapped switches the scope (OCL's "make unique").
  A typed value out of range is said (the minimal length, the maximal compression). Then back
  to the grips on the same part, to chain.
- The sections move by OCL's profile: from 0 at the anchored end to the full move at the grip,
  proportional to their rank; centred, from minus to plus the move. Compression stops 1 mm
  before the matter wholly in two sections meets (`Stretch::section_spans`, OCL's section
  bounds). After the stretch the cutters are set again as OCL does (`StretchDef#cutter_ratios`,
  `kept_cutters`): one in the middle of each gap between the matter of two sections that hold
  some, once moved — each stays between the same vertices, however hard the compression (put
  back at the mean of its sections' moves, a cutter could end in a section's matter after a
  strong compression). A run of empty sections is one gap, one cutter; matter overlapping
  across a cutter leaves none there; none left, one in the middle.
- **Options**: the measure — overall or offset — and centred (saved), and the scope like the
  booleans' when no part is open (an open part has its own).
- **Refused at the click on a grip**: the operation for a 1 mm pull is prepared; a refusal
  (something curved across a cutter) is said, the stretch does not start.
- **Cutters** are saved with the component (« Behavior » above), as OCL saves them on the
  definition. The session store left for several occurrences is thread-local: the tools run
  on one thread, and each test on its own (a shared one let tests see each other's cutters,
  part ids being alike).

**Where the stretch applies: the recipe (§2.4), not the final B-rep.**
- The **blank** is stretched (B-rep stretch below).
- A **machining** whose tool lies in a single section is **translated** (its placement): exact,
  nothing deformed.
- A machining whose tool **crosses a cutter** has **its tool stretched** (a groove along the
  rail grows with it), in the tool's own frame — the cutters and the axis brought in by the
  inverse of its placement; the tool, shared with copied machinings, becomes this machining's
  own. If it cannot be stretched, the stretch is refused and the machining shown.
- The part is then computed again from its new recipe (the recipe task, asynchronous).
- The scope (§2.5): all occurrences, or only this one (a separate part). With one target, no
  divergence; the deformation signature of OCL (`ContainerDef#compute_md5`: section deltas and
  the sign of the local axis, so a mirrored twin matches) comes with several targets.
- With the parametric board blank of §2.10, if it comes, stretching the blank would be
  changing L and anchored machinings would follow; the sections stay for free blanks and for
  machinings.

**B-rep stretch: rewriting the record**, a pure function on `solidcore_record::brep::Brep`
(no kernel), a **piecewise translation** along the axis:
- an element (vertex, edge, face) entirely in one section is **translated** with it: points,
  curves and surfaces with their NURBS form; pcurves and domains unchanged;
- an element crossing a cutter must be **invariant along the axis**, and is lengthened:
  - an edge: a **line parallel to the axis** — its ends follow their vertices, its interval
    and its pcurves' knots grow;
  - a face: a **plane containing the axis direction** — the surface stays, each pcurve point
    shifts by `(Δ·u, Δ·v)` of its section (planes are affine in mm), the domain is taken again
    from the pcurves; or a **cylinder whose axis is parallel** — its NURBS net (SMLib's form,
    v = the height in mm) has its rows past the cutter moved, its v knots, domain and pcurves
    grown;
  - the kernel writes a mirrored solid's lines as degree-1 splines over ±1234567 and its
    cylinders as cones of half angle 0: both taken as what they are;
  - built in `kernel::Stretch` (`crates/kernel/src/stretch.rs`): `apply`, `in_frame` (a tool
    in its own frame), `extent` (along the axis: exact for lines and whole circles,
    conservative for curved faces);
  - anything else crossing a cutter (cone, sphere, torus, spline, intersection curve, a line or
    plane not along the axis) makes the stretch **invalid**: refused, as OCL refuses a curve
    crossing a cutter (`StretchSplitDef#sections_valid?`); moving the cutter usually solves it;
- the kernel reading the record back (the recipe task does it) validates the result; a refusal
  cancels the stretch.
- **Probed on 2026-11-11** (a test, not kept): a box stretched and compressed, a cylinder
  stretched and compressed along its axis (standing and lying), each read back by the kernel,
  then used as a blank and as a tool in a subtraction: all correct. Seen there: the box's
  planes carry no NURBS form and linear pcurves in mm; its lines run t ∈ [0, 1] from start to
  end, a cylinder's seam is parametrised in mm; a compressed cylinder needs its domain shrunk
  with its knots ("parameter interval outside the natural domain" otherwise).
- Rejected: cutting at the cutters, translating the pieces and filling the gaps by extrusion
  (`prism::section` + `extrude_region`) then union — slower, coplanar unions, and nothing for
  compression.

**Operation** (built 2026-11-11): `Operation::Stretch { target, axis, cuts, moves, scope }` —
the axis in the part's frame, the cutters' positions along it (`axis · p`), each section's
displacement (OCL's `edvs`). When prepared, the blank and each tool across a cutter are
stretched on the spot (a refusal refuses the operation then, before any kernel call); the job
then holds a `Task::Stretch` per solid — the same rewrite, read back by the kernel, which
checks it and meshes it — and the recipe task from the new blank. A machining in one section
(its tool's extent, `Stretch::extent` in its frame) only has its placement moved. Guides follow
their section when they have one (a line square to the axis, a circle about it); a line along
the axis stays, an oblique one too. The scope is that of every change to a part (§2.5).

**Preview** (built 2026-11-11): no kernel while dragging. The tool describes the stretch
(`Preview::stretch`: the occurrence, the scope, the axis, cuts and moves); the editor deforms
the part's display mesh on the CPU (`scene::stretched_mesh`) and keeps it in the scene under a
key of its own (`PREVIEW_KEY`, a new revision at each change: the app uploads it like any
mesh). The map is continuous along the axis: each section's move where it holds matter wholly,
a linear blend in between — the B-rep's vertices all lie in that matter, the mesh's other
points on what crosses a cutter, which any increasing map keeps. The triangles are first cut
where the map bends, so that it is affine on each piece: none turns over, however hard the
compression (moving each vertex by its own section turned long triangles over, as did the
continuous map on uncut ones — test `a_compressed_preview_turns_no_triangle_over`). For an
assembly, the editor sketches the document the stretch would make without its geometry
(`Document::stretch_sketch`: the same walk, parts left as they are): each part it stretches
gets a preview mesh (`scene::preview_key(i)`, the first `PREVIEW_KEY`), and each occurrence it
changes — stretched, moved, copied — is drawn from its mesh or at its new place
(`Editor::stretch_swaps`, by path: ids are kept), its highlights moved with it. Those
occurrences are not inferred on while stretching (`Cx::previewed`): they are no longer where
they were.
An open part showing its machinings is drawn as its shape, stretched, during the gesture.
The grips and the measure's ends are drawn by the app over the view (`Preview::points`); the
cutter planes and their outlines in the scene, depth tested (`Preview::fills`,
`Preview::scene_lines` → the renderer's `Frame::fills`, blended without writing depth, and its
depth-tested lines).
Once applied, the stretch stays drawn as previewed while the kernel computes it
(`Editor::held_stretch`, the tool's grips hidden meanwhile): else the part showed as it was for
a frame or more before its new shape came. Test `a_stretch_stays_drawn_stretched_while_computed`.

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
  again on top with its index range and that occurrence's matrix. A selected occurrence (**part
  or assembly**) also shows the bounding box of its content, in its own axes, dashed in the
  selection's colour and depth-tested (`View::selection_boxes`); it follows the move tool's
  ghost, and is not drawn with the stretch tool (which draws its own box). The assembly being
  edited shows the same box in grey, grown by a few pixels (`View::context_box`).
- **Scale** (decided on 2026-11-12, after a 200 × 200 array froze the app): what is drawn or
  listed per occurrence costs one GPU instance, never draw calls or CPU-built geometry. Runs of
  highlights of the same part are drawn in one instanced call; a box outline is one instance of
  the unit cube (`Frame::boxes`), its edges dashed by the shader from every corner; the outliner
  only lays out the rows in view. Lookups over a selection or an assembly go through sets or an
  index (`ContextIndex`), never a search per occurrence. The renderer keeps every buffer within
  the device's limit (what does not fit is dropped, the device is not lost).
- **Spatial index** (decided on 2026-11-12, after a stretch among 10 000 cubes fell to 30 FPS):
  no query per mouse move tests every placement. Each `DisplayScene::sync` rebuilds an index
  of the placed meshes (`scene.rs`, `SceneIndex`): their world boxes in a linear BVH
  (`bvh.rs`: Morton order, halved down to 4 items per leaf, built in O(n log n) by one integer
  sort), and the same entries sorted by occurrence path (an occurrence's parts, at any depth,
  form a run). Picking walks the BVH nearest box first and skips boxes beyond the best hit
  (plus its tie tolerance); inferences take the meshes near the ray (`inferable_where`), the
  rectangle selection those whose box seen meets the rectangle (`parts_where`; a window takes an
  entity only when every part under it, counted by `parts_under`, lies inside); lookups by path
  (`inferable_at`, `part_at`, `inferable_of`) search the sorted run. Results keep the scene's
  order (ties, candidates), as the brute force gave them (test
  `the_scene_index_picks_as_a_scan_does`). The stretch's preview compares the document it
  sketches with the current one by `Document::placed_changes`, which skips what both share.
- **Coincident faces** (decided on 2026-11-11): picking takes the nearest hit, but hits within
  `tie_tolerance` along the ray (1 µm + 1 ppm of the distance) are a tie, broken in a fixed
  order: the solid the ray enters (outward normal against the ray, from the vertex normals)
  before one it leaves (seen through a section, from inside), then the smaller bounding box,
  then the scene order. The choice no longer flickers from one pixel to the next and the same
  rule serves every tool (selection, inferences, paint, booleans). Reaching the other solid
  (cycling, a list under the cursor) is left for later.
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
  A `Line` may be **dashed** (`Dash`: length and phase in points): the vertex shader passes
  the distance along the segment on screen, interpolated linearly on screen, and the fragment
  shader discards the gaps. Lines that run past a parallel projection's depth range (infinite
  guides and axes) are clamped to it, not cut: the line pipeline uses `unclipped_depth` where
  the device has `DEPTH_CLIP_CONTROL` (`render::OPTIONAL_FEATURES`, which the app asks egui's
  device for).
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
     recipe, display) and returns placeholder solids, one per task: a placeholder handed to a
     later call names that task's output (`EARLIER | task`), so a primitive can be cut by a
     recipe in the same job. An operation with no kernel call (rename, group, move…) is
     **done at this step**, with no round trip;
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
- **Render statistics** (View › Statistiques de rendu, a preference): a card in the view's
  top right corner — frames painted over the last second (egui repaints on demand: a still
  view paints none), the interval between the last two, the CPU time of the editor's view and
  of the picture's preparation, whether the picture was redrawn, and `Renderer::stats` (draw
  calls, triangles, instances, lines, boxes, bytes uploaded) with the meshes' GPU memory.

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

- **Model**: `Document.materials: Id → Material { name, color }` (sRGB; later a `type`, the
  part's nature for the cut list, §2.1);
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

Components, occurrences, the parts' recipes (blank + machinings, with their `source`), the
components' guides (§6 quinquies). The final B-rep is not the source: it is saved as a cache
(`shape`), present exactly when the part has machinings, to open without recomputing.

**Version 1 until the app is published** (decided 2026-11-12): the format changes without a
new version and older files are not read — no converter, no reference files. Versioning and
backward compatibility start with the first release. (Files written before by versions 2 and
3, and the first model's converter, were dropped then.)

## 8 bis. SketchUp import

Decided and built on 2026-11-12, to compare loading and display with SketchUp on real models.
Formats weighed: STEP keeps exact geometry (SolidCore reads it, but does not apply assembly
instancing), mesh formats (OBJ, STL, glTF) lose faces or structure; SketchUp itself keeps the
structure one to one, which a performance comparison needs.

- **Export inside SketchUp**: `scripts/sketchup/export.rb` (run in the Ruby console or through
  the Claude Bridge) writes JSON `solidsketch-sketchup-export` v1: per definition its visible
  faces (shared vertices in mm, loops by index, outer first, normal) and instances (definition,
  name, 4×4 transform with its translation in mm, material), the model's own entities as
  `root`. Hidden entities and hidden tags left out.
- **Conversion** (`Document::from_sketchup`, `crates/model/src/import.rs`): a definition's faces
  make a part, its instances an assembly; both: an assembly whose first occurrence is the part
  of its faces (« Name (faces) »). An instance's transformation is split into the rigid T of its
  occurrence and a scale (or shear) **baked into the geometry**, the child definition taken at
  that scale: one component per (definition, scale), §2.2. `no_scale_mask` becomes the
  behavior's `no_stretch` (same bits, §6 sexies); groups' definitions are not `named`.
- **Solids** (`Solid::from_polyhedron`, `crates/kernel/src/polyhedron.rs`): the faces become a
  STEP neutral body (a plane per face, a line per edge shared by its two faces, one shell per
  set of joined faces) built by SolidCore's `step-brep` **from its topology only**
  (`build_from_topology`). Loops are turned to agree with the normals, then the faces of each
  lump turned alike across their edges (SketchUp keeps reversed faces in solids), the lump
  turned inside out if its volume is negative. **Only closed solids are kept**, checked
  before the kernel by counting (each edge between exactly two faces): a definition open,
  non-manifold or with a face the builder leaves out is excluded and reported (its instances
  dropped). Measured on 2026-11-12 (a 1373-definition apartment, 52 k faces): letting
  `build_body` fall back to stitching by geometry on the open ones took 100 s (up to 60 s for
  one definition of 2668 faces); the count rejects them at once, 1 s for the whole. Built in
  parallel, one request per (definition, scale).
- **Curves stay facets**: a hole of 24 segments is 24 planar faces, as SketchUp draws it (a
  comparison at equal geometry). Recognising `ArcCurve`s and smooth faces to rebuild exact
  cylinders may come later.
- **Not carried**: face materials, tags, hidden geometry, attributes (OCL's included).
- **Entry point**: `solidsketch-import export.json [out.solidsketch]` (`apps/import`), which
  prints counts and timings; the file then opens in the app like any other. An import from the
  app's menu may come later.

## 8 ter. STEP import

Decided and built on 2026-11-12. **File › Import…** (Cmd+I) places a STEP file (AP203 /
AP214 / AP242) **in the model**, in the active context, as one occurrence, selected, one step
to undo.

- **Read by the kernel**, as a job (`Task::ImportStep { bytes }` → `Output::Step`), so off the
  UI thread natively and in the worker on the web; the file is parsed once
  (`crates/kernel/src/step.rs`). SolidCore's `step-import` maps the B-rep bodies (units to
  mm); the **product structure** it leaves aside is read here from the same parsed file:
  `PRODUCT_DEFINITION`s and their shape representations (joined by plain
  `SHAPE_REPRESENTATION_RELATIONSHIP`s: the same axes), `NEXT_ASSEMBLY_USAGE_OCCURRENCE`s
  placed by their `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` (`ITEM_DEFINED_TRANSFORMATION`:
  `frame₂ · frame₁⁻¹`, the two representations told apart when a writer swaps them), and
  `MAPPED_ITEM`s (`target · origin⁻¹`). Bodies no product reaches are kept at the top.
  Strings are decoded (`\X2\…\X0\`).
- **Bodies**: built by `step-brep` (`build_body`, strict: topology first, else stitched), in
  parallel natively (`parallel_map`, with the kernel's stack). **Only closed solids**: a
  surface model, an open shell, a face left out or a body that does not close is excluded and
  counted in the status bar.
- **Colours**: a `STYLED_ITEM`'s surface colour (`COLOUR_RGB`, pre-defined colours), on the
  body, its shell, or shared by all its faces, becomes a **material** (one per colour,
  « Couleur #RRGGBB ») on the occurrences of its part. Face colours otherwise are not
  carried yet (a face's index in the built B-rep would have to be matched).
- **Model** (`Document::from_step`, `crates/model/src/step.rs`): a product with one body is a
  part; with several bodies, or bodies and products, an assembly holding one part per body
  (a woodworker's component of several boards is several parts); a product of products an
  assembly. Names come from the products (then the bodies), all `named`.
- **Insertion** (`Operation::Insert { context, fragment, transform }`): a whole document as
  the operation's data; its components and materials copied with fresh ids (a material equal
  to one already there is reused), names made unique. The fragment's single top occurrence
  is placed itself; several are gathered in an assembly named after the file. Generic: the
  SketchUp import, or inserting a `.solidsketch` file, may use it later.
- **Not yet**: face colours, placing the import at the cursor (it lands at the context's
  origin), cancelling a long read.

## 9. Toolchain and dependencies

- Rust **1.97.1** pinned by `rust-toolchain.toml` (the SolidCore crates ask for ≥ 1.88), with
  the `wasm32-unknown-unknown` target.
- `solidcore` is on crates.io, but `solidcore-record`, `-convert` and `-tess` are not: all four
  come from the git mirror `tangency-labs/solidcore`, **pinned to one commit**. Its known bugs
  are reproduced on SolidCore alone in `crates/kernel/tests/solidcore_quirks.rs`, each test
  asserting the bug is still there: run first on an upgrade (REVIEW, "Work: SolidCore").
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
3 bis. **Stretch** (§6 sexies): one part first, in its axes.
4. **Push/pull**: picking a B-rep face, offset (the kernel's `offset_faces`, or extrusion +
   boolean). The riskiest on the kernel side: last.

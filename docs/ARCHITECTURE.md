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
  **material**, its occurrence name and whether it is **hidden** (§6). Moving or rotating only changes T: instant, no kernel,
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
  and the open part's tools. It is SolidCore's tight box (its loose one should the tight one
  fail).
- Tools working on a part use its axes; elsewhere, the context's.
- Changing a part's axes = re-expressing its B-rep in another frame and compensating the T of
  each of its occurrences.
- **To come (§2.11, decided on 2026-11-14)**: the cut list reads the box of the **final**
  B-rep, not the blank's; the blank becomes placed bodies, independent of the part's axes.

### 2.4 Machinings

To come (§2.11, decided on 2026-11-14): blank and machinings become positive and negative
bodies, `Part = (∪ positives) − (∪ negatives)`.

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
- **Shown by default: the shape**, as anywhere else. A **"Machinings" switch** — a button in
  the option bar's "Options" group while a part is open, remembered in the preferences; neither a
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
  the option bar while it is open, its first choice: "All occurrences ×N" / "Only this
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
- **A non-rectangular blank** (decided on 2026-11-13, §6 nonies): a profile × a thickness,
  the profile kept in the blank's recipe — the board is the case of a rectangular profile.
- Open questions: is anchoring on the blank only acceptable (a hole in a chamfered face is
  then expressed against the box)? Should the axes
  tool snap to the 24 orientations on a parametric part, or stay free and offer the conversion
  only on an oblique angle?

### 2.11 Bodies: a part as positive and negative solids

Decided on 2026-11-14 (questions / answers). Branch `feature/part-bodies`, built in steps:
1. the model and the kernel (done, 2026-11-14): `Part { bodies }`, `Body { id, sign, solid,
   placement, profile, source }`; `Task::Recipe { base, positives, negatives }`;
   `Solid::pieces` (the record's bounded regions that are not voids); the operations on
   machinings made operations on bodies (`AddBody { sign }`, `DeleteBodies`,
   `TransformBodies`, `DuplicateBodies`, `ArrayBodies`, `PasteBodies`, `SetProfile { body }`,
   `SetSign`);
   the entity info's dimensions read on the final shape, « en N morceaux, à corriger » instead
   when it is in pieces. The interface otherwise as before: an open part shows its positive
   bodies as ghosts, its negative ones as its machinings; the Profile tool edits its first
   profiled positive body, in that body's axes;
2. the interface of bodies (done, 2026-11-14): in an open part, the drawing tools' bar has a
   « Matière » choice, « Enlever » (a machining) / « Ajouter » (a positive body), back to
   « Enlever » at each opening; the « Corps » switch shows positive bodies as
   ghosts, negative ones as solids, all picked (a negative one first, whatever the depth —
   it lies inside —, else the nearest positive one), selected, moved, copied and erased alike
   (`Piece::Blank(id)`); a double-click on a body with a profile (in Select or Profile) edits
   its profile (`Command::EditProfile`), the « Profil » button the first profiled positive
   body's; the entity info lists the bodies flat, « Corps (n) », blanks first, each numbered
   in its order of creation (kept when its sign changes) with a ＋/－ badge whose click
   toggles its sign — the selected bodies' too when it is one of them
   (`Operation::SetSign { target, bodies, sign, scope }`, the last positive kept, the part
   computed again). The interface says « corps » for any body, « brut » (a blank) for a
   positive one, « usinage » (a machining) for a negative one; no tree of the two kinds, in
   the entity info nor in the outliner: the model is a flat list with a sign, the order
   meaningless, and the outliner keeps to occurrences. Machinings are ghosts too, red, a
   little more opaque than the blanks (`Theme::machining_opacity`, the placement's tint
   weight added to the ghosts' opacity); one that failed stays opaque, in its own colour. A
   body hovered or selected stays see-through: its highlight washes its faces with the
   mark's tint over the ghost's own (`Highlight::ghost`, `fs_face_ghost_mark`), its edges as
   any highlight's. A part in pieces gets
   ⚠ and its name in the error colour in the outliner; the union's warning
   (`Outcome::warning`, `Warning::UnionCut`) is said in the status bar, worked out from two
   volumes (`Task::UnionVolume`: the target and the tool joined before, the result after).
   Not done: the positive bodies apart drawn in the error colour (which lump a body lies in
   is not known cheaply). Then (2026-11-14): a difference whose tool is its single positive
   body (a shape drawn) takes that body whole as the negative one, its profile with it — still
   editable; the « Profil » button edits the body selected when it has a profile. The
   Profile tool's thickness grip was dropped: the Stretch tool along the body's Z does it
   better (snaps, centred, offset or overall, cutters), the Profile tool keeps the outline;
3. stretching bodies (2026-11-14): in an open part showing its bodies, the Stretch tool takes
   the bodies selected (`Operation::StretchBodies { target, bodies, frame, … }`, the stretch's
   axes placed in the part's): along the body's own axes (one, « entity »), the part's
   (several, or « context »), or the model's; each body in one section moved, across a cutter
   stretched in its own axes, its profile kept when it stays one; the guides selected follow
   their section. Their cutters are kept for the session only (nothing on the body in the
   file). The preview draws each body stretched from its recipe mesh (`Editor::body_swaps`);
4. body definitions (2026-11-14): a body is an occurrence of a definition of its part, as an
   occurrence is of a component — copies made by Move or Rotate with a copy, an array, or a
   paste into the part copied from share it, a change to the shape (its profile, a stretch)
   shows in every one. See **Body definitions** below.

**Why**: a profiled blank (§6 nonies) is bound to the part's axes — its outline in their XY
plane, extruded along their Z. Changing a part's axes (§2.3) would then have to transform the
profile, and drop it whenever Z ends up in the plane or oblique. Giving the blank its own
placement in the part frees the profile from the part's axes; going one step further, the
blank and the machinings become one notion: a placed solid with a sign.

```
Part = (∪ positive bodies) − (∪ negative bodies)

Part { bodies: Vec<Body> }                          — replaces blank + profile + machinings
Body { id, placement: T, sign: Positive | Negative, source }
  source = Profile { profile, z0, height }          — §6 nonies, in the body's own axes
         | Brep(Arc<Brep>)                          — a free solid (import, intersection…)
```

- **Every body is placed in the part's axes**, positive or negative (the machinings stay
  there, as today). A profile lives in its body's own axes (its XY plane, its Z), never in the
  part's. Today's blank is one positive body at the identity; today's machinings are the
  negative bodies; a free part (import, boolean result) is one positive `Brep` body.
- **The part's axes are independent of its bodies.** Changing them (§2.3) = correcting every
  body's placement and compensating the T of each occurrence: exact, for any rotation, no
  kernel, the profiles never touched. This supersedes the quarter-turn discussion of §2.10 for
  profiled bodies.
- **A fixed formula, no history**: the union of the positives minus the union of the
  negatives. **Every negative cuts every positive**, those added later included; the order
  bodies were made in does not matter (§2.4 stays true). A hole is filled by deleting its
  negative, not by adding a positive into it.
- **Incremental computation**: adding a negative = one subtraction on the cached final, as
  today; adding a positive `P` = `final ∪ (P − ∪N)` (since `(A ∪ B) − N = (A − N) ∪ (B − N)`);
  a part with a single positive, the common case, makes no union at all. Removing or moving a
  body computes the part again from the union of the positives (worth caching).
- **Failures**: a body whose boolean fails is skipped and reported by its id, the others still
  computed, as a failed machining today (`Part::failed`).

**The cut list reads the final** (supersedes §2.3's blank box): the part's dimensions are the
exact box of the **final** B-rep in the **part's** axes, the grain along their red axis. A
machining that shortens or narrows the part changes its dimensions. The exact outline for the
DXF / SVG exports and nesting is read from the final B-rep too (its faces across the part's Z,
as OpenCutList does), not from the profiles: **a profile is an editing parameter of its
body**, it carries nothing for the cut list (supersedes §6 nonies on that point).

**One piece of wood**: the final must be a single lump. It is not when a positive touches no
other, or when negatives split the part (a groove right through). The part is then **in
error**:
- marked with a warning in the outliner and the cut list, « in N pieces », and shown whole so
  the problem can be seen;
- the positives apart — those outside the lump holding the first positive — drawn and listed
  in the error colour, as failed machinings; a split made by negatives blames no body, only
  the part is marked;
- **no cut-list dimensions** while it is in error: « to correct » instead.

**Stretch** (§6 sexies) goes through the bodies as it goes through an assembly's components:
each body is stretched in its own axes and keeps its profile under §6 nonies' rules,
evaluated there, or becomes a free `Brep` body.

**Booleans between parts** (supersedes §4 on these points; the tool brought into the target's
axes by the relative transformation, the tool occurrence leaving the model, the scope of §2.5,
as today):
- **Union**: the target receives **all the tool part's bodies, positive and negative**, their
  placements composed with the relative transformation; nothing is computed into a new blank,
  every body stays editable. The tool's negatives then cut the target as well (above): a
  through-hole's tool, longer than its part on purpose, drills the target where it enters it.
  **A warning** says so when the tool's negatives remove material from the target.
- **Subtract**: as today, the tool's final shape becomes one negative `Brep` body of the
  target (its bodies are not kept).
- **Intersection**: for now a new part with a single positive `Brep` body (the common
  volume). To think over later.

**Body definitions** (decided and built on 2026-11-14): bodies are moved, copied and arrayed
with the tools that do so to occurrences, so they share their shape as occurrences share their
component.

```
Part { defs: Vec<BodyDef>, bodies: Vec<Body>, shape, failed, faces }
BodyDef { id, solid, profile, source }      — the shape, in its own axes
Body { id, def, sign, placement }           — an occurrence of it, placed in the part's axes
```

- **The sign is the body's**, not the definition's: one shape can be a dowel (positive) here
  and its hole (negative) there; the ＋/－ badge flips the body clicked.
- **Definitions belong to their part** (not to the document, for now): copies, an array, a
  paste into the part copied from share a definition (`copies_of_bodies` gives another part
  copies of the definitions, still shared between its bodies — a union, a paste elsewhere, a
  difference's plain tool); a definition no body uses any more is dropped.
- **A change to a shape changes the definition**: `SetProfile` edits the definition, every
  copy shows it; **« Rendre unique »** (`Operation::MakeBodiesUnique`, the command
  `edit.make_unique` in an open part) gives each body selected a copy of its own.
- **Stretch** follows the components' rules (§6 sexies, `Part::body_stretch`): bodies of one
  definition stretched alike (the same stretch in their axes) share the new shape; the change
  most of them take is made in place (the copies not reached show it), the others go to new
  definitions — all of them do when a body reached keeps the shape (only moved). The preview
  shows the same split. Unlike components, a definition changed in place is not moved back
  by its anchor: the copies not reached show the change in their axes as computed.
- **Display and file**: one mesh per definition (the recipe's key), the copies of a selected
  body marked « related »; the entity info shows `×N` on a shared body; the file writes each
  definition once.

**Format**: version 1 until the release (§8), no compatibility to keep.

**Open**:
- The open part (§2.9): positives selected, moved, turned, deleted and their profile edited
  as the machinings are; how the « Machinings » view shows them (ghost positives, solid
  negatives?) and where the profile grips go when several positives have one.
- The rule naming the « main » lump (the first positive's, above) is a proposal.
- The profiled machinings (worktree `solidsketch-profile-machining`, uncommitted) are set
  aside (2026-11-14): they are negative bodies with a `Profile` source, to build on this model.
- Generators: a carcass (an assembly of parts, one per wall of an initial solid) is not a part
  and stays outside this model — a generator of assemblies whose parts are made of bodies, to
  discuss separately. The procedural track (§2.10) stays open: a parametric source is one more
  kind of `source`.

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
  the machinings of both parts are integrated. To come (§2.11): the union gives the target all
  the tool's bodies, the intersection a new part with one positive body.
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
  trackpad: two fingers = orbit, Shift+two fingers = pan, pinch = zoom. Z stays vertical (the
  Z of the last standard view's frame, below).
  Perspective projection by default, switch to parallel, standard views (top, front, right,
  back, left, iso) and zoom extents.
- **Standard views are in the active frame** (decided on 2026-11-14): the open part's axes,
  else the entered context's, as the axes drawn (§2.3; `Editor::axes_transform`, made
  orthonormal, its scale dropped). The frame's Z is up on screen; a mirrored frame is seen as it
  is drawn (the top view of a left-handed part has its red axis to the left). The camera keeps
  **the frame of its last standard view** (`Camera::frame`; yaw and pitch are measured in it):
  **the orbit keeps that frame's Z up**, the part closed as well, until the next standard view
  (Cmd+1…6 at the model's level brings world Z back). Levelling on world Z at the first orbit
  was tried first and dropped: the view rolled at once, however small the drag. In a mirrored
  frame the orbit turns its yaw the other way, so the model turns the same way on screen. The
  framing is unchanged (the scene's bounds).
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
  context, the model is faded — or hidden, with the document setting `display.hide_rest_of_model`
  (command `view.hide_rest_of_model`, « Masquer le reste du modèle », off by default): while an
  assembly is entered or a part open, what lies outside it (its parts, its guides, the open
  part's other occurrences and their `Related` marks) is neither drawn, picked nor inferred
  on, and zoom extents frames the context alone.
- **Hiding** (decided on 2026-11-12, after SketchUp's `hidden`): an occurrence carries
  `hidden` (saved, `Operation::SetHidden { context, entities, hidden }`, undoable; « Masquer »
  / « Afficher »), toggled by the eye at the right end of each outliner row. One flag per
  occurrence, not per component: a leg of four is hidden alone; an occurrence inside an
  assembly is hidden in every occurrence of the assembly (its definition is shared). A hidden
  occurrence, and everything under it, is neither drawn, picked (click or rectangle),
  inferred on, framed by zoom extents nor taken by select all; its guides go with it; the
  outliner greys its row and those below, and hovering them there marks nothing. Hiding it takes it out of the selection; the
  outliner still selects it (its selection mark then shows where it is). **What is being
  edited shows**: the occurrences hidden along the active path (the assemblies entered, the
  part open) count as shown, what is hidden below them stays hidden
  (`PlacedPart::hidden_at`, the deepest hidden occurrence of the path, compared with the
  active path at each `DisplayScene::sync`). The document setting `display.show_hidden`
  (command `view.show_hidden`, « Voir les éléments masqués », off) draws them faded and lets
  them be picked, inferred on and framed; `view.show_all` (`Operation::ShowAll`, « Tout
  afficher », enabled while something is hidden) shows every occurrence again. Exploding a
  hidden assembly leaves its content hidden; copies keep the flag; imports (STEP, SketchUp)
  come in shown.
- **Eraser** (« Effacer », E, after SketchUp's; decided on 2026-11-13): it takes what a click of
  Select takes — a guide near the cursor, else a machining of the open part while they are
  shown, else the active context's occurrence under it — and nothing outside the context. A
  click erases at once; a drag gathers everything it passes over (sampled every 4 px between
  two cursor positions, so a quick stroke misses nothing) and erases it on release, in one
  operation (one undo step); Esc drops the drag. With Shift held on release, the occurrences
  taken are hidden instead (`SetHidden`), guides and machinings left. The selection is left
  alone (activating the tool does not erase it, unlike SketchUp). What would go is drawn as a
  ghost (`Preview::erasing`): translucent faces as an open part's blank, and translucent edges
  and silhouettes too (`ViewPlacement::faint_edges`, drawn by `render` after the ghosts' faces,
  at the theme's `ghost_edge_opacity`; the blank keeps its edges opaque), in batches of its
  own, its selection marks dropped; erased guides are
  drawn faded. The command `edit.erase` (Delete, the Edit menu) erases the selection as before;
  both build the same operation (`Cx::erase`).
- **A drag is click + move + click** (decided on 2026-11-15, after OpenCutList's SmartDraw,
  `smart_draw_tool.rb`): in Box, Cylinder, Prism and Turn, pressing at the first step and
  dragging places the first point where the button went down, the preview follows the cursor,
  and releasing is the second click (the base's corner, the radius, the next corner, the axis's
  end). Released less than 20 px from where it went down, it is only the first click (a still
  press longer than egui's click time too); a press that took nothing (the tool still at its
  first step) makes the release take nothing either. At a later step, a drag is a click where
  the button is released. Esc during the drag, or another tool, drops the release. Extended the
  same day to the tools with points to place: Move (SketchUp's drag-move: from the press to the
  release), Rotate (centre, then reference), Measure, Section (face, then depth), the booleans
  (target, then tool); Paint takes the release as its click; Copy along a line or a grid and
  Distribute keep their selection rectangle while picking, then take a drag as clicks (Distribute:
  start at the press, end at the release; the copies: placed at the release). Select, Eraser,
  Stretch and Profile keep their own drags. The editor does the translation (`DragStart` →
  hover + click at the press point, `Drag` → hover, `DragEnd` → hover + click), for a tool
  whose `Tool::drag_clicks` asks for it (`DragClicks::PressAndRelease` / `Release` / `No`);
  the tools only see hovers and clicks. One difference with OCL: the first point is placed on
  the press (egui's drag start), OCL waits for the release or the 20 px; the result is the same.
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

  **Typed into the preview's lengths** (decided on 2026-11-14): while a value is typed, the
  length bubbles show it, not only the box. A `PreviewLabel` says which value of `Tool::vcb`
  it measures (`slot`); `vcb::typed` gives each value the item typed for it — a `;` moves on
  to the next bubble (the rectangle's red, then green side) — with what it reads as when that
  says more (`2*@ = 100 mm`, but `600`, not `600 = 600 mm`), greyed after the text typed so
  the two are told apart. An item that does not read yet shows as typed. A value repeated
  by `=` shows `= <result>`, greyed. The bubble being typed into is framed 2 px with a fixed
  caret, right after the text typed (before the result). Already typed bubbles keep their text and the others keep
  the cursor's measure. The preview's geometry still follows the cursor: Enter applies the
  text. A value with no bubble on screen (the height typed with the base, a count `5x`, a
  point `[x;y;z]`) shows only in the box. A bubble too short to write (a move not started) shows
  only while it is typed (`hidden`). Every drawing tool ties its bubbles: shapes (with
  the prism's bulge and tangent radius), Move, Rotate, Stretch, the tape, and the Profile tool, whose
  grips got bubbles for this (way moved, bulge, fillet, radius, thickness).
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
  (anchored cards): **the workspace switcher and the ribbon across the top** (§6 septies,
  2026-11-13; formerly a tool bar centered at the top), the tool option bar centered under the
  ribbon; **Entity info**, **Materials** and **Outliner** are three
  cards docked in two columns, one per edge of the view (`dock.rs`, 2026-11-12). A card's header folds
  it (click) or moves it (drag: it follows the pointer and drops into the column of the nearer edge,
  where a bar shows the insertion point); the View menu closes or shows each one. A card keeps the
  height given by dragging its bottom edge, except the last unfolded list of its column (Materials or
  Outliner; never Entity info, which would only grow empty), which fills the height left; the inner edge sets the column's width (double click: default).
  Order, columns, sizes, folded and closed cards are kept in the preferences, written once the
  pointer is released. Opening or resizing a panel therefore never resizes the view; zoom
  extents frames the area left free.
- **Original SVG icons** (drawn for the project), white and tinted by the theme, loaded by
  `egui_extras` (`svg`): no build step, no binary asset.
- **SketchUp shortcuts**: Space = Select, E = Eraser, M = Move, Q = Rotate, G = Make assembly, Delete =
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
  - **document settings** (`model::Settings`): display (grid — hidden by default —, axes…)
    and units (length unit, decimals). Saved in the file; changing them is
    an operation (`Operation::SetSettings`), hence undoable, and successive changes merge into
    a single step. Not the view's background: a token of the palette (§7 quater, decided on
    2026-11-13 — a file never sets how the app looks). Lengths stay stored in mm: the unit only serves for input
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
  - **Look** (2026-11-13): the bar's buttons are drawn like the ribbon's (no frame until
    hovered, the selection's fill and outline when on), an icon's button square. A choice is
    a **segmented control**: its values side by side, drawn like the ribbon's buttons (no
    track, no outline around the group), the chosen one's mark — a selected ribbon button's
    fill and outline — sliding to the value chosen, easing out in 0.18 s (`panels::segmented`).
    Each choice comes after its name (no colon, grey like a caption), the groups apart by
    separators; the lone
    toggles come last, together in an **"Options"** group, without separators between them.
  - **Two kinds of options**: **settings** (the scope…), remembered per tool in the user
    preferences (`tool_options`), read by the tool from its `Cx`; and the **gesture's state**
    (copy, for Move and Rotate), which the tool holds itself and which is not remembered.
  - First options: the booleans' scope (all occurrences / this one); copy (Move, Rotate).
    Later: axes (model / part, §2.3), keep a boolean's tool (OCL's `keep_cuts`), options of
    push/pull and of machinings.
  - **Length options** (2026-11-13, for the shapes' offset): `OptionValue::Length` (mm), a group
    of the bar (its name as the caption), before the choices, its value in the document's units
    in a text field always framed, its text right-aligned (the field's ground and an outline; since 2026-11-14, before
    a button turning into a field on a click): the outline alone tells the state — the border's
    colour at rest, the hovered ink's, the selection's when not zero, twice as thick when
    focused. Focused, all of it selected; Enter applies, Esc or a click away puts the value
    back. Read like the measurements box
    (`measure::coordinate`, `@` the current value). No modal dialog, unlike OCL. The
    measurements box sets it too, at any step: a text ending in `x` (`3x`, `-2mmx`) goes to the
    active tool's first length option (OCL's `_read_offset`), unless the box takes a count of
    copies (Move's `5x`).

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
- **The move's line** (start point → cursor), **the rotation's radii and the tape's measure**
  (or its offset, 2026-11-14): seen
  through what lies between — plain where they are seen, dashed where faces hide them
  (`Preview::xray` → `Frame::xray`); the protractor stays over everything. Move writes its
  distance at the middle of its line, Rotate its angle on the bisector of its radii, just past
  the protractor (`PreviewLabel`, as Stretch). Move's line and the tape's are arrows,
  their head at the cursor (the destination, the point measured to, the guide's side): heads `ARROW_PX` long on screen,
  in the plane facing the eye (`tools::arrow_head`), drawn over everything. The
  renderer draws such a line plain with the depth-tested lines (depth written), then dashed
  with the depth test reversed (`CompareFunction::Less`, the depth being reversed: what
  something nearer hides), blended, no depth written; where it was drawn plain, its own depth
  keeps the dashes off.
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
  step. Before the first click a small shape at the cursor shows the plane (OCL's
  `_preview_shape_start`); with an offset or a fillet it is drawn dashed and the shape made
  solid, offset out or in by a tenth of its width and rounded: the signs, not the sizes. The
  lines take the colour of its normal's axis (magenta for another locked one) and the sides being drawn get
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
  The offset (decided on 2026-11-13, after OCL's `SmartDrawShapeActionHandler`, option
  `shape_offset`): a saved length option of each tool (Box and Cylinder each their own, unlike
  OCL where the draw actions share it), set in the bar or by typing `<length>x`. It moves the
  outline only, never the height: the box's base grows by it on every side (sharp corners),
  the cylinder's radius by it (in Diameter mode too); negative, it shrinks them, and a shape
  left without width says so and is not made. The lengths written and typed stay those of the
  shape drawn (clicked on a face's edges, typed); while it is not zero the drawn shape is
  dashed and the one made is drawn solid, at every step.
  The box's fillet (added 2026-11-14, the Prism's « Congé », §6 nonies): a saved length option
  of its own, after the offset, set in the bar or by typing `<radius>r`. It rounds the four
  corners of the base made (offset included): the box is then made a `Primitive::Prism`, its
  profile the rectangle with the radius on each corner (recorded as radii, as the Prism's).
  At most half the shorter side made (a stadium; within 1 µm, the clicks' rounding); over
  it, refused and said, the height step kept to change it. Previewed as the offset is.
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
- **Tool names** (`tool.<tool>.name`, also the commands' and ribbon buttons' labels): a verb in
  the infinitive whenever the tool is an action — « Sélectionner », « Effacer », « Dessiner une
  boîte », « Tourner », « Copier en ligne », « Unir », « Mesurer » (decided on 2026-11-15) — a
  noun only where no verb fits without ambiguity (« Plan de coupe »: « Couper » is the clipboard's
  Cut; « Profil »).
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
    square to the edge (for an arc, along the point clicked's radius, wherever the cursor is around; for a circle centred on the point clicked, along the radius through the cursor). Pointing at nothing,
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
  copies — unless the tree also reaches the component unchanged (an occurrence only moved,
  or anything below one): then every change goes to a copy and the component keeps its shape
  (OCL counts moved instances among the stretched ones, `make_unique_c`; fixed 2026-11-14:
  two occurrences of one part in one assembly, the cutter across one only, both changed);
  under « only this one », the path is made the target's own (`separate`), then a
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
    grown; or a **NURBS net extruded along the axis** (same form, as the face's own surface:
    SolidCore writes a prism's arc sides — fillets included — so), stretched the same way;
  - the kernel writes a mirrored solid's lines as degree-1 splines over ±1234567 and its
    cylinders as cones of half angle 0: both taken as what they are; a prism's upright edges
    are degree-1 splines on their own length: a spline (or a two-point polyline) across a
    cutter has its knots and points grown with its interval, else it is no longer read back
    ("parameter interval outside the natural domain") — found 2026-11-14, a filleted prism
    could not be stretched along its height;
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

## 6 septies. Workspaces and ribbon

Decided on 2026-11-13 (questions / answers), after SolidCraft's ribbon (`ui-kit/src/ribbon.rs`,
`ui-egui/src/toolbar_custom.rs`), itself after the workspace / tabbed-toolbar pattern of CAD
tools (CATIA and FreeCAD workbenches, SolidWorks' CommandManager, Fusion). The pattern is free
to use; the drawing is ours: our icons, palette, names and proportions — nothing traced from
another product.

- **Layout**: two floating cards across the top of the view, under the title bar: the
  **workspace card** at the left (the workspace's icon and name, ▾ opens the list of
  workspaces) and the **ribbon card** (a row of tabs over a row of groups; the tabs drawn as the title bar's
  file tabs: centered on a band in the headers' fill, the shown one opening onto the groups), as wide as its
  content and centered on the view; it never covers the workspace card nor passes the right
  margin, so a narrow window narrows it (then *Fitting* below decides what gives way). The view is never resized (§6 bis); the dock's columns start under the ribbon;
  the **tool option bar** becomes a small card of its own, centered under the ribbon; zoom
  extents frames what is left below it.
- **Workspaces**: *Modelling* only for now. *Cut list* (the OCL-like parts list) and *Drawing*
  are planned: the data model has room for them, but they are not listed until they exist. The workspace shown is a user preference, not a
  document setting.
- **Tabs of Modelling**, each a list of groups, each group a list of commands of the registry
  (§7 quater) — a command may be in several groups:
  - **Build**: Create (Box, Cylinder), Modify (Move, Rotate, Stretch), Combine (Union,
    Difference, Intersection), Appearance (Paint);
  - **Assemble**: Components (Make assembly, Explode, Make unique), Modify (Move, Rotate);
  - **Inspect**: Measure (Tape measure, Delete guides), Section (Section plane, Clear
    section), View (Zoom extents, the standard views).
  - A fixed **Selection** group (Select, Eraser) stays at the left of every tab, before the
    tabs' own groups.
  - The tab shown is remembered per workspace (preference). A shortcut runs its command
    whatever the tab: the ribbon does not switch tabs; a running tool whose button is not shown
    lights its group's label instead.
- **The ribbon is data**: a `const` description in the app (`ribbon.rs`: workspaces → tabs →
  groups → command ids, with each group's default pins), checked by a test against the
  registry (every id exists, no group is empty). The ribbon draws it; state, labels, tips and
  enabled/checked come from the registry's `CommandView`s, as for the menus.
- **Pins**: a group shows its **pinned** commands as buttons; its label (with ▾) opens its
  menu, which lists **every** command of the group, each row with a pin to pin or unpin it.
  Right click on a button: Unpin; right click on the ribbon: Reset this tab (its groups and the
  fixed group, keyed `"<workspace>/<group>"`), and the labels' switch. Order is the
  group's order (no drag and drop in this first version). A group with no pin is drawn as a
  single button opening its menu. Preference `ribbon_pins`: `"<workspace>/<tab>/<group>"` →
  the pinned ids, only for groups the user changed (absent = the defaults; unknown ids ignored).
- **Labels**: a button shows its icon over a short label (two lines at most, ≈ 86 pt groups);
  the preference *Ribbon labels* turns them off for a compact ribbon (≈ 62 pt), the name then
  in the tip. A group's label is under its buttons in both cases.
- **Narrow window**: a pure function `fit(groups, room) → plans`, tested without display, after
  SolidCraft's: (1) buttons narrow, labels wrapping then truncating; (2) groups hand their last
  buttons to their menu one at a time, down to one collapsed button; (3) the last groups go
  into a » menu at the end. It depends on the width only (no state from earlier frames): no
  flicker. Nothing is ever unreachable.
  A button is never narrower than its label's longest word: labels wrap between words only.
- **Icons**: drawn for it — Modelling (workspace), pin, Clear section, Delete guides
  (`icons::command` maps a command to its icon, a test checks every command of the ribbon has
  one); the standard views share the views icon; the chevron and » are painted.
- The render statistics card sits in the top right corner of the area the cards leave free
  (`Editor::free_area`), under the ribbon.
- **Code**: `apps/solidsketch/src/ribbon.rs` (description, `Prefs`, `fit`, drawing, tests);
  the old tool bar (`panels::toolbar`) is gone.
- **Texts**: `ribbon.*` keys of the catalog (workspaces, tabs, groups, menus' notes), without
  ellipsis (§6 quater).
- **File tabs** in the title bar (several documents open, as SolidCraft): §6 octies.

## 6 octies. Several documents: file tabs

Decided on 2026-11-13 (questions / answers), after SolidCraft's document tabs
(`crates/ui-kit/src/app_bar.rs`: `TabStrip`, `DocTab`, `tab_layout`; `crates/ui-egui/src/documents.rs`).
The pattern is a web browser's; the drawing is ours.

- **Documents**: the app holds `docs: Vec<Doc>` and `active: Option<usize>` (`None`: the start
  page). A `Doc` is everything that belongs to one document: its `Editor` (history, scene,
  camera, selection, tool and its steps, open part, section), its `path`, `saved_at`, the
  measurements box's text, the Entity info field being edited, and a `DocId` (a counter, never
  reused while the app runs). Every document is handled alike, shown or not: no swapping in
  and out of the app as SolidCraft does (there the active design's state is spread over the
  app, so each new per-design field must be added to the swap by hand; here the type holds
  it). At most **32** documents.
- **Global, not per document** (preferences, as today): the dock's layout, the workspace and
  ribbon tab, the tools' settings, theme, navigation. The model's settings stay in the
  document.
- **Code**: `apps/solidsketch/src/docs.rs` (`Doc`, `Docs`: open, close, move, cycle — tested
  without display), `tabs.rs` (the strip, `layout` and `drop_place` tested), the closing and
  quitting in `app.rs` (`Closing`, `advance_closing`, `save_prompt`); `kernel::SharedKernel` /
  `KernelHandle` (`crates/kernel/src/shared.rs`); `Renderer::set_document` / `drop_document`.
- **Kernel: one service, shared** (§7 ter): one thread on the desktop, one Web Worker in the
  browser (one SolidCore wasm loaded, not one per tab). A multiplexer hands each `Editor` a
  `KernelHandle` implementing `KernelService`: it tags its jobs with its document and polls
  only its own results. `Editor` is unchanged. One queue for all: a document in the
  background keeps computing. A worker crash fails the work in flight of every document that
  had some, as today.
- **Rendering: one `Renderer`, caches per document.** Definition ids restart at 2 in every
  document, so meshes are keyed by document and id, and the material palette slots
  (`Viewport::slots`) are per document; closing a document frees them. Switching tabs uploads
  nothing.
- **Tabs in the title bar** (§7 quinquies), **in place of the document's name**: between
  Undo / Redo and the view's toggles, `+` at the end. A tab shows the file's name (« Sans
  titre » unsaved), a bullet when modified, its path as a tooltip; × on the active and the
  hovered tab. Tabs shrink to 90 pt, then a window of tabs around the active one is shown and
  the others are in a **»** list — a pure function of the width (`tab_layout`), tested. On the
  macOS desktop the empty space around them still moves the window; dragging the only tab, or
  a tab mostly up or down, moves it too (told once, as the drag starts). AppKit's own drag in
  its hidden title bar is turned off (`NSWindow.movable = NO`, `app_drags_window` in `lib.rs`):
  it moved the window from any drag in the top 30 pt or so, a tab's included, before the app
  could tell; `StartDrag` (`performWindowDragWithEvent:`) still works on such a window. Web, Windows, Linux: the same tabs, no window moves.
  The window title is the active document's name.
- **The shown tab** opens onto the view: it runs down to the bar's bottom edge, filled with the
  view's background (`UiTokens::view`), rounded at the top and curving out into the edge at the
  bottom (`tabs::Ground`); its text is `UiTokens::view_text`. The bar draws its own bottom line (egui's panel line would
  cross the tab), under the tab.
- **Interactions**: click selects, × or a middle click closes, `+` opens a new document, drag
  reorders (the tab follows the pointer, lifted — shadow, panel fill, accent outline, unlike the
  shown tab — while the others slide aside to open its place: `tabs::places`), right click: Close, Close others, Close all, Show in Finder / Explorer (desktop).
  Shortcuts: ⌘W / Ctrl+W close tab (`file.close`), ⌘Q (`app.quit`, the app menu's Quit on
  macOS: AppKit's own would end the app without asking), and, fixed whatever the shortcuts,
  Ctrl+Tab / Ctrl+Shift+Tab next / previous tab (`view.next_tab`, `view.previous_tab`, in
  the View menu) and ⌘T / Ctrl+T beside ⌘N (New, in a new tab). No renaming in place: a tab is
  named after its file.
- **Opening**: a file opens in a new tab, except when the active one is blank (never modified,
  no path): it is replaced. A file already open selects its tab. New (`+`, ⌘N, ⌘T) always
  adds a tab. A STEP import goes into the shown document, as before (§8 ter); from the start
  page, into a new one. Recent files stay a global preference (desktop).
- **Reopening** (desktop): the saved documents open when the app quits are opened again at
  launch, the last active one selected — preference *Reopen documents at launch*, on by
  default (`open_files` in the preferences, kept up to date as tabs open and close). Unsaved
  documents are not kept.
- **Closing a modified document**: « Enregistrer les modifications de « X » ? » — Save / Don't
  save / Cancel. Save goes through the save dialog when it has no path, and the tab closes only
  when that document's save is done: file dialogs' outcomes (`file::Outcome`) carry the
  `DocId` they belong to (the user may switch tabs while a dialog is open). Close others / all
  and **quitting** (⌘Q, the window's close button: `close_requested` + `CancelClose`) ask for
  each modified document in turn, its tab selected; Cancel stops the whole batch. Web: the
  page asks before it is left (`beforeunload`) while a document is modified.
- **Start page** (`active == None`): when the last tab closes, and at launch when there is
  nothing to reopen. Buttons New, Open, Import STEP, and the recent files (desktop). No tab is
  selected; the ribbon, the dock, the option bar and the status bar's measurements are hidden;
  the commands that need a document are disabled (menus, palette, shortcuts).
- **Control channel** (§7 quater): the commands act on the active document; `docs.list` (name,
  path, modified, active), `docs.switch`, `docs.new`, `docs.close` (`force` skips the
  question) are added.
- **Texts**: `tabs.*` and `start.*` keys of the catalog, without ellipsis (§6 quater).

## 6 nonies. Prism tool and profiled blanks

Discussed on 2026-11-13, after OpenCutList's `SmartDrawPolygonActionHandler`
(`tool/smart_draw_tool.rb`). Branch `feature/shape-polygon`.

**The sketch lives in the part** (decided): no free-standing sketch entity in the model, no
live link between a sketch and the parts made from it (that would be a parametric history, §1).
(To come, §2.11: the profile belongs to a positive body placed in the part, and the cut list
and exports read the final B-rep, not the profile.) A profiled blank is `Profile × thickness`,
the profile stored in the blank's recipe; to change the outline later, the part is opened and
its profile with it (as its machinings, §2.9). The cut list reads the thickness along the
part's Z and the length and width from the profile's box, and gets the exact outline for DXF /
SVG exports and nesting, without reading it back from the B-rep. (A prism drawn in an open part
is a machining already, §2.9.)

**Steps**: 1. the `Prism` primitive (straight sides) and the Prism tool (first named Polygon),
faithful to OCL (this section); 2. arcs in the profile and the tool (below); 3. the profile
edited in an open part, the part kept parametric (below) — all three built on 2026-11-13.
Not planned: a profile drawn on a face as a dedicated machining step (dropped on 2026-11-13);
a geometric constraint solver (inferences and typed values play that part, SketchUp's way).

**Primitive**: `Primitive::Prism { origin, profile, height }` — `profile`, the outer loop's
corners in the XY plane through `origin` (relative to it, at least three, a simple outline,
either way round: the kernel turns it), extruded along +Z by `height`. Its `at_origin` puts
the part's origin at the lowest corner of the profile's box (arcs included), as a box's.
Built by SolidCore's `prism::extrude::extrude_region` (a `Region2` of `Seg2::Line`s and
`Seg2::Arc`s on the frame, swept from 0 to `height`). Holes later. The tool refuses a profile
that crosses itself before the kernel does.

**Arcs** (step 2, decided on 2026-11-13): each corner is `{ p, bulge }`, as DXF's LWPOLYLINE
(OCL's DXF export): `bulge` = tan(sweep / 4) curves the side from that corner to the next one,
0 straight, positive turning counter-clockwise. The ends are shared, so the outline cannot
open. `kernel::profile` works on these outlines: area, clean-up, simplicity (lines and arcs
crossing), the box, the offset (an arc offset is the concentric arc, as SolidCraft's; an arc
the offset turns over or shrinks to nothing is taken out as a line would be), the fillets,
and points along it for the previews.

**Tool** « Dessiner un prisme » (`tool.prism`, P, ribbon group Create after the cylinder; renamed from « Polygone » on 2026-11-13: what it makes is a prism, the polygon only its outline). Its
states are OCL's: the first point, the points, the height.

- **First point**: the box's first step (§6, §6 ter: the plane of the face aimed at, an edge,
  the ground; the arrows lock the plane, ↓ the face's; the copy modifier held takes a face's
  centroid, Shift tapped then its whole outer loop as the profile, and the height follows; the other modifier measures from a corner). The plane
  locked then stays for the whole profile (OCL's `@locked_normal`), and the arrow locks are
  dropped once the point is placed: from there they lock an axis.
- **Points**: each click adds a corner; the cursor's point is inferred with the lines through
  the last corner (red, green, blue, parallels), the arrows lock the axis through it (→ red,
  ← green, ↑ blue, ↓ frees it; a beep — a word in the status bar — when the axis leaves a
  plane already set), Shift held keeps the line inferred. Each corner placed drops the locks.
  The plane, as OCL: with one corner, the box's second-corner rules (an axis plane through
  the first corner the point lies in, an edge or a guide and the first corner, a face the
  first corner lies on, else the upright plane through both); with two, the plane through
  them and the cursor's point; from three on it is set, and the cursor's point is brought
  onto it. A plane locked at the first point overrides all of this. From three corners, the
  cursor within 10 px of a placed corner snaps to it (on the locked axis, its foot there);
  from two, off any feature, it snaps to the foot of a placed corner on the line from the last
  one, drawn dotted magenta from that corner (OCL's alignment).
- **Closing**: a click on a placed corner (three at least), a double-click (the corner added,
  then closed), or Enter with the measurements box empty (a deviation from OCL). The profile
  then goes to the height.
- **Reversed** (OCL's `measure_reversed`, saved toggle drawn with OCL's icon, the copy modifier tapped): new corners
  go before the first one instead of after the last, so the outline can grow at either end;
  the lines, the locks and the typed lengths then start from the first corner.
- **Guides**: Shift tapped adds a guide from the last corner (the first, reversed) towards the
  cursor: a real guide of the context (§6 quinquies, its own undo step), where OCL draws a
  construction line it erases when leaving the step.
- **Esc** takes away the last corner placed (in the order placed, OCL's stack); with none
  left, back to the first point. From the height, back to the points, all kept.
- **Typed**: a length along the cursor's direction from the last corner (first, reversed), or
  a point (`[x;y;z]`, `<dx;dy;dz>`), brought onto the plane once it is set.
- **Height**: the box's (§6): Shift held or a double-click take the last height, the copy
  modifier held runs it to the first face along the normal from the profile's middle, tapped
  it centres it (« Hauteur centrée », shared with Box and Cylinder).
- **Made**: in the frame of the profile's plane through the first corner (the frame of
  §6's shapes, its `u` horizontal, or red on a horizontal plane): built as is on an axis
  plane, else in that frame and placed. In an open part, a machining (§2.9).
- **Offset** (decided on 2026-11-13): the « Décalage » option of Box and Cylinder (§6 bis,
  saved for the tool, `3x` at any step) moves the outline made, never the height; the outline
  drawn is dashed then, the one made solid, from three corners (the cursor's included), and
  the lengths written and typed stay the drawn ones'. Computed as SolidCraft's sketch offset
  (`crates/sketch/src/offset.rs`), in `kernel::profile::offset`: each side moved along its
  normal, the corners sharp where neighbouring sides meet — exact, no polygon library, ready
  for arcs (an arc offset is a concentric arc). As Clipper's miter offset (OCL's), a side the
  offset turns over is taken out and its neighbours meet instead (a trapezoid can become a
  triangle). Refused, said in the status bar, the height step kept: nothing left (inward past
  the middle), or an outline that would cross itself (an outward offset closing a notch, an
  inward one pinching a neck: one outline cannot be two pieces).
- **Sides** (step 2): the choice « Tracé » in the bar — Ligne, Arc tangent, Arc 2 points —
  turned by Tab, not saved (each outline starts with lines, as SolidCraft's chains); the
  cursor carries a word for an arc mode (the inference's tip). SolidCraft's gesture
  (`ui-egui/src/sketch_line_arc.rs`) switches Ligne ↔ Arc tangent: leave the last corner
  (20 px), come back onto it (8 px) and rest there 0.35 s (drifting 2 px at most); then it
  must be left again before switching back. The side closing the outline follows the mode.
  - *Arc tangent*: from the last corner (the first, reversed), tangent to the side ending
    there, to the cursor's point; with no side yet, a line. Its end straight behind is
    refused. Typed: the radius, the cursor keeping the side the arc turns to.
  - *Arc 2 points* (SketchUp's): a click sets the end, a second one the bulge (the arc
    through the cursor's point); the bulge snaps to the half circle within 10 px. Typed: the
    end as a line's, then the bulge's height (sagitta). Esc drops the arc under way.
- **Fillet** (step 2): the « Congé » length option (saved for the tool, `5r` at any step, as
  the offset's `5x`) rounds every corner between two straight sides at creation, after the
  offset — the radius is the made part's. A corner whose sides are too short for it (the
  fillets at both ends of a side counted) stays sharp. Previewed with the offset's outline.
  Recorded as radii on the sharp corners since 2026-11-14 (« The shape kept while editing »).
- **Whole face** (step 2): Shift with the copy modifier held takes a face's outer outline with
  its arcs too.

**Profile in the part** (step 3, decided on 2026-11-13):

- **The recipe keeps it**: `Part::profile: Option<BlankProfile { profile, z0, height }>` — the
  outline in the part's XY plane, extruded from `z0` by `height` along its Z; the blank solid is
  built from it (`Primitive::Prism`) and stays a cache of it, as the shape is of the machinings
  (§2.4). Box, Cylinder and Prism all record theirs (a rectangle; a disc as two half circles;
  the outline made, offset included, fillets as radii). Imports and booleans make free parts
  (none).
- **Stretching keeps it when it can** (§6 sexies, in the part's axes): along Z, `z0` and
  `height` follow their sections; along any axis in its plane, each corner moves with its
  section, an arc whole in one section rigidly; an arc across a cutter (it would not stay an
  arc), or an axis neither in its plane nor along Z, drops it — the part becomes free. (The
  kernel refuses these stretches anyway, an edge across a cutter having to run along the axis:
  a safeguard.) Open (2026-11-14): with fillets as radii, a stretch across a fillet keeps the
  profile — the blank could then be built again from it rather than stretched by the kernel.
- **The machinings stay where they are** in the part's axes, and so does the origin when the
  outline grows or shrinks (anchoring them on the blank, §2.10, is for later).
- **`Operation::SetProfile { target, profile, z0, height, scope }`**: the blank built again
  from the outline, then the shape from the blank (§2.4); the scope of the open part (§2.9),
  "only this one" making it a separate part. Refused for an outline that is not simple or no
  height.
- **Editing** (`ToolKind::Profile`, « Profil »): a button beside « Usinages » in the option
  bar's Options while a part with a profile is open, on while the tool is the active one (any
  other tool turns it off). The outline is drawn on the bottom cap, where the profile lies,
  whatever the view (on the cap facing the eye until 2026-11-15), with grips: its corners
  (squares; round when free, 2026-11-15), its arcs' middles (diamonds), its straight sides'
  middles (bars along the side; round when free, to insert a corner) — and the cap's middle (the thickness)
  until 2026-11-14, when the Stretch tool on the body took it over (§2.11). A
  grip is taken by a click and dropped by another (SketchUp's Move; a drag works too), Esc
  dropping the gesture; each gesture is one operation (one undo step):
  - a corner follows the cursor, inferred with the lines through where it was and brought onto
    the cap's plane; its sides follow, an arc keeping its bulge. Typed: a distance along the
    move, or a point;
  - a side's middle inserts a corner there, which then moves as a corner;
  - an arc's middle sets its bulge (the arc through the cursor), snapping to the half circle
    and to straight (the chord) within 10 px. Typed: its height (sagitta);
  - Delete over a corner takes it away (the outline must stay simple, at least three corners
    or an arc).
  An outline that would cross itself is refused, said in the status bar. The corners' and the
  sides' grips were then free moves; they keep the outline's shape since 2026-11-14 (below).

**The shape kept while editing** (decided on 2026-11-14, branch `feature/profile`): a box's
profile edited corner by corner stopped being a rectangle, a cylinder's arc grip turned its
disc into an oval, a fillet next to a moved corner lost its tangency. Three ways were weighed:
editing by sides with no stored constraint (SpaceClaim's, Shapr3D's direct editing), the
designer's intent kept in the recipe (fillets as radii), a parametric sketch (stored
constraints, driving dimensions, a solver). The first two now; the sketch later, if needed —
the candidate solver is ezpz (KittyCAD, Rust, MIT; SolveSpace's libslvs is GPLv3, out). The
format is not shaped for it in advance (v1 until the first release).

- **Fillets are radii**: `Corner { p, bulge, fillet }` — `fillet`, a radius on a sharp corner
  between two straight sides (0: sharp); the outline stores the sharp corner, the arc is
  computed wherever the outline is used (the blank built, its box, the display, the exports),
  so it follows every edit and stays tangent. The Prism's « Congé » option records radii
  instead of arcs. An arc tangent to two straight sides that are not parallel, however it was
  drawn (an arc tangent closed tangent, a face taken whole), is a fillet: recognised when the
  profile is recorded, it becomes its radius on the sharp corner. A radius that no longer fits
  (the sides too short, the fillets at both ends counted) leaves its corner sharp, as at
  creation, and comes back when there is room again.
- **Sides keep their direction**: the grip of a corner moves the two sides meeting there
  parallel to themselves, through the cursor's point, and the neighbouring corners slide along
  the sides beyond (a rectangle stays a rectangle, a trapezoid keeps its angles; 2 degrees of
  freedom: inferred on the cap's plane, typed as a distance along the move or a point). When
  that would move every corner (a triangle: a mere scaling), the corner moves alone. The
  grip in a straight side's middle moves that side parallel to itself, its neighbours
  extended (1 degree: the cursor's point taken along the side's normal, typed as a distance).
  A neighbour that cannot meet the moved side any more refuses the move.
- **Free, the copy modifier tapped** (as Move's copy: before or during the gesture, the cursor
  carries the word « Libre », dropped once done; also the « Libre » toggle in the option bar):
  a corner then moves alone as before; a side's middle inserts a corner there, which moves
  alone. Built by `kernel::profile::{shift_sides, corner_moved, side_moved}`.
- **Tangencies kept by rules** (read on the outline as it was when the grip was taken, no
  solver; free moves too; `kernel::profile::rearced`): a fillet (above); a
  half circle tangent to two parallel sides (a rounded end, a slot's end) stays tangent, its
  radius half the gap (its still end slides along its side to face the moved one); a whole
  disc (a cylinder's) has one grip, its radius (typed too; `profile::circle`); an arc tangent to
  a straight neighbour keeps that tangency: its ends go where their sides take them, and the one
  arc leaving that side along it to its other end is taken (tangent at both ends to sides that
  are not parallel, the start's tangency wins, unless only its end moved). Chains of arcs
  tangent to each other (an S) may lose it: that is the sketch's job.
- **An arc tangent to nothing keeps its sagitta** (the rise over its chord, the dimension a
  bent rail is given): its ends follow their sides, its height stays.
- **Fillet grip**: hovering a sharp corner between two straight sides shows a small grip on
  its bisector, 18 px inside; dragging it rounds the corner, the arc through the cursor's
  point on the bisector (typed: the radius). On a fillet, the grip sits at the arc's middle,
  always shown; brought back onto the corner (10 px) it makes it sharp again, as Delete over
  it does. Dragged, the radius stops where a side runs out (its other end's fillet kept);
  typed too big, it is refused, said (`profile::{fillet_middle, fillet_through,
  fillet_max}`).

## 6 decies. Copy, cut and paste

Decided on 2026-11-14 (questions / answers). Built in steps: 1. the model; 2. commands,
clipboard and Paste in place; 3. Paste at the cursor — all done (2026-11-14), machinings and
guides of an open part included.

- **What is copied: whatever can be selected.** Out of an open part: the context's
  occurrences (parts and assemblies, their materials, their components' guides) and the
  context's guides. In an open part: its machinings and its guides. **Cut** = copy, then the
  existing erase (one undo step for the erase).
- **The fragment is a `Document`**, as `Operation::Insert`'s (§8 ter): the closure of the
  components the copied occurrences use (shared by `Arc`: copying costs nearly nothing,
  whatever the model's size) and the materials they are painted with. Its root holds the
  copied occurrences with their **world** transformation (the source context's world
  transformation applied), its guides the copied guides, in world axes too. Machinings copied
  from an open part go into a fragment of their own kind: the tools and their placements **in
  the part's axes**, and the part's guides in those axes.
- **Clipboard: in the app, plus a token.** The fragment stays in memory (`Arc`), one
  clipboard for the whole app: copying in one tab and pasting in another is the "from one
  document to another" case (§6 octies). The system clipboard receives only a small **token**
  (a text naming this clipboard and its serial number): if the user copies something else
  anywhere, the clipboard no longer matches the token and ⌘V does nothing. Not
  carried from one instance of the app to another, nor between browser tabs. Since the
  fragment is a document, carrying its serialised text could come later.
- **Pasting in the same document gives the same parts**: new occurrences of the same parts
  and assemblies ("Leg ×5"), as a copy made by Move (§2.5). The source document is known by
  its `DocId`; a component of the fragment that is no longer in the document (erased, purged
  after a cut) is grafted from the fragment, as one from another document. **In another
  document**, everything is grafted (`Document::graft`: fresh ids, names made unique,
  identical materials reused). Unlike `Insert`, several top occurrences stay loose (not
  gathered in an assembly).
- **Where it lands.** **Paste** (⌘V) hangs the pasted content on the cursor, with the Move
  tool's inferences and typed distance; a click places it, Esc gives it up (the history keeps one step, or
  none). It is held by **the min corner of its bounding box**, in the context's
  axes (low, left, front), so it lands on the ground or on a face as in SketchUp. **Paste in
  place** (⇧⌘V) puts it where it was in world space, brought into the active context. The
  pasted content is selected; one undo step.
- **Machinings paste only into an open part** (of any part, of any document), in its axes:
  "in place" keeps their placement in the part's axes (the holes of one part carried onto its
  twin), at the cursor as the rest. Occurrences do not paste into an open part, nor
  machinings out of one: Paste is disabled. Guides follow the content they were copied with.
- **Commands**: `edit.cut` (⌘X), `edit.copy` (⌘C), `edit.paste` (⌘V), `edit.paste_in_place`
  (⇧⌘V), in the Edit group (palette, shortcuts, control channel); enabled when the selection
  can be copied (`Editor::can_copy`: something selected, nothing computing, the tool at its
  first step), or when the clipboard fits the target (`Editor::can_paste`).
- **Keys** (step 2, `apps/solidsketch/src/clipboard.rs`, `app.rs`): ⌘X, ⌘C and ⌘V reach the
  app as egui's `Event::Cut` / `Event::Copy` / `Event::Paste(text)` — egui-winit sends no key
  for them; the web sends the key *and* the clipboard event, so the keys are ignored — and
  are fixed whatever the shortcuts, done only when enabled, silently otherwise. A text field
  being edited keeps them (egui wants the keyboard). `Event::Paste` pastes only when its text
  is the token; from the menu or the palette, the app's clipboard is trusted (the system
  clipboard is not read). The token is `solidsketch-clipboard:<run nonce>:<copy count>`.
  eframe's `clipboard` feature is on (arboard on the desktop): before, the desktop used only
  egui's in-process clipboard, text fields included.
- **The app's clipboard** (`Clipboard`): the `Clip` (`Clip::Entities(Arc<Document>)` or
  `Clip::Machinings(Arc<MachiningFragment>)`, `Editor::copy` / `cut`), the `DocId` and the
  history revision it was copied at (before the erase, for a cut), and its token.
  `Editor::paste_in_place(clip, reuse)` pastes it; `reuse` is decided by the app (below).
- **Model** (step 1, `crates/model/src/clipboard.rs`): `Document::entities_fragment(instances,
  ids)` and `Document::machinings_fragment(target, ids)` (a `MachiningFragment`: machinings
  and guide shapes) build the fragments; the fragment keeps the source's ids. Two operations,
  one undo step each, Paste in place being an identity or the inverse of the target
  context's world transformation:
  - `Operation::Paste { context, fragment, transform, reuse }`: no kernel. `Document::graft`
    takes a `reuse` flag: with it, the components and materials whose ids are still in the
    document are used as they are, the others grafted with fresh ids (which ids are kept is
    decided before any is drawn: a fresh id may be one of the fragment's). Refused: a cycle
    (an assembly pasted into itself), a transformation not rigid, an empty fragment
    (`Refusal::NothingPasted`);
  - `Operation::PasteMachinings { target, fragment, transform, scope }`: the copies cut from
    the part as it is (one recipe task), the guides added to the part.
- **Same document or not**: `reuse` is true only when the copy's document is the target tab
  and the revision copied is still in its history (`History::holds`: undo stack, current
  state or redo stack): ids only grow from that state, so
  they still name the same things. Undone past the copy and changed since, an id may name
  another component: then everything is grafted.
- **Paste at the cursor** (step 3, `Editor::paste`): the paste is done **in place** first
  (`After::Carry`), then carried by the **Move tool** in a mode of its own
  (`MoveTool::carrying`): already moving, from the low corner of the content's box in the axes
  (the box from the `Clip`'s hull: the corners of its solids' boxes, taken at the copy, where
  they are meshed, and a point of each guide; in world axes for occurrences, the part's for
  machinings), with Move's inferences, locks and typed distance or point; no copy toggle. The
  click **replaces** the paste in place by the same paste moved (`Command::Replace`,
  `Moved::paste`: the transformation composed with the motion in the frame): one step in the
  history. Esc, Undo, Redo or another tool give it up: the paste in place, still the last
  step, is undone (`Editor::settle_carry`). Put down or given up, the tool before comes back.
  So what is carried is in the document meanwhile (drawn by Move's ghost, out of the
  inferences); a paste of machinings computes the part twice (in place, then moved).

## 6 undecies. Turned bodies (revolution)

Discussed on 2026-11-15 (Q&A), branch `feature/profile-edit`. A turned leg, a dowel, a knob, a
ring: a body made by turning a half profile a whole turn about an axis. SolidCore turns a
planar profile of lines and arcs (`solidcore_ops::sweep::revolve`); the kernel already reads
surfaces of revolution (`Surface::PlacedRevolution`, STEP import; non-rigid for the stretch).

- **Code** (step 1, 2026-11-15): `Primitive::Revolution { origin, profile }` built by
  `Solid::new_revolution` (SolidCore's `sweep::revolve`, arcs either way, a point a rounding
  off the axis put on it), `profile::is_turnable`; `BodyProfile { profile, sweep }` with
  `Sweep::Extrusion { z0, height }` (saved as before) or `Sweep::Revolution { revolution:
  Turn::Whole }` (saved `"revolution": "whole"`); `BodyProfile::stretched` follows the
  heights along Z and drops the profile across the axis.
  Step 2: the tool is `ToolKind::Turn` (`tool.turn`, L, icon `turn`), a separate tool for the
  user but the Prism's code in a turning mode (`PrismTool::turning`: the axis's two points
  before the outline, the plane locked through the axis and the profile's first point, turned at once when
  closed — no height —, the axis drawn as an endless guide line in its axis colour (its two
  points marked), the outline's mirror across the axis dashed while drawing, closed as the
  outline is (its side to the cursor and back to the first corner); before the first point,
  its radius from the axis dashed with its length, and the circle it turns along as a faint
  guide (preview guides not opaque go with the faint lines, which blend); options
  fillet, trace, reversed; no offset).
  Step 3: the Profile tool frames a turned body's XZ plane (`edit_frame`; first turned to face
  the eye, dropped the same day at the user's request), an extrusion's bottom cap; a
  corner on the axis slides along it, a side on it has no grip unless free, a move across the
  axis stops on it (bisection along the move), an arc bulging across is refused at the drop.
  `View::bases` holds the half profile in the body's XZ plane, `View::base_axes` its axis
  (dashed, 10 % past the profile at each end).
  Step 4: `stretch_bodies` rebuilds a turned body from its stretched half profile (along its
  axis) and refuses it otherwise (`Refusal::TurnedAcross`: across the axis, or across an arc);
  the Stretch tool locks for good (`Target::fixed`, a padlock that does not unlock, said in the
  status bar) the axes a turned body spans whole across its axis — any cutter would cut it —,
  without saving that lock in the part's cutters; a turned dowel inside a longer board leaves
  the board's axes free, a cutter through the dowel then refused by the model.
- **The profile** (decided): a body's profile is either the extrusion it is today (outline
  in the body's XY plane, `z0`, `height`) or a **revolution**: a half profile in the body's
  XZ plane — its points `(x, z)` with `x`, the radius, never negative —, turned a **whole
  turn about the body's Z**. The axis is the body's, so its placement (§2.11) says where the
  axis is; no axis of its own stored, no partial angle for now (a quarter round can come
  later as an angle). Same corners as an extrusion's (`{ p, bulge, fillet }`): arcs and
  fillets alike. A profile that does not touch the axis gives a ring or a bowl.
- **Created by a tool of its own** (decided), « Tourner » (`tool.turn`, ribbon group Create
  after the prism): first the **axis**, two points (inferences, as a line's; only its origin
  is marked and no length is shown — 2026-11-15, user: only its way matters); then the **half
  profile**, drawn point by point as the Prism's outline (its arcs, fillets, typed lengths
  and inferences — the axis among them, as a guide line, its origin as an endpoint, through
  `Tool::infer_line`, 2026-11-15, user), on one side of the axis, in the plane through the axis and the profile's
  first point (2026-11-15, user: the plane is decided by that click — a point inferred on a
  face or an edge; over nothing, taken on the horizontal plane through the axis's start —
  2026-11-15, user —, that plane seen edge on or a point on the axis giving the plane through
  the axis facing the eye),
  locked for the rest of the outline. It is **closed by hand**, back on its first point (decided: no automatic closing
  along the axis), the side along the axis drawn as any other. A profile crossing the axis
  or itself is refused. The body's Z is the axis (from the first point to the second); as a
  prism drawn on a slanted face takes the face's frame, the part made takes the body's axes
  (its Z along the axis, its origin on the axis at the profile's lowest height,
  `Primitive::at_origin`); in an open part it is a body of it (§2.11, positive or negative as
  the drawing tools'), placed in the part's axes.
- **Edited by the Profile tool** (decided): the half profile drawn in the body's XZ plane
  (2026-11-15, user: where it lies, not turned to face the eye — and an extrusion's outline
  likewise on its bottom cap, whatever the view), with the same grips (squares, bars, free, arcs, fillets),
  plus two rules: a corner is stopped on the axis, never beyond it, and a side lying on the
  axis stays on it. The bold outline of an open part's bodies (their bottom cap for an
  extrusion) is the half profile for a turned body, its axis dashed.
- **Stretch** (decided): along the axis, the body is rebuilt from its profile, the cutters
  moving the half profile's heights (exact). **Across the axis it is refused**: the body's X
  and Y locked (padlocks), the diameter changed by the Profile tool only.
- **Cut list** (decided): nothing particular — the box of the final B-rep in the part's axes,
  as any part (a Ø 40 × 700 leg reads 40 × 40 × 700). A « turned » mark may come with the cut
  list itself.

## 6 duodecies. Copy along a line, copy along a grid, distribute

Decided on 2026-11-15 (questions / answers), after OpenCutList's Smart Handle tool
(`tool/smart_handle_tool.rb`: `SmartHandleActionHandler`, `SmartHandleCopyLineActionHandler`,
`SmartHandleCopyGridActionHandler`, `SmartHandleDistributeActionHandler`). Steps 1 (the model),
2 (the common module, `tools/handle.rs`, and Copy along a line, `tools/copy_line.rs`), 3
(Distribute, `tools/distribute.rs`) and 4 (Copy along a grid, `tools/copy_grid.rs`) built on
2026-11-15.

- **Three tools**, three commands — `tool.copy_line` (« Copier en ligne »), `tool.copy_grid`
  (« Copier en grille ») and `tool.distribute` (« Répartir ») — sharing one module (picking,
  box and its centre, axes option, snapping on the nearest axis, arrow locks, counts typed,
  measure, preview). OCL has them as actions of one tool; here a command is a tool. In the
  ribbon's **Modify** group of Build and Assemble, after Rotate; **no default shortcut**.
- **Targets**: the active context's occurrences (parts and assemblies, with the context's
  guides selected), and in an open part its bodies — as Move and Rotate copy today.
- **Operations**: a list of placements, one per copy, computed by the tool — the model only
  puts them down — and a move of the sources. `Operation::Copies { context, entities, moved:
  DAffine3, placements: Vec<DAffine3> }`: the sources moved to `moved · T` (the identity for
  the line and the grid; Distribute moves them to the first slot, as OCL sets the source
  instance's transformation), each copy of an occurrence placed at `placement · T` (rigid,
  mirror allowed, §2.2; at most `MAX_ARRAY` placements; `T` the source's before the move); and
  its twin for bodies, `Operation::CopyBodies { target, bodies, moved, placements, scope }`
  (copies sharing their body definitions, §2.11). One operation, one undo step. `Array`,
  `ArrayBodies` and Move / Rotate are left as they are: `Array` (one step applied k times) can
  express neither a grid (two steps), OCL's mirrored copies, nor a moved source.
- **Interaction of the line and the grid, as OCL's** (one step: no first click):
  - **picking** as the Stretch (§6 sexies): the selection when the tool starts, else a click,
    a rectangle or Shift clicks; once there is a target, the handle starts at **the centre of
    its box** in the chosen axes;
  - **options**: *measure* — edge to edge (outside), centre to centre, between the parts
    (inside); *axes* — active, context, entity (saved, as the Stretch's); *mirror* — toggled
    by a quick tap of Option (Alt elsewhere), for a single copy only (OCL's warning
    `copies_disable_mirror` otherwise). Option does not toggle copying as in Move: these tools
    always copy;
  - **line**: the cursor projected on the axis nearest on screen, or — on a face — the axis of
    the box face crossed towards the cursor (`_snap_handle`); → ← ↑ lock X, Y, Z;
  - **grid**: the cursor projected on the plane normal to Z, or to the axis locked by the
    arrows; its axes from the active X projected on that plane (`_get_handle_axes`);
  - **minimum distance** by measure (`_get_move_def`'s `dlmin` / `dmin`): the cursor stops at
    it, a smaller typed value is refused (`lt_min_distance`);
  - **typing**: a distance (grid: `dx; dy`), a count `x5`, `*5` or `/5` (grid: `x3; /4`, an
    empty slot = 1 copy); Shift with `+` / `−` changes the count; default `/1`;
  - **after the click**, typing a distance or a count again redoes the copies
    (`Command::Replace`: one undo step), as OCL's `STATE_HANDLE_COPIES`;
  - **mirror** (one copy): line — the copy mirrored across the plane through its centre
    normal to the move (OCL's scaling by −1 then half turn about the move: that reflection);
    grid — the corner copy turned half a turn about the normal, the two side copies mirrored
    as on a line (OCL's transforms).
- **Distribute, as OCL's** (two steps):
  - **picking** as above; then a **first click** sets the start point, a **second** the end
    point (e.g. the inner faces of two uprights); Esc at the second goes back to the first.
    Both snap as the line's (nearest axis on screen or the box face crossed; → ← ↑ lock X,
    Y, Z), the measure line held on the line through the box's centre;
  - **the sources move**: `number` pieces in all, the source taking the first slot, its copies
    the next ones, spread evenly between the two points; while the tool runs, the sources are
    hidden and drawn by the preview (OCL `_hide_instances`);
  - **measure**: centre to centre or between the parts (no edge to edge); the mirror option
    does not apply;
  - **typing**: a count `x5` (5 pieces in all) or `/5` (5 intervals: 4 pieces, `/1` refused);
    Shift with `+` / `−` changes it; a distance sets the end point along the line; a **list
    of spacings** `30; 50` (two items at least) fixes the first gaps, empty slots first
    (`;; 30`) fix the last ones, the rest spread (`_get_move_def`'s start / end spacings);
    after the click, typing a count or spacings again redoes it (`Command::Replace`);
  - **Option held**: a ray from the start point along the line; the end point snaps to the
    first thing it hits (`DisplayScene::pick` with any ray), drawn as an arrow (OCL's
    raytest), so as to spread up to the next part without aiming at it;
  - **double-click** repeats the last distribute (count, spacings, both points) on the new
    target.
- **Preview** (deviation from OCL, which draws the copies' edges in black): the copies as
  Move's ghosts while copying, the boxes dotted (the source's in black, the copies' in the
  axis colour), the measure line or rectangle dashed over everything (bold while an axis is locked), in the axis colour, with its VCB
  bubbles (one per side for the grid), and OCL's mirror badge (`PointShape::Mirror`, 40 px past
  the mirrored copy: in the axis colour for the line, black for the grid). The copies' ghosts are the
  selection's marks drawn at each copy (`Preview::copies`), so what is copied is the selection
  (a click selects it, as Move); once the copies are made the selection is let go (OCL's
  restart).
- **Built so far without** (2026-11-15, steps 2–4): Shift with `+` / `−` (the measurements box
  takes those keys while typing), and OCL's options shared between the
  copy tools (each keeps its own; Distribute's measure defaults to « between the parts », as
  OCL's preset).
- **Distribute, as built** (deviations): the start point is the point inferred (what is
  distributed left out, as OCL hides it), else on the ground; on the geometry the end runs
  along the axis the way from the start mostly goes; what is distributed shows as a ghost in
  place before the end point, then at its slot (`Preview::ghost`, the copies at the others);
  changing the axes takes the points again (OCL keeps the start); the copy modifier's ray
  follows the next cursor move.
- **The grid, as built**: its boxes are drawn at its four corners only (OCL's), its sides'
  lengths along the rectangle's sides from its low corner, each in the colour of the axis it
  runs along.
- **Not now**: OCL's **twins** (picking the other occurrences of the part, same parent, to
  replace them — re-spacing an existing row, which Distribute will soon call for); to discuss
  again after use. OCL's Move along line is not part of this work.
- **Steps**: 1. the model (`Copies`, `CopyBodies`); 2. the common module and Copy along a line;
  3. Distribute; 4. Copy along a grid.

## 6 terdecies. Quick tools after a creation

Decided on 2026-11-15 (questions / answers), after OpenCutList's floating tools of the Smart
Draw tool (`tool/smart_draw_tool.rb`: `_append_floating_tools_at`, `_remove_floating_tools`,
the Smart Handle tool's `callback_action_handler` and `startup_selection`, the typed text
handed back in `smart_tool.rb`'s `onToolUserText`). Built on 2026-11-15, in four steps (below).

- **What**: once a creation tool (Box, Cylinder, Prism, Turn) has put down its part, or its
  body in an open part, a small floating card of buttons appears in the view, each starting a
  tool on what was just made.
- **Where**: a row of buttons centred on the centre of the new part's box (or body's), projected
  on screen, following the view as it orbits or zooms (OCL's anchor); the buttons drawn as
  the ribbon's (our icons, the name in the tip), on a translucent card.
- **When it goes**: at the first click (or typed value) of the next shape, on Command (Alt
  elsewhere, OCL's), Esc, a change of tool or of active context, an undo, and when the tool it started
  hands back.
- **The commands**: a fixed list of tools that make sense on a part just made — Copy along a
  line, Copy along a grid, Distribute, Move, Rotate, Stretch, Paint. Pinned by default: the
  three copy tools (OCL's, less Move along line and Paneling, which SolidSketch has not).
- **Choosing them, as the ribbon's pins** (§6 septies): a last ▾ button opens the list of
  the commands, each row with a pin; right click on a button: Unpin. Order is the list's.
  Preference `quick_tools`: the pinned ids, absent = the defaults (unknown ids ignored). No
  pin at all: only the ▾ button.
- **A click starts the tool on the new part**: the selection becomes what was made, the tool
  starts with it as its target (picking skipped, as with a selection when it starts).
- **Return, as OCL's**: the editor keeps the creation tool as a **return slot** (not a stack:
  one level). The tool started from the card hands back to the creation tool as soon as its
  action is done (the copies' click, Distribute's second click), or on Esc without doing
  anything. Back in the creation tool, the tool that handed back is kept **aside**: what is
  typed before the first click of the next shape goes to it, so `x5` or a distance redoes
  the copies (`Command::Replace`, one undo step), as OCL's draw tool hands its text to
  `previous_action_handler`. The card is not shown again after such a return; given up (Esc,
  nothing done), the card and the selection are back as they were (asked on 2026-11-15, after
  use: OCL drops its card on any return).
- **The return, as built** (`editor/quick.rs`): `Editor::start_quick_tool(kind)` remembers
  the active tool (`quick_return`); the tool started hands back **after the first input whose
  commands apply an operation** (`Command::Apply`, but a `SetBehavior` — the stretch's cutters
  kept —, or `Command::Replace`): no tool tells when it is done, the editor sees it act (copy
  tools at their click or typed distance, Move and Rotate at their last click, Stretch at its last
  click or drag's end, Paint at its first click). Esc that leaves it at its first step (or at it already)
  hands back without anything aside, the card it was started from offered again and the
  selection as it was (`quick::Return`). The tool aside takes what is typed while the creation tool
  is at its first step (before its own options' `<length>x`), and the measurements box shows its
  value; it is let go when the creation tool gets busy (the next shape's first click), on Esc,
  or when a tool is chosen. Choosing a tool drops the return slot too.
- **The card, as built** (`editor/quick.rs`): an `AddPrimitive` or `AddBody` applied by a tool
  ends with `After::Card` (nothing selected, as before), which offers the card on what it made
  — the occurrences created, or in an open part the bodies, unless its machinings are hidden
  (they could not be selected). Its anchor is the centre of their box in the context's axes
  (`handle::occurrences_bounds` / `bodies_bounds`, the copy tools' box), in world;
  `Editor::quick_card()` projects it each frame. The card belongs to the history's revision and
  the context it was made at (any step, undo, redo, entering or leaving, and it is gone); it is
  hidden while the kernel works or the tool is busy, and let go when the tool gets busy, on
  Alt / Esc, or a tool chosen. `start_quick_tool` takes it: what it was offered on becomes the
  selection.
- **The card in the app, as built** (`apps/solidsketch/src/quick.rs`): an `egui::Area` centred on
  `quick_card()`, kept inside the view, drawn after the docked cards; a window frame at 85 %
  opacity, icon buttons 34 pt (icon 22, the name in the tip), then a narrow ▾ (the ribbon's
  chevron) opening the list `quick::COMMANDS` with pins (as a ribbon group's menu, a click on a
  row starts it too). `Action::QuickTool(id)` → `Editor::start_quick_tool`, `Action::QuickPin(id)`
  → `quick::toggle` on the preference `quick_tools: Option<Vec<String>>` (`None` = `DEFAULT`; back
  to the defaults, `None` again). The view under the card gets no hover (another layer): the
  creation tool hides what follows the cursor while it is over the card. Texts `quick_tools.*`.
- **Started otherwise** (ribbon, shortcut), the tools behave as today: no return slot.
- **Not now**: showing the card after a paste; OCL's Move along line and Paneling.
- **Steps**: 1. the return slot and the tool aside; 2. the card offered after a creation, its
  anchor; 3. the card in the app, its pins, its preference; 4. the texts (`quick_tools.*`) and
  the editor's scenarios (copy line, Move, Distribute, Paint started from the card; the card's
  anchor, its going, a body in an open part).

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
| `crates/control` — `solidsketch-control` | The control protocol (§7 septies): methods and their parameters (with their JSON Schemas), the handlers that need only an `Editor` (`model.*`, `tool.*`), the loopback HTTP server with its per-start token and discovery files (desktop). No UI toolkit. | `model`, `tools`, `kernel`, `serde_json`, `schemars` |
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
  (`bvh.rs`: Morton order of the centres, one scale on the three axes, cut where the highest
  differing bit of the codes turns on, down to 4 items per leaf, built in O(n log n) by one
  integer sort; the leaves keep their items' boxes and hand out only those that pass the test;
  when only boxes changed — occurrences moved, meshes come in — it is refit instead, and built
  again once its nodes' areas grow half again what they were at build),
  and the same entries sorted by occurrence path (an occurrence's parts, at any depth,
  form a run). Picking walks the BVH nearest box first and skips boxes beyond the best hit
  (plus its tie tolerance); inferences take the meshes near the ray (`inferable_where`), the
  rectangle selection those whose box seen meets the rectangle (`parts_where`; a window takes an
  entity only when every part under it, counted by `parts_under`, lies inside); lookups by path
  (`inferable_at`, `part_at`, `inferable_of`) search the sorted run. Results keep the scene's
  order (ties, candidates), as the brute force gave them (test
  `the_scene_index_picks_as_a_scan_does`, `the_rectangle_takes_what_a_scan_takes`,
  `the_meshes_near_the_ray_are_those_a_scan_finds`). The stretch's preview compares the
  document it sketches with the current one by `Document::placed_changes`, which skips what
  both share; it counts placements over the distinct assemblies (`count_placed`), not per
  path; the stretch tool keeps its target while the scene's generation, the occurrences and
  the axes stay. The maps and sets keyed by ids or addresses on these paths (an assembly's
  index, the selection, the scene's) hash with `FastMap` / `FastSet` (model, `fast_hash.rs`):
  a multiplicative hash, several times cheaper than SipHash, whose resistance to chosen keys
  a desktop modeller does not need. An operation on a few occurrences among many finds them by
  the assembly's index (`positions_of`), and a move changes them in place, keeping it.
- **Meshes coming in** (decided on 2026-11-12, after a model of 10 000 distinct parts spent
  13 ms per frame while its meshes came): the document unchanged, a display coming from the
  kernel is taken in by `DisplayScene::refresh_meshes`, not a sync — only the definitions
  drawing that solid change (the scene keeps, by solid, the definitions using it), then the
  boxes of their placements (each definition's run of index entries), the BVH refit (rebuilt
  only when a mesh gains or loses its box) and the scene's bounds; the placements, guides and
  recipe stay (test `meshes_taken_in_leave_the_scene_a_sync_would`). `placed_guides` returns
  at once for a model without guides.
- **The selection drawn** (decided on 2026-11-12, after 40 000 occurrences selected cost
  12 ms per frame): its marks, its occurrences' boxes (in their own axes, one per component)
  and the set of its ids are worked out once (`SelectionDrawing`) and kept while the scene's
  generation, the context, the selection and the recipe shown stay; a move's ghost is the same
  moved (test `the_selection_drawn_follows_what_it_hangs_on`). `ContextIndex` searches the
  assembly's own index instead of building one.
- **Occurrences by id** (decided on 2026-11-12, after the stretch tool took 300 ms per hover
  with 10 000 occurrences selected): `Assembly::occurrences` is private; a search by id
  (`occurrence`, `position`) goes through an index (id → position) built after a few searches
  since the last change to the list (`occurrences_mut` drops it; a clone keeps it), so that a
  search per occurrence stays linear in all. Below 16 occurrences, a scan.
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
- **One service, three executions** (`kernel::KernelService`: `submit(id, job)`, `poll()`,
  and `progress()`, `cancel(id)`, §7 sexies): `Inline` (synchronous, tests), `Pool` (a
  thread per core, desktop; it was one thread, `Threaded`, until 2026-11-13), Web Worker
  (browser, in the app). Each result wakes the UI (`request_repaint`).
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
  `theme` preference (system by default, light, dark).
- **The view's background** (decided on 2026-11-13, unlike SketchUp, whose style is saved
  in the model): a token of each palette and mode (`UiTokens::view`, with `view_text` for the
  shown tab's text), not a setting of the document, and not changeable. It stays light in the
  dark mode, only dimmed: the faces, the edges, the guides, the tools' inks (`GUIDE_INK`) and
  the axes are colours drawn for a light ground; a test holds their contrast with it (the
  y axis's green and the picked orange are deep ones, the markers' outline dark in both modes:
  a bright green or orange had the dimmed view's luminance and faded into it). A truly
  dark view would first need those inks as tokens of the mode.
- **Interface tokens** (`UiTokens`, decided on 2026-11-10): panel, header, field, border,
  text, dim text, hover, accent and soft accent, corner radii, spacing.
- **Palettes** (decided on 2026-11-10): a `Palette` (user preference `palette`) gives the
  interface's colours, **each in a light and a dark variant**; the `theme` preference
  (system, light, dark) picks the variant, so the two settings are independent. Built in
  code, no theme files: *Neutral* (SolidCraft's: neutral greys, blue accent),
  *Warm* (warm greys, amber, the default), *Graphite* (cool, deeper greys, teal). A palette
  changes the interface only: the model's and the overlays' tokens stay the theme's. A test
  holds every variant to text ≥ 7:1 and dim text ≥ 3:1 on its grounds.
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
  (127.0.0.1, port: preference `remote_port`, default 7858; `SOLIDSKETCH_CONTROL_PORT` for
  one launch), **off by default**. A new port (typed, or dragged and released, in the
  preferences) restarts it only if it is on and the port changed. Switched on and off
  while the app runs — command `app.remote_control` (checked while on), the *Débogage* (debug) tab of
  the preferences — and remembered (preference `remote_control`); `SOLIDSKETCH_CONTROL=1`
  starts it for one launch without changing the preference. Stopping it frees the port (the
  listener's thread ends; requests on their way get a 503); a script may stop it with
  `POST /run app.remote_control` (its answer still comes). While on, the **title bar** shows
  its mark (left of the preferences): green, flashing at each request; in red when it
  could not start (port taken), still counted as on. Its menu: the address and requests
  served, copy a `curl` calling `app.ping` (reading the token file), stop (or try again). Its
  requests, token and methods: §7 septies (one route, `POST /call`; the first routes are
  gone). Each request is handed to the UI thread, which answers at its next frame.

### X-ray (2026-11-14)

A display mode for the session (`Editor::xray`, command `view.xray`, a toggle of the title
bar — not saved with the document): every part is drawn as a ghost (faces blended at
`Style::ghost_opacity`, no depth written, so every edge is seen), its highlights as a wash of
the mark's tint over it (`Highlight::ghost`); a section cuts it without caps. Inferences see
through everything: `DisplayScene::see_through` (already true for an open part's bodies shown
as ghosts) holds for every piece, so a point behind a face is inferred on — stacked points,
the nearest to the eye. A click picks the nearest solid along the ray, as without X-ray;
cycling through stacked ones on repeated clicks is left for later. A ghost's own opacity
over the ghosts' rides with its placement (`render::Placement::opacity`: a machining's).

## 7 quinquies. Title bar and native menus

Decided on 2026-11-10, after SolidCraft (`ui-kit/src/window_chrome.rs`, `app_bar.rs`). The
menu strip is gone: its room goes to one bar holding the document and the commands used most.

- **Title bar** (`apps/solidsketch/src/titlebar.rs`, 34 pt): the app's mark (`assets/app-icon.svg`)
  and name, the **File** menu (a text button: the registry's File commands), Undo / Redo, the
  file tabs (§6 octies), the standard views (a menu), the view's toggles
  (perspective, grid, axes, silhouettes, section, Materials panel), and at the right the
  command search (opens the palette, shows its shortcut; only its icon when the window is
  narrow) and the preferences. Every button is a registry command, its state and tip from it.
  The bar is **dark in both modes** (as SolidCraft's application bar), its text light: each
  palette's `title_bar` tokens (ground, text, dim text, separators, lit surface; a test holds
  its text to ≥ 7:1 on the ground and the lit surface), applied to the bar's panel only
  (`style::title_bar`); the menus and tips it opens keep the window's style.
  Its buttons, the command search too, are drawn as the tabs' + button: no background at rest, a veil of the text's
  colour when hovered (darker when pressed), the selection's colour when lit or when their
  menu is open (`tabs::veil`, `titlebar::backed`).
  The tool bar keeps the tools, the booleans, the assembly commands and zoom extents.
- **macOS desktop**: the bar *is* the window's title bar — the OS one is hidden and the content
  runs under it (`with_fullsize_content_view`, `titlebar_shown(false)`, `title_shown(false)`,
  `native_options` in `lib.rs`), 76 pt are left for the traffic lights (none in full screen),
  the bar's empty space moves the window (`ViewportCommand::StartDrag`) and a double-click
  zooms it. The window title is still set (Window menu, Mission Control).
- **Native menus on macOS** (`native_menu.rs`, `muda`): the app menu (About, Preferences,
  Services, Hide, Quit), the registry's **File** menu only (decided on 2026-11-13, as the title
  bar: the other commands are in the ribbon, the command search and the shortcuts, ⌘ ones
  included, through the app's keymap), the system's Window menu. The items follow the
  app every frame (enabled, checked, « Annuler Déplacer »); the bar is rebuilt when the
  language or the shortcuts change. A chosen item is its command's id, run as everywhere else.
  - Only **⌘ shortcuts** become key equivalents: AppKit takes them before the window sees the
    key, so single letters (R, M, Space…) would be lost to the VCB and the text fields — they
    stay with the app's keymap, and the menu does not show them.
  - For the same reason, a ⌘ command chosen while a shortcut is captured in the preferences
    goes back to egui as that key (`raw_input_hook`).
- **Web, Windows, Linux**: the OS frame is kept (no caption buttons of our own: resizing,
  snapping and the move loop are the OS's). No menu bar (decided on 2026-11-13, as SolidCraft's
  bar, which has only File): the other menus' commands are reached through the ribbon, the
  command search (every enabled command, the debug ones with the debug menu) and the
  shortcuts; the ☰ button that held every menu is gone.


## 7 sexies. Loading a model

Decided on 2026-11-13 (questions / answers), built the same day. A big file (`Cruise_Assembly
.step`: 583 bodies, ~20 s of meshing on one thread) is shown at once, then refined, under a
window that says how far it is and can stop it.

- **A pool of kernel threads** (`kernel::Pool`, desktop): one per core, each with SolidCore's
  stack. Two queues: an operation's job (anything but meshes) before the meshes asked in the
  background (a job of `Display` tasks only); each in the order submitted. The web keeps **one
  worker** (several would each load SolidCore's wasm).
- **Preview, then final** (`Task::Display { quality }`, `kernel::Quality`): during a load, the
  editor asks every missing solid's **preview** (meshed at a hundredth of the body,
  `solidcore_tess::preview`) before any final mesh: the pool takes them in that order. The scene
  draws a preview (`State::Preview`) until the final mesh replaces it; a preview coming after
  the final is dropped; a final mesh refused leaves the preview drawn. Outside a load (the meshes
  an operation's job brings), final meshes only.
- **Progress** (`kernel::Progress`): a job reports it as it runs (`run_with` and its
  `Context`); the services keep the latest per job (`KernelService::progress`); the web worker
  posts `{ id, progress }` while it runs. The STEP read reports `Reading`, then
  `Bodies { done, total }`. Meshes are counted by the editor as their jobs end.
- **A load** (`Editor::loading`): a file opened (from `Editor::new`) or a STEP file imported,
  until its last final mesh: the file read (an import), previews done / asked, finals done /
  asked. `LoadEnd` (taken once by the app) says how it ended.
- **The window** (`apps/solidsketch`, `loading_window`): a modal over the active document once
  its load has lasted 0.3 s (a small file would only flash it): its title (« Ouverture de… »,
  « Import de… »), a row per stage with its bar, « Annuler » (or Esc). It stays **until the last
  final mesh**. A document loading behind another tab shows it when brought forward.
- **Cancel is a real stop** (`KernelService::cancel`): a job not started is dropped and answered
  `Error::Cancelled` at once; a running one is stopped through its SolidCore `CancelToken`
  (`Kernel::set_cancel_token` on every kernel the job makes, handed to the threads of
  `parallel_map`; a STEP read also stops between bodies). On the web the worker cannot be
  interrupted: the job is answered cancelled, the worker replaced at the next poll and given
  the jobs left. A cancelled import is undone (it is one history step); a cancelled opening
  closes its tab. Esc on an operation computing cancels its job too. A document closed stops its
  jobs (`KernelHandle`'s drop).
- **Measured** (`bench_step_loading`, `BENCH_STEP`, 8 threads, dev profile, Cruise): read 1.75 s,
  every preview drawn at 2.9 s, the last final mesh at 14.7 s; cancelled after the previews, an
  operation done 50 ms later.

## 7 septies. Control protocol (debugging now, MCP later)

Decided on 2026-11-15 (questions / answers), after SolidCraft's control channel and MCP server
(`crates/control`, `crates/mcp`, `crates/mcp-server`, `docs/control-protocol.md`, `docs/mcp.md`,
`docs/browser-api.md` there). The first step is built (below, "Order"); the rest is the plan. It grows the control
channel of §7 quater ("External driving") into a protocol that an agent debugs the running
app with today — the active tool, its preview and inferences, the geometry — and that an MCP
server will serve unchanged later. A developer's tool for now, not a feature of the product:
it stays off by default, in the *Débogage* tab.

### Principles

- **One method set, several transports.** A request is a method and JSON parameters,
  `{"method": "model.geometry", "params": {…}}`; the reply `{"ok": true, "result": …}` or
  `{"ok": false, "error": "…"}`. The same methods are reached over loopback HTTP (desktop),
  from JavaScript in the page (web), and through MCP (each tool a method or a few). A method
  is written once, as a handler on the UI thread.
- **Typed and self-describing.** Each method's parameters and result are Rust types
  (`serde`, `schemars`): the JSON Schemas come from them (`app.methods` lists them, the MCP
  tools' `inputSchema` later). Strict: an unknown field or a wrong type is refused, naming
  the field, and nothing runs.
- **What runs headless says so.** The methods split into namespaces by what they need: the
  model and the tools run on an `Editor` without a window (the `tools` crate already does in
  its tests), the `ui.*` ones need the window. A headless backend serves all but `ui.*`.
- **No waiting by guess.** Every reply that may change the model carries
  `{document, revision}`; `app.wait` returns when the revision moves past a value, or when the
  editor is idle (no kernel job, no load) — never a `sleep` in a script.
- **Bounded replies.** Filters (`ids`, `selection`), opt-in heavy parts (`brep`, `mesh`), a
  size cap with an explicit error: an agent's context is small.
- **Everything undoable.** Methods that change the document go through `Editor` and its
  history, as the user's actions do; a `model.batch` is one undo step.

### Methods

| Namespace | Methods | Headless |
|---|---|---|
| `app.*` | `ping`, `methods` (names, schemas), `commands` (the registry: id, label, enabled, checked, shortcut), `run` (a command id), `wait` (`revision > n` or `idle`, with a timeout), `docs` (list, new, switch, close, open a path) | all but the documents' window side |
| `model.*` | `document` (the recipe: the file's JSON, not saved), `outliner` (the tree with `EntityPath`s), `geometry` (per part or occurrence: id, name, world transform, bounds, volume, pieces, topology counts, OCL dimensions in the part's axes, `stale` while computing; `brep`: the SolidCore record; `mesh`: the display mesh, an approximation), `measure`, `run` (an `Operation`), `batch` (operations in order, rolled back on the first error), both with `expect` (volume, pieces, bounds: measured after, a mismatch an error) | yes |
| `tool.*` | `state` (kind, step, status, VCB values and the one typed, options, busy, the `Preview` as data: lines, labels, ghost solids with bounds and optionally B-rep, the last inference: kind, point, entity, locked axis), `set` (the active tool), `input` (a list of `ToolInput`s — hover, click, double click, drag, keys, modifiers — at pixels or at **world points**, projected by the camera), `option`, `type` (text in the VCB) | yes |
| `view.*` | `camera` (read; set: eye, target, up, fov, perspective, or a standard view, or fit), `project` (world points → pixels), `pick` (pixels or rays → entity, B-rep face or edge, point, inference) | yes (a virtual viewport of a given size) |
| `journal.*` | `log` (`since`: inputs, tools, options, operations applied or failed, kernel jobs with durations and errors, undo, redo), `failures` (each failed kernel job: its operation, its job — input solids and tasks — to run again headless), `record` (inputs from a document and a view, written as a `tools` test) | yes |
| `ui.*` | real egui events (`click`, `move`, `drag`, `key`, `text`, `scroll`), `handles` (where named widgets were drawn last frame), `screenshot` (the viewport or the window, PNG), `messages` | no: the window |

`tool.input` feeds `Editor::input` directly: deterministic, no window focus, the same calls as
the `tools` tests (`click(e, p)`), which a sequence of inputs can later be written out as.
`ui.*` sends real egui events (one pointer event per frame, as SolidCraft): it tests the real
interface — ribbon, panels, the translation of egui events into `ToolInput`s — where the
CGEvent tool is used today. **Named handles** (as SolidCraft's `ui-registry`): widgets publish
their rect under a stable name while they draw (`toolbar:<command id>`, `panel:…`, a tool's
grips), so a script clicks a name, not a pixel.

### Transports

- **Desktop, HTTP on the loopback interface** (the §7 quater server, kept for `curl`):
  `POST /call` with `{method, params}` as body, the only route (the first routes, `GET
  /state`…, are gone: nothing used them but scripts). Off by default, as now.
- **Token**: at each start the server makes a random 256-bit token and writes it, as one line
  of hex, to `control/<port>.token` in the app's data folder (`dirs::data_dir()/SolidSketch`;
  folder `0700`, file `0600`; on Windows, the user alone), and the port to
  `control/current.port`; both removed when the server stops. Every request sends
  `Authorization: Bearer <token>`; `X-SolidSketch: 1` stays (CSRF guard). A client finds the
  running app by these two files. `SOLIDSKETCH_CONTROL_TOKEN` gives a token for a launch
  (tests, a sandbox).
- **Web**: no listener (a page cannot open one). The same methods are exposed to the page as
  `window.solidsketch.call(method, params)`, a promise (`wasm-bindgen`), handled on the UI
  thread between frames like the desktop's. No token: any script of the page already has the
  app's power; it reaches nothing outside the tab. Per method: files are the browser's (the
  document comes and goes as JSON text, `docs.open` takes text, not a path); `screenshot`
  reads the canvas; `wait` and the kernel's work behave as on desktop (the Web Worker
  answers like the threads). What drives it: an agent able to run JavaScript in the tab
  (Claude in Chrome), a Playwright test, the console. Later, the page may register the same
  tools with the browser (WebMCP, `navigator.modelContext`, once browsers ship it).
- **MCP** (`apps/solidsketch-mcp`, stdio): a thin server mapping MCP tools onto the
  methods, through a `Backend` trait with two implementations — **`Remote`**, the running
  desktop app found by `control/current.port` and its token (to watch the agent work), and
  **`Headless`**, an `Editor` and a kernel in the process, no window, used when no app runs
  (`--headless` forces it): every method but `ui.*`. The MCP layer itself (JSON-RPC over
  stdio, lifecycle, schemas) is hand-written (see "Order", 5).

### Crates

- `crates/control` — `solidsketch-control`: the requests and replies (types, schemas), the
  dispatch of a request to a handler, the token and discovery files, the loopback server and a
  client. No UI toolkit. The handlers that need only an `Editor` (`model.*`, `tool.*`,
  `view.*`) live beside it so that both the app and the headless backend call them.
- `apps/solidsketch/src/control.rs`: what the window adds — documents, registry commands,
  `ui.*`, screenshots, the title bar's mark — and the web's `window.solidsketch`.
- `apps/solidsketch-mcp`: the MCP server and its two backends.

### Order

1. **Built** (2026-11-15, branch `feature/control-protocol`): `crates/control` with `POST
   /call` (the only route), the token and discovery files; `app.methods` (the
   parameters' schemas), `app.state`, `app.wait`, `app.commands`, `app.run`, `app.docs`
   (`action`: list, new, switch, close), `app.screenshot`, `model.document`, `model.geometry`
   (summary, `brep`, `mesh`), `tool.state`. `scripts/control.sh <method> [params]` calls one
   from a shell (it finds the port and the token).
2. **Built** (2026-11-15, same branch): `tool.input` — `hover`, `click`, `double_click`,
   `drag` (`from`, `to`, `steps`), `key` (`name`), `vcb` (typed, then Enter), `tool` (by
   its id, `ToolKind::from_id`) — each pointer at a world point (`at`) or a pixel (`px`, the
   viewport's points, origin top left); a click is a hover then the click, as the viewport
   gives them. `view.camera` (read; set: `perspective`, `view`, `eye` + `target`, `fov_deg`
   or `height`, `fit`; the window adds `pixels_per_point`, a screenshot's scale),
   `view.project`, `view.pick` (the scene's pick: part, path, B-rep face, point, normal).
   Every editor method first takes in what the kernel finished (`Editor::poll`: a headless
   session has no frames), and *idle* waits for the meshes too (`Editor::meshing`): a pick
   or an inference after a change needs them. `Camera::project` is in `f64`, the exact
   inverse of `Camera::ray` (it went through the GPU's `f32` matrix: a box clicked at
   600 × 400 came out 599.99995): a world point clicked lands on itself, to 1e-9. Between inputs that depend on a change (a
   face of the box just drawn), `app.wait` with `idle` — the kernel computes in the
   background in the window.
3. **Built** (2026-11-15, same branch): the editor keeps a **journal**
   (`crates/tools/src/editor/journal.rs`, `Editor::journal`): a ring of the last 1 000
   entries, numbered — inputs (hovers in a row are one entry, counted), tools, options,
   operations applied (revision) or failed (why), operation jobs back (ms, error), mesh jobs
   failed, undo, redo; and the last 20 **kernel failures**, each with its `Job` (input solids
   and tasks, serialisable): an operation's job that failed, a recipe that skipped bodies
   (`Output::Recipe::failed`), a mesh refused. `journal.log` (`since`, `limit`),
   `journal.failures` (`seq`: one in full; its `job` read back and given to
   `solidsketch_kernel::run` fails again, headless). **Recording**: `journal.record`
   `start` keeps the document (its JSON) and the tool, then each call from outside —
   `input`, `set_tool`, `set_tool_option`, Shift / copy / Alt changed, undo, redo — with the
   camera and viewport they were given in when they changed (hovers in a row: the last);
   `Editor::depth` leaves out what the editor calls itself (its own hovers, a tool it goes
   back to), so playing them again does not do it twice. `stop` writes a test of
   `crates/tools/tests/<name>.rs` (the document beside it as `<name>.solidsketch`): the
   editor with the kernel inline, the view set, each input after a `poll` (a frame), the
   revision and the parts reached checked. Recorded in the app (a box, then a cylinder on
   it), the test passes and builds the same geometry. Not recorded: what panels do besides
   (the outliner's selection, entering a component from it).
4. **Built** (2026-11-15, same branch). **Real input**: `ui.click` (`double`), `ui.move`,
   `ui.drag` (`steps`), `ui.key` (egui's key names, `pressed` for one edge), `ui.text`,
   `ui.scroll` — each at `where`: a window point (`pos`), a handle's centre (`handle`) or a
   world point's pixel in the viewport (`at`), with `button` and `mods` (`shift`, `ctrl`,
   `alt`, `command`). They are egui events queued one frame at a time
   (`apps/solidsketch/src/driving.rs`; `raw_input_hook` feeds them, the modifiers as
   `ModifiersChanged` events, egui 0.37 having no modifiers in `RawInput`) and answered at the
   frame after the last one, once the widgets took them in; the window need not have the
   focus. Being egui points (`f32`), a box clicked this way measures 599.99993: `tool.input`
   stays the exact path, `ui.*` the real interface's. **Named handles**
   (`apps/solidsketch/src/handles.rs`, after SolidCraft's `ui-registry`): widgets publish
   their rect per frame, double-buffered, kept only while the channel is on (nothing made
   otherwise) — `ribbon:<command>`, `ribbon_group:<group>`, `ribbon_label:<group>`,
   `titlebar:<command>`, `titlebar:file`, `titlebar:views`, `menu:<command>` (menu items),
   `option:<key>` (a toggle or a length field), `option:<key>:<value>` (a choice's
   segment; `part_scope`, `body_sign`, `machinings`, `profile`), `tab:<i>`, `tab_close:<i>`,
   `tab_new`, `quick:<command>`, `quick:more`, `vcb`, `viewport`; `ui.handles` (`prefix`)
   lists them with `pixels_per_point`. `ui.screenshot`: the whole window (egui's
   `request_screenshot`), PNG. `ui.messages` (`clear`): the status bar's texts as they
   changed, the last 200, with their time. Not yet published: the outliner's rows, entity
   info's fields, the palette, the preferences.
   **Web**: `window.solidsketch.call(method, params)` (`apps/solidsketch/src/web.rs`, a
   `wasm_bindgen` export `solidsketchCall`, set on `window` by `web/index.html`), a promise of
   the result (`Uint8Array` of PNG for a screenshot), rejected with an `Error`; at most 256
   calls waiting. Every method of the desktop, served by the same code (`driving::Asker`:
   a call and where its answer goes, whatever carried it); `app.screenshot` is the window's
   (the canvas); no volume in `model.geometry` (the kernel is in the worker); the handles kept
   once the page has called. Checked in headless Chrome (Playwright): a box by `tool.input`
   (computed in the Web Worker), `app.wait`, `model.geometry`, a real click on
   `ribbon:tool.cylinder`, the screenshot, a refusal.
5. **Built** (2026-11-15, same branch): `apps/solidsketch-mcp`, an MCP server over stdio
   (JSON-RPC 2.0, one message per line; revisions 2025-06-18, 2025-03-26, 2024-11-05:
   `initialize`, `ping`, `tools/list`, `tools/call`). **Hand-written** (`src/mcp.rs`, ~200
   lines, `serde_json` only), not `rmcp`: the subset is small, and the SDK would bring tokio
   and its own `schemars`. **One tool per method** the backend serves, `.` written `_`
   (`model.geometry` → `model_geometry`; `app.ping` and `app.methods` left out, MCP has its
   own), its `inputSchema` the method's parameters' (schemars), `readOnlyHint` for the
   methods that only read; a JSON result is a text block, a PNG an image block (base64), a
   method's refusal a result with `isError` and its message (for the model to correct), an
   unknown tool a JSON-RPC error. `initialize` gives instructions: units, `tool_input` at
   world points, `app_wait` after a change, the reading tools. **Backends**
   (`src/backend.rs`): `Remote` — the running app, its port and token read again at every
   call (`solidsketch_control::server::discover` and `call`, a client of the loopback
   server), so an app started or started again later is found; `Headless` — an `Editor` of
   its own (the kernel inline, a viewport of 1280 × 800), on an empty document or `--in
   FILE`, serving every headless method plus `app.state` and `app.wait` (answered at once:
   the kernel ran inline), the window's refused, said why. Without option the server takes
   the app if its channel answers, else headless; `--remote`, `--connect PORT`,
   `--headless`, `--in FILE` choose. The log goes to stderr. In Claude Code:
   `claude mcp add solidsketch -- <repository>/target/debug/solidsketch-mcp`. Checked from
   a client over stdio, headless (12 tools, a box by `tool_input`, its geometry exact, a
   refusal) and on the app (25 tools, the same, a real click on `ribbon:tool.move`, a
   screenshot as an image).

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
- **Solids** (`Solid::from_polyhedron`, `crates/kernel/src/polyhedron.rs`): the faces become
  the STEP entities of a B-rep, in memory (a plane per face, a line per edge shared by its two
  faces, one closed shell per set of joined faces: a `MANIFOLD_SOLID_BREP`, or a
  `SHELL_BASED_SURFACE_MODEL` for several lumps), read by SolidCore's STEP topology reader
  (`solidcore_step::topology::read_solid`) **from their topology only**. Loops are turned to agree with the normals, then the faces of each
  lump turned alike across their edges (SketchUp keeps reversed faces in solids), the lump
  turned inside out if its volume is negative. **Only closed solids are kept**, checked
  before the kernel by counting (each edge between exactly two faces): a definition open,
  non-manifold or that the reader refuses is excluded and reported (its instances
  dropped). Measured on 2026-11-12 (a 1373-definition apartment, 52 k faces), with SolidCore's
  former `step-brep`: letting `build_body` fall back to stitching by geometry on the open ones took 100 s (up to 60 s for
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
  (`crates/kernel/src/step.rs`) by SolidCore's Part 21 parser (`solidcore_record::step::p21`:
  short entity names, strings decoded). SolidCore reads the **product structure** as data
  (`step::product::read`: products, representations, occurrences, maps, styles); it is walked
  here into products and placed instances: `PRODUCT_DEFINITION`s and their shape
  representations (joined by relationships without a transformation: the same axes),
  assembly occurrences placed by their relationship (`ITEM_DEFINED_TRANSFORMATION`:
  `frame₂ · frame₁⁻¹`, the two representations told apart when a writer swaps them), and
  `MAPPED_ITEM`s (`target · origin⁻¹`), each frame's matrix the kernel's
  (`solidcore::geom::step_matrix`). Not `solidcore_ops::step_assembly::placed_bodies`: it
  flattens the tree into world placements, and the model keeps the products shared. Bodies no
  product reaches, and topology outside any representation (`topology::roots`), are kept at
  the top.
- **Bodies**: read from their topology straight into records
  (`solidcore_step::topology::read_solid_with`, in their representation's units), in parallel
  natively (`parallel_map`, with the kernel's stack), then made the kernel's (a round trip).
  The pcurves the reader has no closed form for (a spline edge on a spline face…) are the
  kernel's own trims (`KernelTrims`: the face alone sent with those uses bare,
  `to_kernel_with_bare_uses`). **Only closed solids**: a surface model, an open shell or a body
  the reader refuses is excluded and counted in the status bar.
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

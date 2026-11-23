---
status: partly built
code: [crates/model/src/document.rs, crates/model/src/ops.rs, crates/kernel/src/engine.rs, crates/tools/src/scene.rs]
---

# Data model: parts, assemblies, machinings

The document Skraft edits: components (parts and assemblies) placed by occurrences, a part
being solids — its bodies — combined by a fixed formula. For anyone reading or changing
`crates/model`: what the document holds, how a change to a part reaches its copies, how its
shape is computed, how things are identified. A part's bodies are detailed in
[Bodies](bodies.md); opening a part to edit them, in [Opening a part](opening-a-part.md).

Decision record: [Every object is an occurrence of a part or an assembly](../decisions/0005-parts-assemblies-occurrences.md).

```
Document
├─ components : Id → Arc<Component>
├─ root       : Id of the root assembly (the model, never counted itself)
├─ next_id
├─ settings   : display style and units, saved with the model
├─ materials  : Id → Material { name, color, props }
└─ prop_defs  : key → PropDef — the properties' definitions (Properties)

Component { id, name, named, kind, guides, behavior, props }
  guides   : construction lines and circles in the component's axes (Tape measure and guides)
  behavior : how it is stretched — its cutters and locked axes (Stretch)
  kind     = Part     { defs, bodies, shape, failed, faces }             — a part (Bodies)
           | Assembly { occurrences: Vec<Occurrence> }                   — an assembly
Occurrence { id, name, component: Id, transform: T, material, hidden, props }  — an occurrence
```

Guides, body definitions and bodies have `props` too: typed values by key
([Properties](properties.md)).

Guides: [Tape measure and guides](../tools/tape-measure.md); `behavior`: [Stretch](../tools/stretch.md);
a part's content: [Bodies](bodies.md).

Not yet: the cut list itself; the cut-list dimensions show in the entity info.

## Components: parts and assemblies

- **A single sharing mechanism**: a component is a shared, named definition, placed by
  occurrences. A **part** holds solids (its bodies, positive and negative); an **assembly**
  holds occurrences of other components. "Component" is the common term, which the interface
  seldom uses: it speaks of parts and assemblies.
- **No groups**, no loose geometry: everything in the model is an occurrence.
  Drawing a box creates a part and its first occurrence; gathering occurrences creates an
  assembly (named « Assemblage#1 », « Assemblage#2 »… until the user renames it).
- A part is edited **directly** (no "entering" it); an assembly is opened like a SketchUp
  component (active context, [Ids, paths, context](#ids-paths-context)). A part can also be
  opened, to edit its bodies ([Opening a part](opening-a-part.md)).
- **Name**: the component's name is the part's name; the **occurrence name** names one
  occurrence of that part ("Left", "Right"), as in OpenCutList. Unique in the document
  (`Document::unique_name`): an automatic one (not `named`) numbered from #1 as SketchUp's
  (« Pièce#1 », « Pièce#2 »); one given, taken as it is, a copy of it « Caisson#2 », a copy of
  « Caisson#2 » the first number free ([Automatic names](bodies.md#automatic-names)).
- **Unused definitions stay**: a part or an assembly left without occurrence (erased,
  exploded, replaced, a boolean's tool) stays in the document, named or not, shown in the
  « Composants » card's « Inutilisés » ([Components library](../ui/components-library.md)) and
  placed again from there, until « Purger les inutilisés » (`Operation::PurgeUnused`) drops
  every component not in the model (the solid tools' broom deletes a consumed tool part at
  once, [Solid tools](../tools/solid-tools.md)). But an empty assembly placed nowhere, which holds
  nothing, goes after every operation (`Document::drop_unused_empty_assemblies`). An import
  purges what it brought and does not place (`Document::purge`).
- **Count in the model**: `Document::counts_in_model`, how many times each component is placed
  under the root at any depth (an assembly's count multiplying those below it); a component
  placed nowhere under the root is absent from it.
- **Empty assemblies**: an assembly with neither occurrences nor guides — or holding only
  empty ones — is taken away, its occurrences deleted wherever it is placed, as SketchUp
  deletes an empty group (`Document::dropping_empty_assemblies`, a batch of `Delete`;
  `Editor::drop_empty_assemblies`). After every operation, within its step (its parts erased
  under « Pièces », cut…), the active context excepted: emptied from inside, it stays while
  edited and goes once left (exit, the navigator, the model's row), within the step that
  emptied it when that is the last one (one undo brings it back full), else as a step of its
  own. A parent left holding only it goes too, once left in turn. Unlike empty parts, kept and
  flagged ([Empty parts](bodies.md#empty-parts)): an empty part still holds bodies (its
  machinings) to fix, an empty assembly holds nothing.
- Not yet: **properties** (exclusion from the cut list…) on any component, and the part's
  **type** — the kind of stock it comes from (for wood, OpenCutList's solid wood, sheet good,
  dimensional, edge, veneer, hardware…; other materials have their own) — not a component
  property: it will come from the **material** applied to the occurrence, as in OpenCutList
  (`MaterialAttributes.type`). A material is a name and a colour today.

## Occurrences and transformation

- An occurrence carries **T = translation + rotation + optional mirror** (determinant ±1), its
  **material**, its occurrence name and whether it is **hidden** ([Interaction](../tools/interaction.md)). Moving or rotating only changes T: instant, no kernel,
  no remeshing, no drift of the geometry.
- **T is never baked into the B-rep.** Booleans and stretches work in the part's frame,
  the other operand brought in by the relative transformation; export applies T on the fly.
- **No scale in T.** Scaling (uniformly) changes the dimensions, hence the part: it is a
  geometry change. **Non-uniform scaling is forbidden** everywhere: it breaks circles,
  inferences and dimensions.
- The **mirror stays in T**: a part and its mirror image remain the same part (the cut list
  tells them apart, unless the part is declared symmetric). **Why:** SolidCore applies a
  mirror, alone or with a rotation, to every geometry (planes, cylinders, spheres, cones, tori,
  intersection curves), flips the faces correctly, and the mirrored solid then serves as
  target and as tool alike; cutting then mirroring = mirroring then cutting
  (`crates/kernel/tests/part_model.rs`).

## A part's axes

- The part's frame is the one its bodies are placed in: the length — **the part's main direction**
  (the grain, for wood; no separate property) — runs along the red axis. The cut-list dimensions are
  read on the bounding box of the part's **final** B-rep in this frame ([The cut list reads the
  final](bodies.md#the-cut-list-reads-the-final)) — its **exact** box, which only the kernel
  computes (`Solid::bounds`): the record's vertices miss curved faces (a cylinder has one vertex per
  circle, a torus none). The worker gives it with each display (`Display::bounds`), the scene keeps
  it per solid (`DisplayScene::bounds_of`), for the shapes and the open part's bodies. It is
  SolidCore's tight box (its loose one should the tight one fail).
- Tools working on a part use its axes; elsewhere, the context's.
- The part's axes are independent of its bodies
  ([Bodies placed in the part's axes](bodies.md#bodies-placed-in-the-parts-axes)).
- **`Operation::ChangeAxes { component, axes }`** gives a part or an assembly new axes, nothing
  moving in the world: its occurrences, bodies or children, guides, stretch settings, the views
  of it and the dimensions on it are brought into them; `axes` is a direct rigid transform, the
  model's root refused ([decision 0053](../decisions/0053-changing-a-components-axes.md)). An
  assembly's parts keep their axes. The tool: [Reorient](../tools/reorient.md).
- **Parts a tool draws are born oriented** (`Operation::DrawPrimitive`), by OpenCutList's rule: blue
  across their largest faces, red along their longest edges, oblique on a slanted prism (decision
  0053). Imports, copies, pastes, booleans and stretches keep the axes they have.

## Machinings and the recipe

A machining is a negative body of its part ([Bodies](bodies.md)):
`Part = (∪ positives) − (∪ negatives)`.

- A part is **a recipe**: its bodies. The **final B-rep** is computed by the kernel
  (asynchronous, [The kernel](kernel.md)) and cached (`Part::shape`, saved with the file;
  absent when the part is its single positive body, unplaced: no kernel); it is never the
  source.
- **The order of bodies does not matter** (a set, for the user), the computation can be
  incremental, and a failing machining is isolated and reported without blocking the others.
- **Computation: successive, incremental subtractions.** **Why:** measured (board − n holes,
  dev build), joining the tools first then cutting once is not faster (50 holes: ~0.4–0.8 s
  either way). Adding machinings = subtractions on the cached final; removing or moving one =
  computing the part again from its bodies (`Document::recompute`).
- **Measured failures**: a tool outside the blank, flush with a face, touching an edge or
  tangent does nothing abnormal; a tool that swallows the whole shape leaves the part empty
  ([Empty parts](bodies.md#empty-parts)).
- **The recipe task** (`Task::Recipe { base, positives, negatives }`): the union of the
  positives minus each negative, every input placed by its transform, then joined to `base`
  when given (positives added to a part as it is); a boolean that fails is skipped and
  reported by its index (positives then negatives, one list), the others still done — the task
  never fails on a tool. Removing or moving bodies computes the part again from its bodies;
  copying them adds only the copies to the shape as it is. The failed bodies are kept with the
  shape (`Part::failed`, the same cache, saved with the file) for the interface to show. In
  the web worker a kernel panic still aborts the whole job ([The kernel](kernel.md)): only
  failures SolidCore reports are isolated there.
- **One kernel session**: the recipe's steps run in one SolidCore session, the shape read back
  to a record only at the end. **Why:** a shape between two steps may be one SolidCore writes
  to a record but refuses to read back — two blanks touching along an edge only until a third
  fills the corner (« a sheet touching itself along an edge »): step by step, the third blank
  would fail for nothing. A boolean consumes the shape, so a failing step is marked and the
  session starts again without it (one more session per failure); should the final shape not
  go back to a record, the recipe runs step by step (`recipe_stepwise`). Fewer conversions,
  too: one per input and one at the end, instead of two per step.
- Low level: the tool is **any B-rep**, placed in the part's frame. Not yet: machining types
  (hole, groove, mortise…) generating that tool from their parameters (a proposal:
  [Procedural parts](../decisions/0007-procedural-parts.md)).
- **Links**: each body definition has a `source` (`BodySource`), always `Local` for now; the
  format has the field for machinings linked to hardware (not yet).

## Copies and propagation

- **Copying an occurrence creates an occurrence of the same part**: "Leg ×4" (the count is in
  the entity info, not in the outliner), counted without guessing anything, without thinking of "making a component" before copying.
- A **change to the part** (geometry, bodies, properties) affects **all its occurrences**
  or **only this one**: the **scope** (`Scope::All` / `Scope::This`), an option of the tool
  (option bar, [Application interface](../ui/interface.md)), "all occurrences" by default,
  remembered per tool. It follows the solid operations of Smart Reshape (OpenCutList,
  `common_solid_boolean_apply_worker.rb`):
  - **all occurrences** (OCL's `make_unique: false`): the part is changed in place, and all its
    occurrences — including those seen through copies of an assembly — show the result. The
    operation is computed in the frame of the clicked occurrence;
  - **only this one** (`make_unique`): only the clicked object changes. Along its path, top
    down, each assembly shown elsewhere becomes a separate assembly, then the part becomes a
    separate part (`Document::separate`); the other occurrences are never touched;
  - when an operation has **several targets** (Stretch, [Stretch](../tools/stretch.md)):
    occurrences of one part that the operation leaves identical in their own frame keep a
    single part; if they diverge, they become separate parts even under "all occurrences",
    since the part cannot hold two results (OCL's rule).
- **Propagation is visible**: the number of occurrences affected in the option bar (« Tous les
  exemplaires ×4 »), the other occurrences of an open part marked `Related`
  ([Opening a part](opening-a-part.md)). "Make unique" is **« Faire une pièce à part »**
  (`Operation::MakeUnique`); on an assembly it copies its first level only (its
  sub-assemblies stay shared).
- **No scope for a part placed once**: the choice is left out of the bar when
  the part aimed at (the open part, the boolean's target or the part hovered, Stretch's single
  target) has one occurrence — both choices are the same, `Document::separate` copying nothing
  —; nothing aimed at yet, it shows, to be set beforehand. Its hidden value stays as saved
  (harmless), and Tab finds no choice to switch.

## Ids, paths, context

- **Stable `u64` ids**, drawn from one counter (`next_id`). Those of components are unique in
  the document; those of **occurrences** are unique **within their assembly**, those of
  **bodies within their part** (groups included): a copy of a component ("make a separate
  part", scope "only this one") keeps the ids of its occurrences and of its bodies, so that
  paths, the context and the selection stay valid. A body is named by `(part, body id)`. A
  path is always resolved from the root. An operation designates a face by
  `(part id, face index in the final B-rep)` — enough in direct modelling, since the operation
  applies at once.
- **Occurrence path** (`EntityPath`): an entity of the scene is designated by the path of
  occurrences from the root; its world transformation is the product of the T along the path.
  In a part, `EntityPath::group` names the group of bodies open, if any
  ([Body groups](bodies.md#body-groups)).
- **Active context**: the assembly being edited, and the path leading to it.

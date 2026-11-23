---
status: built
code: [crates/model/src/document.rs, crates/model/src/ops.rs, crates/model/src/weld.rs, crates/model/src/stretch.rs, crates/model/src/clipboard.rs, crates/kernel/src/engine.rs, crates/tools/src/editor/mod.rs, crates/tools/src/editor/selection.rs, crates/tools/src/scene.rs, apps/skraft/src/panels.rs]
---

# Bodies: a part as positive and negative solids

A part is made of **bodies**: placed solids with a sign, the positive ones (blanks) adding
material, the negative ones (machinings) taking it away, by a fixed formula with no history.
Bodies share shapes through definitions, as occurrences share components, and definitions may
be groups of bodies — a CSG tree. This page is the reference for the part's content: the
model, how the part is computed, how bodies are shown, edited, named, and how booleans between
parts and Stretch go through them. Opening a part to edit its bodies: [Opening a part](opening-a-part.md).

## The model

```
Part = (∪ positive bodies) − (∪ negative bodies)

Part    { defs: Vec<BodyDef>, bodies: Vec<Body>, shape, failed, faces }
BodyDef { id, name, named, solid, profile, source, group }   — a shape, in its own axes
          profile : Option<BodyProfile>                      — a profile (Prism tool), solid its cache
          group   : Option<BodyGroup { children, failed }>   — « Body groups » below
Body    { id, name, def, sign: Positive | Negative, placement: T }
                                                             — an occurrence of a definition, placed in the part's axes
```

**Why**: a profiled blank ([Prism tool and profiled blanks](../tools/prism.md)) bound to the
part's axes — its outline in their XY plane, extruded along their Z — would make changing a
part's axes transform the profile, and drop it whenever Z ends up in the plane or oblique.
Giving the blank its own placement in the part frees the profile from the part's axes; one
step further, the blank and the machinings become one notion: a placed solid with a sign.

Decision record: [A part is positive and negative bodies](../decisions/0018-part-bodies.md).

- `shape` is the part as last computed (absent when the part is its single positive body,
  unplaced: `Part::is_plain`, no kernel), `failed` the bodies whose boolean failed, `faces`
  the materials of single faces of the shape ([Look](../render/look.md)). The shape and
  `failed` are a cache saved with the file: the bodies are the source.
- `Solid::pieces` counts the record's bounded regions that are not voids (« One piece »
  below).
- The operations on bodies: `AddBody { sign }`, `DeleteBodies`, `TransformBodies`,
  `DuplicateBodies`, `ArrayBodies`, `CopyBodies`, `PasteBodies`, `SetProfile { body }`,
  `SetSign`, `StretchBodies`, `MakeBodiesUnique`, `GroupBodies`, `UngroupBodies`,
  `RenameBodyDef`, `RenameBody`; each takes the part's `target` path and a scope
  ([Copies and propagation](data-model.md#copies-and-propagation)).

## Bodies placed in the part's axes

- **Every body is placed in the part's axes**, positive or negative. A profile lives in its
  body's own axes (its XY plane, its Z), never in the part's. A part drawn is one positive body
  at the identity; machinings are the negative bodies; a free part (import, boolean result) is
  one positive body of a free shape.
- **The part's axes are independent of its bodies.** Changing them would be correcting every
  body's placement and compensating the T of each occurrence: exact, for any rotation, no
  boolean, the profiles never touched (`Operation::ChangeAxes`,
  [decision 0054](../decisions/0054-changing-a-components-axes.md)). The part's shape is
  transformed, its faces in the same order: a single positive body keeps its face names once
  placed (`Part::is_one_body`), and stays one body when pasted or used as a tool.

## A fixed formula, no history

- The union of the positives minus the union of the negatives, intersected with each
  intersection (`(∪P − ∪N) ∩ I1 ∩ I2 …`, [A third sign](#a-third-sign-the-intersection)). **Every negative
  cuts every positive of its group** — the part's, at the top level ([Body groups](#body-groups))
  —, those added later included; the order bodies were made in does not matter. A hole is
  filled by deleting its negative, not by adding a positive into it.
- **Incremental computation** (`Document::recompute`): adding negatives = subtractions on the
  cached final, adding intersections intersects it with them; adding positives `P` =
  `final ∪ ((P − ∪N) ∩ ∩I)` (since `(A ∪ B) − N = (A − N) ∪ (B − N)`, the recipe's `base`);
  a part with a single positive, the common case, makes no union at all. Removing or moving a
  body computes the part again from its bodies.
- **Failures**: a body whose boolean fails is skipped and reported by its id, the others still
  computed (`Part::failed`; a group's children in `BodyGroup::failed`).

## A third sign: the intersection

A body adds (positive), takes away (negative), or **intersects (∩, « Intersection »)**: its
level — the part's root, a group — keeps only what lies inside it
([0045](../decisions/0045-intersection-a-third-sign.md)). `Sign::Intersect`, written
`"intersect"`.

- **Each intersection limits**: `(∪P − ∪N) ∩ I1 ∩ I2 …`, the order of none mattering
  (`(X − N) ∩ I = (X ∩ I) − N`). Their union limits by grouping them first in an intersection
  group, whose children are positive (the shape it limits by, as a negative group's the shape
  it takes away).
- **The kernel's recipe** takes them after the positives and the negatives (`Task::Recipe`'s
  `intersects`, `RecipeNames::intersects`; the node task's tree, `TreeBody::intersect`);
  failures are reported by index, positives, negatives then intersections. A level limited to
  nothing is empty.
- **Grouping several intersections alone, or ungrouping an intersection group of several
  bodies, is refused** (`Refusal::IntersectsRegrouped`): the union of the shapes is not their
  intersection. An intersection alone groups (an intersection group of one positive child) and
  ungroups freely; intersections beside positives group as negatives do (they then limit that
  group only).
- **Drawn**: the drawing tools' « Solide » in an open part has a third choice, « Intersecter »
  (`Editor::body_sign`); `context.sign` takes `intersect` (docs/control-protocol.md).
- **Shown** in an open part as a ghost of its own hue (`theme.intersect`, a blue-green; the
  machinings' red; `Look::Intersect`), the ∩ icon on its navigator row; the navigator's sign
  button cycles `+ → − → ∩ → +` (« Intersection : la pièce ne garde que ce qui est dedans »).
- **Moved in the tree**, an intersection stays one ([Moving bodies](#moving-bodies)). A node
  over intersections alone is an intersection rounded (`group_sign`).

## Plane welding

`crates/model/src/weld.rs`. After every operation, a planar face of a body it changed that
lies within `WELD` (1e-4 mm) of a parallel planar face of a body of the same part it left
alone is put on that face exactly, its body stretched by the difference (its profile kept, a
definition it shares copied first); the part is then computed again, in the same job.

**Why:** faces a few 1e-5 mm apart come from values read on the `f32` display mesh or from
sums of rounded moves; the booleans keep what lies between them (a negative a hair short left
a 14 nm skin).

- **No drift**, by four rules: a face goes onto a face that is there (its exact value, never a
  mean); the bodies the operation left alone are the references and never move; a face goes
  onto the nearest reference, once (a face welded is no reference: no chains); never onto a
  face of its own body. Only extruded bodies move (caps along their axis, straight sides across
  it); any body with planar faces is a reference.
- **Through groups**: welding works on the part's leaves (no group), each reached through its
  groups and placed in the part's axes (`Leaf`); a leaf the operation changed is welded onto
  the faces of the leaves it left alone, anywhere in the part — in its group or out of it —,
  then the groups on its way are computed again, the deepest first, and the part. A leaf in a
  group shown by copies is never moved (a face lies elsewhere in each copy), a reference still.
- Each weld is in the operation's `Outcome::welds` and the journal (`welded`: part, body,
  reference, distance). Opening a file welds nothing.

## The cut list reads the final

The part's dimensions are the exact box of the **final** B-rep in the **part's** axes, the
length along their red axis ([A part's axes](data-model.md#a-parts-axes)); the entity info shows
them. A machining that shortens or narrows the part changes its dimensions. The exact outline
for the DXF / SVG exports and nesting is to be read from the final B-rep too (its faces across
the part's Z, as OpenCutList does), not from the profiles: **a profile is an editing parameter
of its body**, it carries nothing for the cut list.

## One piece

The final must be a single lump. It is not when a positive touches no other, or when negatives
split the part (a groove right through). The part is then **in error**:
- marked in the outliner: ⚠ and its name in the error colour (« Pièce en N morceaux : à
  corriger »), and shown whole so the problem can be seen;
- **no cut-list dimensions** while it is in error: the entity info says « en N morceaux, à
  corriger » instead;
- checked **at the root only**: a sub-group in several lumps can be meant (a jig of three
  holes).

Not yet: the positives apart — those outside the lump holding the first positive — drawn and
listed in the error colour, as failed machinings (which lump a body lies in is not known
cheaply); a split made by negatives would blame no body, only the part.

## Empty parts

A part may be left without material — its last blank made a machining or deleted (every body,
even), or a machining swallowing it. Nothing is refused: the part is **empty**, its shape
`Solid::empty()` (`Solid::is_empty`, no face), **0 pieces**, the same error as « in N
pieces »: ⚠ and its name in the error colour in the outliner (« Pièce vide : à corriger »),
« vide, à corriger » instead of its dimensions and in the open part's info. Closed, nothing of
it is drawn (the outliner reaches it); open, its bodies are, so a blank can be given back.

Decision record: [A part may be empty](../decisions/0025-empty-parts.md).

- The **kernel** says so, never the model (an operation is recorded with stand-ins, empty too):
  `Task::Recipe` leaves out its empty inputs, reported as failed, and without a positive
  holding material gives its base, or nothing; a negative swallowing the shape gives the base,
  or nothing — not a failed machining; `Task::Boolean`'s difference gives nothing rather than
  failing (nothing cut from nothing, nothing cut by nothing, a tool swallowing the target); an
  empty solid is moved as it is and displayed as nothing. An intersection with nothing in
  common still fails.
- A **group** without a positive child is accepted: its result is empty, so it fails in its
  parent (skipped, marked) as a group whose boolean fails — its outliner row, as any failed
  group's, with ⚠ before its name in the error colour (« Groupe vide : à corriger », « Groupe
  en échec : à corriger »). A **node** whose only child is deleted is such a group: it stays,
  empty and in error, and so do the nodes above it. Deleting a node deletes its solid;
  « Éclater » is what gives its children back.
- The ⚠ goes up to the groups and the part holding a failed body, at any depth, their names as
  they are (« Un corps de ce groupe / cette pièce est en échec : à corriger »), and to the
  assemblies holding a part in error or holding one (« Une pièce de cet assemblage est en
  erreur : à corriger »; each definition looked at once per frame), so a folded tree still
  shows it.
- The file accepts a part or a group without a positive body.

## The interface of bodies

- **Drawing in an open part** ([Opening a part](opening-a-part.md)): the drawing tools' bar has
  a « Solide » choice, « Enlever » (a machining, the subtraction's icon) / « Ajouter » (a
  positive body, the union's icon; the names in the tips), back to « Enlever » at each opening.
- **Shown and picked**: the open part shows positive bodies as ghosts, negative ones as ghosts
  too, red, a little more opaque than the blanks (`Theme::machining_opacity`, the placement's
  tint weight added to the ghosts' opacity); one that failed stays opaque, in its own colour.
  All are picked (a negative one first, whatever the depth — it lies inside —, else the nearest
  positive one), selected, moved, copied and erased alike (`Piece::Blank(id)`,
  `Piece::Tool(id)`). A body hovered or selected stays see-through: its highlight washes its
  faces with the mark's tint over the ghost's own (`Highlight::ghost`, `fs_face_ghost_mark`),
  its edges as any highlight's.
- **The sign**: a toggle on the body's outliner row ([The interface](#the-interface)) flips
  it — the selected bodies' too when it is one of them
  (`Operation::SetSign { target, bodies, sign, scope }`, the last positive too, leaving the part
  empty), the part computed again.
- **Words**: the interface says « corps » for any body, « brut » (a blank) for a positive one,
  « usinage » (a machining) for a negative one. Positives and negatives are not two lists:
  the rows keep their order of creation.
- **Profiles**: a body with a profile opens to edit it ([Edit levels](#edit-levels)). The
  Profile tool edits the outline; the thickness is stretched with the Stretch tool along the
  body's Z (snaps, centred, offset or overall, cutters).

## Stretching bodies

[Stretch](../tools/stretch.md) goes through the bodies as it goes through an assembly's
components.

- In an open part, the Stretch tool takes the bodies selected
  (`Operation::StretchBodies { target, bodies, frame, … }`, the stretch's axes placed in the
  part's): along the body's own axes (one, « entity »), the part's (several, or « context »),
  or the model's. Each body in one section is moved; one across a cutter is stretched in its
  own axes, keeping its profile under [Prism tool and profiled blanks](../tools/prism.md)'
  rules when it stays one, else becoming a free shape; the guides selected follow their
  section. Their cutters are kept for the session only (nothing on the body in the file). The
  preview draws each body stretched from its recipe mesh (`Editor::body_swaps`).
- Shared definitions: « Body definitions » below; groups: « Body groups » below.

## Booleans between parts

The tool is brought into the target's axes by the relative transformation, the tool
occurrence leaves the model unless kept, under the scope ([Booleans across contexts](kernel.md#booleans-across-contexts));
several targets and tools in one operation, each target with the tools reaching it
([Solid tools](../tools/solid-tools.md)).
A tool that is its single positive body, unplaced (a shape drawn), comes as a copy of that
body, its profile with it — still editable; any other comes as a group of copies of its bodies
(`tool_as_body`, its shape the group's: no more kernel work).

- **Union**: the tool part's root comes **as a positive sub-group** of the target, its
  structure kept, its negatives reaching that sub-group only; a through-hole meant to drill
  the target too is had by exploding the group (« Éclater »). A target with machinings at
  its top level first has its bodies put in **a positive group of their own**, at the
  identity (its result the part as it was: no kernel; named after the part when the user
  named it, else « Groupe#n »), so they keep off the tools: `A ∪ B`, not `(A ∪ B) − N_A`. A
  target without one is left flat, so unions in a row stay at one level.
  **Why** not always, nor for a difference: grouping changes nothing of a difference's result
  (`(∪P − ∪N) − B` is `Group(P, N) − B`), and would nest one level deeper at each operation.
- **Difference**: the tool part's root comes **as a negative sub-group** (its bodies kept,
  still editable); each tool reaching the target is one more negative body, all cut at once
  (`recompute` with the bodies added: one recipe, a failing tool marked as any machining).
- **Intersection**: the target limited by the union of the tools reaching it, the tools kept
  ([A third sign](#a-third-sign-the-intersection)): one tool comes as an intersection body (as
  a difference's tool as a negative one, its profile kept when it is a shape drawn), several as
  the positive children of one intersection group — their union limits, not each. The target
  keeps its bodies.
  **No ∩ node**, nor any ordered operation. **Why:** that would turn the tree into a feature
  history (persistent naming); the intersection needs no order, a sign does it.

## Body definitions

Bodies are moved, copied and arrayed with the tools that do so to occurrences, so they share
their shape as occurrences share their component: copies made by Move or Rotate with a copy,
an array, or a paste into the part copied from share it, and a change to the shape (its
profile, a stretch) shows in every one.

- **The sign is the body's**, not the definition's: one shape can be a pin (positive) here
  and its hole (negative) there; the sign toggle flips the body clicked.
- **Definitions belong to their part** (not to the document): copies, an array, a paste into
  the part copied from share a definition (`copies_of_bodies` gives another part copies of the
  definitions, still shared between its bodies — a union, a paste elsewhere, a difference's
  plain tool); a definition no body uses any more is dropped (`drop_unused_defs`).
- **A change to a shape changes the definition**: `SetProfile` edits the definition, every
  copy shows it; the command `edit.make_unique` (« Faire une pièce à part ») in an open part
  (`Operation::MakeBodiesUnique`) gives each body selected a copy of its own.
- **Stretch** follows the components' rules ([Stretch](../tools/stretch.md), `Part::body_stretch`): bodies of one
  definition stretched alike (the same stretch in their axes) share the new shape; the change
  most of them take is made in place (the copies not reached show it), the others go to new
  definitions — all of them do when a body reached keeps the shape (only moved). The preview
  shows the same split. Unlike components, a definition changed in place is not moved back
  by its anchor: the copies not reached show the change in their axes as computed.
- **Display and file**: one mesh per definition (the recipe's key), the copies of a selected
  body marked « related »; the entity info shows `×N` on a shared body; the file writes each
  definition once ([File format](../io/file-format.md)).

## Body groups

A body definition may itself be a group of bodies, so a part is a tree of CSG without
history — the same fixed formula at every level.

```
BodyDef = Leaf  { id, solid, profile, source }   — a shape, in its own axes
        | Group { id, children: Vec<Body> }      — (∪ positive children) − (∪ negative children)
Body    = { id, def, sign, placement }           — a placed, signed occurrence
Part    = the root group (+ its axes, cut-list dimensions)
```

In the code: `BodyDef { group: Option<BodyGroup> }`, `BodyGroup { children, failed }`
(`BodyDef::group`, `children`, `is_group`), the group's `solid` its cached result.

Decision record: [Body definitions may be groups: a CSG tree without history](../decisions/0024-body-groups.md).

- **Where the B-rep lives**: a leaf has a **source** B-rep (or profile); a group has **only a
  cached** B-rep, computed from its children — never one of its own. A group holding a single
  positive child at the identity *is* that child (no boolean, as `Part::is_plain`).
- **A negative's reach is its group**: it cuts the positives of its own group only.
  Exploding a group (its children brought up one level, their placements composed) widens
  the reach of its negatives to the parent's. A negative group is a compound tool (a
  counterbore = two cylinders, a hinge drilling = cup ∪ screws); a negative inside a negative
  group spares material (`parent − (A − B)`).
- **No order between siblings**: only nesting carries meaning; adding a body to a group, in
  any order, gives the same result.
- **Definitions stay local to their part** (groups included): a group's copies in the part
  share it, a change shows in every one, `edit.make_unique` as for leaves; cycles are
  forbidden (a group cannot contain an occurrence of itself or of an ancestor); a union, a
  difference, a paste elsewhere give the target copies of the definitions, the whole tree
  (`copies_of_bodies`, `renamed_children`: every copy of a group holds bodies of its own ids).
  Parts pasted into a part come as groups too.
- **The model**: `Part::{all_bodies, children_of}`, `copies_of` and `is_failed` go through
  groups, `drop_unused_defs` through them (`used_defs`); the file refuses failures outside a
  group's children, or a group holding itself at any depth.
- **Computing**: a group is computed by one `Task::Recipe` (`Document::group_result`; a single
  positive child, unplaced, is the result: no kernel), its children's shapes cached in their
  definitions. A shared group is thus computed once for all its copies. An operation reaching
  inside a group computes it, then the groups above it, innermost first, then the part, in one
  job (the recorder's stand-ins chain them): O(depth) booleans, the siblings' results reused.
- **Failures** are reported by the body's path (group / … / body): a failed child is skipped
  (`BodyGroup::failed`), the others computed; a group whose result is empty or broken fails as
  a whole for its parent.
- **Grouping**: `Operation::GroupBodies` (the group's axes the part's, the body where the first
  one stood; negatives alone make a negative group of positive children — a compound
  machining; the part computed again only when the group mixes signs, or is now the part
  alone) and `Operation::UngroupBodies` (`Refusal::UngroupNegative` for a negative group
  holding a negative: it would add material; fresh ids for the children when the group is
  shared).
- **Stretch**: a group across a cutter has its children stretched in its axes
  (`stretched_def`, recursively), each one across a cutter given a definition of its own —
  inside a group, changes never go in place. **Why:** a definition shared with bodies outside
  it is not changed under them.
- Not yet: extracting a sub-group into a part of its own (« Extraire en pièce », the
  converse of the union).

### The interface

- « Grouper les corps » (`edit.group_bodies`, in Assembler › Composants), « Sortir du groupe »
  ([Moving bodies](#moving-bodies)); in an open part, « Créer un assemblage » (G) groups its
  bodies too. **« Éclater » (`edit.explode`) is one action whatever is selected**: it puts an
  assembly's content in its context and, in an open part, brings up the children of a group or a node.
- **The outliner shows the bodies of every part under it**, a single one too (folded, open
  while the part is), drawn by their nature — an extrusion (`PRISM`), a revolution (`TURN`), a
  free B-rep (`SOLID`), a group (`GROUP`) — and named by their definition (« Body names »
  below), in their order of creation, a group's children under it. Their sign is a toggle at
  the right end, where a part's row has its eye (`UNION` / `SUBTRACT`): a click flips it
  (`Operation::SetSign`, the part open or not; the selected siblings too when the body is one
  of them; the open part's scope, else every occurrence), the last positive one too, leaving
  the part (or group) empty (« Empty parts » above); the rows stay in place (no blanks first).
- A click on a row opens the part and selects the part's own body the row lies in
  (`Editor::toggle_body`); on a body in a group, it opens that group and selects the body
  itself, the groups along the open one unfolded.

### Editing inside a group

- A group opens as a part does, nested in it — `EntityPath::group` (the bodies leading to it,
  the part's own first; `in_group`, `part_path`), the editor's `open_group`
  (`Editor::open_group`), `open_part_path` carrying it. A double-click on a group body opens it
  (`Cx::group_body_at`); Échap, or a click off its box, goes back to its parent, the group
  selected; a group no longer there (undone, ungrouped) closes. The entity info shows the body
  selected in the group, as in the part.
- **Every operation on bodies takes it**: `Document::apply_in_group` puts the group's
  definition in the part's place as a part of its own (`Part::group_as_part`: its children
  the bodies, its result the shape, every definition of the part), applies the operation to
  it unchanged — in the group's axes —, gives the definition the result (every copy of the
  group shows it), computes the groups above it, innermost first, then the part, all in the
  operation's job; a group made to hold itself (pasted into itself) is refused
  (`Error::Cycle`); « Celui-ci » makes the part its own first, as for its bodies.
- `Document::resolve_container` gives what a path leads into — the part, or the group as a
  part, and its axes to world (`Part::group_chain`) —: the editor, the tools, the scene's
  recipe (`RecipeMeshes::within`, the group in the part's axes) and the entity info read it,
  so the tools draw, infer, move, stretch, erase, copy and edit profiles in the group's axes
  as in a part's.
- **Guides**: the part's guides stay the part's, in its axes (`Cx::part_in_context`, where new
  ones are drawn): while an operation runs in a group they are brought into its axes, then
  back, those it left alone put back exactly as they were (no drift); a copy made in a group
  takes its guides in the group's axes, as its bodies.

### Machinings inside groups

In an open part (or group), a positive group is drawn as its result and, over it, the
machinings inside it — at any depth through positive groups — as faint red ghosts
(`RecipeMeshes::nested`, each with the group of the open level it lies in: hidden with it
while it is carried, never picked — the click takes the group, a double-click opens it). A
negative group is one machining, drawn as its result: what it spares inside is not shown in
red. A [body node](#body-nodes) is drawn as its result too, nothing of its children over it:
they are its past, their faces no longer those of the result (a rounded rim), and show by
opening it. X-ray needs nothing more (the ghosts are see-through already).

## Body nodes

A **body node** is a group with an operation ([0042](../decisions/0042-body-nodes-operators.md)):
its children's result, `(∪ positives) − (∪ negatives)`, then rounded or chamfered
([Rounds and chamfers](../tools/rounds.md)). It is a body as any group is — a sign (a
machining rounded: a negative node of positive children), a definition its copies share, its
solid a cache, opened and edited as a group, named « Arrondi#n » / « Chanfrein#n » —; the
tree is its history: what is under it, its past; what is above, its future.

```
BodyGroup { children, failed, op: Option<NodeOp>, refused: Option<NodeRefusal> }
NodeOp    = Blend(BlendOp)                          (a shell to come)
BlendOp   { kind: Round | Chamfer, size, edges: Vec<NodeEdges { name, near }> }
```

`NodeOp` is an enum of the kinds of node (written `"op": {"blend": {"kind": "round", …}}`),
one yet: what is blend-specific asks `NodeOp::blend()`, what is not (the names it holds,
rewritten when bodies move under it: `tags_mut`) takes any. The intersection is no node but a
sign of bodies, the mirror no node but a placement (decided 2026-11-20).

- **Edges by name, in its children only.** A leaf's faces are named by its recipe — a
  profile's caps `Bottom` / `Top`, the face of a side by the side's id (`Corner::id`, stable
  while the profile is edited: [Prism tool and profiled blanks](../tools/prism.md)), a
  fillet's by its corner's: each face the side holding all its vertices and fitting its surface,
  a plane on a straight side, `names::leaf_faces` —, a free shape's by their record index; each name tagged with the
  path of bodies from the node down. The node's children are computed **from their leaves in
  one kernel session** (`Task::Node`, `crates/kernel/src/node.rs`): the tags go through the
  booleans as an application attribute of SolidCore's (a face cut keeps its tag, faces merged
  carry both). An edge is named by its two faces' tags and **matches by inclusion** (a merged
  face carries several; the edge is named by the tag of each face nearest the other's in the
  tree, of one body first: an edge of a board along a top merged with another's keeps its
  name when the other moves away). A name of several edges takes them all, unless only some were picked
  (`near`: the edges nearest those points). The faces a node makes are named by the two faces
  they lie between (`FaceName::Made`: of the edges taken, the one whose two faces run longest
  along it — its rails, not the faces its ends meet), a corner's patch where rounds meet by the
  faces around it, its rounds among them (`FaceName::Corner`, named after them): a node above
  names them as any other. A copy's children renamed rename these too.
- **Computed again** wherever a group is (`Document::computed_group`): an edit inside it, the
  groups above, welding, a stretch.
- **From its children's result, while it holds.** A node keeps, in memory only (not in the
  file, nor in its equality), its children's result before its operation and that result's
  face map, with the children it was computed for (`BodyGroup::raw`, `RawCache`: their tree and
  their leaves' solids, the same `Arc`s). Its children the same (its size or edges changed,
  the tool's preview of them), its task starts from that result (`TreeNode::Base`) and only
  the round is done again; cutters added since (a drill drawn in it), from that result cut by
  them alone; else (a body moved, removed, a positive added: a hole cannot be filled back)
  from its children. While an operation runs in a node or a group under one, the part standing
  for that group is not computed (`Document::deferred`): the node computes it afterwards.
  Measured (`bench_a_node_from_its_childrens_result_or_its_leaves`, debug, a board 600 × 300
  drilled 30 times, its front edge rounded): its size changed 100 ms against 210 ms (the node's
  task 41 ms against 150, the result's display mesh 60 ms either way); a drill added inside
  117 ms against 221 ms (194 against 300 while the part standing for the node was computed).
- **Names beside the caches.** A group's recipe names its inputs (`Task::Recipe`'s `names`: a
  leaf by its recipe, a group by its face map) and gives its result's **face map** — each
  record face's tags, relative to the group (`BodyGroup::faces`) —; a node gives its own. The
  map holds for the solid it was made for only (`faces_of`, a fingerprint of the record:
  `BodyDef::face_map`). A node's task takes a child group that has one **as it is**, its
  cache named by its map (`TreeNode::Mapped`), not computed again from its leaves; one without
  (a group whose solid came from elsewhere: a part pasted as a group, a group edited while no
  node is above it) from its leaves, as before. A group edited under a node is computed with
  its map. Measured (`bench_a_node_over_a_group_from_its_leaves_or_its_cache`, debug): a node
  over a board drilled 30 times, 99 ms from the cache against 202 ms from its leaves; naming the
  group's recipe costs nothing measurable (166 ms against 164 ms).
- **Copies**: a definition copied with its children renamed (`renamed_children`: made
  unique, pasted, split by a stretch) renames the first body of its names' and its map's paths
  with them.
- **Bodies moved in the tree under a node** follow its names: an operation in an opened group
  says which bodies it moved, by their paths from there (`Outcome::relocated`: grouped,
  ungrouped, wrapped by a node, a group made unique and its children renamed), and the nodes on
  the way — the group and those above it — rewrite the paths of their names and maps from
  them down (`ops::relocate`; the tags a face made by a node on the way is named by, from that
  node down). Grouping the bodies of a node leaves its edges as they were.
- **In error** when a name no longer matches an edge, or the kernel refuses its size after an
  edit below it (`refused`; a node computed from its leaves with its parent, having no face
  map of its own, has its refusal noted all the same: `Output::Node`'s `inner`,
  `ops::note_refused`): its children's result shown as it is, ⚠ in the navigator, the
  entity info saying why (`Part::refusal_of`). Applying or changing a node refused fails
  instead.
- **The root is never a node**: new bodies go to the part's root and cut (or join) the node's
  result.

## Flattening

« Aplatir », in a navigator row's menu, keeps the final solid as a plain B-rep and drops the
tree under it; « Éclater » does the converse, keeping the children and dropping the level.

- **`Operation::FlattenBodies { target, bodies, scope }`**, on a group or a node: it becomes a
  free shape of its result, with its name, sign and placement. Only the copies given: the
  others of a shared group keep their tree (a definition of their own for those flattened).
- **`Operation::FlattenPart { target, scope }`**, on a part with a tree: one positive free body,
  unplaced, its shape as it is. The materials of single faces, by their index, still hold.
- **Nothing computed**: the result is already there, the part's shape the same.
- **Faces' names kept** (`BodyDef::kept_faces`, for the solid of fingerprint `kept_faces_of`):
  a node above named its edges by the flattened group's children; the free shape answers with
  the same names through `face_map`, so the round stays on its edges. Once its solid changes
  (a stretch), the names no longer hold, as for any free shape.
- **Refused when failed or empty** (`Refusal::FlattenFailed`), at any depth: the result would
  freeze an error. **Not offered** on a plain body (a profile would only be lost) nor an
  assembly (it would merge separate parts).

## Moving bodies

A body (a leaf, a group, a node) goes from a level of its part to another — the order of the
history changed: a drill taken out of a round drills after it —, or out of the part
([0044](../decisions/0044-moving-bodies-in-the-tree.md)). `Operation::MoveBodies { from,
bodies, to, copy, scope }` (`crates/model/src/moves.rs`), `to` a level of the part
(`BodiesTo::Level`: the bodies leading to a group from the root, empty for the root),
`BodiesTo::NewPart(instances)` (a new part in that assembly) or `BodiesTo::Part(path)` (another
part, at its level `path.group`).

- **Its world place kept**: its placement computed again in the axes of where it goes.
- **Its id and definition kept** inside the part; the names of the nodes above both levels
  follow it (`[g1, b]` becomes `[g2, b]`). **A node it leaves loses the edges it took on it**:
  in error (⚠), said by a toast (`Warning::Moved`'s `in_error`); Undo takes it back.
- **A group shared by several copies in the part, left or entered, is made unique first**
  (its own definition, its children new ids, the names above following them;
  `Warning::Moved`'s `made_unique`); the other copies keep what they hold. The option bar's
  scope is the part's occurrences', as for any edit.
- **It keeps what it does**: its sign turned when the signs on its way differ — a positive
  child of a machining group (taking away) out at the root becomes negative; a drill dropped
  into a machining group (a group of drills) becomes one of its positive children, still
  drilling.
- **Into another part** (`BodiesTo::Part`): bodies pasted into it at that level
  (`PasteBodies`, new ids there), each keeping what it does, then taken away. Another
  occurrence of the same part: its level, as `Level`.
- **Parts into a part** (`Operation::MoveParts { instances, occurrences, to, copy }`): as parts
  pasted into a part (`PasteParts`: one positive body each, a group of its bodies unless it is
  one; negative where the level takes away), the occurrences then taken away; a part into
  itself, or into a part an assembly moved holds, refused (`Error::Cycle`).
- **Out of the part** (`BodiesTo::NewPart`): a new part in the assembly given (its own context
  for « Sortir de la pièce », the model or an assembly dropped on), as bodies pasted out of a
  part ([Copy, cut and paste](../tools/copy-paste.md#across-the-parts-boundary):
  signs kept, all positive with no positive among them — a drill becomes a pin), then the
  bodies taken from the part.
- `copy`: a new body of the same definition, the first one left where it was.
- **« Sortir du groupe »** (`edit.move_out`, Assembler › Composants, after « Grouper les corps »): the
  bodies selected one level up — out of the group or node open into its parent; at the part's
  root, out of the part —; that level then open, what moved selected (`Editor::move_out`).
- **The navigator's drag and drop** (`panels::BodiesDrag`, its part open or not): a body's
  row dragged — the selected siblings with it when it is selected —, dropped on a group's or
  node's row (its middle half) goes into it; on its edges, or on a leaf's row, beside it, in
  its level (a line above or under the row); on its part's row, to its root (the row framed).
  Dropped on another part's row (its body's or group's), into that part at that level; on an
  assembly's row or the model's, a new part there. A part's or assembly's row dragged (its
  selected siblings with it) and dropped on a part's row (or one of its bodies' or groups'),
  into that part as bodies. Where it may not go (where it lies, into itself, its own part),
  the cursor says so. Option
  (Ctrl elsewhere) held at the drop copies. The part's scope when it is open, else every
  occurrence. A body row's menu (right click): « Sortir du groupe » (« Sortir de la pièce » at
  its root), the row's body with its selected siblings.
- **Occurrences between assemblies** (`Operation::MoveOccurrences { instances, occurrences, to,
  copy }`): a part's or assembly's row dropped on an assembly's row, or the model's, goes into
  it, its world place kept, its id too; that assembly's definition changes, every copy of it.
  Into itself, or into an assembly it holds, refused (`Error::Cycle`).

## Body names

A body definition takes a name given by the user, as a part's.

- **On the definition** (`BodyDef::name` + `named`, as `Component`'s), not the body: every
  copy shows it, as every occurrence of a part shows the part's name. Not given: automatic
  (« Automatic names » below), the icon its nature's either way.
- **A body's own name**, as an occurrence's: `Body::name`, empty for none, « Haut <Trou
  charnière#1> » in the navigator (`Operation::RenameBody { part, body, name }`, a group's
  child too; trimmed, empty to clear). Kept by its copies (as an occurrence's), a body made
  unique, a group dissolved (its children's). Across a part's boundary it is the
  occurrence's: a part pasted into a part or a boolean's tool gives its occurrence's name to
  the body it becomes (a plain part with none, its body's); a single body pasted out as a part
  gives its own to the new occurrence. Edited as an occurrence's: a double-click on it in the
  row (F2: where it has one), the entity info's « Exemplaire ».
- **Not unique** when given: unlike parts' names, they never reach the cut list; only for
  finding one's way. No scope either: the name is shared by every occurrence of the part, the
  shape left alone (no kernel).
- **`Operation::RenameBodyDef { part, def, name }`**, trimmed, empty to clear (« Renommer » in
  the history).
- **Kept** wherever the definition goes: its shape changed (stretch, profile, a boolean's
  result, welding), its copies (copied, arrayed, made unique, pasted), a group exploded (the
  children's definitions are the same).
- **Across a part's boundary**: a part the user named (`Component::named`) becoming a body —
  pasted into a part (`Document::parts_as_bodies`), the tool of a union or a difference
  (`tool_as_body`) — names that body's definition, unless it has a name; the other way, bodies
  pasted out as a new part (`PasteAsPart`) name it after their single body's definition, if
  named (made unique as any part's name).
- **Interface**: the navigator shows the name; a double-click on a body row, or F2 while it is
  the one body selected, edits it in place (Enter or a click away names it, emptied it clears,
  Esc keeps it). The entity info's « Nom » of a body or group is a field (emptied: cleared).
  A part's or an assembly's row the same: its component's name edited in place
  (`RenameComponent`; emptied, it is kept; one taken, refused and told) — or, where the
  occurrence has a name of its own (« Instance <Composant> », the entity info's
  « Occurrence »), that name when the double-click is on it, or on F2 (`RenameOccurrence`;
  emptied, cleared), alone in the field, the row's whole width.
- **No naming at creation**: « Nommage des pièces » ([Quick tools after a creation, naming what was just made](../tools/quick-tools.md)) stays for parts; a body
  added to an open part is not asked its name. A group made, or a body made unique, is offered
  its name by a toast ([Naming what was just made](../tools/quick-tools.md#naming-what-was-just-made)).

## Automatic names

One rule for components and body definitions.

Decision record: [Automatic names are stored, numbered from #1](../decisions/0030-stored-automatic-names.md).

- **Stored, numbered from #1** (SketchUp's format): a definition not named by the user is
  named by its base and a number, stable, shared by its copies — a part « Pièce#2 », a body
  definition « Extrusion#2 ». Components unique in the document
  ([Components](data-model.md#components-parts-and-assemblies)), body definitions in their part.
- **Per nature** (`BodyDef::nature`, `Nature::base`: `default_name.extrusion`, `revolution`,
  `solid`, `group`): « Extrusion#1 », « Révolution#1 », « Solide#1 », « Groupe#1 » in one part.
- **Settled after each operation** (`Document::settle_names`, the parts it changed; every part
  of a file read or imported): `Part::settle_names` names anew, from the first number free,
  a definition with no name yet, one whose nature changed (an extrusion stretched askew is a
  free solid now: « Solide#1 »), one whose name an earlier definition holds (a copy made
  unique, bodies pasted from another part). A name given is never touched.
- **Shown apart**: in the navigator an automatic name is in italics, attenuated — as far
  towards the background as the weak text, from the row's colour (a hidden row's paler still,
  the italics telling it there too); a name given as any text. Parts and assemblies the same
  (their component's name only: « Gauche <Pièce#1> »).

## Edit levels

There is no « show the bodies » switch: a double-click goes one level down, as it enters an
assembly, Esc (or a click off the level's grey box) one up:
- **the part open shows its bodies**, always: blanks as ghosts, machinings as red ghosts, all
  picked and selected ([The interface of bodies](#the-interface-of-bodies)). The shape alone is
  seen once the part is closed;
- **a group of bodies opens** inside it ([Editing inside a group](#editing-inside-a-group));
- **a body with a profile opens**: its profile edited (`Command::EditProfile`, the Profile
  tool on it). It is a level: the grey box is the body's (`Editor::context_bounds` →
  `body_bounds`), Esc at the tool's first step, `Editor::exit` or a click off the box leave it
  for its part (or group), the body selected (`Editor::leave_body`); another tool chosen
  leaves it too. The part's other bodies are faded, as what lies outside any context
  (`ViewPlacement::faded`), their bottom caps not drawn. A body without a profile does not
  open. No shortcut for a part of one body: two double-clicks reach its profile, as for any
  part;
- **the navigator stays as it is**: a double-click on a row renames it, its « Profil » row
  opens the body.

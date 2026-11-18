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
  kernel, the profiles never touched. Not yet: an operation changing them.

## A fixed formula, no history

- The union of the positives minus the union of the negatives. **Every negative cuts every
  positive of its group** — the part's, at the top level ([Body groups](#body-groups)) —,
  those added later included; the order bodies were made in does not matter. A hole is filled
  by deleting its negative, not by adding a positive into it.
- **Incremental computation** (`Document::recompute`): adding negatives = subtractions on the
  cached final; adding positives `P` = `final ∪ (P − ∪N)` (since
  `(A ∪ B) − N = (A − N) ∪ (B − N)`, the recipe's `base`); a part with a single positive, the
  common case, makes no union at all. Removing or moving a body computes the part again from
  its bodies.
- **Failures**: a body whose boolean fails is skipped and reported by its id, the others still
  computed (`Part::failed`; a group's children in `BodyGroup::failed`).

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
  en échec : à corriger »).
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
occurrence leaves the model, under the scope ([Booleans across contexts](kernel.md#booleans-across-contexts)).
A tool that is its single positive body, unplaced (a shape drawn), comes as a copy of that
body, its profile with it — still editable; any other comes as a group of copies of its bodies
(`tool_as_body`, its shape the group's: no more kernel work).

- **Union**: the tool part's root comes **as a positive sub-group** of the target, its
  structure kept, its negatives reaching that sub-group only; a through-hole meant to drill
  the target too is had by flattening the group (« Dégrouper »).
- **Difference**: the tool part's root comes **as a negative sub-group** (its bodies kept,
  still editable).
- **Intersection**: a new part with one positive leaf, a free shape (the common volume).
  **No ∩ node**, nor any ordered operation. **Why:** that would turn the tree into a feature
  history (persistent naming).

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
  Flattening a group (its children brought up one level, their placements composed) widens
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
- Not yet: freezing a group into a shape (« Convertir en forme libre », the group becoming a
  free leaf); extracting a sub-group into a part of its own (« Extraire en pièce », the
  converse of the union).

### The interface

- « Grouper les corps » / « Dégrouper » (`edit.group_bodies`, `edit.ungroup_bodies`, in
  Assembler › Composants); in an open part, « Créer un assemblage » (G) and « Éclater » do the
  same on its bodies.
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
red. X-ray needs nothing more (the ghosts are see-through already).

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
  result, welding), its copies (copied, arrayed, made unique, pasted), a group flattened (the
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
- **No naming at creation**: « Nommage des pièces » ([Quick tools after a creation, naming a part just made](../tools/quick-tools.md)) stays for parts; a body
  added to an open part is not asked its name.

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

# Body nodes: an operation applied to the children, the part's tree as its history

- Status: accepted (2026-11-19; not built yet)
- Design: [Bodies](../model/bodies.md#body-groups), [Opening a part](../model/opening-a-part.md)
  — to be described there as the steps are built
- Amends: [Body definitions may be groups](0024-body-groups.md) (its « no ∩ node, nor any
  ordered operation »)

## Context

Discussed and decided on 2026-11-19 (questions / answers), after a headless trial of the naming
(« Trial » below).

Fillets and chamfers on solids: SolidCore builds them (`solidcore_ops::blend`: round,
curvature-continuous, chord, chamfer at equal distances or by sides, variable radius; corners set
back, filled or open, mitred or swept; a refusal names the largest size that builds,
`Error::TooLarge`), the record keeps a round by its definition (`Surface::Blend`), and edges are
taken by their record index, which the display mesh already gives per polyline
(`Mesh::edge_ids`). What is missing is a place for them in the part.

A part is computed from its bodies by a fixed formula, `(∪ positives) − (∪ negatives)`, nested
in groups ([0024](0024-body-groups.md)): its final B-rep is a cache, computed again whenever a
body changes. A fillet applied to that final would be lost at the next computation. 0024 rejected
« an ∩ node, or any ordered operation » because it would turn the tree into a feature history
(persistent naming).

**What actually needs naming** is not the order but the kind of operation:
- operations on **whole solids** (∪, −, ∩, transforms, mirror) name nothing inside their
  operands: « this body minus that one » computes again whatever happens to them — the fixed
  formula needs no naming for that reason, and an ∩ node would need none either;
- operations on **pieces of a result** (round *these edges*, shell *through this face*) store a
  reference into a computed solid, and that reference breaks when the solid changes.

A fillet is of the second kind, in a tree as in a timeline. A timeline (Fusion, SolidWorks,
Onshape) does not make naming easier: every feature there depends on all those before it, and
may point at entities of any of them.

## Decision

### The model

- **A body definition is a leaf or a node**: a node is an operation applied to its children's
  results. The group of 0024 is the plain node `∪/−` (its fixed formula, unordered children);
  a body node is **a group with an operation**: its children's `∪/−` result, then rounded or
  chamfered (later: intersected, shelled, mirrored…). Decided 2026-11-19 (step 1's Q&A) over a
  `BodyNode` replacing `BodyGroup`: no refactor, and all that holds for groups (opening, the
  navigator, stretch, the deepest computed first) holds for nodes.

  ```
  BodyGroup { children, failed, op: Option<NodeOp>, refused: Option<NodeRefusal> }
  NodeOp    { blend: Round | Chamfer, size, edges: Vec<NodeEdges { name, near }> }
  ```

  ```
  Plateau (part)                       root: ∪/−, always a Group
  ├─ Round r = 5  (+)                  [edges: top × sides 1..4]
  │   └─ Group
  │       ├─ Blank (+)
  │       └─ Pocket (−)                ← inside: its rim may be rounded
  └─ Hole ø8 (−)                       ← outside: it drills after the round
  ```

- **A node is a body like any other**: a sign (a rounded negative is a pocket with round
  corners), a definition shared by its copies, a cached solid, a failure mark, a name.
- **The tree is the history.** A node's descendants are its past (what it is made of), its
  ancestors its future (the operations applied to it afterwards), its siblings neither. There is
  no hidden order: the order is the tree's structure, shown in the navigator and changed by
  moving a body into or out of a node.

### Names scoped to the child

A node that takes pieces names them **in its children's result only**, never elsewhere (no
reference across branches — in the spirit of [0007](0007-procedural-parts.md)'s « anchor on the
blank only »). Two levels:

1. **Over a leaf** (box, profile, turned body): an edge is named by the two faces of the recipe
   it lies between (caps, side *i* of the profile, …), stable under every edit of the recipe.
   Needs **stable ids on the profile's sides** (« Trial » below): `profile::Corner` has none,
   its sides are named by their position, so inserting a corner shifts them; a fillet's face is
   named by the two sides it joins.
2. **Over a group**: the leaves' faces are tagged (body, face name) by an application
   attribute of SolidCore's kernel with our own handlers (a face cut in two hands its tag to
   both pieces, two merged join theirs, a face found lying on another joins its tag to the
   survivor's), carried through the booleans of the session computing the group. An edge of the
   result is named by the pair of tags of its faces, {(body a, face i), (body b, face j)}. Such
   a name survives moving or resizing a body of the group.

- **A name matches by inclusion**: an edge whose two faces *carry* the two tags, a merged face
  carrying several (two boards flush: `a:+Z|b:+Z`) — by equality, adding a body flush with
  another would rename its edges.
- **A name of several edges** (a hole cutting a rounded edge in two): **all of them** are taken
  — the intent « the edge between the top and that side » does not change because a hole cuts
  it. When the user picked only some of a name's edges, the node keeps a geometric hint (a point
  per edge picked) and takes the nearest ones only.
- **The faces a node makes** are tagged by the node, after the picked pair of faces they lie
  between (`round(board:+X/board:+Y)`, from `Blended::made` and their neighbours): a node above
  names them as any other (a round over a round).
- **Names live beside the caches**: the record carries no attributes, so each group's cached
  result keeps a **face map** (record face → tags), written in the file with the caches; the
  incremental computation goes on from the cache, and opening a file computes nothing again.

**A name that no longer resolves** (its pair gone) makes the node fail **visibly**: ⚠ on the
node, its child passed through unchanged, as a failed machining is today — never applied to
another edge silently.

### Rules that keep it simple

- **The root stays a group** with its fixed formula, and new bodies go there by default:
  drawing a hole after a round drills the round.
- **« Arrondir » / « Chanfreiner » wrap their child**: the body the edges picked belong to, or
  the smallest group holding them all; rounding edges of the final wraps the root's content (as
  the union already groups a target's bodies, `Document::group_machined`).
- **Opening a node is going back in time**: double-click opens it as a group is opened today; the
  child is shown **raw** (the operation not applied), the referenced edges highlighted, those
  that no longer resolve marked ⚠; not its result (amended 2026-11-19: a ghost of it over the
  children read badly), the part's other bodies around it faded instead. This is where a lost reference
  is repaired (picking its edges again).
- **A body drawn in an open node goes into its child**, as in an open group: a child that was
  a single body becomes a group of it and the new one; the node is computed again with it (a
  new pocket's rim may then be rounded).
- **Stretch goes down into the children** and the node is applied again — a stretch across a
  round no longer stretches the round's surface.
- **A size too large is refused, never reduced**: while the tool runs, the VCB refuses it and
  says the largest that builds (SolidCore's `Error::TooLarge`: « Rayon max 4,2 mm »); after an
  edit below the node (a part made thinner), the node fails (⚠) with that largest size, its
  child passed through. The user's value is never changed.

### The interface

- **A node is a row of its own in the navigator**, as a group: the operation's icon, an
  automatic name (« Arrondi#1 », [0030](0030-stored-automatic-names.md)), its size (« r 5 »),
  its child unfolded under it; its sign toggled as a body's.
- **Its parameters**: the size and the kind (round, chamfer) edited in the entity info, as a
  body's profile; the edges by running the tool again on the node, its edges picked already —
  from the node opened, or a button in the entity info.

## Alternatives considered

- **A fillet baked into the final, the part flattened to one free shape**: trivial, any edge,
  but the bodies are lost (machinings no longer editable). At most an explicit command.
- **A fillet baked into a body's definition** (the body becomes a free shape): only that body's
  own edges, nothing of a machining's rim nor between positives, and no edit of the recipe after.
  The node over a leaf is this, kept editable.
- **A fillet as a machining** (a negative « router bit » body along a straight or circular
  edge, a positive filler in an inner corner): no naming, fits the formula, but exact on lines
  and circles only, corners mitred, never a ball, and a negative cuts every positive of its group.
  Rather a future machining type ([0007](0007-procedural-parts.md)).
- **A timeline** (a linear feature history with a rollback bar): the same naming problem,
  worse bounded — each feature depends on all those before it, references may cross the whole
  history, rolling back hides the whole part.
- **Keeping the fixed formula only** (0024 as it stands): no fillets on solids beyond the
  profiles' 2D fillets (« Boîte »'s « Congé », « Prisme »'s corners) and turned profiles.

## Consequences

- **0024's « no ∩ node, nor any ordered operation » is amended**: nodes on whole solids
  (∩, mirror) need no naming and become possible at once; nodes on pieces need names scoped to
  their child.
- **Locality**: an edit inside a node can invalidate its ancestors only (the path to the root),
  never a sibling branch; a node is computed again only when its subtree changes.
- **Incremental computation** stops at the first non-group node on the path (adding a negative
  to a group under a round computes the round again). A node takes its child groups from their
  caches and face maps (built 2026-11-19, step 2): a node over a board drilled 30 times, 99 ms
  against 202 ms from its leaves (debug).
- **Booleans on round faces** are the kernel's least proven ground: a battery of tests in
  `crates/kernel/tests/solidcore_quirks.rs` before relying on them (round then cut, round then
  stretch, a part with rounds in a union).
- **File format**: `BodyGroup` gains `op` and `refused`; a profile's corners their `id` (v1
  until the first release, [0010](0010-file-format-v1-until-release.md): no compatibility; ids
  settled when a profile is read).
- **SolidCore's opt-in fixes all on** (2026-11-19, `engine::kernel()` and
  `solidcore_ops::session::set_fixes`), as SolidCraft runs: without them a round too large
  ended as « not a closed solid » rather than the refusal naming the size that fits. The
  workspace's tests pass as before; the STEP references (Cruise, Ender3) import the same.
- **New in the tools**: an edge selection (`edge_ids` is read only by the kernel today), with
  the tangent chain (`solidcore_ops::tangent_chain`) on a double-click.
- Possible order:
  1. stable ids on the profile's sides; the node `Round` / `Chamfer` over a leaf, named by its
     recipe's faces;
  2. names through the booleans (tried headless, « Trial » below) and the face map beside the
     caches, then the node over a group;
  3. the nodes on whole solids (∩, mirror), and others (shell) once the naming has proved
     itself.

## Trial (2026-11-19, headless)

`crates/kernel/src/provenance_trial.rs`: the leaves' faces tagged `body:face` with an
application attribute of SolidCore's kernel (`Attribute::application`, our own handlers: a split
keeps the tag, a merge and a coincidence join the tags), the booleans run in one session as
`recipe_session` runs them, the tags read back on the result's faces. `solidcore_ops::provenance`
itself is of no use to us yet: its `attach` / `read` are crate-private.

- **Every face of every result is tagged** (board − pocket, pocket breaking out, hole across an
  edge, an L of two boards, two boards side by side).
- **A pocket moved** keeps every edge name: the rim is `board:+Z × pocket:±X/±Y` at both places.
- **A pocket breaking out of the side** loses exactly the rim edge on that side
  (`board:+Z × pocket:+X` no longer resolves) and new pairs appear (`board:+X × pocket:…`).
- **A hole across an edge** cuts `board:+X × board:+Z` in two edges of one name, and the board's
  `+X` face in two faces of one tag: the rule for a name of several edges is needed.
- **A union merges coplanar faces**, the merged face carrying both tags
  (`board:+X|upright:+X`): **a name must match by inclusion** (an edge whose faces carry the two
  tags), not by equality, or adding a body flush with another renames its edges. The L's inner
  corner is `board:+Z × upright:-X`.
- **A round over the group** (`solidcore_ops::blend`, the edges picked by name: the top's four
  outer edges at 5 mm, or the pocket's rim at 2 mm, its corners mitred) keeps the tags of the
  faces it trims; the faces it makes are untagged — `Blended::made` lists them, the node tags
  them itself (« round of edge N »). 4 edges: 52–59 ms (debug).
- **A profiled leaf edited** (an L outline, its inner corner filleted, minus a hole; then its
  corner c chamfered): with **ids on the profile's sides** (not its corners: a chamfer inserts a
  side, its neighbours keep theirs), 23 names kept, one gone (`side:s1 × side:s2`, the vertical
  edge the chamfer removed), 4 new (the chamfer's), **none moved**. With sides named by their
  position, as `profile::Corner` allows today: 8 kept, 6 gone and **10 moved** — the same name
  on another edge, a node would round it silently. Stable ids belong to the sides; a fillet's
  face is named by its two sides. The extrusion makes a fillet's face a rational spline, not a
  cylinder: a leaf's names must come from its construction (its sides' order), not from its
  surfaces' kinds.
- **A round over a round**: r1 rounds the board's four upright edges (its faces tagged by
  their two faces, `r1(board:+X/board:+Y)`), r2 rounds the top's outer loop over them — its
  names take r1's faces (`board:+Z × r1(…)`): 8 edges picked (4 straight, 4 arcs), the tangent
  chain built, every face of the result named (23, none untagged). The pocket moved and r1 at
  12 instead of 10: both nodes pick the same edges by the same names, the result's names
  unchanged. r2: 365 ms for 8 edges (debug).
- **Tags do not survive the record**: `solidcore-record` has no attributes. A node's names are
  computed in the session that computes its children, from the leaves; a group's cached result
  read from its record carries none — the incremental computation (adding a negative to the
  cached final) has to keep a face map (record face → tags) beside the cache, or compute the
  group again from its leaves when a node above needs names — the face map chosen.


# Linked bodies: a part imprints its final solid in the parts it enters

- Status: accepted (2026-11-23; steps 1 to 4 built; the tool amended below prevails over the
  imprint body and the run back described first)
- Design: [Bodies](../model/bodies.md), [Data model](../model/data-model.md)
- Builds on: [Parts, assemblies, occurrences](0005-parts-assemblies-occurrences.md),
  [Body groups](0024-body-groups.md), [Body nodes](0042-body-nodes-operators.md); takes up the
  linked machinings left open by [Procedural parts](0007-procedural-parts.md)

## Context

Decided on 2026-11-23 (questions / answers), after a headless trial (« Trial » below).

A part often shapes another: a tenon and its mortise, a dowel and its hole, a hinge and its
drillings, a shelf and its housing. Today the machining is a body of its own, placed by hand: it
stays behind when the part that caused it moves. Two wishes, for the tenon:

1. the mortise follows the part carrying the tenon;
2. that part makes its mortise, which **is not the tenon's shape**: deeper, with a clearance. A
   plain link to the same solid cannot say so.

The system must not be about tenons. And copies raise the hard case: four legs sharing one
definition each receive two rails. The rails cannot all be moved at once, since one world vector
is a different move in each turned or mirrored leg, yet the four legs must be able to stay
identical at the end.

## Decision

### An imprint, a linked body, a link

Three separate questions: **what** is imprinted, **where**, and **what it does** there.

```
Sign       = Positive | Negative | Intersect | Imprint
BodySource = Local | Linked
Assembly   { occurrences, links: Vec<Link> }
Link       { source: path, imprint: Id, receiver: path, body: Id, seat: T }
```

```
Rail (part A)
├─ Blank (+)
├─ Tenon (+)                     definition T
└─ Mortise imprint (imprint)     no effect on A, nor on its cut list
    └─ Clearance: end +2, around +0.1
        └─ Tenon                 the same definition T: it follows

Leg (part B)
├─ Blank (+)
└─ Mortise (−)                   linked body

Assembly
└─ Link: rail occurrence → leg occurrence
```

- **What: an imprint**, a body of the source part with a fourth sign. It takes no part in its
  part's formula and is shown as a ghost. It may be any shape: the tenon with a clearance, a
  hinge's drillings, a free solid. **The clearance belongs to the source**: it is the joint's,
  and serves all its occurrences.
- **What it does: the linked body's sign**, in the receiving part: `−` a mortise, but `+` and
  `∩` work as for any body. Its solid is a cache of the imprint's.
- **Where: a link**, held by the **lowest assembly holding both occurrences**, by paths relative
  to it. Copying a cabinet copies its links, and both copies stay right.
- **One linked body, several links**: the four legs show one mortise, in their definition, and
  each rail end has its own link to it.
- **`seat` is kept per link**: the imprint's pose in the receiver's axes when it sits in its
  body. **Why:** read back from the shared mortise, a mirrored leg would give a mirrored rail.

### Following: one definition, seats visibly off

A shared receiver is **never split on its own**. Divergence is a visible state of the source.

- **A seated source that moves drives its linked body**, at once, in every copy of the receiver
  (the definition is shared, as for any edit under « all occurrences »). Every seat of that
  body moves by the same change, in the receiver's axes.
- **Nothing else moves.** The other sources are left where they are and show ⚠ (off their
  body): an edit changes only what it was applied to. A toast offers « Replacer » and
  « Séparer ».
- **« Replacer »** puts each source off back on its seats, each computed in its own receiver's
  axes: this is the simultaneous move that turned and mirrored legs make impossible by hand.
  A source whose seats disagree (a rail whose two legs no longer match) is **not moved**, stays
  ⚠, and the gap is told.
- **A source off drives nothing**: placing it by hand, even approximately, leaves the body under
  the source still seated. It is seated again as soon as it lands exactly.
- **One body asked two different moves does not move.** A rail slid along its length between two
  mirrored legs asks their one mortise +5 and −5: the rail stays where it was put, ⚠. Selecting
  several rails and moving them by one vector can do the same.
- **An imprint leaving its receiver leaves the body where it is**: pulling a rail aside does not
  empty the mortises of four legs. « Amener l'usinage ici » moves it explicitly.
- **« Séparer »** is the explicit way to diverge: the receiver becomes a part of its own
  (`MakeUnique`), its body then following alone.
- **Only a source drives.** A receiver that moves keeps its linked body, which goes with it; the
  sources seated in it show ⚠ (decided 2026-11-23, not tried).

### The tool runs past the shoulder

The body cut in the receiver is the imprint **run back past the face where it joins its part**,
within the source's own material.

**Why:** with the tool ending on the shoulder, pushing a rail in by 5 mm moved the mortise off
the leg's face: a closed cavity behind a 5 mm skin. Run back, it stays open. And since that
length lies in the source's material, it cuts nothing a correctly placed source does not
already occupy. A tool that does not end on the receiver's face is also not coincident with it.

**By default it runs back by the imprint's own length**, from **the faces where the imprinted
body touches the rest of its part** (decided 2026-11-23; finding those faces not tried).

### Clearance

Tried from the cheapest: **by the profile** for an extrusion (`profile::offset` and the height);
**on a node's child**, the node then computed again, for a rounded or chamfered end; else
SolidCore's **`offset_faces`** on a free shape. A clearance refused is shown (⚠), never reduced.

### Collisions are told

A source and receiver sharing material are flagged ⚠, nothing moved. The check runs in the
background after a move, on the links it touched: it must not hold the interface.

Amended on 2026-11-23: since a source cuts its whole solid, it can no longer collide with its own
receiver. What stays told is **two parts seated in one part that share material**, read on their
final solids so that a joint each has cut in the other is no overlap: ⚠ on both rows, and a toast
when an operation makes it. Nothing is moved or refused.

### Creating and breaking

- **« Imprimer »**: from the part carrying the imprints, applied to the parts it enters, after a
  confirmation.
- **A broken link** (source deleted, part copied out of its assembly) leaves the linked body in
  place, **frozen as a local body, ⚠**. A machining is never deleted by losing its link: a toast
  offers « Retirer » or « Garder », and nothing happens before a click (amended 2026-11-23, after
  trying it: a notification alone left the user nothing to do).

### One pass, no cycle

**An imprint may not contain a linked body.** A linked body therefore depends on imprints only,
which depend on nothing incoming: A may imprint B and B imprint A, and one pass updates
everything, with no dependency graph. It runs in the operation's job, after welding: one step of
undo.

### Amended: the tool is the source's whole final solid

Decided on 2026-11-23, after trying it on real joints. **This section prevails** wherever the
others speak of an imprint body, its sign, its run back, or of collisions between a source and its
receiver. Two versions came before:

- an imprint made of the source's **positive bodies**: a tenon left by a machining is no body, and
  on a notched upright astride a plank, the plank lost twice what it should;
- the **contact** of the final solid, run back past its openings by an estimate: a tool 400 mm
  tall for a 172 mm plank, held by the source as a body, which Stretch then moved inside the rail
  (507 mm away from its mortise, in the user's model). And it still assumed a tenon, where a part
  may be its own.

What holds now:

- **The source carries nothing.** `Sign` keeps its three values, and
  `Link { id, source, receiver, body, seat }`, `seat` being the source's axes in the receiver's.
- **The tool is the source's whole final solid**, in its axes, held by the linked body. What lies
  outside the receiver cuts nothing, so nothing has to be run back.
- **Only its walls inside the receiver move**: the one facing an opening is the bottom (`deep`),
  the others the cheeks (`around`). **The clearance belongs to the machining.**
- **A source pushed further bites further, whatever its shape** (the user's decision): its
  shoulder cuts too, with no alert. The tool stays rigid on a move, so what newly enters has no
  clearance until the next change of shape.
- **Stretch never touches a linked body.** It is built again from its source, at the source's new
  place, its seats with it.
- **A linked body has one source part**, whose shape it takes and whose name it bears; identical
  parts get a body each. **It is not edited**: one acts on its source, or breaks the link. The
  navigator says so, on both sides.
- **Each side is read without its linked bodies**, so the rule « no cycle » still holds, and **the
  kernel says which parts share material**, before the confirmation.

### Amended: « Empreindre » is pointed, copies included

Decided on 2026-11-24, on a frame of one rail placed four times, turned by quarters, each end
lapped into the next. « Imprimer » searched itself for the parts the selection entered, and left
out copies of one part: the frame could not be linked at all.

- **Renamed « Empreindre »**: « Imprimer » read as printing on paper.
- **The user points the sources, then the parts they go into**: a tool picking as the solid
  tools do, several sources at once (the dowels of a shelf). **Why:** the material two copies
  share is the same both ways, and a rail is a tenon in one neighbour and a housing for the
  other; nothing in the geometry tells them apart, and both ways at once cut the lap away as well
  as the housing. Pointing says the way, and asks the kernel about the pairs pointed only.
- **Every pair of copies placed as a pair pointed is comes along**, so a table is two clicks, not
  eight, and the confirmation says how many came so. A way that is its own inverse is made once
  per two copies.
- **A part may imprint its copies**, never itself. The linked body lies in the source's own
  definition, read without its linked bodies, so one pass still updates everything.
- **One copy moved alone still drives the body** (the user's decision): in a closed cycle the
  housing then moves inside the moved copy too. « Replacer » places the others in a chain, each
  once the copy it enters has its place: a move in the frame's plane closes round again. An
  offset that adds up round the cycle (one rail raised) has no place for all: it moves those it
  seats and tells the rest. Moving every copy at once stays seated.
- Alternatives considered: the selection's order giving the way, the search kept (tried, but one
  selection could not say which parts are sources); choosing the way in the confirmation, heavier
  to draw; leaving copies out, which keeps identical frames from being linked.

## Alternatives considered

- **Linking the mortise to the tenon, adjustments on the link**: the depth set again for each
  mortise, and no way for a hinge to carry its own drillings.
- **Splitting a shared receiver when it diverges** (OpenCutList's rule, kept by Stretch): rejected
  by the user, the four legs would never become one again once all the rails have moved.
- **Splitting, then merging back when they match again**: fragile; any edit made in between
  keeps them apart, and the cut list changes meanwhile.
- **Moving the other sources automatically**: it guesses at the user's intent.
- **Updating mortises on a click** (« Mettre à jour »): no intermediate state, but mortises stay
  wrong until someone thinks of it. Immediate following was chosen.
- **Constraints and a solver between parts**: another modelling paradigm, against direct
  modelling.
- **Joint types** (tenon, dowel, hinge…) as the base: too narrow. They can come later as
  generators of imprints.
- **Keeping the contact, run back past its openings**: it kept a pushed source's shoulder out of
  the receiver, at the price of a tool of a guessed length, a body in the source for Stretch to
  move, and a tenon assumed.

## Trial (2026-11-23, headless)

Two headless tests, never committed: they prototyped the links over the public operations
before the model had them. The cases that still hold now run in `crates/model/src/link_tests.rs`
and `crates/kernel/tests/imprint.rs`. The trial's table: four legs and four rails with two
tenons each, the legs turned by quarters or mirrored.

- **14 scenarios pass**: following, « Replacer », a source off, pulled out, two moves asked of one
  body, a leg separated, a rail whose ends disagree (its 10 mm gap told), an assembly copied.
- **The tool past the shoulder**: a rail pushed in 5 mm collided by 11 000 mm³ with a skin left;
  run back 30 mm, the mortise stays open and only the shoulder collides, 7 419.8 mm³, the exact
  value. Pushed further than that length, the mortise closes again: the collision tells it.
- **Clearance**: the profile gives the exact volume in 4 ms, against seconds for `offset_faces`
  on the same filleted prism. `offset_faces` was exact on boxes, dowels and blended ends, but
  refused a rounded end with its sides moved and a filleted tool run back past its shoulder
  (« no certified bound »): both pass by the profile or the node's child.
- **Cost** (test profile, a machine under load 20 to 35: orders of magnitude only): following a
  rail, 5 to 20 ms on a plain leg, 0.4 to 2 s on a leg with 30 more machinings; the collision
  check of eight seats, 55 to 80 ms, then 1 to 6 s. Limiting it to the bodies near the source
  halved it in one run (637 ms against 1 389 ms), the volumes identical.

**Not tried**: editing the imprint's shape, Stretch on either part, welding near a linked body,
the file, undo, the whole update as one job, nested assemblies, a body in a group as source or
receiver, moving the receiver, finding the shoulder face automatically.

## Consequences

- `BodySource` gets its `Linked` case, and assemblies their links. No format version bump
  ([0010](0010-file-format-v1-until-release.md)).
- **Moving an occurrence may now need the kernel**, which it never did: only when it is the source
  of a link, computed on release, a preview while dragging. Moving a machining recomputes its part
  from its bodies, which is what makes a heavily machined leg slow.
- Welding must not move a linked body (its place is the link's); it may stay a reference.
- Possible order:
  1. the model: the sign, the linked source, links, the file (built 2026-11-23);
  2. following, seats off, in one job with its undo, links kept through changes of structure
     (built 2026-11-23; the imprint's shape does not follow yet, its links go off);
  3. « Replacer », « Séparer », « Amener l'usinage ici », broken links, ⚠ in the navigator
     (built 2026-11-23);
  4. « Imprimer », the clearance, the source's shape followed (built 2026-11-23, amended above;
     a clearance around a fully rounded end is refused);
  5. the alert between two parts seated in one part (built 2026-11-23).

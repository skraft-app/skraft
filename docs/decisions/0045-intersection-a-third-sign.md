# The intersection, a third sign of bodies

- Status: accepted (2026-11-20); built (2026-11-20): the sign, the kernel's recipe, the
  navigator, the ghosts, Grouper / Dégrouper (step 1); « Solide »'s third choice and the
  « Intersecter » tool keeping its tools (step 2)
- Design: [Bodies](../model/bodies.md#a-third-sign-the-intersection)
- Amends: [Body definitions may be groups](0024-body-groups.md) (two signs),
  [Body nodes](0042-body-nodes-operators.md) (its step 3's « ∩ node »)

## Context

Decided on 2026-11-20 (questions / answers), after a review of body nodes. A part is computed from
its bodies by a fixed formula, `(∪ positives) − (∪ negatives)`, nested in groups: no history, no
naming. The intersection had no place in it: the « Intersecter » tool between parts makes a new
part, a free shape, its tools lost.

The intersection needs no order: `(X − N) ∩ I = (X ∩ I) − N`. It fits the fixed formula as a
third sign, where a node (0042) would add an ordered operation for nothing.

## Decision

- **A body has one of three signs**: positive (adds), negative (takes away), **∩ (limits)**.
  Its name is **« Intersection »** everywhere — the interface (its role, beside « Brut » and
  « Usinage »), the code (`Sign::Intersect`, `intersects`) and the docs —, « Intersecter » the
  drawing tools' choice (decided 2026-11-20).
  A level — a part's root, a group — is `(∪ positives − ∪ negatives) ∩ I1 ∩ I2 …`: **each ∩
  body limits** (two templates laid over each other). Their union limits by grouping them in a
  ∩ group, whose children are positive (as a negative group's: the shape it limits by).
- **No ∩ node**, nor a mirror node (a mirror is a placement): `NodeOp` keeps its blends.
- **The sign chosen**: the navigator's sign button cycles `+ → − → ∩ → +`; the drawing tools'
  « Solide » in an open part gains a third choice, « Intersecter ».
- **Shown**, in an open part, as a ghost of a third hue (a blue-green), beside the blanks' very
  light ghosts and the machinings' red ones; the ∩ icon on its navigator row.
- **The « Intersecter » tool** does as « Soustraire »: each tool part comes into the target as
  a ∩ sub-group, editable; the target keeps its bodies; its options (keep A / B) stay.
- **Grouping several ∩ bodies, or ungrouping a ∩ group of several positives, is refused**
  (the shape would change: the union of the shapes is not their intersection); a ∩ alone groups
  and ungroups freely.
- Adopted with it: a ∩ body moved in the tree stays ∩ (it limits where it goes); a level with
  ∩ bodies and no positive is empty (an empty part, an error shown); a ∩ added is incremental
  (the shape as it is, ∩ the body); stretch and welding take ∩ bodies as the others.

## Alternatives considered

- **A ∩ node** (0042's step 3 as first written): an ordered operation, a node, its children
  intersected — but the intersection needs no order, and a sign keeps every body where it is.
- **Several ∩ bodies' union limiting** (as the tool's `(∪A) ∩ (∪B)`): one ∩ body would then
  stand for « at most here » only with the others; « each limits » is what two templates do.
- **The tool left as it is** (a free shape): the tools lost, nothing to edit after.

## Consequences

- The kernel's recipe takes intersections beside positives and negatives (`Task::Recipe`, the node
  task's tree), named as the others (the names of a node over a limited level).
- The navigator, the ghosts, the file, Grouper / Dégrouper, the moves of bodies, Stretch and
  welding know three signs.

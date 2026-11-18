# Body definitions may be groups: a CSG tree without history

- Status: accepted (2026-11-16)
- Design: [Bodies](../model/bodies.md#body-groups)

## Context

Decided on 2026-11-16 (questions / answers), built the same day (branch
`feature/body-groups`): the model, the interface, editing inside a group, machinings inside
groups shown.

With [a part as positive and negative bodies](0018-part-bodies.md), every negative cut every
positive of the part. A union gave the target all the tool part's bodies, so the tool's
negatives drilled the target where they entered it, which a warning (`Warning::UnionCut`,
worked out from two volumes by `Task::UnionVolume`) said; a difference froze the tool into one
negative `Brep` body. A compound tool (a counterbore = two cylinders, a hinge drilling = cup ∪
screws) had no form of its own.

## Decision

- A body definition is a leaf (a shape, in its own axes) or a **group** of bodies,
  `(∪ positive children) − (∪ negative children)`; the part is the root group. A group has
  only a cached B-rep, computed from its children.
- **A negative's reach is its group** (« every negative cuts every positive » stays true within
  a group). Flattening a group widens its negatives' reach to the parent's.
- **No order between siblings**: only nesting carries meaning.
- The union puts the tool part's root as a **positive sub-group** of the target, the
  difference as a **negative sub-group**, their bodies kept and editable; the intersection
  stays a new part with one positive `Brep` leaf.
- Definitions stay local to their part (groups included); cycles are forbidden.
- One piece is checked at the root only; failures are reported by the body's path.
- The tree shows in the outliner, under its part; a group is opened as a component is.

## Alternatives considered

- **An ∩ node, or any ordered operation**: rejected — that would turn the tree into a feature
  history (persistent naming).
- **The flat list of bodies** with every negative cutting every positive, the union's warning
  and the difference's frozen `Brep` tool (the model until then): `Warning::UnionCut`,
  `Task::UnionVolume` and `Warning::Flattened` went with this decision.

## Consequences

- A through-hole meant to drill the target too is had by flattening.
- Changing a leaf computes its path to the root only, the siblings' results reused; a shared
  group is computed once for all its copies.
- Every operation on bodies runs inside a group as in a part (`Document::apply_in_group`).
- Planned with it and not built: caching `∪P` and `∪N` apart per group, keyed by its
  definition and a version, a group with many children balanced internally; « Convertir en
  forme libre »; « Extraire en pièce »; a document-wide library (tools, hardware,
  `source: Linked`).

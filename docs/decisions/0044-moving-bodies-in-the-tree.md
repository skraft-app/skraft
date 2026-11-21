# Bodies and occurrences moved in the tree: the navigator's drag and « Sortir du groupe »

- Status: accepted (2026-11-20); built (2026-11-20): the move inside a part and « Sortir du
  groupe » (step 1), the navigator's drag and drop and rows' menu (step 2), across parts and
  assemblies (step 3: `BodiesTo::Part` / `NewPart`, `MoveParts`, `MoveOccurrences`)
- Design: [Bodies](../model/bodies.md#moving-bodies), [Copy, cut and paste](../tools/copy-paste.md#across-the-parts-boundary)
- Builds on: [Body nodes](0042-body-nodes-operators.md) (« the order is changed by moving a body
  into or out of a node »), [Parts paste into an open part as bodies](0027-paste-across-the-part-boundary.md)

## Context

Decided on 2026-11-20 (questions / answers), after a review of body nodes. A part's tree is its
history (0042): what is under a node is its past, what is above its future. Yet nothing moved a body
from one level to another but Grouper / Dégrouper and cut / paste — the paste giving a new body (a
new id) in the group open, so a hole could not be taken out of a round to drill after it, keeping
what it is. Since the same review, names follow bodies moved in the tree (`Outcome::relocated`,
`ops::relocate`): a move breaks no other body's edges.

## Decision

- **The gesture**: the navigator's rows dragged and dropped — on a row, into it (a group, a
  node, a part, an assembly); between two rows, into their parent —, and a command « Sortir du
  groupe » (one level up: the context menu, a shortcut) for who does not drag. On a body at a
  part's root it is « Sortir de la pièce »: the body becomes a part beside it.
- **The world place is kept**: only the placement is computed again, in the axes of where it
  lands.
- **A body keeps what it does** (adds or takes away): its sign turned when the signs on its
  way differ — a positive child of a machining group (it took away) out at the root becomes
  negative; a drill dropped into a machining group (a group of drills) becomes one of its
  positive children, still drilling.
- **Option / Ctrl held at the drop copies** instead of moving (as Move's copy key): a new body
  of the same definition, a new occurrence of the same component.
- **Inside a part, a body moved keeps its id and its definition**: the nodes above its old and
  its new level rewrite their names (the ones over both find it again: `[g1, b]` becomes
  `[g2, b]`). **A node it leaves loses the edges it took on it**: allowed, the node in error
  (⚠, its edges lost) as any lost edge, said by a toast; Undo takes it back.
- **A group or node shared by several copies in the part** (one definition, several bodies),
  left or entered: **that copy is made unique first** (its own definition, its children new
  ids), the others keep what they hold; the toast says so. The scope of the option bar
  (« Tous les exemplaires » / « Celui-ci ») is the part's, as for any edit: it counts the part's
  occurrences, not the copies of a group inside it.
- **Across a part's boundary, the paste's rules (0027), moving instead of copying**: a body
  dropped on another part becomes a body of it (sign kept); on the model or an assembly, a new
  part (signs kept, all positive with no positive among them); a part's occurrence dropped on a
  part becomes a body of it (positive, a group of its bodies unless it is one); an occurrence
  moves from an assembly to another. Ids are new across a part (they are unique within one).
- **An occurrence moved into a shared assembly** changes its definition, every copy of it, as
  any edit in an assembly; an assembly dropped into itself or what it holds is refused.

### Steps

1. The move inside a part (`Operation::MoveBodies`: from a level to another, `copy`, the scope,
   the copies made unique, the names rewritten) and « Sortir du groupe » / « Sortir de la
   pièce ».
2. The navigator's drag and drop inside a part (feedback while dragging, Option / Ctrl copies).
3. Across parts and assemblies.

## Alternatives considered

- **Cut and paste only**: no new operation; a body moved is a new one (a new id), the nodes
  above lose it, a group's or node's own names with it. Kept for crossing documents.
- **Commands only** (« Sortir du groupe », « Mettre dans… » choosing the target): no drag; but
  the navigator is where the tree is seen.
- **A shared group edited for every copy** (its definition loses the body, each copy's parent
  receives one): consistent with drawing in a shared group, but a drop elsewhere than the
  parent has no single place to put the copies' bodies.
- **A move that takes a node's edges refused**: safe, but the user would first have to take
  the edges off, then move, then pick again.

## Consequences

- One rewrite of names for every structural edit (group, ungroup, wrap, make unique, move):
  `Outcome::relocated`, relative to the bodies the operation edited.
- The navigator gains drag and drop (it had none); `context.*` (docs/control-protocol.md)
  drives it in tests.

# A part may be empty

- Status: accepted (2026-11-16)
- Design: [Bodies](../model/bodies.md#empty-parts)

## Context

Decided and built on 2026-11-16. A part had to keep a positive body: making its last blank a
machining, or deleting it, was refused (`Refusal::LastPositive`), and a machining swallowing
the part was a failed machining. The file refused a part or a group without a positive body.

## Decision

Nothing is refused: a part left without material is **empty** — its shape `Solid::empty()`,
0 pieces, the same error as a part in several pieces (⚠ and its name in the error colour, « vide,
à corriger » instead of its dimensions). The **kernel** says so, never the model: the recipe
leaves out empty inputs and, without a positive holding material, gives its base or nothing; a
difference gives nothing rather than failing. A group without a positive child is accepted and
fails in its parent.

## Alternatives considered

- **« A part keeps a positive »** (`Refusal::LastPositive`, the rule until then): superseded.

## Consequences

- `Refusal::LastPositive` is gone; the sign toggle flips the last positive too.
- The ⚠ goes up to the groups, parts and assemblies holding a body or a part in error, so a
  folded tree still shows it.
- Closed, an empty part draws nothing (the outliner reaches it); open, its bodies are drawn, so
  a blank can be given back.

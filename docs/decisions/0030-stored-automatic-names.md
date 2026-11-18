# Automatic names are stored, numbered from #1

- Status: accepted (2026-11-17)
- Design: [Bodies](../model/bodies.md#automatic-names)

## Context

Decided on 2026-11-17 (questions / answers, branch `feature/auto-names`). The bodies' first
names were computed, by their nature and their rank among their siblings (« Solide 3 »): they
told copies of one definition apart as if they were not (« Solide 2 », « Solide 3 » for one
shape) and renumbered as bodies came and went.

## Decision

One rule for components and body definitions: a definition not named by the user is named by
its base and a number, in SketchUp's format, **stored**, stable, shared by its copies — a part
« Pièce#2 », a body definition « Extrusion#2 », per nature for bodies (« Extrusion#1 »,
« Révolution#1 », « Solide#1 », « Groupe#1 »). Names are settled after each operation
(`Document::settle_names`); a name given is never touched.

## Alternatives considered

- **Names computed by nature and rank** (« Solide 3 », the rule until then): copies looked
  distinct, numbers changed as bodies came and went.

## Consequences

- Components' automatic names are unique in the document, body definitions' in their part; a
  definition whose nature changed, or whose name an earlier one holds, is named anew.
- The navigator shows an automatic name in italics, attenuated.

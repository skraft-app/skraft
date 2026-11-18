# A part is positive and negative bodies

- Status: accepted (2026-11-14)
- Design: [Bodies](../model/bodies.md)

## Context

Decided on 2026-11-14 (questions / answers), built the same day in steps (branch
`feature/part-bodies`): the model and the kernel, the interface of bodies, stretching bodies,
body definitions.

A part was a recipe: **blank** − (tool₁ ∪ tool₂ ∪ …), the blank a B-rep (or a profile × a
thickness) in the part's axes, the machinings tools placed in them. A profiled blank
([Prism tool and profiled blanks](../tools/prism.md)) is bound to the part's axes — its
outline in their XY plane, extruded along their Z. Changing a part's axes would then have to
transform the profile, and drop it whenever Z ends up in the plane or oblique. The cut-list
dimensions were read on the blank's box.

## Decision

Give the blank its own placement in the part, which frees the profile from the part's axes;
one step further, the blank and the machinings become one notion: a placed solid with a sign.

- `Part = (∪ positive bodies) − (∪ negative bodies)`, `Part { bodies }`, every body placed in
  the part's axes, a profile in its body's own axes. The former blank is one positive body at
  the identity, the former machinings the negative bodies, a free part one positive `Brep`
  body.
- **The part's axes are independent of its bodies**: changing them = correcting every body's
  placement and compensating each occurrence's T, exact, for any rotation, no kernel.
- **A fixed formula, no history**: every negative cuts every positive, those added later
  included; the order does not matter. A hole is filled by deleting its negative.
- **The cut list reads the final**: the dimensions are the exact box of the final B-rep in the
  part's axes; the exact outline for exports and nesting is read from the final B-rep too; a
  profile is an editing parameter of its body only.
- **One piece of wood**: a final in several lumps is in error, with no cut-list dimensions.
- **Booleans between parts**: the union gives the target all the tool part's bodies, positive
  and negative, with a warning when the tool's negatives remove material from the target; the
  difference adds the tool's final shape as one negative `Brep` body; the intersection makes a
  new part with one positive `Brep` body.
- **Body definitions** (the same day): a body is an occurrence of a definition of its part, so
  copies share their shape; the sign is the body's.

## Alternatives considered

- **The blank and its machinings, bound to the part's axes** (the model until then): changing
  the axes transforms the profile, or drops it.
- **Quarter turns of a parametric blank's axes** (the procedural track,
  [Procedural parts](0007-procedural-parts.md)): superseded for profiled bodies.

## Consequences

- Supersedes the data model's blank box for the cut-list dimensions, and the prism page's
  outline read from the profiles.
- Later decisions refined it: [body groups](0024-body-groups.md) (a negative's reach is its group;
  the union and the difference put the tool in as a sub-group, the union's warning gone) and
  [empty parts](0025-empty-parts.md) (no more « a part keeps a positive »).
- Left open then: the rule naming the « main » lump (the first positive's) is a proposal; a
  carcass generator (an assembly of parts) stays outside this model; a parametric source would
  be one more kind of `source`.

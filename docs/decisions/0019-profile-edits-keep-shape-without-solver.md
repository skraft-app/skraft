# Profile edits keep the shape by sides and stored radii, without a solver

- Status: accepted (2026-11-14)
- Design: [Prism tool and profiled blanks](../tools/prism.md#the-shape-kept-while-editing)

## Context

The Profile tool first moved its grips freely: a box's profile edited corner by corner stopped
being a rectangle, a cylinder's arc grip turned its disc into an oval, a fillet next to a moved
corner lost its tangency (branch `feature/profile`, 2026-11-14). The file format is v1 until the
first release, so it need not be shaped in advance for a later design.

## Decision

The first two of three ways weighed, now:

- editing by sides with no stored constraint (SpaceClaim's, Shapr3D's direct editing): a corner's
  grip moves its two sides parallel to themselves, a side's grip moves the side;
- the designer's intent kept in the recipe: fillets stored as radii on sharp corners
  (`Corner { p, bulge, fillet }`), the arc computed wherever the outline is used.

Tangencies are kept by rules read on the outline as it was when the grip was taken
(`kernel::profile::rearced`), with no solver. Free moves (the copy modifier, « Libre ») keep the
former behaviour: a corner moving alone.

## Alternatives considered

- A parametric sketch: stored constraints, driving dimensions, a solver. Left for later, if
  needed; the candidate solver is ezpz (KittyCAD, Rust, MIT). SolveSpace's libslvs is GPLv3: out.

  *Note (2026-11-18):* Skraft is GPL-3.0-or-later since, so libslvs's licence no longer rules it
  out. What remains against it is technical: a C++ library to build for every target, the web
  one (`wasm32-unknown-unknown`) included, where ezpz is plain Rust.

## Consequences

- A rectangle stays a rectangle, a trapezoid keeps its angles, a disc stays round, a fillet stays
  tangent and comes back when there is room again.
- Chains of arcs tangent to each other (an S) may lose their tangency: that is the sketch's job.
- The format is not shaped for a sketch in advance (v1 until the first release).

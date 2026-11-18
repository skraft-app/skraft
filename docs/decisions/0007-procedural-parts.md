# Procedural parts: a parametric blank and machinings anchored on it

- Status: proposed (discussed on 2026-11-11, set aside: nothing decided nor built)
- Design: [Data model: parts, assemblies, machinings](../model/data-model.md), [Bodies](../model/bodies.md)

## Context

A part is already procedural: a recipe ([Machinings and the recipe](../model/data-model.md#machinings-and-the-recipe))
whose final B-rep is only a cache. The track is to push this further without falling into a
general node graph or a timeline.

This proposal predates [a part as positive and negative bodies](0018-part-bodies.md) (2026-11-14):
it speaks of one blank and its machinings; the bodies' model supersedes its quarter-turn
discussion for profiled bodies, and leaves the track open — a parametric source would be one
more kind of body `source`.

## Decision

Proposed, not decided:

- `Part = Blank(params) − Σ Machining(type + params, anchor)`, a two-level CSG tree with a
  fixed shape and parametrised leaves.
- **Parametric blank and anchors**: a "board" blank (L × W × T in the part's axes), and
  machinings anchored on its faces ("face +Z, 37 mm from face −X"): resizing the board, the
  holes follow. Rule: **anchor only on the blank** (its 6 faces and 12 edges, named stably),
  **never on a face of the final B-rep**. Since machinings only remove material and their order
  does not matter, there is no history to replay: this sidesteps persistent face naming, the
  blocker of parametric modelling. A free blank (imported B-rep, boolean result, curved part)
  stays direct, its machinings placed in the part's axes as today.
- **Machining types** (hole, groove, mortise…) are the procedural generators of the tools.
- **Shared tool definitions**, rather than "a machining is a solid that has machinings": a tool
  is composed by **union** (a counterbore = two cylinders, a countersink = cylinder + cone),
  seldom by subtraction, and it is not a part (no material, type, grain, nor place in the cut
  list). What is worth having is sharing: a "hole ø8 × 30" defined once, placed many times,
  changed everywhere — the component / occurrence mechanism applied to tools, and what
  `source: Linked` prepares for hardware. Needs: a tool kind of its own in the document (not a
  `Part`), cycles forbidden, a dependency graph to invalidate the parts using a changed tool.
- **Changing a part's axes** with a parametric blank:
  - **quarter turns** (re-orienting the grain, standing up a part drawn lying): a signed
    permutation of the box's axes (24 rotations). L, W, T are permuted, the named faces
    relabelled by the same permutation, the machinings' parameters unchanged, the occurrences'
    T compensated — exact, no kernel. Anchors are stored **geometrically** (±X, ±Y, ±Z of the
    box), the labels end / edge / face **derived** from the current axes: stored semantically,
    re-orienting the grain would move the holes;
  - **origin**: a convention (e.g. the min corner), not movable; T absorbs translations;
  - **oblique rotation**: meaningless for a board (its box no longer gives the cut-list
    dimensions): only through an explicit "Convert to free shape" (parameters dropped, anchors
    frozen into placements). A free blank keeps any rotation, B-rep and tool placements
    re-expressed.
- **A non-rectangular blank** (decided on 2026-11-13, [Prism tool and profiled blanks](../tools/prism.md)):
  a profile × a thickness, the profile kept in the blank's recipe — the board is the case of a
  rectangular profile.
- Possible order: machining types → parametric board blank and anchors → shared tool
  definitions and hardware-linked machinings.

## Alternatives considered

- **A general node graph or a timeline** (feature history): what the track avoids.
- **Recursive machinings on tools** ("a machining is a solid that has machinings"): not
  wanted; shared tool definitions composed by union instead.
- **Anchors stored semantically** (end / edge / face): re-orienting the grain would move the
  holes.

## Consequences

Open questions:
- Is anchoring on the blank only acceptable (a hole in a chamfered face is then expressed
  against the box)?
- Should the axes tool snap to the 24 orientations on a parametric part, or stay free and
  offer the conversion only on an oblique angle?

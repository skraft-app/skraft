# Dashes laid on the segment, in model units, the guides' on screen

- Status: accepted (2026-11-18)
- Design: [Display](../render/display.md), [Drawing helpers](../render/drawer.md)

## Context

Lines are quads widened by the vertex shader, and dashes are made by the fragment shader
(earlier, dashes were cut on the CPU, up to a thousand pieces a segment). From 2026-11-17 every
dashed line had its dashes spread evenly on screen: a line running away saw its dashes slide
along it as the view orbited (the "swimming" dashes), which the CPU dashes, cut on the segment,
did not have.

## Decision

Since 2026-11-18, dashes are laid on the segment itself: the vertex shader passes a distance
along it with perspective-correct interpolation. Three ways (`render::Lay`):

- **anchored** (a tool's dashed lines, a box's half edges): a dash at both ends, the number of
  periods set by the length on screen, the pattern stretched to fit;
- **in model units** (curves, hidden parts, every other dashed line): one period for the whole
  picture, the power of two (model units) nearest to the pattern's length at the view's focus;
  dashes fade into a line where they would be a few pixels long;
- **on screen** (the guides): as long on screen everywhere, held still as the view pans,
  sliding along the guide as it orbits.

## Alternatives considered

- Dashes spread evenly on screen for every line (the state from 2026-11-17): swimming as the
  view orbits.
- A period chosen pixel by pixel (tried on 2026-11-18, the nearest power of two to the
  pattern's length there): it kept the dashes about their length on screen, but a receding
  guide then had dashes of every size, shrinking then doubling all along it. Refused.
- The guides laid in model units: held still, but their dashes shrank as they ran away. The user
  preferred them swimming, as long on screen (2026-11-18).

## Consequences

- The distance is laid from the segment's nearer end, the phase passed in two parts (high, low;
  computed in `f64`) and taken modulo the period exactly, so a line far from its phase's origin
  keeps its dashes.
- Anchored dashes jump when the count of periods changes; model-unit dashes double or halve as
  the view zooms by a factor two.

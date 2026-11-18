# Stretch rewrites the B-rep record, on the recipe

- Status: accepted (2026-11-11)
- Design: [Stretch](../tools/stretch.md)

## Context

A stretch must lengthen or shorten the matter crossing a cutter and translate the rest, keep
machinings exact, and compress as well as extend, without deforming what lies in one section.

## Decision

- **The stretch applies to the recipe, not to the final B-rep**: the blank (now the bodies) is
  stretched; a machining (a body) lying in one section is only translated (its placement),
  exact; one crossing a cutter has its own shape stretched in its frame. The part is then
  computed again from its new recipe.
- **The B-rep stretch rewrites the record**: a pure function on `solidcore_record::brep::Brep`
  (no kernel), a piecewise translation along the axis. Elements in one section are translated;
  elements crossing a cutter must be invariant along the axis (lines along it, planes
  containing it, cylinders and extruded NURBS nets along it) and are lengthened; anything else
  across a cutter refuses the stretch, as OCL refuses a curve crossing a cutter. The kernel
  reading the record back validates the result.
- Probed on 2026-11-11 by a test, not kept: a box stretched and compressed, a cylinder
  stretched and compressed along its axis (standing and lying), each read back by the kernel,
  then used as a blank and as a tool in a subtraction: all correct.

## Alternatives considered

- Cutting at the cutters, translating the pieces and filling the gaps by extrusion
  (`prism::section` + `extrude_region`) then union: rejected — slower, coplanar unions, and
  nothing for compression.

## Consequences

- No kernel during the gesture: the preview deforms the display mesh on the CPU with the same
  sections.
- Curved geometry across a cutter (cone, sphere, torus, spline, intersection curve) is refused;
  moving the cutter usually solves it.
- Kernel-specific forms had to be recognised as what they are (a mirrored solid's lines as
  degree-1 splines, its cylinders as cones of half angle 0; a prism's upright edges as degree-1
  splines on their own length — found 2026-11-14, when a filleted prism could not be stretched
  along its height).

# Section caps: colour from the back faces, extent from a stencil

- Status: accepted (2026-11-18)
- Design: [Rendering and look](../render/look.md)

## Context

The section view cuts the model by one plane in the fragment shader. Parts are closed B-reps,
so through the cut only their back faces show. The cut must look closed (a cap in the part's
colour, hatched as on a drawing) and stay right where parts touch: a part standing on another,
a pin in its hole — whose two meshes cross by their chords, so a depth test between true
depths settles it facet by facet (stripes).

## Decision

A back-facing fragment, section on, is painted flat and darker in its part's colour, at its true
depth. Then the faces left are drawn again without depth test nor colour, each fragment
flipping the stencil's low bit (an odd count: the ray meets the plane inside a part); where the
bit is set, the back faces are drawn again (`fs_cap`) at a depth brought just behind the plane
(1/64 of the way from the plane to the face, keeping their order), so the cut part's nearest
back face wins and gets the cap's colour and hatch. Hatches are anchored on the plane, one of
six directions per part definition, their pitch following the zoom by powers of two, one pitch
per placement.

## Alternatives considered

- Writing the plane's depth on every back face, without the stencil: it painted every kept part
  behind the plane as a cap and hid its edges.
- A full-screen pass writing only the plane's depth: it left the colour to the faces' depth
  test, which a part in contact won.
- A hatch pitch per pixel: it ended half the lines across a cap seen at a slant, where the level
  changed.
- Fading lines in as the zoom crosses a level: refused, the user wants one ink for all the
  lines.

## Consequences

- The depth buffer carries a stencil: `Depth32FloatStencil8` when the device has it, else
  `Depth24PlusStencil8`.
- Where the count is even (the eye inside a part, parts overlapping), the back faces keep their
  flat, unhatched colour. X-ray ghosts are cut without caps.
- Cut lines are cut on the CPU from the display meshes, so they meet the caps exactly.

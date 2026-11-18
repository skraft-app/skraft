# The sketch lives in the part: a profile is kept by the body it makes

- Status: accepted (2026-11-13)
- Design: [Prism tool and profiled blanks](../tools/prism.md)

## Context

Discussed on 2026-11-13, after OpenCutList's `SmartDrawPolygonActionHandler`
(`tool/smart_draw_tool.rb`), when the Prism tool (first named Polygon) was designed: an outline
drawn point by point, extruded into a part, whose outline must stay editable afterwards. Skraft
is a direct modeller: a parametric history is out of its design ([Skraft — design](../README.md)).

## Decision

The sketch lives in the part: no free-standing sketch entity in the model, no live link between a
sketch and the parts made from it. A profiled blank is `Profile × thickness`, the profile stored
in the blank's recipe; to change the outline later, the part is opened and its profile with it
(as its machinings). A prism drawn in an open part is a machining. Built in three steps, all on
2026-11-13: the `Prism` primitive (straight sides) and the Prism tool, faithful to OCL; arcs in
the profile and the tool (each corner `{ p, bulge }`, DXF's LWPOLYLINE); the profile edited in an
open part, the part kept parametric (`Part::profile: Option<BlankProfile { profile, z0, height }>`,
`Operation::SetProfile`).

The tool was renamed « Dessiner un prisme » from « Polygone » on 2026-11-13: what it makes is a
prism, the polygon only its outline.

## Alternatives considered

- A free-standing sketch entity, linked live to the parts made from it: a parametric history,
  rejected.
- A profile drawn on a face as a dedicated machining step: dropped on 2026-11-13.
- A geometric constraint solver: not planned; inferences and typed values play that part,
  SketchUp's way.

## Consequences

- At the time, the cut list read the thickness along the part's Z and the length and width from
  the profile's box, and the exact outline for DXF / SVG exports and nesting was to come from the
  profile. Bodies ([Bodies](../model/bodies.md)) since moved the profile onto a positive body
  placed in the part (`BodyDef::profile: Option<BodyProfile>`, `SetProfile { body }`), and the
  cut list and exports read the final B-rep: a profile is an editing parameter of its body.
- The machinings stay where they are in the part's axes, and so does the origin, when the
  outline grows or shrinks; anchoring them on the blank was left for later.
- Editing a profile then had to keep its shape without a solver:
  [Profile edits keep the shape without a solver](0019-profile-edits-keep-shape-without-solver.md).

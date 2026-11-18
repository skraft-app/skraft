---
status: built
code: [crates/tools/src/tools/shapes/prism.rs, crates/tools/src/tools/profile.rs, crates/kernel/src/profile.rs, crates/kernel/src/engine.rs, crates/model/src/profile.rs, crates/model/src/ops.rs]
---

# Prism tool and profiled blanks

The Prism tool « Dessiner un prisme » draws a closed outline of lines and arcs and pulls it into a
prism, after OpenCutList's `SmartDrawPolygonActionHandler` (`tool/smart_draw_tool.rb`). The
outline is kept as the profile of the body it makes, and edited later in the open part by the
Profile tool, which keeps the outline's shape (directions, fillets, tangencies) without a
constraint solver.

## The sketch lives in the part

No free-standing sketch entity in the model, no live link between a sketch and the parts made
from it (that would be a parametric history, [Skraft — design](../README.md)). The profile
belongs to a body placed in the part ([Bodies](../model/bodies.md)): a profiled body is
`Profile × thickness`, the profile stored in the body's definition; to change the outline
later, the part is opened and its profile with it ([Opening a part](../model/opening-a-part.md)).
The cut list reads the final B-rep, not the profile: a profile is an editing parameter of its
body ([Bodies](../model/bodies.md)). A prism drawn in an open part is a body of
it ([Opening a part](../model/opening-a-part.md)).

There is no geometric constraint solver: inferences and typed values play that part, SketchUp's
way.

Decision record: [The sketch lives in the part](../decisions/0016-sketch-lives-in-the-part.md)

## The Prism primitive

`Primitive::Prism { origin, profile, height }` — `profile`, the outer loop's corners in the XY
plane through `origin` (relative to it, at least three, a simple outline, either way round: the
kernel turns it), extruded along +Z by `height`. Its `at_origin` puts the part's origin at the
lowest corner of the profile's box (arcs included), as a box's. Built by SolidCore's
`prism::extrude::extrude_region` (a `Region2` of `Seg2::Line`s and `Seg2::Arc`s on the frame,
swept from 0 to `height`; `Solid::new_prism`). One outer loop only: no holes. The tool refuses a
profile that crosses itself before the kernel does.

## Arcs

Each corner is `Corner { p, bulge, fillet }`, as DXF's LWPOLYLINE (OCL's DXF export): `bulge` =
tan(sweep / 4) curves the side from that corner to the next one, 0 straight, positive turning
counter-clockwise; `fillet` is a radius on a sharp corner ([Fillets are
radii](#fillets-are-radii)). The ends are shared, so the outline cannot open.
`kernel::profile` works on these outlines: area, clean-up, simplicity (lines and arcs crossing),
the box, the offset (an arc offset is the concentric arc, as SolidCraft's; an arc the offset
turns over or shrinks to nothing is taken out as a line would be), the fillets, and points along
it for the previews.

## The Prism tool

« Dessiner un prisme » (`tool.prism`, P, ribbon group Create after the cylinder: what it makes is
a prism, the polygon only its outline). Its states are OCL's: the first point, the points, the
height.

- **First point**: the box's first step ([Interaction](interaction.md), [Inferences](inferences.md): the plane of the face aimed at, an edge,
  the ground; the arrows lock the plane, ↓ the face's; the copy modifier held takes a face's
  centroid, Shift tapped then its whole outer loop as the profile, and the height follows; the other modifier measures from a corner). The plane
  locked then stays for the whole profile (OCL's `@locked_normal`), and the arrow locks are
  dropped once the point is placed: from there they lock an axis.
- **Points**: each click adds a corner; the cursor's point is inferred with the lines through
  the last corner (red, green, blue, parallels), the arrows lock the axis through it (→ red,
  ← green, ↑ blue, ↓ frees it; a beep — a word in the status bar — when the axis leaves a
  plane already set), Shift held keeps the line inferred. Each corner placed drops the locks.
  The plane, as OCL: with one corner, the box's second-corner rules (an axis plane through
  the first corner the point lies in, an edge or a guide and the first corner, a face the
  first corner lies on, else the upright plane through both); with two, the plane through
  them and the cursor's point; from three on (or from the first arc) it is set, and the
  cursor's point is brought onto it. A plane locked at the first point overrides all of this.
  From three corners, the cursor within 10 px of a placed corner snaps to it (on the locked
  axis, its foot there); from two, off any feature, it snaps to the foot of a placed corner on
  the line from the last one, drawn dotted magenta from that corner (OCL's alignment).
- **Closing**: a click on a placed corner (three at least), a double-click (the corner added,
  then closed), or Enter with the measurements box empty (a deviation from OCL). The profile
  then goes to the height.
- **Reversed** (« Inversé », OCL's `measure_reversed`, saved toggle drawn with OCL's icon, the copy modifier tapped): new corners
  go before the first one instead of after the last, so the outline can grow at either end;
  the lines, the locks and the typed lengths then start from the first corner.
- **Guides**: Shift tapped adds a guide from the last corner (the first, reversed) towards the
  cursor: a real guide of the context ([Tape measure and guides](tape-measure.md), its own undo step), where OCL draws a
  construction line it erases when leaving the step.
- **Esc** drops an arc under way, else takes away the last corner placed (in the order placed,
  OCL's stack); with none left, back to the first point. From the height, back to the points,
  all kept.
- **Typed**: a length along the cursor's direction from the last corner (first, reversed), or
  a point (`[x;y;z]`, `<dx;dy;dz>`), brought onto the plane once it is set.
- **Height**: the box's ([Interaction](interaction.md)): Shift held or a double-click take the last height, the copy
  modifier held runs it to the first face along the normal from the profile's middle, tapped
  it centres it (« Hauteur centrée », shared with Box and Cylinder).
- **Made**: in the frame of the profile's plane through the first corner (the frame of
  [Interaction](interaction.md)'s shapes, its `u` horizontal, or red on a horizontal plane): built as is on an axis
  plane, else in that frame and placed. In an open part, a body of it ([Opening a part](../model/opening-a-part.md)).
- **Offset**: the « Décalage » option, shared with Box and Cylinder ([Application interface](../ui/interface.md),
  saved for the tool, `3x` at any step), moves the outline made, never the height; the outline
  drawn is dashed then, the one made solid, from three corners (the cursor's included), and
  the lengths written and typed stay the drawn ones'. Computed as SolidCraft's sketch offset
  (`crates/sketch/src/offset.rs`), in `kernel::profile::offset`: each side moved along its
  normal, the corners sharp where neighbouring sides meet — exact, no polygon library, arcs
  included (an arc offset is a concentric arc). As Clipper's miter offset (OCL's), a side the
  offset turns over is taken out and its neighbours meet instead (a trapezoid can become a
  triangle). Refused, said in the status bar, the height step kept: nothing left (inward past
  the middle), or an outline that would cross itself (an outward offset closing a notch, an
  inward one pinching a neck: one outline cannot be two pieces).
- **Sides**: the choice « Tracé » in the bar — Ligne, Arc tangent, Arc 2 points —
  turned by Tab, not saved (each outline starts with lines, as SolidCraft's chains); the
  cursor carries a word for an arc mode (the inference's tip). SolidCraft's gesture
  (`ui-egui/src/sketch_line_arc.rs`) switches Ligne ↔ Arc tangent: leave the last corner
  (20 px), come back onto it (8 px) and rest there 0.35 s (drifting 2 px at most); then it
  must be left again before switching back. The side closing the outline follows the mode.
  - *Arc tangent*: from the last corner (the first, reversed), tangent to the side ending
    there, to the cursor's point; with no side yet, a line. Its end straight behind is
    refused. Typed: the radius, the cursor keeping the side the arc turns to.
  - *Arc 2 points* (SketchUp's): a click sets the end, a second one the bulge (the arc
    through the cursor's point); the bulge snaps to the half circle within 10 px. Typed: the
    end as a line's, then the bulge's height (sagitta). Esc drops the arc under way.
- **Fillet**: the « Congé » length option (saved for the tool, `5r` at any step, as
  the offset's `5x`) rounds every corner between two straight sides at creation, after the
  offset — the radius is the made part's. A corner whose sides are too short for it (the
  fillets at both ends of a side counted) stays sharp. Previewed with the offset's outline.
  Recorded as radii on the sharp corners (`profile::with_fillets`, [Fillets are
  radii](#fillets-are-radii)).
- **Whole face**: Shift with the copy modifier held takes a face's outer outline with its arcs
  too (a face with edges neither straight nor arcs — an ellipse, a free curve — is refused,
  said).

## The profile in the part

- **The recipe keeps it**: `BodyDef::profile: Option<BodyProfile { profile, sweep }>`, in the
  body's axes; for a prism `Sweep::Extrusion { z0, height }` — the outline in the body's XY
  plane, extruded from `z0` by `height` along its Z ([Turned bodies](turned-bodies.md) for
  `Sweep::Revolution`). The body's solid is built from it (`BodyProfile::primitive`,
  `Primitive::Prism`) and stays its cache ([Data model](../model/data-model.md),
  [Bodies](../model/bodies.md)). Box, Cylinder and Prism all record theirs (`BodyProfile::of`: a
  rectangle; a disc as two half circles; the outline made, offset included, fillets as radii).
  Imports, booleans' results and shapes stretched askew are free (none).
- **Stretching keeps it when it can** ([Stretch](stretch.md), in the body's axes;
  `BodyProfile::stretched`): along Z, `z0` and `height` follow their sections; along any axis in
  its plane, each corner moves with its section (its fillet's radius kept), an arc — a fillet's
  too — whole in one section rigidly; an arc across a cutter (it would not stay an arc), or an
  axis neither in its plane nor along Z, drops it — the body becomes free. (The kernel refuses
  these stretches anyway, an edge across a cutter having to run along the axis: a safeguard.)
- **The other bodies stay where they are** in the part's axes, and so does the origin when the
  outline grows or shrinks.
- **`Operation::SetProfile { target, body, profile, scope }`**: the body's definition gets the
  new profile and its solid built again from it (every copy of the body shows it), then the
  part ([Data model](../model/data-model.md)); the scope of the open part ([Opening a part](../model/opening-a-part.md)),
  "only this one" making it a separate part. Refused (`Refusal::BadProfile`) for a body without
  a profile, an outline that is not simple, or no height.

## Editing the profile

The Profile tool (`ToolKind::Profile`, « Profil ») edits a body's profile: the body open, the
deepest edit level ([Bodies](../model/bodies.md) « Edit levels »). The outline is drawn where the
profile lies, on the bottom cap (its `z0`), whatever the view, with grips: its corners (squares;
round when free), its arcs' middles (diamonds), its straight sides' middles (bars along the side;
round when free, to insert a corner), its fillets ([Fillet grip](#fillet-grip)). The thickness is
the Stretch tool's, along the body's Z ([Bodies](../model/bodies.md)). A grip is taken by a click
and dropped by another (SketchUp's Move; a drag works too), Esc dropping the gesture; each
gesture is one `Operation::SetProfile` (one undo step), under the open part's scope:

- a corner follows the cursor, inferred with the lines through where it was and brought onto
  the cap's plane; its sides follow ([Sides keep their direction](#sides-keep-their-direction)).
  Typed: a distance along the move, or a point;
- a side's middle moves that side, or, free, inserts a corner there, which then moves as a
  corner;
- an arc's middle sets its bulge (the arc through the cursor), snapping to the half circle
  and to straight (the chord) within 10 px. Typed: its height (sagitta);
- Delete over a corner takes it away (the outline must stay simple, at least three corners
  or an arc).

An outline that would cross itself is refused, said in the status bar.

## The shape kept while editing

A profile edited grip by grip keeps the designer's shape: a box's profile stays a rectangle, a
cylinder's disc stays round, a fillet next to a moved corner stays tangent. This is done by
editing by sides with no stored constraint (SpaceClaim's, Shapr3D's direct editing) and by
keeping the designer's intent in the recipe (fillets as radii) — no parametric sketch, no
solver. Not yet: chains of arcs tangent to each other (an S) may lose their tangency.

Decision record: [Profile edits keep the shape without a solver](../decisions/0019-profile-edits-keep-shape-without-solver.md)

### Fillets are radii

`Corner { p, bulge, fillet }` — `fillet`, a radius on a sharp corner between two straight sides
(0: sharp); the outline stores the sharp corner, the arc is computed wherever the outline is used
(the body built, its box, the display; `profile::rounded`), so it follows every edit
and stays tangent. The Prism's « Congé » option records radii instead of arcs. An arc tangent to
two straight sides that are not parallel, however it was drawn (an arc tangent closed tangent, a
face taken whole), is a fillet: recognised when the profile is recorded (`profile::recognized`),
it becomes its radius on the sharp corner. A radius that no longer fits (the sides too short, the
fillets at both ends counted) leaves its corner sharp, as at creation, and comes back when there
is room again.

### Sides keep their direction

The grip of a corner moves the two sides meeting there parallel to themselves, through the
cursor's point, and the neighbouring corners slide along the sides beyond (a rectangle stays a
rectangle, a trapezoid keeps its angles; 2 degrees of freedom: inferred on the cap's plane, typed
as a distance along the move or a point). When that would move every corner (a triangle: a mere
scaling), the corner moves alone. The grip in a straight side's middle moves that side parallel
to itself, its neighbours extended (1 degree: the cursor's point taken along the side's normal,
typed as a distance). A neighbour that cannot meet the moved side any more refuses the move.

### Free moves

The copy modifier tapped (as Move's copy: before or during the gesture, the cursor carries the
word « Libre », dropped once done; also the « Libre » toggle in the option bar): a corner then
moves alone, its sides following; a side's middle inserts a corner there, which moves alone.
Built by `kernel::profile::{shift_sides, corner_moved, side_moved}`.

### Tangencies kept by rules

Read on the outline as it was when the grip was taken, no solver; free moves too
(`kernel::profile::rearced`):

- a fillet ([Fillets are radii](#fillets-are-radii));
- a half circle tangent to two parallel sides (a rounded end, a slot's end) stays tangent, its
  radius half the gap (its still end slides along its side to face the moved one);
- a whole disc (a cylinder's) has one grip, its radius (typed too; `profile::circle`);
- an arc tangent to a straight neighbour keeps that tangency: its ends go where their sides take
  them, and the one arc leaving that side along it to its other end is taken (tangent at both
  ends to sides that are not parallel, the start's tangency wins, unless only its end moved);
- **an arc tangent to nothing keeps its sagitta** (the rise over its chord, the dimension a
  bent rail is given): its ends follow their sides, its height stays.

### Fillet grip

Hovering a sharp corner between two straight sides shows a small grip on its bisector, 18 px
inside; dragging it rounds the corner, the arc through the cursor's point on the bisector (typed:
the radius). On a fillet, the grip sits at the arc's middle, always shown; brought back onto the
corner (10 px) it makes it sharp again, as Delete over it does. Dragged, the radius stops where a
side runs out (its other end's fillet kept); typed too big, it is refused, said
(`profile::{fillet_middle, fillet_through, fillet_max}`).

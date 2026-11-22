---
status: built
code: [crates/tools/src/infer/mod.rs, crates/tools/src/infer/points.rs, crates/tools/src/infer/lines.rs, crates/tools/src/infer/crossings.rs, crates/tools/src/infer/curves.rs, crates/kernel/src/snaps.rs, crates/tools/src/tools/mod.rs, crates/tools/src/editor/mod.rs, crates/tools/src/tools/shapes.rs]
---

# Inferences

The point a tool takes under the cursor, as SketchUp infers it: endpoints, midpoints, thirds, centres,
lines along the axes or an edge, their crossings, points tracked, with markers, guide lines and
a tooltip. One engine serves every tool. This page also holds the shape tools' drawing plane,
which is built on it.

## The engine

**A central engine** (`tools/src/infer/`, no UI, no GPU, no state) chooses the points for
every tool. A tool calls `cx.infer(px, keep, plane)`, `cx.infer_from(…)` or
`cx.infer_in_plane(…)`; the `Cx` fills a `Query` with what the editor holds and calls
`infer::infer(scene, camera, size, px, &query)`.

- `Query`: `keep` (the geometry to infer on, and that may hide others: all but what is
  moving), `plane` (fallback plane, world point and normal), `origin` (a point offered as
  `Origin`), `model_axes` (the axes are drawn: their lines through `origin` are inferred on),
  `from` (the tool's start point), `tracked` (the tracked points), `in_plane` (only lines
  lying in `plane`: the rotate tool's protractor), `axes` (the context's, in the world),
  `reference` (the last edge hovered, `EdgeRef`), `lock`, `tool_line` (a line the tool draws
  itself, the « Tournage » axis: inferred on as the axes as drawn, its origin as an endpoint,
  [Turned bodies](turned-bodies.md)).
- `Inference`: `point` (world), `kind` (`InferKind`), `path` (the solid it belongs to, if
  any), `hit` (what the ray hits, whatever was inferred: tools orient themselves on its
  face), `line` / `line2` (the line(s) the point is on: origin and unit direction; the app
  draws the guides from them), `edge` (the edge at the point, the next reference), `curve`
  (the curve `edge` is taken from, as drawn: `infer::Curve`), `crossed` (where two curves
  cross, each of them there — [Tape measure and guides](tape-measure.md)).

Decision record: [One inference engine on the exact B-rep](../decisions/0002-central-inference-engine.md).

## Exact geometry

Candidates come from the **B-rep record** (`f64` vertices, typed curves: exact midpoint of a
line, exact centre of a circle), not from the mesh. A `kernel::Snaps` per solid (its vertices,
its visible edges with their `SnapCurve` — segment, arc, or polyline —, each planar face's
plane) is built with the mesh by `Solid::display()` and cached per `Arc<Solid>` in the scene
like the meshes; it is transformed by each placement. Fallback on the mesher's polyline for
splines and intersection curves (`Mesh::edges` keeps the record's edge index). **Seams** (an
edge between a face and itself, or between two faces of one surface) are recognised on the
record and left out (the mesher does not fill `seams`).

## Priorities

`InferKind::rank`; at equal rank, the closest to the cursor on screen within `APERTURE_PX` =
10 px, then the nearest to the eye:

1. endpoint (`Endpoint`);
2. midpoint / middle of the tracked points / centre / origin (`Midpoint`, `TrackedMiddle`,
   `Center`, `Origin`);
3. third of an edge (`Third`) or constructed point: a line × an edge (`OnEdgeAndLine`), two lines of different origins
   (`OnTwoLines`), a line × a guide circle, a guide circle × an edge, two guide circles
   (`OnGuideAndLine`, `OnGuideAndEdge`, `OnTwoGuides`, [Tape measure and guides](tape-measure.md));
4. on a line (`OnLine`: axes from `from` or from a tracked point, parallel / perpendicular to
   an edge, tangent / radial to an arc, a guide line, an axis as drawn) or on a guide circle
   (`OnGuide`);
5. on edge (`OnEdge`);
6. on face (`OnFace`; projected exactly if the face is planar);
7. fallback plane (`OnPlane`).

A lock comes before everything: the point stays on its line. Points of ranks 1–2 and thirds
can be tracked (`InferKind::is_trackable`), but for the middle of the tracked points; ranks 1–5 are features a tool takes as they are, even off its
working plane (`is_feature`).

## Occlusion

A vertex or an edge behind the surface the ray hits is not a candidate (up to a tolerance);
the edges of the hovered face stay reachable. Inference covers the whole visible scene,
including outside the active context.

## Lines

The linear kinds are grouped in `LineKind`: `Axis(k)` (the context's axis through the start
point), `TrackedAxis(k)` (through a tracked point), `Parallel`, `Perpendicular` (to the last
straight edge hovered, in the tool's plane), `Tangent`, `Radial` (to the last arc hovered,
where it was hovered), `Guide`, `ModelAxis(k)` (the axes as drawn, through their origin) and
`Tool` (the tool's own line).

- Through the tool's start point: the context's axes and, for the straight edge hovered last,
  parallel and perpendicular (in the tool's plane). A line almost aligned with the view
  direction is ignored (it is only a point on screen). If it pierces the face under the cursor
  near it, that point is taken.
- **Tangent to arcs**: a point on an arc (or its midpoint) becomes the reference
  `EdgeRef::Arc { tangent, radial, center, radius }` at the exact point (centre and radius for
  the tape's concentric guides); a straight edge, `EdgeRef::Straight`. Hence "Tangent to arc"
  (parallel to the tangent through the start point) and "Perpendicular to arc" (towards the
  centre). A tangent or a radius that coincides with an axis is announced as the axis.
- The guides and the axes as drawn are lines of the scene (`LineKind::is_fixed`): drawn
  depth-tested, so not inferred where hidden, and an edge to refer to
  ([Tape measure and guides](tape-measure.md)).

## Crossings

- **Line × edge**: `InferKind::OnEdgeAndLine(l)` where a line crosses an edge (segment, arc —
  in its plane or through it —, polyline), within half a pixel brought to depth
  (`CROSSING_PX`); the point is taken on the line. Under a lock, the edge crossed near the
  cursor gives that crossing before any projection. Marker: the edge's square, in the line's
  colour; tooltip "Edge × red axis"…
- **Line × line**: two lines of different origins crossing within half a pixel give
  `InferKind::OnTwoLines(a, b)`, the point taken on the first (the start point's, if any).
  Strict 3D crossing: two lines at different heights do not cross, even when they overlap on
  screen.
- **Line × face**: a line piercing the face under the cursor near it (above).

**Why:** two 3D lines seldom meet exactly, hence the tolerance of a fraction of a pixel brought
to depth, and the point kept on one line so it stays exactly on it.

## Point tracking

A point of the geometry (endpoint, midpoint, third, centre, origin) hovered for `TRACK_AFTER` = 0.5 s
becomes a **temporary reference**, with no click or key, as in SketchUp. The editor gets the
time from the app (`Editor::tick`, called every frame; it asks for a repaint while a point is
waiting). `MAX_TRACKED` = 2 are kept, the most recent; each stays marked on screen with a
ring. Their axes are `LineKind::TrackedAxis(k)` lines ("From point, on red axis"), whose guides
are dashed from the reference. Tracked points drop with the locks. When a rotation's preview
starts, only the latest stays, so that the older one and their middle leave the view.

With two tracked points, **their middle** is a point to infer (`TrackedMiddle`), marked by a
small ring. It is offered even inside a solid (the centre of a box from two opposite corners)
and is not tracked itself: it would replace one of the two points it stands on.

## Thirds

Straight edges and open arcs offer their points at one and two thirds of their length
(`SnapCurve::thirds`, exact), marked by a yellow triangle. They are offered within
`THIRD_APERTURE_PX` = 6 px, and only when the two thirds are at least `2 × APERTURE_PX` apart on
screen.

**Why:** thirds must not take the edge away: with the midpoint's rank and aperture they would
hide nearby crossings and leave little of a short edge to click on (the tape's offset from an
edge starts there).

## Selection box

Move and Rotate, once what they act on is taken, infer on its box (`tools/box_grips.rs`): one
part or body in its own axes, several in one box in the context's axes, drawn dotted.

- **Its 8 corners** are points to start from (`InferKind::BoxCorner`, a green square, ranked
  with the midpoints): Move's start point, Rotate's centre. A real endpoint there comes first.
- **The face of the box under the cursor**, unless something else stands before it, is outlined
  and offers **4 grips**, curved double arrows (their curve fixed on screen), on its two middle lines, `GRIP_INSET_PX` = 28 px inside its sides (at
  most half way to its centre; those too near the centre on screen are not offered). A click on
  a grip goes straight to the angle, in both tools: centre at the face's centre, axis along its
  normal, reference at the grip. Move hands the rotation to a `RotateTool`, so that both turn
  alike (angle typed, copy, arrays). An axis locked by an arrow key turns the grips off.
- **Rotate also takes the face's centre**, its normal as the axis, then goes on as usual
  (reference, angle).

A grip is taken within `GRIP_HIT_PX` = 8 px of its arrow's curve, and shows the protractor it
would turn with as soon as it is aimed at, its zero at the grip. A point of the geometry keeps
the cursor unless the cursor is nearer to the arrow.

## Locks

Held by the editor for every tool: arrows → / ← / ↑ = red / green / blue axis, ↓ = free;
holding Shift locks the current linear inference. The lock (`Lock`) remembers its line's
origin: Shift also holds a line from a tracked point. A point hovered during a lock is
projected onto the line (alignment). Locks drop on every applied operation, on Esc, on a
change of tool and on undo.

Depending on the tool, the axis lock gives the direction (Move), the rotation axis (Rotate) or
the normal of the drawing plane (Box, Cylinder: ↑ horizontal, → perpendicular to red, ← to
green, reversed so that the plane's second side points up). For the shape tools
(`Tool::down_locks_face`), ↓ locks the normal of the face under the cursor instead
(`face_lock`, taken when pressed); the same face again, or none, frees it (after
OpenCutList's smart draw tool).

## Visual feedback

Guide lines (axis colour), drawn by the editor with the other lines
([Drawing helpers](../render/drawer.md)); markers coloured by kind and a tooltip ("Endpoint",
"Midpoint", "On red axis"…: [Texts and languages](../ui/texts.md)) painted by egui over the
viewport (`camera.project`, `paint_inference`): no renderer work.

A marker's shape tells what the point is, and violet is kept for the temporary points:

- of the geometry, filled: endpoint (green disc), midpoint (cyan disc), third (yellow triangle),
  centre (pink disc with a small cross), origin (orange disc on a wider cross);
- temporary, violet rings on a light edge: a tracked point (wide enough to surround the marker
  of the point hovered there), the middle of two tracked points (a small ring; aimed at, a wide
  ring around a dot, with a dashed line between the two points).

## Lines seen through the model

**The move's line** (start point → cursor) and **the tape's measure** (or its offset) are seen
through what lies between — plain where they are seen, translucent dashes where faces hide them
(`Pen::x_ray`: the scene layer with its hidden part shown, [Drawing helpers](../render/drawer.md));
**the rotation's radii**, as every dimension's line, are dashed, translucent where hidden
(`Pen::dimension`, [Dimensions](../render/drawer.md#dimensions)); the protractor stays over
everything, as do the fine dotted lines joining a radius's end to the point inferred off the
protractor's plane that it is the projection of (the reference's stays while the angle is
chosen). Move writes its distance at the middle of its line,
Rotate its angle on the bisector of its radii, just past the protractor (`PreviewLabel`, as
Stretch). Move's line and the tape's are arrows, their head at the cursor (the destination,
the point measured to, the guide's side): heads `ARROW_PX` long on screen, in the plane facing
the eye (`Drawer::arrow_heads`), drawn over everything. The renderer draws such a line plain
with the depth-tested lines (depth written), then dashed with the depth test reversed
(`CompareFunction::Less`, the depth being reversed: what something nearer hides), blended at
`HIDDEN_ALPHA` of its colour, no depth written; where it was drawn plain, its own depth keeps the dashes off.

## Drawing plane

The rules in `tools/shapes.rs`, after OpenCutList's `SmartDrawShapeActionHandler`.

- **First click**: it takes the plane of the face hovered (on an edge, the face it bounds,
  else the ground or the upright plane through the edge), otherwise the context's ground; a
  straight edge askew to the axes there turns the base's sides along it (a guide line
  likewise, taken as an edge; a guide circle by its tangent there, in its plane off a face).
- **The box's second corner** sets the plane again: an axis plane through the first corner it
  lies in, the plane through an edge aimed at and the first corner, the plane of a face the
  first corner lies on, else the upright plane through both corners; a guide aimed at (a line,
  a circle by its tangent there) acts as an edge and always turns the base's sides along it
  (even locked plane). The cylinder keeps the plane of its centre.
- The height runs along the normal, towards the eye. Esc goes back one step.
- **Before the first click** a small shape at the cursor shows the plane (OCL's
  `_preview_shape_start`); with an offset or a fillet it is drawn dashed and the shape made
  solid, offset out or in by a tenth of its width and rounded: the signs, not the sizes.
- The lines take the colour of its normal's axis (magenta for another locked one) and the
  sides being drawn get their lengths written (`Drawing::lengths`, painted by egui like the
  tooltip, last, above everything). A length is written at the middle of its side; when that
  covers the cursor, a marker, the inference tooltip, a cross (`Drawing::marks`, a centre or a
  start, drawn by the app as a flat `+`) or a length placed before, it slides along its side,
  then off it on either side (the first spot free, else the middle).
- **Box**: the opposite corner snaps to the square's within 10 px (not on a feature, nor on a
  plane seen edge on), its diagonal then drawn; its saved option "Centred" (the bar, or a tap
  of the copy modifier, through `Command::SetOption`) takes the first click as the centre of
  the base, typed lengths being the whole ones.
- **Cylinder**: the radius snaps likewise to the red, green or blue axis through the centre,
  and is dashed in that axis's colour when along it. Its saved option "Measure" (Radius or
  Diameter, Tab to turn it over) sets the length shown and typed for the base; the diameter is
  drawn across, through the centre.

### First point and height

- The copy modifier held over a face takes its centroid and, with Shift tapped, the whole face
  as the base (OpenCutList does this for its polygon only; here the box takes the rectangle
  around the face along its longest side, the cylinder a disc's circle); the other modifier
  tapped (Command on macOS, Alt elsewhere) measures it from a corner, typed as `d` (along an
  edge) or `d1;d2` (across a face).
- The height: Shift held, or a double-click on the opposite corner, takes the last height
  again (kept by the editor across tools); the copy modifier held runs it to the first face
  along the normal from the middle of the base; tapped, it centres the height ("Centred
  height", saved).
- The app tells a modifier tapped alone (`Key::Copy`, `Key::Shift`, `Key::Alt`) when nothing
  else, nor another modifier pressed, came in between, and tells the copy modifier held every
  frame (`Editor::set_copy`), as Shift (`Editor::set_shift`).

### Shape offset

After OCL's `SmartDrawShapeActionHandler`, option `shape_offset`: a saved length option of
each tool (Box and Cylinder each their own, unlike OCL where the draw actions share it), set
in the bar or by typing `<length>x`. It moves the outline only, never the height: the box's
base grows by it on every side (sharp corners), the cylinder's radius by it (in Diameter mode
too); negative, it shrinks them, and a shape left without width says so and is not made. The
lengths written and typed stay those of the shape drawn (clicked on a face's edges, typed);
while it is not zero the drawn shape is dashed and the one made is drawn solid, at every step.

### Box fillet

The Prism's « Congé » ([Prism tool and profiled blanks](prism.md)): a saved length option of
the box, after the offset, set in the bar or by typing `<radius>r`. It rounds the four corners
of the base made (offset included): the box is then made a `Primitive::Prism`, its profile the
rectangle with the radius on each corner (recorded as radii, as the Prism's). At most half the
shorter side made (a stadium; within 1 µm, the clicks' rounding); over it, refused and said,
the height step kept to change it. Previewed as the offset is.

### What a shape makes

What a shape tool makes is not selected, nor what a boolean makes: the selection stays as it
was and the next shape follows at once. `Operation::AddPrimitive` takes a rigid frame for that
(`placement: DAffine3`, identity by default in serde); the primitive is built axis-aligned
then transformed.

## Performance

Culling by bounding boxes against the picking cone, through the scene's BVH
([Display](../render/display.md#spatial-index)).

## Tests

Without display, fixed camera: each kind, priorities, occlusion, inference inside a
transformed component, scene 100 km from the origin.

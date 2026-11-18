---
status: built
code: [crates/tools/src/tools/measure.rs, crates/model/src/document.rs, crates/tools/src/guides.rs, crates/tools/src/draw.rs, crates/tools/src/scene.rs, crates/tools/src/infer/curves.rs, crates/tools/src/tools/mod.rs, crates/tools/src/tools/select.rs]
---

# Tape measure and guides

The tape measure (after SketchUp's Tape Measure tool) measures lengths and draws construction
guides — infinite lines and whole circles — that every tool then infers on. Guides are
document entities of the active context: saved, undone, selected, moved and erased like
occurrences, with no kernel.

## Guides in the document

- **Guides are document entities**: `Component::guides`, a list of `Guide { id, shape }` in the
  component's own axes, saved with the file ([File format](../io/file-format.md)) and undone like
  everything else (snapshots, [Undo / redo](../model/undo.md)). `GuideShape` is an infinite
  `Line { origin, dir }` or a whole `Circle { center, normal, radius }`.
- **They belong to the active context**: the open part when one is ([Opening a part](../model/opening-a-part.md)),
  else the assembly being edited (the model at the top). A part's guide shows in each of its
  occurrences and moves with them; changing a part's guides ignores the scope
  ([Data model](../model/data-model.md)): it changes no geometry, so no part is made separate
  for it. A guide drawn with no part open belongs to the assembly and does not follow a part
  moved later. Copying a component (separate part) copies its guides; exploding an assembly
  hands its guides to the context, placed as it was.
- **Ids**: from the document's counter, unique within their component, never equal to an
  occurrence's (assembly) or a machining's (part) of the same component: the selection holds
  them with those, and `Delete`, `Transform`, `Duplicate` take them.
- **Operation**: `AddGuide { component, shape }`, the shape in the component's axes; no kernel.

## Display

- Dashed, depth-tested lines, in the view's drawing (`Drawer::guide`, `guides::Stroke`). A line
  is drawn **as far as the view shows it** (`Camera::visible_segment`): clipped to the view's
  sides (a few pixels past them), kept off the eye in perspective, and an end running off to
  infinity stops within a quarter of a pixel of its vanishing point; seen end on in parallel
  (within 1 mrad: the standard views stop short of the zenith), it is not drawn. A circle is a
  polyline.
- The renderer dashes them **on screen** (`render::Lay::Screen`): dashes and gaps of 6 points
  (`guides::DASH_PX`), the pattern anchored on the line's origin (when in front of the eye;
  else its first point drawn) and running on along a circle's chords, so it holds still as the
  view pans. As the view orbits, the dashes slide along the guide. **Why:** laid on the guide in
  model units instead ([Display](../render/display.md#dashes)), they hold still but shrink with
  the perspective; guides look better as long on screen everywhere. Decision record:
  [Dashes laid on the segment](../decisions/0032-dashes-laid-on-the-segment.md).
- An infinite line is never inside a selection rectangle: only a crossing one takes it.
- The axes are drawn the same way, across the view (their negative halves faded).
- Those of the active context in the theme's `guide`, the others (outside it, or in another
  occurrence of the open part) faded as their parts are (`ThemeColor::FadedGuide`, blended).
- The selected guides in the selection's colour, those hovered (or in the marquee) in the
  hover's; while moving, the selected ones are drawn where they go, and where they were only
  when copying.
- `Display::guides` (on by default; View › Guides, the view bar's button, the model's
  preferences) hides them: not drawn, inferred nor picked, dropped from the selection.

## The tool

`ToolKind::Measure`, `T`, `tools/measure.rs`.

### Guide kinds

The first click takes the inferred point. With the saved option **"Create guides"** on (the
default; a tap of the copy modifier turns it over, as SketchUp's Ctrl), it starts a guide of
the kind the saved choice **"Guide"** says (Tab turns it over, also between the two clicks —
the first click is kept and starts again):

- **Auto** (the default): **on an edge** (`OnEdge`, not an endpoint nor a midpoint),
  **parallel** to a straight edge — the line through the point hovered, along the edge; the
  length shown is the distance between the two lines — or **concentric** to an arc or a circle
  — the circle in its plane through the point hovered (projected on that plane); the length
  shown is the difference of the radii; anywhere else, a measure (below);
- **Line**: parallel to a straight edge; **tangent** to an arc — a line along its tangent at
  the point clicked, offset along its radius in its plane; anywhere else, the line **through
  the point clicked and the next one** (the length between them shown, a length typed doing
  nothing);
- **Circle**: concentric to an arc; anywhere else, **centred on the point clicked** — in the
  face clicked, else level —, the cursor giving the radius (shown and typed as "Radius"): the
  concentric case of a circle of radius zero, so a guide crossing two others (a fillet's
  centre) takes its circle.

### Where curves cross

**Where curves cross** (edges, guides, the axes as drawn: `Inference::crossed`, each curve at
the point, set on `OnEdgeAndLine`, `OnGuideAndLine`, `OnTwoLines`, `OnGuideAndEdge` and
`OnTwoGuides`; not a reference for the next lines) **and at a vertex** (`Endpoint`: the edges
ending there, `SnapCurve::ends`, looked for only at a vertex near the cursor — so the end of an
arc gives its tangent), each gives its guide of the kind chosen, and the fallback above one
more. The first click takes, in Line and Circle, the one from the edge last hovered
(`Cx::reference`) when it is one of them, else the fallback; in Auto always the fallback (a
point is measured from). The other modifier tapped (Command on macOS, Alt elsewhere) turns them
over, the status bar naming the guide ("Tangent guide", "Parallel guide"…).

**Why:** the last edge hovered alone cannot choose: coming along a circle to where it crosses a
line, the cursor is within the line's aperture before the crossing's, so the line is always
last.

### The reference marked

The curve the guide refers to is drawn in magenta as it is drawn itself, from the first click
on; before it, at a point where several cross, the one the click would take. The inference
hands it over with its `EdgeRef` (`Inference::curve`, and each of `Inference::crossed`, an
`infer::Curve`): an edge, end to end, and an arc as sampled, bolder and over everything
(`Pen::bold`, `draw::BOLD`); a guide by its very strokes (`Drawer::guide`), so the magenta
dashes fall on its own; an axis as drawn, across the view, bolder in its own colour, in the
scene. **Why:** an infinite line through the point clicked hid what it was taken from, and its
dashes fell between a guide's.

### Offset and second click

The offset is drawn dashed — in the colour of the axis it runs along, else ink — from the point
clicked to the guide, square to the edge (for an arc, along the point clicked's radius,
wherever the cursor is around; for a circle centred on the point clicked, along the radius
through the cursor). Pointing at nothing, the cursor gives a point of the horizontal plane (the
axes') through the point clicked — for an upright edge, of the face clicked, else of the
upright plane through it facing the eye; the perpendicular is inferred in that plane. The
second click, or a typed length (the offset, towards the cursor's side), adds the guide. A guide
may be offset by zero (along the edge, the tangent itself, or the whole circle of an arc); a
circle shrunk to nothing adds nothing (said in the status bar).

### Measuring

**Anywhere else** in Auto, or with the option off, it measures: the length from the first point
to the point hovered, written on the dashed segment between them (`PreviewLabel`) and in the
measurements box; the second click ends the measure. A length typed then does nothing (no
scaling of the model, meaningless for cut-list parts).

Lines are inferred from the first point (axes, parallel and perpendicular to the edge clicked,
tangent and radial to the arc), so that a guide is offset square to its edge. Esc goes back to
the first click. `EdgeRef::Arc` carries the arc's centre and radius for this.

## Inferences on guides

- The display scene keeps every guide placed in the world (`DisplayScene::guides`,
  `SceneGuide`, from `Document::placed_guides`, rebuilt with the document), which the view
  draws and the engine infers on, for every tool ([Inferences](inferences.md)).
- A guide line is one more line (`LineKind::Guide`, one source each): "On the guide" (rank of
  the lines; the guide becomes the reference edge, so the tape offsets a guide from a guide),
  "Edge × guide", "Guide × guide" and crossings with the tool's lines; a point of it hidden
  behind the model is not inferred (it is drawn depth-tested), and only one lying in the
  protractor's plane counts for the rotate tool.
- A guide circle is a whole arc: "On the guide" (`InferKind::OnGuide`, its arc as the
  reference), its centre (`Center`, tracked like any) and where a line crosses it
  (`InferKind::OnGuideAndLine`), and where it crosses an edge (`InferKind::OnGuideAndEdge`,
  exact: a segment, an arc in its plane — two circles —, or across it — on the line where both
  planes meet; a polyline by its segments). Such a point built by the geometry ranks with those
  built by lines (the nearer on screen wins) and beats any point on a line; where two guide
  circles cross likewise (`InferKind::OnTwoGuides`, "Guide × guide").
- **The axes as drawn are lines too** (for every tool, as in SketchUp): while they show
  (`Display::axes`, or a part open), the context's three axes through its origin
  (`LineKind::ModelAxis`, "On red axis") — crossings with edges, guides and the tool's lines;
  not inferred where hidden; an edge to refer to, so the tape draws a guide from an axis as
  from an edge.
- A guide is an `Owner` with `Piece::Guide(id)` (its path: the component's occurrences, then
  its id): the guides being moved are left out (`Cx::is_moving`).

## Managing guides

- **Picking**: a guide of the active context within `GUIDE_PICK_PX` (5 px) of the cursor and
  not hidden by the model (`Cx::pick_guide`) comes before the solid under it, for the select
  tool's click and hover and for what Move and Rotate take with nothing selected. The marquee
  takes a guide like a solid: a window one lying wholly inside it (a circle), a crossing one
  any of it touches. Select all takes them. In an open part, its guides are selectable, its
  machinings shown or not; the part's guides are not, from outside it.
- **Operations**: no guide-specific ones beyond `AddGuide`. `Delete`, `Transform`,
  `Duplicate`, `Array` and `MakeAssembly` take guide ids of their context among their entities
  (the guides go into the new assembly; guides alone make none; `PaintOccurrences` leaves them
  out). The new assembly's axes have their origin at the low corner of its content's bounds
  (in the context's axes): the content and its guides are brought into them, the new
  occurrence placed there, so nothing moves in the context. The operations on machinings take
  the part's guides — with guides alone, no kernel and no separate part, whatever the scope. In
  an open part, Move, Rotate and Delete always aim at the part (`Cx::moved`), its machinings
  shown or not.
- Edit › "Delete guides" (`edit.delete_guides`) erases those of the active context, shown or
  not. The entity info of one guide says its shape (a circle's radius).

---
status: under way
code: [crates/model/src/drawing.rs, crates/model/src/dimensions.rs, crates/kernel/src/topo.rs, apps/skraft/src/dims.rs, crates/tools/src/hidden_lines.rs, crates/kernel/src/drawing.rs, apps/skraft/src/view_lines.rs, apps/skraft/src/sheet.rs, apps/skraft/src/viewport.rs, apps/skraft/src/panels.rs, apps/skraft/src/templates.rs, apps/skraft/src/pdf.rs, apps/skraft/src/workspace.rs, apps/skraft/src/app.rs]
---

# Drawing

The « Mise en plan » workspace ([Workspaces](workspaces.md)): sheets of paper holding views of
the model and free elements — for the workshop (a part's fabrication drawing) as for clients
(rendered views, a title block of one's own, as SketchUp users do with LayOut). Under way:
listed in debug builds only. What is not built yet is in `docs/REVIEW.md` (« Work: drawing »).

Decision record: [Drawing: free sheets in the document, for the workshop and for clients](../decisions/0048-drawing-sheets-in-the-document.md)

## The drawing in the document

The document's `drawing` (`skraft_model::Drawing`), in its `document.json`
([File format](../io/file-format.md)), changed by operations — undone as the rest, kept
whole in the history's snapshots behind an `Arc`:

- **Sheets**, in their order: a name (« Feuille n » by default), a paper (millimetres,
  landscape: A4 to A0 given), their own elements.
- **Layers**, bottom first: a layer **shared** holds its elements itself and shows them on
  every sheet (the title block); the others' elements are each sheet's. The first sheet comes
  with two: « Plan » and « Cartouche » (shared). A layer may be **hidden** (not drawn) or
  **locked** (drawn, its elements not picked). See [Layers](#layers).
- **Elements**, on a layer, drawn in order: views, texts (`{…}` fields to come), lines,
  rectangles, images (the document's assets, kept while an element shows them). Sheet
  coordinates are paper millimetres from the bottom left corner, y up.
- **The projection** (`Projection`): where projected views go — first-angle (European) by
  default, third-angle.

Operations: `AddSheet` (A3 from the app), `AddSheetFrom` (a template, [Templates](#templates)),
`AddLayer`, `SetLayer` (name, hidden, locked), `ShareLayer`, `DeleteLayer`, `MoveLayer`,
`MoveToLayer`, `SetSheet`, `DeleteSheet` (and the views projected
from its views, on other sheets), `MoveSheet`, `AddElement` (on the first layer not shared
unless one is named), `SetElement` (same kind; consecutive changes of one element merge in the
history), `DeleteElements` (and their projections), `SetProjection`. Refused: a paper outside
1 mm – 10 m, an element not finite or of scale not positive, a view of a component gone, a view
projected from itself or from one of its projections, or from a perspective view, the last
layer not shared deleted (`LastLayer`). A file whose
drawing has an id taken or above `next_id`, an element on no layer (or a shared layer's on a
sheet), an image missing or a view that does not resolve is refused (`FileError::BadDrawing`).

## Views

A view (`View`) has a name, a style and a kind:

- **A base view** shows a **subject** — the model, a component in its own axes, or occurrences
  where they stand — from a **look** — a standard direction (front, back, left, right, top,
  bottom, iso; as the 3D view's, Z up) or any direction, parallel or in perspective (the
  camera's) —, at a **scale** (paper over model), the centre of its subject's box seen there
  drawn at `at`.
- **A projected view** takes a base view (perhaps projected itself) and a side (left, right,
  above, below): its base's subject and scale, turned a quarter towards that side as the
  projection says, aligned with it at a distance; moving the base moves it.
  `Drawing::resolve` gives a view's subject, frame (`Frame`: right, up, forward), scale and
  place, following its bases.

What a view shows: `Document::subject_parts` (the parts placed under it, in its axes, the
hidden ones left out). Every view shows its **frame**: its subject's exact box seen in its
frame, at its scale, dashed (solid when picked), named with its scale (« Vue 1 · 1:10 »). Its
**style** says what fills it:

- **Lines** « Traits »: its lines ([Lines](#lines)), the visible ones bold, the hidden ones
  thin and dashed.
- **Rendered** « Rendu »: a picture of the 3D renderer ([Rendered views](#rendered-views)).
- **Hybrid** « Hybride »: the picture, its visible lines over it.

What a view shows — its parts, its box, what its lines are computed from — is kept per view
for the history's revision and the meshes' generation (`sheet::ViewsCache`): a sheet drawn
again computes none of it.

## Lines

A view's lines are its parts' edges and the silhouettes of their curved faces, seen parallel
along its frame, split where faces hide them, visible or hidden; polylines in its frame, in
model units (`skraft_tools::hidden_lines::ViewLines`). They are computed off the UI thread (a
thread each; on the web on the UI thread) and kept per view (`view_lines::LinesCache`), asked
for by what they depend on — the frame's axes, the parts' meshes and places, the part's shape —
so that moving a view or changing its scale computes nothing; the last ones are shown while
new ones are on their way.

- **A view of one part** (native): SolidCore's exact hidden lines (`skraft_kernel::drawing::
  part_lines`, `solidcore_ops::hlr`: lines and arcs exact, curves sampled; a mirrored
  placement applied to the solid first; an uncertain piece drawn hidden; hidden pieces lying
  under visible ones left out). Refused or failed: the meshes'.
- **Else, on the display meshes** (`hidden_lines::hidden_lines`): every part hiding the others.
  The occluders are the front-facing triangles (edge-on ones, by their cosine, left out), sorted
  nearest first into a grid over the sheet; a segment — a B-rep edge's polyline piece, or a
  silhouette: an edge inside one B-rep face between triangles facing either way — tests the
  occluders of each cell it crosses in that order, until one lies behind it or it is hidden
  whole. The occluders are grown a little (no cracks between neighbours), a segment within
  1e-4 of the view's size of a face's plane is not behind it, and pieces shorter than that
  take their neighbours' look (no specks at corners). Pieces of one edge join into polylines.
  On Cruise (2 789 parts, 3.97 M triangles; `bench_hidden_lines`): 0.4 to 1.2 s a view on every
  core.

The lines are drawn into the view's picture ([Rendered views](#rendered-views)) by the
renderer, over everything (its overlay): the visible ones 0.35 mm wide on paper, nearly black,
the hidden ones 0.18 mm, grey, dashed 2 mm / 1 mm (on screen, from each segment's start), at the
picture's points per paper millimetre.

## Rendered views

Every view is drawn by the 3D view's renderer, its meshes and
palette (materials, textures) shared, into a picture of its own: its subject's parts at their
places in its axes, seen parallel along its frame, framed exactly on its subject's box — the
box its frame shows on the sheet —, on the paper's white, with the view's edges and
silhouettes (`Viewport::sheet_pictures`, the « Composants » thumbnails' way). The sheet asks
for the pictures of the views it shows (`sheet::Wanted`), each the size of its frame on screen
in pixels, by steps of a fifth (a zoom redraws it past a step), 4 096 at most — its parts for
the *Rendered* and *Hybrid* styles, its lines for *Lines* and *Hybrid*; a picture is drawn
again when its direction, its parts and places, its lines or its size change, within
40 ms a frame (the others the next), shown from the next frame. A perspective view is drawn
parallel for now.

## The stage

`Stage::Sheet`: the sheet shown over the whole central area, on its white paper with a
shadow, the stage's ground the view's colour. Framed whole below the ribbon when first shown
(and by zoom extents, or a double click); a drag with the middle or right button pans it (the
first button's drag never does: it selects, moves or draws), the wheel (or a pinch) zooms
about the pointer. The tools' cards, the
view bar and what else belongs to the 3D view are not shown; the ribbon and the panels are
(the entity info alone at first). Under the sheet, its **tabs**: one per sheet, the one shown
lit, a click shows another, a right click offers to delete it; « + » adds one. A document
without a sheet says how to make one. The sheet shown is a view state (`SheetState`), not
saved.

## Placing views

- **Selecting** (the sheet's Select tool, as the 3D view's, docs/tools/interaction.md#selection):
  a click on an element picks it (the last drawn under the pointer), elsewhere on the sheet
  none; Shift+click adds or removes it; a drag off the elements draws a **selection
  rectangle** — left to right (solid) the elements it holds whole, right to left (dashed)
  those it touches, Shift adding or removing them; Esc clears the selection
  (`SheetState::selected`, not saved). A view's frame picked is solid, another element's box
  outlined. « Effacer » (`edit.erase`: the ribbon's Selection group, the Edit menu, Delete or
  Backspace — on a sheet the keys go to it, not to the 3D tool) deletes the selection (one
  `DeleteElements`, undone at once), a view with the views projected from it and its
  dimensions.
- **Tools**: Select (`tool.select`, Space) is the sheet's own tool while the sheet is shown,
  checked while no other runs; Esc goes back one step of a tool (a shape, a rectangle, the
  picks) and does not end it — Space does (a text being typed keeps its spaces: Esc leaves
  the field first). The status bar says what the sheet's tool does at its step
  (`SheetState::status`: selecting, a window or crossing rectangle, moving; each tool's
  clicks; a dimension's picks, then placing it), without the measurements box: nothing is
  typed on a sheet.
- **Moving**: a drag from an element picked moves the selection (from one not picked: it
  alone, or added with Shift), drawn where it goes and changed once let go (one `SetElement`,
  a `Batch` of them for several): a base view anywhere, a projected view along its side only —
  it stays aligned (its distance to its base changes); what a view carries (its projections,
  its dimensions) goes with it, not moved twice.
- **Projected views** (`drawing.project_view`, « Vue projetée », the view picked a parallel
  one): the pointer off the base view's frame shows, dashed, the frame of the view projected
  to the side it is on — the side where it is the farthest out, the frame's half sizes
  weighing —, centred where it is along that side; a click puts it there (named « Vue n », in
  its base's style, picked) and the tool goes on (`SheetTool`).

## Layers

LayOut's: what is drawn goes on the **active layer** (`SheetState::active_layer`: the one
chosen if it is still there, shown and not locked, else the first such one not shared; not
saved). A layer added (`AddLayer`) goes on top, not shared, « Calque n ». **Sharing** a layer
(`ShareLayer`) makes its elements on the sheet shown every sheet's, those of the other sheets on
it deleted; **unsharing** copies them onto every sheet (the first sheet keeping their ids, a
copied view projected from a copied one following it). A layer deleted takes its elements
(views with their projections); the last one not shared stays. `MoveToLayer` moves elements
onto another layer: onto a shared one, every sheet's; off one, onto the sheet shown.

The **« Calques » panel** (`PanelId::Layers`, `view.layers`, `panels::layers`): the drawing's
layers are not a sheet's, so they have a panel of their own, shown with the entity info in
« Mise en plan » (its `Workspace::panels`; a layout arranged before it existed gets it shown
too) and offered on a sheet only; on a sheet the entity info fills its column, the layers
keeping their height (`PanelId::fills_on_sheet`). A row per layer, top first: the active one (a radio), its
name (edited), then icon buttons — shared on every sheet (lit when it is), shown (an eye),
locked (a padlock), up, down, deleted. « Nouveau calque » below. The side panels stop above the
sheets' tabs.

## Free elements

Drawn with the ribbon's « Dessiner » group on the active layer (`SheetTool`): **Texte**
(`drawing.tool_text`: a click puts « Texte » there, picked, written in the entity info; the tool
then back to Select), **Rectangle** and **Trait** (`drawing.tool_rect`, `drawing.tool_line`: two
clicks — its corners, its ends — or a drag, as the 3D view's shapes; previewed from the first
point; a second point under 0.5 mm from it waits for another; Esc drops it; the tool goes on). Every element is picked by a
click (its box: a text's laid out words, a line's ends 3 points around), moved by a drag
(`SetElement` once let go), deleted by « Effacer »; a hidden layer's are not drawn, a locked
one's not picked. Texts are drawn at their size in paper millimetres, lines and rectangles
0.25 mm wide; images as their frame for now.

## Handles

The element picked alone shows its **handles** (white squares, 7 points, with Select; none on
a locked layer's), as a vector drawing tool's; the pointer over one shows what it does (a
resize or move cursor), a drag from it (within 6 points; before the element's own drag)
reshapes the element, previewed, changed once let go (one `SetElement`); Esc drops it
(`Grip`, `Handle`, `regrip`):

- **Rectangle, image**: its 4 corners (two sides) and 4 side middles (one side); it stays a
  rectangle, turned over past the opposite side; Shift on a corner keeps its proportions, the
  opposite corner fixed.
- **Line**: its two ends.
- **Base view**: its frame's corners give its scale — as far out from its centre as the
  pointer, the nearest standard scale —, its centre kept (a projected view takes its base's).
- **Text**: its box's top right gives its height (its baseline kept; 0.1 mm steps, 0.5 to
  200 mm).
- **Dimension**: a linear one's line middle, a radius's or diameter's text point, an angle's arc
  middle (`DimShape::grip`): moved as its drag does.

Free points (polylines, polygons) will come with a tool of their own. Typed values are to
come (docs/REVIEW.md).

## Snapping

Where the pointer aims while a rectangle, a line or a text is put, or a box's or a line's handle
dragged (`Aim`, `aim_for`), as the 3D view's inferences:

- **on a point** within 8 points: the rectangles' and images' corners, the lines' ends, the
  views' frame corners and their parts' vertices (`ViewData::snaps`); the element dragged left
  out; circled in orange;
- else **aligned**, each axis apart, with the nearest such point within 8 points across it (the
  views' vertices left out: so many would align everywhere), or with where the shape started
  (a line's other end for its handle): a dotted line to that point, red along the sheet's x,
  green along its y, as the 3D axes;
- else **on the grid**, the axis no alignment took: the sheet's grid (`Sheet::grid`, paper
  millimetres; « Grille » in the entity info: none, 1, 2, 5 or 10 mm; none at first; carried by
  templates), drawn faint on the paper while its lines are 6 points apart at least, never
  printed.
- **Shift** keeps a line horizontal or vertical from where it started (the nearer); on a box's
  corner it keeps its proportions ([Handles](#handles)).

Moving elements and the dimension tools (their own snapping on the views' vertices and edges)
do not snap this way.

## Fields

A text's `{…}` are filled when drawn (`sheet::Fields`): `{document}` (the document's name, its
file's), `{date}` (today, day/month/year), `{sheet}` (the sheet's name), `{sheet.number}`,
`{sheet.count}`, `{scale}` (the sheet's first base view's); an unknown one is left as it is.
The entity info's « Insérer un champ » menu puts one at the text's end.

## Templates

A **template** is a sheet's paper and layers — the shared ones with their elements but views,
their images (`Document::template_of`) — in a file of its own: the `.skraft` container,
`content: "template"` ([File format](../io/file-format.md)), `.skrafttpl`, refused as a model
(`FileError::TemplateOnly`). The library's are in `<config>/Skraft/templates` (native):
« Enregistrer comme modèle » (`drawing.save_template`, also a sheet tab's right click) writes
the sheet shown there under its name; a `.skrafttpl` opened is put there and a sheet made from it
in the document shown. Skraft comes with one (`Document::default_template`, « A3 avec
cartouche »): a frame 10 mm in and a title block at the bottom right — the document's name, the
sheet's, the date, the scale, « Feuille n / N » —, on the shared « Cartouche ».

The sheet tabs' « + » offers a blank sheet, then the templates (Skraft's first, the library's
by name). A sheet made from a template (`AddSheetFrom`, « Feuille n ») takes its paper; each of
its layers is joined to the document's of the same name and sharing — a shared one still empty
filled with the template's elements, so a title block is never doubled — or added with them;
their images taken in; a layer not shared made if none is left to draw on.

## Dimensions

A dimension (`skraft_model::Dimension`, an element) measures the model **through a view**
(`view`): it goes with it (deleted with it, `Drawing::projections_of`) and is drawn where the
view shows what it measures. Its kinds (`DimKind`):

- **Linear** between two vertices: along the sheet's x (horizontal), y (vertical) or between
  them (aligned — their distance on the view's plane), its line `offset` millimetres of paper
  off them; extension lines 1 mm off the points and 1.5 mm past the line, filled arrows
  (2.5 × 1.2 mm), the value above the line (the document's units, without their symbol).
- **Radius** or **diameter** of a circular edge seen face on: a line from the centre (across
  for a diameter) to the text at `at` from the centre, its arrow on the circle; « R 20 »,
  « Ø 40 ».
- **Angle** between two straight edges as the view shows them: an arc about where they meet
  through `at`, its arrows on the edges' lines (extended to it), the value in degrees.

**Anchors** (`Anchor`): a vertex or an edge of a part placed in the view's subject — its chain
of occurrences from the subject, its part —, with **the names of the faces around it** when the
part's shape has names (`part_names`: a single positive body unplaced, a leaf named by its
recipe — a profile's sides and caps — or a node by its map; decision 0042) and the point
picked, in the part's axes. Found again (`Document::find`) as the model is now: among the
vertices or edges carrying those names (each face's tags met by one of the candidate's
faces), the one nearest the point — so a dimension follows its corner through a stretch —;
without names, the nearest of its kind within a hundredth of the part's size plus 0.1 mm.
Not found (the part gone, a name lost, nothing near): an **orphan**, drawn in red where its
point was, its value « ? », the entity info saying so; it is deleted and drawn again (no
reattaching yet). The topology is read from the record (`skraft_kernel::topo`: vertices,
the faces around a vertex or an edge, an edge's line or circle), no kernel.

**Tools** (the ribbon's « Coter » group, `SheetTool::Linear` / `Radius` / `Diameter` /
`Angle`): the pointer snaps, within 8 points, on a view's vertices (linear) or edges (a circle
face on; a straight edge), marked in orange (each view's vertices and edges on the sheet made
once, `ViewData::snaps`); a click picks it — the second pick in the first's view —; then the
dimension follows the pointer and a click places it on the active layer, picked, the tool
going on. A linear dimension is horizontal when the pointer is past the points' span along y,
vertical past it along x, aligned within it. Esc drops the picks. A
dimension is moved by a drag (a linear one's line across itself, the others' text or arc) and
deleted as any element. The entity info shows its kind and value, or « Orpheline ». In the
PDF: its lines 0.18 mm, its arrows filled, its text turned with it (`sheet::dim_shape`, the
shapes `dims::shape` gives the sheet and the PDF alike).

## PDF

« Exporter en PDF » (`drawing.export_pdf`, the ribbon's « Feuille » group and the File menu)
writes the document's drawing as a PDF (`pdf::drawing_pdf`, with `krilla`), then asks where to
save it (a download on the web), named after the document. It is done at the next frame, where
the GPU is at hand (`App::pdf_asked`). Once written, its toast offers « Ouvrir »: the system's
application for PDFs opens it (`Act::Open`: `open`, `explorer`, `xdg-open`; desktop only).

- **A page per sheet**, in their order, at its paper's size (points: 72 a 25.4 mm), the
  sheet's millimetres (y up) turned into the page's (y down); a hidden layer's elements left
  out.
- **Views**: their picture (*Rendered*, *Hybrid*) drawn now by the 3D renderer at 200 dpi
  (`Viewport::picture_rgba`: a picture of the frame's size at that resolution, read back from
  the GPU), placed on the frame; their lines (*Lines*, *Hybrid*) computed now
  (`sheet::printed`, `LinesJob::run`) and drawn as vectors, each kind one path: the visible ones
  0.35 mm, nearly black; the hidden ones (*Lines*) 0.18 mm, grey, dashed 2 / 1 mm; round caps
  and joins.
- **Texts**: real text (selectable, searchable), in the app's font (IBM Plex Sans, embedded,
  subset), their fields filled; each line from the bottom up, as on the sheet.
- **Lines and rectangles**: 0.25 mm. **Images**: the asset as it is (PNG or JPEG), stretched
  over their rectangle.

## Entity info

In « Mise en plan », the entity info panel shows (`panels::drawing_info`):

- **the view picked**: its type (view, projected view), name (edited), what it shows (the
  model, a component's name, n occurrences); a base view its direction — the standard ones, or
  « Caméra »: the 3D view's camera now, perspective or parallel — and its scale (the standard
  scales); a projected view its base and side, and its scale (its base's); its style;
- **a free element picked**: its type, its layer (a choice: `MoveToLayer`); a text its words
  (a field of several lines, applied when left; « Insérer un champ ») and size (millimetres);
- every element its layer too; several picked, how many;
- **else the sheet**: its name (edited), its size (A4 to A0) and orientation (landscape,
  portrait), the document's projection (first-angle « Européenne », third-angle
  « Américaine »), its grid ([Snapping](#snapping)). The layers have their own panel
  ([Layers](#layers)).

Each change is one operation (`SetElement`, `SetSheet`, `SetProjection`).

## Ribbon

The fixed group: Select and Erase (`edit.erase`). One tab, « Feuille »: « Nouvelle feuille » (`drawing.new_sheet`,
A3, shown) and « Nouvelle vue » (`drawing.add_view`: the front of the part open, else of the
one occurrence selected — its component in its own axes —, else of the occurrences selected,
else of the model; at the largest standard scale (10:1 to 1:5000) fitting half the paper, at
its centre), « Vue projetée » (`drawing.project_view`, checked while its tool runs); the group
« Style »: « Traits », « Rendu », « Hybride » (`drawing.style_lines`,
`_rendered`, `_hybrid`: the views picked, checked when they all have it; `SetElement`, a
`Batch` for several); zoom extents.

## Driving

Handles ([Control protocol](../control-protocol.md)): `sheet` (the stage), `sheet_view:<id>`
(a view's frame), `sheet_element:<id>` (another element's box), `sheet_tab:<id>`, `sheet_add`
and its menu's `sheet_add:blank`, `sheet_add:<n>` (the n-th template), `drawing_insert_field`
and its menu's `drawing_field:<field>`. `model.document` shows the drawing.

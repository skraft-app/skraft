---
status: under way
code: [crates/model/src/drawing.rs, crates/model/src/drawing/ops.rs, crates/model/src/drawing/template.rs, crates/model/src/drawing/view.rs, crates/model/src/dimensions.rs, crates/kernel/src/topo.rs, apps/skraft/src/dims.rs, crates/tools/src/hidden_lines.rs, crates/kernel/src/drawing.rs, apps/skraft/src/view_lines.rs, apps/skraft/src/sheet.rs, apps/skraft/src/sheet_board.rs, apps/skraft/src/sheet_tree.rs, apps/skraft/src/sheet_library.rs, apps/skraft/src/viewport.rs, apps/skraft/src/panels.rs, apps/skraft/src/templates.rs, apps/skraft/src/pdf.rs, apps/skraft/src/workspace.rs, apps/skraft/src/app.rs]
---

# Drawing

The « Mise en plan » workspace ([Workspaces](workspaces.md)): sheets of paper holding views of the
model and free elements — for the workshop (a part's fabrication drawing) as for clients (rendered
views, a title block of one's own, as SketchUp users do with LayOut).

Decision records: [Drawing: free sheets in the document, for the workshop and for clients](../decisions/0048-drawing-sheets-in-the-document.md),
[Drawing: a tree of presentations, sheets and definitions, no layers](../decisions/0049-drawing-a-tree-of-presentations-and-definitions.md),
[Drawing: every sheet on one board, one of them active](../decisions/0050-drawing-every-sheet-on-one-board.md)

## The drawing in the document

The document's `drawing` (`skraft_model::Drawing`), in its `document.json`
([File format](../io/file-format.md)), changed by operations — undone as the rest, kept
whole in the history's snapshots behind an `Arc`. A tree, as the 3D document's:

```
drawing
├─ compositions        : Id → Composition { name, named, elements, source }
└─ presentations : { name, projection, props }
   └─ sheets     : { name, paper, grid, border, props }
      └─ elements: { hidden, locked, kind }
         kind = View | Text | Line | Rect | Circle | Image | Dimension
              | Instance { composition, place, props }
```

- **Presentations** ([Presentations](#presentations)), in their order, hold their **sheets**: a
  name (« Feuille n » by default), a paper (millimetres, landscape: A4 to A0 given), a grid, a
  border, their elements.
- **Elements**, drawn in their container's order, bottom first: views, texts, lines,
  rectangles, images (the document's assets, kept while an element shows them), dimensions,
  **instances** of a composition ([Compositions](#compositions)). Coordinates are paper millimetres in their
  container's axes: a sheet's from its bottom left corner, y up. An element may be **hidden**
  (not drawn, nor what it holds) or **locked** (drawn, not picked).

Operations: `AddPresentation` (with a first sheet if a paper is given), `AddPresentationFrom` (a template, [Templates](#templates)),
`SetPresentation`, `DeletePresentation`, `MovePresentation`; `AddSheet` (to a presentation, else
the first, made if there is none), `AddSheetFrom`, `SetSheet`, `DeleteSheet`, `CopySheet`, `MoveSheet` (within
its presentation or to another); `AddElement` (on top of a sheet or a composition), `SetElement` (same
kind; consecutive changes of one element merge in the history), `DeleteElements` (and what hangs
on them), `SetElementState` (hidden, locked), `MoveElement` (its order); `MakeComposition`,
`ExplodeInstance`, `MakeInstanceUnique`, `RenameComposition`, `DeleteComposition`, `PurgeCompositions`; `SetField`. Refused: a
paper outside 1 mm – 10 m, a border leaving no paper inside, an element not finite or of scale
not positive, a view of a component gone, a view projected from itself or from one of its
projections, or from a perspective view, a view's projection or dimension in another container
(`SplitView`), a composition made to hold itself (`CompositionLoop`). A file whose drawing has an id taken or
above `next_id`, an image missing, a view that does not resolve, what hangs on a view away from
it, an instance of a composition gone, a composition holding itself or a paper's corner in a composition is refused
(`FileError::BadDrawing`).

## Views

A view (`View`) has a name, a style and a kind:

- **A base view** shows a **subject** — the model, a component in its own axes, or occurrences
  where they stand — from a **look** — a standard direction (front, back, left, right, top,
  bottom, iso; as the 3D view's, Z up) or any direction, parallel or in perspective (the
  camera's) —, at a **scale** (paper over model), the centre of its subject's box seen there
  drawn at `at`.
- **A projected view** takes a base view (perhaps projected itself) and a side (left, right,
  above, below): its base's subject and scale, turned a quarter towards that side as the
  projection says, aligned with it at a distance; moving the base moves it. It stays in its
  base's container.
  `Drawing::resolve` gives a view's subject, frame (`Frame`: right, up, forward), scale and
  place, following its bases; `resolve_as` for a given projection, a composition's view being drawn
  with that of the presentation it is placed in.

What a view shows: `Document::subject_parts` (the parts placed under it, in its axes, the
hidden ones left out). Every view shows its **frame**: its subject's exact box seen in its
frame, at its scale, dashed (solid when picked), named with its scale (« Vue 1 · 1:10 »). Its
**style** says what fills it:

- **Lines** « Traits »: its lines ([Lines](#lines)), the visible ones bold, the hidden ones
  thin and dashed.
- **Rendered** « Rendu »: a picture of the 3D renderer ([Rendered views](#rendered-views)).
- **Hybrid** « Hybride »: the picture, its visible lines over it.

What a view shows — its parts, its box, what its lines are computed from — is kept per view
and direction (a composition's projected view looks its presentation's way) for the history's
revision and the meshes' generation (`sheet::ViewsCache`): a sheet drawn again computes none of
it.

### Choosing what a view shows

« Nouvelle vue » and the « Montre » field of a base view's entity info open one dialog
(`view_picker::ViewPicker`, decision 0051): « Le modèle » above the model's tree, its occurrences
with their thumbnails, unfolded by their chevron, a search listing the matches flat. Rows are
ticked, several at once; a row ticked holds what is under it, shown as ticked. Nothing ticked is
the whole model, one occurrence its component in its own axes, several the occurrences where they
stand. For a new view the 3D selection (or the part open) comes ticked; it is then placed by a
click, its frame following the pointer over the active sheet, at the largest standard scale
fitting half the paper, Esc giving up. A view whose content changes keeps its scale and place; a
dimension whose anchor is no longer shown becomes an orphan.

### Framing a view

« Cadrer la vue » (`drawing.frame_view`, or a double click on a view; a projected view opens its
base) shows the view's subject alone over the free area (`view_framer::ViewFramer`, decisions 0051
and 0052). **The view's frame never moves**, what lies outside it masked, and only the picture does:

- a **drag turns it** about the point pressed, which stays where it is in the frame, short of going
  over the top;
- the **wheel** zooms gently at the pointer: in parallel the scale shown runs freely (« ≈ 1:3.7 »)
  and settles on the nearest standard one a quarter of a second after it stops; in perspective the
  eye moves towards the point there;
- the **right button slides the picture**, the point of view unchanged;
- **Shift + drag moves the point of view** across (in parallel, the same as sliding).

A part of the subject always stays in the frame. A bar puts it on a standard direction, the subject
centred. **« Tout montrer »** keeps the frame and adapts the scale (the largest standard one holding
the subject) or the eye; **« Ajuster le cadre »** keeps the scale and sizes the frame around the
subject. **Valider** (Enter) changes the view in one undoable step, its projected views following;
**Annuler** (Esc) leaves it. The 3D view's camera and selection do not move, and while it is open
the commands that change the document are off. A direction is in the subject's axes: « Caméra »
takes the 3D camera into a component's axes through its first occurrence.

**A frame is automatic or free**, as the entity info says. *Automatic* (a new view, or after
« Ajuster le cadre » in parallel): it is the subject's box and follows the model; a view only turned
stays so and takes its new box when validated. *Free*: it stays as placed; a view slid, zoomed or
cropped becomes free. A perspective view's frame is always free.

**A view's crop** (`View::crop`, optional) is its frame on the paper, in millimetres from `at`; a
view without one shows its whole subject. A parallel view keeps its exact scale, so its dimensions
stay right; they stay drawn outside the frame. The **handles** crop it: the corner moves, the
opposite one and the picture stay. With Shift, a parallel view's handle changes its scale, which
takes the crop along. Another direction or subject from the entity info drops the crop.

### Perspective views

In the framing bar, **Perspective** puts the eye where the subject fills the frame the view has,
at 50 mm; its slider (14 to 200 mm, on a 24 mm high film) changes the focal length, the eye moving
so that the subject keeps its size. There, the right button moves the eye across, the wheel towards
the subject, and the standard directions stay in perspective. A view others are projected from
stays parallel.

A perspective view (`Perspective`: eye, opening, and the size on the paper that opening spans) has
**its own frame**, cropped as above. It is marked « Sans échelle »,
takes no part in the sheet's scale field and takes **no dimension**: those it has are kept, hidden,
counted in its entity info, and come back in parallel. Its lines come from the display meshes,
cut at the eye and at its frame ([Lines](#lines)).

## Lines

A view's lines are its parts' edges and the silhouettes of their curved faces, seen parallel
along its frame, split where faces hide them, visible or hidden; polylines in its frame, in
model units (`skraft_tools::hidden_lines::ViewLines`). They are computed off the UI thread
through a **queue**, a bounded number at once, the active sheet's views first, then the others on
screen (on the web on the UI thread, one a frame), and kept per view and, for a projected view,
per projection of its presentation (`view_lines::LinesCache`, `sheet::ViewSlot`: a composition's
projected view may look two ways on the board at once), asked for by what they depend on — the frame's axes, the parts' meshes and places, the
part's shape — so that moving a view or changing its scale computes nothing; the last ones are
shown while new ones are on their way.

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
- **In perspective, always on the meshes** (SolidCore's are orthographic): the same search on the
  scene mapped so that the rays from the eye become parallel, (x, y, z) → (d x / z, d y / z,
  -d² / z), d a distance within the scene. Triangles and segments are cut at a near plane first,
  and at the frame, so that the eye may stand inside the model; the lines are the tangents of
  their angles off the view's axis.

The lines are drawn into the view's picture ([Rendered views](#rendered-views)) by the
renderer, over everything (its overlay): the visible ones 0.35 mm wide on paper, nearly black,
the hidden ones 0.18 mm, grey, dashed 2 mm / 1 mm (on screen, from each segment's start), at the
picture's points per paper millimetre.

## Rendered views

Every view is drawn by the 3D view's renderer, its meshes and
palette (materials, textures) shared, into a picture of its own: its subject's parts at their
places in its axes, seen parallel along its frame, framed exactly on its subject's box — the
box its frame shows on the sheet —, on the paper's white, with the view's edges and
silhouettes (`Viewport::sheet_pictures`, the « Composants » thumbnails' way). The board asks
for the pictures of the views on screen (`sheet::Wanted`), the active sheet's first, one per view
and, for a projected view, per projection, each the size of its frame on screen in pixels, by steps of a fifth (a zoom
redraws it past a step), 4 096 at most — its parts for the *Rendered* and *Hybrid* styles, its
lines for *Lines* and *Hybrid*; a picture is drawn again when its direction, its parts and
places, its lines or its size change, within 40 ms a frame (the others the next), shown from the
next frame. A picture gone off screen is kept within a memory budget (256 MB, 64 on the web), the
least recently seen freed first, so that panning back redraws nothing. A perspective view's picture is seen from its eye, across its own frame.

## The stage

`Stage::Sheet`: **the board** over the whole central area, on the view's colour: every
presentation and every sheet at once, as a desktop publishing application's pasteboard, so that
their sizes and orientations compare at a glance. The tools' cards, the view bar and what else
belongs to the 3D view are not shown; the ribbon and the panels are (the entity info, the
[navigator](#navigator) and the [compositions](#the-compositions-card) at first). A document
without a sheet says how to make one. The board's layout is computed; the active sheet and the
camera are a view state (`SheetState`). None of it is saved.

### The board

- **A column per presentation**, in their order from the left, without a rule between them, its
  name on top. A presentation without a sheet keeps its column, saying how to make one.
- **Its sheets stacked** below, in their order, each on its white paper with a shadow, **centred
  on the column** (as wide as its widest sheet), its name above it. A sheet shows its **border**
  under its elements, and what it holds through its instances ([Compositions](#compositions)).
- **In board millimetres**, y up, from the tree and the papers alone. The gaps follow the drawing's
  largest paper side, so that an A0 among A4s leaves room: a tenth of it between sheets, three
  tenths between columns, which read apart without a rule.
  The names keep their size on screen; a sheet's is left out when its gap has no room for it.
- **The active sheet's column stands out**: its presentation's name on the selection colour, a
  dotted frame around the column, broken by the name. The sheet's own outline stays solid.
- **When the layout changes** (a sheet added, a format changed), the camera follows the active
  sheet, which stays where it is on screen.

### The active sheet

One sheet is **active**, the one edited (`SheetState::current`): the selection, the tools, the
handles, snapping, the entity info and paste act on it.

- **It is drawn with its aids**: an accent outline, its views' frames and names, its grid, what
  goes past its paper.
- **The others are drawn as printed**: what the PDF shows, clipped to their paper, without view
  frames, view names or grid. Nothing on them is picked.
- **A click on another sheet makes it active**, the camera still, nothing picked (a composition
  open is left); **a second click picks**. With a tool too, the click only making the sheet
  active; but while a tool is under way (a shape started, a pick made, a view being projected)
  its clicks are its own. A drag from another sheet makes it active and goes on there: a selection
  rectangle with Select, never moving what lies under it, a shape with a drawing tool.
- **A double click**: on another sheet, makes it active and frames it, never entering a
  composition under the pointer; on the active sheet, enters an instance, or frames the sheet
  from its void; in the board's void, frames the whole board.

### Framing

A drag with the middle or right button pans the board (the first button's drag never does: it
selects, moves or draws), the wheel (or a pinch) zooms about the pointer. The board is framed
whole when first shown and by zoom extents. Whatever is framed is **framed in what the panels
leave**: below the ribbon, between the side columns (`sheet::ui`, `fit`; the whole stage if they
leave under 160 pt).

**The camera targets a sheet, never an element.** In the [navigator](#navigator), a sheet's row
makes it active and frames it, a presentation's does so with its first sheet, or the one of its
sheets already active. An element's row on
another sheet makes that sheet active and frames it; on the active sheet the camera moves only if
the sheet is off screen.

## Placing views

- **Selecting** (the sheet's Select tool, as the 3D view's, docs/tools/interaction.md#selection),
  on the active sheet: a click on an element picks it (the last drawn under the pointer),
  elsewhere none; Shift+click adds or removes it; a drag off the elements draws a **selection
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
  its dimensions) goes with it, not moved twice. Let go over another sheet, it moves there
  ([Moving between sheets](#moving-between-sheets)).
- **Projected views** (`drawing.project_view`, « Vue projetée », the view picked a parallel
  one): the pointer off the base view's frame shows, dashed, the frame of the view projected
  to the side it is on — the side where it is the farthest out, the frame's half sizes
  weighing —, centred where it is along that side; a click puts it there (named « Vue n », in
  its base's style, picked) and the tool goes on (`SheetTool`).

## Presentations

A presentation is a document of sheets for someone (« Atelier », « Client »): its sheets are
numbered on their own (`Drawing::sheet_number`), it says where its projected views go
(`projection`) and holds values for its fields ([Fields](#fields)). A sheet belongs to one; the
compositions are common to all. « Nouvelle présentation » (`drawing.new_presentation`) makes one with
an A3 sheet, in one step, and shows it. Its PDF is a file of its own ([PDF](#pdf)).

## Navigator

The « Navigateur » card (`PanelId::Outliner`, `view.outliner`): on a sheet it shows the drawing's
tree instead of the model's (`sheet_tree::navigator`), as the entity info shows the sheet's; each
workspace keeps its own place for it. The cards are placed as in Modelling: the entity info keeps its height, the navigator fills the
column. Rows, unfolded by their
chevron, the active sheet and its presentation unfolding when another becomes active:

- **The document**, first, at the left edge: a click shows its values in the entity info.
- **A presentation**: a click makes its first sheet active (unless one of its sheets already
  is), frames that sheet and shows the presentation's settings in the entity info, the active
  sheet's in full ink. Its **« + »** adds a sheet: blank, or from a template. Its menu:
  « Enregistrer comme modèle », « Supprimer la présentation ».
- **A sheet**: a click makes it active and frames it, lit while nothing is picked on it. Its menu: « Supprimer la
  feuille ». Elements dragged onto it move there ([below](#moving-between-sheets)). **Dragged by its
  row, it changes place** (`MoveSheet`, one step undone at once): onto another sheet's row, before
  it, or after it by its lower half while that row is folded; onto a presentation's row, to its
  end, also from another presentation. The board and the numbering follow, the active sheet
  staying where it is on screen. **With the copy modifier** (Option on macOS, Ctrl elsewhere) a
  copy goes there instead (`CopySheet`, one step), beside the sheet itself too: its elements under
  ids of their own, its views keeping their projections and dimensions, its instances placing the
  same compositions, its name kept unless the presentation has it (then the next free « Feuille n »
  for a default name, else « Plan#2 »). The copy becomes active.
- **Its elements, the topmost first**: a view's name, a text's first line, a composition's name for an
  instance, else their kind. A click makes their sheet active and picks them, the camera going to
  the sheet and never to the element ([Framing](#framing)), Shift adding or taking off;
  hidden or locked, they are not picked. At the row's right end **the eye** (hidden or shown), **the
  padlock** (under the pointer, or while locked), and under the pointer **up** and **down**, over or
  under the next element (`SetElementState`, `MoveElement`).
- **An instance** unfolds on what its composition holds, one level after another. Its menu: « Modifier
  la composition », « Éclater », « Rendre unique », « Placer une copie ». Only the rows right in the
  level open have their buttons and names to edit, those of the instance entered lit; a click on
  another row leaves the levels that do not lead to it and picks what stands for it in the last
  one left open (the instance, on the sheet).
- **« Nouvelle présentation »**, last: a blank one (an A3 sheet), or one from a template.

A double click (or F2 on the one element picked) renames in place a presentation, a sheet, a view
or a composition (from an instance's row too).

### Moving between sheets

An element right on a sheet is **dragged by its row onto another sheet's** (`MoveElements`), with
what is picked when it is, in any presentation, the copy modifier copying it instead. A view goes
with its projections and dimensions. It keeps its place on the paper, brought back onto a smaller
one (`sheet::offset_into`; too large for it, centred); an instance anchored to a corner keeps its
corner and offset. The sheet it lands on is shown, what moved picked. Not into a composition: that
would change all its instances at once.

**On the board**, the selection dragged from the active sheet and let go over another sheet's paper
moves there (`MoveElements`, one step undone at once), where it was let go, that sheet becoming
active, what moved picked, the camera still; the sheet it would land on is outlined meanwhile. An instance anchored to a corner keeps its corner and
offset. A view goes with its projections and dimensions. A projected view or a dimension picked
without its base view stays on its sheet, and nothing leaves a composition open.

## Compositions

A **composition** is a definition: elements in axes of its own, placed by **instances**, as a
component by its occurrences, in types of their own (they stay out of the cut list, the
components' counts and the purge). A composition may hold instances of others, never itself, at any
depth. `Drawing::placed` walks a sheet through them: each element with its path of instances,
what takes it to the sheet, whether it or an instance above it is hidden or locked.

- **Making one** (« Créer une composition », `drawing.make_composition`, G; `MakeComposition`): the
  elements picked become a composition placed there, at the identity, where the topmost of them
  was; « Composition#n » unless named. A view comes with its
  whole family (its base, its projections, their dimensions). What was anchored to the paper
  keeps its place, without the anchor.
- **A copy** is an instance of the same composition, placed from the navigator (« Placer une copie » on
  an instance, a little aside; « Placer sur la feuille » on a composition, where its first
  instance on a sheet is: a title block at its corner). « Rendre unique » (`drawing.make_unique`,
  `MakeInstanceUnique`) gives one a copy of its own.
- **Exploding** (« Éclater », `drawing.explode`, `ExplodeInstance`) puts copies of what the composition holds where the instance shows
  them. Refused when the instance is turned over a text, a rectangle, an image or a view, which
  have no rotation of their own.
- A composition **neither named nor placed** goes away; a named one stays until `DeleteComposition`, which
  takes its instances.
- **The place** (`Place`): a move and a rotation, never a scale (a view at 1:10 stays true). On
  a sheet it may be measured from a **corner of the paper** (`Corner`): the instance then follows
  it when the format changes, so one title block serves an A3 and an A4. The sheet's frame is not
  an element: it is its **border** (a margin, a line width), drawn from its paper.

**On the sheet** (`sheet::draw_elements`, through `Drawing::placed`), what an instance holds is
drawn in its place and turned with it: lines, boxes, texts, dimensions, views (their picture on
their turned frame, asked for once per projection however many times the view is placed). **An instance is
picked whole**: a click, the selection rectangle and a drag take what is right on the sheet, by
the box of all it shows, outlined when picked; it has no handles, and moving it keeps its anchor
(its offset from the corner changes). Hidden, nothing of it is drawn; locked, it is drawn and not
picked. Its content is snapped on as the sheet's ([Snapping](#snapping)), but a dimension tool
picks only in the views right in the level open: a dimension stays beside its view.

**Editing a composition in place**, as an assembly in the 3D view
([Selection](../tools/interaction.md#selection)): a double click on an instance (or « Modifier la
composition », `drawing.edit_composition`) enters it (`SheetState::open`, the instances entered from the
sheet down; not saved). What lies outside is drawn first, under a veil of the paper's white, then
what it shows, in a dashed grey box 2 mm around it. Everything the sheet does then acts on the
composition, in its axes, turned or not (`Drawing::opened`): picking, moving, handles, the tools, a new
view; what is snapped on stays the sheet's, its points and grid. Every instance of the composition
shows the change as it is made. **To leave it**, one level at a time: a click off its box (inside,
a click in the void only clears the selection), or Esc once nothing is picked; the instance left
is then the one picked. Showing another sheet leaves it too. A composition left empty goes with its
instances, as an empty assembly, as a step of its own.

### The compositions card

The card « Compositions » (`PanelId::Compositions`, `view.compositions`, `sheet_library`) is to
the drawing what [« Composants »](components-library.md) is to the model. It exists only in a
workspace of sheets, and is shown there at first, under the navigator.

- **What it lists**: every composition, by name in natural order, as tiles or rows (the toggle and
  its preference are the components card's), with a search. Its picture is **its outline**
  (`sheet::Sketch`: lines, rectangles, view frames, a bar per line of text), made again when the
  document changes. **« ×n »**: how many times it shows on the sheets, one inside another counted
  per instance of it (`Drawing::counts_on_sheets`).
- **« Inutilisées »**, apart: those on no sheet, even if another unused one places them.
  « Purger » deletes them (`PurgeCompositions`).
- **A click** picks its instances in the level open.
- **A drag onto a sheet places one more**, its outline following the pointer, held by the middle
  of its box since a composition's origin may be anywhere (a title block's is its corner). It
  lands free on the sheet under the pointer, which becomes active (in the level open when it
  already was), and is picked. Let go off the papers or over a card, or Esc, gives it up. Not
  into itself.
- **Its menu**: « Placer sur la feuille » (where its first instance on a sheet is: a title block
  at its corner), « Renommer » (also a double click, or F2 while its instances are picked),
  « Supprimer », with its instances.

## Copy and paste

⌘C, ⌘X and ⌘V act on what is picked on the sheet, or in the composition entered — never on the
3D selection, which is not seen there (`sheet::copy_selected`, `Copied::Sheet`, on the app's one
clipboard, [Copy, cut and paste](../tools/copy-paste.md)). « Tout sélectionner » takes what the
level open holds, but what is hidden or locked.

- **What is copied** (`Document::elements_fragment`): the elements with their families (a view's
  base, projections and dimensions), the compositions they place, their images.
- **Pasted** (`PasteElements`) on top of the active sheet or composition open: at the pointer,
  the box of what was copied centred on it (off that sheet's paper, a little aside), or **in
  place** (⌘⇧V).
  An instance anchored keeps its corner; into a composition, which has no paper, its place.
  What is pasted is picked.
- **In its document**, an instance stays one of its composition and a view keeps showing the
  model. A composition gone with its last instance (a cut) comes back under its id: every paste
  places the same one. Pasted into itself: refused (`CompositionLoop`).
- **From another document** (or a state no longer in the history): as a template's, without
  views nor dimensions, the compositions brought once.

## Free elements

Drawn with the ribbon's « Dessiner » group, on top of the sheet (`SheetTool`): **Texte**
(`drawing.tool_text`: a click puts « Texte » there, picked, written in the entity info; the tool
then back to Select), **Rectangle**, **Cercle** and **Trait** (`drawing.tool_rect`,
`drawing.tool_circle`, `drawing.tool_line`: two clicks — its corners, its centre then a point of
it, its ends — or a drag, as the 3D view's shapes; previewed from the first
point; a second point under 0.5 mm from it waits for another; Esc drops it; the tool goes on). Every element is picked by a
click (its box: a text's laid out words, a line's ends 3 points around), moved by a drag
(`SetElement` once let go), deleted by « Effacer »; a hidden one is not drawn, a locked one not
picked. Texts are drawn at their size in paper millimetres, lines, rectangles and circles
0.25 mm wide (a circle is a true curve in the PDF); images as their frame for now.

## Handles

The element picked alone shows its **handles** (white squares, 7 points, with Select; none on
a locked one), as a vector drawing tool's; the pointer over one shows what it does (a
resize or move cursor), a drag from it (within 6 points; before the element's own drag)
reshapes the element, previewed, changed once let go (one `SetElement`); Esc drops it
(`Grip`, `Handle`, `regrip`):

- **Rectangle, image**: its 4 corners (two sides) and 4 side middles (one side); it stays a
  rectangle, turned over past the opposite side; Shift on a corner keeps its proportions, the
  opposite corner fixed.
- **Line**: its two ends.
- **Circle**: its 4 points right, top, left and bottom give its radius, its centre kept.
- **Base view**: its frame's corners give its scale — as far out from its centre as the
  pointer, the nearest standard scale —, its centre kept (a projected view takes its base's).
- **Text**: its box's top right gives its height, its top on the sheet's grid if it has one (its baseline kept; 0.1 mm steps, 0.5 to
  200 mm).
- **Dimension**: a linear one's line middle, a radius's or diameter's text point, an angle's arc
  middle (`DimShape::grip`): moved as its drag does.

Free points (polylines, polygons) will come with a tool of their own.

## Snapping

Where the pointer aims while a rectangle, a circle, a line or a text is put, or a box's, a
circle's or a line's handle dragged (`Aim`, `aim_for`), as the 3D view's inferences:

- **on a point** within 8 points: the rectangles' and images' corners, the circles' centres and
  their right, top, left and bottom points, the lines' ends, the views' frame corners and their parts' vertices (`ViewData::snaps`), those in instances where
  they show them; the element dragged left out; circled in orange;
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

**Moving the selection** snaps the same way, with its box (`Moving`, `aim_box`, its targets taken
once when the drag starts, what moves with it left out): one of its 4 corners or its centre on a
point, its centre on the paper's; else, each axis apart, one of its sides or its middle aligned,
its middle alone with the paper's centre; else its low side on the grid. A projected view or a linear dimension, which go along one axis, snap along that one only
(not at all in a turned composition). Carried over another sheet, it follows the pointer.

The dimension tools have their own snapping, on the views' vertices and edges.

## Measurements

The status bar's measurements box works on a sheet as in the 3D view (`skraft_tools::vcb`: its
expressions, lists, `@`, points): it shows the value live, what is typed goes to it with no
click, Enter applies it, Backspace edits it, Esc drops what is typed and nothing else. Values
are **lengths on the paper in the document's unit** (a bare number; an explicit unit wins),
whatever a view's scale. It measures (`measure_of`, `typed_kind`):

- **the shape being drawn**, after its first point: a rectangle's `width;height` from that
  corner, towards the pointer; a circle's radius; a line's length along where it goes, or its end at `<dx;dy>`
  from its start, or at `[x;y]` on the sheet;
- **the handle being dragged**: a box's corner `width;height`, a side its one length, from what
  stays fixed; a line's end its length, its direction kept; a circle's point its radius; a view's corner its scale (`1:20`,
  `2:1`, `1/4`, a ratio: any, not only the standard ones); a text's height; a dimension's
  distance (a linear one's offset, a radius's text, an angle's arc);
- **else the shape last made or the handle last let go**, as SketchUp: typed right after the
  gesture, the value corrects it (one more operation), while it is picked alone and has not
  changed since (`SheetState::amend`).

A value refused says why in the status bar until the next one or a click.

## Fields

A text's `{…}` are filled when drawn (`sheet::Fields`): `{document}` (the document's name, its
file's), `{date}` (today, day/month/year), `{sheet}` (the sheet's name), `{sheet.number}`,
`{sheet.count}`, `{scale}` (the first base view the sheet shows, through its instances); an
unknown one is left as it is.
The entity info's « Insérer un champ » menu puts one at the text's end.

**Values in cascade** (`Document::field`): any other field — `{client}`, `{indice}` — takes its
value from the instance showing the text (the innermost first), the sheet, its presentation, then
the document's properties: the model's typed properties (`Props`, set by `SetField`, the document's
by `SetProps`); `{client}` stands for `user.client`. A field without a value stays written
`{client}`, on the sheet and in the PDF, whose export says which ([PDF](#pdf)).

The entity info lists, under « Valeurs », the fields of what is shown (`Drawing::field_names`,
through its instances) with the value set **at that level**; while it has none, the field shows
faintly the one it inherits. Typing sets it, emptying takes it off. « Insérer un champ » offers
Skraft's fields, those the drawing's texts already name, and a new one.

## Templates

A **template** is a presentation — its sheets, their values, the compositions they place, without views
nor dimensions (a composition that held only those goes, with its instances), their images
(`Document::template_of`) — in a file of its own: the `.skraft` container, `content: "template"`
([File format](../io/file-format.md)), `.skrafttpl`, refused as a model (`FileError::TemplateOnly`).
The library's are in `<config>/Skraft/templates` (native). Skraft comes with one
(`Document::default_template`, « A3 avec cartouche »): a sheet with a border 10 mm in and the composition
« Cartouche » anchored at the bottom right — the document's name, the sheet's, the date, the scale,
« Feuille n / N ».

- **Saving**: « Enregistrer comme modèle » (`drawing.save_template`, the File menu, or a
  presentation's menu in the navigator) writes the active sheet's presentation under its name. A template
  of that name is never written over: the same one is left as it is, another takes the next
  number (« Client 2 »).
- **A new sheet**: a presentation's « + » in the navigator offers a blank sheet, then the templates (Skraft's first,
  the library's by name), one of several sheets offering each in a submenu. The sheet
  (`AddSheetFrom`) takes its paper, grid, border, values and elements; it is named « Feuille n »
  from a template of one sheet, else keeps its name unless the presentation has it.
- **A new presentation**: « Nouvelle présentation » in the navigator offers one from each template
  (`AddPresentationFrom`), under its name (numbered if taken), its sheets keeping theirs.
- **A file opened**: a `.skrafttpl` is put in the library, and makes a sheet in the document shown,
  or a presentation when it holds several sheets.

**A composition comes once**: each carries the name of its source (`Composition::source`, the hash of what a
template keeps of it), and one already in the document — brought before, or the very composition the
template was saved from, unchanged — is used as it is.

## Dimensions

A dimension (`skraft_model::Dimension`, an element) measures the model **through a view**
(`view`): it goes with it (deleted with it, `Drawing::projections_of`) and is drawn where the
view shows what it measures. Its kinds (`DimKind`):

- **Linear** between two vertices: along the sheet's x (horizontal), y (vertical) or between
  them (aligned — their distance on the view's plane), its line `offset` millimetres of paper
  off them; extension lines 1 mm off the points and 1.5 mm past the line, filled arrows
  (2.5 × 1.2 mm), the value above the line (the document's units, with their symbol when the
  document shows it).
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
`Angle`): the pointer snaps, within 8 points, on the vertices (linear) or edges (a circle face
on; a straight edge) of a view right in the level open, marked in orange (each view's vertices and edges on the sheet made
once, `ViewData::snaps`); a click picks it — the second pick in the first's view —; then the
dimension follows the pointer and a click places it on the sheet, picked, the tool
going on. A linear dimension is horizontal when the pointer is past the points' span along y,
vertical past it along x, aligned within it. Esc drops the picks. A
dimension is moved by a drag (a linear one's line across itself, the others' text or arc) and
deleted as any element. The entity info shows its kind and value, or « Orpheline ». In the
PDF: its lines 0.18 mm, its arrows filled, its text turned with it (`sheet::dim_shape`, the
shapes `dims::shape` gives the sheet and the PDF alike).

## PDF

« Exporter en PDF » (`drawing.export_pdf`, the ribbon's « Présentation » group and the File menu)
writes **the active sheet's presentation** as a PDF (`pdf::drawing_pdf`, with `krilla`), then asks where to
save it (a download on the web), named after the document, and after the presentation when the
document has several (« Table - Client.pdf »). A field without a value would be printed as it is
written: a warning names them (`pdf::missing_fields`). It is done at the next frame, where
the GPU is at hand (`App::pdf_asked`). Once written, its toast offers « Ouvrir »: the system's
application for PDFs opens it (`Act::Open`: `open`, `explorer`, `xdg-open`; desktop only).

- **A page per sheet** of the presentation, at its paper's size (points: 72 a
  25.4 mm), the sheet's millimetres (y up) turned into the page's (y down); its border, then what
  it shows through its instances (`Drawing::placed`), each drawn in its container's axes under
  its instances' transform; what is hidden left out.
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
- **a free element picked**: its type; a text its words (a field of several lines, applied when
  left; « Insérer un champ ») and size (millimetres);
- **a composition picked** (an instance): its name (edited), its **anchor** (« Libre » or a corner of
  the paper, only right on a sheet; changing it leaves it where it is, `Place::with_corner`), its
  **X** and **Y** (from that corner, else from the sheet's or composition's origin), its **rotation**,
  its values ([Fields](#fields));
- several picked, how many;
- **else the sheet**: its name (edited), its size (A4 to A0) and orientation (landscape,
  portrait), its grid ([Snapping](#snapping)), its **border** (« Cadre », with its margin and line
  width), its values;
- **the presentation** when chosen in the navigator (`SheetState::info`): its name, its number of
  sheets, its projection (first-angle « Européenne », third-angle « Américaine »), its values;
- **the document**, the navigator's first row: its name and its values.

Each change is one operation (`SetElement`, `SetSheet`, `SetPresentation`, `SetField`,
`SetProps`). Lengths are in the document's unit, shown exactly: on paper, 0.25 mm is no « ~ 0 ».

## Ribbon

The fixed group: Select and Erase (`edit.erase`). One tab, « Feuille ». Its group « Présentation »:
« Nouvelle présentation » (`drawing.new_presentation`, blank; from a template, in the
[navigator](#navigator)), « Nouvelle feuille » (`drawing.new_sheet`, A3, shown) and « Exporter en
PDF ». Its group « Vue »: « Nouvelle vue » (`drawing.add_view`: [the dialog](#choosing-what-a-view-shows), then
a click places its front view, at the largest standard scale (10:1 to 1:5000) fitting half the
paper), « Vue projetée » (`drawing.project_view`, checked while its tool runs); the groups « Dessiner »,
« Organiser » (« Créer une composition », « Éclater », « Rendre unique »), « Coter » and
« Style »: « Traits », « Rendu », « Hybride » (`drawing.style_lines`,
`_rendered`, `_hybrid`: the views picked, checked when they all have it; `SetElement`, a
`Batch` for several). Zoom extents is not in the ribbon: its shortcut (Shift+Z), the View menu and
the command search keep it.

## Driving

Handles ([Control protocol](../control-protocol.md)): `sheet` (the stage), `sheet_paper:<id>`
(a sheet's paper on the board), `sheet_view:<id>` (a view's frame on the active sheet),
`sheet_element:<id>` (another element's box there); in the navigator, `sheet_tree:add_sheet:<id>` (a
presentation's « + ») and its menu's `sheet_add:blank`, `sheet_add:<n>` (the n-th template), `sheet_add:<n>:<k>` (its
k-th sheet), `sheet_tree:add` and its menu's `sheet_presentation:new`,
`sheet_presentation:template:<n>`; the navigator's
`sheet_tree:document`, `:presentation:<id>`, `:sheet:<id>`, `:element:<id>`, `:add`, and on an
element's row `:eye:<id>`, `:lock:<id>`, `:up:<id>`, `:down:<id>` (the last three under the
pointer), in an open menu `:menu:<command>`, `:menu:copy`; in the compositions card `compositions:<id>`, `:search`, `:grid`,
`:list`, `:purge`, and in an open menu `:menu:place`, `:menu:delete`; in the entity info
`drawing_border`, `drawing_value:<id>:<n>`, `drawing_field_value:<name>`, `drawing_new_field`;
`drawing_insert_field`
and its menu's `drawing_field:<field>`. `model.document` shows the drawing.

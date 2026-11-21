# Drawing: free sheets in the document, for the workshop and for clients

- Status: accepted (2026-11-20); its layers, shared title block and templates replaced by
  [0049](0049-drawing-a-tree-of-presentations-and-definitions.md)
- Design: [Drawing](../ui/drawing.md) (under way)
- Follows: [A workspace shapes the editor](0047-workspaces-shape-the-editor.md)

## Context

Decided on 2026-11-20 (questions / answers), after a headless trial. *Drawing* « Mise en plan »
is the third workspace (0047). Skraft is a general modeller on real solids: a drawing must
serve a part's fabrication drawing, an assembly's or a whole model's — and the documents sent
to clients, as SketchUp users make them with LayOut: rendered views, texts, images, a title
block of one's own. SolidCore (`793d09e`) brings an exact hidden curve removal
(`blend::hcr::Hcr`: the visible and hidden pieces of a B-rep's edges and silhouettes, one
B-rep per scene) and hatch patterns (`hatch`); Skraft had no vector output.

The trial (`drawing_trial`, removed once the lines are built) drew four views of ZeTable and
of `Cruise_Assembly.step` (2 789 parts placed, 4.48 M triangles) both ways:

- **SolidCore's HCR, part by part**: ~10 ms a part on ZeTable; on Cruise 4–36 s a view on every
  core (32–224 s of CPU), one part up to 35 s, and 2 to 25 % of the runs failing
  (`simple_coincidence_checker`, a zero vector in `unitize`) — those parts have no lines. One
  part never hides another: that needs every part in one scene B-rep.
- **Hidden lines on the display meshes**, every part hiding the others: right on both; on
  Cruise 6.9 s (iso) to 103 s (front, the whole ship stacked behind each point) with a naive
  search (no stop once a segment is hidden, no culling of hidden parts by their boxes).

A second trial, the same day, on SolidCore `3af0aea` (on main since 6c4bf27): its
`solidcore::hlr` (`solidcore_ops::hlr`, from SolidCraft) draws exact orthographic hidden lines
of **placed** bodies, several in one scene, each piece visible, hidden or uncertain, the hidden
pieces under visible ones dropped. It refuses a placement that is not rigid (a mirror: the
body is then prepared placed).

- **ZeTable** (14 parts): 11–24 ms a view, the visible lines' lengths exactly the meshes', none
  uncertain.
- **Ender3-V2** (254 parts, 110 shapes): 38 s to prepare the shapes (one 36 s), then 15–20 s a
  view before refusing it (`Budget("crossing pair")`: 500 million pairs, fixed in the code).
- **Cruise**: shapes prepared in 5 s; every view refused on the same budget (after the curve
  budget, `max_tracks`, raised); 211 placements refused even alone (`Invalid("scene extent")`
  at ordinary places: a face's box left unset, a SolidCore quirk to reproduce).

## Decision

- **Free sheets**: a sheet has a format and holds **views** of anything — a part, an assembly,
  the model — and **free elements**: texts (with fields: title, date, scale, sheet number,
  the document's or the client's properties), images (a logo), lines, rectangles, groups.
- **The title block is not fixed**: it is a group of free elements, on a layer the document's
  sheets share (as LayOut's shared layers). A sheet or a document starts from a **template**
  (format, shared layers, styles), kept in the user's library and copied into the document,
  which stays whole on its own.
- **In the document**: the sheets are the document's, in its `.skraft` file, changed by
  `Operation`s (undone, journalled); a view names its subject by ids and follows the model.
- **A view carries its own point of view**: a standard direction, or the 3D camera of the
  moment (perspective allowed), turned again from the sheet; its own style, hidden parts and
  section. No named scenes in *Modelling*.
- **Three styles per view**: *Lines* « Traits » — the visible and hidden edges as vectors
  (the workshop's drawing); *Rendered* « Rendu » — an image made by the 3D renderer
  (materials, textures, shading) at the print resolution; *Hybrid* « Hybride » — the image with
  the vector lines over it (for clients). The PDF holds both images and vectors.
- **Lines, hybrid computation**: hidden lines computed on the display meshes for a view of
  an assembly; SolidCore's exact `hlr` for a view of one part or a small scene (exact curves,
  for a DXF later; the threshold to set by measuring), falling back to the meshes when it fails
  or refuses. Off the UI thread. The mesh search is to be made fast (stop once hidden, parts
  culled by their boxes, recomputed by the parts changed). *Amended the same day after the
  second trial: `hlr` instead of the HCR, which draws one body without the others.*
- **The stage is the sheet alone**: the paper over the whole area, panned and zoomed; the sheets
  listed in a panel. A view is added from the selection or the model. The 3D view is in
  *Modelling*; the selection is kept across (0047).
- **Base view and projected views**: a base view (subject, direction, scale), its projected
  views aligned to it and at its scale — first-angle (European) by default, third-angle a
  document option; moving the base view moves its projections. Isometric, detail and section
  views later.
- **Dimensions are associative**: hooked on a part's exact vertices and edges by their names
  (decision 0042's, to be extended); their value and place follow the model. A dimension
  whose entity is gone is marked orphan, to be reattached. *Amended when built (Q&A
  2026-11-21): names first, else geometry — by the faces' names where the part's shape has them
  (a single body: a profile's, a node's map), else by the point picked in the part's axes,
  found again within a tolerance; the names reaching multi-body parts' final shapes later.
  Linear (horizontal, vertical, aligned), radius, diameter and angle; placed by picks then a
  click.*
- **Output: PDF** first — vector and images, to scale, a page per sheet; a download on the web.
  SVG, DXF and printing later.

## Alternatives considered

- **One drawing per part** (OpenCutList's spirit), or **an annotated 2D view of the model**
  without sheets: rejected for free sheets, which hold both — and client documents.
- **A drawing file of its own** referencing a `.skraft` (as LayOut a `.skp`): rejected — links,
  reloading and a second history to manage.
- **A title block with set layouts and fields to fill**, or **a background drawn elsewhere**
  (an imported PDF or SVG) with text fields: rejected for free elements and templates.
- **Named scenes** in *Modelling* (SketchUp's), a view referring to one — or both ways:
  rejected, a view carries its own point of view.
- **Lines only**, or **lines and vector fills** of the materials' colours: rejected — client
  documents want textures and shading.
- **Lines from SolidCore's HCR only**: rejected after the trial — failures leave parts without
  lines, slow on some parts, and no part hides another without one scene B-rep.
- **Lines from SolidCore's `hlr` only**: rejected after the second trial — right and fast on a
  small scene, but an assembly of a few hundred parts exceeds its budgets after seconds, some
  bodies are refused; kept for a part or a small scene.
- **Lines from the meshes only**: rejected for a view of one part, where the exact curves cost
  little and serve a DXF.
- **The lines drawn by the GPU on screen, vectors computed only for export**: rejected — the
  screen and the paper could differ. (The *Rendered* style's image is the GPU's, on screen and
  on paper alike.)
- **Sheet and 3D view split** (as *Parts list*), or **the 3D view central and the sheet a
  card**: rejected, the sheet is the deliverable and needs the room.
- **Independent views**, or **a sheet filled at once** (three views and an iso): rejected for
  base and projected views; filling a sheet at once may come as a shortcut over them.
- **Dimensions frozen** when placed: rejected, a drawing would go wrong silently.
- **SVG, DXF or printing first**: PDF first, the others later.

## Consequences

- The document gains sheets, views, free elements, shared layers, dimensions, their operations
  and their file entries (images among its files; format v1: no version bump).
- A library of templates beside the user's preferences, as the materials' (0041).
- A job computes a view's lines from the meshes (or the kernel's `hlr` for a part or a small
  scene), cached per
  view and recomputed when its parts change; the renderer draws a view's image off screen, at
  the sheet's zoom on screen and at the print resolution for the PDF.
- Dimensions need names on more of a part's vertices and edges than 0042 gives today.
- A new stage, `Stage::Sheet`, draws paper, images, lines and annotations in 2D, with editing
  tools for free elements (select, move, text, image, shapes).
- A PDF writer (native and wasm) embedding images and fonts.

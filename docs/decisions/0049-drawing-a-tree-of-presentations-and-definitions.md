# Drawing: a tree of presentations, sheets and definitions, no layers

- Status: accepted (2026-11-21; built)
- Design: [Drawing](../ui/drawing.md) (steps in `docs/REVIEW.md`, « Work: drawing tree »)
- Replaces the layers, the shared title block and the templates of
  [Drawing: free sheets in the document](0048-drawing-sheets-in-the-document.md)

## Context

Decided on 2026-11-21 (questions / answers), after trying 0048's drawing. Its layers are
LayOut's: a shared layer holds its elements once, in millimetres from the paper's bottom left
corner, and shows them on every sheet of the document. Two things follow:

- **Paper formats cannot mix**: a title block set at the bottom right of an A3 is in the wrong
  place on an A4.
- **The workshop's and the client's documents cannot live side by side**: a shared layer is on
  every sheet, hidden or locked for all of them.

The 3D document already has what is missing: a tree, shared definitions, placed instances
(decision 0005).

## Decision

```
Document
└─ drawing
   ├─ compositions        : Id → Composition { name, elements }     the definitions, common to all
   └─ presentations : « Atelier », « Client »…          { name, projection, props }
      └─ sheets                                         { name, paper, grid, border, props }
         └─ elements                                    { hidden, locked, kind }
            kind = View | Text | Line | Rect | Image | Dimension
                 | Instance { composition, place, props }
```

- **Presentations**: the document holds presentations, which hold their sheets. A sheet belongs
  to one. Numbering, values and the PDF are a presentation's; the definitions are common to all,
  so the workshop's and the client's documents are edited in one place and may share a title
  block.
- **One mechanism, as in 3D**: a composition is a named definition placed by instances; a composition may
  hold instances of others, never itself. Gathering elements makes one with an automatic name
  (« Composition#1 »), placed once. *Named « composition » on 2026-11-21, as the 3D's « assembly »:
  « group » let think there was no definition.* A copy shares its definition; « Rendre unique » separates it. A
  composition neither named nor placed goes away; a named one stays until deleted.
- **Types of their own**, not `Component`: compositions are 2D and must stay out of the cut list, the
  components' counts and the purge.
- **An instance moves and turns, never scales**, as an occurrence: a view at 1:10 stays true.
- **The paper's corner**: an instance on a sheet may be anchored to a corner of the paper with
  an offset, and follows it when the format changes. **The frame is not an element**: it is the
  sheet's border (margin, line width), drawn from its paper.
- **Views in compositions**: a composition may hold views. A view, its projections and its dimensions stay
  in one container; grouping one takes the others. Their references are ids within it, so a
  composition placed twice shows the same view twice.
- **Values in cascade**: a field looks for its value in the instance (the innermost first), the
  sheet, the presentation, then the document's properties. These are the model's typed
  properties (`Props`); `{client}` stands for `user.client`.
- **A template holds a presentation** (one or more sheets, their definitions, no view). A whole
  presentation or one of its sheets is made from it; a definition already brought from it is
  reused, never doubled (`Composition::source`).
- **Navigator**: the tree in the « Navigateur » panel, which chooses the sheets and makes them
  (*the tabs first planned under the sheet were dropped on 2026-11-21*), a composition edited in
  place by a double click, the rest dimmed. *The compositions moved out of it on 2026-11-21 into
  a card of their own, as the model's components.*

## Alternatives considered

- **Keeping layers, per presentation or per paper format**: rejected, the coordinates would
  still be the paper's and every format would need its own.
- **Sheets flat in the document, presentations as lists referring to them**: a sheet could
  serve both, but its number would depend on the list shown. A content to share goes in a composition.
- **No presentations for now**: simpler, but adding them later reworks the tree, the tabs, the
  numbering and the export.
- **Groups apart from compositions**, their copies independent (LayOut, Illustrator): more familiar in
  2D, but two notions to handle everywhere; the 3D document has none either.
- **Stretchable compositions**, their elements tied to the instance's edges: one composition for frame and
  title block at any format, at the price of constraints in definitions and their editing.
- **A definition per format**, placed freely: changing a sheet's format means replacing it.
- **Only graphic elements in compositions**: a view could not be grouped with its caption.
- **Links between containers**, by a path of instances as in 3D: each link would have to survive
  copies, explosions and deletions.
- **Values on the instance only** (AutoCAD's composition attributes): a client's name typed on every
  sheet. **On the presentation and the document only**: no revision index per sheet.
- **A template of one sheet**: a standard file of several sheets rebuilt by hand.
- **A definition edited on a page of its own**: more room, but out of its context.

## Consequences

- `Drawing` is a tree; the operations on layers are gone, those on presentations, compositions and
  instances come. Drawings saved before are not read back (format v1).
- Drawing, picking, handles, snapping and the PDF walk the tree with each instance's transform
  (`Drawing::placed`).
- A projected view in a composition is drawn with the projection of the presentation it is placed in
  (`Drawing::resolve_as`): one composition may show first-angle here and third-angle there.
- An instance turned cannot be exploded while it holds what has no rotation of its own (a
  text, a rectangle, an image, a view).

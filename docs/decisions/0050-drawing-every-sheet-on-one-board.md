# Drawing: every sheet on one board, one of them active

- Status: accepted (2026-11-21; built)
- Design: [Drawing](../ui/drawing.md#the-stage)
- Replaces the single sheet on the stage of
  [Drawing: free sheets in the document](0048-drawing-sheets-in-the-document.md); keeps the tree of
  [Drawing: a tree of presentations, sheets and definitions](0049-drawing-a-tree-of-presentations-and-definitions.md)

## Context

Decided on 2026-11-21 (questions / answers), after trying 0049's drawing. The stage shows one
sheet at a time. Since a presentation may mix paper sizes and orientations, and a document may
hold several presentations, what they look like side by side can only be guessed from the
navigator. A desktop publishing application shows all its pages on one pasteboard.

## Decision

- **One board**: the stage shows every presentation and every sheet at once. **A column per
  presentation**, in their order, its name on top, no rule between columns; **its sheets stacked**
  in their order, each with its name, **centred on the column**.
- **The layout is computed** from the tree and the papers, in board millimetres. It is not saved:
  the model, the file format and the PDF do not change.
- **One active sheet**, the one edited: selection, tools, handles, snapping, the entity info and
  paste act on it, as they acted on the sheet shown.
- **A click on another sheet makes it active**, the camera still, nothing picked; a second click
  picks. **A double click on it also frames it**, and never enters a composition under the
  pointer. On the active sheet a double click keeps its role: it enters an occurrence, or frames
  the sheet from its void. In the board's void it frames the whole board.
- **The camera targets a sheet, never an element.** A sheet's row in the navigator makes it active
  and frames it, a presentation's does so with its first sheet. An element's row on another sheet
  makes that sheet active and frames it; on the active sheet the camera moves only if the sheet
  is off screen.
- **The other sheets are drawn as printed**: what the PDF shows, clipped to their paper, without
  view frames, view names or grid. Only the active sheet has these aids and an accent outline.
- **An element dropped over another sheet moves there**, where it was let go, that sheet becoming
  active, in one undo step. A view takes its projections and dimensions. A projected view or a
  dimension without its base view stays on its sheet, and nothing leaves a composition open.

## Alternatives considered

- **A click on another sheet doing nothing**, only a double click changing sheet: the simplest,
  but a sheet in plain sight takes a gesture out of the ordinary to reach.
- **A click activating the sheet and picking at once**, as InDesign: the most fluid, but the
  selection, its rectangle and every tool would have to work across sheets.
- **The camera going to the element picked in the navigator**, or moved only when the target is
  off screen: the first makes the view jump at each row, the second leaves the zooming to do by
  hand from the overview.
- **Sheets aligned on the column's left edge**: their widths compare along one edge, but the
  column reads less evenly than centred on its axis.
- **Other sheets drawn as the active one**: view labels keep their size on screen and clutter
  the overview. **Dimmed**: the active sheet stands out, but the sheets compare less faithfully.
- **An element staying on its sheet when dropped elsewhere**: it would be cut by its paper as
  soon as its sheet is no longer active.
- **Sheets placed freely on the board, their places saved**: a second order to keep in step with
  the navigator's, and data in the file for what is only a way of looking at it.

## Consequences

- A sheet's drawing takes its place on the board (`Paperview`), composed with what its occurrences
  add. The camera is in board millimetres; when the layout changes it follows the active sheet,
  which stays where it is on screen.
- **Pictures and lines are kept per view and, for a projected view, per projection**: two
  presentations of different projections can now show one composition's projected view at the
  same time, and a view whose direction changes keeps its last picture until the next.
- **Several sheets cost more**: lines are computed through a queue, the active sheet's first, and
  pictures gone off screen are kept within a memory budget, so that panning redraws nothing.
- `SheetState` keeps its active sheet (`current`): what acts on a sheet does not change.

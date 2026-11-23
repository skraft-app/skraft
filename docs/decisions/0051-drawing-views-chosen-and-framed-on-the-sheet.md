# Drawing: a view's content and point of view chosen from the sheet

- Status: accepted (2026-11-23; steps 1-2 built)
- Design: [Drawing](../ui/drawing.md#views)
- Amends [Drawing: free sheets in the document](0048-drawing-sheets-in-the-document.md), where a
  view is added from the selection and takes the 3D camera of the moment

## Context

Decided on 2026-11-23 (questions / answers). A new view shows what is selected, but the selection
is made in *Modelling* and the view added in *Drawing*: nothing on the sheet says what the view
will show, or how to choose it. Its direction is a standard one or a copy of the 3D camera, which
also means leaving the sheet. The model already stores any direction and a perspective, but a
perspective view is drawn parallel.

## Decision

- **« Nouvelle vue » asks what to show**: a dialog lists « Le modèle » and the model's tree, with a
  search, thumbnails and several choices at once, the 3D selection only ticked in advance. The view
  is then placed by a click, its frame following the pointer.
- **A view's content can change**: its « Montre » field in the entity info opens the same dialog.
  Its scale and place stay; a dimension whose anchor is no longer shown becomes an orphan.
- **« Cadrer la vue »**: a double click on a view opens the 3D over the central area, turned,
  panned and zoomed by the pointer, with a mask of the view's frame; a bar gives the standard views,
  parallel or perspective, the focal length, Validate and Cancel. *As built in step 2: the view's
  picture drawn large by the sheet's renderer, its subject alone, rather than the 3D view with its
  tools; the parallel / perspective choice and the focal length come with step 3.* The camera and selection of *Modelling* are
  left alone, and validating is one undo step.
- **A view's direction is in its subject's axes**: the 3D camera is converted when a view of a
  component takes it.
- **A perspective view** has an **explicit frame on the paper**, cropped by its handles (the box
  of the subject means nothing with the eye inside the model), is labelled « Sans échelle », and
  takes **no dimensions**: those it has are kept, hidden and flagged, and come back in parallel.
- **Projected views follow** their base when its angle changes. A base with projected views cannot
  go to perspective; the command says why.
- **Its lines** come from the display meshes, projected before the hidden line search: SolidCore's
  exact `hlr` is orthographic.

## Alternatives considered

- **Keeping the selection as the only source**: the misunderstanding that started this.
- **A « Modèle » card to drag from**: always in sight, but it takes room from the sheet and leaves
  a view's content impossible to change in place. It may come on top of the dialog.
- **Orbiting inside the view's frame on the sheet** (LayOut): keeps the context, but cramped on a
  small view, and needs a second navigation in the sheet.
- **Dimensions on a perspective view**: their values could not be read from the paper.

## Consequences

- Steps: 1. the content dialog and the editable subject; 2. « Cadrer la vue » in parallel, the
  camera converted to the subject's axes; 3. perspective pictures with the explicit frame;
  4. perspective lines, *Hybrid* and PDF, measured on a large model.
- `drawing.add_view` no longer makes a view by itself: a pilot goes through the dialog's handles.

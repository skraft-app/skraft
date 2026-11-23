# Drawing: a view frames what it shows

- Status: accepted (2026-11-23; built)
- Design: [Drawing](../ui/drawing.md#framing-a-view)
- Follows [Drawing: a view's content and point of view chosen from the sheet](0051-drawing-views-chosen-and-framed-on-the-sheet.md)

## Context

Decided on 2026-11-23 (questions / answers), after trying 0051. Its framing stays organised around
the centre of the subject: the orbit turns about it, the wheel goes towards it, the handles crop
symmetrically, and a parallel view's frame is the subject's box. A detail cannot be aimed at, nor
an oblique view placed well in its frame.

## Decision

- **A base view may have a crop**: its frame on the paper, in millimetres from the point of the
  sheet its subject hangs on, in parallel and in perspective. Without one, it shows the whole
  subject. A parallel view keeps its exact scale, so its dimensions stay right.
- **A frame is automatic or free**, as the entity info says. *Automatic* (a new view): it is the
  subject's box and follows the model. *Free*: it stays as placed, even if the model grows.
- **In « Cadrer la vue » the frame never moves** and only the picture does: a drag turns it about
  the point pressed, which stays in place; the wheel zooms gently at the pointer (in parallel, the
  scale settles on the nearest standard one when it stops); the **right button slides the
  picture**, the point of view unchanged; **Shift + drag moves the eye**, as a pan in the 3D view
  (in parallel, the same as sliding). A part of the subject always stays in the frame.
- **Two explicit actions**: « Tout montrer » keeps the frame and adapts the scale (or the eye);
  « Ajuster le cadre » keeps the scale and sizes the frame around the subject, a parallel view
  becoming automatic again.
- **An automatic view only turned stays automatic**: its frame takes the new box when validated.
  Slid, zoomed or cropped, it becomes free.
- **A view's handles crop it**: the corner moves, the opposite one stays, the picture does not
  move on the sheet. With Shift, a parallel view's handle changes its scale, as it did.

## Alternatives considered

- **The right button moving the eye**: what a pan does in 3D, but recentring would then change the
  perspective. To try: Shift does it for now.
- **A lens shift on perspective views alone**: parallel views would have no detail framing.
- **A separate kind of detail view**, linked to a base view: more than a frame on the same view.
- **A free zoom in parallel**: the scale written on the sheet would no longer be a standard one.
- **The frame following the subject while it is turned**, until the first slide: the same gesture
  then acted differently before and after, without showing it.
- **The wheel stepping from one standard scale to the next**: up to ×2.5 a notch, the model lost
  in a gesture.

## Consequences

- `View::crop` is optional in the file: files without one read as before.
- A dimension may stand outside its view's crop: it stays drawn.
- A crop falls when the view's direction or subject changes from the entity info: what it framed
  is no longer there.

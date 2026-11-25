---
status: under way
code: [apps/skraft/src/workspaces/common/viewport.rs, apps/skraft/src/system/driving.rs, crates/tools/src/tools/select.rs, web/index.html]
---

# Touch screens

Skraft on a tablet or a phone, through the web app (or a touch screen on Windows). A phone is
for looking at a model, a tablet for modelling too
([0056](../decisions/0056-touch-screens-phone-views-tablet-models.md)). For now the view takes
the fingers; the interface around it is still the desktop's.

## Gestures in the view

- **One finger is the tool's**, as a mouse's left button: down, the tool follows it (inferences,
  preview, the part under it lit); lifted without moving, a click; moved, a drag — a drawing
  tool's drag is a click, a move and a click ([Interaction](../tools/interaction.md)): a box's
  base is drawn by dragging from one corner to the other.
- **With the select tool, one finger dragged orbits**, about the point it came down on (the
  navigation style's pivot, [Navigation styles](navigation-styles.md)). A tap still selects,
  and a double tap is a double-click. There is no selection rectangle by touch yet.
- **Two fingers pan and pinch, with any tool**: the model follows the fingers — moved together,
  it slides by as much; spread or brought together, it grows or shrinks by their distance,
  about their middle. The gesture's pivot is set where it begins, as an orbit's.
- **A touch that becomes navigation leaves the tool alone**: from the second finger down to the
  last one lifted, the tool gets no hover, click or drag. A drag the tool had from the first
  finger is given up (Esc); a point already clicked stays (a box's first corner). Lifting one
  of two fingers does nothing more: the next gesture starts once both are up.

### Why

- One finger for the tool keeps every tool working by touch with nothing of its own; two for
  navigation is what every touch app does, so it never depends on the tool.
- One-finger orbit only with the select tool: there a drag on a phone is almost always meant to
  turn the model, and a tap still picks. Elsewhere the drag is the tool's.
- The pinch is exact (a step of 0.85, `ln(zoom) / ln(1/0.85)` steps), not scaled by the zoom
  speed preference: the model has to stay under the fingers.

### Limits

- The first finger reaches the tool for a frame or two before a second one lands: the tool
  showed what was under it (a height preview, a lit part), hidden when the gesture begins.
- The gestures are fixed, not part of the navigation style.

## How it is built

The platforms emulate a mouse with the first finger (egui-winit natively, eframe's web runner):
a primary button pressed, moved and released, then `PointerGone`; every finger also comes as
`egui::Event::Touch`, which egui turns into `multi_touch()` (translation, zoom) from two fingers.
The viewport (`Viewport::touch_input`) reads those: while the touch navigates (`Touch::fingers`,
`Touch::orbit`) it hides the emulated pointer from the tool, including the release that would
make a click. A tool's `leave` hides what follows the cursor (the select tool lets go of the part
it lit), so a lifted finger leaves nothing lit.

**On the web**, the page keeps the gestures for the app (`web/index.html`): `touch-action: none`
on the canvas (eframe listens to `touchmove` on the document, passively in Chrome, so only this
keeps the browser from panning or zooming the page), no overscroll, no text selection, callout or
tap highlight. eframe already keeps the context menu and Safari's `gesture*` events from the page.

**Testing without a touch screen**: `ui.touch` ([Control protocol](../control-protocol.md))
sends real fingers — each down at `from` a frame after the one before, moved to `to` in
`steps`, lifted in the reverse order, the first last, with the emulated pointer as the
platforms give it. Chrome's device mode emulates one finger (Shift-drag: a pinch).

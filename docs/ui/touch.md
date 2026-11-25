---
status: under way
code: [apps/skraft/src/workspaces/common/viewport.rs, apps/skraft/src/system/driving.rs, crates/tools/src/tools/select.rs, web/index.html, apps/skraft/src/window/form.rs, apps/skraft/src/app/compact.rs, apps/skraft/src/window/titlebar.rs]
---

# Touch screens

Skraft on a tablet or a phone, through the web app (or a touch screen on Windows). A phone is
for looking at a model, a tablet for modelling too
([0056](../decisions/0056-touch-screens-phone-views-tablet-models.md)). The view takes the
fingers; a phone's screen gets the compact arrangement, a tablet's keeps the desktop's.

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

## The compact arrangement

**When**: the primary pointer is a finger (CSS `pointer: coarse`) and the window's short side is
under 600 pt (`form::COMPACT_SIDE`): a phone, either way up. A tablet (an iPad mini's 744 pt) and
a small desktop window under a mouse keep the desktop arrangement. No preference; for testing,
`SKRAFT_FORM=compact` (desktop: the window then opens at a phone's size) or `?form=compact` in the
page's address (`desktop` forces the other).

**Larger under a finger**: wherever the pointer is a finger, tablet included, the whole interface
is drawn 1.2 times larger (egui's zoom, `form::TOUCH_ZOOM`): buttons, rows and text together, the
text near a phone's usual 16 pt.

**What it shows**, every workspace (modelling, parts list, drawing):
- **The bar at the top**: a menu (☰: File's commands, the command search, the preferences), the
  document shown — its menu lists the documents open, a new one, closing it —, undo and redo.
  No file tabs.
- **The stage** takes the room between the bars: the 3D view, the parts list over the whole
  stage, or the sheet. The view bar, the tool's option cards (scrolled sideways when wider than
  the screen), the quick tools and the toasts float over it as on the desktop.
- **The ribbon, in one strip** scrolled sideways with a finger, at the bottom: the workspace's
  cell, the tabs, every group.
- **The cards' tabs** under it (Navigateur, Infos, Matières, Bibliothèque); held sideways, in the
  bar at the top, where they fit, so the view keeps its height. A tab raises its card from the
  bottom over half the stage, a tap on its header takes it to the stage's whole height and back,
  its tab again lowers it. One card at a time; the paint tool raises « Matières ».
- **The measurements box** above the strip while the tool takes a typed value (the system
  keyboard for now). No status bar: its hints speak of keys and a mouse.

*Why*: a phone is for looking at a model, but the user wants every tool within reach; the view
stays the largest thing on the screen, and a card is read over it rather than instead of it.

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

**On a phone or a tablet**: `scripts/serve-web.sh --https` serves `dist/web` on the local
network, at `https://<this Mac>.local:8443`. WebGPU needs a secure page, and `localhost` is one
only on the machine itself; the certificate comes from mkcert, whose root certificate the device
trusts once (the script's `--help`: AirDrop, install the profile, full trust). An Android phone
on USB can do without: `adb reverse tcp:8080 tcp:8080`, then `http://localhost:8080` on it.
Safari's Web Inspector (the iPhone on USB, Settings → Safari → Advanced) shows the page's console.

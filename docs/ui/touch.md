---
status: under way
code: [apps/skraft/src/workspaces/common/viewport.rs, apps/skraft/src/system/driving.rs, crates/tools/src/tools/select.rs, web/index.html, apps/skraft/src/window/form.rs, apps/skraft/src/app/compact.rs, apps/skraft/src/window/titlebar.rs, apps/skraft/src/kit/touch.rs, apps/skraft/src/workspaces/drawing/sheet/stage.rs]
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
- **A long press** (a finger held still 0.5 s, `form::LONG_PRESS`, egui's click time under a
  finger; iOS's) is the right click: a row's or a card's menu opens, and stays open once the
  finger is lifted. With the select tool, a finger that then moves draws a selection rectangle
  instead of turning the view (`Touch::marquee`: egui gives a long-pressed widget no drag, so the
  view gives the tool its drag itself). A frame comes every 0.1 s while a finger is down, for the
  press to become long though nothing moves.
- **On a drawing's board, two fingers pan and pinch it** the same way; one finger is the sheet's
  tool's, as the mouse's left button, but with the select tool a finger dragged from nothing
  pans the board (from a view or a shape it still moves it: the board has no other way).
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

## The magnifier

A finger hides what it points at. While a finger is down for the tool — any tool but select, the
touch not navigating —, a card above it (beside it, on the roomier side, near the view's top;
always within the view) shows what is under it magnified twice: the view's picture, and the
points, marks and inference drawn over it, the inference's tooltip with them. It goes when the
finger is lifted.

It is drawn from the view's own picture (its texture, the part under the finger) and the
overlays drawn again in a layer scaled about the finger (`Viewport::paint_loupe`), so it costs no
second render.

## Modifiers

Under a finger (a coarse pointer, tablet or phone), the tool's card begins with « Touches », the
keyboard's keys the tool takes (`options::touch_keys`):
- **Shift**, a toggle lit until the operation ends (an operation done, the tool changed): with
  the select tool « Ajouter » (add to the selection or take away); with the others « Maj », as
  Shift held — the line inferred locks when a finger next comes down on the view, there being
  none under a finger when the toggle is tapped (`viewport::Latched`).
- **« Copie »** (Move, Rotate): the copy key tapped, the tool's own toggle, lit while the tool
  copies.
- **X, Y, Z** (every tool but select, erase, paint, sample, section, the booleans): the arrow keys,
  locking the red, green or blue axis, lit while locked.

They come first in the card: on a phone the card is wider than the screen, and they are what a
finger reaches for during an operation.

**Esc**: under a finger, while a tool's operation is under way (`Editor::tool_under_way`) and it
waits for no validation, ✗ stands where the validation's ✗ and ✓ go, under the tool's card: it
steps the operation back. Undo and redo are in the title bar.

## The keypad

Under a finger the measurements box is a button: it opens the app's keypad above it, not the
system's keyboard, which would cover half the screen and the model. Its keys: the digits, the
system's decimal and list separators, the minus, erasing, and « Valider » (Enter), which gives the
value to the tool and closes it; it closes too when the box no longer takes a value, or when the
box is tapped again. A keyboard plugged into the tablet still types into the box from anywhere
(`kit/widgets/keypad.rs`).

## The compact arrangement

**When**: the primary pointer is a finger (CSS `pointer: coarse`) and the window's short side is
under 600 pt (`form::COMPACT_SIDE`): a phone, either way up. A tablet (an iPad mini's 744 pt) and
a small desktop window under a mouse keep the desktop arrangement. No preference; for testing,
`SKRAFT_FORM=compact` (desktop: the window then opens at a phone's size) or `?form=compact` in the
page's address (`desktop` forces the other).

**Larger under a finger**: wherever the pointer is a finger, tablet included, the whole interface
is drawn 1.2 times larger (`form::TOUCH_ZOOM`): buttons, rows and text together, the text near a
phone's usual 16 pt. egui's zoom is the interface zoom's preference times this
([Application interface](interface.md#interface-zoom)), the preference still 100 %.

**What it shows**, every workspace (modelling, parts list, drawing):
- **The bar at the top**: a menu (☰: File's commands, the command search, the preferences), the
  document shown — its menu lists the documents open, a new one, closing it —, undo and redo.
  No file tabs.
- **The stage** takes the room between the bars: the 3D view, the parts list over the whole
  stage (at least as wide as its columns, the whole table scrolled both ways with a finger — as
  on the desktop when the list is narrower than its columns), or
  the sheet. The view bar, the tool's option cards (scrolled sideways when wider than
  the screen), the quick tools and the toasts float over it as on the desktop.
- **The ribbon, in one strip** scrolled sideways with a finger, under the bar at the top, where
  it is on the desktop: the workspace's cell, the tabs (centred on what is seen of the strip),
  every group.
- **The cards' tabs** at the bottom (Navigateur, Infos, Matières, Bibliothèque); held sideways, in the
  bar at the top, where they fit, so the view keeps its height. A tab raises its card from the
  bottom over half the stage; its header, dragged, sets its height (let go low, it goes down),
  tapped, takes it to the stage's whole height and back to half; its tab again lowers it. One card at a time; the paint tool raises « Matières ».
- **The measurements box** above the cards' tabs while the tool takes a typed value (its keypad,
  [below](#the-keypad)). No status bar: its hints speak of keys and a mouse.

Windows and boxes keep within a narrow screen (`style::window_width`, `window_body`,
`window_at`): as wide as the screen leaves at most, their body scrolled when larger, held in the
middle rather than where they were left; « À propos » drops its licenses' column, its links one
under the other.

The page keeps the browser's safe areas (no `viewport-fit=cover`): Safari keeps it clear of the
notch and the home indicator, which the app would otherwise have to read from CSS and leave
free itself.

What scrolls under a finger here does so from the first touch on (`ScrollSource::ALL`): egui's
default waits to have seen a touch screen, and the first drag went nowhere.

*Why*: a phone is for looking at a model, but the user wants every tool within reach; the view
stays the largest thing on the screen, and a card is read over it rather than instead of it.

## How it is built

The platforms emulate a mouse with the first finger (egui-winit natively, eframe's web runner):
a primary button pressed, moved and released, then `PointerGone`; every finger also comes as
`egui::Event::Touch`, which egui turns into `multi_touch()` (translation, zoom) from two fingers.
`kit::touch::TwoFingers`, read by a stage before its widgets, says whether the pointer is a
two-finger gesture's — begun over the stage, on its layer, not over a card floating on it — until
the last finger is lifted, and gives its moves: the viewport then hides the emulated pointer from
the tool (`Viewport::touch_input`, which adds the select tool's orbit), the drawing's board makes
its stage sense nothing, so the release never makes a click. A tool's `leave` hides what follows the cursor (the select tool lets go of the part
it lit), so a lifted finger leaves nothing lit.

**On the web**, the page keeps the gestures for the app (`web/index.html`): `touch-action: none`
on the canvas (eframe listens to `touchmove` on the document, passively in Chrome, so only this
keeps the browser from panning or zooming the page), no overscroll, no text selection, callout or
tap highlight. eframe already keeps the context menu and Safari's `gesture*` events from the page.
iOS suspends a hidden page (the screen locked) and drops its downloads: a load that failed after
the page was hidden reloads the page once it is shown again, and the kernel's worker tries its
wasm again 1, 2, 4, 8 and 16 s later before giving up (`web/kernel-worker.js`).

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

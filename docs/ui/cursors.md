---
status: built
code: [apps/skraft/src/kit/cursors.rs, apps/skraft/src/viewport.rs, apps/skraft/assets/cursors, crates/tools/src/tools/mod.rs, crates/tools/src/tools/pick.rs, crates/tools/src/editor/mod.rs, crates/control/src/editor.rs, Cargo.toml]
---

# Mouse cursors

Over the view, the cursor is the active tool's at its step, as OpenCutList's: its SVG cursors
(`img/cursor-*.svg`, 32 × 32 points, their dark blue `#0e416c` made black) and hotspots
(`SmartCursorManager.create_cursor`), each tool choosing as its `get_state_cursor` /
`get_action_cursor` does. **Why:** the cursor tells which tool is at work and what the next
click does (the target or the tool part of a boolean, the height of a box), without reading the
status bar.

Over the rest of the window (title bar, ribbon, panels, menus, toasts), `select` stands for the
system's arrow: one pointer over the whole window. The system's other icons stay where a widget
asks for them: the text cursor in a field, the resize arrows on a panel's or window's edge, the
hand on a link-like button or a grip.

Decision record: [Custom cursors through a winit fork](../decisions/0035-custom-cursors-winit-fork.md).

## The tool's cursor

- `Tool::cursor(&Cx) -> Option<Cursor>` (`crates/tools`, no GPU, names only): none leaves the
  system's icon. `Editor::tool_cursor`; `tool.state` answers it (`cursor`, the variant's name).
- **Picking** ([Select first](../tools/selection.md)): `Picker::cursor(cx, pick)`, OCL's
  `SmartSelectActionHandler` — the selection rectangle's while one is dragged
  (`STATE_SELECT_RECT`), « ± » while Shift is held or the selection gathered
  (`STATE_SELECT_MULTIPLE`), else the tool's own pick cursor.

| Tool | Cursor |
|---|---|
| Select | `select`; `select-plus-minus` with Shift; `select-rect` while dragging a rectangle |
| Move | picking `select-part`; then `move`, `move-copy` with the copy toggle |
| Rotate | picking `select-part`; then the system's crosshair |
| Copy along a line / a grid | `select-copy-line` / `select-copy-grid`, picking and placing (OCL's `STATE_SELECT`, `STATE_HANDLE`) |
| Distribute | picking `select-distribute`; then `select` |
| Stretch | `select-stretch` at every step |
| Booleans | `select-unite-a` / `subtract-a` / `intersect-a` for the target, then `…-b` for the tool part |
| Box, Cylinder, Prism | `pencil-rectangle`, `pencil-circle`, `pencil-polygon`; `pull` for the height (OCL's `STATE_PULL`) |
| Turn | `pencil-polygon` |
| Paint | `paint-part` (occurrences), `paint-face-1` (faces), `picker` (« Prélever ») |
| Erase, Section, Measure, Profile | the system's crosshair (OCL has none) |

The system's icons stay over the view when the kernel is at work (progress) and while navigating
(the hand while orbiting or panning, the magnifier while zooming).

**What is written by the pointer keeps off the cursor's drawing**: each cursor's footprint
(`cursors::footprint`, the bounds of its visible pixels about its hotspot, worked out once from
its rasterised image) places the view's tooltip — the inference's, « Prélever »'s material —
under it (above it near the view's bottom, moved left at its right edge, `viewport::tip_place`)
and is what the lengths keep clear of. A hotspot is anywhere in the 32 × 32 image (a pencil's
tip at its bottom left, the pipette's at its top left), so a fixed offset from the pointer
would put the tooltip under some cursors. The system's cursors count as a 24-point square.

## Handing it to the system

- **One decision per frame**, at the end of `SkraftApp::ui` (`cursors::apply`), once every
  widget has said its `CursorIcon` (egui resets it to `Default` each frame): the view's cursor,
  which it asks for while hovered (`cursors::ask`), else `select` where the icon is still
  `Default`, else the system's icon.

- **Native**: egui's `Context::set_cursor_image` (`CustomCursorImage`, straight RGBA), which
  eframe gives winit (`CustomCursor::from_rgba`). The SVG is rasterised by resvg once per cursor
  and scale, cached: the same `Arc` every frame, which egui-winit dedupes by pointer (no upload
  each frame). The image is sticky between frames: `apply` sets it, or clears it, every frame.
  Windows and Linux take it in physical pixels, at the window's scale.
- **macOS**: winit 0.30.13 sizes the cursor's image in points as many as its pixels (blurry
  on Retina) and puts straight RGBA into a premultiplied bitmap (antialiased edges nearly
  opaque, as long as the cursor moves). Skraft's fork,
  [skraft-app/winit](https://github.com/skraft-app/winit) branch `skraft` (one commit on
  v0.30.13), takes the bitmap at @2x and premultiplies it; the app renders at 2× on macOS
  whatever the screen (AppKit downsamples on a 1x screen). `[patch.crates-io]` in `Cargo.toml`,
  by `rev`: a winit upgrade carries the commit over.
- **Web**: eframe drops `cursor_image` and sets the canvas' inline `cursor` style each frame. A
  style sheet added on the first custom cursor has one `!important` rule per cursor, which wins
  over the inline style, chosen by the canvas' `data-cursor` attribute (the SVG's name, set
  when it changes): PNGs at 1x and 2x in `image-set`, the hotspot in CSS pixels, `default` as
  fallback.

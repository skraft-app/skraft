# Custom cursors through a winit fork

- Status: accepted (2026-11-18)
- Design: [Mouse cursors](../ui/cursors.md)

## Context

The view only had the system's cursors (arrow, crosshair, move, « + »): nothing told which tool
was at work or what its next click does. OpenCutList shows one cursor per tool and step, drawn
as SVG. egui 0.37 hands an RGBA bitmap to the OS (`set_cursor_image`, through winit), but on
macOS winit 0.30.13 makes it blurry on Retina (the image sized in points as many as its pixels)
and wrong at its antialiased edges (straight RGBA in a premultiplied bitmap). The web runner
ignores the bitmap.

## Decision

Decided on 2026-11-18 (questions / answers).

- **OCL's cursors** (its SVGs and hotspots), each tool choosing at its step as OCL does
  (`Tool::cursor`), not a set drawn for Skraft.
- **A fork of winit** for macOS: [skraft-app/winit](https://github.com/skraft-app/winit),
  branch `skraft`, one commit on the release in use (the bitmap at @2x, premultiplied), by
  `[patch.crates-io]` with a `rev`. Nothing of it in the repository; the commit is carried over
  at each winit upgrade, until winit takes such a fix.
- **The web in the same piece of work**: CSS rules (`image-set` of PNGs, `!important` over
  eframe's inline style) chosen by an attribute of the canvas.

## Alternatives considered

- **Accept the blur**: no fork, cursors blurry on every Retina screen — the screens most Macs
  have.
- **Vendoring winit** in the repository (`vendor/winit`, `[patch]` by path): self-contained,
  but about 3 MB and 218 files for a few lines.
- **Cursors painted in the view** (egui's painter, the system's hidden): clipped at the edge
  of the window, a frame late on the pointer.
- **Setting the `NSCursor` ourselves** (objc2) beside winit: winit resets it from its own cursor
  rects.

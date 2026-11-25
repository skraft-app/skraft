# Touch screens: a phone looks at a model, a tablet models

- Status: accepted (2026-11-25; step 1, the view's gestures, built)
- Design: [Touch screens](../ui/touch.md)

## Context

The web app opens on phones and tablets, but its interface is the desktop's: two columns of
cards 270 pt wide over a phone's 390 pt, targets of about 20 pt, and tools that rely on what a
finger does not have — a cursor resting over the model, modifier keys, a right button, a
keyboard for the VCB. The first finger reaches egui as a mouse, so a drag drew a selection
rectangle where it should have turned the model, and two fingers did nothing useful.

## Decision

- **A phone is for looking at a model**: navigating, selecting, measuring, the parts list, the
  drawings. **A tablet is for modelling too.** A phone's interface is not asked to hold every
  tool.
- **The same app, not another one**: the same workspaces and widgets, a compact arrangement
  where the screen is small, the shared widgets larger under a finger. **The arrangement is
  chosen automatically**, by the screen's width and a coarse pointer (a finger), with no
  preference.
- **Gestures**: one finger is the tool's (a tap a click, a drag the tool's drag), except with
  the select tool where a drag orbits; two fingers pan and pinch with any tool, and the tool sees
  nothing of them. Fixed for now, outside the navigation styles.
- **In steps**: (1) the view's gestures and the page's settings; (2) the compact interface for
  phones; (3) modelling by touch on a tablet — what replaces the cursor at rest, the modifiers,
  the right button and the keyboard.

## Alternatives considered

- **A separate viewer app for phones**: a second interface to keep in step with the first, for
  what the compact arrangement gives.
- **One finger orbits with every tool, the tool working through a cursor offset from the finger**
  (as a trackpad): precise, but every point takes two gestures; drawing by dragging, with a
  magnifier above the finger, is to be tried first (step 3).
- **Gestures in the navigation styles**: they copy desktop programs' mouse habits, which touch
  apps do not have; to reconsider if users ask.
- **A preference for the compact arrangement**: one more setting for what the screen already
  tells; a tablet in landscape keeps the desktop arrangement by its width.

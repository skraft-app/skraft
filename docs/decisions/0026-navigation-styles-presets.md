# Navigation styles: presets of other programs, each gesture editable

- Status: accepted (2026-11-16)
- Design: [Navigation styles](../ui/navigation-styles.md)

## Context

Decided on 2026-11-16 (questions / answers). Not every CAD program moves the camera with the same
buttons: SketchUp orbits with the middle button, Fusion 360 pans with it, Onshape and Rhino orbit
with the right one. FreeCAD offers « navigation styles ».

## Decision

- The user picks the habits of the program they come from (SketchUp, Fusion 360, SolidWorks,
  Onshape, Rhino, Blender) and can change each gesture.
- A style covers the mouse's gestures (middle, right and side buttons, with or without
  modifiers), the wheel's direction, the orbit's pivot, the trackpad's gestures, recentring and
  zoom extents. It never covers the keyboard, the left button alone (the tools'), the tools'
  modifiers nor the navigation tools; speeds and orbit directions stay global preferences.
- The wheel's direction moves into the style (`wheel_invert` becomes `NavStyle::wheel_reversed`),
  because it is part of a program's habit. The preferences file being pre-release, the old field
  is not read back.
- Presets are functions (`Preset::style()`), not data in the file: an unedited style is stored as
  its preset alone, so a preset corrected in a later version reaches everyone who has not edited
  it.
- The Navigation tab edits gestures by rows of combos, under a drawing of the mouse.

## Alternatives considered

- Option/Alt+left (Maya's habit): not offered, because Option is the copy modifier on macOS.
- Capturing a gesture by pressing it: rejected for rows of combos, so that every button, the
  side ones included, can be set without a mouse that has it.
- The Navigation tab as a plain table: a first try, redrawn on 2026-11-16 as a drawing of the
  mouse with a framed editor per button.

## Consequences

- A navigation gesture under way owns its modifiers: the tool does not see them change, and a
  modifier pressed during it stays hidden from the tool until released.
- The wheel's directions, the pivots and the trackpads of the programs other than SketchUp come
  from their documentation and are to be checked against each program; the table gets fixed
  then, not the model.

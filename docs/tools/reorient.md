# Reorient

« Réorienter » gives the part or assembly clicked new axes, after OpenCutList's SmartAxes. Nothing
moves in the world ([decision 0054](../decisions/0054-changing-a-components-axes.md),
[A part's axes](../model/data-model.md#a-parts-axes)). The change reaches every occurrence of the
component; an assembly's parts keep their own axes.

## Modes

The bar's « Mode », saved with the tool; Tab goes to the next.

- **Longueur ↔ Largeur**: a quarter turn about blue, clockwise seen from the front; the other
  modifier (Command on macOS, Alt elsewhere) held turns it the other way.
- **Face ↔ Dos**: a half turn about red.
- **Adapter**: blue on the normal of the flat face aimed at, red on its side within 8 pixels of the
  cursor, else its longest side (the nearest to the current red on a tie), the face on green's
  side. The other modifier (Command on macOS, Alt elsewhere) held reverses red. The origin stays. A
  slanted face gives oblique axes; a curved one is refused.
- **Origine**: the point inferred under the cursor becomes the origin, an outline's corner included.

## What it shows

As SmartAxes, the tool shows the target **as it is**: highlighted with the other occurrences of its
component, its axes, a white arrow along its length on its back (dashed, hidden when behind) and
on its front, its box dotted. A tooltip gives its name, material and size, and how many occurrences the change reaches.

What a click would give is drawn over that, in magenta:

- **Longueur ↔ Largeur**: a small curved arrow in the corner of the face towards the eye, from
  the side the arrow points at to the side it will point at. It leaves the corner when the face
  is too small on screen to hold it.
- **Face ↔ Dos**: a ring about red at the arrow's tip, with two heads: a half turn has no
  direction.
- **Adapter**: the face and the edge aimed at, the new front arrow filled, the new box dashed and
  the new axes. The current axes are left out, as in OpenCutList.
- **Origine**: the axes at the point aimed at.

## Why

- **The other modifier reverses red**, where OpenCutList uses Ctrl / Option: that key already holds
  the tree's parent level ([Selection](selection.md)).
- **A turn seen edge on is opened a little**: true to the axes it would flatten into a line, in
  the top view above all for the ring.
- **Axes already right are a message**, not an empty step in the history.

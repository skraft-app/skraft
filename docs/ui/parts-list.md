---
status: built
code: [apps/skraft/src/workspaces/parts/table.rs, apps/skraft/src/app.rs, apps/skraft/src/app/, apps/skraft/src/workspaces.rs, crates/tools/src/editor/library.rs]
---

# Parts list

The « Liste des pièces » workspace ([Workspaces](workspaces.md)): the model's parts in a table on
the left of the central area, the 3D view on the right. A first step towards a cut list: what is
placed, in which material, of which size, how many.

## Rows

- **One row per part placed in the model and material**: a part's occurrences, at any depth,
  grouped by the material they take (`PlacedPart::material`: the occurrence's own, else an
  assembly's above it). A part placed in one material is one row; placed in two, two rows.
  Hidden occurrences count: they are in the model. Parts not placed are not listed.
- **Columns**: name (the part's), material (« Aucune » when none), length, width, thickness —
  the box of the part's final B-rep along its red, green and blue axes (the scene's
  exact bounds, else the shape's vertices; « — » while the kernel computes it) —, quantity (its
  occurrences in that material). Lengths in the document's units; their
  symbol, when the document shows it, is in the column's header (« Longueur (mm) »), not in each
  cell. The columns are as wide as a number at the document's decimals.
- **Sorted** by a column's header: a click sorts by it (names and materials ascending, numbers
  descending), a second click the other way; the chevron beside the header says which. Equal
  rows by name, case ignored; no material last. By name at first.
- **Kept** per document (`Doc::parts_list`: the sort, the rows for the history's revision —
  built again while a shape is not known yet).

## Selection

Both ways, the 3D view's selection being the one:

- **A row clicked selects its occurrences** under the context edited
  (`Editor::select_occurrences`, as « Sélectionner les occurrences » of the library's components:
  the scope turned to « Pièces » when one lies deeper than the context's children).
- **A row is lit** when one of its occurrences is selected, picked at any depth or under an
  occurrence selected in the context edited.

## The stage

At first no side panel is shown: the table over half the area, the view over the other half
(the panels come back from the View menu or the preferences, in their places). The table is a
card titled « Liste des pièces », its title bar the side panels' (`title_bar`;
no chevron: it is neither folded nor moved), from the left column (or the area's edge) to the
3D view; the view goes on
to the area's right edge, the right column over it. For the ribbon the table counts as a column:
the ribbon is centred between it and the right column when it fits there whole, the table then
taking the area's whole height, as the columns; else the ribbon goes above them all, over the
area's width, and the table starts under it ([Workspaces and ribbon](ribbon.md#layout)). The
choice depends on the widths only: dragging the separation may move the ribbon from one to the
other, the table then dropping under it. The separation between
them is dragged (double click: the middle); the share is the preference `table_share` (20 % to
80 % of the room between the columns). Beside the view, the area's ground is the view's colour.
See [Workspaces](workspaces.md#stage).

## Ribbon

The fixed group: Select. One tab, « Liste »: the View group (zoom extents pinned, the standard
views in its menu).

## Driving

Handles ([Control protocol](../control-protocol.md)): `parts_list` (the card),
`parts_list_header:<column>` (`name`, `material`, `length`, `width`, `thickness`, `quantity`),
`parts_list_row:<part>` (`:<material>` after it when the row has one), `stage_split`.

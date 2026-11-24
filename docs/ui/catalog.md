---
status: built
code: [apps/skraft/src/workspaces/common/cards/catalog.rs, apps/skraft/src/workspaces/common/cards/library.rs, apps/skraft/src/workspaces/drawing/cards/library.rs, apps/skraft/src/workspaces/common/cards/materials.rs, apps/skraft/src/workspaces/common/cards.rs, apps/skraft/src/workspaces/common/cards/, crates/tools/src/editor/library.rs, crates/model/src/document.rs, crates/model/src/ops.rs]
---

# Catalog cards

The cards listing a document's definitions, the library (the components, or on a sheet the
compositions) and « Matières », share one presentation (`catalog::show`); each says what its items are through a `Shelf`: name,
colours, picture, interaction, section headers.
**Why:** the first two had grown as copies of each other, and materials needed the same.

## What they share

- **Grid or list**, toggled at the right of the search row, a preference per card
  (`components_list`, `compositions_list`, `materials_list`). Only the rows in view are laid out,
  only the tiles in view drawn.
- **Two sections**, in use then unused, in natural order of names; the second, hidden when
  empty, has the « Purger » button.
- **Renamed in place**: a double-click, F2 on the item chosen, or its menu.

## The materials card

- **« Aucune »** comes first, but under a search: it removes paint.
- **A click chooses** a material (`Editor::choose_material`): the paint tool's material, **the
  active tool unchanged** and the selection kept, so a selection can still be painted. Its
  entity info shows until the selection changes, something else is chosen, or a click in the
  view with another tool than Paint. **Why:** reading or editing a material must not start a
  tool.
- **The brush** over the picture of the item hovered or current starts the paint tool with it;
  so does its menu, which also renames, exports and deletes it. The key B paints with the
  material chosen.
- **The entity info edits it**: name, colour, texture (image, width, height, rotation), its
  [properties](../model/properties.md).
- **The count** `×n` is how many placed parts of the model show the material, on a face or
  through their occurrence, each placement once (`Document::material_counts`).
- **Unused** means nothing refers to it. A material only on unused components stays in
  « Utilisées » with ×0, so that `Operation::PurgeMaterials` never takes paint from a component
  that may be placed again.

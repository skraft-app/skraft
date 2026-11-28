---
status: built
code: [apps/skraft/src/workspaces/common/cards/library.rs, apps/skraft/src/kit/search.rs, crates/tools/src/search.rs, apps/skraft/src/system/thumbnails.rs, apps/skraft/src/workspaces/common/viewport.rs, crates/render/src/lib.rs, apps/skraft/src/workspaces/common/cards.rs, apps/skraft/src/workspaces/common/cards/, apps/skraft/src/window/dock.rs, crates/tools/src/editor/library.rs, crates/model/src/ops.rs, crates/model/src/document.rs]
---

# Components library

The document's parts and assemblies, as definitions: a docked card « Composants » shows them
(thumbnails or a list), places one more occurrence of one at the cursor, and manages them
(purge, select their occurrences, rename, delete, replace), the parts and the assemblies in a
tab each. A search field, shared with the navigator, filters both. After SketchUp's Components window, « In Model ».
**Why:** a component is a definition shared by its occurrences
([Data model](../model/data-model.md#components-parts-and-assemblies)); the navigator shows the
occurrences, nothing showed the definitions. A definition left without occurrence stays, named
or not, shown apart until purged, as SketchUp's: a part erased is placed again from here.
Decision [0038](../decisions/0038-components-library-card.md).

## The card

- **What the library shows outside a sheet**: the card « Bibliothèque » (`PanelId::Library`,
  command `view.library`), beside Entity info, Materials and the navigator, shows the
  workspace's definitions: the components here, the drawing's compositions on a sheet
  ([The compositions card](drawing.md#the-compositions-card)). Folded, moved, sized and closed
  as the others are ([Layout](interface.md#layout)); it may fill its column's height left
  (`PanelId::fills`). Closed by default, under the navigator when shown. **Why:** one card for
  each workspace's definitions, as one navigator for each workspace's tree; the components have
  no use on a sheet (decided with the user, 2026-11-24).
- **What it lists**: the document's components but the model's root — its **parts** and its
  **assemblies**, not the body definitions (the navigator shows those of the open part). Each
  one with its nature's icon (a part's, `icons::COMPONENT` for an assembly), its name (the
  automatic ones italic and attenuated, as everywhere) and **how many times it is in the
  model** (« ×12 »: its occurrences at any depth, an assembly's occurrences multiplying those
  below it; the cut list's count, `Document::counts_in_model`).
- **Two sections**: « Dans le modèle », then « Inutilisés » — the components not in the model
  (absent from `counts_in_model`: placed nowhere, or only in assemblies themselves unused),
  named or not: a definition losing its last occurrence (erased, exploded, replaced, a
  boolean's tool) stays until purged, but an empty assembly, which holds nothing
  (`Document::drop_unused_empty_assemblies`, after every operation). The second is hidden
  when empty.
- **Two tabs**, « Pièces (n) » and « Assemblages (n) », the ribbon's (`ribbon::tab_strip`),
  their band going on from the card's title bar, of the same colour, without the line that
  parts a title bar from the other cards' content, then the search row; each tab's
  count is what it shows (under a search, what the search lets through, so a match in the
  other tab is seen). The tab shown is the document's (`LibraryState.tab`, never on disk), the
  parts' at first. Handles `library:parts`, `library:assemblies`. « Aucune pièce »,
  « Aucun assemblage » in a tab with nothing. **Why:** a part and an assembly are not looked
  for the same way, and toggles filtering by nature did the same with more clicks (asked on
  2026-11-19).
- **Order**: by name, in natural order (« Pièce#2 » before « Pièce#10 », case ignored:
  `library::natural_cmp`). The rows are made again only when
  the document changes (its history's revision, `library::LibraryState`, kept in the
  document), and only those in view are laid out.
- Each row publishes its handle `library:<id>`, the purge button `library:purge`
  ([Control protocol](../control-protocol.md)).
- **Grid or list**: at the right of the search row, the option bar's segmented control
  (`widgets::segmented_sized`, at the row's height, the mark sliding to the value chosen;
  `icons::TILES`, `icons::LIST`; handles `library:grid`, `library:list`). Grid: square tiles of
  thumbnails (`thumbnails::TILE`, 64 pt) wrapped in rows, the name under each (small, elided),
  « ×n » on a pill over the top right corner, the chosen one outlined in the selection's
  colour; laid out whole, only the tiles in view drawn and their thumbnails asked for. List:
  one row per component, 30 pt high (a thumbnail of 26 pt, the name, the count), full width as
  the navigator's rows (`tree_row`), only the rows in view laid out. The choice is a preference
  (`components_list`, the grid by default). The presentation is shared with the other cards of
  definitions: [Catalog cards](catalog.md).

## Thumbnails

- **One picture per component**, drawn off screen by the viewport's renderer
  ([Renderer and document](../render/display.md#renderer-and-document)): a parallel iso view
  from the front, right and above, framed tight on the component's box (its corners' extent
  across the view, a margin of 12 %, `viewport::thumbnail_camera`), lit as the view, on the
  view's background, its edges and silhouettes drawn half as wide as the view's
  (`THUMBNAIL_EDGE_SCALE`); a part's mesh once in its own axes, an
  assembly's parts as placed in it (`thumbnails::Content`: its hidden occurrences left out,
  5 000 placements at most). Square, `TILE` points at the screen's pixel density.
- **Colours**: the view's palette; its faces' paints (in the meshes); each part's material,
  else the theme's default ([Palette and materials](../render/look.md#palette-and-materials)).
- **The renderer's pictures** (`render::Picture`, `Renderer::render_picture`): a picture is its
  resolved texture only (64 kB at 128 px), drawn with a multisampled colour and depth the
  renderer keeps for pictures of one size; for one call it is the renderer's target, with
  its own encoder submitted at once — the buffers it writes are the viewport's, so it is never
  encoded with the viewport's frame —, the viewport's target, the fingerprint of its picture
  and its statistics then put back: a still view stays still.
- **Cached** (`thumbnails::Thumbnails`, in the viewport, all documents together), each under
  a **key**: what it draws (its placements and their materials' colours, made again with the
  document's history revision), its meshes' revisions, the theme's colours it is drawn in (a palette
  changed, not only light / dark, draws them again), its size in pixels. The card
  asks for those it shows (`Thumbnails::get`): one missing or out of date is drawn at the
  viewport's next frame, **within 6 ms per frame** (the others the frames after), and
  registered as an egui texture (`register_native_texture`); the one drawn before shows
  meanwhile, the nature's icon until there is one. A component whose meshes are not all in
  the scene keeps its icon: an unused one (placed nowhere, so in no scene), one whose meshes
  the kernel has not given yet. At most 300 kept (`CAP`), the least recently shown freed
  first; a closed document's freed.

## Placing

- **Dragging a component into the view carries one more occurrence of it at the cursor** — a
  click only chooses it ([Managing](#managing)) —, as a paste
  ([Paste at the cursor](../tools/copy-paste.md#paste-at-the-cursor)):
  `Editor::place_component` applies `PlaceOccurrence` in the active context first (at its
  origin, `After::Carry`), then the Move tool carries it (`MoveTool::carrying`), with Move's
  inferences and locks — **no distance**: none drawn nor typed from where it was put in place,
  which means nothing to the user (no line, no length, no measurements box; the status
  `tool.move.hint.place`); a click
  puts it down (`Command::Replace`, `Moved::paste` composing the motion into the placement:
  one step in the history), Esc, Undo, another tool give it up. A carry under way is given up
  first. The component stays chosen while carried (the entity info shows it, the card marks
  it). Put down, the new occurrence is selected and the tool before comes back; another drag
  from the card places another one. **Why:** a click placing too made every click on the card
  start a carry — choosing a component to read it in the entity info, or the first click of a
  double-click to rename it (asked on 2026-11-19).
- **Shown only over the view**: from the drag's start on the card until the pointer reaches the view,
  and whenever it leaves it (over a card, out of the window), the carried occurrence is drawn
  neither where it was put in place nor at the cursor (`Preview::away`, set by the Move tool
  while it carries a component and the pointer is off the view); no copy toggle in the
  option bar (none for a paste either).
- **The drag**: the press is the first click, the release in the view the second
  ([Drag as click, move, click](../tools/interaction.md#drag-as-click-move-click)). While
  the row holds the drag, egui hovers nothing else, so the app tells the view
  (`Viewport::dragged_in`), which follows the pointer itself (`dragged_in_input`): over the
  view — not over a card floating on it (an egui layer other than the background, a tooltip
  aside) — the carried occurrence follows, the release there puts it down, a release
  elsewhere (back on the card) gives it up. The row's tooltip is not shown while dragged.
- **Held by the origin of its axes**, not by its box's low corner as a paste: the part's axes
  are its own ([A part's axes](../model/data-model.md#a-parts-axes)), as SketchUp's
  insertion point; oriented as the active context.
- **In the active context**: the model, or the assembly edited.
- **Disabled while a part is open**: the card is greyed but its search (a line under the
  search row says why); placing, replacing, deleting and purging wait for the part to be
  closed. A part's bodies come from bodies, not from occurrences
  ([Bodies](../model/bodies.md)); a part placed into one would need the paste's flattening
  ([0027](../decisions/0027-paste-across-the-part-boundary.md)).
- **No component into itself**: the assembly edited and every assembly above it, along the
  active path (`Editor::on_active_path`), are greyed (placing one of them would make a
  cycle): they are not dragged out (`Editor::can_place_component`).

## Managing

- **Choosing**: a click chooses the component (marked in the card; `Editor::choose_component`,
  the selection let go); the entity info then shows it as it shows the model
  ([The model row](interface.md#the-model-row)): its nature, its name, its source, a part's
  material and finish (all edited there; material and finish by `Operation::PaintPart`,
  on the definition whether placed or not), its dimensions, the materials it and its faces carry
  (`component_info`). Not its count in the model: the card's section says whether it is
  placed, the cut list counts it. It stays chosen until anything else is (a click or Esc in the view, a
  selection, a part opened: `Editor::chosen_component`).
- **Context menu** (right click), on the component under it. A right click **does not choose
  it**: choosing lets the selection go, which « Remplacer la sélection » takes.
  - « Sélectionner les exemplaires »: its occurrences under the active context
    (`Editor::occurrences_under_context`), as ids when all are the context's children, else as
    paths with the scope turned to « Pièces » (`Editor::select_occurrences_of`,
    [Selection scope](../tools/selection.md#selection-scope)); greyed when it has none there.
  - « Remplacer la sélection »: the occurrences selected become occurrences of it, each one
    keeping its placement, its occurrence name and its visibility
    (`Operation::ReplaceOccurrences`, grouped per parent with `Operation::At`). Enabled for a
    selection of occurrences that holds none of the assemblies it contains (a cycle) and is
    not only of it (`Editor::can_replace_selection`). At any depth, the paths grouped per
    parent (`Cx::moved_deep`), made unique first as the part scope says. The components left
    unused stay, in « Inutilisés ».
  - « Renommer » (also F2 on the chosen component, and a double-click on its row or tile):
    the name edited in place, as the navigator's (`Operation::RenameComponent`,
    `widgets::rename_field`; in the grid, under the tile).
  - « Supprimer »: the component and **all its occurrences**, at any depth
    (`Operation::DeleteComponent`, one step; the root refused, `Refusal::RootDeleted`); what
    it held stays, unused when placed nowhere else; the assemblies left empty go
    ([Empty assemblies](../model/data-model.md#components-parts-and-assemblies)). Disabled for
    the assembly edited and those above it.
- **« Purger les inutilisés »** (the card's menu, and `components.purge`): drops every
  component of « Inutilisés », with what they hold that the model does not place elsewhere — `Operation::PurgeUnused`, one step, undone as any.

## Search

- **One module for the library and the navigator**, **by name only**: the matcher
  in `tools` (`skraft_tools::search`: `Matcher`, `fold`; no UI, tested), the search field in
  the app (`search::field`), at the top of both cards' contents, under their header (in the
  card, under its tabs); the list under them takes the height left.
- **The field**: a magnifier, the hint « Rechercher », a ✕ inside its right end to clear it
  (drawn on the field's centre line, the buttons' pointer over it, not the text cursor), Esc in
  it clears it too. The line is the field's height (`search::line_height`), its items
  `search::GAP` apart (6 pt), the field as far from what is above it (the navigator's title
  bar line, the card's tabs) and from the list under it. In the navigator, over the whole
  width; in the
  library, left of the grid / list toggle. Handles `library:search`, `navigator:search`.
- **Name**: a substring of the name, case and diacritics ignored (« etag » finds
  « Étagère#2 »): both folded to lower case, the diacritics of Latin letters dropped by a table
  (`search::fold`, no Unicode normalisation crate), the ligatures spelt out (« œ » → « oe »),
  automatic names too. In the navigator a row matches by its component's name or its
  occurrence's name (a body's by its definition's or its own).
- **What the search does**: the library shows only what matches, in both tabs
  (the sections kept; the tabs' counts those it lets through; « Rien ne correspond » in a tab
  where nothing does), its rows made again when the document or the search changes. The
  navigator shows the rows that match and the rows above them (`cards::Filtered`): those that
  do not match themselves attenuated; no fold arrows meanwhile — what holds a match is shown
  open, the rest closed —, the folds left as they are ([Folds](interface.md#folds)), the
  active path not restricting the rows. Whether a definition holds a match is found once per
  definition, kept for the document's revision and the search (`NavigatorState`), so a search
  costs the definitions, not the placements. Emptied, everything is as before.
- **The searches are the document's**: kept per card in the document (`LibraryState.search`,
  `NavigatorState.search`, in `docs::Doc`), never on disk.

## Commands and driving

- `view.library` shows or closes the card; `components.purge` (the card's menu, the command
  palette).
- Control protocol ([Control protocol](../control-protocol.md), `crates/control/src/editor.rs`):
  `model.components` lists them (id, name, kind, named, count, unused) with the one chosen and
  whether the library is disabled; `components.place { component }` starts carrying one, put
  down by a `tool.input` click (Escape gives it up); `components.select`,
  `components.replace`, `components.delete` (`{ component }`) and `components.purge` do as
  the menu's, refused (422) when the menu's entry is greyed. Headless too.

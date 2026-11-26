---
status: built
code: [crates/tools/src/tools/pick.rs, crates/tools/src/tools/select.rs, crates/tools/src/tools/mod.rs, crates/tools/src/editor/mod.rs, crates/tools/src/editor/selection.rs, crates/tools/src/editor/tree.rs, crates/model/src/ops.rs, crates/model/src/clipboard.rs, crates/tools/src/tools/handle.rs, apps/skraft/src/workspaces/common/cards.rs, apps/skraft/src/workspaces/common/cards/, apps/skraft/src/workspaces/common/viewport/drop_up.rs, apps/skraft/src/workspaces/common/occurrence_name.rs]
---

# Select first, and a selection scope

How the tools that act on entities get them, after OpenCutList's `SmartSelectActionHandler`
(`tool/smart_tool.rb`): they **pick first** — the selection when they start, else a picking
phase — then act. A **selection scope** chooses what a pick takes: the children of the active
context (« Context »), or the part under the cursor at any depth (« Parts »), with OCL's tree
drop-up to go up its path.

Decision records: [Tools pick first](../decisions/0031-tools-select-first.md),
[Selection scope « Parts » as paths](../decisions/0029-selection-scope-parts-as-paths.md).

## Select first

- **One behaviour, OCL's** (not a preference): a tool acting on entities takes **the selection
  when it starts**; else it starts with a **picking phase** — a click takes the entity under
  the cursor, Shift clicks add or remove (the set taken once Shift is released), a rectangle
  takes what it encloses (left to right) or touches (right to left) — and only then does its
  action.
- **Move and Rotate**: picking first, then the base point (Move) or the centre (Rotate) — one
  click more than SketchUp's « the first click takes the entity and starts ». Once moved or
  rotated, **the selection is kept, taken again**: the next click is another base point or
  centre (SketchUp's chained moves); Esc lets it go and the tool picks again. Once copied
  (the copy modifier), the selection is let go, as the copy tools' (OCL's restart).
  **Why:** a part is often moved or turned in several goes (two quarter turns, a position
  adjusted); a tool that makes entities lets them go, its count typed after (`5x`) changing
  the last copy, and neither the source nor the copies being the obvious one to keep.
- **Erase and Paint keep the click that acts** (eraser, bucket): Erase lets the selection go
  when it starts (it never erases it; Delete does), then erases what it clicks; Paint still
  paints the whole selection when the click is in it.
- **Booleans ignore the selection**: they pick their targets, then their tools
  ([Solid tools](solid-tools.md)).
- Not concerned: the shape tools, Measure, Section, Profile.

### The picking module

- **One picking module** (`tools/pick.rs`): the `Picker` (click, Shift clicks, rectangle;
  `taken`, `selection`, `deep`, `shift_released`, `status(pick, gather)`), with a `Keep` filter
  (the Stretch keeps the context's occurrences: `Picker::keeping`; a guide near the cursor not
  kept, the solid under it). The copy tools, Distribute, Move, Rotate and the Stretch use it;
  the Stretch's plain click selects too. It picks with the scope (below).
- **One picking step**: the copy tools' « armed » state is the `Picker`'s (`aims`, `drop`,
  `dropped`), and they draw it with `Picker::preview` (the part hovered at any depth), as Move,
  Rotate and the Stretch do.
- Move / Rotate keep `taken` as the last input left it (`drag_clicks` has no `Cx`): picking, a
  drag is a rectangle (`DragClicks::No`); taken, a drag is clicks ([Interaction](interaction.md#drag-as-click-move-click)).
  A typed count after a copy still makes the array while picking.

### Esc, letting go, seeing the take

- **Esc**: `Tool::picks_first` — at the tool's first step (not busy), Esc lets the selection go
  and the tool picks again; while busy, one step back, the selection let go only when that step
  dropped the target (`Tool::dropped`). A quick tool hands back
  ([Quick tools](quick-tools.md)).
- **Let go after an operation** (a copy): `Command::Select([])` right after an operation still
  computed by the kernel (a body copied in an open part) turns its `After::Select` into `After::LetGo`:
  the selection kept until it lands (its ghost held), then cleared.
- **The tool sees what it took at once**: after an input that changed the selection, a tool
  that picks first is given the cursor again (a hover, not journaled) — its status, preview and
  `drag_clicks` follow without waiting for the next move.
- **Erase**: `switch_tool` clears the selection (`clear_selection`) when Erase starts; the
  quick tools select what was made *before* starting the tool.

## Selection scope

- **A shared setting**, read by Select and by every tool's picking phase: « Context » (the
  children of the active context) or « Parts » (the part occurrence under the cursor, at any
  depth, the active context unchanged). A **choice option** in the bar of Select and of every
  tool's picking phase, shared by all (saved once, not per tool), **Tab** toggling it as any
  choice: `PICK_SCOPE` (`context` / `parts`) saved under `SELECTION_SETTINGS`, added last to the
  tool's options by `Editor::tool_options` for a tool whose `Tool::picks_parts` (Select, Move,
  Rotate, Erase, Paint painting occurrences, the copy tools, Distribute, the Stretch), no part
  open, and drawn in the context card beside the tool's ([Context
  card](../ui/interface.md#context-card)), its icons two squares in a dashed frame (the
  context's children) or one square tinted beside a faint one on the diagonal (a part among others); changing it lets the selection go; `Cx::parts`.
- **Parts only**: the leaf part. Going up is OCL's tree drop-up (below).
- **And the guides at any depth**: under « Parts », a guide near the cursor is taken before
  the solid under it, as under « Context », but at any depth under the active context — the
  context's own, an assembly's, a part's (`Cx::guide_at`) — by Select, every tool's picking
  phase (not the Stretch's: `Picker::keeping` takes no guides) and Erase; a rectangle takes the
  guides it encloses or touches with the parts (`marquee_guides`). Shift adds or removes a
  guide while keeping the parts, and a part while keeping the guides. Those within reach are
  drawn **half faded** (`ThemeColor::HalfFadedGuide`), between the context's and the faded
  others ([Tape measure and guides](tape-measure.md#display)). **Why:** « Parts » reaches the
  parts at any depth; leaving their guides out forced opening each part to pick one.
- Inside an open part, the selection stays the bodies' (no scope).
- The navigator's click selects in its context, entering it, whatever the scope.

### The selection as paths

- The context's selection stays ids of the active context (read in many places); « Parts »
  keeps its own lists of paths from the root — the parts' (`EntityPath`, `Editor::deep`) and
  the guides' (`GuidePath`: the occurrences to the guide's component, assembly or part, then
  its id; `Editor::deep_guides`) —, the ids list then empty. « Context » is the case where
  they all start with the active context.
- `Editor::deep` and `deep_guides` are written only through `set_selection` /
  `clear_selection` / `select_paths` / `select_parts_and_guides` (the ids' selection and the
  paths exclusive), pruned after each change (still there, under the context, no part open,
  guides shown); `Command::SelectPaths(paths, guides)`. The paths may name assemblies (chosen in
  the tree drop-up), pruned by `resolve`. Two selected paths leading to the same occurrence of
  the same definition are one.
- **Picking**: `Cx::part_at` (the solid under the cursor, under the context), `marquee_parts`
  (window / crossing per part: the rectangle takes **parts at any depth**, enclosed or
  touched, as the click does); `Picker` and Select take paths under « Parts ».
- **Drawing**: the highlight goes per path — the selection's marks and boxes per path, the
  original hidden while moved, the eraser's and the rectangle's parts and guides
  (`Preview::hover_guide`, `erasing_guides`, `Marquee::guides`); a rectangle under way that
  selects (`Marquee::selects`: Select, picking; not Paint's) shows the selection it would
  leave — what it takes, or with Shift (kept from the gesture's start, as the release reads
  it) the selection with it toggled: its parts marked selected, what it drops unmarked and
  unboxed (`SelectionDrawing::owners`), the guides it adds hovered; the navigator lights the rows
  of the paths; the entity info shows one part, or one guide of an assembly, as in its
  assembly.

### Operations at each parent

- The operations, which take one `context` and ids in it, are grouped per parent assembly —
  several groups make several operations, in one undo step — and a world transform is brought
  into each parent's axes.
- Model: `Operation::At { parent, unique, op }` — `op` (Transform, Duplicate, Array, Copies,
  Delete, SetHidden, MakeAssembly) on the assembly `parent` leads into, its `context` set
  when applied; `unique` makes the assemblies along it unique first, outermost first
  (`MakeUnique`, ids kept) — and `Operation::Batch(ops)`, applied in order, outcomes put
  together, one history step; `Document::shared_along(occurrences)`. An operation `At` / `Batch`
  selects nothing it makes (its ids are not the context's).
- `Moved::Deep { groups, unique }` (`Cx::moved_deep(paths, guides)`): Move / Rotate (copies
  and `5x` arrays too), Delete (`Cx::erase_deep`), Erase (click, sweep, Shift hides) at each
  parent; `unique` = the editor's part scope « only this one » and a path shared.
- **Guides at each component**: an assembly's guides join its group, whose operation splits
  them from the occurrences as at the context; a part's make a group of their own
  (`DeepGroup::part`), whose operation is the one on its bodies at the part (`TransformBodies`,
  `DuplicateBodies`, `ArrayBodies`, `CopyBodies`, `DeleteBodies`), the scope ignored (a part's
  guides change no geometry, [Tape measure and guides](tape-measure.md)); hiding and painting
  leave guides. A guide reached through two occurrences of its component is one: taken once
  (unless made unique, an assembly's), else it would move twice.
- **The copy tools and Distribute**: `Target::of_deep` — the box of the parts in the axes
  chosen (entity: the one part's; active: the model's; context: the active context's), each
  group's bounds taken in its own assembly (the guides selected with them copied along, not
  in the box; guides alone have no box, as under « Context »); their operation
  `Moved::Deep::copies` (`Operation::Copies` at each parent);
  Distribute's inferences leave them out.
- **The Stretch**: parts at any depth **of one assembly** are its target (`Operation::Stretch`
  takes a context path); of several, the status says so (`tool.stretch.hint.one_assembly`) and
  a click picks again ([Stretch](stretch.md)).
- **« Créer un assemblage »**: the parts (and guides) selected **in one assembly** put in a
  new assembly there (`MakeAssembly` taken by `At`), selected by its path, offered its name
  ([Naming what was just made](quick-tools.md#naming-what-was-just-made)); in several, refused,
  the status saying so (`status.group_one_assembly`). The command is enabled by a selection
  at any depth (`Editor::anything_selected`), as « Effacer ».
- **« Rendre unique »**: each element selected gets a definition of its own, whatever the
  « Portée »: the assemblies along its path are made unique first, so that only this occurrence
  changes (`MakeUnique` taken by `At { unique: true }`, one step). The toast names the element
  selected, not the assemblies made unique on the way.
- **« Éclater »**: the assemblies selected (the tree drop-up) put in their parent, the deepest
  first, in one step (`Explode` taken by `At`), what they held then selected by its paths. In
  an assembly placed several times, the « Portée » says whether all its copies change.
- **Paint**: under « Parts », the part under the cursor — the parts selected when it is one of
  them — painted by its path (`PaintParts`, paths from the root); painting parts (either
  scope), a drag draws a rectangle whose take is painted when released, one step (the context's
  occurrences, or the parts at any depth); the level chosen in the tree drop-up is painted, not
  selected (`Tool::tree_pick`); painting faces, no scope, no list.
- **The clipboard**: `Document::paths_fragment` (each occurrence and guide in world axes, an id
  met twice drawn afresh), the hull from each path and guide; cut erases them as Delete does
  ([Copy, cut and paste](copy-paste.md)).

### Making assemblies unique

A part reached through an assembly placed several times: changing it changes every copy of that
assembly. **Making unique is the « Portée » option** (« all » / « only this one »), shown only
when a selected path goes through an assembly placed more than once (`viewport::options::deep_scope`, for
Select, Move, Rotate, Erase, Paint, the copy tools and Distribute — « Tous » without a count);
« only this one » makes the assemblies along it unique when the tool acts, in the same undo
step (OCL's `_make_unique_groups_in_path`; `Operation::At::unique`, `MakeUnique` alone goes one
level).

### The tree drop-up

OCL's (`editor/tree.rs`, `apps/skraft/src/workspaces/common/viewport/drop_up.rs`): under « Parts », while the tool picks
(Select, Erase, Paint always; the others with nothing taken), the copy modifier (Option on
macOS) pressed over a part of an assembly lists its path — the list's bottom right corner on
the cursor —, the model's level first, the levels above the active context disabled; a row
hovered is drawn hovered in the view, a row clicked selected (`select_paths`: the part, or an
assembly); the key released closes it, and the tap it makes is swallowed (no copy, no mirror
toggled). Over a part of the active context itself, no list: the key keeps its meaning. Once
picking is over the key keeps its other meanings (copy in Move / Rotate, mirror in the copy
tools), as in OCL where the state decides. The status says it (« Option maintenu : élément
parent », « Choisir un élément dans la liste »).

**One name writer** (`apps/skraft/src/workspaces/common/occurrence_name.rs`), for the navigator's rows and the
drop-up: the occurrence's own name in semibold, then its definition's between faint angle
brackets, own name or not (an automatic one in italics, greyed).

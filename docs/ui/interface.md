---
status: built
code: [apps/skraft/src/app.rs, apps/skraft/src/dock.rs, apps/skraft/src/panels.rs, apps/skraft/src/toasts.rs, apps/skraft/src/prefs.rs, apps/skraft/src/icons.rs, crates/tools/src/tools/mod.rs, crates/tools/src/editor/selection.rs]
---

# Application interface

The application window around the 3D view: the floating cards (navigator, entity info,
materials, tool option bar), the toasts, the shortcuts, the two levels of settings and the
tool option bar's contract with the tools. The title bar, the ribbon, the view bar and the file
tabs have pages of their own.

## Layout

- Title bar at the top ([Title bar and native menus](title-bar.md)), status bar at the bottom
  with the measurements box (VCB) on the right; in between, the **3D view takes the whole
  width** and the panels **float over it** (anchored cards): **the workspace card and the
  ribbon across the top** ([Workspaces and ribbon](ribbon.md)), the tool option bar centered
  under the ribbon, the view bar along the left or right edge ([View bar](view-bar.md));
  **Entity info**, **Materials** and **Outliner** are three cards docked in two columns, one
  per edge of the view (`dock.rs`), under the ribbon. A card's header (a title bar in the
  headers' fill, a line under it, its title semibold, its chevron at the right end) folds it
  (click) or moves it (drag: it follows the pointer and drops into the column of the nearer
  edge, where a bar shows the insertion point); the commands `view.entity_info`,
  `view.materials` and `view.outliner` (command palette, shortcuts) close or show each one
  (`PanelId::command`). A card keeps the height given by dragging its bottom edge, except the
  last unfolded list of its column (Materials or Outliner; never Entity info, which would only
  grow empty: `PanelId::fills`), which fills the height left; the inner edge sets the column's
  width (`dock::COLUMN_WIDTH` 270 pt by default, `COLUMN_MIN_WIDTH` 200 at least; double click:
  default). Order, columns, sizes, folded and closed cards are kept in the preferences
  (`panels`), written once the pointer is released. Opening or resizing a panel therefore
  never resizes the view; zoom extents frames the area left free.
- The viewport is the image of `render`'s off-screen target, registered as an egui texture;
  rendering is submitted during `update`, before egui paints.
- Wheel ("line" units) = zoom; trackpad scroll ("point" units) = orbit, Shift = pan (the
  navigation style's trackpad gestures, [Navigation styles](navigation-styles.md)); pinch =
  zoom. Copy modifier: Option on macOS, Ctrl elsewhere (short tap = toggle,
  [Interaction](../tools/interaction.md)).

## Navigator

- **The outliner is titled « Navigateur »** (the code keeps `outliner`, its `PanelId` being in
  the preferences and `model.outliner` in the control protocol). Its rows span the whole list
  (`tree_row`): the hover and the selection fill the line edge to edge, indent and buttons
  included, a click anywhere on it but on its buttons (fold, eye, sign) choosing it; a part's
  bodies lie on a band of their own (`band_color`: the card's colour a shade towards the text;
  white in the light theme), the tree of assemblies and parts on the card's, the band and the
  hover reaching the card's border across its margin (`panels::CARD_MARGIN`). The tree follows
  the path being edited down to its end: a part's bodies under it (a single one too), a group's
  under it, and under a body with a profile (an extrusion, a revolution) its « Profil »
  (`icons::PROFILE`), a click editing it (`Action::EditProfile`, `Editor::edit_body_profile`:
  its part and group opened, the tool « Profil » on it). Only the deepest step of the path is
  marked selected: a body selected, not its part; its profile edited (`Editor::profile_body`,
  from `Tool::profile_body`), its « Profil » row, not the body nor the part — its body
  unfolded; the entity info then shows the profile (its body, its corners). A body row shows
  its nature's icon, not its tool's: an extrusion `icons::EXTRUSION` (an ISO square, a vertical
  arrow from its centre), a revolution `icons::REVOLUTION` (a profile standing on its dashed
  axis, an ISO circular arrow around it) — the tools « Prisme » and « Tournage » keep theirs; an
  assembly `icons::COMPONENT` (three ISO cubes about one corner, one up and two down, also
  « Créer un assemblage »'s).

### The model row

The model is on the navigator's first row: named as its file (« Sans titre »), `icons::MODEL`
(a page holding a cube), semibold, at the list's left edge (no indent, no fold nor eye),
scrolling with the tree (row 0 of `show_rows`); alone in an empty model (no hint there). A click
(`Action::SelectModel`, `Editor::select_model`) leaves every assembly and part edited, the
profile's editing with it, and chooses the model: highlighted, the entity info showing it — its
name, its parts and assemblies as placed (one per occurrence at any depth, each definition
walked once), its materials. It is chosen only so (`Editor::model_chosen`), until anything else
is: a click or Esc in the view, a selection, an entry (`model_selected` dropped then, checked
again in `poll`); nothing selected otherwise, « Aucune sélection ». The tree's scroll area keeps
half the rows' spacing above and below (`content_margin`): a row's background spills that much
beyond it, so the first row's is not clipped.

### Following the active path

The navigator follows the active path (`ActivePath`: the assemblies edited, the part open, its
groups open, the body whose profile is edited). Above the path's end each level lists only the
path's step — the open part's siblings too, a group's, the profiled body's —, the end's level
all of its items. The path gone up (or elsewhere), what was unfolded along it beyond what both
share is folded (`ActivePath::fold_left`, the last path kept per document). The fold arrows
stay, to look into an assembly or a part without entering it.

### Folds

- **A new row opens folded**: an undo gives the document's next id back, so the next copy takes
  the id of the one undone, and would inherit its fold (kept by id). The ids given back
  (`Document::next_id` going down) are reborn (`Births`): a row's fold is kept under its ids
  salted with their births, nothing salted while no undo gave an id back. A step redone opens
  folded as well.
- **The folds are the document's**: the rows unfolded, the births and the active path last seen
  are kept in the document (`panels::OutlinerState`, in `docs::Doc`), dropped with it, never on
  disk. A row's fold is written only when it changes (its chevron clicked, the active path
  reaching or leaving it); a frame reads one hash set per row.
  **Why:** every document numbers its things from 1, so fold states keyed by ids alone and
  persisted (egui's `CollapsingState`s, in `app.ron`) open a new part unfolded because a part of
  the same id was unfolded in another tab or an earlier session, and pile up in egui's memory.
- Likewise, at launch (`prune_egui_memory`), the app prunes what egui keeps on disk and never
  drops: its text fields' states (keyed by model ids for the renames and positions), the places
  of every area ever shown (each tooltip's, each menu's: 750 in a month) but the windows the
  user moves (preferences, journal); the app's other areas are placed every frame.

## Entity info

The entity info shows the selection, whatever it is: an assembly or a part (its name,
occurrence, material, dimensions, position…), a guide, and a part open, the body or group
selected in it — its name as the navigator gives it (« Extrusion 3 »), blank or machining (read
only: the sign is toggled in the navigator), failed or empty, its solid's size, how many bodies
share it —; the model when its navigator row is clicked; nothing selected, « Aucune
sélection ». No bodies count nor topology row: the navigator lists the bodies, `model.geometry`
gives the topology.

## Toasts

`toasts.rs`, after OpenCutList's notifications (`SmartTool#notify`): the app's messages — a
file not opened or not saved, the preferences or the control channel failing, a command that
cannot run, too many documents open, « modele.skraft enregistré » — and the editor's notices
(`Editor::take_notices`: an operation failed, with the kernel's or the model's words; a display
failed; a STEP file imported, empty, failed, cancelled; an operation abandoned) stack at the
bottom center, above the status bar, which keeps the tools' hints.

- Four levels: info (neutral), success (green), warning (amber, an operation's
  `Outcome::warning`, the warning sign at its left), error (red), each a border of its colour on
  a pale wash of it (a dark one in a dark theme), the title in the colour and what went wrong
  under it, smaller.
- A click on a toast closes it. An info, a success or a warning goes after 5 s, a bar along its
  bottom emptying towards the left; the pointer over it stops the countdown, full again. An
  error stays.
- A toast may carry a button at its right end (as OpenCutList's « Ouvrir » after an export),
  whose click runs it and closes the toast: « Renommer » on « Nouvelle pièce créée »
  ([Quick tools](../tools/quick-tools.md)), the toast dropped once its part is gone. `Act::Reveal`
  (show a file in the Finder or the Explorer) exists but no toast offers it.
- The same message again counts (×2); six at most, the oldest timed one dropped first.
  `app.state` lists them (`toasts`); handles `toast.<i>` and `toast.<i>.action`. An editor
  nobody takes notices from (headless) keeps its last 32.

## Icons

Original SVG icons (drawn for the project), white and tinted by the theme, loaded by
`egui_extras` (`svg`): no build step, no binary asset.

## Shortcuts

SketchUp's: Space = Select, E = Eraser, M = Move, Q = Rotate, G = Make assembly, T = Tape
measure, Cmd/Ctrl+Z / Shift+Cmd+Z = Undo / Redo, Shift+Z = Zoom extents; ours: R = Box,
C = Cylinder, P = Prism, L = Turn, B = Paint, S = Stretch, W = Hide the rest of the model,
Cmd/Ctrl+K = command palette, Cmd/Ctrl+1…6 = standard views, Cmd/Ctrl+, = Preferences. They all
come from the command registry ([Command registry and external driving](commands.md)) and can
be changed in the preferences. Delete (or Backspace) is not a command: the editor erases the
selection, unless the tool takes the key (`Tool::takes_delete`: the profile's corner under the
cursor). Once a digit is typed (or `@ ( [ < + - * / ;`), keystrokes go to the VCB, confirmed by
Enter. Menus and tooltips write shortcuts as the platform does ([Texts and languages](texts.md)).

## Settings

Two levels, after SketchUp's styles.

- **Document settings** (`model::Settings`): display (grid — hidden by default —, axes…) and
  units (length unit, decimals). Saved in the file; changing them is an operation
  (`Operation::SetSettings`), hence undoable, and successive changes merge into a single step.
  Not the view's background: a token of the palette ([Rendering and look](../render/look.md)) —
  a file never sets how the app looks. Lengths stay stored in mm: the unit only serves for input
  (a bare number is in the document's unit) and display.
- **User preferences** (`apps/skraft/src/prefs.rs`): language ([Texts and languages](texts.md)),
  navigation (orbit speed and direction, mouse and trackpad separately; zoom speed and target;
  the navigation style, [Navigation styles](navigation-styles.md)), the interface (theme,
  palette, panels, ribbon labels), the tools' settings and the settings of new models. A
  versioned JSON file of our own, `preferences.json` in the OS's configuration folder
  (`~/Library/Application Support/Skraft/` on macOS), `localStorage` in the browser; rewritten as
  soon as a preference changes.
- `#[serde(default)]` everywhere: an older or partial file loads, a new setting needs no
  migration.
- **Preferences** window (Cmd/Ctrl+,): *General* tab (language, reopen documents at launch,
  memory of the history), *Model* tab (document settings, with "Use for new models" / "Restore
  the default settings"), *Navigation*, *Interface*, *Shortcuts* and, on the desktop, *Débogage*
  (the control channel, [Command registry and external driving](commands.md)). Its tabs are the
  ribbon's (`ribbon::tab_strip`, as the journal window's), the window at least as wide as they
  are. The commands `view.grid` and `view.axes` change the document too.

Decision record: [Document settings and user preferences](../decisions/0003-document-settings-and-user-preferences.md)

## Tool option bar

A small card of its own, centered under the ribbon, with the tool's options — exclusive choices,
toggles and lengths; no tool name, and no card at all for a tool without options (nor open
part). Every choice is visible and clickable there; a shortcut is only a complement (Tab cycles
the tool's first choice, `Editor::next_choice`). The status bar keeps the guidance for the
current step; what Tab does is not written in the tools' hints but added by the editor
(`Editor::tab_hint`): « · Tab : <choice> → <the value it gives> », only while the tool shows a
choice — none for a part placed once ([Data model](../model/data-model.md)). Tab and Shift+Tab
never reach egui's keyboard navigation while no text field has the focus (`App::raw_input_hook`
takes them out for the tool): egui would focus the first widget, then keep every key from the
tool. In a text field they move between fields; a shortcut being captured or a modal dialog gets
them as they are.

### Tools describe their options

**Tools describe their options, the app draws them**: `Tool::options` returns a list of
`ToolOption` (key, label — which may depend on what is hovered, "×4" —, tooltip, value,
choices), `Tool::set_option` receives a change. **Why:** `tools` stays without an interface: a
test changes an option like a click does.

- **Two kinds of options**: **settings** (the scope…), remembered per tool in the user
  preferences (`tool_options`), read by the tool from its `Cx`; and the **gesture's state**
  (copy, for Move and Rotate), which the tool holds itself and which is not remembered.
- The options are each tool's (its page lists them): the booleans' scope (all occurrences /
  this one), copy (Move, Rotate), the axes (Stretch, the copy tools), the shapes' offset and
  fillet, among others.

### Option icons

A choice's value or a toggle may name an icon (`icon`, a name the app maps to
`apps/skraft/assets/icons/options/<name>.svg`, `icons::option`; `union` and `subtract` are the
booleans' icons, shared with the open part's « Solide »); the bar then draws the icon, its label
going into the tip. Drawn after OCL's motifs (`get_action_option_btn_child` of Smart Reshape and
Smart Draw): Stretch, Box, Cylinder; the radius and "from a corner" have none in OCL, drawn in
the same style. The option's name stays as text before a choice. The card's items are centered
in it.

### Look

The bar's buttons are drawn like the ribbon's (no frame until hovered, the selection's fill and
outline when on), an icon's button square. A choice is a **segmented control**: its values side
by side, drawn like the ribbon's buttons (no track, no outline around the group), the chosen
one's mark — a selected ribbon button's fill and outline — sliding to the value chosen, easing
out in 0.18 s (`panels::segmented`). Each choice comes after its name (no colon, grey like a
caption), the groups apart by separators; the lone toggles come last, together in an
**"Options"** group, without separators between them.

### Length options

For the shapes' offset: `OptionValue::Length` (mm), a group of the bar (its name as the
caption), before the choices, its value in the document's units in a text field always framed,
its text right-aligned (the field's ground and an outline): the outline alone tells the state —
the border's colour at rest, the hovered ink's, the selection's when not zero, twice as thick
when focused. Focused, all of it selected; Enter applies, Esc or a click away puts the value
back. Read like the measurements box (`measure::coordinate`, `@` the current value). No modal
dialog, unlike OCL. The measurements box sets it too, at any step: a text ending in `x` (`3x`,
`-2mmx`) goes to the active tool's first length option (OCL's `_read_offset`), unless the box
takes a count of copies (Move's `5x`).

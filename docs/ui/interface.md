---
status: built
code: [apps/skraft/src/about.rs, apps/skraft/src/app.rs, apps/skraft/src/dock.rs, apps/skraft/src/panels.rs, apps/skraft/src/toasts.rs, apps/skraft/src/prefs.rs, apps/skraft/src/icons.rs, crates/tools/src/tools/mod.rs, crates/tools/src/editor/selection.rs]
---

# Application interface

The application window around the 3D view: the floating cards (navigator, entity info,
materials, tool option bar), the toasts, the shortcuts, the two levels of settings and the
tool option bar's contract with the tools. The title bar, the ribbon, the view bar and the file
tabs have pages of their own.

## Layout

- Title bar at the top ([Title bar and native menus](title-bar.md)), status bar at the bottom
  with the measurements box (VCB) on the right; in between, the **3D view takes the whole
  width** and the panels **float over it** (anchored cards): **the ribbon at the top**, the
  workspace switcher at its left end, between the docked columns or above them when it does not
  fit there ([Workspaces and ribbon](ribbon.md#layout)), the context card and the tool option bar
  under the ribbon ([Context card](#context-card)), the view bar along the left or right edge,
  centered on the view's height ([View bar](view-bar.md));
  **Entity info**, **Materials**, **Outliner** and **Components**
  ([Components library](components-library.md), closed at first) are four cards docked in two columns, one
  per edge of the view (`dock.rs`), over the view's whole height — under the ribbon when it is
  above them. A card's header (a title bar in the
  headers' fill, a line under it, its title semibold, its chevron at the right end) folds it
  (click) or moves it (drag: it follows the pointer and drops into the column of the nearer
  edge, where a bar shows the insertion point); the commands `view.entity_info`,
  `view.materials`, `view.outliner` (on a sheet, the drawing's tree:
  docs/ui/drawing.md#navigator), `view.components` and, in a workspace of sheets,
  `view.compositions` (command palette, shortcuts) close or show each one
  (`PanelId::command`). A card keeps the height given by dragging its bottom edge, except the
  last unfolded list of its column (Materials, Outliner, Components or Compositions; never Entity info, which would only
  grow empty: `PanelId::fills`), which fills the height left; the inner edge sets the column's
  width (`dock::COLUMN_WIDTH` 270 pt by default, `COLUMN_MIN_WIDTH` 200 at least; double click:
  default). Order, columns, sizes, folded and closed cards are kept in the preferences
  (`panels`, per workspace: [Workspaces](workspaces.md#panels)), written once the pointer is
  released. Opening or resizing a panel therefore
  never resizes the view; zoom extents frames the area left free.
- The viewport is the image of `render`'s off-screen target, registered as an egui texture;
  rendering is submitted during `update`, before egui paints.
- Wheel ("line" units) = zoom; trackpad scroll ("point" units) = orbit, Shift = pan (the
  navigation style's trackpad gestures, [Navigation styles](navigation-styles.md)); pinch =
  zoom. Browsers report wheel and trackpad both in pixels: `web/wheel.js` tells them apart, by
  `wheelDeltaY` in Chrome, by how a gesture starts in Safari (a wheel's first event is exactly
  4 px), and hands a wheel on in lines. Copy modifier: Option on macOS, Ctrl elsewhere (short
  tap = toggle, [Interaction](../tools/interaction.md)).

## Navigator

- **A search field** at its top, over the whole width, shared with the « Composants » card: the
  tree filtered to the rows matching a name, and the rows above them
  ([Components library: search](components-library.md#search)).
- **A row dragged** carries a translucent copy of itself under the pointer, in the model's tree as
  in the drawing's (`panels::paint_drag_ghost`): its icon, its name, « +n » for what goes with it.
- **A right-click on a navigator row opens its menu**: « Renommer » (the field a double-click
  opens; on a row with an instance name, « Renommer l'exemplaire » and « Renommer la définition »); on a body, « Sortir du groupe » (« Sortir de la pièce » at the part's root), « Rompre le lien » on a linked one; on a part off its linked machining, « Replacer », « Amener l'usinage ici », « Séparer » ([Links](../model/bodies.md#links)); then « Aplatir » on a group or a part with a tree, « Éclater » on an assembly or a group, « Effacer ». Separators set these groups apart.
  Opening it changes nothing; its row stays highlighted while it is open. Its commands act on the whole selection when the row is in it,
  else on the row alone, chosen as a click would.
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

### Properties

Under those rows, the entity's **properties** ([Properties](../model/properties.md),
`panels/props.rs`): « Propriétés » for a part or an assembly, a body's definition, a guide, the
model; « Propriétés de l'exemplaire » for the occurrence, or the body, selected. A row per
definition applying to the entity, in its keys' order — its value, else its default greyed —,
then the values without a definition, their key greyed (the tooltip says so); a value set has a
button to remove it. The editor follows the kind: a check box, a choice (framed as the
material's), a colour button, else a framed field typed as the measurements box reads (`@`,
expressions, the document's units; a money's currency after its amount, `12 €`, `@*2 usd`).
Nothing shown when the entity has no value and no definition applies to it.

**The definitions** are the model's: its entity info (its navigator row clicked) lists them
under « Définitions des propriétés », one fold each — name, kind (text, integer, number,
yes / no, length, area, volume, angle, mass, price, colour, choice, multiple choice, list of
lengths), choices (`;`-separated, for a choice), default, the entities it applies to (all
checked: all), its key (read only) — and a delete button (the values stay). « Nouvelle
propriété » adds `user.property_<n>`, a text. Rows are laid at widths fixed from the card's
room (`Rows`), not in a `Grid`: a label wider than planned would widen the card at every
relayout.

## Toasts

`toasts.rs`, after OpenCutList's notifications (`SmartTool#notify`): the app's messages — a
file not opened or not saved, the preferences or the control channel failing, a command that
cannot run, too many documents open, « modele.skraft enregistré » — and the editor's notices
(`Editor::take_notices`: an operation failed, with the kernel's or the model's words; a display
failed; a STEP file imported, empty, failed, cancelled; an operation abandoned) stack at the
bottom center, above the status bar — and above the [steps bar](#steps-bar) when it shows —,
the status bar keeping the tools' hints.

- Four levels: info (neutral), success (green), warning (amber, an operation's
  `Outcome::warning`, the warning sign at its left), error (red), each a border of its colour on
  a pale wash of it (a dark one in a dark theme), the title in the colour and what went wrong
  under it, smaller.
- A click on a toast closes it. An info or a success goes after 5 s, a bar along its bottom
  emptying towards the left; the pointer over it stops the countdown, full again. A warning or
  an error stays.
- A toast may carry a button at its right end (as OpenCutList's « Ouvrir » after an export),
  whose click runs it and closes the toast: « Renommer » on « Nouvelle pièce créée », « Nouvel
  assemblage créé », « Nouveau groupe créé », « … rendu unique »
  ([Naming what was just made](../tools/quick-tools.md#naming-what-was-just-made)), the toast
  dropped once what it names is gone. `Act::Reveal`
  (show a file in the Finder or the Explorer) exists but no toast offers it. It may carry a
  second one: « Replacer » and « Séparer » after a linked body moved, « Retirer » and « Garder »
  after a machining lost its last source ([Links](../model/bodies.md#links)), dropped once the
  document has changed.
- The same message again counts (×2); six at most, the oldest timed one dropped first.
  `app.state` lists them (`toasts`); handles `toast.<i>`, `toast.<i>.action` and `toast.<i>.also`. An editor
  nobody takes notices from (headless) keeps its last 32.

## Steps bar

`steps.rs`, after OpenCutList's `SmartTool#set_steps_bar` and `#show_validation`: what the
active tool tells — any tool, through two methods of `Tool` (none by default) —, the steps at
the bottom centre of the view, under the toasts, the validation at the top centre, under the
tool's options:

- **`Tool::steps`** (`Steps { count, index, text }`): the steps' numbers, linked, the current one
  filled, those passed in the text's colour, the next ones dimmed; below, the step's main
  sentence framed (the status bar keeps the hint with its keys). `count` 0: the sentence alone.
  The block stays at the view's centre, whatever the side panels' widths; the sentence keeps to
  its own lines (`\n`), measured, unless wider than the room about the centre that neither panel
  covers (twice the distance to the nearer one): then wrapped to it.
- **`Tool::awaits_validation`**: two buttons, ✗ red and ✓ green (OCL's motifs; the colours of
  the error and success toasts, swapped under the pointer), which give the tool **Esc** and
  **Enter** (`Action::ToolKey`, through `Editor::input`: the journal and the recordings see the
  key). Hidden while the kernel computes (`Editor::tool_awaits_validation`).
- Shown by the solid tools ([Solid tools](../tools/solid-tools.md)): A then B; the validation
  while a set gathers. `tool.state` gives both (`steps`, `awaits_validation`); handles
  `validation:cancel`, `validation:ok`.

## Icons

Original SVG icons (drawn for the project), white and tinted by the theme, loaded by
`egui_extras` (`svg`): no build step, no binary asset.

## Tips

A widget's tip (tooltip) shows centred under it, above it when there is no room below — egui
puts it under its left edge: `.tip(text)` / `.tip_ui(add)` (`style::Tip`) in place of egui's
`on_hover_text` / `on_hover_ui`, the text also the widget's accessible description as egui's.
A tip of a name and what it does (`.tip_more(text, more)`) puts the second smaller, on a line of
its own, no separator: an option drawn as an icon (its name, then its tip). The ribbon's tips
are a command's name and shortcut alone: its options and steps are the option bar's. Its text wraps at 260 pt (egui's
500), a description on a few short lines. It comes after 0.15 s, the next ones at once while
the pointer goes from widget to widget. The tips egui shows by itself (a label cut short, whole
in its tip) keep egui's place; the view's tip by the pointer keeps off the cursor
([Cursors](cursors.md)).

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

## About

« À propos de Skraft » (`app.about`: the File menu, the palette, the app menu on macOS, in place of
AppKit's panel) shows the version, the copyright, that it is free software without warranty, and
the main projects it is built with (each a link, what it does here, its license), and links to
the license, all the third-party licenses and the source of this version (its `v*` tag).
**Why:** the GPL v3 (section 5d) asks an interactive program to show these notices. Links open in
a new tab: on the web the page holds the unsaved work. The third-party notices are a file beside
the app, not in its binary: `THIRD-PARTY-LICENSES.txt`, from the installer or served next to the
web build (`scripts/third-party-licenses.sh`). A desktop build without it reads the copy among
its GitHub release's files.

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

A small card of its own under the ribbon, with the tool's options — exclusive choices, toggles
and lengths; no tool name, and no card at all for a tool without options. Its buttons and
fields are 26 pt high (`style::BAR_BUTTON`), as the view bar's and the quick tools' card's
buttons: the floating bars over the view all as thick. Every widget hovered or pressed
— buttons first — is outlined in the colour of a text field's focus (the theme's hovered and
active strokes, `style.rs`); a focused field's thicker outline takes over. The choices that are
not the tool's but the context's go to the context card beside it (below). Every choice is
visible and clickable there; a shortcut is only a complement (Tab cycles the first choice,
`Editor::next_choice`). The status bar keeps the guidance for the current step; what Tab does is not written in the tools' hints but added by the editor
(`Editor::tab_hint`): « · Tab : <choice> → <the value it gives> », only while the tool shows a
choice — none for a part placed once ([Data model](../model/data-model.md)). Tab and Shift+Tab
never reach egui's keyboard navigation while no text field has the focus (`App::raw_input_hook`
takes them out for the tool): egui would focus the first widget, then keep every key from the
tool. In a text field they move between fields; a shortcut being captured or a modal dialog gets
them as they are.

### Context card

The choices common to every tool that shows them, not the tool's own, are in a **card of their
own, left of the tool's**: **« Sélection »**, what a pick takes ([Selection
scope](../tools/selection.md#selection-scope), the tools that pick parts, no part open), then
**« Portée »**, which copies a change reaches (the open part's scope, [Opening a
part](../model/opening-a-part.md), or the parts selected at any depth through a shared
assembly). The two cards are centered together under the ribbon, between the dock's columns
(`App` lays them out, `panels::context_options` and `panels::tool_options` fill them, in `panels::option_card`: 3 pt
above and below the buttons, as the view bar, 6 on the sides), each translucent at rest and
nearly opaque under the pointer or while a field of it is typed in, as the view bar
(`style::hover_card`). The open
part's « Solide » stays in the tool's card: only the shape tools show it. **Why:** these choices
are not the tool's — switching from Select to Move to Rotate keeps them —, a card apart says so,
always first; mixed in the tool's options, they sat among them at a place of each tool's. The
row centered as a whole, not split at the view's center (the context card ending there, the
tool's starting there): that kept the context card still from one tool to the next but left
nothing centered. Tab still cycles the first choice of all (`Editor::next_choice`): the selection's scope
for a tool without a choice of its own.

### Tools describe their options

**Tools describe their options, the app draws them**: `Tool::options` returns a list of
`ToolOption` (key, label — which may depend on what is hovered, "×4" —, tooltip, value,
choices), `Tool::set_option` receives a change. **Why:** `tools` stays without an interface: a
test changes an option like a click does.

- **Two kinds of options**: **settings** (the scope…), remembered per tool in the user
  preferences (`tool_options`), read by the tool from its `Cx`; and the **gesture's state**
  (copy, for Move and Rotate), which the tool holds itself and which is not remembered.
- The options are each tool's (its page lists them): the booleans' scope (all occurrences /
  this one) and what they keep ([Solid tools](../tools/solid-tools.md)), copy (Move, Rotate), the axes (Stretch, the copy tools), the shapes' offset and
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
**"Options"** group, without separators between them. The fills and outlines — the marks, a
button hovered, the length fields' ground and outline — fade with their card while it is
translucent (`style::fill_alpha`), their ink not; a toggle's swatch (« Pipette »'s material)
keeps its colour. A rail around each choice and each toggle (the values of a choice in one
hollow track, a toggle a rail of one cell) was tried (2026-11-18) and dropped for the ribbon's
look.

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

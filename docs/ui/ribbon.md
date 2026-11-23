---
status: built
code: [apps/skraft/src/ribbon.rs, apps/skraft/src/icons.rs, apps/skraft/src/prefs.rs, apps/skraft/src/app.rs, apps/skraft/src/app/]
---

# Workspaces and ribbon

The ribbon and its workspace switcher: the tools and commands of the app, grouped by tab and
group, across the top of the view. The ribbon follows the workspace / tabbed-toolbar pattern of
CAD tools (CATIA and FreeCAD workbenches, SolidWorks' CommandManager, Fusion). The pattern is
free to use; the drawing is ours: our icons, palette, names and proportions — nothing traced
from another product.

Decision record: [A ribbon with workspaces](../decisions/0015-ribbon-with-workspaces.md)

## Layout

One floating card across the top of the view, under the title bar: at its left end the
**workspace cell**, over the card's whole height (`WORKSPACE_WIDTH` 92 pt): the workspace's
icon over its name, centred above the groups' label line, and the chevron of its menu alone on
that line, at the height of the groups' own; it reacts over its whole surface, the card's margin
included up to the separator (hover filled with the card's corners on its left), and a click
lists the workspaces ([Workspaces](workspaces.md#the-switcher)). A line across the card parts it
from the rest;
then a row of tabs — always, a workspace has one tab at least (a test checks it) — over a row
of groups (the tabs drawn as the title bar's file tabs: centered on
a band in the headers' fill, the band starting from the workspace cell's line, the shown tab
opening onto the groups). The card is as wide as its content and centered on the view, within
its span:

- **between the docked cards' columns** (a split stage's table counting as one, the ribbon then
  centred on the view between it and the right column: [Workspaces](workspaces.md#stage)) when
  it fits there whole (its natural width,
  `ribbon::natural_width`): the columns then take the view's whole height;
- **else above them**, over the view's width less its margins, the columns starting under it; a
  window narrower than the ribbon narrows it ([Narrow window](#narrow-window) decides what gives
  way).

The choice depends on the widths only (the window's, the columns' shown), not on earlier frames:
no flicker; a column's edge dragged may move it from one to the other. The view is never resized
([Application interface](interface.md)); the **tool option bar** is a small card of its own
under the ribbon, the context card left of it ([Context card](interface.md#context-card)); zoom
extents frames what is left below it. The render statistics card sits in the top right corner of
the area the cards leave free (`Editor::free_area`), under the ribbon.
**Why:** a workspace card of its own left of the ribbon made the top row wider than the space
between the columns, which had to start under it.

## Workspaces

The workspace shown is the document's, each with its own ribbon (its fixed group and tabs):
[Workspaces](workspaces.md). *Modelling* only for now.

## Tabs of Modelling

Each tab a list of groups, each group a list of commands of the registry
([Command registry and external driving](commands.md)) — a command may be in several groups:

- **Build** « Construire »: Draw « Dessiner » (Box, Cylinder, Prism, Turn), Manipulate
  « Manipuler » (Move, Rotate, the copy tools: in line, in a grid, distribute), Reshape
  « Remodeler » (Stretch, Union, Difference, Intersection), Appearance (Paint, « Prélever »);
- **Assemble** « Assembler »: Components (Make assembly, Explode, Make unique, Group bodies,
  Ungroup bodies) — the manipulating tools stay in Build only;
- **Inspect** « Inspecter »: Measure (Tape measure, Delete guides; Tape measure pinned), Section
  (Section plane, Clear section; Section plane pinned), View (Zoom extents, the standard views;
  Zoom extents pinned).
- A fixed **Selection** group (Select, Eraser) stays at the left of every tab, before the tabs'
  own groups.
- The tab shown is remembered per workspace (preference). A shortcut runs its command whatever
  the tab (among the workspace's commands, [Workspaces](workspaces.md#commands-available)): the
  ribbon does not switch tabs; a running tool whose button is not shown lights its group's label
  instead.

## The ribbon is data

A `const` description in the app (`workspace::WORKSPACES`: workspaces → tabs → groups → command
ids, with each group's default pins), checked by a test against the registry (every id exists,
no group is empty). The ribbon draws it; state, labels, tips and enabled/checked come from the
registry's `CommandView`s, as for the menus. The active tool's button is filled; a tool
waiting under it (the [tool stack](../tools/interaction.md#tool-stack), `CommandView::waiting`)
shows its frame alone, lighter.

## Pins

A group shows its **pinned** commands as buttons; its label (with ▾) opens its menu, which lists
**every** command of the group, each row with a pin to pin or unpin it. Right click on a button:
Unpin; right click on the ribbon: Reset this tab (its groups and the fixed group), and the
labels' switch. Order is the group's order (buttons are not reordered by a drag, unlike the view
bar's). A group with no pin is drawn as a single button opening its menu. Preference `ribbon`
(`ribbon::Prefs`: `tabs` — the tab shown per workspace —, `pins`, `labels`); `pins`
maps `"<workspace>/<tab>/<group>"` (`"<workspace>/<group>"` for the fixed group) to the pinned
ids, only for groups the user changed (absent = the defaults; unknown ids, workspaces and tabs
ignored).

## Labels

By default a button shows its icon alone (34 × 34 pt), its name in the tip: the compact ribbon
fits between the columns on a laptop's screen. The preference *Ribbon labels* (« Libellés du
ruban », Interface tab, and the ribbon's right-click menu) puts a short label under each icon
(24 pt; two lines at most; buttons 62 pt high, 50 to 74 pt wide). A group's label is under its
buttons in both cases.

## Narrow window

A pure function `arrange(groups, room, folded) → placements`, tested without display:
(1) buttons narrow, labels wrapping then truncating; (2) groups hand their last buttons to their
menu one at a time, down to one collapsed button; (3) the last groups go into a » menu at the
end (`MORE_ROOM`, kept only while a group is in it). It depends on the width only (no state from
earlier frames): no flicker. Nothing is ever unreachable. A button is never narrower than its
label's longest word: labels wrap between words only.

## Icons

Drawn for it — Modelling (workspace), pin, Clear section, Delete guides (`icons::command` maps a
command to its icon, a test checks every command of the ribbon has one); the standard views
share the views icon; » is painted. One chevron (`icons::chevron`, an SVG turned as needed)
marks every fold and drop-down: panels' title bars, outliner rows, ribbon and quick-card menus,
the Matière picker, combo boxes (`icons::combo_chevron`).

## Code and texts

- `apps/skraft/src/ribbon.rs` (description, `Prefs`, `arrange`, drawing, tests); `ribbon::tab_strip`
  also draws the Preferences', the journal window's and the « Composants » card's tabs, its
  band's top corners following the card's or not (`Corners`); `App::document_ui` places the
  card (`App::columns` gives the columns' span).
- Texts: `ribbon.*` keys of the catalog (workspaces, tabs, groups, menus' notes), without
  ellipsis ([Texts and languages](texts.md)).
- File tabs in the title bar (several documents open):
  [Several documents: file tabs](file-tabs.md).

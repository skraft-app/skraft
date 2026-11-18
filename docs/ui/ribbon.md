---
status: built
code: [apps/skraft/src/ribbon.rs, apps/skraft/src/icons.rs, apps/skraft/src/prefs.rs, apps/skraft/src/app.rs]
---

# Workspaces and ribbon

The workspace switcher and the ribbon: the tools and commands of the app, grouped by tab and
group, across the top of the view. The ribbon follows the workspace / tabbed-toolbar pattern of
CAD tools (CATIA and FreeCAD workbenches, SolidWorks' CommandManager, Fusion), after
SolidCraft's ribbon (`ui-kit/src/ribbon.rs`, `ui-egui/src/toolbar_custom.rs`). The pattern is
free to use; the drawing is ours: our icons, palette, names and proportions — nothing traced
from another product.

Decision record: [A ribbon with workspaces](../decisions/0015-ribbon-with-workspaces.md)

## Layout

Two floating cards across the top of the view, under the title bar: the **workspace card** at
the left (the workspace's icon and name, ▾ opens the list of workspaces;
`ribbon::WORKSPACE_WIDTH` 92 pt) and the **ribbon card** (a row of tabs over a row of groups; the
tabs drawn as the title bar's file tabs: centered on a band in the headers' fill, the shown one
opening onto the groups), as wide as its content and centered on the view; it never covers the
workspace card nor passes the right margin, so a narrow window narrows it (then
[Narrow window](#narrow-window) decides what gives way). The view is never resized
([Application interface](interface.md)); the dock's columns start under the ribbon; the **tool
option bar** is a small card of its own, centered under the ribbon; zoom extents frames what is
left below it. The render statistics card sits in the top right corner of the area the cards
leave free (`Editor::free_area`), under the ribbon.

## Workspaces

*Modelling* only. The data model has room for others, but a workspace is not listed until it
exists. The workspace shown is a user preference, not a document setting.

## Tabs of Modelling

Each tab a list of groups, each group a list of commands of the registry
([Command registry and external driving](commands.md)) — a command may be in several groups:

- **Build** « Construire »: Draw « Dessiner » (Box, Cylinder, Prism, Turn), Manipulate
  « Manipuler » (Move, Rotate, the copy tools: in line, in a grid, distribute), Reshape
  « Remodeler » (Stretch, Union, Difference, Intersection), Appearance (Paint);
- **Assemble** « Assembler »: Components (Make assembly, Explode, Make unique, Group bodies,
  Ungroup bodies), Manipulate (Move, Rotate, the copy tools);
- **Inspect** « Inspecter »: Measure (Tape measure, Delete guides; Tape measure pinned), Section
  (Section plane, Clear section; Section plane pinned), View (Zoom extents, the standard views;
  Zoom extents pinned).
- A fixed **Selection** group (Select, Eraser) stays at the left of every tab, before the tabs'
  own groups.
- The tab shown is remembered per workspace (preference). A shortcut runs its command whatever
  the tab: the ribbon does not switch tabs; a running tool whose button is not shown lights its
  group's label instead.

## The ribbon is data

A `const` description in the app (`ribbon::WORKSPACES`: workspaces → tabs → groups → command
ids, with each group's default pins), checked by a test against the registry (every id exists,
no group is empty). The ribbon draws it; state, labels, tips and enabled/checked come from the
registry's `CommandView`s, as for the menus.

## Pins

A group shows its **pinned** commands as buttons; its label (with ▾) opens its menu, which lists
**every** command of the group, each row with a pin to pin or unpin it. Right click on a button:
Unpin; right click on the ribbon: Reset this tab (its groups and the fixed group), and the
labels' switch. Order is the group's order (buttons are not reordered by a drag, unlike the view
bar's). A group with no pin is drawn as a single button opening its menu. Preference `ribbon`
(`ribbon::Prefs`: `workspace`, `tabs` — the tab shown per workspace —, `pins`, `labels`); `pins`
maps `"<workspace>/<tab>/<group>"` (`"<workspace>/<group>"` for the fixed group) to the pinned
ids, only for groups the user changed (absent = the defaults; unknown ids, workspaces and tabs
ignored).

## Labels

A button shows its icon (24 pt) over a short label (two lines at most; buttons 62 pt high, 50 to
74 pt wide); the preference *Ribbon labels* (« Libellés du ruban », Interface tab) turns them
off for a compact ribbon (34 × 34 pt buttons), the name then in the tip. A group's label is
under its buttons in both cases.

## Narrow window

A pure function `fit(groups, room) → plans`, tested without display, after SolidCraft's:
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

- `apps/skraft/src/ribbon.rs` (description, `Prefs`, `fit`, drawing, tests); `ribbon::tab_strip`
  also draws the Preferences' and the journal window's tabs.
- Texts: `ribbon.*` keys of the catalog (workspaces, tabs, groups, menus' notes), without
  ellipsis ([Texts and languages](texts.md)).
- File tabs in the title bar (several documents open, as SolidCraft):
  [Several documents: file tabs](file-tabs.md).

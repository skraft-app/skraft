# A ribbon with workspaces holds the tools, as data with pins

- Status: accepted (2026-11-13)
- Design: [Workspaces and ribbon](../ui/ribbon.md)

## Context

Decided on 2026-11-13 (questions / answers). The tools were in a tool bar centered at the top of
the view (`panels::toolbar`), with the tool option bar as its card's footer. More workspaces
(*Cut list*, the OCL-like parts list, and *Drawing*) are planned, and the commands keep growing.
SolidCraft has a ribbon (`ui-kit/src/ribbon.rs`, `ui-egui/src/toolbar_custom.rs`), itself after
the workspace / tabbed-toolbar pattern of CAD tools (CATIA and FreeCAD workbenches, SolidWorks'
CommandManager, Fusion).

## Decision

- A ribbon card floats across the top of the view, the workspace switcher at its left end;
  the view is never resized. The tool option bar becomes a small card of its own, centered
  under the ribbon. At first the workspace was a card of its own left of the ribbon, the docked
  columns under both: moved into the ribbon on 2026-11-19, so that the ribbon fits between the
  columns and they take the view's whole height (the ribbon going back above them when it
  does not fit there).
- Workspaces → tabs → groups → command ids of the registry, as a `const` description in the app
  checked by a test against the registry; the ribbon draws it, state and labels from the
  registry's `CommandView`s.
- A group shows its pinned commands; its menu lists every command with a pin. The pins, the tab
  per workspace and the labels' switch are user preferences; the labels off by default
  (2026-11-19: the compact ribbon fits between the columns).
- A narrow window is handled by a pure function of the width (`fit`), after SolidCraft's:
  buttons narrow, groups collapse into their menu, the last groups go into a » menu.
- The pattern is free to use; the drawing is ours: our icons, palette, names and proportions —
  nothing traced from another product.

## Alternatives considered

- The tool bar centered at the top (`panels::toolbar`): replaced and removed.

## Consequences

- Only *Modelling* exists; a workspace is not listed until it exists.
- A shortcut runs its command whatever the tab; a running tool whose button is not shown
  lights its group's label.
- The title bar and the native menus can stay small: the other commands are in the ribbon, the
  command search and the shortcuts ([A title bar instead of a menu strip](0006-title-bar-instead-of-menus.md)).

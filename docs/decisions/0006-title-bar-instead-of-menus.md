# A title bar instead of a menu strip; native menus hold File only

- Status: accepted (2026-11-10; File-only menus 2026-11-13; view commands out 2026-11-17)
- Design: [Title bar and native menus](../ui/title-bar.md)

## Context

The app had a menu strip. One application bar, whose only menu is File, can take its place: the
ribbon ([A ribbon with workspaces](0015-ribbon-with-workspaces.md)), the command search and the shortcuts
reach every command of the registry.

## Decision

- 2026-11-10: the menu strip is gone; its room goes to one bar holding the document and the
  commands used most. On the macOS desktop the bar is the window's title bar (the OS one hidden,
  the content under it).
- 2026-11-13: the title bar and the macOS menu bar hold the registry's **File** menu only (plus
  the app menu and the system's Window menu on macOS); the other commands are reached through
  the ribbon, the command search and the shortcuts. On the web, Windows and Linux there is no
  menu bar.
- 2026-11-17: the standard views and the view's toggles leave the title bar for the view bar
  ([View bar](../ui/view-bar.md)), to leave room to the file tabs.

## Alternatives considered

- The menu strip: replaced by the bar.
- A ☰ button holding every menu (web, Windows, Linux): removed with the File-only decision.
- The standard views' menu and the view toggles in the title bar: moved to the view bar.

## Consequences

- There is no View menu any more: the command palette and the shortcuts keep every command.
- On macOS only ⌘ shortcuts become key equivalents; single-letter shortcuts stay with the app's
  keymap.

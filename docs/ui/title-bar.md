---
status: built
code: [apps/skraft/src/titlebar.rs, apps/skraft/src/native_menu.rs, apps/skraft/src/style.rs, apps/skraft/src/tabs.rs, apps/skraft/src/lib.rs]
---

# Title bar and native menus

One bar at the top of the window holds the document and the commands used most; there is no
menu strip. On the macOS desktop the bar is the window's own title bar, and the native menu bar
has only the app menu, File and Window. This page covers the bar, its look and the menus per
platform.

Decision record: [A title bar instead of a menu strip](../decisions/0006-title-bar-instead-of-menus.md)

## Title bar

- `apps/skraft/src/titlebar.rs`, 34 pt (`titlebar::HEIGHT`): the app's mark
  (`assets/app-icon.svg`), name and version (the workspace's, smaller and weak) — while a
  document is shown, one button to the start page (`view.home`,
  [Several documents: file tabs](file-tabs.md)), else a place that moves the window —, the badge
  of a newer version ([Updates](updates.md)), the
  **File** menu (a text button: the registry's File commands), Undo / Redo, the file tabs
  ([Several documents: file tabs](file-tabs.md)), and at the right the command search (opens
  the palette, shows its shortcut; only its icon when the window is narrow), between two
  separators, and the preferences;
  the control channel's mark, while it is on, left of the preferences
  ([Command registry and external driving](commands.md)). Every button is a registry command,
  its state and tip from it. The standard views and the view's toggles are in the view bar
  ([View bar](view-bar.md)), the tools in the ribbon ([Workspaces and ribbon](ribbon.md)).
- The bar is **dark in both modes**, its text light: each
  palette's `title_bar` tokens (ground, text, dim text, separators, lit surface; a test holds
  its text to ≥ 7:1 on the ground and the lit surface), applied to the bar's panel only
  (`style::title_bar`); the menus and tips it opens keep the window's style.
- Its buttons, the command search too, are drawn as the tabs' + button: no background at rest,
  a veil of the text's colour when hovered (darker when pressed), the selection's colour when
  lit or when their menu is open (`tabs::veil`, `titlebar::backed`).

## macOS desktop

The bar *is* the window's title bar — the OS one is hidden and the content runs under it
(`with_fullsize_content_view`, `titlebar_shown(false)`, `title_shown(false)`, `native_options`
in `lib.rs`), 84 pt are left for the traffic lights and a gap before the mark
(`TRAFFIC_LIGHTS`; none in full screen), the bar's empty space moves the window
(`ViewportCommand::StartDrag`) and a double-click zooms it. The window title is still set
(Window menu, Mission Control).

## Native menus on macOS

`native_menu.rs`, `muda`: the app menu (About, Preferences, Services, Hide, Hide others, Show
all, Quit), the registry's **File** menu only (as the title bar: the other commands are in the
ribbon, the view bar, the command search and the shortcuts, ⌘ ones included, through the app's
keymap), the system's Window menu. The items follow the app every frame (enabled, checked,
« Annuler Déplacer »); the bar is rebuilt when the language or the shortcuts change. A chosen
item is its command's id, run as everywhere else.

- Only **⌘ shortcuts** become key equivalents: AppKit takes them before the window sees the key,
  so single letters (R, M, Space…) would be lost to the VCB and the text fields — they stay with
  the app's keymap, and the menu does not show them.
- For the same reason, a ⌘ command chosen while a shortcut is captured in the preferences goes
  back to egui as that key (`raw_input_hook`).

## Web, Windows, Linux

The OS frame is kept (no caption buttons of our own: resizing, snapping and the move loop are
the OS's). No menu bar beyond File: the other menus' commands are
reached through the ribbon, the view bar, the command search (every enabled command, the debug
ones with the debug menu) and the shortcuts.

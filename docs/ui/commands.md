---
status: built
code: [apps/skraft/src/commands.rs, apps/skraft/src/app.rs, apps/skraft/src/control.rs, apps/skraft/src/titlebar.rs, crates/control/src/server.rs]
---

# Command registry and external driving

Every action of the interface is a command of one registry, by a stable id: the title bar, the
ribbon, the view bar, the native menu, the command palette, the keyboard and the control channel
all go through it. This page covers the registry, the changeable shortcuts, the command palette
and the control channel that lets a script drive the app; the protocol spoken on that channel
has its own page ([Control protocol](../control-protocol.md)).

## Registry

- `apps/skraft/src/commands.rs` (`COMMANDS`): every command has a **stable id** (`edit.undo`,
  `tool.move`, `view.top`…), a label (a catalog text), its menu (`commands::Menu`) and a default
  shortcut. The app says whether it is enabled (or checked) and runs it. The File menu (title
  bar, macOS menu bar, [Title bar and native menus](title-bar.md)), the ribbon
  ([Workspaces and ribbon](ribbon.md)), the view bar ([View bar](view-bar.md)) and the keyboard
  come from it (the active tool's own keys — Esc, arrows, Tab, Delete, typed values — stay apart:
  the editor reads them).
- **Menus**: only File is shown as a menu; Edit, View, Tools, Solids group the commands (the
  shortcut preferences), Debug only with the debug menu (`?debug`, `SKRAFT_DEBUG`).

## Shortcuts and command palette

- **Changeable shortcuts**: preference `shortcuts: id → shortcut` (absent = default, empty =
  none), *Shortcuts* tab of the preferences (conflicts flagged). The defaults:
  [Application interface](interface.md#shortcuts).
- **Command palette**: Cmd/Ctrl+K, searches labels and ids (accents ignored), Enter runs. It
  holds every enabled command, the debug ones with the debug menu.

## External driving

- **Control channel** (desktop): an HTTP server on the **loopback** interface (127.0.0.1, port:
  preference `remote_port`, default 7858; `SKRAFT_CONTROL_PORT` for one launch), **off by
  default**. A new port (typed, or dragged and released, in the preferences) restarts it only if
  it is on and the port changed.
- Switched on and off while the app runs — command `app.remote_control` (checked while on), the
  *Débogage* (debug) tab of the preferences — and remembered (preference `remote_control`);
  `SKRAFT_CONTROL=1` starts it for one launch without changing the preference. Stopping it frees
  the port (the listener's thread ends; requests on their way get a 503); a script may stop it
  with `app.run` on `app.remote_control` (its answer still comes).
- While on, the **title bar** shows its mark (left of the preferences): green, flashing at each
  request; in red when it could not start (port taken), still counted as on. Its menu: the
  address and requests served, copy a `curl` calling `app.ping` (reading the token file), stop
  (or try again).
- Its requests, token and methods: [Control protocol](../control-protocol.md) (one route,
  `POST /call`). Each request is handed to the UI thread, which answers at its next frame.

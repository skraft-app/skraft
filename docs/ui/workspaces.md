---
status: built
code: [apps/skraft/src/workspace.rs, apps/skraft/src/ribbon.rs, apps/skraft/src/app.rs, apps/skraft/src/docs.rs, apps/skraft/src/prefs.rs, apps/skraft/src/commands.rs, crates/model/src/file.rs]
---

# Workspaces

A workspace is a profile of the editor: what it shows and what it lets one do, for one kind of
work — modelling, then a cut list or a drawing. Each document is shown in one; the workspace
switcher, at the ribbon's left end ([Workspaces and ribbon](ribbon.md#layout)), changes it.

Decision record: [A workspace shapes the editor](../decisions/0046-workspaces-shape-the-editor.md)

## The switcher

The cell at the ribbon's left end ([Workspaces and ribbon](ribbon.md#layout)) shows the
workspace's icon and name; its menu lists the workspaces, each row its icon, its name (« (en
cours) » after it when under way) and, under it, smaller and weak, what it is for
(`Workspace::about`, `ribbon.workspace.about.*`); the shown one filled. Handles:
`ribbon_workspace`, `ribbon_workspace:<id>` (a row, the menu open).

## What a workspace holds

`workspace::Workspace`, in `workspace::WORKSPACES` (a `const` description, as the ribbon's):

- its **name**, **icon** and what it is for (`about`);
- its **ribbon**: the fixed group (Selection) and the tabs, drawn by `ribbon.rs`
  ([Workspaces and ribbon](ribbon.md));
- its **stage**, what takes the central area (`Stage`): the 3D view only for now, the cards
  floating over it;
- its **own commands** besides its ribbon's (`commands`: a card's, as « Purger » of the
  « Composants » card);
- its **side panels' layout**, a preference per workspace ([Panels](#panels));
- whether it is **under way** (`under_way`): listed in debug builds only.

*Modelling* « Modélisation » is the only one, and has every command of the registry (a test
checks it).

## Commands available

A command is **available** in a workspace when its ribbon has it, it is one of the workspace's
own, or it is in the **common base**: every `file.*`, `app.*`, `view.*` and `debug.*` command,
undo and redo, cut, copy, paste, select all, erase and the Select tool (`workspace::COMMON`,
`COMMON_PREFIXES`). Only those are reached, whatever the way:

- the **shortcuts** (`Keymap::command_for` takes the available ones): a key runs the command
  available in the workspace shown, so one key may serve two workspaces; the *Shortcuts*
  preferences flag a conflict only between commands some workspace has both of
  (`workspace::share`);
- the **command palette**, the menus and the ribbon (`App::command_views` lists the available
  ones);
- `App::run` refuses another (« n'est pas disponible maintenant »), the control channel's
  `app.run` included.

Within a workspace, a shortcut still runs its command whatever the ribbon's tab.

## The document's workspace

- **Each document is shown in one** (`Doc::workspace`); the tabs of the open documents may be in
  different ones. The start page shows none (the first one's commands).
- **Kept in its file**, beside the document: the entry `view.json` ([File format](../io/file-format.md)),
  written at each save, read at each opening (`Document::file_view`). A view state: switching
  marks nothing modified and is not undone — it is not in the document, whose history keeps
  whole snapshots ([Undo](../model/undo.md)).
- **A new document**, or a file naming none, or one this build does not list (an unfinished
  one, in a release), opens in the first workspace, *Modelling* (`workspace::find`).
- **Switching** (`App::set_workspace`) cancels the running tool, the tool stack with it (Select
  comes back), and the measurements box's text; the selection and the level open are kept.
- The tab shown in each workspace's ribbon, the pins and the labels stay preferences
  ([Workspaces and ribbon](ribbon.md#pins)).

## Under way

A workspace not finished has `under_way`: debug builds list it, its name followed by « (en
cours) » in the switcher's menu; a release lists it once finished (`workspace::listed`).

## Panels

The docked cards' layout ([Application interface](interface.md#layout)) is kept per workspace:
the first one's in the preference `panels`, the others' in `workspace_panels` by id; a
workspace never arranged shows the first one's, and starts from it when first arranged
(`Preferences::layout`, `layout_mut`). The *Interface* preferences show or close the panels of
the shown document's workspace.

## Driving

`app.workspace` ([Control protocol](../control-protocol.md)) lists the workspaces this build
lists (id, label, under way, stage) and the active document's; with `id`, shows the document in
it. `app.state` gives the active document's `workspace`; the switcher's handle is
`ribbon_workspace`.

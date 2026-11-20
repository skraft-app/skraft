# A workspace shapes the editor: its ribbon, panels, stage, look and commands; the document's

- Status: accepted (2026-11-20)
- Design: [Workspaces](../ui/workspaces.md), [Workspaces and ribbon](../ui/ribbon.md)
- Amends: [A ribbon with workspaces](0015-ribbon-with-workspaces.md)

## Context

Decided on 2026-11-20 (questions / answers). Decision 0015 made a workspace a row of ribbon
tabs, the workspace shown a user preference, and *Modelling* the only one. The next ones —
*Cut list* « Débit » (the OpenCutList-like parts list) and *Drawing* « Mise en plan » — do not
only need other buttons: a cut list is a table before it is a 3D view, a drawing a sheet; their
panels, their view's look and the commands that make sense differ from Modelling's.

## Decision

- **A workspace is a profile of the editor**, not only a ribbon:
  - its **ribbon**: tabs and the fixed group, as now;
  - its **panels**: cards of its own (a cut list's) and their layout — the dock's layout a
    preference per workspace;
  - its **stage**, the central area: the 3D view, a table or a sheet;
  - its view's **look** (parts coloured by material, exploded…);
  - the **commands available** in it: those of its ribbon and a common base (selection, undo
    and redo, files, view and navigation, the app's).
- **Shortcuts are the workspace's**: a shortcut acts only when its command is available in the
  workspace shown; one key may serve two workspaces. Within a workspace a shortcut still runs
  its command whatever the tab (0015).
- **Switching workspace** cancels the running tool (the tool stack emptied); the selection and
  the open context (a part or an assembly being edited) are kept; the undo history is the
  document's, whatever the workspace.
- **The workspace shown is the document's**: written in its file (format v1), a view state —
  switching marks nothing modified and is not undone; it is written at the next save. Beside the
  document, not in it (an entry `view.json`, 2026-11-20 while building step 1): the undo history
  keeps whole snapshots of the document, so a workspace in its settings would come back with an
  undo. A new document, or a file naming none or one this build does not list, opens in
  *Modelling*. The tab shown per workspace, the pins and the labels stay preferences.
- **An unfinished workspace is listed in debug builds only**, marked as under way; a release
  lists it once it is finished (0015's « not listed until it exists », for releases).
- **Order**: the mechanism first, with *Modelling* alone; then *Cut list* as the first other
  workspace; *Drawing* is a work of its own, later.
- **Cut list's stage is its table**, the 3D view in a card beside it, with the document's camera
  (one per document, shared by its workspaces); the selection is shared both ways (a row lights
  its parts in the view, a part picked in the view its row).

## Alternatives considered

- **The ribbon only** (0015 as it was), cut list and drawing as panels or windows of Modelling:
  rejected, they need their own central area and commands.
- **Ribbon and panels, the 3D view always central**: rejected for the same reason.
- **Shortcuts global**, running their command whatever the workspace, or switching to the
  command's workspace: rejected — a key would act on what the workspace does not show.
- **Each workspace keeping its own tool, selection and context**, or **everything reset** on
  switching: rejected — picking parts in the view then seeing them in the cut list needs the
  selection kept.
- **The workspace a global preference** (0015), or **per file tab without being saved**:
  rejected for the document's own — a file reopens where it was left.
- **Switching marks the document modified**, or **is an undoable operation**: rejected, it is a
  view state.
- **New documents in the last workspace used**: rejected, one starts by modelling.
- **A camera per workspace**: rejected, one view of the document.
- **Cut list: the 3D view central and the list a large docked card**, or **a toggle between the
  table and the view**: rejected for the table central with the view beside it.

## Consequences

- The command registry knows each command's workspaces (its ribbon's, or the common base); the
  shortcuts and the command palette filter on the workspace shown.
- The central area becomes a stage: the 3D view must render into a card's rect as well as the
  whole view (the off-screen target sized to it).
- The dock's layout is kept per workspace; a workspace's own cards exist only in it.
- The file carries the workspace's id in an entry of its own; the control protocol reads and sets
  it.
- 0015 is amended: the workspace is the document's, not a preference; shortcuts are per
  workspace; unfinished workspaces are listed in debug builds.

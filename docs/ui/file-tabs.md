---
status: built
code: [apps/skraft/src/docs.rs, apps/skraft/src/tabs.rs, apps/skraft/src/app.rs, apps/skraft/src/app/, apps/skraft/src/lib.rs, crates/kernel/src/shared.rs, crates/render/src/lib.rs, crates/control/src/params.rs]
---

# Several documents: file tabs

Several documents open at once, one tab each in the title bar, as a web browser's tabs. This page
covers what a document holds, how the kernel and the renderer are shared between documents, the
tabs themselves, opening and closing, and the start page.

Decision record: [Each document holds its whole state](../decisions/0014-documents-own-their-state.md)

## Documents

- The app holds `docs: Vec<Doc>` and `active: Option<usize>` (`None`: the start page). A `Doc`
  is everything that belongs to one document: its `Editor` (history, scene, camera, selection,
  tool and its steps, open part, section), its `path`, `saved_at`, the measurements box's text,
  the Entity info field being edited, the navigator's folds ([Application interface](interface.md)),
  and a `DocId` (a counter, never reused while the app runs). Every document is handled alike,
  shown or not: nothing is swapped in and out of the app. **Why:** the type holds a document's
  state, so a new per-document field needs no swapping code. At most **32** documents
  (`docs::MAX_OPEN`; one more is refused with a toast).
- **Global, not per document** (preferences): the dock's layout (per workspace), the ribbon's
  tab per workspace, the tools' settings, theme, navigation. The model's settings stay in the
  document; the workspace it is shown in is the document's, kept in its file
  ([Workspaces](workspaces.md#the-documents-workspace)).

## Kernel and rendering

- **Kernel: one service, shared** ([The kernel](../model/kernel.md)): a pool of threads on the
  desktop ([Loading a model](loading.md)), one Web Worker in the browser (one SolidCore wasm
  loaded, not one per tab). A multiplexer (`kernel::SharedKernel`) hands each `Editor` a
  `KernelHandle` implementing `KernelService`: it tags its jobs with its document and polls only
  its own results; the `Editor` does not know it shares the service. One queue for all: a
  document in the background keeps computing. A worker crash fails the work in flight of every
  document that had some.
- **Rendering: one `Renderer`, caches per document.** Definition ids restart at 2 in every
  document, so meshes are keyed by document and id, and the material palette slots
  (`Viewport::slots`) are per document; closing a document frees them
  (`Renderer::set_document` / `drop_document`). Switching tabs uploads nothing.

## Tabs in the title bar

- The tabs are in the title bar ([Title bar and native menus](title-bar.md)), **in place of the
  document's name**: between Undo / Redo and the command search, `+` at the end. A tab shows the
  file's name (« Sans titre » unsaved), a bullet when modified, its path as a tooltip; × on the
  active and the hovered tab. Tabs are 90 to 200 pt wide; below 90, a window of tabs around the
  active one is shown and the others are in a **»** list — a pure function of the width
  (`tabs::layout`), tested.
- On the macOS desktop the empty space around them still moves the window; dragging the only
  tab, or a tab mostly up or down, moves it too (told once, as the drag starts). AppKit's own
  drag in its hidden title bar is turned off (`NSWindow.movable = NO`, `app_drags_window` in
  `lib.rs`): it moved the window from any drag in the top 30 pt or so, a tab's included, before
  the app could tell; `StartDrag` (`performWindowDragWithEvent:`) still works on such a window.
  Web, Windows, Linux: the same tabs, no window moves.
- The window title is the active document's name.

### The shown tab

The shown tab opens onto the view: it runs down to the bar's bottom edge, filled with the view's
background (`UiTokens::view`), rounded at the top and curving out into the edge at the bottom
(`tabs::Ground`); its text is `UiTokens::view_text`. The bar draws its own bottom line (egui's
panel line would cross the tab), under the tab.

### Interactions

- Click selects, × or a middle click closes, `+` opens a new document, drag reorders (the tab
  follows the pointer, lifted — shadow, panel fill, accent outline, unlike the shown tab — while
  the others slide aside to open its place: `tabs::places`, `tabs::drop_place`), right click:
  Close, Close others, Close all, Show in Finder / Explorer (desktop).
- Shortcuts: ⌘W / Ctrl+W close tab (`file.close`), ⌘Q (`app.quit`, the app menu's Quit on
  macOS: AppKit's own would end the app without asking), and, fixed whatever the shortcuts,
  Ctrl+Tab / Ctrl+Shift+Tab next / previous tab (`view.next_tab`, `view.previous_tab`, commands
  of the registry too) and ⌘T / Ctrl+T beside ⌘N (New, in a new tab).
- No renaming in place: a tab is named after its file.

## Opening

A file opens in a new tab, except when the active one is blank (never modified, no path): it is
replaced. A file already open selects its tab. New (`+`, ⌘N, ⌘T) always adds a tab. A STEP
import goes into the shown document ([STEP import](../io/step-import.md)); from the start page,
into a new one. Recent files stay a global preference (desktop).

Files dropped on the window, on the desktop and the web, take the same paths by their extension
(`file::drop_file`): `.skraft`, `.skraftmat` and `.skrafttpl` are opened, STEP and SketchUp files
imported; another format is refused by a toast naming what the app takes. A veil says what
letting go does while they are over the window. The editor reads one import at a time: of several
dropped together, the others are refused by a toast.

## Reopening

Desktop: the saved documents open when the app quits are opened again at launch, the last
active one selected — preference *Reopen documents at launch* (General tab), on by default
(`reopen_documents`; `open_files` and `shown_file` in the preferences, kept up to date as tabs
open and close). Unsaved documents are not kept.

## Closing a modified document

« Enregistrer les modifications de « X » ? » — Save / Don't save / Cancel. Save goes through the
save dialog when it has no path, and the tab closes only when that document's save is done: file
dialogs' outcomes (`file::Outcome`) carry the `DocId` they belong to (the user may switch tabs
while a dialog is open). Close others / all and **quitting** (⌘Q, the window's close button:
`close_requested` + `CancelClose`) ask for each modified document in turn, its tab selected;
Cancel stops the whole batch (`app.rs`: `Closing`; `app/documents.rs`: `advance_closing`; `app/dialogs.rs`: `save_prompt`). Web: the
page asks before it is left (`beforeunload`) while a document is modified.

## Start page

`active == None`: when the last tab closes, at launch when there is nothing to reopen (or the
app quit on it), and on demand with documents open: a click on the title bar's mark and name,
or `view.home` (« Page d'accueil »), the tabs staying open. Buttons New, Open, Import STEP, and
the recent files (desktop). No tab is selected; the ribbon, the dock, the option bar and the
status bar's measurements are hidden; the commands that need a document are disabled (menus,
palette, shortcuts). From it, a tab, or next/previous tab (the first/the last), shows a document
again; closing a tab stays on it.

## Control channel

The commands act on the active document ([Control protocol](../control-protocol.md)). `app.docs`
handles the documents by its `action`: `list` (the default: name, path, modified, the one shown,
the question asked), `new`, `switch` and `close` (`index`, from 0; `close` asks to save first,
as the tab's ×, unless `force`).

## Code and texts

- `apps/skraft/src/docs.rs` (`Doc`, `Docs`: open, close, move, cycle — tested without display),
  `tabs.rs` (the strip, `layout` and `drop_place` tested), the closing and quitting in `app/documents.rs`
  (`Closing`, `advance_closing`, `save_prompt`); `kernel::SharedKernel` / `KernelHandle`
  (`crates/kernel/src/shared.rs`); `Renderer::set_document` / `drop_document`.
- Texts: `tabs.*` and `start.*` keys of the catalog, without ellipsis
  ([Texts and languages](texts.md)).

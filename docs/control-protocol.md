---
status: built
code: [crates/control/src/lib.rs, crates/control/src/params.rs, crates/control/src/editor.rs, crates/control/src/json.rs, crates/control/src/server.rs, crates/control/src/instances.rs, crates/control/src/record.rs, crates/tools/src/editor/journal.rs, apps/skraft/src/control.rs, apps/skraft/src/driving.rs, apps/skraft/src/handles.rs, apps/skraft/src/web.rs, apps/skraft/src/identity.rs, apps/skraft/src/journal_window.rs, apps/skraft/build.rs, apps/skraft-mcp/src/main.rs, apps/skraft-mcp/src/mcp.rs, apps/skraft-mcp/src/backend.rs, scripts/control.sh]
---

# Control protocol

The protocol an agent, a script or a test drives the app with: the active tool, its preview and
inferences, the geometry, the journal, real input into the window. One method set reached over
loopback HTTP (desktop), from JavaScript in the page (web), and through an MCP server. It grows
the control channel of [Command registry and external driving](ui/commands.md). A developer's
tool, not a feature of the product: it stays off by default, in the *Débogage* tab.

After SolidCraft's control channel and MCP server (`crates/control`, `crates/mcp`,
`crates/mcp-server`, `docs/control-protocol.md`, `docs/mcp.md`, `docs/browser-api.md` there).

Decision record: [One control protocol, several transports](decisions/0020-control-protocol-one-method-set.md).

## Principles

- **One method set, several transports.** A request is a method and JSON parameters,
  `{"method": "model.geometry", "params": {…}}`; the reply `{"ok": true, "result": …}` or
  `{"ok": false, "error": "…"}`. The same methods are reached over loopback HTTP (desktop),
  from JavaScript in the page (web), and through MCP (each tool a method). A method is written
  once, as a handler on the UI thread.
- **Typed and self-describing.** Each method's parameters and result are Rust types (`serde`,
  `schemars`): the JSON Schemas come from them (`app.methods` lists them, the MCP tools'
  `inputSchema`). Strict: an unknown field or a wrong type is refused, naming the field, and
  nothing runs; every field is optional with a default.
- **What runs headless says so.** The methods split into namespaces by what they need: the
  model and the tools run on an `Editor` without a window, the `ui.*` ones need the window. A
  headless backend serves all but the window's.
- **No waiting by guess.** Every reply that may change the model carries the document's
  `revision`; `app.wait` returns when the revision moves past a value, or when the editor is
  idle (no kernel job, no load) — never a `sleep` in a script.
- **Bounded replies.** Filters (`ids`, `selection`), opt-in heavy parts (`brep`, `mesh`),
  paging (`offset`, `limit`): an agent's context is small.
- **Everything undoable.** Methods that change the document go through `Editor` and its
  history, as the user's actions do.

## Methods

| Namespace | Methods | Headless |
|---|---|---|
| `app.*` | `ping`, `methods` (names, descriptions, schemas), `state` (the active document — name, path, modified, revision, busy, idle —, tool, status, selection, context, open part, toasts, `instance`), `instances`, `wait` (`revision` other than n, and / or `idle`, with a timeout: 10 s by default, 2 min at most), `commands` (the registry: id, label, enabled, checked, shortcut), `run` (a command id), `docs` (`action`: list, new, switch, close — `index`, `force`), `screenshot` (the viewport, PNG) | `ping`, `methods`, `state`, `instances`, `wait` |
| `model.*` | `document` (the recipe: the file's JSON, not saved; `brep` for the records in full, else their counts), `geometry` (per placed part: path, part, name, occurrence name, hidden, material, world transform, pieces, topology counts, bounds local and world, volume, bodies; `ids` / `selection` filters, `offset` / `limit` paging — 100 by default, 2 000 at most —, `stale` while the kernel computes; `brep`: the SolidCore record; `mesh`: the display mesh, an approximation) | yes |
| `tool.*` | `state` (tool, status, busy, revision, copying, the VCB's label and value shown, takes typing, axis and face locks, options with their choices, selection, the last inference, the preview's `drawing`), `input` (a list of inputs, below) | yes |
| `view.*` | `camera` (read; set: `perspective`, `view` — a standard view —, `eye` + `target`, `fov_deg` or `height`, `fit`), `project` (world points → pixels, and whether each is in view), `pick` (pixels or world points → path, part, name, B-rep face, point, normal, distance) | yes (a virtual viewport) |
| `journal.*` | `log`, `failures`, `record` (below) | yes |
| `ui.*` | real egui events (`click`, `move`, `drag`, `key`, `text`, `scroll`), `handles`, `screenshot` (the whole window, PNG), `messages` (the status bar's) | no: the window |

### Tool input and the view

`tool.input` feeds `Editor::input` directly: deterministic, no window focus, the same calls as
the `tools` tests (`click(e, p)`). Inputs: `hover`, `click`, `double_click`, `drag` (`from`,
`to`, `steps`), `key` (`name`), `vcb` (typed, then Enter), `tool` (by its id,
`ToolKind::from_id`) — each pointer at a world point (`at`) or a pixel (`px`, the viewport's
points, origin top left); a click is a hover then the click, as the viewport gives them; the
first input refused stops the rest. `view.camera` in the window adds `pixels_per_point`, a
screenshot's scale.

Every editor method first takes in what the kernel finished (`Editor::poll`: a headless session
has no frames), and *idle* waits for the meshes too (`Editor::meshing`): a pick or an inference
after a change needs them. Between inputs that depend on a change (a face of the box just
drawn), `app.wait` with `idle` — the kernel computes in the background in the window.

`Camera::project` is in `f64`, the exact inverse of `Camera::ray`: a world point clicked lands
on itself, to 1e-9. **Why:** going through the GPU's `f32` matrix, a box clicked at 600 × 400
came out 599.99995.

### Journal and recording

The editor keeps a **journal** (`crates/tools/src/editor/journal.rs`, `Editor::journal`): a ring
of the last 1 000 entries, numbered — inputs (hovers in a row are one entry, counted), tools,
options, operations applied (revision) or failed (why), operation jobs back (ms, error), mesh
jobs failed, undo, redo; and the last 20 **kernel failures**, each with its `Job` (input solids
and tasks, serialisable): an operation's job that failed, a recipe that skipped bodies
(`Output::Recipe::failed`), a mesh refused.

- `journal.log` (`since`, `limit`: 200 by default) gives the entries, each with its `origin`
  (`user` / `control`) and `client`.
- `journal.failures` (`seq`: one in full) gives the failures; a `job` read back and given to
  `skraft_kernel::run` fails again, headless.
- **Recording**: `journal.record` `start` keeps the document (its JSON) and the tool, then each
  call from outside — `input`, `set_tool`, `set_tool_option`, Shift / copy / Alt changed, undo,
  redo — with the camera and viewport they were given in when they changed (hovers in a row:
  the last); `Editor::depth` leaves out what the editor calls itself (its own hovers, a tool it
  goes back to), so playing them again does not do it twice. `stop` writes a test of
  `crates/tools/tests/<name>.rs` (the document beside it as `<name>.skraft`): the editor with
  the kernel inline, the view set, each input after a `poll` (a frame), the revision and the
  parts reached checked. Not recorded: what panels do besides (the outliner's selection,
  entering a component from it).
- **Journal window** (`apps/skraft/src/journal_window.rs`, command `app.journal`: the control
  channel's mark's menu, the command palette): reads the shown tab's journal directly, channel
  on or off — its events (filtered by kind — inputs, hovers, left out at first, tools,
  operations, jobs — and by origin — Tous / Utilisateur / Pilote —, following the last; failed
  ones in red; the pilot's marked with the channel's green mark, the client on hover; copied as
  JSON lines, as `journal.log` gives them), its kernel failures (each copied in full with its
  job, as `journal.failures` `seq` gives it: `skraft_control::json` is public for this), and a
  recording under way. Its two tabs are drawn as the ribbon's (`ribbon::tab_strip`), their band
  joining the window's title bar.

### Real input and named handles

`ui.click` (`double`), `ui.move`, `ui.drag` (`steps`), `ui.key` (egui's key names, `pressed`
for one edge), `ui.text`, `ui.scroll` — each at `where`: a window point (`pos`), a handle's
centre (`handle`) or a world point's pixel in the viewport (`at`), with `button` and `mods`
(`shift`, `ctrl`, `alt`, `command`). They are egui events queued one frame at a time
(`apps/skraft/src/driving.rs`, one pointer event per frame, as SolidCraft; `raw_input_hook`
feeds them, the modifiers as `ModifiersChanged` events, egui 0.37 having no modifiers in
`RawInput`) and answered at the frame after the last one, once the widgets took them in; the
window need not have the focus. They test the real interface — ribbon, panels, the translation
of egui events into `ToolInput`s. Being egui points (`f32`), a box clicked this way measures
599.99993: `tool.input` stays the exact path, `ui.*` the real interface's.

**Named handles** (`apps/skraft/src/handles.rs`, after SolidCraft's `ui-registry`): widgets
publish their rect per frame under a stable name, with whether they are enabled,
double-buffered, kept only while the channel is on (nothing made otherwise), so a script clicks
a name, not a pixel: `ribbon:<command>`, `ribbon_group:<group>`, `ribbon_label:<group>`,
`titlebar:<command>`, `titlebar:file`, `titlebar:views`, `menu:<command>` (menu items),
`option:<key>` (a toggle or a length field), `option:<key>:<value>` (a choice's segment;
`part_scope`, `body_sign`, `machinings`, `profile`), `tab:<i>`, `tab_close:<i>`, `tab_new`,
`quick:<command>`, `quick:more`, `view_bar:<handle>`, `tree:<k>` (the tree drop-up's rows),
`naming.field`, `naming.ok`, `naming.cancel`, `toast.<i>`, `toast.<i>.action`, `vcb`,
`viewport`. `ui.handles` (`prefix`) lists them with `pixels_per_point`. `ui.screenshot`: the
whole window (egui's `request_screenshot`), PNG. `ui.messages` (`clear`): the status bar's
texts as they changed, the last 200, with their time.

Not yet published as handles: the outliner's rows, entity info's fields, the palette, the
preferences.

## Transports

### Desktop: HTTP on the loopback interface

The server of [Command registry and external driving](ui/commands.md), kept for `curl`:
`POST /call` with `{method, params}` as body, the only route. The reply has a status: 400 bad
parameters, 404 no such method, 422 refused, 503 unavailable; `app.ping` is answered by the
server, without the UI. Off by default.

- **Token**: at each start the server makes a random 256-bit token and writes it, as one line of
  hex, to `control/<port>.token` in the app's data folder (`dirs::data_dir()/Skraft`; folder
  `0700`, file `0600`; on Windows, the user alone), and the port to `control/current.port`; both
  removed when the server stops. Every request sends `Authorization: Bearer <token>`;
  `X-Skraft: 1` stays (CSRF guard: a page in a browser cannot send it without a preflight the
  server never answers). A client finds the running app by these two files.
  `SKRAFT_CONTROL_TOKEN` gives a token for a launch (tests, a sandbox).
- `scripts/control.sh <method> [params]` calls one from a shell (it finds the port and the
  token); `control.sh instances` lists the running apps.

### Web: `window.skraft`

No listener (a page cannot open one). The same methods are exposed to the page as
`window.skraft.call(method, params)` (`apps/skraft/src/web.rs`, a `wasm_bindgen` export
`skraftCall`, set on `window` by `web/index.html`): a promise of the result (`Uint8Array` of PNG
for a screenshot), rejected with an `Error`; at most 256 calls waiting. Handled on the UI thread
between frames like the desktop's, by the same code (`driving::Asker`: a call and where its
answer goes, whatever carried it). No token: any script of the page already has the app's
power; it reaches nothing outside the tab.

- Per method: files are the browser's; `app.screenshot` is the window's (the canvas); no volume
  in `model.geometry` (the kernel is in the worker); the handles kept once the page has called;
  `wait` and the kernel's work behave as on desktop (the Web Worker answers like the threads).
- What drives it: an agent able to run JavaScript in the tab (Claude in Chrome), a Playwright
  test, the console.

### MCP: `skraft-mcp`

`apps/skraft-mcp`, an MCP server over stdio (JSON-RPC 2.0, one message per line; revisions
2025-06-18, 2025-03-26, 2024-11-05: `initialize`, `ping`, `tools/list`, `tools/call`).

- **Hand-written** (`src/mcp.rs`, ~200 lines, `serde_json` only), not `rmcp`. **Why:** the subset
  is small, and the SDK would bring tokio and its own `schemars`.
- **One tool per method** the backend serves, `.` written `_` (`model.geometry` →
  `model_geometry`; `app.ping` and `app.methods` left out, MCP has its own), its `inputSchema`
  the method's parameters' (schemars), `readOnlyHint` for the methods that only read; a JSON
  result is a text block, a PNG an image block (base64), a method's refusal a result with
  `isError` and its message (for the model to correct), an unknown tool a JSON-RPC error.
  `initialize` gives instructions: units, `tool_input` at world points, `app_wait` after a
  change, the reading tools, several apps.
- **Backends** (`src/backend.rs`, a `Backend` trait): `Remote` — the running app, its port and
  token read again at every call (`skraft_control::server::discover` and `call`, a client of the
  loopback server), so an app started or started again later is found; `Headless` — an `Editor`
  of its own (the kernel inline, a viewport of 1280 × 800), on an empty document or `--in FILE`,
  serving every headless method (`app.state` and `app.wait` answered at once: the kernel ran
  inline), the window's refused, said why. Without option the server takes the app if its
  channel answers, else headless; `--remote`, `--connect PORT`, `--headless`, `--in FILE`
  choose. The log goes to stderr.
- In Claude Code: `claude mcp add skraft -- <repository>/target/debug/skraft-mcp`.

## Several apps, and who did what

Agents' sessions each launch the app from their own worktree, beside the user's: which one is
which, and which one a client talks to, must be plain; and the journal must tell the user's
doing from the pilot's.

- **Identity of a dev build**: the app's build script (`apps/skraft/build.rs`, `dev` profile
  only) stamps the git worktree, branch, short commit, `dirty` (tracked changes not committed)
  and build time (`SKRAFT_GIT_*`, `SKRAFT_BUILT`; run again on HEAD, the index, the branch's ref
  or any source changed). `apps/skraft/src/identity.rs` reads them (none in a release). Shown
  only in dev builds: a **badge** after the app's name in the title bar — the worktree's short
  name (`skraft-prism-z` → `prism-z`; the branch for the main checkout) and the commit, `*` when
  dirty, on a hue hashed from the name (FNV-1a, the same at every launch) — or, launched by a
  Claude Code session named after a colour (red, orange, yellow, green, cyan, blue, purple,
  pink, or French rouge, jaune, vert, bleu, violet, rose; any case), that colour: the name is
  read from `~/.claude/sessions/<CLAUDE_PID>.json` (`sessionId`, `name`; else the file of that
  `sessionId`) every 2 s by a thread (`identity::Session`), so a `/rename` shows at once — its
  hover gives the worktree, branch, commit, build and launch times, pid, who launched it (the
  session's name too), the channel's port; the window's title is `Skraft [name] — document`.
- **Registry of running apps** (`skraft_control::instances`): every app (desktop) writes
  `control/instances/<pid>.json` — pid, start, version, executable, its channel's port while on
  (kept up to date), its build, `claude_session` (`CLAUDE_CODE_SESSION_ID`, inherited from the
  Claude Code session that launched it), `launcher` (`SKRAFT_LAUNCHER`, free text) — and holds
  a lock on `<pid>.lock` while it runs (`File::try_lock`); the OS frees the lock with a crashed
  process, so `instances::list` tells live apps from leftovers and removes those. Method
  `app.instances` lists them; `app.state` gives `instance`, the app answering.
- **Which app a client calls** (`instances::choose`): among the apps whose channel is on, the
  one its own Claude Code session launched, else one built from the worktree it runs in (path
  components), else the last started (`current.port`). `skraft-mcp` (`Remote`, no `--connect`)
  and `scripts/control.sh` both do.
- **Who did what**: a request may name its client, `X-Skraft-Client: <name>` (80 characters, one
  line; `Call::client`). `skraft-mcp` sends `mcp · session <8> · <worktree>`, `control.sh` the
  same with its name. Each journal entry has an **origin** (`journal::Origin`: `User`, or
  `Control { client }`): the app sets it on every document's editor for each call it serves,
  and for each frame from the events it feeds — a frame fed a client's `ui.*` events is that
  client's, a frame with the person's own input (pointer, keys, wheel, text) the user's, a frame
  with neither keeps the last one's (a tool following the pointer where the pilot left it); the
  native menu bar and files the system opens are the user's. An operation computed later is
  logged with its asker's origin (kept in the pending job), and hovers merge only within one
  origin. The headless MCP's editor is all pilot (`mcp · headless`).

## Crates

- `crates/control` — `skraft-control`: the requests and replies (types, schemas), the dispatch
  of a request to a handler, the token and discovery files, the registry of running apps, the
  loopback server and a client, the recording written as a test. No UI toolkit. The handlers
  that need only an `Editor` (`model.*`, `tool.*`, `view.*`, `journal.*`) live there
  (`editor.rs`) so that both the app and the headless backend call them.
- `apps/skraft/src/control.rs`, `driving.rs`, `handles.rs`, `web.rs`: what the window adds —
  documents, registry commands, `ui.*`, screenshots, the title bar's mark — and the web's
  `window.skraft`.
- `apps/skraft-mcp`: the MCP server and its two backends.

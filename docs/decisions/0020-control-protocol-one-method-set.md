# One control protocol, several transports

- Status: accepted (2026-11-15)
- Design: [Control protocol](../control-protocol.md)

## Context

Decided on 2026-11-15 (questions / answers), after SolidCraft's control channel and MCP server
(`crates/control`, `crates/mcp`, `crates/mcp-server`, `docs/control-protocol.md`, `docs/mcp.md`,
`docs/browser-api.md` there). The app already had a loopback HTTP control channel with a few
routes (`GET /state`…) used only by scripts ([Command registry and external
driving](../ui/commands.md)); real input into the window went through a CGEvent tool. An agent
needs to debug the running app — the active tool, its preview and inferences, the geometry —
and an MCP server was to serve the same later.

## Decision

Grow the control channel into one typed, self-describing method set (`{method, params}`,
parameters and results as Rust types with their JSON Schemas, strict), written once as handlers
on the UI thread and reached through several transports: loopback HTTP (`POST /call`, the only
route, with a per-start token and discovery files), the page's `window.skraft` on the web, and
an MCP server. Methods split by namespace by what they need, so a headless backend serves all
but the window's. Replies carry the revision and `app.wait` replaces sleeps. Built in five
steps on 2026-11-15 (branch `feature/control-protocol`): the protocol and its first methods;
`tool.input` and `view.*`; the journal, failures and recording; real input, named handles and
the web; the MCP server.

## Alternatives considered

- The first routes (`GET /state`…): dropped, nothing used them but scripts.
- `rmcp` for the MCP server: not taken; the subset used is small, and the SDK would bring tokio
  and its own `schemars`. The MCP layer (JSON-RPC over stdio, lifecycle, schemas) is
  hand-written.

## Consequences

- A developer's tool, off by default, in the *Débogage* tab.
- `tool.input` is the exact path (world points, `f64`); `ui.*` the real interface's (egui points,
  `f32`).
- The handlers that need only an `Editor` live in `crates/control`, called by the app and the
  headless MCP backend alike.

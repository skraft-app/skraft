# Each open document holds its whole state; one kernel service and one renderer are shared

- Status: accepted (2026-11-13); the workspace became the document's with
  [A workspace shapes the editor](0046-workspaces-shape-the-editor.md) (2026-11-20)
- Design: [Several documents: file tabs](../ui/file-tabs.md)

## Context

Decided on 2026-11-13 (questions / answers), after SolidCraft's document tabs
(`crates/ui-kit/src/app_bar.rs`: `TabStrip`, `DocTab`, `tab_layout`;
`crates/ui-egui/src/documents.rs`), to open several documents at once, one tab each. In
SolidCraft the active design's state is spread over the app and swapped in and out when the tab
changes.

## Decision

- The app holds `docs: Vec<Doc>` and `active: Option<usize>`; a `Doc` is everything that belongs
  to one document (its `Editor`, path, measurements box text, field being edited, `DocId`).
  Every document is handled alike, shown or not: no swapping.
- One kernel service, shared: a multiplexer hands each `Editor` a `KernelHandle` implementing
  `KernelService`, tagging its jobs with its document and polling only its own results; one
  queue for all. In the browser, one Web Worker (one SolidCore wasm loaded, not one per tab).
- One `Renderer`, its caches per document (meshes keyed by document and id, palette slots per
  document).
- What is not the document's stays a global preference (dock, workspace and ribbon tab, tools'
  settings, theme, navigation).

## Alternatives considered

- Swapping the active document's state in and out of the app, as SolidCraft does: each new
  per-design field must then be added to the swap by hand. Rejected: here the type holds it.
- One kernel (one SolidCore wasm) per tab: rejected for one shared service.

## Consequences

- `Editor` is unchanged by the tabs: it sees a kernel service of its own.
- A document in the background keeps computing; a worker crash fails the work in flight of every
  document that had some.
- Definition ids restart in every document, hence the per-document keys of the render caches;
  switching tabs uploads nothing, closing a document frees its caches.

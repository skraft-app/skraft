# The kernel runs off the UI thread, as stateless jobs

- Status: accepted (2026-11-09)
- Design: [The kernel](../model/kernel.md#asynchronous-kernel)

## Context

The kernel ran on the UI thread: a heavy boolean froze the interface and, in the browser, a
kernel panic (`panic = abort` in wasm) took the whole app down. Decided on 2026-11-09
(questions / answers).

## Decision

- An `Operation` applies in two steps: `Document::prepare` (UI thread) runs it with a
  recorder in place of the kernel, validates everything and gathers the kernel calls into a
  `Job` (deduplicated input records + tasks, placeholders naming earlier tasks' outputs); the
  job goes to a kernel service, which returns each solid's record and display in one round
  trip; `Document::commit` replays the operation with the results. An operation with no kernel
  call is done at the first step.
- The document, the history and the scene stay on the UI thread; the service is stateless.
- One service (`kernel::KernelService`), three executions: inline (tests), threads (desktop),
  a Web Worker (browser).
- On the web, two wasm: the kernel alone in a module Web Worker, the UI's wasm without
  SolidCore (`kernel`'s `engine` feature). Messages in MessagePack (`rmp-serde`, named
  fields).
- During a computation, one operation at a time; navigation and hovering stay active, tool
  clicks, editing shortcuts, undo / redo are ignored; Esc abandons.

## Alternatives considered

- **The kernel on the UI thread** (until then): freezes, and a panic in wasm kills the app.
- **postcard or bincode** for the worker's messages: they do not fit the record's tagged enums.
- **One kernel thread** on the desktop (`Threaded`): replaced on 2026-11-13 by `Pool`, a
  thread per core.

## Consequences

- A kernel panic in the browser only kills the worker: its current work fails, it is
  restarted and the other tasks in flight are sent to it again; **Debug › Crash the kernel**
  (`Task::Panic`) tests the recovery.
- The scene keeps the mesh of every solid the history holds (undo / redo do not remesh); a
  solid without a mesh is asked from the service and drawn by its bounding box meanwhile.
- Compound actions go through an internal queue, one operation at a time.

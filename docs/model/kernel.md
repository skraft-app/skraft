---
status: built
code: [crates/kernel/src/engine.rs, crates/kernel/src/job.rs, crates/kernel/src/lib.rs, crates/model/src/ops.rs, apps/kernel-worker/src/lib.rs, apps/skraft/src/worker.rs, web/kernel-worker.js]
---

# The kernel: the SolidCore record, booleans, asynchronous jobs

How solids are stored (SolidCore's record, plain data), how booleans between parts in
different contexts are expressed, and how the kernel runs off the UI thread — prepared on the
UI thread, computed by a stateless service, committed back. For anyone touching `crates/kernel`
or the way an operation reaches it.

## Storing solids: the SolidCore record

- A solid is a **`solidcore_record::brep::Brep`**: plain data, `Clone` + `serde`, shared by
  `Arc`. The kernel (`solidcore::Kernel` + `solidcore::Brep`) is instantiated only for the
  duration of an operation: record → kernel → operation → record.
- After each operation, the record is **squared up** (`square_frames`, `engine.rs`). **Why:** a
  rotation about an oblique axis leaves plane axes ~1e-15 off a right angle, which SolidCore
  then refuses to read back (tolerance ~3e-16): the next operation would fail.
- Benefits: nearly free undo snapshots, direct saving, no kernel state to maintain. Cost: one
  conversion per operation (a part's recipe: one per input and one at the end,
  [Machinings and the recipe](data-model.md#machinings-and-the-recipe)).

## Booleans across contexts

- The **target is changed in its own frame**: the tool is brought into the target part's frame
  by the relative transformation `inv(world(target)) · world(tool)`.
- **Subtract**: the tool becomes a **negative body** (a machining) of the target part, without
  destroying its geometry; **union**: the tool becomes a positive body of the target, its
  bodies kept and editable; **intersection**: a new part of one positive body, the common
  volume. A tool that is its single positive body comes as that body (its profile with it),
  any other as a group of copies of its bodies ([Body groups](bodies.md#body-groups)).
  `Operation::Boolean { targets, tools, op, scope, … }`: several targets and tools, each target
  with the tools reaching it ([Solid tools](../tools/solid-tools.md)).
- The change affects all occurrences of the target part or only this one ([Copies and propagation](data-model.md#copies-and-propagation)).
- The tool occurrence leaves the model (it lives on in the target's bodies) unless kept
  (« Conserver B »); the other occurrences of its part stay.

## Asynchronous kernel

The kernel does not run on the UI thread. **Why:** a heavy boolean does not freeze the
interface and, in the browser, a kernel panic (`panic = abort` in wasm) only kills the worker.

Decision record: [The kernel runs off the UI thread, as stateless jobs](../decisions/0001-asynchronous-kernel.md).

- **Pure kernel tasks.** An `Operation` applies in two steps:
  1. `Document::prepare` (UI thread) runs the operation with a *recorder* in place of the
     kernel: it validates everything (ids, cycles…), gathers the kernel calls into a `Job`
     (`kernel::Job`: deduplicated input records + `Task`s — primitive, transformation, boolean,
     recipe, stretch, display, STEP import) and returns placeholder solids, one per task: a
     placeholder handed to a later call names that task's output (`EARLIER | task`), so a
     primitive can be cut by a recipe in the same job. An operation with no kernel call
     (rename, group, move…) is **done at this step**, with no round trip;
  2. the `Job` goes to the kernel service, which returns for each solid produced its record
     **and its display** (mesh + snaps), in a single round trip;
  3. `Document::commit` replays the operation on the same document, handing out the results in
     order; `History::push` records it.

  The document, the history and the scene stay on the UI thread; the service is stateless.
- **One service, three executions** (`kernel::KernelService`: `submit(id, job)`, `poll()`,
  and `progress()`, `cancel(id)`, [Loading a model](../ui/loading.md)): `Inline` (synchronous,
  tests), `Pool` (a thread per core, desktop), Web Worker (browser, in the app). Each result
  wakes the UI (`request_repaint`).
- **During a computation**: one operation at a time; navigation and hovering stay active, tool
  clicks, editing shortcuts, undo / redo are ignored; the status bar says it is computing and
  the cursor waits. Esc abandons: the result will be dropped. Compound actions (exploding
  several instances) go through an internal queue, one operation at a time. Replacing a copy by
  an array (`Command::Replace`) restores the copy if the array fails or is abandoned.
- **Meshes**: the scene keeps the mesh of every solid still held by the history (undo / redo do
  not remesh), within the history's memory cap ([Memory cap](undo.md#memory-cap)). A solid
  without a mesh (an opened file) is asked from the service (one task per solid), drawn in the
  meantime by the edges of its bounding box (computed on the record, without the kernel); the
  document stays usable.
- **Web**: two wasm. `apps/kernel-worker` (the kernel alone) runs in a module Web Worker
  (`web/kernel-worker.js`); the UI's wasm does not hold SolidCore: `kernel` has an `engine`
  feature (execution: `solidcore`, `-convert`, `-tess`); without it, it only exposes the types
  (`Solid`, `Mesh`, `Snaps`, `Job`…) and `solidcore-record`. Messages in **MessagePack**
  (`rmp-serde`, named fields: the record has tagged enums, postcard or bincode do not fit), in
  transferred `ArrayBuffer`s. If the worker crashes, its current work fails (a message in the
  status bar), it is restarted and the other tasks in flight are sent to it again.
- **Testing the recovery**: **Debug › Crash the kernel** menu (task `Task::Panic`, followed by a
  normal task that must succeed). The menu is turned on at launch, in any build: `?debug` in the
  page's address, `SKRAFT_DEBUG` in the environment on the desktop.

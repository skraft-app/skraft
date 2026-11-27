---
status: built
code: [apps/skraft/src/system/images.rs, web/image-worker.js, crates/render/src/lib.rs, crates/kernel/src/engine.rs, crates/kernel/src/job.rs, crates/tools/src/editor/loading.rs, apps/skraft/src/app.rs, apps/skraft/src/app/, crates/tools/src/bench.rs, web/index.html, scripts/build-web.sh]
---

# Loading a model

How a big file is opened or imported: shown at once, then refined, under a window that says how
far it is and can stop it. A big file (`Cruise_Assembly.step`: 583 bodies, ~20 s of meshing on
one thread) is the case it is built for. This page covers the kernel's thread pool, the preview
and final meshes, the progress reported, the loading window and cancelling.

## Thread pool

A pool of kernel threads (`kernel::Pool`, desktop): one per core, each with SolidCore's stack.
Two queues: an operation's job (anything but meshes) before the meshes asked in the background
(a job of `Display` tasks only); each in the order submitted. The web keeps **one worker**.
**Why:** several workers would each load SolidCore's wasm.

## Preview, then final

`Task::Display { quality }`, `kernel::Quality`: during a load, the editor asks every missing
solid's **preview** (meshed at a hundredth of the body, `solidcore_tess::preview`) before any
final mesh: the pool takes them in that order. The scene draws a preview (`State::Preview`)
until the final mesh replaces it; a preview coming after the final is dropped; a final mesh
refused leaves the preview drawn. Outside a load (the meshes an operation's job brings), final
meshes only.

A preview does not compute the solid's exact box: its scale comes from SolidCore's loose box,
its bounds from its own mesh widened by the mesh's tolerance, within that loose box. Only the
final mesh brings the exact box dimensions are read on. **Why:** the exact box was 83 % of a
mesh's time on a body of curved faces (`ZeRotor`), so a preview cost as much as the final.

## Progress

`kernel::Progress`: a job reports it as it runs (`run_with` and its `Context`); the services keep
the latest per job (`KernelService::progress`); the web worker posts `{ id, progress }` while it
runs. The STEP read reports `Reading`, then `Bodies { done, total }`. Meshes are counted by the
editor as their jobs end.

## A load

`Editor::loading`: a file opened (from `Editor::new`) or a STEP file imported, until its last
final mesh: the file read (an import), previews done / asked, finals done / asked. `LoadEnd`
(taken once by the app) says how it ended.

## Loading window

`apps/skraft`, `loading_window`: a modal over the active document once its load has lasted
0.3 s (`LOAD_WINDOW_AFTER`; a small file would only flash it): its title (« Ouverture de… »,
« Import de… »), a row per stage with its bar, « Annuler » (or Esc). It stays **until the last
final mesh and the shown document's last texture**. With only textures left, the model is
there: its button is « Continuer sans attendre » (or Esc), which closes the window and keeps
the document, the textures still coming. A document loading behind another tab shows it when
brought forward. **Why:** preparing textures on the interface's thread froze it for 1.4 s on a
0.9 MB file with four textures, before this window could be drawn.

## Cancelling

Cancel is a real stop (`KernelService::cancel`): a job not started is dropped and answered
`Error::Cancelled` at once; a running one is stopped through its SolidCore `CancelToken`
(`Kernel::set_cancel_token` on every kernel the job makes, handed to the threads of
`parallel_map`; a STEP read also stops between bodies). On the web the worker cannot be
interrupted: the job is answered cancelled, the worker replaced at the next poll and given the
jobs left. A cancelled import is undone (it is one history step); a cancelled opening closes its
tab. Esc on an operation computing cancels its job too. A document closed stops its jobs
(`KernelHandle`'s drop).

## Loading the web app

`web/index.html` shows the logo, the app's name and a progress bar until both wasm have arrived
and the UI's runs. The name is drawn as outlines of IBM Plex Sans, and nothing else is written:
the font and the catalogs are in the wasm it waits for. The bar counts decompressed bytes against
the sizes `scripts/build-web.sh` writes into the page, since a compressed response's
`Content-Length` does not give them. The page fetches the kernel's wasm from the start, and the
worker, which only spawns once the UI runs, finds it in the HTTP cache. Without WebGPU the kernel
is not fetched: the app only says what is missing.

When the app cannot run — no WebGPU, or no adapter for it — it says why on the loading screen
(the page's `skraftFailed`): its bar red and full, the app's sentence under it, in the last
theme's colours; the screen is put back if it was already gone, and it keeps hiding the edges
Safari reads its bars from ([Safe areas](touch.md#safe-areas)). A download that fails shows the
browser's own words there, smaller. `?webgpu=off` in the page's address hides `navigator.gpu`,
and `?webgpu=noadapter` makes it give no adapter, to try both.

## Measured

`bench_step_loading` (`BENCH_STEP`, 8 threads, dev profile, Cruise): read 1.75 s, every preview
drawn at 2.9 s, the last final mesh at 14.7 s; cancelled after the previews, an operation done
50 ms later.

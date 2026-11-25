# Skraft — design

Skraft is a simple, intuitive 3D modeller on real solids: 100 % Rust, native (macOS, Windows,
Linux) and WebAssembly. Its geometry is exact B-rep, computed by
[SolidCore](https://github.com/tangency-labs/solidcore) (a Rust port of SMLib, the kernel behind
[SolidCraft](https://github.com/tangency-labs/solidcraft)). It models close to SketchUp, and
many tools follow [OpenCutList](https://github.com/lairdubois/lairdubois-opencutlist-sketchup-extension)'s
interactions: **parts** (solids, made of bodies) and **assemblies**, both **components**, in a
tree — the ground for a cut list later.

These pages describe **how Skraft is built today**, one topic per page. Each page says its
status and its main source files; a short *Why* gives the reasons that still hold, and the
[decision records](decisions/README.md) keep the story: the alternatives weighed, what a
decision replaced. What is not built yet is not described here.

## Principles

- **Direct modelling.** Move, stretch, booleans are applied at once; the B-rep record *is* the
  state, there is no rebuild history.
- **Every change is a typed `Operation`** (`AddPrimitive`, `Transform`, `Boolean`, `AddBody`,
  `MakeAssembly`, …), serialisable, never a direct mutation: operations feed undo and the
  journal, and keep the door open to parametric modelling (whose blocker is persistent face
  naming — [procedural parts](decisions/0007-procedural-parts.md), proposed).
- **Exact geometry, far from the origin.** Inferences work on the record's exact geometry, not
  the mesh; rendering is camera-relative in `f64`
  ([Display](render/display.md#precision)).
- **The kernel off the UI thread**: stateless jobs on a fresh `Kernel`, panics caught
  ([The kernel](model/kernel.md)).
- **Performance is part of the design**: per-occurrence costs go through indexes and instancing
  ([0011](decisions/0011-per-occurrence-costs-instances-and-indexes.md)).

## Glossary

| Word | Meaning |
|---|---|
| **Component** | A definition: a **part** (solids) or an **assembly** (occurrences). |
| **Occurrence** | A placed instance of a component; copies of a component change together until made unique. |
| **Body** | A solid of a part, **positive** (added) or **negative** (removed, a *machining*); a part is ∪ positives − ∪ negatives. |
| **Body definition / group** | A body's shape, shared by its copies; a group nests bodies (a CSG tree without history). |
| **Recipe** | What a body is built from (a primitive, a profile, a boolean), kept to edit it again. |
| **Record** | SolidCore's B-rep as data (`solidcore-record`): the stored state of a solid. |
| **Context** | The open component the tools work in; opening a part shows its bodies. |
| **Inference** | A point or a line the cursor snaps to (endpoint, on edge, axis, …). |
| **VCB** | The measurements box: values typed while a tool runs. |
| **Scope « Parts »** | A selection of parts at any depth, by their paths. |

## Map

**Model** — [Data model](model/data-model.md) · [Bodies](model/bodies.md) ·
[Opening a part](model/opening-a-part.md) · [Properties](model/properties.md) · [The kernel](model/kernel.md) ·
[Undo / redo](model/undo.md)

**Tools** — [Interaction](tools/interaction.md) · [Inferences](tools/inferences.md) ·
[Selection](tools/selection.md) · [Tape measure and guides](tools/tape-measure.md) ·
[Prism and profiles](tools/prism.md) · [Turned bodies](tools/turned-bodies.md) ·
[Draw options](tools/draw-options.md) · [Stretch](tools/stretch.md) ·
[Copy, cut and paste](tools/copy-paste.md) · [Copy arrays](tools/copy-array.md) ·
[Solid tools](tools/solid-tools.md) · [Rounds and chamfers](tools/rounds.md) ·
[Reorient](tools/reorient.md) ·
[Quick tools](tools/quick-tools.md)

**Interface** — [Application interface](ui/interface.md) · [Commands](ui/commands.md) ·
[Ribbon](ui/ribbon.md) · [Workspaces](ui/workspaces.md) · [Parts list](ui/parts-list.md) · [Drawing](ui/drawing.md) · [Title bar](ui/title-bar.md) · [Updates](ui/updates.md) · [File tabs](ui/file-tabs.md) ·
[View bar](ui/view-bar.md) · [Components library](ui/components-library.md) · [Catalog cards](ui/catalog.md) · [Mouse cursors](ui/cursors.md) · [Navigation styles](ui/navigation-styles.md) ·
[Loading a model](ui/loading.md) · [Texts and languages](ui/texts.md) · [Touch screens](ui/touch.md)

**Rendering** — [Display](render/display.md) · [Rendering and look](render/look.md) ·
[Drawing helpers](render/drawer.md)

**Files and driving** — [File format](io/file-format.md) ·
[SketchUp import](io/sketchup-import.md) · [STEP import](io/step-import.md) ·
[Control protocol](control-protocol.md)

**Why** — [Decision records](decisions/README.md)

## Crates

```
                   ┌──────────────┐
                   │    kernel    │  SolidCore behind a safe API
                   └──────┬───────┘
              ┌───────────┴───────────┐
       ┌──────┴──────┐          ┌─────┴──────┐
       │    model    │          │   render   │  wgpu, no egui
       └──────┬──────┘          └─────┬──────┘
       ┌──────┴──────┐                │
       │    tools    │                │
       └──────┬──────┘                │
              └──────────┬────────────┘
                  ┌──────┴──────┐
                  │ app (egui)  │  wiring, panels, viewport
                  └─────────────┘
        (i18n: under every crate but render)
```

| Crate | Role | Depends on |
|---|---|---|
| `crates/i18n` — `skraft-i18n` | The texts ([Texts and languages](ui/texts.md)): the `/locales` catalogs compiled in, `t!` / `tp!`, the current locale, plurals, decimal separator, platform key names. | `rust-i18n` |
| `crates/theme` — `skraft-theme` | The colour tokens ([Rendering and look](render/look.md)) of the viewport and the interface, light and dark themes, sRGB ↔ linear. No UI toolkit. | `serde` |
| `crates/kernel` — `skraft-kernel` | `Solid` (record), primitives, booleans, transformation, face offset, display mesh (`Mesh`: positions, normals, indices, B-rep face per triangle, edges). Runs each call on a fresh `Kernel` and catches its panics. | `solidcore`, `solidcore-record`, `solidcore-convert`, `solidcore-tess`, `skpwright`, `glam`, `i18n` |
| `crates/model` — `skraft-model` | `Document`, components, occurrences, ids, paths, `Operation` + `apply`, undo history, the file (a ZIP of JSON), reading typed values (expressions, `@`, lists, points). No UI dependency; testable without display. | `kernel`, `i18n`, `serde` |
| `crates/render` — `skraft-render` | wgpu pipelines (shaded faces, edges, grid, axes, highlight, previews), GPU cache of meshes per definition, instanced drawing with a view-projection matrix. Knows neither egui nor the document. Usable off screen. | `kernel` (for `Mesh`), `wgpu`, `glam` |
| `crates/tools` — `skraft-tools` | Camera (orbit/pan/zoom, ray from the screen), display scene (the document flattened into placed solids + mesh cache), picking by ray (CPU, triangle → B-rep face), the tools' state machines, typed dimensions, inferences. Testable without UI or GPU. | `model`, `kernel`, `i18n`, `glam` |
| `crates/control` — `skraft-control` | The control protocol ([Control protocol](control-protocol.md)): methods and their parameters (with their JSON Schemas), the handlers that need only an `Editor` (`model.*`, `tool.*`), the loopback HTTP server with its per-start token and discovery files (desktop). No UI toolkit. | `model`, `tools`, `kernel`, `serde_json`, `schemars` |
| `apps/skraft` | eframe: title bar, ribbon, navigator, entity info, status bar + VCB, viewport (`render`'s off-screen target shown as an egui texture), kernel service (native threads or a Web Worker), translation of egui events into tool events, opening/saving. Native and wasm. | all of the above, `eframe`, `egui-wgpu` |
| `apps/kernel-worker` | The kernel in a web worker: runs the jobs the web UI posts. | `kernel` |
| `apps/import` — `skraft-import` | Converts a SketchUp (`.skp`) or STEP file to a `.skraft` file, as the app imports it, with counts and timings. | `model`, `kernel` |
| `apps/skraft-mcp` | An MCP server (stdio) over the control protocol: the running app, or an editor of its own without a window. | `control`, `tools`, `model`, `kernel` |

## Toolchain and dependencies

- Rust **1.97.1** pinned by `rust-toolchain.toml` (the SolidCore crates ask for ≥ 1.88), with
  the `wasm32-unknown-unknown` target.
- `solidcore` is on crates.io, but `solidcore-record`, `-convert` and `-tess` are not: all four
  come from the git mirror `tangency-labs/solidcore`, **pinned to one commit**. Its known bugs
  are reproduced on SolidCore alone in `crates/kernel/tests/solidcore_quirks.rs`, each test
  asserting the bug is still there: run first on an upgrade.
- wasm: an 8 MiB stack (`.cargo/config.toml`) — SolidCore recurses legitimately and wasm has no
  guard page. Kernel panics cannot be caught in wasm (`panic = abort`): they are confined to the
  kernel's Web Worker ([The kernel](model/kernel.md)).
- `rust-i18n` reads the `/locales` catalogs at compile time; `crates/i18n/build.rs` makes a
  changed catalog rebuild it.
- Licences: GPL-3.0-or-later; the binaries linking skpwright (GPL-3.0-only, the SketchUp
  reader: the app, the kernel worker) are distributed under GPLv3. `deny.toml` lists the licences a dependency may have (checked by
  `cargo deny` in CI), `scripts/third-party-licenses.sh` writes the notices shipped with the
  installers.
- Checks: a pull request runs `.github/workflows/checks.yml` on Linux (rustfmt, Clippy with its
  warnings denied, the tests). The workflows' actions are pinned by commit; Dependabot
  (`.github/dependabot.yml`) proposes their updates monthly, and for the crates security fixes
  only.
- Web: a `v*` tag publishes the browser version on GitHub Pages, at https://try.skraft.app
  (`.github/workflows/deploy-web.yml`). Only a tag is published, as the « À propos » window links
  to the source of the version's tag; run by hand on a branch, the workflow only builds. Pages'
  `github-pages` environment must allow the `v*` tags.

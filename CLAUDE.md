# Skraft

A simple, intuitive 3D modeller on real solids: exact B-reps computed by the SolidCore kernel,
100 % Rust, native (macOS, Windows, Linux) and WebAssembly. How it is designed: start at
[docs/README.md](docs/README.md), then read only the page of the topic at hand.

## Build, run, test

- `cargo build --workspace`, `cargo test --workspace`: always `--workspace`, never `-p <crate>`
  (it unifies the dependencies' features differently and rebuilds SolidCore from scratch).
- Run the app from the debug build, `cargo run -p skraft` (`target/debug/skraft`); never
  `--release` to try something (a full rebuild).
- Web: `scripts/build-web.sh` (into `dist/web`).
- Drive a running app: launch it with `SKRAFT_CONTROL=1` (and `SKRAFT_CONTROL_PORT` for a port
  of its own), then `scripts/control.sh <method> [json]` or the `skraft-mcp` server
  ([docs/control-protocol.md](docs/control-protocol.md)).
- One worktree per piece of work: `scripts/new-worktree.sh <name> [--build]` makes
  `../skraft-<name>` with main's build cache; `scripts/drop-worktree.sh <name> [--unmerged]`
  removes it, its cache and its branch.

## Crates

`crates/`: `kernel` (SolidCore, the B-rep record) → `model` (document, operations) and `render`
(wgpu, no egui, no document) → `tools` (editor, tools, inferences; no GPU) → `apps/skraft`
(egui app). Also `control` (control protocol), `i18n`, `theme`; `apps/kernel-worker` (the
kernel in a web worker), `apps/import` (a `.skp` or STEP file to a `.skraft` file),
`apps/skraft-mcp` (MCP server over the control protocol).

## Conventions

- **Docs and code comments in English.** User-facing texts only in the catalogs,
  `locales/fr.yml` (the reference) and `locales/en.yml` with the same keys (a test checks it) —
  never literal strings in the code. No `…` in the catalogs: the code appends it.
- **Action tools are named by an infinitive verb** (« Copier en ligne », « Unir »); the drawing
  tools by what they make (« Boîte », « Prisme »), their « Dessiner » group giving the verb.
- **A tool ported from OpenCutList copies its interaction** (states, picking, previews, keys),
  not only its algorithm.
- **Interface elements are built once**: before drawing a button, field, row, card, modal
  box or 2D overlay, look in the shared modules
  ([docs/ui/interface.md#widgets](docs/ui/interface.md#widgets)); what two screens draw alike
  moves there, never a local copy.
- **Performance is part of the design**: measure (bench, timing tests) and optimise where it
  helps.
- **Versions**: `0.0.x` are test builds, x going up by one at each build sent to testers,
  a number never reused. **File format v1 until `0.1.0`**, the version that locks it: no
  format version bumps, no compatibility code; a build that breaks older files says so in
  its changelog entry.
- **Design docs** ([docs/](docs/README.md)): a page describes the present state; a short
  *Why* keeps the reasons that still hold; structuring decisions and their alternatives go to
  `docs/decisions/`. Code comments cite a page by its path (`docs/model/bodies.md`). A change
  of design updates its page in the same branch.
- `docs/REVIEW.md` is private (ignored by git, linked into every worktree): the todo, risks
  and reviews. What is not built yet goes there, not into the design pages.
- `CHANGELOG.md`: one entry per `v*` tag, for users, written with the `/changelog` skill.

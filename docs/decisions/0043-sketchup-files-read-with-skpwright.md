# Read SketchUp files directly with skpwright, in the kernel

- Status: accepted (2026-11-20); supersedes [0012](0012-sketchup-import-through-json-export.md)
- Design: [SketchUp import](../io/sketchup-import.md)

## Context

Until now a SketchUp model came in through a Ruby script run inside SketchUp, writing JSON for a
command-line converter ([0012](0012-sketchup-import-through-json-export.md)): SketchUp had to be
installed and the model opened, and the app could not import a `.skp` itself, nor could the web
version. [skpwright](https://github.com/hew3d/skpwright) (crates.io, GPL-3.0-only) is a
clean-room reader of the `.skp` format, pure Rust (its only dependency a DEFLATE decoder),
releases 3 to 2027, compiling to wasm.

Checked on 2026-11-20 before deciding: on a 1373-definition apartment (52 k faces), the same
definitions and faces as the Ruby export, and **all 1527 placed parts with the same world
transform and the same solid** (vertices within 1 nm) and the same 806 occurrence names as the
import through JSON; the file read in 0.5 s. Its `Behaviour::extra` is SketchUp's
`no_scale_mask`, checked against SketchUp on every bit.

## Decision

**File › Importer…** takes `.skp` files as STEP ones: the kernel reads the file
(`Task::ImportSketchUp { bytes }` → `Output::SketchUp`, `crates/kernel/src/sketchup.rs`) with
skpwright, takes each definition at each scale it is placed at and builds the faces of each as
a solid, in parallel; `Document::from_sketchup` makes the components, and
`Operation::Insert` places them in the model. The Ruby export and its JSON are removed.

**Why in the kernel**: like STEP, the reading and the solids run off the UI thread natively and
in the kernel's worker on the web, with progress and cancel; the web UI does not link the
reader (the kernel's `engine` feature carries it).

## Alternatives considered

- **Keep the Ruby export beside it**: two ways in for one result, the JSON one needing
  SketchUp; dropped.
- **Read the `.skp` in the model crate, the kernel only building polyhedra**: the parse (0.5 s
  on the apartment, more in the browser) on the UI thread, and a job of many tasks that fails
  as a whole when one solid fails, where an import excludes one definition.
- **Trimble's SketchUp SDK**: native libraries, macOS and Windows only, not redistributable
  under the GPL, no wasm.
- **Write our own reader**: skpwright documents the format (a specification and a Kaitai
  grammar) and is tested against SketchUp's own exports; nothing to gain.

## Consequences

- skpwright is **GPL-3.0-only**: Skraft's sources stay GPL-3.0-or-later, the app and the kernel
  worker that link it are distributed under GPLv3 (`deny.toml`, `about.toml` accept it).
- One young library by one author (0.6.1, started 2026-07): a `.skp` it cannot read fails the
  import with its message; its diagnostics (records skipped) come in the report.
- The kernel worker grows by about 1.1 MB of wasm (skpwright alone, release with LTO, before
  `wasm-opt`), beside about 25 MB; the web UI does not link it.
- No SketchUp needed anywhere: the tests read `.skp` fixtures (`crates/kernel/tests/fixtures`),
  both containers (2017 and 2026).
- What the reader gives and the import does not take yet: arcs and circles (`Curve::arc`, for
  cylinders instead of facets), a texture's placement on each face, attribute dictionaries (an
  OpenCutList model's `ladb_opencutlist`).

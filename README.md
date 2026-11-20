# Skraft

**A simple, intuitive 3D modeller built on real solids.**

Skraft draws like the push-and-pull modellers you already know, but everything you make is an
exact solid: a boundary representation (B-rep) computed by the
[SolidCore](https://github.com/tangency-labs/solidcore) kernel, not a mesh of triangles. Holes
stay round, booleans stay exact, every part always knows its volume and its true dimensions.
It is written entirely in Rust and runs natively on macOS, Windows and Linux.

> [!NOTE]
> Skraft is at an early stage. Expect rough edges, and files saved now may not open in later
> versions.

<!-- TODO: screenshot or GIF of the app -->

## Features

- **Parts and assemblies.** Everything you draw is a part with its own material. Parts gather
  into assemblies, and all copies of a part change together until you make one unique.
- **Parts built from solids.** A part is made of solids it adds and solids it removes, which
  you can still edit after the fact. X-ray shows what is hidden inside.
- **Drawing tools.** Boxes (with offset and fillets), cylinders, prisms from a drawn profile and
  turned shapes around an axis.
- **Editing tools.** Move, rotate, stretch, union, subtract and intersect, copy along a line or
  a grid, distribute, copy and paste.
- **Precision.** Inferences on points, edges and axes, typed lengths and angles while you draw,
  a tape measure and guides, section views with hatched caps.
- **Your way of working.** Several documents in tabs, a navigator of the model, navigation
  presets matching other CAD programs (SketchUp, Fusion, SolidWorks, Onshape, Rhino, Blender).
- **STEP import** (AP203, AP214, AP242), with its assembly structure.

## Download

Installers for each release are on the
[Releases page](https://github.com/skraft-app/skraft/releases):

| System | File |
|---|---|
| macOS, Apple silicon | `Skraft-<version>-macos-arm64.dmg` |
| macOS, Intel | `Skraft-<version>-macos-x86_64.dmg` |
| Windows | `Skraft-<version>-windows-x86_64.msi` (or `.zip`, without installing) |
| Linux | `Skraft-<version>-linux-x86_64.AppImage` |

The installers are not signed by a known publisher yet, so your system will warn you the first
time:

- **macOS**: if Skraft will not open, go to *System Settings › Privacy & Security* and click
  *Open Anyway*.
- **Windows**: in the SmartScreen dialog, click *More info*, then *Run anyway*.
- **Linux**: make the AppImage executable (`chmod +x Skraft-*.AppImage`), then run it.

## Driving Skraft from an agent

Skraft exposes a control protocol: an AI agent or a script can read the model, drive the tools
at exact points, move the camera and follow the journal of what happened, through the same
calls as the user's actions (all undoable).

`skraft-mcp` serves this protocol as an [MCP](https://modelcontextprotocol.io) server over
stdio. It talks to the running app, or, if none is running, to an editor of its own without a
window. It is not part of the installers yet: build it from source.

```sh
cargo build --release -p skraft-mcp
claude mcp add skraft -- "$PWD/target/release/skraft-mcp"
```

The app's side is off by default: turn it on in the preferences' debug tab, or launch the app
with `SKRAFT_CONTROL=1`. Options of `skraft-mcp`: `--remote` (the running app), `--connect PORT`,
`--headless`, `--in FILE.skraft`. For a quick call from the shell, `scripts/control.sh <method>
[json]`. The protocol is described in
[docs/control-protocol.md](docs/control-protocol.md).

## Building from source

The Rust toolchain is pinned by `rust-toolchain.toml` (rustup installs it on the first build).
On Linux, install `libwayland-dev` and `libxkbcommon-dev` first.

```sh
cargo run -p skraft              # the desktop app
scripts/build-web.sh             # the browser version, into dist/web
```

How Skraft is designed (data model, tools, rendering, kernel) is in [docs/](docs/README.md),
one page per topic, with the decision records behind it; what changes from one release to the next is in
[CHANGELOG.md](CHANGELOG.md).

## Credits

- [SolidCore](https://github.com/tangency-labs/solidcore) by Tangency Labs, the B-rep kernel
  that computes every solid (Apache-2.0).
- [SolidCraft](https://github.com/tangency-labs/solidcraft), from which Skraft takes much of its
  rendering, its theme and its control protocol.
- [OpenCutList](https://github.com/lairdubois/lairdubois-opencutlist-sketchup-extension), the
  model for parts, materials and many of the tools' interactions.
- [skpwright](https://github.com/hew3d/skpwright) by Kurt Granroth, the reader of SketchUp files
  (GPL-3.0-only).
- [egui](https://github.com/emilk/egui) and [wgpu](https://github.com/gfx-rs/wgpu) for the
  interface and the display.
- The fonts IBM Plex Sans, IBM Plex Mono and DejaVu Sans.

The full list of the crates and fonts Skraft ships with, and their licenses, comes with each
installer (`THIRD-PARTY-LICENSES.txt`).

## License

Skraft is free software, under the [GNU General Public License v3.0 or later](LICENSE). The
application, which links skpwright (GPL-3.0-only), is distributed under the GPL v3.

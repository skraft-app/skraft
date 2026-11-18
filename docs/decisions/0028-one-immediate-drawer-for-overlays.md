# One immediate drawer for everything drawn over the model

- Status: accepted (2026-11-17)
- Design: [Drawing helpers](../render/drawer.md)

## Context

Decided on 2026-11-17 (questions / answers), after OpenCutList's Kuix 3D motifs
(`lib/kuix/entity/3d/`: `Motif3d`, `EdgeMotif3d`, `BoxMotif3d` / `BoxCornersMotif3d`,
`*FillMotif3d`, `Mesh`, `Points`, `AxesHelper`; the styles `line_stipple`, `on_top`,
`show_hidden_lines`, `shell_on_top`). Before it, the helpers were scattered (`dashed` and
`arrow_head` in `tools/mod.rs`; `line`, `segments`, `pixels`, `mark`, `label`, `container_box`
in `shapes.rs`; `draw_box(es)`, `draw_axes`, `draw_measure` in `handle.rs`; another `label` in
`measure.rs`; `Editor::dashed`), the pixel-to-model conversion redone in seven files, dashes cut
on the CPU (up to a thousand pieces a segment) though the renderer dashes on the GPU, and
`viewport.rs` choosing the look of each of the View's fields.

## Decision

One `Drawer` (`crates/tools/src/draw.rs`), immediate, not retained, with Kuix's primitives and
styles (pens, brushes, stipples, layers, hidden lines, `shell_on_top`), writing one output,
`Drawing`: styled lines and fills, and boxes drawn instanced by the renderer. Every tool's
preview and every drawing of the editor goes through it; `viewport.rs` only translates.
Dashes are made by the line shader. Boxes in front over the scene (`shell_on_top`) by default,
as Kuix, off for the selection and context boxes.

## Alternatives considered

- Kuix's retained scene of motifs: a tool's preview is rebuilt at each view anyway, so retained
  motifs would save nothing; what depends on pixels stays computed in the tools crate with the
  camera, and tests read what is drawn. Kuix gives the vocabulary and the styles only.

## Consequences

- Built in four steps on 2026-11-17: the drawer and the renderer additions (`Dash`,
  `BoxOutline` with `shell_on_top` / `over`, `Fill` with `triangles` / `cull_back` and four fill
  pipelines, `Frame.hidden_lines` instead of `xray`); the tools one by one (shapes, profile,
  measure, rotate, move, handle and the copy tools, stretch, section); the editor's drawings;
  the guides, axes and boxes. Appearance unchanged but for the boxes, whose fronts now show
  over the scene (the handles' boxes were wholly hidden behind faces).
- Gone: `Preview`'s `lines`, `scene_lines`, `xray`, `fills`, `labels`, `marks`, `points`,
  `guides`, with `PreviewLine`, `PreviewFill`, `tools::dashed`, `tools::arrow_head` and
  `shapes.rs`'s `pixels`, `label`, `mark`, `line`, `segments`; `View`'s `overlay`,
  `scene_lines`, `xray`, `fills`, `bases`, `base_axes`, `selection_boxes`, `context_box`,
  `guides`, `faded_guides`, `selected_guides`, `hovered_guides`, `preview_guides`, `axes`; the
  renderer's axis colours.
- Sampled curves dash on from chord to chord (before: each chord dashed alone — solid when
  short —, a dashed circle every other chord); the profile's outline is closed (its last side
  was missing).
- The editor's view with 10 000 parts selected (`bench_array`) measured 0.21 ms, against 0.18
  before.

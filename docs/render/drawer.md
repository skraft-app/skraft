---
status: built
code: [crates/tools/src/draw.rs, crates/tools/src/tools/mod.rs, crates/tools/src/editor/view.rs, crates/tools/src/guides.rs, apps/skraft/src/workspaces/common/viewport.rs, apps/skraft/src/kit/overlay.rs, crates/render/src/lib.rs, crates/render/src/shader.wgsl]
---

# Drawing helpers

One `Drawer` for everything drawn over the model — a tool's preview (strokes, fills, grips,
marks, lengths) and the editor's own drawings (bases, axes, guides, boxes) — with the
vocabulary and styles of OpenCutList's Kuix 3D motifs (`lib/kuix/entity/3d/`: `Motif3d`,
`EdgeMotif3d`, `BoxMotif3d` / `BoxCornersMotif3d`, `*FillMotif3d`, `Mesh`, `Points`,
`AxesHelper`; the styles `line_stipple`, `on_top`, `show_hidden_lines`, `shell_on_top`). How the
renderer then draws lines and dashes is in [Display](display.md).

Decision record: [One immediate drawer for everything drawn over the model](../decisions/0028-one-immediate-drawer-for-overlays.md).

## Principles

- **Immediate, not retained.** `crates/tools/src/draw.rs`: a `Drawer` over the tool's `Cx` (or
  the editor), a frame stack (`in_frame`, `with_frame`) and the output, with Kuix's
  primitives. `Cx::drawer` gives one in the world's axes, `Cx::axes_drawer` one in the
  context's axes. **Why:** a tool's preview is rebuilt at each view anyway, so retained motifs
  would save nothing; what depends on pixels (arrow heads, corners of 20 px, the faces turned
  to the eye) stays computed in the tools crate, with the camera; tests read what is drawn.
  Kuix gives the vocabulary and the styles, not its retained scene.
- **One output, `Drawing`**: `lines` (segments with their `Pen` and a `phase`), `fills`, `boxes`
  (`BoxEdges`), `grips` (`PreviewPoint`'s shapes, drawn over everything after the fills),
  `marks` (crosses, which the lengths keep clear of) and `lengths`. Mostly flat lines and
  polygons, styled; higher-level primitives where the GPU gains: **boxes** go out as boxes,
  drawn instanced (`render::BoxOutline`), as the selection's.
- **Everything goes through it**: a tool's preview (`Preview.drawing`; `d.px(p, n)` for pixels
  to model) and the editor's own drawings (`View.drawing`). The state fields of `Preview` and
  `View` (hover, ghost, copies, stretch, section…) are not drawings and stay out. The tool's
  drawing is appended after the editor's. `viewport.rs` only translates (`Translated`,
  `theme_color`), and still tells the grid where the axes are.
- The control protocol's `tool.state` gives the preview's `drawing` (lines with their pen,
  fills, boxes, grips, marks, lengths), [Control protocol](../control-protocol.md).

## Primitives

Strokes: `line` (and `line_phased`), `edge` (arrow heads at either end, `Ends`: Kuix's px size,
`ARROW_PX` 10), `arrow_heads` (the heads of `edge` alone), `polyline`, `segments`, `rectangle`,
`polygon`, `curve` (sampled curves, their dashes running on from chord to chord), `circle`,
`arc`, `turn_arrow` (an arc with heads at `Ends`, lying in its plane, a fixed size on screen: a
turn to come), boxes (`box_`, `box_styled`, `oriented_box`, `oriented_boxes`: corners option, px,
clamped to half the edge; `shell_on_top`), `arrow`, `chevron`, `axes`, `guide` (a guide shape's
strokes, each stipple at its phase). Fills: `fill_polygon`, `fill_triangles` (free triangles,
Kuix's `Mesh`), `fill_rectangle`, `fill_disc`, `fill_box`, `fill_arrow`. Points and texts:
`grip`, `mark`, `length` (not written under 1 µm, nor under 30 px on screen unless its value is
being typed),
`length_always` (written however short on screen), `label` (any text along a segment, the
lengths' helpers written on it). A primitive may draw both its outline and its fill.

## Style

- **`Pen`** (lines): `color` — linear RGBA, or a theme colour named (`Color::Theme(ThemeColor)`:
  `Edge`, `Guide`, `FadedGuide`, `HalfFadedGuide`, `Hover`, `Selected`, `ContextBox`), resolved
  by the app with the current palette and mode —; `width` in points (`NORMAL` 1.5, `BOLD` 3);
  `stipple`; `lay` (how the stipple lies, `Lay`); `layer`; `blended` (drawn over what it does
  not hide at its alpha, no depth written: the faded and half faded guides); `hidden` (its
  hidden part drawn again). Helpers: `bold`,
  `bold_if`, `dashed` (long dashes, anchored), `dotted`, `anchored`, `over`, `blended`,
  `hidden`, `x_ray` (plain where seen, translucent long dashes where hidden), `Pen::dimension`
  ([Dimensions](#dimensions)), `Pen::guide` (long dashes laid on screen, blended when
  translucent).
- **`Brush`** (fills): `color` (its alpha the opacity), `layer`, `cull_back` (`front_only`: a
  translucent box's back and front not adding up).
- **Layers**: `Scene` (depth-tested, the default), `Over` (Kuix's `on_top`), `Axis` (the model's
  axes, [Depth of lines](display.md#depth-of-lines)), and `Scene` with
  its hidden part shown in its own style (`Hidden`, Kuix's `show_hidden_lines`: long dashes by
  default, its colour and width optional, its alpha scaled by `alpha`, `HIDDEN_ALPHA` 0.3 by
  default), blended over the faces that hide it. Fills take the same layers.
- **Stipples**: Kuix's five — `Solid`, `Dotted` (1.5 / 3), `ShortDashes` (3 / 3), `LongDashes`
  (the guides' 6 / 6), `DashDot` (6 / 3 / 1.5 / 3) — made by the line shader (a pattern of up to
  four lengths, on / off, in points). They lie on the line (`Lay`) anchored (a dash at both
  ends, the period adjusted a little on screen), in model units at a phase (one period for the
  picture, set at the view's focus), or on screen at a phase (the guides'), [Display](display.md).
  `Pen::stipple` anchors `ShortDashes`: in model units a period this short falls under the
  shader's fade at some zooms and reads as a plain line (`curve` still lays its dashes in model
  units). No dashes are cut on the CPU.

The tools' colours are in one place, `crates/tools/src/tools/mod.rs`: the inks (`INK`, `RED`,
`GREEN`, `BLUE`, `MAGENTA`, `PICK`, which take the mode's), `WHITE`, `BLACK`, `GREY`; `axis_ink`
gives an axis' ink, `ink_along` that of the axis a direction runs along.

## Dimensions

Every line that gives or leads to a dimension draws with one pen, `Pen::dimension(color)`: long
dashes anchored, **in the scene**, in full colour where seen, and where faces hide it the same
dashes at `HIDDEN_ALPHA`, blended over them (they take the colour of what hides them). The
hidden part keeps the anchoring of the part seen (`viewport.rs`: same stipple, same `Lay`), so
the dashes run on through what hides them; bold under an axis lock where the tool locks one.
It draws the copy tools' measures and distribute's grey lines to them, the shapes' and the
prism's measures, radii, chords and diagonals, the profile's dashed measures, Rotate's radii,
Stretch's grip axes, measure and the line from where the interior handle is taken. Move's line
and the tape's measure are plain where seen (`x_ray`: the gesture, the tape itself), their
hidden part the same translucent dashes. Over everything stay what is a handle or an inference:
a shape's outline, the protractor, Stretch's interior handle, the arrow heads, the dashed lines
from tracked points. **Why:** over everything, a measure through the part copied or stretched
did not tell where it runs; one style for all dimensions reads the same in every tool.

## Boxes

`BoxEdges { bounds, pen, corners, shell_on_top }`, drawn instanced by the renderer, dashed from
their corners, each half edge anchored (the pen's anchoring and hidden style ignored;
`Layer::Over`: drawn wholly over the scene). `BoxStyle` defaults to whole edges with
`shell_on_top`, as Kuix: the copy tools', the handles', « Dessiner dans »'s container — the
edges of the faces turned to the eye drawn over the scene, the back ones hidden. Off for the
selection and context boxes (they wrap what they mark; a large selection's would cover the
view). Which faces face the eye is decided by the instanced boxes' shader (normal against the
ray through the face's centre, edge-on not exposed: `vs_box_front`, `face_exposed`), no CPU
cost.

## Renderer side

- `Dash { pattern, lay }` (`Lay::Anchored`, `Lay::Model(phase)`, `Lay::Screen(phase)`).
- `BoxOutline { transform, color, width, dash, corners, shell_on_top, over }`.
- `Fill { corners, color, triangles, over, cull_back }`: fills in the scene or over everything
  (a pass without depth test), both sides or the front only — four fill pipelines; triangles
  as input besides convex polygons.
- `Frame.hidden_lines`: lines drawn only where faces hide them, in their own style; the app puts
  their visible part with the depth-tested lines.
- Named theme colours are resolved by the app (`viewport.rs`, `theme_color`).

## Screen overlays

What is drawn in points on the screen rather than in the model goes through egui's painter, the
3D view's as the drawing sheet's, with the shapes of `apps/skraft/src/kit/overlay.rs`: the dash
patterns named by what they mean (`SELECTION`, `FRAME`, `GHOST`, `ALIGNMENT`, `TRACKED`, `LEVEL`),
the selection rectangle (`marquee`), the square grip, the cross, the markers of the points inferred (`marker`), a text turned about its corner
(`turned_text`).

*Why:* the two stages had their own copies, and their selection rectangles had drifted apart.

## What draws through it

- **Shapes** (Box, Cylinder, Prism, Turn): through `Cx::axes_drawer`; `pen(color, dash)` in
  `shapes.rs` (over the scene, dashed anchored), their measures `Pen::dimension`. Sampled curves (circles, fillets, arcs, the
  turned mirror) go through `Drawer::curve`. « Dessiner dans »'s container: two boxes, dotted
  (OCL's stipple) and its corners, both `shell_on_top`, the renderer choosing the front edges.
- **Profile**: the outline bold (closed), its cap and sides, the fillet stubs, its grips, its
  length (`length_always` for the grip taken's), its dashed measures `Pen::dimension`.
- **Tape measure**: in the world's axes, the curve referred to (bold magenta over the scene, an
  arc as a curve; an axis bold in the scene), the measure or offset `x_ray` (plain where seen,
  translucent long dashes where hidden), its heads over the scene (`arrow_heads`), its length, its mark; its
  guides through `Drawer::guide`.
- **Rotate**: the protractor (its circle a curve, its ticks) over the scene, the radii `Pen::dimension`,
  the angle (`label`) and the centre's mark.
- **Move**: the move's line `x_ray`, its head over the scene, its length (`length_always`), the
  start's mark.
- **Handles and the copy tools**: `handle.rs`'s `draw_box` / `draw_boxes` go out as instanced
  boxes, dashed, their fronts over the scene (`shell_on_top`), no more than 100; `draw_axes` in
  the scene, bold when locked, heads at both ends; `draw_measure` `Pen::dimension`, bold under
  an axis lock; `mirror_badge` a grip. Copy along a line, copy as a grid and distribute
  draw through them (grips, marks, lengths `length_always`; distribute's grey lines `Pen::dimension` and
  its bold ray with its head).
- **Stretch**: the grips' axes (`Pen::dimension`, grey when locked with their padlock), the
  grips, the interior handle (bold over the scene, its chevrons, its grip, the line from where
  it is taken `Pen::dimension`), the cutters (translucent fills in the scene, their outline dashed when
  going or being added, bold when picked, their centre), the measure (`Pen::dimension`), its grips and its length
  (`label`, the tool's own formatting).
- **Section plane tool**: the plane's outline and normal.
- **The editor** (`View.drawing`): the open part's profiled bodies' bottom caps (bold, as
  curves) and its turned bodies' axes (long dashes) in the theme's edge colour
  (`ThemeColor::Edge`); the dashed line from a tracked point to the inference; the selection's
  boxes (`ThemeColor::Selected`, dashed, wholly in the scene: `Drawer::oriented_boxes`, `place`d
  by a move's ghost) and the context's (`ThemeColor::ContextBox`); the guides (`Drawer::guide`
  with `Pen::guide`) in `Guide`, `Hover`, `Selected`, `FadedGuide` or `HalfFadedGuide` (both
  blended; half faded: within reach of a click under « Parts »,
  [Selection](../tools/selection.md#selection-scope)), the tools' previewed guides likewise;
  the context's axes when shown (or a part open), their negative halves at 0.35.

---
status: built
code: [crates/render/src/lib.rs, crates/render/src/shader.wgsl, crates/theme/src/lib.rs, apps/skraft/src/style.rs, apps/skraft/src/viewport.rs, crates/tools/src/section_cut.rs, crates/tools/src/tools/paint.rs, crates/tools/src/tools/section.rs, crates/model/src/settings.rs]
---

# Rendering and look

How the app looks and how the model is shaded: the still image that costs no GPU work, the
colour tokens of the theme and the interface's palettes, the fonts, the materials and their
rendering, silhouettes, the section view with its caps, hatches and cut lines, and x-ray. How
the document becomes meshes and lines is in [Display](display.md).

Taken from SolidCraft's rendering and interface (questions / answers, 2026-11-10).

## Still image

`Renderer::render` computes a **fingerprint** of everything the picture depends on: `Frame`
(view, projection, background, style, batches, highlights, lines, grid, axes, pixel density,
section), the target size, and a generation counter bumped by every mesh upload and target
change. Same fingerprint as the last picture: nothing is encoded, the target keeps it, `render`
returns `false`. A still view costs no GPU work.

### Render statistics

*View › Statistiques de rendu* (command `view.render_stats`, a preference): a card in the view's
top right corner — frames painted over the last second (egui repaints on demand: a still view
paints none), the interval between the last two, the CPU time of the editor's view and of the
picture's preparation, whether the picture was redrawn, and `Renderer::stats` (draw calls,
triangles, instances, lines, boxes, bytes uploaded) with the meshes' GPU memory.

## Theme

- **`crates/theme`** (no egui, like `render`): named **tokens** in linear RGBA — default front
  and back faces, edge ink, highlights (selected, hover, picked, ghost, other occurrences),
  fading outside the context, section cap shade, axes, selection rectangle, inference markers,
  tooltip. Two themes, **light** and **dark**; the app sets egui's theme from the user's
  `theme` preference (system by default, light, dark).
- `render` keeps no colour of its own: faces and ink come in `Frame::style`.

### View background

A token of each palette and mode (`UiTokens::view`, with `view_text` for the shown tab's text),
not a setting of the document (unlike SketchUp, whose style is saved in the model), and not
changeable. Light in the light mode; in the dark one a dark grey, a little lighter than the
panels (the translucent cards over it, [Application interface](../ui/interface.md#context-card),
stay apart from it).

What is drawn on the view takes the mode's **inks** (`Theme::inks`, `Inks::LIGHT` /
`Inks::dark()`): the tools' preview lines, the axes' red, green and blue, a line along no axis,
what a tool picked — deep in the light mode (the y axis's green and the picked orange deep ones:
a bright green or orange had a light view's luminance and faded into it), light in the dark
mode. The tools name them by the light mode's constants (`GUIDE_INK`, `AXIS_X`…, the `tools`
crate's `INK`, `RED`…); the app turns each into the mode's where it resolves a drawing's
colours (`Theme::ink`: a colour of one of those hues, of any opacity, becomes the mode's, its
opacity kept; the viewport's lines, grips and crosses). The guides, the context box, the grid
(`Theme::grid`, to `render::Style::grid`, mixed over the view a little, its tens more) and the
selection's and hover's edges have a dark mode's value of their own. The lengths keep their
light inks: they are written on a white label. The model keeps its colours in both modes — its
faces, and its edges black, read against the faces (on the dark view the faces show the shape,
the silhouettes against it no longer). A test holds the inks' contrast with the view of each
palette and mode, the edges' with the faces.

**Why:** with the light view kept in the dark mode, the translucent cards and the dark panels
sat on a bright ground. The tools' inks map by hue rather than each tool naming a token: their
colours go as plain RGBA through every drawing API (lengths, grips, previews) — one mapping
where they are drawn covers them all.

### Interface tokens and palettes

- **Interface tokens** (`UiTokens`): panel, header, field, border, text, dim text, hover,
  accent and soft accent, corner radii, spacing.
- **Palettes**: a `Palette` (user preference `palette`) gives the interface's colours, **each in
  a light and a dark variant**; the `theme` preference (system, light, dark) picks the variant,
  so the two settings are independent. Built in code, no theme files: *Neutral* (SolidCraft's:
  neutral greys, blue accent), *Warm* (warm greys, amber, the default), *Graphite* (cool, deeper
  greys, teal). A palette changes the interface only: the model's and the overlays' tokens stay
  the theme's. A test holds every variant to text ≥ 7:1 and dim text ≥ 3:1 on its grounds.
- `apps/skraft/src/style.rs` turns a palette's tokens into egui's visuals and spacing for
  **both** egui styles (light and dark) when it is chosen (at start-up, then on a change); egui
  switches with its theme, nothing is restyled per frame. *Preferences › Interface* shows each
  palette as a card drawn in its colours (the variant shown now); a click applies it at once.

### Fonts

IBM Plex Sans (variable) and IBM Plex Mono (regular), SIL OFL (`apps/skraft/assets/fonts/`),
embedded in the binary (≈ 670 kB): Sans regular for text, semibold for headings, Mono for
monospace; each then the **key symbols**, and monospace last Plex Sans (Plex Mono has no
Greek). Plex has no macOS key symbols (⌘ ⌥ ⌃ ⌫ ⌦ ⏎ ⇥): a subset of DejaVu Sans (Bitstream Vera
licence, `assets/fonts/dejavu-sans/`, ≈ 18 kB), its arrows and technical symbols blocks only
(U+2190–21FF, U+2300–23FF), fills them in. The arrow keys are written ← ↑ → ↓ (Plex's), not
with egui's triangles (⏴), which no font here has.

No other font: eframe's `default_fonts` is off. Its Ubuntu Light and Hack (≈ 670 kB) drew none
of the app's texts, only characters Plex lacks in a typed or imported name (Latin Extended-B,
polytonic Greek, Armenian, Georgian, mathematical symbols), which now show as empty boxes, as
emoji already did. A test holds every family to every character of the catalog, of the
symbols the code writes, and of Greek.

## Palette and materials

- **Model**: `Document.materials: Id → Material { name, color }` (sRGB);
  `Occurrence.material: Option<Id>` (the copy's material, as in OpenCutList; an assembly's
  occurrence paints the parts below it that have none); `Part.faces: BTreeMap<u32, Id>`, the
  material of single **faces** of the part's final B-rep (so of all its occurrences).
  Priority: face > occurrence > the theme's default; back faces keep the back colour.
  **Accepted limit**: a face is named by its index in the record, which a boolean renumbers;
  an index past the solid's faces is ignored.
- **Operations**: `AddMaterial`, `EditMaterial` (name, colour; dragging a colour merges into one
  step), `DeleteMaterial` (unpaints what used it), `PaintOccurrences`, `PaintFaces`. None calls
  the kernel. A file painting with an unknown material is refused.
- **Rendering**: a **palette** (a storage buffer of linear RGBA); each instance carries a
  palette index, each vertex the index of its painted face (or none). Changing a material's
  colour rewrites only the palette; painting an occurrence only the instances (rewritten each
  frame); painting a face re-uploads that one part's mesh. The app gives each material a
  palette slot for the session, never reused, so baked face indices stay valid.
- **Paint tool** (`tool.paint`, B, as SketchUp's paint bucket): a click paints with the current
  material (none: the paint is removed). Its option « Peindre » (Tab switches): « L'exemplaire »
  — the occurrence clicked, or the whole selection when it is in it — or « La face » — the face
  clicked, on every occurrence of its part. Under the « Parts » scope
  ([Selection](../tools/selection.md)), the part under the cursor at any depth, in its assembly
  (the parts selected when it is one of them), and the level chosen in the tree drop-up.
  Painting occurrences, a drag draws a rectangle: what it encloses (left to right) or touches
  (right to left) is painted when it is released (one step); Esc drops it.
- **Interface**: the **Matières** panel (list, colour, name, add, delete, the current
  material — picking one starts the paint tool); the occurrence's material in the entity info.

## Silhouettes

**Geometric, in the shader**: when a mesh is uploaded, `render` collects every mesh edge shared
by two triangles (vertices welded by position) that is not a B-rep edge and whose normals
differ, with both normals. The lines' vertex shader draws it only where one triangle faces the
eye and the other does not: the outline of a curved surface (a cylinder's sides). Same wide
lines as edges. Document setting `display.silhouettes` (on by default).

## Section view

### Plane

**One plane** in the document settings: `display.section: Option<Section { origin, normal }>`
in world coordinates, and `display.section_active`. What lies on the side the normal points to
is cut away. Changing it is a `SetSettings` (undoable). The **cut** is made in the fragment
shader (faces, edges, silhouettes); the plane is passed in camera coordinates (computed in
`f64`). **Picking and inferences** ignore what is cut away.

### Caps

**Colour from the back faces, extent from a stencil.** Our parts are closed B-reps, so through
the cut only their back shows: a back-facing fragment, section on, is painted flat and darker
in its part's colour (a colour per part), at its **true depth** with the faces' usual bias
(edges keep winning). Then, while a section is on:

1. the faces left are drawn again without depth test nor colour, each fragment flipping the
   stencil's low bit — an odd count means the ray meets the plane inside a part;
2. where the bit is set, the **back faces are drawn again** (`fs_cap`, front faces discarded),
   at a depth brought just behind the plane — 1/64 of the way from the plane to the face, so
   their order is kept: the nearest back face, the cut part's, wins, and the cap gets its colour
   and hatch.

Any front face behind the plane loses, even one lying on that back face: a part in contact (a
part standing on another), a pin in its hole — whose two meshes cross by their chords, so
the depth test between true depths would settle it facet by facet (stripes). What lies behind
the cap (the part's far edges) is hidden too. Where the count is even (the eye inside a part,
parts overlapping), the back faces keep their first, flat, unhatched colour. The depth buffer is
`Depth32FloatStencil8` when the device has it, else `Depth24PlusStencil8`.

**Why:** the stencil gives the cap its extent and the back faces its depth order, so a part in
contact or a pin in its hole cannot win over the cap, nor the kept parts behind the plane be
painted as caps.

Decision record: [Section caps from back faces and a stencil](../decisions/0034-section-caps-from-back-faces-and-stencil.md).

### Hatches

In the caps' fragment shader, the cap pixel's point on the plane (`on_plane`, along its ray) is
measured across one of **six directions** — lines at 45°, 135°, and about 27°, 63°, 117°,
153° — the part's definition chooses (a hash of its mesh's key, carried by its instances: one
part, one hatch, as on a drawing; two neighbours alike one time in six), so a part keeps its
hatch through a move and under a highlight. The directions are whole combinations of a basis
`u`, `v` of the plane (world Z × the normal, else world X): `u ± v`, `2u ± v`, `u ± 2v`; the
basis anchored at a point of the plane near the eye snapped to 4 m in it (`f32` precision; whole
combinations, so the pattern is unchanged), computed in `f64`.

- **Anchored on the plane** (they do not slide when orbiting or panning), their **pitch follows
  the zoom by powers of two**, **one per placement** (`hatch_pitch`, in the vertex shader): the
  one that puts 8 to 16 points between lines (`HATCH_SPACING`) where the placement's centre
  falls on the plane, its slant there counted. So a cap shows a pattern drawn on the plane,
  closing up with the perspective.
- Every other line comes or goes at once when the zoom crosses a level, no fading in (the user
  wants one ink for all the lines); where lines close up too much to be told apart (far off, at
  a slant), their mean tone instead.
- Antialiased from the pixel's footprint (derivatives of the plane point, taken before any
  branch), 1 point wide, in the edges' ink at `hatch_opacity` (theme) over the cap's colour,
  before the highlight's tint.

**Why** one pitch per placement: a pitch per pixel ends half the lines across a cap seen at a
slant, where the level changes.

### Cut lines

The outline of each cut, bold (`BOLD_WIDTH`) in the edges' ink, depth-tested as the tools'
lines (pulled towards the eye). Cut on the CPU from the **display meshes**, so they meet the
caps exactly (`tools::section_cut`): each placement drawn opaque (not a ghost) whose part's
bounds straddle the plane has its triangles sliced by the plane taken into its definition's
axes (a vertex on the plane counts with the side kept, so an edge on the plane gives one
segment). Kept by (definition, mesh revision, plane in its axes) from one frame to the next
while used: a still plane costs nothing, a moving one (the Section plane tool) re-cuts only what
it crosses. In `View::section_cuts`, each with its placement's fading, or the mark of the
highlight drawn over it (its cut lines in the highlight's edge colour, after the others). The
app makes them lines. No setting: hatches and cut lines show with every section.

### Section plane tool

`tool.section`: click a face → the plane lies on it, normal outwards; move the cursor to sink it
along the normal, a second click (or a typed depth) places it. *View › Afficher la coupe*
(`view.section`) toggles it, *View › Supprimer la coupe* (`view.section_clear`) removes it.

## X-ray

A display mode (`Editor::xray`, command `view.xray`, a toggle of the view bar,
[View bar](../ui/view-bar.md)), the document's setting `display.xray`: every part is drawn as a
ghost (faces blended at `Style::ghost_opacity`, no depth written, so every edge is seen), its
highlights as a wash of the mark's tint over it (`Highlight::ghost`); a section cuts it without
caps. Inferences see through everything: `DisplayScene::see_through` (already true for an open
part's bodies shown as ghosts) holds for every piece, so a point behind a face is inferred on —
stacked points, the nearest to the eye. A click picks the nearest solid along the ray, as
without X-ray. A ghost's own opacity over the ghosts' rides with its placement
(`render::Placement::opacity`: a machining's).

Not yet: cycling through stacked solids on repeated clicks.

# Capture: a frame over the view, rendered off screen

- Status: accepted (2026-11-28; step 1 built)
- Design: [Capture](../ui/capture.md)

## Context

Nothing takes a picture of the model: the only images are the control protocol's
`app.screenshot` (the window as displayed, `ctx.request_screenshot`), the file's thumbnail and
a drawing's « Rendu » views. A user who wants an image for a client, a forum or a README
crops a system screenshot: the screen's resolution, the selection's highlight, the tool's
preview and the cursor in it.

What exists already:

- **`Renderer::render_picture`** draws a frame of its own, at a size of its own, off screen
  (the library's thumbnails, the drawing's rendered views).
- **`Frame::pixels_per_point`**: line widths are in points, so a picture drawn at another
  density keeps the lines' proportions.
- **`system/file.rs`** saves bytes through the save panel natively, as a download on the web;
  the `image` crate encodes PNG and JPEG.

## Decision

**A tool, « Capturer »** (`tool.capture`), in Inspect › View, beside the standard views. It
holds a **capture frame** over the view; the user navigates behind it as usual (orbit, pan,
zoom, the standard views, the view bar), then the validation **✓** under the option bar (or
Enter) renders the frame and opens the save panel, the tool staying for another capture;
**✗** (Esc) gives it up. **Why** a tool: it has a state (the frame) and options, and the tool stack
([0036](0036-tool-stack.md)) gives the option bar, the keys and the exit already.

**The frame**: a rectangle over the view, **centred on its free part** (below the ribbon and
the option bar, between the docked cards: what zoom extents frames), the largest of its ratio
that fits with a margin, the outside dimmed. The ratio is an option: « Libre », 16:9, 4:3, 3:2,
1:1, A4 landscape and portrait. In « Libre », the frame's corners size it, symmetric about its
centre. **Why** the free part's centre, not the view's: the cards float over the view, so a
frame centred on the view would run under them; zoom extents puts the model in the middle of
the free part, so in the middle of the frame. The image is a crop of what the view shows,
perspective included: exactly what the frame shows.

**What the image shows is what the frame shows**, rendered again off screen
(`render_picture`) with the view's camera, its projection narrowed to the frame:

- **At the size chosen**, not the screen's: the longer side in pixels (1280, 1920, 2560,
  3840, each listed as the image it gives), the other from the ratio (in « Libre », from the
  frame's proportions). A size typed may come later.
- **Lines as in the frame**: `pixels_per_point` scaled by the image's width over the frame's
  on screen, so a 4K image looks like the frame enlarged, not like thinner lines.
- **Never in it**: the tool's previews, the inferences, the cursor, the frame itself, the
  view bar and the cards.
- **As displayed**: the style (edges, x-ray, shading), the section, what is hidden, the
  camera's projection.
- **Options**: « Sélection » (off by default: its highlight and boxes), « Guides », « Axes »
  (the model's axes and the grid); the **background**: the theme's, white, or transparent
  (PNG only); the **format**: PNG or JPEG (quality 92, no option).

**The options are the tool's settings** ([Application interface](../ui/interface.md#tools-describe-their-options)):
remembered per tool in the user preferences, not in the document. **Why**: a ratio and a
size are a user's habit (their site, their forum), not the model's; the file format stays as
it is.

**Split**: `tools` holds the tool (the frame in view coordinates, the options, the hints) and
asks for a capture (an editor request carrying the frame and the options); the app renders,
reads the texture back, encodes off the UI thread and saves. **Why**: `tools` has no GPU,
and a test drives the tool like a click.

**Steps**:

1. The tool, the frame, the off-screen render, PNG and JPEG saved (natively and on the web).
2. Copy to the clipboard (an option of the tool, or a second button).
3. **Tiles** past the GPU's largest texture (`max_texture_dimension_2d`, 8192 most often,
   less on some web GPUs) or the memory budget: the projection split in sub-frustums, each
   rendered then stitched; until then the width is capped there.

## Alternatives considered

- **Copying the screen** (`request_screenshot`, what `app.screenshot` does): the screen's
  resolution, and every overlay in it.
- **A rectangle dragged over the view** (a system screenshot's gesture): tied to the screen's
  pixels, and the framing is set once, before the camera is adjusted.
- **The frame centred on the whole view**: it ran under the cards, and away from where zoom
  extents puts the model.
- **A free frame placed anywhere**: one more thing to place, for little.
- **Exporting a drawing's view as an image**: its framer is there already
  ([0052](0052-drawing-a-view-frames-what-it-shows.md)), but a sheet and a view to make for a
  picture of the model. May come as a complement (« Exporter la vue en image »).
- **The frame and the options kept in the document** (as SketchUp's scenes): a format change
  for a habit; scenes, if they come, will be a decision of their own.
- **A command with a dialog** (size, format, then the current view): no way to see the
  crop of another ratio than the view's.

## Consequences

- The view keeps its camera: the capture frame narrows the projection for the render only.
- A width larger than the frame on screen is rendered, not upscaled: a large capture costs a
  render at that size (a 4K image with MSAA is about 260 MB of targets until step 3's tiles).
- On the web the read-back is asynchronous (`map_async`): the save panel comes a few frames
  later.
- `app.screenshot` stays the window as displayed; a control method for the capture can come
  later, the tool's request being the same.

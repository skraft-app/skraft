---
status: built
code: [crates/tools/src/tools/capture.rs, crates/tools/src/editor/view.rs, apps/skraft/src/workspaces/common/viewport/capture.rs, apps/skraft/src/workspaces/common/viewport.rs, apps/skraft/src/workspaces/common/viewport/options.rs, apps/skraft/src/kit/overlay.rs, apps/skraft/src/app/documents.rs, apps/skraft/src/system/file.rs]
---

# Capture

**« Capturer »** (`tool.capture`, Inspect › View, pinned) saves an image of the 3D view: a frame
over the view, the user navigating behind it as usual, then **✓** (the validation under the
option bar, or Enter), and the system's save panel (a download on the web). The image is
rendered again off screen at the size chosen, not copied from the screen. Decision record:
[Capture](../decisions/0060-capture-a-frame-over-the-view-rendered-off-screen.md).

## The frame

**Centred on the free part of the view** — below the ribbon, the option bar and the validation, between the
docked cards: what zoom extents frames (`Editor::free_area`) —, the largest of its ratio that
keeps 32 pt from its edges (`capture::frame_in`). Outside it the view is dimmed; under its right
corner, the image's size in pixels. **Why** there and not at the view's centre: the cards float
over the view, so its centre is not where the model is seen, and zoom extents puts the model in
the middle of the frame.

The ratio is an option: « Libre », 16:9 (the default), 4:3, 3:2, 1:1, A4 landscape and portrait.
**Free**, the frame's four corners are handles (the resize cursor over them): a corner dragged
sizes the frame about its centre, kept inside the margins; the size comes back for the
document's session (`Cx::capture_free`), as the last fillet's radius does.

The tool never picks: the arrow cursor, clicks do nothing; the navigation is the editor's. It
always waits for its validation (`awaits_validation`): **✓** (Enter) captures, the tool staying
for another image; **✗** (Esc) gives it up, back to Select. **Why** the validation rather than
a button of its own: the place and look every tool waiting for a go already has.

## The image

`Viewport::start_shot` renders the view's own frame (`Viewport::with_frame`, what the viewport
draws) into a picture of the image's size (`render_picture`):

- **The projection narrowed to the frame**: the clip space scaled and moved so that the frame
  fills the picture (`crop`). The image is exactly what the frame shows, perspective included.
- **The lines as in the frame**: `pixels_per_point` is the image's width over the frame's in
  points, so a large image is the frame enlarged, its edges no thinner.
- **The size**: the longer side chosen (1280, 1920 by default, 2560, 3840; each shown as the
  image it gives, « 1920 × 1080 »), the other from the ratio; scaled down to the GPU's largest
  texture when past it. The picture's multisampled targets are freed after it
  (`Renderer::release_picture_targets`).
- **The ground**: the theme's, white, or **transparent** — rendered on black then on white, each
  pixel's coverage from their difference (`unblend`, as the file's picture). PNG only: with
  JPEG the choice is not offered, white is used.
- **Read back without waiting**: the copy's buffer is mapped asynchronously and polled each
  frame (`Shot::poll`), the web's GPU not letting the interface wait; then encoded off the
  interface's thread on the desktop (`file::export_encoded`) — PNG with its alpha, JPEG at
  quality 92 —, and offered as `<document>.png` / `.jpg`.

## What the image keeps

**The view shows what the image will**: while the tool is on, the view leaves out what the
options leave out (`Preview::left_out`, carried by `View::left_out`):

| Option | Key | Off (the default) |
|---|---|---|
| « Sélection » | `keep_selection` | The selection's marks and boxes, the box of the part open or the assembly edited, the other occurrences a change would reach. |
| « Guides » | `keep_guides` | Every guide. |
| « Axes » | `keep_axes` | The axes and the grid. |

On, they show as the view's settings show them. Never in the image: the tool's previews, the
inferences, the cursor, the frame, the cards and bars. The style, the section, what is hidden and
the camera's projection are the view's.

The options — ratio, size, ground, format and these three — are the tool's settings, kept in the
user's preferences (`tool_options`), not in the document.

## Option bar

Its choices are drop-down lists (`Tool::lists_choices`), its three toggles icons (the select
tool's arrow, the guides', the axes'), the cards opaque over the dimmed view
([Application interface](interface.md#tool-option-bar)).

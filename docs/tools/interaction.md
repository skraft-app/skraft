---
status: built
code: [crates/tools/src/tools/mod.rs, crates/tools/src/editor/mod.rs, crates/tools/src/camera.rs, crates/tools/src/tools/select.rs, crates/tools/src/tools/erase.rs, crates/tools/src/scene.rs, crates/tools/src/vcb.rs, crates/model/src/expr.rs, crates/model/src/measure.rs]
---

# Interaction: SketchUp-style tools

How the user acts on the model: the active tool and its preview, navigation and standard
views, selection, hiding, the eraser, drags, typed dimensions and copies. The interaction
follows SketchUp's, with OpenCutList's refinements where noted. Inferences (the point a tool
takes under the cursor) have their own page: [Inferences](inferences.md).

## Active tool and preview

An **active tool** (`ToolKind`: Select, Box, Cylinder, Prism, Turn, Move, Rotate, the
booleans, Erase, Paint, Sample, Section, Measure, Stretch, the copy tools, Profile) receives
the viewport's events (hover, click, drag, keys) and produces, at the end, an `Operation`.
During the interaction it exposes a **preview** (ghost geometry) without touching the document.

## Tool stack

As SketchUp's `push_tool` / `pop_tool`: a tool may run **over** the active one, which waits
under it **as it is** (its step, its hover, its options) and comes back when the one over it
hands back (`editor/stack.rs`, `Editor::stack`, each entry with what it waits for, `Over`):

- **Held** (`Over::Held`): the paint tool holds « Prélever » (`Command::Hold`) when the other
  modifier (Command on macOS, Alt elsewhere) is pressed; released, or Esc, and the paint tool is
  back. The paint tool holds it on the press only, not while the key stays down after Esc.
  The tool held knows it (`Cx::held`).
- **Quick tools** (`Over::Quick`): a tool started from the quick tools' card runs over the
  creation tool and hands back once its action is done, or given up
  ([Quick tools](quick-tools.md#the-return)).
- Esc with the tool over at its first step hands back too. **Choosing a tool** (ribbon, key,
  command, `Command::Start`) empties the stack.
- The ribbon marks the tools waiting under the active one with their frame alone
  (`Editor::suspended_tools`, `CommandView::waiting`).

Decision record: [A tool stack](../decisions/0036-tool-stack.md)

## Navigation

SketchUp's navigation: middle button = orbit around the point aimed at, Shift+middle = pan,
wheel = zoom towards the cursor, middle double-click = recentre; trackpad: two fingers =
orbit, Shift+two fingers = pan, pinch = zoom. Z stays vertical (the Z of the last standard
view's frame, below). This is the default **navigation style**: the mouse's and the
trackpad's gestures can take another program's habits ([Navigation styles](../ui/navigation-styles.md)).
Perspective projection by default, switch to parallel, standard views (top, front, right,
back, left, iso) and zoom extents.

## Standard views in the active frame

Standard views are taken in the active frame: the open part's axes, else the entered
context's, as the axes drawn ([Data model](../model/data-model.md); `Editor::axes_transform`,
made orthonormal, its scale dropped). The frame's Z is up on screen; a mirrored frame is seen
as it is drawn (the top view of a left-handed part has its red axis to the left). The camera
keeps **the frame of its last standard view** (`Camera::frame`; yaw and pitch are measured in
it): **the orbit keeps that frame's Z up**, the part closed as well, until the next standard
view (Cmd+1…6 at the model's level brings world Z back). In a mirrored frame the orbit turns
its yaw the other way, so the model turns the same way on screen. The framing is unchanged
(the scene's bounds).

**Why:** levelling on world Z at the first orbit rolls the view at once, however small the
drag.

## Selection

- A click selects the top-level entity of the active context, Shift+click adds/removes,
  double-click on a group/component enters it, a click off the grey box of the one being
  edited (void or another entity) leaves it, one level per click; inside the box, a click in
  the void only clears the selection.
- **Esc** goes back one step of the active tool; at its first step, it does nothing more for a
  tool other than Select (the tool stays: only Space — the `tool.select` command — goes back
  to Select), and clears the selection with Select. Tools that pick first let the selection go
  instead ([Select first](selection.md#select-first)).
- **Rectangle**: dragging draws a selection rectangle: left to right (solid line, "window"),
  it takes the entities it fully contains; right to left (dashed, "crossing"), those it
  touches; Shift adds/removes. The test is done on screen, on the projected vertices and
  triangles of the display meshes.
- **Outside the active context**, the model is faded — or hidden, with the document setting
  `display.hide_rest_of_model` (command `view.hide_rest_of_model`, « Masquer le reste du
  modèle », off by default): while an assembly is entered or a part open, what lies outside it
  (its parts, its guides, the open part's other occurrences and their `Related` marks) is
  neither drawn, picked nor inferred on, and zoom extents frames the context alone.
- The selection scope « Parts » (parts at any depth) and the tools' picking phase:
  [Select first, and a selection scope](selection.md).

## Hiding

After SketchUp's `hidden`: an occurrence carries `hidden` (saved,
`Operation::SetHidden { context, entities, hidden }`, undoable; « Masquer » / « Afficher »),
toggled by the eye at the right end of each outliner row. One flag per occurrence, not per
component: a leg of four is hidden alone; an occurrence inside an assembly is hidden in every
occurrence of the assembly (its definition is shared).

- A hidden occurrence, and everything under it, is neither drawn, picked (click or
  rectangle), inferred on, framed by zoom extents nor taken by select all; its guides go with
  it; the outliner greys its row and those below, and hovering them there marks nothing.
  Hiding it takes it out of the selection; the outliner still selects it (its selection mark
  then shows where it is).
- **What is being edited shows**: the occurrences hidden along the active path (the
  assemblies entered, the part open) count as shown, what is hidden below them stays hidden
  (`PlacedPart::hidden_at`, the deepest hidden occurrence of the path, compared with the
  active path at each `DisplayScene::sync`).
- The document setting `display.show_hidden` (command `view.show_hidden`, « Voir les éléments
  masqués », off) draws them faded and lets them be picked, inferred on and framed;
  `view.show_all` (`Operation::ShowAll`, « Tout afficher », enabled while something is hidden)
  shows every occurrence again.
- Exploding a hidden assembly leaves its content hidden; copies keep the flag; imports (STEP,
  SketchUp) come in shown.

## Eraser

« Effacer », E, after SketchUp's. It takes what a click of Select takes — a guide near the
cursor, else a machining of the open part while they are shown, else the active context's
occurrence under it — and nothing outside the context; under the « Parts » scope, a guide
near the cursor at any depth under the context, else the part under it
([Selection](selection.md#selection-scope)).

- A click erases at once; a drag gathers everything it passes over (sampled every 4 px between
  two cursor positions, so a quick stroke misses nothing) and erases it on release, in one
  operation (one undo step); Esc drops the drag. With Shift held on release, the occurrences
  taken are hidden instead (`SetHidden`), guides and machinings left.
- The selection is let go when the tool starts (activating it erases nothing, unlike
  SketchUp).
- What would go is drawn as a ghost (`Preview::erasing`): translucent faces as an open part's
  blank, and translucent edges and silhouettes too (`ViewPlacement::faint_edges`, drawn by
  `render` after the ghosts' faces, at the style's `ghost_edge_opacity`; the blank keeps its
  edges opaque), in batches of its own, its selection marks dropped; erased guides are drawn
  faded.
- The command `edit.erase` (Delete, the Edit menu) erases the selection; both build the same
  operation (`Cx::erase`).

## Drag as click, move, click

After OpenCutList's SmartDraw (`smart_draw_tool.rb`): in Box, Cylinder, Prism and Turn,
pressing at the first step and dragging places the first point where the button went down,
the preview follows the cursor, and releasing is the second click (the base's corner, the
radius, the next corner, the axis's end).

- Released less than 20 px from where it went down, it is only the first click (a still press
  longer than egui's click time too); a press that took nothing (the tool still at its first
  step) makes the release take nothing either. At a later step, a drag is a click where the
  button is released. Esc during the drag, or another tool, drops the release.
- The same for the tools with points to place: Move (SketchUp's drag-move: from the press to
  the release), Rotate (centre, then reference), Measure, Section (face, then depth), the
  booleans (target, then tool); Paint painting faces, and « Prélever », take the release as
  their click (Paint painting occurrences draws a rectangle); Copy along a line or a
  grid and Distribute keep their selection rectangle while picking, then take a drag as clicks
  (Distribute: start at the press, end at the release; the copies: placed at the release).
  Select, Eraser, Stretch and Profile keep their own drags.
- The editor does the translation (`DragStart` → hover + click at the press point, `Drag` →
  hover, `DragEnd` → hover + click), for a tool whose `Tool::drag_clicks` asks for it
  (`DragClicks::PressAndRelease` / `Release` / `No`); the tools only see hovers and clicks.
- One difference with OCL: the first point is placed on the press (egui's drag start), OCL
  waits for the release or the 20 px; the result is the same.

## Booleans

A two-click tool (target, then tool), which designates solids of any context: this is how the
rule of [The kernel](../model/kernel.md) is expressed.

## Typed dimensions

The VCB, taken from OpenCutList (`UserTextHelper` + `LengthExpressionUtils`): during a tool,
typing a value sets the dimension. One can type:

- expressions: `600-2*19`, `2m*(2+3)`, `min(@; 300)`, `floor(@ - 5; 12; 15; 20)`;
- `@`, the gesture's current value: `@/2`, `@+19`;
- lists separated by `;` (empty slot = current value: `;400`); `=` repeats a value (`600=`
  = `600;600`);
- a shape's height right with the first input, optional: `600;400;19`, `50==` (a cube),
  `radius;height` for the cylinder; the shape is then done;
- points: `[x;y;z]` in the context's axes, `<dx;dy;dz>` from the start point (Move).

A bare number is in the document's unit: the computation is done in it, so `@*3` and `@-2`
both read. Units are checked strictly (`2m*3m` and `@/@` are refused). A typed length keeps
the direction of the gesture. `;` always separates a list; `,` does too on a system whose list
separator it is (decimal `.`: `10,5` is two values), otherwise `,` and `.` are both decimal
points. A list only splits outside parentheses and brackets.

### Typed lists

**Every list typed goes through one helper**, `measure::list`, as OCL's `_split_user_text`:
lengths, a point's coordinates, the copies' counts (`5x=` is `5x;5x`), Distribute's spacings
(`10==;20`), and the bubbles showing what is typed. It splits, trims, unfolds `=` (a lone `=`
is no repeat, refused), and says of each value which item typed it and whether it is a
repeat. Values beyond those a field takes are left out, typed or repeated (`600===` for two
values is `600;600`, `1;2;3` too), never an error. Lists written back (the box's current
values) are joined by the system's list separator (`measure::join`). A new field taking a list
reads it with `measure::list`.

The evaluator lives in `model` (`expr`, `measure`), to serve the fields too (Position in the
entity info). Each tool declares what it takes (`Tool::vcb`: lengths, angle, number of copies,
whether a point is accepted) with its current values; the editor reads the input and passes it
a `ToolInput::Value`, or shows the error.

### Typed into the preview's lengths

While a value is typed, the length bubbles show it, not only the box. A `PreviewLabel` says
which value of `Tool::vcb` it measures (`slot`); `vcb::typed` gives each value the item typed
for it — a `;` moves on to the next bubble (the rectangle's red, then green side) — with what
it reads as when that says more (`2*@ = 100 mm`, but `600`, not `600 = 600 mm`), greyed after
the text typed so the two are told apart. An item that does not read yet shows as typed. A
value repeated by `=` shows `= <result>`, greyed. The bubble being typed into is framed 2 px
with a fixed caret, right after the text typed (before the result). Already typed bubbles keep
their text and the others keep the cursor's measure. The preview's geometry still follows the
cursor: Enter applies the text. A value with no bubble on screen (the height typed with the
base, a count `5x`, a point `[x;y;z]`) shows only in the box. A bubble too short to write (a
move not started) shows only while it is typed (`hidden`). Every drawing tool ties its
bubbles: shapes (with the prism's bulge and tangent radius), Move, Rotate, Stretch, the tape,
and the Profile tool, whose grips have bubbles for this (way moved, bulge, fillet, radius,
thickness).

## Copies and arrays

SketchUp's, in Move and Rotate: a short tap of the copy modifier (Option on macOS, Ctrl
elsewhere), alone, toggles copying, before or during the gesture; the cursor carries a "+" and
copying drops once done. Right after a copy, typing `5x` (or `*5`) makes it an array of 5 with
the same step, `/5` spreads 5 copies over the same way; one can retype to correct. The array
is an operation (`Operation::Array`: `count` copies, the k-th transformed k times by `step`),
which replaces the copy in the history (`Command::Replace`): a single undo step. Translation
as well as rotation (circular array).

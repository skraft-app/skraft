---
status: built
code: [crates/tools/src/navigation.rs, crates/tools/src/editor/navigation.rs, crates/tools/src/camera.rs, apps/skraft/src/viewport.rs, apps/skraft/src/nav_prefs.rs, apps/skraft/src/prefs.rs]
---

# Navigation styles

Not every CAD program moves the camera with the same buttons: SketchUp orbits with the middle
button, Fusion 360 pans with it, Onshape and Rhino orbit with the right one. Skraft lets you pick the habits of the program you come from, as
FreeCAD's « navigation styles » do. You start from a preset and can change each gesture. The
keyboard and the tools' modifiers stay Skraft's own.

Decision record: [Navigation styles: presets of other programs, each gesture editable](../decisions/0026-navigation-styles-presets.md)

## What a style covers, what it does not

- **The style covers** the mouse's gestures (middle, right and side buttons 4/5, with or without
  modifiers), the wheel's direction, the orbit's pivot, the trackpad's gestures, recentring and
  zoom extents.
- **The style never covers**:
  - **the keyboard**: Cmd/Ctrl+1…6 (standard views), Space (`tool.select`), the tools' shortcuts
    and the VCB are the same in every style;
  - **the left button alone**: it belongs to the tools. Option/Alt+left (Maya's habit) is not
    offered either, because Option is the copy modifier on macOS ([Interaction](../tools/interaction.md));
  - **the tools' modifiers**: Shift (inference lock, [Inferences](../tools/inferences.md)) and the copy modifier (Option on
    macOS, Ctrl elsewhere) keep their meaning as soon as no navigation gesture is under way (see
    [Priority to the button](#priority-to-the-button));
  - **the navigation tools** (Orbit…, left button): unchanged, whatever the style.
- The speeds and the orbit's directions (mouse and trackpad separately, [Interaction](../tools/interaction.md)) stay global
  preferences, outside the style. The wheel's direction is the style's
  (`NavStyle::wheel_reversed`). **Why:** it is part of a program's habit.

## Model

The types live in `crates/tools` (`navigation.rs`, next to the camera), with serde, and can be
tested without the UI. The preferences (`prefs::Navigation::style`) hold one style.

```rust
pub struct NavStyle {
    pub preset: Preset,            // the style it was taken from
    pub custom: bool,              // edited since: the full style is stored
    pub bindings: Vec<Binding>,    // the mouse's gestures
    pub trackpad: TrackpadBindings,// two-finger scroll, Shift+scroll → Orbit | Pan; pinch = zoom
    pub pivot: Pivot,              // Aimed | SelectionOrModel | ScreenCentre
    pub wheel_reversed: bool,      // forward = zoom out (Fusion, SolidWorks)
}
pub enum Preset { SketchUp, Fusion, SolidWorks, Onshape, Rhino, Blender }
pub struct Binding { pub trigger: Trigger, pub mods: Mods, pub action: NavAction }
pub enum Trigger { Drag(Button), Click(Button), DoubleClick(Button) }
pub enum Button { Middle, Right, Back, Forward }      // never Primary
pub struct Mods { pub shift: bool, pub ctrl: bool, pub alt: bool, pub cmd: bool }
pub enum NavAction { Orbit, Pan, ZoomDrag, ZoomWindow, Recentre, ZoomExtents, PreviousView, NextView }
```

- **Presets are functions** (`Preset::style()`), not data in the file. While a style has not
  been edited, `preferences.json` stores only `{"preset": "fusion"}`. A preset corrected in a
  later version therefore reaches everyone who has not edited it. Once edited, the full style is
  stored with `custom: true` and the preset it came from (« Personnalisé (Fusion 360) »,
  « Rétablir Fusion 360 »).
- **Mods are physical keys**: a preset's Ctrl is Ctrl on every OS. Cmd/Ctrl+scroll stays egui's
  zoom (`zoom_delta`, [Interaction](../tools/interaction.md)), outside the bindings.
- **Resolution** (`NavStyle::resolve(trigger, mods) -> Option<NavAction>`): the binding whose
  trigger and modifiers match exactly, else nothing. A style has at most one binding per
  (trigger, modifiers); the editor refuses duplicates.
- **Restrictions on a binding** (`NavStyle::error`, a binding refused is not applied):
  `Click(Right)` (reserved for a context menu, see below); `Click` on a button that also has a
  `DoubleClick` with the same modifiers (the first click would fire before the double-click could
  be told apart); a drag action (`Orbit`, `Pan`, `ZoomDrag`, `ZoomWindow`) on a click or a click
  action (`Recentre`, `ZoomExtents`, `PreviousView`, `NextView`) on a drag; a second binding
  with the same trigger and modifiers (the first one applies). Every other combination is
  allowed.

## The presets

| Preset | Orbit | Pan | Zoom drag | Middle double-click | Wheel forward | Pivot | Trackpad scroll / Shift+scroll |
|---|---|---|---|---|---|---|---|
| SketchUp (default) | Middle | Shift+middle | — | Recentre | closer | Aimed | Orbit / Pan |
| Fusion 360 | Shift+middle | Middle | — | Zoom extents | farther | Aimed | Pan / Orbit |
| SolidWorks | Middle | Ctrl+middle | Shift+middle | Zoom extents | farther | Selection or model | Orbit / Pan |
| Onshape | Right | Middle, Ctrl+right | — | Zoom extents | closer | Aimed | Orbit / Pan |
| Rhino | Right | Shift+right | Ctrl+right | — | closer | Screen centre | Orbit / Pan |
| Blender | Middle | Shift+middle | Ctrl+middle | — | closer | Screen centre | Orbit / Pan |

- In every preset, button 4 (back) = `Click` → previous view and button 5 (forward) = `Click`
  → next view. The pinch zooms in every preset.
- The wheel's directions, the pivots and the trackpads of the programs other than SketchUp come
  from their documentation. Not yet: they have not been checked against each program.

## Gestures

- **Live modifiers**: the button is fixed when it goes down; the modifiers are read at every
  pointer event. The action is resolved again at every event, so pressing Shift in the middle of
  a middle-button orbit turns it into a pan (SketchUp's and Blender's habit). The pivot stays
  the one taken when the button went down. With modifiers that match no binding, the gesture
  does nothing until they match again; it ends only when the button is released.
- **Right button: click ≠ drag**. When `Drag(Right)` has a binding, nothing happens until the
  pointer has moved past egui's drag threshold. Released before that, it is a click, which
  stays reserved for a context menu: the viewport has none (not yet), so it does nothing. Ctrl+left
  click on macOS stays a left click with Ctrl (winit), the tool's.
- **The middle and side buttons**: a drag starts at once (`drag_started_by`).
- **The pivot** (`Editor::nav_begin_about`):
  - `Aimed`: the point picked under the cursor, else the camera's target;
  - `SelectionOrModel`: the centre of the selection's bounds, else the scene's bounds;
  - `ScreenCentre`: the camera's target (`eye + forward * focus`).
  `Recentre` always takes the point under the cursor.
- **ZoomDrag**: vertical motion, up = closer (the other way when `wheel_reversed`), towards the
  point under the cursor when the button went down, at the wheel's rate per 20 px.
- **ZoomWindow**: the drag draws a dashed rectangle (the selection rectangle's look). Released,
  the view keeps its direction and fits the rectangle; a rectangle smaller than the drag
  threshold does nothing.
- **Previous / next view**: a history of the camera, per document (the camera is per document,
  [Several documents](file-tabs.md)). A view is pushed at the end of each navigation gesture, standard view or zoom
  extents. Wheel zooms closer than 0.5 s to each other make one step. At most 50 views
  (`VIEWS_KEPT`); going
  back then navigating drops the views after it, as a browser does.

## Priority to the button

- Once a navigation gesture is under way, its modifiers belong to the navigation: the editor
  keeps the values of `set_shift`, `set_copy` and `set_alt` from before the gesture, and the
  tool does not see them change.
- An inference lock taken before the gesture is still on after it.
- After the gesture, a modifier pressed during it stays hidden from the tool until it is
  released. A Shift held to pan therefore does not lock an inference when the button goes up.
- **The copy modifier's short tap** (toggle, [Interaction](../tools/interaction.md)) does not count when the key was down during a
  navigation gesture. SolidWorks' Ctrl+middle pan on Windows does not toggle the copy.

## Interface

The Preferences' *Navigation* tab (`apps/skraft/src/nav_prefs.rs`): one column of labels, the same width (170 pt) in every
section, each section a semibold title and a thin line (`style::settings_section`,
`settings_grid`, the Model tab's too); the tab scrolls down to the screen's bottom.

- **Style de navigation**: « Comme » (the six presets; « Personnalisé (…) » when edited, with
  « Rétablir … »), « Pivot de l'orbite ».
- **Souris**: first the whole mouse's settings, « Molette » (inverser le sens, the style's) and
  its orbit inversions; then a **drawing of the mouse**, centred: the left button dotted (« Gauche : les
  outils », not to be picked), the wheel (middle button), the right button, the side buttons 5
  and 4 on its left. Each button is led by a line to its gestures, written as sentences
  (« ⇧ Glisser → Panoramique »; « — » for none; a refused one in red): the side buttons' on the
  left, the middle and right ones' on the right. A click on a button, or on its gestures, picks
  it (filled, its title semibold and, with its line, in the accent colour; the middle button at
  first). Under the drawing, a framed editor with **a tab per button** (the ribbon's tabs, named
  as the buttons in the drawing; the frame, and so the window, as wide as they are), the tab shown and the button
  picked in the drawing being one: picking either picks the other. Its gestures, one row each
  across the frame: the modifiers as toggles (⇧ ⌃ ⌥ ⌘
  on macOS, Maj Ctrl Alt elsewhere), the trigger (« Glisser », « Clic », « Double-clic »), → the
  action (only those of the trigger's kind), delete; why a row is refused painted under it (not
  widening the columns). « Ajouter un geste » adds one on that button (a drag, a click for the
  side buttons) with the first modifiers it does not have yet, so it applies at once. The
  framed editor ends the section.
- **Trackpad**: « Deux doigts » and « ⇧ + deux doigts » (Orbite / Panoramique), « Pincer » (Zoom,
  fixed), the trackpad's orbit inversions.
- **Vitesses**: orbit and zoom speeds, « Cible du zoom » (vers le curseur); then, under a line,
  « Valeurs par défaut ».
- Any change switches the style to custom. Action names: « Orbite », « Panoramique », « Zoom »,
  « Zoom fenêtre », « Recentrer », « Zoom étendu », « Vue précédente », « Vue suivante ».
  Program names are not translated (i18n catalog, [Texts and languages](texts.md)).
- Rows of combos rather than capturing a gesture by pressing it. **Why:** every button, the side
  ones included, can be set without a mouse that has it.

## Code

- `crates/tools/src/navigation.rs`: the types, `Preset::style()`, `NavStyle::resolve`,
  `drags_with`, `error` (the refused bindings), the stored form (`Stored`: the preset alone till
  edited). Tests: each preset resolves its table, the refused bindings, the stored form.
- `crates/tools/src/editor/navigation.rs`: `Editor::nav_begin_about` (the pivot of the style),
  `zoom_window` (`Camera::zoom_window`), the views gone through (`view_changing`, called before
  any camera move: gesture, standard view, zoom extents, projection, recentre, a wheel or pinch
  burst; `previous_view`, `next_view`). The view a move ends on is recorded lazily, at the next
  move or at « previous view ». A new or replaced document starts with no history.
- `apps/skraft/src/viewport.rs`: a gesture state (`Gesture`: its button, the press point,
  the pointer, its action, the modifiers the tool had) resolved at every frame; the tool's
  modifiers (`Viewport::tool_modifiers`); the zoom window's dashed rectangle; the cursor (a hand
  for orbit and pan, a magnifier for the zooms). `Viewport::navigating` tells the modifier taps
  (`App::modifier_taps`) to ignore a tap made during a gesture.
- The control protocol's `ui.drag` / `ui.click` send every button (`"button"`: `left`,
  `right`, `middle`, `back`, `forward`; `driving.rs`), with modifiers: the presets can be
  driven from a script.

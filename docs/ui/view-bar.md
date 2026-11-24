---
status: built
code: [apps/skraft/src/view_bar.rs, apps/skraft/src/pins.rs, apps/skraft/src/app.rs, apps/skraft/src/app/, apps/skraft/src/kit/icons.rs, crates/tools/src/editor/mod.rs]
---

# View bar

A column of icon buttons over the left or right edge of the view, holding the view's settings:
the standard views, zoom extents, the projection and the display toggles. After SolidWorks'
heads-up view toolbar. **Why:** the view's settings sit in the view, beside what they change,
and leave the title bar to the file tabs.

## What

A column of icon buttons floating over the **left edge of the view** (or the right one, see
[Left or right](#left-or-right)), **centered on the view's height** (never over the ribbon nor
past the view's bottom margin) — right of the left column of docked cards when there is one —,
a window frame translucent at rest and nearly opaque under the
pointer, as the quick tools' card
([Quick tools after a creation, naming a part just made](../tools/quick-tools.md)).

## The commands

The standard views (one button opening their menu), zoom extents, perspective / parallel (lit in
perspective), X-ray, hide the rest of the model, section, grid, axes, guides, silhouettes, show
hidden. Pinned by default: views, zoom extents, perspective, X-ray, hide the rest of the model,
section. The Materials panel is not in it: a panel, not a view setting (`view.materials`, from
the command palette or a shortcut, shows it).

## Choosing them

As the ribbon's and the quick tools' pins: a last ▾ button opens the list with a pin on each row
(a click on a row runs it); right click on a button: Unpin. **Reordered by a drag** (the quick
tools' card too): a button dragged along the column — past egui's threshold, so a click stays a
click — follows the pointer, faded, a line where it will go; a command pinned goes at the end.
Preference `view_bar: Option<Vec<String>>`, the pinned ids **in the user's order** (absent = the
defaults, unknown ids and repeats ignored; back to the defaults, absent again).

## Left or right

As the side panels' two columns: a grip (two rows of dots) at the bar's head; dragged, the bar
follows the pointer, the slot on the pointer's side lit as a docked card's empty column, and let
go it is docked there. A slot: between that side's column of cards and the view, centered on
the view's height. Its menus open towards the view. Preference
`view_bar_side` (`dock::Dock`, left by default); zoom extents stops at the bar on either side
(`view_bar::Place`, `Action::ViewBarSide`): it frames what is left free beside the bar.

## The title bar keeps the rest

The title bar has no standard views' menu nor view toggles: it keeps the mark, File, Undo /
Redo, the file tabs, the command search and the preferences
([Title bar and native menus](title-bar.md)). The command palette (and the shortcuts) keep
every command: there is no View menu, the title bar and the macOS menu bar having only File.

## X-ray and hide the rest are the document's

`display.xray` joins `display.hide_rest_of_model` ([Display](../render/display.md); X-ray in
[Rendering and look](../render/look.md)): saved with the document, one per tab, undone as the
other display settings. `Editor::set_xray` applies a `SetSettings`; the scene takes
`display.xray` in `sync_scene`.

## As built

`apps/skraft/src/view_bar.rs`: an `egui::Area` at (`free_left` or the view's left margin, its
top from the height last drawn so that it is centered: `view_bar::Place`, a frame whose height
changed discarded), a window frame at 35 % / 92 % (`style::translucent`: the hover of the last
frame, or one of its menus open), buttons 26 pt (icon 18), as the option bar's and the quick tools' card's (`style::BAR_BUTTON`), the selection's fill behind a lit one; the
views button is the pseudo-command `view.views` (its menu opens towards the view,
`RectAlign::RIGHT_START` on the left, `LEFT_START` on the right), the ▾ menu as the quick
tools' (`pins.rs`: `pinned` / `toggle` / `move_to` and the drag, `pins::reorder`, shared;
`Action::ViewMove` / `QuickMove`), a toggle's row with its check box (`style::menu_check`, the
menus' check column), pinned or not. `Action::ViewPin(id)` → preference `view_bar`. Handles
`view_bar:<command>`, `view_bar:more` ([Control protocol](../control-protocol.md)). Icon
`hide_rest` (a cube, another dashed behind it); `icons::command` knows the view toggles. Texts
`view_bar.*`.

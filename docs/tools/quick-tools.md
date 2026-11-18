---
status: built
code: [crates/tools/src/editor/quick.rs, apps/skraft/src/quick.rs, crates/tools/src/editor/naming.rs, apps/skraft/src/naming.rs, crates/tools/src/editor/mod.rs, crates/tools/src/tools/shapes.rs]
---

# Quick tools after a creation, naming a part just made

What follows a creation by Box, Cylinder, Prism or Turn: a floating card of tools to start on what
was just made, after OpenCutList's floating tools of its Smart Draw tool, and the naming of the
new part, after OCL's `ask_name`. Both come with each creation, each on its own.

## Quick tools after a creation

After OpenCutList's floating tools of the Smart Draw tool (`tool/smart_draw_tool.rb`:
`_append_floating_tools_at`, `_remove_floating_tools`, the Smart Handle tool's
`callback_action_handler` and `startup_selection`, the typed text handed back in
`smart_tool.rb`'s `onToolUserText`).

- **What**: once a creation tool (Box, Cylinder, Prism, Turn) has put down its part, or its
  body in an open part, a small floating card of buttons appears in the view, each starting a
  tool on what was just made. Started otherwise (ribbon, shortcut), the tools behave as usual: no
  return slot.
- **Where**: a row of buttons centred on the centre of the new part's box (or body's), projected
  on screen, following the view as it orbits or zooms (OCL's anchor); the buttons drawn as
  the ribbon's (our icons, the name in the tip), on a translucent card.
- **When it goes**: at the first click (or typed value) of the next shape, on Command (Alt
  elsewhere, OCL's), Esc, a change of tool or of active context, an undo, and when the tool it started
  hands back.
- **The commands**: a fixed list of tools that make sense on a part just made — Copy along a
  line, Copy along a grid, Distribute, Move, Rotate, Stretch, Paint (`quick::COMMANDS`). Pinned
  by default (`quick::DEFAULT`): the three copy tools (OCL's, less Move along line and Paneling,
  which Skraft has not).
- **Choosing them, as the ribbon's pins** ([Workspaces and ribbon](../ui/ribbon.md)): a last ▾
  button opens the list of the commands, each row with a pin; right click on a button: Unpin.
  Order is the user's: a button dragged along the row ([View bar](../ui/view-bar.md);
  `quick::move_to`), a pin added at the end. Preference `quick_tools`: the pinned ids, absent =
  the defaults (unknown ids ignored). No pin at all: only the ▾ button.
- **A click starts the tool on the new part**: the selection becomes what was made, the tool
  starts with it as its target (picking skipped, as with a selection when it starts).

### The return

As OCL's: the editor keeps the creation tool as a **return slot** (not a stack: one level). The
tool started from the card hands back to the creation tool as soon as its action is done (the
copies' click, Distribute's second click), or on Esc without doing anything. Back in the creation
tool, the tool that handed back is kept **aside**: what is typed before the first click of the
next shape goes to it, so `x5` or a distance redoes the copies (`Command::Replace`, one undo
step), as OCL's draw tool hands its text to `previous_action_handler`. The card is not shown again
after such a return; given up (Esc, nothing done), the card and the selection are back as they
were (a deviation: OCL drops its card on any return).

Decision record: [Quick tools hand back through a one-level return slot](../decisions/0022-quick-tool-return-slot.md)

As built (`editor/quick.rs`):

- `Editor::start_quick_tool(kind)` remembers the active tool (`quick_return`); the tool started
  hands back **after the first input whose commands apply an operation** (`Command::Apply`, but a
  `SetBehavior` — the stretch's cutters kept —, or `Command::Replace`).
  **Why**: no tool tells when it is done; the editor sees it act (copy tools at their click or
  typed distance, Move and Rotate at their last click, Stretch at its last click or drag's end,
  Paint at its first click).
- Esc that leaves it at its first step (or at it already) hands back without anything aside, the
  card it was started from offered again and the selection as it was (`quick::Return`).
- The tool aside takes what is typed while the creation tool is at its first step (before its own
  options' `<length>x`), and the measurements box shows its value; it is let go when the creation
  tool gets busy (the next shape's first click), on Esc, or when a tool is chosen. Choosing a tool
  drops the return slot too.

### The card

In the editor (`editor/quick.rs`): an `AddPrimitive` or `AddBody` applied by a tool ends with
`After::Card { ask_name }` (nothing selected), which offers the card on what it made — the
occurrences created, or in an open part the bodies, unless its machinings are hidden (they could
not be selected). Its anchor is the centre of their box in the context's axes
(`handle::occurrences_bounds` / `bodies_bounds`, the copy tools' box), in world;
`Editor::quick_card()` projects it each frame. The card belongs to the history's revision and the
context it was made at (any step, undo, redo, entering or leaving, and it is gone); it is hidden
while the kernel works or the tool is busy, and let go when the tool gets busy, on Alt / Esc, or
a tool chosen (`Editor::dismiss_quick_card`). `start_quick_tool` takes it: what it was offered on
becomes the selection.

In the app (`apps/skraft/src/quick.rs`): an `egui::Area` centred on `quick_card()`, kept inside
the view, drawn after the docked cards; a window frame at 35 % opacity at rest (its
border and shadow faded alike, the icons not) and 92 % under the pointer (`style::translucent`,
hover of the last frame kept in egui's temp data), icon buttons 34 pt (icon 22, the name in the tip), then a
narrow ▾ (the ribbon's chevron) opening the list `quick::COMMANDS` with pins (as a ribbon group's
menu, a click on a row starts it too; its note wraps within the rows' width).
`Action::QuickTool(id)` → `Editor::start_quick_tool`, `Action::QuickPin(id)` → `quick::toggle` on
the preference `quick_tools: Option<Vec<String>>` (`None` = `DEFAULT`; back to the defaults,
`None` again). The view under the card gets no hover (another layer): the creation tool hides
what follows the cursor while it is over the card. Texts `quick_tools.*`.

Not yet: the card after a paste.

## Naming a part just made

After OpenCutList's Smart Draw tool (`tool/smart_draw_tool.rb`, `fn_ask_name`, the same in
`smart_build_tool.rb`): once it has made a part, either it asks its name at once
(`UI.inputbox`, option `ask_name`), or a success notification « Nouvelle pièce créée : {{ name }} »
offers a « Renommer » button that asks it. The editor says when, checks the name and names the
part (`editor/naming.rs`, `Naming`); the dialog is the app's (`apps/skraft/src/naming.rs`).

- **When**: a creation tool (Box, Cylinder, Prism, Turn) has made a part (`AddPrimitive`), not
  a body in an open part (`AddBody`: no new part), nor a copy or a paste (the same parts).
- **The toast**: a success « Nouvelle pièce créée : Pièce#3 » with a « Renommer » button, at
  the bottom center as the others ([Toasts](../ui/interface.md#toasts)); the quick tools' card
  ([above](#quick-tools-after-a-creation)) shows as well, each on its own. The button opens the
  naming dialog. The toast goes as soon as its part is no more (undone, erased): no button on a
  part that does not exist.
- **The option « Nommage des pièces »** (OCL's `ask_name`): a toggle of each creation tool's
  option bar, kept with its settings (`ask_name`, off by default, shared by the four tools,
  [Shared settings](draw-options.md#shared-settings)), OCL's icon (a frame holding an N and a
  caret); hidden in an open part. On, the dialog opens as soon as the part is made, and no toast.
- **The dialog**: a modal « Nom de la pièce », one field for the part's name
  (`RenameComponent`), filled with its name, wholly selected (typing replaces it); Enter or OK
  names it, Esc or Cancel keeps its name. An empty name or one another component has: OK greyed
  and a line under the field says why; the dialog stays.
- **Undo**: named from the dialog opened by itself, the name joins the creation's step (one
  undo takes the part away, its label the creation's), as OCL's inputbox inside the operation;
  from the toast's button, a step of its own (« Renommer »).

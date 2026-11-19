# A tool stack: a tool over another suspends it, as it is

- Status: accepted (2026-11-19); supersedes [0022](0022-quick-tool-return-slot.md)
- Design: [Interaction: the tool stack](../tools/interaction.md#tool-stack)

## Context

The paint tool's pipette was first one more choice of its option (« L'exemplaire » / « La face »
/ « Pipette ») and the modifier held: an odd mix, a way of taking a material among ways of
painting. SketchUp keeps tools apart and lets one run over another (`Sketchup::Tools`'
`push_tool` / `pop_tool`). The quick tools already handed back to the creation tool through a
one-level return slot ([0022](0022-quick-tool-return-slot.md)), which made the creation tool
again from nothing. Decided on 2026-11-19 (questions / answers).

## Decision

- « Prélever » is a tool of its own (`ToolKind::Sample`, ribbon « Apparence » beside « Peindre »,
  no key). Chosen on its own, it replaces the active tool, as any tool, and starts the paint
  tool with the material it took.
- The editor keeps a **stack** of the tools waiting under the active one (`Editor::stack`,
  `editor/stack.rs`), each **as it was** (its step, its hover, its options), with what it waits
  for (`Over`):
  - `Held`: the paint tool holds « Prélever » over itself when the other modifier is pressed
    (`Command::Hold`); released, or Esc, and the paint tool is back. Each click takes meanwhile.
  - `Quick { card, selection }`: a quick tool started from the card over the creation tool, which
    hands back once its action is done (the tool that ran kept aside) or given up (the card and
    the selection put back) — 0022's behaviour, through the stack.
- Choosing a tool (ribbon, key, command, `Command::Start`) empties the stack.
- The ribbon marks the active tool, and the tools waiting under it with their frame alone.

## Alternatives considered

- **The pipette as a choice of the paint tool's option** (built first, same day): mixes what a
  click paints with taking a material; replaced.
- **A one-level « previous tool » for the pipette only**: a second return mechanism beside the
  quick tools' slot; one stack serves both.
- **Making the tool under again on return** (0022's way): loses its state; the stack keeps the
  instance, so the paint tool comes back as it was left.
- **« Prélever » from the ribbon pushed over the active tool**: it would hand back to a tool that
  does not paint; it starts the paint tool instead (asked on 2026-11-19).

## Consequences

- Any tool may later hold another one for a key or a gesture (`Command::Hold`), or the stack
  carry more reasons (`Over`).
- The paint tool holds « Prélever » only when the modifier is **pressed** (not while it stays
  held after Esc).
- The journal records the modifier, not the push: a replay pushes again from it.

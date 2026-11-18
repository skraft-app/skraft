# Quick tools hand back through a one-level return slot

- Status: accepted (2026-11-15)
- Design: [Quick tools after a creation, naming a part just made](../tools/quick-tools.md#the-return)

## Context

Decided on 2026-11-15 (questions / answers), after OpenCutList's floating tools of its Smart
Draw tool: once a creation tool has made a part, a card of tools starts one of them on it, and
OCL comes back to the draw tool afterwards, handing it the text typed next
(`previous_action_handler`, `onToolUserText`).

## Decision

- The editor keeps the creation tool as a **return slot**: not a stack, one level.
- The tool started from the card hands back as soon as its action is done, or on Esc without
  doing anything. No tool tells when it is done: the editor sees it act — it hands back after the
  first input whose commands apply an operation (`Command::Apply`, but a `SetBehavior`, or
  `Command::Replace`).
- Back in the creation tool, the tool that handed back is kept **aside**: what is typed before
  the next shape's first click goes to it (`x5` or a distance redoes the copies,
  `Command::Replace`).
- Started otherwise (ribbon, shortcut), the tools have no return slot.
- Built on 2026-11-15 in four steps: the return slot and the tool aside; the card offered after a
  creation, its anchor; the card in the app, its pins, its preference; the texts and the editor's
  scenarios.

## Alternatives considered

- OCL drops its card on any return. Asked on 2026-11-15, after use: given up (Esc, nothing done),
  the card and the selection are back as they were; after a return with something done, the
  card is not shown again.

## Consequences

- The card belongs to the history's revision and the context it was made at.
- Not now: showing the card after a paste; OCL's Move along line and Paneling (which Skraft has
  not).

---
status: built
code: [crates/tools/src/tools/boolean.rs, crates/model/src/boolean.rs, crates/model/src/ops.rs]
---

# Solid tools: union, subtract, intersect

« Unir », « Soustraire » and « Intersecter » combine parts, after OpenCutList's solid actions of
Smart Reshape (`tool/smart_reshape_tool.rb`: `SmartReshapeSolidActionHandler`;
`worker/common/common_solid_boolean_apply_worker.rb`): the parts **A** (the targets) are changed
by the parts **B** (the tools). What each one makes of the tools (a negative body, positive
bodies, a free shape) is the model's: [Booleans between parts](../model/bodies.md#booleans-between-parts).

## Interaction

- **Two steps**, A then B, each with its cursor (OCL's `select_a`, `select_b`). A click takes the
  part under the cursor and goes on: to B, or the operation applied.
- **Several parts** (OCL's multiple selection): a Shift click adds a part to the step's set or
  takes it out, and the set then waits — a plain click too adds or takes out, once the set holds
  more than one. **Enter**, or the ✓ button, goes on; ✗ is Esc. Shift let go with a set waiting
  says so in the status bar (OCL's `validate_a` / `validate_b` notifications).
- **The steps bar** ([Steps bar](../ui/interface.md#steps-bar)): ① A, ② B, and the step's
  sentence; the ✗ / ✓ buttons, under the tool's options, while a set waits.
- **A union takes one target**: its first step takes one part and goes on; its tools gather as
  the others'.
- A part of A is not taken as a tool (not hovered, a click on it does nothing).
- **Esc**: the tools let go, then back to A with none kept, then the targets let go.
- **The selection is ignored** ([Selection](selection.md)): the tool always starts at A.
- A drag from a part to another is a click on each (`DragClicks`).
- **Marks**: the targets in the picked orange (`Mark::Picked`), the tools lighter
  (`Mark::PickedTool`, `Theme::picked_tool`), the other occurrences of the first target's part
  `Related` under « Tous les exemplaires ».

## Options

In the option bar ([Application interface](../ui/interface.md)), remembered:

| Option | Key | |
|---|---|---|
| « Portée » | `scope` | All occurrences of a target's part, or only the one clicked ([Copies and propagation](../model/data-model.md#copies-and-propagation)); per tool, left out for a part placed once. |
| « Conserver A » | `keep_targets` | The targets stay as they were; each result goes to a new part placed beside its target (same place, material, occurrence name). |
| « Conserver B » | `keep_tools` | The tools' occurrences stay. |
| Broom | `drop_unused` | The tools' parts left without occurrence are deleted, even named (OCL's `remove_unused_definitions`, its broom icon); without it they stay in the « Composants » card's « Inutilisés » until purged ([Components library](../ui/components-library.md)). |

The last three are shared by the three tools (`synced_with`, OCL's
`get_action_option_sync_actions`).

## The operation

`Operation::Boolean { targets, tools, op, scope, keep_targets, keep_tools, drop_unused }`, one
undo step (`Document::booleans`, `crates/model/src/boolean.rs`):

- **The tools reaching a target**: those whose bounds meet the target's, in its axes (a box test
  on the records, conservative, no kernel — `extent_along`, `placed_extent`). A difference cuts
  each target with the tools reaching it; an intersection keeps what each target shares with the
  **union** of those tools (OCL's `(∪A) ∩ (∪B)`, fragment by fragment), the tools kept in the
  target as an intersection body or group, editable
  ([A third sign](../model/bodies.md#a-third-sign-the-intersection)); a union joins every tool
  to its target. A target reached by none is left alone; none reached at all, the operation is
  refused (« aucun outil n'atteint une cible »).
- **Tools leave the model** once used, unless kept; a tool that reaches nothing stays.
- **Targets of one part** ([Copies and propagation](../model/data-model.md#copies-and-propagation)):
  those the operation leaves identical in their own axes — reached by tools of the same shape
  placed alike — keep one part; the others get parts of their own, the change most of them take
  made in place. Under « Celui-ci », the part changes in place only when the targets are every
  placement of it; else every group gets a part of its own (`Document::separate`).
- Refused: a part both in A and in B, no target or no tool, a union of several targets.

# Solid tools take sets of targets and tools, as OpenCutList's

- Status: accepted (2026-11-19)
- Design: [Solid tools](../tools/solid-tools.md)

## Context

The booleans took two clicks, a target then a tool, and always consumed the tool: drilling ten
boards meant twenty clicks, and a tool needed again had to be copied first. OpenCutList's solid
actions take a set A and a set B (Shift to gather, a validation to go on) and offer to keep A, keep
B, and delete the definitions left unused. A named part consumed by a boolean also stayed in the
document without occurrence, out of sight (no components library). Decided on 2026-11-19
(questions / answers).

## Decision

- `Operation::Boolean` takes `targets` and `tools` (several of each), one undo step, with
  `keep_targets`, `keep_tools` and `drop_unused`.
- **A union takes one target**, several tools.
- Each target takes **the tools reaching it** (a box test, no kernel); an intersection keeps
  what a target shares with the union of its tools.
- Targets of one part reached alike keep sharing it; diverging ones get parts of their own (the
  Stretch's rule, OCL's).
- **The selection stays ignored**: the tool always starts at A.
- The components library (listing, placing, purging unused components) is a separate piece of
  work.

## Alternatives considered

- **OCL's union of several targets**: touching groups merging into the first target of each,
  decomposition into lumps; Skraft's union adds bodies to one part, so several targets would need
  a contact test and a choice of the receiving part. One target keeps it plain.
- **Every tool on every target**: needless bodies in parts a tool does not reach.
- **The selection as A** (select first, [0031](0031-tools-select-first.md)): kept as before; the
  tool always picks its sets itself.
- **Deleting a consumed named tool part always**: an option instead, as OCL's, the default
  keeping it.

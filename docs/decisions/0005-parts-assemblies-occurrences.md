# Every object is an occurrence of a part or an assembly

- Status: accepted (2026-11-10)
- Design: [Data model: parts, assemblies, machinings](../model/data-model.md)

## Context

The v0 prototype was built on a SketchUp-like model: groups and components (create from the
selection, explode, enter / leave editing, make unique, multiple instances), with loose
geometry. Skraft's parts are meant to be cut-list parts, as in OpenCutList: counted without
guessing, copies of one part recognised as such, a part's type and main direction known.

## Decision

- **A single sharing mechanism**: a component is a shared, named definition, placed by
  occurrences. A part holds a B-rep (then: its blank and its machinings, since:
  [its bodies](0018-part-bodies.md)); an assembly holds occurrences of other components.
- **No more groups**, no more loose geometry: everything in the model is an occurrence.
  Drawing a box creates a part and its first occurrence; gathering occurrences creates an
  assembly.
- A part is edited directly; an assembly is opened like a SketchUp component.
- The part's type comes from the material applied to the occurrence, as in OpenCutList
  (`MaterialAttributes.type`), not from a component property.
- An occurrence's T is a translation, a rotation and an optional mirror: never baked into the
  B-rep, no scale. The mirror stays in T (checked on 2026-11-10, `crates/kernel/tests/part_model.rs`):
  a part and its mirror image remain the same part.
- A change to a part affects all its occurrences or only this one (the scope), after the solid
  operations of Smart Reshape (OpenCutList, `common_solid_boolean_apply_worker.rb`).
- Stable `u64` ids, unique in the document for components, within their assembly for
  occurrences, so that a copy of a component keeps its occurrences' ids.

## Alternatives considered

- **The former model** (groups and components with loose geometry, step 2 of the v0
  prototype): built, then replaced. It was dropped with its file converter on 2026-11-12: the
  app is not published, the format stays at version 1 until it is
  ([File format version 1 until the first release](0010-file-format-v1-until-release.md)).

## Consequences

- Copying an occurrence creates an occurrence of the same part: "Leg ×4", without thinking of
  "making a component" before copying.
- Propagation must be visible (the number of occurrences affected, the affected occurrences
  marked), and "Make unique" becomes "Make a separate part".
- Paths of occurrences from the root designate entities; the active context is an assembly
  and the path leading to it.

# Typed properties, their schema optional

- Status: accepted (2026-11-19)
- Design: [Properties](../model/properties.md)

## Context

Materials, and nearly every entity of the model, need metadata: what a cut list, a quote or an
extension reads (a price, a supplier, a species), what an import brings (SketchUp's attribute
dictionaries, OpenCutList's among them). SketchUp's attributes are open — any dictionary, any
key, untyped Ruby values — and the interface never shows them. Decided on 2026-11-19 (questions
/ answers): typed values, a schema optional.

## Decision

- **Every value typed**: `Value` (bool, int, real, text, length, area, volume, angle, mass,
  money, colour, choice, list), written with its kind in the file.
- **Keys namespaced**: `<namespace>.<name>` (`user.*`, `skp.<dictionary>.*`, an extension's).
- **A schema optional**: `Document.prop_defs` — label, kind, choices, default, the entities it
  applies to — checks the values of the keys it defines; a key without one keeps its typed values.
- **On the entities**: a field `props` on components (the root's: the model's), occurrences,
  body definitions, bodies, materials and guides; not on faces.
- **Two operations**: `SetProps` (keys set or removed on one entity), `SetPropDef` (a key
  defined or not any more; refused if values there contradict it).
- **Pastes rename** a key defined otherwise here (`key_2`), with a warning.
- **What Skraft's code reads stays typed fields**, not properties.

## Alternatives considered

- **SketchUp's attributes as they are** (dictionaries of untyped values): nothing to build a form
  from, no units, a typo is a new key.
- **A schema required** (a definition for every key, made by an import for its own): strict,
  but an import or an extension would have to declare everything it brings, and a file of an
  extension not installed would show nothing.
- **A table of properties in the document**, by entity: every operation that copies, moves or
  makes unique would have to keep it in step.
- **Properties for Skraft's own data** (a material's thickness, grain): `get` by string where
  a field is checked by the compiler.

## Consequences

- Every entity's struct has one field more; copies carry it with the data.
- A part pasted into a part (a group of bodies) leaves its own properties behind.
- Shown and edited in the entity info, the definitions in the model's (asked on 2026-11-19).

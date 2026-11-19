---
status: built
code: [crates/model/src/props.rs, apps/skraft/src/panels/props.rs, crates/model/src/ops.rs, crates/model/src/document.rs, crates/model/src/clipboard.rs]
---

# Properties

Typed values the user, an import or an extension attaches to the model's entities — a price, a
supplier, a species, a sheet's reference —, and the document's definitions that give them a
label, a kind and choices. For anyone reading or changing `crates/model`: what a property is,
where it lives, how a definition checks it, how it travels with copies and pastes. They are shown
and edited in the entity info ([Application interface](../ui/interface.md#properties)).

Decision record: [Typed properties, their schema optional](../decisions/0040-typed-properties-schema-optional.md).

## Values

```
Props = key → Value
Value = Bool | Int | Real | Text
      | Length (mm) | Area (mm²) | Volume (mm³) | Angle (rad) | Mass (kg)
      | Money { amount, currency }      currency: ISO 4217 (« EUR »)
      | Color [u8; 3]                   sRGB
      | Choice (one of its definition's choices)
      | List [Value]                    of one kind, no list in a list
```

Each value is written with its kind (`{"length": 18.0}`, `{"money": {"amount": 12.5,
"currency": "EUR"}}`): a key without a definition keeps its kind, whatever reads the file.
Lengths, areas, volumes, angles and masses are stored in the model's units and shown, typed, in
the document's (`Settings::units`). A value is **well formed** when its numbers are finite, a
currency named, a list's elements of one kind.

**A key** is `<namespace>.<name>`: the namespace in lowercase ASCII letters, digits, `_` or
`-`, the name not empty, no control character. `user.*` for what the user declares;
`skp.<dictionary>.*` for SketchUp attributes an import brings (to come); an extension's own
later.

## Where they live

On the entities themselves, each a field `props` (absent from the file when empty):

| Entity | `EntityKind` | `PropTarget` |
|---|---|---|
| The model: the root assembly | `model` | `Component(root)` |
| A part, an assembly | `part`, `assembly` | `Component(id)` |
| An occurrence | `occurrence` | `Occurrence { context, occurrence }` |
| A body definition (every body of it) | `body_def` | `BodyDef { part, def }` |
| A body | `body` | `Body { part, body }` |
| A material | `material` | `Material(id)` |
| A guide | `guide` | `Guide { component, guide }` |

Not on faces: their indices change at every boolean.

They go **with the data**, as names do: an occurrence copied (Move's copy, arrays, pastes)
keeps its occurrence's properties, a part made unique its part's, a body copied or stretched its
own and its definition's. What changes kind loses them: a part pasted into a part becomes a
group of bodies, its own properties left behind (its bodies' kept).

## Definitions

`Document.prop_defs: key → PropDef`, **optional**:

```
PropDef { label, kind: ValueKind, choices, default, applies_to: {EntityKind} }
```

- `label`: its name as shown (« Prix »); empty, the key's name.
- `kind`: one of the values' kinds, `List(kind)` a list of one.
- `choices`: a choice's values (of a choice, or a list of choices), none empty or twice;
  required exactly then.
- `default`: what an entity without a value is shown with; it fits the definition.
- `applies_to`: the kinds of entities it applies to; empty, all.

A value of a defined key must **fit** its definition — its kind, among its choices — and be on
an entity it applies to. A key without a definition takes any well formed value, shown as it is
(an import's, an extension's).

## Operations

- `SetProps { target, props: key → Option<Value> }`: each key set, or removed (`None`).
  Refused: a key not well formed (`BadPropKey`), a value not well formed or not as its
  definition says (`BadPropValue`). Changes in a row to the same keys of one entity undo in one
  go (typing a value).
- `SetPropDef { key, def: Option<PropDef> }`: defines a key, or removes its definition — its
  values stay, as they are. Refused: a definition not well formed (`BadPropDef`), or one the
  values already there do not fit (`PropDefContradicted`): change them first.

## Pastes and imports

What comes from another document brings its definitions (the clipboard's fragments carry all
of the source's, `Document::entities_fragment`, `BodyFragment::prop_defs`).
`Document::adopt_prop_defs` takes them in: a key not defined here takes its definition; one
defined the same way stays; one **defined otherwise**, or a value without a definition that the
definition here does not accept, is **renamed** — `key_2`, `key_3`… — in everything pasted, its
definition under the new key. The operation then warns (`Warning::PropsRenamed`, old → new).
Pasted into the document copied from, nothing changes.

## In the file

Each entity's `props` in `document.json`, the definitions in `prop_defs`
([File format](../io/file-format.md)). Opening checks them (`Document::check_props`): keys
and values well formed, every value fitting its definition — else the file is refused
(`FileError::BadProp`, its key).

### Why

- **On the entities, not in a table of the document**: copies, make unique, pastes and
  stretches carry them as they carry names, with no operation to keep a table in step.
- **The kind with each value, the schema optional** (asked on 2026-11-19): SketchUp's
  attributes are untyped Ruby values, no unit, no schema, out of the interface; here a length
  is a length, shown in the model's units, whether a definition says so or not. A definition
  adds what a form needs — a label, choices, where it applies — and checks.
- **What Skraft's code reads stays typed fields** (a material's kind, thicknesses, grain, its
  texture, to come): properties are for the user, imports and extensions, not
  `get("ocl.thickness")` everywhere.

---
status: built
code: [scripts/sketchup/export.rb, crates/model/src/import.rs, crates/kernel/src/polyhedron.rs, apps/import/src/main.rs]
---

# SketchUp import

A SketchUp model brought into Skraft with its structure one to one (definitions, instances,
transformations), to compare loading and display with SketchUp on real models. A developer's
path: an export script run inside SketchUp, then a command-line converter.

Decision record: [Import SketchUp models through a JSON export](../decisions/0012-sketchup-import-through-json-export.md).

## Export inside SketchUp

`scripts/sketchup/export.rb` (run in the Ruby console or through the Claude Bridge) writes JSON
`skraft-sketchup-export` v1: per definition its visible faces (shared vertices in mm, loops by
index, outer first, normal) and instances (definition, name, 4×4 transform with its translation
in mm, material), the model's own entities as `root`. Hidden entities and hidden tags left out.

## Conversion

`Document::from_sketchup` (`crates/model/src/import.rs`): a definition's faces make a part, its
instances an assembly; both: an assembly whose first occurrence is the part of its faces
(« Name (faces) »). An instance's transformation is split into the rigid T of its occurrence and
a scale (or shear) **baked into the geometry**, the child definition taken at that scale: one
component per (definition, scale), [Data model](../model/data-model.md). `no_scale_mask` becomes
the behavior's `no_stretch` (same bits, [Stretch](../tools/stretch.md)); groups' definitions are
not `named`.

## Solids

`Solid::from_polyhedron` (`crates/kernel/src/polyhedron.rs`): the faces become the STEP entities
of a B-rep, in memory (a plane per face, a line per edge shared by its two faces, one closed
shell per set of joined faces: a `MANIFOLD_SOLID_BREP`, or a `SHELL_BASED_SURFACE_MODEL` for
several lumps), read by SolidCore's STEP topology reader (`solidcore_step::topology::read_solid`)
**from their topology only**. Loops are turned to agree with the normals, then the faces of each
lump turned alike across their edges (SketchUp keeps reversed faces in solids), the lump turned
inside out if its volume is negative. Built in parallel, one request per (definition, scale).

**Only closed solids are kept**, checked before the kernel by counting (each edge between
exactly two faces): a definition open, non-manifold or that the reader refuses is excluded and
reported (its instances dropped). **Why:** falling back to stitching by geometry on the open
ones is slow — measured on a 1373-definition apartment (52 k faces), 100 s, up to 60 s for one
definition of 2668 faces, with SolidCore's former `step-brep`; the count rejects them at once,
1 s for the whole.

## Scope

- **Curves stay facets**: a hole of 24 segments is 24 planar faces, as SketchUp draws it (a
  comparison at equal geometry).
- **Not carried**: face materials, tags, hidden geometry, attributes (OCL's included).
- **Entry point**: `skraft-import export.json [out.skraft]` (`apps/import`), which prints counts
  and timings; the file then opens in the app like any other. The same tool converts a STEP
  file (`.step`, `.stp`) to a `.skraft` ([STEP import](step-import.md)). Not yet: an import from
  the app's menu.

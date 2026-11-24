---
status: built
code: [crates/kernel/src/sketchup.rs, crates/kernel/src/polyhedron.rs, crates/model/src/import.rs, crates/tools/src/editor/document.rs, apps/skraft/src/system/file.rs, apps/import/src/main.rs]
---

# SketchUp import

**File › Importer…** (`file.import`, Cmd+I) places a SketchUp file (`.skp`, any release from 3
to 2027) **in the model**, as a [STEP import](step-import.md) does: in the active context, as one
occurrence, selected, one step to undo. Its structure comes one to one: definitions shared by
their instances, transformations, the materials painted on instances and faces, with their
textures.

Decision record: [Read SketchUp files directly with skpwright, in the kernel](../decisions/0043-sketchup-files-read-with-skpwright.md).

## Reading

**Read by the kernel**, as a job (`Task::ImportSketchUp { bytes }` → `Output::SketchUp`,
`crates/kernel/src/sketchup.rs`), off the UI thread natively and in the worker on the web, by
[skpwright](https://github.com/hew3d/skpwright) (a clean-room `.skp` reader: the 2017-style
stream container of releases 3 to 2020, the ZIP container of 2021 and later). From its model:

- a definition's entities are its geometry run (`GeometryRun::def_index`, linked to the
  definition by `Model::definition_links`); the model's own, the run of no definition and the
  instances outside every run;
- the visible faces (not hidden, their tag visible) with their loops, in mm from the file's own
  inches (`Mesh::vertices_in`: metres lose the last bits), and their normal;
- the visible instances: their definition, their 13-value transform (a 3×3 by rows, the
  translation in inches, a homogeneous weight dividing it all), their material, their name (a
  definition's instances found by persistent id in `Model::instances`);
- a definition's `Behaviour::extra` is SketchUp's `no_scale_mask` (checked against SketchUp on
  every bit), which becomes the component's `no_stretch` ([Stretch](../tools/stretch.md));
  groups' definitions are not `named`; image definitions are left out.

## Scales

An instance's transformation is split into the rigid T of its occurrence (rotation, maybe a
mirror, and the translation) and a scale (or a shear) **baked into the geometry**, the child
definition taken at that scale: one component per (definition, scale), [Data
model](../model/data-model.md). The kernel walks this plan from the model's entities down and
numbers each node after those it places.

## Solids

`Solid::from_polyhedron` (`crates/kernel/src/polyhedron.rs`), for each node with faces, in
parallel (`parallel_map`, reporting `Progress::Bodies`): the faces become the STEP entities of a
B-rep, in memory (a plane per face, a line per edge shared by its two faces, one closed shell
per set of joined faces: a `MANIFOLD_SOLID_BREP`, or a `SHELL_BASED_SURFACE_MODEL` for several
lumps), read by SolidCore's STEP topology reader (`solidcore_step::topology::read_solid`)
**from their topology only**. An edge drawn on a face without splitting it is taken out
first: SketchUp runs the face's loop along it and back, ending inside the face (a spur: dropped)
or joining the border to a hole (a bridge: the loop parted into the border and the hole, the
largest loop the outer one). Loops are turned to agree with the normals, then the faces of each
lump turned alike across their edges (SketchUp keeps reversed faces in solids), the lump turned
inside out if its volume is negative.

**Only closed solids are kept**, checked before the kernel by counting (each edge between
exactly two faces): a definition open, non-manifold or that the reader refuses is excluded and
reported (its instances dropped). **Why:** falling back to stitching by geometry on the open
ones is slow — measured on a 1373-definition apartment (52 k faces), 100 s, up to 60 s for one
definition of 2668 faces, with SolidCore's former `step-brep`; the count rejects them at once.

## Model

`Document::from_sketchup` (`crates/model/src/import.rs`): a node's faces make a part, its
instances an assembly; both: an assembly whose first occurrence is the part of its faces
(« Name (faces) »). The fragment's root, named after the file, holds the model's own faces (a
part) and its top instances; [`Operation::Insert`](step-import.md#insertion) places it.

## Materials

A material used by an instance or a face becomes a document material: its name, its colour (a
texture's average colour), and its **texture**, the image (PNG or JPEG, as the file holds it)
among the document's assets laid at the real size SketchUp gives it
([Textures](../render/look.md#textures): a box projection in the part's axes, not SketchUp's
per-face placement). An instance's material goes on its occurrence. A **face's** goes on its
part (`Part::faces`, by the face's index in the solid's record): the material of its side facing
out — a face SketchUp holds reversed shows its back —, none when only its inner side is
painted (unseen in SketchUp too). The kernel finds each face again in the built solid by its
vertices (`painted` in `crates/kernel/src/sketchup.rs`).

## In the app

As a STEP import: the loading window while the kernel reads (« Lecture du fichier », then the
solids built, « Lecture des corps »), cancellable; a toast with the parts imported, then a
warning toast with the count of definitions excluded. `skraft-import model.skp [out.skraft]`
(`apps/import`) does the same from the command line, into an empty model, and prints counts,
timings (reading, building, the slowest builds) and what was left out.

Measured on the apartment above (24 MB, SketchUp 2026): read 0.5 s, solids 2.6 s on all
cores, 1527 placed parts.

## Scope

- **Curves stay facets**: a hole of 24 segments is 24 planar faces, as SketchUp draws it,
  though the file gives the arcs (`Curve::arc`).
- **Not carried**: tags, hidden geometry, attributes (OCL's included), edges alone, images,
  texts, dimensions, a texture's placement on each face (Skraft projects it on the part's box).

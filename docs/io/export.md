---
status: built
code: [crates/kernel/src/export.rs, crates/kernel/src/step_export.rs, crates/kernel/src/skp_export.rs, crates/kernel/src/job.rs, crates/model/src/export.rs, crates/model/src/paint.rs, crates/tools/src/editor/document.rs, apps/skraft/src/workspaces/common/dialogs/export.rs, apps/skraft/src/app/dialogs.rs, apps/skraft/src/app/frame.rs]
---

# Export

**Fichier › Exporter…** (`file.export`, Cmd+E) writes the whole model or the selection as a
**STEP** file (AP242), exact — what a CAD program reads back as the solids Skraft computed —,
or as a **SketchUp** file (`.skp`), as faces. Decision record:
[Exporters](../decisions/0059-exporters-step-exact-skp-meshed.md).

## What goes out

`Document::export` (`crates/model/src/export.rs`) gathers the products of the whole model, or
of occurrence paths, the same for both formats (`ExportProduct`, `crates/kernel/src/export.rs`):

- **The whole model**: the root, the top product, named after the document, in the model's axes.
- **The selection** (`Editor::export_selection`): the parts and assemblies selected at any depth
  (« Pièces »), else the occurrences selected in the context; bodies selected in a part open
  export that part, whole. An occurrence under another selected one goes with it (`outermost`).
  **One component goes out from its own origin**, itself the top product, named after it;
  **several from their common parent's** (the deepest assembly holding them all), each placed
  as it is in it, under a top product named after that parent (the document's when it is the
  model).
- **One product per component**, written once and placed by each of its occurrences, with the
  occurrence's name.
- **A part is one body: its final shape** (`Part::shape`, machinings cut, what the cut list
  measures), named after the part. An empty part is a product without a body.
- **Colours as seen** (`Document::seen_paint`): what is under (the material, or a face's
  covering; a texture's own colour) with the finish's coats composed over it as the viewport
  lays them (`compose`, `crates/model/src/paint.rs`); a finish over nothing is laid over the
  light theme's face. Named after what is under and the finish (« Chêne + Vernis »). The
  part's colour is its body's; each face that shows something else has its own. Nothing under
  and no finish: no colour.
- **Left out**: hidden occurrences and what is under them; guides, recipes and profiles,
  properties, the drawing. Not yet: material designations and a bought component's source
  (STEP), OpenCutList's attributes (SketchUp).

## STEP

The kernel writes it (`Task::ExportStep { export }` → `Output::StepFile`), off the UI thread
natively, in the worker on the web, whose UI does not link the writer. The records are the
job's inputs (a shape shared by several parts crosses once).

- **Products and placements**: `solidcore_step::write::product::products` writes the products
  (`NEXT_ASSEMBLY_USAGE_OCCURRENCE`s under the top product), their placements and colours
  (`STYLED_ITEM`s, a face's on the face). The [STEP import](step-import.md) reads the same
  structure back.
- **Mirrors**: STEP places rigidly. An occurrence whose placement mirrors places its
  component's **mirrored twin** (`MIRROR`, x → −x in the component's axes), the mirror moved
  out of the placement; an assembly's twin places its children through the mirror, so a child
  mirrored twice is the plain product again. A twin keeps its component's name; the kernel
  mirrors its records, each once.
- **Exact**: each body's record goes through `write::topology`: faces on their exact surfaces
  with their pcurves, solids with voids. A cone of no angle (a round's) is written as the
  cylinder it is (`step_ready`): the writer would write a `CONICAL_SURFACE` of semi-angle 0,
  which STEP forbids. A body it cannot state exactly fails the export, with its name: no
  faceted fallback.
- Units are millimetres; the header names the file and `Skraft <version>`, no time stamp.

## SketchUp

The kernel writes it (`Task::ExportSkp { export }` → `Output::SkpFile { bytes, notes }`) with
skpwright (`crates/kernel/src/skp_export.rs`), the same way.

- **Components**: a definition per product (a name SketchUp already holds numbered `#2`), placed
  by instances (`PlacedInstance`, its transform `transform13`: the 3×3 by rows, the
  translation in inches). The top product without a body of its own (the model, a common
  parent) leaves its children at the model's top; a part exported alone stands there itself.
  **Mirrors stay mirrors**: SketchUp places them as they are, no twin.
- **Faces**: each part's final shape meshed (`solidcore_tess`, at the facets chosen), the
  facets' corners welded (to a nanometre). A **planar face is one SketchUp face**: its
  triangles' boundary chained into loops, the widest the outer one, the others its holes, all
  turning about the face's normal. A **curved face is its facets**, the edges between two of
  them **soft and smooth** (SketchUp draws them as one surface); the edges between two faces
  stay drawn.
- **Facets** (`Facets`): « Normales » as Skraft displays them (`MeshTolerance::display_of`),
  « Grossières » a chord four times larger and facets turning twice as much, « Fines » a
  quarter of the chord and half the turn.
- **Paint**: a SketchUp material per colour (`Material::Solid`, named after it). A **part's
  colour on its instances**, as OpenCutList reads a part's material; a face with its own on
  the face.
- **Release** (`SkpRelease`): **2017** (`Model::to_release(Release::R2017)`), opened by
  SketchUp Make 2017 and every later release, or **2026** (`Model::to_archive`), the current
  container, read from SketchUp 2021. What the release cannot hold comes back as notes: a
  warning toast lists them.

*Why* faces built from the B-rep's faces rather than all triangles: a SketchUp user pushes and
pulls faces, and a box exported as twelve triangles is no box.

## In the app

The export box (`workspaces/common/dialogs/export.rs`) chooses the **format** — STEP, or SKP
with its **Version** (2017, 2026) and its **Facettes** (Grossières, Normales, Fines) — and the
**content** — « Modèle entier » or « Sélection », the selection chosen when there is one,
greyed when there is none — and says how many parts go out (each occurrence counted,
`ExportPlan::parts`, counted when the box opens); with no visible solid, « Exporter » is greyed.
The format and its options hold for the session (`Choice`).

While the kernel writes, the status bar says « Écriture de … ». The file comes back to the app
(`Editor::take_exported`), which asks where to save it (`file::export_file`, the top product's
name and the format's extension offered; a download on the web); a toast says it is written,
with « Ouvrir ».

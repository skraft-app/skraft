---
status: built
code: [crates/kernel/src/step_export.rs, crates/kernel/src/job.rs, crates/model/src/step_export.rs, crates/model/src/paint.rs, crates/tools/src/editor/document.rs, apps/skraft/src/app/command_run.rs, apps/skraft/src/app/frame.rs]
---

# STEP export

**Fichier › Exporter…** (`file.export`, Cmd+E) writes the whole model or the selection as a
STEP file (AP242), exact: what a CAD program reads back as the solids Skraft computed, not facets.
Decision record: [Exporters](../decisions/0059-exporters-step-exact-skp-meshed.md).

## What goes out

`Document::step_export` (`crates/model/src/step_export.rs`) gathers the products of the whole
model, or of occurrence paths:

- **The whole model**: the root, the top product, named after the document, in the model's axes.
- **The selection** (`Editor::export_selection`): the parts and assemblies selected at any depth
  (« Pièces »), else the occurrences selected in the context; bodies selected in a part open
  export that part, whole. An occurrence under another selected one goes with it (`outermost`).
  **One component goes out from its own origin**, itself the top product, named after it;
  **several from their common parent's** (the deepest assembly holding them all), each placed
  as it is in it, under a top product named after that parent (the document's when it is the
  model).

- **One product per component**, written once and placed by each of its occurrences
  (`NEXT_ASSEMBLY_USAGE_OCCURRENCE`s under the top product, named after the document); the
  occurrence's name is the placement's. The [STEP import](step-import.md) reads the same
  structure back.
- **A part is one solid: its final shape** (`Part::shape`, machinings cut, what the cut list
  measures), named after the part. An empty part is a product without a body.
- **Mirrors**: STEP places rigidly. An occurrence whose placement mirrors places its
  component's **mirrored twin** (`MIRROR`, x → −x in the component's axes), the mirror moved
  out of the placement; an assembly's twin places its children through the mirror, so a child
  mirrored twice is the plain product again. A twin keeps its component's name.
- **Colours as seen** (`Document::seen_srgb`): what is under (the material, or a face's
  covering; a texture's own colour) with the finish's coats composed over it as the viewport
  lays them (`compose`, `crates/model/src/paint.rs`); a finish over nothing is laid over the
  light theme's face. The part's colour is its body's, each face that shows something else a
  face colour (`STYLED_ITEM` on the face). Nothing under and no finish: no colour.
- **Left out**: hidden occurrences and what is under them; guides, recipes and profiles (STEP
  holds no history), properties, the drawing. Not yet: material designations and a bought
  component's source, which the import reads.

## Writing

The kernel writes it, as a job (`Task::ExportStep { export }` → `Output::StepFile`): off the
UI thread natively, in the worker on the web, whose UI does not link the writer. The records
are the job's inputs (a shape shared by several parts crosses once); a twin's records are
mirrored by the kernel, each once. `solidcore_step::write::product::products` writes the
products, their placements and colours; each body's record goes through
`write::topology` **exactly**: faces on their exact surfaces with their pcurves, solids with
voids. A cone of no angle (a round's) is written as the cylinder it is (`step_ready`): the
writer would write a `CONICAL_SURFACE` of semi-angle 0, which STEP forbids. A body it cannot
state exactly fails the export, with its name: no faceted fallback.
Units are millimetres; the header names the file and `Skraft <version>`, no time stamp.

## In the app

The export box (`workspaces/common/dialogs/export.rs`) chooses the **format** — STEP; SKP
greyed, to come — and the **content** — « Modèle entier » or « Sélection », the selection chosen
when there is one, greyed when there is none — and says how many parts go out (each occurrence
counted, `StepExportPlan::parts`, counted when the box opens); with no visible solid,
« Exporter » is greyed. Then the kernel writes the file and the app asks where to save it.

While the kernel writes, the status bar says « Écriture de … ». The file comes back to the app
(`Editor::take_exported`), which asks where to save it (`file::export_file`, the top product's
name with `.step` offered; a download on the web); a toast says it is written, with
« Ouvrir ». 

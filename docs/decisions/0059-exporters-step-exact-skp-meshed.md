# Exporters: STEP exact, by the kernel; SketchUp later, as meshes

- Status: accepted (2026-11-28; STEP and SketchUp built)
- Design: [Export](../io/export.md)

## Context

A model leaves Skraft only as a `.skraft` file or a drawing's PDF: nothing a CAD program, a
CNC workflow or SketchUp reads. Both libraries the imports use also write:

- **SolidCore's `solidcore-step`** (already linked by the kernel, `engine` feature) has an
  AP242 writer, `write::product`: products, assemblies placed by rigid transformations,
  named solids, body and face colours, and each body's **record written exactly**
  (`write::topology`: faces on their exact surfaces, their pcurves, solids with voids). What
  STEP cannot state exactly is refused, never approximated.
- **skpwright** (already linked, [0043](0043-sketchup-files-read-with-skpwright.md)) writes
  `.skp` files: a 2026 file from a `Model` (`Document::from_model`), or one of releases 3
  to 2020 (`StreamDocument`), with attribute dictionaries. SketchUp only knows polygons.

## Decision

**Two exporters, STEP first.** SketchUp export comes after, its own step: it loses the exact
geometry (curved faces as facets, smoothed edges) and needs a check that SketchUp opens what
skpwright writes.

**An export box** (**Fichier › Exporter…**, `file.export`): the format (STEP; SKP with its
release and facets), the content — the whole model or the selection —, the parts that go
out; then the system's save panel. **Why** a box rather than a command per format and content:
the SketchUp export will have options, and a selection exported by surprise (a part left
selected) would make a file missing most of the model.

**The selection goes out from its origin**: one component from its own axes, as the top
product; several from their common parent's (the deepest assembly holding them), each placed as
in it. Bodies selected in an open part export the part, whole.

**STEP, exact**:

- **Written by the kernel**, as a job (`Task::ExportStep` → `Output::StepFile`): off the UI
  thread natively, in the kernel's worker on the web. The model gathers the job
  (`Document::step_export`): a product per component, the records as the job's inputs.
- **One product per component, shared**: a part or an assembly is written once and placed by
  each occurrence, as the import reads it back. The root is the top product, named after the
  document.
- **A part is one body: its final shape** (`Part::shape`, what the cut list measures), not its
  bodies — a receiver gets the part as it is made, its machinings cut.
- **A mirrored occurrence places a mirrored twin** of its component (the same name): STEP
  placements are rigid. The kernel mirrors the twin's records; an assembly's twin places its
  children's twins, so only components under an odd number of mirrors have one.
- **Colours as seen**: a part's colour is its material's under its finish's coats, composed
  as the viewport does; a face painted on its own (a covering, a finish) its own colour.
  Nothing under and no finish: no colour, the receiver's default.
- **Hidden occurrences are left out**, with what is under them: the file holds what is seen.
- **Refused as a whole**: a body the writer refuses fails the export, naming the part; no
  faceted fallback.

**SketchUp, as faces** (skpwright): a definition per component, placed by instances (mirrors
kept: SketchUp places them); each part's final shape meshed, a planar face one SketchUp face
with its holes, a curved face its facets, softened and smoothed between them. The box chooses
the **release** — 2017 by default, opened by SketchUp Make 2017 and every later release, or
2026 — and **how fine the facets** are (coarse, as displayed, fine). A part's colour goes on
its instances, as OpenCutList reads it; a face painted on its own on the face. A texture goes
out with its material, its image embedded at its real size, laid by SketchUp: no placement per
face, the materials staying on the instances.

## Alternatives considered

- **Writing in the model crate, on the UI thread**: the web UI would link `solidcore-step`
  (it does not link the kernel), a large model would freeze the interface, and mirroring a
  record needs the kernel anyway.
- **A part's bodies as its solids** (positives and negatives): a receiver would get blanks and
  the tools apart, nothing like the part.
- **A command per format and content** (« Exporter la sélection en STEP »): entries doubling
  with each format and option, nowhere to say what goes out.
- **The selection in world axes**: what is exported alone (a part for the machine) would come
  far from the origin.
- **A flat list of placed bodies** (`solidcore_ops::step_assembly` style): no shared products,
  a file as large as its copies, the tree lost.
- **A faceted body (`FACETED_BREP`) when the exact record is refused**: a file that looks
  right and is not; better to say which part did not go out.
- **All triangles in SketchUp**: a box as twelve triangles is no box to push and pull.
- **Only the 2026 release**: SketchUp Make 2017, free and still common in workshops, would not
  open the files.
- **SketchUp first**: more asked by woodworkers, but lossy and unverified; STEP is exact and
  only needs the model's structure mapped.

## Consequences

- Not written yet: material designations and a bought component's source (the import reads
  them, the writer has no entity for them), properties, guides, the drawing; a file per part;
  OpenCutList's attributes in a SketchUp file.
- A model with a mirrored part holds both hands in the file: two products of the same name.
- The time stamp of the header is left empty (no clock in the kernel's worker).

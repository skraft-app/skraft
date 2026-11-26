---
status: built
code: [crates/kernel/src/step.rs, crates/kernel/src/job.rs, crates/model/src/step.rs, crates/model/src/ops.rs, crates/tools/src/editor/document.rs, crates/tools/src/editor/loading.rs, apps/skraft/src/system/file.rs, apps/import/src/main.rs]
---

# STEP import

**File › Importer…** (`file.import`, Cmd+I) places a STEP file (AP203 / AP214 / AP242) **in the
model**, in the active context, as one occurrence, selected, one step to undo. The kernel reads
the file and its product structure; the model keeps the products shared as components. The same
command imports SketchUp files ([SketchUp import](sketchup-import.md)), told by their extension.

## Reading

**Read by the kernel**, as a job (`Task::ImportStep { bytes }` → `Output::Step`), so off the UI
thread natively and in the worker on the web; the file is parsed once
(`crates/kernel/src/step.rs`) by SolidCore's Part 21 parser (`solidcore_record::step::p21`:
short entity names, strings decoded). SolidCore reads the **product structure** as data
(`step::product::read`: products, representations, occurrences, maps, styles); it is walked
here into products and placed instances:

- `PRODUCT_DEFINITION`s and their shape representations (joined by relationships without a
  transformation: the same axes);
- assembly occurrences placed by their relationship (`ITEM_DEFINED_TRANSFORMATION`:
  `frame₂ · frame₁⁻¹`, the two representations told apart when a writer swaps them);
- `MAPPED_ITEM`s (`target · origin⁻¹`), each frame's matrix the kernel's
  (`solidcore::geom::step_matrix`);
- bodies no product reaches, and topology outside any representation (`topology::roots`), kept
  at the top.

**Why** not `solidcore_ops::step_assembly::placed_bodies`: it flattens the tree into world
placements, and the model keeps the products shared.

## Bodies

Read from their topology straight into records (`solidcore_step::topology::read_solid_with`, in
their representation's units), in parallel natively (`parallel_map`, with the kernel's stack),
then made the kernel's (a round trip). The pcurves the reader has no closed form for (a spline
edge on a spline face…) are the kernel's own trims (`KernelTrims`: the face alone sent with
those uses bare, `to_kernel_with_bare_uses`). **Only closed solids**: a surface model, an open
shell or a body the reader refuses is excluded and counted.

## Colours

A `STYLED_ITEM`'s surface colour (`COLOUR_RGB`, pre-defined colours), on the body, its shell, or
shared by all its faces, becomes a **coloration** (one per colour, « Couleur #RRGGBB ») of its
part. Not yet: other face colours (a face's index in the built B-rep would
have to be matched).

## Source and material

Read from the exchange beside the product structure, which carries neither
([decision 0057](../decisions/0057-materials-on-the-definition-typed-sources.md)):

- **Bought**: a definition whose formation is a
  `PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE` with `.BOUGHT.` gives a component
  « Acheté » — its part, or its assembly when it has several bodies or products: one row of the
  parts list, a vendor's slide whole. `.MADE.`, `.NOT_KNOWN.` or no source: « Fabriqué ».
- **Material**: a `MATERIAL_DESIGNATION` (its name; what it characterises — one definition in
  AP203, a set in AP214 and AP242 — a product definition, its `PRODUCT_DEFINITION_SHAPE`, its
  formation or its product) gives a **material** « Massif » of that name (one per name, a
  neutral grey: the file gives no colour) to the parts of the product's own bodies; a colour
  stays a coloration over it.

## Model

`Document::from_step` (`crates/model/src/step.rs`): a product with one body is a part; with several
bodies, or bodies and products, an assembly holding one part per body (a component made of several
solids — a cabinet of boards, a frame of tubes — is several parts); a product of products an
assembly. Names come from the products (then the bodies), all `named`.

## Insertion

`Operation::Insert { context, fragment, transform }`: a whole document as the operation's data;
its components and materials copied with fresh ids (a material equal to one already there is
reused), names made unique. The fragment's single top occurrence is placed itself; several are
gathered in an assembly named after the file. The import inserts it at the context's origin
(`transform` the identity), selected, then zooms to the model's extents.

## In the app

- While the kernel reads, the status bar says « Lecture de … » and the loading window shows the
  import ([Loading a model](../ui/loading.md)); the rest of the editor stays usable. Cancelling
  it (« Annuler » or Esc) cancels the kernel's job and undoes the insertion if it was made.
- The result is a toast: the parts imported; then, when some were left out, a warning toast of
  its own with their count (« N corps non solides ignorés », only closed solids imported) —
  under the success it went unseen. A file with no solid, or one the kernel could not read, an
  error toast.
- `skraft-import file.step [out.skraft]` (`apps/import`) does the same from the command line,
  into an empty model, as it does a `.skp` ([SketchUp import](sketchup-import.md)).

Not yet: placing the import at the cursor (it lands at the context's origin).

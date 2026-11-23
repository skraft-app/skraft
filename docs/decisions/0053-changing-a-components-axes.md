# Changing a component's axes: one operation, dimensions read on the axes

- Status: accepted (2026-11-23; not built)
- Design: [A part's axes](../model/data-model.md#a-parts-axes),
  [Bodies placed in the part's axes](../model/bodies.md#bodies-placed-in-the-parts-axes)
- Interaction after OpenCutList's SmartAxes (`ruby/tool/smart_axes_tool.rb`)

## Context

Decided on 2026-11-23 (questions / answers). A part's length is its main direction, the grain for
wood, but nothing can change a part's axes: a part drawn or imported keeps the ones it was born
with. The bodies model was shaped for this change: bodies are placed in the part's axes and their
profiles live in their own.

Two readings of the length disagree today. The design says it runs along the red axis; the parts
list and the control protocol sort the box, largest first, and a texture's grain follows the
mesh's longest axis (`Batch::grain`). With a sorted box, turning a part's axes changes nothing in
its cut list, and a grain cannot run along a board's width.

## Decision

### One operation

**Changing a component's axes by `A`** (a direct rigid transform, from the new axes to the old)
moves nothing in the world:

- each occurrence of the component: `T` becomes `T · A`, a mirrored one staying mirrored;
- a part's top-level bodies, or an assembly's occurrences: their placement `P` becomes `A⁻¹ · P`;
  definitions and profiles are not touched;
- the guides: by `A⁻¹`;
- a part's computed shape: transformed by `A⁻¹`, no boolean;
- what refers to the component's axes follows: dimension anchors' points, and the views below.

It applies to the component, so to all its occurrences. **An assembly changes its own axes only**:
the parts inside keep theirs, and so their dimensions.

### Dimensions read the axes

Red is the **length**, green the **width**, blue the **thickness**, unsorted, in the parts list,
the control protocol and for a texture's grain. No flag says whether a part is oriented: its axes
are the only statement.

### Parts are born oriented

The **drawing tools** (box, prism, cylinder, turned body) and the **imports** (STEP, `.skp`) orient
a new part: its longest dimension on red, its shortest on blue.

- Quarter turns only, never an oblique rotation. Equal dimensions keep their current order. The
  length and thickness keep a positive direction, the width's follows from a direct frame.
- A `.skp` part whose axes OpenCutList locked (`orientation_locked_on_axis`) keeps them.
- Copies, pastes and boolean results keep their axes. So does a part stretched past another of its
  dimensions: orientation is settled once, at birth.

### Drawing views keep their image

A view of the component in its own axes (`Subject::Component`) has its direction, and a
perspective's eye, brought into the new axes, and its place on the sheet corrected: **the sheet
does not change**. It stays a standard view when the new direction and up are one, and otherwise
becomes a camera view. Views of the model or of occurrences stand in the world: nothing to do.

### Stretch settings

On a **quarter turn**, the locked axes and cutters are permuted with the axes, a cutter's ratio `r`
becoming `1 − r` on a reversed axis. On an **oblique** rotation they no longer mean anything: they
return to the default and a message says so. Undo restores them.

### Two tools

- **« Réorienter »**, whose modes never move the geometry, Tab going to the next, as SmartAxes:
  - *Permuter Longueur ↔ Largeur*;
  - *Permuter Face ↔ Dos*;
  - *Adapter les axes*: a face gives blue, an edge in it gives red (its longest outer edge by
    default), the modifier key reversing it; the origin stays. A slanted face gives oblique axes;
  - *Déplacer l'origine*: a point picked becomes the origin.
- **« Retourner »**, along the length, width or thickness: **mirrors one occurrence** about its
  box's centre. It changes that occurrence's `T` and not the component's axes.

## Alternatives considered

- **Sorted by default, a lock set by the tool** (OpenCutList's `auto_orient`): existing parts would
  not change, but every reader would have two cases, and a part's length could swap axes when
  stretched.
- **Always the axes, without orienting at birth**: the simplest, but a board drawn standing reads
  18 × 400 × 600 until it is reoriented.
- **Always sorted**: no way to state a grain along the smaller dimension.
- **Orienting every new part**, boolean results and pastes included: an operation would change axes
  the user expects to stay.
- **Views following the new axes**: « Dessus » stays « Dessus » but shows another face, and sheets
  change through an edit made elsewhere.
- **Quarter turns only**: safer for the kernel, but the true dimensions of a part lying askew could
  not be read.
- **An assembly's rotation applied to its parts**: it would change the cut list of parts shared
  with other assemblies.
- **« Retourner » as a mode of « Réorienter »**, as in SmartAxes: one button, but the tool could no
  longer promise that the geometry stays where it is.

## Consequences

- **A reoriented part is no longer plain** (`Part::is_plain`: a single positive body, unplaced). It
  stores a computed shape, a cost to measure on parts born oriented.
- **Dimension anchors rely on plain parts' face names** (`part_names`). These names must also hold
  for a single positive body that is placed, or a reoriented part's dimensions stop following a
  stretch or become orphans. This needs a transform to keep the record's face order, which the
  face materials (`Part::faces`, by index) need too; to be tested first.
- **SolidCore's plane axes** can come out off square after a slanted rotation (`square_frames`,
  upstream #3265): oblique axes are to be tested through a read-back and a boolean before relying
  on them.
- **Existing files are not migrated** (format v1, [0010](0010-file-format-v1-until-release.md)):
  their parts read in the order of their axes until reoriented. The changelog entry of the build
  says so.
- Moving the origin changes where the library holds the component, as expected of its axes.

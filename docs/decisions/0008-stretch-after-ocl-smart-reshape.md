# The Stretch tool is OpenCutList's Smart Reshape stretch

- Status: accepted (2026-11-11)
- Design: [Stretch](../tools/stretch.md)

## Context

Parts are cut-list parts: lengthening one must keep its details' sizes (a rail's tenons, a
bracket's holes), its grooves must follow.
A reshaping tool was needed; push/pull was the other candidate.

## Decision

- Port OpenCutList's Smart Reshape stretch (`tool/smart_reshape_tool.rb`,
  `SmartReshapeStretchActionHandler`; the analysis in
  `worker/common/common_stretch_split_worker.rb` and `model/stretch/`, the application in
  `common_stretch_apply_worker.rb`): a grip on each face of the box, cutters splitting the
  target into sections, each section translated by its share of the move, the matter across a
  cutter lengthened or shortened.
- It comes **before push/pull**: more useful for parts made to size, and safer on the kernel side.
- The tool was rebuilt the same day on OCL's own interaction (its states, `_snap_*` and
  `_preview_*`), then extended to OCL's interior handles, to assemblies and to several
  occurrences of one context (OCL's `active_selection_path` + `active_selection_instances`,
  `root_split`, `_get_edit_transformation`), all on 2026-11-11.
- On 2026-11-12, after OCL, a component stretched in a walk has its origin follow its anchor
  (the section holding the end of its extent nearest its origin); before, there was no anchor.

## Alternatives considered

- Push/pull first: left for later.

## Consequences

- The stretch works on the recipe (bodies, components), the B-rep record only rewritten for a
  solid across a cutter ([Stretch rewrites the B-rep record](0009-stretch-rewrites-brep-record.md)).
- Cutters became a property of the component
  ([Cutters and locks as a typed Behavior](0013-stretch-behavior-on-component.md)).
- Where the kernel cannot follow OCL (a part turned in the stretch's axes across a cutter has
  no face along the axis), the stretch is refused; OCL deforms any geometry.

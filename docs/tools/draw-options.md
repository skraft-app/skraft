---
status: built
code: [crates/tools/src/tools/shapes.rs, crates/tools/src/tools/mod.rs, crates/tools/src/editor/mod.rs, crates/model/src/ops.rs]
---

# Drawing in a part's assembly, with its material

Two options of the part creation tools (Box, Cylinder, Prism, Turn), after OpenCutList's Smart
Draw tool: draw the new part into the assembly of a part aimed at, and give it that part's
material. Also the settings these tools share. After OCL's `tool/smart_draw_tool.rb`: options
`draw_in` and `pick_material`, `onPickerChanged` at `STATE_SHAPE_START`, `onActivePartChanged`,
`_create_entity`, `_get_picked_material`, `_refresh_pick_material_btn`, the container drawn by
`smart_tool.rb` `_preview_part`.

## Two toggles

Two toggles of the part creation tools' bar, kept with each tool's settings, off by default,
OCL's icons: « Dessiner dans » (`draw_in`, box corners round two squares) and « Pipette »
(`pick_material`, a pipette). Hidden in an open part, as « Nommage des pièces »: a body made there
is no new part.

## The part picked

While either is on, before the first click, the part under the cursor — at any depth under the
active context, whatever the selection scope — is picked at each hover; the first click keeps the
last one (none over the void). Its own snapping does not change: the part is the one whose face is
under the cursor.

## Draw in

« Dessiner dans »: the part made goes into the assembly holding the picked part, when it is not
the active context. That assembly is shown, as OCL, by its box: dark grey corners and a dotted box
(`Kuix::BoxCornersMotif3d`, `BoxMotif3d`): the edges of its faces turned to the eye drawn over the
scene (OCL's `shell_on_top`), the back ones in it, hidden behind faces. The new part is placed
there in that assembly's axes (`Operation::At`). When the assembly is placed more than once, the
« Portée » decides, as for parts moved at any depth ([Select first, and a selection
scope](selection.md)): every copy shows the new part, or (« seulement celle-ci ») the assemblies
along the path are first made unique.

## Pick material

« Pipette »: the part made takes the picked part's material — its occurrence's own, else the one
it inherits from its assemblies (as the view paints it); none, none. The toggle's button is filled
with that material's colour (its icon black or white to stay readable) and its tip ends with
« : <material name> ». `AddPrimitive` carries that `material` (the occurrence's).

## Shared settings

As OCL's `get_action_option_sync_actions`, a setting changed in one part creation tool is changed
in the others that have it (`shapes::synced_with`) — the offset and the centred height between
Box, Cylinder and Prism (Turn has neither); « Dessiner dans », « Pipette » and « Nommage des
pièces » ([Naming what was just made](quick-tools.md#naming-what-was-just-made)) between all four.
The others stay each tool's own (the box's « Centré », the fillet, the prism's « Inversé »).

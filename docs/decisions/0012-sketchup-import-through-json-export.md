# Import SketchUp models through a JSON export of their structure

- Status: accepted (2026-11-12)
- Design: [SketchUp import](../io/sketchup-import.md)

## Context

Decided and built on 2026-11-12, to compare loading and display with SketchUp on real models.
A performance comparison needs the model's structure one to one (definitions shared by their
instances), at equal geometry.

## Decision

A Ruby script run inside SketchUp (`scripts/sketchup/export.rb`) writes the definitions' visible
faces and their instances as JSON (`skraft-sketchup-export` v1); `Document::from_sketchup` turns
each definition into a part and / or an assembly, one component per (definition, scale), and
each closed lump of faces into a B-rep read by SolidCore's STEP topology reader from its
topology only. Curves stay facets. Only closed solids are kept, checked by counting before the
kernel.

## Alternatives considered

- STEP: keeps exact geometry, and SolidCore reads it, but does not apply assembly instancing.
- Mesh formats (OBJ, STL, glTF): lose faces or structure.
- Letting the reader fall back to stitching by geometry on open definitions: measured on a
  1373-definition apartment (52 k faces), with SolidCore's former `step-brep`, 100 s (up to 60 s
  for one definition of 2668 faces); the count rejects them at once, 1 s for the whole.

## Consequences

- Open, non-manifold or refused definitions are excluded and reported, their instances dropped.
- Face materials, tags, hidden geometry and attributes are not carried.
- The entry point is a command-line tool (`skraft-import`), not the app's menu.

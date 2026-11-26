# Materials: a part's material on its definition, a type saying where a material goes

- Status: accepted (2026-11-26)
- Design: [Palette and materials](../render/look.md#palette-and-materials),
  [Parts list](../ui/parts-list.md), [Data model](../model/data-model.md),
  [STEP import](../io/step-import.md), [SketchUp import](../io/sketchup-import.md)
- Amends [0005](0005-parts-assemblies-occurrences.md): the part's type comes from the material
  of its definition, no longer of its occurrence.

## Context

A material went on an occurrence (as in SketchUp and OpenCutList, whose definitions hold only
geometry), an assembly's occurrence painting the parts below it that had none, and on single
faces of a part (`Part.faces`). Painting an assembly was handy but matched nothing real: what a
part is made of is true of the part, not of one of its copies, and two copies in two materials
are two parts (two cut list rows, two costs). Painting a part, on the other hand, covers its
surface. Hardware (a drawer slide: an assembly of a rail, a ball cage and a runner, in several
colours) has to be one row of the parts list, and the room's walls or an existing fridge, drawn
to work around them, none. Decided on 2026-11-26 (questions / answers).

STEP draws the same lines: a material designation belongs to the product's definition
(`MATERIAL_DESIGNATION`), a colour to the geometry (`STYLED_ITEM` on a solid, a shell or a face),
and a product is made or bought (`PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE`:
`.MADE.`, `.BOUGHT.`, `.NOT_KNOWN.`), whatever its own structure.

## Decision

- **One catalog of materials, each with a type** (`Material.kind`) saying where it goes:
  - **substance** (what a part is made of: solid wood, sheet good, metal…): on a part's
    definition;
  - **surface** (paint, varnish, edge banding, veneer): on faces;
  - **untyped** (a colour, « Couleur #RRGGBB » of an import): anywhere, for the look only.
  The substance's own kinds (OpenCutList's solid wood, sheet good, dimensional…) come later,
  under it.
- **A part's material is on its definition** (`Part.material`), as substance or untyped.
  **Only parts have one**: an assembly never does, whatever its source; painting it lays the
  material on the parts below it. **No material on occurrences, and no inheritance through
  assemblies.** Display: face > the part's definition > the theme's default.
- **A component has a source** (`Component.source`), a field of the model like its name, not a
  property ([Properties](../model/properties.md)): it cannot be removed or renamed.
  « Fabriqué » (`Made`, the default), « Acheté » (`Bought`), « Existant » (`Existing`).
- **The parts list** walks the tree from the model: a `Made` part is a row per definition, a
  `Made` assembly is gone through; a `Bought` component (a part or an assembly) is **one row,
  not gone through**; an `Existing` component is **no row, not gone through**. A material counts
  only on a part the list shows; inside an article or an existing component, materials are
  only seen. An article assembly's row has no material (« — »): what identifies it (a
  reference, a supplier, a price) comes later. This replaces the property « exclusion from the
  cut list » once planned on occurrences.
- **The paint tool takes its target from the material's type**; the option « L'exemplaire /
  La face » goes:
  - a substance paints the definition of what is clicked, with the scope (« Toutes » /
    « Celle-ci », the second making the part unique); an assembly paints the definitions of the
    parts below it, at any depth, an article's or an existing component's included;
  - a surface paints the face clicked (on every occurrence of its part);
  - an untyped material paints the definition, or the face through the tool's option, kept
    for it only;
  - **refused**: a substance on a face (that is a veneer, a surface), a surface on a definition
    (a finish covering a whole part, later).
- **A material's type may change while used**: only its type changes. What it now cannot be on
  stays, **flagged** (⚠ in the parts list, the navigator, the entity info and on the material in
  the « Matières » card, a click there selecting those uses), for the user to repaint, unpaint
  or set the type back. Nothing is unpainted nor moved for them: the app does not guess at an
  edit, as when a stretch takes a round's edges away and the node shows ⚠.
- **Faces are painted one by one** for now: their index in the record stays their name, with
  its known limit (a boolean renumbers them).
- **Imports**:
  - STEP: a product's source from `.BOUGHT.` (`.MADE.` and `.NOT_KNOWN.` give `Made`); a colour
    on a body or a shell becomes an untyped material on its part, a part's product's
    `MATERIAL_DESIGNATION` a substance; an option « Importer comme » (Fabriqué / Acheté /
    Existant) sets the imported root's source (a vendor's slide file: one article).
  - SketchUp: an instance's material goes on its definition, untyped; a definition painted
    differently by its instances is made unique, one definition per material; an assembly's
    instance's material goes on the parts below it that have none.
  - The same « Importer comme » option for a `.skp`.

## Alternatives considered

- **Keeping the material on the occurrence** (SketchUp's, OpenCutList's): a part's nature on
  one of its copies, a part in two rows of the list.
- **Two notions, a « material » and a separate « look »**: two catalogs to explain; the untyped
  material is the look.
- **The type deciding nothing, the tool's option choosing the target**: the user picks a mode
  where the material already says where it goes.
- **« Acheté » as a material type** (OpenCutList's *hardware*): a bought slide has a substance
  (zinc-plated steel) and is bought, two facts; and an assembly bought has no single material.
- **A material on an article assembly**, for its row: a slide is made of parts, each in its
  own material, not of one material; it would be shown nowhere, an exception to explain. An
  article is counted, not cut: it is told by its reference, not by a material.
- **Source as a typed property**: it could be removed or renamed, and the parts list reads it.
- **Layers / tags for the environment** (SketchUp's): a display notion deciding what is made; a
  part left in the wrong layer would leave the list unnoticed. Layers may come later to hide the
  environment at once.
- **A material's type change**: refused while used (unpainting everything by hand to fix a
  typo), unpainting what no longer fits (destructive, past what was edited), or moving it (a
  substance turned surface laid on every face of its parts: a guess).
- **A finish on the definition now**, covering all faces present and future: one more slot to
  explain; faces one by one first.

## Consequences

- **Files**: materials painted on occurrences are lost (format v1 until 0.1.0: no
  compatibility code); the changelog says so.
- One part, one material: trying a copy in walnut makes it unique.
- An article assembly shows no material in the parts list until articles get their
  reference, supplier and price.
- Edge banding and veneers in the parts list wait for stable face names.
- A lock on components (`locked`, as SketchUp's) may come later, apart from the source.

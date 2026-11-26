# Materials, coverings, colorations: three families, on the part's definition

- Status: accepted (2026-11-26; step 1 built: the material on the definition; three families
  decided the same day; « Importer comme » dropped and the navigator's source mark decided
  the same day)
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

What a part is made of, what covers a face and what colours it are three things. Solid oak and
an oak veneer are not counted nor cut alike (OpenCutList's types: solid wood, sheet good,
dimensional, edge, veneer), and an edge banding has nothing to do beside solid wood in one list
of types. A paint gives a colour and is never cut; a tinted varnish lets the wood show. Layers
stack: an MDF panel, an oak veneer on a face, a paint or a stain over it.

STEP draws the same lines: a material designation belongs to the product's definition
(`MATERIAL_DESIGNATION`), a colour to the geometry (`STYLED_ITEM` on a solid, a shell or a face),
and a product is made or bought (`PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE`:
`.MADE.`, `.BOUGHT.`, `.NOT_KNOWN.`), whatever its own structure.

## Decision

- **Three families, each a table of its own** in the document, each a tab of the « Matières »
  card with its own « + », search, « Aucune » and unused items:
  - **materials** (« Matériaux »): what a part is made of, counted and cut; a **type**:
    « Massif » (the default), « Panneau », « Profilé » (dimensional timber, a tube, a bar);
  - **coverings** (« Revêtements »): what covers a face, counted (later, with stable face
    names); a type: « Placage » (the default), « Chant »;
  - **colorations** (« Colorations »): what colours a face, never counted. The colours of
    imports are colorations. Two categories:
    - a **coat** (« Couche », the default): a colour, maybe a texture, a **mode** and an
      **intensity** (0 to 100 %): « Couvrant » lays it over what is under (normal blending,
      the intensity its opacity: a paint, a thinned paint), « Teintant » multiplies what is
      under by it (the intensity its strength: a stain, a tinted varnish, the grain still
      seen). Its product and price, later, are the coat's;
    - a **scheme** (« Schéma »): an ordered list of coats of the catalog, referred to, not
      copied — « Noyer verni » is « Teinte noyer » then « Vernis ambré », the varnish existing
      once whatever the schemes using it. One level: a scheme holds coats, never schemes.
  Each item has a colour and maybe a texture, to be seen. **A type changes only within its
  family**, and every type of a family goes to the same place: nothing can end up where it does
  not go. An item never changes family (a new one is made).
- **Where they go**, on the part's definition and its faces; **only parts have them**: an
  assembly never does, whatever its source; painting it lays them on the parts below it. **None
  on occurrences, and no inheritance through assemblies.**

  | | Counted | Seen |
  |---|---|---|
  | Part | its material | its coloration over its material |
  | Face | its covering | its coloration, else the part's, over its covering, else the part's material |

  Under a coloration, the theme's default when nothing is there. A slot takes only an item of
  its family (the model refuses any other); a coat or a scheme alike.
- **A coloration is composed, not drawn layer by layer**: each coat maps what is under, per
  channel, to `A·x + B` (covering: `(1 − a)·x + a·c`; tinting: `x·((1 − s) + s·c)`), and the
  coats of a scheme compose into one such map, worked out once, applied by the shader over what
  is under (its colour or its texture): any number of coats costs one. A coat's texture shows
  only when it covers fully (« Couvrant », 100 %: it hides what is under and starts the
  composition again); otherwise its colour counts.
- **Deleting a coat used by schemes** removes it from them, after a confirmation saying how
  many; the purge keeps a coat only schemes use.
- **A component has a source** (`Component.source`), a field of the model like its name, not a
  property ([Properties](../model/properties.md)): it cannot be removed or renamed.
  « Fabriqué » (`Made`, the default), « Acheté » (`Bought`), « Existant » (`Existing`), chosen
  in the entity info (« Provenance »). In the navigator, a component bought or existing has a
  mark at the right of its row (a tag, a wall), its tooltip naming it.
- **The parts list** walks the tree from the model: a `Made` part is a row per definition, a
  `Made` assembly is gone through; a `Bought` component (a part or an assembly) is **one row,
  not gone through**; an `Existing` component is **no row, not gone through**. A material counts
  only on a part the list shows; inside an article or an existing component, materials are
  only seen. An article assembly's row has no material (« — »): what identifies it (a
  reference, a supplier, a price) comes later. This replaces the property « exclusion from the
  cut list » once planned on occurrences.
- **The paint tool lays what it is given where its family goes**: a material on the part
  clicked (the definition, with the scope: « Tous les exemplaires » / « Celui-ci », the second
  making the part unique); a covering on the face clicked (on every occurrence of its part); a
  coloration on the part or on the face, as the tool's option says (« La pièce / La face »),
  shown only for it. « Aucune » takes off the family of the tab it was chosen in. An assembly
  clicked lays it on the parts below it, at any depth, an article's or an existing component's
  included.
- **Faces are painted one by one** for now: their index in the record stays their name, with
  its known limit (a boolean renumbers them). Veneers and edges counted in the parts list wait
  for stable face names.
- **Imports**:
  - STEP: a product's source from `.BOUGHT.` (`.MADE.` and `.NOT_KNOWN.` give `Made`); a colour
    on a body or a shell becomes a **coloration** of its part, a part's product's
    `MATERIAL_DESIGNATION` a material (solid).
  - SketchUp: an instance's material becomes a **material** of its part (what it is made of),
    a face's a **coloration** of its face — or, by OpenCutList's type of it when it has one,
    the material, covering or coloration it says (sheet good, dimensional, edge, veneer…; a
    type that does not go where it is laid gives a coloration); a definition painted
    differently by its instances is made unique, one per item (decided 2026-11-26, as the
    user's earlier files were converted).
  - No option at import: the occurrence imported comes in selected, its source set in the
    entity info in one click (a vendor's slide file: one article).

## Alternatives considered

- **Keeping the material on the occurrence** (SketchUp's, OpenCutList's): a part's nature on
  one of its copies, a part in two rows of the list.
- **One kind of material with a type « substance / surface / untyped »** (decided first, the
  same day): a veneer and a paint in one slot of a face, so no paint over a veneer; and one
  « oak » for the solid and the veneer, which are not counted nor cut alike.
- **Two families, materials (typed solid… edge) and appearances** (decided next, the same
  day): an edge banding among the types of solid wood; and a type change could leave a
  material where it did not go, which then had to be flagged (⚠) everywhere.
- **One table with a family field**: a material could change family, and every slot would
  check it; a table per family makes a wrong one impossible.
- **One material for the solid and the veneer**, where it is laid telling them apart: counted
  and cut alike, which they are not.
- **Three cards**: three places to choose what the paint tool lays.
- **An opacity alone** for colorations: a thinned white over walnut greys it; a stain darkens
  and colours the wood, its grain still seen — a multiplication, not a mix.
- **Coats copied into each scheme**: the varnish (its product, its price) repeated in every
  combination.
- **A stack of coats on each face**, painted coat by coat: the same scheme painted again
  coat by coat on every face, the tool to tell adding from replacing, and the finishing
  scheme, a notion of the workshop, nameless.
- **Schemes holding schemes**: cycles, and stacks hard to read; one level suffices.
- **The type deciding nothing, the tool's option choosing the target**: the user picks a mode
  where the material already says where it goes.
- **« Acheté » as a material type** (OpenCutList's *hardware*): a bought slide has a substance
  (zinc-plated steel) and is bought, two facts; and an assembly bought has no single material.
- **A material on an article assembly**, for its row: a slide is made of parts, each in its
  own material, not of one material; it would be shown nowhere, an exception to explain. An
  article is counted, not cut: it is told by its reference, not by a material.
- **Source as a typed property**: it could be removed or renamed, and the parts list reads it.
- **An option « Importer comme »** (Fabriqué / Acheté / Existant) at import (decided first,
  the same day): a dialog of its own, the import having none, for what the entity info does in
  one click on the occurrence imported, selected.
- **Layers / tags for the environment** (SketchUp's): a display notion deciding what is made; a
  part left in the wrong layer would leave the list unnoticed. Layers may come later to hide the
  environment at once.
- **A finish on the definition now**, covering all faces present and future: one more slot to
  explain; faces one by one first.

## Consequences

- **Files**: earlier files whose faces are painted no longer open, and materials painted on
  occurrences are lost (format v1 until 0.1.0: no compatibility code); the changelog says so.
- One part, one material: trying a copy in walnut makes it unique (a coloration too: it is
  the part's).
- An article assembly shows no material in the parts list until articles get their
  reference, supplier and price.
- Coverings in the parts list wait for stable face names.
- The renderer shows what is under and the coloration over it: a palette entry per pair met
  (what is under, the coloration), its colour or texture and the coloration's map.
- Later modes (« Éclaircissant », a whitewash: lighten or overlay) fit the same map only in
  part (overlay is not affine): to see when asked.
- A lock on components (`locked`, as SketchUp's) may come later, apart from the source.

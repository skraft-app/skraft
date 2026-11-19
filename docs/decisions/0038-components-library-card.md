# The document's components in a card of their own, placed at the cursor by their origin

- Status: accepted (2026-11-19)
- Design: [Components library](../ui/components-library.md)

## Context

A component is a definition placed by occurrences; the navigator shows the occurrences, nothing
showed the definitions. A part the user named stayed in the document once its last occurrence
was gone (`Component::named`, `Document::purge` after every operation, the unnamed ones
dropped), invisible, and could neither be placed again nor purged. The solid tools' rework
(0037) ran into it (named tool parts left in an invisible library). SketchUp shows a model's
definitions in its Components window (« In Model »), places one by a click, and offers purge,
select instances, replace selected. Decided on 2026-11-19 (questions / answers).

## Decision

- **A docked card « Composants »** of its own, beside Entity info, Materials and the navigator:
  the document's **parts and assemblies** (not the body definitions), their count in the model,
  the unused ones in a section apart.
- **3D thumbnails**, rendered off screen lazily and cached per definition and revision, with a
  grid / list toggle.
- **A click or a drag carries one more occurrence at the cursor**, as a paste (`PlaceOccurrence`
  then `MoveTool::carrying`), **held by the origin of its axes**, in the active context.
- **Disabled while a part is open**; an assembly is never placed into itself.
- **Unused definitions stay**, named or not, until purged (as SketchUp's « In Model »); no
  more purge after every operation, but of the empty assemblies placed nowhere (they hold
  nothing). Asked on 2026-11-19, once the card showed only the named ones.
- **Managing**: purge the unused, select the occurrences, rename, delete
  (with all its occurrences), replace the selected occurrences.
- **One search module** for this card and the navigator, by name (case and diacritics
  ignored); the navigator's tree filtered to the matching rows and the rows above them. The
  card's parts and assemblies **in a tab each**, their counts those the search lets through.

## Alternatives considered

- **A mode of the navigator** (tree of occurrences / flat list of definitions): one card fewer,
  but both cannot be seen at once — placing from the list while seeing the tree.
- **One « Bibliothèque » card with tabs, Components and Materials**: both are the document's
  definitions, but materials are applied, components placed; the Materials card stays.
- **The body definitions too** (of the open part, or of every part, placed across the part's
  boundary): the navigator already shows the open part's; a later need.
- **A list with icons only**, or one preview of the item hovered: cheaper, but a part is
  recognised by its shape; thumbnails, rendered lazily, cost only what is shown.
- **Drag only**, or **a dedicated « Placer » tool** (insertion point, glued to faces, rotation
  keys): the paste's carrying already gives the inferences (its typed distance left out: from
  where the occurrence was put in place it means nothing, asked 2026-11-19); a richer tool can
  come later on the same start.
- **Held by its box's low corner** (as a paste) or its bottom centre: the origin of its axes is
  the component's own point, as SketchUp's insertion point.
- **Placing into an open part as the paste does** (bodies flattened, 0027) or **into the
  assembly holding it**: the user chose the library disabled while a part is open.
- **Purging the unnamed unused definitions after every operation** (as before): a part erased
  could not be placed again, and only the named ones showed in « Inutilisés ».
- **Separate searches**, the navigator's a « go to the row » without filtering: two ways to
  look for the same names.
- **Dimensions as a criterion** (« ep:18 »): needs a syntax; left for the cut list.
- **Nature toggles and a material menu** in the card (its first version): the toggles did a
  tab's job with more clicks; a component has no material of its own (its occurrences', several
  and inherited down the assemblies), so the menu answered a question about placements, not
  definitions — dropped on 2026-11-19.

## Consequences

- A boolean's tool part, an exploded assembly, the parts replaced or erased now pile up in
  « Inutilisés » until purged; the solid tools' « remove unused definitions » (0037) drops a
  boolean's.
- New operations: `PurgeUnused`, `DeleteComponent`, `ReplaceOccurrences`; the solid tools'
  « remove unused definitions » (0037, not merged yet) and `PurgeUnused` meet at their merge.
- A component carried is drawn only while the pointer is over the view (`Preview::away`).
- The carrying of `MoveTool` takes the point it holds (the content's box corner for a paste,
  the component's origin here).
- A thumbnail cache per document; later its pictures may be kept in the file's cache, once the
  file is a container.
- The navigator's rows learn to be filtered without their folds being written.

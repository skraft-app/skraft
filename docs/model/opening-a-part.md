---
status: built
code: [crates/tools/src/editor/mod.rs, crates/tools/src/editor/view.rs, crates/tools/src/scene.rs, crates/tools/src/tools/mod.rs, crates/render/src/lib.rs]
---

# Opening a part: its axes and its machinings

A part is edited directly; it is **opened** to edit what it is made of — its bodies, blanks
and machinings ([Bodies](bodies.md)). This page says how a part is opened and closed, what is
shown and picked while it is open, and how the tools work in its axes. Opening a body (its
profile) and a group of bodies, one level further down, is in [Edit levels](bodies.md#edit-levels)
and [Body groups](bodies.md#body-groups).

## Opening and closing

- **Double-clicking a part's occurrence opens the part**, as double-clicking an assembly's
  enters it: the editor keeps the assembly context and adds the occurrence open in it
  (`Editor::open_part`). Esc (Select, nothing selected), a click off its grey box or a click in
  the navigator close it, the occurrence left selected; an undo that removes it closes it.
  While it is open nothing else is selectable.
- **The navigator marks the level open**: the part's row while it is the level open (nothing
  selected in it); a group (or a body node) open in it, that group's row, the part and the
  groups above it on the way marked only by their name in the accent's colour.
- **What surrounds it is faded**, as outside any context; the part's **other occurrences** are
  marked `Related`: whatever is changed reaches them, under the scope ([Copies and propagation](data-model.md#copies-and-propagation)). A **grey dashed
  box**, a few pixels off its blank, says the part is open, as for an assembly entered.

## The part's axes

- **The part's axes** replace the drawn axes, as an assembly's do when it is entered, and show
  even when the axes are hidden: its origin, through the occurrence's T (a mirrored occurrence
  shows its left-handed frame). In an open part, **tools draw and infer in these axes**
  ([A part's axes](data-model.md#a-parts-axes)):
  axis locks, inferred axes, the ground, typed points, the frame of a box drawn. The tools see
  two frames: the context's, which operations are expressed in, and the axes
  (`Cx::axes`, `Cx::axes_in_context` = the occurrence's T). **Drawing in an open part adds a
  body** to it (`Operation::AddBody { sign }`, a machining unless the bar's « Solide » choice says
  « Ajouter », [Bodies](bodies.md#the-interface-of-bodies)): the shape drawn, in the part's
  axes, cut (or joined) at once; the part stays open.

## What is shown: its bodies

- **The open part shows its bodies**, always ([Edit levels](bodies.md#edit-levels)): **the
  blanks as ghosts** (very light faces, sharp edges and silhouettes, drawn after the opaque
  pass, depth tested without writing) and **the machinings as ghosts too**, red and a little
  more opaque, the intersections (∩ bodies) likewise in a blue-green of their own, a linked
  machining with its edges and silhouettes dashed (`Look::LinkedTool`, `Batch::dashed_edges`:
  laid along the edge in model units, a power of two near ten half widths on screen; a
  silhouette's on screen); one that failed stays opaque, in its own colour
  ([The interface of bodies](bodies.md#the-interface-of-bodies)). The machinings are picked
  first. Once the part is closed, its shape alone is seen.
- **How**: the display scene meshes the recipe of the part shown — one mesh per body
  definition (a copied body shares its original's) — under keys apart from components' ids
  (`RecipeMeshes`); the view gives each placement a `Look` (shape, blank, tool, failed tool)
  and leaves out the open occurrence's shape; the renderer draws a **ghost batch**'s edges like
  any other, then, after everything opaque, its front faces blended at the theme's
  `ghost_opacity`, depth tested without writing depth (no cap in a section); the tools take two
  palette entries the viewport adds after the materials.
- **Inferences see what is drawn**: the bodies stand in for the open occurrence's shape
  (`DisplayScene::inferable`, `pick_inferable`), with their snaps — a tool's axis, the centre
  of its end, its edges out of the blank. Each piece says what it is (`Piece`: shape, blank,
  tool of a machining, guide); a tool's filter sees it (`Owner`), so a move or a rotation
  leaves the bodies it moves out (`Cx::is_moving`). Picking for the other tools (paint,
  booleans, section, navigation) stays on the shapes.

## Editing the bodies

- **Bodies are edited there**: the selection is then the open part's bodies (a click on one,
  Select all), erased (Delete), moved (copy and array too) and turned; while dragging, the
  moved body is a ghost where it goes, not drawn where it was, and the part is computed again
  when the change is applied (asynchronous, [The kernel](kernel.md)). Move and Rotate say what
  they change through `Cx::moved` (`Moved`: occurrences of the context, or bodies of the open
  part, each in its frame), copies and arrays through `LastCopy`.
- **The scope** of every change to the open part (drawing, editing bodies) is a choice of
  the context card while it is open ([Context card](../ui/interface.md#context-card)): « Tous les exemplaires ×N » /
  « Celui-ci », **back to « all » at each opening**. **Why:** a change to one occurrence only
  makes it a separate part: better chosen each time. The `Related` marks follow it. Shown only
  for a part placed more than once (`Editor::open_part_copies`, hidden occurrences and copies
  of assemblies counted): for one alone both choices are the same (`Document::separate` copies
  nothing), the scope stays « all ».
- **The navigator lists every part's bodies** under its row ([Body groups](bodies.md#body-groups)):
  hovering a row draws its body hovered, hovering a body in the view lights its row; a click
  opens the part and selects the body (Shift adds or removes). The entity info shows what is
  selected, a body too ([Bodies](bodies.md)).
- Not yet: marquee selection of bodies (the marquee takes occurrences).

---
status: built
code: [crates/tools/src/tools/blend.rs, crates/tools/src/infer/edges.rs, crates/tools/src/editor/mod.rs, crates/model/src/ops.rs, crates/kernel/src/node.rs, crates/kernel/src/names.rs]
---

# Rounds and chamfers

« Arrondir » and « Chanfreiner » (the ribbon's « Remodeler » group) round or chamfer edges of a
body or of a part. What they make is a **body node** wrapping what the edges belong to: its
children's result, then rounded — the model's, [Body nodes](../model/bodies.md#body-nodes);
why: [0042](../decisions/0042-body-nodes-operators.md). OpenCutList has no such tool (nor
SketchUp): the interaction is Skraft's.

## Picking the edges

- **Under the cursor**: the edge of the shapes shown nearest the cursor, within the
  inferences' aperture and seen (nothing in front of it, not cut by the section), lit
  (`infer::edge_at`, on the display mesh's snap edges). In an open part, the edges of its
  bodies (each body's own, as its recipe is shown); else the edges of the parts' shapes.
- **A click** takes the edge, or lets it go when taken. All the edges are of one body or one
  part: once one is taken, only the edges of its body or part light and are taken (the others
  still hide what lies behind them); Esc lets them all go to pick on another. **A double click** takes its whole tangent
  chain (`infer::tangent_chain`: the edges running on from it with the same tangent at their
  shared ends, on the display's snap edges) — the kernel refuses a round that stops inside a
  tangent chain.
- **The size** (the radius, the distance) is typed in the measurements box at any time, or in
  the option bar: a saved option of each tool (`size`), 2 mm at first, the next round taking it
  again.
- **Enter** or ✓ applies it (the steps bar, as the solid tools'); Esc or ✗ lets every edge go.

Each edge goes to the kernel as **a point inside it** (its middle, half way round a whole
circle), in the part's axes (the open part's or group's, else the part's): a point at a
vertex between two edges names neither, and is refused.

## What it makes

`Operation::Blend { target, bodies, blend, size, picks, scope }`:
- in an open part, the body the edges are of is wrapped (`bodies` that one);
- on a closed part, all its bodies (its root's content).

The kernel names the edges the picks are nearest (in the node's children), applies the
round, and the node keeps their names. A size the kernel does not build is **refused, never
reduced**: the operation fails with the largest that builds (« trop grand : 1 mm au plus »).

**A round larger than the faces it lies on is kept**: SolidCore runs it past them and trims it
by the faces beyond (a board 18 mm thick, its top edge rounded 30: the front face gone, the
round's cylinder cut by the bottom), a valid shape — a router profile larger than the board.
« Trop grand » is the kernel's only: rounds that meet (both long edges of a lath).
**Why** not refused (decided 2026-11-20): the shape is what the size asks for; refusing it
would be guessing what the user meant.

## The preview

Each change (an edge, the size) asks the kernel for the node alone (`Document::blend_job`,
`Task::Node`, not the part computed again around it), the job before it dropped
(`Editor::update_blend_preview`). Its result is drawn in the body's or the part's stead (the
stretch's swaps, `Editor::blend_done`); a refusal is said in the status bar. Applied, the
preview is held until the kernel gives the part its new shape (`Editor::held`, as a stretch's).

## Editing a node

- **Its size** in the entity info (« Rayon » / « Distance »): `Operation::SetBlend`, every copy
  of the node changed; a size refused is not applied. The navigator shows it after the node's
  name (« Arrondi#1 r 5 »).
- **Its edges**: the entity info's « Choisir les arêtes » opens the node and starts its tool
  (`Editor::edit_blend`). In a node opened, the tool picks the node's edges again rather than
  wrapping a body: its children are shown as they are (the node not applied, as any group
  opened), its edges lit and picked already. Edges are read and picked on the **children's
  result** (`Document::node_raw`, `DisplayScene::node_raw`, `names::edges_named`), not on the
  children shown: an edge between two of them lies on neither. That result is the node's
  cache, or, when it does not hold (a file just read, it is not saved), computed in the
  background when the node is opened, its edges lit once it comes (`Editor::update_node_raw`).
  Enter applies `SetBlend` with the picks, the node's edges named again — with the node's own
  size (shown in the bar and the measurements box, following the entity info's; typed
  there, it changes the node's, not the tool's saved one) — then closes the node, selected,
  the selection tool back. ✓ shows only once its edges or its size changed; Esc takes its edges
  back as they are.
- **A node open, under another tool than its own** (the selection's, after a double click):
  its edges are lit on its children all the same, the context card names it with its size and
  « Modifier » (its tool started on it: its edges, its size), and the status bar says so first
  (`Editor::open_node`). The navigator marks the node's row open, as a part's, the levels above
  it on the way only by their name in the accent's colour.
- **Its children**, by opening it as a group: they are edited as in a group, and the node
  applied again by its names. Opened, the part's shape is not drawn (it holds the node's
  result, which would show over its children raw): the bodies of the levels above it, but those
  on the way to it, are drawn faded instead, each where it is (`Editor::around_open_node`); a
  plain group opened keeps the part's shape faded around it. A name that no longer matches an
  edge leaves the node in error (⚠ in the navigator, its tooltip and the entity info saying
  why, its children's result shown as it is). An edit changes only the bodies it is applied
  to: a cutter flush with a face stretched does not follow it, and when that takes the edge
  away the node says so. **Why:** the app makes no guess at what the user meant to change.

Not yet: a round nested inside a node opened (the tool there edits the node).

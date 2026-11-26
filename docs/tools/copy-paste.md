---
status: built
code: [crates/model/src/clipboard.rs, crates/model/src/ops.rs, crates/tools/src/editor/clipboard.rs, crates/tools/src/tools/mover.rs, apps/skraft/src/system/clipboard.rs, apps/skraft/src/app.rs, apps/skraft/src/app/]
---

# Copy, cut and paste

Copy, cut and paste whatever can be selected — occurrences and guides of an assembly, bodies and
guides of an open part — within a document or from one document (tab) to another, in place or
at the cursor. What is copied is a fragment held in memory by the app; the system clipboard only
receives a token naming it.
On a drawing's sheet the same commands act on its elements ([Drawing](../ui/drawing.md#copy-and-paste)).

## What is copied

Whatever can be selected. Out of an open part: the context's occurrences (parts and assemblies,
their materials, their components' guides) and the context's guides; parts picked at any depth
(the « Parts » scope, [Select first, and a selection scope](selection.md)) each from its own
assembly, and the guides picked at any depth with them, each in world axes
(`Document::paths_fragment`). In an open part: its bodies (« machinings ») and its
guides. **Cut** = copy, then the existing erase (one undo step for the erase).

## Fragments

- **The entities' fragment is a `Document`**, as `Operation::Insert`'s ([STEP import](../io/step-import.md)): the closure of the
  components the copied occurrences use (shared by `Arc`: copying costs nearly nothing,
  whatever the model's size) and the materials they are painted with. Its root holds the
  copied occurrences with their **world** transformation (the source context's world
  transformation applied), its guides the copied guides, in world axes too.
- **Bodies copied from an open part** go into a fragment of their own kind, a `BodyFragment {
  defs, bodies, guides, world }`: the bodies and their placements **in the part's axes** (in an
  open group, the group's, [Bodies](../model/bodies.md) « Body groups »), the definitions they
  use, the part's guides in those axes, and `world`, the world transformation of the part copied
  from.
- Built by `crates/model/src/clipboard.rs`: `Document::entities_fragment(from, ids)`,
  `Document::paths_fragment(paths)`, `Document::bodies_fragment(target, ids)`. The fragment keeps
  the source's ids.

Decision record: [Copy and paste through fragments and an in-app clipboard](../decisions/0017-copy-paste-fragments-and-app-clipboard.md)

## The clipboard

**In the app, plus a token.** The fragment stays in memory (`Arc`), one clipboard for the whole
app: copying in one tab and pasting in another is the "from one document to another" case
([Several documents](../ui/file-tabs.md)). The system clipboard receives only a small **token**
(a text naming this clipboard and its serial number): if the user copies something else
anywhere, the clipboard no longer matches the token and ⌘V does nothing. Not carried from one
instance of the app to another, nor between browser tabs.

- **The editor's `Clip`** (`crates/tools/src/editor/clipboard.rs`): its `content` —
  `Content::Entities(Arc<Document>)` or `Content::Machinings(Arc<BodyFragment>)` (`Editor::copy` /
  `cut`) — and its `hull`: points bounding it in the frame of its content (world for
  occurrences, the part's axes for bodies), the corners of its solids' boxes (taken at the copy,
  where they are meshed) and a point of each guide.
- **The app's `Clipboard`** (`apps/skraft/src/system/clipboard.rs`): the `Clip`, the `DocId` and the
  history revision it was copied at (before the erase, for a cut), and its token
  `skraft-clipboard:<run nonce>:<copy count>` (the nonce drawn once per run: two instances give
  different tokens).

## Same document or not

- **Pasting in the same document gives the same parts**: new occurrences of the same parts
  and assemblies ("Leg ×5"), as a copy made by Move ([Data model](../model/data-model.md)). The source document is known by
  its `DocId`; a component of the fragment that is no longer in the document (erased, purged
  after a cut) is grafted from the fragment, as one from another document. **In another
  document**, everything is grafted (`Document::graft`: fresh ids, names made unique,
  identical materials reused). Unlike `Insert`, several top occurrences stay loose (not
  gathered in an assembly).
- **`reuse`** is true only when the copy's document is the target tab and the revision copied is
  still in its history (`Clipboard::reuse`, `History::holds`: undo stack, current state or redo
  stack): ids only grow from that state, so they still name the same things. Undone past the copy
  and changed since, an id may name another component: then everything is grafted.
- **Bodies pasted into the part copied from** are copies of the same definitions (the solid and
  the profile unchanged); elsewhere, copies of the definitions with fresh ids, bodies sharing a
  definition still sharing its copy.

## Where it lands

**Paste** (⌘V) hangs the pasted content on the cursor, with the Move tool's inferences and typed
distance; a click places it, Esc gives it up (the history keeps one step, or none). It is held
by **the min corner of its bounding box**, in the context's axes (low, left, front), so it lands
on the ground or on a face as in SketchUp. **Paste in place** (⇧⌘V) puts it where it was in
world space, brought into the active context — bodies too, brought into the open part's axes
(the fragment's `world`). The pasted content is selected; one undo step.

**Why** bodies pasted in place keep their world place rather than their placement in the part's
axes: every other paste keeps the world place; carrying holes onto a twin part is a paste at the
cursor.

## Across the part's boundary

Everything pastes anywhere (`Editor::can_paste`: nothing computing, the tool idle):


| Copied | Pasted | Operation |
|---|---|---|
| occurrences | out of an open part | `Paste` |
| occurrences | into an open part | `PasteParts`: bodies of it |
| bodies | into an open part (any part, of any document) | `PasteBodies` |
| bodies | out of an open part | `PasteAsPart`: a new part |

- **Parts into an open part** (`Operation::PasteParts { target, fragment, transform, scope }`,
  the entities' fragment, in world axes, brought into the part's; `Document::parts_as_bodies`):
  **one body per part**, at any depth of the assemblies copied (hidden occurrences left out),
  placed by its world transformation, **positive** whatever the « Solide » choice (the ＋/－
  badge changes it after). As the difference's tool ([Bodies](../model/bodies.md)): a part that
  is its single positive body (a shape drawn) gives that body whole, its profile with it — still
  editable; any other part comes as **a group of its bodies** ([Bodies](../model/bodies.md)
  « Body groups »), its shape the group's: nothing lost. Copies of one part share one
  definition. A part the user named names the body it becomes, unless that body has a name
  ([Bodies](../model/bodies.md) « Body names »). The guides copied go into the part. The bodies
  pasted are seen, selected and carried.
- **Bodies out of an open part** (`Operation::PasteAsPart { context, fragment, transform }`):
  **one new part** holding every body pasted, their placements kept — **its axes are the
  part's copied from** (the fragment's `world`), so the occurrence is placed by it, then brought
  into the context. **Signs kept**; with no positive among them, they all become positive (a
  hole's shape becomes a part: a pin). Positives apart make a part « in N pieces », as
  anywhere. The definitions are copied (they belong to their part); the guides copied become the
  new part's. Named as a new part.
- Guides follow the content they were copied with.
- Outcome warnings (a paste's, the union's) are toasts of the **warning** level (amber); a
  paste's warning is said once, when the paste is put down (not for the paste in place under the
  cursor).

Decision record: [Parts paste into an open part as bodies, bodies out of one as a new part](../decisions/0027-paste-across-the-part-boundary.md)

## Operations

One undo step each, Paste in place being an identity or the inverse of the target context's
world transformation (`transform`, rigid):

- `Operation::Paste { context, fragment, transform, reuse }`: no kernel. `Document::graft`
  takes a `reuse` flag: with it, the components and materials whose ids are still in the
  document are used as they are, the others grafted with fresh ids (which ids are kept is
  decided before any is drawn: a fresh id may be one of the fragment's). Refused: a cycle
  (an assembly pasted into itself), a transformation not rigid, an empty fragment
  (`Refusal::NothingPasted`);
- `Operation::PasteBodies { target, fragment, transform, scope }`: the copies added to the part
  as it is (one recipe task), the guides added to the part; refused when not rigid or empty;
  "only this one" makes the part a separate one first;
- `Operation::PasteParts { target, fragment, transform, scope }` and
  `Operation::PasteAsPart { context, fragment, transform }` ([Across the part's
  boundary](#across-the-parts-boundary)).

## Commands and keys

- **Commands**: `edit.cut` (⌘X), `edit.copy` (⌘C), `edit.paste` (⌘V), `edit.paste_in_place`
  (⇧⌘V), in the Edit group (palette, shortcuts, control channel); enabled when the selection
  can be copied (`Editor::can_copy`: something selected, nothing computing, the tool at its
  first step), or when the clipboard can be pasted (`Editor::can_paste`).
- **Keys** (`apps/skraft/src/system/clipboard.rs`, `app/keys.rs`, `app/command_run.rs`): ⌘X, ⌘C and ⌘V reach the
  app as egui's `Event::Cut` / `Event::Copy` / `Event::Paste(text)` — egui-winit sends no key
  for them; the web sends the key *and* the clipboard event, so the keys are ignored — and
  are fixed whatever the shortcuts, done only when enabled, silently otherwise. A text field
  being edited keeps them (egui wants the keyboard). `Event::Paste` pastes only when its text
  is the token (`Clipboard::is_on_system_clipboard`); from the menu or the palette, the app's
  clipboard is trusted (the system clipboard is not read). eframe's `clipboard` feature is on
  (arboard on the desktop): text fields use the system clipboard too, not only egui's
  in-process one.

## Paste at the cursor

`Editor::paste`: the paste is done **in place** first (`After::Carry`), then carried by the
**Move tool** in a mode of its own (`MoveTool::carrying`): already moving, from the low corner of
the content's box in the axes (the box of the `Clip`'s hull, in world axes for occurrences, the
part's copied from for bodies, brought into the axes tools work in), with Move's inferences,
locks and typed distance or point; no copy toggle (none in the option bar). The click **replaces** the paste in place by
the same paste moved (`Command::Replace`, `Moved::paste`: the transformation composed with the
motion in the frame): one step in the history. Esc, Undo, Redo or another tool give it up: the
paste in place, still the last step, is undone (`Editor::settle_carry`). Put down or given up,
the tool before comes back. So what is carried is in the document meanwhile (drawn by Move's
ghost, out of the inferences); a paste of bodies computes the part twice (in place, then moved).

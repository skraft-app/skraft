# « Filaire », a third state of an occurrence: visible but inert

- Status: accepted (2026-11-27); built (2026-11-27)
- Design: [Hiding and wireframe](../tools/interaction.md#hiding-and-wireframe)
- Amends: [Every object is an occurrence of a part or an assembly](0005-parts-assemblies-occurrences.md)
  (`hidden`, a flag, becomes `Visibility`, a state)

## Context

Decided on 2026-11-27, asked for while modelling: an occurrence was either shown or hidden,
and neither served to draw **from** a part without drawing **on** it. To start a line in the
middle of where a board stands, the board had to be shown — and then the cursor snapped to its
faces, edges and corners instead of the point wanted — or hidden, and then there was nothing
left to aim at.

X-ray (`display.xray`) already draws everything see-through, but it is a view of the whole
model and, by design, hides nothing from inferences: it makes the problem worse, not better.
`display.hide_rest_of_model` is tied to the active context, not chosen per occurrence.

What was missing is a per-occurrence state that is **seen but not touched**.

## Decision

- **`Occurrence::hidden: bool` becomes `Occurrence::visibility: Visibility`**, one of `Shown`,
  `Wireframe`, `Hidden`. One state per occurrence, as the flag was; `Operation::SetHidden`
  becomes `Operation::SetVisibility { context, entities, visibility }`.
- **`Wireframe` is « Filaire »** in the interface, « Mettre en filaire » the operation's label
  (decided 2026-11-27). The name says what it is for, not how it is drawn.
- **Drawn see-through, with its edges as they are**: `ViewPlacement::ghost`, the faces at the
  style's `ghost_opacity`, no new rendering. Where the thing stands reads exactly, and zoom
  extents still frames it.
- **Inert**: neither picked (click or rectangle), inferred on, hovered nor taken by select all,
  and it hides nothing behind it. One gate does it — the scene's index walk asks
  `DisplayScene::is_live` (drawn, and not in wireframe) where it asked `is_shown` — so picking,
  inferences, the marquee and the counts under a path all follow from one place.
- **Its guides go inert with it** (`SceneGuide::wireframe`, drawn faded): an occurrence put
  aside takes its construction lines with it, or the cursor would still catch them.
- **Inherited, hiding wins**: everything under an occurrence takes its state, the deepest one
  along a path winning, and `Hidden` above beats `Wireframe` below. `Visibility` is ordered by
  how much it takes away, so exploding takes the `max` of the assembly's state and its
  content's.
- **No setting to reveal it**, unlike `display.show_hidden`: it is always visible, so there is
  nothing to reveal. `view.show_all` brings back the hidden and the wireframe alike
  (`Editor::has_unshown`).
- **A modelling-time aid, not a property of the product**: the parts list counts it and a
  drawing shows it. Only the hidden are left out of `Document::subject_parts`.

## Alternatives considered

- **A fourth rendering, edges only (a true wireframe)**: lighter on screen and unmistakably
  inert, but it needs a faces-off batch mode in `render`, and a board then reads as an empty box
  rather than as a solid standing somewhere. The name « Filaire » was kept for what it is for;
  a translucent ghost shows the volume, which is what one aims at.
- **Translucent faces *and* faint edges** (the eraser's preview look): the strongest « inert »
  signal, but it already means « what would go » when the eraser runs, and the edges are exactly
  what makes the position readable.
- **A per-occurrence X-ray flag**: X-ray hides nothing from inferences on purpose, so it would
  have been see-through *and* still snapping — the opposite of what was asked.
- **Keeping `hidden: bool` and adding `wireframe: bool`**: two flags that can both be true, an
  invalid state to normalise everywhere. Three mutually exclusive states are one `match`.
- **A state on bodies too** (a machining inert while a part is open): useful, but bodies have no
  `hidden` today, so it would be a second mechanism to design and document rather than an
  extension of one. Left out.

## Consequences

- **Older files lose which occurrences were hidden**: the field is `visibility` now, and
  `"hidden": true` is ignored on reading (file format v1 until `0.1.0`, no compatibility code —
  [0010](0010-file-format-v1-until-release.md)). The release's changelog says so.
- `PlacedPart` and `PlacedGuide` carry `wireframe_at` beside `hidden_at`, both filled by one
  `SetAbove` carried down the document's walks; `set_off` (was `hidden_off`) answers for either.
- The navigator's eye cycles shown → wireframe → hidden → shown, with a third icon
  (`EYE_WIRE`); a row in wireframe reads as usual, since the thing is on screen.

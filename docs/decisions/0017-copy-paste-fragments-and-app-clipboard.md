# Copy and paste through document fragments and an in-app clipboard with a token

- Status: accepted (2026-11-14)
- Design: [Copy, cut and paste](../tools/copy-paste.md)

## Context

Decided on 2026-11-14 (questions / answers). Copying must cover whatever can be selected (an
assembly's occurrences and guides, an open part's machinings and guides), within a document and
from one document (tab) to another, whatever the model's size.

## Decision

- **The fragment is a `Document`**, as `Operation::Insert`'s: the closure of the components the
  copied occurrences use, shared by `Arc` (copying costs nearly nothing), and their materials;
  its root holds the copied occurrences and guides in **world** axes. Machinings copied from an
  open part go into a fragment of their own kind, in the part's axes.
- **Clipboard: in the app, plus a token.** The fragment stays in memory, one clipboard for the
  whole app; the system clipboard receives only a small token naming it. Copying something else
  anywhere replaces the token, and ⌘V then does nothing.
- **Pasting in the same document gives the same parts** (new occurrences, as Move's copy); in
  another document everything is grafted (`Document::graft`). Reuse is decided by the document
  and whether the revision copied is still in its history (ids only grow from that state).
- **Paste** hangs the content on the cursor (held by the min corner of its box, as SketchUp);
  **Paste in place** puts it where it was in world space.
- Machinings pasted only into an open part; occurrences did not paste into an open part, nor
  machinings out of one (Paste disabled) — superseded by
  [Paste across the part's boundary](0027-paste-across-the-part-boundary.md).
- Built in steps on 2026-11-14: the model; the commands, clipboard and Paste in place; Paste at
  the cursor (done in place, then carried by the Move tool, the click replacing it by the same
  paste moved).

## Alternatives considered

None recorded.

## Consequences

- Copying is cheap whatever the model's size; a paste of machinings computes the part twice when
  pasted at the cursor (in place, then moved).
- The clipboard is not carried from one instance of the app to another, nor between browser
  tabs; since the fragment is a document, carrying its serialised text could come later.
- Until 2026-11-17, machinings pasted "in place" kept their placement in the part's axes (the
  holes of one part carried onto its twin), which the user took for a bug: the other pastes keep
  the world place. They now land where they were in world space (`BodyFragment::world`);
  carrying holes onto a twin is a paste at the cursor.

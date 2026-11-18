# Parts paste into an open part as bodies, bodies out of one as a new part

- Status: accepted (2026-11-16)
- Design: [Copy, cut and paste](../tools/copy-paste.md#across-the-parts-boundary)

## Context

Decided on 2026-11-16 (questions / answers; branch `feature/paste-into-part`). Until then
occurrences did not paste into an open part, nor machinings out of one: Paste was disabled.
Bodies ([Bodies](../model/bodies.md)) make a part's content bodies with a sign, which a paste can
carry either way.

## Decision

This supersedes "Occurrences do not paste into an open part, nor machinings out of one".

- **Parts into an open part** (`Operation::PasteParts`): one body per part, at any depth of the
  assemblies copied (hidden occurrences left out), placed by its world transformation, positive
  whatever the « Solide » choice. A part that is its single positive body gives that body whole,
  its profile with it; any other part came first as one `Brep` body, flattened with a warning,
  then (since 2026-11-16) as a group of its bodies ([Bodies](../model/bodies.md) « Body
  groups »): nothing lost.
- **Bodies out of an open part** (`Operation::PasteAsPart`): one new part holding every body
  pasted, its axes the part's copied from (`BodyFragment::world`); signs kept, all positive when
  none is.
- Both carried at the cursor as the other pastes are.

## Alternatives considered

None recorded.

## Consequences

- Paste is never disabled by the context any more.
- Outcome warnings became toasts of a fourth level, **warning** (amber), the union's too.
- The bodies pasted are seen, selected and carried (the « Corps » switch was turned on for it
  until 2026-11-17).

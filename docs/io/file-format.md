---
status: built
code: [crates/model/src/file.rs, crates/model/src/settings.rs, apps/skraft/src/file.rs, apps/skraft/src/opened.rs, packaging/]
---

# File format

What a `.skraft` file holds, how the operating systems hand one to the app, and the versioning
policy until the first release.

## Content

Versioned JSON (`{"format": "skraft", "version": 1, …}`): serde serialisation of the
document, B-reps included (SolidCore's record format), and its settings (`settings`; missing:
default values). In the browser: download and file upload.

Components, occurrences, the parts' recipes (blank + machinings, with their `source`), the
components' guides ([Tape measure and guides](../tools/tape-measure.md)). The final B-rep is not
the source: it is saved as a cache (`shape`), present exactly when the part has machinings, to
open without recomputing.

## Extension and file associations

**Extension `.skraft`**, MIME type `application/x-skraft`. The installers associate it with
the app (`packaging/`): an exported UTI `fr.lairdubois.skraft.document` and a document
type on macOS, a ProgId on Windows, a shared-mime-info type and the `.desktop` entry's
`MimeType` on Linux. A file double-clicked reaches the app as an argument (Windows, Linux: a
new instance) or as an Apple Event « open documents » (macOS, launched or running: winit does
not handle it, the app installs its own handler when it will finish launching); either way it
waits in a queue (`apps/skraft/src/opened.rs`) and opens in a tab of its own at the next
frame.

## Version 1 until the app is published

The format changes without a new version and older files are not read — no converter, no
reference files. Versioning and backward compatibility start with the first release.

Decision record: [File format version 1 until the first release](../decisions/0010-file-format-v1-until-release.md).

# The .skraft file is a ZIP of JSON entries

- Status: accepted (2026-11-19)
- Design: [File format](../io/file-format.md)

## Context

A `.skraft` file was one JSON of the document, its B-reps inline. Measured on real models
(`ZeTable.skraft`, 6 parts, 16 occurrences: 227 kB): 95 % of it is SolidCore records (faces with
their pcurves, edges), half of them caches (the parts' shapes, the profiled bodies' solids);
gzip alone makes it 15 times smaller. The file is to receive the materials' textures, and
materials are to be exported alone. Decided on 2026-11-19 (questions / answers): a ZIP, the
caches kept, the structure readable as JSON.

## Decision

- **A ZIP** (Deflate): `manifest.json` (format, version, `content`, generator),
  `document.json` (the document, its solids named), `breps/<name>.json` (the solids it is made
  of), `cache/<name>.json` (those computed from them).
- **B-reps as SolidCore's record JSON**, one entry each, named by 128 bits of the BLAKE3 hash
  of that JSON: the same solid, one entry.
- **The caches are kept** in the file, to open without computing; a file missing one is refused.
- **`document.json` readable**: indented, arrays of plain values on one line.
- **No dates, a fixed order**: the same document, the same bytes.
- Through serde: the two fields of solids (`Part::shape`, `BodyDef::solid`) serialise through
  `crate::file::solid(_opt)`, which writes the record inline — or, while a file is written or
  read, its entry's name (a thread-local for the time of one `serde_json` call). The inline
  form stays `Document::to_json`, for the journal's recordings and the tests.

## Alternatives considered

- **SolidCore's native binary body** (`solidcore_record::native`, `SCBREP05`, SolidCraft's):
  measured on `ZeTable`, 308 kB against 220 kB of JSON, 27.8 kB against 21.8 kB once each entry
  is compressed, and slower to read; it versions the same geometry schema as the JSON's
  `version`. Dropped: nothing gained, readability lost.
- **One compressed JSON** (gzip of the old file: 15 kB against 25 kB): smaller, but no
  entries for binary assets (textures, thumbnail), no shared B-reps, nothing readable without
  decompressing all.
- **A binary of our own** (MessagePack, postcard): a format to document and version beside
  SolidCore's, unreadable; compressed JSON is as small.
- **Dropping the caches** (7.7 kB of JSON for `ZeTable` without them): opening would compute
  every booleaned part and profiled body again; refused for the opening's speed.
- **zstd**: smaller than Deflate (11 against 15 kB for one stream), but a C library, a problem
  for the web build.

## Consequences

- `ZeTable.skraft`: 227 kB → 25.5 kB; written in ~3 ms, read in ~2 ms (dev profile).
- The files written before (one JSON) are not read (`FileError::NotZip`):
  [0010](0010-file-format-v1-until-release.md).
- New dependencies: `zip` (Deflate only, `flate2` / `miniz_oxide`) and `blake3`.
- Each entry compressed apart costs ~40 % against one stream (no shared dictionary); a large
  model's entries can be read in parallel, later.

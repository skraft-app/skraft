---
status: built
code: [crates/model/src/file.rs, crates/model/src/settings.rs, apps/skraft/src/file.rs, apps/skraft/src/opened.rs, packaging/]
---

# File format

What a `.skraft` file holds, how the operating systems hand one to the app, and the versioning
policy until the first release.

## Content

A **ZIP** (Deflate), whose entries are JSON — but the images:

```
model.skraft
├── manifest.json               {"format": "skraft", "version": 1, "content": "model", "generator": "Skraft 0.1.0"}
├── document.json               the document: its structure, readable
├── breps/<name>.json           the B-reps it is made of
├── cache/<name>.json           the B-reps computed from them
└── textures/<name>.<png|jpg>   the images of the materials' textures, as they came (stored)
```

- **`manifest.json`**, read first: the format, its version (a newer one is refused:
  `FileError::TooNew`), what the file holds (`content`: `model`, or `materials` for a
  [`.skraftmat`](#materials-alone-skraftmat)) and the app that wrote it, for information.
- **`document.json`**: the serde serialisation of the `Document` — components, occurrences,
  the parts' bodies and body definitions with their profiles and `source`, the components' guides
  ([Tape measure and guides](../tools/tape-measure.md)), the materials, the settings
  (`settings`; missing: default values). Its solids are the **names** of their entries. Indented,
  arrays of plain values on one line (`"transform": [1.0, 0.0, …]`): read with `unzip -p
  model.skraft document.json`.
- **The B-reps**: one entry each, SolidCore's record as JSON (its own `version`: the geometry
  schema). An entry's name is 128 bits of the BLAKE3 hash of that JSON: a solid used twice is
  stored once. `breps/` holds those the document is made of — the solids of the leaf bodies
  without a profile (imported, booleans' results, stretched askew); `cache/` those computed
  from them, kept to open without computing them again — the parts' shapes (present exactly when
  a part is not its single positive body), the profiled bodies' and the groups' solids. A
  cache equal to a source is that source. A file is refused when a solid it names is missing,
  caches included: they are not computed again on opening.
- **The images** ([Textures](../render/look.md#textures)): one entry each, its name its id
  (128 bits of the BLAKE3 hash of its bytes) and its kind's extension, not compressed again;
  `document.json`'s `assets` names them with their kind (`"<id>": "jpeg"`). A file naming an
  image it does not have is refused.
- **No dates** in the ZIP (1980-01-01) and entries in a fixed order: the same document gives
  the same bytes.

Opening reads every entry, then the document; then `Document::check` checks its invariants
(ids, the shapes in step with their bodies, known materials). In the browser: download and
file upload.

`Document::to_json` / `from_json` keep the document as **one JSON, its B-reps and images
inline** (an image's bytes in base64): what a
recording of the journal starts from ([Control protocol](../control-protocol.md)) and what
`model.document` shows, and the tests'. It is not a file the app opens.

### Why

- **The B-reps are 95 % of a model** (`ZeTable.skraft`: 220 of 227 kB as one JSON, half of
  them caches); compressed, the file is 9 times smaller (25 kB), and reads as fast.
- **JSON for the B-reps too**: SolidCore's native binary body (`solidcore_record::native`,
  `SCBREP05`) measured 40 % larger than the JSON, and still 30 % larger compressed, slower to
  read; it versions the same geometry schema. The JSON stays readable.
- **A ZIP** rather than one compressed JSON: entries apart (the structure readable alone, B-reps
  shared by name, binary assets — textures, a thumbnail — beside them), a standard every tool
  opens. Deflate in Rust (`zip` + `miniz_oxide`) builds for the web as well.

Decision record: [The .skraft file is a ZIP of JSON entries](../decisions/0039-file-format-zip-of-json-entries.md).

## Materials alone (`.skraftmat`)

The materials of a model, exported to be imported into another: **the same container**,
`content: "materials"`, holding a document without geometry (`Document::materials_fragment`:
its root empty, the materials, their images, every definition of properties) — as the
clipboard's fragments are documents ([0017](../decisions/0017-copy-paste-fragments-and-app-clipboard.md)).
`Document::materials_file` writes one (the Matières panel's export button: a menu, all the
materials or one, the file named after it),
`Document::read_file` reads either kind with what it holds (`from_file` refuses a
`.skraftmat`: `FileError::MaterialsOnly`). Imported (`Operation::ImportMaterials`, a graft):
a material equal to one of the model's is that one, the others added with their images, the
definitions of their properties taken in (renamed where they contradict,
[Properties](../model/properties.md#pastes-and-imports)). A `.skraftmat` opened — the panel's import
button, a double click — is imported into the model shown (a new one from the start page).
A library of materials is a folder of them.

Decision record: [Textures by box projection, materials alone in a .skraftmat](../decisions/0041-textures-and-materials-files.md).

## Extension and file associations

**Extension `.skraft`**, MIME type `application/x-skraft` (a subclass of `application/zip`
on Linux); **`.skraftmat`**, `application/x-skraft-materials`, likewise. The installers
associate them with the app (`packaging/`): exported UTIs `app.skraft.document` and
`app.skraft.materials` and their document types on macOS, a ProgId each on Windows, a
shared-mime-info type each and the `.desktop` entry's `MimeType` on Linux. A file double-clicked
reaches the app as an argument (Windows, Linux: a new instance) or as an Apple Event « open
documents » (macOS, launched or running: winit does not handle it, the app installs its own
handler when it will finish launching); either way it waits in a queue
(`apps/skraft/src/opened.rs`) and opens in a tab of its own at the next frame — a
`.skraftmat` imported into the model shown.

## Version 1 until the app is published

The format changes without a new version and older files are not read — no converter, no
reference files. Versioning and backward compatibility start with the first release.

Decision record: [File format version 1 until the first release](../decisions/0010-file-format-v1-until-release.md).

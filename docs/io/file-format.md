---
status: built
code: [crates/model/src/file.rs, crates/model/src/settings.rs, apps/skraft/src/system/file.rs, apps/skraft/src/system/opened.rs, packaging/]
---

# File format

What a `.skraft` file holds, how the operating systems hand one to the app, and the versioning
policy until version 0.1.0.

## Content

A **ZIP** (Deflate), whose entries are JSON — but the images:

```
model.skraft
├── manifest.json               {"format": "skraft", "version": 1, "content": "model", "generator": "Skraft 0.0.1"}
├── document.json               the document: its structure, readable
├── view.json                   how it was last seen (workspace, cameras), when there is something to say
├── breps/<name>.json           the B-reps it is made of
├── cache/<name>.json           the B-reps computed from them
├── textures/<name>.<png|jpg>   the images of the materials' textures, as they came (stored)
└── thumbnail.png               a picture of the model, for the start page (stored)
```

- **`manifest.json`**, read first: the format, its version (a newer one is refused:
  `FileError::TooNew`), what the file holds (`content`: `model`, or `materials` for a
  [`.skraftmat`](#materials-alone-skraftmat)) and the app that wrote it, for information.
- **`document.json`**: the serde serialisation of the `Document` — components, occurrences,
  the assemblies' [links](../model/bodies.md#links),
  the parts' bodies and body definitions with their profiles and `source`, the components' guides
  ([Tape measure and guides](../tools/tape-measure.md)), the drawing's presentations, sheets and compositions
  ([Drawing](../ui/drawing.md)), the catalog (`catalog`: its three tables, `materials`,
  `coverings`, `finishes`), the settings
  (`settings`; missing: default values). Its solids are the **names** of their entries. Indented,
  arrays of plain values on one line (`"transform": [1.0, 0.0, …]`): read with `unzip -p
  model.skraft document.json`.
- **`view.json`** (`FileView`): how the file was last seen, not part of the document — the
  workspace it was shown in ([Workspaces](../ui/workspaces.md#the-documents-workspace)), the 3D
  camera, and the drawing's active sheet and board camera ([Drawing](../ui/drawing.md)). Written
  at each save, beside the document, never in its history: changing it neither modifies the
  document nor is undone, so a view changed and closed without saving is not kept. Absent when
  it holds nothing (`Document::to_file` writes none); missing or unreadable, it never fails an
  opening (`Document::file_view`: the defaults). A camera that cannot be one is ignored, the
  model or the board then framed whole; a sheet that is gone is left out.
- **The B-reps**: one entry each, SolidCore's record as JSON (its own `version`: the geometry
  schema). An entry's name is 128 bits of the BLAKE3 hash of that JSON: a solid used twice is
  stored once. `breps/` holds those the document is made of — the solids of the leaf bodies
  without a profile (imported, booleans' results, stretched askew); `cache/` those computed
  from them, kept to open without computing them again — the parts' shapes (present exactly when
  a part is not its single positive body), the profiled bodies' and the groups' solids. A
  cache equal to a source is that source. A file is refused when a solid it names is missing,
  caches included: they are not computed again on opening. A group's cache comes with its
  **face map** (`faces`, `faces_of`: its faces' names and the fingerprint of the solid they
  name, in the document's JSON, [Body nodes](../model/bodies.md#body-nodes)).
- **The images** ([Textures](../render/look.md#textures)): one entry each, its name its id
  (128 bits of the BLAKE3 hash of its bytes) and its kind's extension, not compressed again;
  `document.json`'s `assets` names them with their kind (`"<id>": "jpeg"`). A file naming an
  image it does not have is refused.
- **`thumbnail.png`**: what the model looks like, for the start page's recent files
  ([File tabs](../ui/file-tabs.md#start-page)) and the Finder's Quick Look on macOS
  ([below](#extension-and-file-associations)). Drawn at each save by the viewport's renderer
  (`Viewport::file_picture`): a parallel iso view framed on the whole model as the library's
  thumbnails, 480 × 270 pixels, in the light look whatever the theme, on a clear ground — drawn
  on black then on white, each pixel's coverage read from their difference —, so that the card
  shows it on the view's ground of any palette and mode. When the save cannot draw it (the
  document not shown, nothing drawn yet, the web, which cannot wait for the GPU), the one the
  file carried is written again. Read alone (`Document::file_thumbnail`), without the rest of
  the file; absent, or past 4 MB, the file has no picture. Optional: older files open as
  they are.
- **No dates** in the ZIP (1980-01-01) and entries in a fixed order: the same document gives
  the same bytes.

Opening reads every entry, then the document; then `Document::check` checks its invariants
(ids, the shapes in step with their bodies, known materials). In the browser: download and
file upload.

### Untrusted files

A file may come from anyone. The size its ZIP declares for an entry only sizes a first
reservation, and reading stops with `FileError::TooLarge` past what the file may expand to: 256
times its size, at least 64 MiB, at most 4 GiB (1 GiB on the web). `view.json` stops at 1 MiB,
and is then ignored, as an unreadable view is.

**Why:** a forged size of a few bytes aborted the app, an allocation failure being impossible to
catch, and lost the other tabs; a few hundred kilobytes could fill the memory. Real files expand
less than 10 times, so the limit leaves them a wide margin.

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

## Drawing templates (`.skrafttpl`)

A drawing's template ([Drawing](../ui/drawing.md#templates)): **the same container**,
`content: "template"`, holding a document whose only content is its drawing — one presentation,
its sheets and the compositions they place, without views, their images (`Document::template_of`,
`template_file`). `from_file` refuses it (`FileError::TemplateOnly`); opened, it goes into the
library and makes a sheet, or a presentation when it holds several sheets.

## Extension and file associations

**Extension `.skraft`**, MIME type `application/x-skraft` (a subclass of `application/zip`
on Linux); **`.skraftmat`**, `application/x-skraft-materials`, likewise. The installers
associate them with the app (`packaging/`): exported UTIs `app.skraft.document` and
`app.skraft.materials` and their document types on macOS, a ProgId each on Windows, a
shared-mime-info type each and the `.desktop` entry's `MimeType` on Linux. A file double-clicked
reaches the app as an argument (Windows, Linux: a new instance) or as an Apple Event « open
documents » (macOS, launched or running: winit does not handle it, the app installs its own
handler when it will finish launching); either way it waits in a queue
(`apps/skraft/src/system/opened.rs`) and opens in a tab of its own at the next frame — a
`.skraftmat` imported into the model shown.

**Quick Look on macOS**: Skraft.app carries two extensions (`packaging/macos/quicklook/`,
built by `scripts/build-quicklook.sh`), for `app.skraft.document`: the thumbnail the Finder
draws in place of the icon, and the preview on Space (macOS 12 and later). Both show
`thumbnail.png`, found through the ZIP's central directory and read alone, stored as it is; a
file without it, or not written as the app writes it, keeps the document icon. They run
sandboxed, signed ad hoc with their entitlements before the app (never `--deep`, which would
sign them again without). Without a picture, a file has the document icon macOS draws
(`CFBundleTypeIconSystemGenerated`): a page badged with the app's icon, its extension under
it. Windows and Linux show the app's icon only.

**In the browser, the file pickers do not filter** (« Ouvrir », « Importer », materials
imported; an image for a texture still does): any file can be picked, and it is taken by its
extension like a file dropped on the window — opened, imported, or refused with the formats
the app takes. **Why:** iOS turns an `accept` extension into a system file type and drops
those it does not know, filtering on the others alone: a `.skp` some app on the device
declares left the `.step` files greyed out, and `.skraft`, which none declares, was picked
only because nothing was left to filter on.

## Version 1 until 0.1.0

The `0.0.x` versions are test builds, x going up by one at each build sent to testers. Between
them the format changes without a new version and older files are not read — no converter, no
reference files; a build that breaks older files says so in its changelog entry. Version 0.1.0
locks the format: versioning and backward compatibility start there.

Decision record: [File format version 1 until the first release](../decisions/0010-file-format-v1-until-release.md).

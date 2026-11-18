# Two levels of settings: the document's, the user's

- Status: accepted (2026-11-09; the view's background out of the document 2026-11-13)
- Design: [Application interface](../ui/interface.md#settings)

## Context

Decided on 2026-11-09, after SketchUp's styles: some settings belong to the model (how it is
displayed, its units), others to the person using the app (language, navigation, tools).

## Decision

- **Document settings** (`model::Settings`): display (grid, axes…) and units (length unit,
  decimals). Saved in the file; changing them is an operation (`Operation::SetSettings`), hence
  undoable, successive changes merged into a single step. Lengths stay stored in mm: the unit
  only serves for input and display.
- **User preferences** (`apps/skraft/src/prefs.rs`): language, navigation, the tools' settings
  and the settings of new models; a versioned JSON file of our own (`preferences.json` in the
  OS's configuration folder, `localStorage` in the browser), rewritten as soon as a preference
  changes. `#[serde(default)]` everywhere.
- 2026-11-13: the view's background is not a document setting but a token of the palette
  ([Rendering and look](../render/look.md)): a file never sets how the app looks.

## Alternatives considered

- The view's background as a document setting: taken out on 2026-11-13.

## Consequences

- An older or partial preferences file loads; a new setting needs no migration.
- A display toggle that belongs to the document (grid, axes, X-ray, hide the rest of the model)
  is undone as an operation and travels with the file; the Preferences' *Model* tab edits the
  open document's settings, with "Use for new models" / "Restore the default settings".

# Changelog

What changes in Skraft from one release to the next, for its users. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

An entry is written when a version is tagged (`/changelog` in Claude Code, from
`scripts/changelog.sh`); the release workflow publishes it as the GitHub Release's text.

## [0.0.2] - 2026-11-22

### Added
- **About Skraft**: a window with the version, the credits, the licenses and a link to the source code, in the **File** menu, the command palette and the macOS app menu.
- Drop a file on the window to open it (Skraft files, materials, templates) or import it (STEP, SketchUp).
- Browser version: a loading screen with progress, and the document's name in the tab.

### Changed
- **Parts list**: the column headers show the unit, and the columns widen to fit the decimals.
- **Navigator**: the right-click menu is reordered, and a row stays highlighted while its menu is open.
- Light themes: hovering no longer looks like a selection.

### Fixed
- Deleting the only solid inside a round or chamfer no longer crashes the app: it stays, empty, and shows an error.
- **Show the unit** now applies to the dimensions of drawings.
- Browser version: importing a SketchUp file no longer crashes the app.
- Safari: the mouse wheel zooms instead of orbiting.
- Preferences: the units example shows 5 or 6 decimals when they are chosen.

## [0.0.1] - 2026-11-22

First test version of Skraft, a simple and intuitive 3D modeller built on real solids, thanks
to the SolidCore kernel. Everything you draw is a part with its own material, gathered into
assemblies, its copies all changing together. It runs on macOS (Apple silicon and Intel),
Windows and Linux.

The file format is not final yet: files saved with this version may not open in a later test
version, so don't keep important work only in Skraft.

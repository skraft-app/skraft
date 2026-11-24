# Changelog

What changes in Skraft from one release to the next, for its users. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

An entry is written when a version is tagged (`/changelog` in Claude Code, from
`scripts/changelog.sh`); the release workflow publishes it as the GitHub Release's text.

## [0.0.4] - 2026-11-24

### Added
- **Imprint** is pointed: pick the parts to go through, then the parts that imprint them, as with **Subtract**, several at once. Copies of a part placed alike come along, so a frame of one rail placed four times is linked in two clicks, and a part may imprint its own copies.
- The solid tools and **Imprint** mark each part picked by its role, with an A or B badge at the point clicked: what receives the change is drawn selected, what makes it see-through as the body it becomes.
- The middle of two tracked points is shown as a violet dot, to snap on.

### Changed
- **Imprint** is much faster on dowels and straight or filleted tenons: milliseconds instead of seconds.
- **Imprint**: the clearances are named by opening and depth, with no extra depth by default, and the confirmation is laid out around a figure.
- Linked bodies are drawn with dashed edges and marked in the navigator. Moving, turning, copying or rounding one is refused with a tip naming its source, and a double-click on it selects that source.
- **Move** and **Rotate** keep the selection once done, so a part can be moved or turned in several goes; Esc lets it go.
- Hovering tints the faces only, and a selection rectangle shows the selection it will leave as it is drawn, Shift included.
- **Check for updates** moved from the **File** menu to a button in the About window.

### Fixed
- **Put Back** places copies that enter each other in a chain, which closes a frame round a rail moved in its plane.
- Stretching mirrored or taller parts, or only one copy of a part, keeps their mortises and grooves following their sources.
- The navigator tells for each copy whether it imprints, and hiding a part drops it from the selection.
- Right after a copy, typing x5 in the measurements box sets the count.

## [0.0.3] - 2026-11-24

### Added
- **Imprint**: a part that enters other parts cuts its whole shape into them, with a clearance all around and an extra depth. The machining follows the part when it moves or changes; a part moved off its machining is flagged, with **Put Back** or **Separate** to settle it, and two parts overlapping inside the same part are told.
- **Reorient**: change a part's axes (swap length and width, swap front and back, adapt to a face and an edge, move the origin) without moving it. The parts list reads length, width and thickness along these axes, and a texture's grain runs along the length.
- Drawing: a view can show its subject in perspective, and a view can be framed on a detail of its subject (turned, zoomed and slid right on the sheet, with **Frame View**).
- Drawing: a dialog chooses what a view shows, when it is added and later on.
- Skraft tells you in the title bar when a new version is out and opens its download page.
- A file reopens with the view it was saved with: the 3D camera and the drawing's active sheet.

### Changed
- Files holding a drawing view in perspective, made with an earlier version, no longer open.
- New boxes and prisms are born oriented: length along their longest side, thickness across their largest faces.
- **Library**: the components and the compositions are shown in one card, which lists what the workspace uses.
- **Materials**: the card is a catalog of tiles or rows, a material is edited in the entity info, and unused materials can be purged. Choosing a material no longer changes the tool.
- Drawing: the active sheet's presentation is outlined on the board.
- **Move** no longer has rotation grips: turning a part is the job of **Rotate**.
- **Rotate**: a dotted line joins a radius to the point aimed at off the plane, and only the last point tracked stays shown.
- Opening a textured file no longer freezes the window: each material shows its colour until its texture is loaded.

### Fixed
- **Make Unique** and **Explode** now work on parts and assemblies picked under the Parts scope.
- Rounds and chamfers: letting go of the last edge removes the round, and a size typed before the edges load is applied.
- Drawing: a view is redrawn when the model, a material or a texture it shows changes, and a PDF waits for the textures before being written.
- Links in the app (About Skraft, updates) now open in the browser on the desktop.
- Zoomed in close, faces behind the eye no longer sweep across the view; edges no longer thin out where they meet a face, and the axes always show over the edges they lie on.

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

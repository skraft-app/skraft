# Changelog

What changes in Skraft from one release to the next, for its users. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

An entry is written when a version is tagged (`/changelog` in Claude Code, from
`scripts/changelog.sh`); the release workflow publishes it as the GitHub Release's text.

## [0.0.8] - 2026-11-28

### Added
- **Export** (File menu, ⌘E): write the whole model or the selection as a STEP file, exact, or as a SketchUp file (release 2017 or 2026), with its materials, textures and units, curved faces smoothed.
- **Capture**: frame the view and save it as a PNG or JPEG image, at the ratio, size and background you choose; the image is drawn again at its size, not copied from the screen.
- Inferences find where the edges and faces of the solids cross each other, marked with an ×, and these crossings can be tracked like a vertex.
- The navigator lists assemblies before parts and groups before bodies; a button next to its search sorts each kind by name, in natural order (2 before 10). The parts list sorts names the same way.
- On a touch screen, a finger scrolls the lists of the panels; a long press then a move picks a row up to drag it.

### Changed
- Parts, free bodies, profiles, assemblies and groups have icons of their own, easier to tell apart in the navigator.
- The update prompt is a modal box like the save prompt, with its release notes formatted.
- Blank sheets are A4 landscape, as is the bundled template, and a new sheet takes the paper of the last one made.
- The gesture modifiers in the preferences are shown as key caps; the preferences icon is a gear.
- The view bar goes to the view's edge when the cards of its column end above it.

### Fixed
- **Measure** draws a guide parallel to a chamfer's edges.
- Library thumbnails are redrawn when the colour palette changes.
- A file that could not be written, such as a drawing's PDF, no longer says the materials could not be exported.
- In the web version on iPhone and iPad, **Import** no longer greys out STEP files.
- The web version tells Chrome users how to turn WebGPU back on when Chrome turned it off for their graphics card.

## [0.0.7] - 2026-11-27

### Added
- **Stretch** goes through arcs, rounds, holes and slanted faces: what crosses the cutter is scaled along the axis, so a circle becomes an ellipse and a hole goes oval. The default cutter keeps clear of holes, so a hole goes oval only where you put the cutter yourself.
- The navigator shows a part's material and finish side by side, then a mark for faces painted with a covering or a finish of their own, named in its tooltip.
- The finish of a library part can be changed in the entity info, even when the part is not placed in the model.
- A fillet takes the last radius typed by a double-click or with Shift, from one part to the next.
- The navigator selects as your system's lists do: a click selects a row alone, ⌘-click (Ctrl-click on Windows and Linux) adds or removes it, Shift-click selects the rows in between, and the arrow keys move the selection (Shift extends it) and fold or unfold rows. In the 3D view, Shift still adds or removes.

### Changed
- Colorations are now called **Finishes** and schemes **Sequences**.
- A tool's refusal shows as an error toast near where you work, instead of in the status bar. Warnings and errors go away after 10 seconds, unless they wait for a choice.
- A tall box keeps the rectangle you drew as its profile, as a tall prism or cylinder does.
- A body's sign in the navigator goes − → + → ∩ on each click, in the order of the drawing tools' option bar.
- Files made with an earlier version lose the finishes of their parts and faces, and no longer open when they hold a sequence.

### Fixed
- **Stretch** no longer refuses a cutter beside a concave arc, a round whose surface comes out of a boolean, or some edges lengthened along their curve.
- **Stretch** no longer halves a board stretched back across an arc it had already scaled, and stretches through a chamfer.
- **Stretch** refuses a handle between two cutters that would move nothing, instead of doing nothing silently.
- **Stretch** refuses a compression that would run a part's matter through itself, such as a hole near an end compressed past it, instead of making a broken part.
- A value typed with too many nested parentheses is refused instead of closing the app.
- The web version says plainly when the browser offers no graphics card to WebGPU (some Android phones), and stops its loading bar and the kernel's download.

## [0.0.6] - 2026-11-26

### Added
- Materials come in three families, each a tab of the **Materials** panel: **Materials** say what a part is made of (Generic, Solid Wood, Sheet, Profile), **Coverings** what a face is covered with (Generic, Veneer, Edge Banding), and **Colorations** colour a part or a face over them. A coloration is a **Coat**, covering or tinting at an intensity, or a **Scheme** that lays several coats in order.
- A component says whether it is **Made**, **Bought** or **Existing** (**Source** in the entity info): a bought one is a single row of the parts list (a drawer slide), an existing one has no row (a wall, a fridge).
- SketchUp import lays an instance's paint on its part as a material and a face's paint as a coloration, in the family and type given by OpenCutList when the material has one. STEP import reads the products marked as bought and their material designation.
- A colour box of its own: a hex code field, favourite colours kept from one session to the next, and copy and paste of a colour with a right click or ⌘C / ⌘V over it.
- A texture is turned by quarter turns with a ↻ button on its image, and shows turned as on the parts.
- The material of a library part can be changed in the entity info, even when the part is not placed in the model.
- A window too small for the app shows a message instead, asking to turn the device on a touch screen.
- The web version says on its loading screen why it cannot run when the browser has no WebGPU.

### Changed
- A part's material belongs to the part, not to one of its copies: painting an assembly paints every part under it, and painting only one copy makes it a part of its own.
- A material has a colour or a texture, not both: picking an image gives it the image's mean colour, editable again once the image is removed. Colorations have a colour only.
- The **Shortcuts** tab of the preferences is redesigned: a search line, the commands under their menu, a click on a row to type its new shortcut, ↺ to put the default back, and a warning on shortcuts used twice.
- The entity info lists a part's or assembly's rows first, then those of the selected occurrence; it no longer shows how many times a part or a material is used.
- The preferences and other app windows grow to fit their content.
- In Safari on iPhone and iPad, the browser's bars take the app's colours with less height lost; other browsers lose none.
- Files made with an earlier version no longer open when faces are painted, when a drawing places a composition, or when a view shows chosen occurrences. Materials painted on occurrences and textures of colorations are dropped from older files.

## [0.0.5] - 2026-11-25

### Added
- Skraft works (better) on touch screens, in the web version: on a phone to look at a model (a compact layout, the parts list and drawing board scrolled and pinched with the fingers), on a tablet to model. One finger turns the view, two slide and zoom; a long press is the right click or a selection rectangle; values are typed on the app's own keypad, and a button under the finger steps an operation back.
- The start page shows the recent files as cards with a picture of their model, saved with the file, and a large **New** button.
- On macOS, Quick Look shows a picture of a `.skraft` file, and Skraft's documents get icons of their own.
- **Interface zoom** in the **Interface** tab of the preferences; the zoom shows at the centre of the window when it changes.
- The web version can be added to a phone's home screen, with its icon, and shows its version while it loads.

### Changed
- The Windows installer and the macOS disk image have a look of their own.
- The **Journal** filters its events by two lists, what happened and who did it.
- The interface zoom set before this version is not kept: Skraft starts again at 100 %.

### Fixed
- **Select** no longer leaves a part lit when the cursor leaves the 3D view.

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

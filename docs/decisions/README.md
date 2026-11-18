# Decision records

Why Skraft is built the way it is: one record per structuring decision — its context, the
decision, the alternatives weighed and its consequences. A record is not rewritten: a later
decision supersedes it. The design pages ([../README.md](../README.md)) describe the present
state and link the records behind it.

| # | Decision | Status |
|---|---|---|
| 0001 | [The kernel runs off the UI thread, as stateless jobs](0001-asynchronous-kernel.md) | accepted (2026-11-09) |
| 0002 | [One inference engine, on the exact B-rep](0002-central-inference-engine.md) | accepted (2026-11-09) |
| 0003 | [Two levels of settings: the document's, the user's](0003-document-settings-and-user-preferences.md) | accepted (2026-11-09; the view's background out of the document 2026-11-13) |
| 0004 | [Render a part's final B-rep as one instanced mesh](0004-part-mesh-as-instanced-rendering-unit.md) | accepted (2026-11-09) |
| 0005 | [Every object is an occurrence of a part or an assembly](0005-parts-assemblies-occurrences.md) | accepted (2026-11-10) |
| 0006 | [A title bar instead of a menu strip; native menus hold File only](0006-title-bar-instead-of-menus.md) | accepted (2026-11-10; File-only menus 2026-11-13; view commands out 2026-11-17) |
| 0007 | [Procedural parts: a parametric blank and machinings anchored on it](0007-procedural-parts.md) | proposed (discussed on 2026-11-11, set aside: nothing decided nor built) |
| 0008 | [The Stretch tool is OpenCutList's Smart Reshape stretch](0008-stretch-after-ocl-smart-reshape.md) | accepted (2026-11-11) |
| 0009 | [Stretch rewrites the B-rep record, on the recipe](0009-stretch-rewrites-brep-record.md) | accepted (2026-11-11) |
| 0010 | [File format version 1 until the first release](0010-file-format-v1-until-release.md) | accepted (2026-11-12) |
| 0011 | [Per-occurrence costs: one GPU instance, lookups through indexes](0011-per-occurrence-costs-instances-and-indexes.md) | accepted (2026-11-12) |
| 0012 | [Import SketchUp models through a JSON export of their structure](0012-sketchup-import-through-json-export.md) | accepted (2026-11-12) |
| 0013 | [Cutters and locks as a typed Behavior on the component](0013-stretch-behavior-on-component.md) | accepted (2026-11-12) |
| 0014 | [Each open document holds its whole state; one kernel service and one renderer are shared](0014-documents-own-their-state.md) | accepted (2026-11-13) |
| 0015 | [A ribbon with workspaces holds the tools, as data with pins](0015-ribbon-with-workspaces.md) | accepted (2026-11-13) |
| 0016 | [The sketch lives in the part: a profile is kept by the body it makes](0016-sketch-lives-in-the-part.md) | accepted (2026-11-13) |
| 0017 | [Copy and paste through document fragments and an in-app clipboard with a token](0017-copy-paste-fragments-and-app-clipboard.md) | accepted (2026-11-14) |
| 0018 | [A part is positive and negative bodies](0018-part-bodies.md) | accepted (2026-11-14) |
| 0019 | [Profile edits keep the shape by sides and stored radii, without a solver](0019-profile-edits-keep-shape-without-solver.md) | accepted (2026-11-14) |
| 0020 | [One control protocol, several transports](0020-control-protocol-one-method-set.md) | accepted (2026-11-15) |
| 0021 | [Copies as a list of placements computed by the tool](0021-copies-as-placement-lists.md) | accepted (2026-11-15) |
| 0022 | [Quick tools hand back through a one-level return slot](0022-quick-tool-return-slot.md) | accepted (2026-11-15) |
| 0023 | [A turned body is a half profile turned a whole turn about the body's Z](0023-turned-body-half-profile-about-body-z.md) | accepted (2026-11-15) |
| 0024 | [Body definitions may be groups: a CSG tree without history](0024-body-groups.md) | accepted (2026-11-16) |
| 0025 | [A part may be empty](0025-empty-parts.md) | accepted (2026-11-16) |
| 0026 | [Navigation styles: presets of other programs, each gesture editable](0026-navigation-styles-presets.md) | accepted (2026-11-16) |
| 0027 | [Parts paste into an open part as bodies, bodies out of one as a new part](0027-paste-across-the-part-boundary.md) | accepted (2026-11-16) |
| 0028 | [One immediate drawer for everything drawn over the model](0028-one-immediate-drawer-for-overlays.md) | accepted (2026-11-17) |
| 0029 | [Selection scope « Parts »: a second selection, as paths](0029-selection-scope-parts-as-paths.md) | accepted (2026-11-17) |
| 0030 | [Automatic names are stored, numbered from #1](0030-stored-automatic-names.md) | accepted (2026-11-17) |
| 0031 | [Tools pick first, as OpenCutList's](0031-tools-select-first.md) | accepted (2026-11-17) |
| 0032 | [Dashes laid on the segment, in model units, the guides' on screen](0032-dashes-laid-on-the-segment.md) | accepted (2026-11-18) |
| 0033 | [The undo history is capped by memory](0033-history-memory-cap.md) | accepted (2026-11-18) |
| 0034 | [Section caps: colour from the back faces, extent from a stencil](0034-section-caps-from-back-faces-and-stencil.md) | accepted (2026-11-18) |
| 0035 | [Custom cursors through a winit fork](0035-custom-cursors-winit-fork.md) | accepted (2026-11-18) |

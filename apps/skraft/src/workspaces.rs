//! Workspaces (docs/ui/workspaces.md, decision 0047): each a profile of the editor — its
//! ribbon (tabs and the fixed group, drawn by [`crate::window::ribbon`]), its stage, the commands
//! available in it — and the document's: each document is shown in one, kept in its file
//! ([`skraft_model::FileView`]).
//!
//! A command is available in a workspace when its ribbon has it, it is one of the workspace's
//! own ([`Workspace::commands`]) or one of the common base ([`COMMON`], [`COMMON_PREFIXES`]).
//! Shortcuts and the command palette only reach the commands available in the workspace shown.

use egui::ImageSource;
use skraft_i18n::t;

use crate::kit::icons;
use crate::window::ribbon::{Group, Tab, group};

/// A workspace: a profile of the editor.
pub struct Workspace {
    pub id: &'static str,
    pub label: fn() -> String,
    /// What it is for, in a few words (under its name in the switcher's menu).
    pub about: fn() -> String,
    pub icon: ImageSource<'static>,
    /// Not finished: listed in debug builds only, marked as under way (decision 0047).
    pub under_way: bool,
    /// What takes the central area.
    pub stage: Stage,
    /// Its commands besides its ribbon's and the common base (a card's, a menu's).
    pub commands: &'static [&'static str],
    /// The side panels it shows until the user arranges its own (none: as the first
    /// workspace's), each in the first workspace's place (docs/ui/workspaces.md#panels).
    pub panels: Option<&'static [crate::window::dock::PanelId]>,
    /// The ribbon's group shown before the tab's own, on every tab (Selection).
    pub fixed: Group,
    pub tabs: &'static [Tab],
}

/// What takes the central area of a workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The 3D view, over the whole area, the cards floating over it.
    View,
    /// The parts list on the left (docs/ui/parts-list.md), the 3D view on the right, a
    /// separation to drag between them.
    Parts,
    /// A sheet of the drawing, alone (docs/ui/drawing.md).
    Sheet,
}

impl Stage {
    pub fn id(self) -> &'static str {
        match self {
            Stage::View => "view",
            Stage::Parts => "parts",
            Stage::Sheet => "sheet",
        }
    }
}

/// Available in every workspace: files, the app's own, the view and its navigation, the
/// debug menu…
pub const COMMON_PREFIXES: &[&str] = &["file.", "app.", "view.", "debug."];
/// …and the editing every workspace shares.
pub const COMMON: &[&str] = &[
    "edit.undo",
    "edit.redo",
    "edit.cut",
    "edit.copy",
    "edit.paste",
    "edit.paste_in_place",
    "edit.select_all",
    "edit.erase",
    "tool.select",
];

const STANDARD_VIEWS: &[&str] = &["view.zoom_extents", "view.top", "view.front", "view.right", "view.back", "view.left", "view.iso"];

/// Every workspace, the first the one a document opens in. Drawing and a cut list are planned
/// (decision 0047).
pub const WORKSPACES: &[Workspace] = &[
    Workspace {
        id: "modelling",
        label: || t!("ribbon.workspace.modelling"),
        about: || t!("ribbon.workspace.about.modelling"),
        icon: icons::MODELLING,
        under_way: false,
        stage: Stage::View,
        // The « Composants » card's.
        commands: &["components.purge"],
        panels: None,
        fixed: group("selection", || t!("ribbon.group.selection"), &["tool.select", "tool.erase"], &["tool.select", "tool.erase"]),
        tabs: &[
            Tab {
                id: "build",
                label: || t!("ribbon.tab.build"),
                groups: &[
                    group(
                        "draw",
                        || t!("ribbon.group.draw"),
                        &["tool.box", "tool.cylinder", "tool.prism", "tool.turn"],
                        &["tool.box", "tool.cylinder", "tool.prism", "tool.turn"],
                    ),
                    group(
                        "handle",
                        || t!("ribbon.group.handle"),
                        &["tool.move", "tool.rotate", "tool.copy_line", "tool.copy_grid", "tool.distribute"],
                        &["tool.move", "tool.rotate", "tool.copy_line", "tool.copy_grid", "tool.distribute"],
                    ),
                    group(
                        "reshape",
                        || t!("ribbon.group.reshape"),
                        &[
                            "tool.stretch",
                            "tool.reorient",
                            "tool.round",
                            "tool.chamfer",
                            "tool.union",
                            "tool.difference",
                            "tool.intersection",
                        ],
                        &[
                            "tool.stretch",
                            "tool.reorient",
                            "tool.round",
                            "tool.chamfer",
                            "tool.union",
                            "tool.difference",
                            "tool.intersection",
                        ],
                    ),
                    group("appearance", || t!("ribbon.group.appearance"), &["tool.paint", "tool.sample"], &["tool.paint", "tool.sample"]),
                ],
            },
            Tab {
                id: "assemble",
                label: || t!("ribbon.tab.assemble"),
                groups: &[group(
                    "components",
                    || t!("ribbon.group.components"),
                    &["edit.make_assembly", "edit.explode", "edit.make_unique", "edit.group_bodies", "edit.move_out", "edit.imprint"],
                    &["edit.make_assembly", "edit.explode", "edit.make_unique", "edit.group_bodies", "edit.move_out", "edit.imprint"],
                )],
            },
            Tab {
                id: "inspect",
                label: || t!("ribbon.tab.inspect"),
                groups: &[
                    group("measure", || t!("ribbon.group.measure"), &["tool.measure", "edit.delete_guides"], &["tool.measure"]),
                    group("section", || t!("ribbon.group.section"), &["tool.section", "view.section_clear"], &["tool.section"]),
                    group("view", || t!("ribbon.group.view"), STANDARD_VIEWS, &["view.zoom_extents"]),
                ],
            },
        ],
    },
    Workspace {
        id: "parts",
        label: || t!("ribbon.workspace.parts"),
        about: || t!("ribbon.workspace.about.parts"),
        icon: icons::PARTS_LIST,
        under_way: false,
        stage: Stage::Parts,
        commands: &[],
        // The table and the view alone.
        panels: Some(&[]),
        fixed: group("selection", || t!("ribbon.group.selection"), &["tool.select"], &["tool.select"]),
        tabs: &[Tab {
            id: "list",
            label: || t!("ribbon.tab.list"),
            groups: &[group("view", || t!("ribbon.group.view"), STANDARD_VIEWS, &["view.zoom_extents"])],
        }],
    },
    Workspace {
        id: "drawing",
        label: || t!("ribbon.workspace.drawing"),
        about: || t!("ribbon.workspace.about.drawing"),
        icon: icons::DRAWING,
        under_way: false,
        stage: Stage::Sheet,
        commands: &["drawing.save_template", "drawing.edit_composition"],
        // The sheet, the entity info to set the view picked, the navigator of its tree, the
        // compositions to place.
        panels: Some(&[
            crate::window::dock::PanelId::EntityInfo,
            crate::window::dock::PanelId::Navigator,
            crate::window::dock::PanelId::Compositions,
        ]),
        fixed: group("selection", || t!("ribbon.group.selection"), &["tool.select", "edit.erase"], &["tool.select", "edit.erase"]),
        tabs: &[Tab {
            id: "sheet",
            label: || t!("ribbon.tab.sheet"),
            groups: &[
                group(
                    "presentation",
                    || t!("ribbon.group.presentation"),
                    &["drawing.new_presentation", "drawing.new_sheet", "drawing.export_pdf"],
                    &["drawing.new_presentation", "drawing.new_sheet", "drawing.export_pdf"],
                ),
                group(
                    "views",
                    || t!("ribbon.group.view"),
                    &["drawing.add_view", "drawing.project_view", "drawing.frame_view"],
                    &["drawing.add_view", "drawing.project_view", "drawing.frame_view"],
                ),
                group(
                    "draw",
                    || t!("ribbon.group.draw"),
                    &["drawing.tool_text", "drawing.tool_rect", "drawing.tool_circle", "drawing.tool_line"],
                    &["drawing.tool_text", "drawing.tool_rect", "drawing.tool_circle", "drawing.tool_line"],
                ),
                group(
                    "arrange",
                    || t!("ribbon.group.arrange"),
                    &["drawing.make_composition", "drawing.explode", "drawing.make_unique"],
                    &["drawing.make_composition", "drawing.explode", "drawing.make_unique"],
                ),
                group(
                    "dimension",
                    || t!("ribbon.group.dimension"),
                    &["drawing.dim_linear", "drawing.dim_radius", "drawing.dim_diameter", "drawing.dim_angle"],
                    &["drawing.dim_linear", "drawing.dim_radius", "drawing.dim_diameter", "drawing.dim_angle"],
                ),
                group(
                    "style",
                    || t!("ribbon.group.style"),
                    &["drawing.style_lines", "drawing.style_rendered", "drawing.style_hybrid"],
                    &["drawing.style_lines", "drawing.style_rendered", "drawing.style_hybrid"],
                ),
            ],
        }],
    },
];

/// The workspaces this build lists: an unfinished one in debug builds only.
pub fn listed() -> impl Iterator<Item = &'static Workspace> {
    WORKSPACES.iter().filter(|w| !w.under_way || cfg!(debug_assertions))
}

/// The workspace `id` if this build lists it, else the first one (a new document, a file
/// naming none or one unknown here).
pub fn find(id: Option<&str>) -> &'static Workspace {
    listed().find(|w| Some(w.id) == id).unwrap_or(&WORKSPACES[0])
}

/// Is `a` available wherever `b` is, in one workspace at least? (Two commands may share a
/// shortcut when no workspace has both.)
pub fn share(a: &str, b: &str) -> bool {
    WORKSPACES.iter().any(|w| w.has(a) && w.has(b))
}

impl Workspace {
    /// The tab `id`, or the first one.
    pub fn tab(&self, id: Option<&str>) -> &'static Tab {
        let tabs: &'static [Tab] = self.tabs;
        tabs.iter().find(|t| Some(t.id) == id).unwrap_or(&tabs[0])
    }

    /// Is the command `id` available in it?
    pub fn has(&self, id: &str) -> bool {
        // A panel of sheets, out of the common « view. » base.
        if id == "view.compositions" {
            return self.stage == Stage::Sheet;
        }
        COMMON.contains(&id)
            || COMMON_PREFIXES.iter().any(|p| id.starts_with(p))
            || self.commands.contains(&id)
            || std::iter::once(&self.fixed).chain(self.tabs.iter().flat_map(|t| t.groups)).any(|g| g.commands.contains(&id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{self, COMMANDS};

    #[test]
    fn the_first_workspace_is_finished_and_ids_are_unique() {
        assert!(!WORKSPACES[0].under_way, "a document opens in it: listed in every build");
        let mut ids: Vec<_> = WORKSPACES.iter().map(|w| w.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), WORKSPACES.len());
        assert_eq!(find(None).id, WORKSPACES[0].id);
        assert_eq!(find(Some("gone")).id, WORKSPACES[0].id);
        assert!(listed().any(|w| w.id == WORKSPACES[0].id));
    }

    #[test]
    fn the_parts_list_and_the_drawing_are_listed_in_every_build() {
        for id in ["parts", "drawing"] {
            let w = WORKSPACES.iter().find(|w| w.id == id).expect("a workspace");
            assert!(!w.under_way, "{id}: a release lists only what is not under way");
        }
    }

    #[test]
    fn every_workspace_has_a_tab() {
        // The ribbon always shows its tab row (docs/ui/ribbon.md#layout).
        for w in WORKSPACES {
            assert!(!w.tabs.is_empty(), "{} has no tab", w.id);
        }
    }

    #[test]
    fn workspaces_name_commands_of_the_registry() {
        for id in COMMON.iter().chain(WORKSPACES.iter().flat_map(|w| w.commands)) {
            assert!(commands::find(id).is_some(), "{id} is not a command");
        }
        for p in COMMON_PREFIXES {
            assert!(COMMANDS.iter().any(|c| c.id.starts_with(p)), "{p}: no command");
        }
    }

    #[test]
    fn modelling_has_every_command_but_other_workspaces_own() {
        // Every command is reachable somewhere; Modelling has all but what only another
        // workspace does (the drawing's sheets).
        let m = find(Some("modelling"));
        let own = |id: &str| WORKSPACES.iter().any(|w| w.id != m.id && w.has(id) && !COMMON.contains(&id));
        let missing: Vec<_> = COMMANDS.iter().map(|c| c.id).filter(|id| !m.has(id) && !own(id)).collect();
        assert!(missing.is_empty(), "in no workspace: {missing:?}");
        assert!(!m.has("drawing.add_view") && find(Some("drawing")).has("drawing.add_view"));
        assert!(share("tool.box", "edit.undo"));
        assert!(!m.has("nope"));
    }
}

pub(crate) mod common;
pub(crate) mod drawing;
pub(crate) mod modelling;
pub(crate) mod parts;

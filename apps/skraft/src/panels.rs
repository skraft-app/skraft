//! Side panels: the entity info, the materials and the outliner, the tool option bar and the
//! menu items, all drawn from the command registry's state
//! (docs/ui/commands.md).

use glam::{DAffine3, DVec3};
use skraft_i18n::{t, tp};
use skraft_model::{
    BodiesTo, ComponentKind, Document, EntityPath, FastMap, FastSet, GuideShape, Id, Nature, Occurrence, Operation, PropTarget, Scope,
    Sign, Units, measure,
};
use skraft_tools::{Editor, OptionValue, ToolKind, ToolOption};

use crate::commands::Menu;
use crate::icons;
use crate::style::Tip;
use crate::widgets::{
    Field, Fold, OPTION_HEIGHT, RENAMING, Segment, TreeLine, band_color, cut_label, field_stroke, framed_field, hidden_color, linked_mark,
    option_button, paint_drag_ghost, rename_field, segmented, set_drag_ghost, start_rename, tree_line, tree_row, warning,
};

mod props;

/// What a panel asks the app to do (applied after drawing, so the panels only read).
pub enum Action {
    /// A command of the registry, by id.
    Command(&'static str),
    /// The control channel failed to start: try again (the title bar's mark).
    RemoteRetry,
    /// An outliner's click: selects or deselects, Shift (`true`) adding to the selection.
    SelectPath(EntityPath, bool),
    /// A row of the tree drop-up hovered (none: left), or clicked (docs/tools/selection.md).
    TreeHover(Option<usize>),
    TreePick(usize),
    /// The outliner's first row, the model, clicked: every assembly and part edited left,
    /// nothing selected.
    SelectModel,
    /// A component chosen in the library (docs/ui/components-library.md).
    ChooseComponent(Id),
    /// One more occurrence of a component carried at the cursor, dragged out of its row (the
    /// view follows the drag, docs/ui/components-library.md#placing).
    PlaceComponent(Id),
    /// A card of definitions as a list (true) or a grid (docs/ui/catalog.md).
    CardList(crate::catalog::Card, bool),
    /// The library's menu: its occurrences selected, the selection replaced by it, deleted.
    SelectOccurrences(Id),
    ReplaceSelection(Id),
    DeleteComponent(Id),
    Apply(Operation),
    /// A key for the active tool, from a button (the validation's ✓ Enter, ✗ Esc,
    /// docs/ui/interface.md#steps-bar).
    ToolKey(skraft_tools::Key),
    /// An option of the active tool changed in its bar.
    ToolOption(&'static str, OptionValue),
    /// A length option of the active tool typed in its field of the bar.
    ToolLength(&'static str, String),
    /// A material chosen in its card (none: « Aucune »): what the paint tool paints with, the
    /// tool left as it is (docs/ui/catalog.md).
    ChooseMaterial(Option<Id>),
    /// Paint with this material (none: remove paint): the paint tool starts.
    PaintWith(Option<Id>),
    /// Pick an image for this material's texture (docs/render/look.md#textures).
    PickTexture(Id),
    /// Export the document's materials (`.skraftmat`): one, or all (`None`); import some
    /// (docs/io/file-format.md).
    ExportMaterials(Option<Id>),
    ImportMaterials,
    /// What changes to the open part reach (docs/model/data-model.md).
    PartScope(Scope),
    /// Whether shapes drawn in the open part add material or take it away (docs/model/bodies.md).
    BodySign(Sign),
    /// The body of the open part hovered in the outliner, if any (its part open on it, the
    /// bodies shown).
    HoverMachining(Option<Id>),
    /// A body of a part chosen in the outliner (docs/model/bodies.md#body-groups): the part, and the group
    /// the path names, opened; the body selected or deselected, Shift (`true`) adding it.
    SelectBody(EntityPath, Id, bool),
    /// The body node `Id` of the part (or group) open: opened, its tool started on its edges
    /// (docs/tools/rounds.md).
    EditBlend(Id),
    /// A body's « Profil » row chosen in the outliner: its part (and group) opened, its profile
    /// edited.
    EditProfile(EntityPath, Id),
    /// The occurrence hovered in the outliner, if any.
    HoverOutliner(Option<EntityPath>),
    /// Hide the occurrence (true) or show it again: the outliner's eye.
    SetHidden(EntityPath, bool),
    /// The ribbon's own state: tab, pins, labels (docs/ui/ribbon.md).
    Ribbon(crate::ribbon::Change),
    /// The shown document into workspace `id` (decision 0047).
    Workspace(&'static str),
    /// The parts `paths` (from the root) selected: a row of the parts list (docs/ui/parts-list.md).
    SelectParts(Vec<EntityPath>),
    /// A tool started from the quick tools' card, on what was just made (docs/tools/quick-tools.md).
    QuickTool(&'static str),
    /// A command of the quick tools' card pinned, or unpinned.
    QuickPin(&'static str),
    /// A command of the view bar pinned, or unpinned (docs/ui/view-bar.md).
    ViewPin(&'static str),
    /// A pinned command dragged before another (none: at the end), in the view bar or in the
    /// quick tools' card.
    ViewMove(&'static str, Option<&'static str>),
    /// The view bar's grip dragged to the other side.
    ViewBarSide(crate::dock::Dock),
    QuickMove(&'static str, Option<&'static str>),
    /// On the file tabs (docs/ui/file-tabs.md).
    Tab(crate::tabs::TabAction),
}

/// A command as shown now: its label, whether it can run, its state, its shortcut.
#[derive(Clone, Debug)]
pub struct CommandView {
    pub id: &'static str,
    pub label: String,
    pub menu: Menu,
    pub group: bool,
    pub enabled: bool,
    /// A toggle's state, or whether a tool is the active one.
    pub checked: Option<bool>,
    /// A tool waiting under the active one, which hands back to it (the tool stack).
    pub waiting: bool,
    /// As shown on this platform.
    pub shortcut: Option<String>,
}

impl CommandView {
    /// Its tool tip: the label and the shortcut.
    pub fn tip(&self) -> String {
        match &self.shortcut {
            Some(s) => t!("toolbar.with_shortcut", label = self.label, shortcut = s),
            None => self.label.clone(),
        }
    }
}

/// The delete key's name (Backspace works too).
pub fn delete_key() -> String {
    if skraft_i18n::mac() { "⌫".to_owned() } else { t!("key.delete") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_dragged_go_to_another_level_of_their_part_not_into_themselves() {
        let part = EntityPath::new(Vec::new(), Id(5));
        // Bodies 6 and 7 of the group 9 dragged.
        let drag = BodiesDrag { from: part.clone().in_group(vec![Id(9)]), bodies: vec![Id(6), Id(7)] };
        assert_eq!(drag.to(true, &part, Vec::new()), Some(BodiesTo::Level(Vec::new())), "to the part's root");
        assert_eq!(drag.to(true, &part, vec![Id(9), Id(8)]), Some(BodiesTo::Level(vec![Id(9), Id(8)])), "into a group beside them");
        assert_eq!(drag.to(true, &part, vec![Id(9)]), None, "where they lie");
        assert_eq!(drag.to(true, &part, vec![Id(9), Id(7)]), None, "into one of them");
        let other = EntityPath::new(Vec::new(), Id(4));
        assert_eq!(drag.to(false, &other, vec![Id(3)]), Some(BodiesTo::Part(other.clone().in_group(vec![Id(3)]))), "into another part");
    }

    /// The top of the text drawn in a navigator row: its name, or the field renaming it.
    fn text_top(ctx: &egui::Context, renaming: bool) -> f32 {
        let mut top = f32::NAN;
        // The field takes the focus on its first frame, then settles.
        for time in 0..3 {
            let input = egui::RawInput { time: Some(time as f64), ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                let height = ui.spacing().interact_size.y;
                tree_row(ui, height, false, false, false, |ui| match renaming {
                    true => drop(rename_field(ui, egui::Id::unique("rename_test"), "Solide 1")),
                    false => drop(ui.add(egui::Label::new("Solide 1").selectable(false))),
                });
            });
            output.textures_delta.clear();
            top = output.shapes.iter().find_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.pos.y) } else { None }).unwrap();
        }
        top
    }

    /// The navigator's rows under the search `text`: (depth, occurrence, body, dimmed).
    fn filtered(e: &Editor, text: &str) -> Vec<(usize, Id, Option<Id>, bool)> {
        let doc = e.document();
        let matcher = skraft_tools::search::Matcher::new(text);
        let mut holds = FastMap::default();
        let mut rows = Vec::new();
        Filtered { matcher: &matcher, holds: &mut holds, doc }.rows(doc.root(), &mut Vec::new(), &mut rows, false, 0);
        rows.iter().map(|r| (r.path.len(), r.occurrence.id, r.body.as_ref().map(|b| b.body.id), r.dim)).collect()
    }

    #[test]
    fn a_search_keeps_what_matches_and_the_rows_above_it() {
        use glam::DVec3;
        use skraft_model::Primitive;
        let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
        let root = e.document().root();
        let add = |e: &mut Editor, x: f64| {
            let p = Primitive::Box { origin: DVec3::new(x, 0.0, 0.0), size: DVec3::splat(10.0) };
            e.apply(Operation::AddPrimitive { context: root, primitive: p, placement: DAffine3::IDENTITY, material: None })
                .unwrap()
                .unwrap()
                .created[0]
        };
        let (a, b, c) = (add(&mut e, 0.0), add(&mut e, 100.0), add(&mut e, 200.0));
        let g = e.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().unwrap().created[0];
        let pa = e.document().resolve(&EntityPath::new(vec![g], a)).unwrap().component;
        e.apply(Operation::RenameComponent { component: pa, name: "Étagère".into() }).unwrap();

        // A part deep in the assembly: the assembly above it, attenuated, then it.
        let rows = filtered(&e, "etag");
        assert_eq!(rows, vec![(0, g, None, true), (1, a, None, false)]);
        assert!(rows.iter().all(|r| r.1 != b && r.1 != c));
        // Bodies: under their part, by their definition's name.
        let pc = e.document().resolve(&EntityPath::root(c)).unwrap().component;
        let part = e.document().part(pc).unwrap();
        let body = part.def_of(&part.bodies[0]).name.clone();
        let rows = filtered(&e, &body);
        assert!(rows.iter().any(|r| r.2.is_some() && !r.3), "{rows:?}");
        assert!(rows.iter().filter(|r| r.2.is_none()).all(|r| r.3), "the parts and assembly above, attenuated");
    }

    #[test]
    fn the_rename_field_keeps_the_name_where_the_row_had_it() {
        let ctx = egui::Context::default();
        crate::style::install_fonts(&ctx);
        let label = text_top(&ctx, false);
        assert!(label > 0.0, "the row's name centred in it: {label}");
        assert_eq!(text_top(&ctx, true), label);
    }
}

mod drawing_info;
mod info;
mod material;
mod options;
mod outliner;

pub(crate) use drawing_info::*;
pub(crate) use info::*;
pub(crate) use material::*;
pub(crate) use options::*;
pub(crate) use outliner::*;

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
    Field, Fold, OPTION_HEIGHT, RENAMING, Segment, band_color, cut_label, field_stroke, fold_toggle, framed_field, hidden_color,
    option_button, paint_drag_ghost, rename_field, segmented, set_drag_ghost, start_rename, tree_row, warning,
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

/// Whether the context card has anything to show (docs/ui/interface.md#context-card): the
/// selection's scope of a tool that picks parts, or the open part's scope.
pub fn has_context_options(editor: &Editor) -> bool {
    shows_part_scope(editor) || editor.tool_options().iter().any(is_context_option) || shows_open_node(editor)
}

/// A round or a chamfer open, another tool on: the card says it, and how its edges are changed
/// (docs/tools/rounds.md).
fn shows_open_node(editor: &Editor) -> bool {
    !editor.blending() && editor.open_node().is_some()
}

/// Whether the tool's card has anything to show: the active tool's own options, or an open
/// part's « Solide ».
pub fn has_tool_options(editor: &Editor) -> bool {
    shows_body_sign(editor) || editor.tool_options().iter().any(|o| !is_context_option(o))
}

/// An option of the context's, not the tool's: the selection's scope, which the editor adds to
/// every tool that picks parts (docs/tools/selection.md#selection-scope).
fn is_context_option(o: &ToolOption) -> bool {
    o.key == skraft_tools::tools::PICK_SCOPE
}

/// The open part's scope, only for a part placed more than once (docs/model/data-model.md): for one alone,
/// « Celui-ci » and « Tous » are the same.
fn shows_part_scope(editor: &Editor) -> bool {
    (editor.open_part().is_some() && editor.open_part_copies() > 1) || deep_scope(editor)
}

/// Parts selected at any depth through an assembly placed more than once, by a tool that
/// changes them (« Parts », docs/tools/selection.md): « only this one » makes the assemblies along unique.
fn deep_scope(editor: &Editor) -> bool {
    editor.open_part().is_none()
        && matches!(
            editor.tool_kind(),
            ToolKind::Select
                | ToolKind::Move
                | ToolKind::Rotate
                | ToolKind::Erase
                | ToolKind::Paint
                | ToolKind::CopyLine
                | ToolKind::CopyGrid
                | ToolKind::Distribute
        )
        && editor.deep_shared()
}

/// The open part's « Solide », while a shape is drawn in it (docs/model/bodies.md).
fn shows_body_sign(editor: &Editor) -> bool {
    editor.open_part().is_some() && matches!(editor.tool_kind(), ToolKind::Box | ToolKind::Cylinder | ToolKind::Prism | ToolKind::Turn)
}

/// A group's name before its buttons, after a separator but for the first of its card: grey, a
/// caption — its buttons are what the eye looks for.
fn option_group(ui: &mut egui::Ui, first: &mut bool, label: &str, tip: &str) {
    if !std::mem::take(first) {
        ui.separator();
    }
    ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color())).tip(tip);
}

/// The context card (docs/ui/interface.md#context-card), left of the tool's: what a pick takes
/// (the selection's scope, docs/tools/selection.md), then which copies a change reaches (the
/// open part's scope, docs/model/opening-a-part.md) — the same for every tool that shows them.
pub fn context_options(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let options = editor.tool_options();
    ui.horizontal(|ui| {
        // The row as high as its buttons from the start: the first label is centered on them.
        ui.set_min_height(OPTION_HEIGHT);
        let mut first = true;
        for o in options.iter().filter(|o| is_context_option(o)) {
            choice_option(ui, &mut first, o, actions);
        }
        // The node open: its name and size, its edges picked again from here.
        if shows_open_node(editor)
            && let Some((node, name, op)) = editor.open_node()
        {
            option_group(ui, &mut first, &name, &t!("option_bar.node_open_tip"));
            let size = editor.document().settings().units.format(op.size);
            let size = match op.kind {
                skraft_model::kernel::names::Blend::Round => t!("panel.outliner.round_size", size = size),
                skraft_model::kernel::names::Blend::Chamfer => t!("panel.outliner.chamfer_size", size = size),
            };
            ui.label(size);
            if ui.button(t!("option_bar.node_edit")).tip(t!("option_bar.node_edit_tip")).clicked() {
                actions.push(Action::EditBlend(node));
            }
        }
        if shows_part_scope(editor) {
            option_group(ui, &mut first, &t!("tool.boolean.option.scope"), &t!("option_bar.part_scope_tip"));
            let all = if deep_scope(editor) {
                t!("tool.boolean.option.all")
            } else {
                t!("tool.boolean.option.all_count", count = editor.open_part_copies())
            };
            let scope = editor.part_scope();
            let scopes = [
                (Scope::All, Segment::new(&all, &t!("tool.boolean.option.all_tip"), Some("scope_all"), scope == Scope::All)),
                (
                    Scope::This,
                    Segment::new(
                        &t!("tool.boolean.option.this"),
                        &t!("tool.boolean.option.this_tip"),
                        Some("scope_this"),
                        scope == Scope::This,
                    ),
                ),
            ];
            let (values, segments): (Vec<_>, Vec<_>) = scopes.into_iter().unzip();
            if let Some(i) = segmented(ui, egui::Id::unique("option_bar_part_scope"), &segments, |i| {
                ["option:part_scope:all", "option:part_scope:this"][i].into()
            }) {
                actions.push(Action::PartScope(values[i]));
            }
        }
    });
}

/// The active tool's card (docs/ui/interface.md): its choices, each a segmented control after
/// its name, then its lone toggles together in an "Options" group, all drawn like the ribbon's
/// buttons. An open part puts its « Solide » first (docs/model/bodies.md).
pub fn tool_options(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let options: Vec<_> = editor.tool_options().into_iter().filter(|o| !is_context_option(o)).collect();
    ui.horizontal(|ui| {
        ui.set_min_height(OPTION_HEIGHT);
        let mut first = true;
        if shows_body_sign(editor) {
            option_group(ui, &mut first, &t!("option_bar.body_sign"), &t!("option_bar.body_sign_tip"));
            let sign = editor.body_sign();
            let signs = [
                (
                    Sign::Negative,
                    Segment::new(&t!("option_bar.remove"), &t!("option_bar.remove_tip"), Some("subtract"), sign == Sign::Negative),
                ),
                (Sign::Positive, Segment::new(&t!("option_bar.add"), &t!("option_bar.add_tip"), Some("union"), sign == Sign::Positive)),
                // An intersection (docs/decisions/0045-intersection-a-third-sign.md).
                (
                    Sign::Intersect,
                    Segment::new(&t!("option_bar.intersect"), &t!("option_bar.intersect_tip"), Some("intersect"), sign == Sign::Intersect),
                ),
            ];
            let (values, segments): (Vec<_>, Vec<_>) = signs.into_iter().unzip();
            if let Some(i) = segmented(ui, egui::Id::unique("option_bar_body_sign"), &segments, |i| {
                ["option:body_sign:negative", "option:body_sign:positive", "option:body_sign:intersect"][i].into()
            }) {
                actions.push(Action::BodySign(values[i]));
            }
        }
        for o in &options {
            let OptionValue::Length(v) = o.value else { continue };
            option_group(ui, &mut first, &o.label, &o.tip);
            if let Some(text) = length_option(ui, o.key, &o.tip, v, &editor.document().settings().units) {
                actions.push(Action::ToolLength(o.key, text));
            }
        }
        for o in &options {
            choice_option(ui, &mut first, o, actions);
        }
        let toggles: Vec<_> =
            options.iter().filter_map(|o| if let OptionValue::Toggle(on) = o.value { Some((o, on)) } else { None }).collect();
        if toggles.is_empty() {
            return;
        }
        option_group(ui, &mut first, &t!("option_bar.options"), &t!("option_bar.options_tip"));
        // Side by side like the ribbon's buttons.
        ui.spacing_mut().item_spacing.x = crate::ribbon::BUTTON_GAP;
        for (o, on) in toggles {
            if option_button(ui, &format!("option:{}", o.key), on, &o.label, &o.tip, o.icon, o.swatch) {
                actions.push(Action::ToolOption(o.key, OptionValue::Toggle(!on)));
            }
        }
    });
}

/// A choice option: its name, then its values as a segmented control (nothing for another kind).
fn choice_option(ui: &mut egui::Ui, first: &mut bool, o: &ToolOption, actions: &mut Vec<Action>) {
    let OptionValue::Choice(current) = &o.value else { return };
    option_group(ui, first, &o.label, &o.tip);
    let segments: Vec<_> = o.choices.iter().map(|c| Segment::new(&c.label, &c.tip, c.icon, current == c.value)).collect();
    if let Some(c) =
        segmented(ui, egui::Id::unique(("option_bar", o.key)), &segments, |i| format!("option:{}:{}", o.key, o.choices[i].value))
    {
        actions.push(Action::ToolOption(o.key, OptionValue::Choice(o.choices[c].value.to_owned())));
    }
}

/// A length option's value, after its group's name: a [`framed_field`], lit when the value is
/// not zero. Esc or a click away puts the value back. The text typed, when applied.
fn length_option(ui: &mut egui::Ui, key: &str, tip: &str, value: f64, units: &Units) -> Option<String> {
    let shown = units.format(value);
    let exact = format!("{}{}", units.exact_number(value), units.suffix());
    let field = Field {
        hint: None,
        tip: Some(tip),
        width: 80.0,
        align: egui::Align2::RIGHT_CENTER,
        lit: value != 0.0,
        addon: None,
        blur_applies: false,
        empty_applies: false,
        handle: Some(format!("option:{key}")),
    };
    framed_field(ui, egui::Id::unique(("length_option", key)), &shown, &exact, field)
}

/// The delete key's name (Backspace works too).
pub fn delete_key() -> String {
    if skraft_i18n::mac() { "⌫".to_owned() } else { t!("key.delete") }
}

// ---- outliner ----------------------------------------------------------------------------------

fn occurrence_icon(doc: &Document, o: &Occurrence) -> egui::ImageSource<'static> {
    if doc.part(o.component).is_ok() { icons::BOX } else { icons::COMPONENT }
}

/// The name of an instance — an occurrence, a body — in its navigator row, its colour `color`,
/// on `back`, as [`crate::instance_name`] writes it (own name, then the definition's — `def`:
/// its component's, its body definition's — between brackets; automatic, `auto`, attenuated
/// unless the row is selected). Its response, and where its own name ends and the
/// definition's starts (halfway across the space between them) where it has one.
fn instance_label(
    ui: &mut egui::Ui,
    own: &str,
    def: &str,
    auto: bool,
    color: egui::Color32,
    selected: bool,
    back: egui::Color32,
) -> (egui::Response, Option<f32>) {
    let weak = if selected { color } else { color.lerp_to_gamma(back, weak_share(ui.visuals())) };
    let size = egui::TextStyle::Body.resolve(ui.style()).size;
    let job = crate::instance_name::job(own, def, auto, crate::instance_name::NameInk { size, ink: color, weak });
    let r = ui.add(egui::Label::new(job).selectable(false).truncate());
    let split = (!own.is_empty()).then(|| r.rect.min.x + crate::instance_name::split(ui, own, size));
    (r, split)
}

/// What the outliner keeps of a document between frames, in the document itself (closed with
/// it, never written to disk): the rows unfolded, the ids reborn, the active path last seen.
#[derive(Default)]
pub struct OutlinerState {
    /// The keys of the rows unfolded (a row missing: folded).
    open: FastSet<egui::Id>,
    births: Births,
    active: Option<ActivePath>,
    /// Its search (docs/ui/components-library.md#search), and whether a definition holds a
    /// row it lets through, by definition — kept for the document's revision and the search
    /// it was found for.
    pub search: String,
    holds: Option<(u64, String, Holds)>,
}

/// Whether a definition holds a row a search lets through, by definition.
type Holds = FastMap<Id, bool>;

/// One line of the outliner: an occurrence at some depth of the tree, or a body of the part it
/// places.
struct OutlinerRow<'d> {
    /// The occurrences above it.
    path: Vec<Id>,
    occurrence: &'d Occurrence,
    /// Its folding state, for an assembly, a part, a group.
    fold: Option<Fold>,
    /// An assembly above it is hidden: it is not drawn either.
    hidden_above: bool,
    /// The body this line shows, of the part `occurrence` places: none for the occurrence's own.
    body: Option<BodyRow<'d>>,
    /// Shown under a search only for what it holds (it does not match): attenuated.
    dim: bool,
}

/// A body in the outliner, under its part (docs/model/bodies.md#body-groups).
struct BodyRow<'d> {
    part: &'d skraft_model::Part,
    body: &'d skraft_model::Body,
    /// The groups it lies in, the part's own body first: a click opens them.
    group: Vec<Id>,
    /// Its siblings, itself included: the bodies of the part or group it lies in.
    siblings: &'d [skraft_model::Body],
    /// The line is the body's profile's, under it, not the body's own.
    profile: bool,
}

/// The icon of a body's nature (docs/model/bodies.md#body-names).
fn nature_icon(nature: Nature) -> egui::ImageSource<'static> {
    match nature {
        Nature::Extrusion => icons::EXTRUSION,
        Nature::Revolution => icons::REVOLUTION,
        Nature::Solid => icons::SOLID,
        Nature::Group => icons::GROUP,
        Nature::Round => icons::ROUND,
        Nature::Chamfer => icons::CHAMFER,
    }
}

/// How far the weak text lies from the text towards the background, 0 to 1.
pub(crate) fn weak_share(v: &egui::Visuals) -> f32 {
    let (text, weak, back) = (v.text_color(), v.weak_text_color(), v.window_fill);
    let d = |a: egui::Color32, b: egui::Color32| {
        (a.r() as f32 - b.r() as f32).abs() + (a.g() as f32 - b.g() as f32).abs() + (a.b() as f32 - b.b() as f32).abs()
    };
    let span = d(text, back);
    if span > 0.0 { (d(text, weak) / span).clamp(0.0, 1.0) } else { 0.0 }
}

/// What the interface calls the body `b` of `p`: its definition's name (docs/model/bodies.md#body-names),
/// automatic or not.
fn body_label(p: &skraft_model::Part, b: &skraft_model::Body) -> String {
    p.def_of(b).name.clone()
}

impl BodyRow<'_> {
    fn label(&self) -> String {
        body_label(self.part, self.body)
    }

    fn icon(&self) -> egui::ImageSource<'static> {
        nature_icon(self.part.def_of(self.body).nature())
    }
}

/// The outliner, in `area`: the search field `search::GAP` under `line` (the bottom of the
/// card's title bar line), the model (named `name`, its file's) on the first row, then the
/// tree; only the rows in view are laid out (a model can hold tens of thousands of
/// occurrences).
pub fn outliner(
    ui: &mut egui::Ui,
    editor: &Editor,
    state: &mut OutlinerState,
    name: &str,
    actions: &mut Vec<Action>,
    area: egui::ScrollArea,
    (body, line): (f32, f32),
) {
    let doc = editor.document();
    // The search field over the whole width, by name only (docs/ui/components-library.md#search),
    // as far from the title bar as the « Composants » card's from its tabs; the tree in the
    // height left under it, its first row as far again (the item spacing and the scroll
    // area's top margin below).
    let top = ui.cursor().top();
    ui.add_space((line + crate::search::GAP - ui.cursor().top()).max(0.0));
    let salt = ui.make_persistent_id("outliner_search");
    crate::search::field(ui, salt, "outliner", &mut state.search, |_, _| {});
    let left = (body - (ui.cursor().top() - top)).max(0.0);
    let area = area.max_height(left).min_scrolled_height(left);
    let height = ui.spacing().interact_size.y;
    // Room above the first row and below the last for their backgrounds, which spill half the
    // rows' spacing beyond them (`tree_row`): the scroll area clips to its content and margin,
    // so the first row selected was shorter than the others.
    let mut margin = ui.spacing().scroll.content_margin;
    let gap = (ui.spacing().item_spacing.y / 2.0).ceil() as i8;
    margin.top = margin.top.max(gap);
    margin.bottom = margin.bottom.max(gap);
    let area = area.content_margin(margin);
    // The ids handed out again since the last frame reborn (an empty model too: undone
    // to it, then made again).
    if state.births.next_id != doc.next_id() {
        state.births.update(doc.next_id());
    }
    if doc.root_assembly().occurrences().is_empty() {
        area.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            model_row(ui, editor, name, height, actions);
        });
        actions.push(Action::HoverMachining(None));
        return;
    }
    // The active path gone up (or elsewhere): what was open below folded.
    let active = ActivePath::of(editor);
    if state.active.as_ref() != Some(&active)
        && let Some(before) = state.active.replace(active.clone())
    {
        before.fold_left(&state.births, &mut state.open, &active);
    }
    let mut rows = Vec::new();
    let matcher = skraft_tools::search::Matcher::new(&state.search);
    if matcher.is_active() {
        // Under a search: every row it lets through and the rows above them, whatever is
        // folded (the folds left as they are).
        let revision = editor.history().revision();
        if state.holds.as_ref().is_none_or(|(r, f, _)| *r != revision || *f != state.search) {
            state.holds = Some((revision, state.search.clone(), FastMap::default()));
        }
        let Some((_, _, holds)) = state.holds.as_mut() else { return };
        let mut f = Filtered { matcher: &matcher, holds, doc };
        f.rows(doc.root(), &mut Vec::new(), &mut rows, false, 0);
    } else {
        flatten(&state.births, &mut state.open, editor, doc, doc.root(), &active.occurrences, &mut Vec::new(), &mut rows, false, 0);
    }
    if rows.is_empty() && matcher.is_active() {
        area.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.weak(t!("search.no_match"));
        });
        actions.push(Action::HoverMachining(None));
        return;
    }
    let folds = &mut state.open;
    let salt = ui.make_persistent_id("outliner_folds");
    let selection: std::collections::HashSet<Id> = editor.selection().iter().copied().collect();
    let text = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let on_selection = ui.visuals().selection.stroke.color;
    let mut over = None;
    let mut over_body = None;
    // The body whose profile is edited: its « Profil » row the one selected.
    let editing = editor.profile_body();
    // The components in error at any depth, each found once (definitions are shared).
    let mut errors = FastMap::default();
    // Row 0 the model's, the tree's after it.
    area.show_rows(ui, height, rows.len() + 1, |ui, range| {
        ui.set_min_width(ui.available_width());
        if range.start == 0 {
            model_row(ui, editor, name, height, actions);
        }
        for row in rows.drain(range.start.saturating_sub(1)..range.end.saturating_sub(1)) {
            match &row.body {
                Some(b) if b.profile => {
                    profile_row(ui, editor, &row, height, editing, actions);
                    continue;
                }
                Some(_) => {
                    body_row(ui, editor, row, height, (salt, folds), actions, &mut over_body);
                    continue;
                }
                None => {}
            }
            let o = row.occurrence;
            // The part open: selected while none of its bodies is (a body selected is marked on
            // its own row, not its part's).
            // The part open: marked open when it is the level open, else (a group open in it) only
            // as on the way to it, its name in the accent's colour (docs/model/opening-a-part.md).
            let open_here = editor.open_part() == Some(o.id) && editor.context() == row.path.as_slice();
            let open = open_here && editor.open_group().is_empty() && selection.is_empty() && editing.is_none();
            let on_path = open_here && !editor.open_group().is_empty();
            let selected = (editor.context() == row.path.as_slice() && (selection.contains(&o.id) || open))
                || editor.deep_selection().iter().any(|p| p.instances == row.path && p.entity == o.id);
            // Hidden, itself or by an assembly above: greyed.
            let greyed = o.hidden || row.hidden_above;
            let tint = if selected || on_path {
                on_selection
            } else if greyed {
                hidden_color(ui.visuals(), false)
            } else if row.dim {
                weak
            } else {
                text
            };
            // A part in several pieces, or empty: an error to correct (docs/model/bodies.md). A body failed
            // in it, at any depth, or a part in error in an assembly: ⚠ too, its name as it is
            // (the error is below, seen once unfolded).
            let part = doc.part(o.component).ok();
            let pieces = part.map_or(1, |p| p.pieces());
            let broken = pieces != 1;
            let inner = !broken && in_error(doc, o.component, &mut errors, 0);
            // A name edited in place, from a double-click, or F2 while it is the one occurrence
            // selected: the occurrence's own, shown first, where it has one and the double-click
            // is on it (F2: where it has one), else its component's (« Instance <Composant> »).
            let rename = ui.make_persistent_id(("outliner_rename", &row.path, o.id));
            let rename_occurrence = rename.with("occurrence");
            let renaming = match ui.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(RENAMING))) {
                Some(id) if id == rename => Some(false),
                Some(id) if id == rename_occurrence => Some(true),
                _ => None,
            };
            let name = doc.component(o.component).map(|c| c.name.clone()).unwrap_or_default();
            let (response, (eye, renamed, split)) = tree_row(ui, height, selected, false, false, |ui| {
                // The toggle button (or, for a part, its room) one indent per level, so all
                // the icons of a level line up.
                let spacing = ui.spacing().item_spacing.x;
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(ui.spacing().indent * row.path.len() as f32);
                match row.fold {
                    Some(fold) => fold_toggle(ui, salt, fold, folds),
                    None => ui.add_space(ui.spacing().indent),
                }
                ui.spacing_mut().item_spacing.x = spacing;
                ui.add(egui::Image::new(occurrence_icon(doc, o)).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
                // The eye first, at the right end (open while shown, struck through once
                // hidden), then the name in the room left, truncated with "…" so a long name
                // never pushes the eye out.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (icon, tip) =
                        if o.hidden { (icons::EYE_OFF, t!("panel.outliner.show")) } else { (icons::EYE, t!("panel.outliner.hide")) };
                    let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(if selected { tint } else { weak });
                    let eye = ui.add(egui::Button::image(img).frame(false)).tip(tip).clicked();
                    // Its own material, if any: a dot of its colour left of the eye, a column of
                    // its own so the names stay aligned.
                    if let Some(m) = o.material.and_then(|m| doc.material(m).ok()) {
                        material_dot(ui, m, greyed);
                    }
                    let renamed = ui
                        .with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if broken || inner {
                                ui.add(warning(ui));
                            }
                            let color = if broken { ui.visuals().error_fg_color } else { tint };
                            // The name edited alone in its field, the whole row's width.
                            match renaming {
                                Some(true) => return (rename_field(ui, rename_occurrence, &o.name), None),
                                Some(false) => return (rename_field(ui, rename, &name), None),
                                None => {}
                            }
                            let auto = doc.component(o.component).is_ok_and(|c| !c.named);
                            let back = ui.visuals().window_fill;
                            let (_, split) = instance_label(ui, &o.name, &name, auto, color, selected, back);
                            (None, split)
                        })
                        .inner;
                    (eye, renamed.0, renamed.1)
                })
                .inner
            });
            let tip = match pieces {
                0 => Some(t!("panel.outliner.empty_part")),
                1 if inner && part.is_some() => Some(t!("panel.outliner.failed_inside_part")),
                1 if inner => Some(t!("panel.outliner.error_inside_assembly")),
                1 => None,
                _ => Some(t!("panel.outliner.pieces", count = pieces)),
            };
            let response = match tip {
                Some(tip) => response.tip(tip),
                None => response,
            };
            let path = EntityPath::new(row.path, o.id);
            // The whole row, its indent and toggle included.
            if ui.rect_contains_pointer(response.rect) {
                over = Some(path.clone());
            }
            crate::handles::publish(|| format!("outliner:{}", o.id.0), response.rect);
            // Bodies dragged onto their part's row: to its root (decision 0044).
            // Dragged (decision 0044): the selected occurrences of its assembly with it when it
            // is selected. Dropped on: a part takes bodies or parts in, at its root; an assembly
            // makes a part of bodies.
            let response = response.interact(egui::Sense::click_and_drag());
            let with: Vec<Id> = match selected && editor.context() == path.instances.as_slice() {
                true => doc
                    .context_of(&path.instances)
                    .and_then(|c| doc.assembly(c))
                    .map(|a| a.occurrences().iter().map(|x| x.id).filter(|id| selection.contains(id)).collect())
                    .unwrap_or_default(),
                false => vec![o.id],
            };
            if response.drag_started() {
                let shown = if o.name.is_empty() { name.clone() } else { o.name.to_string() };
                set_drag_ghost(ui.ctx(), occurrence_icon(doc, o), shown, with.len());
            }
            response.dnd_set_drag_payload(PartsDrag { instances: path.instances.clone(), occurrences: with });
            let (bodies_over, parts_over) = dragged_over(&response);
            if bodies_over.is_some() || parts_over.is_some() {
                let is_part = doc.part(o.component).is_ok();
                let target: DropTarget<'_> = match (bodies_over, parts_over) {
                    (Some(drag), _) if is_part => {
                        let same = same_part(doc, &drag.from.part_path(), &path);
                        drag.to(same, &path, Vec::new()).map(|to| (Mark::Into, Box::new(move |copy| drag.op(editor, to, copy)) as Box<_>))
                    }
                    (Some(drag), _) => {
                        let inside: Vec<Id> = path.instances.iter().copied().chain([o.id]).collect();
                        Some((Mark::Into, Box::new(move |copy| drag.op(editor, BodiesTo::NewPart(inside), copy)) as Box<_>))
                    }
                    (None, Some(drag)) if is_part => {
                        let at = path.clone();
                        drag.onto_part(doc, &at, Vec::new(), false).map(|_| {
                            (Mark::Into, Box::new(move |copy| drag.onto_part(doc, &at, Vec::new(), copy).expect("checked")) as Box<_>)
                        })
                    }
                    (None, Some(drag)) => {
                        let inside: Vec<Id> = path.instances.iter().copied().chain([o.id]).collect();
                        drag.onto_assembly(doc, inside.clone(), false)
                            .map(|_| (Mark::Into, Box::new(move |copy| drag.onto_assembly(doc, inside, copy).expect("checked")) as Box<_>))
                    }
                    _ => None,
                };
                drop_on_row(ui, &response, 0.0, target, actions);
            }
            // A component's name is never empty, and unique: an emptied one is kept, one taken
            // refused (the failure told); an occurrence's emptied is cleared.
            let alone = selected && selection.len() == 1;
            let on_occurrence = match (split, response.interact_pointer_pos()) {
                (Some(split), Some(at)) if response.double_clicked() => at.x < split,
                (split, _) => split.is_some(),
            };
            let key = if on_occurrence { rename_occurrence } else { rename };
            match (renaming, start_rename(ui, &response, renaming.is_some(), alone, key, renamed)) {
                (Some(true), Some(Some(name))) => {
                    let context = doc.context_of(&path.instances).unwrap_or(doc.root());
                    actions.push(Action::Apply(Operation::RenameOccurrence { context, occurrence: o.id, name }));
                }
                (_, Some(Some(name))) if !name.is_empty() => {
                    actions.push(Action::Apply(Operation::RenameComponent { component: o.component, name }));
                }
                _ => {}
            }
            let in_selection = (editor.open_part().is_none() && editor.context() == path.instances.as_slice() && selection.contains(&o.id))
                || editor.deep_selection().iter().any(|p| p.instances == path.instances && p.entity == o.id);
            let select = (!in_selection).then(|| Action::SelectPath(path.clone(), false));
            let own = (!o.name.is_empty()).then_some(rename_occurrence);
            // « Aplatir » on a part with a tree: the parts selected with it, when it is.
            let composed = part.is_some_and(|p| p.has_tree());
            row_menu(
                &response,
                doc.assembly(o.component).is_ok(),
                (rename, own),
                select,
                actions,
                |_, _| {},
                |ui, actions| {
                    if composed && flatten_button(ui) {
                        let targets: Vec<EntityPath> = match in_selection {
                            true => selection
                                .iter()
                                .map(|id| EntityPath::new(path.instances.clone(), *id))
                                .chain(editor.deep_selection().iter().cloned())
                                .filter(|q| doc.resolve_part(q).is_ok_and(|(_, p, _)| p.has_tree()))
                                .collect(),
                            false => vec![path.clone()],
                        };
                        let part_open = editor.open_part_path().is_some_and(|x| x.part_path() == path.part_path());
                        let scope = if part_open { editor.part_scope() } else { Scope::All };
                        let ops = targets.into_iter().map(|target| Operation::FlattenPart { target, scope }).collect();
                        actions.push(Action::Apply(Operation::Batch(ops)));
                    }
                },
            );
            if eye {
                actions.push(Action::SetHidden(path, !o.hidden));
            } else if response.clicked() {
                actions.push(Action::SelectPath(path, ui.input(|i| i.modifiers.shift)));
            }
        }
    });
    paint_drag_ghost(ui.ctx());
    actions.push(Action::HoverOutliner(over));
    actions.push(Action::HoverMachining(over_body));
}

/// A navigator row's material: a round dot of its colour, ringed so a pale one shows on the
/// band, faded on a hidden row (`greyed`); its name in the tip.
fn material_dot(ui: &mut egui::Ui, m: &skraft_model::Material, greyed: bool) {
    const SIZE: f32 = 10.0;
    let (r, response) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
    let [red, g, b] = m.color;
    let alpha = if greyed { 0.4 } else { 1.0 };
    let fill = egui::Color32::from_rgb(red, g, b).gamma_multiply(alpha);
    let ring = ui.visuals().widgets.noninteractive.fg_stroke.color.gamma_multiply(0.5 * alpha);
    ui.painter().circle(r.center(), SIZE / 2.0 - 0.5, fill, egui::Stroke::new(1.0, ring));
    response.tip(&m.name);
}

/// The model, the outliner's first row, at the list's left edge (no indent, no fold nor eye):
/// a click chooses it (`Editor::model_chosen`), highlighted then, the entity info showing it.
fn model_row(ui: &mut egui::Ui, editor: &Editor, name: &str, height: f32, actions: &mut Vec<Action>) {
    let selected = editor.model_chosen();
    let tint = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
    let (response, ()) = tree_row(ui, height, selected, false, false, |ui| {
        ui.add(egui::Image::new(icons::MODEL).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
        let font = crate::style::semibold(egui::TextStyle::Body.resolve(ui.style()).size);
        ui.add(egui::Label::new(egui::RichText::new(name).font(font).color(tint)).selectable(false).truncate());
    });
    // Bodies dropped on the model: a part of them at its root (decision 0044).
    let (bodies_over, parts_over) = dragged_over(&response);
    if let Some(drag) = bodies_over {
        let make = Box::new(move |copy| drag.op(editor, BodiesTo::NewPart(Vec::new()), copy));
        drop_on_row(ui, &response, 0.0, Some((Mark::Into, make)), actions);
    } else if let Some(drag) = parts_over {
        // Occurrences dropped on the model: to its root.
        let doc = editor.document();
        let target = drag
            .onto_assembly(doc, Vec::new(), false)
            .map(|_| (Mark::Into, Box::new(move |copy| drag.onto_assembly(doc, Vec::new(), copy).expect("checked")) as DropOperation<'_>));
        drop_on_row(ui, &response, 0.0, target, actions);
    }
    if response.tip(t!("panel.outliner.model_tip")).clicked() {
        actions.push(Action::SelectModel);
    }
}

/// Where the editing is (the outliner's active path): the assemblies edited and the part open,
/// the groups open in it, the body whose profile is edited.
#[derive(Clone, Debug, Default, PartialEq)]
struct ActivePath {
    occurrences: Vec<Id>,
    groups: Vec<Id>,
    profile: Option<Id>,
}

impl ActivePath {
    fn of(editor: &Editor) -> Self {
        let mut occurrences = editor.context().to_vec();
        let part = editor.open_part();
        occurrences.extend(part);
        let groups = if part.is_some() { editor.open_group().to_vec() } else { Vec::new() };
        Self { occurrences, groups, profile: editor.profile_body().filter(|_| part.is_some()) }
    }

    /// `self` left for `now`: the rows unfolded along `self` beyond what both share, folded
    /// (out of `folds`).
    fn fold_left(&self, births: &Births, folds: &mut FastSet<egui::Id>, now: &Self) {
        let mut fold = |id: egui::Id| {
            folds.remove(&id);
        };
        let shared = self.occurrences.iter().zip(&now.occurrences).take_while(|(a, b)| a == b).count();
        for k in shared..self.occurrences.len() {
            fold(births.occurrence_fold(&self.occurrences[..k], self.occurrences[k]));
        }
        // In the part open: its groups and the body whose profile was edited.
        let Some((&part, above)) = self.occurrences.split_last() else { return };
        let same_part = self.occurrences == now.occurrences;
        let shared = if same_part { self.groups.iter().zip(&now.groups).take_while(|(a, b)| a == b).count() } else { 0 };
        for k in shared..self.groups.len() {
            fold(births.body_fold((above, part), &self.groups[..k], self.groups[k]));
        }
        if let Some(body) = self.profile
            && !(same_part && self.groups == now.groups && now.profile == Some(body))
        {
            fold(births.body_fold((above, part), &self.groups, body));
        }
    }
}

/// The births of the ids handed out again: an undo takes the document's next id back, the
/// next things made take the ids of those undone. A row's fold is kept under its ids salted
/// with their births, so that a new thing does not open as the one undone was (a copy
/// unfolded, undone, made again).
#[derive(Clone, Debug, Default)]
struct Births {
    /// The document's next id when last seen.
    next_id: u64,
    /// The ids given back by undos, each with how many times.
    of: FastMap<Id, u32>,
}

impl Births {
    /// The document's next id now `next_id`: the ids above it, given back, reborn.
    fn update(&mut self, next_id: u64) {
        for id in next_id..self.next_id {
            *self.of.entry(Id(id)).or_default() += 1;
        }
        self.next_id = next_id;
    }

    /// The births of `ids`, folded into one salt (0 while no id was given back).
    fn salt<'a>(&self, ids: impl IntoIterator<Item = &'a Id>) -> u64 {
        if self.of.is_empty() {
            return 0;
        }
        ids.into_iter().fold(0, |h: u64, id| h.rotate_left(7) ^ u64::from(self.of.get(id).copied().unwrap_or(0)))
    }

    /// The fold of the occurrence `id` under the occurrences `path`.
    fn occurrence_fold(&self, path: &[Id], id: Id) -> egui::Id {
        egui::Id::unique(("outliner", path, id, self.salt(path.iter().chain([&id]))))
    }

    /// The fold of the body `id` in the groups `group` of the part of the occurrence `at`.
    fn body_fold(&self, at: (&[Id], Id), group: &[Id], id: Id) -> egui::Id {
        let salt = self.salt(at.0.iter().chain([&at.1]).chain(group).chain([&id]));
        egui::Id::unique(("outliner_body", at.0, at.1, group, id, salt))
    }
}

/// The rows of the tree under `assembly`, those of folded assemblies left out (`hidden_above`:
/// an occurrence above `assembly` is hidden).
#[allow(clippy::too_many_arguments)]
fn flatten<'d>(
    births: &Births,
    folds: &mut FastSet<egui::Id>,
    editor: &Editor,
    doc: &'d Document,
    assembly: Id,
    active: &[Id],
    path: &mut Vec<Id>,
    out: &mut Vec<OutlinerRow<'d>>,
    hidden_above: bool,
    depth: usize,
) {
    let Ok(a) = doc.assembly(assembly) else { return };
    if depth > skraft_model::MAX_DEPTH {
        return;
    }
    // Above the end of the active path: only its step at this level.
    let step = active.get(path.len()).copied().filter(|_| active.starts_with(path));
    for o in a.occurrences() {
        if step.is_some_and(|id| id != o.id) {
            continue;
        }
        if let Ok(p) = doc.part(o.component) {
            // Its bodies under it, a single one too.
            let key = births.occurrence_fold(path, o.id);
            // Open while the part is.
            let open = unfold(folds, key, editor.context() == path.as_slice() && editor.open_part() == Some(o.id));
            out.push(OutlinerRow {
                path: path.clone(),
                occurrence: o,
                fold: Some(Fold { key, open }),
                hidden_above,
                body: None,
                dim: false,
            });
            if open {
                let open_group = (editor.context() == path.as_slice() && editor.open_part() == Some(o.id)).then(|| editor.open_group());
                let row = |fold, group, body, siblings, profile| OutlinerRow {
                    path: path.clone(),
                    occurrence: o,
                    fold,
                    hidden_above,
                    body: Some(BodyRow { part: p, body, group, siblings, profile }),
                    dim: false,
                };
                let at = (path.as_slice(), o.id);
                // The profile edited, in the group open: its body unfolded.
                let open = Open { group: open_group.unwrap_or(&[]), profile: open_group.and(editor.profile_body()) };
                body_rows(births, folds, at, p, &p.bodies, &mut Vec::new(), open, &mut |r: BodyLine<'d>| {
                    out.push(row(r.0, r.1, r.2, r.3, r.4))
                });
            }
            continue;
        }
        if doc.assembly(o.component).is_err() {
            out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: None, hidden_above, body: None, dim: false });
            continue;
        }
        let key = births.occurrence_fold(path, o.id);
        // Open along the context being edited.
        let along = editor.context().len() > path.len() && editor.context().starts_with(path) && editor.context()[path.len()] == o.id;
        let open = unfold(folds, key, along);
        out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: Some(Fold { key, open }), hidden_above, body: None, dim: false });
        if open {
            path.push(o.id);
            flatten(births, folds, editor, doc, o.component, active, path, out, hidden_above || o.hidden, depth + 1);
            path.pop();
        }
    }
}

/// The navigator's rows under a search (docs/ui/components-library.md#search): the rows it lets
/// through — an occurrence by its name or its component's, a body by its name or its
/// definition's —, and the rows above them, attenuated when they do not pass themselves. No
/// folds: what holds a match is shown open, the rest closed.
struct Filtered<'a, 'd> {
    matcher: &'a skraft_tools::search::Matcher,
    /// Does a definition hold a row that passes?
    holds: &'a mut Holds,
    doc: &'d Document,
}

impl<'d> Filtered<'_, 'd> {
    /// Does the occurrence `o` pass itself?
    fn occurrence(&self, o: &Occurrence) -> bool {
        let Ok(c) = self.doc.component(o.component) else { return false };
        self.matcher.name([o.name.as_str(), c.name.as_str()])
    }

    /// Does the body `b` of `p` pass?
    fn body(&self, p: &skraft_model::Part, b: &skraft_model::Body) -> bool {
        self.matcher.name([b.name.as_str(), p.def_of(b).name.as_str()])
    }

    /// Does one of `bodies`, or a body in their groups, pass?
    fn holds_body(&self, p: &skraft_model::Part, bodies: &[skraft_model::Body], depth: usize) -> bool {
        depth <= skraft_model::MAX_DEPTH && bodies.iter().any(|b| self.body(p, b) || self.holds_body(p, p.def_of(b).children(), depth + 1))
    }

    /// Does the definition `c` hold a row that passes?
    fn holds(&mut self, c: Id, depth: usize) -> bool {
        if let Some(h) = self.holds.get(&c) {
            return *h;
        }
        let h = match self.doc.component(c).map(|c| &c.kind) {
            Ok(ComponentKind::Part(p)) => self.holds_body(p, &p.bodies, 0),
            Ok(ComponentKind::Assembly(a)) if depth <= skraft_model::MAX_DEPTH => {
                a.occurrences().iter().any(|o| self.occurrence(o) || self.holds(o.component, depth + 1))
            }
            _ => false,
        };
        self.holds.insert(c, h);
        h
    }

    /// The rows of the assembly `assembly` under `path`.
    fn rows(&mut self, assembly: Id, path: &mut Vec<Id>, out: &mut Vec<OutlinerRow<'d>>, hidden_above: bool, depth: usize) {
        let doc = self.doc;
        let Ok(a) = doc.assembly(assembly) else { return };
        if depth > skraft_model::MAX_DEPTH {
            return;
        }
        for o in a.occurrences() {
            let me = self.occurrence(o);
            let below = self.holds(o.component, depth + 1);
            if !me && !below {
                continue;
            }
            out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: None, hidden_above, body: None, dim: !me });
            if !below {
                continue;
            }
            if let Ok(p) = doc.part(o.component) {
                self.body_rows(p, &p.bodies, &mut Vec::new(), path, o, hidden_above, out);
            } else {
                path.push(o.id);
                self.rows(o.component, path, out, hidden_above || o.hidden, depth + 1);
                path.pop();
            }
        }
    }

    /// The rows of `bodies` (of the part `o` places, in the groups `group`) that pass, and the
    /// groups holding one.
    #[allow(clippy::too_many_arguments)]
    fn body_rows(
        &self,
        p: &'d skraft_model::Part,
        bodies: &'d [skraft_model::Body],
        group: &mut Vec<Id>,
        path: &[Id],
        o: &'d Occurrence,
        hidden_above: bool,
        out: &mut Vec<OutlinerRow<'d>>,
    ) {
        if group.len() > skraft_model::MAX_DEPTH {
            return;
        }
        for b in bodies {
            let me = self.body(p, b);
            let children = p.def_of(b).children();
            let below = self.holds_body(p, children, group.len() + 1);
            if !me && !below {
                continue;
            }
            let body = Some(BodyRow { part: p, body: b, group: group.clone(), siblings: bodies, profile: false });
            out.push(OutlinerRow { path: path.to_vec(), occurrence: o, fold: None, hidden_above, body, dim: !me });
            if below {
                group.push(b.id);
                self.body_rows(p, children, group, path, o, hidden_above, out);
                group.pop();
            }
        }
    }
}

/// Whether the row of fold `key` is open: unfolded in `folds`, or `forced` (along the active
/// path), then kept unfolded there (folded again as the active path leaves it, `fold_left`).
fn unfold(folds: &mut FastSet<egui::Id>, key: egui::Id, forced: bool) -> bool {
    if forced {
        folds.insert(key);
        true
    } else {
        folds.contains(&key)
    }
}

/// Is the component `id` in error, or does it hold one, at any depth: a part in several pieces
/// or empty, or holding a body failed; an assembly holding such a part. Remembered in `memo`,
/// `depth` assemblies down.
fn in_error(doc: &Document, id: Id, memo: &mut FastMap<Id, bool>, depth: usize) -> bool {
    if let Some(e) = memo.get(&id) {
        return *e;
    }
    let e = match doc.part(id) {
        Ok(p) => p.pieces() != 1 || holds_failure(p, &p.bodies, 0),
        Err(_) => {
            depth <= skraft_model::MAX_DEPTH
                && doc.assembly(id).is_ok_and(|a| a.occurrences().iter().any(|o| in_error(doc, o.component, memo, depth + 1)))
        }
    };
    memo.insert(id, e);
    e
}

/// Does one of `bodies` (of the part `p`, `depth` groups down) fail, or a body of a group among
/// them, at any depth?
fn holds_failure(p: &skraft_model::Part, bodies: &[skraft_model::Body], depth: usize) -> bool {
    depth <= skraft_model::MAX_DEPTH && bodies.iter().any(|b| p.is_failed(b.id) || holds_failure(p, p.def_of(b).children(), depth + 1))
}

/// A body row's parts: its folding state (a group's, a profiled body's), the groups it lies in,
/// the body, its siblings, whether the row is the body's profile's.
type BodyLine<'d> = (Option<Fold>, Vec<Id>, &'d skraft_model::Body, &'d [skraft_model::Body], bool);

/// What is open in a part, its rows unfolded along it: the group open (the bodies leading to
/// it), the body of that group whose profile is edited.
#[derive(Clone, Copy)]
struct Open<'a> {
    group: &'a [Id],
    profile: Option<Id>,
}

/// The rows of `bodies` (of the part `p` the occurrence `at` places, in the groups `group`), in
/// their order of creation (a row toggling its sign stays under the pointer);
/// under an open group, its children. The groups along `open` (the group open in the part) are
/// unfolded.
#[allow(clippy::too_many_arguments)]
fn body_rows<'d>(
    births: &Births,
    folds: &mut FastSet<egui::Id>,
    at: (&[Id], Id),
    p: &'d skraft_model::Part,
    bodies: &'d [skraft_model::Body],
    group: &mut Vec<Id>,
    open: Open,
    out: &mut dyn FnMut(BodyLine<'d>),
) {
    if group.len() > skraft_model::MAX_DEPTH {
        return;
    }
    // Above the end of the active path (the group open, the profile edited): only its step.
    let step = match open.group.get(group.len()) {
        Some(&id) if open.group.starts_with(group) => Some(id),
        _ => open.profile.filter(|_| open.group == group.as_slice()),
    };
    for b in bodies {
        if step.is_some_and(|id| id != b.id) {
            continue;
        }
        let children = p.def_of(b).children();
        let profiled = p.profile_of(b).is_some();
        if children.is_empty() && !profiled {
            out((None, group.clone(), b, bodies, false));
            continue;
        }
        let key = births.body_fold(at, group, b.id);
        // Along the group open, or its profile edited: unfolded.
        let edited = open.profile == Some(b.id) && open.group == group.as_slice();
        group.push(b.id);
        let opened = unfold(folds, key, edited || open.group.starts_with(group));
        group.pop();
        out((Some(Fold { key, open: opened }), group.clone(), b, bodies, false));
        if !opened {
            continue;
        }
        if profiled {
            out((None, group.clone(), b, bodies, true));
        } else {
            group.push(b.id);
            body_rows(births, folds, at, p, children, group, open, out);
            group.pop();
        }
    }
}

/// A body's « Profil » line, under it on the band of its part's bodies: selected while it is
/// edited (`editing`, in the part or group open), a click editing it (its part and group
/// opened).
fn profile_row(ui: &mut egui::Ui, editor: &Editor, row: &OutlinerRow, height: f32, editing: Option<Id>, actions: &mut Vec<Action>) {
    let Some(b) = &row.body else { return };
    let path = EntityPath::new(row.path.clone(), row.occurrence.id).in_group(b.group.clone());
    let selected = editor.open_part_path().as_ref() == Some(&path) && editing == Some(b.body.id);
    let v = ui.visuals();
    let tint = if selected {
        v.selection.stroke.color
    } else if row.occurrence.hidden || row.hidden_above {
        hidden_color(v, true)
    } else {
        v.text_color()
    };
    let (response, ()) = tree_row(ui, height, selected, false, true, |ui| {
        // Under its body's icon: one level further, its fold's room left.
        let spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(ui.spacing().indent * (row.path.len() + 3 + b.group.len()) as f32);
        ui.spacing_mut().item_spacing.x = spacing;
        ui.add(egui::Image::new(icons::PROFILE).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
        let label = egui::RichText::new(t!("panel.outliner.profile")).color(tint);
        ui.add(egui::Label::new(label).selectable(false).truncate());
    });
    if response.tip(t!("panel.outliner.profile_tip")).clicked() && !selected {
        actions.push(Action::EditProfile(path, b.body.id));
    }
}

/// One body's line of the outliner, on the band of its part's bodies: its fold for a group, its
/// nature's icon and name, its sign toggle at the right end; a click opens its part and selects
/// the part's own body it lies in (Shift: added). Its part open on its siblings, it is hovered
/// and selected with the view (`over`: the body hovered here). The toggle
/// flips its sign — the selected bodies' too when it is one of them, in its part or group
/// opened —, in every occurrence of the part unless it is open for this one alone
/// (« Celui-ci »).
#[allow(clippy::too_many_arguments)]
/// Bodies dragged in the navigator (decision 0044, docs/model/bodies.md#moving-bodies): the
/// level they lie in (its part's path and groups), which of its bodies.
#[derive(Clone, Debug)]
struct BodiesDrag {
    from: EntityPath,
    bodies: Vec<Id>,
}

impl BodiesDrag {
    /// Where bodies dropped at the level `level` of the part `part` places go, if they may: in
    /// their own part (`same`), another level of it, not where they lie nor inside one of them;
    /// in another part, into it.
    fn to(&self, same: bool, part: &EntityPath, level: Vec<Id>) -> Option<BodiesTo> {
        if !same {
            return Some(BodiesTo::Part(part.clone().in_group(level)));
        }
        (level != self.from.group && !level.iter().any(|g| self.bodies.contains(g))).then_some(BodiesTo::Level(level))
    }

    /// The move to `to`, with the scope of their part when it is open.
    fn op(&self, editor: &Editor, to: BodiesTo, copy: bool) -> Operation {
        let part_open = editor.open_part_path().is_some_and(|o| o.part_path() == self.from.part_path());
        let scope = if part_open { editor.part_scope() } else { Scope::All };
        Operation::MoveBodies { from: self.from.clone(), bodies: self.bodies.clone(), to, copy, scope }
    }
}

/// The operation a drop makes, told whether the copy modifier is held.
type DropOperation<'a> = Box<dyn FnOnce(bool) -> Operation + 'a>;
/// Where a drag may land on a row: how it is marked, and what dropping it there does.
type DropTarget<'a> = Option<(Mark, DropOperation<'a>)>;

/// Occurrences dragged in the navigator (decision 0044): the assembly they lie in (the
/// occurrences leading to it), which of its occurrences.
#[derive(Clone, Debug)]
struct PartsDrag {
    instances: Vec<Id>,
    occurrences: Vec<Id>,
}

impl PartsDrag {
    /// Into the part `part` places, at its level `level`, as its bodies: not a part they place,
    /// nor one inside them (an assembly dragged), nor themselves.
    fn onto_part(&self, doc: &Document, part: &EntityPath, level: Vec<Id>, copy: bool) -> Option<Operation> {
        let n = self.instances.len();
        let inside = part.instances.len() > n && part.instances[..n] == self.instances[..] && self.occurrences.contains(&part.instances[n]);
        let itself = part.instances == self.instances && self.occurrences.contains(&part.entity);
        let component = doc.resolve(part).ok()?.component;
        let context = doc.context_of(&self.instances).ok()?;
        let placed =
            doc.assembly(context).ok()?.occurrences().iter().filter(|o| self.occurrences.contains(&o.id)).any(|o| o.component == component);
        (!inside && !itself && !placed).then(|| Operation::MoveParts {
            instances: self.instances.clone(),
            occurrences: self.occurrences.clone(),
            to: part.clone().in_group(level),
            copy,
        })
    }
}

impl PartsDrag {
    /// Into the assembly `to` leads to (empty: the model): not the one they lie in, nor one of
    /// them or one they hold (decision 0044).
    fn onto_assembly(&self, doc: &Document, to: Vec<Id>, copy: bool) -> Option<Operation> {
        let (from, into) = (doc.context_of(&self.instances).ok()?, doc.context_of(&to).ok()?);
        let holds = doc
            .assembly(from)
            .ok()?
            .occurrences()
            .iter()
            .filter(|o| self.occurrences.contains(&o.id))
            .any(|o| doc.contains_component(o.component, into));
        (from != into && !holds).then(|| Operation::MoveOccurrences {
            instances: self.instances.clone(),
            occurrences: self.occurrences.clone(),
            to,
            copy,
        })
    }
}

/// Does the occurrence `a` place the same part as `b`?
fn same_part(doc: &Document, a: &EntityPath, b: &EntityPath) -> bool {
    match (doc.resolve(a), doc.resolve(b)) {
        (Ok(x), Ok(y)) => x.component == y.component,
        _ => false,
    }
}

/// The path of the group `id` lying in the groups `group`: the level its children are in.
fn inner_path(group: &[Id], id: Id) -> Vec<Id> {
    group.iter().copied().chain([id]).collect()
}

/// How a row shows where a drop on it goes.
#[derive(Clone, Copy)]
enum Mark {
    /// Into the row's group, part or assembly: the row framed.
    Into,
    /// Beside the row, in its level: a line above it (`true`) or under it.
    Beside(bool),
}

/// While something is dragged over a row (`response`, from `indent` on): `target` — how the
/// row shows where it would go and the operation (given whether the platform's copy key is
/// held), none when it may not go there — marked, the cursor; dropped, the operation.
fn drop_on_row(ui: &egui::Ui, response: &egui::Response, indent: f32, target: DropTarget<'_>, actions: &mut Vec<Action>) {
    let copy = crate::viewport::modifiers(ui.ctx(), ui.input(|i| i.modifiers)).copy;
    let Some((mark, op)) = target else {
        ui.ctx().set_cursor_icon(egui::CursorIcon::NoDrop);
        return;
    };
    let stroke = egui::Stroke::new(1.5, ui.visuals().selection.stroke.color);
    let r = response.rect;
    match mark {
        Mark::Into => {
            ui.painter().rect_stroke(r.shrink(1.0), 3.0, stroke, egui::StrokeKind::Inside);
        }
        Mark::Beside(above) => {
            let y = if above { r.top() } else { r.bottom() };
            ui.painter().hline(r.left() + indent..=r.right(), y, stroke);
        }
    }
    ui.ctx().set_cursor_icon(if copy { egui::CursorIcon::Copy } else { egui::CursorIcon::Grabbing });
    // Taken as the type it is: egui's takes the payload whatever its type, and keeps it only
    // when it is the one asked.
    let released = match response.dnd_hover_payload::<BodiesDrag>() {
        Some(_) => response.dnd_release_payload::<BodiesDrag>().is_some(),
        None => response.dnd_release_payload::<PartsDrag>().is_some(),
    };
    if released {
        actions.push(Action::Apply(op(copy)));
    }
}

/// Is something of the navigator dragged over `response`?
fn dragged_over(response: &egui::Response) -> (Option<std::sync::Arc<BodiesDrag>>, Option<std::sync::Arc<PartsDrag>>) {
    (response.dnd_hover_payload::<BodiesDrag>(), response.dnd_hover_payload::<PartsDrag>())
}

fn body_row(
    ui: &mut egui::Ui,
    editor: &Editor,
    row: OutlinerRow,
    height: f32,
    folds: (egui::Id, &mut FastSet<egui::Id>),
    actions: &mut Vec<Action>,
    over: &mut Option<Id>,
) {
    let Some(b) = &row.body else { return };
    let path = EntityPath::new(row.path.clone(), row.occurrence.id).in_group(b.group.clone());
    let open = editor.open_part_path().as_ref() == Some(&path);
    let shown = open && editor.showing_machinings();
    // A group (a node) open: its row marked open, as a part's is, while nothing in it is
    // selected; those on the way to it, their name in the accent's colour.
    let inner: Vec<Id> = b.group.iter().copied().chain([b.body.id]).collect();
    let open_path = editor.open_part_path().filter(|o| o.part_path() == path.part_path());
    let is_open = open_path.as_ref().is_some_and(|o| o.group == inner);
    let on_path = open_path.as_ref().is_some_and(|o| o.group.len() > inner.len() && o.group.starts_with(&inner));
    let open_mark = is_open && editor.selection().is_empty() && editor.profile_body().is_none();
    let selected = (shown && editor.selection().contains(&b.body.id)) || open_mark;
    let lit = shown && editor.hovered_machining() == Some(b.body.id);
    let greyed = row.occurrence.hidden || row.hidden_above;
    let v = ui.visuals();
    let weak = v.weak_text_color();
    let tint = if selected || on_path {
        v.selection.stroke.color
    } else if greyed {
        hidden_color(v, true)
    } else if row.dim {
        weak
    } else {
        v.text_color()
    };
    let failed = b.part.is_failed(b.body.id);
    // A group failed (empty, or its boolean failing): ⚠ as a part in error, an error to correct
    // (docs/model/bodies.md#empty-parts); one holding a body failed, at any depth: ⚠ too, its name as it is.
    let def = b.part.def_of(b.body);
    let inner = def.is_group() && !failed && holds_failure(b.part, def.children(), 0);
    let warned = (failed && def.is_group()) || inner;
    // A name edited in place (docs/model/bodies.md#body-names), from a double-click, or F2 while it is the
    // one body selected: the body's own, shown first, where it has one and the double-click is
    // on it (F2: where it has one), else its definition's (« Haut <Trou#1> »), as an
    // occurrence's row.
    let rename = ui.make_persistent_id(("outliner_rename", &row.path, row.occurrence.id, &b.group, b.body.id));
    let rename_own = rename.with("own");
    let renaming = match ui.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(RENAMING))) {
        Some(id) if id == rename => Some(false),
        Some(id) if id == rename_own => Some(true),
        _ => None,
    };
    let (response, (toggled, renamed, split)) = tree_row(ui, height, selected, lit, true, |ui| {
        let spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;
        // Under the part's icon: its level, plus one, plus the groups above.
        ui.add_space(ui.spacing().indent * (row.path.len() + 1 + b.group.len()) as f32);
        match row.fold {
            Some(fold) => fold_toggle(ui, folds.0, fold, folds.1),
            None => ui.add_space(ui.spacing().indent),
        }
        ui.spacing_mut().item_spacing.x = spacing;
        ui.add(egui::Image::new(b.icon()).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
        // The sign toggle first, at the right end where a part's row has its eye, then the
        // name in the room left.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon, tip) = match b.body.sign {
                Sign::Positive => (icons::UNION, t!("panel.outliner.sign_positive")),
                Sign::Negative => (icons::SUBTRACT, t!("panel.outliner.sign_negative")),
                Sign::Intersect => (icons::INTERSECT, t!("panel.outliner.sign_intersect")),
            };
            let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(if selected { tint } else { weak });
            let toggled = ui.add(egui::Button::image(img).frame(false)).tip(tip).clicked();
            let renamed = ui
                .with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if warned {
                        ui.add(warning(ui));
                    }
                    match renaming {
                        Some(true) => return (rename_field(ui, rename_own, &b.body.name), None),
                        Some(false) => return (rename_field(ui, rename, &b.label()), None),
                        None => {}
                    }
                    let color = if failed { ui.visuals().error_fg_color } else { tint };
                    let back = band_color(ui.visuals());
                    let (_, split) = instance_label(ui, &b.body.name, &b.label(), !def.named, color, selected, back);
                    // A body node's size after its name (docs/tools/rounds.md).
                    if let Some(op) = def.op().and_then(|o| o.blend()) {
                        let size = editor.document().settings().units.format(op.size);
                        let text = match op.kind {
                            skraft_model::kernel::names::Blend::Round => t!("panel.outliner.round_size", size = size),
                            skraft_model::kernel::names::Blend::Chamfer => t!("panel.outliner.chamfer_size", size = size),
                        };
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(text).color(weak));
                    }
                    (None, split)
                })
                .inner;
            (toggled, renamed.0, renamed.1)
        })
        .inner
    });
    let response = if let Some(r) = b.part.refusal_of(b.body.id) {
        response.tip(skraft_model::blend_refusal(r))
    } else if warned {
        let tip = match (inner, def.solid.is_empty()) {
            (true, _) => t!("panel.outliner.failed_inside_group"),
            (false, true) => t!("panel.outliner.empty_group"),
            (false, false) => t!("panel.outliner.failed_group"),
        };
        response.tip(tip)
    } else {
        response
    };
    if shown && ui.rect_contains_pointer(response.rect) {
        *over = Some(b.body.id);
    }
    // Dragged (decision 0044): the selected siblings with it when it is selected; dropped on, a
    // group's middle takes them in, its edges and a leaf put them beside it, in its level.
    crate::handles::publish(|| format!("outliner_body:{}", b.body.id.0), response.rect);
    let response = response.interact(egui::Sense::click_and_drag());
    let with: Vec<Id> = match selected {
        true => editor.selection().iter().copied().filter(|id| b.siblings.iter().any(|s| s.id == *id)).collect(),
        false => vec![b.body.id],
    };
    if response.drag_started() {
        let shown = if b.body.name.is_empty() { b.label() } else { b.body.name.to_string() };
        set_drag_ghost(ui.ctx(), b.icon(), shown, with.len());
    }
    response.dnd_set_drag_payload(BodiesDrag { from: path.clone(), bodies: with.clone() });
    let (bodies_over, parts_over) = dragged_over(&response);
    if bodies_over.is_some() || parts_over.is_some() {
        let doc = editor.document();
        let part = path.part_path();
        let r = response.rect;
        let y = ui.input(|i| i.pointer.hover_pos()).map_or(r.center().y, |p| p.y);
        let into = def.is_group() && (y - r.top()) > r.height() * 0.25 && (r.bottom() - y) > r.height() * 0.25;
        let (level, mark) = match into {
            true => (inner_path(&b.group, b.body.id), Mark::Into),
            false => (b.group.clone(), Mark::Beside(y < r.center().y)),
        };
        let target: DropTarget<'_> = match (bodies_over, parts_over) {
            (Some(drag), _) => {
                let same = same_part(doc, &drag.from.part_path(), &part);
                drag.to(same, &part, level).map(|to| (mark, Box::new(move |copy| drag.op(editor, to, copy)) as Box<_>))
            }
            (None, Some(drag)) => drag
                .onto_part(doc, &part, level.clone(), false)
                .map(|_| (mark, Box::new(move |copy| drag.onto_part(doc, &part, level, copy).expect("checked")) as Box<_>)),
            _ => None,
        };
        let indent = ui.spacing().indent * (row.path.len() + 1 + b.group.len()) as f32;
        drop_on_row(ui, &response, indent, target, actions);
    }
    let alone = selected && editor.selection().len() == 1;
    let on_own = match (split, response.interact_pointer_pos()) {
        (Some(split), Some(at)) if response.double_clicked() => at.x < split,
        (split, _) => split.is_some(),
    };
    let key = if on_own { rename_own } else { rename };
    // Its menu, with « Sortir du groupe » (« Sortir de la pièce », at its root).
    let select = (!(open && editor.selection().contains(&b.body.id))).then(|| Action::SelectBody(path.clone(), b.body.id, false));
    let own = (!b.body.name.is_empty()).then_some(rename_own);
    let is_group = def.is_group();
    let move_out = |ui: &mut egui::Ui, actions: &mut Vec<Action>| {
        ui.separator();
        let (label, to) = match b.group.split_last() {
            Some((_, up)) => (t!("menu.edit.move_out"), skraft_model::BodiesTo::Level(up.to_vec())),
            None => (t!("menu.edit.move_out_of_part"), skraft_model::BodiesTo::NewPart(path.instances.clone())),
        };
        let button = ui.button(label);
        crate::handles::publish(|| "outliner_menu:move_out".into(), button.rect);
        if button.clicked() {
            let part_open = editor.open_part_path().is_some_and(|o| o.part_path() == path.part_path());
            let scope = if part_open { editor.part_scope() } else { Scope::All };
            actions.push(Action::Apply(Operation::MoveBodies { from: path.clone(), bodies: with.clone(), to, copy: false, scope }));
        }
    };
    let flatten = |ui: &mut egui::Ui, actions: &mut Vec<Action>| {
        if is_group && flatten_button(ui) {
            let part_open = editor.open_part_path().is_some_and(|o| o.part_path() == path.part_path());
            let scope = if part_open { editor.part_scope() } else { Scope::All };
            actions.push(Action::Apply(Operation::FlattenBodies { target: path.clone(), bodies: with.clone(), scope }));
        }
    };
    row_menu(&response, is_group, (rename, own), select, actions, move_out, flatten);
    let part = row.occurrence.component;
    match (renaming, start_rename(ui, &response, renaming.is_some(), alone, key, renamed)) {
        (Some(true), Some(Some(name))) => actions.push(Action::Apply(Operation::RenameBody { part, body: b.body.id, name })),
        (_, Some(Some(name))) => actions.push(Action::Apply(Operation::RenameBodyDef { part, def: b.body.def, name })),
        _ => {}
    }
    if toggled {
        let sign = match b.body.sign {
            // + → − → ∩ → + (docs/decisions/0045-intersection-a-third-sign.md).
            Sign::Positive => Sign::Negative,
            Sign::Negative => Sign::Intersect,
            Sign::Intersect => Sign::Positive,
        };
        // Selected (in its part or group, opened): the selected siblings with it.
        let bodies = match selected {
            true => editor.selection().iter().copied().filter(|id| b.siblings.iter().any(|s| s.id == *id)).collect(),
            false => vec![b.body.id],
        };
        // Its part open (here or in one of its groups): the scope it is open with.
        let part_open = editor.open_part_path().is_some_and(|o| o.part_path() == path.part_path());
        let scope = if part_open { editor.part_scope() } else { Scope::All };
        actions.push(Action::Apply(Operation::SetSign { target: path, bodies, sign, scope }));
    } else if response.clicked() {
        actions.push(Action::SelectBody(path, b.body.id, ui.input(|i| i.modifiers.shift)));
    }
}

/// The menu's « Aplatir » (docs/model/bodies.md#flattening): clicked?
fn flatten_button(ui: &mut egui::Ui) -> bool {
    let button = ui.button(t!("common.flatten"));
    crate::handles::publish(|| "outliner_menu:flatten".into(), button.rect);
    button.clicked()
}

/// A navigator row's right-click menu: « Renommer » (the field a double-click opens, under the
/// key `rename`; with an instance name, under `own`, one entry for each name), what `move_out`
/// adds (with its own separator), then « Aplatir » (`flatten`), « Éclater » where the row can be
/// (`explodable`), « Effacer ». Opening it changes nothing; the commands act on the selection
/// when the row is in it, else on the row alone, `select` choosing it first (docs/ui/interface.md).
fn row_menu(
    response: &egui::Response,
    explodable: bool,
    (rename, own): (egui::Id, Option<egui::Id>),
    mut select: Option<Action>,
    actions: &mut Vec<Action>,
    move_out: impl FnOnce(&mut egui::Ui, &mut Vec<Action>),
    flatten: impl FnOnce(&mut egui::Ui, &mut Vec<Action>),
) {
    egui::Popup::context_menu(response).style(crate::style::menu).show(|ui| {
        let names = match own {
            Some(own) => vec![
                (t!("panel.outliner.rename_instance"), own, "rename_instance"),
                (t!("panel.outliner.rename_definition"), rename, "rename"),
            ],
            None => vec![(t!("panel.outliner.rename"), rename, "rename")],
        };
        for (label, key, handle) in names {
            let button = ui.button(label);
            crate::handles::publish(|| format!("outliner_menu:{handle}"), button.rect);
            if button.clicked() {
                ui.data_mut(|d| d.insert_temp(egui::Id::unique(RENAMING), key));
            }
        }
        move_out(ui, actions);
        ui.separator();
        flatten(ui, actions);
        if explodable {
            let button = ui.button(t!("menu.edit.explode"));
            crate::handles::publish(|| "outliner_menu:explode".into(), button.rect);
            if button.clicked() {
                actions.extend(select.take());
                actions.push(Action::Command("edit.explode"));
            }
        }
        let button = ui.button(t!("menu.edit.erase"));
        crate::handles::publish(|| "outliner_menu:delete".into(), button.rect);
        if button.clicked() {
            actions.extend(select.take());
            actions.push(Action::Command("edit.erase"));
        }
    });
}

// ---- entity info -------------------------------------------------------------------------------

/// A name, in a [`framed_field`] applied by a click away too. `width` is fixed by the caller:
/// an infinite width inside a `Grid` feeds back on the column width of the previous frame and
/// widens the side panel at every relayout.
fn text_field(ui: &mut egui::Ui, key: (Id, u8), current: &str, width: f32) -> Option<String> {
    name_field(ui, key, current, width, false)
}

/// A [`text_field`], its text emptied applying too when `empty` (to clear a name).
fn name_field(ui: &mut egui::Ui, key: (Id, u8), current: &str, width: f32, empty: bool) -> Option<String> {
    let field = Field {
        hint: None,
        tip: None,
        width,
        align: egui::Align2::LEFT_CENTER,
        lit: false,
        addon: None,
        blur_applies: true,
        empty_applies: empty,
        handle: None,
    };
    // The ui's id: the caller's, salted by its document, keeps fields of documents apart.
    let id = ui.make_persistent_id(("info_field", key));
    // Laid out as the label beside it (`field_label`): level with it.
    ui.vertical(|ui| framed_field(ui, id, current, current, field)).inner
}

/// The entity info's grid spacing.
const INFO_SPACING: egui::Vec2 = egui::vec2(8.0, 6.0);

/// The width of the entity info's label column: its widest label in the language shown, whatever
/// the entity. Its grids share one id and an egui `Grid` lays a frame out with the column widths
/// of the frame before: sized by its own labels, the column took another entity's width for one
/// frame when the selection changed kind (or a row came or went), the values sliding sideways.
fn info_label_width(ui: &egui::Ui) -> f32 {
    const LABELS: &[&str] = &[
        "panel.entity_info.type",
        "panel.entity_info.name",
        "panel.entity_info.occurrence",
        "panel.entity_info.occurrences",
        "panel.entity_info.material",
        "panel.entity_info.materials",
        "panel.entity_info.dimensions",
        "panel.entity_info.position",
        "panel.entity_info.in_model",
        "panel.entity_info.parts",
        "panel.entity_info.assemblies",
        "panel.entity_info.shape",
        "panel.entity_info.body",
        "panel.entity_info.corners",
        "panel.entity_info.role",
        "panel.entity_info.state",
        "panel.entity_info.copies",
        "tool.round.vcb",
        "tool.chamfer.vcb",
        "panel.entity_info.subject",
        "panel.entity_info.direction",
        "panel.entity_info.scale",
        "panel.entity_info.style",
        "panel.entity_info.projected_from",
        "panel.entity_info.paper",
        "panel.entity_info.orientation",
        "panel.entity_info.projection",
        "panel.entity_info.grid",
        "panel.entity_info.anchor",
        "panel.entity_info.rotation",
        "panel.entity_info.border",
        "panel.entity_info.margin",
        "panel.entity_info.border_width",
        "panel.entity_info.sheets",
        "panel.entity_info.text",
        "panel.entity_info.text_size",
        "panel.entity_info.value",
    ];
    let font = egui::TextStyle::Body.resolve(ui.style());
    let painter = ui.painter();
    LABELS
        .iter()
        .map(|key| painter.layout_no_wrap(skraft_i18n::translate(key, &[]), font.clone(), egui::Color32::PLACEHOLDER).size().x)
        .fold(0.0, f32::max)
        .ceil()
}

/// An entity info's grid: label and value columns, the labels' as wide whatever the entity
/// ([`info_label_width`]; the value column is always wider).
fn info_grid(ui: &egui::Ui) -> egui::Grid {
    egui::Grid::new("info").num_columns(2).spacing(INFO_SPACING).min_col_width(info_label_width(ui))
}

/// The width of a field in an entity info's value column: the room left by the labels.
fn info_field_width(ui: &egui::Ui) -> f32 {
    (ui.available_width() - info_label_width(ui) - INFO_SPACING.x).max(60.0)
}

/// A grid row's label, level with the [`framed_field`] beside it (or the first one, on top).
fn field_label(ui: &mut egui::Ui, text: String) {
    ui.vertical(|ui| {
        let size = egui::vec2(0.0, OPTION_HEIGHT);
        ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(text).extend());
        });
    });
}

fn dims(lo: DVec3, hi: DVec3, units: &Units) -> String {
    let d = hi - lo;
    format!("{} × {} × {}{}", units.number(d.x), units.number(d.y), units.number(d.z), units.suffix())
}

/// What is selected: an occurrence (a part, an assembly), a guide; a part open, a body or a
/// group of its bodies, in it or in the group open (docs/model/bodies.md) — nothing shown for the part itself,
/// the selection empty.
pub fn info(ui: &mut egui::Ui, editor: &Editor, name: &str, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let units = &doc.settings().units;
    // A component chosen in the library, its occurrence carried at the cursor too
    // (docs/ui/components-library.md#managing).
    if let Some(c) = editor.chosen_component() {
        return component_info(ui, editor, c, actions);
    }
    // A material chosen in its card, over the selection it keeps (docs/ui/catalog.md).
    if let Some(m) = editor.chosen_material() {
        return material_info(ui, editor, m, actions);
    }
    // A part or a guide of an assembly selected at any depth (« Parts », docs/tools/selection.md):
    // shown as in its own assembly; several, or a part's guide, counted.
    let (context, sel) = match (editor.deep_selection(), editor.deep_guides()) {
        ([], []) => (editor.context_definition(), editor.selection().to_vec()),
        ([p], []) => match doc.context_of(&p.instances) {
            Ok(c) => (c, vec![p.entity]),
            Err(_) => return,
        },
        ([], [g]) if let Ok(c) = doc.context_of(&g.instances) => (c, vec![g.guide]),
        (parts, guides) => {
            ui.label(tp!("panel.entity_info.selected", parts.len() + guides.len()));
            return;
        }
    };
    let sel = sel.as_slice();
    let Ok(asm) = doc.assembly(context) else { return };
    if let Some(open) = editor.open_part().and_then(|id| asm.occurrence(id)) {
        let Some((part, p, _)) = editor.open_part_path().and_then(|path| doc.resolve_container(&path).ok()) else { return };
        // A body's profile edited: it is what is chosen (the outliner's « Profil » row).
        let edited = editor.profile_body().and_then(|id| p.bodies.iter().position(|b| b.id == id));
        match sel {
            [] => match edited {
                Some(k) => profile_info(ui, &p, k),
                None => {
                    ui.weak(t!("panel.entity_info.no_selection"));
                }
            },
            [id] => {
                if let Some(k) = p.bodies.iter().position(|b| b.id == *id) {
                    body_info(ui, editor, part, &p, k, actions);
                } else if let Some(g) = doc.component(open.component).ok().and_then(|c| c.guide(*id)) {
                    guide_info(ui, doc, open.component, g, actions);
                }
            }
            many => {
                ui.label(tp!("panel.entity_info.selected", many.len()));
            }
        }
        return;
    }
    match sel {
        [] => {
            if editor.model_chosen() {
                model_info(ui, doc, name, actions);
            } else if editor.context().is_empty() {
                ui.weak(t!("panel.entity_info.no_selection"));
            } else {
                let name = doc.component(context).map(|c| c.name.as_str()).unwrap_or("?");
                ui.label(t!("panel.entity_info.editing", name = name));
                ui.weak(t!("panel.entity_info.editing_exit"));
            }
        }
        [id] if doc.component(context).is_ok_and(|c| c.guide(*id).is_some()) => {
            let Some(g) = doc.component(context).ok().and_then(|c| c.guide(*id)) else { return };
            guide_info(ui, doc, context, g, actions);
        }
        [id] => {
            let Some(o) = asm.occurrence(*id) else { return };
            let Ok(c) = doc.component(o.component) else { return };
            let field = info_field_width(ui);
            info_grid(ui).show(ui, |ui| {
                ui.label(t!("panel.entity_info.type"));
                ui.label(if c.is_part() { t!("panel.entity_info.type_part") } else { t!("panel.entity_info.type_assembly") });
                ui.end_row();
                field_label(ui, t!("panel.entity_info.name"));
                if let Some(name) = text_field(ui, (c.id, 1), &c.name, field) {
                    actions.push(Action::Apply(Operation::RenameComponent { component: c.id, name }));
                }
                ui.end_row();
                field_label(ui, t!("panel.entity_info.occurrence"));
                // Emptied, cleared: the occurrence shown by its component's name alone.
                if let Some(name) = name_field(ui, (o.id, 0), &o.name, field, true) {
                    actions.push(Action::Apply(Operation::RenameOccurrence { context, occurrence: o.id, name }));
                }
                ui.end_row();
                ui.label(t!("panel.entity_info.occurrences"));
                ui.label(doc.occurrence_count(c.id).to_string());
                ui.end_row();
                field_label(ui, t!("panel.entity_info.material"));
                if let Some(m) = material_choice(ui, doc, ("occurrence_material", o.id), o.material, field) {
                    actions.push(Action::Apply(Operation::PaintOccurrences { context, entities: vec![o.id], material: m }));
                }
                ui.end_row();
                if let ComponentKind::Part(p) = &c.kind {
                    // In the part's own axes: length, width, thickness as the part lies; the
                    // final shape's exact box, once the kernel has given it (docs/model/bodies.md). None for a
                    // part in several pieces, or empty: an error to correct.
                    if let Some(error) = pieces_error(p.pieces()) {
                        ui.label(t!("panel.entity_info.dimensions"));
                        let text = egui::RichText::new(error).color(ui.visuals().error_fg_color);
                        cut_label(ui, text, field);
                        ui.end_row();
                    } else if let Some((lo, hi)) = editor.scene().bounds_of(p.shape()) {
                        ui.label(t!("panel.entity_info.dimensions"));
                        cut_label(ui, dims(lo, hi, units), field);
                        ui.end_row();
                    }
                }
                // At the top of its three fields, level with the first.
                field_label(ui, t!("panel.entity_info.position"));
                // Each after its axis' letter, in its colour. Typed like in the measurements
                // box: `@+19`, `600-2*19`, `2cm`; shown as everywhere (`~ 12` when rounded),
                // edited to the micron.
                let mut t = o.transform.translation;
                let before = t;
                ui.vertical(|ui| {
                    let axes = [skraft_theme::AXIS_X, skraft_theme::AXIS_Y, skraft_theme::AXIS_Z];
                    for (k, (v, axis)) in [&mut t.x, &mut t.y, &mut t.z].into_iter().zip(axes).enumerate() {
                        let letter = ["X", "Y", "Z"][k];
                        let shown = units.format(*v);
                        let exact = format!("{}{}", units.exact_number(*v), units.suffix());
                        let addon = Some((letter, crate::viewport::color(axis)));
                        let f = Field {
                            hint: None,
                            tip: None,
                            width: field,
                            align: egui::Align2::RIGHT_CENTER,
                            lit: false,
                            addon,
                            blur_applies: true,
                            empty_applies: false,
                            handle: None,
                        };
                        let id = ui.make_persistent_id(("info_position", o.id, k));
                        if let Some(text) = framed_field(ui, id, &shown, &exact, f)
                            && let Ok(mm) = measure::coordinate(&text, units, *v)
                        {
                            *v = mm;
                        }
                    }
                });
                if t != before {
                    actions.push(Action::Apply(Operation::Transform {
                        context,
                        entities: vec![o.id],
                        transform: DAffine3::from_translation(t - before),
                    }));
                }
                ui.end_row();
            });
            // The part's (or assembly's), then the occurrence's own (docs/model/properties.md).
            props::section(ui, doc, &PropTarget::Component(c.id), t!("panel.properties.title"), actions);
            let occurrence = PropTarget::Occurrence { context, occurrence: o.id };
            props::section(ui, doc, &occurrence, t!("panel.properties.occurrence_title"), actions);
        }
        many => {
            ui.label(tp!("panel.entity_info.selected", many.len()));
        }
    }
}

/// A component chosen in the library (docs/ui/components-library.md#managing): its nature, its name
/// (edited there), how many times it is in the model, a part's dimensions, the materials its
/// occurrences and faces carry.
fn component_info(ui: &mut egui::Ui, editor: &Editor, id: Id, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let Ok(c) = doc.component(id) else { return };
    let units = &doc.settings().units;
    let field = info_field_width(ui);
    let count = doc.counts_in_model().get(&id).copied().unwrap_or(0);
    // Its materials: its occurrences' own, its faces'; in the order of the document's.
    let mut used: FastSet<Id> = FastSet::default();
    for a in doc.components().filter_map(|c| c.as_assembly()).chain(std::iter::once(doc.root_assembly())) {
        used.extend(a.occurrences().iter().filter(|o| o.component == id).filter_map(|o| o.material));
    }
    if let ComponentKind::Part(p) = &c.kind {
        used.extend(p.faces.values().copied());
    }
    let materials: Vec<&str> = doc.materials().filter(|(m, _)| used.contains(m)).map(|(_, m)| m.name.as_str()).collect();
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(if c.is_part() { t!("panel.entity_info.type_part") } else { t!("panel.entity_info.type_assembly") });
        ui.end_row();
        field_label(ui, t!("panel.entity_info.name"));
        if let Some(name) = text_field(ui, (c.id, 1), &c.name, field) {
            actions.push(Action::Apply(Operation::RenameComponent { component: c.id, name }));
        }
        ui.end_row();
        ui.label(t!("panel.entity_info.in_model"));
        ui.label(count.to_string());
        ui.end_row();
        if let ComponentKind::Part(p) = &c.kind {
            if let Some(error) = pieces_error(p.pieces()) {
                ui.label(t!("panel.entity_info.dimensions"));
                cut_label(ui, egui::RichText::new(error).color(ui.visuals().error_fg_color), field);
                ui.end_row();
            } else if let Some((lo, hi)) = editor.scene().bounds_of(p.shape()) {
                ui.label(t!("panel.entity_info.dimensions"));
                cut_label(ui, dims(lo, hi, units), field);
                ui.end_row();
            }
        }
        if !materials.is_empty() {
            ui.label(t!("panel.entity_info.materials"));
            cut_label(ui, materials.join(", "), field);
            ui.end_row();
        }
    });
}

/// The model, chosen (the navigator's first row clicked): its name (`name`, its file's), its
/// parts and assemblies as placed, its materials; its properties and the definitions of all
/// (docs/model/properties.md).
fn model_info(ui: &mut egui::Ui, doc: &Document, name: &str, actions: &mut Vec<Action>) {
    let (parts, assemblies) = placed_counts(doc, doc.root(), &mut FastMap::default(), 0);
    let field = info_field_width(ui);
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("panel.entity_info.type_model"));
        ui.end_row();
        ui.label(t!("panel.entity_info.name"));
        cut_label(ui, name, field);
        ui.end_row();
        ui.label(t!("panel.entity_info.parts"));
        ui.label(parts.to_string());
        ui.end_row();
        ui.label(t!("panel.entity_info.assemblies"));
        ui.label(assemblies.to_string());
        ui.end_row();
        ui.label(t!("panel.entity_info.materials"));
        ui.label(doc.materials().count().to_string());
        ui.end_row();
    });
    props::section(ui, doc, &PropTarget::Component(doc.root()), t!("panel.properties.title"), actions);
    props::definitions(ui, doc, actions);
}

/// How many parts and assemblies `assembly` places, one per occurrence at any depth; each
/// definition walked once (`memo`), so a model of many copies costs its definitions, not its
/// placements.
fn placed_counts(doc: &Document, assembly: Id, memo: &mut FastMap<Id, (usize, usize)>, depth: usize) -> (usize, usize) {
    if let Some(&counts) = memo.get(&assembly) {
        return counts;
    }
    let Ok(a) = doc.assembly(assembly) else { return (0, 0) };
    if depth > skraft_model::MAX_DEPTH {
        return (0, 0);
    }
    let mut counts = (0, 0);
    for o in a.occurrences() {
        if doc.part(o.component).is_ok() {
            counts.0 += 1;
        } else if doc.assembly(o.component).is_ok() {
            let (parts, assemblies) = placed_counts(doc, o.component, memo, depth + 1);
            counts.0 += parts;
            counts.1 += assemblies + 1;
        }
    }
    memo.insert(assembly, counts);
    counts
}

/// What is wrong with a part of `pieces` lumps of material, if anything: a part is one piece,
/// in several or none (empty, docs/model/bodies.md#empty-parts) an error to correct.
fn pieces_error(pieces: usize) -> Option<String> {
    match pieces {
        0 => Some(t!("panel.entity_info.empty")),
        1 => None,
        _ => Some(t!("panel.entity_info.pieces", count = pieces)),
    }
}

/// A guide of `component`: its shape (a circle's radius), its properties.
fn guide_info(ui: &mut egui::Ui, doc: &Document, component: Id, g: &skraft_model::Guide, actions: &mut Vec<Action>) {
    let units = &doc.settings().units;
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("panel.entity_info.type_guide"));
        ui.end_row();
        ui.label(t!("panel.entity_info.shape"));
        ui.label(match g.shape {
            GuideShape::Line { .. } => t!("panel.entity_info.guide_line"),
            GuideShape::Circle { radius, .. } => t!("panel.entity_info.guide_circle", radius = units.format(radius)),
        });
        ui.end_row();
    });
    props::section(ui, doc, &PropTarget::Guide { component, guide: g.id }, t!("panel.properties.title"), actions);
}

/// The profile of the body `k` of `p` (the open part, or the group open in it), being edited:
/// its body, how it is swept, its corners.
fn profile_info(ui: &mut egui::Ui, p: &skraft_model::Part, k: usize) {
    let b = &p.bodies[k];
    let Some(profile) = p.profile_of(b) else { return };
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("panel.entity_info.type_profile"));
        ui.end_row();
        ui.label(t!("panel.entity_info.body"));
        ui.label(body_label(p, b));
        ui.end_row();
        ui.label(t!("panel.entity_info.corners"));
        ui.label(profile.profile.len().to_string());
        ui.end_row();
    });
}

/// The body `k` of `p` (the open part, or the group open in it, docs/model/bodies.md): a body or a group, its
/// name as the outliner gives it — its definition's, edited here —, blank or machining (its
/// sign toggled in the outliner), failed or not, its solid's size, how many bodies share its
/// definition.
fn body_info(ui: &mut egui::Ui, editor: &Editor, part: Id, p: &skraft_model::Part, k: usize, actions: &mut Vec<Action>) {
    let units = &editor.document().settings().units;
    let b = &p.bodies[k];
    let def = p.def_of(b);
    let field = info_field_width(ui);
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(match def.nature() {
            Nature::Round => t!("default_name.round"),
            Nature::Chamfer => t!("default_name.chamfer"),
            _ if def.is_group() => t!("panel.entity_info.type_group"),
            _ => t!("panel.entity_info.type_body"),
        });
        ui.end_row();
        // Its definition's: every copy shows it; emptied, it is named by its nature again.
        field_label(ui, t!("panel.entity_info.name"));
        if let Some(name) = name_field(ui, (def.id, 2), &body_label(p, b), field, true) {
            actions.push(Action::Apply(Operation::RenameBodyDef { part, def: def.id, name }));
        }
        ui.end_row();
        // Its own, as an occurrence's: emptied, cleared.
        field_label(ui, t!("panel.entity_info.occurrence"));
        if let Some(name) = name_field(ui, (b.id, 3), &b.name, field, true) {
            actions.push(Action::Apply(Operation::RenameBody { part, body: b.id, name }));
        }
        ui.end_row();
        ui.label(t!("panel.entity_info.role"));
        ui.label(match b.sign {
            Sign::Positive => t!("panel.entity_info.role_positive"),
            Sign::Negative => t!("panel.entity_info.role_negative"),
            Sign::Intersect => t!("panel.entity_info.role_intersect"),
        });
        ui.end_row();
        // A body node's size, edited here (docs/tools/rounds.md).
        if let Some(op) = def.op().and_then(|o| o.blend()) {
            let label = match op.kind {
                skraft_model::kernel::names::Blend::Round => t!("tool.round.vcb"),
                skraft_model::kernel::names::Blend::Chamfer => t!("tool.chamfer.vcb"),
            };
            field_label(ui, label);
            let shown = units.format(op.size);
            let exact = format!("{}{}", units.exact_number(op.size), units.suffix());
            let f = Field {
                hint: None,
                tip: None,
                width: field,
                align: egui::Align2::RIGHT_CENTER,
                lit: false,
                addon: None,
                blur_applies: true,
                empty_applies: false,
                handle: Some("info:blend_size".into()),
            };
            let id = ui.make_persistent_id(("info_blend_size", b.id));
            if let Some(text) = ui.vertical(|ui| framed_field(ui, id, &shown, &exact, f)).inner
                && let Ok(size) = measure::length(&text, units, op.size)
                && size > 0.0
                && let Some(target) = editor.open_part_path()
            {
                let scope = editor.part_scope();
                actions.push(Action::Apply(Operation::SetBlend { target, body: b.id, blend: op.kind, size, picks: Vec::new(), scope }));
            }
            ui.end_row();
            // Its edges, picked again on its children (the node opened).
            ui.label("");
            if ui.button(t!("panel.entity_info.blend_edges")).tip(t!("panel.entity_info.blend_edges_tip")).clicked() {
                actions.push(Action::EditBlend(b.id));
            }
            ui.end_row();
        }
        if p.is_failed(b.id) {
            ui.label(t!("panel.entity_info.state"));
            let error = if let Some(r) = p.refusal_of(b.id) {
                skraft_model::blend_refusal(r)
            } else if def.solid.is_empty() {
                t!("panel.entity_info.empty")
            } else {
                t!("panel.entity_info.failed")
            };
            cut_label(ui, egui::RichText::new(error).color(ui.visuals().error_fg_color), field);
            ui.end_row();
        }
        // Its solid's exact box, in its own axes, once the kernel has given it.
        if let Some((lo, hi)) = editor.scene().bounds_of(&def.solid) {
            ui.label(t!("panel.entity_info.dimensions"));
            cut_label(ui, dims(lo, hi, units), field);
            ui.end_row();
        }
        let copies = p.copies_of(b.def);
        if copies > 1 {
            ui.label(t!("panel.entity_info.copies"));
            ui.label(copies.to_string());
            ui.end_row();
        }
    });
    // Its definition's (every copy's), then its own (docs/model/properties.md).
    let doc = editor.document();
    props::section(ui, doc, &PropTarget::BodyDef { part, def: def.id }, t!("panel.properties.title"), actions);
    props::section(ui, doc, &PropTarget::Body { part, body: b.id }, t!("panel.properties.occurrence_title"), actions);
}

// ---- materials ---------------------------------------------------------------------------------

/// Colours new materials take in turn (woods, then panels).
pub(crate) const NEW_COLORS: [[u8; 3]; 6] = [[196, 152, 98], [120, 78, 48], [222, 196, 150], [160, 110, 70], [235, 235, 228], [90, 92, 96]];

/// A swatch of `color`, `size` points square.
pub(crate) fn swatch(ui: &mut egui::Ui, color: Option<[u8; 3]>, size: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let stroke = egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color);
    match color {
        Some([red, g, b]) => {
            ui.painter().rect(r, 2.0, egui::Color32::from_rgb(red, g, b), stroke, egui::StrokeKind::Inside);
        }
        None => {
            ui.painter().rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
            ui.painter().line_segment([r.left_bottom(), r.right_top()], stroke);
        }
    }
}

/// A material picked in a list (« Aucune » first), framed as a [`framed_field`]: its name and a
/// downward triangle. `Some(choice)` when it changed.
fn material_choice(ui: &mut egui::Ui, doc: &Document, salt: (&str, Id), current: Option<Id>, width: f32) -> Option<Option<Id>> {
    let name = |m: Option<Id>| match m {
        Some(id) => doc.material(id).map_or("?".to_owned(), |m| m.name.clone()),
        None => t!("panel.materials.none"),
    };
    // Laid out as the label beside it (`field_label`): level with it.
    ui.vertical(|ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, OPTION_HEIGHT), egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let id = ui.make_persistent_id(salt);
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let visuals = ui.visuals();
        let stroke = field_stroke(visuals, open, false, response.hovered());
        let corners = visuals.widgets.inactive.corner_radius;
        let p = ui.painter();
        p.rect(rect, corners, visuals.extreme_bg_color, stroke, egui::StrokeKind::Inside);
        let ink = visuals.text_color();
        let inner = rect.shrink2(egui::vec2(6.0, 0.0));
        // The chevron, as the combo boxes draw it.
        let icon = egui::Rect::from_center_size(egui::pos2(inner.right() - 5.0, inner.center().y), egui::vec2(14.0, 14.0));
        icons::chevron(ui, icon.center(), 14.0, ink, 0.0);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let text = ui.fonts_mut(|f| f.layout_no_wrap(name(current), font, ink));
        let clip = inner.with_max_x(icon.left() - 4.0);
        ui.painter_at(clip).galley(egui::pos2(inner.left(), inner.center().y - text.size().y / 2.0), text, ink);

        let mut picked = current;
        egui::Popup::menu(&response).id(id).width(width).style(crate::style::menu).show(|ui| {
            ui.set_min_width(ui.available_width());
            ui.selectable_value(&mut picked, None, t!("panel.materials.none"));
            for (id, m) in doc.materials() {
                ui.horizontal(|ui| {
                    swatch(ui, Some(m.color), 12.0);
                    ui.selectable_value(&mut picked, Some(id), &m.name);
                });
            }
        });
        (picked != current).then_some(picked)
    })
    .inner
}

/// A texture's preview (points) under the pointer.
const THUMB_PREVIEW: f32 = 160.0;

/// The material chosen in its card: its name, colour, texture (its image, real size, rotation),
/// how many parts show it, its properties (docs/ui/catalog.md).
fn material_info(ui: &mut egui::Ui, editor: &Editor, id: Id, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let Ok(m) = doc.material(id) else { return };
    let units = &doc.settings().units;
    let width = info_field_width(ui);
    // Walking the model once per revision, not per frame.
    let key = ui.make_persistent_id(("material_count", id));
    let revision = editor.history().revision();
    let count = match ui.data(|d| d.get_temp::<(u64, u64)>(key)) {
        Some((r, n)) if r == revision => n,
        _ => {
            let n = doc.material_counts().get(&id).copied().unwrap_or(0);
            ui.data_mut(|d| d.insert_temp(key, (revision, n)));
            n
        }
    };
    fn number(width: f32, tip: &str) -> Field<'_> {
        Field {
            hint: None,
            tip: Some(tip),
            width,
            align: egui::Align2::RIGHT_CENTER,
            lit: false,
            addon: None,
            blur_applies: true,
            empty_applies: false,
            handle: None,
        }
    }
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("common.material"));
        ui.end_row();
        field_label(ui, t!("panel.entity_info.name"));
        if let Some(name) = text_field(ui, (id, 2), &m.name, width) {
            actions.push(Action::Apply(Operation::EditMaterial { material: id, name, color: m.color }));
        }
        ui.end_row();
        field_label(ui, t!("panel.materials.color"));
        let mut c = m.color;
        if ui.color_edit_button_srgb(&mut c).tip(t!("panel.materials.color_tip")).changed() {
            actions.push(Action::Apply(Operation::EditMaterial { material: id, name: m.name.clone(), color: c }));
        }
        ui.end_row();
        ui.label(t!("panel.entity_info.in_model"));
        ui.label(count.to_string());
        ui.end_row();
        field_label(ui, t!("panel.materials.texture"));
        ui.horizontal(|ui| {
            let h = OPTION_HEIGHT;
            let ink = ui.visuals().text_color();
            let thumb = m.texture.as_ref().and_then(|t| crate::materials::thumbnail(ui.ctx(), doc, &t.image));
            let clicked = match &thumb {
                Some(tex) => {
                    let img = egui::Image::new(tex)
                        .uv(crate::materials::centre_square(tex))
                        .maintain_aspect_ratio(false)
                        .fit_to_exact_size(egui::vec2(h, h))
                        .corner_radius(3.0);
                    let r = ui.add(egui::Button::image(img).frame(false).min_size(egui::vec2(h, h)));
                    r.tip_ui(|ui| {
                        ui.label(t!("panel.materials.texture_change_tip"));
                        ui.add(egui::Image::new(tex).max_size(egui::vec2(THUMB_PREVIEW, THUMB_PREVIEW)).corner_radius(4.0));
                    })
                    .clicked()
                }
                None => {
                    let icon = egui::Image::new(icons::TEXTURE).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(ink);
                    let tip =
                        if m.texture.is_some() { t!("panel.materials.texture_change_tip") } else { t!("panel.materials.texture_tip") };
                    ui.add(egui::Button::image(icon).selected(m.texture.is_some()).min_size(egui::vec2(h, h))).tip(tip).clicked()
                }
            };
            if clicked {
                actions.push(Action::PickTexture(id));
            }
            if m.texture.is_some() {
                let close = egui::Image::new(icons::CLOSE).fit_to_exact_size(egui::vec2(12.0, 12.0)).tint(ink);
                if ui.add(egui::Button::image(close).min_size(egui::vec2(h, h))).tip(t!("panel.materials.texture_remove_tip")).clicked() {
                    actions.push(Action::Apply(Operation::SetTexture { material: id, texture: None, image: None }));
                }
            }
        });
        ui.end_row();
        if let Some(texture) = &m.texture {
            let set = |t: skraft_model::Texture| Action::Apply(Operation::SetTexture { material: id, texture: Some(t), image: None });
            let sizes = [
                (t!("panel.materials.texture_width"), t!("panel.materials.texture_width_tip")),
                (t!("panel.materials.texture_height"), t!("panel.materials.texture_height_tip")),
            ];
            for (k, (label, tip)) in sizes.iter().enumerate() {
                field_label(ui, label.clone());
                let v = texture.size[k];
                let (shown, exact) = (units.format(v), format!("{}{}", units.exact_number(v), units.suffix()));
                let fid = ui.make_persistent_id(("texture_size", id, k));
                if let Some(text) = ui.vertical(|ui| framed_field(ui, fid, &shown, &exact, number(width, tip))).inner
                    && let Ok(mm) = measure::length(&text, units, v)
                    && mm > 0.0
                {
                    let mut t = texture.clone();
                    t.size[k] = mm;
                    actions.push(set(t));
                }
                ui.end_row();
            }
            field_label(ui, t!("panel.materials.texture_rotation"));
            let deg = texture.rotation.to_degrees();
            let shown = format!("{}°", skraft_i18n::number(&format!("{}", (deg * 100.0).round() / 100.0)));
            let tip = t!("panel.materials.texture_rotation_tip");
            let fid = ui.make_persistent_id(("texture_rotation", id));
            if let Some(text) = ui.vertical(|ui| framed_field(ui, fid, &shown, &shown, number(width, &tip))).inner
                && let Ok(d) = measure::angle(&text, deg)
            {
                actions.push(set(skraft_model::Texture { rotation: d.to_radians(), ..texture.clone() }));
            }
            ui.end_row();
        }
    });
    props::section(ui, doc, &PropTarget::Material(id), t!("panel.properties.title"), actions);
}

// ---- the drawing (docs/ui/drawing.md) ----------------------------------------------------------

/// The entity info in « Mise en plan »: the view picked on the sheet shown — its name, what it
/// shows, from where, at what scale, in what style; a projected view its base and side —, else
/// the sheet — its name, its paper, the document's projection.
pub fn drawing_info(ui: &mut egui::Ui, editor: &Editor, st: &mut crate::sheet::SheetState, title: &str, actions: &mut Vec<Action>) {
    use crate::sheet::InfoOn;
    use skraft_model::{
        Border, Corner, Direction, ElementKind, Instance, Look, Paper, Place, Side, Subject, Value, View, ViewKind, ViewStyle,
    };
    let doc = editor.document();
    let d = doc.drawing();
    let field = info_field_width(ui);
    let combo = |salt: (&str, u64)| egui::ComboBox::from_id_salt(salt).icon(crate::icons::combo_chevron).width(field);
    // Several picked: how many.
    let many = crate::sheet::selection(editor, st).len();
    if many > 1 {
        ui.label(tp!("panel.entity_info.selected", many));
        return;
    }
    // Nothing picked: the document or the presentation, if chosen in the navigator.
    if many == 0 {
        match st.info {
            InfoOn::Document => return document_info(ui, doc, title, field, actions),
            InfoOn::Presentation => {
                if let Some(p) = st.presentation(d) {
                    return presentation_info(ui, doc, p, field, actions);
                }
            }
            InfoOn::Sheet => {}
        }
    }
    let Some(sheet) = st.shown(d) else {
        ui.weak(t!("drawing.no_sheet"));
        return;
    };
    let units = &doc.settings().units;
    // A free element picked: its kind; a text its words and size; an instance its place.
    if let Some(el) = crate::sheet::selected_element(editor, st).filter(|el| !matches!(el.kind, ElementKind::View(_))) {
        let id = el.id;
        info_grid(ui).show(ui, |ui| {
            ui.label(t!("panel.entity_info.type"));
            ui.label(match &el.kind {
                ElementKind::Text { .. } => t!("panel.entity_info.type_text"),
                ElementKind::Line { .. } => t!("panel.entity_info.type_line"),
                ElementKind::Rect { .. } => t!("panel.entity_info.type_rect"),
                ElementKind::Circle { .. } => t!("panel.entity_info.type_circle"),
                ElementKind::Dimension(dim) => match dim.kind {
                    skraft_model::DimKind::Linear { orient: skraft_model::Orient::Horizontal, .. } => t!("drawing.dim.horizontal"),
                    skraft_model::DimKind::Linear { orient: skraft_model::Orient::Vertical, .. } => t!("drawing.dim.vertical"),
                    skraft_model::DimKind::Linear { .. } => t!("drawing.dim.aligned"),
                    skraft_model::DimKind::Radius { diameter: false, .. } => t!("drawing.dim.radius"),
                    skraft_model::DimKind::Radius { .. } => t!("drawing.dim.diameter"),
                    skraft_model::DimKind::Angle { .. } => t!("drawing.dim.angle"),
                },
                ElementKind::Instance(_) => t!("panel.entity_info.type_instance"),
                _ => t!("panel.entity_info.type_image"),
            });
            ui.end_row();
            // A dimension's value as the model is now, or why it has none.
            if let ElementKind::Dimension(dim) = &el.kind
                && let Some(s) = crate::sheet::dim_shape(editor, dim, d.projection_of(dim.view))
            {
                ui.label(t!("panel.entity_info.value"));
                if s.orphan {
                    ui.colored_label(ui.visuals().error_fg_color, t!("drawing.dim.orphan")).on_hover_text(t!("drawing.dim.orphan_tip"));
                } else {
                    ui.label(&s.text);
                }
                ui.end_row();
            }
            if let ElementKind::Text { at, text, size } = &el.kind {
                let set = |text: String, size: f64| {
                    Action::Apply(Operation::SetElement { element: id, kind: ElementKind::Text { at: *at, text, size } })
                };
                field_label(ui, t!("panel.entity_info.text"));
                ui.vertical(|ui| {
                    // The words as typed, applied when the field is left.
                    let key = ui.make_persistent_id(("drawing_text", id.0));
                    let mut buf: String = ui.data(|m| m.get_temp::<String>(key)).unwrap_or_else(|| text.clone());
                    let edit = ui.add(egui::TextEdit::multiline(&mut buf).desired_width(field).desired_rows(2));
                    if edit.has_focus() {
                        ui.data_mut(|m| m.insert_temp(key, buf.clone()));
                    } else {
                        ui.data_mut(|m| m.remove::<String>(key));
                    }
                    if edit.lost_focus() && buf != *text {
                        actions.push(set(buf.clone(), *size));
                    }
                    // A field put at the end (docs/ui/drawing.md#fields).
                    let button = ui.button(t!("drawing.insert_field"));
                    crate::handles::publish(|| "drawing_insert_field".into(), button.rect);
                    let menu = egui::Popup::menu(&button).style(crate::style::menu);
                    menu.close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                        // Skraft's own, those the drawing's texts already name, a new one.
                        let custom = crate::sheet::custom_fields(d.all_field_names());
                        let own = crate::sheet::FIELDS.iter().map(|f| (*f, field_name(f)));
                        for (f, label) in own.chain(custom.iter().map(|f| (f.as_str(), f.clone()))) {
                            let item = ui.add(crate::style::menu_item(label, None, false));
                            crate::handles::publish(|| format!("drawing_field:{f}"), item.rect);
                            if item.clicked() {
                                actions.push(set(format!("{text}{{{f}}}"), *size));
                                ui.close();
                            }
                        }
                        ui.separator();
                        let key = ui.make_persistent_id(("drawing_new_field", id.0));
                        let mut name = ui.data(|m| m.get_temp::<String>(key)).unwrap_or_default();
                        let edit = ui.add(egui::TextEdit::singleline(&mut name).hint_text(t!("drawing.new_field")).desired_width(150.0));
                        crate::handles::publish(|| "drawing_new_field".into(), edit.rect);
                        let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let clean = name.replace(['{', '}', '.'], " ").trim().to_owned();
                        ui.data_mut(|m| m.insert_temp(key, name));
                        if entered && !clean.is_empty() {
                            ui.data_mut(|m| m.remove::<String>(key));
                            actions.push(set(format!("{text}{{{clean}}}"), *size));
                            ui.close();
                        }
                    });
                });
                ui.end_row();
                ui.label(t!("panel.entity_info.text_size"));
                let mut v = *size;
                if ui.add(egui::DragValue::new(&mut v).speed(0.1).range(0.5..=200.0).suffix(" mm")).changed() {
                    actions.push(set(text.clone(), v));
                }
                ui.end_row();
            }
            // An instance: its definition, its place, its values.
            if let ElementKind::Instance(i) = &el.kind {
                let set_place = |place: Place| {
                    Action::Apply(Operation::SetElement { element: id, kind: ElementKind::Instance(Instance { place, ..i.clone() }) })
                };
                let composition = d.composition(i.composition);
                if let Some(b) = composition {
                    field_label(ui, t!("panel.entity_info.name"));
                    if let Some(name) = text_field(ui, (id, 12), &b.name, field) {
                        actions.push(Action::Apply(Operation::RenameComposition { composition: b.id, name }));
                    }
                    ui.end_row();
                }
                // A corner of the paper only right on a sheet; changing it keeps it where it is.
                if st.open.is_empty() {
                    let label = |c: Option<Corner>| match c {
                        None => t!("drawing.anchor.free"),
                        Some(Corner::BottomLeft) => t!("drawing.anchor.bottom_left"),
                        Some(Corner::BottomRight) => t!("drawing.anchor.bottom_right"),
                        Some(Corner::TopLeft) => t!("drawing.anchor.top_left"),
                        Some(Corner::TopRight) => t!("drawing.anchor.top_right"),
                    };
                    field_label(ui, t!("panel.entity_info.anchor"));
                    combo(("drawing_anchor", id.0)).selected_text(label(i.place.corner)).show_ui(ui, |ui| {
                        for c in [None, Some(Corner::BottomLeft), Some(Corner::BottomRight), Some(Corner::TopLeft), Some(Corner::TopRight)]
                        {
                            if ui.selectable_label(i.place.corner == c, label(c)).clicked() && i.place.corner != c {
                                actions.push(set_place(i.place.with_corner(c, sheet.paper)));
                            }
                        }
                    });
                    ui.end_row();
                }
                let at = i.place.at;
                if let Some(Value::Length(x)) = typed_row(ui, "X".to_owned(), (id, 13), &Value::Length(at.x), units, field) {
                    actions.push(set_place(Place { at: glam::DVec2::new(x, at.y), ..i.place }));
                }
                if let Some(Value::Length(y)) = typed_row(ui, "Y".to_owned(), (id, 14), &Value::Length(at.y), units, field) {
                    actions.push(set_place(Place { at: glam::DVec2::new(at.x, y), ..i.place }));
                }
                let turn = Value::Angle(i.place.rotation);
                if let Some(Value::Angle(r)) = typed_row(ui, t!("panel.entity_info.rotation"), (id, 15), &turn, units, field) {
                    actions.push(set_place(Place { rotation: r, ..i.place }));
                }
                if let Some(b) = composition {
                    let names = field_rows(d.field_names(&b.elements), Some(&i.props));
                    let on = ValuesOn::Instance { id, instance: i, sheet: sheet.id, above: &st.open };
                    value_rows(ui, doc, &on, names, field, actions);
                }
            }
        });
        return;
    }
    if let Some((id, view)) = crate::sheet::selected_view(editor, st) {
        let set = |v: View| Action::Apply(Operation::SetElement { element: id, kind: ElementKind::View(v) });
        info_grid(ui).show(ui, |ui| {
            ui.label(t!("panel.entity_info.type"));
            ui.label(match view.kind {
                ViewKind::Base { .. } => t!("panel.entity_info.type_view"),
                ViewKind::Projected { .. } => t!("panel.entity_info.type_projected_view"),
            });
            ui.end_row();
            field_label(ui, t!("panel.entity_info.name"));
            if let Some(name) = text_field(ui, (id, 10), &view.name, field) {
                actions.push(set(View { name, ..view.clone() }));
            }
            ui.end_row();
            match &view.kind {
                ViewKind::Base { subject, look, scale, at } => {
                    ui.label(t!("panel.entity_info.subject"));
                    let shows = match subject {
                        Subject::Model => t!("drawing.subject.model"),
                        Subject::Component(c) => doc.component(*c).map_or_else(|_| "—".into(), |c| c.name.clone()),
                        Subject::Occurrences(paths) => t!("drawing.subject.occurrences", count = paths.len()),
                    };
                    // Changed in the dialog a new view is chosen in (decision 0051).
                    let change = ui.add_sized([field, ui.spacing().interact_size.y], egui::Button::new(shows).truncate());
                    crate::handles::publish(|| "view_subject".into(), change.rect);
                    if change.tip(t!("drawing.picker.change_tip")).clicked() {
                        let purpose = crate::view_picker::Purpose::Change(id);
                        st.picker = Some(crate::view_picker::ViewPicker::new(doc, purpose, subject.clone()));
                    }
                    ui.end_row();
                    // Another scale shows the same part of it; another direction, the whole.
                    let was = look;
                    let base = |look: Look, s: f64| View {
                        crop: if look == *was { view.crop.map(|c| c.scaled(s / *scale)) } else { None },
                        kind: ViewKind::Base { subject: subject.clone(), look, scale: s, at: *at },
                        ..view.clone()
                    };
                    field_label(ui, t!("panel.entity_info.direction"));
                    let shown = match look {
                        Look::Standard(d) => direction_label(*d),
                        Look::Camera { .. } => t!("drawing.direction.camera"),
                    };
                    combo(("drawing_direction", id.0)).selected_text(shown).show_ui(ui, |ui| {
                        for d in [
                            Direction::Front,
                            Direction::Back,
                            Direction::Left,
                            Direction::Right,
                            Direction::Top,
                            Direction::Bottom,
                            Direction::Iso,
                        ] {
                            if ui.selectable_label(*look == Look::Standard(d), direction_label(d)).clicked() {
                                actions.push(set(base(Look::Standard(d), *scale)));
                            }
                        }
                        ui.separator();
                        // The 3D view's camera now, perspective or parallel.
                        if ui.selectable_label(matches!(look, Look::Camera { .. }), t!("drawing.direction.camera")).clicked() {
                            // A perspective keeps the frame the view has now.
                            let size = d.resolve(id).map_or(glam::DVec2::splat(100.0), |r| {
                                let (lo, hi) = crate::sheet::view_frame(editor, &r);
                                (hi - lo).max(glam::DVec2::splat(10.0))
                            });
                            let look = crate::view_framer::camera_look(doc, subject, &editor.camera, size);
                            actions.push(set(base(look, *scale)));
                        }
                    });
                    ui.end_row();
                    if look.frame().1.is_some() {
                        ui.label(t!("panel.entity_info.scale"));
                        ui.label(t!("drawing.no_scale"));
                        ui.end_row();
                        let hidden = d.container_of(id).and_then(|c| d.elements_in(c)).map_or(0, |list| {
                            list.iter().filter(|el| matches!(&el.kind, ElementKind::Dimension(dim) if dim.view == id)).count()
                        });
                        if hidden > 0 {
                            ui.label(t!("panel.entity_info.view_dimensions"));
                            ui.label(t!("drawing.dims_hidden", count = hidden));
                            ui.end_row();
                        }
                    } else {
                        field_label(ui, t!("panel.entity_info.scale"));
                        combo(("drawing_scale", id.0)).selected_text(crate::sheet::scale_label(*scale)).show_ui(ui, |ui| {
                            for s in crate::sheet::SCALES {
                                if ui.selectable_label((s - scale).abs() < 1e-12, crate::sheet::scale_label(*s)).clicked() {
                                    actions.push(set(base(look.clone(), *s)));
                                }
                            }
                        });
                        ui.end_row();
                    }
                }
                ViewKind::Projected { base, side, .. } => {
                    let base_name = d
                        .element(*base)
                        .and_then(|el| match &el.kind {
                            ElementKind::View(v) => Some(v.name.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let side = match side {
                        Side::Left => t!("drawing.side.left"),
                        Side::Right => t!("drawing.side.right"),
                        Side::Above => t!("drawing.side.above"),
                        Side::Below => t!("drawing.side.below"),
                    };
                    ui.label(t!("panel.entity_info.projected_from"));
                    ui.label(format!("{base_name}, {side}"));
                    ui.end_row();
                    ui.label(t!("panel.entity_info.scale"));
                    ui.label(d.resolve(id).map_or_else(String::new, |r| crate::sheet::scale_label(r.scale)));
                    ui.end_row();
                }
            }
            if let ViewKind::Base { look, .. } = &view.kind {
                ui.label(t!("panel.entity_info.view_frame"));
                let automatic = view.crop.is_none() && look.frame().1.is_none();
                ui.label(if automatic { t!("drawing.frame_auto") } else { t!("drawing.frame_free") });
                ui.end_row();
            }
            field_label(ui, t!("panel.entity_info.style"));
            let label = |s: ViewStyle| match s {
                ViewStyle::Lines => t!("menu.drawing.style_lines"),
                ViewStyle::Rendered => t!("menu.drawing.style_rendered"),
                ViewStyle::Hybrid => t!("menu.drawing.style_hybrid"),
            };
            combo(("drawing_style", id.0)).selected_text(label(view.style)).show_ui(ui, |ui| {
                for s in [ViewStyle::Lines, ViewStyle::Rendered, ViewStyle::Hybrid] {
                    if ui.selectable_label(view.style == s, label(s)).clicked() {
                        actions.push(set(View { style: s, ..view.clone() }));
                    }
                }
            });
            ui.end_row();
        });
        return;
    }
    // The sheet.
    let id = sheet.id;
    let paper = sheet.paper;
    let portrait = paper.height > paper.width;
    let landscape = |p: Paper| if p.height > p.width { Paper { width: p.height, height: p.width } } else { p };
    let set_paper = |p: Paper, portrait: bool| {
        let p = landscape(p);
        let p = if portrait { Paper { width: p.height, height: p.width } } else { p };
        Action::Apply(Operation::SetSheet { sheet: id, name: sheet.name.clone(), paper: p, grid: sheet.grid, border: sheet.border })
    };
    const SIZES: [(&str, Paper); 5] = [("A4", Paper::A4), ("A3", Paper::A3), ("A2", Paper::A2), ("A1", Paper::A1), ("A0", Paper::A0)];
    let named = SIZES.iter().find(|(_, p)| *p == landscape(paper)).map(|(n, _)| n.to_string());
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("panel.entity_info.type_sheet"));
        ui.end_row();
        field_label(ui, t!("panel.entity_info.name"));
        if let Some(name) = text_field(ui, (id, 11), &sheet.name, field) {
            actions.push(Action::Apply(Operation::SetSheet { sheet: id, name, paper, grid: sheet.grid, border: sheet.border }));
        }
        ui.end_row();
        field_label(ui, t!("panel.entity_info.paper"));
        let shown = named.unwrap_or_else(|| format!("{} × {}", units.number(paper.width), units.number(paper.height)));
        combo(("drawing_paper", id.0)).selected_text(shown).show_ui(ui, |ui| {
            for (n, p) in SIZES {
                if ui.selectable_label(p == landscape(paper), n).clicked() {
                    actions.push(set_paper(p, portrait));
                }
            }
        });
        ui.end_row();
        field_label(ui, t!("panel.entity_info.orientation"));
        let orientation = |portrait: bool| if portrait { t!("drawing.paper.portrait") } else { t!("drawing.paper.landscape") };
        combo(("drawing_orientation", id.0)).selected_text(orientation(portrait)).show_ui(ui, |ui| {
            for o in [false, true] {
                if ui.selectable_label(portrait == o, orientation(o)).clicked() && o != portrait {
                    actions.push(set_paper(paper, o));
                }
            }
        });
        ui.end_row();
        // The grid the points drawn snap to when nothing else is near (docs/ui/drawing.md#snapping).
        field_label(ui, t!("panel.entity_info.grid"));
        let grid_label = |g: f64| if g > 0.0 { format!("{} mm", units.number(g)) } else { t!("drawing.grid_none") };
        combo(("drawing_grid", id.0)).selected_text(grid_label(sheet.grid)).show_ui(ui, |ui| {
            for g in [0.0, 1.0, 2.0, 5.0, 10.0] {
                if ui.selectable_label(sheet.grid == g, grid_label(g)).clicked() && g != sheet.grid {
                    actions.push(Action::Apply(Operation::SetSheet {
                        sheet: id,
                        name: sheet.name.clone(),
                        paper,
                        grid: g,
                        border: sheet.border,
                    }));
                }
            }
        });
        ui.end_row();
        // Its frame, drawn from its paper (docs/ui/drawing.md#compositions).
        let set_border = |border: Option<Border>| {
            Action::Apply(Operation::SetSheet { sheet: id, name: sheet.name.clone(), paper, grid: sheet.grid, border })
        };
        field_label(ui, t!("panel.entity_info.border"));
        let mut on = sheet.border.is_some();
        let check = ui.checkbox(&mut on, "");
        crate::handles::publish(|| "drawing_border".into(), check.rect);
        if check.changed() {
            actions.push(set_border(on.then_some(Border { margin: 10.0, width: 0.25 })));
        }
        ui.end_row();
        if let Some(b) = sheet.border {
            if let Some(Value::Length(margin)) =
                typed_row(ui, t!("panel.entity_info.margin"), (id, 16), &Value::Length(b.margin), units, field)
            {
                actions.push(set_border(Some(Border { margin, ..b })));
            }
            if let Some(Value::Length(width)) =
                typed_row(ui, t!("panel.entity_info.border_width"), (id, 17), &Value::Length(b.width), units, field)
            {
                actions.push(set_border(Some(Border { width, ..b })));
            }
        }
        let names = field_rows(d.field_names(&sheet.elements), Some(&sheet.props));
        value_rows(ui, doc, &ValuesOn::Sheet(sheet), names, field, actions);
    });
}

/// A row of the entity info holding a value of its kind (a length, an angle), shown and typed in
/// the document's units. The value typed.
fn typed_row(
    ui: &mut egui::Ui,
    label: String,
    key: (Id, u8),
    value: &skraft_model::Value,
    units: &Units,
    width: f32,
) -> Option<skraft_model::Value> {
    field_label(ui, label);
    let f = Field {
        hint: None,
        tip: None,
        width,
        align: egui::Align2::RIGHT_CENTER,
        lit: false,
        addon: None,
        blur_applies: true,
        empty_applies: false,
        handle: Some(format!("drawing_value:{}:{}", key.0.0, key.1)),
    };
    let id = ui.make_persistent_id(("drawing_typed", key));
    // Exact: on paper a line of 0.25 mm is no « ~ 0 », whatever the precision of the model's.
    let exact = value.exact(units);
    let typed = ui.vertical(|ui| framed_field(ui, id, &exact, &exact, f)).inner;
    ui.end_row();
    typed.and_then(|t| skraft_model::Value::typed(&value.kind(), &t, units, Some(value)))
}

/// What a field's value is set on, in the entity info.
enum ValuesOn<'a> {
    Document,
    Presentation(&'a skraft_model::Presentation),
    Sheet(&'a skraft_model::Sheet),
    /// An instance on `sheet` under the instances `above`.
    Instance {
        id: Id,
        instance: &'a skraft_model::Instance,
        sheet: Id,
        above: &'a [Id],
    },
}

/// The fields to list: those `names` Skraft does not fill itself, and those `props` has a value
/// for, under the name a text gives them.
fn field_rows(names: std::collections::BTreeSet<String>, props: Option<&skraft_model::Props>) -> Vec<String> {
    let held = props.into_iter().flat_map(|p| p.iter()).map(|(k, _)| k.strip_prefix("user.").unwrap_or(k).to_owned());
    let all: std::collections::BTreeSet<String> = names.into_iter().chain(held).collect();
    crate::sheet::custom_fields(all)
}

/// The fields `names`, a row each: the value set on `on`, typed over or emptied; while it has
/// none, faint, the one it takes from above (docs/ui/drawing.md#fields).
fn value_rows(ui: &mut egui::Ui, doc: &Document, on: &ValuesOn<'_>, names: Vec<String>, width: f32, actions: &mut Vec<Action>) {
    use skraft_model::{FieldOn, Value, ValueKind, field_key};
    if names.is_empty() {
        return;
    }
    ui.weak(t!("panel.entity_info.values"));
    ui.label("");
    ui.end_row();
    let d = doc.drawing();
    let units = &doc.settings().units;
    let root = doc.component(doc.root()).ok().map(|c| &c.props);
    let owner = match on {
        ValuesOn::Document => doc.root().0,
        ValuesOn::Presentation(p) => p.id.0,
        ValuesOn::Sheet(s) => s.id.0,
        ValuesOn::Instance { id, .. } => id.0,
    };
    let label_width = info_label_width(ui);
    for name in names {
        let key = field_key(&name);
        let in_root = root.and_then(|p| p.get(&key));
        let (own, above): (Option<&Value>, Option<&Value>) = match on {
            ValuesOn::Document => (in_root, None),
            ValuesOn::Presentation(p) => (p.props.get(&key), in_root),
            ValuesOn::Sheet(s) => (s.props.get(&key), d.presentation_of(s.id).and_then(|p| p.props.get(&key)).or(in_root)),
            ValuesOn::Instance { instance, sheet, above, .. } => (instance.props.get(&key), doc.field(*sheet, above, &key)),
        };
        ui.vertical(|ui| {
            let size = egui::vec2(label_width, OPTION_HEIGHT);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(&name).truncate());
            });
        });
        let hint = above.map(|v| v.shown(units));
        let shown = own.map_or_else(String::new, |v| v.shown(units));
        let exact = own.map_or_else(String::new, |v| v.exact(units));
        let f = Field {
            hint: hint.as_deref(),
            tip: None,
            width,
            align: egui::Align2::LEFT_CENTER,
            lit: false,
            addon: None,
            blur_applies: true,
            empty_applies: true,
            handle: Some(format!("drawing_field_value:{name}")),
        };
        let id = ui.make_persistent_id(("drawing_field_value", owner, &key));
        let typed = ui.vertical(|ui| framed_field(ui, id, &shown, &exact, f)).inner;
        ui.end_row();
        let Some(text) = typed else { continue };
        // Of the kind it has or inherits; emptied, it is taken off.
        let value = (!text.is_empty()).then(|| {
            let kind = own.or(above).map_or(ValueKind::Text, Value::kind);
            Value::typed(&kind, &text, units, own).unwrap_or(Value::Text(text))
        });
        actions.push(Action::Apply(match on {
            ValuesOn::Document => Operation::SetProps {
                target: skraft_model::PropTarget::Component(doc.root()),
                props: std::collections::BTreeMap::from([(key, value)]),
            },
            ValuesOn::Presentation(p) => Operation::SetField { on: FieldOn::Presentation(p.id), key, value },
            ValuesOn::Sheet(s) => Operation::SetField { on: FieldOn::Sheet(s.id), key, value },
            ValuesOn::Instance { id, .. } => Operation::SetField { on: FieldOn::Instance(*id), key, value },
        }));
    }
}

/// The entity info of the document in « Mise en plan »: its name, the values of its fields.
fn document_info(ui: &mut egui::Ui, doc: &Document, title: &str, field: f32, actions: &mut Vec<Action>) {
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("panel.entity_info.type_document"));
        ui.end_row();
        ui.label(t!("panel.entity_info.name"));
        ui.label(title);
        ui.end_row();
        let names = crate::sheet::custom_fields(doc.drawing().all_field_names());
        value_rows(ui, doc, &ValuesOn::Document, names, field, actions);
    });
}

/// The entity info of a presentation: its name, its sheets, where its projected views go, the
/// values of its fields.
fn presentation_info(ui: &mut egui::Ui, doc: &Document, p: &skraft_model::Presentation, field: f32, actions: &mut Vec<Action>) {
    use skraft_model::Projection;
    let d = doc.drawing();
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("panel.entity_info.type_presentation"));
        ui.end_row();
        field_label(ui, t!("panel.entity_info.name"));
        if let Some(name) = text_field(ui, (p.id, 18), &p.name, field) {
            actions.push(Action::Apply(Operation::SetPresentation { presentation: p.id, name, projection: p.projection }));
        }
        ui.end_row();
        ui.label(t!("panel.entity_info.sheets"));
        ui.label(p.sheets.len().to_string());
        ui.end_row();
        field_label(ui, t!("panel.entity_info.projection"));
        let label = |x: Projection| match x {
            Projection::FirstAngle => t!("drawing.projection.first_angle"),
            Projection::ThirdAngle => t!("drawing.projection.third_angle"),
        };
        egui::ComboBox::from_id_salt(("drawing_projection", p.id.0))
            .icon(crate::icons::combo_chevron)
            .width(field)
            .selected_text(label(p.projection))
            .show_ui(ui, |ui| {
                for x in [Projection::FirstAngle, Projection::ThirdAngle] {
                    if ui.selectable_label(p.projection == x, label(x)).clicked() && x != p.projection {
                        actions.push(Action::Apply(Operation::SetPresentation { presentation: p.id, name: p.name.clone(), projection: x }));
                    }
                }
            });
        ui.end_row();
        let used = p.sheets.iter().flat_map(|s| d.field_names(&s.elements)).collect();
        let names = field_rows(used, Some(&p.props));
        value_rows(ui, doc, &ValuesOn::Presentation(p), names, field, actions);
    });
}

/// A text field's name in the « Insérer un champ » menu.
fn field_name(f: &str) -> String {
    match f {
        "document" => t!("drawing.field.document"),
        "date" => t!("drawing.field.date"),
        "sheet" => t!("drawing.field.sheet"),
        "sheet.number" => t!("drawing.field.sheet_number"),
        "sheet.count" => t!("drawing.field.sheet_count"),
        "scale" => t!("drawing.field.scale"),
        other => other.to_owned(),
    }
}

/// A standard direction's name.
pub(crate) fn direction_label(d: skraft_model::Direction) -> String {
    use skraft_model::Direction;
    match d {
        Direction::Front => t!("drawing.direction.front"),
        Direction::Back => t!("drawing.direction.back"),
        Direction::Left => t!("drawing.direction.left"),
        Direction::Right => t!("drawing.direction.right"),
        Direction::Top => t!("drawing.direction.top"),
        Direction::Bottom => t!("drawing.direction.bottom"),
        Direction::Iso => t!("drawing.direction.iso"),
    }
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

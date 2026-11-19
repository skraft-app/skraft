//! Side panels: the entity info, the materials and the outliner, the tool option bar and the
//! menu items, all drawn from the command registry's state
//! (docs/ui/commands.md).

use glam::{DAffine3, DVec3};
use skraft_i18n::{t, tp};
use skraft_model::{
    ComponentKind, Document, EntityPath, FastMap, FastSet, GuideShape, Id, Nature, Occurrence, Operation, PropTarget, Scope, Sign, Units,
    measure,
};
use skraft_tools::{Editor, OptionValue, ToolKind, ToolOption};

use crate::commands::Menu;
use crate::icons;

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
    Apply(Operation),
    /// An option of the active tool changed in its bar.
    ToolOption(&'static str, OptionValue),
    /// A length option of the active tool typed in its field of the bar.
    ToolLength(&'static str, String),
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
    /// A body's « Profil » row chosen in the outliner: its part (and group) opened, its profile
    /// edited.
    EditProfile(EntityPath, Id),
    /// The occurrence hovered in the outliner, if any.
    HoverOutliner(Option<EntityPath>),
    /// Hide the occurrence (true) or show it again: the outliner's eye.
    SetHidden(EntityPath, bool),
    /// The ribbon's own state: tab, workspace, pins, labels (docs/ui/ribbon.md).
    Ribbon(crate::ribbon::Change),
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

/// A value of the entity info cut short (…) at `width` rather than widening the panel; whole
/// in its tool tip (egui's, when cut).
///
/// One line high, not a `ui.scope`: in a `Grid` cell a scope spans the whole height the row had
/// the frame before (its centred row layout), so the row never shrank back — the « Dimensions »
/// row kept the three position fields' height of the row it replaced (an assembly's).
fn cut_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, width: f32) {
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add(egui::Label::new(text).truncate());
    });
}

/// An outliner row's fold toggle, one indent wide: the panels' chevron (as in their title bars),
/// turning from right (folded) to down (open) as it opens; a click folds or unfolds the row (in
/// `folds`, its document's). `salt` (the document's outliner) keeps its click and turn apart
/// from another document's row of the same ids.
fn fold_toggle(ui: &mut egui::Ui, salt: egui::Id, fold: Fold, folds: &mut FastSet<egui::Id>) {
    let (_, rect) = ui.allocate_space(egui::vec2(ui.spacing().indent, ui.spacing().icon_width));
    let id = salt.with(fold.key);
    let response = ui.interact(rect, id, egui::Sense::click());
    let mut open = fold.open;
    if response.clicked() {
        open = !open;
        if open {
            folds.insert(fold.key);
        } else {
            folds.remove(&fold.key);
        }
        ui.ctx().request_repaint();
    }
    let openness = ui.ctx().animate_bool_responsive(id, open);
    let color = ui.style().interact(&response).fg_stroke.color;
    let center = egui::pos2(rect.left() + ui.spacing().indent / 2.0, rect.center().y);
    icons::chevron(ui, center, 14.0, color, -(1.0 - openness) * std::f32::consts::FRAC_PI_2);
}

/// The warning sign before an error to correct (a part in several pieces, a shortcut's
/// conflict), in the error colour, as high as the text.
pub fn warning(ui: &egui::Ui) -> egui::Image<'static> {
    let side = ui.text_style_height(&egui::TextStyle::Body);
    egui::Image::new(icons::WARNING).fit_to_exact_size(egui::vec2(side, side)).tint(ui.visuals().error_fg_color)
}

/// Whether a command is shown in a menu as a check box (a display toggle).
pub fn is_toggle(v: &CommandView) -> bool {
    v.checked.is_some() && v.id.starts_with("view.")
}

/// A menu item: a check box for a toggle, else a button with its shortcut; no hover text.
/// `column` when the menu has check boxes: the label after their column. True when clicked.
pub fn command_item(ui: &mut egui::Ui, v: &CommandView, column: bool) -> bool {
    let mut button = if is_toggle(v) {
        crate::style::menu_item(&v.label, v.checked, column)
    } else {
        crate::style::menu_item(&v.label, None, column).selected(v.checked == Some(true))
    };
    if let Some(s) = &v.shortcut {
        button = button.shortcut_text(s);
    }
    let r = ui.add_enabled(v.enabled, button);
    crate::handles::publish_enabled(|| format!("menu:{}", v.id), r.rect, v.enabled);
    r.clicked()
}

/// The frame of the option bar's cards (docs/ui/interface.md#context-card): as tight above and
/// below its buttons as the view bar around its own, a little more room on the sides for the
/// groups' names.
pub fn option_card(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style).inner_margin(egui::Margin::symmetric(6, 3))
}

/// Whether the context card has anything to show (docs/ui/interface.md#context-card): the
/// selection's scope of a tool that picks parts, or the open part's scope.
pub fn has_context_options(editor: &Editor) -> bool {
    shows_part_scope(editor) || editor.tool_options().iter().any(is_context_option)
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
    ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color())).on_hover_text(tip);
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
            ];
            let (values, segments): (Vec<_>, Vec<_>) = signs.into_iter().unzip();
            if let Some(i) = segmented(ui, egui::Id::unique("option_bar_body_sign"), &segments, |i| {
                ["option:body_sign:negative", "option:body_sign:positive"][i].into()
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

/// The height of the option bar's buttons; an icon's button is a square of it.
const OPTION_HEIGHT: f32 = crate::style::BAR_BUTTON;
const OPTION_ICON: f32 = crate::style::BAR_ICON;
/// The side margins of a text button.
const OPTION_PAD: f32 = 8.0;
/// How long a segmented control's mark takes to slide to the value chosen, in seconds.
const SLIDE_TIME: f64 = 0.18;

/// What a button of the option bar shows: its icon (its label then in the tip), else its label.
enum Face {
    Icon(egui::ImageSource<'static>),
    Text(std::sync::Arc<egui::Galley>),
}

impl Face {
    fn new(ui: &egui::Ui, label: &str, icon: Option<&str>) -> Self {
        match icon.and_then(icons::option) {
            Some(icon) => Face::Icon(icon),
            None => Face::Text(ui.painter().layout_no_wrap(
                label.to_owned(),
                egui::TextStyle::Button.resolve(ui.style()),
                egui::Color32::PLACEHOLDER,
            )),
        }
    }

    /// Its button's width at `height`: a square for an icon.
    fn width(&self, height: f32) -> f32 {
        match self {
            Face::Icon(_) => height,
            Face::Text(g) => g.size().x + 2.0 * OPTION_PAD,
        }
    }

    fn paint(&self, ui: &egui::Ui, rect: egui::Rect, ink: egui::Color32) {
        match self {
            Face::Icon(icon) => egui::Image::new(icon.clone())
                .tint(ink)
                .paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(OPTION_ICON))),
            Face::Text(g) => ui.painter().galley(rect.center() - g.size() / 2.0, g.clone(), ink),
        }
    }
}

/// The tip of a button: an icon's adds its label.
fn face_tip(face: &Face, label: &str, tip: &str) -> String {
    match face {
        Face::Icon(_) => t!("option_bar.icon_tip", label = label, tip = tip),
        Face::Text(_) => tip.to_owned(),
    }
}

/// A length option's value, after its group's name: a [`framed_field`], lit when the value is
/// not zero. Esc or a click away puts the value back. The text typed, when applied.
fn length_option(ui: &mut egui::Ui, key: &str, tip: &str, value: f64, units: &Units) -> Option<String> {
    let shown = units.format(value);
    let exact = format!("{}{}", units.exact_number(value), units.suffix());
    let field = Field {
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

/// How a [`framed_field`] looks and applies.
struct Field<'a> {
    tip: Option<&'a str>,
    /// The whole width, its add-on's included.
    width: f32,
    /// A number's to the right, a name's to the left.
    align: egui::Align2,
    /// Outlined with the selection's colour (a tool option's value set).
    lit: bool,
    /// A letter in a colour, in a box on the left (an axis' `X`).
    addon: Option<(&'a str, egui::Color32)>,
    /// A click away applies what was typed (Entity Info), else puts the value back (the tool
    /// options, applied by Enter only).
    blur_applies: bool,
    /// An empty text applies too (a body's name cleared, docs/model/bodies.md#body-names), else it puts the
    /// value back.
    empty_applies: bool,
    /// Its name among the named handles (docs/control-protocol.md), if published.
    handle: Option<String>,
}

/// The outline of a [`framed_field`] (and of a [`material_choice`]): the border's colour at rest,
/// the hovered ink's when hovered, the selection's when lit, twice as thick when focused (or open).
fn field_stroke(visuals: &egui::Visuals, focused: bool, lit: bool, hovered: bool) -> egui::Stroke {
    if focused {
        egui::Stroke::new(2.0, visuals.selection.stroke.color)
    } else if lit {
        egui::Stroke::new(1.0, visuals.selection.stroke.color)
    } else if hovered {
        egui::Stroke::new(1.0, visuals.widgets.hovered.fg_stroke.color)
    } else {
        visuals.widgets.noninteractive.bg_stroke
    }
}

/// The one text field of the interface (the tool options', Entity Info's, the materials'):
/// always framed, the fields' ground, an outline telling its state — the border's colour at
/// rest, the hovered ink's when hovered, the selection's when lit, twice as thick when focused.
/// Focused, all of it is selected to be typed over: the `exact` text, as shown
/// as `shown` (`~ 12` edited as `12,346`), Enter on it unchanged applying nothing; Esc puts the
/// value back. The text typed, when applied.
fn framed_field(ui: &mut egui::Ui, id: egui::Id, shown: &str, exact: &str, f: Field) -> Option<String> {
    let field_id = id.with("field");
    // While focused, the text typed (kept across frames); else the value.
    // The outline from the state of the last frame (the field's is only known once drawn).
    let focused = ui.memory(|m| m.has_focus(field_id));
    // While focused, the text typed (kept across frames); else the value.
    let mut text: String = focused.then(|| ui.data(|d| d.get_temp(id))).flatten().unwrap_or_else(|| shown.to_owned());
    let hovered = ui.ctx().read_response(field_id).is_some_and(|r| r.hovered());
    let visuals = ui.visuals();
    // Faded with its card while it is translucent (the option bar's, docs/ui/interface.md#look),
    // its text not.
    let alpha = crate::style::fill_alpha(ui);
    let border = faded(visuals.widgets.noninteractive.bg_stroke, alpha);
    let stroke = faded(field_stroke(visuals, focused, f.lit, hovered), alpha);
    let radius = visuals.widgets.inactive.corner_radius;
    let addon = f.addon.map(|_| OPTION_HEIGHT).unwrap_or(0.0);
    let mut corners = radius;
    if addon > 0.0 {
        (corners.nw, corners.sw) = (0, 0);
    }
    // The margin keeps room for the thicker outline: focusing does not move the text.
    let frame = egui::Frame::new()
        .fill(visuals.extreme_bg_color.gamma_multiply(alpha))
        .stroke(stroke)
        .corner_radius(corners)
        .inner_margin(egui::Margin::symmetric(6, 2) - egui::Margin::from(stroke.width));
    let (ground, ink) = (visuals.widgets.inactive.bg_fill, visuals.text_color());

    // Its row as high as the field from the start: `ui.horizontal` starts one at
    // `interact_size.y`, centered in the parent's row, and grows it downwards to the field's
    // height — the field lower than its neighbours, the parent's row taller for those after it.
    let row = egui::vec2(f.width, OPTION_HEIGHT);
    let r = ui
        .allocate_ui_with_layout(row, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            // The add-on's border and the field's are one line: the field's, drawn over it.
            ui.spacing_mut().item_spacing.x = -border.width;
            if let Some((letter, color)) = f.addon {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(OPTION_HEIGHT), egui::Sense::hover());
                let p = ui.painter();
                let rect_corners = egui::CornerRadius { nw: radius.nw, sw: radius.sw, ne: 0, se: 0 };
                p.rect(rect, rect_corners, ground.lerp_to_gamma(ink, 0.04).gamma_multiply(alpha), border, egui::StrokeKind::Inside);
                let font = egui::TextStyle::Button.resolve(ui.style());
                p.text(rect.center(), egui::Align2::CENTER_CENTER, letter, font, color);
            }
            let width = f.width - addon + if addon > 0.0 { border.width } else { 0.0 };
            let field = egui::TextEdit::singleline(&mut text)
                .id(field_id)
                .frame(frame)
                .desired_width(width)
                .min_size(egui::vec2(width, OPTION_HEIGHT))
                .align(f.align);
            let r = ui.add(field);
            match f.tip {
                Some(tip) => r.on_hover_text(tip),
                None => r,
            }
        })
        .inner;
    if let Some(name) = f.handle {
        crate::handles::publish(|| name, r.rect);
    }

    // Just focused (clicked, tabbed into): the real value, all of it selected, to be typed over.
    if r.gained_focus()
        && let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), r.id)
    {
        text = exact.to_owned();
        ui.ctx().request_repaint();
        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
        state.cursor.set_char_range(Some(all));
        state.store(ui.ctx(), r.id);
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        let applied =
            ui.input(|i| if i.key_pressed(egui::Key::Escape) { false } else { f.blur_applies || i.key_pressed(egui::Key::Enter) });
        let unchanged = text.trim() == exact || text.trim() == shown;
        return (applied && (f.empty_applies || !text.trim().is_empty()) && !unchanged).then(|| text.trim().to_owned());
    }
    // egui's focus, not `Response::has_focus`: that one is also false while the window is not
    // the system's focused one, and the text typed then (control channel, docs/control-protocol.md) was lost.
    if ui.memory(|m| m.has_focus(r.id)) {
        ui.data_mut(|d| d.insert_temp(id, text));
    }
    None
}

/// A toggle of the option bar, drawn like a ribbon button: no frame until hovered, the
/// selection's fill and outline when on — faded with its card while it is translucent
/// (`style::fill_alpha`, docs/ui/interface.md#look), its ink not. True when clicked.
fn option_button(
    ui: &mut egui::Ui,
    handle: &str,
    selected: bool,
    label: &str,
    tip: &str,
    icon: Option<&str>,
    swatch: Option<[u8; 3]>,
) -> bool {
    let face = Face::new(ui, label, icon);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(face.width(OPTION_HEIGHT), OPTION_HEIGHT), egui::Sense::click());
    crate::handles::publish(|| handle.to_owned(), rect);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let visuals = ui.style().interact_selectable(&response, selected);
    let alpha = crate::style::fill_alpha(ui);
    let stroke = faded(visuals.bg_stroke, alpha);
    // On, filled with its colour (« Pipette »'s material, docs/tools/draw-options.md), as OCL's
    // button: the toggle's value, not faded.
    let swatch = swatch.filter(|_| selected).map(|[r, g, b]| egui::Color32::from_rgb(r, g, b));
    if let Some(fill) = swatch {
        ui.painter().rect(rect, visuals.corner_radius, fill, stroke, egui::StrokeKind::Inside);
    } else if selected || response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill.gamma_multiply(alpha), stroke, egui::StrokeKind::Inside);
    }
    let ink = match swatch {
        Some(fill) => ink_on(fill),
        None if selected => visuals.text_color(),
        None => ui.visuals().text_color(),
    };
    face.paint(ui, rect, ink);
    response.on_hover_text(face_tip(&face, label, tip)).clicked()
}

/// `stroke` faded by `alpha`.
fn faded(mut stroke: egui::Stroke, alpha: f32) -> egui::Stroke {
    stroke.color = stroke.color.gamma_multiply(alpha);
    stroke
}

/// The ink readable on `fill`: black on a light colour, white on a dark one (OCL's
/// `ColorUtils.color_is_dark?`).
fn ink_on(fill: egui::Color32) -> egui::Color32 {
    let l = egui::Rgba::from(fill);
    let luminance = 0.2126 * l.r() + 0.7152 * l.g() + 0.0722 * l.b();
    if luminance > 0.18 { egui::Color32::BLACK } else { egui::Color32::WHITE }
}

/// A value of a choice in its segmented control.
struct Segment {
    label: String,
    tip: String,
    icon: Option<&'static str>,
    selected: bool,
}

impl Segment {
    fn new(label: &str, tip: &str, icon: Option<&'static str>, selected: bool) -> Self {
        Segment { label: label.to_owned(), tip: tip.to_owned(), icon, selected }
    }
}

/// A choice as a segmented control: its values side by side, drawn like ribbon buttons, the
/// chosen one's mark (a selected ribbon button's fill and outline) sliding to the value chosen.
/// The index of a value clicked other than the chosen one.
/// Each segment is published as the handle `handle(index)` (docs/control-protocol.md).
fn segmented(ui: &mut egui::Ui, id: egui::Id, segments: &[Segment], handle: impl Fn(usize) -> String) -> Option<usize> {
    let faces: Vec<Face> = segments.iter().map(|s| Face::new(ui, &s.label, s.icon)).collect();
    let widths: Vec<f32> = faces.iter().map(|f| f.width(OPTION_HEIGHT)).collect();
    let (row, _) = ui.allocate_exact_size(egui::vec2(widths.iter().sum(), OPTION_HEIGHT), egui::Sense::hover());
    let radius = ui.visuals().widgets.inactive.corner_radius;
    // The marks faded with their card while it is translucent (docs/ui/interface.md#look).
    let alpha = crate::style::fill_alpha(ui);

    // The segments' places along the row, from its left.
    let lefts: Vec<f32> = widths.iter().scan(0.0, |x, w| Some(std::mem::replace(x, *x + w))).collect();
    let place = |i: usize| egui::Rect::from_min_size(row.min + egui::vec2(lefts[i], 0.0), egui::vec2(widths[i], OPTION_HEIGHT));

    let responses: Vec<egui::Response> = segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let response = ui.interact(place(i), id.with(i), egui::Sense::click());
            crate::handles::publish(|| handle(i), place(i));
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, s.selected, &s.label));
            if response.hovered() && !s.selected {
                let visuals = ui.style().interact(&response);
                ui.painter().rect_filled(place(i), radius, visuals.weak_bg_fill.gamma_multiply(alpha));
            }
            response
        })
        .collect();

    if let Some(c) = segments.iter().position(|s| s.selected) {
        let (left, right) = slide(ui, id.with("slide"), (lefts[c], lefts[c] + widths[c]));
        let mark = egui::Rect::from_x_y_ranges(row.left() + left..=row.left() + right, row.y_range());
        let selected = ui.style().interact_selectable(&responses[c], true);
        ui.painter().rect(
            mark,
            radius,
            selected.weak_bg_fill.gamma_multiply(alpha),
            faded(selected.bg_stroke, alpha),
            egui::StrokeKind::Inside,
        );
    }

    let mut clicked = None;
    for (i, (s, response)) in segments.iter().zip(responses).enumerate() {
        let ink = if s.selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
        faces[i].paint(ui, place(i), ink);
        if response.on_hover_text(face_tip(&faces[i], &s.label, &s.tip)).clicked() && !s.selected {
            clicked = Some(i);
        }
    }
    clicked
}

/// A segmented control's mark on its way to `to` (its left and right, from the row's left: a
/// moving bar does not slide it), easing out over [`SLIDE_TIME`] from wherever it was.
fn slide(ui: &egui::Ui, id: egui::Id, to: (f32, f32)) -> (f32, f32) {
    #[derive(Clone, Copy)]
    struct Slide {
        from: (f32, f32),
        to: (f32, f32),
        start: f64,
    }
    impl Slide {
        fn at(&self, now: f64) -> (f32, f32) {
            let t = ((now - self.start) / SLIDE_TIME).clamp(0.0, 1.0) as f32;
            let e = 1.0 - (1.0 - t).powi(3);
            (egui::lerp(self.from.0..=self.to.0, e), egui::lerp(self.from.1..=self.to.1, e))
        }
    }
    let now = ui.input(|i| i.time);
    let kept = ui.data(|d| d.get_temp::<Slide>(id));
    let s = match kept {
        Some(s) if s.to == to => s,
        // Shown at once the first time, else from where it is now.
        _ => {
            let s = match kept {
                Some(k) => Slide { from: k.at(now), to, start: now },
                None => Slide { from: to, to, start: now - SLIDE_TIME },
            };
            ui.data_mut(|d| d.insert_temp(id, s));
            s
        }
    };
    if now - s.start < SLIDE_TIME {
        ui.ctx().request_repaint();
    }
    s.at(now)
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

/// A row's fold: its key in its document's [`OutlinerState`], open or not.
#[derive(Clone, Copy)]
struct Fold {
    key: egui::Id,
    open: bool,
}

/// What the outliner keeps of a document between frames, in the document itself (closed with
/// it, never written to disk): the rows unfolded, the ids reborn, the active path last seen.
#[derive(Default)]
pub struct OutlinerState {
    /// The keys of the rows unfolded (a row missing: folded).
    open: FastSet<egui::Id>,
    births: Births,
    active: Option<ActivePath>,
}

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
    }
}

/// How far the weak text lies from the text towards the background, 0 to 1.
fn weak_share(v: &egui::Visuals) -> f32 {
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

/// The margin inside a panel's card, around its content (its border just outside).
pub const CARD_MARGIN: f32 = 6.0;

/// The background of a part's bodies in the outliner: white in the light theme, in the dark one
/// the card's a shade towards the text.
fn band_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode { v.window_fill.lerp_to_gamma(v.text_color(), 0.07) } else { egui::Color32::WHITE }
}

/// The text and icons of a hidden row (itself or by an assembly above): the weak text halfway
/// to the row's background (`band`: a part's bodies'), well behind the rows shown.
fn hidden_color(v: &egui::Visuals, band: bool) -> egui::Color32 {
    let back = if band { band_color(v) } else { v.window_fill };
    v.weak_text_color().lerp_to_gamma(back, 0.5)
}

/// One line of the outliner across the whole list: its background — `band` that of a part's
/// bodies, then the selection, or the hover (`lit`: hovered in the view) — painted under the
/// widgets `add` lays in it from left to right. The response is the line's click; the buttons
/// in it (a fold, an eye, a sign), added after, take theirs first.
fn tree_row<R>(
    ui: &mut egui::Ui,
    height: f32,
    selected: bool,
    lit: bool,
    band: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> (egui::Response, R) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    // Edge to edge of the list, its indent included, and half the spacing above and below: the
    // band of a part's bodies unbroken, the hover covering the line's whole height.
    let gap = ui.spacing().item_spacing.y / 2.0;
    // Out to the card's border, across its margin: the list's width (a vertical scroll area
    // does not clip across), widened by the margin, painted past the list's clip.
    let x = ui.max_rect().x_range();
    let clip = egui::Rect::from_x_y_ranges(x, ui.clip_rect().y_range()).expand2(egui::vec2(CARD_MARGIN, 0.0));
    let back = egui::Rect::from_x_y_ranges(clip.x_range(), rect.y_range()).expand2(egui::vec2(0.0, gap));
    let v = ui.visuals();
    let fill = if selected {
        Some(v.selection.bg_fill)
    } else if lit || ui.rect_contains_pointer(back) {
        Some(v.widgets.hovered.weak_bg_fill)
    } else if band {
        Some(band_color(v))
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().with_clip_rect(clip).rect_filled(back, 0.0, fill);
    }
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    let inner = add(&mut row);
    (response, inner)
}

/// The outliner, in `area`: the model (named `name`, its file's) on the first row, then the
/// tree; only the rows in view are laid out (a model can hold tens of thousands of
/// occurrences).
pub fn outliner(
    ui: &mut egui::Ui,
    editor: &Editor,
    state: &mut OutlinerState,
    name: &str,
    actions: &mut Vec<Action>,
    area: egui::ScrollArea,
) {
    let doc = editor.document();
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
    flatten(&state.births, &mut state.open, editor, doc, doc.root(), &active.occurrences, &mut Vec::new(), &mut rows, false, 0);
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
            let open = editor.open_part() == Some(o.id) && selection.is_empty() && editing.is_none();
            let selected = (editor.context() == row.path.as_slice() && (selection.contains(&o.id) || open))
                || editor.deep_selection().iter().any(|p| p.instances == row.path && p.entity == o.id);
            // Hidden, itself or by an assembly above: greyed.
            let greyed = o.hidden || row.hidden_above;
            let tint = if selected {
                on_selection
            } else if greyed {
                hidden_color(ui.visuals(), false)
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
                    let eye = ui.add(egui::Button::image(img).frame(false)).on_hover_text(tip).clicked();
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
                Some(tip) => response.on_hover_text(tip),
                None => response,
            };
            let path = EntityPath::new(row.path, o.id);
            // The whole row, its indent and toggle included.
            if ui.rect_contains_pointer(response.rect) {
                over = Some(path.clone());
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
            if eye {
                actions.push(Action::SetHidden(path, !o.hidden));
            } else if response.clicked() {
                actions.push(Action::SelectPath(path, ui.input(|i| i.modifiers.shift)));
            }
        }
    });
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
    response.on_hover_text(&m.name);
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
    if response.on_hover_text(t!("panel.outliner.model_tip")).clicked() {
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
            out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: Some(Fold { key, open }), hidden_above, body: None });
            if open {
                let open_group = (editor.context() == path.as_slice() && editor.open_part() == Some(o.id)).then(|| editor.open_group());
                let row = |fold, group, body, siblings, profile| OutlinerRow {
                    path: path.clone(),
                    occurrence: o,
                    fold,
                    hidden_above,
                    body: Some(BodyRow { part: p, body, group, siblings, profile }),
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
            out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: None, hidden_above, body: None });
            continue;
        }
        let key = births.occurrence_fold(path, o.id);
        // Open along the context being edited.
        let along = editor.context().len() > path.len() && editor.context().starts_with(path) && editor.context()[path.len()] == o.id;
        let open = unfold(folds, key, along);
        out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: Some(Fold { key, open }), hidden_above, body: None });
        if open {
            path.push(o.id);
            flatten(births, folds, editor, doc, o.component, active, path, out, hidden_above || o.hidden, depth + 1);
            path.pop();
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
    if response.on_hover_text(t!("panel.outliner.profile_tip")).clicked() && !selected {
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
    let selected = shown && editor.selection().contains(&b.body.id);
    let lit = shown && editor.hovered_machining() == Some(b.body.id);
    let greyed = row.occurrence.hidden || row.hidden_above;
    let v = ui.visuals();
    let weak = v.weak_text_color();
    let tint = if selected {
        v.selection.stroke.color
    } else if greyed {
        hidden_color(v, true)
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
            };
            let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(if selected { tint } else { weak });
            let toggled = ui.add(egui::Button::image(img).frame(false)).on_hover_text(tip).clicked();
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
                    (None, split)
                })
                .inner;
            (toggled, renamed.0, renamed.1)
        })
        .inner
    });
    let response = if warned {
        let tip = match (inner, def.solid.is_empty()) {
            (true, _) => t!("panel.outliner.failed_inside_group"),
            (false, true) => t!("panel.outliner.empty_group"),
            (false, false) => t!("panel.outliner.failed_group"),
        };
        response.on_hover_text(tip)
    } else {
        response
    };
    if shown && ui.rect_contains_pointer(response.rect) {
        *over = Some(b.body.id);
    }
    let alone = selected && editor.selection().len() == 1;
    let on_own = match (split, response.interact_pointer_pos()) {
        (Some(split), Some(at)) if response.double_clicked() => at.x < split,
        (split, _) => split.is_some(),
    };
    let key = if on_own { rename_own } else { rename };
    let part = row.occurrence.component;
    match (renaming, start_rename(ui, &response, renaming.is_some(), alone, key, renamed)) {
        (Some(true), Some(Some(name))) => actions.push(Action::Apply(Operation::RenameBody { part, body: b.body.id, name })),
        (_, Some(Some(name))) => actions.push(Action::Apply(Operation::RenameBodyDef { part, def: b.body.def, name })),
        _ => {}
    }
    if toggled {
        let sign = match b.body.sign {
            Sign::Positive => Sign::Negative,
            Sign::Negative => Sign::Positive,
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

/// The temporary key of the body row whose name is edited in place, if any.
const RENAMING: &str = "outliner_renaming";

/// A navigator row's name edited in place, under the key `rename` (`renaming`: it is): a
/// double-click on the row (`response`), or F2 while it is the one thing selected (`alone`),
/// opens its field; `renamed`, the field's outcome, closes it and is given back.
fn start_rename(
    ui: &mut egui::Ui,
    response: &egui::Response,
    renaming: bool,
    alone: bool,
    rename: egui::Id,
    renamed: Option<Option<String>>,
) -> Option<Option<String>> {
    if renamed.is_some() {
        ui.data_mut(|d| d.remove::<egui::Id>(egui::Id::unique(RENAMING)));
    } else if !renaming {
        let f2 = alone && ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(egui::Key::F2));
        if response.double_clicked() || f2 {
            ui.data_mut(|d| d.insert_temp(egui::Id::unique(RENAMING), rename));
        }
    }
    renamed
}

/// A row's name (a body's, a component's) edited in place in its navigator row (docs/model/bodies.md#body-names), under the key
/// `id`: the name `current` wholly selected as it opens, to be typed over. Enter or a click away
/// gives the text typed — empty: the name cleared —, Esc nothing: `Some` once done, `Some(None)`
/// when nothing changes; None while it is edited.
fn rename_field(ui: &mut egui::Ui, id: egui::Id, current: &str) -> Option<Option<String>> {
    let (text_id, started) = (id.with("text"), id.with("started"));
    let first = !ui.data(|d| d.get_temp::<bool>(started).unwrap_or(false));
    let mut text = ui.data(|d| d.get_temp::<String>(text_id)).unwrap_or_else(|| current.to_owned());
    // Centred in the row as the name it replaces: egui's theme gives the field the row's height
    // (`interact_size`), taller than a line of text, and would keep the text at its top.
    let edit = egui::TextEdit::singleline(&mut text)
        .id(id.with("edit"))
        .desired_width(ui.available_width())
        .margin(egui::vec2(2.0, 0.0))
        .align(egui::Align2::LEFT_CENTER);
    let r = ui.add(edit);
    let done = |ui: &mut egui::Ui| ui.data_mut(|d| (d.remove::<String>(text_id), d.remove::<bool>(started)));
    // Focused and all of it selected as it opens (and whenever focus comes back to it).
    if first || r.gained_focus() {
        if first {
            r.request_focus();
            ui.data_mut(|d| d.insert_temp(started, true));
        }
        let mut state = egui::TextEdit::load_state(ui.ctx(), r.id).unwrap_or_default();
        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
        state.cursor.set_char_range(Some(all));
        state.store(ui.ctx(), r.id);
        ui.ctx().request_repaint();
    }
    // egui's focus, as `framed_field`'s (the window not the system's focused one).
    let focused = ui.memory(|m| m.has_focus(r.id));
    if r.lost_focus() || (!first && !focused && !r.gained_focus()) {
        done(ui);
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let name = text.trim();
        return Some((!escaped && name != current).then(|| name.to_owned()));
    }
    ui.data_mut(|d| d.insert_temp(text_id, text));
    None
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
            // Room left for the value column (labels ~ 80 pt, plus the grid spacing).
            let field = (ui.available_width() - 90.0).max(60.0);
            egui::Grid::new("info").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
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

/// The model, chosen (the navigator's first row clicked): its name (`name`, its file's), its
/// parts and assemblies as placed, its materials; its properties and the definitions of all
/// (docs/model/properties.md).
fn model_info(ui: &mut egui::Ui, doc: &Document, name: &str, actions: &mut Vec<Action>) {
    let (parts, assemblies) = placed_counts(doc, doc.root(), &mut FastMap::default(), 0);
    let field = (ui.available_width() - 90.0).max(60.0);
    egui::Grid::new("info").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
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
    egui::Grid::new("info").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
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
    egui::Grid::new("info").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
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
    let field = (ui.available_width() - 90.0).max(60.0);
    egui::Grid::new("info").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(if def.is_group() { t!("panel.entity_info.type_group") } else { t!("panel.entity_info.type_body") });
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
        });
        ui.end_row();
        if p.is_failed(b.id) {
            ui.label(t!("panel.entity_info.state"));
            let error = if def.solid.is_empty() { t!("panel.entity_info.empty") } else { t!("panel.entity_info.failed") };
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
const NEW_COLORS: [[u8; 3]; 6] = [[196, 152, 98], [120, 78, 48], [222, 196, 150], [160, 110, 70], [235, 235, 228], [90, 92, 96]];

/// A swatch of `color`, `size` points square.
fn swatch(ui: &mut egui::Ui, color: Option<[u8; 3]>, size: f32) {
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

/// The Matières panel (docs/render/look.md): the document's materials — colour, name, delete — and the
/// one the paint tool paints with; picking one starts the paint tool.
pub fn materials(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let painting = editor.tool_kind() == skraft_tools::ToolKind::Paint;
    let current = editor.paint_material();
    let tint = ui.visuals().text_color();
    let brush = |ui: &mut egui::Ui, on: bool, tip: &str| {
        let img = egui::Image::new(icons::PAINT).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint);
        ui.add(egui::Button::image(img).selected(on)).on_hover_text(tip).clicked()
    };
    ui.horizontal(|ui| {
        if brush(ui, painting && current.is_none(), &t!("panel.materials.unpaint_tip")) {
            actions.push(Action::PaintWith(None));
        }
        swatch(ui, None, 16.0);
        ui.label(t!("panel.materials.none"));
    });
    for (id, m) in doc.materials() {
        ui.horizontal(|ui| {
            if brush(ui, painting && current == Some(id), &t!("panel.materials.paint_tip")) {
                actions.push(Action::PaintWith(Some(id)));
            }
            let mut c = m.color;
            if ui.color_edit_button_srgb(&mut c).on_hover_text(t!("panel.materials.color_tip")).changed() {
                actions.push(Action::Apply(Operation::EditMaterial { material: id, name: m.name.clone(), color: c }));
            }
            // Room left for the texture and delete buttons, as high as the field
            // (`OPTION_HEIGHT`) and square.
            let width = (ui.available_width() - 2.0 * (OPTION_HEIGHT + ui.spacing().item_spacing.x)).max(60.0);
            if let Some(name) = text_field(ui, (id, 2), &m.name, width) {
                actions.push(Action::Apply(Operation::EditMaterial { material: id, name, color: m.color }));
            }
            // As high as the name's field beside it.
            let h = OPTION_HEIGHT;
            let ink = ui.visuals().text_color();
            // Its texture's thumbnail, else the texture icon: a click picks an image.
            let thumb = m.texture.as_ref().and_then(|t| thumbnail(ui.ctx(), doc, &t.image));
            let clicked = match &thumb {
                Some(tex) => {
                    // Its middle, square: the whole image under the pointer.
                    let [w, t] = tex.size().map(|v| v.max(1) as f32);
                    let (du, dv) = ((1.0 - (t / w).min(1.0)) / 2.0, (1.0 - (w / t).min(1.0)) / 2.0);
                    let uv = egui::Rect::from_min_max(egui::pos2(du, dv), egui::pos2(1.0 - du, 1.0 - dv));
                    let img = egui::Image::new(tex).uv(uv).maintain_aspect_ratio(false).fit_to_exact_size(egui::vec2(h, h)).corner_radius(3.0);
                    let r = ui.add(egui::Button::image(img).frame(false).min_size(egui::vec2(h, h)));
                    r.on_hover_ui(|ui| {
                        ui.label(t!("panel.materials.texture_change_tip"));
                        ui.add(egui::Image::new(tex).max_size(egui::vec2(THUMB_PREVIEW, THUMB_PREVIEW)).corner_radius(4.0));
                    })
                    .clicked()
                }
                None => {
                    let texture = egui::Image::new(icons::TEXTURE).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(ink);
                    let tip = if m.texture.is_some() { t!("panel.materials.texture_change_tip") } else { t!("panel.materials.texture_tip") };
                    ui.add(egui::Button::image(texture).selected(m.texture.is_some()).min_size(egui::vec2(h, h))).on_hover_text(tip).clicked()
                }
            };
            if clicked {
                actions.push(Action::PickTexture(id));
            }
            let delete = egui::Image::new(icons::CLOSE).fit_to_exact_size(egui::vec2(12.0, 12.0)).tint(ink);
            if ui.add(egui::Button::image(delete).min_size(egui::vec2(h, h))).on_hover_text(t!("panel.materials.delete_tip")).clicked() {
                actions.push(Action::Apply(Operation::DeleteMaterial { material: id }));
            }
        });
        if let Some(texture) = &m.texture {
            texture_row(ui, doc, id, texture, actions);
        }
    }
    let ink = ui.visuals().text_color();
    let icon = |i: egui::ImageSource<'static>| egui::Image::new(i).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(ink);
    ui.horizontal_wrapped(|ui| {
        if ui.add(egui::Button::image_and_text(icon(icons::ADD), t!("panel.materials.new"))).clicked() {
            let count = doc.materials().count();
            let taken = |n: &str| doc.materials().any(|(_, m)| m.name == n);
            let name = (count + 1..).map(|k| t!("default_name.material", number = k)).find(|n| !taken(n)).unwrap_or_default();
            actions.push(Action::Apply(Operation::AddMaterial { name, color: NEW_COLORS[count % NEW_COLORS.len()] }));
        }
        let none = doc.materials().next().is_none();
        // A menu: all of them, or one.
        let export = ui.add_enabled(!none, egui::Button::image(icon(icons::EXPORT)).min_size(egui::vec2(OPTION_HEIGHT, OPTION_HEIGHT)));
        let export = export.on_hover_text(t!("panel.materials.export_tip"));
        egui::Popup::menu(&export).id(ui.make_persistent_id("export_materials")).style(crate::style::menu).show(|ui| {
            ui.set_min_width(160.0);
            if ui.button(t!("panel.materials.export_all")).clicked() {
                actions.push(Action::ExportMaterials(None));
            }
            ui.separator();
            for (id, m) in doc.materials() {
                let thumb = m.texture.as_ref().and_then(|t| thumbnail(ui.ctx(), doc, &t.image));
                let r = ui.horizontal(|ui| {
                    match &thumb {
                        Some(tex) => {
                            ui.add(egui::Image::new(tex).maintain_aspect_ratio(false).fit_to_exact_size(egui::vec2(12.0, 12.0)).corner_radius(2.0));
                        }
                        None => swatch(ui, Some(m.color), 12.0),
                    }
                    ui.button(&m.name).clicked()
                });
                if r.inner {
                    actions.push(Action::ExportMaterials(Some(id)));
                }
            }
        });
        let import = ui.add(egui::Button::image(icon(icons::IMPORT)).min_size(egui::vec2(OPTION_HEIGHT, OPTION_HEIGHT)));
        if import.on_hover_text(t!("panel.materials.import_tip")).clicked() {
            actions.push(Action::ImportMaterials);
        }
    });
}

/// A texture's thumbnail's side (texels), and its preview's (points) under the pointer.
const THUMB_SIZE: u32 = 128;
const THUMB_PREVIEW: f32 = 160.0;

/// The thumbnail of the image `id` of `doc`, made once (kept in egui's temporary memory by
/// image id: the same image, the same thumbnail in every document); none when it does not
/// read.
fn thumbnail(ctx: &egui::Context, doc: &Document, id: &str) -> Option<egui::TextureHandle> {
    let key = egui::Id::unique(("texture_thumbnail", id));
    if let Some(made) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(key)) {
        return made;
    }
    let made = doc.asset(id).and_then(|a| image::load_from_memory(&a.bytes).ok()).map(|img| {
        let img = img.thumbnail(THUMB_SIZE, THUMB_SIZE).to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        ctx.load_texture(format!("thumbnail {id}"), egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
    });
    ctx.data_mut(|d| d.insert_temp(key, made.clone()));
    made
}

/// A material's texture, under its row: its real size (width along the grain × height) and
/// its rotation, typed as the measurements box reads them; a button to remove it.
fn texture_row(ui: &mut egui::Ui, doc: &Document, id: Id, texture: &skraft_model::Texture, actions: &mut Vec<Action>) {
    let units = &doc.settings().units;
    let set = |t: skraft_model::Texture| Action::Apply(Operation::SetTexture { material: id, texture: Some(t), image: None });
    fn field(width: f32, tip: &str) -> Field<'_> {
        Field {
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
    ui.horizontal(|ui| {
        // Under the name, past the brush and the colour.
        ui.add_space(2.0 * OPTION_HEIGHT);
        let room = ui.available_width() - OPTION_HEIGHT - 4.0 * ui.spacing().item_spacing.x - 12.0;
        let (size_w, angle_w) = ((room * 0.36).max(48.0), (room * 0.28).max(40.0));
        let size_tips = [t!("panel.materials.texture_width_tip"), t!("panel.materials.texture_height_tip")];
        for (k, tip) in size_tips.iter().enumerate() {
            let v = texture.size[k];
            let (shown, exact) = (units.format(v), format!("{}{}", units.exact_number(v), units.suffix()));
            let fid = ui.make_persistent_id(("texture_size", id, k));
            if let Some(text) = ui.vertical(|ui| framed_field(ui, fid, &shown, &exact, field(size_w, tip))).inner
                && let Ok(mm) = measure::length(&text, units, v)
                && mm > 0.0
            {
                let mut t = texture.clone();
                t.size[k] = mm;
                actions.push(set(t));
            }
            if k == 0 {
                ui.label("×");
            }
        }
        let deg = texture.rotation.to_degrees();
        let shown = format!("{}°", skraft_i18n::number(&format!("{}", (deg * 100.0).round() / 100.0)));
        let tip = t!("panel.materials.texture_rotation_tip");
        let fid = ui.make_persistent_id(("texture_rotation", id));
        if let Some(text) = ui.vertical(|ui| framed_field(ui, fid, &shown, &shown, field(angle_w, &tip))).inner
            && let Ok(d) = measure::angle(&text, deg)
        {
            actions.push(set(skraft_model::Texture { rotation: d.to_radians(), ..texture.clone() }));
        }
        let close = egui::Image::new(icons::CLOSE).fit_to_exact_size(egui::vec2(12.0, 12.0)).tint(ui.visuals().text_color());
        let h = OPTION_HEIGHT;
        if ui.add(egui::Button::image(close).min_size(egui::vec2(h, h))).on_hover_text(t!("panel.materials.texture_remove_tip")).clicked() {
            actions.push(Action::Apply(Operation::SetTexture { material: id, texture: None, image: None }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mark's place at `time` (seconds), heading to `to`.
    fn slide_at(ctx: &egui::Context, time: f64, to: (f32, f32)) -> (f32, f32) {
        let mut at = (f32::NAN, f32::NAN);
        let input = egui::RawInput { time: Some(time), ..Default::default() };
        ctx.run_ui(input, |ui| at = slide(ui, egui::Id::unique("test_slide"), to)).textures_delta.clear();
        at
    }

    #[test]
    fn the_mark_is_shown_at_once_then_slides_to_the_value_chosen() {
        let ctx = egui::Context::default();
        let (a, b) = ((0.0, 26.0), (52.0, 78.0));
        assert_eq!(slide_at(&ctx, 1.0, a), a, "no slide the first time");
        assert_eq!(slide_at(&ctx, 1.1, a), a);
        let start = slide_at(&ctx, 1.1 + SLIDE_TIME / 2.0, b);
        assert_eq!(start, a, "starts where it was");
        let half = slide_at(&ctx, 1.1 + SLIDE_TIME, b);
        assert!(a.0 < half.0 && half.0 < b.0 && half.1 - half.0 == 26.0, "on its way: {half:?}");
        assert_eq!(slide_at(&ctx, 1.1 + 2.0 * SLIDE_TIME, b), b);
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

    #[test]
    fn the_rename_field_keeps_the_name_where_the_row_had_it() {
        let ctx = egui::Context::default();
        crate::style::install_fonts(&ctx);
        let label = text_top(&ctx, false);
        assert!(label > 0.0, "the row's name centred in it: {label}");
        assert_eq!(text_top(&ctx, true), label);
    }
}

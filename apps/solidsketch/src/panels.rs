//! Side panels: the entity info, the materials and the outliner, the tool option bar and the
//! menu items, all drawn from the command registry's state (docs/ARCHITECTURE.md
//! §7 quater).

use glam::{DAffine3, DVec3};
use solidsketch_i18n::{t, tp};
use solidsketch_model::{
    ComponentKind, Document, EntityPath, FastMap, FastSet, GuideShape, Id, Nature, Occurrence, Operation, Scope, Sign, Units, measure,
};
use solidsketch_tools::{Editor, OptionValue, ToolKind};

use crate::commands::Menu;
use crate::icons;

/// What a panel asks the app to do (applied after drawing, so the panels only read).
pub enum Action {
    /// A command of the registry, by id.
    Command(&'static str),
    /// The control channel failed to start: try again (the title bar's mark).
    RemoteRetry,
    /// An outliner's click: selects or deselects, Shift (`true`) adding to the selection.
    SelectPath(EntityPath, bool),
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
    /// The "Machinings" switch of an open part (ARCHITECTURE.md §2.9), kept in the preferences.
    ToggleMachinings,
    /// The open part's profile edited, or not (the tool « Profil », §6 nonies, step 3).
    ToggleProfile,
    /// What changes to the open part reach (§2.5).
    PartScope(Scope),
    /// Whether shapes drawn in the open part add material or take it away (§2.11).
    BodySign(Sign),
    /// The body of the open part hovered in the outliner, if any (its part open on it, the
    /// bodies shown).
    HoverMachining(Option<Id>),
    /// A body of a part chosen in the outliner (§2.11 « Body groups »): the part, and the group
    /// the path names, opened; the body selected or deselected, Shift (`true`) adding it.
    SelectBody(EntityPath, Id, bool),
    /// A body's « Profil » row chosen in the outliner: its part (and group) opened, its profile
    /// edited.
    EditProfile(EntityPath, Id),
    /// The occurrence hovered in the outliner, if any.
    HoverOutliner(Option<EntityPath>),
    /// Hide the occurrence (true) or show it again: the outliner's eye.
    SetHidden(EntityPath, bool),
    /// The ribbon's own state: tab, workspace, pins, labels (docs/ARCHITECTURE.md §6 septies).
    Ribbon(crate::ribbon::Change),
    /// A tool started from the quick tools' card, on what was just made (§6 terdecies).
    QuickTool(&'static str),
    /// A command of the quick tools' card pinned, or unpinned.
    QuickPin(&'static str),
    /// On the file tabs (docs/ARCHITECTURE.md §6 octies).
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

/// Whether the option bar has anything to show: the active tool's options, or an open part's
/// switches.
pub fn has_tool_options(editor: &Editor) -> bool {
    editor.open_part().is_some() || !editor.tool_options().is_empty()
}

/// The active tool's option bar (docs/ARCHITECTURE.md §6 bis): its choices, each a segmented
/// control after its name, then its lone toggles together in an "Options" group, all drawn like
/// the ribbon's buttons. An open part puts its scope first and its "Machinings" switch among
/// the toggles (§2.9).
pub fn tool_options(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let options = editor.tool_options();
    ui.horizontal(|ui| {
        // The row as high as its buttons from the start: the first label is centered on them.
        ui.set_min_height(OPTION_HEIGHT);
        let mut first = true;
        let mut group = |ui: &mut egui::Ui, label: &str, tip: &str| {
            if !std::mem::take(&mut first) {
                ui.separator();
            }
            // A group's name grey, a caption: its buttons are what the eye looks for.
            ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color())).on_hover_text(tip);
        };
        if editor.open_part().is_some() {
            group(ui, &t!("tool.boolean.option.scope"), &t!("option_bar.part_scope_tip"));
            let all = match editor.open_part_copies() {
                n if n > 1 => t!("tool.boolean.option.all_count", count = n),
                _ => t!("tool.boolean.option.all_one"),
            };
            let scope = editor.part_scope();
            let scopes = [
                (Scope::All, Segment::new(&all, &t!("tool.boolean.option.all_tip"), None, scope == Scope::All)),
                (
                    Scope::This,
                    Segment::new(&t!("tool.boolean.option.this"), &t!("tool.boolean.option.this_tip"), None, scope == Scope::This),
                ),
            ];
            let (values, segments): (Vec<_>, Vec<_>) = scopes.into_iter().unzip();
            if let Some(i) = segmented(ui, egui::Id::unique("option_bar_part_scope"), &segments, |i| {
                ["option:part_scope:all", "option:part_scope:this"][i].into()
            }) {
                actions.push(Action::PartScope(values[i]));
            }
            // A shape drawn in it: a machining, or material added (§2.11).
            if matches!(editor.tool_kind(), ToolKind::Box | ToolKind::Cylinder | ToolKind::Prism | ToolKind::Turn) {
                group(ui, &t!("option_bar.body_sign"), &t!("option_bar.body_sign_tip"));
                let sign = editor.body_sign();
                let signs = [
                    (Sign::Negative, Segment::new(&t!("option_bar.remove"), &t!("option_bar.remove_tip"), None, sign == Sign::Negative)),
                    (Sign::Positive, Segment::new(&t!("option_bar.add"), &t!("option_bar.add_tip"), None, sign == Sign::Positive)),
                ];
                let (values, segments): (Vec<_>, Vec<_>) = signs.into_iter().unzip();
                if let Some(i) = segmented(ui, egui::Id::unique("option_bar_body_sign"), &segments, |i| {
                    ["option:body_sign:negative", "option:body_sign:positive"][i].into()
                }) {
                    actions.push(Action::BodySign(values[i]));
                }
            }
        }
        for o in &options {
            let OptionValue::Length(v) = o.value else { continue };
            group(ui, &o.label, &o.tip);
            if let Some(text) = length_option(ui, o.key, &o.tip, v, &editor.document().settings().units) {
                actions.push(Action::ToolLength(o.key, text));
            }
        }
        for o in &options {
            let OptionValue::Choice(current) = &o.value else { continue };
            group(ui, &o.label, &o.tip);
            let segments: Vec<_> = o.choices.iter().map(|c| Segment::new(&c.label, &c.tip, c.icon, current == c.value)).collect();
            if let Some(c) =
                segmented(ui, egui::Id::unique(("option_bar", o.key)), &segments, |i| format!("option:{}:{}", o.key, o.choices[i].value))
            {
                actions.push(Action::ToolOption(o.key, OptionValue::Choice(o.choices[c].value.to_owned())));
            }
        }
        let toggles: Vec<_> =
            options.iter().filter_map(|o| if let OptionValue::Toggle(on) = o.value { Some((o, on)) } else { None }).collect();
        if toggles.is_empty() && editor.open_part().is_none() {
            return;
        }
        group(ui, &t!("option_bar.options"), &t!("option_bar.options_tip"));
        // Side by side like the ribbon's buttons.
        ui.spacing_mut().item_spacing.x = crate::ribbon::BUTTON_GAP;
        if editor.open_part().is_some() {
            let on = editor.machinings_switch();
            if option_button(ui, "option:machinings", on, &t!("option_bar.machinings"), &t!("option_bar.machinings_tip"), None) {
                actions.push(Action::ToggleMachinings);
            }
            if editor.open_part_profile() {
                let on = editor.tool_kind() == ToolKind::Profile;
                if option_button(ui, "option:profile", on, &t!("option_bar.profile"), &t!("option_bar.profile_tip"), None) {
                    actions.push(Action::ToggleProfile);
                }
            }
        }
        for (o, on) in toggles {
            if option_button(ui, &format!("option:{}", o.key), on, &o.label, &o.tip, o.icon) {
                actions.push(Action::ToolOption(o.key, OptionValue::Toggle(!on)));
            }
        }
    });
}

/// The height of the option bar's buttons; an icon's button is a square of it.
const OPTION_HEIGHT: f32 = 26.0;
const OPTION_ICON: f32 = 18.0;
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
    /// An empty text applies too (a body's name cleared, §2.11 « Body names »), else it puts the
    /// value back.
    empty_applies: bool,
    /// Its name among the named handles (docs/ARCHITECTURE.md §7 septies), if published.
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
    let border = visuals.widgets.noninteractive.bg_stroke;
    let stroke = field_stroke(visuals, focused, f.lit, hovered);
    let radius = visuals.widgets.inactive.corner_radius;
    let addon = f.addon.map(|_| OPTION_HEIGHT).unwrap_or(0.0);
    let mut corners = radius;
    if addon > 0.0 {
        (corners.nw, corners.sw) = (0, 0);
    }
    // The margin keeps room for the thicker outline: focusing does not move the text.
    let frame = egui::Frame::new()
        .fill(visuals.extreme_bg_color)
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
                p.rect(rect, rect_corners, ground.lerp_to_gamma(ink, 0.04), border, egui::StrokeKind::Inside);
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
    // the system's focused one, and the text typed then (control channel, §7 septies) was lost.
    if ui.memory(|m| m.has_focus(r.id)) {
        ui.data_mut(|d| d.insert_temp(id, text));
    }
    None
}

/// A toggle of the option bar, drawn like a ribbon button: no frame until hovered, the
/// selection's fill and outline when on. True when clicked.
fn option_button(ui: &mut egui::Ui, handle: &str, selected: bool, label: &str, tip: &str, icon: Option<&str>) -> bool {
    let face = Face::new(ui, label, icon);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(face.width(OPTION_HEIGHT), OPTION_HEIGHT), egui::Sense::click());
    crate::handles::publish(|| handle.to_owned(), rect);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
    }
    let ink = if selected { visuals.text_color() } else { ui.visuals().text_color() };
    face.paint(ui, rect, ink);
    response.on_hover_text(face_tip(&face, label, tip)).clicked()
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
/// Each segment is published as the handle `handle(index)` (§7 septies).
fn segmented(ui: &mut egui::Ui, id: egui::Id, segments: &[Segment], handle: impl Fn(usize) -> String) -> Option<usize> {
    let faces: Vec<Face> = segments.iter().map(|s| Face::new(ui, &s.label, s.icon)).collect();
    let widths: Vec<f32> = faces.iter().map(|f| f.width(OPTION_HEIGHT)).collect();
    let (row, _) = ui.allocate_exact_size(egui::vec2(widths.iter().sum(), OPTION_HEIGHT), egui::Sense::hover());
    let radius = ui.visuals().widgets.inactive.corner_radius;

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
                ui.painter().rect_filled(place(i), radius, visuals.weak_bg_fill);
            }
            response
        })
        .collect();

    if let Some(c) = segments.iter().position(|s| s.selected) {
        let (left, right) = slide(ui, id.with("slide"), (lefts[c], lefts[c] + widths[c]));
        let mark = egui::Rect::from_x_y_ranges(row.left() + left..=row.left() + right, row.y_range());
        let selected = ui.style().interact_selectable(&responses[c], true);
        ui.painter().rect(mark, radius, selected.weak_bg_fill, selected.bg_stroke, egui::StrokeKind::Inside);
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
    if solidsketch_i18n::mac() { "⌫".to_owned() } else { t!("key.delete") }
}

// ---- outliner ----------------------------------------------------------------------------------

fn occurrence_icon(doc: &Document, o: &Occurrence) -> egui::ImageSource<'static> {
    if doc.part(o.component).is_ok() { icons::BOX } else { icons::COMPONENT }
}

/// The name of an instance — an occurrence, a body — in its navigator row, its colour `color`:
/// its definition's (`def`: its component's, its body definition's), after its own where it has
/// one (« Gauche <Pied> »), the definition's in italics and attenuated when automatic (`auto`,
/// §2.1, §2.11 « Automatic names »), on `back`. Its response, and where its own name ends and
/// the definition's starts (halfway across the space between them) where it has one.
fn instance_label(
    ui: &mut egui::Ui,
    own: &str,
    def: &str,
    auto: bool,
    color: egui::Color32,
    selected: bool,
    back: egui::Color32,
) -> (egui::Response, Option<f32>) {
    let plain = name_format(ui, color, false, selected, back);
    let mut job = egui::text::LayoutJob::default();
    if !own.is_empty() {
        job.append(&format!("{own} <"), 0.0, plain.clone());
    }
    job.append(def, 0.0, name_format(ui, color, auto, selected, back));
    if !own.is_empty() {
        job.append(">", 0.0, plain);
    }
    let r = ui.add(egui::Label::new(job).selectable(false).truncate());
    let split = (!own.is_empty()).then(|| {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let width = |t: String| ui.fonts_mut(|f| f.layout_no_wrap(t, font.clone(), color).size().x);
        r.rect.min.x + (width(own.to_owned()) + width(format!("{own} "))) / 2.0
    });
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

/// A body in the outliner, under its part (§2.11 « Body groups »).
struct BodyRow<'d> {
    part: &'d solidsketch_model::Part,
    body: &'d solidsketch_model::Body,
    /// The groups it lies in, the part's own body first: a click opens them.
    group: Vec<Id>,
    /// Its siblings, itself included: the bodies of the part or group it lies in.
    siblings: &'d [solidsketch_model::Body],
    /// The line is the body's profile's, under it, not the body's own.
    profile: bool,
}

/// The icon of a body's nature (§2.11 « Body names »).
fn nature_icon(nature: Nature) -> egui::ImageSource<'static> {
    match nature {
        Nature::Extrusion => icons::EXTRUSION,
        Nature::Revolution => icons::REVOLUTION,
        Nature::Solid => icons::SOLID,
        Nature::Group => icons::GROUP,
    }
}

/// A name in the navigator, its colour `color`: given by the user, as it is; automatic
/// (« Pièce#2 », « Extrusion#1 », §2.1, §2.11 « Body names »), in italics, attenuated — towards
/// `back` as the weak text is, unless selected (`color` then the selection's).
fn name_format(ui: &egui::Ui, color: egui::Color32, auto: bool, selected: bool, back: egui::Color32) -> egui::TextFormat {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let v = ui.visuals();
    let color = match auto && !selected {
        // As far from the text as the weak text is from it, wherever the text stands.
        true => color.lerp_to_gamma(back, weak_share(v)),
        false => color,
    };
    egui::TextFormat { font_id, color, italics: auto, ..Default::default() }
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

/// What the interface calls the body `b` of `p`: its definition's name (§2.11 « Body names »),
/// automatic or not.
fn body_label(p: &solidsketch_model::Part, b: &solidsketch_model::Body) -> String {
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
            let selected = editor.context() == row.path.as_slice() && (selection.contains(&o.id) || open);
            // Hidden, itself or by an assembly above: greyed.
            let greyed = o.hidden || row.hidden_above;
            let tint = if selected {
                on_selection
            } else if greyed {
                hidden_color(ui.visuals(), false)
            } else {
                text
            };
            // A part in several pieces, or empty: an error to correct (§2.11). A body failed
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
    if depth > solidsketch_model::MAX_DEPTH {
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
            depth <= solidsketch_model::MAX_DEPTH
                && doc.assembly(id).is_ok_and(|a| a.occurrences().iter().any(|o| in_error(doc, o.component, memo, depth + 1)))
        }
    };
    memo.insert(id, e);
    e
}

/// Does one of `bodies` (of the part `p`, `depth` groups down) fail, or a body of a group among
/// them, at any depth?
fn holds_failure(p: &solidsketch_model::Part, bodies: &[solidsketch_model::Body], depth: usize) -> bool {
    depth <= solidsketch_model::MAX_DEPTH && bodies.iter().any(|b| p.is_failed(b.id) || holds_failure(p, p.def_of(b).children(), depth + 1))
}

/// A body row's parts: its folding state (a group's, a profiled body's), the groups it lies in,
/// the body, its siblings, whether the row is the body's profile's.
type BodyLine<'d> = (Option<Fold>, Vec<Id>, &'d solidsketch_model::Body, &'d [solidsketch_model::Body], bool);

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
    p: &'d solidsketch_model::Part,
    bodies: &'d [solidsketch_model::Body],
    group: &mut Vec<Id>,
    open: Open,
    out: &mut dyn FnMut(BodyLine<'d>),
) {
    if group.len() > solidsketch_model::MAX_DEPTH {
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
/// the part's own body it lies in (Shift: added). Its part open on its siblings and « Corps »
/// on, it is hovered and selected with the view (`over`: the body hovered here). The toggle
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
    // (§2.11 « Empty parts »); one holding a body failed, at any depth: ⚠ too, its name as it is.
    let def = b.part.def_of(b.body);
    let inner = def.is_group() && !failed && holds_failure(b.part, def.children(), 0);
    let warned = (failed && def.is_group()) || inner;
    // A name edited in place (§2.11 « Body names »), from a double-click, or F2 while it is the
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

/// A row's name (a body's, a component's) edited in place in its navigator row (§2.11 « Body names »), under the key
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
/// group of its bodies, in it or in the group open (§2.11) — nothing shown for the part itself,
/// the selection empty.
pub fn info(ui: &mut egui::Ui, editor: &Editor, name: &str, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let units = &doc.settings().units;
    let context = editor.context_definition();
    let sel = editor.selection();
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
                    guide_info(ui, g, units);
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
                model_info(ui, doc, name);
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
            guide_info(ui, g, units);
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
                    // final shape's exact box, once the kernel has given it (§2.11). None for a
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
                    let axes = [solidsketch_theme::AXIS_X, solidsketch_theme::AXIS_Y, solidsketch_theme::AXIS_Z];
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
        }
        many => {
            ui.label(tp!("panel.entity_info.selected", many.len()));
        }
    }
}

/// The model, chosen (the navigator's first row clicked): its name (`name`, its file's), its
/// parts and assemblies as placed, its materials.
fn model_info(ui: &mut egui::Ui, doc: &Document, name: &str) {
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
}

/// How many parts and assemblies `assembly` places, one per occurrence at any depth; each
/// definition walked once (`memo`), so a model of many copies costs its definitions, not its
/// placements.
fn placed_counts(doc: &Document, assembly: Id, memo: &mut FastMap<Id, (usize, usize)>, depth: usize) -> (usize, usize) {
    if let Some(&counts) = memo.get(&assembly) {
        return counts;
    }
    let Ok(a) = doc.assembly(assembly) else { return (0, 0) };
    if depth > solidsketch_model::MAX_DEPTH {
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

/// What is wrong with a part of `pieces` lumps of material, if anything: a part is one piece of
/// wood, in several or none (empty, §2.11 « Empty parts ») an error to correct.
fn pieces_error(pieces: usize) -> Option<String> {
    match pieces {
        0 => Some(t!("panel.entity_info.empty")),
        1 => None,
        _ => Some(t!("panel.entity_info.pieces", count = pieces)),
    }
}

/// A guide: its shape (a circle's radius).
fn guide_info(ui: &mut egui::Ui, g: &solidsketch_model::Guide, units: &Units) {
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
}

/// The profile of the body `k` of `p` (the open part, or the group open in it), being edited:
/// its body, how it is swept, its corners.
fn profile_info(ui: &mut egui::Ui, p: &solidsketch_model::Part, k: usize) {
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

/// The body `k` of `p` (the open part, or the group open in it, §2.11): a body or a group, its
/// name as the outliner gives it — its definition's, edited here —, blank or machining (its
/// sign toggled in the outliner), failed or not, its solid's size, how many bodies share its
/// definition.
fn body_info(ui: &mut egui::Ui, editor: &Editor, part: Id, p: &solidsketch_model::Part, k: usize, actions: &mut Vec<Action>) {
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

/// The Matières panel (§7 quater): the document's materials — colour, name, delete — and the
/// one the paint tool paints with; picking one starts the paint tool.
pub fn materials(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let painting = editor.tool_kind() == solidsketch_tools::ToolKind::Paint;
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
            // Room left for the delete button, as high as the field (`OPTION_HEIGHT`) and square.
            let width = (ui.available_width() - OPTION_HEIGHT - ui.spacing().item_spacing.x).max(60.0);
            if let Some(name) = text_field(ui, (id, 2), &m.name, width) {
                actions.push(Action::Apply(Operation::EditMaterial { material: id, name, color: m.color }));
            }
            let delete = egui::Image::new(icons::CLOSE).fit_to_exact_size(egui::vec2(12.0, 12.0)).tint(ui.visuals().text_color());
            // As high as the name's field beside it.
            let h = OPTION_HEIGHT;
            if ui.add(egui::Button::image(delete).min_size(egui::vec2(h, h))).on_hover_text(t!("panel.materials.delete_tip")).clicked() {
                actions.push(Action::Apply(Operation::DeleteMaterial { material: id }));
            }
        });
    }
    let add = egui::Image::new(icons::ADD).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(ui.visuals().text_color());
    if ui.add(egui::Button::image_and_text(add, t!("panel.materials.new"))).clicked() {
        let count = doc.materials().count();
        let taken = |n: &str| doc.materials().any(|(_, m)| m.name == n);
        let name = (count + 1..).map(|k| t!("default_name.material", number = k)).find(|n| !taken(n)).unwrap_or_default();
        actions.push(Action::Apply(Operation::AddMaterial { name, color: NEW_COLORS[count % NEW_COLORS.len()] }));
    }
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

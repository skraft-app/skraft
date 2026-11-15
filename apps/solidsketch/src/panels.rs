//! Side panels: the entity info, the materials and the outliner, the tool option bar and the
//! menu items, all drawn from the command registry's state (docs/ARCHITECTURE.md
//! §7 quater).

use glam::{DAffine3, DVec3};
use solidsketch_i18n::{t, tp};
use solidsketch_model::{ComponentKind, Document, EntityPath, GuideShape, Id, Occurrence, Operation, Scope, Sign, Units, measure};
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
    /// Machinings of the open part chosen in its list: shown, then selected.
    SelectMachinings(Vec<Id>),
    /// The machining hovered in the list, if any.
    HoverMachining(Option<Id>),
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

/// A menu item: a check box for a toggle, else a button with its shortcut; no hover text.
/// True when clicked.
pub fn command_item(ui: &mut egui::Ui, v: &CommandView) -> bool {
    let toggle = v.checked.is_some() && v.id.starts_with("view.");
    let mut button = if toggle {
        let mark = if v.checked == Some(true) { "✔ " } else { "    " };
        egui::Button::new(format!("{mark}{}", v.label))
    } else {
        egui::Button::new(&v.label).selected(v.checked == Some(true))
    };
    if let Some(s) = &v.shortcut {
        button = button.shortcut_text(s);
    }
    ui.add_enabled(v.enabled, button).clicked()
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
            if let Some(i) = segmented(ui, egui::Id::unique("option_bar_part_scope"), &segments) {
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
                if let Some(i) = segmented(ui, egui::Id::unique("option_bar_body_sign"), &segments) {
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
            if let Some(c) = segmented(ui, egui::Id::unique(("option_bar", o.key)), &segments) {
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
            if option_button(ui, on, &t!("option_bar.machinings"), &t!("option_bar.machinings_tip"), None) {
                actions.push(Action::ToggleMachinings);
            }
            if editor.open_part_profile() {
                let on = editor.tool_kind() == ToolKind::Profile;
                if option_button(ui, on, &t!("option_bar.profile"), &t!("option_bar.profile_tip"), None) {
                    actions.push(Action::ToggleProfile);
                }
            }
        }
        for (o, on) in toggles {
            if option_button(ui, on, &o.label, &o.tip, o.icon) {
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
    let field =
        Field { tip: Some(tip), width: 80.0, align: egui::Align2::RIGHT_CENTER, lit: value != 0.0, addon: None, blur_applies: false };
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

    let r = ui
        .horizontal(|ui| {
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
        return (applied && !text.trim().is_empty() && !unchanged).then(|| text.trim().to_owned());
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text));
    }
    None
}

/// A toggle of the option bar, drawn like a ribbon button: no frame until hovered, the
/// selection's fill and outline when on. True when clicked.
fn option_button(ui: &mut egui::Ui, selected: bool, label: &str, tip: &str, icon: Option<&str>) -> bool {
    let face = Face::new(ui, label, icon);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(face.width(OPTION_HEIGHT), OPTION_HEIGHT), egui::Sense::click());
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
fn segmented(ui: &mut egui::Ui, id: egui::Id, segments: &[Segment]) -> Option<usize> {
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

/// "Pied", "Gauche <Pied>".
pub fn occurrence_label(doc: &Document, o: &Occurrence) -> String {
    let name = doc.component(o.component).map(|c| c.name.as_str()).unwrap_or("?");
    if o.name.is_empty() { name.to_owned() } else { format!("{} <{name}>", o.name) }
}

/// One line of the outliner: an occurrence at some depth of the tree.
struct OutlinerRow<'d> {
    /// The occurrences above it.
    path: Vec<Id>,
    occurrence: &'d Occurrence,
    /// Its folding state, for an assembly.
    fold: Option<egui::collapsing_header::CollapsingState>,
    /// An assembly above it is hidden: it is not drawn either.
    hidden_above: bool,
}

/// The outliner, in `area`: only the rows in view are laid out (a model can hold tens of
/// thousands of occurrences).
pub fn outliner(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let doc = editor.document();
    if doc.root_assembly().occurrences().is_empty() {
        ui.weak(t!("panel.outliner.empty", shortcut = "R"));
        return;
    }
    let mut rows = Vec::new();
    flatten(ui.ctx(), editor, doc, doc.root(), &mut Vec::new(), &mut rows, false, 0);
    let selection: std::collections::HashSet<Id> = editor.selection().iter().copied().collect();
    let tint = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let height = ui.spacing().interact_size.y;
    let mut over = None;
    area.show_rows(ui, height, rows.len(), |ui, range| {
        ui.set_min_width(ui.available_width());
        for row in rows.drain(range) {
            let o = row.occurrence;
            let selected = editor.context() == row.path.as_slice() && (selection.contains(&o.id) || editor.open_part() == Some(o.id));
            // Hidden, itself or by an assembly above: greyed.
            let greyed = o.hidden || row.hidden_above;
            let tint = if greyed { weak } else { tint };
            let width = ui.available_width();
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            let row_ui = ui.allocate_ui_with_layout(egui::vec2(width, height), layout, |ui| {
                // The toggle button (or, for a part, its room) one indent per level, so all
                // the icons of a level line up.
                let spacing = ui.spacing().item_spacing.x;
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(ui.spacing().indent * row.path.len() as f32);
                match row.fold {
                    Some(mut fold) => {
                        fold.show_toggle_button(ui, egui::collapsing_header::paint_default_icon);
                        fold.store(ui.ctx());
                    }
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
                    let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(weak);
                    let eye = ui.add(egui::Button::image(img).frame(false)).on_hover_text(tip).clicked();
                    let label = ui
                        .with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            // A part in several pieces: an error to correct (§2.11).
                            let pieces = doc.part(o.component).map_or(1, |p| p.pieces());
                            let (label, broken) = if pieces > 1 {
                                (egui::RichText::new(format!("⚠ {}", occurrence_label(doc, o))).color(ui.visuals().error_fg_color), true)
                            } else {
                                (egui::RichText::new(occurrence_label(doc, o)), false)
                            };
                            let label = if greyed && !broken { label.color(weak) } else { label };
                            let r = ui.add(egui::Button::selectable(selected, label).truncate());
                            if broken { r.on_hover_text(t!("panel.outliner.pieces", count = pieces)) } else { r }
                        })
                        .inner;
                    (label, eye)
                })
                .inner
            });
            let path = EntityPath::new(row.path, o.id);
            // The whole row, its indent and toggle included.
            if ui.rect_contains_pointer(row_ui.response.rect) {
                over = Some(path.clone());
            }
            let (label, eye) = row_ui.inner;
            if eye {
                actions.push(Action::SetHidden(path, !o.hidden));
            } else if label.clicked() {
                actions.push(Action::SelectPath(path, ui.input(|i| i.modifiers.shift)));
            }
        }
    });
    actions.push(Action::HoverOutliner(over));
}

/// The rows of the tree under `assembly`, those of folded assemblies left out (`hidden_above`:
/// an occurrence above `assembly` is hidden).
#[allow(clippy::too_many_arguments)]
fn flatten<'d>(
    ctx: &egui::Context,
    editor: &Editor,
    doc: &'d Document,
    assembly: Id,
    path: &mut Vec<Id>,
    out: &mut Vec<OutlinerRow<'d>>,
    hidden_above: bool,
    depth: usize,
) {
    let Ok(a) = doc.assembly(assembly) else { return };
    if depth > solidsketch_model::MAX_DEPTH {
        return;
    }
    for o in a.occurrences() {
        if doc.assembly(o.component).is_err() {
            out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: None, hidden_above });
            continue;
        }
        let id = egui::Id::unique(("outliner", path.as_slice(), o.id));
        let mut fold = egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, false);
        // Open along the context being edited.
        if editor.context().starts_with(&[path.as_slice(), &[o.id]].concat()) {
            fold.set_open(true);
        }
        let open = fold.is_open();
        out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: Some(fold), hidden_above });
        if open {
            path.push(o.id);
            flatten(ctx, editor, doc, o.component, path, out, hidden_above || o.hidden, depth + 1);
            path.pop();
        }
    }
}

// ---- entity info -------------------------------------------------------------------------------

/// A name, in a [`framed_field`] applied by a click away too. `width` is fixed by the caller:
/// an infinite width inside a `Grid` feeds back on the column width of the previous frame and
/// widens the side panel at every relayout.
fn text_field(ui: &mut egui::Ui, key: (Id, u8), current: &str, width: f32) -> Option<String> {
    let field = Field { tip: None, width, align: egui::Align2::LEFT_CENTER, lit: false, addon: None, blur_applies: true };
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

pub fn info(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let units = &doc.settings().units;
    let context = editor.context_definition();
    let sel = editor.selection();
    let Ok(asm) = doc.assembly(context) else { return };
    if let Some(open) = editor.open_part().and_then(|id| asm.occurrence(id)) {
        open_part_info(ui, editor, open, actions);
        return;
    }
    match sel {
        [] => {
            if editor.context().is_empty() {
                ui.weak(t!("panel.entity_info.no_selection"));
            } else {
                let name = doc.component(context).map(|c| c.name.as_str()).unwrap_or("?");
                ui.label(t!("panel.entity_info.editing", name = name));
                ui.weak(t!("panel.entity_info.editing_exit"));
            }
        }
        [id] if doc.component(context).is_ok_and(|c| c.guide(*id).is_some()) => {
            let Some(g) = doc.component(context).ok().and_then(|c| c.guide(*id)) else { return };
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
                if let Some(name) = text_field(ui, (o.id, 0), &o.name, field) {
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
                    // part in several pieces: an error to correct.
                    let pieces = p.pieces();
                    if pieces > 1 {
                        ui.label(t!("panel.entity_info.dimensions"));
                        ui.colored_label(ui.visuals().error_fg_color, t!("panel.entity_info.pieces", count = pieces));
                        ui.end_row();
                    } else if let Some((lo, hi)) = editor.scene().bounds_of(p.shape()) {
                        ui.label(t!("panel.entity_info.dimensions"));
                        ui.label(dims(lo, hi, units));
                        ui.end_row();
                    }
                    ui.label(t!("panel.entity_info.bodies"));
                    ui.label(t!(
                        "panel.entity_info.bodies_value",
                        count = p.bodies.len(),
                        positives = p.positives().count(),
                        negatives = p.negatives().count()
                    ));
                    ui.end_row();
                    let (f, ed, v) = p.shape().topology_counts();
                    ui.label(t!("panel.entity_info.topology"));
                    ui.label(t!("panel.entity_info.topology_counts", faces = f, edges = ed, vertices = v));
                    ui.end_row();
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
                        let f = Field { tip: None, width: field, align: egui::Align2::RIGHT_CENTER, lit: false, addon, blur_applies: true };
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

/// The open part (ARCHITECTURE.md §2.9, §2.11): its name, how to close it, and its bodies in a
/// flat list — its blanks first, then its machinings —, each numbered in its order of creation,
/// with its solid's size, the failed ones marked; hovered and selected with the view. A click
/// shows the bodies if they are not, and selects (Shift: adds), or deselects a body selected; a
/// click on a body's
/// sign badge toggles it (the selected bodies' too, when it is one of them).
fn open_part_info(ui: &mut egui::Ui, editor: &Editor, open: &Occurrence, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let units = &doc.settings().units;
    let Ok(c) = doc.component(open.component) else { return };
    ui.label(t!("panel.entity_info.editing", name = c.name));
    ui.weak(t!("panel.entity_info.editing_part_exit"));
    let Some(p) = c.as_part() else { return };
    let pieces = p.pieces();
    if pieces > 1 {
        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {}", t!("panel.entity_info.pieces", count = pieces)));
    }
    let shown = editor.showing_machinings();
    let hovered = editor.hovered_machining();
    let mut over = None;
    // Numbered in the part's order (of creation), listed blanks first.
    let mut bodies: Vec<(usize, &solidsketch_model::Body)> = p.bodies.iter().enumerate().map(|(k, b)| (k + 1, b)).collect();
    bodies.sort_by_key(|(_, b)| b.sign == Sign::Negative);
    ui.add_space(6.0);
    ui.strong(t!("panel.entity_info.bodies_count", count = bodies.len()));
    for (number, m) in bodies {
        // The solid's exact box, once the kernel has given it.
        let size = editor
            .scene()
            .bounds_of(p.solid_of(m))
            .map_or_else(|| solidsketch_i18n::ellipsis(String::new()), |(lo, hi)| dims(lo, hi, units));
        // A body sharing its definition: how many show it.
        let row = match p.copies_of(m.def) {
            1 => t!("panel.entity_info.body_row", number = number, size = size),
            n => t!("panel.entity_info.body_row_shared", number = number, size = size, copies = n),
        };
        let mut text = egui::RichText::new(row);
        if p.is_failed(m.id) {
            text = egui::RichText::new(t!("panel.entity_info.body_failed", row = text.text())).color(ui.visuals().error_fg_color);
        }
        if shown && hovered == Some(m.id) {
            text = text.strong();
        }
        let selected = shown && editor.selection().contains(&m.id);
        ui.horizontal(|ui| {
            let (tip, other) = match m.sign {
                Sign::Positive => (t!("panel.entity_info.sign_positive_tip"), Sign::Negative),
                Sign::Negative => (t!("panel.entity_info.sign_negative_tip"), Sign::Positive),
            };
            let b = sign_badge(ui, m.sign).on_hover_text(tip);
            if b.hovered() {
                over = Some(m.id);
            }
            if b.clicked()
                && let Some(target) = editor.open_part_path()
            {
                let ids = match selected {
                    true => editor.selection().iter().copied().filter(|id| p.body(*id).is_some()).collect(),
                    false => vec![m.id],
                };
                actions.push(Action::Apply(Operation::SetSign { target, bodies: ids, sign: other, scope: editor.part_scope() }));
            }
            let r = ui.selectable_label(selected, text);
            let r = if shown { r } else { r.on_hover_text(t!("panel.entity_info.body_show_tip")) };
            if r.hovered() {
                over = Some(m.id);
            }
            if r.clicked() {
                if !shown {
                    actions.push(Action::ToggleMachinings);
                }
                // Selected, taken out of the selection; else selected alone (Shift: added).
                let ids = if selected {
                    editor.selection().iter().copied().filter(|id| *id != m.id).collect()
                } else if shown && ui.input(|i| i.modifiers.shift) {
                    editor.selection().iter().copied().chain([m.id]).collect()
                } else {
                    vec![m.id]
                };
                actions.push(Action::SelectMachinings(ids));
            }
        });
    }
    actions.push(Action::HoverMachining(over));
}

/// A body's sign, `+` or `−`, drawn with lines in a small button (not every font has them).
fn sign_badge(ui: &mut egui::Ui, sign: Sign) -> egui::Response {
    let side = ui.spacing().interact_size.y.min(18.0);
    let (rect, r) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact(&r);
        ui.painter().rect(rect, v.corner_radius, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
        let c = rect.center();
        let h = side * 0.25;
        let stroke = egui::Stroke::new(1.5, v.fg_stroke.color);
        ui.painter().line_segment([c - egui::vec2(h, 0.0), c + egui::vec2(h, 0.0)], stroke);
        if sign == Sign::Positive {
            ui.painter().line_segment([c - egui::vec2(0.0, h), c + egui::vec2(0.0, h)], stroke);
        }
    }
    r
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
        // The triangle, as egui's combo box draws it.
        let icon = egui::Rect::from_center_size(egui::pos2(inner.right() - 5.0, inner.center().y), egui::vec2(9.0, 5.0));
        p.add(egui::Shape::convex_polygon(vec![icon.left_top(), icon.right_top(), icon.center_bottom()], ink, egui::Stroke::NONE));
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
            let width = (ui.available_width() - 30.0).max(60.0);
            if let Some(name) = text_field(ui, (id, 2), &m.name, width) {
                actions.push(Action::Apply(Operation::EditMaterial { material: id, name, color: m.color }));
            }
            if ui.small_button("✕").on_hover_text(t!("panel.materials.delete_tip")).clicked() {
                actions.push(Action::Apply(Operation::DeleteMaterial { material: id }));
            }
        });
    }
    if ui.button(format!("+ {}", t!("panel.materials.new"))).clicked() {
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
}

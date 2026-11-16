//! Side panels: the entity info, the materials and the outliner, the tool option bar and the
//! menu items, all drawn from the command registry's state (docs/ARCHITECTURE.md
//! §7 quater).

use glam::{DAffine3, DVec3};
use solidsketch_i18n::{t, tp};
use solidsketch_model::{ComponentKind, Document, EntityPath, FastMap, GuideShape, Id, Occurrence, Operation, Scope, Sign, Units, measure};
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
    /// A body of a part chosen in the outliner (§2.11 « Body groups »): the part, and the group
    /// the path names, opened; the body selected or deselected, Shift (`true`) adding it.
    SelectBody(EntityPath, Id, bool),
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
fn cut_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, width: f32) {
    ui.scope(|ui| {
        ui.set_max_width(width);
        ui.add(egui::Label::new(text).truncate());
    });
}

/// The warning sign before an error to correct (a part in several pieces, a shortcut's
/// conflict), in the error colour, as high as the text.
/// An outliner row's fold toggle: the panels' chevron (as in their title bars), turning from
/// right (folded) to down (open) as it opens.
fn chevron_icon(ui: &mut egui::Ui, openness: f32, response: &egui::Response) {
    let color = ui.style().interact(response).fg_stroke.color;
    // egui's toggle rect is small (its triangle's): the chevron, as wide as the title bars'.
    icons::chevron(ui, response.rect.center(), 14.0, color, -(1.0 - openness) * std::f32::consts::FRAC_PI_2);
}

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
        return (applied && !text.trim().is_empty() && !unchanged).then(|| text.trim().to_owned());
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

/// "Pied", "Gauche <Pied>".
pub fn occurrence_label(doc: &Document, o: &Occurrence) -> String {
    let name = doc.component(o.component).map(|c| c.name.as_str()).unwrap_or("?");
    if o.name.is_empty() { name.to_owned() } else { format!("{} <{name}>", o.name) }
}

/// One line of the outliner: an occurrence at some depth of the tree, or a body of the part it
/// places.
struct OutlinerRow<'d> {
    /// The occurrences above it.
    path: Vec<Id>,
    occurrence: &'d Occurrence,
    /// Its folding state, for an assembly, a part of several bodies, a group.
    fold: Option<egui::collapsing_header::CollapsingState>,
    /// An assembly above it is hidden: it is not drawn either.
    hidden_above: bool,
    /// The body this line shows, of the part `occurrence` places: none for the occurrence's own.
    body: Option<BodyRow<'d>>,
}

/// A body in the outliner, under its part (§2.11 « Body groups »).
struct BodyRow<'d> {
    part: &'d solidsketch_model::Part,
    body: &'d solidsketch_model::Body,
    /// Its number among its siblings, in their order of creation.
    number: usize,
    /// The groups it lies in, the part's own body first: a click opens them.
    group: Vec<Id>,
    /// Its siblings, itself included: the bodies of the part or group it lies in.
    siblings: &'d [solidsketch_model::Body],
}

/// What a body is made of, its row's icon and name.
#[derive(Clone, Copy)]
enum Nature {
    /// A profile extruded along the body's Z.
    Extrusion,
    /// A half profile turned about the body's Z.
    Revolution,
    /// A free B-rep (imported, a boolean's result, stretched askew).
    Solid,
    /// A group of bodies.
    Group,
}

impl BodyRow<'_> {
    fn nature(&self) -> Nature {
        let def = self.part.def_of(self.body);
        match &def.profile {
            _ if def.is_group() => Nature::Group,
            Some(f) if f.is_turned() => Nature::Revolution,
            Some(_) => Nature::Extrusion,
            None => Nature::Solid,
        }
    }

    fn label(&self) -> String {
        let n = self.number;
        match self.nature() {
            Nature::Extrusion => t!("panel.outliner.extrusion", number = n),
            Nature::Revolution => t!("panel.outliner.revolution", number = n),
            Nature::Solid => t!("panel.outliner.solid", number = n),
            Nature::Group => t!("panel.outliner.group", number = n),
        }
    }

    fn icon(&self) -> egui::ImageSource<'static> {
        match self.nature() {
            Nature::Extrusion => icons::PRISM,
            Nature::Revolution => icons::TURN,
            Nature::Solid => icons::SOLID,
            Nature::Group => icons::GROUP,
        }
    }
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
    // The components in error at any depth, each found once (definitions are shared).
    let mut errors = FastMap::default();
    area.show_rows(ui, height, rows.len(), |ui, range| {
        ui.set_min_width(ui.available_width());
        for row in rows.drain(range) {
            if row.body.is_some() {
                body_row(ui, editor, row, height, actions);
                continue;
            }
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
                        fold.show_toggle_button(ui, chevron_icon);
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
                            // A part in several pieces, or empty: an error to correct (§2.11). A
                            // body failed in it, at any depth, or a part in error in an assembly:
                            // ⚠ too, its name as it is (the error is below, seen once unfolded).
                            let part = doc.part(o.component).ok();
                            let pieces = part.map_or(1, |p| p.pieces());
                            let broken = pieces != 1;
                            let inner = !broken && in_error(doc, o.component, &mut errors, 0);
                            let label = egui::RichText::new(occurrence_label(doc, o));
                            let label = if broken {
                                label.color(ui.visuals().error_fg_color)
                            } else if greyed {
                                label.color(weak)
                            } else {
                                label
                            };
                            let button = if broken || inner {
                                egui::Button::selectable(selected, (warning(ui), label))
                            } else {
                                egui::Button::selectable(selected, label)
                            };
                            let r = ui.add(button.truncate());
                            match pieces {
                                0 => r.on_hover_text(t!("panel.outliner.empty_part")),
                                1 if inner && part.is_some() => r.on_hover_text(t!("panel.outliner.failed_inside_part")),
                                1 if inner => r.on_hover_text(t!("panel.outliner.error_inside_assembly")),
                                1 => r,
                                _ => r.on_hover_text(t!("panel.outliner.pieces", count = pieces)),
                            }
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
        if let Ok(p) = doc.part(o.component) {
            // A part of its single body: nothing under it.
            if p.is_plain() && !p.def_of(&p.bodies[0]).is_group() {
                out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: None, hidden_above, body: None });
                continue;
            }
            let id = egui::Id::unique(("outliner", path.as_slice(), o.id));
            let mut fold = egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, false);
            // Open while the part is.
            if editor.context() == path.as_slice() && editor.open_part() == Some(o.id) {
                fold.set_open(true);
            }
            let open = fold.is_open();
            out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: Some(fold), hidden_above, body: None });
            if open {
                let open_group = (editor.context() == path.as_slice() && editor.open_part() == Some(o.id)).then(|| editor.open_group());
                let row = |fold, number, group, body, siblings| OutlinerRow {
                    path: path.clone(),
                    occurrence: o,
                    fold,
                    hidden_above,
                    body: Some(BodyRow { part: p, body, number, group, siblings }),
                };
                let at = (path.as_slice(), o.id);
                body_rows(ctx, at, p, &p.bodies, &mut Vec::new(), open_group.unwrap_or(&[]), &mut |r: BodyLine<'d>| {
                    out.push(row(r.0, r.1, r.2, r.3, r.4))
                });
            }
            continue;
        }
        if doc.assembly(o.component).is_err() {
            out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: None, hidden_above, body: None });
            continue;
        }
        let id = egui::Id::unique(("outliner", path.as_slice(), o.id));
        let mut fold = egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, false);
        // Open along the context being edited.
        if editor.context().starts_with(&[path.as_slice(), &[o.id]].concat()) {
            fold.set_open(true);
        }
        let open = fold.is_open();
        out.push(OutlinerRow { path: path.clone(), occurrence: o, fold: Some(fold), hidden_above, body: None });
        if open {
            path.push(o.id);
            flatten(ctx, editor, doc, o.component, path, out, hidden_above || o.hidden, depth + 1);
            path.pop();
        }
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
    depth <= solidsketch_model::MAX_DEPTH
        && bodies.iter().any(|b| p.is_failed(b.id) || holds_failure(p, p.def_of(b).children(), depth + 1))
}

/// A body row's parts: its folding state (a group's), its number, the groups it lies in, the
/// body, its siblings.
type BodyLine<'d> =
    (Option<egui::collapsing_header::CollapsingState>, usize, Vec<Id>, &'d solidsketch_model::Body, &'d [solidsketch_model::Body]);

/// The rows of `bodies` (of the part `p` the occurrence `at` places, in the groups `group`), in
/// their order of creation — a row toggling its sign stays under the pointer —, each numbered;
/// under an open group, its children. The groups along `open` (the group open in the part) are
/// unfolded.
fn body_rows<'d>(
    ctx: &egui::Context,
    at: (&[Id], Id),
    p: &'d solidsketch_model::Part,
    bodies: &'d [solidsketch_model::Body],
    group: &mut Vec<Id>,
    open: &[Id],
    out: &mut dyn FnMut(BodyLine<'d>),
) {
    if group.len() > solidsketch_model::MAX_DEPTH {
        return;
    }
    for (number, b) in bodies.iter().enumerate().map(|(k, b)| (k + 1, b)) {
        let children = p.def_of(b).children();
        if children.is_empty() {
            out((None, number, group.clone(), b, bodies));
            continue;
        }
        let id = egui::Id::unique(("outliner_body", at.0, at.1, group.as_slice(), b.id));
        let mut fold = egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, false);
        group.push(b.id);
        if open.starts_with(group) {
            fold.set_open(true);
        }
        let opened = fold.is_open();
        group.pop();
        out((Some(fold), number, group.clone(), b, bodies));
        if opened {
            group.push(b.id);
            body_rows(ctx, at, p, children, group, open, out);
            group.pop();
        }
    }
}

/// One body's line of the outliner: its fold for a group, its nature's icon and name, its sign
/// toggle at the right end; a click opens its part and selects the part's own body it lies in
/// (Shift: added). The toggle flips its sign — the selected bodies' too when it is one of them,
/// in its part or group opened —, in every occurrence of the part unless it is open for this
/// one alone (« Celui-ci »); the only positive body among its siblings is refused (the error
/// says why).
fn body_row(ui: &mut egui::Ui, editor: &Editor, row: OutlinerRow, height: f32, actions: &mut Vec<Action>) {
    let Some(b) = &row.body else { return };
    let path = EntityPath::new(row.path.clone(), row.occurrence.id).in_group(b.group.clone());
    let open = editor.open_part_path().as_ref() == Some(&path);
    let selected = open && editor.showing_machinings() && editor.selection().contains(&b.body.id);
    let greyed = row.occurrence.hidden || row.hidden_above;
    let weak = ui.visuals().weak_text_color();
    let tint = if greyed { weak } else { ui.visuals().text_color() };
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let (clicked, toggled) = ui
        .allocate_ui_with_layout(egui::vec2(ui.available_width(), height), layout, |ui| {
            let spacing = ui.spacing().item_spacing.x;
            ui.spacing_mut().item_spacing.x = 0.0;
            // Under the part's icon: its level, plus one, plus the groups above.
            ui.add_space(ui.spacing().indent * (row.path.len() + 1 + b.group.len()) as f32);
            match row.fold {
                Some(mut fold) => {
                    fold.show_toggle_button(ui, chevron_icon);
                    fold.store(ui.ctx());
                }
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
                let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(weak);
                let toggled = ui.add(egui::Button::image(img).frame(false)).on_hover_text(tip).clicked();
                let clicked = ui
                    .with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let failed = b.part.is_failed(b.body.id);
                        let mut label = egui::RichText::new(b.label());
                        if failed {
                            label = label.color(ui.visuals().error_fg_color);
                        } else if greyed {
                            label = label.color(tint);
                        }
                        // A group failed (empty, or its boolean failing): ⚠ as a part in error,
                        // an error to correct (§2.11 « Empty parts »); one holding a body failed,
                        // at any depth: ⚠ too, its name as it is.
                        let def = b.part.def_of(b.body);
                        let inner = def.is_group() && !failed && holds_failure(b.part, def.children(), 0);
                        if (failed && def.is_group()) || inner {
                            let tip = match (inner, def.solid.is_empty()) {
                                (true, _) => t!("panel.outliner.failed_inside_group"),
                                (false, true) => t!("panel.outliner.empty_group"),
                                (false, false) => t!("panel.outliner.failed_group"),
                            };
                            let button = egui::Button::selectable(selected, (warning(ui), label)).truncate();
                            ui.add(button).on_hover_text(tip).clicked()
                        } else {
                            ui.add(egui::Button::selectable(selected, label).truncate()).clicked()
                        }
                    })
                    .inner;
                (clicked, toggled)
            })
            .inner
        })
        .inner;
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
    } else if clicked {
        actions.push(Action::SelectBody(path, b.body.id, ui.input(|i| i.modifiers.shift)));
    }
}

// ---- entity info -------------------------------------------------------------------------------

/// A name, in a [`framed_field`] applied by a click away too. `width` is fixed by the caller:
/// an infinite width inside a `Grid` feeds back on the column width of the previous frame and
/// widens the side panel at every relayout.
fn text_field(ui: &mut egui::Ui, key: (Id, u8), current: &str, width: f32) -> Option<String> {
    let field = Field { tip: None, width, align: egui::Align2::LEFT_CENTER, lit: false, addon: None, blur_applies: true, handle: None };
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
                    ui.label(t!("panel.entity_info.bodies"));
                    cut_label(
                        ui,
                        t!(
                            "panel.entity_info.bodies_value",
                            count = p.bodies.len(),
                            positives = p.positives().count(),
                            negatives = p.negatives().count()
                        ),
                        field,
                    );
                    ui.end_row();
                    let (f, ed, v) = p.shape().topology_counts();
                    ui.label(t!("panel.entity_info.topology"));
                    cut_label(ui, t!("panel.entity_info.topology_counts", faces = f, edges = ed, vertices = v), field);
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
                        let f = Field {
                            tip: None,
                            width: field,
                            align: egui::Align2::RIGHT_CENTER,
                            lit: false,
                            addon,
                            blur_applies: true,
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

/// What is wrong with a part of `pieces` lumps of material, if anything: a part is one piece of
/// wood, in several or none (empty, §2.11 « Empty parts ») an error to correct.
fn pieces_error(pieces: usize) -> Option<String> {
    match pieces {
        0 => Some(t!("panel.entity_info.empty")),
        1 => None,
        _ => Some(t!("panel.entity_info.pieces", count = pieces)),
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
    // In a group of its bodies (§2.11 « Body groups »): the group's.
    let Some((_, p, _)) = editor.open_part_path().and_then(|path| doc.resolve_container(&path).ok()) else { return };
    let p = &*p;
    match editor.open_group().len() {
        0 => {
            ui.label(t!("panel.entity_info.editing", name = c.name));
            ui.weak(t!("panel.entity_info.editing_part_exit"));
        }
        depth => {
            ui.label(t!("panel.entity_info.editing_group", name = c.name, depth = depth));
            ui.weak(t!("panel.entity_info.editing_group_exit"));
        }
    }
    if let Some(error) = pieces_error(p.pieces()) {
        ui.horizontal(|ui| {
            ui.add(warning(ui));
            ui.colored_label(ui.visuals().error_fg_color, error);
        });
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
        // A group: how many bodies it holds (§2.11 « Body groups »).
        let held = p.def_of(m).children().len();
        let row = if held > 0 { t!("panel.entity_info.group_row", row = row, count = held) } else { row };
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
}

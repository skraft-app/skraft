//! Side panels: the entity info, the materials and the outliner, the tool option bar and the
//! menu items, all drawn from the command registry's state (docs/ARCHITECTURE.md
//! §7 quater).

use glam::{DAffine3, DVec3};
use solidsketch_i18n::{t, tp};
use solidsketch_model::{ComponentKind, Document, EntityPath, GuideShape, Id, Occurrence, Operation, Scope, Units, measure};
use solidsketch_tools::{Editor, OptionValue, ToolKind};

use crate::commands::Menu;
use crate::icons;

/// What a panel asks the app to do (applied after drawing, so the panels only read).
pub enum Action {
    /// A command of the registry, by id.
    Command(&'static str),
    SelectPath(EntityPath),
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

/// A length option's value, after its group's name: a button like the ribbon's (lit when not
/// zero), which a click turns into a text field in place (Enter applies, Esc or a click away
/// cancels). The text typed, when applied.
fn length_option(ui: &mut egui::Ui, key: &str, tip: &str, value: f64, units: &Units) -> Option<String> {
    let id = egui::Id::unique(("length_option", key));
    let shown = units.format(value);
    let editing: Option<String> = ui.data(|d| d.get_temp(id));
    let Some(mut text) = editing else {
        if option_button(ui, value != 0.0, &shown, tip, None) {
            ui.data_mut(|d| d.insert_temp(id, shown));
        }
        return None;
    };
    let field = egui::TextEdit::singleline(&mut text)
        .desired_width(80.0)
        .min_size(egui::vec2(80.0, OPTION_HEIGHT))
        .align(egui::Align2::LEFT_CENTER);
    let r = ui.add(field.id(id.with("field")));
    // Opened by the click: focused, all of it selected, to be typed over.
    if !r.has_focus() && !r.lost_focus() {
        r.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), r.id) {
            let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), r.id);
        }
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
        return (entered && !text.trim().is_empty() && text.trim() != shown).then(|| text.trim().to_owned());
    }
    ui.data_mut(|d| d.insert_temp(id, text));
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
                            let label = egui::RichText::new(occurrence_label(doc, o));
                            let label = if greyed { label.color(weak) } else { label };
                            ui.add(egui::Button::selectable(selected, label).truncate())
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
                actions.push(Action::SelectPath(path));
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

/// Text being edited in the info panel: (entity or definition, field) → text.
#[derive(Default)]
pub struct InfoState {
    editing: Option<((Id, u8), String)>,
}

/// `width` is fixed by the caller: an infinite width inside a `Grid` feeds back on the column
/// width of the previous frame and widens the side panel at every relayout.
fn text_field(ui: &mut egui::Ui, state: &mut InfoState, key: (Id, u8), current: &str, width: f32) -> Option<String> {
    let mut text = match &state.editing {
        Some((k, t)) if *k == key => t.clone(),
        _ => current.to_owned(),
    };
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(width));
    if r.has_focus() || r.changed() {
        state.editing = Some((key, text.clone()));
    }
    if r.lost_focus() {
        state.editing = None;
        if text != current && !text.trim().is_empty() {
            return Some(text.trim().to_owned());
        }
    }
    None
}

fn dims(lo: DVec3, hi: DVec3, units: &Units) -> String {
    let d = hi - lo;
    format!("{} × {} × {}{}", units.number(d.x), units.number(d.y), units.number(d.z), units.suffix())
}

pub fn info(ui: &mut egui::Ui, editor: &Editor, state: &mut InfoState, actions: &mut Vec<Action>) {
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
                ui.label(t!("panel.entity_info.name"));
                if let Some(name) = text_field(ui, state, (c.id, 1), &c.name, field) {
                    actions.push(Action::Apply(Operation::RenameComponent { component: c.id, name }));
                }
                ui.end_row();
                ui.label(t!("panel.entity_info.occurrence"));
                if let Some(name) = text_field(ui, state, (o.id, 0), &o.name, field) {
                    actions.push(Action::Apply(Operation::RenameOccurrence { context, occurrence: o.id, name }));
                }
                ui.end_row();
                ui.label(t!("panel.entity_info.occurrences"));
                ui.label(doc.occurrence_count(c.id).to_string());
                ui.end_row();
                ui.label(t!("panel.entity_info.material"));
                if let Some(m) = material_choice(ui, doc, ("occurrence_material", o.id), o.material, field) {
                    actions.push(Action::Apply(Operation::PaintOccurrences { context, entities: vec![o.id], material: m }));
                }
                ui.end_row();
                if let ComponentKind::Part(p) = &c.kind {
                    // In the part's own axes: length, width, thickness as the part lies; the
                    // blank's exact box, once the kernel has given it.
                    if let Some((lo, hi)) = editor.scene().bounds_of(&p.blank) {
                        ui.label(t!("panel.entity_info.dimensions"));
                        ui.label(dims(lo, hi, units));
                        ui.end_row();
                    }
                    ui.label(t!("panel.entity_info.machinings"));
                    ui.label(p.machinings.len().to_string());
                    ui.end_row();
                    let (f, ed, v) = p.shape().topology_counts();
                    ui.label(t!("panel.entity_info.topology"));
                    ui.label(t!("panel.entity_info.topology_counts", faces = f, edges = ed, vertices = v));
                    ui.end_row();
                }
                // At the top of its three fields, level with the first.
                ui.vertical(|ui| {
                    let size = egui::vec2(0.0, ui.spacing().interact_size.y);
                    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(t!("panel.entity_info.position")).extend());
                    });
                });
                // Shown in the document's unit; the model stays in mm.
                let unit = units.length.mm();
                let mut t = o.transform.translation / unit;
                let before = t;
                ui.vertical(|ui| {
                    let axes = [solidsketch_theme::AXIS_X, solidsketch_theme::AXIS_Y, solidsketch_theme::AXIS_Z];
                    for (v, axis) in [&mut t.x, &mut t.y, &mut t.z].into_iter().zip(axes) {
                        // Right-aligned: the button and its text edit align their text as the layout's cross
                        // alignment (its main one is always centred).
                        let size = egui::vec2(field, ui.spacing().interact_size.y);
                        ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Max), |ui| {
                            // Each tinted with its axis' colour, as a pastel over the field's ground.
                            let axis = crate::viewport::color(axis);
                            let vis = ui.visuals_mut();
                            let ground = vis.extreme_bg_color;
                            let tint = |k: f32| ground.lerp_to_gamma(axis, k);
                            vis.extreme_bg_color = tint(0.18);
                            for (w, k) in
                                [(&mut vis.widgets.inactive, 0.25), (&mut vis.widgets.hovered, 0.35), (&mut vis.widgets.active, 0.4)]
                            {
                                w.bg_fill = tint(k);
                                w.weak_bg_fill = tint(k);
                            }
                            // Typed like in the measurements box: `@+19`, `600-2*19`, `2cm`.
                            let current = *v * unit;
                            ui.add(
                                egui::DragValue::new(v)
                                    .min_size(size)
                                    .speed(1.0)
                                    .suffix(units.suffix())
                                    .max_decimals(units.decimals as usize)
                                    .custom_formatter(|n, decimals| {
                                        solidsketch_i18n::number(&egui::emath::format_with_decimals_in_range(n, decimals))
                                    })
                                    .custom_parser(move |s| measure::coordinate(s, units, current).ok().map(|mm| mm / unit)),
                            );
                        });
                    }
                });
                if t != before {
                    actions.push(Action::Apply(Operation::Transform {
                        context,
                        entities: vec![o.id],
                        transform: DAffine3::from_translation((t - before) * unit),
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

/// The open part (ARCHITECTURE.md §2.9): its name, how to close it, and its machinings — each
/// with its tool's size, the failed ones marked; hovered and selected with the view. A click
/// shows the machinings if they are not, and selects (Shift: adds or removes).
fn open_part_info(ui: &mut egui::Ui, editor: &Editor, open: &Occurrence, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let units = &doc.settings().units;
    let Ok(c) = doc.component(open.component) else { return };
    ui.label(t!("panel.entity_info.editing", name = c.name));
    ui.weak(t!("panel.entity_info.editing_part_exit"));
    let Some(p) = c.as_part() else { return };
    ui.add_space(6.0);
    if p.machinings.is_empty() {
        ui.weak(t!("panel.entity_info.no_machinings"));
        return;
    }
    ui.strong(t!("panel.entity_info.machinings_count", count = p.machinings.len()));
    let shown = editor.showing_machinings();
    let hovered = editor.hovered_machining();
    let mut over = None;
    for (k, m) in p.machinings.iter().enumerate() {
        // The tool's exact box, once the kernel has given it.
        let size =
            editor.scene().bounds_of(&m.tool).map_or_else(|| solidsketch_i18n::ellipsis(String::new()), |(lo, hi)| dims(lo, hi, units));
        let mut text = egui::RichText::new(t!("panel.entity_info.machining_row", number = k + 1, size = size));
        if p.is_failed(m.id) {
            text = egui::RichText::new(t!("panel.entity_info.machining_failed", row = text.text())).color(ui.visuals().error_fg_color);
        }
        if shown && hovered == Some(m.id) {
            text = text.strong();
        }
        let selected = shown && editor.selection().contains(&m.id);
        let r = ui.selectable_label(selected, text);
        let r = if shown { r } else { r.on_hover_text(t!("panel.entity_info.machining_show_tip")) };
        if r.hovered() {
            over = Some(m.id);
        }
        if r.clicked() {
            if !shown {
                actions.push(Action::ToggleMachinings);
            }
            let mut ids = if shown && ui.input(|i| i.modifiers.shift) { editor.selection().to_vec() } else { Vec::new() };
            match ids.iter().position(|id| *id == m.id) {
                Some(i) => {
                    ids.remove(i);
                }
                None => ids.push(m.id),
            }
            actions.push(Action::SelectMachinings(ids));
        }
    }
    actions.push(Action::HoverMachining(over));
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

/// A material picked in a list (« Aucune » first): `Some(choice)` when it changed.
fn material_choice(ui: &mut egui::Ui, doc: &Document, salt: (&str, Id), current: Option<Id>, width: f32) -> Option<Option<Id>> {
    let name = |m: Option<Id>| match m {
        Some(id) => doc.material(id).map_or("?".to_owned(), |m| m.name.clone()),
        None => t!("panel.materials.none"),
    };
    let mut picked = current;
    egui::ComboBox::from_id_salt(salt).width(width).selected_text(name(current)).show_ui(ui, |ui| {
        ui.selectable_value(&mut picked, None, t!("panel.materials.none"));
        for (id, m) in doc.materials() {
            ui.horizontal(|ui| {
                swatch(ui, Some(m.color), 12.0);
                ui.selectable_value(&mut picked, Some(id), &m.name);
            });
        }
    });
    (picked != current).then_some(picked)
}

/// The Matières panel (§7 quater): the document's materials — colour, name, delete — and the
/// one the paint tool paints with; picking one starts the paint tool.
pub fn materials(ui: &mut egui::Ui, editor: &Editor, state: &mut InfoState, actions: &mut Vec<Action>) {
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
            if let Some(name) = text_field(ui, state, (id, 2), &m.name, width) {
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

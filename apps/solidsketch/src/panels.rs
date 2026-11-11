//! Side panels: the tool bar (left), the entity info, the materials and the outliner (right),
//! and the menu items, all drawn from the command registry's state (docs/ARCHITECTURE.md
//! §7 quater).

use glam::{DAffine3, DVec3};
use solidsketch_i18n::{t, tp};
use solidsketch_model::{ComponentKind, Document, EntityPath, GuideShape, Id, Occurrence, Operation, Scope, Units, measure};
use solidsketch_tools::{Editor, OptionValue};

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
    /// Paint with this material (none: remove paint): the paint tool starts.
    PaintWith(Option<Id>),
    /// The "Machinings" switch of an open part (ARCHITECTURE.md §2.9), kept in the preferences.
    ToggleMachinings,
    /// What changes to the open part reach (§2.5).
    PartScope(Scope),
    /// Machinings of the open part chosen in its list: shown, then selected.
    SelectMachinings(Vec<Id>),
    /// The machining hovered in the list, if any.
    HoverMachining(Option<Id>),
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

/// A menu item: a check box for a toggle, else a button with its shortcut. True when clicked.
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
    ui.add_enabled(v.enabled, button).on_hover_text(v.id).clicked()
}

/// Whether the option bar has anything to show: the active tool's options, or an open part's
/// switches.
pub fn has_tool_options(editor: &Editor) -> bool {
    editor.open_part().is_some() || !editor.tool_options().is_empty()
}

/// The active tool's option bar (docs/ARCHITECTURE.md §6 bis): its options — a choice as a
/// row of buttons, a toggle as one. An open part puts its "Machinings" switch first (§2.9).
pub fn tool_options(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        if editor.open_part().is_some() {
            let on = editor.machinings_switch();
            if ui.selectable_label(on, t!("option_bar.machinings")).on_hover_text(t!("option_bar.machinings_tip")).clicked() {
                actions.push(Action::ToggleMachinings);
            }
            ui.separator();
            ui.label(t!("option_bar.choice", option = t!("tool.boolean.option.scope"))).on_hover_text(t!("option_bar.part_scope_tip"));
            let all = match editor.open_part_copies() {
                n if n > 1 => t!("tool.boolean.option.all_count", count = n),
                _ => t!("tool.boolean.option.all_one"),
            };
            let scope = editor.part_scope();
            for (s, label, tip) in [
                (Scope::All, all, t!("tool.boolean.option.all_tip")),
                (Scope::This, t!("tool.boolean.option.this"), t!("tool.boolean.option.this_tip")),
            ] {
                if ui.selectable_label(scope == s, label).on_hover_text(tip).clicked() && scope != s {
                    actions.push(Action::PartScope(s));
                }
            }
        }
        for (i, o) in editor.tool_options().into_iter().enumerate() {
            if i > 0 || editor.open_part().is_some() {
                ui.separator();
            }
            match &o.value {
                OptionValue::Choice(current) => {
                    ui.label(t!("option_bar.choice", option = o.label)).on_hover_text(&o.tip);
                    for c in &o.choices {
                        if ui.selectable_label(current == c.value, &c.label).on_hover_text(&c.tip).clicked() && current != c.value {
                            actions.push(Action::ToolOption(o.key, OptionValue::Choice(c.value.to_owned())));
                        }
                    }
                }
                OptionValue::Toggle(on) => {
                    if ui.selectable_label(*on, &o.label).on_hover_text(&o.tip).clicked() {
                        actions.push(Action::ToolOption(o.key, OptionValue::Toggle(!on)));
                    }
                }
            }
        }
    });
}

pub fn icon_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, selected: bool, enabled: bool, tip: &str) -> bool {
    let tint = ui.visuals().text_color();
    let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(24.0, 24.0)).tint(tint);
    ui.add_enabled(enabled, egui::Button::image(img).selected(selected).min_size(egui::vec2(34.0, 34.0))).on_hover_text(tip).clicked()
}

/// The delete key's name (Backspace works too).
fn delete_key() -> String {
    if solidsketch_i18n::mac() { "⌫".to_owned() } else { t!("key.delete") }
}

/// What a tool bar button adds to its command's tip.
#[derive(Clone, Copy)]
enum Hint {
    None,
    /// The copy modifier and the axis keys (move, rotate).
    Copy,
    /// Target, then tool (booleans).
    Boolean,
    /// The delete key (erase is not a command shortcut: the tool reads it).
    Delete,
}

/// The tool bar: each button a command (its state and tip from the registry); `None` is a
/// separator. Undo, redo and the view's toggles are in the title bar ([`crate::titlebar`]).
pub fn toolbar(ui: &mut egui::Ui, views: &[CommandView], actions: &mut Vec<Action>) {
    let buttons: [Option<(egui::ImageSource<'static>, &str, Hint)>; 19] = [
        Some((icons::SELECT, "tool.select", Hint::None)),
        Some((icons::BOX, "tool.box", Hint::None)),
        Some((icons::CYLINDER, "tool.cylinder", Hint::None)),
        Some((icons::MOVE, "tool.move", Hint::Copy)),
        Some((icons::ROTATE, "tool.rotate", Hint::Copy)),
        Some((icons::PAINT, "tool.paint", Hint::None)),
        Some((icons::SECTION, "tool.section", Hint::None)),
        Some((icons::MEASURE, "tool.measure", Hint::None)),
        None,
        Some((icons::UNION, "tool.union", Hint::Boolean)),
        Some((icons::SUBTRACT, "tool.difference", Hint::Boolean)),
        Some((icons::INTERSECT, "tool.intersection", Hint::Boolean)),
        None,
        Some((icons::COMPONENT, "edit.make_assembly", Hint::None)),
        Some((icons::EXPLODE, "edit.explode", Hint::None)),
        Some((icons::UNIQUE, "edit.make_unique", Hint::None)),
        Some((icons::ERASE, "edit.erase", Hint::Delete)),
        None,
        Some((icons::EXTENTS, "view.zoom_extents", Hint::None)),
    ];
    for b in buttons {
        let Some((icon, id, hint)) = b else {
            ui.separator();
            continue;
        };
        let Some(v) = views.iter().find(|v| v.id == id) else { continue };
        let selected = v.checked == Some(true);
        let tip = match hint {
            Hint::None => v.tip(),
            Hint::Copy => {
                t!("toolbar.with_hint", tip = v.tip(), hint = t!("toolbar.copy_hint", copy_key = solidsketch_tools::tools::copy_key()))
            }
            Hint::Boolean => t!("toolbar.with_hint", tip = v.tip(), hint = t!("toolbar.boolean_hint")),
            Hint::Delete => t!("toolbar.with_shortcut", label = v.label, shortcut = delete_key()),
        };
        if icon_button(ui, icon, selected, v.enabled, &tip) {
            actions.push(Action::Command(v.id));
        }
    }
}

// ---- outliner ----------------------------------------------------------------------------------

fn occurrence_icon(doc: &Document, o: &Occurrence) -> egui::ImageSource<'static> {
    if doc.part(o.component).is_ok() { icons::BOX } else { icons::COMPONENT }
}

/// "Pied", "Gauche <Pied>", with "×4" when the part has several copies (the propagation made
/// visible, ARCHITECTURE.md §2.5).
pub fn occurrence_label(doc: &Document, o: &Occurrence) -> String {
    let name = doc.component(o.component).map(|c| c.name.as_str()).unwrap_or("?");
    let mut label = if o.name.is_empty() { name.to_owned() } else { format!("{} <{name}>", o.name) };
    let n = doc.occurrence_count(o.component);
    if n > 1 {
        label.push_str(&format!("  ×{n}"));
    }
    label
}

pub fn outliner(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let doc = editor.document();
    if doc.root_assembly().occurrences.is_empty() {
        ui.weak(t!("panel.outliner.empty", shortcut = "R"));
        return;
    }
    tree(ui, editor, doc, doc.root(), &mut Vec::new(), actions, 0);
}

fn tree(ui: &mut egui::Ui, editor: &Editor, doc: &Document, assembly: Id, path: &mut Vec<Id>, actions: &mut Vec<Action>, depth: usize) {
    let Ok(a) = doc.assembly(assembly) else { return };
    if depth > solidsketch_model::MAX_DEPTH {
        return;
    }
    let tint = ui.visuals().text_color();
    for o in &a.occurrences {
        let selected = editor.context() == path.as_slice() && (editor.selection().contains(&o.id) || editor.open_part() == Some(o.id));
        // A leaf reserves the toggle button's width (egui gives it the full indent) so all the
        // icons of a level line up, one indent to the right of their parent's icon.
        let row = |ui: &mut egui::Ui, lead: f32| {
            ui.horizontal(|ui| {
                ui.add_space(lead);
                ui.add(egui::Image::new(occurrence_icon(doc, o)).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
                ui.selectable_label(selected, occurrence_label(doc, o))
            })
            .inner
        };
        if doc.assembly(o.component).is_err() {
            if row(ui, ui.spacing().indent).clicked() {
                actions.push(Action::SelectPath(EntityPath::new(path.clone(), o.id)));
            }
            continue;
        }
        let id = ui.make_persistent_id(("outliner", path.clone(), o.id));
        // Open along the context being edited.
        let in_context = editor.context().starts_with(&[path.as_slice(), &[o.id]].concat());
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        if in_context {
            state.set_open(true);
        }
        let (_, header, _) = state.show_header(ui, |ui| row(ui, 0.0)).body(|ui| {
            path.push(o.id);
            tree(ui, editor, doc, o.component, path, actions, depth + 1);
            path.pop();
        });
        if header.inner.clicked() {
            actions.push(Action::SelectPath(EntityPath::new(path.clone(), o.id)));
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
    format!("{} × {} × {} {}", units.number(d.x), units.number(d.y), units.number(d.z), units.length.symbol())
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
                ui.label(t!("panel.entity_info.position"));
                // Shown in the document's unit; the model stays in mm.
                let unit = units.length.mm();
                let mut t = o.transform.translation / unit;
                let before = t;
                ui.horizontal(|ui| {
                    for v in [&mut t.x, &mut t.y, &mut t.z] {
                        // Typed like in the measurements box: `@+19`, `600-2*19`, `2cm`.
                        let current = *v * unit;
                        ui.add(
                            egui::DragValue::new(v)
                                .speed(1.0)
                                .suffix(format!(" {}", units.length.symbol()))
                                .max_decimals(units.decimals as usize)
                                .custom_formatter(|n, decimals| {
                                    solidsketch_i18n::number(&egui::emath::format_with_decimals_in_range(n, decimals))
                                })
                                .custom_parser(move |s| measure::coordinate(s, units, current).ok().map(|mm| mm / unit)),
                        );
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

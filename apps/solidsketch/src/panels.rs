//! Side panels: the tool bar (left), the outliner and the entity info (right).

use glam::{DAffine3, DVec3};
use solidsketch_model::{Boolean, ComponentKind, Document, EntityPath, Id, Occurrence, Operation, Units, measure};
use solidsketch_tools::{Editor, OptionValue, ToolKind};

use crate::icons;

/// What a panel asks the app to do (applied after drawing, so the panels only read).
pub enum Action {
    Tool(ToolKind),
    Undo,
    Redo,
    Erase,
    MakeAssembly,
    Explode,
    MakeUnique,
    ZoomExtents,
    ToggleParallel,
    SelectPath(EntityPath),
    Apply(Operation),
    /// Débogage (`?debug`, `SOLIDSKETCH_DEBUG`): make the kernel panic.
    CrashKernel,
    /// An option of the active tool changed in its bar.
    ToolOption(&'static str, OptionValue),
}

/// The active tool's option bar (docs/ARCHITECTURE.md §6 bis): its name, then its options —
/// a choice as a row of buttons, a toggle as one.
pub fn tool_options(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.strong(editor.tool_kind().label());
        for o in editor.tool_options() {
            ui.separator();
            match &o.value {
                OptionValue::Choice(current) => {
                    ui.label(format!("{} :", o.label)).on_hover_text(&o.tip);
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

fn icon_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, selected: bool, enabled: bool, tip: &str) -> bool {
    let tint = ui.visuals().text_color();
    let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(24.0, 24.0)).tint(tint);
    ui.add_enabled(enabled, egui::Button::image(img).selected(selected).min_size(egui::vec2(34.0, 34.0)))
        .on_hover_text(tip)
        .clicked()
}

pub fn toolbar(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let kind = editor.tool_kind();
    let has_sel = !editor.selection().is_empty();
    ui.add_space(4.0);
    let tools: [(egui::ImageSource<'static>, ToolKind, &str); 8] = [
        (icons::SELECT, ToolKind::Select, "Sélection (Espace)"),
        (icons::BOX, ToolKind::Box, "Boîte (B)"),
        (icons::CYLINDER, ToolKind::Cylinder, "Cylindre (C)"),
        (icons::MOVE, ToolKind::Move, "Déplacer (M) · Option/Ctrl : copier · flèches : axe"),
        (icons::ROTATE, ToolKind::Rotate, "Faire pivoter (Q) · Option/Ctrl : copier · flèches : axe"),
        (icons::UNION, ToolKind::Boolean(Boolean::Union), "Union : cible, puis outil"),
        (icons::SUBTRACT, ToolKind::Boolean(Boolean::Difference), "Soustraire : cible, puis outil"),
        (icons::INTERSECT, ToolKind::Boolean(Boolean::Intersection), "Intersecter : cible, puis outil"),
    ];
    for (i, (icon, k, tip)) in tools.into_iter().enumerate() {
        if i == 5 {
            ui.separator();
        }
        if icon_button(ui, icon, kind == k, true, tip) {
            actions.push(Action::Tool(k));
        }
    }
    ui.separator();
    if icon_button(ui, icons::COMPONENT, false, has_sel, "Créer un assemblage (G)") {
        actions.push(Action::MakeAssembly);
    }
    if icon_button(ui, icons::EXPLODE, false, has_sel, "Éclater") {
        actions.push(Action::Explode);
    }
    if icon_button(ui, icons::UNIQUE, false, has_sel, "Faire une pièce à part") {
        actions.push(Action::MakeUnique);
    }
    if icon_button(ui, icons::ERASE, false, has_sel, "Effacer (Suppr)") {
        actions.push(Action::Erase);
    }
    ui.separator();
    if icon_button(ui, icons::UNDO, false, editor.history().can_undo(), "Annuler (⌘Z)") {
        actions.push(Action::Undo);
    }
    if icon_button(ui, icons::REDO, false, editor.history().can_redo(), "Rétablir (⇧⌘Z)") {
        actions.push(Action::Redo);
    }
    ui.separator();
    if icon_button(ui, icons::EXTENTS, false, true, "Zoom étendu (Maj+Z)") {
        actions.push(Action::ZoomExtents);
    }
    if icon_button(ui, icons::PERSPECTIVE, !editor.camera.is_parallel(), true, "Perspective / parallèle") {
        actions.push(Action::ToggleParallel);
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
        ui.weak("Modèle vide : dessiner une boîte (B)");
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
        let selected = editor.context() == path.as_slice() && editor.selection().contains(&o.id);
        let row = |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(occurrence_icon(doc, o)).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
                ui.selectable_label(selected, occurrence_label(doc, o))
            })
            .inner
        };
        if doc.assembly(o.component).is_err() {
            if row(ui).clicked() {
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
        let (_, header, _) = state.show_header(ui, |ui| row(ui)).body(|ui| {
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
    match sel {
        [] => {
            if editor.context().is_empty() {
                ui.weak("Aucune sélection");
            } else {
                let name = doc.component(context).map(|c| c.name.as_str()).unwrap_or("?");
                ui.label(format!("Édition de « {name} »"));
                ui.weak("Double-clic dans le vide pour sortir");
            }
        }
        [id] => {
            let Some(o) = asm.occurrence(*id) else { return };
            let Ok(c) = doc.component(o.component) else { return };
            // Room left for the value column (labels ~ 80 pt, plus the grid spacing).
            let field = (ui.available_width() - 90.0).max(60.0);
            egui::Grid::new("info").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Type");
                ui.label(if c.is_part() { "Pièce" } else { "Assemblage" });
                ui.end_row();
                ui.label("Nom");
                if let Some(name) = text_field(ui, state, (c.id, 1), &c.name, field) {
                    actions.push(Action::Apply(Operation::RenameComponent { component: c.id, name }));
                }
                ui.end_row();
                ui.label("Exemplaire");
                if let Some(name) = text_field(ui, state, (o.id, 0), &o.name, field) {
                    actions.push(Action::Apply(Operation::RenameOccurrence { context, occurrence: o.id, name }));
                }
                ui.end_row();
                ui.label("Exemplaires");
                ui.label(doc.occurrence_count(c.id).to_string());
                ui.end_row();
                if let ComponentKind::Part(p) = &c.kind {
                    // In the part's own axes: length, width, thickness as the part lies.
                    if let Some((lo, hi)) = p.blank.vertex_bounds() {
                        ui.label("Dimensions");
                        ui.label(dims(lo, hi, units));
                        ui.end_row();
                    }
                    ui.label("Usinages");
                    ui.label(p.machinings.len().to_string());
                    ui.end_row();
                    let (f, ed, v) = p.shape().topology_counts();
                    ui.label("Topologie");
                    ui.label(format!("{f} faces · {ed} arêtes · {v} sommets"));
                    ui.end_row();
                }
                ui.label("Position");
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
            ui.label(format!("{} sélectionnés", many.len()));
        }
    }
}

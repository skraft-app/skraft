//! Side panels: the tool bar (left), the outliner and the entity info (right).

use glam::{DAffine3, DVec3};
use solidsketch_model::{Boolean, DefinitionKind, Document, Entity, EntityPath, Id, Operation, Units};
use solidsketch_tools::{Editor, ToolKind};

use crate::icons;

/// What a panel asks the app to do (applied after drawing, so the panels only read).
pub enum Action {
    Tool(ToolKind),
    Undo,
    Redo,
    Erase,
    Make(DefinitionKind),
    Explode,
    MakeUnique,
    ZoomExtents,
    ToggleParallel,
    SelectPath(EntityPath),
    Apply(Operation),
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
    if icon_button(ui, icons::GROUP, false, has_sel, "Créer un groupe") {
        actions.push(Action::Make(DefinitionKind::Group));
    }
    if icon_button(ui, icons::COMPONENT, false, has_sel, "Créer un composant (G)") {
        actions.push(Action::Make(DefinitionKind::Component));
    }
    if icon_button(ui, icons::EXPLODE, false, has_sel, "Éclater") {
        actions.push(Action::Explode);
    }
    if icon_button(ui, icons::UNIQUE, false, has_sel, "Rendre unique") {
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

fn entity_icon(doc: &Document, e: &Entity) -> egui::ImageSource<'static> {
    match e {
        Entity::Solid(_) => icons::BOX,
        Entity::Instance(i) => match doc.definition(i.definition).map(|d| d.kind) {
            Ok(DefinitionKind::Component) => icons::COMPONENT,
            _ => icons::GROUP,
        },
    }
}

pub fn entity_label(doc: &Document, e: &Entity) -> String {
    match e {
        Entity::Solid(s) => s.name.clone(),
        Entity::Instance(i) => {
            let def = doc.definition(i.definition).map(|d| d.name.as_str()).unwrap_or("?");
            if i.name.is_empty() { def.to_owned() } else { format!("{} <{def}>", i.name) }
        }
    }
}

pub fn outliner(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let root = doc.root_definition();
    if root.entities.is_empty() {
        ui.weak("Modèle vide : dessiner une boîte (B)");
        return;
    }
    tree(ui, editor, doc, doc.root(), &mut Vec::new(), actions, 0);
}

fn tree(ui: &mut egui::Ui, editor: &Editor, doc: &Document, def: Id, path: &mut Vec<Id>, actions: &mut Vec<Action>, depth: usize) {
    let Ok(d) = doc.definition(def) else { return };
    if depth > solidsketch_model::MAX_DEPTH {
        return;
    }
    let tint = ui.visuals().text_color();
    for e in &d.entities {
        let selected = editor.context() == path.as_slice() && editor.selection().contains(&e.id());
        let row = |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(entity_icon(doc, e)).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
                ui.selectable_label(selected, entity_label(doc, e))
            })
            .inner
        };
        match e {
            Entity::Solid(_) => {
                if row(ui).clicked() {
                    actions.push(Action::SelectPath(EntityPath::new(path.clone(), e.id())));
                }
            }
            Entity::Instance(i) => {
                let id = ui.make_persistent_id(("outliner", path.clone(), i.id));
                // Open along the context being edited.
                let in_context = editor.context().starts_with(&[path.as_slice(), &[i.id]].concat());
                let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                if in_context {
                    state.set_open(true);
                }
                let (_, header, _) = state.show_header(ui, |ui| row(ui)).body(|ui| {
                    path.push(i.id);
                    tree(ui, editor, doc, i.definition, path, actions, depth + 1);
                    path.pop();
                });
                if header.inner.clicked() {
                    actions.push(Action::SelectPath(EntityPath::new(path.clone(), i.id)));
                }
            }
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
    let Ok(def) = doc.definition(context) else { return };
    match sel {
        [] => {
            if editor.context().is_empty() {
                ui.weak("Aucune sélection");
            } else {
                ui.label(format!("Édition de « {} »", def.name));
                ui.weak("Double-clic dans le vide pour sortir");
            }
        }
        [id] => {
            let Some(e) = def.entity(*id) else { return };
            // Room left for the value column (labels ~ 80 pt, plus the grid spacing).
            let field = (ui.available_width() - 90.0).max(60.0);
            egui::Grid::new("info").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| match e {
                Entity::Solid(s) => {
                    ui.label("Type");
                    ui.label("Solide (B-rep)");
                    ui.end_row();
                    ui.label("Nom");
                    if let Some(name) = text_field(ui, state, (s.id, 0), &s.name, field) {
                        actions.push(Action::Apply(Operation::RenameEntity { context, entity: s.id, name }));
                    }
                    ui.end_row();
                    let (f, ed, v) = s.solid.topology_counts();
                    ui.label("Topologie");
                    ui.label(format!("{f} faces · {ed} arêtes · {v} sommets"));
                    ui.end_row();
                    if let Some(b) = editor
                        .scene()
                        .definition_mesh(context)
                        .and_then(|dm| dm.parts.iter().find(|p| p.solid == s.id))
                        .and_then(|p| p.bounds)
                    {
                        ui.label("Dimensions");
                        ui.label(dims(b.0, b.1, units));
                        ui.end_row();
                    }
                }
                Entity::Instance(i) => {
                    let Ok(d) = doc.definition(i.definition) else { return };
                    let component = d.kind == DefinitionKind::Component;
                    ui.label("Type");
                    ui.label(if component { "Composant" } else { "Groupe" });
                    ui.end_row();
                    ui.label(if component { "Définition" } else { "Nom" });
                    if let Some(name) = text_field(ui, state, (d.id, 1), &d.name, field) {
                        actions.push(Action::Apply(Operation::RenameDefinition { definition: d.id, name }));
                    }
                    ui.end_row();
                    if component {
                        ui.label("Instance");
                        if let Some(name) = text_field(ui, state, (i.id, 0), &i.name, field) {
                            actions.push(Action::Apply(Operation::RenameEntity { context, entity: i.id, name }));
                        }
                        ui.end_row();
                        ui.label("Instances");
                        ui.label(doc.instance_count(d.id).to_string());
                        ui.end_row();
                    }
                    ui.label("Position");
                    // Shown in the document's unit; the model stays in mm.
                    let unit = units.length.mm();
                    let mut t = i.transform.translation / unit;
                    let before = t;
                    ui.horizontal(|ui| {
                        for v in [&mut t.x, &mut t.y, &mut t.z] {
                            ui.add(
                                egui::DragValue::new(v)
                                    .speed(1.0)
                                    .suffix(format!(" {}", units.length.symbol()))
                                    .max_decimals(units.decimals as usize),
                            );
                        }
                    });
                    if t != before {
                        actions.push(Action::Apply(Operation::Transform {
                            context,
                            entities: vec![i.id],
                            transform: DAffine3::from_translation((t - before) * unit),
                        }));
                    }
                    ui.end_row();
                    ui.label("");
                    let kind = if component { DefinitionKind::Group } else { DefinitionKind::Component };
                    let label = if component { "Convertir en groupe" } else { "Convertir en composant" };
                    if ui.add_enabled(!component || doc.instance_count(d.id) <= 1, egui::Button::new(label)).clicked() {
                        actions.push(Action::Apply(Operation::SetKind { definition: d.id, kind }));
                    }
                    ui.end_row();
                }
            });
        }
        many => {
            ui.label(format!("{} entités sélectionnées", many.len()));
        }
    }
}

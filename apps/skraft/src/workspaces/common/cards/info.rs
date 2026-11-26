//! The entity info of what is selected in the model (docs/ui/interface.md#entity-info).

use super::*;

/// A name, in a [`framed_field`] applied by a click away too. `width` is fixed by the caller:
/// an infinite width inside a `Grid` feeds back on the column width of the previous frame and
/// widens the side panel at every relayout.
pub(crate) fn text_field(ui: &mut egui::Ui, key: (Id, u8), current: &str, width: f32) -> Option<String> {
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
pub(crate) fn info_label_width(ui: &egui::Ui) -> f32 {
    const LABELS: &[&str] = &[
        "panel.entity_info.type",
        "panel.entity_info.name",
        "panel.entity_info.occurrence",
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
        "panel.entity_info.linked_to",
        "panel.entity_info.imprint_around",
        "panel.entity_info.imprint_deep",
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
pub(crate) fn info_grid(ui: &egui::Ui) -> egui::Grid {
    egui::Grid::new("info").num_columns(2).spacing(INFO_SPACING).min_col_width(info_label_width(ui))
}

/// The width of a field in an entity info's value column: the room left by the labels.
pub(crate) fn info_field_width(ui: &egui::Ui) -> f32 {
    (ui.available_width() - info_label_width(ui) - INFO_SPACING.x).max(60.0)
}

/// A grid row's label, level with the [`framed_field`] beside it (or the first one, on top).
pub(crate) fn field_label(ui: &mut egui::Ui, text: String) {
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
        ([p], []) => match doc.context_of(&p.occurrences) {
            Ok(c) => (c, vec![p.entity]),
            Err(_) => return,
        },
        ([], [g]) if let Ok(c) = doc.context_of(&g.occurrences) => (c, vec![g.guide]),
        (parts, guides) => {
            ui.label(tp!("panel.entity_info.selected", parts.len() + guides.len()));
            return;
        }
    };
    let sel = sel.as_slice();
    let Ok(asm) = doc.assembly(context) else { return };
    if let Some(open) = editor.open_part().and_then(|id| asm.occurrence(id)) {
        let Some((part, p, _)) = editor.open_part_path().and_then(|path| doc.resolve_container(&path).ok()) else { return };
        // A body's profile edited: it is what is chosen (the navigator's « Profil » row).
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
                if let ComponentKind::Part(p) = &c.kind {
                    // The part's material: every copy of it shows it (docs/render/look.md).
                    field_label(ui, t!("panel.entity_info.material"));
                    if let Some(m) = material_choice(ui, doc, ("part_material", c.id), p.material, field) {
                        let targets = vec![EntityPath::new(editor.context().to_vec(), o.id)];
                        actions.push(Action::Apply(Operation::PaintParts { targets, material: m, scope: Scope::All }));
                    }
                    ui.end_row();
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
                        let addon = Some((letter, crate::workspaces::common::viewport::color(axis)));
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
/// (edited there), how many times it is in the model, a part's dimensions, the materials it and
/// its faces carry.
fn component_info(ui: &mut egui::Ui, editor: &Editor, id: Id, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let Ok(c) = doc.component(id) else { return };
    let units = &doc.settings().units;
    let field = info_field_width(ui);
    let count = doc.counts_in_model().get(&id).copied().unwrap_or(0);
    // Its materials: the part's own, its faces'; in the order of the document's.
    let mut used: FastSet<Id> = FastSet::default();
    if let ComponentKind::Part(p) = &c.kind {
        used.extend(p.faces.values().copied().chain(p.material));
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
/// name as the navigator gives it — its definition's, edited here —, blank or machining (its
/// sign toggled in the navigator), failed or not, its solid's size, how many bodies share its
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
        // What it follows, whose name it bears (docs/model/bodies.md#links); one that lost its
        // source keeps that name. Else its definition's: every copy shows it; emptied, it is named
        // by its nature again.
        let source = editor.link_map().source_of(part, b.id).and_then(|s| editor.document().component(s).ok());
        if let Some(c) = source {
            ui.label(t!("panel.entity_info.linked_to"));
            cut_label(ui, egui::RichText::new(&c.name), field);
        } else if def.source == skraft_model::BodySource::Linked {
            ui.label(t!("panel.entity_info.name"));
            cut_label(ui, egui::RichText::new(body_label(p, b)), field);
        } else {
            field_label(ui, t!("panel.entity_info.name"));
            if let Some(name) = name_field(ui, (def.id, 2), &body_label(p, b), field, true) {
                actions.push(Action::Apply(Operation::RenameBodyDef { part, def: def.id, name }));
            }
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
        // A linked body made by « Empreindre »: its clearance (docs/model/bodies.md#links).
        if let Some(r) = def.imprint.as_ref().filter(|_| source.is_some()) {
            let c = r.clearance;
            let rows =
                [("around", t!("panel.entity_info.imprint_around"), c.around), ("deep", t!("panel.entity_info.imprint_deep"), c.deep)];
            for (key, label, value) in rows {
                field_label(ui, label);
                let shown = units.format(value);
                let exact = format!("{}{}", units.exact_number(value), units.suffix());
                let f = Field {
                    hint: None,
                    tip: None,
                    width: field,
                    align: egui::Align2::RIGHT_CENTER,
                    lit: false,
                    addon: None,
                    blur_applies: true,
                    empty_applies: false,
                    handle: Some(format!("info:imprint_{key}")),
                };
                let id = ui.make_persistent_id(("info_imprint", key, b.id));
                if let Some(text) = ui.vertical(|ui| framed_field(ui, id, &shown, &exact, f)).inner
                    && let Ok(v) = measure::length(&text, units, value)
                {
                    let (around, deep) = if key == "around" { (v, c.deep) } else { (c.around, v) };
                    actions.push(Action::Apply(Operation::SetClearance { part, body: b.id, around, deep }));
                }
                ui.end_row();
            }
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

//! The entity info's properties (docs/model/properties.md, docs/ui/interface.md#properties): an
//! entity's values, those its definitions give first; the model's definitions, edited in its
//! entity info.

use std::collections::BTreeSet;

use skraft_i18n::t;
use skraft_model::{Document, EntityKind, Operation, PropDef, PropTarget, Units, Value, ValueKind};

use super::{Action, Field, OPTION_HEIGHT, cut_label, framed_field};
use crate::icons;

/// The kinds a definition offers, in its menu.
const KINDS: [fn() -> ValueKind; 14] = [
    || ValueKind::Text,
    || ValueKind::Int,
    || ValueKind::Real,
    || ValueKind::Bool,
    || ValueKind::Length,
    || ValueKind::Area,
    || ValueKind::Volume,
    || ValueKind::Angle,
    || ValueKind::Mass,
    || ValueKind::Money,
    || ValueKind::Color,
    || ValueKind::Choice,
    || ValueKind::List(Box::new(ValueKind::Choice)),
    || ValueKind::List(Box::new(ValueKind::Length)),
];

const ENTITIES: [EntityKind; 8] = [
    EntityKind::Model,
    EntityKind::Part,
    EntityKind::Assembly,
    EntityKind::Occurrence,
    EntityKind::BodyDef,
    EntityKind::Body,
    EntityKind::Material,
    EntityKind::Guide,
];

fn kind_name(kind: &ValueKind) -> String {
    match kind {
        ValueKind::Bool => t!("property.kind.bool"),
        ValueKind::Int => t!("property.kind.int"),
        ValueKind::Real => t!("property.kind.real"),
        ValueKind::Text => t!("property.kind.text"),
        ValueKind::Length => t!("property.kind.length"),
        ValueKind::Area => t!("property.kind.area"),
        ValueKind::Volume => t!("property.kind.volume"),
        ValueKind::Angle => t!("property.kind.angle"),
        ValueKind::Mass => t!("property.kind.mass"),
        ValueKind::Money => t!("property.kind.money"),
        ValueKind::Color => t!("property.kind.color"),
        ValueKind::Choice => t!("property.kind.choice"),
        ValueKind::List(k) if **k == ValueKind::Choice => t!("property.kind.list_choice"),
        ValueKind::List(k) if **k == ValueKind::Length => t!("property.kind.list_length"),
        ValueKind::List(k) => t!("property.kind.list", kind = kind_name(k)),
    }
}

fn entity_name(kind: EntityKind) -> String {
    match kind {
        EntityKind::Model => t!("property.entity.model"),
        EntityKind::Part => t!("property.entity.part"),
        EntityKind::Assembly => t!("property.entity.assembly"),
        EntityKind::Occurrence => t!("property.entity.occurrence"),
        EntityKind::BodyDef => t!("property.entity.body_def"),
        EntityKind::Body => t!("property.entity.body"),
        EntityKind::Material => t!("property.entity.material"),
        EntityKind::Guide => t!("property.entity.guide"),
    }
}

/// A key's name, shown when its definition has no label (or it has no definition): what
/// follows its namespace.
fn key_name(key: &str) -> &str {
    key.split_once('.').map_or(key, |(_, name)| name)
}

/// A square button with an icon, as high as the fields: true when clicked.
fn icon_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, tip: &str) -> bool {
    let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(12.0, 12.0)).tint(ui.visuals().text_color());
    let h = OPTION_HEIGHT;
    ui.add(egui::Button::image(img).min_size(egui::vec2(h, h))).on_hover_text(tip).clicked()
}

/// A text field, `width` wide, applied by a click away too, emptied too: the text, when applied.
fn text_input(ui: &mut egui::Ui, id: egui::Id, current: &str, width: f32) -> Option<String> {
    let field = Field {
        tip: None,
        width,
        align: egui::Align2::LEFT_CENTER,
        lit: false,
        addon: None,
        blur_applies: true,
        empty_applies: true,
        handle: None,
    };
    ui.vertical(|ui| framed_field(ui, id, current, current, field)).inner
}

/// Rows of a label and what follows it, at widths fixed from the room there is, not a `Grid`:
/// a grid's columns follow their content of the last frame, and a field as wide as the room left
/// beside a label wider than planned widens the card at every relayout.
struct Rows {
    label: f32,
    field: f32,
}

impl Rows {
    const GAP: f32 = 8.0;

    /// `button`: room kept after the field for a square button.
    fn new(ui: &egui::Ui, button: bool) -> Rows {
        let total = ui.available_width();
        let label = (total * 0.4).clamp(60.0, 130.0);
        let button = if button { OPTION_HEIGHT + Self::GAP } else { 0.0 };
        Rows { label, field: (total - label - Self::GAP - button).max(60.0) }
    }

    /// A row: `label` cut to its width, level with the field's top, then `add`.
    fn row<R>(&self, ui: &mut egui::Ui, label: impl Into<egui::WidgetText>, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::Response {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = Self::GAP;
            let size = egui::vec2(self.label, OPTION_HEIGHT);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(self.label);
                cut_label(ui, label, self.label);
            });
            add(ui);
        })
        .response
    }
}

/// A heading over a group of rows.
fn heading(ui: &mut egui::Ui, text: String) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).strong());
}

/// A choice among `options` (none: « — »), framed as the fields, `width` wide: the one picked,
/// when it changed.
fn choice_field(ui: &mut egui::Ui, id: egui::Id, current: Option<&str>, options: &[String], width: f32) -> Option<String> {
    ui.vertical(|ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, OPTION_HEIGHT), egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let visuals = ui.visuals();
        let stroke = super::field_stroke(visuals, open, false, response.hovered());
        let p = ui.painter();
        p.rect(rect, visuals.widgets.inactive.corner_radius, visuals.extreme_bg_color, stroke, egui::StrokeKind::Inside);
        let ink = visuals.text_color();
        let inner = rect.shrink2(egui::vec2(6.0, 0.0));
        let icon = egui::Rect::from_center_size(egui::pos2(inner.right() - 5.0, inner.center().y), egui::vec2(14.0, 14.0));
        icons::chevron(ui, icon.center(), 14.0, ink, 0.0);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let text = ui.fonts_mut(|f| f.layout_no_wrap(current.unwrap_or("—").to_owned(), font, ink));
        let clip = inner.with_max_x(icon.left() - 4.0);
        ui.painter_at(clip).galley(egui::pos2(inner.left(), inner.center().y - text.size().y / 2.0), text, ink);
        let mut picked = None;
        egui::Popup::menu(&response).id(id).width(width).style(crate::style::menu).show(|ui| {
            ui.set_min_width(ui.available_width());
            for o in options {
                if ui.selectable_label(current == Some(o.as_str()), o).clicked() && current != Some(o.as_str()) {
                    picked = Some(o.clone());
                }
            }
        });
        picked
    })
    .inner
}

/// The editor of a value of `kind` (`choices` a choice's), `width` wide: a check box, a choice,
/// a colour, else a field typed as the measurements box reads (docs/model/properties.md#values).
/// The value set, when it changed; `shown` greyed when the entity has none (its default).
fn value_editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    kind: &ValueKind,
    choices: &[String],
    value: Option<&Value>,
    units: &Units,
    width: f32,
) -> Option<Value> {
    match kind {
        ValueKind::Bool => {
            let mut on = matches!(value, Some(Value::Bool(true)));
            ui.vertical(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(width, OPTION_HEIGHT), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_width(width);
                    ui.checkbox(&mut on, "").changed().then_some(Value::Bool(on))
                })
                .inner
            })
            .inner
        }
        ValueKind::Choice => {
            let current = match value {
                Some(Value::Choice(c)) => Some(c.as_str()),
                _ => None,
            };
            choice_field(ui, id, current, choices, width).map(Value::Choice)
        }
        ValueKind::Color => {
            let mut c = match value {
                Some(Value::Color(c)) => *c,
                _ => [128, 128, 128],
            };
            ui.vertical(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(width, OPTION_HEIGHT), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_width(width);
                    ui.color_edit_button_srgb(&mut c).changed().then_some(Value::Color(c))
                })
                .inner
            })
            .inner
        }
        _ => {
            let (shown, exact) = value.map_or((String::new(), String::new()), |v| (v.shown(units), v.exact(units)));
            let numeric = !matches!(kind, ValueKind::Text | ValueKind::List(_));
            let field = Field {
                tip: None,
                width,
                align: if numeric { egui::Align2::RIGHT_CENTER } else { egui::Align2::LEFT_CENTER },
                lit: false,
                addon: None,
                blur_applies: true,
                empty_applies: false,
                handle: None,
            };
            let text = ui.vertical(|ui| framed_field(ui, id, &shown, &exact, field)).inner?;
            Value::typed(kind, &text, units, value)
        }
    }
}

/// The « Propriétés » rows of the entity `target` under `title`: a row per definition applying
/// to it (its value, else its default greyed), then its values without one, as they are; each
/// value set with a button to remove it. Nothing when it has none and none applies.
pub fn section(ui: &mut egui::Ui, doc: &Document, target: &PropTarget, title: String, actions: &mut Vec<Action>) {
    let (Ok(kind), Ok(props)) = (doc.prop_kind(target), doc.props(target)) else { return };
    let defs = doc.prop_defs();
    let defined: Vec<(&String, &PropDef)> = defs.iter().filter(|(_, d)| d.applies(kind)).collect();
    let raw: Vec<(&String, &Value)> = props.iter().filter(|(k, _)| !defs.contains_key(*k)).collect();
    if defined.is_empty() && raw.is_empty() {
        return;
    }
    let units = &doc.settings().units;
    heading(ui, title);
    let rows = Rows::new(ui, true);
    let set = |key: &str, value: Option<Value>| {
        Action::Apply(Operation::SetProps { target: target.clone(), props: [(key.to_owned(), value)].into() })
    };
    for (key, def) in defined {
        let label = if def.label.is_empty() { key_name(key).to_owned() } else { def.label.clone() };
        let value = props.get(key);
        rows.row(ui, label, |ui| {
            let id = ui.make_persistent_id(("prop", target, key));
            // None set: its default, greyed.
            let shown = value.or(def.default.as_ref());
            let r = ui.scope(|ui| {
                if value.is_none() {
                    ui.style_mut().visuals.override_text_color = Some(ui.visuals().weak_text_color());
                }
                value_editor(ui, id, &def.kind, &def.choices, shown, units, rows.field)
            });
            if let Some(v) = r.inner {
                actions.push(set(key, Some(v)));
            }
            if value.is_some() && icon_button(ui, icons::CLOSE, &t!("panel.properties.clear_tip")) {
                actions.push(set(key, None));
            }
        });
    }
    for (key, value) in raw {
        rows.row(ui, egui::RichText::new(key.as_str()).weak(), |ui| {
            let id = ui.make_persistent_id(("prop", target, key));
            if let Some(v) = value_editor(ui, id, &value.kind(), &[], Some(value), units, rows.field) {
                actions.push(set(key, Some(v)));
            }
            if icon_button(ui, icons::CLOSE, &t!("panel.properties.clear_tip")) {
                actions.push(set(key, None));
            }
        })
        .on_hover_text(t!("panel.properties.raw_tip"));
    }
}

/// The model's definitions of properties (its entity info): one fold each — its name, kind,
/// choices, default, the entities it applies to, its key —, a button to add one.
pub fn definitions(ui: &mut egui::Ui, doc: &Document, actions: &mut Vec<Action>) {
    heading(ui, t!("panel.properties.definitions"));
    let units = &doc.settings().units;
    let defs = doc.prop_defs();
    let define = |key: &str, def: Option<PropDef>| Action::Apply(Operation::SetPropDef { key: key.to_owned(), def });
    if defs.is_empty() {
        ui.weak(t!("panel.properties.none"));
    }
    for (key, def) in defs {
        let label = if def.label.is_empty() { key_name(key).to_owned() } else { def.label.clone() };
        let header = format!("{label} · {}", kind_name(&def.kind));
        egui::CollapsingHeader::new(header).id_salt(("prop_def", key)).show(ui, |ui| {
            let rows = Rows::new(ui, false);
            let field = rows.field;
            rows.row(ui, t!("panel.properties.label"), |ui| {
                let id = ui.make_persistent_id(("prop_def_label", key));
                if let Some(text) = text_input(ui, id, &def.label, field) {
                    actions.push(define(key, Some(PropDef { label: text, ..def.clone() })));
                }
            });
            rows.row(ui, t!("panel.properties.kind"), |ui| {
                let names: Vec<String> = KINDS.iter().map(|k| kind_name(&k())).collect();
                let id = ui.make_persistent_id(("prop_def_kind", key));
                if let Some(name) = choice_field(ui, id, Some(&kind_name(&def.kind)), &names, field)
                    && let Some(kind) = KINDS.iter().map(|k| k()).find(|k| kind_name(k) == name)
                {
                    let choice = matches!(&kind, ValueKind::Choice) || matches!(&kind, ValueKind::List(k) if **k == ValueKind::Choice);
                    // A choice needs choices: the first ones offered, to rename.
                    let choices = match (choice, def.choices.is_empty()) {
                        (true, true) => vec![t!("panel.properties.choice", n = 1), t!("panel.properties.choice", n = 2)],
                        (true, false) => def.choices.clone(),
                        (false, _) => Vec::new(),
                    };
                    actions.push(define(key, Some(PropDef { kind, choices, default: None, ..def.clone() })));
                }
            });
            if !def.choices.is_empty() {
                rows.row(ui, t!("panel.properties.choices"), |ui| {
                    let shown = skraft_model::measure::join(&def.choices);
                    let id = ui.make_persistent_id(("prop_def_choices", key));
                    let tip = t!("panel.properties.choices_tip");
                    let f = Field {
                        tip: Some(&tip),
                        width: field,
                        align: egui::Align2::LEFT_CENTER,
                        lit: false,
                        addon: None,
                        blur_applies: true,
                        empty_applies: false,
                        handle: None,
                    };
                    if let Some(text) = ui.vertical(|ui| framed_field(ui, id, &shown, &shown, f)).inner {
                        let mut choices: Vec<String> = Vec::new();
                        for item in skraft_model::measure::list(&text) {
                            if !item.value.is_empty() && !choices.contains(&item.value) {
                                choices.push(item.value);
                            }
                        }
                        // The default kept only if still one of them.
                        let mut d = PropDef { choices, ..def.clone() };
                        if d.default.as_ref().is_some_and(|v| !d.accepts(v)) {
                            d.default = None;
                        }
                        actions.push(define(key, Some(d)));
                    }
                });
            }
            rows.row(ui, t!("panel.properties.default"), |ui| {
                let id = ui.make_persistent_id(("prop_def_default", key));
                let width = field - OPTION_HEIGHT - 8.0;
                if let Some(v) = value_editor(ui, id, &def.kind, &def.choices, def.default.as_ref(), units, width) {
                    actions.push(define(key, Some(PropDef { default: Some(v), ..def.clone() })));
                }
                if def.default.is_some() && icon_button(ui, icons::CLOSE, &t!("panel.properties.clear_tip")) {
                    actions.push(define(key, Some(PropDef { default: None, ..def.clone() })));
                }
            });
            rows.row(ui, t!("panel.properties.applies_to"), |ui| {
                ui.vertical(|ui| {
                    ui.set_width(field);
                    let all = def.applies_to.is_empty();
                    for kind in ENTITIES {
                        let mut on = all || def.applies_to.contains(&kind);
                        if ui.checkbox(&mut on, entity_name(kind)).changed() {
                            // From « all », the others; every one checked again: all.
                            let mut set: BTreeSet<EntityKind> = if all { ENTITIES.into() } else { def.applies_to.clone() };
                            if on {
                                set.insert(kind);
                            } else {
                                set.remove(&kind);
                            }
                            if set.len() == ENTITIES.len() {
                                set.clear();
                            }
                            // None checked: refused (empty is all), nothing done.
                            if !set.is_empty() || on {
                                actions.push(define(key, Some(PropDef { applies_to: set, ..def.clone() })));
                            }
                        }
                    }
                });
            });
            rows.row(ui, t!("panel.properties.key"), |ui| cut_label(ui, egui::RichText::new(key.as_str()).weak(), field));
            if ui.button(t!("panel.properties.delete")).on_hover_text(t!("panel.properties.delete_tip")).clicked() {
                actions.push(define(key, None));
            }
        });
    }
    let add = egui::Image::new(icons::ADD).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(ui.visuals().text_color());
    if ui.add(egui::Button::image_and_text(add, t!("panel.properties.new"))).clicked() {
        let n = (1..).find(|n| !defs.contains_key(&format!("user.property_{n}"))).unwrap_or(1);
        let def = PropDef { label: t!("panel.properties.new_label", n = n), ..PropDef::new(ValueKind::Text) };
        actions.push(define(&format!("user.property_{n}"), Some(def)));
    }
}

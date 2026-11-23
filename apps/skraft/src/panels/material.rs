//! A material in the entity info: its swatch, its choice, its fields.

use super::*;

/// Colours new materials take in turn (woods, then panels).
pub(crate) const NEW_COLORS: [[u8; 3]; 6] = [[196, 152, 98], [120, 78, 48], [222, 196, 150], [160, 110, 70], [235, 235, 228], [90, 92, 96]];

/// A swatch of `color`, `size` points square.
pub(crate) fn swatch(ui: &mut egui::Ui, color: Option<[u8; 3]>, size: f32) {
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
pub(super) fn material_choice(ui: &mut egui::Ui, doc: &Document, salt: (&str, Id), current: Option<Id>, width: f32) -> Option<Option<Id>> {
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

/// A texture's preview (points) under the pointer.
const THUMB_PREVIEW: f32 = 160.0;

/// The material chosen in its card: its name, colour, texture (its image, real size, rotation),
/// how many parts show it, its properties (docs/ui/catalog.md).
pub(super) fn material_info(ui: &mut egui::Ui, editor: &Editor, id: Id, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let Ok(m) = doc.material(id) else { return };
    let units = &doc.settings().units;
    let width = info_field_width(ui);
    // Walking the model once per revision, not per frame.
    let key = ui.make_persistent_id(("material_count", id));
    let revision = editor.history().revision();
    let count = match ui.data(|d| d.get_temp::<(u64, u64)>(key)) {
        Some((r, n)) if r == revision => n,
        _ => {
            let n = doc.material_counts().get(&id).copied().unwrap_or(0);
            ui.data_mut(|d| d.insert_temp(key, (revision, n)));
            n
        }
    };
    fn number(width: f32, tip: &str) -> Field<'_> {
        Field {
            hint: None,
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
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("common.material"));
        ui.end_row();
        field_label(ui, t!("panel.entity_info.name"));
        if let Some(name) = text_field(ui, (id, 2), &m.name, width) {
            actions.push(Action::Apply(Operation::EditMaterial { material: id, name, color: m.color }));
        }
        ui.end_row();
        field_label(ui, t!("panel.materials.color"));
        let mut c = m.color;
        if ui.color_edit_button_srgb(&mut c).tip(t!("panel.materials.color_tip")).changed() {
            actions.push(Action::Apply(Operation::EditMaterial { material: id, name: m.name.clone(), color: c }));
        }
        ui.end_row();
        ui.label(t!("panel.entity_info.in_model"));
        ui.label(count.to_string());
        ui.end_row();
        field_label(ui, t!("panel.materials.texture"));
        ui.horizontal(|ui| {
            let h = OPTION_HEIGHT;
            let ink = ui.visuals().text_color();
            let thumb = m.texture.as_ref().and_then(|t| crate::materials::thumbnail(ui.ctx(), doc, &t.image));
            let clicked = match &thumb {
                Some(tex) => {
                    let img = egui::Image::new(tex)
                        .uv(crate::materials::centre_square(tex))
                        .maintain_aspect_ratio(false)
                        .fit_to_exact_size(egui::vec2(h, h))
                        .corner_radius(3.0);
                    let r = ui.add(egui::Button::image(img).frame(false).min_size(egui::vec2(h, h)));
                    r.tip_ui(|ui| {
                        ui.label(t!("panel.materials.texture_change_tip"));
                        ui.add(egui::Image::new(tex).max_size(egui::vec2(THUMB_PREVIEW, THUMB_PREVIEW)).corner_radius(4.0));
                    })
                    .clicked()
                }
                None => {
                    let icon = egui::Image::new(icons::TEXTURE).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(ink);
                    let tip =
                        if m.texture.is_some() { t!("panel.materials.texture_change_tip") } else { t!("panel.materials.texture_tip") };
                    ui.add(egui::Button::image(icon).selected(m.texture.is_some()).min_size(egui::vec2(h, h))).tip(tip).clicked()
                }
            };
            if clicked {
                actions.push(Action::PickTexture(id));
            }
            if m.texture.is_some() {
                let close = egui::Image::new(icons::CLOSE).fit_to_exact_size(egui::vec2(12.0, 12.0)).tint(ink);
                if ui.add(egui::Button::image(close).min_size(egui::vec2(h, h))).tip(t!("panel.materials.texture_remove_tip")).clicked() {
                    actions.push(Action::Apply(Operation::SetTexture { material: id, texture: None, image: None }));
                }
            }
        });
        ui.end_row();
        if let Some(texture) = &m.texture {
            let set = |t: skraft_model::Texture| Action::Apply(Operation::SetTexture { material: id, texture: Some(t), image: None });
            let sizes = [
                (t!("panel.materials.texture_width"), t!("panel.materials.texture_width_tip")),
                (t!("panel.materials.texture_height"), t!("panel.materials.texture_height_tip")),
            ];
            for (k, (label, tip)) in sizes.iter().enumerate() {
                field_label(ui, label.clone());
                let v = texture.size[k];
                let (shown, exact) = (units.format(v), format!("{}{}", units.exact_number(v), units.suffix()));
                let fid = ui.make_persistent_id(("texture_size", id, k));
                if let Some(text) = ui.vertical(|ui| framed_field(ui, fid, &shown, &exact, number(width, tip))).inner
                    && let Ok(mm) = measure::length(&text, units, v)
                    && mm > 0.0
                {
                    let mut t = texture.clone();
                    t.size[k] = mm;
                    actions.push(set(t));
                }
                ui.end_row();
            }
            field_label(ui, t!("panel.materials.texture_rotation"));
            let deg = texture.rotation.to_degrees();
            let shown = format!("{}°", skraft_i18n::number(&format!("{}", (deg * 100.0).round() / 100.0)));
            let tip = t!("panel.materials.texture_rotation_tip");
            let fid = ui.make_persistent_id(("texture_rotation", id));
            if let Some(text) = ui.vertical(|ui| framed_field(ui, fid, &shown, &shown, number(width, &tip))).inner
                && let Ok(d) = measure::angle(&text, deg)
            {
                actions.push(set(skraft_model::Texture { rotation: d.to_radians(), ..texture.clone() }));
            }
            ui.end_row();
        }
    });
    props::section(ui, doc, &PropTarget::Material(id), t!("panel.properties.title"), actions);
}

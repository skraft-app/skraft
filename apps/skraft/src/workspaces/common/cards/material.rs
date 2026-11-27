//! A material in the entity info: its swatch, its choice, its fields.

use super::*;
use crate::kit::widgets::color::{BOX_RADIUS, color32};
use crate::kit::widgets::{self, IconButton};
use crate::workspaces::common::cards::materials;

/// Colours new materials take in turn (woods, then panels).
pub(crate) const NEW_COLORS: [[u8; 3]; 6] = [[196, 152, 98], [120, 78, 48], [222, 196, 150], [160, 110, 70], [235, 235, 228], [90, 92, 96]];

/// A family of the catalog, as the interface names one of its items.
pub(crate) fn family_label(family: Family) -> String {
    match family {
        Family::Material => t!("panel.materials.family.material"),
        Family::Covering => t!("panel.materials.family.covering"),
        Family::Finish => t!("panel.materials.family.finish"),
    }
}

/// A material's or a covering's type, as the interface names it
/// (docs/decisions/0057-materials-on-the-definition-typed-sources.md).
pub(crate) fn kind_label(kind: MaterialKind) -> String {
    match kind {
        MaterialKind::Generic => t!("panel.materials.kind.generic"),
        MaterialKind::GenericCovering => t!("panel.materials.kind.generic_covering"),
        MaterialKind::Solid => t!("panel.materials.kind.solid"),
        MaterialKind::Sheet => t!("panel.materials.kind.sheet"),
        MaterialKind::Coat => t!("panel.materials.kind.coat"),
        MaterialKind::Scheme => t!("panel.materials.kind.scheme"),
        MaterialKind::Profile => t!("panel.materials.kind.profile"),
        MaterialKind::Veneer => t!("panel.materials.kind.veneer"),
        MaterialKind::Edge => t!("panel.materials.kind.edge"),
    }
}

/// A material picked among those `keep` lets through (`none` first, the word agreeing with the
/// row's: « Aucun » matériau, « Aucune » finish), framed as a [`framed_field`]: its name and
/// a downward triangle. `Some(choice)` when it changed.
pub(super) fn material_choice(
    ui: &mut egui::Ui,
    doc: &Document,
    salt: (&str, Id),
    current: Option<Id>,
    width: f32,
    keep: impl Fn(&skraft_model::Material) -> bool,
    none: String,
) -> Option<Option<Id>> {
    item_choice(ui, doc, salt, current, width, keep, (none, true))
}

/// An item to pick among those `keep` lets through, the field showing `title` (« Aucune »
/// not offered): `Some(Some(item))` once one is picked.
pub(super) fn material_choice_titled(
    ui: &mut egui::Ui,
    doc: &Document,
    salt: (&str, Id),
    title: String,
    width: f32,
    keep: impl Fn(&skraft_model::Material) -> bool,
) -> Option<Option<Id>> {
    item_choice(ui, doc, salt, None, width, keep, (title, false))
}

/// [`material_choice`]'s field and menu: `empty` the text shown for none, and whether it is
/// offered.
fn item_choice(
    ui: &mut egui::Ui,
    doc: &Document,
    salt: (&str, Id),
    current: Option<Id>,
    width: f32,
    keep: impl Fn(&skraft_model::Material) -> bool,
    (empty, offer_none): (String, bool),
) -> Option<Option<Id>> {
    let name = |m: Option<Id>| match m {
        Some(id) => doc.material(id).map_or("?".to_owned(), |m| m.name.clone()),
        None => empty.clone(),
    };
    // An item's swatch, drawn as the « Matières » card's tiles (a scheme its coats, none
    // crossed): before its name in the field and in the menu.
    let swatch = |ui: &egui::Ui, r: egui::Rect, item: Option<Id>| match item {
        Some(id) => materials::paint_material(ui, doc, r, id, 2.0),
        None => materials::paint_none(ui, r, ui.visuals().weak_text_color(), 2.0),
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
        // The choice's swatch first, as its menu row shows it (not for a title: nothing chosen).
        let mut left = inner.left();
        if offer_none {
            let side = crate::kit::style::MENU_MARK;
            swatch(ui, egui::Rect::from_min_size(egui::pos2(left, inner.center().y - side / 2.0), egui::Vec2::splat(side)), current);
            left += side + ui.spacing().icon_spacing;
        }
        let clip = inner.with_max_x(icon.left() - 4.0);
        ui.painter_at(clip).galley(egui::pos2(left, inner.center().y - text.size().y / 2.0), text, ink);

        let mut picked = current;
        egui::Popup::menu(&response).id(id).width(width).style(crate::kit::style::menu).show(|ui| {
            ui.set_min_width(ui.available_width());
            // Each row its swatch where the menus' check boxes go: full width, the names lined up.
            let mut row = |ui: &mut egui::Ui, item: Option<Id>, label: &str| {
                let lead = crate::kit::style::menu_mark(move |ui, r, _| swatch(ui, r, item));
                if ui.add(crate::kit::style::menu_item_lead(lead, label).selected(picked == item)).clicked() {
                    picked = item;
                }
            };
            if offer_none {
                row(ui, None, &empty);
            }
            for (id, m) in doc.materials().filter(|(_, m)| keep(m)) {
                row(ui, Some(id), &m.name);
            }
        });
        (picked != current).then_some(picked)
    })
    .inner
}

/// A coat's rows (decision 0057): its blend, « Couvrant » or « Teintant », and its intensity.
fn coat_rows(ui: &mut egui::Ui, id: Id, m: &skraft_model::Material, width: f32, actions: &mut Vec<Action>) {
    field_label(ui, t!("panel.materials.blend"));
    let blends = [BlendMode::Cover, BlendMode::Tint];
    let labels = [t!("panel.materials.blend_cover"), t!("panel.materials.blend_tint")];
    let current = &labels[usize::from(m.blend == BlendMode::Tint)];
    let salt = ("coat_blend", id);
    let picked = ui.scope(|ui| super::props::choice_field(ui, ui.make_persistent_id(salt), Some(current), &labels, width)).inner;
    if let Some(k) = picked.and_then(|p| labels.iter().position(|l| *l == p)) {
        actions.push(Action::Apply(Operation::SetCoat { material: id, blend: blends[k], intensity: m.intensity }));
    }
    ui.end_row();
    field_label(ui, t!("panel.materials.intensity"));
    let shown = format!("{} %", skraft_i18n::number(&format!("{}", (m.intensity * 100.0).round())));
    let tip = t!("panel.materials.intensity_tip");
    let field = Field {
        hint: None,
        tip: Some(&tip),
        width,
        align: egui::Align2::RIGHT_CENTER,
        lit: false,
        addon: None,
        blur_applies: true,
        empty_applies: false,
        handle: None,
    };
    let fid = ui.make_persistent_id(("coat_intensity", id));
    if let Some(text) = ui.vertical(|ui| framed_field(ui, fid, &shown, &shown, field)).inner
        && let Ok(v) = text.trim().trim_end_matches('%').trim().replace(',', ".").parse::<f32>()
    {
        let intensity = (v / 100.0).clamp(0.0, 1.0);
        actions.push(Action::Apply(Operation::SetCoat { material: id, blend: m.blend, intensity }));
    }
    ui.end_row();
}

/// A scheme's rows (decision 0057): a coat of the catalog to add on top, then its coats as
/// they lie, the top one first, each moved up or down or taken out.
fn scheme_rows(ui: &mut egui::Ui, doc: &Document, id: Id, m: &skraft_model::Material, width: f32, actions: &mut Vec<Action>) {
    let set = |coats: Vec<Id>| Action::Apply(Operation::SetCoats { material: id, coats });
    field_label(ui, t!("panel.materials.coats"));
    let is_coat = |x: &skraft_model::Material| x.kind == Some(MaterialKind::Coat);
    if let Some(Some(coat)) = material_choice_titled(ui, doc, ("scheme_add", id), t!("panel.materials.coat_add"), width, is_coat) {
        let mut coats = m.coats.clone();
        coats.push(coat);
        actions.push(set(coats));
    }
    ui.end_row();
    // The top one first: `k` counts from the bottom, as the scheme holds them.
    for (k, coat) in m.coats.iter().enumerate().rev() {
        ui.label("");
        ui.horizontal(|ui| {
            ui.set_width(width);
            let c = doc.material(*coat).ok();
            // Its swatch as the menus and the « Matières » card draw it.
            let (swatch, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
            match c {
                Some(_) => materials::paint_material(ui, doc, swatch, *coat, 2.0),
                None => materials::paint_none(ui, swatch, ui.visuals().weak_text_color(), 2.0),
            }
            ui.label(c.map_or("?", |c| c.name.as_str()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if small(ui, icons::CLOSE, t!("panel.materials.coat_remove_tip"), true).clicked() {
                    let mut coats = m.coats.clone();
                    coats.remove(k);
                    actions.push(set(coats));
                }
                if small(ui, icons::ARROW_DOWN, t!("panel.materials.coat_down_tip"), k > 0).clicked() {
                    let mut coats = m.coats.clone();
                    coats.swap(k, k - 1);
                    actions.push(set(coats));
                }
                if small(ui, icons::ARROW_UP, t!("panel.materials.coat_up_tip"), k + 1 < m.coats.len()).clicked() {
                    let mut coats = m.coats.clone();
                    coats.swap(k, k + 1);
                    actions.push(set(coats));
                }
            });
        });
        ui.end_row();
    }
}

/// The small unframed button of the material's rows (a coat moved or taken out, a texture
/// removed).
fn small(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, tip: String, enabled: bool) -> egui::Response {
    IconButton::new(icon, SMALL_ICON, SMALL_ICON).enabled(enabled).show(ui).tip(tip)
}

/// A [`small`] button's icon (points).
const SMALL_ICON: f32 = 12.0;

/// The height of a material's colour or texture box in the entity info (points).
const TEXTURE_HEIGHT: f32 = 64.0;

/// One half of the look row, `width` wide: its box, its caption under it, centred.
fn look_half(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(width, TEXTURE_HEIGHT * 2.0), egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 2.0;
        add(ui)
    });
}

/// The caption under a box of the look row.
fn caption(ui: &mut egui::Ui, text: String) {
    ui.label(crate::kit::style::caption(text));
}

/// An editable colour box of `size` (docs/ui/interface.md#colours): the colour picked, when
/// changed.
fn color_box(ui: &mut egui::Ui, id: Id, color: [u8; 3], size: egui::Vec2) -> Option<[u8; 3]> {
    let (r, picked) = crate::kit::widgets::color::color_box(ui, ("material_color", id), color, size, true);
    r.tip(t!("panel.materials.color_tip"));
    picked
}

/// A textured material's colour, its image's mean: shown, locked, copied only.
fn locked_color_box(ui: &mut egui::Ui, id: Id, color: [u8; 3], size: egui::Vec2) {
    let (r, _) = crate::kit::widgets::color::color_box(ui, ("material_color", id), color, size, false);
    widgets::picture_badge(ui, r.rect, egui::Align2::RIGHT_BOTTOM, icons::LOCK, SMALL_ICON);
    r.on_hover_cursor(egui::CursorIcon::NotAllowed).tip(t!("panel.materials.color_locked_tip"));
}

/// A texture's image of `size`, its middle filling it, the whole image under the pointer: its
/// rect, and whether it was clicked (to change it).
fn texture_box(ui: &mut egui::Ui, tex: &egui::TextureHandle, size: egui::Vec2) -> (egui::Rect, bool) {
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
    materials::paint_texture(ui, tex, rect, true, BOX_RADIUS);
    let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
    crate::kit::handles::publish(|| "info:texture".to_owned(), rect);
    let clicked = r
        .tip_ui(|ui| {
            ui.label(t!("panel.materials.texture_change_tip"));
            // The whole image, turned as on the parts.
            let [w, h] = tex.size().map(|v| v.max(1) as f32);
            let scale = (THUMB_PREVIEW / w.max(h)).min(1.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h) * scale, egui::Sense::hover());
            materials::paint_texture(ui, tex, rect, false, 4.0);
        })
        .clicked();
    (rect, clicked)
}

/// The texture's box without an image: a dashed frame offering to add one; its rect, and whether
/// it was clicked.
fn empty_texture_box(ui: &mut egui::Ui, size: egui::Vec2) -> (egui::Rect, bool) {
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
    crate::kit::handles::publish(|| "info:texture".to_owned(), rect);
    let visuals = ui.visuals();
    let ink = if r.hovered() { visuals.text_color() } else { visuals.weak_text_color() };
    let painter = ui.painter();
    painter.rect_filled(rect, BOX_RADIUS, visuals.extreme_bg_color);
    crate::kit::overlay::dashed_rect(painter, rect.shrink(0.5), egui::Stroke::new(1.0, ink), crate::kit::overlay::GHOST);
    let icon = egui::Rect::from_center_size(rect.center() - egui::vec2(0.0, 8.0), egui::Vec2::splat(20.0));
    egui::Image::new(icons::IMAGE).tint(ink).paint_at(ui, icon);
    let font = egui::TextStyle::Small.resolve(ui.style());
    ui.painter().text(rect.center() + egui::vec2(0.0, 12.0), egui::Align2::CENTER_CENTER, t!("panel.materials.texture_add"), font, ink);
    let clicked = r.on_hover_cursor(egui::CursorIcon::PointingHand).tip(t!("panel.materials.texture_tip")).clicked();
    (rect, clicked)
}

/// The texture's remove button, over its box's top right corner: whether it was clicked.
fn remove_badge(ui: &mut egui::Ui, rect: egui::Rect, id: Id) -> bool {
    let key = ui.make_persistent_id(("texture_remove", id));
    let r = widgets::badge_button(ui, rect, egui::Align2::RIGHT_TOP, icons::CLOSE, SMALL_ICON - 2.0, key)
        .tip(t!("panel.materials.texture_remove_tip"));
    crate::kit::handles::publish(|| "info:texture_remove".to_owned(), r.rect);
    r.clicked()
}

/// The texture's quarter turn button, over its box's bottom left corner: whether it was clicked.
fn rotate_badge(ui: &mut egui::Ui, rect: egui::Rect, id: Id) -> bool {
    let key = ui.make_persistent_id(("texture_rotate", id));
    let r = widgets::badge_button(ui, rect, egui::Align2::LEFT_BOTTOM, icons::ROTATE, SMALL_ICON, key)
        .tip(t!("panel.materials.texture_rotate_tip"));
    crate::kit::handles::publish(|| "info:texture_rotate".to_owned(), r.rect);
    r.clicked()
}

/// A texture's preview (points) under the pointer.
const THUMB_PREVIEW: f32 = 256.0;

/// The material chosen in its card: its family and type, name, colour, texture (its image turned
/// by quarter turns, its real size), its properties (docs/ui/catalog.md); how many parts show it is the card's.
pub(super) fn material_info(ui: &mut egui::Ui, editor: &Editor, id: Id, actions: &mut Vec<Action>) {
    let doc = editor.document();
    let Ok(m) = doc.material(id) else { return };
    let units = &doc.settings().units;
    let width = info_field_width(ui);
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
        // Its family, its name; a material's or a covering's type, among its family's: how it is
        // counted.
        ui.label(t!("panel.entity_info.type"));
        ui.label(family_label(m.family()));
        ui.end_row();
        field_label(ui, t!("panel.entity_info.name"));
        if let Some(name) = text_field(ui, (id, 2), &m.name, width) {
            actions.push(Action::Apply(Operation::EditMaterial { material: id, name, color: m.color }));
        }
        ui.end_row();
        if let Some(kind) = m.kind {
            field_label(ui, t!("panel.materials.kind_label"));
            let kinds = m.family().kinds();
            let labels: Vec<String> = kinds.iter().map(|k| kind_label(*k)).collect();
            let current = kind_label(kind);
            let salt = ("material_kind", id);
            let picked = ui.scope(|ui| super::props::choice_field(ui, ui.make_persistent_id(salt), Some(&current), &labels, width)).inner;
            if let Some(k) = picked.and_then(|p| labels.iter().position(|l| *l == p)) {
                actions.push(Action::Apply(Operation::SetMaterialKind { material: id, kind: kinds[k] }));
            }
            ui.end_row();
        }
        if m.is_scheme() {
            scheme_rows(ui, doc, id, m, width, actions);
        } else {
            if m.kind == Some(MaterialKind::Coat) {
                coat_rows(ui, id, m, width, actions);
            }
            // A finish has its colour alone (docs/decisions/0057-materials-on-the-definition-typed-sources.md).
            if m.family() == Family::Finish {
                field_label(ui, t!("panel.materials.color"));
                if let Some(c) = ui.vertical(|ui| color_box(ui, id, m.color, egui::vec2(width, TEXTURE_HEIGHT))).inner {
                    actions.push(Action::Apply(Operation::EditMaterial { material: id, name: m.name.clone(), color: c }));
                }
                ui.end_row();
                return;
            }
            // Its colour and its texture side by side, the texture winning: a textured material's
            // colour is its image's mean, locked until the image is removed (docs/render/look.md#textures).
            field_label(ui, t!("panel.materials.look"));
            // Top-down, as the fields: a grid centres a cell on the row's height of the frame
            // before, which a cell taller than a row would make drift.
            ui.vertical(|ui| {
                let gap = ui.spacing().item_spacing.x;
                let half = egui::vec2(((width - gap) / 2.0).max(OPTION_HEIGHT), TEXTURE_HEIGHT);
                ui.horizontal_top(|ui| {
                    look_half(ui, half.x, |ui| {
                        if m.texture.is_some() {
                            locked_color_box(ui, id, m.color, half);
                            caption(ui, t!("panel.materials.color_mean"));
                        } else {
                            if let Some(c) = color_box(ui, id, m.color, half) {
                                actions.push(Action::Apply(Operation::EditMaterial { material: id, name: m.name.clone(), color: c }));
                            }
                            caption(ui, t!("panel.materials.color"));
                        }
                    });
                    look_half(ui, half.x, |ui| {
                        let thumb =
                            m.texture.as_ref().and_then(|t| materials::thumbnail(ui.ctx(), doc, &t.image, materials::quarters(t.rotation)));
                        let (rect, change) = match (&m.texture, &thumb) {
                            (Some(_), Some(tex)) => texture_box(ui, tex, half),
                            // Its image not decoded yet: its colour meanwhile, as the 3D view.
                            (Some(_), None) => {
                                let (rect, r) = ui.allocate_exact_size(half, egui::Sense::click());
                                ui.painter().rect_filled(rect, BOX_RADIUS, color32(m.color));
                                (rect, r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked())
                            }
                            (None, _) => empty_texture_box(ui, half),
                        };
                        if change {
                            actions.push(Action::PickTexture(id));
                        }
                        if m.texture.is_some() && remove_badge(ui, rect, id) {
                            actions.push(Action::Apply(Operation::SetTexture { material: id, texture: None, image: None, color: None }));
                        }
                        // A quarter turn more at each click: the grain the other way.
                        if let Some(t) = &m.texture
                            && rotate_badge(ui, rect, id)
                        {
                            let rotation = f64::from((materials::quarters(t.rotation) + 1) % 4) * std::f64::consts::FRAC_PI_2;
                            let texture = Some(skraft_model::Texture { rotation, ..t.clone() });
                            actions.push(Action::Apply(Operation::SetTexture { material: id, texture, image: None, color: None }));
                        }
                        caption(ui, t!("panel.materials.texture"));
                    });
                });
            });
            ui.end_row();
            if let Some(texture) = &m.texture {
                let set = |t: skraft_model::Texture| {
                    Action::Apply(Operation::SetTexture { material: id, texture: Some(t), image: None, color: None })
                };
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
            }
        }
    });
    props::section(ui, doc, &PropTarget::Material(id), t!("panel.properties.title"), actions);
}

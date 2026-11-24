//! The entity info of the drawing (docs/ui/drawing.md).

use skraft_model::{
    Border, Corner, Direction, Drawing, Element, ElementKind, Instance, Look, Paper, Place, Sheet, Side, Subject, Value, View, ViewKind,
    ViewStyle,
};

use super::*;

/// What the rows of the drawing's entity info read.
#[derive(Clone, Copy)]
struct DrawingCx<'a> {
    editor: &'a Editor,
    doc: &'a Document,
    d: &'a Drawing,
    units: &'a Units,
    /// The fields' width.
    field: f32,
}

/// A choice of the entity info, `field` wide.
fn info_combo(field: f32, salt: (&str, u64)) -> egui::ComboBox<'_> {
    egui::ComboBox::from_id_salt(salt).icon(crate::kit::icons::combo_chevron).width(field)
}

/// The entity info in « Mise en plan »: the view picked on the sheet shown — its name, what it
/// shows, from where, at what scale, in what style; a projected view its base and side —, else
/// the sheet — its name, its paper, the document's projection.
pub fn drawing_info(
    ui: &mut egui::Ui,
    editor: &Editor,
    st: &mut crate::drawing::sheet::SheetState,
    title: &str,
    actions: &mut Vec<Action>,
) {
    use crate::drawing::sheet::InfoOn;
    let doc = editor.document();
    let d = doc.drawing();
    let field = info_field_width(ui);
    // Several picked: how many.
    let many = crate::drawing::sheet::selection(editor, st).len();
    if many > 1 {
        ui.label(tp!("panel.entity_info.selected", many));
        return;
    }
    // Nothing picked: the document or the presentation, if chosen in the navigator.
    if many == 0 {
        match st.info {
            InfoOn::Document => return document_info(ui, doc, title, field, actions),
            InfoOn::Presentation => {
                if let Some(p) = st.presentation(d) {
                    return presentation_info(ui, doc, p, field, actions);
                }
            }
            InfoOn::Sheet => {}
        }
    }
    let Some(sheet) = st.shown(d) else {
        ui.weak(t!("drawing.no_sheet"));
        return;
    };
    let cx = DrawingCx { editor, doc, d, units: &doc.settings().units, field };
    // A free element picked: its kind; a text its words and size; an instance its place.
    if let Some(el) = crate::drawing::sheet::selected_element(editor, st).filter(|el| !matches!(el.kind, ElementKind::View(_))) {
        info_grid(ui).show(ui, |ui| element_rows(ui, cx, el, sheet, &st.open, actions));
        return;
    }
    if let Some((id, view)) = crate::drawing::sheet::selected_view(editor, st) {
        info_grid(ui).show(ui, |ui| view_rows(ui, cx, id, view, st, actions));
        return;
    }
    info_grid(ui).show(ui, |ui| sheet_rows(ui, cx, sheet, actions));
}

/// What a free element is, in words.
fn element_type(kind: &ElementKind) -> String {
    match kind {
        ElementKind::Text { .. } => t!("panel.entity_info.type_text"),
        ElementKind::Line { .. } => t!("panel.entity_info.type_line"),
        ElementKind::Rect { .. } => t!("panel.entity_info.type_rect"),
        ElementKind::Circle { .. } => t!("panel.entity_info.type_circle"),
        ElementKind::Dimension(dim) => match dim.kind {
            skraft_model::DimKind::Linear { orient: skraft_model::Orient::Horizontal, .. } => t!("drawing.dim.horizontal"),
            skraft_model::DimKind::Linear { orient: skraft_model::Orient::Vertical, .. } => t!("drawing.dim.vertical"),
            skraft_model::DimKind::Linear { .. } => t!("drawing.dim.aligned"),
            skraft_model::DimKind::Radius { diameter: false, .. } => t!("drawing.dim.radius"),
            skraft_model::DimKind::Radius { .. } => t!("drawing.dim.diameter"),
            skraft_model::DimKind::Angle { .. } => t!("drawing.dim.angle"),
        },
        ElementKind::Instance(_) => t!("panel.entity_info.type_instance"),
        _ => t!("panel.entity_info.type_image"),
    }
}

/// The rows of a free element picked (`open`: the instances entered).
fn element_rows(ui: &mut egui::Ui, cx: DrawingCx<'_>, el: &Element, sheet: &Sheet, open: &[Id], actions: &mut Vec<Action>) {
    let DrawingCx { editor, d, .. } = cx;
    let id = el.id;
    ui.label(t!("panel.entity_info.type"));
    ui.label(element_type(&el.kind));
    ui.end_row();
    // A dimension's value as the model is now, or why it has none.
    if let ElementKind::Dimension(dim) = &el.kind
        && let Some(s) = crate::drawing::sheet::dim_shape(editor, dim, d.projection_of(dim.view))
    {
        ui.label(t!("panel.entity_info.value"));
        if s.orphan {
            ui.colored_label(ui.visuals().error_fg_color, t!("drawing.dim.orphan")).on_hover_text(t!("drawing.dim.orphan_tip"));
        } else {
            ui.label(&s.text);
        }
        ui.end_row();
    }
    if let ElementKind::Text { at, text, size } = &el.kind {
        text_rows(ui, cx, id, (at, text, size), actions);
    }
    // An instance: its definition, its place, its values.
    if let ElementKind::Instance(i) = &el.kind {
        instance_rows(ui, cx, id, i, sheet, open, actions);
    }
}

/// A text's words, with the fields to put in them, and its size.
fn text_rows(ui: &mut egui::Ui, cx: DrawingCx<'_>, id: Id, (at, text, size): (&glam::DVec2, &String, &f64), actions: &mut Vec<Action>) {
    let DrawingCx { d, field, .. } = cx;
    let set =
        |text: String, size: f64| Action::Apply(Operation::SetElement { element: id, kind: ElementKind::Text { at: *at, text, size } });
    field_label(ui, t!("panel.entity_info.text"));
    ui.vertical(|ui| {
        // The words as typed, applied when the field is left.
        let key = ui.make_persistent_id(("drawing_text", id.0));
        let mut buf: String = ui.data(|m| m.get_temp::<String>(key)).unwrap_or_else(|| text.clone());
        let edit = ui.add(egui::TextEdit::multiline(&mut buf).desired_width(field).desired_rows(2));
        if edit.has_focus() {
            ui.data_mut(|m| m.insert_temp(key, buf.clone()));
        } else {
            ui.data_mut(|m| m.remove::<String>(key));
        }
        if edit.lost_focus() && buf != *text {
            actions.push(set(buf.clone(), *size));
        }
        // A field put at the end (docs/ui/drawing.md#fields).
        let append = |f: &str| set(format!("{text}{{{f}}}"), *size);
        field_menu(ui, d, id, &append, actions);
    });
    ui.end_row();
    ui.label(t!("panel.entity_info.text_size"));
    let mut v = *size;
    if ui.add(egui::DragValue::new(&mut v).speed(0.1).range(0.5..=200.0).suffix(" mm")).changed() {
        actions.push(set(text.clone(), v));
    }
    ui.end_row();
}

/// The button putting a field at the end of a text: Skraft's, those the drawing's texts name, a
/// new one; `append` what that makes of the text.
fn field_menu(ui: &mut egui::Ui, d: &Drawing, id: Id, append: &dyn Fn(&str) -> Action, actions: &mut Vec<Action>) {
    let button = ui.button(t!("drawing.insert_field"));
    crate::kit::handles::publish(|| "drawing_insert_field".into(), button.rect);
    let menu = egui::Popup::menu(&button).style(crate::kit::style::menu);
    menu.close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        // Skraft's own, those the drawing's texts already name, a new one.
        let custom = crate::drawing::sheet::custom_fields(d.all_field_names());
        let own = crate::drawing::sheet::FIELDS.iter().map(|f| (*f, field_name(f)));
        for (f, label) in own.chain(custom.iter().map(|f| (f.as_str(), f.clone()))) {
            let item = ui.add(crate::kit::style::menu_item(label, None, false));
            crate::kit::handles::publish(|| format!("drawing_field:{f}"), item.rect);
            if item.clicked() {
                actions.push(append(f));
                ui.close();
            }
        }
        ui.separator();
        let key = ui.make_persistent_id(("drawing_new_field", id.0));
        let mut name = ui.data(|m| m.get_temp::<String>(key)).unwrap_or_default();
        let edit = ui.add(egui::TextEdit::singleline(&mut name).hint_text(t!("drawing.new_field")).desired_width(150.0));
        crate::kit::handles::publish(|| "drawing_new_field".into(), edit.rect);
        let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clean = name.replace(['{', '}', '.'], " ").trim().to_owned();
        ui.data_mut(|m| m.insert_temp(key, name));
        if entered && !clean.is_empty() {
            ui.data_mut(|m| m.remove::<String>(key));
            actions.push(append(&clean));
            ui.close();
        }
    });
}

/// An instance's definition, place and values.
fn instance_rows(ui: &mut egui::Ui, cx: DrawingCx<'_>, id: Id, i: &Instance, sheet: &Sheet, open: &[Id], actions: &mut Vec<Action>) {
    let DrawingCx { doc, d, units, field, .. } = cx;
    let set_place =
        |place: Place| Action::Apply(Operation::SetElement { element: id, kind: ElementKind::Instance(Instance { place, ..i.clone() }) });
    let composition = d.composition(i.composition);
    if let Some(b) = composition {
        field_label(ui, t!("panel.entity_info.name"));
        if let Some(name) = text_field(ui, (id, 12), &b.name, field) {
            actions.push(Action::Apply(Operation::RenameComposition { composition: b.id, name }));
        }
        ui.end_row();
    }
    // A corner of the paper only right on a sheet; changing it keeps it where it is.
    if open.is_empty() {
        let label = |c: Option<Corner>| match c {
            None => t!("drawing.anchor.free"),
            Some(Corner::BottomLeft) => t!("drawing.anchor.bottom_left"),
            Some(Corner::BottomRight) => t!("drawing.anchor.bottom_right"),
            Some(Corner::TopLeft) => t!("drawing.anchor.top_left"),
            Some(Corner::TopRight) => t!("drawing.anchor.top_right"),
        };
        field_label(ui, t!("panel.entity_info.anchor"));
        info_combo(field, ("drawing_anchor", id.0)).selected_text(label(i.place.corner)).show_ui(ui, |ui| {
            for c in [None, Some(Corner::BottomLeft), Some(Corner::BottomRight), Some(Corner::TopLeft), Some(Corner::TopRight)] {
                if ui.selectable_label(i.place.corner == c, label(c)).clicked() && i.place.corner != c {
                    actions.push(set_place(i.place.with_corner(c, sheet.paper)));
                }
            }
        });
        ui.end_row();
    }
    let at = i.place.at;
    if let Some(Value::Length(x)) = typed_row(ui, "X".to_owned(), (id, 13), &Value::Length(at.x), units, field) {
        actions.push(set_place(Place { at: glam::DVec2::new(x, at.y), ..i.place }));
    }
    if let Some(Value::Length(y)) = typed_row(ui, "Y".to_owned(), (id, 14), &Value::Length(at.y), units, field) {
        actions.push(set_place(Place { at: glam::DVec2::new(at.x, y), ..i.place }));
    }
    let turn = Value::Angle(i.place.rotation);
    if let Some(Value::Angle(r)) = typed_row(ui, t!("panel.entity_info.rotation"), (id, 15), &turn, units, field) {
        actions.push(set_place(Place { rotation: r, ..i.place }));
    }
    if let Some(b) = composition {
        let names = field_rows(d.field_names(&b.elements), Some(&i.props));
        let on = ValuesOn::Instance { id, instance: i, sheet: sheet.id, above: open };
        value_rows(ui, doc, &on, names, field, actions);
    }
}

/// The rows of a view picked.
fn view_rows(
    ui: &mut egui::Ui,
    cx: DrawingCx<'_>,
    id: Id,
    view: &View,
    st: &mut crate::drawing::sheet::SheetState,
    actions: &mut Vec<Action>,
) {
    let DrawingCx { d, field, .. } = cx;
    let set = |v: View| Action::Apply(Operation::SetElement { element: id, kind: ElementKind::View(v) });
    ui.label(t!("panel.entity_info.type"));
    ui.label(match view.kind {
        ViewKind::Base { .. } => t!("panel.entity_info.type_view"),
        ViewKind::Projected { .. } => t!("panel.entity_info.type_projected_view"),
    });
    ui.end_row();
    field_label(ui, t!("panel.entity_info.name"));
    if let Some(name) = text_field(ui, (id, 10), &view.name, field) {
        actions.push(set(View { name, ..view.clone() }));
    }
    ui.end_row();
    match &view.kind {
        ViewKind::Base { .. } => base_view_rows(ui, cx, id, view, st, actions),
        ViewKind::Projected { .. } => projected_view_rows(ui, d, id, view),
    }
    if let ViewKind::Base { look, .. } = &view.kind {
        ui.label(t!("panel.entity_info.view_frame"));
        let automatic = view.crop.is_none() && look.frame().1.is_none();
        ui.label(if automatic { t!("drawing.frame_auto") } else { t!("drawing.frame_free") });
        ui.end_row();
    }
    field_label(ui, t!("panel.entity_info.style"));
    let label = |s: ViewStyle| match s {
        ViewStyle::Lines => t!("menu.drawing.style_lines"),
        ViewStyle::Rendered => t!("menu.drawing.style_rendered"),
        ViewStyle::Hybrid => t!("menu.drawing.style_hybrid"),
    };
    info_combo(field, ("drawing_style", id.0)).selected_text(label(view.style)).show_ui(ui, |ui| {
        for s in [ViewStyle::Lines, ViewStyle::Rendered, ViewStyle::Hybrid] {
            if ui.selectable_label(view.style == s, label(s)).clicked() {
                actions.push(set(View { style: s, ..view.clone() }));
            }
        }
    });
    ui.end_row();
}

/// What a view shows, changed in the dialog a new view is chosen in.
fn view_subject_row(ui: &mut egui::Ui, doc: &Document, st: &mut crate::drawing::sheet::SheetState, id: Id, subject: &Subject, field: f32) {
    ui.label(t!("panel.entity_info.subject"));
    let shows = match subject {
        Subject::Model => t!("drawing.subject.model"),
        Subject::Component(c) => doc.component(*c).map_or_else(|_| "—".into(), |c| c.name.clone()),
        Subject::Occurrences(paths) => t!("drawing.subject.occurrences", count = paths.len()),
    };
    // Changed in the dialog a new view is chosen in (decision 0051).
    let change = ui.add_sized([field, ui.spacing().interact_size.y], egui::Button::new(shows).truncate());
    crate::kit::handles::publish(|| "view_subject".into(), change.rect);
    if change.tip(t!("drawing.picker.change_tip")).clicked() {
        let purpose = crate::drawing::view_picker::Purpose::Change(id);
        st.picker = Some(crate::drawing::view_picker::ViewPicker::new(doc, purpose, subject.clone()));
    }
    ui.end_row();
}

/// A base view's subject, direction and scale.
fn base_view_rows(
    ui: &mut egui::Ui,
    cx: DrawingCx<'_>,
    id: Id,
    view: &View,
    st: &mut crate::drawing::sheet::SheetState,
    actions: &mut Vec<Action>,
) {
    let DrawingCx { editor, doc, d, field, .. } = cx;
    let ViewKind::Base { subject, look, scale, at } = &view.kind else { return };
    let set = |v: View| Action::Apply(Operation::SetElement { element: id, kind: ElementKind::View(v) });
    view_subject_row(ui, doc, st, id, subject, field);
    // Another scale shows the same part of it; another direction, the whole.
    let was = look;
    let base = |look: Look, s: f64| View {
        crop: if look == *was { view.crop.map(|c| c.scaled(s / *scale)) } else { None },
        kind: ViewKind::Base { subject: subject.clone(), look, scale: s, at: *at },
        ..view.clone()
    };
    field_label(ui, t!("panel.entity_info.direction"));
    let shown = match look {
        Look::Standard(d) => direction_label(*d),
        Look::Camera { .. } => t!("drawing.direction.camera"),
    };
    info_combo(field, ("drawing_direction", id.0)).selected_text(shown).show_ui(ui, |ui| {
        for d in [Direction::Front, Direction::Back, Direction::Left, Direction::Right, Direction::Top, Direction::Bottom, Direction::Iso] {
            if ui.selectable_label(*look == Look::Standard(d), direction_label(d)).clicked() {
                actions.push(set(base(Look::Standard(d), *scale)));
            }
        }
        ui.separator();
        // The 3D view's camera now, perspective or parallel.
        if ui.selectable_label(matches!(look, Look::Camera { .. }), t!("drawing.direction.camera")).clicked() {
            // A perspective keeps the frame the view has now.
            let size = d.resolve(id).map_or(glam::DVec2::splat(100.0), |r| {
                let (lo, hi) = crate::drawing::sheet::view_frame(editor, &r);
                (hi - lo).max(glam::DVec2::splat(10.0))
            });
            let look = crate::drawing::view_framer::camera_look(doc, subject, &editor.camera, size);
            actions.push(set(base(look, *scale)));
        }
    });
    ui.end_row();
    if look.frame().1.is_some() {
        ui.label(t!("panel.entity_info.scale"));
        ui.label(t!("drawing.no_scale"));
        ui.end_row();
        let hidden = d
            .container_of(id)
            .and_then(|c| d.elements_in(c))
            .map_or(0, |list| list.iter().filter(|el| matches!(&el.kind, ElementKind::Dimension(dim) if dim.view == id)).count());
        if hidden > 0 {
            ui.label(t!("panel.entity_info.view_dimensions"));
            ui.label(t!("drawing.dims_hidden", count = hidden));
            ui.end_row();
        }
    } else {
        field_label(ui, t!("panel.entity_info.scale"));
        info_combo(field, ("drawing_scale", id.0)).selected_text(crate::drawing::sheet::scale_label(*scale)).show_ui(ui, |ui| {
            for s in crate::drawing::sheet::SCALES {
                if ui.selectable_label((s - scale).abs() < 1e-12, crate::drawing::sheet::scale_label(*s)).clicked() {
                    actions.push(set(base(look.clone(), *s)));
                }
            }
        });
        ui.end_row();
    }
}

/// A projected view's base, side and scale.
fn projected_view_rows(ui: &mut egui::Ui, d: &Drawing, id: Id, view: &View) {
    let ViewKind::Projected { base, side, .. } = &view.kind else { return };
    let base_name = d
        .element(*base)
        .and_then(|el| match &el.kind {
            ElementKind::View(v) => Some(v.name.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let side = match side {
        Side::Left => t!("drawing.side.left"),
        Side::Right => t!("drawing.side.right"),
        Side::Above => t!("drawing.side.above"),
        Side::Below => t!("drawing.side.below"),
    };
    ui.label(t!("panel.entity_info.projected_from"));
    ui.label(format!("{base_name}, {side}"));
    ui.end_row();
    ui.label(t!("panel.entity_info.scale"));
    ui.label(d.resolve(id).map_or_else(String::new, |r| crate::drawing::sheet::scale_label(r.scale)));
    ui.end_row();
}

/// The rows of the sheet.
fn sheet_rows(ui: &mut egui::Ui, cx: DrawingCx<'_>, sheet: &Sheet, actions: &mut Vec<Action>) {
    let DrawingCx { doc, d, units, field, .. } = cx;
    let id = sheet.id;
    let paper = sheet.paper;
    let portrait = paper.height > paper.width;
    let landscape = |p: Paper| if p.height > p.width { Paper { width: p.height, height: p.width } } else { p };
    let set_paper = |p: Paper, portrait: bool| {
        let p = landscape(p);
        let p = if portrait { Paper { width: p.height, height: p.width } } else { p };
        Action::Apply(Operation::SetSheet { sheet: id, name: sheet.name.clone(), paper: p, grid: sheet.grid, border: sheet.border })
    };
    const SIZES: [(&str, Paper); 5] = [("A4", Paper::A4), ("A3", Paper::A3), ("A2", Paper::A2), ("A1", Paper::A1), ("A0", Paper::A0)];
    let named = SIZES.iter().find(|(_, p)| *p == landscape(paper)).map(|(n, _)| n.to_string());
    ui.label(t!("panel.entity_info.type"));
    ui.label(t!("panel.entity_info.type_sheet"));
    ui.end_row();
    field_label(ui, t!("panel.entity_info.name"));
    if let Some(name) = text_field(ui, (id, 11), &sheet.name, field) {
        actions.push(Action::Apply(Operation::SetSheet { sheet: id, name, paper, grid: sheet.grid, border: sheet.border }));
    }
    ui.end_row();
    field_label(ui, t!("panel.entity_info.paper"));
    let shown = named.unwrap_or_else(|| format!("{} × {}", units.number(paper.width), units.number(paper.height)));
    info_combo(field, ("drawing_paper", id.0)).selected_text(shown).show_ui(ui, |ui| {
        for (n, p) in SIZES {
            if ui.selectable_label(p == landscape(paper), n).clicked() {
                actions.push(set_paper(p, portrait));
            }
        }
    });
    ui.end_row();
    field_label(ui, t!("panel.entity_info.orientation"));
    let orientation = |portrait: bool| if portrait { t!("drawing.paper.portrait") } else { t!("drawing.paper.landscape") };
    info_combo(field, ("drawing_orientation", id.0)).selected_text(orientation(portrait)).show_ui(ui, |ui| {
        for o in [false, true] {
            if ui.selectable_label(portrait == o, orientation(o)).clicked() && o != portrait {
                actions.push(set_paper(paper, o));
            }
        }
    });
    ui.end_row();
    sheet_grid_row(ui, cx, sheet, actions);
    sheet_border_rows(ui, cx, sheet, actions);
    let names = field_rows(d.field_names(&sheet.elements), Some(&sheet.props));
    value_rows(ui, doc, &ValuesOn::Sheet(sheet), names, field, actions);
}

/// The sheet's grid.
fn sheet_grid_row(ui: &mut egui::Ui, cx: DrawingCx<'_>, sheet: &Sheet, actions: &mut Vec<Action>) {
    let DrawingCx { units, field, .. } = cx;
    let (id, paper) = (sheet.id, sheet.paper);
    // The grid the points drawn snap to when nothing else is near (docs/ui/drawing.md#snapping).
    field_label(ui, t!("panel.entity_info.grid"));
    let grid_label = |g: f64| if g > 0.0 { format!("{} mm", units.number(g)) } else { t!("drawing.grid_none") };
    info_combo(field, ("drawing_grid", id.0)).selected_text(grid_label(sheet.grid)).show_ui(ui, |ui| {
        for g in [0.0, 1.0, 2.0, 5.0, 10.0] {
            if ui.selectable_label(sheet.grid == g, grid_label(g)).clicked() && g != sheet.grid {
                actions.push(Action::Apply(Operation::SetSheet {
                    sheet: id,
                    name: sheet.name.clone(),
                    paper,
                    grid: g,
                    border: sheet.border,
                }));
            }
        }
    });
    ui.end_row();
}

/// The sheet's frame.
fn sheet_border_rows(ui: &mut egui::Ui, cx: DrawingCx<'_>, sheet: &Sheet, actions: &mut Vec<Action>) {
    let DrawingCx { units, field, .. } = cx;
    let (id, paper) = (sheet.id, sheet.paper);
    // Its frame, drawn from its paper (docs/ui/drawing.md#compositions).
    let set_border = |border: Option<Border>| {
        Action::Apply(Operation::SetSheet { sheet: id, name: sheet.name.clone(), paper, grid: sheet.grid, border })
    };
    field_label(ui, t!("panel.entity_info.border"));
    let mut on = sheet.border.is_some();
    let check = ui.checkbox(&mut on, "");
    crate::kit::handles::publish(|| "drawing_border".into(), check.rect);
    if check.changed() {
        actions.push(set_border(on.then_some(Border { margin: 10.0, width: 0.25 })));
    }
    ui.end_row();
    if let Some(b) = sheet.border {
        if let Some(Value::Length(margin)) = typed_row(ui, t!("panel.entity_info.margin"), (id, 16), &Value::Length(b.margin), units, field)
        {
            actions.push(set_border(Some(Border { margin, ..b })));
        }
        if let Some(Value::Length(width)) =
            typed_row(ui, t!("panel.entity_info.border_width"), (id, 17), &Value::Length(b.width), units, field)
        {
            actions.push(set_border(Some(Border { width, ..b })));
        }
    }
}

/// A row of the entity info holding a value of its kind (a length, an angle), shown and typed in
/// the document's units. The value typed.
fn typed_row(
    ui: &mut egui::Ui,
    label: String,
    key: (Id, u8),
    value: &skraft_model::Value,
    units: &Units,
    width: f32,
) -> Option<skraft_model::Value> {
    field_label(ui, label);
    let f = Field {
        hint: None,
        tip: None,
        width,
        align: egui::Align2::RIGHT_CENTER,
        lit: false,
        addon: None,
        blur_applies: true,
        empty_applies: false,
        handle: Some(format!("drawing_value:{}:{}", key.0.0, key.1)),
    };
    let id = ui.make_persistent_id(("drawing_typed", key));
    // Exact: on paper a line of 0.25 mm is no « ~ 0 », whatever the precision of the model's.
    let exact = value.exact(units);
    let typed = ui.vertical(|ui| framed_field(ui, id, &exact, &exact, f)).inner;
    ui.end_row();
    typed.and_then(|t| skraft_model::Value::typed(&value.kind(), &t, units, Some(value)))
}

/// What a field's value is set on, in the entity info.
pub(super) enum ValuesOn<'a> {
    Document,
    Presentation(&'a skraft_model::Presentation),
    Sheet(&'a skraft_model::Sheet),
    /// An instance on `sheet` under the instances `above`.
    Instance {
        id: Id,
        instance: &'a skraft_model::Instance,
        sheet: Id,
        above: &'a [Id],
    },
}

/// The fields to list: those `names` Skraft does not fill itself, and those `props` has a value
/// for, under the name a text gives them.
fn field_rows(names: std::collections::BTreeSet<String>, props: Option<&skraft_model::Props>) -> Vec<String> {
    let held = props.into_iter().flat_map(|p| p.iter()).map(|(k, _)| k.strip_prefix("user.").unwrap_or(k).to_owned());
    let all: std::collections::BTreeSet<String> = names.into_iter().chain(held).collect();
    crate::drawing::sheet::custom_fields(all)
}

/// The fields `names`, a row each: the value set on `on`, typed over or emptied; while it has
/// none, faint, the one it takes from above (docs/ui/drawing.md#fields).
fn value_rows(ui: &mut egui::Ui, doc: &Document, on: &ValuesOn<'_>, names: Vec<String>, width: f32, actions: &mut Vec<Action>) {
    use skraft_model::{FieldOn, Value, ValueKind, field_key};
    if names.is_empty() {
        return;
    }
    ui.weak(t!("panel.entity_info.values"));
    ui.label("");
    ui.end_row();
    let d = doc.drawing();
    let units = &doc.settings().units;
    let root = doc.component(doc.root()).ok().map(|c| &c.props);
    let owner = match on {
        ValuesOn::Document => doc.root().0,
        ValuesOn::Presentation(p) => p.id.0,
        ValuesOn::Sheet(s) => s.id.0,
        ValuesOn::Instance { id, .. } => id.0,
    };
    let label_width = info_label_width(ui);
    for name in names {
        let key = field_key(&name);
        let in_root = root.and_then(|p| p.get(&key));
        let (own, above): (Option<&Value>, Option<&Value>) = match on {
            ValuesOn::Document => (in_root, None),
            ValuesOn::Presentation(p) => (p.props.get(&key), in_root),
            ValuesOn::Sheet(s) => (s.props.get(&key), d.presentation_of(s.id).and_then(|p| p.props.get(&key)).or(in_root)),
            ValuesOn::Instance { instance, sheet, above, .. } => (instance.props.get(&key), doc.field(*sheet, above, &key)),
        };
        ui.vertical(|ui| {
            let size = egui::vec2(label_width, OPTION_HEIGHT);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(&name).truncate());
            });
        });
        let hint = above.map(|v| v.shown(units));
        let shown = own.map_or_else(String::new, |v| v.shown(units));
        let exact = own.map_or_else(String::new, |v| v.exact(units));
        let f = Field {
            hint: hint.as_deref(),
            tip: None,
            width,
            align: egui::Align2::LEFT_CENTER,
            lit: false,
            addon: None,
            blur_applies: true,
            empty_applies: true,
            handle: Some(format!("drawing_field_value:{name}")),
        };
        let id = ui.make_persistent_id(("drawing_field_value", owner, &key));
        let typed = ui.vertical(|ui| framed_field(ui, id, &shown, &exact, f)).inner;
        ui.end_row();
        let Some(text) = typed else { continue };
        // Of the kind it has or inherits; emptied, it is taken off.
        let value = (!text.is_empty()).then(|| {
            let kind = own.or(above).map_or(ValueKind::Text, Value::kind);
            Value::typed(&kind, &text, units, own).unwrap_or(Value::Text(text))
        });
        actions.push(Action::Apply(match on {
            ValuesOn::Document => Operation::SetProps {
                target: skraft_model::PropTarget::Component(doc.root()),
                props: std::collections::BTreeMap::from([(key, value)]),
            },
            ValuesOn::Presentation(p) => Operation::SetField { on: FieldOn::Presentation(p.id), key, value },
            ValuesOn::Sheet(s) => Operation::SetField { on: FieldOn::Sheet(s.id), key, value },
            ValuesOn::Instance { id, .. } => Operation::SetField { on: FieldOn::Instance(*id), key, value },
        }));
    }
}

/// The entity info of the document in « Mise en plan »: its name, the values of its fields.
fn document_info(ui: &mut egui::Ui, doc: &Document, title: &str, field: f32, actions: &mut Vec<Action>) {
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("panel.entity_info.type_document"));
        ui.end_row();
        ui.label(t!("panel.entity_info.name"));
        ui.label(title);
        ui.end_row();
        let names = crate::drawing::sheet::custom_fields(doc.drawing().all_field_names());
        value_rows(ui, doc, &ValuesOn::Document, names, field, actions);
    });
}

/// The entity info of a presentation: its name, its sheets, where its projected views go, the
/// values of its fields.
fn presentation_info(ui: &mut egui::Ui, doc: &Document, p: &skraft_model::Presentation, field: f32, actions: &mut Vec<Action>) {
    use skraft_model::Projection;
    let d = doc.drawing();
    info_grid(ui).show(ui, |ui| {
        ui.label(t!("panel.entity_info.type"));
        ui.label(t!("panel.entity_info.type_presentation"));
        ui.end_row();
        field_label(ui, t!("panel.entity_info.name"));
        if let Some(name) = text_field(ui, (p.id, 18), &p.name, field) {
            actions.push(Action::Apply(Operation::SetPresentation { presentation: p.id, name, projection: p.projection }));
        }
        ui.end_row();
        ui.label(t!("panel.entity_info.sheets"));
        ui.label(p.sheets.len().to_string());
        ui.end_row();
        field_label(ui, t!("panel.entity_info.projection"));
        let label = |x: Projection| match x {
            Projection::FirstAngle => t!("drawing.projection.first_angle"),
            Projection::ThirdAngle => t!("drawing.projection.third_angle"),
        };
        egui::ComboBox::from_id_salt(("drawing_projection", p.id.0))
            .icon(crate::kit::icons::combo_chevron)
            .width(field)
            .selected_text(label(p.projection))
            .show_ui(ui, |ui| {
                for x in [Projection::FirstAngle, Projection::ThirdAngle] {
                    if ui.selectable_label(p.projection == x, label(x)).clicked() && x != p.projection {
                        actions.push(Action::Apply(Operation::SetPresentation { presentation: p.id, name: p.name.clone(), projection: x }));
                    }
                }
            });
        ui.end_row();
        let used = p.sheets.iter().flat_map(|s| d.field_names(&s.elements)).collect();
        let names = field_rows(used, Some(&p.props));
        value_rows(ui, doc, &ValuesOn::Presentation(p), names, field, actions);
    });
}

/// A text field's name in the « Insérer un champ » menu.
fn field_name(f: &str) -> String {
    match f {
        "document" => t!("drawing.field.document"),
        "date" => t!("drawing.field.date"),
        "sheet" => t!("drawing.field.sheet"),
        "sheet.number" => t!("drawing.field.sheet_number"),
        "sheet.count" => t!("drawing.field.sheet_count"),
        "scale" => t!("drawing.field.scale"),
        other => other.to_owned(),
    }
}

/// A standard direction's name.
pub(crate) fn direction_label(d: skraft_model::Direction) -> String {
    use skraft_model::Direction;
    match d {
        Direction::Front => t!("drawing.direction.front"),
        Direction::Back => t!("drawing.direction.back"),
        Direction::Left => t!("drawing.direction.left"),
        Direction::Right => t!("drawing.direction.right"),
        Direction::Top => t!("drawing.direction.top"),
        Direction::Bottom => t!("drawing.direction.bottom"),
        Direction::Iso => t!("drawing.direction.iso"),
    }
}

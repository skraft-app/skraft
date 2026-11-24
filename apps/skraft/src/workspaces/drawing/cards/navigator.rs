//! The navigator on a drawing's sheet (docs/ui/drawing.md#navigator, decision 0049): the
//! presentations, their sheets and what these hold, the topmost first, an instance unfolding on
//! what its composition holds. Each element right on a sheet is picked, hidden, locked and put
//! over or under the others here; the compositions themselves have their card (`drawing::cards::library`).

use egui::ImageSource;
use skraft_i18n::t;
use skraft_model::{DimKind, Drawing, Element, ElementKind, FastSet, Id, Operation, Orient, Presentation, Sheet, View, ViewKind};
use skraft_tools::Editor;

use crate::commands::Action;
use crate::kit::icons;
use crate::kit::style::Tip;
use crate::kit::widgets::{self, Fold, IconButton};
use crate::workspaces::drawing::sheet::{InfoOn, SheetAsk, SheetState};

/// The side of a row's small buttons.
const BUTTON: f32 = 18.0;
/// The side of a row's icon.
const ICON: f32 = 14.0;

/// One line of the navigator.
#[derive(Debug, PartialEq)]
enum Row<'d> {
    /// The tree's root: the document.
    Document,
    Presentation {
        p: &'d Presentation,
        fold: Fold,
    },
    Sheet {
        s: &'d Sheet,
        fold: Option<Fold>,
    },
    Element(ElementRow<'d>),
    /// « Nouvelle présentation ».
    Add,
}

/// An element in the tree.
#[derive(Debug, PartialEq)]
struct ElementRow<'d> {
    /// The sheet it shows on.
    sheet: Id,
    el: &'d Element,
    /// The instances leading to it from the sheet, outermost first.
    path: Vec<Id>,
    /// Its place in its container (bottom first), and how many it holds.
    index: usize,
    count: usize,
    /// An instance above it is hidden.
    hidden_above: bool,
    fold: Option<Fold>,
}

fn key(kind: &str, id: Id) -> egui::Id {
    egui::Id::unique(("drawing_navigator", kind, id.0))
}

/// The fold of instance `id` under the instances `path`: one composition placed twice folds apart.
fn instance_key(sheet: Id, path: &[Id], id: Id) -> egui::Id {
    egui::Id::unique(("drawing_navigator", "instance", sheet.0, path, id.0))
}

fn fold_of(open: &FastSet<egui::Id>, key: egui::Id) -> Fold {
    Fold { key, open: open.contains(&key) }
}

/// What is dragged in the navigator: elements right on sheet `sheet`.
#[derive(Clone)]
struct SheetDrag {
    sheet: Id,
    elements: Vec<Id>,
}

/// A sheet dragged by its row, to put it elsewhere in the order (docs/ui/drawing.md#navigator).
#[derive(Clone)]
struct SheetOrderDrag {
    sheet: Id,
}

/// What puts sheet `moved`, or a copy of it (`copy`), in presentation `p`: beside its sheet `at`
/// (after it or before it), or at its end; none when that changes nothing.
fn sheet_drop(d: &Drawing, moved: Id, p: &Presentation, at: Option<(Id, bool)>, copy: bool) -> Option<Operation> {
    if copy {
        d.sheet(moved)?;
        let index = match at {
            Some((target, after)) => p.sheets.iter().position(|s| s.id == target)? + usize::from(after),
            None => p.sheets.len(),
        };
        return Some(Operation::CopySheet { sheet: moved, presentation: p.id, index });
    }
    let others: Vec<Id> = p.sheets.iter().map(|s| s.id).filter(|s| *s != moved).collect();
    let index = match at {
        Some((target, after)) => others.iter().position(|s| *s == target)? + usize::from(after),
        None => others.len(),
    };
    let same_place = p.sheets.iter().position(|s| s.id == moved) == Some(index);
    (!same_place && d.sheet(moved).is_some()).then_some(Operation::MoveSheet { sheet: moved, presentation: Some(p.id), index })
}

/// While a sheet is dragged over row `response`: where it would go marked (`line`: a line above
/// it or under it, none: the row framed), the cursor; let go, `op`, a copy made active by the
/// stage. No `op`: it may not go there.
fn sheet_drop_on_row(
    ui: &egui::Ui,
    response: &egui::Response,
    st: &mut SheetState,
    line: Option<bool>,
    op: Option<Operation>,
    actions: &mut Vec<Action>,
) {
    let Some(op) = op else {
        ui.ctx().set_cursor_icon(egui::CursorIcon::NoDrop);
        return;
    };
    let stroke = egui::Stroke::new(1.5, ui.visuals().selection.stroke.color);
    let r = response.rect;
    match line {
        Some(above) => {
            let y = if above { r.top() } else { r.bottom() };
            ui.painter().hline(r.left() + ui.spacing().indent..=r.right(), y, stroke);
        }
        None => {
            ui.painter().rect_stroke(r.shrink(1.0), 3.0, stroke, egui::StrokeKind::Inside);
        }
    }
    let copying = matches!(op, Operation::CopySheet { .. });
    ui.ctx().set_cursor_icon(if copying { egui::CursorIcon::Copy } else { egui::CursorIcon::Grabbing });
    if response.dnd_release_payload::<SheetOrderDrag>().is_some() {
        match op {
            Operation::CopySheet { sheet, presentation, index } => st.asked = Some(SheetAsk::CopySheet { sheet, presentation, index }),
            op => actions.push(Action::Apply(op)),
        }
    }
}

/// The rows of drawing `d`, as far as they are unfolded.
fn rows<'d>(d: &'d Drawing, open: &FastSet<egui::Id>) -> Vec<Row<'d>> {
    let mut out = vec![Row::Document];
    for p in &d.presentations {
        let fold = fold_of(open, key("presentation", p.id));
        out.push(Row::Presentation { p, fold });
        if !fold.open {
            continue;
        }
        for s in &p.sheets {
            let fold = (!s.elements.is_empty()).then(|| fold_of(open, key("sheet", s.id)));
            out.push(Row::Sheet { s, fold });
            if fold.is_some_and(|f| f.open) {
                elements(d, open, s.id, &s.elements, &mut Vec::new(), false, &mut out);
            }
        }
    }
    out.push(Row::Add);
    out
}

/// The rows of `list`, the topmost first, through the instances unfolded (`path`: those above).
#[allow(clippy::too_many_arguments)]
fn elements<'d>(
    d: &'d Drawing,
    open: &FastSet<egui::Id>,
    sheet: Id,
    list: &'d [Element],
    path: &mut Vec<Id>,
    hidden_above: bool,
    out: &mut Vec<Row<'d>>,
) {
    for (index, el) in list.iter().enumerate().rev() {
        let composition = el.kind.instance().and_then(|i| d.composition(i.composition)).filter(|b| !b.elements.is_empty());
        // By the instances leading to it: one composition placed twice folds apart.
        let fold = composition.map(|_| fold_of(open, instance_key(sheet, path, el.id)));
        out.push(Row::Element(ElementRow { sheet, el, path: path.clone(), index, count: list.len(), hidden_above, fold }));
        if let (Some(b), Some(f)) = (composition, fold)
            && f.open
            && path.len() < 32
        {
            path.push(el.id);
            elements(d, open, sheet, &b.elements, path, hidden_above || el.hidden, out);
            path.pop();
        }
    }
}

/// An element's icon.
fn icon_of(el: &Element) -> ImageSource<'static> {
    match &el.kind {
        ElementKind::View(View { kind: ViewKind::Projected { .. }, .. }) => icons::VIEW_PROJECT,
        ElementKind::View(_) => icons::VIEWS,
        ElementKind::Text { .. } => icons::SHEET_TEXT,
        ElementKind::Line { .. } => icons::SHEET_LINE,
        ElementKind::Rect { .. } => icons::SHEET_RECT,
        ElementKind::Circle { .. } => icons::SHEET_CIRCLE,
        ElementKind::Image { .. } => icons::TEXTURE,
        ElementKind::Dimension(dim) => match dim.kind {
            DimKind::Linear { .. } => icons::DIM_LINEAR,
            DimKind::Radius { diameter: false, .. } => icons::DIM_RADIUS,
            DimKind::Radius { .. } => icons::DIM_DIAMETER,
            DimKind::Angle { .. } => icons::DIM_ANGLE,
        },
        ElementKind::Instance(_) => icons::COMPOSITION,
    }
}

/// What an element is called in the tree: a view's or a composition's name, a text's first line, else
/// its kind.
fn name_of(d: &Drawing, el: &Element) -> String {
    match &el.kind {
        ElementKind::View(v) => v.name.clone(),
        ElementKind::Text { text, .. } => text.lines().next().unwrap_or_default().to_owned(),
        ElementKind::Line { .. } => t!("panel.entity_info.type_line"),
        ElementKind::Rect { .. } => t!("panel.entity_info.type_rect"),
        ElementKind::Circle { .. } => t!("panel.entity_info.type_circle"),
        ElementKind::Image { .. } => t!("panel.entity_info.type_image"),
        ElementKind::Dimension(dim) => match dim.kind {
            DimKind::Linear { orient: Orient::Horizontal, .. } => t!("drawing.dim.horizontal"),
            DimKind::Linear { orient: Orient::Vertical, .. } => t!("drawing.dim.vertical"),
            DimKind::Linear { .. } => t!("drawing.dim.aligned"),
            DimKind::Radius { diameter: false, .. } => t!("drawing.dim.radius"),
            DimKind::Radius { .. } => t!("drawing.dim.diameter"),
            DimKind::Angle { .. } => t!("drawing.dim.angle"),
        },
        ElementKind::Instance(i) => d.composition(i.composition).map_or_else(String::new, |b| b.name.clone()),
    }
}

/// A row's small button, frameless.
fn small_button(ui: &mut egui::Ui, icon: ImageSource<'static>, tint: egui::Color32, enabled: bool, tip: String) -> egui::Response {
    IconButton::new(icon, BUTTON, ICON).ink(tint).enabled(enabled).show(ui).tip(tip)
}

/// What a line shows.
struct Line<'a> {
    depth: usize,
    fold: Option<Fold>,
    icon: ImageSource<'static>,
    name: &'a str,
    strong: bool,
    tint: egui::Color32,
    selected: bool,
    /// Its name edited under this key, when it can be.
    rename: Option<egui::Id>,
    /// F2 renames it: it is the one thing picked.
    alone: bool,
}

/// A line of the tree: its indent, fold, icon and name, what `right` adds at its right end (the
/// pointer over the line or not). Gives its response, the name typed, what `right` gave.
fn line<R>(
    ui: &mut egui::Ui,
    height: f32,
    l: Line<'_>,
    (salt, folds): (egui::Id, &mut FastSet<egui::Id>),
    right: impl FnOnce(&mut egui::Ui, bool) -> R,
) -> (egui::Response, Option<Option<String>>, R) {
    let renaming = l.rename.is_some_and(|k| ui.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(widgets::RENAMING))) == Some(k));
    let icon = egui::Image::new(l.icon).fit_to_exact_size(egui::vec2(ICON, ICON)).tint(l.tint);
    let line = widgets::TreeLine { height, selected: l.selected, lit: false, band: false, depth: l.depth, fold: l.fold, icon };
    // Its buttons close together.
    let buttons = |ui: &mut egui::Ui, over: bool| {
        let spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;
        let extra = right(ui, over);
        ui.spacing_mut().item_spacing.x = spacing;
        extra
    };
    let label = |ui: &mut egui::Ui| {
        if renaming {
            return widgets::rename_field(ui, l.rename.expect("renaming"), l.name);
        }
        let mut text = egui::RichText::new(l.name).color(l.tint);
        if l.strong {
            text = text.strong();
        }
        ui.add(egui::Label::new(text).truncate().selectable(false));
        None
    };
    let (response, (extra, renamed)) = widgets::tree_line(ui, line, (salt, folds), buttons, label);
    let renamed = match l.rename {
        Some(k) => widgets::start_rename(ui, &response, renaming, l.alone, k, renamed),
        None => None,
    };
    (response, renamed, extra)
}

/// The navigator on a sheet, in `area`.
pub fn show(ui: &mut egui::Ui, editor: &Editor, st: &mut SheetState, title: &str, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let d = editor.document().drawing();
    // Another sheet shown: its presentation and itself unfolded.
    let shown = st.shown(d).map(|s| s.id);
    let active = st.presentation(d).map(|p| p.id);
    if st.tree_shown != shown {
        st.tree_shown = shown;
        if let Some(p) = active {
            st.tree_open.insert(key("presentation", p));
        }
        if let Some(s) = shown {
            st.tree_open.insert(key("sheet", s));
        }
    }
    // The instances entered unfolded, what is edited in view.
    if let Some(s) = shown {
        for k in 0..st.open.len() {
            let fold = instance_key(s, &st.open[..k], st.open[k]);
            st.tree_open.insert(fold);
        }
    }
    let height = ui.spacing().interact_size.y;
    // Room for the first and last rows' backgrounds, as the 3D navigator's.
    let mut margin = ui.spacing().scroll.content_margin;
    let gap = (ui.spacing().item_spacing.y / 2.0).ceil() as i8;
    margin.top = margin.top.max(gap);
    margin.bottom = margin.bottom.max(gap);
    let area = area.content_margin(margin);
    let salt = ui.make_persistent_id("drawing_navigator_folds");
    let mut lines = rows(d, &st.tree_open);
    let n = lines.len();
    let mut lines: Vec<Option<Row<'_>>> = lines.drain(..).map(Some).collect();
    area.show_rows(ui, height, n, |ui, range| {
        ui.set_min_width(ui.available_width());
        for k in range {
            let Some(row) = lines[k].take() else { continue };
            match row {
                Row::Document => document_row(ui, height, st, title),
                Row::Presentation { p, fold } => presentation_row(ui, height, d, st, p, fold, active, salt, actions),
                Row::Sheet { s, fold } => sheet_row(ui, height, d, st, s, fold, shown, salt, actions),
                Row::Element(row) => element_row(ui, height, d, st, row, salt, actions),
                Row::Add => {
                    let tint = ui.visuals().weak_text_color();
                    let name = t!("menu.drawing.new_presentation");
                    let l = Line {
                        depth: 0,
                        fold: None,
                        icon: icons::ADD,
                        name: &name,
                        strong: false,
                        tint,
                        selected: false,
                        rename: None,
                        alone: false,
                    };
                    let (response, _, ()) = line(ui, height, l, (salt, &mut st.tree_open), |_, _| ());
                    crate::kit::handles::publish(|| "navigator:add".into(), response.rect);
                    presentation_menu(&response, st, actions);
                }
            }
        }
    });
    widgets::paint_drag_ghost(ui.ctx());
    // The stage frames what a row asked for at its next frame.
    if st.framing_asked() {
        ui.ctx().request_repaint();
    }
}

/// The root, at the list's left edge as the 3D navigator's model: a click shows the document's
/// values in the entity info.
fn document_row(ui: &mut egui::Ui, height: f32, st: &mut SheetState, title: &str) {
    let selected = st.info == InfoOn::Document && st.selected.is_empty() && st.open.is_empty();
    let tint = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
    let (response, ()) = widgets::tree_row(ui, height, selected, false, false, |ui| {
        ui.add(egui::Image::new(icons::MODEL).fit_to_exact_size(egui::vec2(ICON, ICON)).tint(tint));
        let font = crate::kit::style::semibold(egui::TextStyle::Body.resolve(ui.style()).size);
        ui.add(egui::Label::new(egui::RichText::new(title).font(font).color(tint)).selectable(false).truncate());
    });
    crate::kit::handles::publish(|| "navigator:document".into(), response.rect);
    if response.clicked() {
        st.open.clear();
        st.selected.clear();
        st.info = InfoOn::Document;
    }
}

/// The menu of « Nouvelle présentation »: a blank one, or one from each template.
fn presentation_menu(button: &egui::Response, st: &mut SheetState, actions: &mut Vec<Action>) {
    egui::Popup::menu(button).style(crate::kit::style::menu).show(|ui| {
        let blank = ui.add(crate::kit::style::menu_item(t!("drawing.blank_presentation"), None, false));
        crate::kit::handles::publish(|| "navigator:menu:new_presentation".into(), blank.rect);
        if blank.clicked() {
            actions.push(Action::Command("drawing.new_presentation"));
            ui.close();
        }
        ui.separator();
        for (k, tpl) in crate::workspaces::drawing::templates::all().into_iter().enumerate() {
            let item = ui.add(crate::kit::style::menu_item(&tpl.name, None, false));
            crate::kit::handles::publish(|| format!("navigator:menu:presentation_template:{k}"), item.rect);
            if item.clicked() {
                st.asked = Some(SheetAsk::PresentationFrom(tpl.document));
                ui.close();
            }
        }
    });
}

/// The menu of a presentation's « + »: a blank sheet, or one from a template, a template of
/// several offering each (docs/ui/drawing.md#templates).
fn sheet_menu(button: &egui::Response, st: &mut SheetState, presentation: Id) {
    egui::Popup::menu(button).style(crate::kit::style::menu).show(|ui| {
        let blank = ui.add(crate::kit::style::menu_item(t!("drawing.blank_sheet"), None, false));
        crate::kit::handles::publish(|| "navigator:menu:blank_sheet".into(), blank.rect);
        if blank.clicked() {
            st.asked = Some(SheetAsk::NewSheet(presentation));
            ui.close();
        }
        ui.separator();
        for (k, tpl) in crate::workspaces::drawing::templates::all().into_iter().enumerate() {
            let of: Vec<(Id, String)> = tpl.document.drawing().sheets().map(|s| (s.id, s.name.clone())).collect();
            if of.len() > 1 {
                let sub = ui.menu_button(&tpl.name, |ui| {
                    for (j, (sheet, name)) in of.iter().enumerate() {
                        let item = ui.add(crate::kit::style::menu_item(name, None, false));
                        crate::kit::handles::publish(|| format!("navigator:menu:sheet_template:{k}:{j}"), item.rect);
                        if item.clicked() {
                            st.asked = Some(SheetAsk::SheetFrom {
                                template: tpl.document.clone(),
                                sheet: Some(*sheet),
                                name: Some(name.clone()),
                                presentation,
                            });
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    }
                });
                crate::kit::handles::publish(|| format!("navigator:menu:sheet_template:{k}"), sub.response.rect);
            } else {
                let item = ui.add(crate::kit::style::menu_item(&tpl.name, None, false));
                crate::kit::handles::publish(|| format!("navigator:menu:sheet_template:{k}"), item.rect);
                if item.clicked() {
                    st.asked = Some(SheetAsk::SheetFrom { template: tpl.document.clone(), sheet: None, name: None, presentation });
                    ui.close();
                }
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn presentation_row(
    ui: &mut egui::Ui,
    height: f32,
    d: &Drawing,
    st: &mut SheetState,
    p: &Presentation,
    fold: Fold,
    active: Option<Id>,
    salt: egui::Id,
    actions: &mut Vec<Action>,
) {
    let on = active == Some(p.id);
    let tint = if on { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    let rename = ui.make_persistent_id(("drawing_navigator_rename", "presentation", p.id.0));
    let selected = on && st.info == InfoOn::Presentation && st.selected.is_empty() && st.open.is_empty();
    let tint = if selected { ui.visuals().selection.stroke.color } else { tint };
    let l = Line {
        depth: 0,
        fold: Some(fold),
        icon: icons::DRAWING,
        name: &p.name,
        strong: on,
        tint,
        selected,
        rename: Some(rename),
        alone: false,
    };
    let weak = ui.visuals().weak_text_color();
    let (response, renamed, add) =
        line(ui, height, l, (salt, &mut st.tree_open), |ui, _| small_button(ui, icons::ADD, weak, true, t!("menu.drawing.new_sheet")));
    crate::kit::handles::publish(|| format!("navigator:add_sheet:{}", p.id.0), add.rect);
    sheet_menu(&add, st, p.id);
    crate::kit::handles::publish(|| format!("navigator:presentation:{}", p.id.0), response.rect);
    if let Some(Some(name)) = renamed
        && !name.is_empty()
    {
        actions.push(Action::Apply(Operation::SetPresentation { presentation: p.id, name, projection: p.projection }));
    }
    // A sheet dragged onto it goes to its end.
    if let Some(drag) = response.dnd_hover_payload::<SheetOrderDrag>() {
        let copy = crate::workspaces::common::viewport::modifiers(ui.ctx(), ui.input(|i| i.modifiers)).copy;
        sheet_drop_on_row(ui, &response, st, None, sheet_drop(d, drag.sheet, p, None, copy), actions);
    }
    if response.clicked() {
        st.show_presentation(d, p.id);
        st.frame_presentation(d, p.id);
        st.open.clear();
        st.selected.clear();
        st.info = InfoOn::Presentation;
    }
    response.context_menu(|ui| {
        let save = ui.button(t!("menu.drawing.save_template"));
        crate::kit::handles::publish(|| "navigator:menu:save_template".into(), save.rect);
        if save.clicked() {
            st.show_presentation(d, p.id);
            actions.push(Action::Command("drawing.save_template"));
            ui.close();
        }
        if ui.button(t!("drawing.tree.delete_presentation")).clicked() {
            actions.push(Action::Apply(Operation::DeletePresentation { presentation: p.id }));
            ui.close();
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn sheet_row(
    ui: &mut egui::Ui,
    height: f32,
    d: &Drawing,
    st: &mut SheetState,
    s: &Sheet,
    fold: Option<Fold>,
    shown: Option<Id>,
    salt: egui::Id,
    actions: &mut Vec<Action>,
) {
    // Lit while it is what the entity info shows: nothing picked on it.
    let selected = shown == Some(s.id) && st.info == InfoOn::Sheet && st.selected.is_empty() && st.open.is_empty();
    let tint = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
    let rename = ui.make_persistent_id(("drawing_navigator_rename", "sheet", s.id.0));
    let l = Line { depth: 1, fold, icon: icons::SHEET, name: &s.name, strong: false, tint, selected, rename: Some(rename), alone: false };
    let (response, renamed, ()) = line(ui, height, l, (salt, &mut st.tree_open), |_, _| ());
    crate::kit::handles::publish(|| format!("navigator:sheet:{}", s.id.0), response.rect);
    // Dragged by its row, it goes elsewhere in the order.
    let response = response.interact(egui::Sense::click_and_drag());
    if response.drag_started() {
        widgets::set_drag_ghost(ui.ctx(), icons::SHEET, s.name.clone(), 1);
    }
    response.dnd_set_drag_payload(SheetOrderDrag { sheet: s.id });
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    // Another sheet over it: before it, or after it by its lower half; unfolded, what follows
    // it is its elements, so only before it (the presentation's row takes it to the end).
    if let Some(drag) = response.dnd_hover_payload::<SheetOrderDrag>() {
        let lower = ui.ctx().pointer_interact_pos().is_some_and(|p| p.y > response.rect.center().y);
        let after = lower && !fold.is_some_and(|f| f.open);
        let copy = crate::workspaces::common::viewport::modifiers(ui.ctx(), ui.input(|i| i.modifiers)).copy;
        let op = d.presentation_of(s.id).and_then(|p| sheet_drop(d, drag.sheet, p, Some((s.id, after)), copy));
        sheet_drop_on_row(ui, &response, st, Some(!after), op, actions);
    }
    // Elements dragged from the rows below: moved onto it, or copied with the copy modifier
    // (docs/ui/drawing.md#navigator).
    if let Some(drag) = response.dnd_hover_payload::<SheetDrag>() {
        let copy = crate::workspaces::common::viewport::modifiers(ui.ctx(), ui.input(|i| i.modifiers)).copy;
        if drag.sheet != s.id || copy {
            let stroke = egui::Stroke::new(1.5, ui.visuals().selection.stroke.color);
            ui.painter().rect_stroke(response.rect.shrink(1.0), 3.0, stroke, egui::StrokeKind::Inside);
            ui.ctx().set_cursor_icon(if copy { egui::CursorIcon::Copy } else { egui::CursorIcon::Grabbing });
            if let Some(drag) = response.dnd_release_payload::<SheetDrag>() {
                st.asked = Some(SheetAsk::Drop { elements: drag.elements.clone(), to: s.id, copy });
            }
        } else {
            ui.ctx().set_cursor_icon(egui::CursorIcon::NoDrop);
        }
    }
    if let Some(Some(name)) = renamed
        && !name.is_empty()
    {
        actions.push(Action::Apply(Operation::SetSheet { sheet: s.id, name, paper: s.paper, grid: s.grid, border: s.border }));
    }
    if response.clicked() {
        st.show_sheet(s.id);
        st.frame_sheet(s.id);
        st.open.clear();
        st.selected.clear();
        st.info = InfoOn::Sheet;
    }
    response.context_menu(|ui| {
        if ui.button(t!("drawing.delete_sheet")).clicked() {
            actions.push(Action::Apply(Operation::DeleteSheet { sheet: s.id }));
            ui.close();
        }
    });
}

/// A click on the row of element `id` under the instances `path`: its sheet shown, the levels
/// open kept as far as they lead to it, what stands for it in the last one picked (Shift adds or
/// takes it off) unless it is hidden or locked.
fn pick(st: &mut SheetState, d: &Drawing, sheet: Id, path: &[Id], id: Id, shift: bool) {
    st.info = InfoOn::Sheet;
    // The camera goes to the sheet, never to the element (docs/ui/drawing.md#framing).
    if st.shown(d).is_some_and(|s| s.id == sheet) {
        st.reveal_sheet(sheet);
    } else {
        st.frame_sheet(sheet);
    }
    st.show_sheet(sheet);
    st.open_to(path);
    let top = path.get(st.open.len()).copied().unwrap_or(id);
    if d.element(top).is_none_or(|el| el.hidden || el.locked) {
        return;
    }
    if shift {
        match st.selected.iter().position(|x| *x == top) {
            Some(i) => {
                st.selected.remove(i);
            }
            None => st.selected.push(top),
        }
    } else {
        st.selected = vec![top];
    }
}

fn element_row(
    ui: &mut egui::Ui,
    height: f32,
    d: &Drawing,
    st: &mut SheetState,
    row: ElementRow<'_>,
    salt: egui::Id,
    actions: &mut Vec<Action>,
) {
    let el = row.el;
    // Right in the level open: picked, hidden, locked and ordered here.
    let editable = row.path == st.open;
    let entered = st.open.starts_with(&row.path) && st.open.get(row.path.len()) == Some(&el.id);
    let selected = editable && st.selected.contains(&el.id);
    let v = ui.visuals();
    let (on, weak) = (v.selection.stroke.color, v.weak_text_color());
    let tint = if selected || entered {
        on
    } else if el.hidden || row.hidden_above {
        widgets::hidden_color(v, false)
    } else if editable {
        v.text_color()
    } else {
        weak
    };
    let name = name_of(d, el);
    // A view's name, or the name of what an instance places.
    let renames = editable && matches!(el.kind, ElementKind::View(_) | ElementKind::Instance(_));
    let rename = renames.then(|| ui.make_persistent_id(("drawing_navigator_rename", "element", el.id.0)));
    let l = Line {
        depth: 2 + row.path.len(),
        fold: row.fold,
        icon: icon_of(el),
        name: &name,
        strong: entered,
        tint,
        selected,
        rename,
        alone: selected && st.selected.len() == 1,
    };
    let button = if selected { on } else { weak };
    let (response, renamed, buttons) = line(ui, height, l, (salt, &mut st.tree_open), |ui, over| {
        // What is in a composition is changed with the composition entered (docs/ui/drawing.md#compositions).
        if !editable {
            return None;
        }
        let (icon, tip) = if el.hidden { (icons::EYE_OFF, t!("panel.navigator.show")) } else { (icons::EYE, t!("panel.navigator.hide")) };
        let eye = small_button(ui, icon, button, true, tip);
        crate::kit::handles::publish(|| format!("navigator:eye:{}", el.id.0), eye.rect);
        // The padlock while locked, or under the pointer; its room kept, the names aligned.
        let lock = if el.locked || over {
            let (icon, tip) = if el.locked { (icons::LOCK, t!("drawing.tree.unlock")) } else { (icons::UNLOCK, t!("drawing.tree.lock")) };
            let r = small_button(ui, icon, button, true, tip);
            crate::kit::handles::publish(|| format!("navigator:lock:{}", el.id.0), r.rect);
            r.clicked()
        } else {
            ui.allocate_space(egui::vec2(BUTTON, BUTTON));
            false
        };
        let (mut up, mut down) = (false, false);
        if over {
            // Listed topmost first: « up » is over the next one in the container's order.
            let r = small_button(ui, icons::ARROW_DOWN, button, row.index > 0, t!("drawing.tree.down"));
            crate::kit::handles::publish(|| format!("navigator:down:{}", el.id.0), r.rect);
            down = r.clicked();
            let r = small_button(ui, icons::ARROW_UP, button, row.index + 1 < row.count, t!("drawing.tree.up"));
            crate::kit::handles::publish(|| format!("navigator:up:{}", el.id.0), r.rect);
            up = r.clicked();
        }
        Some((eye.clicked(), lock, up, down))
    });
    crate::kit::handles::publish(|| format!("navigator:element:{}", el.id.0), response.rect);
    // Right on a sheet and free: dragged onto another sheet, with what is picked when it is.
    let response = if row.path.is_empty() && !el.locked {
        let response = response.interact(egui::Sense::click_and_drag());
        let elements = if selected { st.selected.clone() } else { vec![el.id] };
        if response.drag_started() {
            widgets::set_drag_ghost(ui.ctx(), icon_of(el), name.clone(), elements.len());
        }
        response.dnd_set_drag_payload(SheetDrag { sheet: row.sheet, elements });
        response
    } else {
        response
    };
    if let Some(Some(name)) = renamed
        && !name.is_empty()
    {
        match &el.kind {
            ElementKind::View(v) => {
                actions.push(Action::Apply(Operation::SetElement { element: el.id, kind: ElementKind::View(View { name, ..v.clone() }) }));
            }
            ElementKind::Instance(i) => actions.push(Action::Apply(Operation::RenameComposition { composition: i.composition, name })),
            _ => {}
        }
    }
    match buttons {
        Some((true, ..)) => {
            actions.push(Action::Apply(Operation::SetElementState { elements: vec![el.id], hidden: Some(!el.hidden), locked: None }));
        }
        Some((_, true, ..)) => {
            actions.push(Action::Apply(Operation::SetElementState { elements: vec![el.id], hidden: None, locked: Some(!el.locked) }));
        }
        Some((_, _, true, _)) => actions.push(Action::Apply(Operation::MoveElement { element: el.id, index: row.index + 1 })),
        Some((_, _, _, true)) => actions.push(Action::Apply(Operation::MoveElement { element: el.id, index: row.index - 1 })),
        _ => {
            if response.clicked() {
                pick(st, d, row.sheet, &row.path, el.id, ui.input(|i| i.modifiers.shift));
            }
        }
    }
    // An instance's menu acts on it alone.
    if editable && el.kind.instance().is_some() && !el.locked && !el.hidden {
        response.context_menu(|ui| {
            for (label, command) in [
                (t!("menu.drawing.edit_composition"), "drawing.edit_composition"),
                (t!("menu.drawing.explode"), "drawing.explode"),
                (t!("menu.drawing.make_unique"), "drawing.make_unique"),
            ] {
                let item = ui.button(label);
                crate::kit::handles::publish(|| format!("navigator:menu:{command}"), item.rect);
                if item.clicked() {
                    st.show_sheet(row.sheet);
                    st.selected = vec![el.id];
                    actions.push(Action::Command(command));
                    ui.close();
                }
            }
            // One more instance of its composition, a little aside.
            let copy = ui.button(t!("drawing.tree.copy"));
            crate::kit::handles::publish(|| "navigator:menu:copy".into(), copy.rect);
            if copy.clicked() {
                st.show_sheet(row.sheet);
                st.place_composition = el.kind.instance().map(|i| i.composition);
                ui.close();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec2;
    use skraft_model::{Container, Document, History, Paper};

    use super::*;

    fn add(h: &mut History, to: Container, element: ElementKind) -> Id {
        h.apply(Operation::AddElement { to, element }).unwrap().element.unwrap()
    }

    fn line_kind() -> ElementKind {
        ElementKind::Line { from: DVec2::ZERO, to: DVec2::X }
    }

    /// One sheet: a rectangle under a named composition's instance (a line inside).
    fn drawing() -> (History, Id, Id, Id, Id) {
        let mut h = History::new(Document::new());
        let s = h.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().sheet.unwrap();
        let to = Container::Sheet(s);
        let inner = add(&mut h, to, line_kind());
        let i = h.apply(Operation::MakeComposition { elements: vec![inner], name: Some("Cartouche".into()) }).unwrap().element.unwrap();
        let r = add(&mut h, to, ElementKind::Rect { min: DVec2::ZERO, max: DVec2::ONE });
        (h, s, i, r, inner)
    }

    fn ids(rows: &[Row<'_>]) -> Vec<Id> {
        rows.iter()
            .filter_map(|r| match r {
                Row::Element(e) => Some(e.el.id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_tree_lists_the_topmost_first_as_far_as_it_is_unfolded() {
        let (h, s, i, r, inner) = drawing();
        let d = h.document().drawing();
        let p = d.presentations[0].id;
        // All folded: the document, the presentation, « new ».
        let mut open = FastSet::default();
        assert_eq!(rows(d, &open).len(), 3);
        assert_eq!(rows(d, &open)[0], Row::Document);
        open.insert(key("presentation", p));
        assert_eq!(rows(d, &open).len(), 4, "its sheet");
        open.insert(key("sheet", s));
        assert_eq!(ids(&rows(d, &open)), vec![r, i], "the last drawn first");
        // The instance unfolded: what its composition holds under it, picking the instance.
        let fold = rows(d, &open)
            .iter()
            .find_map(|x| match x {
                Row::Element(e) if e.el.id == i => e.fold,
                _ => None,
            })
            .unwrap();
        open.insert(fold.key);
        let all = rows(d, &open);
        assert_eq!(ids(&all), vec![r, i, inner]);
        let Some(Row::Element(child)) = all.iter().find(|x| matches!(x, Row::Element(e) if e.el.id == inner)) else { panic!() };
        assert_eq!(child.path, vec![i]);
    }

    #[test]
    fn a_click_picks_what_is_right_on_the_sheet_unless_locked() {
        let (mut h, s, i, r, _) = drawing();
        let mut st = SheetState::default();
        pick(&mut st, h.document().drawing(), s, &[], i, false);
        assert_eq!(st.selected, vec![i]);
        pick(&mut st, h.document().drawing(), s, &[], r, true);
        assert_eq!(st.selected, vec![i, r]);
        pick(&mut st, h.document().drawing(), s, &[], i, true);
        assert_eq!(st.selected, vec![r]);
        h.apply(Operation::SetElementState { elements: vec![i], hidden: None, locked: Some(true) }).unwrap();
        pick(&mut st, h.document().drawing(), s, &[], i, false);
        assert_eq!(st.selected, vec![r], "locked: not picked");
    }

    #[test]
    fn a_click_keeps_the_level_open_as_far_as_it_leads_to_the_row() {
        let (h, s, i, r, inner) = drawing();
        let d = h.document().drawing();
        let mut st = SheetState::default();
        pick(&mut st, d, s, &[i], inner, false);
        assert_eq!((st.open.as_slice(), st.selected.as_slice()), (&[][..], &[i][..]), "closed: what stands for it on the sheet");
        st.enter(i);
        pick(&mut st, d, s, &[i], inner, false);
        assert_eq!((st.open.as_slice(), st.selected.as_slice()), (&[i][..], &[inner][..]), "entered: itself");
        pick(&mut st, d, s, &[], r, false);
        assert_eq!((st.open.as_slice(), st.selected.as_slice()), (&[][..], &[r][..]), "elsewhere: the composition left");
    }

    #[test]
    fn a_sheet_dropped_goes_beside_a_sheet_or_to_a_presentations_end() {
        let mut h = History::new(Document::new());
        let o = h.apply(Operation::AddPresentation { name: None, paper: Some(Paper::A3) }).unwrap();
        let (p, a) = (o.presentation.unwrap(), o.sheet.unwrap());
        let mut add_sheet = |presentation| {
            h.apply(Operation::AddSheet { presentation: Some(presentation), name: None, paper: Paper::A4 }).unwrap().sheet.unwrap()
        };
        let (b, c) = (add_sheet(p), add_sheet(p));
        let q = h.apply(Operation::AddPresentation { name: None, paper: None }).unwrap().presentation.unwrap();
        let order =
            |h: &History, id: Id| -> Vec<Id> { h.document().drawing().presentation(id).unwrap().sheets.iter().map(|s| s.id).collect() };
        let drop = |h: &History, moved, to, at| {
            let d = h.document().drawing();
            sheet_drop(d, moved, d.presentation(to).unwrap(), at, false)
        };
        // Where it already is, or onto itself: nothing.
        assert_eq!(drop(&h, a, p, Some((b, false))), None);
        assert_eq!(drop(&h, c, p, Some((b, true))), None);
        assert_eq!(drop(&h, c, p, None), None);
        assert_eq!(drop(&h, b, p, Some((b, true))), None);
        // Down past the next one, then up to the top.
        h.apply(drop(&h, a, p, Some((b, true))).unwrap()).unwrap();
        assert_eq!(order(&h, p), vec![b, a, c]);
        h.apply(drop(&h, c, p, Some((b, false))).unwrap()).unwrap();
        assert_eq!(order(&h, p), vec![c, b, a]);
        // To the end of its presentation, then into another, empty.
        h.apply(drop(&h, c, p, None).unwrap()).unwrap();
        assert_eq!(order(&h, p), vec![b, a, c]);
        h.apply(drop(&h, a, q, None).unwrap()).unwrap();
        assert_eq!((order(&h, p), order(&h, q)), (vec![b, c], vec![a]));
        h.undo();
        assert_eq!(order(&h, p), vec![b, a, c], "one step");
        // With the copy key: anywhere, beside itself too, the index in the list as it is.
        let d = h.document().drawing();
        let copied = |at, to: Id| sheet_drop(d, a, d.presentation(to).unwrap(), at, true);
        assert_eq!(copied(Some((a, true)), p), Some(Operation::CopySheet { sheet: a, presentation: p, index: 2 }));
        assert_eq!(copied(Some((b, false)), p), Some(Operation::CopySheet { sheet: a, presentation: p, index: 0 }));
        assert_eq!(copied(None, q), Some(Operation::CopySheet { sheet: a, presentation: q, index: 0 }));
    }

    #[test]
    fn the_presentation_shown_keeps_its_place_while_empty() {
        let mut h = History::new(Document::new());
        let first = h.apply(Operation::AddPresentation { name: Some("Atelier".into()), paper: Some(Paper::A3) }).unwrap();
        let (a, sheet) = (first.presentation.unwrap(), first.sheet.unwrap());
        let b = h.apply(Operation::AddPresentation { name: Some("Client".into()), paper: None }).unwrap().presentation.unwrap();
        let mut st = SheetState::default();
        let d = h.document().drawing();
        assert_eq!(st.presentation(d).unwrap().id, a);
        assert_eq!(st.shown(d).unwrap().id, sheet);
        st.show_presentation(d, b);
        assert_eq!(st.presentation(d).unwrap().id, b, "empty, and shown");
        assert!(st.shown(d).is_none(), "no sheet of another presentation in its place");
        st.show_presentation(d, a);
        assert_eq!(st.shown(d).unwrap().id, sheet);
    }
}

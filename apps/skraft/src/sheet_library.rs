//! The « Compositions » card (docs/ui/drawing.md#the-compositions-card): the drawing's
//! definitions, as the « Composants » card shows the model's: tiles or rows, how many times each
//! is on the sheets, the unused ones apart; a click picks its instances in the level open, a drag
//! onto the sheet places one more at the pointer, its menu places, renames or deletes it.

use std::collections::HashMap;

use skraft_i18n::{t, tp};
use skraft_model::{Container, Drawing, Id, Operation, Projection};
use skraft_tools::Editor;
use skraft_tools::search::Matcher;

use crate::library::{LIST_ROW, LIST_THUMB, Line, Rows, TILE_GAP, TILE_LABEL, count_badge, name_job, natural_cmp, view_toggle};
use crate::panels::{Action, rename_field, tree_row};
use crate::sheet::{SheetState, Sketch, sketch_of};
use crate::style::Tip;

/// What the card keeps of a document between frames (never on disk): its search, its rows and
/// the compositions' outlines, made again when the document or what they depend on changes.
#[derive(Clone, Debug, Default)]
pub struct CompositionsState {
    search: String,
    rows: Option<(u64, String, Rows)>,
    sketches: Option<(u64, Projection, HashMap<Id, Option<Sketch>>)>,
}

impl CompositionsState {
    /// The outline of composition `id`, drawn as the presentation shown projects its views.
    pub fn sketch(&mut self, e: &Editor, id: Id, projection: Projection) -> Option<&Sketch> {
        let revision = e.history().revision();
        if self.sketches.as_ref().is_none_or(|(r, p, _)| *r != revision || *p != projection) {
            self.sketches = Some((revision, projection, HashMap::new()));
        }
        let (_, _, sketches) = self.sketches.as_mut()?;
        sketches.entry(id).or_insert_with(|| sketch_of(e, id, projection)).as_ref()
    }
}

/// The compositions whose name holds what `matcher` was typed, by section, in natural order.
fn rows_of(d: &Drawing, matcher: &Matcher) -> Rows {
    let counts = d.counts_on_sheets();
    let mut rows = Rows::default();
    for c in d.compositions.values().filter(|c| matcher.name([c.name.as_str()])) {
        match counts.get(&c.id) {
            Some(&n) => rows.used.push((c.id, n as u64)),
            None => rows.unused.push(c.id),
        }
    }
    let name = |id: &Id| d.composition(*id).map_or("", |c| c.name.as_str());
    rows.used.sort_by(|a, b| natural_cmp(name(&a.0), name(&b.0)));
    rows.unused.sort_by(|a, b| natural_cmp(name(a), name(b)));
    rows
}

/// Can composition `id` be placed in the level open? Not inside itself.
fn placeable(d: &Drawing, st: &SheetState, id: Id) -> bool {
    st.container(d).is_some_and(|c| !matches!(c, Container::Composition(x) if d.reaches(id, x)))
}

/// Its instances a click picks: those in the level open, neither hidden nor locked.
fn instances_here(d: &Drawing, st: &SheetState, id: Id) -> Vec<Id> {
    st.container(d)
        .and_then(|c| d.elements_in(c))
        .unwrap_or(&[])
        .iter()
        .filter(|el| !el.hidden && !el.locked && el.kind.instance().is_some_and(|i| i.composition == id))
        .map(|el| el.id)
        .collect()
}

/// The colours of an item.
struct Ink {
    selected: bool,
    text: egui::Color32,
    weak: egui::Color32,
}

fn ink_of(ui: &egui::Ui, d: &Drawing, st: &SheetState, id: Id) -> Ink {
    let here = instances_here(d, st, id);
    let selected = !here.is_empty() && st.selected.len() == here.len() && here.iter().all(|i| st.selected.contains(i));
    let v = ui.visuals();
    let text = if selected {
        v.selection.stroke.color
    } else if placeable(d, st, id) {
        v.text_color()
    } else {
        v.weak_text_color()
    };
    Ink { selected, text, weak: v.weak_text_color() }
}

/// The outline of `id` in `rect`, on a ground of its own.
fn picture(ui: &egui::Ui, e: &Editor, st: &mut SheetState, id: Id, rect: egui::Rect, ink: &Ink) {
    ui.painter().rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
    let projection = st.projection(e.document().drawing());
    if let Some(sketch) = st.library.sketch(e, id, projection) {
        sketch.paint_in(ui.painter(), rect.shrink((rect.width() * 0.12).max(2.0)), egui::Stroke::new(1.0, ink.text));
    }
}

/// The card's content, `height` high, in `area`: the search and the grid / list toggle, then the
/// compositions in their sections.
pub fn compositions(
    ui: &mut egui::Ui,
    editor: &Editor,
    st: &mut SheetState,
    as_list: bool,
    height: f32,
    actions: &mut Vec<Action>,
    area: egui::ScrollArea,
) {
    let d = editor.document().drawing();
    let top = ui.cursor().top();
    let revision = editor.history().revision();
    let matcher = Matcher::new(&st.library.search);
    if st.library.rows.as_ref().is_none_or(|(r, f, _)| *r != revision || *f != st.library.search) {
        st.library.rows = Some((revision, st.library.search.clone(), rows_of(d, &matcher)));
    }
    let salt = ui.make_persistent_id("compositions_search");
    crate::search::field(ui, salt, "compositions", &mut st.library.search, |ui, side| {
        view_toggle(ui, side, as_list, "compositions", actions)
    });
    ui.add_space(crate::search::GAP - ui.spacing().item_spacing.y);
    let Some((_, _, rows)) = st.library.rows.clone() else { return };
    if rows.is_empty() {
        ui.weak(if matcher.is_active() { t!("search.no_match") } else { t!("panel.compositions.none") });
        return;
    }
    let room = (height - (ui.cursor().top() - top)).max(0.0);
    let area = area.max_height(room).min_scrolled_height(room);
    if as_list {
        list(ui, editor, d, st, &rows, actions, area);
    } else {
        grid(ui, editor, d, st, &rows, actions, area);
    }
}

/// The compositions as rows: an outline, the name, the count.
fn list(ui: &mut egui::Ui, e: &Editor, d: &Drawing, st: &mut SheetState, rows: &Rows, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let lines = rows.lines();
    area.show_rows(ui, LIST_ROW, lines.len(), |ui, range| {
        ui.set_min_width(ui.available_width());
        for line in &lines[range] {
            let (id, count) = match *line {
                Line::Header { unused } => {
                    header(ui, LIST_ROW, unused, actions);
                    continue;
                }
                Line::Item { id, count } => (id, count),
            };
            let Some(c) = d.composition(id) else { continue };
            let ink = ink_of(ui, d, st, id);
            let rename = ui.make_persistent_id(("compositions_rename", id));
            let renaming = is_renaming(ui, rename);
            let (response, renamed) = tree_row(ui, LIST_ROW, ink.selected, false, false, |ui| {
                let (square, _) = ui.allocate_exact_size(egui::Vec2::splat(LIST_THUMB), egui::Sense::hover());
                picture(ui, e, st, id, square, &ink);
                if renaming {
                    return rename_field(ui, rename, &c.name);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(n) = count {
                        ui.add(egui::Label::new(egui::RichText::new(format!("×{n}")).color(ink.weak)).selectable(false));
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(name_job(ui, &c.name, !c.named, ink.text, ink.selected)).selectable(false).truncate());
                    });
                });
                None
            });
            item(ui, d, st, id, count, response, (rename, renaming, renamed), actions);
        }
    });
}

/// The compositions as tiles, their names under them, their count over a corner.
fn grid(ui: &mut egui::Ui, e: &Editor, d: &Drawing, st: &mut SheetState, rows: &Rows, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let header_height = ui.spacing().interact_size.y;
    let apart = !rows.unused.is_empty();
    area.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        let tile = crate::thumbnails::TILE;
        let columns = (((ui.available_width() + TILE_GAP) / (tile + TILE_GAP)).floor() as usize).max(1);
        let used: Vec<(Id, Option<u64>)> = rows.used.iter().map(|&(id, n)| (id, Some(n))).collect();
        let unused: Vec<(Id, Option<u64>)> = rows.unused.iter().map(|&id| (id, None)).collect();
        for (section, items) in [(false, used), (true, unused)] {
            if items.is_empty() {
                continue;
            }
            if apart {
                header(ui, header_height, section, actions);
            }
            for chunk in items.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = TILE_GAP;
                    for &(id, count) in chunk {
                        let Some(c) = d.composition(id) else { continue };
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(tile, tile + TILE_LABEL), egui::Sense::click_and_drag());
                        let rename = ui.make_persistent_id(("compositions_rename", id));
                        let renaming = is_renaming(ui, rename);
                        let mut renamed = None;
                        if ui.is_rect_visible(rect) {
                            let ink = ink_of(ui, d, st, id);
                            let square = egui::Rect::from_min_size(rect.min, egui::Vec2::splat(tile));
                            picture(ui, e, st, id, square, &ink);
                            let v = ui.visuals();
                            let stroke = if ink.selected {
                                egui::Stroke::new(2.0, v.selection.stroke.color)
                            } else if response.hovered() {
                                v.widgets.hovered.bg_stroke
                            } else {
                                egui::Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color)
                            };
                            ui.painter().rect_stroke(square, 4.0, stroke, egui::StrokeKind::Inside);
                            if let Some(n) = count {
                                count_badge(ui, square, n, ink.weak);
                            }
                            let label = egui::Rect::from_min_max(egui::pos2(rect.min.x, square.max.y + 2.0), rect.max);
                            let layout = egui::Layout::top_down(egui::Align::Center);
                            let mut under = ui.new_child(egui::UiBuilder::new().max_rect(label).layout(layout));
                            if renaming {
                                renamed = rename_field(&mut under, rename, &c.name);
                            } else {
                                let small = egui::TextStyle::Small.resolve(under.style()).size;
                                let mut job = name_job(&under, &c.name, !c.named, ink.text, ink.selected);
                                job.sections.iter_mut().for_each(|s| s.format.font_id.size = small);
                                job.wrap = egui::text::TextWrapping::truncate_at_width(label.width());
                                under.add(egui::Label::new(job).selectable(false));
                            }
                        }
                        item(ui, d, st, id, count, response, (rename, renaming, renamed), actions);
                    }
                });
                ui.add_space(TILE_GAP / 2.0);
            }
        }
    });
}

fn is_renaming(ui: &egui::Ui, rename: egui::Id) -> bool {
    ui.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(crate::panels::RENAMING))) == Some(rename)
}

/// An item's interaction, a row's or a tile's: its tip, its rename, its click or drag, its menu.
#[allow(clippy::too_many_arguments)]
fn item(
    ui: &mut egui::Ui,
    d: &Drawing,
    st: &mut SheetState,
    id: Id,
    count: Option<u64>,
    response: egui::Response,
    (rename, renaming, renamed): (egui::Id, bool, Option<Option<String>>),
    actions: &mut Vec<Action>,
) {
    crate::handles::publish(|| format!("compositions:{}", id.0), response.rect);
    let name = d.composition(id).map_or_else(String::new, |c| c.name.clone());
    let tip = match count {
        Some(n) => tp!("panel.compositions.count_tip", n as usize),
        None => t!("panel.compositions.unused_tip"),
    };
    let response = ui.interact(response.rect, response.id, egui::Sense::click_and_drag());
    let response = if response.dragged() { response } else { response.tip(format!("{name}\n{tip}")) };
    let here = instances_here(d, st, id);
    let alone = !here.is_empty() && st.selected == here;
    let key = egui::Id::unique(crate::panels::RENAMING);
    let f2 = || alone && ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(egui::Key::F2));
    if renamed.is_some() {
        ui.data_mut(|d| d.remove::<egui::Id>(key));
    } else if !renaming && (response.double_clicked() || f2()) {
        ui.data_mut(|d| d.insert_temp(key, rename));
    }
    if let Some(Some(name)) = renamed
        && !name.is_empty()
    {
        actions.push(Action::Apply(Operation::RenameComposition { composition: id, name }));
    }
    let can_place = placeable(d, st, id);
    // Dragged, it may land on any sheet of the board, whatever the level open or the sheet active.
    let can_carry = d.sheets().next().is_some();
    if response.drag_started_by(egui::PointerButton::Primary) && can_carry {
        st.carried = Some(id);
    } else if response.clicked() {
        st.selected = here;
    }
    if response.dragged_by(egui::PointerButton::Primary) && can_carry {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    egui::Popup::context_menu(&response).style(crate::style::menu).show(|ui| {
        let place = ui.add_enabled(can_place, egui::Button::new(t!("drawing.tree.place")));
        crate::handles::publish(|| "compositions:menu:place".into(), place.rect);
        if place.clicked() {
            st.place_composition = Some(id);
        }
        ui.separator();
        if ui.button(t!("panel.components.rename")).clicked() {
            ui.data_mut(|d| d.insert_temp(key, rename));
        }
        let delete = ui.button(t!("panel.components.delete"));
        crate::handles::publish(|| "compositions:menu:delete".into(), delete.rect);
        if delete.tip(t!("panel.compositions.delete_tip")).clicked() {
            actions.push(Action::Apply(Operation::DeleteComposition { composition: id }));
        }
    });
}

/// A section's header: its title, and for the unused ones « Purger ».
fn header(ui: &mut egui::Ui, height: f32, unused: bool, actions: &mut Vec<Action>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    let title = if unused { t!("panel.compositions.unused") } else { t!("panel.compositions.on_sheets") };
    let font = crate::style::semibold(egui::TextStyle::Small.resolve(row.style()).size);
    row.add(egui::Label::new(egui::RichText::new(title).font(font).color(row.visuals().weak_text_color())).selectable(false));
    if unused {
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let purge = ui.add(egui::Button::new(t!("panel.components.purge")).small());
            crate::handles::publish(|| "compositions:purge".into(), purge.rect);
            if purge.tip(t!("panel.compositions.purge_tip")).clicked() {
                actions.push(Action::Apply(Operation::PurgeCompositions));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec2;
    use skraft_model::{Document, ElementKind, History, Instance, Paper, Place};

    use super::*;

    #[test]
    fn the_card_lists_those_on_the_sheets_then_the_unused_with_the_search() {
        let mut h = History::new(Document::new());
        let s = h.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().sheet.unwrap();
        let to = Container::Sheet(s);
        let line = |h: &mut History| {
            h.apply(Operation::AddElement { to, element: ElementKind::Line { from: DVec2::ZERO, to: DVec2::X } }).unwrap().element.unwrap()
        };
        let (a, b) = (line(&mut h), line(&mut h));
        let cartouche = h.apply(Operation::MakeComposition { elements: vec![a], name: Some("Cartouche".into()) }).unwrap().element.unwrap();
        let other = h.apply(Operation::MakeComposition { elements: vec![b], name: Some("Légende".into()) }).unwrap().element.unwrap();
        let of = |h: &History, i: Id| h.document().drawing().element(i).unwrap().kind.instance().unwrap().composition;
        let (c, l) = (of(&h, cartouche), of(&h, other));
        let place = Place::free(DVec2::ONE);
        h.apply(Operation::AddElement {
            to,
            element: ElementKind::Instance(Instance { composition: c, place, props: Default::default() }),
        })
        .unwrap();
        h.apply(Operation::DeleteElements { elements: vec![other] }).unwrap();
        let rows = rows_of(h.document().drawing(), &Matcher::new(""));
        assert_eq!(rows.used, vec![(c, 2)]);
        assert_eq!(rows.unused, vec![l], "named, placed nowhere: kept apart");
        let found = rows_of(h.document().drawing(), &Matcher::new("lég"));
        assert!(found.used.is_empty() && found.unused == vec![l]);
    }
}

//! The « Compositions » card (docs/ui/drawing.md#the-compositions-card): the drawing's
//! definitions, as the « Composants » card shows the model's: tiles or rows, how many times each
//! is on the sheets, the unused ones apart; a click picks its instances in the level open, a drag
//! onto the sheet places one more at the pointer, its menu places, renames or deletes it.

use std::collections::HashMap;

use skraft_i18n::{t, tp};
use skraft_model::{Container, Drawing, Id, Operation, Projection};
use skraft_tools::Editor;
use skraft_tools::search::Matcher;

use crate::cards::catalog::{self, Card, Ink, Renaming, Rows, Shelf};
use crate::cards::library::natural_cmp;
use crate::commands::Action;
use crate::drawing::sheet::{SheetState, Sketch, sketch_of};
use crate::kit::style::Tip;

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

fn ink_of(ui: &egui::Ui, d: &Drawing, st: &SheetState, id: Id) -> Ink {
    let here = instances_here(d, st, id);
    let selected = !here.is_empty() && st.selected.len() == here.len() && here.iter().all(|i| st.selected.contains(i));
    Ink::new(ui, selected, !placeable(d, st, id))
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
    let matcher = Matcher::new(&st.library.search);
    let library = &mut st.library;
    let rows = crate::kit::search::listed(&mut library.rows, editor.history().revision(), &library.search, || rows_of(d, &matcher)).clone();
    let salt = ui.make_persistent_id("compositions_search");
    crate::kit::search::field(ui, salt, "compositions", &mut st.library.search, |ui, side| {
        catalog::view_toggle(ui, side, as_list, Card::Compositions, actions)
    });
    ui.add_space(crate::kit::search::GAP - ui.spacing().item_spacing.y);
    if rows.is_empty() {
        ui.weak(if matcher.is_active() { t!("search.no_match") } else { t!("panel.compositions.none") });
        return;
    }
    let area = catalog::fit(ui, area, height, top);
    catalog::show(ui, &mut Compositions { e: editor, d, st }, &rows, as_list, actions, area);
}

/// The card's items, for the shared presentation.
struct Compositions<'a> {
    e: &'a Editor,
    d: &'a Drawing,
    st: &'a mut SheetState,
}

impl Shelf for Compositions<'_> {
    fn rename_salt(&self) -> &'static str {
        "compositions_rename"
    }

    fn has(&self, id: Id) -> bool {
        self.d.composition(id).is_some()
    }

    fn name(&self, id: Id) -> (String, bool) {
        self.d.composition(id).map_or_else(|| (String::new(), false), |c| (c.name.clone(), !c.named))
    }

    fn ink(&mut self, ui: &egui::Ui, id: Id) -> Ink {
        ink_of(ui, self.d, self.st, id)
    }

    fn picture(&mut self, ui: &mut egui::Ui, rect: egui::Rect, id: Id, ink: &Ink) {
        picture(ui, self.e, self.st, id, rect, ink);
    }

    fn interact(
        &mut self,
        ui: &mut egui::Ui,
        id: Id,
        count: Option<u64>,
        response: egui::Response,
        rename: Renaming,
        actions: &mut Vec<Action>,
    ) {
        item(ui, self.d, self.st, id, count, response, rename, actions);
    }

    fn header(&mut self, ui: &mut egui::Ui, height: f32, unused: bool, actions: &mut Vec<Action>) {
        let title = if unused { t!("panel.compositions.unused") } else { t!("panel.compositions.on_sheets") };
        let purge = unused.then(|| (true, t!("panel.compositions.purge_tip"), "compositions:purge"));
        if catalog::header(ui, height, title, purge) {
            actions.push(Action::Apply(Operation::PurgeCompositions));
        }
    }
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
    rename: Renaming,
    actions: &mut Vec<Action>,
) {
    crate::kit::handles::publish(|| format!("compositions:{}", id.0), response.rect);
    let name = d.composition(id).map_or_else(String::new, |c| c.name.clone());
    let tip = match count {
        Some(n) => tp!("panel.compositions.count_tip", n as usize),
        None => t!("panel.compositions.unused_tip"),
    };
    let response = ui.interact(response.rect, response.id, egui::Sense::click_and_drag());
    let response = if response.dragged() { response } else { response.tip(format!("{name}\n{tip}")) };
    let here = instances_here(d, st, id);
    let alone = !here.is_empty() && st.selected == here;
    let key = rename.key;
    if let Some(name) = rename.gesture(ui, &response, alone) {
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
    egui::Popup::context_menu(&response).style(crate::kit::style::menu).show(|ui| {
        let place = ui.add_enabled(can_place, egui::Button::new(t!("drawing.tree.place")));
        crate::kit::handles::publish(|| "compositions:menu:place".into(), place.rect);
        if place.clicked() {
            st.place_composition = Some(id);
        }
        ui.separator();
        if ui.button(t!("panel.components.rename")).clicked() {
            Renaming::start(ui, key);
        }
        let delete = ui.button(t!("panel.components.delete"));
        crate::kit::handles::publish(|| "compositions:menu:delete".into(), delete.rect);
        if delete.tip(t!("panel.compositions.delete_tip")).clicked() {
            actions.push(Action::Apply(Operation::DeleteComposition { composition: id }));
        }
    });
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

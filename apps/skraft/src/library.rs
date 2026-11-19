//! The « Composants » card (docs/ui/components-library.md): the document's parts and assemblies,
//! how many times each is in the model, the unused ones apart; a click chooses one (the entity
//! info shows it), its menu selects its occurrences, replaces the selection by it, renames or
//! deletes it; « Purger » drops the unused ones.

use std::cmp::Ordering;

use skraft_i18n::{t, tp};
use skraft_model::{Document, Id};
use skraft_tools::Editor;
use skraft_tools::search::{Filter, Matcher, Nature};

use crate::icons;
use crate::thumbnails::Thumbnails;
use crate::panels::{Action, rename_field, tree_row, weak_share};

/// What the card keeps of a document between frames, in the document itself (never on disk):
/// its search, its rows — made again when the document (its history's revision) or the search
/// changes.
#[derive(Default)]
pub struct LibraryState {
    pub filter: Filter,
    rows: Option<(u64, Filter, Rows)>,
}

/// The components in their sections, each in natural order of names.
#[derive(Default)]
struct Rows {
    /// In the model, with their count there.
    used: Vec<(Id, u64)>,
    /// Placed nowhere (named).
    unused: Vec<Id>,
}

/// One line of the card.
enum Line {
    Header { unused: bool },
    Item { id: Id, count: Option<u64> },
}

impl Rows {
    /// The components that pass `matcher` (docs/ui/components-library.md#search): by their
    /// name, their nature, the materials they show.
    fn of(doc: &Document, matcher: &Matcher) -> Rows {
        let counts = doc.counts_in_model();
        let shown = matcher.material().map(|_| skraft_tools::search::materials_shown(doc));
        let mut used = Vec::new();
        let mut unused = Vec::new();
        for c in doc.components() {
            let nature = if c.is_part() { Nature::Part } else { Nature::Assembly };
            let materials = shown.as_ref().and_then(|s| s.get(&c.id)).into_iter().flatten().copied();
            if !matcher.matches(nature, [c.name.as_str()], materials) {
                continue;
            }
            match counts.get(&c.id) {
                Some(&n) => used.push((c.id, n)),
                None => unused.push(c.id),
            }
        }
        let name = |id: &Id| doc.component(*id).map(|c| c.name.as_str()).unwrap_or("");
        used.sort_by(|a, b| natural_cmp(name(&a.0), name(&b.0)));
        unused.sort_by(|a, b| natural_cmp(name(a), name(b)));
        Rows { used, unused }
    }

    /// The lines shown: the headers only when there are unused ones.
    fn lines(&self) -> Vec<Line> {
        let mut lines = Vec::with_capacity(self.used.len() + self.unused.len() + 2);
        let apart = !self.unused.is_empty();
        if apart && !self.used.is_empty() {
            lines.push(Line::Header { unused: false });
        }
        lines.extend(self.used.iter().map(|&(id, n)| Line::Item { id, count: Some(n) }));
        if apart {
            lines.push(Line::Header { unused: true });
            lines.extend(self.unused.iter().map(|&id| Line::Item { id, count: None }));
        }
        lines
    }
}

/// Names compared as people read them: case ignored, runs of digits by their value
/// (« Pièce#2 » before « Pièce#10 »).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    // Names equal but for case or leading zeros: the first such difference decides.
    let mut tie = Ordering::Equal;
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return tie,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(d) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(d);
                        it.next();
                    }
                    digits
                };
                let (da, db) = (run(&mut a), run(&mut b));
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
                tie = tie.then(da.len().cmp(&db.len()));
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                tie = tie.then(x.cmp(&y));
                a.next();
                b.next();
            }
        }
    }
}

/// A component's name in its row: its colour `ink`, an automatic one in italics, attenuated
/// unless the row is selected.
fn name_job(ui: &egui::Ui, name: &str, auto: bool, ink: egui::Color32, selected: bool) -> egui::text::LayoutJob {
    let size = egui::TextStyle::Body.resolve(ui.style()).size;
    let weak = if selected { ink } else { ink.lerp_to_gamma(ui.visuals().window_fill, weak_share(ui.visuals())) };
    let format = egui::TextFormat {
        font_id: egui::FontId::proportional(size),
        color: if auto { weak } else { ink },
        italics: auto,
        ..Default::default()
    };
    egui::text::LayoutJob::single_section(name.to_owned(), format)
}

/// How the card is drawn: its document (the thumbnails' key), the thumbnails, as a list or a
/// grid.
pub struct Look<'a> {
    pub doc: u64,
    pub thumbnails: &'a mut Thumbnails,
    pub list: bool,
    /// The height the card gives its content: the rows above the list (the toggle, the note
    /// of a part open) take theirs from it, so the list ends at the card's bottom.
    pub height: f32,
}

/// A list row's height, and its thumbnail's side (points).
const LIST_ROW: f32 = 30.0;
const LIST_THUMB: f32 = 26.0;
/// The room under a tile for its name (points), and between tiles.
const TILE_LABEL: f32 = 18.0;
const TILE_GAP: f32 = 6.0;

/// The card's content, in `area`: the grid / list toggle, then the components in their
/// sections, as tiles of thumbnails or as rows (only the rows in view laid out, only the
/// thumbnails in view asked for: a model may hold thousands of definitions).
pub fn components(ui: &mut egui::Ui, editor: &Editor, state: &mut LibraryState, look: Look<'_>, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let doc = editor.document();
    let top = ui.cursor().top();
    // The search (docs/ui/components-library.md#search): the field left of the grid / list
    // toggle, the natures and the material under it.
    let salt = ui.make_persistent_id("components_search");
    crate::search::field(ui, salt, "components", &mut state.filter.text, |ui| view_toggle(ui, look.list, actions));
    crate::search::filters(ui, salt, "components", &mut state.filter, doc);
    let revision = editor.history().revision();
    if state.rows.as_ref().is_none_or(|(r, f, _)| *r != revision || *f != state.filter) {
        state.rows = Some((revision, state.filter.clone(), Rows::of(doc, &state.filter.matcher())));
    }
    let Some((_, _, rows)) = &state.rows else { return };
    let disabled = editor.library_disabled();
    if disabled {
        ui.add(egui::Label::new(egui::RichText::new(t!("panel.components.disabled")).weak()).wrap());
    }
    if rows.used.is_empty() && rows.unused.is_empty() {
        let none = if state.filter.is_active() { t!("search.no_match") } else { t!("panel.components.empty") };
        ui.weak(none);
        return;
    }
    let ppp = ui.ctx().pixels_per_point();
    let mut thumbs = Thumbs {
        doc: look.doc,
        thumbnails: look.thumbnails,
        dark: ui.visuals().dark_mode,
        px: ((crate::thumbnails::TILE * ppp).round() as u32).clamp(16, 512),
    };
    let cx = Cx { editor, chosen: editor.chosen_component(), disabled };
    let height = (look.height - (ui.cursor().top() - top)).max(0.0);
    let area = area.max_height(height).min_scrolled_height(height);
    if look.list {
        list(ui, &cx, rows, &mut thumbs, actions, area);
    } else {
        grid(ui, &cx, rows, &mut thumbs, actions, area);
    }
    if thumbs.thumbnails.pending() {
        ui.ctx().request_repaint();
    }
}

/// What every item of the card is drawn with.
struct Cx<'a> {
    editor: &'a Editor,
    chosen: Option<Id>,
    disabled: bool,
}

/// The thumbnails, as the card asks for them.
struct Thumbs<'a> {
    doc: u64,
    thumbnails: &'a mut Thumbnails,
    dark: bool,
    px: u32,
}

impl Thumbs<'_> {
    fn get(&mut self, editor: &Editor, id: Id) -> Option<egui::TextureId> {
        self.thumbnails.get(self.doc, editor, id, self.dark, self.px)
    }
}

/// The grid / list toggle, at the right end of the search row (laid out right to left).
fn view_toggle(ui: &mut egui::Ui, list: bool, actions: &mut Vec<Action>) {
    let tint = ui.visuals().text_color();
    ui.spacing_mut().item_spacing.x = 2.0;
    for (on, icon, tip, handle) in [
        (list, icons::LIST, t!("panel.components.as_list"), "components:list"),
        (!list, icons::TILES, t!("panel.components.as_grid"), "components:grid"),
    ] {
        let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint);
        let b = ui.add(egui::Button::image(img).selected(on));
        crate::handles::publish(|| handle.into(), b.rect);
        if b.on_hover_text(tip).clicked() && !on {
            actions.push(Action::ComponentsList(handle == "components:list"));
        }
    }
}

/// The components as rows: a thumbnail, the name, the count.
fn list(ui: &mut egui::Ui, cx: &Cx<'_>, rows: &Rows, thumbs: &mut Thumbs<'_>, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let lines = rows.lines();
    let doc = cx.editor.document();
    area.show_rows(ui, LIST_ROW, lines.len(), |ui, range| {
        ui.set_min_width(ui.available_width());
        for line in &lines[range] {
            let (id, count) = match *line {
                Line::Header { unused } => {
                    header(ui, LIST_ROW, unused, cx.disabled, actions);
                    continue;
                }
                Line::Item { id, count } => (id, count),
            };
            let Ok(c) = doc.component(id) else { continue };
            let ink = Ink::of(ui, cx, id);
            let rename = ui.make_persistent_id(("components_rename", id));
            let renaming = renaming(ui, rename);
            let thumb = thumbs.get(cx.editor, id);
            let (response, renamed) = tree_row(ui, LIST_ROW, ink.selected, false, false, |ui| {
                picture(ui, egui::Vec2::splat(LIST_THUMB), thumb, c.is_part(), &ink);
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
            item(ui, cx, id, count, response, (rename, renaming, renamed), actions);
        }
    });
}

/// The components as tiles of thumbnails, their names under them, their count over a corner;
/// the thumbnails asked for only for the tiles in view.
fn grid(ui: &mut egui::Ui, cx: &Cx<'_>, rows: &Rows, thumbs: &mut Thumbs<'_>, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let doc = cx.editor.document();
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
                header(ui, header_height, section, cx.disabled, actions);
            }
            for chunk in items.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = TILE_GAP;
                    for &(id, count) in chunk {
                        let Ok(c) = doc.component(id) else { continue };
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(tile, tile + TILE_LABEL), egui::Sense::click_and_drag());
                        let rename = ui.make_persistent_id(("components_rename", id));
                        let renaming = renaming(ui, rename);
                        let mut renamed = None;
                        if ui.is_rect_visible(rect) {
                            let ink = Ink::of(ui, cx, id);
                            let square = egui::Rect::from_min_size(rect.min, egui::Vec2::splat(tile));
                            let v = ui.visuals();
                            let stroke = if ink.selected {
                                egui::Stroke::new(2.0, v.selection.stroke.color)
                            } else if response.hovered() {
                                v.widgets.hovered.bg_stroke
                            } else {
                                egui::Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color)
                            };
                            let thumb = thumbs.get(cx.editor, id);
                            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(square));
                            picture(&mut inner, egui::Vec2::splat(tile), thumb, c.is_part(), &ink);
                            ui.painter().rect_stroke(square, 4.0, stroke, egui::StrokeKind::Inside);
                            if let Some(n) = count {
                                count_badge(ui, square, n, &ink);
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
                        item(ui, cx, id, count, response, (rename, renaming, renamed), actions);
                    }
                });
                ui.add_space(TILE_GAP / 2.0);
            }
        }
    });
}

/// The colours an item is drawn in.
struct Ink {
    selected: bool,
    greyed: bool,
    text: egui::Color32,
    weak: egui::Color32,
}

impl Ink {
    fn of(ui: &egui::Ui, cx: &Cx<'_>, id: Id) -> Ink {
        let v = ui.visuals();
        let selected = cx.chosen == Some(id);
        // Greyed: the whole card while a part is open, the assemblies along the active path.
        let greyed = cx.disabled || cx.editor.on_active_path(id);
        let text = if selected {
            v.selection.stroke.color
        } else if greyed {
            v.weak_text_color()
        } else {
            v.text_color()
        };
        Ink { selected, greyed, text, weak: v.weak_text_color() }
    }
}

/// A thumbnail in a `size` square (rounded), else the nature's icon in its middle; faded when
/// greyed.
fn picture(ui: &mut egui::Ui, size: egui::Vec2, thumb: Option<egui::TextureId>, part: bool, ink: &Ink) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let fade = if ink.greyed { 0.45 } else { 1.0 };
    match thumb {
        Some(t) => {
            egui::Image::new((t, size)).corner_radius(4.0).tint(egui::Color32::WHITE.gamma_multiply(fade)).paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
            let icon = if part { icons::BOX } else { icons::COMPONENT };
            let side = (size.x * 0.45).clamp(12.0, 28.0);
            egui::Image::new(icon).tint(ink.text).paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(side)));
        }
    }
}

/// « ×n » over a tile's top right corner, on a pill of the card's colour.
fn count_badge(ui: &egui::Ui, square: egui::Rect, n: u64, ink: &Ink) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(format!("×{n}"), font, ink.weak);
    let pad = egui::vec2(4.0, 1.0);
    let min = egui::pos2(square.max.x - galley.size().x - 2.0 * pad.x - 3.0, square.min.y + 3.0);
    let r = egui::Rect::from_min_size(min, galley.size() + 2.0 * pad);
    ui.painter().rect_filled(r, r.height() / 2.0, ui.visuals().window_fill.gamma_multiply(0.85));
    ui.painter().galley(r.min + pad, galley, ink.weak);
}

/// Is the item of key `rename` renamed in place now?
fn renaming(ui: &egui::Ui, rename: egui::Id) -> bool {
    ui.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(crate::panels::RENAMING))) == Some(rename)
}

/// An item's interaction, a row's or a tile's: its tip, its rename, its click or drag (placing
/// it), its menu.
fn item(
    ui: &mut egui::Ui,
    cx: &Cx<'_>,
    id: Id,
    count: Option<u64>,
    response: egui::Response,
    (rename, renaming, renamed): (egui::Id, bool, Option<Option<String>>),
    actions: &mut Vec<Action>,
) {
    crate::handles::publish(|| format!("components:{}", id.0), response.rect);
    let name = cx.editor.document().component(id).map(|c| c.name.clone()).unwrap_or_default();
    let tip = match count {
        Some(n) => tp!("panel.components.count_tip", n as usize),
        None => t!("panel.components.unused_tip"),
    };
    let tip = if cx.editor.on_active_path(id) {
        format!("{name}\n{tip}\n{}", t!("panel.components.on_path"))
    } else {
        format!("{name}\n{tip}")
    };
    // Dragged out of the card too (docs/ui/components-library.md#placing).
    let response = ui.interact(response.rect, response.id, egui::Sense::click_and_drag());
    let response = if response.dragged() { response } else { response.on_hover_text(tip) };
    if cx.disabled {
        return;
    }
    // Renamed in place: from the menu, or F2 while it is the one chosen — not by a
    // double-click, the first click choosing (and placing) it. A component's name is never
    // empty, and unique (one taken refused, the failure told).
    let key = egui::Id::unique(crate::panels::RENAMING);
    if renamed.is_some() {
        ui.data_mut(|d| d.remove::<egui::Id>(key));
    } else if !renaming && cx.chosen == Some(id) && ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(egui::Key::F2)) {
        ui.data_mut(|d| d.insert_temp(key, rename));
    }
    if let Some(Some(name)) = renamed
        && !name.is_empty()
    {
        actions.push(Action::Apply(skraft_model::Operation::RenameComponent { component: id, name }));
    }
    // A click, or a drag out of the card, carries one more occurrence of it at the cursor
    // (chosen meanwhile); an assembly along the active path is only chosen (it would hold
    // itself).
    let place = cx.editor.can_place_component(id);
    if response.drag_started_by(egui::PointerButton::Primary) && place {
        actions.push(Action::PlaceComponent(id, true));
    } else if response.clicked() {
        actions.push(if place { Action::PlaceComponent(id, false) } else { Action::ChooseComponent(id) });
    }
    if response.dragged_by(egui::PointerButton::Primary) && place {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    menu(cx.editor, &response, id, rename, actions);
}

/// A section's header: its title, and for the unused ones the « Purger » button.
fn header(ui: &mut egui::Ui, height: f32, unused: bool, disabled: bool, actions: &mut Vec<Action>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    let title = if unused { t!("panel.components.unused") } else { t!("panel.components.in_model") };
    let font = crate::style::semibold(egui::TextStyle::Small.resolve(row.style()).size);
    row.add(egui::Label::new(egui::RichText::new(title).font(font).color(row.visuals().weak_text_color())).selectable(false));
    if unused {
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let purge = ui.add_enabled(!disabled, egui::Button::new(t!("panel.components.purge")).small());
            crate::handles::publish(|| "components:purge".into(), purge.rect);
            if purge.on_hover_text(t!("panel.components.purge_tip")).clicked() {
                actions.push(Action::Command("components.purge"));
            }
        });
    }
}

/// A component's menu (right click): its occurrences selected, the selection replaced by it,
/// renamed, deleted. The selection is kept: it is what « Remplacer la sélection » takes.
fn menu(editor: &Editor, response: &egui::Response, id: Id, rename: egui::Id, actions: &mut Vec<Action>) {
    egui::Popup::context_menu(response).style(crate::style::menu).show(|ui| {
        let mut row = |ui: &mut egui::Ui, label: String, enabled: bool, action: Action| {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                actions.push(action);
            }
        };
        let here = !editor.occurrences_under_context(id).is_empty();
        row(ui, t!("panel.components.select"), here, Action::SelectOccurrences(id));
        row(ui, t!("panel.components.replace"), editor.can_replace_selection(id), Action::ReplaceSelection(id));
        ui.separator();
        if ui.button(t!("panel.components.rename")).clicked() {
            ui.data_mut(|d| d.insert_temp(egui::Id::unique(crate::panels::RENAMING), rename));
        }
        let delete = ui.add_enabled(editor.can_delete_component(id), egui::Button::new(t!("panel.components.delete")));
        if delete.on_hover_text(t!("panel.components.delete_tip")).clicked() {
            actions.push(Action::DeleteComponent(id));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_lists_what_the_search_lets_through() {
        use glam::{DAffine3, DVec3};
        use skraft_model::{Operation, Primitive};
        let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
        let root = e.document().root();
        let mut ids = Vec::new();
        for x in [0.0, 100.0] {
            let p = Primitive::Box { origin: DVec3::new(x, 0.0, 0.0), size: DVec3::splat(10.0) };
            ids.push(e.apply(Operation::AddPrimitive { context: root, primitive: p, placement: DAffine3::IDENTITY, material: None }).unwrap().unwrap().created[0]);
        }
        e.apply(Operation::MakeAssembly { context: root, entities: vec![ids[0]], name: Some("Caisson".into()) }).unwrap();
        let all = Rows::of(e.document(), &Filter::default().matcher());
        assert_eq!(all.used.len(), 3);
        let named = Rows::of(e.document(), &Filter { text: "CAIS".into(), ..Filter::default() }.matcher());
        assert_eq!(named.used.len(), 1);
        let parts = Rows::of(e.document(), &Filter { assemblies: false, ..Filter::default() }.matcher());
        assert!(parts.used.iter().all(|(id, _)| e.document().part(*id).is_ok()) && parts.used.len() == 2);
        let oak = e.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap().unwrap().material.unwrap();
        e.apply(Operation::PaintOccurrences { context: root, entities: vec![ids[1]], material: Some(oak) }).unwrap();
        let painted = Rows::of(e.document(), &Filter { material: Some(oak), ..Filter::default() }.matcher());
        assert_eq!(painted.used.len(), 1, "the part painted oak");
    }

    #[test]
    fn names_sort_as_read() {
        let mut names = vec!["Pièce#10", "pièce#2", "Assemblage#1", "Pièce#2", "Caisson", "Pièce#02", "Pièce", "étagère"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["Assemblage#1", "Caisson", "Pièce", "Pièce#2", "Pièce#02", "pièce#2", "Pièce#10", "étagère"]);
    }
}

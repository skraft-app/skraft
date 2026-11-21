//! The « Composants » card (docs/ui/components-library.md): the document's parts and assemblies
//! in a tab each, how many times each is in the model, the unused ones apart; a click chooses one (the entity
//! info shows it), its menu selects its occurrences, replaces the selection by it, renames or
//! deletes it; « Purger » drops the unused ones.

use std::cmp::Ordering;

use skraft_i18n::{t, tp};
use skraft_model::{Document, Id};
use skraft_tools::Editor;
use skraft_tools::search::Matcher;

use crate::icons;
use crate::panels::{Action, rename_field, tree_row, weak_share};
use crate::ribbon::Corners;
use crate::style::Tip;
use crate::thumbnails::Thumbnails;

/// What the card keeps of a document between frames, in the document itself (never on disk):
/// its search, its tab, its rows — made again when the document (its history's revision) or
/// the search changes.
#[derive(Default)]
pub struct LibraryState {
    pub search: String,
    pub tab: Tab,
    rows: Option<(u64, String, Shelves)>,
}

/// The card's tabs: the parts, the assemblies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Parts,
    Assemblies,
}

/// The components the search lets through, a tab's each.
#[derive(Default)]
struct Shelves {
    parts: Rows,
    assemblies: Rows,
}

impl Shelves {
    /// The components whose name holds what `matcher` was typed
    /// (docs/ui/components-library.md#search), by nature.
    fn of(doc: &Document, matcher: &Matcher) -> Shelves {
        let counts = doc.counts_in_model();
        let mut shelves = Shelves::default();
        for c in doc.components() {
            if !matcher.name([c.name.as_str()]) {
                continue;
            }
            let rows = if c.is_part() { &mut shelves.parts } else { &mut shelves.assemblies };
            match counts.get(&c.id) {
                Some(&n) => rows.used.push((c.id, n)),
                None => rows.unused.push(c.id),
            }
        }
        let name = |id: &Id| doc.component(*id).map(|c| c.name.as_str()).unwrap_or("");
        for rows in [&mut shelves.parts, &mut shelves.assemblies] {
            rows.used.sort_by(|a, b| natural_cmp(name(&a.0), name(&b.0)));
            rows.unused.sort_by(|a, b| natural_cmp(name(a), name(b)));
        }
        shelves
    }

    fn tab(&self, tab: Tab) -> &Rows {
        match tab {
            Tab::Parts => &self.parts,
            Tab::Assemblies => &self.assemblies,
        }
    }
}

/// A tab's components in their sections, each in natural order of names.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rows {
    /// In the model, with their count there.
    pub used: Vec<(Id, u64)>,
    /// Placed nowhere (named).
    pub unused: Vec<Id>,
}

/// One line of the card.
pub(crate) enum Line {
    Header { unused: bool },
    Item { id: Id, count: Option<u64> },
}

impl Rows {
    pub fn len(&self) -> usize {
        self.used.len() + self.unused.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The lines shown: the headers only when there are unused ones.
    pub fn lines(&self) -> Vec<Line> {
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

/// Orders names as a person reads them: runs of digits by their value, letters without regard
/// to case, a prefix first. Names alike by that measure are told apart at their first real
/// difference (the shorter spelling of a number, else plain `char` order), so that only
/// identical strings are `Equal`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut left, mut right) = (a, b);
    let mut tie = Ordering::Equal;
    loop {
        let (Some(x), Some(y)) = (left.chars().next(), right.chars().next()) else {
            return right.is_empty().cmp(&left.is_empty()).then(tie);
        };
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let (run_x, rest_x) = left.split_at(digit_run(left));
            let (run_y, rest_y) = right.split_at(digit_run(right));
            let (value_x, value_y) = (run_x.trim_start_matches('0'), run_y.trim_start_matches('0'));
            // Without leading zeros, more digits is a larger number; as many compare as text.
            let by_value = value_x.len().cmp(&value_y.len()).then_with(|| value_x.cmp(value_y));
            if by_value != Ordering::Equal {
                return by_value;
            }
            tie = tie.then(run_x.len().cmp(&run_y.len()));
            (left, right) = (rest_x, rest_y);
        } else {
            if x != y {
                let by_letter = x.to_lowercase().cmp(y.to_lowercase());
                if by_letter != Ordering::Equal {
                    return by_letter;
                }
                tie = tie.then(x.cmp(&y));
            }
            (left, right) = (&left[x.len_utf8()..], &right[y.len_utf8()..]);
        }
    }
}

/// How many ASCII digits `s` starts with.
fn digit_run(s: &str) -> usize {
    s.bytes().take_while(u8::is_ascii_digit).count()
}

/// A component's name in its row: its colour `ink`, an automatic one in italics, attenuated
/// unless the row is selected.
pub(crate) fn name_job(ui: &egui::Ui, name: &str, auto: bool, ink: egui::Color32, selected: bool) -> egui::text::LayoutJob {
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
/// grid, where its title bar ends.
pub struct Look<'a> {
    pub doc: u64,
    pub thumbnails: &'a mut Thumbnails,
    pub list: bool,
    /// The height the card gives its content: the rows above the list (the toggle, the note
    /// of a part open) take theirs from it, so the list ends at the card's bottom.
    pub height: f32,
    /// The bottom of the card's title bar, where the tabs' band goes on from it (no line
    /// between), and the card's inner margin, which the band spans to reach its edges.
    pub band_top: f32,
    pub margin: f32,
}

/// A list row's height, and its thumbnail's side (points).
pub(crate) const LIST_ROW: f32 = 30.0;
pub(crate) const LIST_THUMB: f32 = 26.0;
/// The room under a tile for its name (points), and between tiles.
pub(crate) const TILE_LABEL: f32 = 18.0;
pub(crate) const TILE_GAP: f32 = 6.0;

/// The card's content, in `area`: the tabs (parts, assemblies), the search and the grid / list
/// toggle, then the tab's components in their sections, as tiles of thumbnails or as rows (only
/// the rows in view laid out, only the thumbnails in view asked for: a model may hold thousands
/// of definitions).
pub fn components(
    ui: &mut egui::Ui,
    editor: &Editor,
    state: &mut LibraryState,
    look: Look<'_>,
    actions: &mut Vec<Action>,
    area: egui::ScrollArea,
) {
    let doc = editor.document();
    let top = ui.cursor().top();
    let revision = editor.history().revision();
    let matcher = Matcher::new(&state.search);
    if state.rows.as_ref().is_none_or(|(r, f, _)| *r != revision || *f != state.search) {
        state.rows = Some((revision, state.search.clone(), Shelves::of(doc, &matcher)));
    }
    let Some((_, _, shelves)) = &state.rows else { return };
    // The tabs, the ribbon's, their band from the title bar's line across the card; each with
    // what the search lets through, so a match in the other is seen.
    let labels = [
        t!("panel.components.parts_tab", count = shelves.parts.len()),
        t!("panel.components.assemblies_tab", count = shelves.assemblies.len()),
    ];
    let ids = [ui.make_persistent_id("components_tab_parts"), ui.make_persistent_id("components_tab_assemblies")];
    let m = look.margin as i8;
    let inset = egui::Margin { left: m, right: m, top: (ui.cursor().top() - look.band_top).max(0.0).ceil() as i8, bottom: 0 };
    let shown = match state.tab {
        Tab::Parts => 0,
        Tab::Assemblies => 1,
    };
    let handles = ["components:parts", "components:assemblies"];
    match crate::ribbon::tab_strip(ui, &labels, &ids, Some(shown), ui.available_width(), inset, Corners::Square, &handles) {
        Some(0) => state.tab = Tab::Parts,
        Some(_) => state.tab = Tab::Assemblies,
        None => {}
    }
    let gap = crate::search::GAP - ui.spacing().item_spacing.y;
    ui.add_space(gap);
    // The search (docs/ui/components-library.md#search): the field left of the grid / list
    // toggle, as far from it as from the tabs above and the list below (`search::GAP`).
    let salt = ui.make_persistent_id("components_search");
    crate::search::field(ui, salt, "components", &mut state.search, |ui, side| view_toggle(ui, side, look.list, "components", actions));
    ui.add_space(gap);
    let rows = shelves.tab(state.tab);
    let disabled = editor.library_disabled();
    if disabled {
        ui.add(egui::Label::new(egui::RichText::new(t!("panel.components.disabled")).weak()).wrap());
    }
    if rows.is_empty() {
        let none = match state.tab {
            _ if matcher.is_active() => t!("search.no_match"),
            Tab::Parts => t!("panel.components.no_parts"),
            Tab::Assemblies => t!("panel.components.no_assemblies"),
        };
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

/// The grid / list toggle, at the right end of the search row: the option bar's segmented
/// control, of the row's height.
pub(crate) fn view_toggle(ui: &mut egui::Ui, side: f32, list: bool, card: &str, actions: &mut Vec<Action>) {
    use crate::panels::Segment;
    let segments = [
        Segment::image(&t!("panel.components.as_grid"), "", icons::TILES, !list),
        Segment::image(&t!("panel.components.as_list"), "", icons::LIST, list),
    ];
    let id = ui.make_persistent_id((card, "view"));
    let handle = |i: usize| format!("{card}:{}", ["grid", "list"][i]);
    if let Some(i) = crate::panels::segmented_sized(ui, id, &segments, (side, 14.0), handle) {
        actions.push(Action::ComponentsList(i == 1));
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
pub(crate) fn count_badge(ui: &egui::Ui, square: egui::Rect, n: u64, weak: egui::Color32) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(format!("×{n}"), font, weak);
    let pad = egui::vec2(4.0, 1.0);
    let min = egui::pos2(square.max.x - galley.size().x - 2.0 * pad.x - 3.0, square.min.y + 3.0);
    let r = egui::Rect::from_min_size(min, galley.size() + 2.0 * pad);
    ui.painter().rect_filled(r, r.height() / 2.0, ui.visuals().window_fill.gamma_multiply(0.85));
    ui.painter().galley(r.min + pad, galley, weak);
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
    let tip =
        if cx.editor.on_active_path(id) { format!("{name}\n{tip}\n{}", t!("panel.components.on_path")) } else { format!("{name}\n{tip}") };
    // Dragged out of the card too (docs/ui/components-library.md#placing).
    let response = ui.interact(response.rect, response.id, egui::Sense::click_and_drag());
    let response = if response.dragged() { response } else { response.tip(tip) };
    if cx.disabled {
        return;
    }
    // Renamed in place: from the menu, by a double-click on its row or tile (the first click
    // only chooses it), or F2 while it is the one chosen. A component's name is never
    // empty, and unique (one taken refused, the failure told).
    let key = egui::Id::unique(crate::panels::RENAMING);
    let f2 = || cx.chosen == Some(id) && ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(egui::Key::F2));
    if renamed.is_some() {
        ui.data_mut(|d| d.remove::<egui::Id>(key));
    } else if !renaming && (response.double_clicked() || f2()) {
        ui.data_mut(|d| d.insert_temp(key, rename));
    }
    if let Some(Some(name)) = renamed
        && !name.is_empty()
    {
        actions.push(Action::Apply(skraft_model::Operation::RenameComponent { component: id, name }));
    }
    // A click chooses it; a drag out of the card carries one more occurrence of it at the
    // cursor (chosen meanwhile), but of an assembly along the active path (it would hold
    // itself).
    let place = cx.editor.can_place_component(id);
    if response.drag_started_by(egui::PointerButton::Primary) && place {
        actions.push(Action::PlaceComponent(id));
    } else if response.clicked() {
        actions.push(Action::ChooseComponent(id));
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
            if purge.tip(t!("panel.components.purge_tip")).clicked() {
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
        if delete.tip(t!("panel.components.delete_tip")).clicked() {
            actions.push(Action::DeleteComponent(id));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_lists_what_the_search_lets_through_in_its_tabs() {
        use glam::{DAffine3, DVec3};
        use skraft_model::{Operation, Primitive};
        let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
        let root = e.document().root();
        let mut ids = Vec::new();
        for x in [0.0, 100.0] {
            let p = Primitive::Box { origin: DVec3::new(x, 0.0, 0.0), size: DVec3::splat(10.0) };
            ids.push(
                e.apply(Operation::AddPrimitive { context: root, primitive: p, placement: DAffine3::IDENTITY, material: None })
                    .unwrap()
                    .unwrap()
                    .created[0],
            );
        }
        e.apply(Operation::MakeAssembly { context: root, entities: vec![ids[0]], name: Some("Caisson".into()) }).unwrap();
        let all = Shelves::of(e.document(), &Matcher::default());
        assert_eq!((all.parts.used.len(), all.assemblies.used.len()), (2, 1), "a tab each");
        assert!(all.parts.used.iter().all(|(id, _)| e.document().part(*id).is_ok()));
        let named = Shelves::of(e.document(), &Matcher::new("CAIS"));
        assert_eq!((named.parts.len(), named.assemblies.len()), (0, 1));
    }

    #[test]
    fn names_sort_as_read() {
        let mut names = vec!["Pièce#10", "pièce#2", "Assemblage#1", "Pièce#2", "Caisson", "Pièce#02", "Pièce", "étagère"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["Assemblage#1", "Caisson", "Pièce", "Pièce#2", "Pièce#02", "pièce#2", "Pièce#10", "étagère"]);
    }

    #[test]
    fn natural_order_cases() {
        use Ordering::{Equal, Greater, Less};
        // Runs by value, whatever their length.
        assert_eq!(natural_cmp("a9", "a10"), Less);
        let long = "9".repeat(60);
        assert_eq!(natural_cmp(&long, &format!("1{long}")), Less);
        assert_eq!(natural_cmp(&format!("000{long}"), &format!("1{long}")), Less);
        // Case aside, then a prefix first, before any difference of case or zeros.
        assert_eq!(natural_cmp("abc", "ABD"), Less);
        assert_eq!(natural_cmp("ÉA", "éb"), Less);
        assert_eq!(natural_cmp("AB", "abc"), Less);
        assert_eq!(natural_cmp("a01", "A1b"), Less);
        // Equal so far: the first real difference decides.
        assert_eq!(natural_cmp("a1", "a01"), Less);
        assert_eq!(natural_cmp("a01", "a1"), Greater);
        assert_eq!(natural_cmp("A", "a"), Less);
        assert_eq!(natural_cmp("a1B", "A01b"), Greater, "a after A, at the first place");
        assert_eq!(natural_cmp("x01Y", "x1y"), Greater, "the zeros come first");
        // Only identical strings are equal.
        assert_eq!(natural_cmp("Pièce#02", "Pièce#02"), Equal);
        assert_eq!(natural_cmp("", ""), Equal);
        assert_eq!(natural_cmp("", "a"), Less);
    }
}

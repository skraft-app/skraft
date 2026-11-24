//! What the cards listing a document's definitions share (docs/ui/catalog.md): the « Composants »,
//! « Compositions » and « Matières » cards show their items as tiles or rows, in two sections,
//! renamed in place. Each card says what its items are through a [`Shelf`].

use skraft_i18n::t;
use skraft_model::Id;

use crate::kit::icons;
use crate::kit::widgets::{rename_field, tree_row};
use crate::panels::{Action, weak_share};

/// A list row's height, and its picture's side (points).
pub(crate) const LIST_ROW: f32 = 30.0;
pub(crate) const LIST_THUMB: f32 = 26.0;
/// The room under a tile for its name (points), and between tiles.
pub(crate) const TILE_LABEL: f32 = 18.0;
pub(crate) const TILE_GAP: f32 = 6.0;

/// The cards with a grid / list toggle, each with its own preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    Components,
    Compositions,
    Materials,
}

impl Card {
    /// Its handles' prefix (docs/control-protocol.md).
    fn handle(self) -> &'static str {
        match self {
            Card::Components => "components",
            Card::Compositions => "compositions",
            Card::Materials => "materials",
        }
    }
}

/// A card's items in their sections, each in natural order of names.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rows {
    /// In use, with their count.
    pub used: Vec<(Id, u64)>,
    /// In use nowhere.
    pub unused: Vec<Id>,
}

/// One line of a card as a list.
#[derive(Clone, Copy)]
enum Line {
    Header { unused: bool },
    Lead,
    Item { id: Id, count: Option<u64> },
}

impl Rows {
    pub fn len(&self) -> usize {
        self.used.len() + self.unused.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The lines shown: the card's first item when it has one, the headers only when there are
    /// unused ones.
    fn lines(&self, lead: bool) -> Vec<Line> {
        let mut lines = Vec::with_capacity(self.len() + 3);
        if lead {
            lines.push(Line::Lead);
        }
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

/// The colours an item is drawn in.
pub(crate) struct Ink {
    pub selected: bool,
    pub greyed: bool,
    pub text: egui::Color32,
    pub weak: egui::Color32,
}

impl Ink {
    /// As the theme has them: the selection's colour, else weak when `greyed`.
    pub fn new(ui: &egui::Ui, selected: bool, greyed: bool) -> Ink {
        let v = ui.visuals();
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

/// An item's rename in place: its key, whether it is under way, what its field gave this frame.
pub(crate) struct Renaming {
    pub key: egui::Id,
    pub active: bool,
    pub renamed: Option<Option<String>>,
}

impl Renaming {
    fn of(ui: &egui::Ui, salt: &'static str, id: Id) -> Renaming {
        let key = ui.make_persistent_id((salt, id));
        let active = ui.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(crate::kit::widgets::RENAMING))) == Some(key);
        Renaming { key, active, renamed: None }
    }

    /// Starts it (a menu's « Renommer »).
    pub fn start(ui: &egui::Ui, key: egui::Id) {
        ui.data_mut(|d| d.insert_temp(egui::Id::unique(crate::kit::widgets::RENAMING), key));
    }

    /// Its gestures: ended by its field, started by a double-click or by F2 when `f2` (the item
    /// alone chosen). The name to apply, when one was typed (never empty).
    pub fn gesture(self, ui: &egui::Ui, response: &egui::Response, f2: bool) -> Option<String> {
        let key = egui::Id::unique(crate::kit::widgets::RENAMING);
        let f2 = || f2 && ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(egui::Key::F2));
        if self.renamed.is_some() {
            ui.data_mut(|d| d.remove::<egui::Id>(key));
        } else if !self.active && (response.double_clicked() || f2()) {
            Renaming::start(ui, self.key);
        }
        self.renamed.flatten().filter(|name| !name.is_empty())
    }
}

/// What a card lists.
pub(crate) trait Shelf {
    /// The key its items' renames are kept under.
    fn rename_salt(&self) -> &'static str;
    /// Is `id` still there? Asked for every item, in view or not: cheap.
    fn has(&self, id: Id) -> bool;
    /// The name of `id`, and whether it is an automatic one (italic, attenuated).
    fn name(&self, id: Id) -> (String, bool);
    fn ink(&mut self, ui: &egui::Ui, id: Id) -> Ink;
    /// Draws the picture of `id` in `rect`.
    fn picture(&mut self, ui: &mut egui::Ui, rect: egui::Rect, id: Id, ink: &Ink);
    /// The item's interaction, a row's or a tile's: its tip, rename, click or drag, menu.
    fn interact(
        &mut self,
        ui: &mut egui::Ui,
        id: Id,
        count: Option<u64>,
        response: egui::Response,
        rename: Renaming,
        actions: &mut Vec<Action>,
    );
    /// A section's header.
    fn header(&mut self, ui: &mut egui::Ui, height: f32, unused: bool, actions: &mut Vec<Action>);

    /// The name of an item standing before the others, not one of the document's (« Aucune »).
    fn lead(&self) -> Option<String> {
        None
    }
    fn lead_ink(&mut self, ui: &egui::Ui) -> Ink {
        Ink::new(ui, false, false)
    }
    fn lead_picture(&mut self, _ui: &mut egui::Ui, _rect: egui::Rect, _ink: &Ink) {}
    fn lead_interact(&mut self, _ui: &mut egui::Ui, _response: egui::Response, _actions: &mut Vec<Action>) {}
}

/// `area` as high as what is left of a card's `height`, from `top` (where its content starts)
/// down to the cursor.
pub(crate) fn fit(ui: &egui::Ui, area: egui::ScrollArea, height: f32, top: f32) -> egui::ScrollArea {
    let room = (height - (ui.cursor().top() - top)).max(0.0);
    area.max_height(room).min_scrolled_height(room)
}

/// The items of `rows`, as a list or as a grid, in `area`: only the rows in view are laid out,
/// only the tiles in view drawn (a model may hold thousands of definitions).
pub(crate) fn show(
    ui: &mut egui::Ui,
    shelf: &mut impl Shelf,
    rows: &Rows,
    as_list: bool,
    actions: &mut Vec<Action>,
    area: egui::ScrollArea,
) {
    if as_list {
        list(ui, shelf, rows, actions, area);
    } else {
        grid(ui, shelf, rows, actions, area);
    }
}

fn list<S: Shelf>(ui: &mut egui::Ui, shelf: &mut S, rows: &Rows, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let lines = rows.lines(shelf.lead().is_some());
    area.show_rows(ui, LIST_ROW, lines.len(), |ui, range| {
        ui.set_min_width(ui.available_width());
        for line in &lines[range] {
            match *line {
                Line::Header { unused } => shelf.header(ui, LIST_ROW, unused, actions),
                Line::Lead => {
                    let name = shelf.lead().unwrap_or_default();
                    let ink = shelf.lead_ink(ui);
                    let (response, _) = tree_row(ui, LIST_ROW, ink.selected, false, false, |ui| {
                        let (square, _) = ui.allocate_exact_size(egui::Vec2::splat(LIST_THUMB), egui::Sense::hover());
                        shelf.lead_picture(ui, square, &ink);
                        row_text(ui, &name, false, None, &ink);
                    });
                    shelf.lead_interact(ui, response, actions);
                }
                Line::Item { id, count } => {
                    if !shelf.has(id) {
                        continue;
                    }
                    let (name, auto) = shelf.name(id);
                    let ink = shelf.ink(ui, id);
                    let mut rename = Renaming::of(ui, shelf.rename_salt(), id);
                    let (key, active) = (rename.key, rename.active);
                    let (response, renamed) = tree_row(ui, LIST_ROW, ink.selected, false, false, |ui| {
                        let (square, _) = ui.allocate_exact_size(egui::Vec2::splat(LIST_THUMB), egui::Sense::hover());
                        shelf.picture(ui, square, id, &ink);
                        if active {
                            return rename_field(ui, key, &name);
                        }
                        row_text(ui, &name, auto, count, &ink);
                        None
                    });
                    rename.renamed = renamed;
                    shelf.interact(ui, id, count, response, rename, actions);
                }
            }
        }
    });
}

/// A row's name and, at its right end, its count.
fn row_text(ui: &mut egui::Ui, name: &str, auto: bool, count: Option<u64>, ink: &Ink) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(n) = count {
            ui.add(egui::Label::new(egui::RichText::new(format!("×{n}")).color(ink.weak)).selectable(false));
        }
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(name_job(ui, name, auto, ink.text, ink.selected)).selectable(false).truncate());
        });
    });
}

/// A cell of the grid.
#[derive(Clone, Copy)]
enum Cell {
    Lead,
    Item(Id, Option<u64>),
}

fn grid<S: Shelf>(ui: &mut egui::Ui, shelf: &mut S, rows: &Rows, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let header_height = ui.spacing().interact_size.y;
    let apart = !rows.unused.is_empty();
    let lead = shelf.lead().is_some();
    area.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        let tile = crate::thumbnails::TILE;
        let columns = (((ui.available_width() + TILE_GAP) / (tile + TILE_GAP)).floor() as usize).max(1);
        let mut first: Vec<Cell> = Vec::with_capacity(rows.used.len() + 1);
        if lead {
            first.push(Cell::Lead);
        }
        first.extend(rows.used.iter().map(|&(id, n)| Cell::Item(id, Some(n))));
        let second: Vec<Cell> = rows.unused.iter().map(|&id| Cell::Item(id, None)).collect();
        for (unused, cells) in [(false, first), (true, second)] {
            if cells.is_empty() {
                continue;
            }
            // No « in use » header over the lead alone.
            if apart && (unused || !rows.used.is_empty()) {
                shelf.header(ui, header_height, unused, actions);
            }
            for chunk in cells.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = TILE_GAP;
                    for &cell in chunk {
                        match cell {
                            Cell::Lead => {
                                let (rect, response) = ui.allocate_exact_size(egui::vec2(tile, tile + TILE_LABEL), egui::Sense::click());
                                if ui.is_rect_visible(rect) {
                                    let name = shelf.lead().unwrap_or_default();
                                    let ink = shelf.lead_ink(ui);
                                    let inert = Renaming { key: response.id, active: false, renamed: None };
                                    tile_at(ui, rect, &response, &ink, None, &inert, (&name, false), |ui, square| {
                                        shelf.lead_picture(ui, square, &ink)
                                    });
                                }
                                shelf.lead_interact(ui, response, actions);
                            }
                            Cell::Item(id, count) => {
                                if !shelf.has(id) {
                                    continue;
                                }
                                let (rect, response) =
                                    ui.allocate_exact_size(egui::vec2(tile, tile + TILE_LABEL), egui::Sense::click_and_drag());
                                let mut rename = Renaming::of(ui, shelf.rename_salt(), id);
                                if ui.is_rect_visible(rect) {
                                    let (name, auto) = shelf.name(id);
                                    let ink = shelf.ink(ui, id);
                                    rename.renamed = tile_at(ui, rect, &response, &ink, count, &rename, (&name, auto), |ui, square| {
                                        shelf.picture(ui, square, id, &ink)
                                    });
                                }
                                shelf.interact(ui, id, count, response, rename, actions);
                            }
                        }
                    }
                });
                ui.add_space(TILE_GAP / 2.0);
            }
        }
    });
}

/// A tile in `rect`: its picture, its outline, its count over a corner, its name (or its rename
/// field) under it. What the rename field gave.
#[allow(clippy::too_many_arguments)]
fn tile_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    ink: &Ink,
    count: Option<u64>,
    rename: &Renaming,
    (name, auto): (&str, bool),
    picture: impl FnOnce(&mut egui::Ui, egui::Rect),
) -> Option<Option<String>> {
    let tile = rect.width();
    let square = egui::Rect::from_min_size(rect.min, egui::Vec2::splat(tile));
    picture(ui, square);
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
    if rename.active {
        return rename_field(&mut under, rename.key, name);
    }
    let small = egui::TextStyle::Small.resolve(under.style()).size;
    let mut job = name_job(&under, name, auto, ink.text, ink.selected);
    job.sections.iter_mut().for_each(|s| s.format.font_id.size = small);
    job.wrap = egui::text::TextWrapping::truncate_at_width(label.width());
    under.add(egui::Label::new(job).selectable(false));
    None
}

/// An item's name in its row: its colour `ink`, an automatic one in italics, attenuated unless
/// the row is selected.
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

/// « ×n » over a tile's top right corner, on a pill of the card's colour.
fn count_badge(ui: &egui::Ui, square: egui::Rect, n: u64, weak: egui::Color32) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(format!("×{n}"), font, weak);
    let pad = egui::vec2(4.0, 1.0);
    let min = egui::pos2(square.max.x - galley.size().x - 2.0 * pad.x - 3.0, square.min.y + 3.0);
    let r = egui::Rect::from_min_size(min, galley.size() + 2.0 * pad);
    ui.painter().rect_filled(r, r.height() / 2.0, ui.visuals().window_fill.gamma_multiply(0.85));
    ui.painter().galley(r.min + pad, galley, weak);
}

/// The grid / list toggle, at the right end of the search row: the option bar's segmented
/// control, of the row's height.
pub(crate) fn view_toggle(ui: &mut egui::Ui, side: f32, list: bool, card: Card, actions: &mut Vec<Action>) {
    use crate::kit::widgets::Segment;
    let segments = [
        Segment::image(&t!("panel.components.as_grid"), "", icons::TILES, !list),
        Segment::image(&t!("panel.components.as_list"), "", icons::LIST, list),
    ];
    let handle = card.handle();
    let id = ui.make_persistent_id((handle, "view"));
    let name = |i: usize| format!("{handle}:{}", ["grid", "list"][i]);
    if let Some(i) = crate::kit::widgets::segmented_sized(ui, id, &segments, (side, 14.0), name) {
        actions.push(Action::CardList(card, i == 1));
    }
}

/// A section's header: its `title`, and `purge`'s button (enabled, its tip, its handle) when it
/// has one. Whether the button was clicked.
pub(crate) fn header(ui: &mut egui::Ui, height: f32, title: String, purge: Option<(bool, String, &'static str)>) -> bool {
    use crate::kit::style::Tip;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    let font = crate::kit::style::semibold(egui::TextStyle::Small.resolve(row.style()).size);
    row.add(egui::Label::new(egui::RichText::new(title).font(font).color(row.visuals().weak_text_color())).selectable(false));
    let Some((enabled, tip, handle)) = purge else { return false };
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let button = ui.add_enabled(enabled, egui::Button::new(t!("panel.components.purge")).small());
        crate::kit::handles::publish(|| handle.into(), button.rect);
        button.tip(tip).clicked()
    })
    .inner
}

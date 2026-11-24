//! The library outside a sheet, the components (docs/ui/components-library.md): the
//! document's parts and assemblies in a tab each, how many times each is in the model, the
//! unused ones apart; a click chooses one (the entity info shows it), its menu selects its
//! occurrences, replaces the selection by it, renames or deletes it; « Purger » drops the
//! unused ones.

use std::cmp::Ordering;

use skraft_i18n::{t, tp};
use skraft_model::{Document, Id};
use skraft_tools::Editor;
use skraft_tools::search::Matcher;

use crate::commands::Action;
use crate::kit::icons;
use crate::kit::style::Tip;
use crate::system::thumbnails::Thumbnails;
use crate::window::ribbon::Corners;
use crate::workspaces::common::cards::catalog::{self, Card, Ink, Renaming, Rows, Shelf};

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
    let matcher = Matcher::new(&state.search);
    let shelves = crate::kit::search::listed(&mut state.rows, editor.history().revision(), &state.search, || Shelves::of(doc, &matcher));
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
    match crate::window::ribbon::tab_strip(ui, &labels, &ids, Some(shown), ui.available_width(), inset, Corners::Square, &handles) {
        Some(0) => state.tab = Tab::Parts,
        Some(_) => state.tab = Tab::Assemblies,
        None => {}
    }
    let gap = crate::kit::search::GAP - ui.spacing().item_spacing.y;
    ui.add_space(gap);
    // The search (docs/ui/components-library.md#search): the field left of the grid / list
    // toggle, as far from it as from the tabs above and the list below (`search::GAP`).
    let salt = ui.make_persistent_id("components_search");
    crate::kit::search::field(ui, salt, "components", &mut state.search, |ui, side| {
        catalog::view_toggle(ui, side, look.list, Card::Components, actions)
    });
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
    let thumbs = Thumbs {
        doc: look.doc,
        thumbnails: look.thumbnails,
        dark: ui.visuals().dark_mode,
        px: ((crate::system::thumbnails::TILE * ppp).round() as u32).clamp(16, 512),
    };
    let mut shelf = Components { editor, chosen: editor.chosen_component(), disabled, thumbs };
    let area = catalog::fit(ui, area, look.height, top);
    catalog::show(ui, &mut shelf, rows, look.list, actions, area);
    if shelf.thumbs.thumbnails.pending() {
        ui.ctx().request_repaint();
    }
}

/// The card's items, for the shared presentation.
struct Components<'a> {
    editor: &'a Editor,
    chosen: Option<Id>,
    disabled: bool,
    thumbs: Thumbs<'a>,
}

impl Shelf for Components<'_> {
    fn rename_salt(&self) -> &'static str {
        "components_rename"
    }

    fn has(&self, id: Id) -> bool {
        self.editor.document().component(id).is_ok()
    }

    fn name(&self, id: Id) -> (String, bool) {
        self.editor.document().component(id).map_or_else(|_| (String::new(), false), |c| (c.name.clone(), !c.named))
    }

    /// Greyed: the whole card while a part is open, the assemblies along the active path.
    fn ink(&mut self, ui: &egui::Ui, id: Id) -> Ink {
        Ink::new(ui, self.chosen == Some(id), self.disabled || self.editor.on_active_path(id))
    }

    fn picture(&mut self, ui: &mut egui::Ui, rect: egui::Rect, id: Id, ink: &Ink) {
        let thumb = self.thumbs.get(self.editor, id);
        let part = self.editor.document().component(id).is_ok_and(|c| c.is_part());
        picture(ui, rect, thumb, part, ink);
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
        item(ui, self, id, count, response, rename, actions);
    }

    fn header(&mut self, ui: &mut egui::Ui, height: f32, unused: bool, actions: &mut Vec<Action>) {
        let title = if unused { t!("panel.components.unused") } else { t!("panel.components.in_model") };
        let purge = unused.then(|| (!self.disabled, t!("panel.components.purge_tip"), "components:purge"));
        if catalog::header(ui, height, title, purge) {
            actions.push(Action::Command("components.purge"));
        }
    }
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

/// A thumbnail in the square `rect` (rounded), else the nature's icon in its middle; faded when
/// greyed.
fn picture(ui: &mut egui::Ui, rect: egui::Rect, thumb: Option<egui::TextureId>, part: bool, ink: &Ink) {
    let size = rect.size();
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

/// An item's interaction, a row's or a tile's: its tip, its rename, its click or drag (placing
/// it), its menu.
fn item(
    ui: &mut egui::Ui,
    cx: &Components<'_>,
    id: Id,
    count: Option<u64>,
    response: egui::Response,
    rename: Renaming,
    actions: &mut Vec<Action>,
) {
    crate::kit::handles::publish(|| format!("components:{}", id.0), response.rect);
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
    let key = rename.key;
    if let Some(name) = rename.gesture(ui, &response, cx.chosen == Some(id)) {
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
    menu(cx.editor, &response, id, key, actions);
}

/// A component's menu (right click): its occurrences selected, the selection replaced by it,
/// renamed, deleted. The selection is kept: it is what « Remplacer la sélection » takes.
fn menu(editor: &Editor, response: &egui::Response, id: Id, rename: egui::Id, actions: &mut Vec<Action>) {
    egui::Popup::context_menu(response).style(crate::kit::style::menu).show(|ui| {
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
            Renaming::start(ui, rename);
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

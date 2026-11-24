//! The dialog choosing what a view shows (docs/ui/drawing.md#choosing-what-a-view-shows, decision
//! 0051): the whole model, or occurrences ticked in the model's tree.

use std::collections::HashSet;

use skraft_i18n::t;
use skraft_model::{ComponentKind, Document, EntityPath, Id, Subject};
use skraft_tools::Editor;
use skraft_tools::search::Matcher;

use crate::catalog::{LIST_ROW, LIST_THUMB};
use crate::kit::widgets::{Confirm, Tick, confirm_buttons, tick, tree_row};

/// The list's height and the dialog's width (points); a level's indent.
const LIST_HEIGHT: f32 = 340.0;
const WIDTH: f32 = 440.0;
const INDENT: f32 = 16.0;
/// Deepest level of the tree walked (a guard, as the document's own walks).
const MAX_DEPTH: usize = 64;

/// What the choice is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// A view to place.
    New,
    /// This view's content.
    Change(Id),
}

/// What the user did in the dialog this frame.
pub enum Answer {
    Open,
    Cancel,
    Done(Subject),
}

/// A line of the tree.
#[derive(Clone, Debug)]
struct Row {
    path: EntityPath,
    name: String,
    component: Id,
    depth: usize,
    folder: bool,
}

/// The dialog's state (never saved).
#[derive(Clone, Debug)]
pub struct ViewPicker {
    pub purpose: Purpose,
    /// What it was opened on: kept as it is unless the user changes the ticks.
    original: Subject,
    checked: Vec<EntityPath>,
    touched: bool,
    search: String,
    unfolded: HashSet<Vec<Id>>,
    rows: Option<(u64, String, Vec<Row>)>,
}

/// A path's occurrences from the root, its own last.
fn chain(p: &EntityPath) -> Vec<Id> {
    p.instances.iter().copied().chain([p.entity]).collect()
}

/// The first place component `target` stands under `component`.
pub(crate) fn first_path(doc: &Document, component: Id, target: Id, prefix: &mut Vec<Id>) -> Option<EntityPath> {
    if prefix.len() >= MAX_DEPTH {
        return None;
    }
    let ComponentKind::Assembly(a) = &doc.component(component).ok()?.kind else { return None };
    for o in a.occurrences() {
        if o.component == target {
            return Some(EntityPath::new(prefix.clone(), o.id));
        }
        prefix.push(o.id);
        let found = first_path(doc, o.component, target, prefix);
        prefix.pop();
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The rows of what stands under `component`: the unfolded ones, or all those matching a search,
/// flat.
fn walk(doc: &Document, component: Id, unfolded: &HashSet<Vec<Id>>, m: &Matcher, prefix: &mut Vec<Id>, out: &mut Vec<Row>) {
    if prefix.len() >= MAX_DEPTH {
        return;
    }
    let Ok(ComponentKind::Assembly(a)) = doc.component(component).map(|c| &c.kind) else { return };
    for o in a.occurrences().iter().filter(|o| !o.hidden) {
        let Ok(c) = doc.component(o.component) else { continue };
        let folder = matches!(&c.kind, ComponentKind::Assembly(s) if !s.occurrences().is_empty());
        let name = if o.name.is_empty() { c.name.clone() } else { format!("{} ({})", o.name, c.name) };
        if m.name([name.as_str()]) {
            let depth = if m.is_active() { 0 } else { prefix.len() };
            out.push(Row { path: EntityPath::new(prefix.clone(), o.id), name, component: o.component, depth, folder });
        }
        prefix.push(o.id);
        if folder && (m.is_active() || unfolded.contains(prefix.as_slice())) {
            walk(doc, o.component, unfolded, m, prefix, out);
        }
        prefix.pop();
    }
}

/// What ticking `paths` shows: none, the model; one occurrence, its component in its own axes;
/// several, where they stand.
pub fn subject_of_paths(doc: &Document, paths: &[EntityPath]) -> Subject {
    match paths {
        [] => Subject::Model,
        [one] => doc.resolve(one).map_or(Subject::Model, |o| Subject::Component(o.component)),
        _ => Subject::Occurrences(paths.to_vec()),
    }
}

impl ViewPicker {
    /// Opened on `subject`, what it shows ticked and in sight.
    pub fn new(doc: &Document, purpose: Purpose, subject: Subject) -> ViewPicker {
        let checked = match &subject {
            Subject::Model => Vec::new(),
            Subject::Component(c) => first_path(doc, doc.root(), *c, &mut Vec::new()).into_iter().collect(),
            Subject::Occurrences(paths) => paths.clone(),
        };
        let mut unfolded = HashSet::new();
        for p in &checked {
            for n in 1..=p.instances.len() {
                unfolded.insert(p.instances[..n].to_vec());
            }
        }
        ViewPicker { purpose, original: subject, checked, touched: false, search: String::new(), unfolded, rows: None }
    }

    fn tick(&self, path: &EntityPath) -> Tick {
        let c = chain(path);
        if self.checked.iter().any(|p| chain(p) == c) {
            Tick::Yes
        } else if self.checked.iter().any(|p| c.starts_with(&chain(p))) {
            Tick::Covered
        } else {
            Tick::No
        }
    }

    /// Ticks or unticks a row; what it covers is no longer ticked apart.
    fn toggle(&mut self, path: &EntityPath) {
        match self.tick(path) {
            Tick::Covered => return,
            Tick::Yes => {
                let c = chain(path);
                self.checked.retain(|p| chain(p) != c);
            }
            Tick::No => {
                let c = chain(path);
                self.checked.retain(|p| !chain(p).starts_with(&c));
                self.checked.push(path.part_path());
            }
        }
        self.touched = true;
    }

    fn whole_model(&mut self) {
        self.checked.clear();
        self.touched = true;
    }

    fn subject(&self, doc: &Document) -> Subject {
        if self.touched { subject_of_paths(doc, &self.checked) } else { self.original.clone() }
    }

    /// The dialog, this frame.
    pub fn show(&mut self, ctx: &egui::Context, e: &Editor, thumbnails: &mut crate::thumbnails::Thumbnails, doc_id: u64) -> Answer {
        let doc = e.document();
        let typing = ctx.memory(|m| m.focused().is_some());
        let revision = e.history().revision();
        let mut answer = Answer::Open;
        let title = match self.purpose {
            Purpose::New => t!("drawing.picker.title_new"),
            Purpose::Change(_) => t!("drawing.picker.title_change"),
        };
        let modal = crate::app::modal(ctx, egui::Id::unique("view_picker"), WIDTH, title, |ui| {
            let salt = ui.make_persistent_id("view_picker_search");
            crate::kit::search::field(ui, salt, "view_picker", &mut self.search, |_, _| {});
            ui.add_space(6.0);
            let rows = crate::kit::search::listed(&mut self.rows, revision, &self.search, || {
                let mut rows = Vec::new();
                walk(doc, doc.root(), &self.unfolded, &Matcher::new(&self.search), &mut Vec::new(), &mut rows);
                rows
            })
            .clone();
            let searching = !self.search.trim().is_empty();
            // The whole model, above the tree.
            let (whole, ticked) = tree_row(ui, LIST_ROW, self.checked.is_empty(), false, false, |ui| {
                let ticked = tick(ui, if self.checked.is_empty() { Tick::Yes } else { Tick::No }).clicked();
                ui.add(egui::Label::new(t!("drawing.subject.model")).selectable(false));
                ticked
            });
            crate::kit::handles::publish(|| "view_picker.model".into(), whole.rect);
            if whole.clicked() || ticked {
                self.whole_model();
            }
            ui.separator();
            let (mut toggled, mut folded) = (None, None);
            // The « Composants » card's size: one thumbnail each, drawn once.
            let px = ((crate::thumbnails::TILE * ctx.pixels_per_point()).round() as u32).clamp(16, 512);
            let dark = ui.visuals().dark_mode;
            egui::ScrollArea::vertical()
                .id_salt("view_picker_rows")
                .max_height(LIST_HEIGHT)
                .min_scrolled_height(LIST_HEIGHT)
                .auto_shrink(false)
                .show_rows(ui, LIST_ROW, rows.len(), |ui, range| {
                    ui.set_min_width(ui.available_width());
                    for row in &rows[range] {
                        let ticks = self.tick(&row.path);
                        let key = chain(&row.path);
                        let open = self.unfolded.contains(&key);
                        let (response, (fold, ticked)) = tree_row(ui, LIST_ROW, ticks == Tick::Yes, ticks == Tick::Covered, false, |ui| {
                            ui.add_space(row.depth as f32 * INDENT);
                            let (square, chevron) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::click());
                            if row.folder && !searching {
                                let angle = if open { 0.0 } else { -std::f32::consts::FRAC_PI_2 };
                                crate::kit::icons::chevron(ui, square.center(), 12.0, ui.visuals().weak_text_color(), angle);
                            }
                            let ticked = tick(ui, ticks).clicked();
                            let (thumb, _) = ui.allocate_exact_size(egui::Vec2::splat(LIST_THUMB), egui::Sense::hover());
                            if let Some(texture) = thumbnails.get(doc_id, e, row.component, dark, px) {
                                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                                ui.painter().image(texture, thumb, uv, egui::Color32::WHITE);
                            }
                            ui.add(egui::Label::new(&row.name).selectable(false).truncate());
                            (row.folder && !searching && chevron.clicked(), ticked)
                        });
                        crate::kit::handles::publish(|| format!("view_picker.row:{}", row.path.entity.0), response.rect);
                        if fold {
                            folded = Some(key);
                        } else if response.clicked() || ticked {
                            toggled = Some(row.path.clone());
                        }
                    }
                });
            if let Some(path) = toggled {
                self.toggle(&path);
            }
            if let Some(key) = folded {
                if !self.unfolded.remove(&key) {
                    self.unfolded.insert(key);
                }
                self.rows = None;
            }
            ui.add_space(12.0);
            let label = match self.purpose {
                Purpose::New => t!("drawing.picker.place"),
                Purpose::Change(_) => t!("drawing.picker.apply"),
            };
            ui.horizontal(|ui| match confirm_buttons(ui, "view_picker", label, true, t!("naming.cancel")) {
                Some(Confirm::Ok) => answer = Answer::Done(self.subject(doc)),
                Some(Confirm::Cancel) => answer = Answer::Cancel,
                None => {}
            });
        });
        if thumbnails.pending() {
            ctx.request_repaint();
        }
        if matches!(answer, Answer::Open) && !typing && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            answer = Answer::Done(self.subject(doc));
        }
        if matches!(answer, Answer::Open) && modal.should_close() {
            answer = Answer::Cancel;
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(chain: &[u64]) -> EntityPath {
        let (last, first) = chain.split_last().unwrap();
        EntityPath::new(first.iter().map(|i| Id(*i)).collect(), Id(*last))
    }

    #[test]
    fn a_row_ticked_takes_in_what_it_holds_and_covers_it() {
        let mut p = ViewPicker::new(&Document::new(), Purpose::New, Subject::Model);
        p.toggle(&path(&[1, 2]));
        p.toggle(&path(&[1]));
        assert_eq!(p.checked, vec![path(&[1])], "the child is held by its parent now");
        assert_eq!(p.tick(&path(&[1, 2])), Tick::Covered);
        p.toggle(&path(&[1, 2]));
        assert_eq!(p.checked, vec![path(&[1])], "a covered row does not change");
        p.toggle(&path(&[1]));
        assert!(p.checked.is_empty());
    }

    #[test]
    fn opened_and_left_alone_it_keeps_its_subject() {
        let doc = Document::new();
        let gone = Subject::Component(Id(999));
        let mut p = ViewPicker::new(&doc, Purpose::Change(Id(1)), gone.clone());
        assert_eq!(p.subject(&doc), gone, "no row stands for it, and nothing was changed");
        p.whole_model();
        assert_eq!(p.subject(&doc), Subject::Model);
    }
}

//! The parts list, the stage of the « Liste des pièces » workspace (docs/ui/parts-list.md): one
//! row per part placed in the model and material, its dimensions (its final B-rep's box along
//! its red, green and blue axes) and how many are placed; sorted by a column's header; a row
//! chosen selects its parts in the view, a part selected in the view lights its row.

use std::cmp::Ordering;
use std::sync::Arc;

use egui::{Align, Layout, Sense, vec2};
use egui_extras::{Column as TableColumn, TableBuilder};
use glam::DVec3;
use skraft_i18n::t;
use skraft_model::kernel::Solid;
use skraft_model::{Document, EntityPath, FastMap, Id, Units};
use skraft_tools::Editor;

use crate::cards::panels::Action;

/// A row: a part, with one material, and where it is placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub part: Id,
    pub material: Option<Id>,
    pub name: String,
    /// The material's name; none: no material.
    pub material_name: Option<String>,
    /// Length, width, thickness (along its axes); none while its shape is not known yet.
    pub dims: Option<[f64; 3]>,
    /// Its occurrences with this material, from the root, in the model's order.
    pub paths: Vec<EntityPath>,
}

impl Row {
    pub fn quantity(&self) -> usize {
        self.paths.len()
    }
}

/// The rows of `doc`: one per part placed in the model and material its occurrences take, in
/// the order they are first placed. Hidden occurrences count: they are in the model.
/// `bounds`: a shape's box in its part's axes.
pub fn rows(doc: &Document, bounds: impl Fn(&Arc<Solid>) -> Option<(DVec3, DVec3)>) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    let mut index: FastMap<(Id, Option<Id>), usize> = FastMap::default();
    for pp in doc.placed_parts() {
        let key = (pp.part, pp.material);
        let i = *index.entry(key).or_insert_with(|| {
            let name = doc.component(pp.part).map(|c| c.name.clone()).unwrap_or_default();
            let material_name = pp.material.and_then(|m| doc.material(m).ok()).map(|m| m.name.clone());
            let dims = bounds(&pp.shape).map(|(lo, hi)| (hi - lo).to_array());
            out.push(Row { part: pp.part, material: pp.material, name, material_name, dims, paths: Vec::new() });
            out.len() - 1
        });
        out[i].paths.push(pp.path);
    }
    out
}

/// The columns, in their order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Column {
    #[default]
    Name,
    Material,
    Length,
    Width,
    Thickness,
    Quantity,
}

impl Column {
    const ALL: [Column; 6] = [Column::Name, Column::Material, Column::Length, Column::Width, Column::Thickness, Column::Quantity];

    fn label(self) -> String {
        match self {
            Column::Name => t!("parts_list.column.name"),
            Column::Material => t!("parts_list.column.material"),
            Column::Length => t!("parts_list.column.length"),
            Column::Width => t!("parts_list.column.width"),
            Column::Thickness => t!("parts_list.column.thickness"),
            Column::Quantity => t!("parts_list.column.quantity"),
        }
    }

    /// Its header: a length's says the unit when the document shows it.
    fn header(self, units: &Units) -> String {
        match self {
            Column::Length | Column::Width | Column::Thickness if units.symbol => {
                t!("parts_list.column.with_unit", name = self.label(), unit = units.length.symbol())
            }
            _ => self.label(),
        }
    }

    fn numeric(self) -> bool {
        !matches!(self, Column::Name | Column::Material)
    }

    /// `a` before `b` by this column, ascending; equal ones by name, then as placed.
    fn order(self, a: &Row, b: &Row) -> Ordering {
        let dim = |r: &Row, k: usize| r.dims.map_or(f64::NEG_INFINITY, |d| d[k]);
        let text = |s: &str| s.to_lowercase();
        let by = match self {
            Column::Name => Ordering::Equal,
            Column::Material => {
                // No material last.
                match (&a.material_name, &b.material_name) {
                    (Some(x), Some(y)) => text(x).cmp(&text(y)),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                }
            }
            Column::Length => dim(a, 0).total_cmp(&dim(b, 0)),
            Column::Width => dim(a, 1).total_cmp(&dim(b, 1)),
            Column::Thickness => dim(a, 2).total_cmp(&dim(b, 2)),
            Column::Quantity => a.quantity().cmp(&b.quantity()),
        };
        by.then_with(|| text(&a.name).cmp(&text(&b.name)))
    }
}

/// How the rows are sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub ascending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Sort { column: Column::Name, ascending: true }
    }
}

/// Sorts `rows` (stable: equal rows keep their order).
pub fn sort(rows: &mut [Row], s: Sort) {
    rows.sort_by(|a, b| {
        let o = s.column.order(a, b);
        if s.ascending { o } else { o.reverse() }
    });
}

/// A document's parts list as shown: its sort, and its rows kept for the history's revision
/// (built again while a shape is not known yet: the kernel is computing it).
#[derive(Default)]
pub struct PartsListState {
    pub sort: Sort,
    rows: Option<(u64, Sort, Vec<Row>)>,
}

impl PartsListState {
    fn rows(&mut self, editor: &Editor) -> &[Row] {
        let revision = editor.history().revision();
        let fresh =
            self.rows.as_ref().is_some_and(|(r, s, rows)| *r == revision && *s == self.sort && rows.iter().all(|r| r.dims.is_some()));
        if !fresh {
            let scene = editor.scene();
            let mut rows = rows(editor.document(), |shape| scene.bounds_of(shape).or_else(|| shape.vertex_bounds()));
            sort(&mut rows, self.sort);
            self.rows = Some((revision, self.sort, rows));
        }
        self.rows.as_ref().map_or(&[], |(_, _, rows)| rows)
    }
}

/// Is any of `row`'s occurrences selected: picked at any depth, or under an occurrence
/// selected in the context edited?
fn lit(editor: &Editor, row: &Row) -> bool {
    let context = editor.context();
    let (selection, deep) = (editor.selection(), editor.deep_selection());
    // The occurrence of the context's assembly a part is placed under: the next one of its
    // chain after the context's.
    let under = |p: &EntityPath| p.instances.starts_with(context).then(|| p.instances.get(context.len()).copied().unwrap_or(p.entity));
    row.paths.iter().any(|p| {
        deep.iter().any(|d| d.instances == p.instances && d.entity == p.entity) || under(p).is_some_and(|id| selection.contains(&id))
    })
}

const ROW_HEIGHT: f32 = 22.0;

/// The parts list of the shown document, filling `ui`.
pub fn ui(ui: &mut egui::Ui, editor: &Editor, state: &mut PartsListState, actions: &mut Vec<Action>) {
    let units = editor.document().settings().units.clone();
    let mut sort = state.sort;
    let rows = state.rows(editor);
    if rows.is_empty() {
        ui.centered_and_justified(|ui| ui.weak(t!("parts_list.empty")));
        return;
    }
    let length = |mm: f64| units.number(mm);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width = |text: String| ui.painter().layout_no_wrap(text, font.clone(), egui::Color32::WHITE).size().x;
    // The widest number at these decimals, rounded; not measured on the rows, at every frame.
    let numbers = width(format!("{}00000,{}", skraft_model::APPROX, "0".repeat(units.decimals.max(1) as usize)));
    // A header's room: its text and the sort's chevron.
    let dim_width = |c: Column| numbers.max(width(c.header(&units)) + 16.0).max(64.0) + 12.0;
    let dim_widths = [Column::Length, Column::Width, Column::Thickness].map(dim_width);
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(false)
        .sense(Sense::click())
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(TableColumn::remainder().at_least(120.0).clip(true))
        .column(TableColumn::initial(130.0).at_least(80.0).clip(true));
    for w in dim_widths {
        table = table.column(TableColumn::exact(w));
    }
    table = table.column(TableColumn::exact(56.0));
    table
        .header(ROW_HEIGHT + 2.0, |mut header| {
            for c in Column::ALL {
                header.col(|ui| {
                    let layout = if c.numeric() { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
                    ui.with_layout(layout, |ui| {
                        let shown = sort.column == c;
                        let ink = if shown { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
                        let label = egui::Label::new(egui::RichText::new(c.header(&units)).color(ink).strong())
                            .sense(Sense::click())
                            .selectable(false);
                        let response = ui.add(label);
                        crate::kit::handles::publish(|| format!("parts_list_header:{c:?}").to_lowercase(), response.rect);
                        if shown {
                            let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                            // Up: ascending.
                            let angle = if sort.ascending { std::f32::consts::PI } else { 0.0 };
                            crate::kit::icons::chevron(ui, rect.center(), 10.0, ink, angle);
                        }
                        if response.clicked() {
                            sort = if shown {
                                Sort { ascending: !sort.ascending, ..sort }
                            } else {
                                Sort { column: c, ascending: !c.numeric() }
                            };
                        }
                    });
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                let r = &rows[row.index()];
                row.set_selected(lit(editor, r));
                let dim = |k: usize| r.dims.map_or_else(|| "—".to_owned(), |d| length(d[k]));
                let right = |ui: &mut egui::Ui, text: String| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.add(egui::Label::new(text).selectable(false)));
                };
                row.col(|ui| {
                    ui.add(egui::Label::new(&r.name).selectable(false).truncate());
                });
                row.col(|ui| match &r.material_name {
                    Some(m) => {
                        ui.add(egui::Label::new(m).selectable(false).truncate());
                    }
                    None => {
                        ui.add(egui::Label::new(egui::RichText::new(t!("panel.materials.none")).weak()).selectable(false));
                    }
                });
                row.col(|ui| right(ui, dim(0)));
                row.col(|ui| right(ui, dim(1)));
                row.col(|ui| right(ui, dim(2)));
                row.col(|ui| right(ui, r.quantity().to_string()));
                let response = row.response();
                crate::kit::handles::publish(
                    || match r.material {
                        Some(m) => format!("parts_list_row:{}:{}", r.part.0, m.0),
                        None => format!("parts_list_row:{}", r.part.0),
                    },
                    response.rect,
                );
                if response.clicked() {
                    actions.push(Action::SelectParts(r.paths.clone()));
                }
            });
        });
    state.sort = sort;
}

#[cfg(test)]
mod tests {
    use super::*;
    use skraft_model::{History, Operation, Primitive};

    fn part_row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
        rows.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}: {rows:?}"))
    }

    #[test]
    fn one_row_per_part_and_material_its_occurrences_counted() {
        let mut h = History::new(Document::new());
        let root = h.document().root();
        let add = |h: &mut History, p: Primitive| {
            let applied = h
                .apply(Operation::AddPrimitive { context: root, primitive: p, placement: glam::DAffine3::IDENTITY, material: None })
                .unwrap();
            applied.created[0]
        };
        let a = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(18.0, 600.0, 400.0) });
        let _b = add(&mut h, Primitive::Box { origin: DVec3::new(0.0, 0.0, 500.0), size: DVec3::new(50.0, 50.0, 700.0) });
        h.apply(Operation::Duplicate { context: root, entities: vec![a], transform: glam::DAffine3::from_translation(DVec3::X * 100.0) })
            .unwrap();
        let doc = h.document();
        let rows = rows(doc, |s| s.vertex_bounds());
        assert_eq!(rows.len(), 2, "{rows:?}");
        let shelf = &rows[0];
        assert_eq!(shelf.quantity(), 2, "the copy shares its part");
        assert_eq!(shelf.dims, Some([18.0, 600.0, 400.0]), "along its axes");
        assert_eq!(rows[1].quantity(), 1);
        assert_eq!(rows[1].dims, Some([50.0, 50.0, 700.0]));
        let _ = part_row(&rows, &shelf.name);
    }

    #[test]
    fn a_length_header_says_the_unit_when_the_document_shows_it() {
        let mut units = Units { length: skraft_model::LengthUnit::Inch, ..Default::default() };
        assert_eq!(Column::Length.header(&units), Column::Length.label());
        units.symbol = true;
        assert!(Column::Length.header(&units).ends_with("(in)"));
        assert_eq!(Column::Quantity.header(&units), Column::Quantity.label());
    }

    #[test]
    fn rows_sort_by_a_column_then_by_name() {
        let row = |name: &str, dims: [f64; 3], n: usize, material: Option<&str>| Row {
            part: Id(0),
            material: None,
            name: name.into(),
            material_name: material.map(str::to_owned),
            dims: Some(dims),
            paths: vec![EntityPath::new(Vec::new(), Id(1)); n],
        };
        let mut rows = vec![
            row("b", [100.0, 50.0, 18.0], 1, Some("Pin")),
            row("A", [300.0, 50.0, 18.0], 4, None),
            row("c", [200.0, 60.0, 22.0], 2, Some("Chêne")),
        ];
        let names = |rows: &[Row]| rows.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        sort(&mut rows, Sort::default());
        assert_eq!(names(&rows), ["A", "b", "c"], "case ignored");
        sort(&mut rows, Sort { column: Column::Length, ascending: false });
        assert_eq!(names(&rows), ["A", "c", "b"]);
        sort(&mut rows, Sort { column: Column::Material, ascending: true });
        assert_eq!(names(&rows), ["c", "b", "A"], "no material last");
        sort(&mut rows, Sort { column: Column::Quantity, ascending: true });
        assert_eq!(names(&rows), ["b", "c", "A"]);
    }
}

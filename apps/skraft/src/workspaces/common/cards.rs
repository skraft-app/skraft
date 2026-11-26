//! The document's cards, for any workspace (docs/ui/interface.md): the navigator's tree of the
//! model, the entity info and its properties, the components and materials cards and what they
//! share.

use glam::{DAffine3, DVec3};
use skraft_i18n::{t, tp};
use skraft_model::{
    BodiesTo, ComponentKind, Document, EntityPath, Family, FastMap, FastSet, GuideShape, Id, MaterialKind, Nature, Occurrence, Operation,
    PropTarget, Scope, Sign, Units, measure,
};
use skraft_tools::Editor;

use crate::kit::icons;
use crate::kit::style::Tip;
use crate::kit::widgets::{
    Field, Fold, OPTION_HEIGHT, RENAMING, TreeLine, band_color, cut_label, field_stroke, framed_field, hidden_color, linked_badge,
    linked_mark, paint_drag_ghost, rename_field, set_drag_ghost, start_rename, tree_line, tree_row, warning,
};

mod props;

use crate::commands::Action;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_dragged_go_to_another_level_of_their_part_not_into_themselves() {
        let part = EntityPath::new(Vec::new(), Id(5));
        // Bodies 6 and 7 of the group 9 dragged.
        let drag = BodiesDrag { from: part.clone().in_group(vec![Id(9)]), bodies: vec![Id(6), Id(7)] };
        assert_eq!(drag.to(true, &part, Vec::new()), Some(BodiesTo::Level(Vec::new())), "to the part's root");
        assert_eq!(drag.to(true, &part, vec![Id(9), Id(8)]), Some(BodiesTo::Level(vec![Id(9), Id(8)])), "into a group beside them");
        assert_eq!(drag.to(true, &part, vec![Id(9)]), None, "where they lie");
        assert_eq!(drag.to(true, &part, vec![Id(9), Id(7)]), None, "into one of them");
        let other = EntityPath::new(Vec::new(), Id(4));
        assert_eq!(drag.to(false, &other, vec![Id(3)]), Some(BodiesTo::Part(other.clone().in_group(vec![Id(3)]))), "into another part");
    }

    /// The top of the text drawn in a navigator row: its name, or the field renaming it.
    fn text_top(ctx: &egui::Context, renaming: bool) -> f32 {
        let mut top = f32::NAN;
        // The field takes the focus on its first frame, then settles.
        for time in 0..3 {
            let input = egui::RawInput { time: Some(time as f64), ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                let height = ui.spacing().interact_size.y;
                tree_row(ui, height, false, false, false, |ui| match renaming {
                    true => drop(rename_field(ui, egui::Id::unique("rename_test"), "Solide 1")),
                    false => drop(ui.add(egui::Label::new("Solide 1").selectable(false))),
                });
            });
            output.textures_delta.clear();
            top = output.shapes.iter().find_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.pos.y) } else { None }).unwrap();
        }
        top
    }

    /// The navigator's rows under the search `text`: (depth, occurrence, body, dimmed).
    fn filtered(e: &Editor, text: &str) -> Vec<(usize, Id, Option<Id>, bool)> {
        let doc = e.document();
        let matcher = skraft_tools::search::Matcher::new(text);
        let mut holds = FastMap::default();
        let mut rows = Vec::new();
        Filtered { matcher: &matcher, holds: &mut holds, doc }.rows(doc.root(), &mut Vec::new(), &mut rows, false, 0);
        rows.iter().map(|r| (r.path.len(), r.occurrence.id, r.body.as_ref().map(|b| b.body.id), r.dim)).collect()
    }

    #[test]
    fn a_search_keeps_what_matches_and_the_rows_above_it() {
        use glam::DVec3;
        use skraft_model::Primitive;
        let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
        let root = e.document().root();
        let add = |e: &mut Editor, x: f64| {
            let p = Primitive::Box { origin: DVec3::new(x, 0.0, 0.0), size: DVec3::splat(10.0) };
            e.apply(Operation::AddPrimitive { context: root, primitive: p, placement: DAffine3::IDENTITY, material: None })
                .unwrap()
                .unwrap()
                .created[0]
        };
        let (a, b, c) = (add(&mut e, 0.0), add(&mut e, 100.0), add(&mut e, 200.0));
        let g = e.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().unwrap().created[0];
        let pa = e.document().resolve(&EntityPath::new(vec![g], a)).unwrap().component;
        e.apply(Operation::RenameComponent { component: pa, name: "Étagère".into() }).unwrap();

        // A part deep in the assembly: the assembly above it, attenuated, then it.
        let rows = filtered(&e, "etag");
        assert_eq!(rows, vec![(0, g, None, true), (1, a, None, false)]);
        assert!(rows.iter().all(|r| r.1 != b && r.1 != c));
        // Bodies: under their part, by their definition's name.
        let pc = e.document().resolve(&EntityPath::root(c)).unwrap().component;
        let part = e.document().part(pc).unwrap();
        let body = part.def_of(&part.bodies[0]).name.clone();
        let rows = filtered(&e, &body);
        assert!(rows.iter().any(|r| r.2.is_some() && !r.3), "{rows:?}");
        assert!(rows.iter().filter(|r| r.2.is_none()).all(|r| r.3), "the parts and assembly above, attenuated");
    }

    #[test]
    fn the_rename_field_keeps_the_name_where_the_row_had_it() {
        let ctx = egui::Context::default();
        crate::kit::style::install_fonts(&ctx);
        let label = text_top(&ctx, false);
        assert!(label > 0.0, "the row's name centred in it: {label}");
        assert_eq!(text_top(&ctx, true), label);
    }
}

pub(crate) mod catalog;
mod info;
pub(crate) mod library;
mod material;
pub(crate) mod materials;
mod navigator;

pub(crate) use info::*;
pub(crate) use material::*;
pub(crate) use navigator::*;

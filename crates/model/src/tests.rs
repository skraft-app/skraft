use std::sync::Arc;

use glam::{DAffine3, DVec3};

use crate::*;

fn cube(at: DVec3, size: f64) -> Primitive {
    Primitive::Box { origin: at, size: DVec3::splat(size) }
}

/// Adds a primitive at the root, returns its occurrence.
fn add(h: &mut History, p: Primitive) -> Id {
    let root = h.document().root();
    h.apply(Operation::AddPrimitive { context: root, primitive: p, placement: DAffine3::IDENTITY }).unwrap().created[0]
}

fn occ(doc: &Document, path: &EntityPath) -> Occurrence {
    doc.resolve(path).unwrap().clone()
}

/// The part's world bounds (its occurrence's transform applied to its shape's corners).
fn world_bounds(doc: &Document, path: &EntityPath) -> (DVec3, DVec3) {
    let (_, part, t) = doc.resolve_part(path).unwrap();
    let (lo, hi) = part.shape().vertex_bounds().unwrap();
    let mut out = (DVec3::INFINITY, DVec3::NEG_INFINITY);
    for i in 0..8 {
        let c = DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let p = t.transform_point3(c);
        out = (out.0.min(p), out.1.max(p));
    }
    out
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-9
}

#[test]
fn a_primitive_is_a_part_in_its_own_axes() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, Primitive::Box { origin: DVec3::new(10.0, 20.0, 30.0), size: DVec3::new(-5.0, 6.0, 7.0) });
    let doc = h.document();
    let o = occ(doc, &EntityPath::root(a));
    let part = doc.part(o.component).unwrap();
    // The blank starts at the part's origin; the occurrence puts it back where it was drawn.
    let (lo, hi) = part.blank.vertex_bounds().unwrap();
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(5.0, 6.0, 7.0)), "{lo} {hi}");
    assert!(close(o.transform.translation, DVec3::new(5.0, 20.0, 30.0)));
    assert_eq!(doc.component(o.component).unwrap().name, "Boîte");
    assert!(!doc.component(o.component).unwrap().named);

    assert!(h.undo());
    assert!(h.document().root_assembly().occurrences.is_empty());
    assert_eq!(h.document().components().count(), 0, "the part went with its occurrence");
    assert!(h.redo());
    assert_eq!(h.document().root_assembly().occurrences[0].id, a);
}

#[test]
fn moving_copying_and_arraying_need_no_kernel() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let doc = h.document().clone();
    let root = doc.root();
    let part = occ(&doc, &EntityPath::root(a)).component;
    let t = DAffine3::from_translation(DVec3::new(20.0, 0.0, 0.0));
    let rot = DAffine3::from_rotation_z(0.3) * t;
    for op in [
        Operation::Transform { context: root, entities: vec![a], transform: rot },
        Operation::Duplicate { context: root, entities: vec![a], transform: t },
        Operation::Array { context: root, entities: vec![a], step: t, count: 3 },
    ] {
        assert!(matches!(doc.prepare(&op).unwrap(), Prepared::Done(_)), "{op:?}");
    }

    // Moving changes the occurrence only: the part's B-rep is the same.
    let blank = doc.part(part).unwrap().blank.clone();
    h.apply(Operation::Transform { context: root, entities: vec![a], transform: rot }).unwrap();
    assert!(Arc::ptr_eq(&blank, &h.document().part(part).unwrap().blank));
    assert!(close(occ(h.document(), &EntityPath::root(a)).transform.translation, rot.translation));

    // Copies are more occurrences of the same part.
    let out = h.apply(Operation::Array { context: root, entities: vec![a], step: t, count: 3 }).unwrap();
    assert_eq!(out.created.len(), 3);
    assert_eq!(h.document().occurrence_count(part), 4);
    assert_eq!(h.document().components().count(), 1);
    let xs: Vec<f64> = out.created.iter().map(|id| occ(h.document(), &EntityPath::root(*id)).transform.translation.x).collect();
    let x0 = rot.translation.x;
    assert!(xs.iter().zip([x0 + 20.0, x0 + 40.0, x0 + 60.0]).all(|(x, e)| (x - e).abs() < 1e-9), "{xs:?}");
    assert!(h.apply(Operation::Array { context: root, entities: vec![a], step: t, count: 0 }).is_err());
}

#[test]
fn mirrors_place_but_scales_are_refused() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let root = h.document().root();
    let mirror = DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0));
    h.apply(Operation::Duplicate { context: root, entities: vec![a], transform: mirror }).unwrap();
    assert_eq!(h.document().components().count(), 1, "left and right are the same part");
    for t in [DAffine3::from_scale(DVec3::splat(2.0)), DAffine3::from_scale(DVec3::new(1.0, 2.0, 1.0))] {
        assert!(h.apply(Operation::Transform { context: root, entities: vec![a], transform: t }).is_err());
        assert!(h.apply(Operation::Duplicate { context: root, entities: vec![a], transform: t }).is_err());
    }
}

#[test]
fn unnamed_parts_go_with_their_last_occurrence() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::splat(20.0), 5.0));
    let (pa, pb) = (occ(h.document(), &EntityPath::root(a)).component, occ(h.document(), &EntityPath::root(b)).component);
    assert_eq!(h.document().component(pb).unwrap().name, "Boîte#2");
    h.apply(Operation::RenameComponent { component: pb, name: "Pied".into() }).unwrap();
    assert!(h.apply(Operation::RenameComponent { component: pa, name: "Pied".into() }).is_err(), "names are unique");
    h.apply(Operation::Delete { context: root, entities: vec![a, b] }).unwrap();
    assert!(h.document().component(pa).is_err());
    let kept = h.document().component(pb).unwrap();
    assert!(kept.named, "a named part stays in the library");
    // …and can be placed again.
    let out = h.apply(Operation::PlaceOccurrence { context: root, component: pb, transform: DAffine3::IDENTITY }).unwrap();
    assert_eq!(occ(h.document(), &EntityPath::root(out.created[0])).component, pb);
}

#[test]
fn assemblies_make_explode_and_share() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let g = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().created[0];
    let doc = h.document();
    assert_eq!(doc.root_assembly().occurrences.len(), 1);
    let asm = occ(doc, &EntityPath::root(g)).component;
    assert_eq!(doc.component(asm).unwrap().name, "Assemblage");
    assert_eq!(doc.context_of(&[g]).unwrap(), asm);
    assert!(doc.is_assembly_occurrence(root, g));

    // Moving the assembly moves its occurrence only.
    let t = DAffine3::from_translation(DVec3::new(100.0, 0.0, 0.0));
    h.apply(Operation::Transform { context: root, entities: vec![g], transform: t }).unwrap();
    let placed = h.document().placed_parts();
    assert_eq!(placed.len(), 2);
    assert!(placed.iter().all(|p| p.path.instances == vec![g]));
    assert!(close(world_bounds(h.document(), &EntityPath::new(vec![g], a)).0, DVec3::new(100.0, 0.0, 0.0)));

    // A copy of the assembly shares it: two paths to each part.
    h.apply(Operation::Duplicate { context: root, entities: vec![g], transform: t }).unwrap();
    assert_eq!(h.document().placed_parts().len(), 4);
    assert_eq!(h.document().occurrence_count(asm), 2);

    // Making one copy unique gives it an assembly of its own, the parts still shared.
    h.apply(Operation::MakeUnique { context: root, occurrence: g }).unwrap();
    let own = occ(h.document(), &EntityPath::root(g)).component;
    assert_ne!(own, asm);
    assert_eq!(h.document().component(own).unwrap().name, "Assemblage#2");
    assert_eq!(h.document().components().filter(|c| c.is_part()).count(), 2);

    // Exploding puts its occurrences in place, the assembly's transform composed; the unnamed
    // assembly goes.
    let out = h.apply(Operation::Explode { context: root, occurrence: g }).unwrap();
    assert_eq!(out.created.len(), 2);
    assert!(h.document().component(own).is_err());
    let xs: Vec<f64> = out.created.iter().map(|id| world_bounds(h.document(), &EntityPath::root(*id)).0.x).collect();
    assert!(xs.iter().zip([100.0, 120.0]).all(|(x, e)| (x - e).abs() < 1e-9), "{xs:?}");
    // A part's occurrence is not exploded.
    assert!(h.apply(Operation::Explode { context: root, occurrence: out.created[0] }).is_err());
    // No assembly inside itself.
    assert!(matches!(
        h.apply(Operation::PlaceOccurrence { context: asm, component: asm, transform: DAffine3::IDENTITY }),
        Err(Error::Cycle)
    ));
}

#[test]
fn a_difference_machines_the_part_for_all_its_occurrences() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::new(1000.0, 0.0, 0.0), size: DVec3::new(600.0, 300.0, 18.0) });
    let copy = h.apply(Operation::Duplicate { context: root, entities: vec![board], transform: DAffine3::from_translation(DVec3::Y * 500.0) }).unwrap().created[0];
    // A drill through the first board's top, in an assembly moved elsewhere.
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(0.0, 0.0, 6.0), radius: 4.0, height: 20.0 });
    let g = h.apply(Operation::MakeAssembly { context: root, entities: vec![drill], name: None }).unwrap().created[0];
    h.apply(Operation::Transform { context: root, entities: vec![g], transform: DAffine3::from_translation(DVec3::new(1100.0, 50.0, 0.0)) }).unwrap();
    let part = occ(h.document(), &EntityPath::root(board)).component;
    let blank = h.document().part(part).unwrap().blank.clone();

    let target = EntityPath::root(board);
    let tool = EntityPath::new(vec![g], drill);
    h.apply(Operation::Boolean { target: target.clone(), tool, op: Boolean::Difference, scope: Scope::All }).unwrap();
    let doc = h.document();
    let p = doc.part(part).unwrap();
    assert!(Arc::ptr_eq(&p.blank, &blank), "the blank stays");
    assert_eq!(p.machinings.len(), 1);
    // The drill, in the board's axes: 100, 50 from its corner, its base 6 up.
    assert!(close(p.machinings[0].placement.translation, DVec3::new(100.0, 50.0, 6.0)), "{}", p.machinings[0].placement.translation);
    assert!(p.shape().topology_counts().0 > blank.topology_counts().0);
    // Both occurrences show the hole; the drill left the model, its assembly and part with it.
    assert_eq!(doc.resolve_part(&EntityPath::root(copy)).unwrap().0, part);
    assert!(doc.resolve(&EntityPath::root(g)).is_err() || doc.assembly(doc.context_of(&[g]).unwrap()).unwrap().occurrences.is_empty());
    assert_eq!(doc.components().filter(|c| c.is_part()).count(), 1, "{:?}", doc.components().map(|c| &c.name).collect::<Vec<_>>());

    // A union makes a new blank, without machinings.
    let other = add(&mut h, Primitive::Box { origin: DVec3::new(1590.0, 0.0, 0.0), size: DVec3::new(20.0, 300.0, 100.0) });
    h.apply(Operation::Boolean { target, tool: EntityPath::root(other), op: Boolean::Union, scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.machinings.is_empty());
    assert!(!Arc::ptr_eq(&p.blank, &blank));
    let (_, hi) = p.blank.vertex_bounds().unwrap();
    assert!((hi.x - 610.0).abs() < 1e-9 && (hi.z - 100.0).abs() < 1e-9, "{hi}");
}

#[test]
fn errors_leave_the_document_unchanged() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let before = h.document().clone();
    let r = h.apply(Operation::AddPrimitive { context: root, primitive: cube(DVec3::ZERO, 0.0), placement: DAffine3::IDENTITY });
    assert!(matches!(r, Err(Error::Kernel(_))), "{r:?}");
    assert_eq!(*h.document(), before);
    assert!(!h.can_undo());
    assert!(h.apply(Operation::Delete { context: root, entities: vec![Id(999)] }).is_err());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let same = EntityPath::root(a);
    assert!(h.apply(Operation::Boolean { target: same.clone(), tool: same, op: Boolean::Union, scope: Scope::All }).is_err());
}

#[test]
fn file_round_trip() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 50.0, 18.0) });
    let b = add(&mut h, Primitive::Cylinder { origin: DVec3::new(50.0, 25.0, 10.0), radius: 4.0, height: 20.0 });
    h.apply(Operation::Boolean { target: EntityPath::root(a), tool: EntityPath::root(b), op: Boolean::Difference, scope: Scope::All }).unwrap();
    h.apply(Operation::MakeAssembly { context: root, entities: vec![a], name: Some("Tiroir".into()) }).unwrap();
    let json = h.document().to_json().unwrap();
    assert!(json.contains("\"version\":2"));
    let back = Document::from_json(&json).unwrap();
    assert_eq!(back, *h.document());
    assert!(Document::from_json(r#"{"format":"other","version":1}"#).is_err());
    assert!(Document::from_json(&json.replacen("\"version\":2", "\"version\":99", 1)).is_err());
}

/// Reference files of each version, read back as they were written (docs/ARCHITECTURE.md §8):
/// a format change that breaks old files fails here.
#[test]
fn reference_files_still_open() {
    // v1: a free box at the root, a group holding a free cylinder, a component with two
    // instances (one named).
    let v1 = Document::from_json(include_str!("../tests/files/v1.solidsketch")).unwrap();
    let placed = v1.placed_parts();
    assert_eq!(placed.len(), 4, "{placed:?}");
    assert_eq!(v1.components().filter(|c| c.is_part()).count(), 3);
    let root = v1.root_assembly();
    assert_eq!(root.occurrences.len(), 4);
    let names: Vec<&str> = v1.components().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Boîte") && names.contains(&"Cylindre") && names.contains(&"Groupe") && names.contains(&"Composant"), "{names:?}");
    assert!(v1.components().find(|c| c.name == "Composant").unwrap().named);
    assert!(!v1.components().find(|c| c.name == "Groupe").unwrap().named);
    assert!(root.occurrences.iter().any(|o| o.name == "Gauche"));
    // Nothing moved: free solids keep their coordinates, placed at identity.
    let boite = placed.iter().find(|p| v1.component(p.part).unwrap().name == "Boîte").unwrap();
    assert!(boite.transform.abs_diff_eq(DAffine3::IDENTITY, 0.0));
    assert!(close(boite.shape.vertex_bounds().unwrap().0, DVec3::new(10.0, 0.0, 0.0)));

    // v2: a board with a drill hole, in a named assembly.
    let v2 = Document::from_json(include_str!("../tests/files/v2.solidsketch")).unwrap();
    let parts: Vec<&Part> = v2.components().filter_map(|c| c.as_part()).collect();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].machinings.len(), 1);
    assert!(v2.components().any(|c| c.name == "Tiroir" && c.named));
}

/// Writes the reference files again: `SOLIDSKETCH_WRITE_FILES=1 cargo test -p solidsketch-model
/// write_reference_files`. Only for a new version: the old ones must stay as they are.
#[test]
fn write_reference_files() {
    if std::env::var_os("SOLIDSKETCH_WRITE_FILES").is_none() {
        return;
    }
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 50.0, 18.0) });
    let b = add(&mut h, Primitive::Cylinder { origin: DVec3::new(50.0, 25.0, 10.0), radius: 4.0, height: 20.0 });
    h.apply(Operation::Boolean { target: EntityPath::root(a), tool: EntityPath::root(b), op: Boolean::Difference, scope: Scope::All }).unwrap();
    h.apply(Operation::MakeAssembly { context: root, entities: vec![a], name: Some("Tiroir".into()) }).unwrap();
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/files");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/v2.solidsketch"), h.document().to_json().unwrap()).unwrap();

    // Version 1 as its serde wrote it (definitions holding solids and instances; the layout
    // checked against a file saved by that version).
    use serde_json::json;
    let solid = |p: Primitive| serde_json::to_value(p.build().unwrap()).unwrap();
    let t = |x: f64| serde_json::to_value(DAffine3::from_translation(DVec3::new(x, 0.0, 0.0))).unwrap();
    let v1 = json!({
        "format": "solidsketch",
        "version": 1,
        "document": {
            "definitions": {
                "1": { "id": 1, "name": "Modèle", "kind": "Root", "entities": [
                    { "Solid": { "id": 2, "name": "Boîte", "solid": solid(cube(DVec3::new(10.0, 0.0, 0.0), 20.0)) } },
                    { "Instance": { "id": 4, "name": "", "definition": 3, "transform": t(100.0) } },
                    { "Instance": { "id": 7, "name": "", "definition": 6, "transform": t(200.0) } },
                    { "Instance": { "id": 8, "name": "Gauche", "definition": 6, "transform": t(300.0) } }
                ] },
                "3": { "id": 3, "name": "Groupe", "kind": "Group", "entities": [
                    { "Solid": { "id": 5, "name": "Cylindre", "solid": solid(Primitive::Cylinder { origin: DVec3::ZERO, radius: 5.0, height: 10.0 }) } }
                ] },
                "6": { "id": 6, "name": "Composant", "kind": "Component", "entities": [
                    { "Solid": { "id": 9, "name": "Boîte", "solid": solid(cube(DVec3::ZERO, 5.0)) } }
                ] }
            },
            "root": 1,
            "next_id": 10,
            "settings": serde_json::to_value(Settings::default()).unwrap()
        }
    });
    std::fs::write(format!("{dir}/v1.solidsketch"), v1.to_string()).unwrap();
}

#[test]
fn settings_are_saved_undone_and_merged() {
    let mut h = History::new(Document::new());
    let mut s = h.document().settings().clone();
    s.display.grid = true;
    h.apply(Operation::SetSettings(s.clone())).unwrap();
    s.units.length = LengthUnit::Centimeter;
    h.apply(Operation::SetSettings(s.clone())).unwrap();
    // Two changes in a row: one step.
    assert_eq!(h.journal().count(), 1);
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.settings(), &s);
    assert!(h.undo());
    assert_eq!(h.document().settings(), &Settings::default());
}

#[test]
fn units_format() {
    let mut u = Units::default();
    assert_eq!(u.format(12.5), "12,5 mm");
    assert_eq!(u.format(300.0), "300 mm");
    u.length = LengthUnit::Centimeter;
    u.decimals = 1;
    assert_eq!(u.format(1234.0), "123,4 cm");
    assert_eq!(u.format(-0.01), "0 cm");
}

#[test]
fn revision_tells_the_saved_state() {
    let mut h = History::new(Document::new());
    assert_eq!(h.revision(), 0);
    add(&mut h, cube(DVec3::ZERO, 10.0));
    add(&mut h, cube(DVec3::splat(20.0), 5.0));
    let saved = h.revision();
    // Undo then another step: same journal length, but not the saved document.
    assert!(h.undo());
    assert_ne!(h.revision(), saved);
    add(&mut h, cube(DVec3::splat(40.0), 5.0));
    assert_eq!(h.journal().count(), 2);
    assert_ne!(h.revision(), saved);
    // Back to the saved state by undo / redo: unmodified again.
    assert!(h.undo());
    let before = h.revision();
    add(&mut h, cube(DVec3::splat(20.0), 5.0));
    assert_ne!(h.revision(), saved, "an equal step redone by hand is still a new state");
    assert!(h.undo());
    assert_eq!(h.revision(), before);
    assert!(h.redo());
    assert_ne!(h.revision(), before);
}

#[test]
fn merged_settings_make_a_new_revision() {
    let mut h = History::new(Document::new());
    let mut s = Settings::default();
    s.display.grid = !s.display.grid;
    h.apply(Operation::SetSettings(s.clone())).unwrap();
    let saved = h.revision();
    s.display.axes = !s.display.axes;
    h.apply(Operation::SetSettings(s)).unwrap();
    assert_eq!(h.journal().count(), 1, "the two settings steps merge");
    assert_ne!(h.revision(), saved);
    // Undoing the merged step goes back before both, redoing returns to the merged state.
    let merged = h.revision();
    assert!(h.undo());
    assert_eq!(h.revision(), 0);
    assert!(h.redo());
    assert_eq!(h.revision(), merged);
}

#[test]
fn prepare_gathers_kernel_calls_and_commit_replays_them() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::splat(5.0), 10.0));
    let doc = h.document().clone();

    // A boolean: both shapes cross, one task.
    let op = Operation::Boolean { target: EntityPath::root(a), tool: EntityPath::root(b), op: Boolean::Union, scope: Scope::All };
    let Prepared::Kernel(job) = doc.prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert_eq!(job.inputs.len(), 2);
    assert_eq!(job.tasks.len(), 1);
    let outputs = kernel::run(&job).unwrap();

    // Out of step outputs are refused; the right ones give what `apply` gives.
    assert!(doc.commit(&op, Vec::new()).is_err());
    let applied = doc.commit(&op, outputs).unwrap();
    assert_eq!(applied.displays.len(), 1);
    assert!(applied.displays.iter().all(|(_, d)| d.is_ok()));
    let (direct, outcome) = doc.apply(&op).unwrap();
    assert_eq!(applied.document, direct);
    assert_eq!(applied.outcome, outcome);

    // Validation happens before any kernel call.
    let bad = Operation::Boolean { target: EntityPath::root(a), tool: EntityPath::root(Id(999)), op: Boolean::Union, scope: Scope::All };
    assert!(doc.prepare(&bad).is_err());
}

/// Opens a file given by `SOLIDSKETCH_OPEN` (a check of a real file, by hand):
/// `SOLIDSKETCH_OPEN=path cargo test -p solidsketch-model open_given_file -- --nocapture`.
#[test]
fn open_given_file() {
    let Some(path) = std::env::var_os("SOLIDSKETCH_OPEN") else { return };
    let doc = Document::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
    for c in doc.components() {
        println!("{} {:?} named={} occurrences={}", c.id, c.name, c.named, doc.occurrence_count(c.id));
    }
    println!("{} parts placed", doc.placed_parts().len());
    // And it saves again.
    assert_eq!(Document::from_json(&doc.to_json().unwrap()).unwrap(), doc);
}

#[test]
fn only_this_one_makes_the_way_to_it_unique() {
    // A shelf part, in a cabinet assembly copied twice; plus a loose copy of the shelf.
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let shelf = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(500.0, 300.0, 18.0) });
    let loose = h.apply(Operation::Duplicate { context: root, entities: vec![shelf], transform: DAffine3::from_translation(DVec3::Y * 1000.0) }).unwrap().created[0];
    let cab = h.apply(Operation::MakeAssembly { context: root, entities: vec![shelf], name: Some("Caisson".into()) }).unwrap().created[0];
    let cab2 = h.apply(Operation::Duplicate { context: root, entities: vec![cab], transform: DAffine3::from_translation(DVec3::X * 1000.0) }).unwrap().created[0];
    let part = occ(h.document(), &EntityPath::new(vec![cab], shelf)).component;
    let cabinet = occ(h.document(), &EntityPath::root(cab)).component;
    assert_eq!(h.document().placed_parts().iter().filter(|p| p.part == part).count(), 3);

    // A hole in the shelf of the second cabinet only.
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(1250.0, 150.0, 10.0), radius: 4.0, height: 20.0 });
    let target = EntityPath::new(vec![cab2], shelf);
    h.apply(Operation::Boolean { target: target.clone(), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::This }).unwrap();
    let doc = h.document();
    // The path still names it; its cabinet and its shelf are their own now.
    let (own_part, p, _) = doc.resolve_part(&target).unwrap();
    assert_ne!(own_part, part);
    assert_eq!(p.machinings.len(), 1);
    let own_cabinet = occ(doc, &EntityPath::root(cab2)).component;
    assert_ne!(own_cabinet, cabinet);
    assert_eq!(doc.component(own_cabinet).unwrap().name, "Caisson#2");
    // The others are untouched: the first cabinet's shelf and the loose one share the old part.
    assert_eq!(doc.resolve_part(&EntityPath::new(vec![cab], shelf)).unwrap().0, part);
    assert_eq!(doc.resolve_part(&EntityPath::root(loose)).unwrap().0, part);
    assert!(doc.part(part).unwrap().machinings.is_empty());
    // Occurrence ids are unique within an assembly only: the file says so, and opens.
    let back = Document::from_json(&doc.to_json().unwrap()).unwrap();
    assert_eq!(back, *doc);

    // Nothing shared on the way: nothing copied.
    let n = doc.components().count();
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(1300.0, 150.0, 10.0), radius: 4.0, height: 20.0 });
    h.apply(Operation::Boolean { target: target.clone(), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::This }).unwrap();
    assert_eq!(h.document().components().count(), n);
    assert_eq!(h.document().resolve_part(&target).unwrap().1.machinings.len(), 2);

    // All occurrences: the first cabinet's shelf and the loose one both get the hole.
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(250.0, 150.0, 10.0), radius: 4.0, height: 20.0 });
    h.apply(Operation::Boolean { target: EntityPath::new(vec![cab], shelf), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::All }).unwrap();
    assert_eq!(h.document().part(part).unwrap().machinings.len(), 1);
    assert_eq!(h.document().resolve_part(&EntityPath::root(loose)).unwrap().0, part);
}

#[test]
fn materials_paint_occurrences_assemblies_and_faces() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let oak = h.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap().material.unwrap();
    let walnut = h.apply(Operation::AddMaterial { name: "Noyer".into(), color: [90, 60, 40] }).unwrap().material.unwrap();
    assert_eq!(h.document().materials().count(), 2);

    // An occurrence painted: its placed part takes the material, the other does not.
    h.apply(Operation::PaintOccurrences { context: root, entities: vec![a], material: Some(oak) }).unwrap();
    let placed = |h: &History, o: Id| h.document().placed_parts().into_iter().find(|p| p.path.entity == o).unwrap().material;
    assert_eq!(placed(&h, a), Some(oak));
    assert_eq!(placed(&h, b), None);

    // An assembly painted paints the parts in it that have no material of their own.
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().created[0];
    h.apply(Operation::PaintOccurrences { context: root, entities: vec![asm], material: Some(walnut) }).unwrap();
    assert_eq!(placed(&h, a), Some(oak));
    assert_eq!(placed(&h, b), Some(walnut));

    // A face of a part: kept by the part (every occurrence), refused past the last face.
    let part = h.document().placed_parts().into_iter().find(|p| p.path.entity == b).unwrap().part;
    h.apply(Operation::PaintFaces { part, faces: vec![0, 2], material: Some(oak) }).unwrap();
    assert_eq!(h.document().part(part).unwrap().faces.len(), 2);
    assert!(h.apply(Operation::PaintFaces { part, faces: vec![6], material: Some(oak) }).is_err());
    h.apply(Operation::PaintFaces { part, faces: vec![2], material: None }).unwrap();
    assert_eq!(h.document().part(part).unwrap().faces.keys().copied().collect::<Vec<_>>(), [0]);

    // Recolouring in a row is one step; the file keeps it all.
    let before = h.document().clone();
    h.apply(Operation::EditMaterial { material: oak, name: "Chêne".into(), color: [190, 150, 100] }).unwrap();
    h.apply(Operation::EditMaterial { material: oak, name: "Chêne".into(), color: [200, 160, 110] }).unwrap();
    assert_eq!(h.document().material(oak).unwrap().color, [200, 160, 110]);
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(&back, h.document());
    assert!(h.undo());
    assert_eq!(h.document(), &before, "both recolourings undo together");

    // Deleting a material unpaints what used it.
    h.apply(Operation::DeleteMaterial { material: oak }).unwrap();
    assert_eq!(placed(&h, a), Some(walnut), "the assembly's material shows through");
    assert!(h.document().part(part).unwrap().faces.is_empty());
    assert!(h.apply(Operation::PaintOccurrences { context: root, entities: vec![asm], material: Some(oak) }).is_err());
}

#[test]
fn a_file_with_an_unknown_material_is_refused() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let m = h.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap().material.unwrap();
    h.apply(Operation::PaintOccurrences { context: root, entities: vec![a], material: Some(m) }).unwrap();
    let json = h.document().to_json().unwrap();
    let broken = json.replace(&format!("\"material\":{}", m.0), "\"material\":999");
    assert_ne!(broken, json);
    assert!(Document::from_json(&broken).is_err());
}

#[test]
fn the_section_is_a_setting() {
    let mut h = History::new(Document::new());
    let mut s = h.document().settings().clone();
    assert!(s.display.silhouettes && s.display.active_section().is_none());
    s.display.section = Some(Section { origin: DVec3::new(0.0, 0.0, 5.0), normal: DVec3::Z });
    s.display.section_active = true;
    h.apply(Operation::SetSettings(s)).unwrap();
    let sec = h.document().settings().display.active_section().unwrap();
    assert!(sec.cuts(DVec3::new(0.0, 0.0, 6.0)) && !sec.cuts(DVec3::new(0.0, 0.0, 4.0)));
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.settings(), h.document().settings());
}

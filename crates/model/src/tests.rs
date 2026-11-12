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
        let c =
            DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
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
    assert_eq!(doc.component(o.component).unwrap().name, "Pièce");
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
    assert_eq!(h.document().component(pb).unwrap().name, "Pièce#2");
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
    let copy = h
        .apply(Operation::Duplicate { context: root, entities: vec![board], transform: DAffine3::from_translation(DVec3::Y * 500.0) })
        .unwrap()
        .created[0];
    // A drill through the first board's top, in an assembly moved elsewhere.
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(0.0, 0.0, 6.0), radius: 4.0, height: 20.0 });
    let g = h.apply(Operation::MakeAssembly { context: root, entities: vec![drill], name: None }).unwrap().created[0];
    h.apply(Operation::Transform {
        context: root,
        entities: vec![g],
        transform: DAffine3::from_translation(DVec3::new(1100.0, 50.0, 0.0)),
    })
    .unwrap();
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
    h.apply(Operation::Boolean { target: EntityPath::root(a), tool: EntityPath::root(b), op: Boolean::Difference, scope: Scope::All })
        .unwrap();
    h.apply(Operation::MakeAssembly { context: root, entities: vec![a], name: Some("Tiroir".into()) }).unwrap();
    let json = h.document().to_json().unwrap();
    assert!(json.contains("\"version\":1"));
    let back = Document::from_json(&json).unwrap();
    assert_eq!(back, *h.document());
    assert!(Document::from_json(r#"{"format":"other","version":1}"#).is_err());
    assert!(Document::from_json(&json.replacen("\"version\":1", "\"version\":99", 1)).is_err());
}

#[test]
fn guides_belong_to_their_component() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::new(100.0, 0.0, 0.0), 10.0));
    let part = occ(h.document(), &EntityPath::root(a)).component;
    let line = GuideShape::Line { origin: DVec3::new(0.0, 5.0, 0.0), dir: DVec3::X };
    let g = h.apply(Operation::AddGuide { component: part, shape: line }).unwrap().guide.unwrap();
    assert_eq!(h.document().component(part).unwrap().guide(g).unwrap().shape, line);
    // Placed by its occurrence, as the part.
    let placed = h.document().placed_guides();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].path, vec![a]);
    assert!(close(placed[0].transform.translation, DVec3::new(100.0, 0.0, 0.0)));

    // Refused: no direction, no radius; unknown component.
    let flat = GuideShape::Line { origin: DVec3::ZERO, dir: DVec3::ZERO };
    assert!(h.apply(Operation::AddGuide { component: root, shape: flat }).is_err());
    let dot = GuideShape::Circle { center: DVec3::ZERO, normal: DVec3::Z, radius: 0.0 };
    assert!(h.apply(Operation::AddGuide { component: root, shape: dot }).is_err());
    assert!(h.apply(Operation::AddGuide { component: Id(999), shape: line }).is_err());

    // Saved and read back; undone.
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.component(part).unwrap().guides.len(), 1);
    assert!(h.undo());
    assert!(h.document().component(part).unwrap().guides.is_empty());
    assert!(h.redo());

    // A separate part keeps its guides, ids and all.
    let b = h.apply(Operation::Duplicate { context: root, entities: vec![a], transform: DAffine3::IDENTITY }).unwrap().created[0];
    h.apply(Operation::MakeUnique { context: root, occurrence: b }).unwrap();
    let copy = occ(h.document(), &EntityPath::root(b)).component;
    assert_ne!(copy, part);
    assert_eq!(h.document().component(copy).unwrap().guide(g).unwrap().shape, line);

    // An exploded assembly hands its guides to the context, where they stood.
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a], name: None }).unwrap().created[0];
    // Its axes have their origin at the low corner of its content.
    assert!(close(occ(h.document(), &EntityPath::root(asm)).transform.translation, DVec3::new(100.0, 0.0, 0.0)));
    let asm_def = occ(h.document(), &EntityPath::root(asm)).component;
    h.apply(Operation::AddGuide { component: asm_def, shape: line }).unwrap();
    let shift = DAffine3::from_translation(DVec3::new(0.0, 0.0, 50.0));
    h.apply(Operation::Transform { context: root, entities: vec![asm], transform: shift }).unwrap();
    h.apply(Operation::Explode { context: root, occurrence: asm }).unwrap();
    let guides = &h.document().component(root).unwrap().guides;
    assert_eq!(guides.len(), 1);
    assert_eq!(guides[0].shape, GuideShape::Line { origin: DVec3::new(100.0, 5.0, 50.0), dir: DVec3::X });
}

#[test]
fn a_file_with_a_bad_guide_is_refused() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    h.apply(Operation::AddGuide { component: root, shape: GuideShape::Line { origin: DVec3::ZERO, dir: DVec3::Y } }).unwrap();
    let json = h.document().to_json().unwrap();
    // A guide whose id is past the counter.
    let past = json.replace("\"guides\":[{\"id\":2", "\"guides\":[{\"id\":99");
    assert_ne!(past, json);
    assert!(matches!(Document::from_json(&past), Err(Error::File(FileError::BadGuide(Id(99))))));
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
    u.symbol = false;
    assert_eq!(u.format(1234.0), "123,4");
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
    let loose = h
        .apply(Operation::Duplicate { context: root, entities: vec![shelf], transform: DAffine3::from_translation(DVec3::Y * 1000.0) })
        .unwrap()
        .created[0];
    let cab = h.apply(Operation::MakeAssembly { context: root, entities: vec![shelf], name: Some("Caisson".into()) }).unwrap().created[0];
    let cab2 = h
        .apply(Operation::Duplicate { context: root, entities: vec![cab], transform: DAffine3::from_translation(DVec3::X * 1000.0) })
        .unwrap()
        .created[0];
    let part = occ(h.document(), &EntityPath::new(vec![cab], shelf)).component;
    let cabinet = occ(h.document(), &EntityPath::root(cab)).component;
    assert_eq!(h.document().placed_parts().iter().filter(|p| p.part == part).count(), 3);

    // A hole in the shelf of the second cabinet only.
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(1250.0, 150.0, 10.0), radius: 4.0, height: 20.0 });
    let target = EntityPath::new(vec![cab2], shelf);
    h.apply(Operation::Boolean { target: target.clone(), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::This })
        .unwrap();
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
    h.apply(Operation::Boolean { target: target.clone(), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::This })
        .unwrap();
    assert_eq!(h.document().components().count(), n);
    assert_eq!(h.document().resolve_part(&target).unwrap().1.machinings.len(), 2);

    // All occurrences: the first cabinet's shelf and the loose one both get the hole.
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(250.0, 150.0, 10.0), radius: 4.0, height: 20.0 });
    h.apply(Operation::Boolean {
        target: EntityPath::new(vec![cab], shelf),
        tool: EntityPath::root(drill),
        op: Boolean::Difference,
        scope: Scope::All,
    })
    .unwrap();
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

/// A board drilled once at (20, 25), its occurrence and part.
fn drilled_board(h: &mut History) -> (EntityPath, Id, Id) {
    let board = add(h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let drill = add(h, Primitive::Cylinder { origin: DVec3::new(20.0, 25.0, -5.0), radius: 4.0, height: 30.0 });
    let target = EntityPath::root(board);
    h.apply(Operation::Boolean { target: target.clone(), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::All })
        .unwrap();
    let (part, p, _) = h.document().resolve_part(&target).unwrap();
    (target, part, p.machinings[0].id)
}

fn faces(doc: &Document, part: Id) -> usize {
    doc.part(part).unwrap().shape().topology_counts().0
}

#[test]
fn machinings_are_copied_arrayed_moved_and_deleted() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    let one = faces(h.document(), part);

    // A copy is cut from the shape as it is: one tool in the recipe.
    let dx = |x: f64| DAffine3::from_translation(DVec3::X * x);
    let op = Operation::DuplicateMachinings { target: target.clone(), machinings: vec![hole], transform: dx(40.0), scope: Scope::All };
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Recipe { tools, .. }] if tools.len() == 1), "{:?}", job.tasks);
    let copy = h.apply(op).unwrap().machinings[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.machinings.len(), 2);
    assert!(close(p.machining(copy).unwrap().placement.translation, DVec3::new(60.0, 25.0, -5.0)));
    let two = faces(h.document(), part);
    assert!(two > one, "{two} > {one}");

    // An array: three more, 40 apart.
    let created = h
        .apply(Operation::ArrayMachinings { target: target.clone(), machinings: vec![copy], step: dx(40.0), count: 3, scope: Scope::All })
        .unwrap()
        .machinings;
    assert_eq!(created.len(), 3);
    let p = h.document().part(part).unwrap();
    assert!(close(p.machining(created[2]).unwrap().placement.translation, DVec3::new(180.0, 25.0, -5.0)));
    assert!(p.failed.is_empty());

    // A move computes the part again from the blank: every tool in the recipe.
    let op = Operation::TransformMachinings { target: target.clone(), machinings: vec![hole], transform: dx(-10.0), scope: Scope::All };
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Recipe { tools, .. }] if tools.len() == 5));
    h.apply(op).unwrap();
    assert!(close(h.document().part(part).unwrap().machining(hole).unwrap().placement.translation, DVec3::new(10.0, 25.0, -5.0)));
    assert!(
        h.apply(Operation::TransformMachinings {
            target: target.clone(),
            machinings: vec![hole],
            transform: DAffine3::from_scale(DVec3::splat(2.0)),
            scope: Scope::All
        })
        .is_err()
    );

    // Deleting all but one, then the last: the blank is the shape again, without the kernel.
    let all: Vec<Id> = h.document().part(part).unwrap().machinings.iter().map(|m| m.id).collect();
    h.apply(Operation::DeleteMachinings { target: target.clone(), machinings: all[1..].to_vec(), scope: Scope::All }).unwrap();
    assert_eq!(faces(h.document(), part), one);
    let op = Operation::DeleteMachinings { target: target.clone(), machinings: vec![hole], scope: Scope::All };
    assert!(matches!(h.document().prepare(&op).unwrap(), Prepared::Done(_)));
    h.apply(op).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.machinings.is_empty() && p.shape.is_none());

    // Refused: no machining named, one the part does not have.
    assert!(h.apply(Operation::DeleteMachinings { target: target.clone(), machinings: vec![], scope: Scope::All }).is_err());
    assert!(h.apply(Operation::DeleteMachinings { target, machinings: vec![hole], scope: Scope::All }).is_err());
    // Undo brings them back.
    h.undo();
    assert_eq!(h.document().part(part).unwrap().machinings.len(), 1);
}

#[test]
fn a_failing_machining_is_skipped_and_reported() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    // A tool swallowing the board, as a moved machining could (put in by hand: a boolean
    // refuses it at once).
    let mut doc = h.document().clone();
    let huge = doc.fresh_id();
    let tool = Arc::new(kernel::Solid::new_box(DVec3::splat(-100.0), DVec3::splat(1000.0)).unwrap());
    doc.part_mut(part).unwrap().machinings.push(Machining {
        id: huge,
        tool,
        placement: DAffine3::IDENTITY,
        source: MachiningSource::Local,
    });
    let mut h = History::new(doc);

    // Moving the hole computes the part from the blank: the huge tool fails, the hole cuts.
    h.apply(Operation::TransformMachinings {
        target: target.clone(),
        machinings: vec![hole],
        transform: DAffine3::from_translation(DVec3::X * 50.0),
        scope: Scope::All,
    })
    .unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.is_failed(huge) && !p.is_failed(hole));
    let blank = p.blank.topology_counts().0;
    assert!(faces(h.document(), part) > blank);
    // Saved with the file.
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert!(back.part(part).unwrap().is_failed(huge));
    // Gone with it.
    h.apply(Operation::DeleteMachinings { target, machinings: vec![huge], scope: Scope::All }).unwrap();
    assert!(h.document().part(part).unwrap().failed.is_empty());
}

#[test]
fn only_this_one_separates_a_machined_part_keeping_machining_ids() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let (target, part, hole) = drilled_board(&mut h);
    let other = h
        .apply(Operation::Duplicate {
            context: root,
            entities: vec![target.entity],
            transform: DAffine3::from_translation(DVec3::Y * 100.0),
        })
        .unwrap()
        .created[0];

    h.apply(Operation::DeleteMachinings { target: target.clone(), machinings: vec![hole], scope: Scope::This }).unwrap();
    let doc = h.document();
    let (own, p, _) = doc.resolve_part(&target).unwrap();
    assert_ne!(own, part);
    assert!(p.machinings.is_empty());
    // The other occurrence keeps the hole, under the same id.
    let (shared, q, _) = doc.resolve_part(&EntityPath::root(other)).unwrap();
    assert_eq!(shared, part);
    assert!(q.machining(hole).is_some());

    // Two parts with the same machining id: the file opens again (ids are unique per part).
    let mut h = History::new(Document::new());
    let (target, _, hole) = drilled_board(&mut h);
    let root = h.document().root();
    h.apply(Operation::Duplicate { context: root, entities: vec![target.entity], transform: DAffine3::from_translation(DVec3::Y * 100.0) })
        .unwrap();
    h.apply(Operation::TransformMachinings {
        target: target.clone(),
        machinings: vec![hole],
        transform: DAffine3::from_translation(DVec3::X * 30.0),
        scope: Scope::This,
    })
    .unwrap();
    let doc = h.document();
    assert_eq!(doc.components().filter(|c| c.as_part().is_some_and(|p| p.machining(hole).is_some())).count(), 2);
    assert_eq!(Document::from_json(&doc.to_json().unwrap()).unwrap(), *doc);
}

#[test]
fn a_primitive_drawn_in_a_part_is_a_machining() {
    let mut h = History::new(Document::new());
    let (target, part, _) = drilled_board(&mut h);
    let before = faces(h.document(), part);
    // A groove along the board's top, flush with its ends, over the hole, in its axes.
    let groove = Primitive::Box { origin: DVec3::new(0.0, 20.0, 12.0), size: DVec3::new(200.0, 10.0, 10.0) };
    let out = h
        .apply(Operation::AddMachining { target: target.clone(), primitive: groove, placement: DAffine3::IDENTITY, scope: Scope::All })
        .unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.machinings.len(), 2);
    let m = p.machining(out.machinings[0]).unwrap();
    // The tool at its own origin, placed where it was drawn.
    assert!(close(m.placement.translation, DVec3::new(0.0, 20.0, 12.0)));
    assert!(close(m.tool.vertex_bounds().unwrap().0, DVec3::ZERO));
    assert!(p.failed.is_empty(), "{:?}", p.failed);
    assert!(faces(h.document(), part) > before, "{} {before}", faces(h.document(), part));
    assert!(
        h.apply(Operation::AddMachining {
            target,
            primitive: cube(DVec3::ZERO, 5.0),
            placement: DAffine3::from_scale(DVec3::splat(2.0)),
            scope: Scope::All
        })
        .is_err()
    );
}

#[test]
fn guides_are_erased_moved_copied_and_grouped_like_occurrences() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let line = GuideShape::Line { origin: DVec3::new(0.0, 5.0, 0.0), dir: DVec3::X };
    let g = h.apply(Operation::AddGuide { component: root, shape: line }).unwrap().guide.unwrap();
    let guides = |h: &History| h.document().component(root).unwrap().guides.clone();
    let up = DAffine3::from_translation(DVec3::new(0.0, 0.0, 20.0));

    // Moved with an occurrence.
    h.apply(Operation::Transform { context: root, entities: vec![a, g], transform: up }).unwrap();
    assert_eq!(guides(&h)[0].shape, GuideShape::Line { origin: DVec3::new(0.0, 5.0, 20.0), dir: DVec3::X });
    assert!(close(occ(h.document(), &EntityPath::root(a)).transform.translation, DVec3::new(0.0, 0.0, 20.0)));
    // Copied, alone (no kernel): the copy among what was created.
    let out = h.apply(Operation::Array { context: root, entities: vec![g], step: up, count: 2 }).unwrap();
    assert_eq!(out.created.len(), 2);
    assert_eq!(guides(&h).len(), 3);
    assert_eq!(guides(&h)[2].shape, GuideShape::Line { origin: DVec3::new(0.0, 5.0, 60.0), dir: DVec3::X });
    // Painting a selection holding a guide paints only the occurrences.
    let oak = h.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap().material.unwrap();
    h.apply(Operation::PaintOccurrences { context: root, entities: vec![a, g], material: Some(oak) }).unwrap();
    // Erased.
    h.apply(Operation::Delete { context: root, entities: vec![out.created[0]] }).unwrap();
    assert_eq!(guides(&h).len(), 2);
    // Grouped: the guide goes into the assembly, where it stood — in its axes, whose origin is
    // the low corner of its content.
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, g], name: None }).unwrap().created[0];
    assert_eq!(guides(&h).len(), 1);
    assert!(close(occ(h.document(), &EntityPath::root(asm)).transform.translation, DVec3::new(0.0, 0.0, 20.0)));
    let def = occ(h.document(), &EntityPath::root(asm)).component;
    let moved = GuideShape::Line { origin: DVec3::new(0.0, 5.0, 0.0), dir: DVec3::X };
    assert_eq!(h.document().component(def).unwrap().guide(g).unwrap().shape, moved);
    // Only guides: no assembly.
    let rest = guides(&h)[0].id;
    assert!(h.apply(Operation::MakeAssembly { context: root, entities: vec![rest], name: None }).is_err());
}

#[test]
fn a_part_s_guides_go_with_its_machinings() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 100.0));
    let b = add(&mut h, Primitive::Cylinder { origin: DVec3::new(50.0, 50.0, 80.0), radius: 5.0, height: 30.0 });
    h.apply(Operation::Boolean { target: EntityPath::root(a), tool: EntityPath::root(b), op: Boolean::Difference, scope: Scope::All })
        .unwrap();
    let target = EntityPath::root(a);
    let part = occ(h.document(), &target).component;
    let circle = GuideShape::Circle { center: DVec3::new(50.0, 50.0, 100.0), normal: DVec3::Z, radius: 20.0 };
    let g = h.apply(Operation::AddGuide { component: part, shape: circle }).unwrap().guide.unwrap();
    let m = h.document().part(part).unwrap().machinings[0].id;
    let shift = DAffine3::from_translation(DVec3::new(10.0, 0.0, 0.0));

    // Guides alone: no kernel, no separate part even "only this one".
    let doc = h.document().clone();
    let op = Operation::TransformMachinings { target: target.clone(), machinings: vec![g], transform: shift, scope: Scope::This };
    assert!(matches!(doc.prepare(&op).unwrap(), Prepared::Done(_)));
    h.apply(op).unwrap();
    assert_eq!(occ(h.document(), &target).component, part);
    let GuideShape::Circle { center, .. } = h.document().component(part).unwrap().guide(g).unwrap().shape else { panic!() };
    assert!(close(center, DVec3::new(60.0, 50.0, 100.0)));
    // Copied with a machining: both copies created.
    let out =
        h.apply(Operation::DuplicateMachinings { target: target.clone(), machinings: vec![m, g], transform: shift, scope: Scope::All });
    assert_eq!(out.unwrap().machinings.len(), 2);
    assert_eq!(h.document().component(part).unwrap().guides.len(), 2);
    // Erased alone.
    h.apply(Operation::DeleteMachinings { target, machinings: vec![g], scope: Scope::All }).unwrap();
    assert_eq!(h.document().component(part).unwrap().guides.len(), 1);
    assert_eq!(h.document().part(part).unwrap().machinings.len(), 2);
}

/// A stretch of the occurrence `target`, in its component's axes.
fn stretch(doc: &Document, target: &EntityPath, axis: DVec3, cuts: Vec<f64>, moves: Vec<f64>, scope: Scope) -> Operation {
    let frame = doc.resolve(target).unwrap().transform;
    Operation::Stretch {
        context: target.instances.clone(),
        occurrences: vec![target.entity],
        frame,
        axis,
        cuts,
        moves,
        scope,
        cutters: None,
    }
}

/// A stretch along the part's X: the cutter at `cut`, the far side moved by `d`.
fn stretch_x(doc: &Document, target: &EntityPath, cut: f64, d: f64, scope: Scope) -> Operation {
    stretch(doc, target, DVec3::X, vec![cut], vec![0.0, d], scope)
}

#[test]
fn a_stretch_stretches_the_blank_and_moves_or_stretches_the_machinings() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    // A second hole past the cutter, and a groove along the whole board.
    let far = h
        .apply(Operation::DuplicateMachinings {
            target: target.clone(),
            machinings: vec![hole],
            transform: DAffine3::from_translation(DVec3::X * 160.0),
            scope: Scope::All,
        })
        .unwrap()
        .machinings[0];
    let groove = Primitive::Box { origin: DVec3::new(0.0, 20.0, 12.0), size: DVec3::new(200.0, 10.0, 10.0) };
    let groove = h
        .apply(Operation::AddMachining { target: target.clone(), primitive: groove, placement: DAffine3::IDENTITY, scope: Scope::All })
        .unwrap()
        .machinings[0];

    // The blank and the groove's tool are stretched, the holes only moved; then the recipe.
    let op = stretch_x(h.document(), &target, 100.0, 50.0, Scope::All);
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(
        matches!(&job.tasks[..], [kernel::Task::Stretch { .. }, kernel::Task::Stretch { .. }, kernel::Task::Recipe { tools, .. }] if tools.len() == 3),
        "{:?}",
        job.tasks
    );
    h.apply(op).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(close(p.blank.vertex_bounds().unwrap().1, DVec3::new(250.0, 50.0, 18.0)));
    assert!(close(p.machining(hole).unwrap().placement.translation, DVec3::new(20.0, 25.0, -5.0)));
    assert!(close(p.machining(far).unwrap().placement.translation, DVec3::new(230.0, 25.0, -5.0)));
    assert!(close(p.machining(groove).unwrap().tool.vertex_bounds().unwrap().1, DVec3::new(250.0, 10.0, 10.0)));
    assert!(p.failed.is_empty(), "{:?}", p.failed);
    assert!(close(p.shape().bounds().unwrap().1, DVec3::new(250.0, 50.0, 18.0)));

    // Compressed back, then undone.
    h.apply(stretch_x(h.document(), &target, 100.0, -50.0, Scope::All)).unwrap();
    assert!(close(h.document().part(part).unwrap().blank.vertex_bounds().unwrap().1, DVec3::new(200.0, 50.0, 18.0)));
    h.undo();
    h.undo();
    assert!(close(h.document().part(part).unwrap().blank.vertex_bounds().unwrap().1, DVec3::new(200.0, 50.0, 18.0)));
    assert!(close(h.document().part(part).unwrap().machining(far).unwrap().placement.translation, DVec3::new(180.0, 25.0, -5.0)));
}

#[test]
fn a_bare_part_is_stretched_without_a_recipe() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let target = EntityPath::root(board);
    let op = stretch(h.document(), &target, DVec3::Y, vec![25.0], vec![-10.0, 10.0], Scope::All);
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Stretch { .. }]), "{:?}", job.tasks);
    h.apply(op).unwrap();
    let (_, p, t) = h.document().resolve_part(&target).unwrap();
    assert!(p.shape.is_none());
    // The origin, in the section moving by -10, follows it (OCL's anchor).
    assert_eq!(p.blank.vertex_bounds().unwrap(), (DVec3::ZERO, DVec3::new(200.0, 70.0, 18.0)));
    assert!(close(t.translation, DVec3::new(0.0, -10.0, 0.0)));
}

#[test]
fn the_origin_follows_its_section_and_the_other_occurrences_show_the_same_change() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let target = EntityPath::root(board);
    let other = h
        .apply(Operation::Duplicate { context: root, entities: vec![board], transform: DAffine3::from_translation(DVec3::Y * 100.0) })
        .unwrap()
        .created[0];
    let other = EntityPath::root(other);
    // The origin's side pulled: the origin moves with it, the far end stays.
    h.apply(stretch(h.document(), &target, DVec3::X, vec![100.0], vec![-30.0, 0.0], Scope::All)).unwrap();
    let doc = h.document();
    let (part, p, t) = doc.resolve_part(&target).unwrap();
    assert_eq!(p.blank.vertex_bounds().unwrap(), (DVec3::ZERO, DVec3::new(230.0, 50.0, 18.0)));
    assert!(close(t.translation, DVec3::new(-30.0, 0.0, 0.0)));
    assert!(close(world_bounds(doc, &target).0, DVec3::new(-30.0, 0.0, 0.0)));
    // The other occurrence, not stretched, shows the same change in its own axes.
    assert_eq!(doc.resolve_part(&other).unwrap().0, part);
    let (lo, hi) = world_bounds(doc, &other);
    assert!(close(lo, DVec3::new(-30.0, 100.0, 0.0)) && close(hi, DVec3::new(200.0, 150.0, 18.0)), "{lo} {hi}");
}

#[test]
fn the_origin_stays_when_its_section_does_not_move() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let target = EntityPath::root(board);
    h.apply(stretch_x(h.document(), &target, 100.0, 30.0, Scope::All)).unwrap();
    let (_, p, t) = h.document().resolve_part(&target).unwrap();
    assert_eq!(p.blank.vertex_bounds().unwrap(), (DVec3::ZERO, DVec3::new(230.0, 50.0, 18.0)));
    assert!(close(t.translation, DVec3::ZERO));
}

#[test]
fn a_stretch_of_only_this_one_makes_it_a_separate_part() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let (target, part, _) = drilled_board(&mut h);
    let other = h
        .apply(Operation::Duplicate {
            context: root,
            entities: vec![target.entity],
            transform: DAffine3::from_translation(DVec3::Y * 100.0),
        })
        .unwrap()
        .created[0];
    h.apply(stretch_x(h.document(), &target, 100.0, 50.0, Scope::This)).unwrap();
    let doc = h.document();
    let (mine, p, _) = doc.resolve_part(&target).unwrap();
    assert_ne!(mine, part);
    assert!(close(p.blank.vertex_bounds().unwrap().1, DVec3::new(250.0, 50.0, 18.0)));
    let (theirs, q, _) = doc.resolve_part(&EntityPath::root(other)).unwrap();
    assert_eq!(theirs, part);
    assert!(close(q.blank.vertex_bounds().unwrap().1, DVec3::new(200.0, 50.0, 18.0)));
}

#[test]
fn guides_follow_their_section() {
    let mut h = History::new(Document::new());
    let (target, part, _) = drilled_board(&mut h);
    let guide = |h: &mut History, shape| h.apply(Operation::AddGuide { component: part, shape }).unwrap().guide.unwrap();
    let across = guide(&mut h, GuideShape::Line { origin: DVec3::new(150.0, 0.0, 0.0), dir: DVec3::Y });
    let along = guide(&mut h, GuideShape::Line { origin: DVec3::new(0.0, 25.0, 0.0), dir: DVec3::X });
    let ring = guide(&mut h, GuideShape::Circle { center: DVec3::new(150.0, 25.0, 9.0), normal: DVec3::X, radius: 5.0 });
    let before = guide(&mut h, GuideShape::Line { origin: DVec3::new(50.0, 0.0, 0.0), dir: DVec3::Z });
    h.apply(stretch_x(h.document(), &target, 100.0, 50.0, Scope::All)).unwrap();
    let c = h.document().component(part).unwrap();
    let shape = |id| c.guide(id).unwrap().shape;
    assert_eq!(shape(across), GuideShape::Line { origin: DVec3::new(200.0, 0.0, 0.0), dir: DVec3::Y });
    assert_eq!(shape(along), GuideShape::Line { origin: DVec3::new(0.0, 25.0, 0.0), dir: DVec3::X });
    assert_eq!(shape(ring), GuideShape::Circle { center: DVec3::new(200.0, 25.0, 9.0), normal: DVec3::X, radius: 5.0 });
    assert_eq!(shape(before), GuideShape::Line { origin: DVec3::new(50.0, 0.0, 0.0), dir: DVec3::Z });
}

#[test]
fn a_stretch_that_cannot_apply_is_refused_when_prepared() {
    let mut h = History::new(Document::new());
    // A standing cylinder, cut across its axis: refused before any kernel call.
    let rod = add(&mut h, Primitive::Cylinder { origin: DVec3::ZERO, radius: 10.0, height: 40.0 });
    let target = EntityPath::root(rod);
    let refused = |op: &Operation| {
        let e = h.document().prepare(op).unwrap_err();
        assert!(matches!(e, Error::Kernel(kernel::Error::Stretch(_))), "{e:?}");
    };
    refused(&stretch_x(h.document(), &target, 0.0, 10.0, Scope::All));
    // Malformed: a move per section missing; compressed past nothing.
    refused(&stretch(h.document(), &target, DVec3::Z, vec![20.0], vec![0.0], Scope::All));
    refused(&stretch(h.document(), &target, DVec3::Z, vec![20.0], vec![0.0, -45.0], Scope::All));
    assert!(h.apply(stretch_x(h.document(), &target, 0.0, 10.0, Scope::All)).is_err());
    // Unchanged.
    assert!((h.document().resolve_part(&target).unwrap().1.blank.bounds().unwrap().1.z - 40.0).abs() < 1e-9);
    // Along its axis it stretches.
    h.apply(stretch(h.document(), &target, DVec3::Z, vec![20.0], vec![0.0, 10.0], Scope::All)).unwrap();
    let (_, p, _) = h.document().resolve_part(&target).unwrap();
    assert!((p.blank.bounds().unwrap().1.z - 50.0).abs() < 1e-9);
}

/// A frame along X at the root, made an assembly: two rails 0–600 (one part, the second a copy
/// of the first 300 further along Y) and two legs 0–50 and 550–600 (one part). Returns the
/// history, the assembly's occurrence, then the rails' and the legs' occurrences in it.
fn frame(rail_shift: f64) -> (History, Id, [Id; 2], [Id; 2]) {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let rail = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 20.0, 40.0) });
    let leg = add(&mut h, Primitive::Box { origin: DVec3::new(0.0, 0.0, -400.0), size: DVec3::new(50.0, 50.0, 400.0) });
    let copy = |h: &mut History, id: Id, by: DVec3| {
        h.apply(Operation::Duplicate { context: root, entities: vec![id], transform: DAffine3::from_translation(by) }).unwrap().created[0]
    };
    let rail2 = copy(&mut h, rail, DVec3::new(rail_shift, 300.0, 0.0));
    let leg2 = copy(&mut h, leg, DVec3::X * 550.0);
    let a = h.apply(Operation::MakeAssembly { context: root, entities: vec![rail, rail2, leg, leg2], name: None }).unwrap().created[0];
    (h, a, [rail, rail2], [leg, leg2])
}

/// The X extent of an occurrence of the assembly `a` (at the root), and its part.
fn x_of(doc: &Document, a: Id, o: Id) -> ((f64, f64), Id) {
    let path = EntityPath::new(vec![a], o);
    let (lo, hi) = world_bounds(doc, &path);
    ((lo.x, hi.x), doc.resolve(&path).unwrap().component)
}

#[test]
fn parts_cut_at_different_places_between_the_same_pieces_keep_one_part() {
    // A plain rail, its copy 100 further along X: cut at 300, one is cut 300 into its length,
    // the other 200 — between the same pieces (its ends), the same stretch: one part.
    let (mut h, a, rails, _) = frame(100.0);
    let rail_part = x_of(h.document(), a, rails[0]).1;
    h.apply(stretch_x(h.document(), &EntityPath::root(a), 300.0, 100.0, Scope::All)).unwrap();
    let doc = h.document();
    assert_eq!((x_of(doc, a, rails[0]), x_of(doc, a, rails[1])), (((0.0, 700.0), rail_part), ((100.0, 800.0), rail_part)));
}

#[test]
fn an_assembly_stretched_moves_what_is_in_one_section_and_stretches_what_crosses() {
    let (mut h, a, rails, legs) = frame(0.0);
    let (rail_part, leg_part) = (x_of(h.document(), a, rails[0]).1, x_of(h.document(), a, legs[0]).1);
    let target = EntityPath::root(a);
    // The spans a compression keeps apart: the matter at each end.
    let spans = h.document().stretch_spans(&[], &[a], &DAffine3::IDENTITY, DVec3::X, &[300.0]).unwrap();
    assert_eq!(spans, vec![Some((0.0, 50.0)), Some((550.0, 600.0))]);
    let (lo, hi) = h.document().stretch_bounds(&[], &[a], &DAffine3::IDENTITY).unwrap().unwrap();
    assert!(close(lo, DVec3::new(0.0, 0.0, -400.0)) && close(hi, DVec3::new(600.0, 320.0, 40.0)), "{lo} {hi}");

    let out = h.apply(stretch_x(h.document(), &target, 300.0, 100.0, Scope::All)).unwrap();
    let doc = h.document();
    // The rails, one part still, stretched once; the far leg moved, its part untouched.
    assert_eq!(x_of(doc, a, rails[0]), ((0.0, 700.0), rail_part));
    assert_eq!(x_of(doc, a, rails[1]), ((0.0, 700.0), rail_part));
    assert_eq!(x_of(doc, a, legs[0]), ((0.0, 50.0), leg_part));
    assert_eq!(x_of(doc, a, legs[1]), ((650.0, 700.0), leg_part));
    assert_eq!(out.stretched.iter().map(|s| (s.part, s.from)).collect::<Vec<_>>(), vec![(rail_part, rail_part)]);
    assert!((doc.part(leg_part).unwrap().blank.bounds().unwrap().1.x - 50.0).abs() < 1e-9);
}

#[test]
fn occurrences_of_one_part_stretched_differently_become_separate_parts() {
    // The second rail 100 further along X, the rail drilled 250 into its length: cut at 300,
    // the first rail has its hole before the cutter, the second after it.
    let (mut h, a, rails, _) = frame(100.0);
    let rail_part = x_of(h.document(), a, rails[0]).1;
    let hole = Primitive::Cylinder { origin: DVec3::new(250.0, 10.0, -10.0), radius: 5.0, height: 60.0 };
    let target = EntityPath::new(vec![a], rails[0]);
    h.apply(Operation::AddMachining { target, primitive: hole, placement: DAffine3::IDENTITY, scope: Scope::All }).unwrap();
    h.apply(stretch_x(h.document(), &EntityPath::root(a), 300.0, 100.0, Scope::All)).unwrap();
    let doc = h.document();
    let ((r0, p0), (r1, p1)) = (x_of(doc, a, rails[0]), x_of(doc, a, rails[1]));
    assert_eq!((r0, r1), ((0.0, 700.0), (100.0, 800.0)));
    // The first met keeps the part; the other is a copy, its ids kept.
    assert_eq!(p0, rail_part);
    assert_ne!(p1, rail_part);
    assert_eq!(doc.part(p1).unwrap().blank.bounds().unwrap().1.x, 700.0);
}

#[test]
fn an_assembly_shown_twice_is_stretched_everywhere_or_only_there() {
    for scope in [Scope::All, Scope::This] {
        let (mut h, a, rails, legs) = frame(0.0);
        let root = h.document().root();
        let b = h
            .apply(Operation::Duplicate { context: root, entities: vec![a], transform: DAffine3::from_translation(DVec3::Y * 1000.0) })
            .unwrap()
            .created[0];
        // The rail's part shown outside the assembly too.
        let rail_part = x_of(h.document(), a, rails[0]).1;
        let alone =
            h.apply(Operation::PlaceOccurrence { context: root, component: rail_part, transform: DAffine3::IDENTITY }).unwrap().created[0];
        h.apply(stretch_x(h.document(), &EntityPath::root(a), 300.0, 100.0, scope)).unwrap();
        let doc = h.document();
        let alone_x = world_bounds(doc, &EntityPath::root(alone)).1.x;
        let (b_rail, b_leg) = (x_of(doc, b, rails[0]).0, x_of(doc, b, legs[1]).0);
        assert_eq!(x_of(doc, a, rails[1]).0, (0.0, 700.0), "{scope:?}");
        assert_eq!(x_of(doc, a, legs[1]).0, (650.0, 700.0), "{scope:?}");
        match scope {
            // Everywhere: the other assembly, the rail alone.
            Scope::All => assert_eq!((b_rail, b_leg, alone_x), ((0.0, 700.0), (650.0, 700.0), 700.0)),
            // Only there: the assembly and its rail are its own; the rest as it was.
            Scope::This => {
                assert_eq!((b_rail, b_leg, alone_x), ((0.0, 600.0), (550.0, 600.0), 600.0));
                assert_ne!(doc.resolve(&EntityPath::root(a)).unwrap().component, doc.resolve(&EntityPath::root(b)).unwrap().component);
                assert_ne!(x_of(doc, a, rails[0]).1, rail_part);
                assert_eq!(x_of(doc, a, rails[0]).1, x_of(doc, a, rails[1]).1);
            }
        }
    }
}

#[test]
fn a_part_of_an_assembly_that_cannot_stretch_is_named() {
    let (mut h, a, _, _) = frame(0.0);
    // A rod in the assembly across the cutter, square to the axis: it cannot be stretched along X.
    let asm = h.document().resolve(&EntityPath::root(a)).unwrap().component;
    let cyl = Primitive::Cylinder { origin: DVec3::new(300.0, 150.0, 0.0), radius: 10.0, height: 40.0 };
    h.apply(Operation::AddPrimitive { context: asm, primitive: cyl, placement: DAffine3::IDENTITY }).unwrap();
    let e = h.document().prepare(&stretch_x(h.document(), &EntityPath::root(a), 300.0, 100.0, Scope::All)).unwrap_err();
    assert!(matches!(&e, Error::InPart { error, .. } if matches!(**error, Error::Kernel(kernel::Error::Stretch(_)))), "{e:?}");
}

#[test]
fn a_copy_turned_half_round_stretched_alike_keeps_its_part() {
    for (turn, mirror) in
        [(DAffine3::from_rotation_z(std::f64::consts::PI), false), (DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)), true)]
    {
        let mut h = History::new(Document::new());
        let root = h.document().root();
        let rail = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 20.0, 40.0) });
        // The copy turned (or mirrored) about its far end: it spans 0–600 along X too, its own
        // X running the other way.
        let t = DAffine3::from_translation(DVec3::new(600.0, if mirror { 300.0 } else { 320.0 }, 0.0)) * turn;
        let rail2 = h.apply(Operation::Duplicate { context: root, entities: vec![rail], transform: t }).unwrap().created[0];
        let a = h.apply(Operation::MakeAssembly { context: root, entities: vec![rail, rail2], name: None }).unwrap().created[0];
        let part = x_of(h.document(), a, rail).1;
        assert_eq!(x_of(h.document(), a, rail2).0, (0.0, 600.0));
        // Cut off-centre, its far end pulled 100: both stretched alike, one part still, the
        // turned one moved along its own axis to show it.
        h.apply(stretch_x(h.document(), &EntityPath::root(a), 200.0, 100.0, Scope::All)).unwrap();
        let doc = h.document();
        let ((r0, p0), (r1, p1)) = (x_of(doc, a, rail), x_of(doc, a, rail2));
        assert_eq!((r0, r1), ((0.0, 700.0), (0.0, 700.0)), "mirror {mirror}");
        assert_eq!((p0, p1), (part, part), "mirror {mirror}");
    }
}

/// Three boards of one part at the root, 0–200 along X, 100 apart along Y.
fn three_boards() -> (History, Id, [Id; 3]) {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let copy = |h: &mut History, by: f64| {
        h.apply(Operation::Duplicate { context: root, entities: vec![board], transform: DAffine3::from_translation(DVec3::Y * by) })
            .unwrap()
            .created[0]
    };
    let (b2, b3) = (copy(&mut h, 100.0), copy(&mut h, 300.0));
    let part = h.document().resolve(&EntityPath::root(board)).unwrap().component;
    (h, part, [board, b2, b3])
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

/// The world X extent of the root occurrence `o`, and its part.
fn root_x(doc: &Document, o: Id) -> ((f64, f64), Id) {
    let path = EntityPath::root(o);
    let (lo, hi) = world_bounds(doc, &path);
    ((lo.x, hi.x), doc.resolve(&path).unwrap().component)
}

#[test]
fn several_occurrences_stretched_together_share_their_change() {
    let (mut h, part, [b1, b2, b3]) = three_boards();
    // Their common box, in the root's axes.
    let (lo, hi) = h.document().stretch_bounds(&[], &[b1, b2], &DAffine3::IDENTITY).unwrap().unwrap();
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(200.0, 150.0, 18.0)), "{lo} {hi}");
    let op = |scope| Operation::Stretch {
        context: vec![],
        occurrences: vec![b1, b2],
        frame: DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![100.0],
        moves: vec![0.0, 50.0],
        scope,
        cutters: None,
    };

    // Only these: one copy for both, the third as it was.
    let out = h.apply(op(Scope::This)).unwrap();
    let doc = h.document();
    let ((x1, p1), (x2, p2), (x3, p3)) = (root_x(doc, b1), root_x(doc, b2), root_x(doc, b3));
    assert_eq!((x1, x2, x3), ((0.0, 250.0), (0.0, 250.0), (0.0, 200.0)));
    assert_eq!((p1 == p2, p1 != part, p3), (true, true, part));
    assert_eq!(out.stretched.iter().map(|s| (s.part, s.from)).collect::<Vec<_>>(), vec![(p1, part)]);

    // All: the part itself, the third showing it.
    h.undo();
    h.apply(op(Scope::All)).unwrap();
    let doc = h.document();
    assert_eq!((root_x(doc, b1), root_x(doc, b2), root_x(doc, b3)), (((0.0, 250.0), part), ((0.0, 250.0), part), ((0.0, 250.0), part)));
}

#[test]
fn several_occurrences_in_the_context_axes_turned_ones_take_their_own_change() {
    let (mut h, part, [b1, b2, _]) = three_boards();
    let root = h.document().root();
    // The second turned square, standing at 100–150 along X, 0–200 along Y.
    let turn = DAffine3::from_translation(DVec3::new(250.0, 0.0, 0.0)) * DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    h.apply(Operation::Transform { context: root, entities: vec![b2], transform: turn }).unwrap();
    let (lo, hi) = world_bounds(h.document(), &EntityPath::root(b2));
    assert!(close(lo, DVec3::new(100.0, 0.0, 0.0)) && close(hi, DVec3::new(150.0, 200.0, 18.0)), "{lo} {hi}");
    let op = |cut: f64| Operation::Stretch {
        context: vec![],
        occurrences: vec![b1, b2],
        frame: DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![cut],
        moves: vec![0.0, 50.0],
        scope: Scope::All,
        cutters: None,
    };

    // Cut at 50: the first board stretched along its length, the turned one only moved.
    h.apply(op(50.0)).unwrap();
    let doc = h.document();
    let ((x1, p1), (x2, p2)) = (root_x(doc, b1), root_x(doc, b2));
    assert!(near(x1, (0.0, 250.0)) && near(x2, (150.0, 200.0)), "{x1:?} {x2:?}");
    assert_eq!((p1, p2), (part, part));
    h.undo();

    // Cut at 125: both cross it, the turned one across its width — a change of its own.
    h.apply(op(125.0)).unwrap();
    let doc = h.document();
    let ((x1, p1), (x2, p2)) = (root_x(doc, b1), root_x(doc, b2));
    assert!(near(x1, (0.0, 250.0)) && near(x2, (100.0, 200.0)), "{x1:?} {x2:?}");
    assert_eq!(p1, part);
    assert_ne!(p2, part);
    let (lo, hi) = world_bounds(doc, &EntityPath::root(b2));
    assert!(close(lo, DVec3::new(100.0, 0.0, 0.0)) && close(hi, DVec3::new(200.0, 200.0, 18.0)), "{lo} {hi}");
}

#[test]
fn a_stretch_in_the_model_axes_reaches_into_a_turned_context() {
    // A board in an assembly turned square and moved: stretched along the model's X, from
    // inside the assembly, it grows across its width.
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let a = h.apply(Operation::MakeAssembly { context: root, entities: vec![board], name: None }).unwrap().created[0];
    let turn = DAffine3::from_translation(DVec3::new(1000.0, 0.0, 0.0)) * DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    h.apply(Operation::Transform { context: root, entities: vec![a], transform: turn }).unwrap();
    let path = EntityPath::new(vec![a], board);
    let (lo, hi) = world_bounds(h.document(), &path);
    assert!(close(lo, DVec3::new(950.0, 0.0, 0.0)) && close(hi, DVec3::new(1000.0, 200.0, 18.0)), "{lo} {hi}");

    // The model's axes in the context's.
    let frame = h.document().transform_of(&[a]).unwrap().inverse();
    let (blo, bhi) = h.document().stretch_bounds(&[a], &[board], &frame).unwrap().unwrap();
    assert!(close(blo, lo) && close(bhi, hi), "{blo} {bhi}");
    let op = Operation::Stretch {
        context: vec![a],
        occurrences: vec![board],
        frame,
        axis: DVec3::X,
        cuts: vec![975.0],
        moves: vec![0.0, 30.0],
        scope: Scope::All,
        cutters: None,
    };
    h.apply(op).unwrap();
    let (lo, hi) = world_bounds(h.document(), &path);
    assert!(close(lo, DVec3::new(950.0, 0.0, 0.0)) && close(hi, DVec3::new(1030.0, 200.0, 18.0)), "{lo} {hi}");
}

#[test]
fn a_stretch_without_occurrences_is_refused() {
    let (h, _, _) = three_boards();
    let op = Operation::Stretch {
        context: vec![],
        occurrences: vec![],
        frame: DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![100.0],
        moves: vec![0.0, 50.0],
        scope: Scope::All,
        cutters: None,
    };
    assert!(matches!(h.document().prepare(&op), Err(Error::Invalid(Refusal::NothingStretched))));
}

#[test]
fn a_behavior_is_set_merged_saved_copied_and_checked() {
    let (mut h, part, [b1, ..]) = three_boards();
    let root = h.document().root();
    let behavior = |mask: u8, x: Vec<f64>, y: Vec<f64>| Behavior { no_stretch: StretchMask(mask), cutters: Some([x, y, vec![0.5]]) };
    let set = |b: Behavior| Operation::SetBehavior { component: part, behavior: b };
    // Two changes in a row to one component: one undo step.
    h.apply(set(behavior(0, vec![0.5], vec![0.5]))).unwrap();
    // Y locked: its cutters dropped, the mask the one truth.
    h.apply(set(behavior(2, vec![0.25, 0.75], vec![0.3]))).unwrap();
    let saved = &h.document().component(part).unwrap().behavior;
    assert_eq!(*saved, behavior(2, vec![0.25, 0.75], vec![]));
    assert!(saved.no_stretch.locks(1) && !saved.no_stretch.locks(0));
    assert!(saved.no_stretch.locks_along(DVec3::new(0.6, 0.8, 0.0)) && !saved.no_stretch.locks_along(DVec3::X));
    assert!(h.undo());
    assert!(h.document().component(part).unwrap().behavior.is_default());
    assert!(h.redo());

    // Saved and read back.
    let json = h.document().to_json().unwrap();
    assert_eq!(Document::from_json(&json).unwrap(), *h.document());
    // A file with cutters out of ]0, 1[ is refused.
    let bad = json.replacen("[0.25,0.75]", "[0.25,1.75]", 1);
    assert_ne!(bad, json);
    assert!(matches!(Document::from_json(&bad), Err(Error::File(FileError::BadBehavior(id))) if id == part));
    // So is such an operation.
    assert!(matches!(h.document().prepare(&set(behavior(0, vec![0.6, 0.4], vec![0.5]))), Err(Error::Invalid(Refusal::BadBehavior))));
    assert!(matches!(h.document().prepare(&set(behavior(128, vec![0.5], vec![0.5]))), Err(Error::Invalid(Refusal::BadBehavior))));

    // A separate part keeps it.
    h.apply(Operation::MakeUnique { context: root, occurrence: b1 }).unwrap();
    let copy = occ(h.document(), &EntityPath::root(b1)).component;
    assert_ne!(copy, part);
    assert_eq!(h.document().component(copy).unwrap().behavior, behavior(2, vec![0.25, 0.75], vec![]));
}

#[test]
fn a_stretch_sets_its_targets_cutters() {
    let (mut h, part, [b1, ..]) = three_boards();
    h.apply(Operation::SetBehavior { component: part, behavior: Behavior { no_stretch: StretchMask(4), cutters: None } }).unwrap();
    let cutters = [vec![0.4], vec![0.5], vec![0.5]];
    let op = match stretch_x(h.document(), &EntityPath::root(b1), 100.0, 50.0, Scope::This) {
        Operation::Stretch { context, occurrences, frame, axis, cuts, moves, scope, .. } => {
            Operation::Stretch { context, occurrences, frame, axis, cuts, moves, scope, cutters: Some(Box::new(cutters.clone())) }
        }
        _ => unreachable!(),
    };
    h.apply(op).unwrap();
    // On the copy the stretch made, its lock kept (and its cutters dropped); the others' part
    // as it was.
    let copy = occ(h.document(), &EntityPath::root(b1)).component;
    assert_ne!(copy, part);
    let b = &h.document().component(copy).unwrap().behavior;
    assert_eq!(*b, Behavior { no_stretch: StretchMask(4), cutters: Some([vec![0.4], vec![0.5], vec![]]) });
    assert_eq!(h.document().component(part).unwrap().behavior.cutters, None);
}

#[test]
fn a_locked_part_is_a_block_moved_with_its_origin() {
    // A board 0–200 along X and a hinge across the cutter at 100, in an assembly.
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let hinge = add(&mut h, cube(DVec3::new(90.0, 0.0, 18.0), 20.0));
    let a = h.apply(Operation::MakeAssembly { context: root, entities: vec![board, hinge], name: None }).unwrap().created[0];
    let path = EntityPath::new(vec![a], hinge);
    let hinge_part = occ(h.document(), &path).component;
    let origin = occ(h.document(), &path).transform.translation.x;
    assert!(origin < 100.0, "the hinge's origin at {origin}");
    let frame = h.document().resolve(&EntityPath::root(a)).unwrap().transform;
    let op = || Operation::Stretch {
        context: vec![],
        occurrences: vec![a],
        frame,
        axis: DVec3::X,
        cuts: vec![100.0],
        moves: vec![0.0, 50.0],
        scope: Scope::All,
        cutters: None,
    };
    // Free, the hinge is stretched.
    let out = h.apply(op()).unwrap();
    assert!(out.stretched.iter().any(|s| s.from == hinge_part));
    let (lo, hi) = world_bounds(h.document(), &path);
    assert!((lo.x - 90.0).abs() < 1e-9 && (hi.x - 160.0).abs() < 1e-9, "{lo} {hi}");
    h.undo();

    // Locked along X (all axes, as OCL's hardware): moved with its origin's section — here
    // not at all — and not stretched; the board is.
    let all = Behavior { no_stretch: StretchMask(StretchMask::ALL), cutters: None };
    h.apply(Operation::SetBehavior { component: hinge_part, behavior: all }).unwrap();
    // The compression limit counts it whole in its origin's section.
    let spans = h.document().stretch_spans(&[], &[a], &frame, DVec3::X, &[100.0]).unwrap();
    assert!(spans[0].is_some_and(|(_, hi)| (hi - 110.0).abs() < 1e-9), "{spans:?}");
    let out = h.apply(op()).unwrap();
    assert!(!out.stretched.iter().any(|s| s.from == hinge_part));
    assert_eq!(occ(h.document(), &path).component, hinge_part);
    let (lo, hi) = world_bounds(h.document(), &path);
    assert!((lo.x - 90.0).abs() < 1e-9 && (hi.x - 110.0).abs() < 1e-9, "{lo} {hi}");
    let (lo, hi) = world_bounds(h.document(), &EntityPath::new(vec![a], board));
    assert!((lo.x).abs() < 1e-9 && (hi.x - 250.0).abs() < 1e-9, "{lo} {hi}");
    h.undo();

    // Locked along Y only: stretched along X as before.
    let y = Behavior { no_stretch: StretchMask(2), cutters: None };
    h.apply(Operation::SetBehavior { component: hinge_part, behavior: y }).unwrap();
    let out = h.apply(op()).unwrap();
    assert!(out.stretched.iter().any(|s| s.from == hinge_part));
}

/// What `placed_changes` should find, the long way: every placement compared by path.
fn placed_changes_by_path(after: &Document, before: &Document, touched: &[Id]) -> Vec<(EntityPath, Id, Id)> {
    let was: std::collections::HashMap<EntityPath, PlacedPart> = before.placed_parts().into_iter().map(|p| (p.path.clone(), p)).collect();
    let mut out: Vec<(EntityPath, Id, Id)> = after
        .placed_parts()
        .into_iter()
        .filter_map(|p| {
            let b = was.get(&p.path)?;
            let changed = p.part != b.part
                || !Arc::ptr_eq(&p.shape, &b.shape)
                || !p.transform.abs_diff_eq(b.transform, 1e-9)
                || touched.contains(&p.part);
            changed.then_some((p.path, p.part, b.part))
        })
        .collect();
    out.sort_by_key(|(p, _, _)| format!("{p:?}"));
    out
}

fn placed_changes_found(after: &Document, before: &Document, touched: &[Id]) -> Vec<(EntityPath, Id, Id)> {
    let mut out: Vec<(EntityPath, Id, Id)> =
        after.placed_changes(before, touched).into_iter().map(|(p, was, _)| (p.path, p.part, was)).collect();
    out.sort_by_key(|(p, _, _)| format!("{p:?}"));
    out
}

#[test]
fn placed_changes_skip_only_what_is_shared() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 100.0));
    let b = add(&mut h, cube(DVec3::X * 300.0, 100.0));
    let root = h.document().root();
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a], name: None }).unwrap().created[0];
    h.apply(Operation::Duplicate { context: root, entities: vec![asm], transform: DAffine3::from_translation(DVec3::Y * 300.0) }).unwrap();
    let before = h.document().clone();
    let component = before.resolve(&EntityPath::root(asm)).unwrap().component;
    let inner = before.assembly(component).unwrap().occurrences[0].id;
    let part_b = before.resolve(&EntityPath::root(b)).unwrap().component;

    // Nothing changed: nothing found; a part named touched: each of its placements.
    assert!(before.placed_changes(&before, &[]).is_empty());
    assert_eq!(placed_changes_found(&before, &before, &[part_b]), placed_changes_by_path(&before, &before, &[part_b]));
    assert_eq!(placed_changes_found(&before, &before, &[part_b]).len(), 1);

    // The part inside the assembly stretched for all: both copies of the assembly — the
    // assembly itself the same, holding a changed part.
    let stretch = |scope| Operation::Stretch {
        context: vec![asm],
        occurrences: vec![inner],
        frame: DAffine3::IDENTITY,
        axis: DVec3::Z,
        cuts: vec![50.0],
        moves: vec![0.0, 40.0],
        scope,
        cutters: None,
    };
    for scope in [Scope::All, Scope::This] {
        h.apply(stretch(scope)).unwrap();
        let after = h.document();
        let found = placed_changes_found(after, &before, &[]);
        assert_eq!(found, placed_changes_by_path(after, &before, &[]), "{scope:?}");
        assert_eq!(found.len(), if scope == Scope::All { 2 } else { 1 }, "{scope:?}");
        h.undo();
    }

    // B moved: only it.
    h.apply(Operation::Transform { context: root, entities: vec![b], transform: DAffine3::from_translation(DVec3::Z) }).unwrap();
    let found = placed_changes_found(h.document(), &before, &[]);
    assert_eq!(found, placed_changes_by_path(h.document(), &before, &[]));
    assert_eq!(found.len(), 1);
}

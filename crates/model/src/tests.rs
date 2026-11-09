use glam::{DAffine3, DVec3};

use crate::*;

fn cube(at: DVec3, size: f64) -> Primitive {
    Primitive::Box { origin: at, size: DVec3::splat(size) }
}

/// Adds a primitive at the root, returns its id.
fn add(h: &mut History, p: Primitive) -> Id {
    let root = h.document().root();
    h.apply(Operation::AddPrimitive { context: root, primitive: p, placement: glam::DAffine3::IDENTITY }).unwrap().created[0]
}

fn solid_x_min(doc: &Document, path: &EntityPath) -> f64 {
    doc.resolve(path).unwrap().as_solid().unwrap().solid.vertex_bounds().unwrap().0.x
}

#[test]
fn add_undo_redo() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    assert_eq!(h.document().root_definition().entities.len(), 1);
    assert!(h.undo());
    assert!(h.document().root_definition().entities.is_empty());
    assert!(h.redo());
    assert_eq!(h.document().root_definition().entities[0].id(), a);
    assert_eq!(h.journal().count(), 1);
}

#[test]
fn snapshots_share_untouched_solids() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let before = h.document().resolve(&EntityPath::root(a)).unwrap().as_solid().unwrap().solid.clone();
    add(&mut h, cube(DVec3::splat(20.0), 5.0));
    let after = h.document().resolve(&EntityPath::root(a)).unwrap().as_solid().unwrap().solid.clone();
    assert!(std::sync::Arc::ptr_eq(&before, &after));
}

#[test]
fn group_move_explode() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let g = h
        .apply(Operation::MakeContainer { context: root, entities: vec![a, b], kind: DefinitionKind::Group, name: None })
        .unwrap()
        .created[0];
    let doc = h.document();
    assert_eq!(doc.root_definition().entities.len(), 1);
    let gdef = doc.resolve(&EntityPath::root(g)).unwrap().as_instance().unwrap().definition;
    assert_eq!(doc.definition(gdef).unwrap().kind, DefinitionKind::Group);
    assert_eq!(doc.definition(gdef).unwrap().name, "Groupe");

    // Moving the group moves its transform, not its solids.
    let t = DAffine3::from_translation(DVec3::new(100.0, 0.0, 0.0));
    h.apply(Operation::Transform { context: root, entities: vec![g], transform: t }).unwrap();
    let placed = h.document().placed_solids();
    assert_eq!(placed.len(), 2);
    assert!(placed.iter().all(|(p, t, _)| p.instances == vec![g] && t.translation.x == 100.0));
    assert_eq!(solid_x_min(h.document(), &EntityPath::new(vec![g], a)), 0.0);

    // Exploding bakes the transform into the solids and drops the group's definition.
    let out = h.apply(Operation::Explode { context: root, instance: g }).unwrap();
    let doc = h.document();
    assert_eq!(out.created.len(), 2);
    assert!(doc.definition(gdef).is_err(), "orphan group purged");
    let xs: Vec<f64> = out.created.iter().map(|id| solid_x_min(doc, &EntityPath::root(*id))).collect();
    assert_eq!(xs, vec![100.0, 120.0]);
}

#[test]
fn component_instances_share_edits() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let i1 = h
        .apply(Operation::MakeContainer {
            context: root,
            entities: vec![a],
            kind: DefinitionKind::Component,
            name: Some("Pied".into()),
        })
        .unwrap()
        .created[0];
    let def = h.document().resolve(&EntityPath::root(i1)).unwrap().as_instance().unwrap().definition;
    let i2 = h
        .apply(Operation::PlaceInstance {
            context: root,
            definition: def,
            transform: DAffine3::from_translation(DVec3::new(50.0, 0.0, 0.0)),
        })
        .unwrap()
        .created[0];
    assert_eq!(h.document().instance_count(def), 2);
    assert_eq!(h.document().placed_solids().len(), 2);

    // Editing the definition (adding a solid into it) shows in both instances.
    h.apply(Operation::AddPrimitive { context: def, primitive: cube(DVec3::new(0.0, 0.0, 10.0), 2.0), placement: glam::DAffine3::IDENTITY }).unwrap();
    assert_eq!(h.document().placed_solids().len(), 4);

    // A component cannot contain itself.
    let err = h.apply(Operation::PlaceInstance { context: def, definition: def, transform: DAffine3::IDENTITY });
    assert!(matches!(err, Err(Error::Cycle)));

    // Make unique: i2 gets its own copy; editing it leaves i1 alone.
    h.apply(Operation::MakeUnique { context: root, instance: i2 }).unwrap();
    let def2 = h.document().resolve(&EntityPath::root(i2)).unwrap().as_instance().unwrap().definition;
    assert_ne!(def, def2);
    assert_eq!(h.document().definition(def2).unwrap().name, "Pied#2");
    h.apply(Operation::AddPrimitive { context: def2, primitive: cube(DVec3::new(0.0, 0.0, 20.0), 2.0), placement: glam::DAffine3::IDENTITY }).unwrap();
    assert_eq!(h.document().definition(def).unwrap().entities.len(), 2);
    assert_eq!(h.document().definition(def2).unwrap().entities.len(), 3);

    // A component with two instances cannot become a group; with one it can.
    h.apply(Operation::PlaceInstance { context: root, definition: def2, transform: DAffine3::IDENTITY }).unwrap();
    assert!(h.apply(Operation::SetKind { definition: def2, kind: DefinitionKind::Group }).is_err());
    h.apply(Operation::SetKind { definition: def, kind: DefinitionKind::Group }).unwrap();
}

#[test]
fn duplicating_a_group_copies_its_definition() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let g = h
        .apply(Operation::MakeContainer { context: root, entities: vec![a], kind: DefinitionKind::Group, name: None })
        .unwrap()
        .created[0];
    let c = h
        .apply(Operation::Duplicate {
            context: root,
            entities: vec![g],
            transform: DAffine3::from_translation(DVec3::X * 30.0),
        })
        .unwrap()
        .created[0];
    let doc = h.document();
    let d1 = doc.resolve(&EntityPath::root(g)).unwrap().as_instance().unwrap().definition;
    let d2 = doc.resolve(&EntityPath::root(c)).unwrap().as_instance().unwrap().definition;
    assert_ne!(d1, d2, "a group has a single instance");
}

#[test]
fn boolean_across_contexts() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    // Target: a 20 mm cube inside a group moved by +100 X.
    let a = add(&mut h, cube(DVec3::ZERO, 20.0));
    let g = h
        .apply(Operation::MakeContainer { context: root, entities: vec![a], kind: DefinitionKind::Group, name: None })
        .unwrap()
        .created[0];
    h.apply(Operation::Transform {
        context: root,
        entities: vec![g],
        transform: DAffine3::from_translation(DVec3::X * 100.0),
    })
    .unwrap();
    // Tool: a cylinder at the root, through the middle of the moved cube.
    let tool = add(&mut h, Primitive::Cylinder { origin: DVec3::new(110.0, 10.0, -5.0), radius: 4.0, height: 30.0 });

    h.apply(Operation::Boolean {
        target: EntityPath::new(vec![g], a),
        tool: EntityPath::root(tool),
        op: Boolean::Difference,
        keep_tool: false,
    })
    .unwrap();
    let doc = h.document();
    assert_eq!(doc.root_definition().entities.len(), 1, "tool consumed");
    let s = doc.resolve(&EntityPath::new(vec![g], a)).unwrap().as_solid().unwrap();
    // The hole is in the group's coordinates: the cube still spans 0..20 there.
    let (lo, hi) = s.solid.vertex_bounds().unwrap();
    assert!(lo.x.abs() < 1e-9 && (hi.x - 20.0).abs() < 1e-9);
    assert!(s.solid.topology_counts().0 >= 7, "a hole adds faces: {:?}", s.solid.topology_counts());
}

#[test]
fn errors_leave_the_document_unchanged() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let before = h.document().clone();
    let r = h.apply(Operation::AddPrimitive { context: root, primitive: cube(DVec3::ZERO, -1.0), placement: glam::DAffine3::IDENTITY });
    assert!(matches!(r, Err(Error::Kernel(_))));
    assert_eq!(*h.document(), before);
    assert!(!h.can_undo());
    assert!(h.apply(Operation::Delete { context: root, entities: vec![Id(999)] }).is_err());
}

#[test]
fn file_round_trip() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, Primitive::Torus { center: DVec3::ZERO, major_radius: 10.0, minor_radius: 2.0 });
    h.apply(Operation::MakeContainer { context: root, entities: vec![a], kind: DefinitionKind::Component, name: None })
        .unwrap();
    let json = h.document().to_json().unwrap();
    let back = Document::from_json(&json).unwrap();
    assert_eq!(back, *h.document());
    assert!(Document::from_json(r#"{"format":"other","version":1}"#).is_err());
    assert!(Document::from_json(&json.replacen("\"version\":1", "\"version\":99", 1)).is_err());
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
    // A file written before settings existed loads with the defaults.
    let old = Document::new().to_json().unwrap();
    let start = old.find(",\"settings\"").unwrap();
    let end = start + old[start..].find("}}}").unwrap() + 2;
    let old = format!("{}{}", &old[..start], &old[end..]);
    assert!(!old.contains("settings"), "{old}");
    assert_eq!(Document::from_json(&old).unwrap().settings(), &Settings::default());
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
fn array_repeats_the_step() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let root = h.document().root();
    let step = DAffine3::from_translation(DVec3::new(20.0, 0.0, 0.0));
    let out = h.apply(Operation::Array { context: root, entities: vec![a], step, count: 3 }).unwrap();
    assert_eq!(out.created.len(), 3);
    let xs: Vec<f64> = out.created.iter().map(|id| solid_x_min(h.document(), &EntityPath::new(vec![], *id))).collect();
    assert!(xs.iter().zip([20.0, 40.0, 60.0]).all(|(x, e)| (x - e).abs() < 1e-9), "{xs:?}");
    assert_eq!(h.journal().count(), 2); // one step
    assert!(h.apply(Operation::Array { context: root, entities: vec![a], step, count: 0 }).is_err());
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

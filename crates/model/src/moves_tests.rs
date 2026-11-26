//! Bodies moved in a part's tree (decision 0044, docs/model/bodies.md#moving-bodies).

use glam::{DAffine3, DVec3};
use skraft_kernel::names::{Blend, NodeRefusal};

use crate::*;

fn volume(doc: &Document, part: Id) -> f64 {
    doc.part(part).unwrap().shape().volume().unwrap()
}

const HOLE: f64 = std::f64::consts::PI * 16.0 * 18.0;

/// A board 200 × 50 × 18 (its path, part, board body) with drills ø8 at `xs` (their bodies).
fn drilled(h: &mut History, xs: &[f64]) -> (EntityPath, Id, Id, Vec<Id>) {
    let root = h.document().root();
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) };
    let occ = h
        .apply(Operation::AddPrimitive { context: root, primitive: board, placement: DAffine3::IDENTITY, material: None })
        .unwrap()
        .created[0];
    let target = EntityPath::root(occ);
    let (part, p, _) = h.document().resolve_part(&target).unwrap();
    let board = p.bodies[0].id;
    let drills = xs.iter().map(|x| drill(h, &target, *x)).collect();
    (target, part, board, drills)
}

/// A drill ø8 through the board at `x`, added to the level `target` names.
fn drill(h: &mut History, target: &EntityPath, x: f64) -> Id {
    let add = Operation::AddBody {
        target: target.clone(),
        primitive: Primitive::Cylinder { origin: DVec3::new(x, 25.0, -5.0), radius: 4.0, height: 30.0 },
        placement: DAffine3::IDENTITY,
        sign: Sign::Negative,
        scope: Scope::All,
    };
    h.apply(add).unwrap().bodies[0]
}

/// A round of `size` on the edge nearest `at`, wrapping `bodies` of `target`.
fn round(h: &mut History, target: &EntityPath, bodies: Vec<Id>, size: f64, at: DVec3) -> Id {
    let op = Operation::Blend { target: target.clone(), bodies, blend: Blend::Round, size, picks: vec![at], scope: Scope::All };
    h.apply(op).unwrap().bodies[0]
}

fn move_to(from: EntityPath, bodies: Vec<Id>, to: Vec<Id>) -> Operation {
    Operation::MoveBodies { from, bodies, to: BodiesTo::Level(to), copy: false, scope: Scope::All }
}

/// What a round of `r` along `length` takes off a square edge.
fn round_off(r: f64, length: f64) -> f64 {
    (1.0 - std::f64::consts::FRAC_PI_4) * r * r * length
}

#[test]
fn a_drill_taken_out_of_a_round_keeps_itself_and_the_round_its_edge() {
    let mut h = History::new(Document::new());
    let (target, part, board, drills) = drilled(&mut h, &[20.0, 150.0]);
    let node = round(&mut h, &target, vec![board, drills[0], drills[1]], 3.0, DVec3::new(100.0, 0.0, 18.0));
    let sharp = 200.0 * 50.0 * 18.0 - 2.0 * HOLE - round_off(3.0, 200.0);
    assert!((volume(h.document(), part) - sharp).abs() < 1e-2);
    // The second drill out of the node, to the part's root: the same body, where it was.
    let out = h.apply(move_to(target.clone().in_group(vec![node]), vec![drills[1]], Vec::new())).unwrap();
    assert_eq!(out.bodies, vec![drills[1]], "its id kept");
    assert_eq!(out.warning, None);
    let p = h.document().part(part).unwrap();
    assert!(p.bodies.iter().any(|b| b.id == drills[1] && b.sign == Sign::Negative), "at the root");
    assert!(p.def_of(p.body(node).unwrap()).children().iter().all(|c| c.id != drills[1]), "out of the node");
    assert_eq!(p.refusal_of(node), None);
    assert!((volume(h.document(), part) - sharp).abs() < 1e-2, "{}", volume(h.document(), part));
    // And back into it.
    h.apply(move_to(target.clone(), vec![drills[1]], vec![node])).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.def_of(p.body(node).unwrap()).children().iter().any(|c| c.id == drills[1]));
    assert!((volume(h.document(), part) - sharp).abs() < 1e-2);
}

#[test]
fn a_node_whose_edges_were_on_a_body_taken_out_is_in_error_and_says_so() {
    let mut h = History::new(Document::new());
    let (target, part, board, drills) = drilled(&mut h, &[100.0]);
    // The drill's rim on the board's top rounded (picked half way round).
    let node = round(&mut h, &target, vec![board, drills[0]], 1.0, DVec3::new(100.0, 29.0, 18.0));
    let before = h.document().clone();
    let out = h.apply(move_to(target.clone().in_group(vec![node]), drills.clone(), Vec::new())).unwrap();
    assert!(matches!(&out.warning, Some(Warning::Moved { in_error, .. }) if in_error.len() == 1), "{:?}", out.warning);
    let p = h.document().part(part).unwrap();
    assert!(matches!(p.refusal_of(node), Some(NodeRefusal::Lost(_))), "{:?}", p.refusal_of(node));
    // Undone: as it was.
    h.undo();
    assert_eq!(*h.document(), before);
}

#[test]
fn a_node_over_both_levels_follows_a_body_moved_between_its_groups() {
    let mut h = History::new(Document::new());
    let (target, part, board, drills) = drilled(&mut h, &[20.0]);
    let group = |h: &mut History, bodies: Vec<Id>| {
        h.apply(Operation::GroupBodies { target: target.clone(), bodies, scope: Scope::All }).unwrap().bodies[0]
    };
    let g1 = group(&mut h, vec![board, drills[0]]);
    // A second group: a block beside the board.
    let root = target.clone();
    let block = h
        .apply(Operation::AddBody {
            target: root.clone(),
            primitive: Primitive::Box { origin: DVec3::new(0.0, 100.0, 0.0), size: DVec3::new(40.0, 40.0, 18.0) },
            placement: DAffine3::IDENTITY,
            sign: Sign::Positive,
            scope: Scope::All,
        })
        .unwrap()
        .bodies[0];
    let g2 = group(&mut h, vec![block]);
    // The round over both, on the board's front top edge: its name through g1.
    let node = round(&mut h, &target, vec![g1, g2], 2.0, DVec3::new(100.0, 0.0, 18.0));
    let v = volume(h.document(), part);
    let name_path = |h: &History| {
        let p = h.document().part(part).unwrap();
        p.def_of(p.body(node).unwrap()).op().unwrap().edges()[0].name.faces[0].path.clone()
    };
    assert_eq!(name_path(&h), vec![g1.0, board.0]);
    // The board moved from g1 to g2: the round finds its edge through g2.
    h.apply(move_to(target.clone().in_group(vec![node, g1]), vec![board], vec![node, g2])).unwrap();
    assert_eq!(name_path(&h), vec![g2.0, board.0]);
    let p = h.document().part(part).unwrap();
    assert_eq!(p.refusal_of(node), None);
    // The drill left in g1 cuts nothing there any more (its board gone): the hole is filled.
    assert!((volume(h.document(), part) - (v + HOLE)).abs() < 1e-2, "{} vs {}", volume(h.document(), part), v + HOLE);
}

#[test]
fn a_shared_group_left_is_made_unique_the_other_copies_keep_the_body() {
    let mut h = History::new(Document::new());
    let (target, part, board, drills) = drilled(&mut h, &[20.0]);
    let g =
        h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![board, drills[0]], scope: Scope::All }).unwrap().bodies[0];
    let copy = Operation::CopyBodies {
        target: target.clone(),
        bodies: vec![g],
        moved: DAffine3::IDENTITY,
        placements: vec![DAffine3::from_translation(DVec3::Y * 80.0)],
        scope: Scope::All,
    };
    let g2 = h.apply(copy).unwrap().bodies[0];
    let v = volume(h.document(), part);
    // The drill out of the first copy: that copy its own, the second keeps its drill.
    let out = h.apply(move_to(target.clone().in_group(vec![g]), drills.clone(), Vec::new())).unwrap();
    assert!(matches!(&out.warning, Some(Warning::Moved { made_unique, .. }) if made_unique.len() == 1), "{:?}", out.warning);
    let p = h.document().part(part).unwrap();
    let (d1, d2) = (p.def_of(p.body(g).unwrap()), p.def_of(p.body(g2).unwrap()));
    assert_ne!(d1.id, d2.id);
    assert_eq!((d1.children().len(), d2.children().len()), (1, 2));
    // The drill at the root, a new id (the copy's children renamed): the same shape.
    assert_eq!(p.bodies.len(), 3);
    assert!((volume(h.document(), part) - v).abs() < 1e-2);
}

#[test]
fn a_body_out_of_a_machining_group_keeps_taking_away() {
    let mut h = History::new(Document::new());
    let (target, part, _, drills) = drilled(&mut h, &[20.0, 60.0]);
    // Two drills grouped: a negative group of positive children.
    let g = h.apply(Operation::GroupBodies { target: target.clone(), bodies: drills.clone(), scope: Scope::All }).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.body(g).unwrap().sign, Sign::Negative);
    let v = volume(h.document(), part);
    h.apply(move_to(target.clone().in_group(vec![g]), vec![drills[0]], Vec::new())).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.body(drills[0]).unwrap().sign, Sign::Negative, "still a drill");
    assert!((volume(h.document(), part) - v).abs() < 1e-2);
}

#[test]
fn a_body_out_of_the_part_becomes_a_part_beside_it() {
    let mut h = History::new(Document::new());
    let (target, part, _, drills) = drilled(&mut h, &[20.0]);
    let parts = |h: &History| h.document().assembly(h.document().root()).unwrap().occurrences().len();
    let out = h
        .apply(Operation::MoveBodies {
            from: target.clone(),
            bodies: drills.clone(),
            to: BodiesTo::NewPart(Vec::new()),
            copy: false,
            scope: Scope::All,
        })
        .unwrap();
    assert_eq!(parts(&h), 2);
    assert_eq!(out.created.len(), 1);
    // The board whole again; the drill a pin (a hole's shape as a part).
    assert!((volume(h.document(), part) - 200.0 * 50.0 * 18.0).abs() < 1e-2);
    let pin = h.document().resolve_part(&EntityPath::root(out.created[0])).unwrap().1.shape().volume().unwrap();
    assert!((pin - std::f64::consts::PI * 16.0 * 30.0).abs() < 1e-2, "{pin}");
}

#[test]
fn a_copy_leaves_the_body_where_it_was_and_into_itself_is_refused() {
    let mut h = History::new(Document::new());
    let (target, part, board, drills) = drilled(&mut h, &[20.0]);
    let node = round(&mut h, &target, vec![board, drills[0]], 3.0, DVec3::new(100.0, 0.0, 18.0));
    let copy = Operation::MoveBodies {
        from: target.clone().in_group(vec![node]),
        bodies: drills.clone(),
        to: BodiesTo::Level(Vec::new()),
        copy: true,
        scope: Scope::All,
    };
    let out = h.apply(copy).unwrap();
    assert_ne!(out.bodies, drills, "a new body");
    let p = h.document().part(part).unwrap();
    assert!(p.def_of(p.body(node).unwrap()).children().iter().any(|c| c.id == drills[0]), "the first one left");
    assert_eq!(p.refusal_of(node), None);
    // The node into itself.
    assert!(matches!(h.apply(move_to(target.clone(), vec![node], vec![node])), Err(Error::Cycle)));
}

/// A block 200 × 50 × 18 at `z`, a part of its own at the model's root: its occurrence.
fn block(h: &mut History, z: f64) -> Id {
    let root = h.document().root();
    let b = Primitive::Box { origin: DVec3::new(0.0, 0.0, z), size: DVec3::new(200.0, 50.0, 18.0) };
    h.apply(Operation::AddPrimitive { context: root, primitive: b, placement: DAffine3::IDENTITY, material: None }).unwrap().created[0]
}

fn part_volume(h: &History, occurrence: Id) -> f64 {
    h.document().resolve_part(&EntityPath::root(occurrence)).unwrap().1.shape().volume().unwrap()
}

#[test]
fn a_body_moved_into_another_part_keeps_its_world_place() {
    let mut h = History::new(Document::new());
    // The drill (z −5 to 25) through the board, into a block above it (z 20 to 38): it drills
    // 5 mm of the block once there, the board whole again.
    let (target, part, _, drills) = drilled(&mut h, &[20.0]);
    let above = block(&mut h, 20.0);
    let full = 200.0 * 50.0 * 18.0;
    let op = Operation::MoveBodies {
        from: target.clone(),
        bodies: drills.clone(),
        to: BodiesTo::Part(EntityPath::root(above)),
        copy: false,
        scope: Scope::All,
    };
    let out = h.apply(op).unwrap();
    assert_eq!(out.bodies.len(), 1);
    assert!((volume(h.document(), part) - full).abs() < 1e-2);
    let pin = std::f64::consts::PI * 16.0 * 5.0;
    assert!((part_volume(&h, above) - (full - pin)).abs() < 1e-2, "{}", part_volume(&h, above));
    let (_, q, _) = h.document().resolve_part(&EntityPath::root(above)).unwrap();
    assert_eq!(q.body(out.bodies[0]).unwrap().sign, Sign::Negative, "still a drill");
}

#[test]
fn a_body_moved_into_an_assembly_becomes_a_part_there() {
    let mut h = History::new(Document::new());
    let (target, _, _, drills) = drilled(&mut h, &[20.0]);
    let other = block(&mut h, 100.0);
    let root = h.document().root();
    let assembly = h.apply(Operation::MakeAssembly { context: root, entities: vec![other], name: None }).unwrap().created[0];
    let op = Operation::MoveBodies {
        from: target.clone(),
        bodies: drills.clone(),
        to: BodiesTo::NewPart(vec![assembly]),
        copy: false,
        scope: Scope::All,
    };
    h.apply(op).unwrap();
    let doc = h.document();
    let inside = doc.context_of(&[assembly]).unwrap();
    assert_eq!(doc.assembly(inside).unwrap().occurrences().len(), 2, "the block and the drill's part");
}

#[test]
fn a_part_moved_into_another_becomes_its_body_and_not_into_itself() {
    let mut h = History::new(Document::new());
    let (target, part, _, _) = drilled(&mut h, &[20.0]);
    let above = block(&mut h, 20.0);
    let before = volume(h.document(), part);
    let op = Operation::MoveParts { from: Vec::new(), occurrences: vec![above], to: target.clone(), copy: false };
    let out = h.apply(op).unwrap();
    let root = h.document().root();
    assert_eq!(h.document().assembly(root).unwrap().occurrences().len(), 1, "the block's occurrence gone");
    let p = h.document().part(part).unwrap();
    assert_eq!(p.body(out.bodies[0]).unwrap().sign, Sign::Positive);
    // The block joined above the board (the drill cuts 5 mm of it, as a body of the part).
    let pin = std::f64::consts::PI * 16.0 * 5.0;
    assert!((volume(h.document(), part) - (before + 200.0 * 50.0 * 18.0 - pin)).abs() < 1e-2, "{}", volume(h.document(), part));
    // A part into itself.
    let root = h.document().root();
    let own = h.document().assembly(root).unwrap().occurrences()[0].id;
    let into_itself = Operation::MoveParts { from: Vec::new(), occurrences: vec![own], to: target.clone(), copy: false };
    assert!(matches!(h.apply(into_itself), Err(Error::Cycle)));
}

#[test]
fn an_occurrence_moved_into_another_assembly_keeps_its_world_place() {
    let mut h = History::new(Document::new());
    let a = block(&mut h, 0.0);
    let b = block(&mut h, 100.0);
    let root = h.document().root();
    let s = h.apply(Operation::MakeAssembly { context: root, entities: vec![b], name: None }).unwrap().created[0];
    // The assembly moved off: its axes are not the model's.
    h.apply(Operation::Transform { context: root, entities: vec![s], transform: DAffine3::from_translation(DVec3::new(30.0, 40.0, 0.0)) })
        .unwrap();
    let world = |h: &History, path: &[Id], id: Id| {
        let doc = h.document();
        let o = doc.assembly(doc.context_of(path).unwrap()).unwrap().occurrences().iter().find(|o| o.id == id).unwrap().clone();
        doc.transform_of(path).unwrap() * o.transform
    };
    let before = world(&h, &[], a);
    let out = h.apply(Operation::MoveOccurrences { from: Vec::new(), occurrences: vec![a], to: vec![s], copy: false }).unwrap();
    assert_eq!(out.created, vec![a], "its id kept");
    assert!(world(&h, &[s], a).abs_diff_eq(before, 1e-9), "where it was");
    assert_eq!(h.document().assembly(root).unwrap().occurrences().len(), 1, "out of the model's root");
    // Back out, a copy: the one in the assembly left.
    let out = h.apply(Operation::MoveOccurrences { from: vec![s], occurrences: vec![a], to: Vec::new(), copy: true }).unwrap();
    assert!(world(&h, &[], out.created[0]).abs_diff_eq(before, 1e-9));
    assert_eq!(h.document().assembly(h.document().context_of(&[s]).unwrap()).unwrap().occurrences().len(), 2);
    // The assembly into itself.
    let into_itself = Operation::MoveOccurrences { from: Vec::new(), occurrences: vec![s], to: vec![s], copy: false };
    assert!(matches!(h.apply(into_itself), Err(Error::Cycle)));
}

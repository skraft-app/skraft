//! The intersection, a third sign of bodies (docs/decisions/0045-intersection-a-third-sign.md).

use glam::{DAffine3, DVec3};

use crate::*;

fn volume(doc: &Document, part: Id) -> f64 {
    doc.part(part).unwrap().shape().volume().unwrap()
}

/// A board 200 × 50 × 18 (its path, part, board body).
fn board(h: &mut History) -> (EntityPath, Id, Id) {
    let root = h.document().root();
    let b = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) };
    let occ =
        h.apply(Operation::AddPrimitive { context: root, primitive: b, placement: DAffine3::IDENTITY, material: None }).unwrap().created[0];
    let target = EntityPath::root(occ);
    let (part, p, _) = h.document().resolve_part(&target).unwrap();
    let board = p.bodies[0].id;
    (target, part, board)
}

/// A body of `sign`, a box from `lo` of `size`, added to the level `target` names.
fn add(h: &mut History, target: &EntityPath, lo: DVec3, size: DVec3, sign: Sign) -> Id {
    let op = Operation::AddBody {
        target: target.clone(),
        primitive: Primitive::Box { origin: lo, size },
        placement: DAffine3::IDENTITY,
        sign,
        scope: Scope::All,
    };
    h.apply(op).unwrap().bodies[0]
}

#[test]
fn an_intersection_keeps_what_lies_inside_it_and_each_limits() {
    let mut h = History::new(Document::new());
    let (target, part, _) = board(&mut h);
    // An intersection over the left half: the board's left half kept, added as the shape it was cut by.
    let left = add(&mut h, &target, DVec3::new(-10.0, -10.0, -10.0), DVec3::new(110.0, 70.0, 40.0), Sign::Intersect);
    assert!((volume(h.document(), part) - 100.0 * 50.0 * 18.0).abs() < 1e-2, "{}", volume(h.document(), part));
    let Prepared::Kernel(job) = h
        .document()
        .prepare(&Operation::AddBody {
            target: target.clone(),
            primitive: Primitive::Box { origin: DVec3::new(50.0, -10.0, -10.0), size: DVec3::new(200.0, 70.0, 40.0) },
            placement: DAffine3::IDENTITY,
            sign: Sign::Intersect,
            scope: Scope::All,
        })
        .unwrap()
    else {
        panic!("a kernel job")
    };
    // An intersection added: the shape as it is, limited by it alone.
    assert!(
        job.tasks.iter().any(|t| matches!(t, kernel::Task::Recipe { positives, negatives, intersects, base: None, .. } if positives.len() == 1 && negatives.is_empty() && intersects.len() == 1)),
        "{:?}",
        job.tasks
    );
    // A second intersection over x 50 to 250: each limits, x 50 to 100 left.
    add(&mut h, &target, DVec3::new(50.0, -10.0, -10.0), DVec3::new(200.0, 70.0, 40.0), Sign::Intersect);
    assert!((volume(h.document(), part) - 50.0 * 50.0 * 18.0).abs() < 1e-2, "{}", volume(h.document(), part));
    // Its sign written and read back.
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap().body(left).unwrap().sign, Sign::Intersect);
    // The navigator's cycle: an intersection made a blank again, the board and it joined.
    h.apply(Operation::SetSign { target: target.clone(), bodies: vec![left], sign: Sign::Positive, scope: Scope::All }).unwrap();
    assert!(volume(h.document(), part) > 50.0 * 50.0 * 18.0);
}

#[test]
fn intersections_do_not_group_or_ungroup_together_but_one_does() {
    let mut h = History::new(Document::new());
    let (target, part, _) = board(&mut h);
    let a = add(&mut h, &target, DVec3::new(-10.0, -10.0, -10.0), DVec3::new(110.0, 70.0, 40.0), Sign::Intersect);
    let b = add(&mut h, &target, DVec3::new(50.0, -10.0, -10.0), DVec3::new(200.0, 70.0, 40.0), Sign::Intersect);
    let v = volume(h.document(), part);
    // Two intersections grouped: their union would limit, not each.
    let both = Operation::GroupBodies { target: target.clone(), bodies: vec![a, b], scope: Scope::All };
    assert!(matches!(h.apply(both), Err(Error::Invalid(Refusal::IntersectsRegrouped))));
    // One grouped: an intersection group of a positive child, the same shape; ungrouped back.
    let g = h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![a], scope: Scope::All }).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.body(g).unwrap().sign, Sign::Intersect);
    assert_eq!(p.def_of(p.body(g).unwrap()).children()[0].sign, Sign::Positive);
    assert!((volume(h.document(), part) - v).abs() < 1e-2);
    h.apply(Operation::UngroupBodies { target: target.clone(), bodies: vec![g], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.bodies.iter().any(|x| x.id == a && x.sign == Sign::Intersect), "an intersection again");
    assert!((volume(h.document(), part) - v).abs() < 1e-2);
}

#[test]
fn an_intersection_in_a_group_limits_that_group_only() {
    let mut h = History::new(Document::new());
    let (target, part, board) = board(&mut h);
    // An intersection over the board's left half, a second board beside; then the board and the
    // intersection grouped: the intersection limits that group only, the second board whole again.
    let intersect = add(&mut h, &target, DVec3::new(-10.0, -10.0, -10.0), DVec3::new(110.0, 70.0, 40.0), Sign::Intersect);
    let other = add(&mut h, &target, DVec3::new(0.0, 100.0, 0.0), DVec3::new(200.0, 50.0, 18.0), Sign::Positive);
    // At the root, it limits both: the second board, outside it, gone.
    assert!((volume(h.document(), part) - 100.0 * 50.0 * 18.0).abs() < 1e-2, "{}", volume(h.document(), part));
    h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![board, intersect], scope: Scope::All }).unwrap();
    let _ = other;
    assert!((volume(h.document(), part) - (100.0 + 200.0) * 50.0 * 18.0).abs() < 1e-2, "{}", volume(h.document(), part));
}

/// A box part at the model's root: its occurrence.
fn part_box(h: &mut History, lo: DVec3, size: DVec3) -> Id {
    let root = h.document().root();
    let b = Primitive::Box { origin: lo, size };
    h.apply(Operation::AddPrimitive { context: root, primitive: b, placement: DAffine3::IDENTITY, material: None }).unwrap().created[0]
}

#[test]
fn the_intersect_tool_keeps_its_tools_as_an_intersection_of_the_target() {
    let mut h = History::new(Document::new());
    let (target, part, blank) = board(&mut h);
    // One tool over the left half: the target keeps its board, limited by an intersection.
    let tool = part_box(&mut h, DVec3::new(-10.0, -10.0, -10.0), DVec3::new(110.0, 70.0, 40.0));
    h.apply(Operation::boolean(target.clone(), EntityPath::root(tool), Boolean::Intersection, Scope::All)).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.body(blank).is_some(), "the board kept, editable");
    assert_eq!(p.intersects().count(), 1);
    assert!((volume(h.document(), part) - 100.0 * 50.0 * 18.0).abs() < 1e-2, "{}", volume(h.document(), part));
    // Two tools, at both ends: their union limits (an intersection group), not each.
    let mut h = History::new(Document::new());
    let (target, part, _) = board(&mut h);
    let left = part_box(&mut h, DVec3::new(-10.0, -10.0, -10.0), DVec3::new(60.0, 70.0, 40.0));
    let right = part_box(&mut h, DVec3::new(150.0, -10.0, -10.0), DVec3::new(60.0, 70.0, 40.0));
    let op = Operation::Boolean {
        targets: vec![target.clone()],
        tools: vec![EntityPath::root(left), EntityPath::root(right)],
        op: Boolean::Intersection,
        scope: Scope::All,
        keep_targets: false,
        keep_tools: false,
        drop_unused: false,
    };
    h.apply(op).unwrap();
    let p = h.document().part(part).unwrap();
    let limit = p.intersects().next().expect("an intersection");
    assert_eq!(p.def_of(limit).children().len(), 2);
    assert!((volume(h.document(), part) - 2.0 * 50.0 * 50.0 * 18.0).abs() < 1e-2, "{}", volume(h.document(), part));
}

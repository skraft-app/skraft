//! Body nodes (docs/decisions/0042-body-nodes-operators.md): a round wrapping bodies, found
//! again by its names when they change inside it; refusals; the file.

use glam::{DAffine3, DVec3};
use skraft_kernel::names::{Blend, NodeRefusal};

use crate::*;

fn volume(doc: &Document, part: Id) -> f64 {
    doc.part(part).unwrap().shape().volume().unwrap()
}

/// A board 200 × 50 × 18 drilled by a ø8 hole: its path, part, board body and drill.
fn drilled_board(h: &mut History) -> (EntityPath, Id, Id, Id) {
    let root = h.document().root();
    let mut add = |p: Primitive| {
        h.apply(Operation::AddPrimitive { context: root, primitive: p, placement: DAffine3::IDENTITY, material: None }).unwrap().created[0]
    };
    let board = add(Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let drill = add(Primitive::Cylinder { origin: DVec3::new(20.0, 25.0, -5.0), radius: 4.0, height: 30.0 });
    let target = EntityPath::root(board);
    h.apply(Operation::boolean(target.clone(), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let (part, p, _) = h.document().resolve_part(&target).unwrap();
    (target, part, p.positives().next().unwrap().id, p.negatives().next().unwrap().id)
}

/// What a round of `r` along `length` takes off a square edge.
fn round_off(r: f64, length: f64) -> f64 {
    (1.0 - std::f64::consts::FRAC_PI_4) * r * r * length
}

#[test]
fn a_round_wraps_its_bodies_and_follows_them() {
    let mut h = History::new(Document::new());
    let (target, part, board, drill) = drilled_board(&mut h);
    let sharp = volume(h.document(), part);
    // The top's long edge at y = 0, rounded 3.
    let op = Operation::Blend {
        target: target.clone(),
        bodies: vec![board, drill],
        blend: Blend::Round,
        size: 3.0,
        picks: vec![DVec3::new(100.0, 0.0, 18.0)],
        scope: Scope::All,
    };
    let node = h.apply(op).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.bodies.len(), 1, "the node in its bodies' place");
    let def = p.def_of(p.body(node).unwrap());
    assert_eq!(def.nature(), Nature::Round);
    let op = def.op().unwrap();
    assert_eq!((op.edges.len(), op.edges[0].near.len()), (1, 0));
    assert!((volume(h.document(), part) - (sharp - round_off(3.0, 200.0))).abs() < 1e-2, "{}", volume(h.document(), part));

    // The drill moved inside the node: the round is applied again, by its name.
    let inside = target.clone().in_group(vec![node]);
    let moved = Operation::TransformBodies {
        target: inside.clone(),
        bodies: vec![drill],
        transform: DAffine3::from_translation(DVec3::X * 50.0),
        scope: Scope::All,
    };
    h.apply(moved).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.def_of(p.body(node).unwrap()).group.as_ref().unwrap().refused, None);
    assert!((volume(h.document(), part) - (sharp - round_off(3.0, 200.0))).abs() < 1e-2);

    // Its size changed, then one too large: refused, the document as it was.
    let set = |size: f64| Operation::SetBlend {
        target: target.clone(),
        body: node,
        blend: Blend::Round,
        size,
        picks: Vec::new(),
        scope: Scope::All,
    };
    h.apply(set(5.0)).unwrap();
    assert!((volume(h.document(), part) - (sharp - round_off(5.0, 200.0))).abs() < 1e-2);
    // Both long top edges at 30, on a top 50 wide: they would cross.
    let before = h.document().clone();
    let both = Operation::SetBlend {
        target: target.clone(),
        body: node,
        blend: Blend::Round,
        size: 30.0,
        picks: vec![DVec3::new(100.0, 0.0, 18.0), DVec3::new(100.0, 50.0, 18.0)],
        scope: Scope::All,
    };
    let refused = h.apply(both);
    assert!(matches!(refused, Err(Error::Blend(NodeRefusal::TooLarge { size: Some(_) }))), "{refused:?}");
    assert_eq!(*h.document(), before);

    // Saved and read back as it is; undone step by step.
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap(), h.document().part(part).unwrap());
    assert!(h.undo() && h.undo() && h.undo());
    assert!((volume(h.document(), part) - sharp).abs() < 1e-3);
}

#[test]
fn a_rounded_machining_is_a_negative_node() {
    let mut h = History::new(Document::new());
    let (target, part, _, drill) = drilled_board(&mut h);
    let sharp = volume(h.document(), part);
    // The drill's top rim (outside the board: it stands 7 above it) chamfered, picked mid-arc
    // (its rim is two half circles: their ends pick neither): still a machining.
    let op = Operation::Blend {
        target: target.clone(),
        bodies: vec![drill],
        blend: Blend::Chamfer,
        size: 1.0,
        picks: vec![DVec3::new(20.0, 29.0, 25.0)],
        scope: Scope::All,
    };
    let node = h.apply(op).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.body(node).unwrap().sign, Sign::Negative);
    assert_eq!(p.def_of(p.body(node).unwrap()).nature(), Nature::Chamfer);
    // The chamfered end lies outside the board: the part is as it was.
    assert!((volume(h.document(), part) - sharp).abs() < 1e-3);
}

#[test]
fn a_pick_near_no_edge_is_refused() {
    let mut h = History::new(Document::new());
    let (target, _, board, _) = drilled_board(&mut h);
    let before = h.document().clone();
    let op = Operation::Blend {
        target,
        bodies: vec![board],
        blend: Blend::Round,
        size: 1.0,
        picks: vec![DVec3::new(100.0, 25.0, 18.0)],
        scope: Scope::All,
    };
    assert!(matches!(h.apply(op), Err(Error::Blend(NodeRefusal::NoEdge))));
    assert_eq!(*h.document(), before);
}

#[test]
fn a_node_over_a_group_takes_its_cache_and_face_map() {
    let mut h = History::new(Document::new());
    let (target, part, board, drill) = drilled_board(&mut h);
    let group =
        h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![board, drill], scope: Scope::All }).unwrap().bodies[0];
    // The group computed with its face map.
    let p = h.document().part(part).unwrap();
    assert!(p.def_of(p.body(group).unwrap()).face_map().is_some(), "the group's face map");
    let node = Operation::Blend {
        target: target.clone(),
        bodies: vec![group],
        blend: Blend::Round,
        size: 3.0,
        picks: vec![DVec3::new(100.0, 0.0, 18.0)],
        scope: Scope::All,
    };
    let node = h.apply(node).unwrap().bodies[0];
    // Changed: the node's task takes the group as it is, not from its leaves.
    let set =
        Operation::SetBlend { target: target.clone(), body: node, blend: Blend::Round, size: 4.0, picks: Vec::new(), scope: Scope::All };
    let Prepared::Kernel(job) = h.document().prepare(&set).unwrap() else { panic!("a kernel job") };
    let [kernel::Task::Node { children, .. }] = job.tasks.as_slice() else { panic!("{:?}", job.tasks) };
    assert!(matches!(children.as_slice(), [kernel::names::TreeBody { node: kernel::names::TreeNode::Mapped { .. }, .. }]), "{children:?}");
    h.apply(set).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.refusal_of(node), None);
    assert!(p.def_of(p.body(node).unwrap()).face_map().is_some(), "the node's own face map");
}

#[test]
fn a_node_made_unique_keeps_its_names() {
    let mut h = History::new(Document::new());
    let (target, part, board, drill) = drilled_board(&mut h);
    let op = Operation::Blend {
        target: target.clone(),
        bodies: vec![board, drill],
        blend: Blend::Round,
        size: 3.0,
        picks: vec![DVec3::new(100.0, 0.0, 18.0)],
        scope: Scope::All,
    };
    let node = h.apply(op).unwrap().bodies[0];
    // Copied beside it, then made unique: its children get ids of their own, and its names with them.
    let copy = Operation::CopyBodies {
        target: target.clone(),
        bodies: vec![node],
        moved: DAffine3::IDENTITY,
        placements: vec![DAffine3::from_translation(DVec3::Y * 80.0)],
        scope: Scope::All,
    };
    let copy = h.apply(copy).unwrap().bodies[0];
    h.apply(Operation::MakeBodiesUnique { target: target.clone(), bodies: vec![copy], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    let (a, b) = (p.body(node).unwrap(), p.body(copy).unwrap());
    assert_ne!(a.def, b.def, "a definition of its own");
    let child = p.def_of(b).children()[0].id;
    assert!(p.def_of(b).op().unwrap().edges[0].name.faces.iter().all(|f| f.path[0] != p.def_of(a).children()[0].id.0), "renamed");
    assert!(p.def_of(b).op().unwrap().edges[0].name.faces.iter().any(|f| f.path[0] == child.0));
    // Changed, it finds its edge again by its renamed names.
    let set =
        Operation::SetBlend { target: target.clone(), body: copy, blend: Blend::Round, size: 4.0, picks: Vec::new(), scope: Scope::All };
    h.apply(set).unwrap();
    assert_eq!(h.document().part(part).unwrap().refusal_of(copy), None);
}

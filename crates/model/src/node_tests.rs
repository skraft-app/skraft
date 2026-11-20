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
    // Made: the node's task takes the group as it is, not from its leaves.
    let Prepared::Kernel(job) = h.document().prepare(&node).unwrap() else { panic!("a kernel job") };
    let [kernel::Task::Node { children, .. }, ..] = job.tasks.as_slice() else { panic!("{:?}", job.tasks) };
    assert!(matches!(children.as_slice(), [kernel::names::TreeBody { node: kernel::names::TreeNode::Mapped { .. }, .. }]), "{children:?}");
    let node = h.apply(node).unwrap().bodies[0];
    // Changed, its children as they were: from their result the node keeps (`Base`).
    let set =
        Operation::SetBlend { target: target.clone(), body: node, blend: Blend::Round, size: 4.0, picks: Vec::new(), scope: Scope::All };
    let Prepared::Kernel(job) = h.document().prepare(&set).unwrap() else { panic!("a kernel job") };
    let [kernel::Task::Node { children, .. }, ..] = job.tasks.as_slice() else { panic!("{:?}", job.tasks) };
    assert!(matches!(children.as_slice(), [kernel::names::TreeBody { node: kernel::names::TreeNode::Base { .. }, .. }]), "{children:?}");
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

/// The round of `drilled_board`'s top long edge at y = 0, wrapping `bodies` of `target`.
fn round_front(h: &mut History, target: &EntityPath, bodies: Vec<Id>, size: f64) -> Id {
    let op = Operation::Blend {
        target: target.clone(),
        bodies,
        blend: Blend::Round,
        size,
        picks: vec![DVec3::new(100.0, 0.0, 18.0)],
        scope: Scope::All,
    };
    h.apply(op).unwrap().bodies[0]
}

#[test]
fn grouping_or_ungrouping_inside_a_node_keeps_its_edges() {
    let mut h = History::new(Document::new());
    let (target, part, board, drill) = drilled_board(&mut h);
    let sharp = volume(h.document(), part);
    let node = round_front(&mut h, &target, vec![board, drill], 3.0);
    let rounded = sharp - round_off(3.0, 200.0);
    // Grouped inside it: the board and the drill one level down, the round on the same edge.
    let inside = target.clone().in_group(vec![node]);
    let g = h.apply(Operation::GroupBodies { target: inside.clone(), bodies: vec![board, drill], scope: Scope::All }).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.refusal_of(node), None);
    let names = &p.def_of(p.body(node).unwrap()).op().unwrap().edges[0].name;
    assert!(names.faces.iter().all(|f| f.path == [g.0, board.0]), "{names:?}");
    assert!((volume(h.document(), part) - rounded).abs() < 1e-2);
    // Ungrouped: back to its children.
    h.apply(Operation::UngroupBodies { target: inside, bodies: vec![g], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.refusal_of(node), None);
    assert!((volume(h.document(), part) - rounded).abs() < 1e-2);
}

#[test]
fn grouping_inside_an_inner_node_keeps_the_outer_nodes_edges() {
    let mut h = History::new(Document::new());
    let (target, part, board, drill) = drilled_board(&mut h);
    // r1 on the top's front edge, r2 over it on the arc between r1's face and the board's end
    // at x = 200.
    let r1 = round_front(&mut h, &target, vec![board, drill], 3.0);
    let r2 = Operation::Blend {
        target: target.clone(),
        bodies: vec![r1],
        blend: Blend::Round,
        size: 1.0,
        picks: vec![DVec3::new(200.0, 3.0 - 3.0 * 0.5f64.sqrt(), 15.0 + 3.0 * 0.5f64.sqrt())],
        scope: Scope::All,
    };
    let r2 = h.apply(r2).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    let made = |f: &kernel::names::FaceTag| matches!(f.face, kernel::names::FaceName::Made(_));
    assert!(p.def_of(p.body(r2).unwrap()).op().unwrap().edges[0].name.faces.iter().any(made), "r2 on r1's face");
    let before = volume(h.document(), part);
    let inside = target.clone().in_group(vec![r2, r1]);
    h.apply(Operation::GroupBodies { target: inside, bodies: vec![board, drill], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!((p.refusal_of(r1), p.refusal_of(r2)), (None, None));
    assert!((volume(h.document(), part) - before).abs() < 1e-2);
}

#[test]
fn a_group_made_unique_inside_a_node_keeps_its_edges() {
    let mut h = History::new(Document::new());
    let (target, part, board, drill) = drilled_board(&mut h);
    let g = h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![board, drill], scope: Scope::All }).unwrap().bodies[0];
    let node = round_front(&mut h, &target, vec![g], 3.0);
    // A copy of the group inside the node (sharing its definition), then the first made unique:
    // its children get ids of their own.
    let inside = target.clone().in_group(vec![node]);
    let copy = Operation::CopyBodies {
        target: inside.clone(),
        bodies: vec![g],
        moved: DAffine3::IDENTITY,
        placements: vec![DAffine3::from_translation(DVec3::Z * 100.0)],
        scope: Scope::All,
    };
    h.apply(copy).unwrap();
    h.apply(Operation::MakeBodiesUnique { target: inside, bodies: vec![g], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    let def = p.def_of(p.body(node).unwrap());
    let own = def.children().iter().find(|c| c.id == g).map(|c| p.def(c.def).unwrap().children()[0].id).unwrap();
    assert_ne!(own, board, "renamed");
    assert_eq!(p.refusal_of(node), None);
    assert!(def.op().unwrap().edges[0].name.faces.iter().all(|f| f.path == [g.0, own.0]), "{:?}", def.op());
}

#[test]
fn a_node_inside_computed_with_its_parent_shows_its_refusal() {
    let mut h = History::new(Document::new());
    let (target, part, board, drill) = drilled_board(&mut h);
    // r1 on the top's two long edges, r2 over it on the top's end at x = 200.
    let r1 = Operation::Blend {
        target: target.clone(),
        bodies: vec![board, drill],
        blend: Blend::Round,
        size: 3.0,
        picks: vec![DVec3::new(100.0, 0.0, 18.0), DVec3::new(100.0, 50.0, 18.0)],
        scope: Scope::All,
    };
    let r1 = h.apply(r1).unwrap().bodies[0];
    let r2 = Operation::Blend {
        target: target.clone(),
        bodies: vec![r1],
        blend: Blend::Round,
        size: 1.0,
        picks: vec![DVec3::new(200.0, 25.0, 18.0)],
        scope: Scope::All,
    };
    let r2 = h.apply(r2).unwrap().bodies[0];
    // r1 without its face map (its solid from elsewhere) and at 30 on a board 50 wide (its two
    // rounds would cross), not computed again: r2 computes it from its leaves.
    let mut doc = h.document().clone();
    let p = doc.part_mut(part).unwrap();
    let def = p.any_body(r1).unwrap().def;
    let g = p.defs.iter_mut().find(|d| d.id == def).unwrap().group.as_mut().unwrap();
    g.faces.clear();
    g.op.as_mut().unwrap().size = 30.0;
    let mut h = History::new(doc);
    let set =
        Operation::SetBlend { target: target.clone(), body: r2, blend: Blend::Round, size: 2.0, picks: Vec::new(), scope: Scope::All };
    h.apply(set).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.refusal_of(r2), None);
    assert!(matches!(p.refusal_of(r1), Some(NodeRefusal::TooLarge { .. })), "{:?}", p.refusal_of(r1));
}

#[test]
fn a_node_made_unique_renames_its_made_faces_names() {
    let mut h = History::new(Document::new());
    let (target, part, board, drill) = drilled_board(&mut h);
    let r1 = round_front(&mut h, &target, vec![board, drill], 3.0);
    // Copied 80 along Y and made unique: its children get ids of their own, and the names of
    // the faces it made with them (they are named by its children's faces).
    let copy = Operation::CopyBodies {
        target: target.clone(),
        bodies: vec![r1],
        moved: DAffine3::IDENTITY,
        placements: vec![DAffine3::from_translation(DVec3::Y * 80.0)],
        scope: Scope::All,
    };
    let copy = h.apply(copy).unwrap().bodies[0];
    h.apply(Operation::MakeBodiesUnique { target: target.clone(), bodies: vec![copy], scope: Scope::All }).unwrap();
    // r2 over the copy, on the arc between its round and the board's end at x = 200.
    let s = 3.0 * 0.5f64.sqrt();
    let r2 = Operation::Blend {
        target: target.clone(),
        bodies: vec![copy],
        blend: Blend::Round,
        size: 1.0,
        picks: vec![DVec3::new(200.0, 80.0 + 3.0 - s, 15.0 + s)],
        scope: Scope::All,
    };
    let r2 = h.apply(r2).unwrap().bodies[0];
    // The copy changed inside r2: computed again from its children, its round's face named by
    // their ids — r2 still finds its edge.
    let set = Operation::SetBlend {
        target: target.clone().in_group(vec![r2]),
        body: copy,
        blend: Blend::Round,
        size: 4.0,
        picks: Vec::new(),
        scope: Scope::All,
    };
    h.apply(set).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!((p.refusal_of(copy), p.refusal_of(r2)), (None, None));
}

/// The children of the first node task `op`'s job runs.
fn node_task(h: &History, op: &Operation) -> Vec<kernel::names::TreeBody> {
    let Prepared::Kernel(job) = h.document().prepare(op).unwrap() else { panic!("a kernel job") };
    job.tasks
        .iter()
        .find_map(|t| match t {
            kernel::Task::Node { children, .. } => Some(children.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a node task: {:?}", job.tasks))
}

#[test]
fn a_node_starts_from_its_childrens_result_while_it_holds() {
    use kernel::names::{TreeBody, TreeNode};
    let mut h = History::new(Document::new());
    let (target, part, board, drill) = drilled_board(&mut h);
    let sharp = volume(h.document(), part);
    let hole = std::f64::consts::PI * 16.0 * 18.0;
    let node = round_front(&mut h, &target, vec![board, drill], 3.0);
    // Its size changed: its children's result as it keeps it, the round alone done again.
    let set =
        Operation::SetBlend { target: target.clone(), body: node, blend: Blend::Round, size: 4.0, picks: Vec::new(), scope: Scope::All };
    assert!(matches!(node_task(&h, &set).as_slice(), [TreeBody { node: TreeNode::Base { .. }, .. }]));
    h.apply(set).unwrap();
    assert!((volume(h.document(), part) - (sharp - round_off(4.0, 200.0))).abs() < 1e-2);
    // A drill added inside it: that result cut by the drill alone.
    let inside = target.clone().in_group(vec![node]);
    let add = Operation::AddBody {
        target: inside.clone(),
        primitive: Primitive::Cylinder { origin: DVec3::new(150.0, 25.0, -5.0), radius: 4.0, height: 30.0 },
        placement: DAffine3::IDENTITY,
        sign: Sign::Negative,
        scope: Scope::All,
    };
    assert!(
        matches!(
            node_task(&h, &add).as_slice(),
            [TreeBody { node: TreeNode::Base { .. }, .. }, TreeBody { negative: true, node: TreeNode::Leaf { .. }, .. }]
        ),
        "{:?}",
        node_task(&h, &add)
    );
    // The part standing for the node while the drill is added is not computed: no recipe.
    let Prepared::Kernel(job) = h.document().prepare(&add).unwrap() else { panic!("a kernel job") };
    assert!(!job.tasks.iter().any(|t| matches!(t, kernel::Task::Recipe { .. })), "{:?}", job.tasks);
    let second = h.apply(add).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.refusal_of(node), None);
    assert!((volume(h.document(), part) - (sharp - hole - round_off(4.0, 200.0))).abs() < 1e-2, "{}", volume(h.document(), part));
    // A drill moved: its children computed again (a hole cannot be filled back), the same.
    let moved = Operation::TransformBodies {
        target: inside,
        bodies: vec![second],
        transform: DAffine3::from_translation(DVec3::X * 10.0),
        scope: Scope::All,
    };
    assert!(node_task(&h, &moved).iter().all(|b| !matches!(b.node, TreeNode::Base { .. })));
    h.apply(moved).unwrap();
    assert!((volume(h.document(), part) - (sharp - hole - round_off(4.0, 200.0))).abs() < 1e-2);
    // Its edges found again each time, by their names.
    assert_eq!(h.document().part(part).unwrap().refusal_of(node), None);
}

/// What a node's children's result kept saves: a board 600 × 300 × 18 drilled 30 times, its
/// front edge rounded, then its size changed and a drill added inside it — from that result,
/// and from its leaves (the result forgotten). Run with `--ignored --nocapture`.
#[test]
#[ignore]
fn bench_a_node_from_its_childrens_result_or_its_leaves() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 300.0, 18.0) };
    let board = h
        .apply(Operation::AddPrimitive { context: root, primitive: board, placement: DAffine3::IDENTITY, material: None })
        .unwrap()
        .created[0];
    let target = EntityPath::root(board);
    let (part, p, _) = h.document().resolve_part(&target).unwrap();
    let blank = p.bodies[0].id;
    let drill = |x: f64| Operation::AddBody {
        target: target.clone(),
        primitive: Primitive::Cylinder { origin: DVec3::new(x, 150.0, -5.0), radius: 4.0, height: 30.0 },
        placement: DAffine3::IDENTITY,
        sign: Sign::Negative,
        scope: Scope::All,
    };
    for k in 0..30 {
        h.apply(drill(30.0 + 18.0 * k as f64)).unwrap();
    }
    let bodies: Vec<Id> = h.document().part(part).unwrap().bodies.iter().map(|b| b.id).collect();
    assert!(bodies.contains(&blank) && bodies.len() == 31);
    let round = Operation::Blend {
        target: target.clone(),
        bodies,
        blend: Blend::Round,
        size: 3.0,
        picks: vec![DVec3::new(300.0, 0.0, 18.0)],
        scope: Scope::All,
    };
    let t = std::time::Instant::now();
    let node = h.apply(round).unwrap().bodies[0];
    println!("the round made: {:?}", t.elapsed());
    let inside = target.clone().in_group(vec![node]);
    let forget = |doc: &Document| {
        let mut doc = doc.clone();
        let p = doc.part_mut(part).unwrap();
        let def = p.body(node).unwrap().def;
        p.defs.iter_mut().find(|d| d.id == def).unwrap().group.as_mut().unwrap().raw = RawCell::default();
        doc
    };
    for (label, size) in [("its size", 4.0), ("its size again", 5.0)] {
        let set =
            Operation::SetBlend { target: target.clone(), body: node, blend: Blend::Round, size, picks: Vec::new(), scope: Scope::All };
        let mut cold = History::new(forget(h.document()));
        let t = std::time::Instant::now();
        cold.apply(set.clone()).unwrap();
        let leaves = t.elapsed();
        let t = std::time::Instant::now();
        h.apply(set).unwrap();
        println!("{label}: {:?} from its children's result, {leaves:?} from its leaves", t.elapsed());
    }
    for x in [40.0, 58.0] {
        let add = Operation::AddBody {
            target: inside.clone(),
            primitive: Primitive::Cylinder { origin: DVec3::new(x, 60.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
            sign: Sign::Negative,
            scope: Scope::All,
        };
        let mut cold = History::new(forget(h.document()));
        let t = std::time::Instant::now();
        cold.apply(add.clone()).unwrap();
        let leaves = t.elapsed();
        let t = std::time::Instant::now();
        h.apply(add).unwrap();
        println!("a drill added inside: {:?} from its children's result, {leaves:?} from its leaves", t.elapsed());
    }
    assert_eq!(h.document().part(part).unwrap().refusal_of(node), None);
}

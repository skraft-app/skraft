//! The node task (docs/decisions/0042-body-nodes-operators.md): edges picked by points and
//! named, found again by their names after the children change, refusals.

use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::names::{Blend, FaceName, NodeEdges, NodeOp, NodeRefusal, TreeBody, TreeNode};
use skraft_kernel::profile;
use skraft_kernel::{Job, Output, Primitive, Solid, Task, run};

/// A rectangle's prism, its sides' ids 1..4 from its corner at the origin: 1 along −Y's side
/// (y = 0), 2 along +X's, 3 along +Y's, 4 along −X's.
fn board(l: f64, w: f64, t: f64) -> Primitive {
    let mut p = profile::polygon(&[DVec2::ZERO, DVec2::new(l, 0.0), DVec2::new(l, w), DVec2::new(0.0, w)]);
    profile::settle_ids(&mut p);
    Primitive::Prism { origin: DVec3::ZERO, profile: p, height: t }
}

fn leaf(id: u64, negative: bool, at: DVec3, solid: u32, recipe: Option<Primitive>) -> TreeBody {
    TreeBody { id, negative, placement: DAffine3::from_translation(at), node: TreeNode::Leaf { solid, recipe } }
}

struct Ran {
    solid: Solid,
    edges: Vec<NodeEdges>,
    failed: Vec<Vec<u64>>,
    refusal: Option<NodeRefusal>,
}

fn node(inputs: Vec<Arc<Solid>>, children: Vec<TreeBody>, op: NodeOp, picks: Vec<[f64; 3]>) -> Ran {
    let job = Job { inputs, tasks: vec![Task::Node { children, op, picks }] };
    match run(&job).unwrap().pop() {
        Some(Output::Node { solid, edges, failed, refusal, .. }) => Ran { solid, edges, failed, refusal },
        o => panic!("{o:?}"),
    }
}

fn round(size: f64, edges: Vec<NodeEdges>) -> NodeOp {
    NodeOp { blend: Blend::Round, size, edges }
}

/// A board 100 × 60 × 20 (body 1) minus a pocket 30 × 20 × 15 (body 2) at `pocket_x`.
fn board_and_pocket(pocket_x: f64) -> (Vec<Arc<Solid>>, Vec<TreeBody>) {
    let b = board(100.0, 60.0, 20.0);
    let solid = Arc::new(b.build().unwrap());
    let pocket = Arc::new(Solid::new_box(DVec3::ZERO, DVec3::new(30.0, 20.0, 15.0)).unwrap());
    (vec![solid, pocket], vec![leaf(1, false, DVec3::ZERO, 0, Some(b)), leaf(2, true, DVec3::new(pocket_x, 20.0, 10.0), 1, None)])
}

fn volume(s: &Solid) -> f64 {
    s.volume().unwrap()
}

#[test]
fn edges_picked_are_named_by_the_recipe_and_found_again_after_an_edit() {
    let (inputs, children) = board_and_pocket(35.0);
    let sharp = 100.0 * 60.0 * 20.0 - 30.0 * 20.0 * 10.0;
    // The top's edge along y = 0 (side 1) and along x = 100 (side 2).
    let picks = vec![[50.0, 0.0, 20.0], [100.0, 30.0, 20.0]];
    let first = node(inputs.clone(), children, round(5.0, Vec::new()), picks);
    assert_eq!(first.refusal, None);
    assert_eq!(first.edges.len(), 2, "{:?}", first.edges);
    let faces: Vec<_> = first.edges.iter().map(|e| (e.name.faces[0].face.clone(), e.name.faces[1].face.clone())).collect();
    assert!(faces.contains(&(FaceName::Top, FaceName::Side(1))) || faces.contains(&(FaceName::Side(1), FaceName::Top)), "{faces:?}");
    assert!(first.edges.iter().all(|e| e.near.is_empty() && e.name.faces.iter().all(|f| f.path == [1])), "the whole edges, on body 1");
    // Two rounds of 5 along 60 and 100 mm: (1 − π/4)·25 mm² each per mm.
    let cut = (1.0 - std::f64::consts::FRAC_PI_4) * 25.0;
    // Where the two meet, at the corner, a little less is taken than both.
    let expected = sharp - cut * (100.0 + 60.0);
    assert!((volume(&first.solid) - expected).abs() < 30.0, "{} vs {expected}", volume(&first.solid));
    // The pocket moved: the names find the same two edges.
    let (inputs, children) = board_and_pocket(55.0);
    let again = node(inputs, children, round(5.0, first.edges.clone()), Vec::new());
    assert_eq!(again.refusal, None);
    assert_eq!(again.edges, first.edges);
    assert!((volume(&again.solid) - volume(&first.solid)).abs() < 1e-3);
}

#[test]
fn the_pockets_rim_is_named_by_the_board_top_and_the_pocket() {
    let (inputs, children) = board_and_pocket(35.0);
    let first = node(inputs, children, round(2.0, Vec::new()), vec![[50.0, 20.0, 20.0]]);
    assert_eq!(first.refusal, None);
    let [e] = first.edges.as_slice() else { panic!("{:?}", first.edges) };
    let paths: Vec<_> = e.name.faces.iter().map(|f| f.path.clone()).collect();
    assert!(paths.contains(&vec![1]) && paths.contains(&vec![2]), "{paths:?}");
}

#[test]
fn a_size_too_large_is_refused_with_the_largest() {
    // A lath 2 mm wide, both its top's long edges rounded 1.5: they would cross.
    let b = board(30.0, 2.0, 10.0);
    let solid = Arc::new(b.build().unwrap());
    let sharp = volume(&solid);
    let r = node(
        vec![solid],
        vec![leaf(1, false, DVec3::ZERO, 0, Some(b))],
        round(1.5, Vec::new()),
        vec![[15.0, 0.0, 10.0], [15.0, 2.0, 10.0]],
    );
    match r.refusal {
        Some(NodeRefusal::TooLarge { size: Some(size) }) => assert!(size > 0.5 && size <= 1.0 + 1e-9, "{size}"),
        other => panic!("{other:?}"),
    }
    // Left as it was.
    assert!((volume(&r.solid) - sharp).abs() < 1e-6);
}

#[test]
fn a_name_whose_faces_are_gone_is_lost() {
    let (inputs, children) = board_and_pocket(35.0);
    let first = node(inputs.clone(), children, round(2.0, Vec::new()), vec![[50.0, 20.0, 20.0]]);
    // The pocket gone from the children: its rim's name no longer matches.
    let (_, mut children) = board_and_pocket(35.0);
    children.pop();
    let r = node(inputs, children, round(2.0, first.edges.clone()), Vec::new());
    assert!(matches!(&r.refusal, Some(NodeRefusal::Lost(l)) if l.len() == 1), "{:?}", r.refusal);
}

#[test]
fn a_node_over_a_node_names_the_inner_rounds() {
    let (inputs, children) = board_and_pocket(35.0);
    // The inner node: the board's four upright edges rounded 10.
    let corners = vec![[0.0, 0.0, 10.0], [100.0, 0.0, 10.0], [100.0, 60.0, 10.0], [0.0, 60.0, 10.0]];
    let inner = node(inputs.clone(), children.clone(), round(10.0, Vec::new()), corners);
    assert_eq!((inner.refusal.clone(), inner.edges.len()), (None, 4));
    let inner_body = TreeBody {
        id: 7,
        negative: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Group { children, op: Some(round(10.0, inner.edges.clone())) },
    };
    // The outer node: the top's outer loop, its four straight edges and the four arcs over the
    // inner rounds (a tangent chain: all of it, or the kernel refuses).
    let d = 10.0 * (1.0 - 0.5f64.sqrt());
    let picks = vec![
        [50.0, 0.0, 20.0],
        [100.0, 30.0, 20.0],
        [50.0, 60.0, 20.0],
        [0.0, 30.0, 20.0],
        [100.0 - d, d, 20.0],
        [100.0 - d, 60.0 - d, 20.0],
        [d, 60.0 - d, 20.0],
        [d, d, 20.0],
    ];
    let outer = node(inputs, vec![inner_body], round(3.0, Vec::new()), picks);
    assert_eq!(outer.refusal, None, "{:?}", outer.edges);
    assert!(
        outer.edges.iter().any(|e| e.name.faces.iter().any(|f| f.path == [7] && matches!(f.face, FaceName::Made(_)))),
        "an inner round's face named: {:?}",
        outer.edges
    );
    assert!(outer.failed.is_empty());
}

#[test]
fn a_named_recipe_gives_its_face_map_and_a_node_over_it_names_the_same() {
    use skraft_kernel::names::{InputNames, RecipeNames};
    let (inputs, children) = board_and_pocket(35.0);
    let b = board(100.0, 60.0, 20.0);
    // The group's recipe, named: its result's face map.
    let names = RecipeNames { positives: vec![(1, InputNames::Recipe(Some(b)))], negatives: vec![(2, InputNames::Recipe(None))] };
    let job = Job {
        inputs: inputs.clone(),
        tasks: vec![Task::Recipe {
            base: None,
            positives: vec![(0, DAffine3::IDENTITY)],
            negatives: vec![(1, DAffine3::from_translation(DVec3::new(35.0, 20.0, 10.0)))],
            names: Some(names),
        }],
    };
    let Some(Output::Recipe { solid: group, faces: Some(map), .. }) = run(&job).unwrap().pop() else { panic!("no face map") };
    assert_eq!(map.len(), group.record().faces.len());
    assert!(map.iter().all(|tags| !tags.is_empty()), "every face named: {map:?}");
    assert!(map.iter().flatten().any(|t| t.path == [1] && t.face == FaceName::Top));
    // A node over the group: from its leaves, or from its cache and map — the same names, the
    // same solid.
    let picks = vec![[50.0, 0.0, 20.0], [50.0, 20.0, 20.0]];
    let from_leaves = node(
        inputs.clone(),
        vec![TreeBody { id: 9, negative: false, placement: DAffine3::IDENTITY, node: TreeNode::Group { children, op: None } }],
        round(2.0, Vec::new()),
        picks.clone(),
    );
    let mapped = TreeBody { id: 9, negative: false, placement: DAffine3::IDENTITY, node: TreeNode::Mapped { solid: 0, faces: map } };
    let from_cache = node(vec![Arc::new(group)], vec![mapped], round(2.0, Vec::new()), picks);
    assert_eq!(from_leaves.refusal, None);
    assert_eq!(from_cache.refusal, None);
    assert_eq!(from_cache.edges, from_leaves.edges);
    assert!((volume(&from_cache.solid) - volume(&from_leaves.solid)).abs() < 1e-6);
}

/// Measures (run with `--ignored --nocapture`): a node over a board drilled 30 times, from its
/// leaves or from the group's cache and face map.
#[test]
#[ignore]
fn bench_a_node_over_a_group_from_its_leaves_or_its_cache() {
    use skraft_kernel::names::{InputNames, RecipeNames};
    let b = board(600.0, 300.0, 18.0);
    let hole = Arc::new(Solid::new_cylinder(DVec3::ZERO, 4.0, 30.0).unwrap());
    let mut inputs = vec![Arc::new(b.build().unwrap()), hole];
    let at = |k: usize| DVec3::new(30.0 + 18.0 * k as f64, 150.0, -5.0);
    let mut children = vec![leaf(1, false, DVec3::ZERO, 0, Some(b.clone()))];
    children.extend((0..30).map(|k| leaf(10 + k as u64, true, at(k), 1, None)));
    let names = RecipeNames {
        positives: vec![(1, InputNames::Recipe(Some(b)))],
        negatives: (0..30).map(|k| (10 + k as u64, InputNames::Recipe(None))).collect(),
    };
    let job = Job {
        inputs: inputs.clone(),
        tasks: vec![Task::Recipe {
            base: None,
            positives: vec![(0, DAffine3::IDENTITY)],
            negatives: (0..30).map(|k| (1, DAffine3::from_translation(at(k)))).collect(),
            names: Some(names),
        }],
    };
    // The same recipe unnamed: what naming costs.
    let mut plain = job.clone();
    if let Task::Recipe { names, .. } = &mut plain.tasks[0] {
        *names = None;
    }
    for _ in 0..2 {
        let t = std::time::Instant::now();
        run(&plain).unwrap();
        println!("the group, unnamed: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        run(&job).unwrap();
        println!("the group, named: {:?}", t.elapsed());
    }
    let Some(Output::Recipe { solid: group, faces: Some(map), .. }) = run(&job).unwrap().pop() else { panic!("no face map") };
    let picks = vec![[300.0, 0.0, 18.0]];
    let t = std::time::Instant::now();
    let from_leaves = node(
        inputs.clone(),
        vec![TreeBody { id: 9, negative: false, placement: DAffine3::IDENTITY, node: TreeNode::Group { children, op: None } }],
        round(3.0, Vec::new()),
        picks.clone(),
    );
    println!("the node from its leaves: {:?}", t.elapsed());
    inputs = vec![Arc::new(group)];
    let t = std::time::Instant::now();
    let from_cache = node(
        inputs,
        vec![TreeBody { id: 9, negative: false, placement: DAffine3::IDENTITY, node: TreeNode::Mapped { solid: 0, faces: map } }],
        round(3.0, Vec::new()),
        picks,
    );
    println!("the node from the cache: {:?}", t.elapsed());
    assert_eq!(from_cache.edges, from_leaves.edges);
}

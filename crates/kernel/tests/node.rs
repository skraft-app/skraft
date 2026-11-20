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
    TreeBody { id, negative, intersect: false, placement: DAffine3::from_translation(at), node: TreeNode::Leaf { solid, recipe } }
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
    NodeOp::blend_of(Blend::Round, size, edges)
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
        intersect: false,
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
    let names = RecipeNames {
        positives: vec![(1, InputNames::Recipe(Some(b)))],
        negatives: vec![(2, InputNames::Recipe(None))],
        intersects: vec![],
    };
    let job = Job {
        inputs: inputs.clone(),
        tasks: vec![Task::Recipe {
            base: None,
            positives: vec![(0, DAffine3::IDENTITY)],
            negatives: vec![(1, DAffine3::from_translation(DVec3::new(35.0, 20.0, 10.0)))],
            intersects: vec![],
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
        vec![TreeBody {
            id: 9,
            negative: false,
            intersect: false,
            placement: DAffine3::IDENTITY,
            node: TreeNode::Group { children, op: None },
        }],
        round(2.0, Vec::new()),
        picks.clone(),
    );
    let mapped = TreeBody {
        id: 9,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Mapped { solid: 0, faces: map },
    };
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
        intersects: vec![],
    };
    let job = Job {
        inputs: inputs.clone(),
        tasks: vec![Task::Recipe {
            base: None,
            positives: vec![(0, DAffine3::IDENTITY)],
            negatives: (0..30).map(|k| (1, DAffine3::from_translation(at(k)))).collect(),
            intersects: vec![],
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
        vec![TreeBody {
            id: 9,
            negative: false,
            intersect: false,
            placement: DAffine3::IDENTITY,
            node: TreeNode::Group { children, op: None },
        }],
        round(3.0, Vec::new()),
        picks.clone(),
    );
    println!("the node from its leaves: {:?}", t.elapsed());
    inputs = vec![Arc::new(group)];
    let t = std::time::Instant::now();
    let from_cache = node(
        inputs,
        vec![TreeBody {
            id: 9,
            negative: false,
            intersect: false,
            placement: DAffine3::IDENTITY,
            node: TreeNode::Mapped { solid: 0, faces: map },
        }],
        round(3.0, Vec::new()),
        picks,
    );
    println!("the node from the cache: {:?}", t.elapsed());
    assert_eq!(from_cache.edges, from_leaves.edges);
}

/// `points` as a profile (`bulges` on some of its sides), its sides' ids 1, 2, … in order.
fn outline(points: &[(f64, f64)], bulges: &[(usize, f64)]) -> Vec<profile::Corner> {
    let mut p = profile::polygon(&points.iter().map(|&(x, y)| DVec2::new(x, y)).collect::<Vec<_>>());
    for &(k, b) in bulges {
        p[k].bulge = b;
    }
    profile::settle_ids(&mut p);
    p
}

/// A leaf's faces' names, sorted, checked one per face.
fn names_of(prim: &Primitive) -> Vec<FaceName> {
    let s = prim.build().unwrap();
    let names = skraft_kernel::names::leaf_faces(s.record(), Some(prim));
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "one name per face: {names:?}");
    sorted
}

#[test]
fn a_leafs_faces_each_take_the_name_of_their_side() {
    let turned = |p| Primitive::Revolution { origin: DVec3::ZERO, profile: p };
    let prism = |p| Primitive::Prism { origin: DVec3::ZERO, profile: p, height: 5.0 };
    // A turned disc: its vertices all on its rim, where its bottom, cylinder and top meet
    // (named all alike by the sides' middles once). Side 4 lies on the axis: no face.
    let disc = outline(&[(0.0, 0.0), (50.0, 0.0), (50.0, 10.0), (0.0, 10.0)], &[]);
    assert_eq!(names_of(&turned(disc)), vec![FaceName::Side(1), FaceName::Side(2), FaceName::Side(3)]);
    // Turned: a groove (two discs facing each other, an inner cylinder), a dome's arc, its top.
    let knob = outline(
        &[(0.0, 0.0), (30.0, 0.0), (30.0, 10.0), (25.0, 10.0), (25.0, 15.0), (30.0, 15.0), (30.0, 20.0), (10.0, 40.0), (0.0, 40.0)],
        &[(6, 0.4)],
    );
    assert_eq!(names_of(&turned(knob)), (1..=8).map(FaceName::Side).collect::<Vec<_>>());
    // A D: a chord and an arc between the same two corners, the plane on the chord.
    let d = outline(&[(0.0, 0.0), (40.0, 0.0)], &[(1, 1.0)]);
    let s = prism(d.clone()).build().unwrap();
    let names = skraft_kernel::names::leaf_faces(s.record(), Some(&prism(d.clone())));
    for (f, name) in s.record().faces.iter().zip(&names) {
        let plane =
            matches!(f.surface, solidcore_record::brep::Surface::Plane { .. } | solidcore_record::brep::Surface::PlaneThrough { .. });
        assert!(!matches!(name, FaceName::Side(2)) || !plane, "the arc's face curved: {names:?}");
    }
    assert_eq!(names_of(&prism(d)), vec![FaceName::Bottom, FaceName::Top, FaceName::Side(1), FaceName::Side(2)]);
    // A T and a filleted corner: each side its face, the fillet its own.
    let t = outline(&[(0.0, 0.0), (100.0, 0.0), (100.0, 3.0), (52.0, 3.0), (52.0, 60.0), (48.0, 60.0), (48.0, 3.0), (0.0, 3.0)], &[]);
    assert_eq!(names_of(&prism(t)).len(), 10);
    let mut l = outline(&[(0.0, 0.0), (60.0, 0.0), (60.0, 20.0), (0.0, 20.0)], &[]);
    l[1].fillet = 8.0;
    let names = names_of(&prism(l));
    assert!(names.iter().any(|n| matches!(n, FaceName::Fillet(_))), "{names:?}");
    assert_eq!(names.len(), 7, "{names:?}");
}

#[test]
fn a_turned_discs_top_rim_is_named_alone() {
    let disc = outline(&[(0.0, 0.0), (50.0, 0.0), (50.0, 10.0), (0.0, 10.0)], &[]);
    let prim = Primitive::Revolution { origin: DVec3::ZERO, profile: disc };
    let solid = Arc::new(prim.build().unwrap());
    let sharp = volume(&solid);
    let r = node(vec![solid], vec![leaf(1, false, DVec3::ZERO, 0, Some(prim))], round(2.0, Vec::new()), vec![[-50.0, 0.0, 10.0]]);
    assert_eq!(r.refusal, None);
    let [e] = r.edges.as_slice() else { panic!("{:?}", r.edges) };
    // The whole rim by its name (between the cylinder and the top), not a point among two.
    assert!(e.near.is_empty(), "{e:?}");
    let faces: Vec<_> = e.name.faces.iter().map(|f| f.face.clone()).collect();
    assert_eq!(faces, vec![FaceName::Side(2), FaceName::Side(3)]);
    let rim = (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * std::f64::consts::TAU * 50.0;
    assert!((sharp - volume(&r.solid) - rim).abs() < 5.0, "{} vs {rim}", sharp - volume(&r.solid));
}

#[test]
fn a_records_fingerprint_tells_its_vertices() {
    // Written in the file with each face map: a value of its own, not the platform's hasher's.
    let s = Solid::new_box(DVec3::ZERO, DVec3::new(10.0, 20.0, 30.0)).unwrap();
    assert_eq!(skraft_kernel::names::fingerprint(s.record()), skraft_kernel::names::fingerprint(s.record()));
    let other = Solid::new_box(DVec3::ZERO, DVec3::new(10.0, 20.0, 31.0)).unwrap();
    assert_ne!(skraft_kernel::names::fingerprint(s.record()), skraft_kernel::names::fingerprint(other.record()));
}

#[test]
fn an_edge_along_a_merged_face_is_named_by_its_own_bodys() {
    // Boards a (body 5, x 0..100) and b (body 1, x 100..200) flush: their tops merge, b's tag
    // first by id. The edge of a's top and its end at x = 0 is a's alone.
    let b = board(100.0, 60.0, 20.0);
    let solid = Arc::new(b.build().unwrap());
    let flush =
        |bx: f64| vec![leaf(5, false, DVec3::ZERO, 0, Some(b.clone())), leaf(1, false, DVec3::new(bx, 0.0, 0.0), 0, Some(b.clone()))];
    let first = node(vec![solid.clone()], flush(100.0), round(2.0, Vec::new()), vec![[0.0, 30.0, 20.0]]);
    assert_eq!(first.refusal, None);
    let [e] = first.edges.as_slice() else { panic!("{:?}", first.edges) };
    assert!(e.name.faces.iter().all(|f| f.path == [5]), "{e:?}");
    // b moved away: the edge is still there, and its name with it.
    let again = node(vec![solid], flush(300.0), round(2.0, first.edges.clone()), Vec::new());
    assert_eq!(again.refusal, None);
}

#[test]
fn a_node_inside_refused_says_why() {
    // A lath 2 mm wide (body 1): its top's long edges named by a round of 0.5 that fits.
    let b = board(30.0, 2.0, 10.0);
    let solid = Arc::new(b.build().unwrap());
    let lath = || vec![leaf(1, false, DVec3::ZERO, 0, Some(b.clone()))];
    let fits = node(vec![solid.clone()], lath(), round(0.5, Vec::new()), vec![[15.0, 0.0, 10.0], [15.0, 2.0, 10.0]]);
    assert_eq!(fits.refusal, None);
    // The same names at 1.5, a node (body 7) computed from its leaves inside another that
    // rounds the end at x = 30: refused, its child goes on, and the outer node says why.
    let inner = TreeBody {
        id: 7,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Group { children: lath(), op: Some(round(1.5, fits.edges.clone())) },
    };
    let job = Job {
        inputs: vec![solid],
        tasks: vec![Task::Node { children: vec![inner], op: round(0.3, Vec::new()), picks: vec![[30.0, 1.0, 10.0]] }],
    };
    match run(&job).unwrap().pop() {
        Some(Output::Node { refusal, inner, .. }) => {
            assert_eq!(refusal, None);
            assert!(matches!(inner.as_slice(), [(path, NodeRefusal::TooLarge { .. })] if *path == vec![7]), "{inner:?}");
        }
        o => panic!("{o:?}"),
    }
}

/// A node's result over a board 100 × 60 × 20 (body 1): `blend` of `size` on the edges nearest
/// `picks`.
fn blended_board(
    blend: Blend,
    size: f64,
    picks: Vec<[f64; 3]>,
) -> (Arc<Solid>, Vec<TreeBody>, Ran, Vec<Vec<skraft_kernel::names::FaceTag>>) {
    let b = board(100.0, 60.0, 20.0);
    let solid = Arc::new(b.build().unwrap());
    let children = vec![leaf(1, false, DVec3::ZERO, 0, Some(b))];
    let job = Job {
        inputs: vec![solid.clone()],
        tasks: vec![Task::Node { children: children.clone(), op: NodeOp::blend_of(blend, size, Vec::new()), picks }],
    };
    match run(&job).unwrap().pop() {
        Some(Output::Node { solid: s, edges, failed, refusal, faces, .. }) => {
            (solid, children, Ran { solid: s, edges, failed, refusal }, faces)
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_corner_where_rounds_meet_is_named() {
    // The three edges at the corner (100, 0, 20) rounded: a corner's patch between the three
    // rounds, named by them (it was left without a name: its edges could not be taken).
    let (_, _, r, faces) = blended_board(Blend::Round, 5.0, vec![[50.0, 0.0, 20.0], [100.0, 30.0, 20.0], [100.0, 0.0, 10.0]]);
    assert_eq!(r.refusal, None);
    assert!(faces.iter().all(|f| f.len() == 1), "every face named once: {faces:?}");
    let mut names: Vec<_> = faces.iter().map(|f| f[0].clone()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), faces.len(), "one name per face");
    let corner = faces.iter().find_map(|f| match &f[0].face {
        FaceName::Corner(around) => Some(around.clone()),
        _ => None,
    });
    let around = corner.expect("the corner's patch");
    assert_eq!(around.len(), 3);
    assert!(around.iter().all(|t| matches!(t.face, FaceName::Made(_))), "{around:?}");
}

#[test]
fn rounds_over_chamfers_meeting_at_a_corner_take_their_edges() {
    // The three edges at the corner chamfered 5: the chamfers meet at a point, along three
    // edges each between two of them; a round over the node takes each by their names.
    let (solid, children, r, _) = blended_board(Blend::Chamfer, 5.0, vec![[50.0, 0.0, 20.0], [100.0, 30.0, 20.0], [100.0, 0.0, 10.0]]);
    assert_eq!(r.refusal, None);
    let inner = TreeBody {
        id: 7,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Group { children, op: Some(NodeOp::blend_of(Blend::Chamfer, 5.0, r.edges)) },
    };
    let made = |t: &skraft_kernel::names::FaceTag, a: FaceName, b: FaceName| match &t.face {
        FaceName::Made(between) => between.iter().map(|x| x.face.clone()).collect::<Vec<_>>() == vec![a, b],
        _ => false,
    };
    use FaceName::{Side, Top};
    for (pick, a, b) in [
        ([98.75, 3.75, 16.25], (Top, Side(2)), (Side(1), Side(2))),
        ([96.25, 1.25, 16.25], (Top, Side(1)), (Side(1), Side(2))),
        ([96.25, 3.75, 18.75], (Top, Side(1)), (Top, Side(2))),
    ] {
        let o = node(vec![solid.clone()], vec![inner.clone()], round(0.5, Vec::new()), vec![pick]);
        assert_eq!(o.refusal, None, "{pick:?}");
        let [e] = o.edges.as_slice() else { panic!("{:?}", o.edges) };
        let [x, y] = &e.name.faces;
        assert!(
            (made(x, a.0.clone(), a.1.clone()) && made(y, b.0.clone(), b.1.clone()))
                || (made(y, a.0.clone(), a.1.clone()) && made(x, b.0.clone(), b.1.clone())),
            "{pick:?}: {e:?}"
        );
    }
}

#[test]
fn an_edge_of_a_name_on_a_whole_circle_is_found_again_by_its_point() {
    // A torus (body 2, upright, major 20, minor 5) joined to a board (body 1) at its top: its
    // one face meets the top along two circles, r 15 and r 25, of one name. The outer one
    // picked, found again by its point — a circle's ends are one vertex, on its seam: by
    // their chord, the inner one was the nearer.
    let b = board(100.0, 100.0, 20.0);
    let inputs = vec![Arc::new(b.build().unwrap()), Arc::new(Solid::new_torus(DVec3::ZERO, 20.0, 5.0).unwrap())];
    let children = || vec![leaf(1, false, DVec3::ZERO, 0, Some(b.clone())), leaf(2, false, DVec3::new(50.0, 50.0, 20.0), 1, None)];
    let first = node(inputs.clone(), children(), round(1.0, Vec::new()), vec![[50.0, 75.0, 20.0]]);
    assert_eq!(first.refusal, None);
    let [e] = first.edges.as_slice() else { panic!("{:?}", first.edges) };
    assert_eq!(e.near.len(), 1, "one of the name's two circles: {e:?}");
    let again = node(inputs, children(), round(1.0, first.edges.clone()), Vec::new());
    assert_eq!(again.refusal, None);
    assert!((volume(&again.solid) - volume(&first.solid)).abs() < 1e-3, "{} vs {}", volume(&again.solid), volume(&first.solid));
}

#[test]
fn a_hole_across_an_edge_cuts_its_name_in_two_and_a_whole_name_takes_both() {
    // The board's top edge at x = 100 (body 1) crossed by a hole r 5 (body 2) at y = 30: two
    // pieces of one name, y 0–25 and 35–60.
    let b = board(100.0, 60.0, 20.0);
    let inputs = vec![Arc::new(b.build().unwrap()), Arc::new(Solid::new_cylinder(DVec3::ZERO, 5.0, 30.0).unwrap())];
    let children = |hole: bool| {
        let mut c = vec![leaf(1, false, DVec3::ZERO, 0, Some(b.clone()))];
        if hole {
            c.push(leaf(2, true, DVec3::new(100.0, 30.0, -5.0), 1, None));
        }
        c
    };
    // Near no edge: refused, the children's result as it is.
    let sharp = node(inputs.clone(), children(true), round(2.0, Vec::new()), vec![[50.0, 30.0, 20.0]]);
    assert_eq!(sharp.refusal, Some(NodeRefusal::NoEdge));
    let sharp = volume(&sharp.solid);
    // One piece picked: that one only, by its point.
    let one = node(inputs.clone(), children(true), round(2.0, Vec::new()), vec![[100.0, 10.0, 20.0]]);
    let [e] = one.edges.as_slice() else { panic!("{:?}", one.edges) };
    assert_eq!(e.near.len(), 1);
    // Both picked: the whole name.
    let both = node(inputs.clone(), children(true), round(2.0, Vec::new()), vec![[100.0, 10.0, 20.0], [100.0, 50.0, 20.0]]);
    let [e] = both.edges.as_slice() else { panic!("{:?}", both.edges) };
    assert!(e.near.is_empty());
    let (one, both) = (sharp - volume(&one.solid), sharp - volume(&both.solid));
    assert!((both - 2.0 * one).abs() < 0.5, "{both} vs 2 × {one}");
    // Picked whole before the hole was drilled: after it, the round takes both pieces (the
    // intent, « the edge between the top and that side », is the same).
    let before = node(inputs.clone(), children(false), round(2.0, Vec::new()), vec![[100.0, 30.0, 20.0]]);
    let after = node(inputs, children(true), round(2.0, before.edges), Vec::new());
    assert_eq!(after.refusal, None);
    assert!((sharp - volume(&after.solid) - both).abs() < 1e-3);
}

#[test]
fn a_profile_edited_keeps_its_edges_names_by_its_sides_ids() {
    // An L (sides 1–6, side 4 the inner wall at x = 20, from y 20 to 80), the top edge of that
    // wall rounded; then a corner moved (100, 20 → 30) and one inserted on side 1 (at x = 50):
    // the sides keep their ids, the name finds the same edge — the round as picked afresh.
    let l = outline(&[(0.0, 0.0), (100.0, 0.0), (100.0, 20.0), (20.0, 20.0), (20.0, 80.0), (0.0, 80.0)], &[]);
    let mut edited = l.clone();
    edited[2].p = DVec2::new(100.0, 30.0);
    edited.insert(1, profile::Corner::new(DVec2::new(50.0, 0.0)));
    profile::settle_ids(&mut edited);
    assert_eq!(edited.iter().map(|c| c.id).collect::<Vec<_>>(), vec![1, 7, 2, 3, 4, 5, 6], "the new corner a new id");
    let run = |p: &[profile::Corner], op: NodeOp, picks: Vec<[f64; 3]>| {
        let prim = Primitive::Prism { origin: DVec3::ZERO, profile: p.to_vec(), height: 20.0 };
        node(vec![Arc::new(prim.build().unwrap())], vec![leaf(1, false, DVec3::ZERO, 0, Some(prim))], op, picks)
    };
    let first = run(&l, round(2.0, Vec::new()), vec![[20.0, 50.0, 20.0]]);
    let [e] = first.edges.as_slice() else { panic!("{:?}", first.edges) };
    let faces: Vec<_> = e.name.faces.iter().map(|f| f.face.clone()).collect();
    assert_eq!(faces, vec![FaceName::Top, FaceName::Side(4)]);
    let again = run(&edited, round(2.0, first.edges.clone()), Vec::new());
    assert_eq!(again.refusal, None);
    let fresh = run(&edited, round(2.0, Vec::new()), vec![[20.0, 50.0, 20.0]]);
    assert_eq!(fresh.edges, first.edges, "picked afresh: the same name");
    assert!((volume(&again.solid) - volume(&fresh.solid)).abs() < 1e-6);
}

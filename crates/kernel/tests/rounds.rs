//! Booleans on round faces, the kernel's least proven ground (docs/decisions/0042-body-nodes-operators.md,
//! « Consequences »): a rounded or chamfered node's result cut, joined, rounded again, its
//! children stretched — each as the app runs it (the node's solid an input of its parent's
//! recipe, read back from its record). Each result is checked against itself: the kernel's
//! volume against its display mesh's, and `A − B` + `A ∩ B` against `A`.

use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::names::{Blend, NodeOp, TreeBody, TreeNode};
use skraft_kernel::profile;
use skraft_kernel::{BooleanOp, Job, Output, Primitive, Solid, Task, run};

/// A rectangle's prism from the origin.
fn board(l: f64, w: f64, t: f64) -> Primitive {
    let mut p = profile::polygon(&[DVec2::ZERO, DVec2::new(l, 0.0), DVec2::new(l, w), DVec2::new(0.0, w)]);
    profile::settle_ids(&mut p);
    Primitive::Prism { origin: DVec3::ZERO, profile: p, height: t }
}

fn leaf(id: u64, at: DVec3, solid: u32, recipe: Option<Primitive>) -> TreeBody {
    TreeBody { id, negative: false, intersect: false, placement: DAffine3::from_translation(at), node: TreeNode::Leaf { solid, recipe } }
}

/// `b`'s top edges, `l × w × t`, at their middles.
fn top_edges(l: f64, w: f64, t: f64) -> Vec<[f64; 3]> {
    vec![[l / 2.0, 0.0, t], [l, w / 2.0, t], [l / 2.0, w, t], [0.0, w / 2.0, t]]
}

/// The node `blend` of `size` over the board `l × w × t`, its edges `picks`: its solid, its
/// edges' names.
fn blended(l: f64, w: f64, t: f64, blend: Blend, size: f64, picks: Vec<[f64; 3]>) -> (Solid, Vec<skraft_kernel::names::NodeEdges>) {
    let b = board(l, w, t);
    let solid = Arc::new(b.build().unwrap());
    let op = NodeOp::blend_of(blend, size, Vec::new());
    let job = Job { inputs: vec![solid], tasks: vec![Task::Node { children: vec![leaf(1, DVec3::ZERO, 0, Some(b))], op, picks }] };
    match run(&job).unwrap().pop() {
        Some(Output::Node { solid, edges, refusal: None, failed, .. }) if failed.is_empty() => (solid, edges),
        o => panic!("the node: {o:?}"),
    }
}

/// The board 100 × 60 × 20, its four top edges rounded 5.
fn rounded() -> Solid {
    blended(100.0, 60.0, 20.0, Blend::Round, 5.0, top_edges(100.0, 60.0, 20.0)).0
}

/// The same, chamfered 5.
fn chamfered() -> Solid {
    blended(100.0, 60.0, 20.0, Blend::Chamfer, 5.0, top_edges(100.0, 60.0, 20.0)).0
}

/// A group's recipe, as the app runs it over its children's results: `(∪ positives) − (∪
/// negatives)`; none of them may fail.
fn recipe(positives: &[(&Solid, DVec3)], negatives: &[(&Solid, DVec3)]) -> Solid {
    let mut inputs = Vec::new();
    let mut place = |list: &[(&Solid, DVec3)]| -> Vec<(u32, DAffine3)> {
        list.iter()
            .map(|(s, at)| {
                inputs.push(Arc::new((*s).clone()));
                (inputs.len() as u32 - 1, DAffine3::from_translation(*at))
            })
            .collect()
    };
    let (positives, negatives) = (place(positives), place(negatives));
    let job = Job { inputs, tasks: vec![Task::Recipe { base: None, positives, negatives, intersects: vec![], names: None }] };
    match run(&job).unwrap().pop() {
        Some(Output::Recipe { solid, failed, display, .. }) => {
            assert!(failed.is_empty(), "inputs failed: {failed:?}");
            assert!(display.is_ok(), "no display: {display:?}");
            solid
        }
        o => panic!("the recipe: {o:?}"),
    }
}

/// Volume enclosed by the display mesh, positive when its triangles face outwards.
fn mesh_volume(s: &Solid) -> f64 {
    let m = s.mesh().unwrap();
    let p = |i: u32| DVec3::from_array(m.positions[i as usize].map(f64::from)) + m.origin;
    m.indices.chunks(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2])))).sum::<f64>() / 6.0
}

/// `s` is a sound solid: read back, its volume `expected` (when given), its mesh closed and
/// outward, its volume the solid's within its sag (1 %, small curved pieces).
fn sound(s: &Solid, expected: Option<f64>, what: &str) -> f64 {
    let back = s.transformed(&DAffine3::IDENTITY).unwrap_or_else(|e| panic!("{what}: not read back: {e:?}"));
    let v = back.volume().unwrap_or_else(|e| panic!("{what}: no volume: {e:?}"));
    let m = mesh_volume(&back);
    assert!(v > 0.0 && (v - m).abs() <= 1e-2 * v, "{what}: volume {v} vs its mesh's {m}");
    if let Some(e) = expected {
        assert!((v - e).abs() <= 1e-4 * e, "{what}: volume {v} vs {e}");
    }
    v
}

/// `a − b` and `a ∩ b` (b at `at`) are sound and make up `a`.
fn cut_and_rest(a: &Solid, b: &Solid, at: DVec3, what: &str) -> Solid {
    let va = sound(a, None, what);
    let cut = recipe(&[(a, DVec3::ZERO)], &[(b, at)]);
    let vc = sound(&cut, None, &format!("{what}: A − B"));
    let rest = a.boolean(&b.transformed(&DAffine3::from_translation(at)).unwrap(), BooleanOp::Intersection).unwrap();
    let vr = sound(&rest, None, &format!("{what}: A ∩ B"));
    assert!(vr > 1.0 && vc < va, "{what}: B misses A ({vc} of {va})");
    assert!((vc + vr - va).abs() <= 1e-4 * va, "{what}: {vc} + {vr} ≠ {va}");
    cut
}

const R: f64 = 5.0;

/// The rounded board's volume: rounds of 5 along the top's loop of 2 × (100 + 60) mm,
/// `(1 − π/4)·r²` taken per mm; at each corner the two rounds meet on a mitre, the part both
/// take counted twice: `∫₀ʳ (r − √(r² − q²))² dq = (5/3 − π/2)·r³`.
fn rounded_volume() -> f64 {
    let pi = std::f64::consts::PI;
    100.0 * 60.0 * 20.0 - (1.0 - pi / 4.0) * R * R * 2.0 * 160.0 + 4.0 * (5.0 / 3.0 - pi / 2.0) * R * R * R
}

/// The same chamfered: `r²/2` per mm, `∫₀ʳ q² dq = r³/3` at each corner.
fn chamfered_volume() -> f64 {
    100.0 * 60.0 * 20.0 - 0.5 * R * R * 2.0 * 160.0 + 4.0 * R * R * R / 3.0
}

#[test]
fn the_rounded_board_itself() {
    sound(&rounded(), Some(rounded_volume()), "rounded");
    sound(&chamfered(), Some(chamfered_volume()), "chamfered");
}

// ── Round, then cut ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_hole_through_the_round() {
    // Upright, across the side, the round and the top along y = 0.
    let hole = Solid::new_cylinder(DVec3::ZERO, 4.0, 40.0).unwrap();
    cut_and_rest(&rounded(), &hole, DVec3::new(50.0, 3.0, -10.0), "hole");
}

#[test]
fn a_hole_along_the_round() {
    // Lying along X under the round's face, crossing it out of the board's end.
    let hole =
        Solid::new_cylinder(DVec3::ZERO, 3.0, 40.0).unwrap().transformed(&DAffine3::from_rotation_y(std::f64::consts::FRAC_PI_2)).unwrap();
    cut_and_rest(&rounded(), &hole, DVec3::new(80.0, 4.0, 16.0), "hole along");
}

#[test]
fn a_notch_across_the_round() {
    let notch = Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 20.0, 20.0)).unwrap();
    cut_and_rest(&rounded(), &notch, DVec3::new(40.0, -10.0, 10.0), "notch");
}

#[test]
fn an_end_cut_off_through_the_corner_rounds() {
    // The plane x = 97 crosses the round along y = 0 and the corner's ball.
    let end = Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 80.0, 40.0)).unwrap();
    cut_and_rest(&rounded(), &end, DVec3::new(97.0, -10.0, -10.0), "end");
}

#[test]
fn a_pocket_whose_wall_lies_on_the_round_s_tangent_line() {
    // The round meets the top along y = 5: the pocket's wall there.
    let pocket = Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 20.0, 20.0)).unwrap();
    cut_and_rest(&rounded(), &pocket, DVec3::new(40.0, R, 10.0), "pocket on the tangent line");
}

#[test]
fn a_pocket_whose_floor_lies_on_the_round_s_tangent_line() {
    // The round meets the side along z = 15: a step's floor there, open on the side.
    let step = Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 20.0, 20.0)).unwrap();
    cut_and_rest(&rounded(), &step, DVec3::new(40.0, -10.0, 20.0 - R), "step on the tangent line");
}

#[test]
fn a_cylinder_on_the_round_s_own_surface() {
    // Lying along X, radius 5, on the round's axis (y = 5, z = 15): its surface the round's.
    let rod =
        Solid::new_cylinder(DVec3::ZERO, R, 20.0).unwrap().transformed(&DAffine3::from_rotation_y(std::f64::consts::FRAC_PI_2)).unwrap();
    cut_and_rest(&rounded(), &rod, DVec3::new(40.0, R, 20.0 - R), "coaxial");
}

#[test]
fn a_notch_across_the_chamfer() {
    let notch = Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 20.0, 20.0)).unwrap();
    cut_and_rest(&chamfered(), &notch, DVec3::new(40.0, -10.0, 10.0), "notch in the chamfer");
}

#[test]
fn a_hole_through_the_chamfer() {
    let hole = Solid::new_cylinder(DVec3::ZERO, 4.0, 40.0).unwrap();
    cut_and_rest(&chamfered(), &hole, DVec3::new(50.0, 3.0, -10.0), "hole in the chamfer");
}

// ── Rounds in a union ───────────────────────────────────────────────────────────────────────────

#[test]
fn two_rounded_boards_overlapping() {
    let a = rounded();
    let u = recipe(&[(&a, DVec3::ZERO), (&a, DVec3::new(70.0, 20.0, 0.0))], &[]);
    let v = sound(&u, None, "union");
    let both =
        a.boolean(&a.transformed(&DAffine3::from_translation(DVec3::new(70.0, 20.0, 0.0))).unwrap(), BooleanOp::Intersection).unwrap();
    let vb = sound(&both, None, "their intersection");
    assert!((v + vb - 2.0 * rounded_volume()).abs() <= 1e-4 * v, "{v} + {vb} ≠ 2 × the board");
}

#[test]
fn two_rounded_boards_end_to_end() {
    // Their ends' planes coincide, their rounds meet at x = 100 along the same line.
    let a = rounded();
    let u = recipe(&[(&a, DVec3::ZERO), (&a, DVec3::new(100.0, 0.0, 0.0))], &[]);
    sound(&u, Some(2.0 * rounded_volume()), "end to end");
}

#[test]
fn two_rounded_boards_stacked() {
    // One on the other's top, shifted: the bottom on the top's flat, its edge inside the round.
    let a = rounded();
    let u = recipe(&[(&a, DVec3::ZERO), (&a, DVec3::new(30.0, 3.0, 20.0))], &[]);
    sound(&u, Some(2.0 * rounded_volume()), "stacked");
}

#[test]
fn a_box_pushed_into_the_round() {
    let a = rounded();
    let block = Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 10.0, 10.0)).unwrap();
    let at = DVec3::new(40.0, -7.0, 12.0);
    let u = recipe(&[(&a, DVec3::ZERO), (&block, at)], &[]);
    let v = sound(&u, None, "box in the round");
    let both = a.boolean(&block.transformed(&DAffine3::from_translation(at)).unwrap(), BooleanOp::Intersection).unwrap();
    let vb = sound(&both, None, "box ∩ board");
    assert!((v + vb - rounded_volume() - 2000.0).abs() <= 1e-4 * v, "{v} + {vb} ≠ board + box");
}

#[test]
fn a_rounded_board_and_a_chamfered_one_overlapping() {
    let (a, b) = (rounded(), chamfered());
    let u = recipe(&[(&a, DVec3::ZERO), (&b, DVec3::new(50.0, 30.0, 10.0))], &[]);
    sound(&u, None, "round ∪ chamfer");
}

// ── Round, cut, round again ─────────────────────────────────────────────────────────────────────

#[test]
fn a_node_over_a_rounded_board_notched() {
    // Group: the rounded node minus a notch across its round; a node over it rounds the
    // notch's upright edges (sharp, from the side to the notch's walls).
    let b = board(100.0, 60.0, 20.0);
    let solid = Arc::new(b.build().unwrap());
    let notch = Arc::new(Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 20.0, 20.0)).unwrap());
    let inner = NodeOp::blend_of(Blend::Round, R, Vec::new());
    let (_, names) = blended(100.0, 60.0, 20.0, Blend::Round, R, top_edges(100.0, 60.0, 20.0));
    let rounded_node = TreeBody {
        id: 5,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Group { children: vec![leaf(1, DVec3::ZERO, 0, Some(b))], op: Some(inner.clone().with_edges(names)) },
    };
    let notched = TreeBody {
        id: 6,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Group {
            children: vec![rounded_node, TreeBody { negative: true, ..leaf(2, DVec3::new(40.0, -10.0, 5.0), 1, None) }],
            op: None,
        },
    };
    let op = NodeOp::blend_of(Blend::Round, 2.0, Vec::new());
    let picks = vec![[40.0, 0.0, 10.0], [60.0, 0.0, 10.0]];
    let job = Job { inputs: vec![solid, notch], tasks: vec![Task::Node { children: vec![notched], op, picks }] };
    match run(&job).unwrap().pop() {
        Some(Output::Node { solid, refusal: None, failed, edges, .. }) if failed.is_empty() => {
            assert_eq!(edges.len(), 2, "{edges:?}");
            sound(&solid, None, "round over the notch");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_round_over_the_edge_where_a_notch_meets_the_round() {
    // The notch's wall meets the round's face along an arc: that arc rounded in turn.
    let b = board(100.0, 60.0, 20.0);
    let solid = Arc::new(b.build().unwrap());
    let notch = Arc::new(Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 20.0, 20.0)).unwrap());
    let (_, names) = blended(100.0, 60.0, 20.0, Blend::Round, R, top_edges(100.0, 60.0, 20.0));
    let rounded_node = TreeBody {
        id: 5,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Group { children: vec![leaf(1, DVec3::ZERO, 0, Some(b))], op: Some(NodeOp::blend_of(Blend::Round, R, names)) },
    };
    let notched = TreeBody {
        id: 6,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Group {
            children: vec![rounded_node, TreeBody { negative: true, ..leaf(2, DVec3::new(40.0, -10.0, 5.0), 1, None) }],
            op: None,
        },
    };
    // The arc on the wall x = 40, round's axis at (y 5, z 15): its middle at 45°.
    let s = std::f64::consts::FRAC_1_SQRT_2 * R;
    let picks = vec![[40.0, R - s, 20.0 - R + s]];
    let op = NodeOp::blend_of(Blend::Round, 1.0, Vec::new());
    let job = Job { inputs: vec![solid, notch], tasks: vec![Task::Node { children: vec![notched], op, picks }] };
    match run(&job).unwrap().pop() {
        Some(Output::Node { solid, refusal: None, failed, edges, .. }) if failed.is_empty() => {
            assert!(!edges.is_empty());
            sound(&solid, None, "round over the notch's arc");
        }
        o => panic!("{o:?}"),
    }
}

// ── Round, then its child stretched ─────────────────────────────────────────────────────────────

#[test]
fn a_rounded_board_whose_board_is_stretched_keeps_its_rounds() {
    // The app stretches a node's children and computes the node again (docs/model/bodies.md):
    // the names find the four edges on the longer board.
    let (_, names) = blended(100.0, 60.0, 20.0, Blend::Round, R, top_edges(100.0, 60.0, 20.0));
    let longer = board(150.0, 60.0, 20.0);
    let solid = Arc::new(longer.build().unwrap());
    let op = NodeOp::blend_of(Blend::Round, R, names.clone());
    let job = Job {
        inputs: vec![solid],
        tasks: vec![Task::Node { children: vec![leaf(1, DVec3::ZERO, 0, Some(longer))], op, picks: Vec::new() }],
    };
    match run(&job).unwrap().pop() {
        Some(Output::Node { solid, refusal: None, failed, edges, .. }) if failed.is_empty() => {
            assert_eq!(edges, names);
            let section = 60.0 * 20.0 - (1.0 - std::f64::consts::FRAC_PI_4) * R * R * 2.0;
            sound(&solid, Some(rounded_volume() + 50.0 * section), "stretched");
            // And cut again.
            let hole = Solid::new_cylinder(DVec3::ZERO, 4.0, 40.0).unwrap();
            cut_and_rest(&solid, &hole, DVec3::new(140.0, 3.0, -10.0), "stretched, then a hole");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_rounded_solid_s_record_stretched_along_its_rounds() {
    // A record stretched directly (a part's final shape is not, but a leaf from a rounded
    // import would be): the rounds along X are cylinders along the axis.
    let s = skraft_kernel::Stretch::new(DVec3::X, vec![50.0], vec![0.0, 30.0]).unwrap();
    match s.apply(&rounded()) {
        Ok(longer) => {
            let section = 60.0 * 20.0 - (1.0 - std::f64::consts::FRAC_PI_4) * R * R * 2.0;
            sound(&longer, Some(rounded_volume() + 30.0 * section), "record stretched");
        }
        Err(e) => panic!("refused: {e:?}"),
    }
}

// ── All edges rounded: three rounds meet at each corner ─────────────────────────────────────────

/// The board 100 × 60 × 20, its twelve edges rounded 5: a ball's eighth at each corner.
fn pebble() -> Solid {
    let (l, w, t) = (100.0, 60.0, 20.0);
    let mut picks = top_edges(l, w, t);
    picks.extend(top_edges(l, w, t).into_iter().map(|[x, y, _]| [x, y, 0.0]));
    picks.extend([[0.0, 0.0, t / 2.0], [l, 0.0, t / 2.0], [l, w, t / 2.0], [0.0, w, t / 2.0]]);
    blended(l, w, t, Blend::Round, R, picks).0
}

/// Its volume: the box less `(1 − π/4)·r²` per mm along the edges' straight parts, and at
/// each corner the cube `r³` less the ball's eighth.
fn pebble_volume() -> f64 {
    let pi = std::f64::consts::PI;
    let straight = 4.0 * ((100.0 - 2.0 * R) + (60.0 - 2.0 * R) + (20.0 - 2.0 * R));
    100.0 * 60.0 * 20.0 - (1.0 - pi / 4.0) * R * R * straight - 8.0 * (R * R * R - pi * R * R * R / 6.0)
}

#[test]
fn the_pebble_itself() {
    sound(&pebble(), Some(pebble_volume()), "pebble");
}

#[test]
fn a_hole_through_a_corner_ball() {
    // Upright through the top corner's ball at (100, 0, 20), off its centre.
    let hole = Solid::new_cylinder(DVec3::ZERO, 3.0, 40.0).unwrap();
    cut_and_rest(&pebble(), &hole, DVec3::new(97.0, 2.0, -10.0), "hole in the ball");
}

#[test]
fn a_corner_cut_off_on_a_slant() {
    // A cube turned 45° about Z, its edge across the corner: planes slicing the ball and the
    // three rounds around it.
    let cube = Solid::new_box(DVec3::splat(-10.0), DVec3::splat(20.0))
        .unwrap()
        .transformed(&DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_4))
        .unwrap();
    cut_and_rest(&pebble(), &cube, DVec3::new(100.0, 0.0, 15.0), "corner off");
}

#[test]
fn two_pebbles_overlapping_at_their_corners() {
    let a = pebble();
    let at = DVec3::new(90.0, 50.0, 10.0);
    let u = recipe(&[(&a, DVec3::ZERO), (&a, at)], &[]);
    let v = sound(&u, None, "pebbles");
    let both = a.boolean(&a.transformed(&DAffine3::from_translation(at)).unwrap(), BooleanOp::Intersection).unwrap();
    let vb = sound(&both, None, "their intersection");
    assert!((v + vb - 2.0 * pebble_volume()).abs() <= 1e-4 * v, "{v} + {vb} ≠ 2 × the pebble");
}

#[test]
fn a_ball_on_a_corner_ball() {
    // A ball centred on the corner ball's centre, larger: their surfaces concentric.
    let ball = Solid::new_sphere(DVec3::ZERO, 8.0).unwrap();
    let a = pebble();
    let at = DVec3::new(100.0 - R, R, 20.0 - R);
    let u = recipe(&[(&a, DVec3::ZERO), (&ball, at)], &[]);
    sound(&u, None, "ball on the ball");
    cut_and_rest(&a, &ball, at, "ball out of the ball");
}

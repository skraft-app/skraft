//! The stretch of a solid by rewriting its record (docs/tools/stretch.md): each result
//! is read back by the kernel, measured, and used in a boolean.

use std::f64::consts::{FRAC_PI_2, PI};

use glam::{DAffine3, DMat3, DVec3};
use skraft_kernel::{BooleanOp, Solid, Stretch, StretchError};

/// Volume enclosed by the display mesh, positive when its triangles face outwards.
fn mesh_volume(s: &Solid) -> f64 {
    let m = s.mesh().unwrap();
    let p = |i: u32| DVec3::from_array(m.positions[i as usize].map(f64::from)) + m.origin;
    m.indices.chunks(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2])))).sum::<f64>() / 6.0
}

/// The record read back by the kernel (and written again): the kernel's own checks.
fn read_back(s: &Solid) -> Solid {
    s.transformed(&DAffine3::IDENTITY).expect("the stretched record reads back")
}

fn bounds(s: &Solid) -> (DVec3, DVec3) {
    s.bounds().unwrap()
}

fn assert_near(a: DVec3, b: DVec3) {
    assert!((a - b).abs().max_element() < 1e-6, "{a} ≠ {b}");
}

fn assert_close(a: f64, b: f64, rel: f64) {
    assert!((a - b).abs() <= rel * b.abs(), "{a} ≠ {b}");
}

fn x_stretch(cuts: &[f64], moves: &[f64]) -> Stretch {
    Stretch::new(DVec3::X, cuts.to_vec(), moves.to_vec()).unwrap()
}

fn board() -> Solid {
    Solid::new_box(DVec3::ZERO, DVec3::new(600.0, 300.0, 18.0)).unwrap()
}

/// A cylinder of radius 8, 50 long, along +X from the origin.
fn lying_cylinder() -> Solid {
    Solid::new_cylinder(DVec3::ZERO, 8.0, 50.0).unwrap().transformed(&DAffine3::from_rotation_y(FRAC_PI_2)).unwrap()
}

#[test]
fn a_board_stretches_and_compresses() {
    for d in [150.0, -200.0] {
        let s = read_back(&x_stretch(&[300.0], &[0.0, d]).apply(&board()).unwrap());
        assert_near(bounds(&s).1, DVec3::new(600.0 + d, 300.0, 18.0));
        assert_close(mesh_volume(&s), (600.0 + d) * 300.0 * 18.0, 1e-9);
        // Still a working solid: a hole near the moved end.
        let hole = Solid::new_cylinder(DVec3::new(550.0 + d, 150.0, -5.0), 10.0, 30.0).unwrap();
        s.boolean(&hole, BooleanOp::Difference).unwrap();
    }
}

#[test]
fn a_mirrored_board_stretches() {
    // The kernel writes a mirrored solid's lines as degree-1 splines: still lines.
    let mirror = DAffine3::from_mat3(DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)));
    let tilted = DAffine3::from_rotation_x(0.4) * mirror;
    let board = board().transformed(&tilted).unwrap();
    let s = read_back(&x_stretch(&[-300.0], &[-100.0, 0.0]).apply(&board).unwrap());
    let (lo, hi) = bounds(&s);
    assert_close(hi.x - lo.x, 700.0, 1e-9);
    assert_close(mesh_volume(&s), 700.0 * 300.0 * 18.0, 1e-6); // f32 mesh
}

#[test]
fn several_cuts_move_each_section() {
    let s = read_back(&x_stretch(&[100.0, 400.0], &[-20.0, 0.0, 50.0]).apply(&board()).unwrap());
    let (lo, hi) = bounds(&s);
    assert_near(lo, DVec3::new(-20.0, 0.0, 0.0));
    assert_near(hi, DVec3::new(650.0, 300.0, 18.0));
}

#[test]
fn equal_moves_only_translate() {
    let s = read_back(&x_stretch(&[300.0], &[30.0, 30.0]).apply(&board()).unwrap());
    let (lo, hi) = bounds(&s);
    assert_near(lo, DVec3::new(30.0, 0.0, 0.0));
    assert_near(hi, DVec3::new(630.0, 300.0, 18.0));
}

#[test]
fn a_cylinder_stretches_along_its_axis() {
    let standing = Solid::new_cylinder(DVec3::ZERO, 8.0, 50.0).unwrap();
    let mirrored = lying_cylinder().transformed(&DAffine3::from_mat3(DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)))).unwrap();
    let cases = [
        (standing, Stretch::new(DVec3::Z, vec![25.0], vec![0.0, 30.0]).unwrap(), 80.0),
        (Solid::new_cylinder(DVec3::ZERO, 8.0, 50.0).unwrap(), Stretch::new(DVec3::Z, vec![25.0], vec![0.0, -20.0]).unwrap(), 30.0),
        (lying_cylinder(), x_stretch(&[25.0], &[0.0, 40.0]), 90.0),
        // Along −X, from −50 to 0: the moving section is the low one.
        (mirrored, x_stretch(&[-25.0], &[-15.0, 0.0]), 65.0),
    ];
    for (solid, stretch, length) in cases {
        let s = read_back(&stretch.apply(&solid).unwrap());
        let (lo, hi) = bounds(&s);
        assert_close(stretch.axis().dot(hi - lo).abs(), length, 1e-9);
        assert_close(mesh_volume(&s), PI * 64.0 * length, 2e-2);
        // As a tool, through a blank.
        let blank = Solid::new_box(lo - DVec3::splat(20.0) + stretch.axis().abs() * 30.0, DVec3::splat(40.0)).unwrap();
        blank.boolean(&s, BooleanOp::Difference).unwrap();
    }
}

#[test]
fn a_prism_with_arcs_stretches_along_its_height() {
    use skraft_kernel::profile::{polygon, with_fillets};
    // A board with rounded corners: SolidCore writes its fillets' sides as NURBS nets extruded
    // along Z, not as cylinders.
    let corners = polygon(&[glam::DVec2::ZERO, glam::DVec2::new(600.0, 0.0), glam::DVec2::new(600.0, 300.0), glam::DVec2::new(0.0, 300.0)]);
    let rounded = Solid::new_prism(DVec3::ZERO, &with_fillets(&corners, 20.0), 18.0).unwrap();
    let area = 600.0 * 300.0 - (4.0 - PI) * 400.0;
    let mirror = DAffine3::from_mat3(DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)));
    for solid in [rounded.clone(), rounded.transformed(&mirror).unwrap()] {
        for (d, height) in [(32.0, 50.0), (-8.0, 10.0)] {
            let stretch = Stretch::new(DVec3::Z, vec![9.0], vec![0.0, d]).unwrap();
            let s = read_back(&stretch.apply(&solid).unwrap());
            let (lo, hi) = bounds(&s);
            assert_close(hi.z - lo.z, height, 1e-9);
            assert_close(mesh_volume(&s), area * height, 1e-3);
            // Still a working solid: a hole through it, near a fillet.
            let x = if lo.x < 0.0 { -560.0 } else { 40.0 };
            let hole = Solid::new_cylinder(DVec3::new(x, 40.0, -5.0), 10.0, height + 10.0).unwrap();
            s.boolean(&hole, BooleanOp::Difference).unwrap();
        }
    }
    // Lying along X, stretched along X: the same.
    let lying = rounded.transformed(&DAffine3::from_rotation_y(FRAC_PI_2)).unwrap();
    let s = read_back(&x_stretch(&[9.0], &[0.0, 32.0]).apply(&lying).unwrap());
    assert_close(mesh_volume(&s), area * 50.0, 1e-3);
    // Across its height, through the fillets: scaled over x 580 … 600 (docs/decisions/0058-stretch-scales-curves-across-a-cutter.md).
    assert_scaled(&rounded, 590.0, 10.0, (580.0, 600.0));
}

#[test]
fn a_cutter_beside_an_arc_is_not_across_it() {
    use skraft_kernel::profile::{polygon, with_fillets};
    // An arc spans only its own angles, not its whole circle's: a cutter where the circle passes
    // but not the arc crosses only straight matter.
    let corners = polygon(&[glam::DVec2::ZERO, glam::DVec2::new(600.0, 0.0), glam::DVec2::new(600.0, 300.0), glam::DVec2::new(0.0, 300.0)]);
    let rounded = Solid::new_prism(DVec3::ZERO, &with_fillets(&corners, 100.0), 18.0).unwrap(); // arcs over x 500 … 600
    // A concave arc made by a boolean (an edge with a NURBS form): x ≈ 363 … 537, its circle 350 … 550.
    let cylinder = Solid::new_cylinder(DVec3::new(450.0, 360.0, -5.0), 100.0, 30.0).unwrap();
    let carved = board().boolean(&cylinder, BooleanOp::Difference).unwrap();
    for (solid, beside, across) in [(rounded, 450.0, 550.0), (carved, 355.0, 450.0)] {
        let s = read_back(&x_stretch(&[beside], &[0.0, 100.0]).apply(&solid).unwrap());
        assert_close(mesh_volume(&s), mesh_volume(&solid) + 100.0 * 300.0 * 18.0, 1e-3);
        // Across the arc: scaled over the arc's own extent, not its circle's.
        let arc = if across < 500.0 { (370.0, 530.0) } else { (500.0, 600.0) };
        assert_scaled(&solid, across, 100.0, arc);
        // The extent and the sections' extents still reach the arcs.
        let x = x_stretch(&[beside], &[0.0, 0.0]);
        assert_close(x.extent(&solid).unwrap().1, 600.0, 1e-9);
    }
}

#[test]
fn a_bracket_stretches_beside_its_arc() {
    // A user's bracket, 480 long along X: straight up to x = 220, then a concave arc to 350 (its
    // circle reaches back to x = 76), and a round along its whole length whose NURBS net runs
    // along u. The cutter at x ≈ 115 crosses only straight matter.
    let record = serde_json::from_str(include_str!("fixtures/bracket-round-along-u.json")).unwrap();
    let bracket = Solid::from_record(record);
    let volume = |s: &Solid| s.volume().unwrap();
    // Its section at the cutter, mm².
    let section = 3184.9;
    for d in [100.0, -50.0] {
        let s = read_back(&x_stretch(&[115.0], &[0.0, d]).apply(&bracket).unwrap());
        assert_close(bounds(&s).1.x, 480.0 + d, 1e-9);
        assert_close(volume(&s), volume(&bracket) + d * section, 1e-4);
        let hole = Solid::new_cylinder(DVec3::new(400.0 + d, 200.0, -5.0), 10.0, 60.0).unwrap();
        s.boolean(&hole, BooleanOp::Difference).unwrap();
    }
}

#[test]
fn upright_splines_keep_their_ends_on_their_knots() {
    // A user's part whose edges along the axis are degree-1 splines (SolidCore's prism uprights).
    // Lengthened, an edge's interval and knots must end on the same number: one ulp apart, the
    // interval left the curve's natural domain and the kernel refused the record.
    let record = serde_json::from_str(include_str!("fixtures/foot-upright-splines.json")).unwrap();
    let foot = Solid::from_record(record);
    let x = Stretch::new(DVec3::NEG_Y, vec![-169.82631660309173], vec![-395.61269526791114, 0.0]).unwrap();
    let (lo, hi) = x.extent(&foot).unwrap();
    let s = read_back(&x.apply(&foot).unwrap());
    let (lo2, hi2) = x.extent(&s).unwrap();
    assert_close(hi2 - lo2, hi - lo + 395.61269526791114, 1e-9);
}

#[test]
fn a_tool_is_stretched_in_its_own_frame() {
    // A tool along its own Z, placed along the part's X from x = 100 to 150.
    let tool = Solid::new_cylinder(DVec3::ZERO, 8.0, 50.0).unwrap();
    let placement = DAffine3::from_translation(DVec3::new(100.0, 50.0, 9.0)) * DAffine3::from_rotation_y(FRAC_PI_2);
    let stretch = x_stretch(&[120.0], &[0.0, 100.0]);
    let s = read_back(&stretch.in_frame(&placement).apply(&tool).unwrap());
    let (lo, hi) = bounds(&s.transformed(&placement).unwrap());
    assert_close(lo.x, 100.0, 1e-9);
    assert_close(hi.x, 250.0, 1e-9);
}

#[test]
fn a_machined_shape_stretches() {
    // A hole across the board, in the moving section: carried along.
    let hole = Solid::new_cylinder(DVec3::new(450.0, 150.0, -5.0), 10.0, 30.0).unwrap();
    let drilled = board().boolean(&hole, BooleanOp::Difference).unwrap();
    let s = read_back(&x_stretch(&[300.0], &[0.0, 100.0]).apply(&drilled).unwrap());
    assert_close(mesh_volume(&s), 700.0 * 300.0 * 18.0 - PI * 100.0 * 18.0, 1e-3);
    // The hole moved with its section: a peg at its new place does not touch the board.
    let peg = Solid::new_cylinder(DVec3::new(550.0, 150.0, -5.0), 9.0, 30.0).unwrap();
    assert!(s.boolean(&peg, BooleanOp::Intersection).is_err());

    // A blind hole along the board, across the cutter: lengthened with it.
    let bore = lying_cylinder().transformed(&DAffine3::from_translation(DVec3::new(-10.0, 150.0, 9.0))).unwrap();
    let bore = x_stretch(&[25.0], &[0.0, 460.0]).apply(&bore).unwrap(); // −10 … 500
    let bored = board().boolean(&bore, BooleanOp::Difference).unwrap();
    let s = read_back(&x_stretch(&[300.0], &[0.0, 100.0]).apply(&bored).unwrap());
    assert_close(mesh_volume(&s), 700.0 * 300.0 * 18.0 - PI * 64.0 * 600.0, 1e-3);
}

#[test]
fn a_solid_in_one_section_is_translated() {
    let ball = Solid::new_sphere(DVec3::new(100.0, 0.0, 0.0), 10.0).unwrap();
    let s = read_back(&x_stretch(&[50.0], &[0.0, 25.0]).apply(&ball).unwrap());
    let (lo, hi) = bounds(&s);
    assert_near((lo + hi) / 2.0, DVec3::new(125.0, 0.0, 0.0));
}

/// `solid` stretched by `d` at the cutter `cut` (the far side moving), what crosses it curved
/// scaled over `extent` (docs/decisions/0058-stretch-scales-curves-across-a-cutter.md): read
/// back, its volume grown by `(k − 1)` times the matter in that extent, and still an operand.
fn assert_scaled(solid: &Solid, cut: f64, d: f64, extent: (f64, f64)) -> Solid {
    let s = read_back(&x_stretch(&[cut], &[0.0, d]).apply(solid).unwrap());
    let (lo, hi) = bounds(solid);
    let k = (extent.1 - extent.0 + d) / (extent.1 - extent.0);
    let slab =
        Solid::new_box(DVec3::new(extent.0, lo.y - 1.0, lo.z - 1.0), DVec3::new(extent.1 - extent.0, hi.y - lo.y + 2.0, hi.z - lo.z + 2.0))
            .unwrap();
    let inside = solid.boolean(&slab, BooleanOp::Intersection).unwrap().volume().unwrap();
    assert_close(s.volume().unwrap(), solid.volume().unwrap() + (k - 1.0) * inside, 1e-6);
    assert_close(mesh_volume(&s), s.volume().unwrap(), 2e-2);
    assert_close(bounds(&s).1.x, hi.x + d, 1e-9);
    let (slo, shi) = bounds(&s);
    let probe = Solid::new_box(slo - DVec3::splat(5.0), (shi - slo) * 0.5 + DVec3::splat(5.0)).unwrap();
    s.boolean(&probe, BooleanOp::Difference).unwrap();
    s
}

#[test]
fn what_crosses_curved_is_scaled() {
    // A ball, a cylinder across the axis, a cone and a torus along it, each whole one group:
    // an ellipsoid, an elliptic cylinder, a longer cone, a stretched ring.
    assert_scaled(&Solid::new_sphere(DVec3::ZERO, 10.0).unwrap(), 0.0, 10.0, (-10.0, 10.0));
    assert_scaled(&Solid::new_cylinder(DVec3::new(0.0, 0.0, -5.0), 8.0, 10.0).unwrap(), 0.0, 10.0, (-8.0, 8.0));
    let cone = Solid::new_cone(DVec3::ZERO, 10.0, 5.0, 40.0).unwrap().transformed(&DAffine3::from_rotation_y(FRAC_PI_2)).unwrap();
    assert_scaled(&cone.transformed(&DAffine3::from_translation(DVec3::new(-20.0, 0.0, 0.0))).unwrap(), 0.0, 10.0, (-20.0, 20.0));
    assert_scaled(&Solid::new_torus(DVec3::ZERO, 20.0, 5.0).unwrap(), 0.0, 10.0, (-25.0, 25.0));
    // Compressed too.
    assert_scaled(&Solid::new_sphere(DVec3::ZERO, 10.0).unwrap(), 0.0, -8.0, (-10.0, 10.0));
    // A board slanted in its plane, its corner on the cutter: its two long sides, slanted
    // planes, each its own group from corner to corner; the corners move as their sections.
    let slanted = board().transformed(&DAffine3::from_rotation_z(0.3)).unwrap();
    let s = read_back(&x_stretch(&[0.0], &[0.0, 10.0]).apply(&slanted).unwrap());
    assert_close(s.volume().unwrap(), (600.0 * 300.0 + 10.0 * 300.0 * 0.3f64.cos()) * 18.0, 1e-9);
}

#[test]
fn a_chamfer_across_a_cutter_is_scaled() {
    // A board 400 × 300 × 50, its top edges chamfered 20 (the app's body node), thickened across
    // its chamfers along Z read from a turned frame (its noise, 1e-16): the chamfers are scaled,
    // their planes' slanted axes no longer unit, and no longer square by that noise times the
    // scale, which the kernel's plane cannot hold. Each is written again on unit square axes, its
    // pcurves with it.
    use skraft_kernel::names::{Blend, NodeOp, TreeBody, TreeNode};
    use skraft_kernel::{Job, Output, Primitive, Task, profile, run};
    let (l, w, t, c) = (400.0, 300.0, 50.0, 20.0);
    let mut corners = profile::polygon(&[glam::DVec2::ZERO, glam::DVec2::new(l, 0.0), glam::DVec2::new(l, w), glam::DVec2::new(0.0, w)]);
    profile::settle_ids(&mut corners);
    let prism = Primitive::Prism { origin: DVec3::ZERO, profile: corners, height: t };
    let leaf = TreeBody {
        id: 1,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Leaf { solid: 0, recipe: Some(prism.clone()) },
    };
    let picks = vec![[l / 2.0, 0.0, t], [l, w / 2.0, t], [l / 2.0, w, t], [0.0, w / 2.0, t]];
    let job = Job {
        inputs: vec![prism.build().unwrap().into()],
        tasks: vec![Task::Node { children: vec![leaf], op: NodeOp::blend_of(Blend::Chamfer, c, Vec::new()), picks }],
    };
    let Some(Output::Node { solid: chamfered, .. }) = run(&job).unwrap().pop() else { panic!("the chamfered board") };
    let d = 300.0;
    let axis = DVec3::new(-1.1877578532930653e-16, -1.884519498393009e-16, 1.0);
    let s = read_back(&Stretch::new(axis, vec![t - c / 2.0], vec![0.0, d]).unwrap().apply(&chamfered).unwrap());
    assert_close(bounds(&s).1.z, t + d, 1e-9);
    // The chamfers' band, c high, grown to c + d: its matter scaled by (c + d) / c.
    let band = |s: &Solid, z0: f64, z1: f64| {
        let slab = Solid::new_box(DVec3::new(-1.0, -1.0, z0), DVec3::new(l + 2.0, w + 2.0, z1 - z0)).unwrap();
        s.boolean(&slab, BooleanOp::Intersection).unwrap().volume().unwrap()
    };
    let inside = band(&chamfered, t - c, t);
    assert_close(s.volume().unwrap(), chamfered.volume().unwrap() + inside * d / c, 1e-6);
}

#[test]
fn a_scaled_arc_keeps_a_square_frame() {
    // A board carved by a large cylinder, compressed across the arc along an axis a hair off X
    // (a turned frame's): the ellipse's frame square to rounding, else the kernel refuses it
    // ("the frame's axes are not unit and square").
    let board = Solid::new_box(DVec3::ZERO, DVec3::new(396.93783738585364, 237.37428288556868, 38.038596832646)).unwrap();
    let cylinder =
        Solid::new_cylinder(DVec3::new(115.32155070381755, 367.2756827467284, -5.0), 232.59534655971154, 48.038596832646).unwrap();
    let carved = board.boolean(&cylinder, BooleanOp::Difference).unwrap();
    let axis = DVec3::new(1.0, -1.6653345369377348e-16, -2.379301982956869e-16);
    let s = read_back(&Stretch::new(axis, vec![129.6671852667689], vec![0.0, -291.2253859679609]).unwrap().apply(&carved).unwrap());
    assert!(s.volume().unwrap() < carved.volume().unwrap());
}

#[test]
fn a_concave_arc_stretched_across_comes_back() {
    // A board whose end is a concave arc, x 186 … 234, stretched across the arc, then back. Once
    // scaled, the arc side is an elliptic net whose middle control points lie far out (x ≈ −240):
    // its extent along the axis is its boundary's (a ruled net), else the way back scales it
    // about that hull and halves the board.
    use skraft_kernel::profile::{Corner, polygon};
    let (l, w) = (233.72269661294416, 275.993212093354);
    let mut corners = polygon(&[glam::DVec2::ZERO, glam::DVec2::new(l, 0.0), glam::DVec2::new(l, w), glam::DVec2::new(0.0, w)]);
    corners[1] = Corner::arc(corners[1].p, -0.34307316814988387);
    let end = Solid::new_prism(DVec3::ZERO, &corners, 41.88307882214465).unwrap();
    let (cut, d) = (209.5749354828884, 289.2995786655431);
    let s = read_back(&x_stretch(&[cut], &[0.0, d]).apply(&end).unwrap());
    assert_close(bounds(&s).1.x, l + d, 1e-9);
    let (lo, _) = x_stretch(&[0.0], &[0.0, 0.0]).section_spans(&s)[1].unwrap();
    assert!(lo > 0.0, "the scaled arc's extent is its own: {lo}");
    let back = read_back(&x_stretch(&[cut + d / 2.0], &[0.0, -d]).apply(&s).unwrap());
    assert_close(back.volume().unwrap(), end.volume().unwrap(), 1e-9);
}

#[test]
fn a_hole_across_a_cutter_goes_oval() {
    // Ø 50 through the board at x = 200: scaled over x 175 … 225, the board lengthened around it.
    let hole = Solid::new_cylinder(DVec3::new(200.0, 150.0, -5.0), 25.0, 30.0).unwrap();
    let drilled = board().boolean(&hole, BooleanOp::Difference).unwrap();
    for d in [60.0, -20.0] {
        let s = read_back(&x_stretch(&[200.0], &[0.0, d]).apply(&drilled).unwrap());
        let k = (50.0 + d) / 50.0;
        assert_close(s.volume().unwrap(), (600.0 + d) * 300.0 * 18.0 - PI * 625.0 * k * 18.0, 1e-6);
    }
}

#[test]
fn a_rounded_end_cut_in_its_bulge_stretches_into_half_an_ellipse() {
    use skraft_kernel::profile::{polygon, with_fillets};
    // A 60 wide tenon with a half round end, x 0 … 330: its ends at x = 300 stay, its tip moves.
    let corners = polygon(&[glam::DVec2::ZERO, glam::DVec2::new(330.0, 0.0), glam::DVec2::new(330.0, 60.0), glam::DVec2::new(0.0, 60.0)]);
    let tenon = Solid::new_prism(DVec3::ZERO, &with_fillets(&corners, 30.0), 20.0).unwrap();
    let s = read_back(&x_stretch(&[315.0], &[0.0, 30.0]).apply(&tenon).unwrap());
    assert_close(bounds(&s).1.x, 360.0, 1e-9);
    // The far rounded end over 300 … 330 doubled; the near one (0 … 30) as it was.
    let near = 30.0 * 60.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 900.0 * 2.0;
    let far = 30.0 * 60.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 900.0 * 2.0;
    assert_close(s.volume().unwrap(), (near + 270.0 * 60.0 + 2.0 * far) * 20.0, 1e-6);
}

#[test]
fn too_much_compression_is_refused() {
    assert_eq!(x_stretch(&[300.0], &[0.0, -650.0]).apply(&board()), Err(StretchError::Collapsed));
}

#[test]
fn extents_along_the_axis() {
    let x = x_stretch(&[0.0], &[0.0, 0.0]);
    let (lo, hi) = x.extent(&board()).unwrap();
    assert_close(lo, 0.0, 1e-9);
    assert_close(hi, 600.0, 1e-9);
    // A standing cylinder: its circles, not its seam's vertices.
    let (lo, hi) = x.extent(&Solid::new_cylinder(DVec3::new(100.0, 0.0, 0.0), 8.0, 50.0).unwrap()).unwrap();
    assert_close(lo, 92.0, 1e-9);
    assert_close(hi, 108.0, 1e-9);
    let (lo, hi) = x.extent(&Solid::new_sphere(DVec3::new(100.0, 0.0, 0.0), 10.0).unwrap()).unwrap();
    assert!(lo <= 90.0 + 1e-9 && hi >= 110.0 - 1e-9);
}

#[test]
fn malformed_stretches_are_refused() {
    assert_eq!(Stretch::new(DVec3::ZERO, vec![], vec![0.0]), Err(StretchError::Invalid));
    assert_eq!(Stretch::new(DVec3::X, vec![2.0, 1.0], vec![0.0, 0.0, 0.0]), Err(StretchError::Invalid));
    assert_eq!(Stretch::new(DVec3::X, vec![1.0], vec![0.0]), Err(StretchError::Invalid));
}

#[test]
fn a_stretch_crosses_to_the_worker_and_back() {
    use skraft_kernel::{Job, Task, decode, encode};
    let job = Job { inputs: vec![board().into()], tasks: vec![Task::Stretch { solid: 0, stretch: x_stretch(&[300.0], &[0.0, 10.0]) }] };
    let back: Job = decode(&encode(&job).unwrap()).unwrap();
    assert_eq!(back.tasks, job.tasks);
    // Malformed data is refused when read.
    #[derive(serde::Serialize)]
    struct Raw {
        axis: DVec3,
        cuts: Vec<f64>,
        moves: Vec<f64>,
    }
    let bad = encode(&Raw { axis: DVec3::X, cuts: vec![1.0], moves: vec![0.0] }).unwrap();
    assert!(decode::<Stretch>(&bad).is_err());
    // Run: the board 10 longer.
    let out = skraft_kernel::run(&job).unwrap();
    let [skraft_kernel::Output::Solid { solid, display }] = &out[..] else { panic!("{out:?}") };
    assert!(display.is_ok());
    assert_close(bounds(solid).1.x, 610.0, 1e-9);
}

#[test]
fn the_matter_of_each_section() {
    // A board cut at 100 and 400: the end faces' edges are in the end sections, nothing whole in
    // the middle one.
    let spans = x_stretch(&[100.0, 400.0], &[0.0, 0.0, 0.0]).section_spans(&board());
    assert_eq!(spans, vec![Some((0.0, 0.0)), None, Some((600.0, 600.0))]);
}

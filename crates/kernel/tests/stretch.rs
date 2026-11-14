//! The stretch of a solid by rewriting its record (docs/ARCHITECTURE.md §6 sexies): each result
//! is read back by the kernel, measured, and used in a boolean.

use std::f64::consts::{FRAC_PI_2, PI};

use glam::{DAffine3, DMat3, DVec3};
use solidsketch_kernel::{BooleanOp, Solid, Stretch, StretchError};

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
    use solidsketch_kernel::profile::{polygon, with_fillets};
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
    // Across its height, through the fillets: refused.
    assert!(matches!(x_stretch(&[590.0], &[0.0, 10.0]).apply(&rounded), Err(StretchError::EdgeAcross(_) | StretchError::FaceAcross(_))));
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

#[test]
fn what_cannot_stretch_is_refused() {
    let stretch = x_stretch(&[0.0], &[0.0, 10.0]);
    let refused = |s: &Solid| matches!(stretch.apply(s), Err(StretchError::EdgeAcross(_) | StretchError::FaceAcross(_)));
    assert!(refused(&Solid::new_sphere(DVec3::ZERO, 10.0).unwrap()));
    // A cylinder across the axis, a cone and a torus along it.
    assert!(refused(&Solid::new_cylinder(DVec3::new(0.0, 0.0, -5.0), 8.0, 10.0).unwrap()));
    let cone = Solid::new_cone(DVec3::ZERO, 10.0, 5.0, 40.0).unwrap().transformed(&DAffine3::from_rotation_y(FRAC_PI_2)).unwrap();
    assert!(refused(&cone.transformed(&DAffine3::from_translation(DVec3::new(-20.0, 0.0, 0.0))).unwrap()));
    assert!(refused(&Solid::new_torus(DVec3::ZERO, 20.0, 5.0).unwrap()));
    // A board slanted in its plane: its long faces are no longer along the axis.
    let slanted = board().transformed(&DAffine3::from_rotation_z(0.3)).unwrap();
    assert!(refused(&slanted));
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
    use solidsketch_kernel::{Job, Task, decode, encode};
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
    let out = solidsketch_kernel::run(&job).unwrap();
    let [solidsketch_kernel::Output::Solid { solid, display }] = &out[..] else { panic!("{out:?}") };
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

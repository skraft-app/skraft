//! What the parts / machinings model (docs/model/data-model.md) asks of SolidCore: mirrors in a
//! placement, a blank minus many tools, and how a bad machining fails. Run with
//! `cargo test -p skraft-kernel --test part_model -- --nocapture` to see the timings.

use std::time::Instant;

use glam::{DAffine3, DMat3, DQuat, DVec3};
use skraft_kernel::{BooleanOp, Error, Solid};

/// Volume enclosed by the display mesh, positive when its triangles face outwards.
fn mesh_volume(s: &Solid) -> f64 {
    let m = s.mesh().unwrap();
    let p = |i: u32| DVec3::from_array(m.positions[i as usize].map(f64::from));
    m.indices.chunks(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2])))).sum::<f64>() / 6.0
}

fn mirror_x() -> DAffine3 {
    DAffine3::from_mat3(DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)))
}

/// A mirror that is not about a principal plane: a rotation, a reflection and a translation.
fn skew_mirror() -> DAffine3 {
    DAffine3::from_translation(DVec3::new(300.0, -40.0, 12.0))
        * DAffine3::from_quat(DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 3.0).normalize(), 0.7))
        * mirror_x()
}

fn board() -> Solid {
    Solid::new_box(DVec3::ZERO, DVec3::new(600.0, 300.0, 18.0)).unwrap()
}

/// Every kind of geometry a part may hold: planes, cylinders, spheres, cones, tori, and the
/// spline intersection curves of booleans.
fn samples() -> Vec<(&'static str, Solid)> {
    let b = Solid::new_box(DVec3::ZERO, DVec3::splat(40.0)).unwrap();
    let cyl = Solid::new_cylinder(DVec3::new(20.0, 20.0, -5.0), 8.0, 50.0).unwrap();
    let sph = Solid::new_sphere(DVec3::new(40.0, 40.0, 40.0), 15.0).unwrap();
    // Two cylinders crossing at an angle: a spline intersection curve.
    let tilted = Solid::new_cylinder(DVec3::new(0.0, 0.0, -30.0), 6.0, 60.0)
        .unwrap()
        .transformed(&(DAffine3::from_translation(DVec3::new(20.0, 20.0, 20.0)) * DAffine3::from_rotation_y(1.0)))
        .unwrap();
    vec![
        ("box", b.clone()),
        ("cylinder", Solid::new_cylinder(DVec3::ZERO, 10.0, 30.0).unwrap()),
        ("sphere", Solid::new_sphere(DVec3::ZERO, 10.0).unwrap()),
        ("cone", Solid::new_cone(DVec3::ZERO, 10.0, 4.0, 20.0).unwrap()),
        ("torus", Solid::new_torus(DVec3::ZERO, 20.0, 5.0).unwrap()),
        ("box - cylinder", b.boolean(&cyl, BooleanOp::Difference).unwrap()),
        ("box - sphere", b.boolean(&sph, BooleanOp::Difference).unwrap()),
        ("box - tilted cylinder", b.boolean(&tilted, BooleanOp::Difference).unwrap()),
    ]
}

#[test]
fn mirrored_placements_keep_solids_valid() {
    for (name, s) in samples() {
        let v = mesh_volume(&s);
        for (how, t) in [("mirror x", mirror_x()), ("rotated mirror", skew_mirror())] {
            let m = s.transformed(&t).unwrap_or_else(|e| panic!("{name}, {how}: {e}"));
            assert_eq!(m.topology_counts(), s.topology_counts(), "{name}, {how}");
            let vm = mesh_volume(&m);
            assert!((vm - v).abs() < 1e-2 * v.abs(), "{name}, {how}: volume {vm} for {v} (faces inside out?)");
            // Its points are the mirrored points.
            let (lo, hi) = s.vertex_bounds().unwrap_or((DVec3::ZERO, DVec3::ZERO));
            let (mlo, mhi) = m.vertex_bounds().unwrap_or((DVec3::ZERO, DVec3::ZERO));
            if how == "mirror x" && lo != hi {
                assert!((mlo.x + hi.x).abs() < 1e-9 && (mhi.x + lo.x).abs() < 1e-9, "{name}: {mlo} {mhi}");
            }
            // A mirrored solid takes part in booleans, as target and as tool.
            let probe = Solid::new_sphere(t.transform_point3(DVec3::splat(5.0)), 6.0).unwrap();
            m.boolean(&probe, BooleanOp::Difference).unwrap_or_else(|e| panic!("{name}, {how}, as target: {e}"));
            probe.boolean(&m, BooleanOp::Union).unwrap_or_else(|e| panic!("{name}, {how}, as tool: {e}"));
        }
        println!("{name:>22}: mirrors ok, volume {v:.0}");
    }
}

#[test]
fn mirrored_tool_cuts_like_the_mirrored_part() {
    // Cutting a mirrored copy = mirroring the cut copy: left and right stay the same part.
    let groove = Solid::new_box(DVec3::new(50.0, -1.0, 10.0), DVec3::new(20.0, 302.0, 9.0)).unwrap();
    let cut_then_mirror = board().boolean(&groove, BooleanOp::Difference).unwrap().transformed(&skew_mirror()).unwrap();
    let mirror_then_cut =
        board().transformed(&skew_mirror()).unwrap().boolean(&groove.transformed(&skew_mirror()).unwrap(), BooleanOp::Difference).unwrap();
    assert_eq!(cut_then_mirror.topology_counts(), mirror_then_cut.topology_counts());
    let (a, b) = (mesh_volume(&cut_then_mirror), mesh_volume(&mirror_then_cut));
    assert!((a - b).abs() < 1e-6 * a, "{a} {b}");
    let (alo, ahi) = cut_then_mirror.vertex_bounds().unwrap();
    let (blo, bhi) = mirror_then_cut.vertex_bounds().unwrap();
    assert!((alo - blo).length() < 1e-6 && (ahi - bhi).length() < 1e-6);
}

/// `n` drill holes of ⌀8 × 12 through the top of the board, on a grid.
fn holes(n: usize) -> Vec<Solid> {
    (0..n)
        .map(|i| {
            let (x, y) = (40.0 + 50.0 * (i % 10) as f64, 40.0 + 50.0 * (i / 10) as f64);
            Solid::new_cylinder(DVec3::new(x, y, 6.0), 4.0, 20.0).unwrap()
        })
        .collect()
}

#[test]
fn blank_minus_many_tools() {
    let hole_volume = std::f64::consts::PI * 16.0 * 12.0;
    for n in [5, 20, 50] {
        let tools = holes(n);
        let expected = 600.0 * 300.0 * 18.0 - n as f64 * hole_volume;

        let t = Instant::now();
        let mut seq = board();
        for tool in &tools {
            seq = seq.boolean(tool, BooleanOp::Difference).unwrap();
        }
        let sequential = t.elapsed();

        let t = Instant::now();
        let mut union = tools[0].clone();
        for tool in &tools[1..] {
            union = union.boolean(tool, BooleanOp::Union).unwrap();
        }
        let tools_union = t.elapsed();
        let once = board().boolean(&union, BooleanOp::Difference).unwrap();
        let union_then_cut = t.elapsed();

        for (how, s) in [("sequential", &seq), ("union then cut", &once)] {
            let v = mesh_volume(s);
            assert!((v - expected).abs() < 1e-3 * expected, "{n} holes, {how}: {v} for {expected}");
        }
        assert_eq!(seq.topology_counts(), once.topology_counts(), "{n} holes");
        println!(
            "{n:>3} holes: sequential {:>7.1} ms | union of tools {:>7.1} ms + cut = {:>7.1} ms | {:?} faces/edges/vertices",
            sequential.as_secs_f64() * 1e3,
            tools_union.as_secs_f64() * 1e3,
            union_then_cut.as_secs_f64() * 1e3,
            seq.topology_counts()
        );
    }
}

#[test]
fn how_bad_machinings_fail() {
    let b = board();
    let cases: Vec<(&str, Solid)> = vec![
        ("tool beside the blank", Solid::new_box(DVec3::new(1000.0, 0.0, 0.0), DVec3::splat(10.0)).unwrap()),
        ("tool swallowing the blank", Solid::new_box(DVec3::splat(-10.0), DVec3::new(700.0, 400.0, 50.0)).unwrap()),
        ("tool flush with a face", Solid::new_box(DVec3::new(100.0, 100.0, 18.0), DVec3::new(50.0, 50.0, 10.0)).unwrap()),
        ("rebate sharing two faces", Solid::new_box(DVec3::new(-1.0, -1.0, 9.0), DVec3::new(602.0, 20.0, 9.0)).unwrap()),
        ("tool touching an edge", Solid::new_box(DVec3::new(600.0, 300.0, 0.0), DVec3::splat(10.0)).unwrap()),
        ("hole tangent to a side", Solid::new_cylinder(DVec3::new(300.0, 4.0, 6.0), 4.0, 20.0).unwrap()),
    ];
    for (name, tool) in cases {
        let t = Instant::now();
        let r = b.boolean(&tool, BooleanOp::Difference);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let what = match &r {
            Ok(s) => format!("ok, {:?}, volume {:.0}", s.topology_counts(), mesh_volume(s)),
            Err(Error::Empty(_)) => "empty result".into(),
            Err(e) => format!("error: {e}"),
        };
        println!("{name:>26}: {what} ({ms:.1} ms)");
    }
    // A tool taking everything says so.
    let all = Solid::new_box(DVec3::splat(-10.0), DVec3::new(700.0, 400.0, 50.0)).unwrap();
    assert!(matches!(b.boolean(&all, BooleanOp::Difference), Err(Error::Empty(_))));
    // Whatever each did, the board is still usable: one bad machining spoils nothing else.
    let ok = b.boolean(&holes(1)[0], BooleanOp::Difference).unwrap();
    assert!(mesh_volume(&ok) > 0.0);
}

/// Largest |u·v| / (|u||v|) over the plane faces: 0 when every plane's axes are square.
fn worst_skew(s: &Solid) -> f64 {
    use solidcore_record::brep::Surface;
    s.record()
        .faces
        .iter()
        .filter_map(|f| match &f.surface {
            Surface::Plane { u_axis, v_axis, .. } => {
                let (u, v) = (DVec3::from_array(*u_axis), DVec3::from_array(*v_axis));
                Some((u.dot(v) / (u.length() * v.length())).abs())
            }
            _ => None,
        })
        .fold(0.0, f64::max)
}

#[test]
fn slanted_rotations_leave_solids_usable() {
    // A rotation about a slanted axis left plane axes ~1e-15 off square, which SolidCore then
    // refused to read back: the next boolean failed. `square_frames` (engine.rs) fixes it.
    let rot = DAffine3::from_quat(DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 3.0).normalize(), 0.7));
    let tr = DAffine3::from_translation(DVec3::new(300.0, -40.0, 12.0));
    let probe = Solid::new_sphere(DVec3::ZERO, 1.0).unwrap();
    for (name, s) in samples() {
        for (how, t) in [("rotation", tr * rot), ("rotated mirror", skew_mirror()), ("twice", tr * rot * rot)] {
            let m = s.transformed(&t).unwrap();
            assert!(worst_skew(&m) < 2e-16, "{name}, {how}: skew {:e}", worst_skew(&m));
            m.boolean(&probe, BooleanOp::Union).unwrap_or_else(|e| panic!("{name}, {how}: {e}"));
        }
    }
}

/// Changing a part's axes transforms its shape: what is held by index (the faces' names and
/// materials) must still name the same faces (docs/decisions/0054-changing-a-components-axes.md).
#[test]
fn a_rigid_transform_keeps_the_record_s_order() {
    let slanted = DAffine3::from_quat(DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 3.0).normalize(), 0.7));
    let moved = DAffine3::from_translation(DVec3::new(300.0, -40.0, 12.0));
    let turns = [
        ("quarter turn", DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2)),
        ("half turn", DAffine3::from_rotation_x(std::f64::consts::PI)),
        ("translation", moved),
        ("oblique", slanted),
        ("oblique, moved", moved * slanted),
    ];
    let probe = Solid::new_sphere(DVec3::ZERO, 1.0).unwrap();
    for (name, s) in samples() {
        for (how, t) in &turns {
            let m = s.transformed(t).unwrap_or_else(|e| panic!("{name}, {how}: {e}"));
            let (a, b) = (s.record(), m.record());
            assert_eq!(a.faces.len(), b.faces.len(), "{name}, {how}: faces");
            for (i, (fa, fb)) in a.faces.iter().zip(&b.faces).enumerate() {
                let same = std::mem::discriminant(&fa.surface) == std::mem::discriminant(&fb.surface);
                assert!(same, "{name}, {how}: face {i} changed kind");
            }
            assert_eq!(a.vertices.len(), b.vertices.len(), "{name}, {how}: vertices");
            for (i, (va, vb)) in a.vertices.iter().zip(&b.vertices).enumerate() {
                let want = t.transform_point3(DVec3::from_array(va.point));
                assert!(want.distance(DVec3::from_array(vb.point)) < 1e-9, "{name}, {how}: vertex {i} moved elsewhere");
                assert_eq!(s.vertex_faces(i as u32), m.vertex_faces(i as u32), "{name}, {how}: faces around vertex {i}");
            }
            assert_eq!(a.edges.len(), b.edges.len(), "{name}, {how}: edges");
            for i in 0..a.edges.len() as u32 {
                assert_eq!(s.edge_faces(i), m.edge_faces(i), "{name}, {how}: faces along edge {i}");
            }
            // There and back, as an undone change of axes followed by another operation.
            let back = m.transformed(&t.inverse()).unwrap_or_else(|e| panic!("{name}, {how}, back: {e}"));
            assert_eq!(back.topology_counts(), s.topology_counts(), "{name}, {how}, back");
            m.boolean(&probe, BooleanOp::Union).unwrap_or_else(|e| panic!("{name}, {how}, boolean: {e}"));
            back.boolean(&probe, BooleanOp::Union).unwrap_or_else(|e| panic!("{name}, {how}, boolean back: {e}"));
        }
    }
}

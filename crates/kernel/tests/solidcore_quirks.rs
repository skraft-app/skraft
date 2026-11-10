//! SolidCore's known bugs, reproduced on SolidCore itself — **without** our workarounds. Each
//! test asserts that the bug is **still there**: when one fails after a SolidCore upgrade, the
//! bug is fixed upstream; drop its workaround, then the test, and tick its line in
//! docs/REVIEW.md ("Work: SolidCore").
//!
//! Checked at: tangency-labs/solidcore `e4f040d` (crate 1.0.1, 2026-11-09), the `rev` pinned
//! in the workspace's Cargo.toml. Update this line with each upgrade.
//!
//! That our results are right in spite of them is checked elsewhere (`engine.rs`'s tests,
//! `part_model.rs`).
//!
//! `cargo test -p solidsketch-kernel --test solidcore_quirks`

use glam::{DAffine3, DQuat, DVec3};
use solidcore::primitive::api_primitives as prim;
use solidcore::query::api_queries::brep_bounding_box;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::{Brep, Kernel, KernelOptions, Point3};

fn kernel() -> Kernel {
    Kernel::new(&KernelOptions::default())
}

/// SolidCore's matrix convention: row vectors, translation in row 3.
fn row_major(t: &DAffine3) -> [[f64; 4]; 4] {
    let (x, y, z, w) = (t.matrix3.x_axis, t.matrix3.y_axis, t.matrix3.z_axis, t.translation);
    [[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [z.x, z.y, z.z, 0.0], [w.x, w.y, w.z, 1.0]]
}

fn transformed(k: &mut Kernel, mut b: Brep, t: &DAffine3) -> Brep {
    transform_matrix(k, &mut b, &row_major(t), false).expect("transform");
    b
}

/// **Plane axes off square after a slanted rotation.** A rotation about an axis that is not a
/// principal one leaves plane and frame axes ~1e-15 off square in the record, and SolidCore
/// reads a record back only within ~3e-16 (`surface::from_parameters`): the solid cannot be
/// used again (its next boolean fails).
///
/// Workaround: `square_frames` (engine.rs), on every record the kernel writes.
#[test]
fn a_slanted_rotation_leaves_a_record_that_cannot_be_read_back() {
    let mut k = kernel();
    let b = prim::create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 600.0, 300.0, 18.0).unwrap();
    let rot = DAffine3::from_quat(DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 3.0).normalize(), 0.7));
    let mut b = transformed(&mut k, b, &rot);
    let (record, _) = solidcore_convert::brep::to_record(&mut k, &mut b).unwrap();
    let back = solidcore_convert::brep::to_kernel_body(&mut kernel(), &record, true);
    assert!(back.is_err(), "fixed upstream? the record reads back: drop `square_frames` (engine.rs)");
}

/// **Tight bounding box of a mirrored cylinder, read back from a record.** A cylinder read
/// from a record, mirrored about a plane containing its axis, written to a record and read
/// back (what every kernel call does: records in, records out): its side is then a cone of
/// half angle 0 whose frame points the other way (−Z, origin at the top), and
/// `brep_bounding_box(.., tight: true)` gives 1234567 (a placeholder) for its end along the
/// axis. The loose box is right; the same body never written to a record is right too.
///
/// Workaround: `Solid::bounds` (engine.rs) takes the tight box within the loose one.
#[test]
fn the_tight_box_of_a_mirrored_cylinder_is_wrong_along_its_axis() {
    use solidcore_convert::brep::{to_kernel_body, to_record};
    let round_trip = |k: &mut Kernel, b: &mut Brep| {
        let (r, _) = to_record(k, b).unwrap();
        let mut k2 = kernel();
        let b2 = to_kernel_body(&mut k2, &r, true).unwrap();
        (k2, b2)
    };
    let mut k = kernel();
    let mut c = prim::create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 30.0).unwrap();
    let (mut k, c) = round_trip(&mut k, &mut c);
    let mut m = transformed(&mut k, c, &DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
    let (k, m) = round_trip(&mut k, &mut m);
    let tight = brep_bounding_box(&k, &m, true).unwrap();
    let loose = brep_bounding_box(&k, &m, false).unwrap();
    assert!((loose.max_at(2) - 30.0).abs() < 1e-3, "the loose box: {}", loose.max_at(2));
    assert!(
        tight.max_at(2) > 1e6,
        "fixed upstream? the tight box ends at {}: `Solid::bounds` (engine.rs) can take it alone",
        tight.max_at(2)
    );
}

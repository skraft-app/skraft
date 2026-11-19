//! SolidCore's known bugs, reproduced on SolidCore itself — **without** our workarounds. Each
//! test asserts that the bug is **still there**: when one fails after a SolidCore upgrade, the
//! bug is fixed upstream; drop its workaround, then the test.
//!
//! Checked at: tangency-labs/solidcore `793d09e` (2026-11-19), the `rev` pinned
//! in the workspace's Cargo.toml. Update this line with each upgrade.
//!
//! That our results are right in spite of them is checked elsewhere (`engine.rs`'s tests,
//! `part_model.rs`).
//!
//! `cargo test -p skraft-kernel --test solidcore_quirks`

use glam::{DAffine3, DQuat, DVec3};
use solidcore::primitive::api_primitives as prim;
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

/// One face of `Cruise_Assembly.step` (STEP Tools' public sample, ADVANCED_FACE #519524 of
/// `Rear_Deck_Under`), its entities renumbered: a disc 0.04 mm across bounded by one closed
/// circle, whose vertex lies at the circle's parameter 0 to ~1e-12.
const ONE_TURN_DISC: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('disc','',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=ADVANCED_FACE('',(#2),#10,.F.);
#2=FACE_BOUND('',#3,.T.);
#3=EDGE_LOOP('',(#4));
#4=ORIENTED_EDGE('',*,*,#5,.F.);
#5=EDGE_CURVE('',#6,#6,#8,.T.);
#6=VERTEX_POINT('',#7);
#7=CARTESIAN_POINT('',(-24.3767641709491,28.3683202389854,3.1));
#8=CIRCLE('',#9,0.0394153987628501);
#9=AXIS2_PLACEMENT_3D('',#11,#12,#13);
#10=PLANE('',#14);
#11=CARTESIAN_POINT('',(-24.3393669553066,28.3807710189537,3.1));
#12=DIRECTION('',(0.,0.,-1.));
#13=DIRECTION('',(-0.948797089877946,-0.315886185578192,0.));
#14=AXIS2_PLACEMENT_3D('',#11,#12,#15);
#15=DIRECTION('',(-0.315886185578182,0.94879708987795,0.));
#20=OPEN_SHELL('',(#1));
#21=SHELL_BASED_SURFACE_MODEL('',(#20));
ENDSEC;
END-ISO-10303-21;
";

/// **A full-turn circle edge a hair short of its grid's quarter line has a sliver arc in its
/// form, which the kernel cannot write back.** The disc's edge is read on `[4 − 1.4e-12,
/// 8 − 1.4e-12]` (one turn, in quarters of its frame's grid), five quarters; since upstream
/// #18200 (`3b43b05`, `cut_to` in `solidcore/src/eval/form.rs`) such a form's first and last
/// arcs are cut to the edge's ends, and the first is cut to a sliver 1.4e-12 long: knots
/// `s0 ×3, 4.0 ×2`. The record is valid (two distinct values); the kernel takes them as one,
/// and writing its body back to a record fails: "knot multiplicity" (5 > degree + 1).
///
/// No workaround: such a body is left out at import (`Rear_Deck_Under` in `Cruise_Assembly.step`
/// since `85b73de`; read at `f02d13e`).
#[test]
fn a_circle_edge_a_hair_short_of_one_turn_cannot_be_written_back() {
    let ex = solidcore_record::step::p21::parse(ONE_TURN_DISC).unwrap();
    let read = solidcore_step::topology::read_solid(&ex, 21, Default::default()).expect("the disc is read");
    read.brep.validate().expect("the record is valid");
    let [edge] = read.brep.edges.as_slice() else { panic!("one edge") };
    let Some(solidcore_record::brep::Curve::RationalSpline { knots, .. }) = &edge.nurbs else { panic!("a circle's form") };
    let sliver = knots[3] - knots[2];
    let mut k = kernel();
    let mut b = solidcore_convert::brep::to_kernel_body(&mut k, &read.brep, true).expect("into the kernel");
    let back = solidcore_convert::brep::to_record(&mut k, &mut b);
    assert!(
        sliver > 0.0 && sliver < 1e-11 && back.is_err_and(|e| e.to_string().contains("knot multiplicity")),
        "fixed upstream? the disc's edge has no sliver arc or its body is written back: drop its workaround and this test"
    );
}

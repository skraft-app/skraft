//! An imprint's tool (docs/decisions/0055-linked-bodies-through-imprints.md): the source's whole
//! final solid, its walls inside the receiver moved. Each case is checked by what it leaves of the
//! receiver.

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::profile;
use skraft_kernel::{BooleanOp, Clearance, Primitive, Solid};

fn cuboid(origin: [f64; 3], size: [f64; 3]) -> Solid {
    Primitive::Box { origin: DVec3::from_array(origin), size: DVec3::from_array(size) }.build().unwrap()
}

fn clearance(around: f64, deep: f64) -> Clearance {
    Clearance { around, deep }
}

fn close(v: f64, expected: f64) {
    assert!((v - expected).abs() < 1e-3, "{v} against {expected}");
}

/// What the tool, in the source's axes placed by `placed`, takes off the receiver.
fn removed_at(receiver: &Solid, tool: &Solid, placed: &DAffine3) -> f64 {
    let tool = tool.transformed(placed).unwrap();
    receiver.volume().unwrap() - receiver.boolean(&tool, BooleanOp::Difference).unwrap().volume().unwrap()
}

fn removed(receiver: &Solid, tool: &Solid) -> f64 {
    removed_at(receiver, tool, &ID)
}

/// The source against the receiver once cut.
fn left_in_common(receiver: &Solid, tool: &Solid, source: &Solid) -> f64 {
    let cut = receiver.boolean(tool, BooleanOp::Difference).unwrap();
    match cut.boolean(source, BooleanOp::Intersection) {
        Ok(c) => c.volume().unwrap(),
        Err(skraft_kernel::Error::Empty(_)) => 0.0,
        Err(e) => panic!("{e}"),
    }
}

const ID: DAffine3 = DAffine3::IDENTITY;

#[test]
fn a_blind_tenon_has_a_bottom_and_four_cheeks() {
    let block = cuboid([0.0, 0.0, 0.0], [100.0, 100.0, 60.0]);
    // Coming down from the top, 22 deep.
    let tenon = cuboid([30.0, 40.0, 38.0], [40.0, 10.0, 60.0]);
    let tool = block.imprint_tool(&tenon, &ID, &clearance(0.1, 2.0), None).unwrap();
    close(removed(&block, &tool), 40.2 * 10.2 * 24.0);
    assert!(left_in_common(&block, &tool, &tenon) < 1e-6);
    let (lo, hi) = tool.vertex_bounds().unwrap();
    assert!((lo.z - 36.0).abs() < 1e-9 && (hi.z - 98.0).abs() < 1e-9, "the bottom +2, the far end as it is: {lo} {hi}");
}

#[test]
fn without_clearance_the_tool_is_the_source() {
    let block = cuboid([0.0, 0.0, 0.0], [100.0, 100.0, 60.0]);
    let tenon = cuboid([30.0, 40.0, 38.0], [40.0, 10.0, 60.0]);
    assert_eq!(block.imprint_tool(&tenon, &ID, &clearance(0.0, 0.0), None).unwrap(), tenon);
}

#[test]
fn a_part_that_is_its_own_tenon_is_cut_through_a_notch() {
    // The source's notch, across a plank, open on three sides: the case that started this.
    let plank = cuboid([0.0, 0.0, 0.0], [900.0, 275.0, 176.0]);
    let upright = cuboid([300.0, -10.0, 70.0], [124.0, 295.0, 500.0]);
    let tool = plank.imprint_tool(&upright, &ID, &clearance(0.1, 2.0), None).unwrap();
    close(removed(&plank, &tool), 124.2 * 275.0 * (176.0 - 68.0));
    assert!(left_in_common(&plank, &tool, &upright) < 1e-6);
}

#[test]
fn a_rail_flush_with_a_plank_has_a_bottom_past_its_width() {
    let plank = cuboid([224.64, 285.95, 0.0], [1103.38, 357.35, 172.17]);
    let rail = cuboid([753.6, 285.95, 0.0], [186.41, 146.92, 939.35]);
    let tool = plank.imprint_tool(&rail, &ID, &clearance(0.1, 2.0), None).unwrap();
    close(removed(&plank, &tool), 186.61 * 148.92 * 172.17);
    assert!(left_in_common(&plank, &tool, &rail) < 1e-6);
}

#[test]
fn a_shoulder_on_the_receiver_is_not_moved() {
    let block = cuboid([0.0, 0.0, 0.0], [100.0, 100.0, 60.0]);
    let rail = cuboid([20.0, 30.0, 60.0], [60.0, 40.0, 200.0]);
    let tenon = rail.boolean(&cuboid([30.0, 45.0, 38.0], [40.0, 10.0, 22.0]), BooleanOp::Union).unwrap();
    let tool = block.imprint_tool(&tenon, &ID, &clearance(0.1, 2.0), None).unwrap();
    close(removed(&block, &tool), 40.2 * 10.2 * 24.0);
    assert!(left_in_common(&block, &tool, &tenon) < 1e-6);
}

#[test]
fn a_tenon_made_by_a_machining_is_cut_as_it_is() {
    let block = cuboid([0.0, 0.0, 0.0], [100.0, 100.0, 60.0]);
    let blank = cuboid([30.0, 40.0, 38.0], [40.0, 30.0, 60.0]);
    // A rebate through the blank: its section is an L inside the block.
    let rebate = cuboid([50.0, 55.0, 30.0], [30.0, 20.0, 80.0]);
    let tenon = blank.boolean(&rebate, BooleanOp::Difference).unwrap();
    let section = 40.0 * 30.0 - 20.0 * 15.0;
    let tool = block.imprint_tool(&tenon, &ID, &clearance(0.0, 0.0), None).unwrap();
    close(removed(&block, &tool), 22.0 * section);
    let tool = block.imprint_tool(&tenon, &ID, &clearance(0.0, 2.0), None).unwrap();
    close(removed(&block, &tool), 24.0 * section);
    assert!(left_in_common(&block, &tool, &tenon) < 1e-6);
}

#[test]
fn a_filleted_tenon_keeps_its_rounded_cheeks() {
    let block = cuboid([-30.0, -30.0, 0.0], [100.0, 70.0, 60.0]);
    let mut outline = profile::polygon(&[DVec2::ZERO, DVec2::new(40.0, 0.0), DVec2::new(40.0, 10.0), DVec2::new(0.0, 10.0)]);
    profile::settle_ids(&mut outline);
    outline.iter_mut().for_each(|c| c.fillet = 3.0);
    let tenon = Primitive::Prism { origin: DVec3::new(0.0, 0.0, 38.0), profile: outline.clone(), height: 60.0 }.build().unwrap();
    let offset = profile::offset(&profile::rounded(&outline), 0.1).unwrap();
    let area = Primitive::Prism { origin: DVec3::ZERO, profile: offset, height: 1.0 }.build().unwrap().volume().unwrap();
    let tool = block.imprint_tool(&tenon, &ID, &clearance(0.1, 2.0), None).unwrap();
    close(removed(&block, &tool), area * 24.0);
    assert!(left_in_common(&block, &tool, &tenon) < 1e-6);
}

#[test]
fn a_dowel_has_one_round_cheek() {
    let block = cuboid([-30.0, -30.0, 0.0], [60.0, 60.0, 40.0]);
    let dowel = Primitive::Cylinder { origin: DVec3::new(0.0, 0.0, 18.0), radius: 4.0, height: 50.0 }.build().unwrap();
    let tool = block.imprint_tool(&dowel, &ID, &clearance(0.1, 2.0), None).unwrap();
    close(removed(&block, &tool), std::f64::consts::PI * 4.1 * 4.1 * 24.0);
}

#[test]
fn a_slanted_tenon_is_grown_through_its_faces() {
    let block = cuboid([-100.0, -100.0, 0.0], [200.0, 200.0, 60.0]);
    let turn = DAffine3::from_rotation_z(0.5);
    let tenon = cuboid([-20.0, -5.0, 38.0], [40.0, 10.0, 60.0]).transformed(&turn).unwrap();
    let tool = block.imprint_tool(&tenon, &ID, &clearance(0.1, 2.0), None).unwrap();
    close(removed(&block, &tool), 40.2 * 10.2 * 24.0);
    assert!(left_in_common(&block, &tool, &tenon) < 1e-6);
}

#[test]
fn the_tool_comes_in_the_axes_of_its_source() {
    let block = cuboid([0.0, 0.0, 0.0], [100.0, 100.0, 60.0]);
    let tenon = cuboid([0.0, 0.0, 0.0], [40.0, 10.0, 60.0]);
    for placed in [
        DAffine3::from_translation(DVec3::new(30.0, 40.0, 38.0)),
        DAffine3::from_translation(DVec3::new(70.0, 40.0, 38.0)) * DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)),
    ] {
        let tool = block.imprint_tool(&tenon, &placed, &clearance(0.0, 2.0), None).unwrap();
        let (lo, hi) = tool.vertex_bounds().unwrap();
        assert!((lo - DVec3::new(0.0, 0.0, -2.0)).length() < 1e-9 && (hi - DVec3::new(40.0, 10.0, 60.0)).length() < 1e-9, "{lo} {hi}");
        close(removed_at(&block, &tool, &placed), 40.0 * 10.0 * 24.0);
    }
}

#[test]
fn faces_merely_touching_are_no_contact() {
    let a = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    assert!(a.has_contact(&cuboid([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]), &ID).unwrap());
    assert!(!a.has_contact(&cuboid([10.0, 0.0, 0.0], [10.0, 10.0, 10.0]), &ID).unwrap(), "a face");
    assert!(!a.has_contact(&cuboid([10.0, 10.0, 0.0], [10.0, 10.0, 10.0]), &ID).unwrap(), "an edge");
    assert!(!a.has_contact(&cuboid([50.0, 0.0, 0.0], [10.0, 10.0, 10.0]), &ID).unwrap(), "apart");
    // Boxes overlapping, the solids apart: an L around a block.
    let l = cuboid([0.0, 0.0, 0.0], [30.0, 30.0, 10.0])
        .boolean(&cuboid([10.0, 10.0, -1.0], [30.0, 30.0, 12.0]), BooleanOp::Difference)
        .unwrap();
    assert!(!l.has_contact(&cuboid([15.0, 15.0, 0.0], [10.0, 10.0, 10.0]), &ID).unwrap());
}

#[test]
fn without_contact_the_tool_stays_as_it_was() {
    let block = cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let away = cuboid([50.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let previous = cuboid([1.0, 1.0, 1.0], [2.0, 2.0, 2.0]);
    let c = clearance(0.1, 2.0);
    assert_eq!(block.imprint_tool(&away, &ID, &c, Some(&previous)).unwrap(), previous);
    assert!(block.imprint_tool(&away, &ID, &c, None).is_err());
}

#[test]
fn a_clearance_is_valid_when_finite() {
    assert!(clearance(-0.1, 2.0).is_valid());
    assert!(!clearance(f64::NAN, 0.0).is_valid());
    assert!(!clearance(0.0, f64::INFINITY).is_valid());
}

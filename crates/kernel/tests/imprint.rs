//! An imprint's tool (docs/decisions/0055-linked-bodies-through-imprints.md): a body grown by a
//! clearance, deeper at its end, run back past its shoulder, each checked against the volume it
//! should have.

use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::names::{Blend, NodeOp, TreeBody, TreeNode};
use skraft_kernel::profile;
use skraft_kernel::{Clearance, Job, Output, Primitive, Solid, Task, run};

const L: f64 = 40.0;
const W: f64 = 10.0;
const H: f64 = 22.0;

fn clearance(axis: Option<DVec3>, around: f64, deep: f64, back: f64) -> Clearance {
    Clearance { axis, around, deep, back }
}

fn rectangle() -> Vec<profile::Corner> {
    let mut p = profile::polygon(&[DVec2::ZERO, DVec2::new(L, 0.0), DVec2::new(L, W), DVec2::new(0.0, W)]);
    profile::settle_ids(&mut p);
    p
}

fn close(s: &Solid, expected: f64) {
    let v = s.volume().unwrap();
    assert!((v - expected).abs() < 1e-3, "{v} against {expected}");
}

#[test]
fn a_box_stays_a_box_along_any_of_its_axes() {
    let b = Primitive::Box { origin: DVec3::new(1.0, 2.0, 3.0), size: DVec3::new(L, W, H) }.build().unwrap();
    for (axis, along) in [(DVec3::X, L), (-DVec3::X, L), (DVec3::Y, W), (-DVec3::Y, W), (DVec3::Z, H), (-DVec3::Z, H)] {
        let tool = b.imprinted(None, &clearance(Some(axis), 0.1, 2.0, 30.0)).unwrap();
        let k = axis.abs().max_position();
        let size = DVec3::new(L, W, H);
        let across: f64 = (0..3).filter(|i| *i != k).map(|i| size[i] + 0.2).product();
        close(&tool, across * (along + 32.0));
        let (lo, hi) = tool.vertex_bounds().unwrap();
        let (b_lo, b_hi) = b.vertex_bounds().unwrap();
        let (end, shoulder) = if axis[k] > 0.0 { (hi[k] - b_hi[k], b_lo[k] - lo[k]) } else { (b_lo[k] - lo[k], hi[k] - b_hi[k]) };
        assert!((end - 2.0).abs() < 1e-9 && (shoulder - 30.0).abs() < 1e-9, "{axis}: {end} {shoulder}");
    }
}

#[test]
fn no_clearance_is_the_body_itself() {
    let b = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(L, W, H) }.build().unwrap();
    assert_eq!(b.imprinted(None, &clearance(Some(DVec3::X), 0.0, 0.0, 0.0)).unwrap(), b);
    assert_eq!(b.imprinted(None, &clearance(None, 0.0, 2.0, 5.0)).unwrap(), b, "no axis: only around it");
}

#[test]
fn an_extrusion_is_drawn_again_from_its_profile() {
    let mut p = rectangle();
    p.iter_mut().for_each(|c| c.fillet = 3.0);
    let recipe = Primitive::Prism { origin: DVec3::ZERO, profile: p.clone(), height: H };
    let solid = recipe.build().unwrap();
    let offset = profile::offset(&profile::rounded(&p), 0.1).unwrap();
    let area = Primitive::Prism { origin: DVec3::ZERO, profile: offset, height: 1.0 }.build().unwrap().volume().unwrap();
    for axis in [DVec3::Z, -DVec3::Z] {
        let tool = solid.imprinted(Some(&recipe), &clearance(Some(axis), 0.1, 2.0, 30.0)).unwrap();
        close(&tool, area * (H + 32.0));
        let (lo, hi) = tool.bounds().unwrap();
        let (a, b) = if axis.z > 0.0 { (-30.0, H + 2.0) } else { (-2.0, H + 30.0) };
        assert!((lo.z - a).abs() < 1e-6 && (hi.z - b).abs() < 1e-6, "{lo} {hi}");
    }
}

#[test]
fn a_dowel_has_its_faces_offset() {
    let dowel = Primitive::Cylinder { origin: DVec3::ZERO, radius: 4.0, height: H }.build().unwrap();
    let tool = dowel.imprinted(None, &clearance(Some(DVec3::Z), 0.1, 2.0, 30.0)).unwrap();
    close(&tool, std::f64::consts::PI * 4.1 * 4.1 * (H + 32.0));
    let tool = dowel.imprinted(None, &clearance(Some(-DVec3::Z), 0.1, 2.0, 0.0)).unwrap();
    let (lo, hi) = tool.bounds().unwrap();
    assert!((lo.z + 2.0).abs() < 1e-3 && (hi.z - H).abs() < 1e-3, "the end is at the bottom: {lo} {hi}");
}

#[test]
fn a_body_along_a_slanted_axis_goes_through_its_faces() {
    let dowel = Primitive::Cylinder { origin: DVec3::ZERO, radius: 4.0, height: H }.build().unwrap();
    let turn = DAffine3::from_axis_angle(DVec3::new(1.0, 1.0, 0.0).normalize(), 0.7);
    let slanted = dowel.transformed(&turn).unwrap();
    let axis = turn.transform_vector3(DVec3::Z).normalize();
    let tool = slanted.imprinted(None, &clearance(Some(axis), 0.0, 2.0, 5.0)).unwrap();
    close(&tool, std::f64::consts::PI * 16.0 * (H + 7.0));
}

#[test]
fn a_rounded_end_too_tight_to_offset_is_refused_never_reduced() {
    let b = Primitive::Prism { origin: DVec3::ZERO, profile: rectangle(), height: H };
    let solid = Arc::new(b.build().unwrap());
    let picks = vec![[L / 2.0, 0.0, H], [L, W / 2.0, H], [L / 2.0, W, H], [0.0, W / 2.0, H]];
    let leaf = TreeBody {
        id: 1,
        negative: false,
        intersect: false,
        placement: DAffine3::IDENTITY,
        node: TreeNode::Leaf { solid: 0, recipe: Some(b) },
    };
    let op = NodeOp::blend_of(Blend::Round, 2.0, Vec::new());
    let job = Job { inputs: vec![solid], tasks: vec![Task::Node { children: vec![leaf], op, picks }] };
    let Some(Output::Node { solid: rounded, refusal: None, .. }) = run(&job).unwrap().pop() else { panic!() };
    // Only deeper: the round follows its end.
    let v = rounded.volume().unwrap();
    close(&rounded.imprinted(None, &clearance(Some(DVec3::Z), 0.0, 2.0, 0.0)).unwrap(), v + 2.0 * L * W);
    // Around it too: SolidCore refuses (no certified bound).
    assert!(rounded.imprinted(None, &clearance(Some(DVec3::Z), 0.1, 2.0, 0.0)).is_err());
}

#[test]
fn a_clearance_is_valid_when_finite_with_a_unit_axis() {
    assert!(clearance(Some(DVec3::X), -0.1, 2.0, 0.0).is_valid());
    assert!(clearance(None, 0.0, 0.0, 0.0).is_valid());
    assert!(!clearance(Some(DVec3::X * 2.0), 0.0, 0.0, 0.0).is_valid());
    assert!(!clearance(Some(DVec3::X), f64::NAN, 0.0, 0.0).is_valid());
    assert!(!clearance(Some(DVec3::X), 0.0, 0.0, -1.0).is_valid());
}

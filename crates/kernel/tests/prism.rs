//! Prisms of outlines with arcs (docs/tools/prism.md), built by SolidCore.
#![cfg(feature = "engine")]

use glam::{DVec2, DVec3};
use skraft_kernel::Solid;
use skraft_kernel::profile::{Corner, polygon, with_fillets};

#[test]
fn a_prism_with_arcs_is_built() {
    let v = DVec2::new;
    // A square with its top a half circle out, 10 high.
    let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
    let s = Solid::new_prism(DVec3::ZERO, &tomb, 10.0).unwrap();
    // 4 sides, 2 caps; 12 edges; 8 vertices.
    assert_eq!(s.topology_counts(), (6, 12, 8));
    // Clockwise, the same.
    let back = skraft_kernel::profile::reversed(&tomb);
    assert_eq!(Solid::new_prism(DVec3::ZERO, &back, 10.0).unwrap().topology_counts(), (6, 12, 8));

    // A board with rounded corners, its fillets as radii on the sharp corners.
    let board = with_fillets(&polygon(&[v(0.0, 0.0), v(600.0, 0.0), v(600.0, 300.0), v(0.0, 300.0)]), 20.0);
    let s = Solid::new_prism(DVec3::new(0.0, 0.0, 5.0), &board, 18.0).unwrap();
    assert_eq!(s.topology_counts(), (10, 24, 16));
    let (lo, hi) = s.vertex_bounds().unwrap();
    assert!((lo - DVec3::new(0.0, 0.0, 5.0)).length() < 1e-6 && (hi - DVec3::new(600.0, 300.0, 23.0)).length() < 1e-6, "{lo} {hi}");

    // A disc as two half circles, either way round: a cylinder of two faces.
    let disc = [Corner::arc(v(-5.0, 0.0), 1.0), Corner::arc(v(5.0, 0.0), 1.0)];
    assert_eq!(Solid::new_prism(DVec3::ZERO, &disc, 10.0).unwrap().topology_counts(), (4, 6, 4));
    let disc = skraft_kernel::profile::reversed(&disc);
    assert_eq!(Solid::new_prism(DVec3::ZERO, &disc, 10.0).unwrap().topology_counts(), (4, 6, 4));
}

#[test]
fn only_a_box_fills_its_box() {
    let v = DVec2::new;
    let b = Solid::new_box(DVec3::new(5.0, -3.0, 2.0), DVec3::new(600.0, 300.0, 18.0)).unwrap();
    assert!(b.fills_its_box());
    // A square prism is a box too.
    let square = polygon(&[v(0.0, 0.0), v(40.0, 0.0), v(40.0, 20.0), v(0.0, 20.0)]);
    assert!(Solid::new_prism(DVec3::ZERO, &square, 10.0).unwrap().fills_its_box());
    // Not a box: rounded corners, a slanted side (six faces, eight vertices), a cylinder.
    assert!(!Solid::new_prism(DVec3::ZERO, &with_fillets(&square, 2.0), 10.0).unwrap().fills_its_box());
    let slanted = polygon(&[v(0.0, 0.0), v(40.0, 0.0), v(30.0, 20.0), v(0.0, 20.0)]);
    assert!(!Solid::new_prism(DVec3::ZERO, &slanted, 10.0).unwrap().fills_its_box());
    assert!(!Solid::new_cylinder(DVec3::ZERO, 10.0, 30.0).unwrap().fills_its_box());
    // A box turned off its axes no longer fills its axis-aligned box.
    let turned = b.transformed(&glam::DAffine3::from_rotation_z(0.3)).unwrap();
    assert!(!turned.fills_its_box());
}

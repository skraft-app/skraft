//! Prisms of outlines with arcs (docs/ARCHITECTURE.md §6 nonies), built by SolidCore.
#![cfg(feature = "engine")]

use glam::{DVec2, DVec3};
use solidsketch_kernel::Solid;
use solidsketch_kernel::profile::{Corner, fillet, polygon};

#[test]
fn a_prism_with_arcs_is_built() {
    let v = DVec2::new;
    // A square with its top a half circle out, 10 high.
    let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
    let s = Solid::new_prism(DVec3::ZERO, &tomb, 10.0).unwrap();
    // 4 sides, 2 caps; 12 edges; 8 vertices.
    assert_eq!(s.topology_counts(), (6, 12, 8));
    // Clockwise, the same.
    let back = solidsketch_kernel::profile::reversed(&tomb);
    assert_eq!(Solid::new_prism(DVec3::ZERO, &back, 10.0).unwrap().topology_counts(), (6, 12, 8));

    // A board with rounded corners.
    let board = fillet(&polygon(&[v(0.0, 0.0), v(600.0, 0.0), v(600.0, 300.0), v(0.0, 300.0)]), 20.0);
    let s = Solid::new_prism(DVec3::new(0.0, 0.0, 5.0), &board, 18.0).unwrap();
    assert_eq!(s.topology_counts(), (10, 24, 16));
    let (lo, hi) = s.vertex_bounds().unwrap();
    assert!((lo - DVec3::new(0.0, 0.0, 5.0)).length() < 1e-6 && (hi - DVec3::new(600.0, 300.0, 23.0)).length() < 1e-6, "{lo} {hi}");

    // A disc as two half circles, either way round: a cylinder of two faces.
    let disc = [Corner::arc(v(-5.0, 0.0), 1.0), Corner::arc(v(5.0, 0.0), 1.0)];
    assert_eq!(Solid::new_prism(DVec3::ZERO, &disc, 10.0).unwrap().topology_counts(), (4, 6, 4));
    let disc = solidsketch_kernel::profile::reversed(&disc);
    assert_eq!(Solid::new_prism(DVec3::ZERO, &disc, 10.0).unwrap().topology_counts(), (4, 6, 4));
}


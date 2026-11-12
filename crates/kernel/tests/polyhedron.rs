//! `Solid::from_polyhedron`: planar-faced solids, as a SketchUp import gives them.

use glam::DVec3;
use solidsketch_kernel::{PolyFace, Polyhedron, Solid};

/// A box `lo..hi`: 8 vertices, 6 faces, loops given in any turning (the builder orients them).
fn cuboid(lo: DVec3, hi: DVec3, offset: u32) -> (Vec<DVec3>, Vec<PolyFace>) {
    let v = |i: u32| {
        DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z })
    };
    let vertices = (0..8).map(v).collect();
    let face = |normal: DVec3, ids: [u32; 4]| PolyFace { normal, loops: vec![ids.iter().map(|i| i + offset).collect()] };
    let faces = vec![
        face(-DVec3::Z, [0, 1, 3, 2]),
        face(DVec3::Z, [4, 5, 7, 6]),
        face(-DVec3::Y, [0, 1, 5, 4]),
        face(DVec3::Y, [2, 3, 7, 6]),
        face(-DVec3::X, [0, 2, 6, 4]),
        face(DVec3::X, [1, 3, 7, 5]),
    ];
    (vertices, faces)
}

fn volume_of_bounds(s: &Solid) -> DVec3 {
    let (lo, hi) = s.bounds().unwrap();
    hi - lo
}

#[test]
fn a_box_builds_closed() {
    let (vertices, faces) = cuboid(DVec3::ZERO, DVec3::new(100.0, 50.0, 20.0), 0);
    let s = Solid::from_polyhedron(&Polyhedron { vertices, faces }).unwrap();
    assert_eq!(s.topology_counts(), (6, 12, 8));
    assert!((volume_of_bounds(&s) - DVec3::new(100.0, 50.0, 20.0)).length() < 1e-6);
    // It is a working solid: a boolean on it succeeds.
    let tool = Solid::new_box(DVec3::new(10.0, 10.0, -1.0), DVec3::new(10.0, 10.0, 30.0)).unwrap();
    let cut = s.boolean(&tool, solidsketch_kernel::BooleanOp::Difference).unwrap();
    assert!(cut.topology_counts().0 > 6);
}

#[test]
fn inside_out_faces_are_turned() {
    let (vertices, mut faces) = cuboid(DVec3::ZERO, DVec3::splat(10.0), 0);
    for f in &mut faces {
        f.normal = -f.normal;
    }
    let poly = Polyhedron { vertices, faces };
    let (_, volume) = poly.oriented().unwrap();
    assert!((volume - 1000.0).abs() < 1e-6);
    Solid::from_polyhedron(&poly).unwrap();
}

#[test]
fn a_face_with_a_hole() {
    // A 100×100×10 plate with a square through hole 40..60: the top and bottom faces have a
    // hole loop, four inner faces line it.
    let mut vertices: Vec<DVec3> = Vec::new();
    for z in [0.0, 10.0] {
        for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0), (40.0, 40.0), (60.0, 40.0), (60.0, 60.0), (40.0, 60.0)] {
            vertices.push(DVec3::new(x, y, z));
        }
    }
    let quad = |n: DVec3, ids: [u32; 4]| PolyFace { normal: n, loops: vec![ids.to_vec()] };
    let faces = vec![
        PolyFace { normal: -DVec3::Z, loops: vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7]] },
        PolyFace { normal: DVec3::Z, loops: vec![vec![8, 9, 10, 11], vec![12, 13, 14, 15]] },
        quad(-DVec3::Y, [0, 1, 9, 8]),
        quad(DVec3::X, [1, 2, 10, 9]),
        quad(DVec3::Y, [2, 3, 11, 10]),
        quad(-DVec3::X, [3, 0, 8, 11]),
        // The hole's sides face into it.
        quad(DVec3::Y, [4, 5, 13, 12]),
        quad(-DVec3::X, [5, 6, 14, 13]),
        quad(-DVec3::Y, [6, 7, 15, 14]),
        quad(DVec3::X, [7, 4, 12, 15]),
    ];
    let poly = Polyhedron { vertices, faces };
    let (_, volume) = poly.oriented().unwrap();
    assert!((volume - (100.0 * 100.0 - 400.0) * 10.0).abs() < 1e-6);
    let s = Solid::from_polyhedron(&poly).unwrap();
    assert_eq!(s.topology_counts().0, 10);
}

#[test]
fn an_open_box_is_refused() {
    let (vertices, mut faces) = cuboid(DVec3::ZERO, DVec3::splat(10.0), 0);
    faces.remove(1);
    assert!(Solid::from_polyhedron(&Polyhedron { vertices, faces }).is_err());
}

#[test]
fn two_separate_boxes_make_two_lumps() {
    let (mut vertices, mut faces) = cuboid(DVec3::ZERO, DVec3::splat(10.0), 0);
    let (v2, f2) = cuboid(DVec3::new(20.0, 0.0, 0.0), DVec3::new(30.0, 10.0, 10.0), 8);
    vertices.extend(v2);
    faces.extend(f2);
    let s = Solid::from_polyhedron(&Polyhedron { vertices, faces }).unwrap();
    assert_eq!(s.topology_counts().0, 12);
}

#[test]
fn a_reversed_face_is_set_right() {
    let (vertices, mut faces) = cuboid(DVec3::ZERO, DVec3::splat(10.0), 0);
    faces[3].normal = -faces[3].normal;
    let poly = Polyhedron { vertices, faces };
    let (_, volume) = poly.oriented().unwrap();
    assert!((volume - 1000.0).abs() < 1e-6);
    Solid::from_polyhedron(&poly).unwrap();
}

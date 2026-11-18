//! Turned bodies (docs/ARCHITECTURE.md §6 undecies): a half profile turned a whole turn about
//! Z, built by SolidCore's rotational sweep.
#![cfg(feature = "engine")]

use std::f64::consts::PI;

use glam::{DVec2, DVec3};
use solidsketch_kernel::profile::{Corner, polygon, reversed, with_fillets};
use solidsketch_kernel::{Primitive, Solid};

fn v(x: f64, z: f64) -> DVec2 {
    DVec2::new(x, z)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn a_rectangle_touching_the_axis_turns_into_a_cylinder() {
    let r = polygon(&[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 100.0), v(0.0, 100.0)]);
    let s = Solid::new_revolution(DVec3::new(5.0, 6.0, 7.0), &r).unwrap();
    assert!(close(s.volume().unwrap(), PI * 400.0 * 100.0), "{}", s.volume().unwrap());
    let (lo, hi) = s.bounds().unwrap();
    assert!((lo - DVec3::new(-15.0, -14.0, 7.0)).length() < 1e-6 && (hi - DVec3::new(25.0, 26.0, 107.0)).length() < 1e-6, "{lo} {hi}");
    // Meshed for the display, its edges drawn.
    let m = s.mesh().unwrap();
    assert!(!m.indices.is_empty() && !m.edges.is_empty());
    // Clockwise, the same.
    assert!(close(Solid::new_revolution(DVec3::ZERO, &reversed(&r)).unwrap().volume().unwrap(), PI * 400.0 * 100.0));
}

#[test]
fn a_profile_off_the_axis_turns_into_a_ring() {
    let r = polygon(&[v(10.0, 0.0), v(20.0, 0.0), v(20.0, 30.0), v(10.0, 30.0)]);
    let s = Solid::new_revolution(DVec3::ZERO, &r).unwrap();
    assert!(close(s.volume().unwrap(), PI * (400.0 - 100.0) * 30.0), "{}", s.volume().unwrap());
    // A round section: a torus.
    let disc = [Corner::arc(v(15.0, 0.0), 1.0), Corner::arc(v(25.0, 0.0), 1.0)];
    let s = Solid::new_revolution(DVec3::ZERO, &disc).unwrap();
    assert!(close(s.volume().unwrap(), 2.0 * PI * PI * 20.0 * 25.0), "{}", s.volume().unwrap());
}

#[test]
fn arcs_either_way_and_fillets_turn() {
    // A dome: a quarter circle from the axis's foot out and up to the axis (counter-clockwise).
    let dome = [Corner::new(v(0.0, 0.0)), Corner::arc(v(10.0, 0.0), (PI / 8.0).tan()), Corner::new(v(0.0, 10.0))];
    let s = Solid::new_revolution(DVec3::ZERO, &dome).unwrap();
    assert!(close(s.volume().unwrap(), 2.0 / 3.0 * PI * 1000.0), "{}", s.volume().unwrap());
    // The same clockwise: its arc turns the other way.
    assert!(close(Solid::new_revolution(DVec3::ZERO, &reversed(&dome)).unwrap().volume().unwrap(), 2.0 / 3.0 * PI * 1000.0));
    // A leg with rounded outer corners: less than its cylinder, more than the one inside.
    let leg = polygon(&[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 100.0), v(0.0, 100.0)]);
    let mut leg = with_fillets(&leg, 0.0);
    leg[1].fillet = 5.0;
    leg[2].fillet = 5.0;
    let vol = Solid::new_revolution(DVec3::ZERO, &leg).unwrap().volume().unwrap();
    assert!(vol < PI * 400.0 * 100.0 && vol > PI * 225.0 * 100.0, "{vol}");
}

#[test]
fn a_profile_across_the_axis_is_refused() {
    let r = polygon(&[v(-5.0, 0.0), v(20.0, 0.0), v(20.0, 100.0), v(-5.0, 100.0)]);
    assert!(Solid::new_revolution(DVec3::ZERO, &r).is_err());
    // An arc bulging across the axis, its corners on it.
    let r = [Corner::new(v(0.0, 0.0)), Corner::new(v(20.0, 0.0)), Corner::new(v(20.0, 10.0)), Corner::arc(v(0.0, 10.0), 0.5)];
    assert!(Solid::new_revolution(DVec3::ZERO, &r).is_err());
}

#[test]
fn an_arc_between_two_points_of_the_axis_bulging_inwards_turns() {
    // Its corners on the axis, its centre behind it: a spindle torus's outer piece (SolidCore
    // refused it up to `f02d13e`). A cylinder less the turned lens, x(z) = −3.75 + √(6.25² − (z − 5)²).
    let r = [Corner::new(v(0.0, 0.0)), Corner::new(v(20.0, 0.0)), Corner::new(v(20.0, 10.0)), Corner::arc(v(0.0, 10.0), -0.5)];
    let n = 100_000;
    let lens: f64 = (0..n)
        .map(|i| {
            let z = (i as f64 + 0.5) / n as f64 * 10.0;
            let x = -3.75 + (6.25f64.powi(2) - (z - 5.0).powi(2)).sqrt();
            PI * x * x * 10.0 / n as f64
        })
        .sum();
    assert!(close(Solid::new_revolution(DVec3::ZERO, &r).unwrap().volume().unwrap(), PI * 400.0 * 10.0 - lens));
}

#[test]
fn at_its_origin_a_revolution_stands_on_its_lowest_height() {
    let r = polygon(&[v(0.0, 5.0), v(20.0, 5.0), v(20.0, 100.0), v(0.0, 100.0)]);
    let (p, at) = Primitive::Revolution { origin: DVec3::new(1.0, 2.0, 3.0), profile: r }.at_origin();
    assert_eq!(at, DVec3::new(1.0, 2.0, 8.0));
    let Primitive::Revolution { origin, profile } = p else { panic!() };
    assert_eq!(origin, DVec3::ZERO);
    assert_eq!(profile[0].p, v(0.0, 0.0));
}

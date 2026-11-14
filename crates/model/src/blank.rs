//! A blank's profile, kept in the part's recipe (docs/ARCHITECTURE.md §6 nonies, step 3): the
//! outline in the part's XY plane, extruded from `z0` by `height` along its Z. The blank solid
//! is built from it and stays its cache. Its fillets are radii on its sharp corners
//! ([`profile::rounded`] makes them arcs).

use std::f64::consts::FRAC_PI_2;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use solidsketch_kernel::profile::{self, Corner, Side};
use solidsketch_kernel::{Primitive, Stretch};

/// The profile a part's blank is made of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlankProfile {
    pub profile: Vec<Corner>,
    pub z0: f64,
    pub height: f64,
}

/// Below this, a direction is across an axis; above 1 − this, along it.
const SQUARE: f64 = 1e-9;

impl BlankProfile {
    /// The profile of a primitive built at its own origin ([`Primitive::at_origin`]): a box's
    /// rectangle, a cylinder's disc (two half circles), a prism's outline (an arc tangent to two
    /// straight sides recorded as a fillet, [`profile::recognized`]); none for the others.
    pub fn of(primitive: &Primitive) -> Option<BlankProfile> {
        match *primitive {
            Primitive::Box { origin, size } => {
                let (lo, s) = (origin.min(origin + size), size.abs());
                let r = [DVec2::ZERO, DVec2::new(s.x, 0.0), DVec2::new(s.x, s.y), DVec2::new(0.0, s.y)];
                Some(BlankProfile { profile: profile::polygon(&r.map(|p| p + lo.truncate())), z0: lo.z, height: s.z })
            }
            Primitive::Cylinder { origin, radius, height } => {
                let c = origin.truncate();
                let disc = vec![Corner::arc(c + DVec2::X * radius, 1.0), Corner::arc(c - DVec2::X * radius, 1.0)];
                Some(BlankProfile { profile: disc, z0: origin.z, height })
            }
            Primitive::Prism { origin, ref profile, height } => Some(BlankProfile {
                profile: profile::recognized(profile).iter().map(|c| Corner { p: c.p + origin.truncate(), ..*c }).collect(),
                z0: origin.z,
                height,
            }),
            _ => None,
        }
    }

    /// The primitive the blank is built from.
    pub fn primitive(&self) -> Primitive {
        Primitive::Prism { origin: DVec3::new(0.0, 0.0, self.z0), profile: self.profile.clone(), height: self.height }
    }

    /// The outline's shape: its fillets made arcs.
    pub fn outline(&self) -> Vec<Corner> {
        profile::rounded(&self.profile)
    }

    /// Can a blank be built from it: a simple outline, sharp and rounded, a height?
    pub fn is_valid(&self) -> bool {
        self.height.is_finite()
            && self.height > 0.0
            && self.z0.is_finite()
            && profile::is_simple(&profile::cleaned(&self.profile))
            && profile::is_simple(&self.outline())
    }

    /// The profile stretched as the blank is (§6 sexies, in the part's axes), when it stays one:
    /// along Z, its caps follow their sections; along an axis in its plane, each corner moves
    /// with its section (its fillet's radius kept), an arc wholly in one section rigidly — a
    /// fillet's too. None across an arc, along an axis neither in its plane nor along Z, or when
    /// the outline would not stay simple.
    pub fn stretched(&self, s: &Stretch) -> Option<BlankProfile> {
        let a = s.axis();
        if a.z.abs() > 1.0 - SQUARE {
            // The caps' positions along the axis, each moved by its section.
            let (lo, hi) = (self.z0, self.z0 + self.height);
            let (mlo, mhi) = (s.move_at(a.z * lo) * a.z, s.move_at(a.z * hi) * a.z);
            let b = BlankProfile { profile: self.profile.clone(), z0: lo + mlo, height: hi + mhi - lo - mlo };
            return b.is_valid().then_some(b);
        }
        if a.z.abs() > SQUARE {
            return None;
        }
        let u = a.truncate().normalize();
        let along = |p: DVec2| u.dot(p);
        // An arc must lie in one section, whole.
        for side in profile::sides(&self.outline()) {
            if let Side::Arc { c, r, .. } = side {
                let mut pts = vec![side.a(), side.b()];
                // Its extremes along the axis, where they lie on it.
                for k in 0..4 {
                    let q = c + DVec2::from_angle(u.to_angle() + k as f64 * FRAC_PI_2) * r;
                    if arc_holds(&side, q) {
                        pts.push(q);
                    }
                }
                let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(along(*p)), h.max(along(*p))));
                s.rigid_move(lo, hi)?;
            }
        }
        let profile = self.profile.iter().map(|c| Corner { p: c.p + u * s.move_at(along(c.p)), ..*c }).collect();
        let b = BlankProfile { profile, ..self.clone() };
        b.is_valid().then_some(b)
    }
}

/// Is `q` (on its circle) within the arc's sweep?
fn arc_holds(side: &Side, q: DVec2) -> bool {
    let Side::Arc { c, start, sweep, .. } = *side else { return false };
    let along = (((q - c).to_angle() - start) * sweep.signum()).rem_euclid(std::f64::consts::TAU);
    along <= sweep.abs() + 1e-9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn of_the_shapes() {
        let b = BlankProfile::of(&Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 300.0, 18.0) }).unwrap();
        assert_eq!(b.profile.len(), 4);
        assert_eq!((b.z0, b.height), (0.0, 18.0));
        assert!(b.is_valid());
        let c = BlankProfile::of(&Primitive::Cylinder { origin: DVec3::ZERO, radius: 5.0, height: 10.0 }).unwrap();
        assert!(c.is_valid() && profile::signed_area2(&c.profile) > 0.0);
        assert!(BlankProfile::of(&Primitive::Sphere { center: DVec3::ZERO, radius: 1.0 }).is_none());
    }

    #[test]
    fn stretched_along_its_axes() {
        let board = BlankProfile::of(&Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 300.0, 18.0) }).unwrap();
        // Along X, a cutter at 300 moving what lies beyond by 100.
        let s = Stretch::new(DVec3::X, vec![300.0], vec![0.0, 100.0]).unwrap();
        let b = board.stretched(&s).unwrap();
        assert_eq!(profile::bounds(&b.profile).unwrap().1, DVec2::new(700.0, 300.0));
        // Along Z: the top cap up by 2.
        let s = Stretch::new(DVec3::Z, vec![9.0], vec![0.0, 2.0]).unwrap();
        let b = board.stretched(&s).unwrap();
        assert_eq!((b.z0, b.height), (0.0, 20.0));
        // Askew in its plane: each corner slides along it, kept.
        let s = Stretch::new(DVec3::new(1.0, 1.0, 0.0), vec![300.0], vec![0.0, 100.0]).unwrap();
        assert!(board.stretched(&s).is_some());
        // Out of its plane, not along Z: dropped.
        let s = Stretch::new(DVec3::new(1.0, 0.0, 1.0), vec![300.0], vec![0.0, 100.0]).unwrap();
        assert!(board.stretched(&s).is_none());
        // A disc cut across: dropped; moved whole: kept.
        let disc = BlankProfile::of(&Primitive::Cylinder { origin: DVec3::ZERO, radius: 5.0, height: 10.0 }).unwrap();
        assert!(disc.stretched(&Stretch::new(DVec3::X, vec![0.0], vec![0.0, 3.0]).unwrap()).is_none());
        let moved = disc.stretched(&Stretch::new(DVec3::X, vec![-10.0], vec![0.0, 3.0]).unwrap()).unwrap();
        assert!((profile::bounds(&moved.profile).unwrap().0 - DVec2::new(-2.0, -5.0)).length() < 1e-9);
    }

    #[test]
    fn fillets_are_kept_as_radii() {
        let square = profile::polygon(&[DVec2::ZERO, DVec2::new(600.0, 0.0), DVec2::new(600.0, 300.0), DVec2::new(0.0, 300.0)]);
        // A prism made with its fillets as arcs (a face taken whole): recorded as radii.
        let arcs = profile::rounded(&profile::with_fillets(&square, 20.0));
        let b = BlankProfile::of(&Primitive::Prism { origin: DVec3::new(10.0, 0.0, 0.0), profile: arcs, height: 18.0 }).unwrap();
        assert_eq!(b.profile.len(), 4);
        assert!(b.profile.iter().all(|c| (c.fillet - 20.0).abs() < 1e-9), "{:?}", b.profile);
        assert!(b.is_valid());
        let near = |a: DVec2, b: DVec2| a.distance(b) < 1e-9;
        let (lo, hi) = profile::bounds(&b.outline()).unwrap();
        assert!(near(lo, DVec2::new(10.0, 0.0)) && near(hi, DVec2::new(610.0, 300.0)), "{lo} {hi}");
        // Stretched beside its fillets: they follow, radius kept.
        let s = Stretch::new(DVec3::X, vec![300.0], vec![0.0, 100.0]).unwrap();
        let w = b.stretched(&s).unwrap();
        assert!(w.profile.iter().all(|c| (c.fillet - 20.0).abs() < 1e-9));
        assert!(near(profile::bounds(&w.outline()).unwrap().1, DVec2::new(710.0, 300.0)));
        // Across a fillet: dropped (the kernel refuses it too).
        assert!(b.stretched(&Stretch::new(DVec3::X, vec![600.0], vec![0.0, 100.0]).unwrap()).is_none());
    }
}

//! A body's profile, kept in the part's recipe (docs/tools/prism.md, docs/model/bodies.md): the
//! outline in the body's XY plane, extruded from `z0` by `height` along its Z — or a half
//! profile in its XZ plane turned a whole turn about its Z (docs/tools/turned-bodies.md). The body's solid is
//! built from it and stays its cache. Its fillets are radii on its sharp corners
//! ([`profile::rounded`] makes them arcs).

use std::f64::consts::FRAC_PI_2;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use skraft_kernel::profile::{self, Corner, Side};
use skraft_kernel::{Primitive, Stretch};

/// The profile a body is made of: its corners — `(x, y)` in the body's XY plane for an
/// extrusion, `(x, z)` in its XZ plane for a revolution (`x` the radius) — and how it is swept.
/// Its corners always have ids of their own ([`profile::settle_ids`]: made so, and read so).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Stored")]
pub struct BodyProfile {
    pub profile: Vec<Corner>,
    #[serde(flatten)]
    pub sweep: Sweep,
}

/// A [`BodyProfile`] as read, its ids settled once read.
#[derive(Deserialize)]
struct Stored {
    profile: Vec<Corner>,
    #[serde(flatten)]
    sweep: Sweep,
}

impl From<Stored> for BodyProfile {
    fn from(s: Stored) -> BodyProfile {
        BodyProfile { profile: s.profile, sweep: s.sweep }.settled()
    }
}

/// How a body's profile is swept into its solid.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Sweep {
    /// Along the body's Z, from `z0` by `height`.
    Extrusion { z0: f64, height: f64 },
    /// About the body's Z, `revolution` the turn (a whole one).
    Revolution { revolution: Turn },
}

/// How far a revolution turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Turn {
    Whole,
}

/// Below this, a direction is across an axis; above 1 − this, along it.
const SQUARE: f64 = 1e-9;

impl BodyProfile {
    /// The profile of a primitive built at its own origin ([`Primitive::at_origin`]): a box's
    /// rectangle, a cylinder's disc (two half circles), a prism's outline (an arc tangent to two
    /// straight sides recorded as a fillet, [`profile::recognized`]); none for the others.
    pub fn of(primitive: &Primitive) -> Option<BodyProfile> {
        match *primitive {
            Primitive::Box { origin, size } => {
                let (lo, s) = (origin.min(origin + size), size.abs());
                let r = [DVec2::ZERO, DVec2::new(s.x, 0.0), DVec2::new(s.x, s.y), DVec2::new(0.0, s.y)];
                Some(BodyProfile::extruded(profile::polygon(&r.map(|p| p + lo.truncate())), lo.z, s.z))
            }
            Primitive::Cylinder { origin, radius, height } => {
                let c = origin.truncate();
                let disc = vec![Corner::arc(c + DVec2::X * radius, 1.0), Corner::arc(c - DVec2::X * radius, 1.0)];
                Some(BodyProfile::extruded(disc, origin.z, height))
            }
            Primitive::Prism { origin, ref profile, height } => Some(BodyProfile::extruded(
                profile::recognized(profile).iter().map(|c| Corner { p: c.p + origin.truncate(), ..*c }).collect(),
                origin.z,
                height,
            )),
            // Turned about the body's Z: its axis must be the body's.
            Primitive::Revolution { origin, ref profile } if origin.truncate() == DVec2::ZERO => Some(BodyProfile::turned(
                profile::recognized(profile).iter().map(|c| Corner { p: c.p + DVec2::Y * origin.z, ..*c }).collect(),
            )),
            _ => None,
        }
    }

    /// The outline `profile` extruded from `z0` by `height`.
    pub fn extruded(profile: Vec<Corner>, z0: f64, height: f64) -> BodyProfile {
        BodyProfile { profile, sweep: Sweep::Extrusion { z0, height } }.settled()
    }

    /// The half profile `profile` turned a whole turn.
    pub fn turned(profile: Vec<Corner>) -> BodyProfile {
        BodyProfile { profile, sweep: Sweep::Revolution { revolution: Turn::Whole } }.settled()
    }

    /// The same, each corner with an id of its own (one just made, or split from another,
    /// given the next free one): its sides' names (docs/decisions/0042-body-nodes-operators.md).
    pub fn settled(mut self) -> BodyProfile {
        profile::settle_ids(&mut self.profile);
        self
    }

    /// An extrusion's base and height; none for a revolution.
    pub fn extrusion(&self) -> Option<(f64, f64)> {
        match self.sweep {
            Sweep::Extrusion { z0, height } => Some((z0, height)),
            Sweep::Revolution { .. } => None,
        }
    }

    /// Is it a half profile turned about the body's Z?
    pub fn is_turned(&self) -> bool {
        matches!(self.sweep, Sweep::Revolution { .. })
    }

    /// The primitive the body is built from.
    pub fn primitive(&self) -> Primitive {
        match self.sweep {
            Sweep::Extrusion { z0, height } => Primitive::Prism { origin: DVec3::new(0.0, 0.0, z0), profile: self.profile.clone(), height },
            Sweep::Revolution { .. } => Primitive::Revolution { origin: DVec3::ZERO, profile: self.profile.clone() },
        }
    }

    /// The outline's shape: its fillets made arcs.
    pub fn outline(&self) -> Vec<Corner> {
        profile::rounded(&self.profile)
    }

    /// Can a body be built from it: a simple outline, sharp and rounded, a height?
    /// For a revolution: all on the side of positive X, touching the axis or not.
    pub fn is_valid(&self) -> bool {
        let simple = profile::is_simple(&profile::cleaned(&self.profile)) && profile::is_simple(&self.outline());
        match self.sweep {
            Sweep::Extrusion { z0, height } => height.is_finite() && height > 0.0 && z0.is_finite() && simple,
            Sweep::Revolution { .. } => simple && profile::is_turnable(&self.profile),
        }
    }

    /// The profile stretched as its body is (docs/tools/stretch.md, in the body's axes), when it stays one.
    /// An extrusion: along Z, its caps follow their sections; along an axis in its plane, each
    /// corner moves with its section (its fillet's radius kept), an arc wholly in one section
    /// rigidly — a fillet's too. A revolution: along Z (its profile's Y), the same; across its
    /// axis, never (it would not stay round). None across an arc, along an axis neither in its
    /// plane nor along Z, or when the outline would not stay simple.
    pub fn stretched(&self, s: &Stretch) -> Option<BodyProfile> {
        let a = s.axis();
        match self.sweep {
            Sweep::Extrusion { z0, height } if a.z.abs() > 1.0 - SQUARE => {
                // The caps' positions along the axis, each moved by its section.
                let (lo, hi) = (z0, z0 + height);
                let (mlo, mhi) = (s.move_at(a.z * lo) * a.z, s.move_at(a.z * hi) * a.z);
                let b = BodyProfile::extruded(self.profile.clone(), lo + mlo, hi + mhi - lo - mlo);
                b.is_valid().then_some(b)
            }
            Sweep::Extrusion { .. } if a.z.abs() > SQUARE => None,
            Sweep::Extrusion { .. } => self.stretched_in_plane(s, a.truncate().normalize()),
            Sweep::Revolution { .. } if a.z.abs() > 1.0 - SQUARE => self.stretched_in_plane(s, DVec2::Y * a.z.signum()),
            Sweep::Revolution { .. } => None,
        }
    }

    /// The profile stretched along `u`, the stretch's axis in the profile's plane: each corner
    /// moved with its section, an arc wholly in one section rigidly.
    fn stretched_in_plane(&self, s: &Stretch, u: DVec2) -> Option<BodyProfile> {
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
        let b = BodyProfile { profile, ..self.clone() };
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
        let b = BodyProfile::of(&Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 300.0, 18.0) }).unwrap();
        assert_eq!(b.profile.len(), 4);
        assert_eq!(b.extrusion(), Some((0.0, 18.0)));
        assert!(b.is_valid());
        let c = BodyProfile::of(&Primitive::Cylinder { origin: DVec3::ZERO, radius: 5.0, height: 10.0 }).unwrap();
        assert!(c.is_valid() && profile::signed_area2(&c.profile) > 0.0);
        assert!(BodyProfile::of(&Primitive::Sphere { center: DVec3::ZERO, radius: 1.0 }).is_none());
    }

    #[test]
    fn stretched_along_its_axes() {
        let board = BodyProfile::of(&Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 300.0, 18.0) }).unwrap();
        // Along X, a cutter at 300 moving what lies beyond by 100.
        let s = Stretch::new(DVec3::X, vec![300.0], vec![0.0, 100.0]).unwrap();
        let b = board.stretched(&s).unwrap();
        assert_eq!(profile::bounds(&b.profile).unwrap().1, DVec2::new(700.0, 300.0));
        // Along Z: the top cap up by 2.
        let s = Stretch::new(DVec3::Z, vec![9.0], vec![0.0, 2.0]).unwrap();
        let b = board.stretched(&s).unwrap();
        assert_eq!(b.extrusion(), Some((0.0, 20.0)));
        // Askew in its plane: each corner slides along it, kept.
        let s = Stretch::new(DVec3::new(1.0, 1.0, 0.0), vec![300.0], vec![0.0, 100.0]).unwrap();
        assert!(board.stretched(&s).is_some());
        // Out of its plane, not along Z: dropped.
        let s = Stretch::new(DVec3::new(1.0, 0.0, 1.0), vec![300.0], vec![0.0, 100.0]).unwrap();
        assert!(board.stretched(&s).is_none());
        // A disc cut across: dropped; moved whole: kept.
        let disc = BodyProfile::of(&Primitive::Cylinder { origin: DVec3::ZERO, radius: 5.0, height: 10.0 }).unwrap();
        assert!(disc.stretched(&Stretch::new(DVec3::X, vec![0.0], vec![0.0, 3.0]).unwrap()).is_none());
        let moved = disc.stretched(&Stretch::new(DVec3::X, vec![-10.0], vec![0.0, 3.0]).unwrap()).unwrap();
        assert!((profile::bounds(&moved.profile).unwrap().0 - DVec2::new(-2.0, -5.0)).length() < 1e-9);
    }

    #[test]
    fn turned() {
        let v = DVec2::new;
        let half = profile::polygon(&[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 100.0), v(0.0, 100.0)]);
        // Of a revolution about the body's Z only.
        let b = BodyProfile::of(&Primitive::Revolution { origin: DVec3::Z * 5.0, profile: half.clone() }).unwrap();
        assert!(b.is_turned() && b.is_valid());
        assert_eq!(profile::bounds(&b.profile).unwrap().0, v(0.0, 5.0));
        assert!(BodyProfile::of(&Primitive::Revolution { origin: DVec3::X, profile: half.clone() }).is_none());
        assert_eq!(b.primitive(), Primitive::Revolution { origin: DVec3::ZERO, profile: b.profile.clone() });
        // Across its axis: not valid.
        assert!(!BodyProfile::turned(profile::polygon(&[v(-1.0, 0.0), v(20.0, 0.0), v(20.0, 100.0)])).is_valid());
        // Stretched along Z: its heights follow; across: dropped.
        let t = BodyProfile::turned(half);
        let up = t.stretched(&Stretch::new(DVec3::Z, vec![50.0], vec![0.0, 10.0]).unwrap()).unwrap();
        assert_eq!(profile::bounds(&up.profile).unwrap().1, v(20.0, 110.0));
        let down = t.stretched(&Stretch::new(DVec3::NEG_Z, vec![-50.0], vec![0.0, 10.0]).unwrap()).unwrap();
        assert_eq!(profile::bounds(&down.profile).unwrap().0, v(0.0, -10.0));
        assert!(t.stretched(&Stretch::new(DVec3::X, vec![10.0], vec![0.0, 10.0]).unwrap()).is_none());
    }

    #[test]
    fn saved_by_its_sweep() {
        let square = profile::polygon(&[DVec2::ZERO, DVec2::X, DVec2::ONE, DVec2::Y]);
        let e = BodyProfile::extruded(square.clone(), 0.0, 18.0);
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"z0\":0.0") && json.contains("\"height\":18.0"), "{json}");
        assert_eq!(serde_json::from_str::<BodyProfile>(&json).unwrap(), e);
        let t = BodyProfile::turned(square);
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"revolution\":\"whole\"") && !json.contains("z0"), "{json}");
        assert_eq!(serde_json::from_str::<BodyProfile>(&json).unwrap(), t);
    }

    #[test]
    fn fillets_are_kept_as_radii() {
        let square = profile::polygon(&[DVec2::ZERO, DVec2::new(600.0, 0.0), DVec2::new(600.0, 300.0), DVec2::new(0.0, 300.0)]);
        // A prism made with its fillets as arcs (a face taken whole): recorded as radii.
        let arcs = profile::rounded(&profile::with_fillets(&square, 20.0));
        let b = BodyProfile::of(&Primitive::Prism { origin: DVec3::new(10.0, 0.0, 0.0), profile: arcs, height: 18.0 }).unwrap();
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

//! Plane welding (docs/ARCHITECTURE.md §2.11): after an operation, a planar face of a body it
//! changed that lies a hair off a planar face of a body it left alone — closer than [`WELD`],
//! parallel — is put on it exactly, its body stretched by that hair (its profile kept). Faces a
//! few 1e-5 mm apart come from values read on the `f32` display mesh or from sums of rounded
//! moves; the booleans would keep what lies between them: a skin, a sliver, two lumps.
//!
//! The rules keep it from drifting: a face goes onto a face that is there (its exact value,
//! never a mean between them); only the bodies the operation changed move, the others are the
//! references and never move; a face goes onto the nearest reference within [`WELD`], once
//! (no chains: a face welded is no reference for another); never onto a face of its own body.
//! After each operation, two parallel faces of bodies of a part are on one plane or [`WELD`]
//! apart at least, between bodies the operation left alone and those it changed.
//!
//! Only extruded bodies (their profile, §6 nonies) move: their caps along their axis, their
//! straight sides across it. Any body with planar faces is a reference.

use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_kernel::Stretch;
use solidsketch_kernel::profile::{self, Side};

use crate::document::{Body, BodyDef, ComponentKind, Document, Id, Part};
use crate::ops::Solver;
use crate::{BodyProfile, Result};

/// Faces closer than this (mm) are welded: far above the `f32` mesh's errors (some 1e-5 mm at
/// a metre), far below what is shown (1 µm).
pub const WELD: f64 = 1e-4;
/// Faces closer than this (mm) are one already.
const SAME: f64 = 1e-9;
/// How parallel (sine of the angle) two faces must be.
const PARALLEL: f64 = 1e-9;

/// A face put on another by [`Document::weld`]: in the part `part`, a face of the body `body`
/// moved by `distance` (mm, signed along the face's normal as the body's axes give it: +Z for a
/// cap) onto a face of the body `reference`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weld {
    pub part: Id,
    pub body: Id,
    pub reference: Id,
    pub distance: f64,
}

/// A planar face of an extruded body that can be welded.
#[derive(Clone, Copy, Debug)]
enum Face {
    /// The cap at `z0` (`top`: at `z0 + height`), across Z.
    Cap { top: bool },
    /// The straight side from the corner `index` to the next.
    Side { index: usize },
}

/// A plane of the part: `normal · p = offset`.
#[derive(Clone, Copy, Debug)]
struct PartPlane {
    normal: DVec3,
    offset: f64,
}

impl Document {
    /// Welds the faces of the bodies the last operation changed (from `before`, the document
    /// it was applied to) onto those of the bodies it left alone, part by part, and computes
    /// again the parts it welded. Every kernel call goes through `solver`, as the operation's.
    pub(crate) fn weld(&mut self, before: &Document, solver: &mut dyn Solver) -> Result<Vec<Weld>> {
        let mut welds = Vec::new();
        let parts: Vec<Id> = self
            .components
            .iter()
            .filter(|(id, c)| matches!(c.kind, ComponentKind::Part(_)) && before.components.get(id).is_none_or(|b| !Arc::ptr_eq(b, c)))
            .map(|(id, _)| *id)
            .collect();
        for id in parts {
            let Some(old) = before.components.get(&id).and_then(|c| match &c.kind {
                ComponentKind::Part(p) => Some(p),
                _ => None,
            }) else {
                continue;
            };
            let done = self.weld_part(id, old, solver)?;
            if !done.is_empty() {
                self.recompute(id, None, solver)?;
                welds.extend(done);
            }
        }
        Ok(welds)
    }

    /// [`Document::weld`] for the part `id`, `old` as it was.
    fn weld_part(&mut self, id: Id, old: &Part, solver: &mut dyn Solver) -> Result<Vec<Weld>> {
        let p = self.part(id)?.clone();
        if p.bodies.len() < 2 {
            return Ok(Vec::new());
        }
        let changed = |b: &Body| match old.body(b.id) {
            None => true,
            Some(o) => {
                let (d, od) = (p.def(b.def), old.def(o.def));
                o.placement != b.placement || d.zip(od).is_none_or(|(d, od)| !Arc::ptr_eq(&d.solid, &od.solid) || d.profile != od.profile)
            }
        };
        let (moved, kept): (Vec<&Body>, Vec<&Body>) = p.bodies.iter().partition(|b| changed(b));
        if moved.is_empty() || kept.is_empty() {
            return Ok(Vec::new());
        }
        // The references: every planar face of the bodies left alone, in the part's axes.
        let references: Vec<(PartPlane, Id)> = kept.iter().flat_map(|b| planes_of(&p, b).into_iter().map(|q| (q, b.id))).collect();

        let mut welds = Vec::new();
        let mut defs = p.defs.clone();
        let mut bodies = p.bodies.clone();
        for b in moved {
            let Some(def) = p.def(b.def) else { continue };
            let Some(mut profile) = def.profile.clone().filter(|f| f.extrusion().is_some()) else { continue };
            let mut solid = def.solid.clone();
            let mut done = Vec::new();
            for face in faces(&profile) {
                // Each face once, from the profile as it is now (an earlier weld of the body
                // may have moved it by a hair: the stretch is then that hair shorter).
                let Some((plane, axis, at)) = face_plane(&profile, face, &b.placement) else { continue };
                let Some((distance, reference)) = nearest(&plane, &references) else { continue };
                let Some(stretch) = face_stretch(&profile, face, axis, at, distance) else { continue };
                let Some(next) = profile.stretched(&stretch) else { continue };
                solid = solver.stretch(&solid, &stretch)?;
                profile = next;
                done.push(Weld { part: id, body: b.id, reference, distance });
            }
            if done.is_empty() {
                continue;
            }
            // Its own definition: one it shares keeps its shape for the others.
            let shared = p.bodies.iter().any(|o| o.id != b.id && o.def == b.def);
            if shared {
                let copy = BodyDef { id: self.fresh_id(), solid, profile: Some(profile), ..def.clone() };
                if let Some(body) = bodies.iter_mut().find(|x| x.id == b.id) {
                    body.def = copy.id;
                }
                defs.push(copy);
            } else if let Some(d) = defs.iter_mut().find(|d| d.id == b.def) {
                (d.solid, d.profile) = (solid, Some(profile));
            }
            welds.extend(done);
        }
        if !welds.is_empty() {
            let p = self.part_mut(id)?;
            (p.defs, p.bodies) = (defs, bodies);
            p.drop_unused_defs();
        }
        Ok(welds)
    }
}

/// The planar faces of an extruded profile that can be welded: its caps, its straight sides.
fn faces(profile: &BodyProfile) -> Vec<Face> {
    let sides = profile::sides(&profile.profile);
    let straight = sides
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, Side::Line { a, b } if a.distance(*b) > SAME))
        .map(|(index, _)| Face::Side { index });
    [Face::Cap { top: false }, Face::Cap { top: true }].into_iter().chain(straight).collect()
}

/// The plane of `face` in the part's axes (its body placed by `t`), its normal in the body's
/// axes and where it lies along it.
fn face_plane(profile: &BodyProfile, face: Face, t: &DAffine3) -> Option<(PartPlane, DVec3, f64)> {
    let (z0, height) = profile.extrusion()?;
    let (axis, point) = match face {
        Face::Cap { top } => {
            let z = if top { z0 + height } else { z0 };
            (DVec3::Z, DVec3::new(0.0, 0.0, z))
        }
        Face::Side { index } => {
            let n = profile.profile.len();
            let (a, b) = (profile.profile[index].p, profile.profile[(index + 1) % n].p);
            let u = (b - a).perp().try_normalize()?;
            (DVec3::new(u.x, u.y, 0.0), a.extend(z0))
        }
    };
    let normal = t.matrix3.mul_vec3(axis).try_normalize()?;
    let plane = PartPlane { normal, offset: normal.dot(t.transform_point3(point)) };
    Some((plane, axis, axis.dot(point)))
}

/// The planes of the faces of `b`: from its profile when extruded (exact, cheap), else from its
/// solid's record.
fn planes_of(p: &Part, b: &Body) -> Vec<PartPlane> {
    let t = &b.placement;
    if let Some(profile) = p.def(b.def).and_then(|d| d.profile.as_ref()).filter(|f| f.extrusion().is_some()) {
        return faces(profile).into_iter().filter_map(|f| face_plane(profile, f, t).map(|(q, ..)| q)).collect();
    }
    p.solid_of(b)
        .planes()
        .filter_map(|q| {
            let normal = t.matrix3.mul_vec3(q.normal).try_normalize()?;
            Some(PartPlane { normal, offset: normal.dot(t.transform_point3(q.point)) })
        })
        .collect()
}

/// The move (along `plane`'s normal) onto the nearest reference parallel to it within
/// [`WELD`], and that reference's body; none when it is on one already.
fn nearest(plane: &PartPlane, references: &[(PartPlane, Id)]) -> Option<(f64, Id)> {
    let mut best: Option<(f64, Id)> = None;
    for (r, body) in references {
        if plane.normal.cross(r.normal).length() > PARALLEL {
            continue;
        }
        let offset = if plane.normal.dot(r.normal) > 0.0 { r.offset } else { -r.offset };
        let d = offset - plane.offset;
        if d.abs() < SAME {
            return None;
        }
        if d.abs() < WELD && best.is_none_or(|(b, _)| d.abs() < b.abs()) {
            best = Some((d, *body));
        }
    }
    best
}

/// The stretch, in the body's axes, moving `face` (at `at` along `axis`) by `distance` and
/// nothing else: cutters halfway to what lies on either side of it along the axis.
fn face_stretch(profile: &BodyProfile, face: Face, axis: DVec3, at: f64, distance: f64) -> Option<Stretch> {
    let (z0, height) = profile.extrusion()?;
    let others: Vec<f64> = match face {
        Face::Cap { top } => vec![if top { z0 } else { z0 + height }],
        Face::Side { .. } => {
            let u = DVec2::new(axis.x, axis.y);
            profile.profile.iter().map(|c| u.dot(c.p)).filter(|x| (x - at).abs() > SAME).collect()
        }
    };
    let below = others.iter().copied().filter(|x| *x < at).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))));
    let above = others.iter().copied().filter(|x| *x > at).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))));
    let (mut cuts, mut moves) = (Vec::new(), Vec::new());
    if let Some(lo) = below {
        cuts.push((lo + at) / 2.0);
        moves.push(0.0);
    }
    moves.push(distance);
    if let Some(hi) = above {
        cuts.push((at + hi) / 2.0);
        moves.push(0.0);
    }
    if cuts.is_empty() {
        return None;
    }
    Stretch::new(axis, cuts, moves).ok()
}

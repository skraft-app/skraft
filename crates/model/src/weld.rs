//! Plane welding (docs/model/bodies.md): after an operation, a planar face of a body it
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
//! Only extruded bodies (their profile, docs/tools/prism.md) move: their caps along their axis, their
//! straight sides across it. Any body with planar faces is a reference. Through groups of
//! bodies (docs/model/bodies.md#body-groups), the leaves are welded, placed in the part's axes, onto
//! leaves anywhere in the part; a leaf in a group shown by copies never moves (its faces lie
//! elsewhere in each), a reference still.

use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::Stretch;
use skraft_kernel::profile::{self, Side};

use crate::document::{Body, BodyDef, BodySource, ComponentKind, Document, Id, Part};
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

    /// [`Document::weld`] for the part `id`, `old` as it was: through its groups of bodies
    /// (docs/model/bodies.md#body-groups), on its leaves placed in its axes.
    fn weld_part(&mut self, id: Id, old: &Part, solver: &mut dyn Solver) -> Result<Vec<Weld>> {
        let p = self.part(id)?.clone();
        let now = leaves(&p);
        if now.len() < 2 {
            return Ok(Vec::new());
        }
        let before = leaves(old);
        let changed = |l: &Leaf| match before.iter().find(|o| o.path == l.path) {
            None => true,
            Some(o) => {
                let (d, od) = (p.def(l.body.def), old.def(o.body.def));
                o.body.placement != l.body.placement
                    || d.zip(od).is_none_or(|(d, od)| !Arc::ptr_eq(&d.solid, &od.solid) || d.profile != od.profile)
            }
        };
        let (moved, kept): (Vec<&Leaf>, Vec<&Leaf>) = now.iter().partition(|l| changed(l));
        if moved.is_empty() || kept.is_empty() {
            return Ok(Vec::new());
        }
        // The references: every planar face of the leaves left alone, in the part's axes.
        let references: Vec<(PartPlane, Id)> =
            kept.iter().flat_map(|l| planes_of(&p, &l.body).into_iter().map(|q| (q, l.body.id))).collect();

        let mut welds = Vec::new();
        let mut next = p.clone();
        // The groups whose children changed, by depth: computed again, the deepest first.
        let mut groups: Vec<(usize, Id)> = Vec::new();
        for l in moved {
            // In a group shown by copies, a face lies elsewhere in each: never moved.
            if l.in_shared_group {
                continue;
            }
            let Some(def) = p.def(l.body.def) else { continue };
            // A linked body's place is its link's (docs/decisions/0055-linked-bodies-through-imprints.md).
            if def.source == BodySource::Linked {
                continue;
            }
            let Some(mut profile) = def.profile.clone().filter(|f| f.extrusion().is_some()) else { continue };
            let mut solid = def.solid.clone();
            let mut done = Vec::new();
            for face in faces(&profile) {
                // Each face once, from the profile as it is now (an earlier weld of the body
                // may have moved it by a hair: the stretch is then that hair shorter).
                let Some((plane, axis, at)) = face_plane(&profile, face, &l.body.placement) else { continue };
                let Some((distance, reference)) = nearest(&plane, &references) else { continue };
                let Some(stretch) = face_stretch(&profile, face, axis, at, distance) else { continue };
                let Some(stretched) = profile.stretched(&stretch) else { continue };
                solid = solver.stretch(&solid, &stretch)?;
                profile = stretched;
                done.push(Weld { part: id, body: l.body.id, reference, distance });
            }
            if done.is_empty() {
                continue;
            }
            // Its own definition: one it shares keeps its shape for the others.
            if p.copies_of(l.body.def) > 1 {
                let copy = BodyDef { id: self.fresh_id(), solid, profile: Some(profile), ..def.clone() };
                let new = copy.id;
                next.defs.push(copy);
                if let Some(b) = body_at_mut(&mut next, &l.path) {
                    b.def = new;
                }
            } else if let Some(d) = next.defs.iter_mut().find(|d| d.id == l.body.def) {
                (d.solid, d.profile) = (solid, Some(profile));
            }
            if let Ok((chain, _)) = p.group_chain(&l.path[..l.path.len() - 1]) {
                for (depth, g) in chain.into_iter().enumerate() {
                    if !groups.contains(&(depth, g)) {
                        groups.push((depth, g));
                    }
                }
            }
            welds.extend(done);
        }
        if welds.is_empty() {
            return Ok(Vec::new());
        }
        groups.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
        for (_, g) in groups {
            let Some(group) = next.def(g).and_then(|d| d.group.clone()) else { continue };
            let (solid, group, inner) = Self::computed_group(&next.defs, group, &[], solver)?;
            crate::ops::note_refused(&mut next.defs, inner);
            if let Some(d) = next.defs.iter_mut().find(|d| d.id == g) {
                d.solid = solid;
                d.group = Some(group);
            }
        }
        next.drop_unused_defs();
        *self.part_mut(id)? = next;
        Ok(welds)
    }
}

/// A body of a part that is no group, reached through its groups (docs/model/bodies.md#body-groups).
struct Leaf {
    /// The bodies leading to it, its own last.
    path: Vec<Id>,
    /// The body, placed in the part's axes.
    body: Body,
    /// Does a group on its way show in several copies?
    in_shared_group: bool,
}

/// The leaves of `p`, each group's once per copy of it.
fn leaves(p: &Part) -> Vec<Leaf> {
    fn walk(p: &Part, bodies: &[Body], at: DAffine3, path: &mut Vec<Id>, shared: bool, out: &mut Vec<Leaf>) {
        if path.len() > crate::MAX_DEPTH {
            return;
        }
        for b in bodies {
            let placement = at * b.placement;
            path.push(b.id);
            match p.def(b.def) {
                Some(d) if d.is_group() => {
                    let shared = shared || p.copies_of(b.def) > 1;
                    walk(p, d.children(), placement, path, shared, out);
                }
                _ => out.push(Leaf { path: path.clone(), body: Body { placement, ..b.clone() }, in_shared_group: shared }),
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(p, &p.bodies, DAffine3::IDENTITY, &mut Vec::new(), false, &mut out);
    out
}

/// The body `path` leads to in `p` (the part's own first, through its groups).
fn body_at_mut<'p>(p: &'p mut Part, path: &[Id]) -> Option<&'p mut Body> {
    let (last, above) = path.split_last()?;
    if above.is_empty() {
        return p.bodies.iter_mut().find(|b| b.id == *last);
    }
    let (chain, _) = p.group_chain(above).ok()?;
    let g = *chain.last()?;
    p.defs.iter_mut().find(|d| d.id == g)?.group.as_mut()?.children.iter_mut().find(|b| b.id == *last)
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

//! Linked bodies (docs/decisions/0055-linked-bodies-through-imprints.md): a part's imprint, a body
//! with no effect on its own part, makes a linked body in another part. The link between them
//! is held by an assembly holding both occurrences, by paths relative to it: a copy of that
//! assembly keeps its links.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use skraft_kernel::{Solid, Stretch};

use crate::document::{Body, BodyDef, BodySource, ComponentKind, Document, Id, Part, Sign};
use crate::ops::{Outcome, Solver};
use crate::{Error, FastMap, Refusal, Result};

/// How far (each term of the transform, mm for its translation) an imprint may lie from its
/// seat and still sit in it.
const SEATED: f64 = 1e-6;

/// An imprint of one occurrence seated in a linked body of another. Several links may share one
/// body: the rails entering the legs of one definition, each by a link of its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub id: Id,
    /// The occurrences leading from the assembly holding the link to the source part.
    pub source: Vec<Id>,
    /// The imprint: a body at the root of the source part.
    pub imprint: Id,
    /// The occurrences leading from that assembly to the receiving part.
    pub receiver: Vec<Id>,
    /// The linked body: a body at the root of the receiving part.
    pub body: Id,
    /// The imprint's pose in the receiver's axes when it sits in `body`. Per link: read back
    /// from a body shared by mirrored receivers, it would give a mirrored source.
    pub seat: DAffine3,
}

/// Where a link stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// The imprint lies on its seat.
    Seated,
    /// Everything is there, the imprint elsewhere.
    Off,
    /// A path, the imprint or the linked body is gone.
    Broken,
}

/// What the linked bodies did after an operation: each by its receiving part and its id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Followed {
    /// Moved with the sources seated in them, in every copy of their part.
    pub moved: Vec<(Id, Id)>,
    /// Asked different moves by their sources: left where they are.
    pub conflicts: Vec<(Id, Id)>,
}

impl Document {
    /// The part `path` leads to from the assembly `assembly`, and its axes in that assembly's.
    fn through(&self, assembly: Id, path: &[Id]) -> Result<(Id, DAffine3)> {
        if path.is_empty() {
            return Err(Error::Invalid(Refusal::BadLink));
        }
        let (mut at, mut t) = (assembly, DAffine3::IDENTITY);
        for &i in path {
            let o = self.assembly(at)?.occurrence(i).ok_or(Error::NotFound(i))?;
            t *= o.transform;
            at = o.component;
        }
        self.part(at)?;
        Ok((at, t))
    }

    /// The source part, the receiving part and the imprint's pose in the receiver's axes.
    fn imprint_pose(&self, assembly: Id, source: &[Id], imprint: Id, receiver: &[Id]) -> Result<(Id, Id, DAffine3)> {
        let (from, from_t) = self.through(assembly, source)?;
        let (to, to_t) = self.through(assembly, receiver)?;
        let b = self.part(from)?.body(imprint).filter(|b| b.sign == Sign::Imprint).ok_or(Error::Invalid(Refusal::BadLink))?;
        Ok((from, to, to_t.inverse() * from_t * b.placement))
    }

    /// Where `link`, held by the assembly `assembly`, stands.
    pub fn link_state(&self, assembly: Id, link: &Link) -> LinkState {
        let Ok((_, to, pose)) = self.imprint_pose(assembly, &link.source, link.imprint, &link.receiver) else {
            return LinkState::Broken;
        };
        let (Ok(q), Ok((from, _))) = (self.part(to), self.through(assembly, &link.source)) else { return LinkState::Broken };
        let Some(b) = q.body(link.body).filter(|b| q.def_of(b).source == BodySource::Linked) else { return LinkState::Broken };
        let Some(imprint) = self.part(from).ok().and_then(|p| p.body(link.imprint).map(|i| p.solid_of(i))) else {
            return LinkState::Broken;
        };
        // On its seat, and the body still its shape, there (it may have been moved or the
        // imprint changed: its shape does not follow yet).
        if pose.abs_diff_eq(link.seat, SEATED) && same_place(imprint, &pose, q.solid_of(b), &b.placement) {
            LinkState::Seated
        } else {
            LinkState::Off
        }
    }

    /// The imprint of `link` in the axes of the assembly holding it.
    fn imprint_place(&self, assembly: Id, link: &Link) -> Option<DAffine3> {
        let (from, t) = self.through(assembly, &link.source).ok()?;
        Some(t * self.part(from).ok()?.body(link.imprint)?.placement)
    }

    /// Does the imprint of `link`, at `pose` in the receiver's axes, still reach into the
    /// receiver's box? Read from the record, no kernel: conservative on curved faces.
    fn imprint_enters(&self, assembly: Id, link: &Link, pose: &DAffine3) -> bool {
        let Ok((from, to, _)) = self.imprint_pose(assembly, &link.source, link.imprint, &link.receiver) else { return false };
        let Some((lo, hi)) = self.component_bounds(to) else { return false };
        let Some(solid) = self.part(from).ok().and_then(|p| p.body(link.imprint).map(|b| p.solid_of(b))) else { return false };
        (0..3).all(|k| {
            let Ok(s) = Stretch::new(DVec3::AXES[k], Vec::new(), vec![0.0]) else { return false };
            let shift = s.axis().dot(pose.translation);
            s.in_frame(pose).extent(solid).is_some_and(|(a, b)| a + shift < hi[k] && b + shift > lo[k])
        })
    }

    /// The linked bodies after an operation (`before` the document it was applied to): each
    /// goes with the sources that were seated in it and moved, in every copy of its part, its
    /// links' seats with it. A receiver moved drives nothing; an imprint leaving its receiver
    /// leaves the body; different moves asked of one body leave it where it is. Decided on
    /// transforms and on `before`'s solids alone: the same when the job is recorded and replayed.
    pub(crate) fn follow_links(&mut self, before: &Document, solver: &mut dyn Solver) -> Result<Followed> {
        let mut asks: BTreeMap<(Id, Id), Vec<DAffine3>> = BTreeMap::new();
        for (id, c) in &self.components {
            let ComponentKind::Assembly(a) = &c.kind else { continue };
            if a.links().is_empty() {
                continue;
            }
            let Ok(old) = before.assembly(*id) else { continue };
            let old: FastMap<Id, &Link> = old.links().iter().map(|l| (l.id, l)).collect();
            for l in a.links() {
                let Some(was) = old.get(&l.id) else { continue };
                let Ok((_, to, pose)) = self.imprint_pose(*id, &l.source, l.imprint, &l.receiver) else { continue };
                if pose.abs_diff_eq(was.seat, SEATED) || before.link_state(*id, was) != LinkState::Seated {
                    continue;
                }
                let (Some(a0), Some(a1)) = (before.imprint_place(*id, was), self.imprint_place(*id, l)) else { continue };
                let there = self.part(to).is_ok_and(|q| q.body(l.body).is_some());
                if a0.abs_diff_eq(a1, SEATED) || !there || !before.imprint_enters(*id, was, &pose) {
                    continue;
                }
                asks.entry((to, l.body)).or_default().push(pose * was.seat.inverse());
            }
        }
        let mut followed = Followed::default();
        let mut changed = BTreeSet::new();
        for ((part, body), moves) in asks {
            if !moves.iter().all(|d| d.abs_diff_eq(moves[0], SEATED)) {
                followed.conflicts.push((part, body));
                continue;
            }
            if let Some(b) = self.part_mut(part)?.bodies.iter_mut().find(|b| b.id == body) {
                b.placement = moves[0] * b.placement;
            }
            self.map_seats(part, Some(body), &moves[0]);
            changed.insert(part);
            followed.moved.push((part, body));
        }
        for part in changed {
            self.recompute(part, None, solver)?;
        }
        Ok(followed)
    }

    /// The seats of the links into the part `part` (of its body `body` alone, if given), each
    /// changed by `map`, in that part's axes.
    pub(crate) fn map_seats(&mut self, part: Id, body: Option<Id>, map: &DAffine3) {
        for id in self.link_holders() {
            let Ok(a) = self.assembly(id) else { continue };
            let reached: Vec<usize> = (0..a.links().len())
                .filter(|&k| {
                    let l = &a.links()[k];
                    body.is_none_or(|b| b == l.body) && self.through(id, &l.receiver).is_ok_and(|(to, _)| to == part)
                })
                .collect();
            if reached.is_empty() {
                continue;
            }
            if let Ok(a) = self.assembly_mut(id) {
                for k in reached {
                    a.links_mut()[k].seat = *map * a.links()[k].seat;
                }
            }
        }
    }

    /// The assemblies holding links.
    fn link_holders(&self) -> Vec<Id> {
        self.components
            .iter()
            .filter(|(_, c)| matches!(&c.kind, ComponentKind::Assembly(a) if !a.links().is_empty()))
            .map(|(id, _)| *id)
            .collect()
    }

    /// [`crate::Operation::AddLink`].
    pub(crate) fn add_link(
        &mut self,
        (context, source, imprint, receiver): (Id, &[Id], Id, &[Id]),
        sign: Sign,
        onto: Option<Id>,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        if sign == Sign::Imprint || source == receiver {
            return Err(Error::Invalid(Refusal::BadLink));
        }
        let (from, to, seat) = self.imprint_pose(context, source, imprint, receiver)?;
        let p = self.part(from)?;
        let solid = p.body(imprint).map(|b| p.solid_of(b).clone()).ok_or(Error::NotFound(imprint))?;
        let body = match onto {
            Some(id) => {
                let q = self.part(to)?;
                let there = q.body(id).filter(|b| b.sign == sign && q.def_of(b).source == BodySource::Linked);
                if !there.is_some_and(|b| same_place(&solid, &seat, q.solid_of(b), &b.placement)) {
                    return Err(Error::Invalid(Refusal::BadLink));
                }
                id
            }
            None => {
                let (def, id) = (self.fresh_id(), self.fresh_id());
                let q = self.part_mut(to)?;
                q.defs.push(BodyDef { source: BodySource::Linked, ..BodyDef::new(def, solid) });
                q.bodies.push(Body::new(id, def, sign, seat));
                self.recompute(to, Some(&[id]), solver)?;
                id
            }
        };
        let link = Link { id: self.fresh_id(), source: source.to_vec(), imprint, receiver: receiver.to_vec(), body, seat };
        self.assembly_mut(context)?.links_mut().push(link);
        Ok(Outcome { bodies: vec![body], ..Outcome::default() })
    }

    /// Refuses an imprint holding a linked body, in the parts changed since `before`: linked
    /// bodies depend on imprints only, so one pass updates them, with no cycle.
    pub(crate) fn check_imprints(&self, before: &Document) -> Result<()> {
        for (id, c) in &self.components {
            let ComponentKind::Part(p) = &c.kind else { continue };
            if before.components.get(id).is_some_and(|b| std::sync::Arc::ptr_eq(b, c))
                || !p.defs.iter().any(|d| d.source == BodySource::Linked)
            {
                continue;
            }
            if p.all_bodies().any(|b| b.sign == Sign::Imprint && holds_linked(p, b.def, 0)) {
                return Err(Error::Invalid(Refusal::ImprintLinked));
            }
        }
        Ok(())
    }
}

fn holds_linked(p: &Part, def: Id, depth: usize) -> bool {
    let Some(d) = p.def(def).filter(|_| depth <= crate::MAX_DEPTH) else { return false };
    d.source == BodySource::Linked || d.children().iter().any(|c| holds_linked(p, c.def, depth + 1))
}

/// Do the solids `a` and `b`, placed, lie in the same place: their vertices the same points,
/// whatever the placements (a mirrored receiver sees a symmetric body through a mirror)? A solid
/// without vertices (a sphere, a torus) only by the same placement.
fn same_place(a: &Solid, ta: &DAffine3, b: &Solid, tb: &DAffine3) -> bool {
    let points = |s: &Solid, t: &DAffine3| -> Vec<DVec3> {
        s.record().vertices.iter().map(|v| t.transform_point3(DVec3::from_array(v.point))).collect()
    };
    let (pa, mut pb) = (points(a, ta), points(b, tb));
    if pa.len() != pb.len() || a.record().faces.len() != b.record().faces.len() {
        return false;
    }
    if pa.is_empty() {
        return ta.abs_diff_eq(*tb, SEATED);
    }
    pb.sort_by(|p, q| p.x.total_cmp(&q.x));
    pa.iter().all(|p| {
        let from = pb.partition_point(|q| q.x < p.x - SEATED);
        pb[from..].iter().take_while(|q| q.x <= p.x + SEATED).any(|q| q.distance(*p) <= SEATED)
    })
}

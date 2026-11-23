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

/// Rewrites a link's path: given the assembly it stands in and the rest of the path, how many
/// occurrences to replace and by which; `None` to go on as it is.
type RouteStep<'a> = &'a dyn Fn(Id, &[Id]) -> Option<(usize, Vec<Id>)>;

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

    /// `path`, from `holder`, through what `step` rewrites: given where it stands and the rest
    /// of the path, how many occurrences it replaces and by which. `None` when it leads nowhere.
    fn rerouted(&self, holder: Id, path: &[Id], step: RouteStep) -> Option<Vec<Id>> {
        let (mut at, mut i, mut out) = (holder, 0, Vec::with_capacity(path.len() + 1));
        while i < path.len() {
            let (n, ids) = step(at, &path[i..]).unwrap_or_else(|| (1, vec![path[i]]));
            for id in ids {
                at = self.assembly(at).ok()?.occurrence(id)?.component;
                out.push(id);
            }
            i += n;
        }
        Some(out)
    }

    /// Every link's paths through `step`, then each link in the lowest assembly holding both
    /// its ends. A path that leads nowhere is left: the link is broken.
    fn reroute_links(&mut self, step: RouteStep) {
        for id in self.link_holders() {
            let Ok(a) = self.assembly(id) else { continue };
            let links: Vec<Link> = a
                .links()
                .iter()
                .map(|l| Link {
                    source: self.rerouted(id, &l.source, step).unwrap_or_else(|| l.source.clone()),
                    receiver: self.rerouted(id, &l.receiver, step).unwrap_or_else(|| l.receiver.clone()),
                    ..l.clone()
                })
                .collect();
            if links != a.links()
                && let Ok(a) = self.assembly_mut(id)
            {
                *a.links_mut() = links;
            }
        }
        self.settle_link_holders();
    }

    /// Each link in the lowest assembly holding both its ends: a copy of that assembly keeps it.
    fn settle_link_holders(&mut self) {
        for id in self.link_holders() {
            let Ok(a) = self.assembly(id) else { continue };
            let (mut kept, mut lowered) = (Vec::with_capacity(a.links().len()), Vec::new());
            for l in a.links() {
                let (mut at, mut depth) = (id, 0);
                while l.source.len() > depth + 1 && l.receiver.len() > depth + 1 && l.source[depth] == l.receiver[depth] {
                    let Some(next) = self.assembly(at).ok().and_then(|a| a.occurrence(l.source[depth])).map(|o| o.component) else { break };
                    (at, depth) = (next, depth + 1);
                }
                if depth == 0 {
                    kept.push(l.clone());
                } else {
                    lowered.push((at, Link { source: l.source[depth..].to_vec(), receiver: l.receiver[depth..].to_vec(), ..l.clone() }));
                }
            }
            if lowered.is_empty() {
                continue;
            }
            if let Ok(a) = self.assembly_mut(id) {
                *a.links_mut() = kept;
            }
            for (at, l) in lowered {
                if let Ok(a) = self.assembly_mut(at) {
                    a.links_mut().push(l);
                }
            }
        }
    }

    /// The occurrences `entities` of `context` gathered in the assembly its occurrence `made`
    /// places: their links go through it.
    pub(crate) fn links_gathered(&mut self, context: Id, entities: &[Id], made: Id) {
        self.reroute_links(&|at, rest| (at == context && entities.contains(&rest[0])).then(|| (1, vec![made, rest[0]])));
    }

    /// The occurrence `exploded` of `context`, an occurrence of `assembly`, replaced by copies
    /// of its content (`copies`: each occurrence of `assembly`, and its copy in `context`). The
    /// links through it go through its copies; those it held are held by `context` too.
    pub(crate) fn links_exploded(&mut self, context: Id, exploded: Id, assembly: Id, copies: &[(Id, Id)]) -> Result<()> {
        let copy = |id: Id| copies.iter().find(|(old, _)| *old == id).map(|(_, new)| *new);
        self.reroute_links(&|at, rest| {
            (at == context && rest[0] == exploded).then(|| rest.get(1).and_then(|c| copy(*c)).map(|c| (2, vec![c]))).flatten()
        });
        let inner: Vec<Link> = self.assembly(assembly)?.links().to_vec();
        for l in inner {
            let (Some(s), Some(r)) = (l.source.first().and_then(|c| copy(*c)), l.receiver.first().and_then(|c| copy(*c))) else { continue };
            let source = std::iter::once(s).chain(l.source[1..].iter().copied()).collect();
            let receiver = std::iter::once(r).chain(l.receiver[1..].iter().copied()).collect();
            let link = Link { id: self.fresh_id(), source, receiver, ..l };
            self.assembly_mut(context)?.links_mut().push(link);
        }
        self.settle_link_holders();
        Ok(())
    }

    /// The occurrences `occurrences` of the assembly `instances` leads to, moved to the one
    /// `to` leads to: the links ending in them, held on the way to the first, go by the second,
    /// held by an assembly above both when they were held below it.
    pub(crate) fn links_moved(&mut self, instances: &[Id], occurrences: &[Id], to: &[Id]) -> Result<()> {
        let common = instances.iter().zip(to).take_while(|(a, b)| a == b).count();
        for k in (0..=instances.len()).rev() {
            let Ok(holder) = self.context_of(&instances[..k]) else { continue };
            let tail = &instances[k..];
            let base = k.min(common);
            let carried = |p: &[Id]| p.len() > tail.len() && p.starts_with(tail) && occurrences.contains(&p[tail.len()]);
            let (moved, kept): (Vec<Link>, Vec<Link>) =
                self.assembly(holder)?.links().iter().cloned().partition(|l| carried(&l.source) || carried(&l.receiver));
            if moved.is_empty() {
                continue;
            }
            let route = |p: &[Id]| -> Vec<Id> {
                if carried(p) {
                    to[base..].iter().chain(&p[tail.len()..]).copied().collect()
                } else {
                    instances[base..k].iter().chain(p).copied().collect()
                }
            };
            let routed: Vec<Link> =
                moved.iter().map(|l| Link { source: route(&l.source), receiver: route(&l.receiver), ..l.clone() }).collect();
            *self.assembly_mut(holder)?.links_mut() = kept;
            let above = self.context_of(&instances[..base])?;
            self.assembly_mut(above)?.links_mut().extend(routed);
        }
        self.settle_link_holders();
        Ok(())
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

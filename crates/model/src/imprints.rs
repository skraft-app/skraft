//! « Empreindre » (docs/decisions/0055-linked-bodies-through-imprints.md): the parts chosen cut their
//! **final solid** in the parts it enters, however it was built. The tool is that whole solid, its
//! walls inside the receiver moved by a clearance, computed by the kernel; a linked body of the
//! receiver holds it. Both sides are read without their own linked bodies: a mortise is never part
//! of what makes a tool, so parts may imprint each other with no cycle.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use skraft_kernel::{Clearance, Job, Output, Solid, Stretch};

use crate::document::{Body, BodyDef, BodySource, ComponentKind, Document, Id, MAX_DEPTH, Part, Sign};
use crate::links::{Link, LinkState, same_place};
use crate::ops::{Operation, Outcome, Recorder, Solver};
use crate::{Error, FastSet, Refusal, Result};

/// How far two poses may differ and be one.
const SAME: f64 = 1e-6;

/// How a linked body differs from its source, inside the receiver.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImprintRecipe {
    pub clearance: Clearance,
}

/// A source and a part it enters, by their occurrences from the assembly imprinting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImprintPair {
    pub source: Vec<Id>,
    pub receiver: Vec<Id>,
}

/// The kernel's question before « Empreindre »: which of these pairs share material? Boxes say which
/// may; only the solids can say which do.
#[derive(Clone, Debug)]
pub struct ImprintProbe {
    context: Id,
    /// What to run; nothing to ask when it holds no task.
    pub job: Job,
    /// Each pair, the task answering for it (pairs placed alike share one), and whether it was
    /// pointed (else it comes by the copies).
    tests: Vec<(ImprintPair, usize, bool)>,
}

/// What « Empreindre » would do, for its confirmation.
#[derive(Clone, Debug)]
pub struct ImprintPlan {
    context: Id,
    pairs: Vec<ImprintPair>,
    machinings: usize,
    receivers: Vec<Id>,
    /// The receiving parts with a machining that some of their copies have no source for.
    pub partial: Vec<Id>,
    /// The links between the occurrences pointed; the others come by the copies.
    pointed: usize,
}

impl ImprintPlan {
    /// How many links it makes.
    pub fn link_count(&self) -> usize {
        self.pairs.len()
    }

    /// How many of its links come by the copies placed as the parts pointed are.
    pub fn by_copies(&self) -> usize {
        self.pairs.len() - self.pointed
    }

    /// How many machinings it adds to the receiving parts.
    pub fn machining_count(&self) -> usize {
        self.machinings
    }

    /// The parts receiving a machining.
    pub fn receivers(&self) -> &[Id] {
        &self.receivers
    }

    /// The operation doing it, with this clearance on the cheeks and on the bottom.
    pub fn operation(&self, around: f64, deep: f64) -> Operation {
        Operation::Imprint { context: self.context, pairs: self.pairs.clone(), around, deep }
    }
}

/// A part placed under the assembly a plan looks in.
struct Placed {
    path: Vec<Id>,
    part: Id,
    t: DAffine3,
}

/// A pair, with its parts and the source's axes in the receiver's.
struct Resolved {
    pair: ImprintPair,
    from: Id,
    to: Id,
    pose: DAffine3,
}

/// A link seated: its parts, the source's axes in the receiver's.
struct Live {
    from: Id,
    to: Id,
    pose: DAffine3,
    body: Id,
}

/// One machining of a receiver: the pairs sharing it.
struct Slot {
    to: Id,
    /// The pair it was first seen for.
    first: usize,
    /// A linked body already there.
    body: Option<Id>,
    reached: BTreeSet<Vec<Id>>,
}

/// Does `solid`, at `pose`, reach strictly into the box `lo..hi`? From the record, no kernel:
/// conservative on curved faces.
pub(crate) fn reaches(solid: &Solid, pose: &DAffine3, lo: DVec3, hi: DVec3) -> bool {
    (0..3).all(|k| {
        let Ok(s) = Stretch::new(DVec3::AXES[k], Vec::new(), vec![0.0]) else { return false };
        let shift = s.axis().dot(pose.translation);
        s.in_frame(pose).extent(solid).is_some_and(|(a, b)| a + shift < hi[k] && b + shift > lo[k])
    })
}

/// The box of `lo..hi` placed by `t`.
pub(crate) fn placed_box(t: &DAffine3, (lo, hi): (DVec3, DVec3)) -> (DVec3, DVec3) {
    (0..8).fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(a, b), i| {
        let c =
            DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
        let p = t.transform_point3(c);
        (a.min(p), b.max(p))
    })
}

pub(crate) fn boxes_meet(a: &(DVec3, DVec3), b: &(DVec3, DVec3)) -> bool {
    (0..3).all(|k| a.0[k] < b.1[k] && a.1[k] > b.0[k])
}

/// What a part's tools are made of, to tell whether it changed: its bodies but its linked ones.
fn own_bodies(p: &Part) -> Vec<(Id, Sign, DAffine3, *const Solid)> {
    p.bodies
        .iter()
        .filter(|b| p.def_of(b).source != BodySource::Linked)
        .map(|b| (b.id, b.sign, b.placement, Arc::as_ptr(p.solid_of(b))))
        .collect()
}

impl Document {
    /// Every part under the assembly `context`, with its path from it.
    fn placed_under(&self, context: Id) -> Vec<Placed> {
        fn walk(doc: &Document, at: Id, path: &mut Vec<Id>, t: DAffine3, out: &mut Vec<Placed>) {
            if path.len() > MAX_DEPTH {
                return;
            }
            let Ok(a) = doc.assembly(at) else { return };
            for o in a.occurrences() {
                let placed = t * o.transform;
                path.push(o.id);
                if doc.part(o.component).is_ok() {
                    out.push(Placed { path: path.clone(), part: o.component, t: placed });
                } else {
                    walk(doc, o.component, path, placed, out);
                }
                path.pop();
            }
        }
        let mut out = Vec::new();
        walk(self, context, &mut Vec::new(), DAffine3::IDENTITY, &mut out);
        out
    }

    /// The solid of `part` without its linked bodies: what it is by itself.
    fn own_solid(&self, part: Id, cache: &mut BTreeMap<Id, Arc<Solid>>, solver: &mut dyn Solver) -> Result<Arc<Solid>> {
        if let Some(s) = cache.get(&part) {
            return Ok(s.clone());
        }
        let p = self.part(part)?;
        let solid = if p.bodies.iter().any(|b| p.def_of(b).source == BodySource::Linked) {
            let list = |sign: Sign| -> Vec<(Arc<Solid>, DAffine3)> {
                p.bodies
                    .iter()
                    .filter(|b| b.sign == sign && p.def_of(b).source != BodySource::Linked)
                    .map(|b| (p.solid_of(b).clone(), b.placement))
                    .collect()
            };
            solver.recipe(None, &list(Sign::Positive), &list(Sign::Negative), &list(Sign::Intersect))?.0
        } else {
            p.shape().clone()
        };
        cache.insert(part, solid.clone());
        Ok(solid)
    }

    /// Can `source` reach into `receiver` (paths of occurrences from the model), by their boxes:
    /// what « Empreindre » lets the cursor take before the kernel is asked.
    pub fn may_imprint(&self, source: &[Id], receiver: &[Id]) -> bool {
        let root = self.root();
        let boxed = |path: &[Id]| {
            let (part, t) = self.through(root, path).ok()?;
            self.part(part).ok()?;
            Some(placed_box(&t, self.component_bounds(part)?))
        };
        source != receiver && boxed(source).zip(boxed(receiver)).is_some_and(|(a, b)| boxes_meet(&a, &b))
    }

    /// The kernel's question before « Empreindre »: `sources` pointed, then `receivers`, paths
    /// of occurrences from the model. Each source against each receiver counts, and so does every
    /// pair of copies placed as one of them is (the rails of a table in their legs, a frame of one
    /// rail in its copies); the kernel says which share material. Two copies of one part are
    /// paired one way.
    pub fn imprint_probe(&self, sources: &[Vec<Id>], receivers: &[Vec<Id>]) -> Result<ImprintProbe> {
        let context = self.root();
        let placed = self.placed_under(context);
        let find = |path: &[Id]| placed.iter().find(|p| p.path == path).ok_or(Error::NotFound(path.last().copied().unwrap_or(context)));
        let mut ways: Vec<(Id, Id, DAffine3)> = Vec::new();
        for s in sources {
            let s = find(s)?;
            for r in receivers {
                let r = find(r)?;
                let pose = r.t.inverse() * s.t;
                if r.path != s.path && !ways.iter().any(|(f, t, p)| *f == s.part && *t == r.part && p.abs_diff_eq(pose, SAME)) {
                    ways.push((s.part, r.part, pose));
                }
            }
        }
        let mut linked: FastSet<(Id, Id)> = FastSet::default();
        // The model's own links too: `components()` leaves it out.
        for c in self.components.values() {
            if let ComponentKind::Assembly(h) = &c.kind {
                for l in h.links() {
                    if let (Some(s), Some(r)) = (l.source.last(), l.receiver.last()) {
                        linked.insert((*s, *r));
                    }
                }
            }
        }
        let mut recorder = Recorder::default();
        let mut own = BTreeMap::new();
        let mut tests = Vec::new();
        let mut seen: FastSet<(&[Id], &[Id])> = FastSet::default();
        for &(from, to, pose) in &ways {
            let mut task = None;
            for a in placed.iter().filter(|a| a.part == from) {
                for b in placed.iter().filter(|b| b.part == to && b.path != a.path) {
                    if !(b.t.inverse() * a.t).abs_diff_eq(pose, SAME) {
                        continue;
                    }
                    let (Some(ai), Some(bi)) = (a.path.last(), b.path.last()) else { continue };
                    // Linked already, or the two copies paired the other way.
                    if linked.contains(&(*ai, *bi)) || (from == to && linked.contains(&(*bi, *ai))) || seen.contains(&(&b.path, &a.path)) {
                        continue;
                    }
                    if !seen.insert((&a.path, &b.path)) {
                        continue;
                    }
                    let t = match task {
                        Some(t) => t,
                        None => {
                            let source = self.own_solid(from, &mut own, &mut recorder)?;
                            let receiver = self.own_solid(to, &mut own, &mut recorder)?;
                            *task.insert(recorder.contact(&source, &receiver, pose))
                        }
                    };
                    let pointed = sources.contains(&a.path) && receivers.contains(&b.path);
                    tests.push((ImprintPair { source: a.path.clone(), receiver: b.path.clone() }, t, pointed));
                }
            }
        }
        Ok(ImprintProbe { context, job: recorder.into_job(), tests })
    }

    /// What « Empreindre » would do, from the kernel's `outputs` for `probe`.
    pub fn imprint_plan(&self, probe: &ImprintProbe, outputs: &[Output]) -> Result<ImprintPlan> {
        let touching: Vec<&(ImprintPair, usize, bool)> =
            probe.tests.iter().filter(|(_, t, _)| matches!(outputs.get(*t), Some(Output::Contact(true)))).collect();
        let pointed = touching.iter().filter(|(_, _, p)| *p).count();
        let pairs: Vec<ImprintPair> = touching.into_iter().map(|(p, _, _)| p.clone()).collect();
        let resolved = self.resolved(probe.context, &pairs)?;
        let (slots, _) = self.slots_of(&resolved, &self.live_links());
        let placed = self.placed_under(probe.context);
        let partial: BTreeSet<Id> =
            slots.iter().filter(|s| placed.iter().filter(|x| x.part == s.to).count() > s.reached.len()).map(|s| s.to).collect();
        Ok(ImprintPlan {
            context: probe.context,
            machinings: slots.iter().filter(|s| s.body.is_none()).count(),
            receivers: slots.iter().map(|s| s.to).collect::<BTreeSet<_>>().into_iter().collect(),
            partial: partial.into_iter().collect(),
            pointed,
            pairs,
        })
    }

    fn resolved(&self, context: Id, pairs: &[ImprintPair]) -> Result<Vec<Resolved>> {
        pairs
            .iter()
            .map(|pair| {
                let (from, from_t) = self.through(context, &pair.source)?;
                let (to, to_t) = self.through(context, &pair.receiver)?;
                Ok(Resolved { pair: pair.clone(), from, to, pose: to_t.inverse() * from_t })
            })
            .collect()
    }

    /// The links seated, anywhere.
    fn live_links(&self) -> Vec<Live> {
        let mut out = Vec::new();
        for holder in self.link_holders() {
            let Ok(a) = self.assembly(holder) else { continue };
            for l in a.links() {
                if self.link_state(holder, l) != LinkState::Seated {
                    continue;
                }
                let (Ok((from, from_t)), Ok((to, to_t))) = (self.through(holder, &l.source), self.through(holder, &l.receiver)) else {
                    continue;
                };
                out.push(Live { from, to, pose: to_t.inverse() * from_t, body: l.body });
            }
        }
        out
    }

    /// The machinings `pairs` make, and the one of each: one part in the same place of a
    /// receiver's copies is one body, on one already there if it is seated so. A body has one
    /// source part: it takes that part's shape.
    fn slots_of(&self, pairs: &[Resolved], live: &[Live]) -> (Vec<Slot>, Vec<usize>) {
        let shape = |part: Id| self.part(part).ok().map(|p| p.shape().clone());
        let mut slots: Vec<Slot> = Vec::new();
        let mut of = Vec::with_capacity(pairs.len());
        for (i, r) in pairs.iter().enumerate() {
            let at = slots.iter().position(|s| {
                let first = &pairs[s.first];
                s.to == r.to && r.from == first.from && shape(r.from).is_some_and(|a| same_place(&a, &r.pose, &a, &first.pose))
            });
            let at = at.unwrap_or_else(|| {
                let body = live.iter().find(|l| l.from == r.from && l.to == r.to && l.pose.abs_diff_eq(r.pose, SAME)).map(|l| l.body);
                slots.push(Slot { to: r.to, first: i, body, reached: BTreeSet::new() });
                slots.len() - 1
            });
            slots[at].reached.insert(r.pair.receiver.clone());
            of.push(at);
        }
        (slots, of)
    }

    /// [`Operation::Imprint`].
    pub(crate) fn imprint(
        &mut self,
        context: Id,
        pairs: &[ImprintPair],
        around: f64,
        deep: f64,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        let clearance = Clearance { around, deep };
        if !clearance.is_valid() {
            return Err(Error::Invalid(Refusal::BadClearance));
        }
        if pairs.is_empty() {
            return Err(Error::Invalid(Refusal::NothingToImprint));
        }
        let resolved = self.resolved(context, pairs)?;
        let live = self.live_links();
        let (slots, slot_of) = self.slots_of(&resolved, &live);
        // The tools first, from the parts as they are; then what is added to them.
        let mut own = BTreeMap::new();
        let mut tools = Vec::with_capacity(slots.len());
        for s in &slots {
            let first = &resolved[s.first];
            tools.push(match s.body {
                Some(_) => None,
                None => {
                    let source = self.own_solid(first.from, &mut own, solver)?;
                    let receiver = self.own_solid(s.to, &mut own, solver)?;
                    Some(solver.imprint(&source, &receiver, &first.pose, None, &clearance)?)
                }
            });
        }
        let mut bodies: Vec<Option<Id>> = slots.iter().map(|s| s.body).collect();
        let mut added: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
        for (k, (s, tool)) in slots.iter().zip(tools).enumerate() {
            let Some(tool) = tool else { continue };
            let (def, id) = (self.fresh_id(), self.fresh_id());
            let d = BodyDef { source: BodySource::Linked, imprint: Some(ImprintRecipe { clearance }), ..BodyDef::new(def, tool) };
            let q = self.part_mut(s.to)?;
            q.defs.push(d);
            q.bodies.push(Body::new(id, def, Sign::Negative, resolved[s.first].pose));
            added.entry(s.to).or_default().push(id);
            bodies[k] = Some(id);
        }
        let mut links = Vec::with_capacity(resolved.len());
        for (r, slot) in resolved.iter().zip(&slot_of) {
            let body = bodies[*slot].ok_or(Error::NotFound(r.to))?;
            links.push(Link { id: self.fresh_id(), source: r.pair.source.clone(), receiver: r.pair.receiver.clone(), body, seat: r.pose });
        }
        let machinings = added.values().map(Vec::len).sum();
        for (part, ids) in added {
            self.recompute(part, Some(&ids), solver)?;
        }
        let count = links.len();
        self.assembly_mut(context)?.links_mut().extend(links);
        self.settle_link_holders();
        Ok(Outcome { imprinted: (machinings, count), ..Outcome::default() })
    }

    /// The tool of the linked body `body` of `to` built again from `from`, whose axes are `pose`
    /// in the receiver's, with `clearance` if given; the body placed there. No contact left: the
    /// tool as it was, where it was in the receiver.
    fn rebuild_tool(
        &mut self,
        (from, to, body): (Id, Id, Id),
        pose: &DAffine3,
        clearance: Option<Clearance>,
        own: &mut BTreeMap<Id, Arc<Solid>>,
        solver: &mut dyn Solver,
    ) -> Result<()> {
        let q = self.part(to)?;
        let b = q.body(body).ok_or(Error::NotFound(body))?;
        let Some(recipe) = q.def_of(b).imprint.clone() else { return Ok(()) };
        let (def, previous, placed) = (b.def, q.solid_of(b).clone(), b.placement);
        let previous = if placed.abs_diff_eq(*pose, SAME) { previous } else { solver.transform(&previous, &(pose.inverse() * placed))? };
        let clearance = clearance.unwrap_or(recipe.clearance);
        let source = self.own_solid(from, own, solver)?;
        let receiver = self.own_solid(to, own, solver)?;
        let tool = solver.imprint(&source, &receiver, pose, Some(&previous), &clearance)?;
        let q = self.part_mut(to)?;
        if let Some(d) = q.defs.iter_mut().find(|d| d.id == def) {
            d.solid = tool;
            d.imprint = Some(ImprintRecipe { clearance });
        }
        if let Some(b) = q.bodies.iter_mut().find(|b| b.id == body) {
            b.placement = *pose;
        }
        Ok(())
    }

    /// [`Operation::SetClearance`]: on this linked body, its tool built again.
    pub(crate) fn set_clearance(&mut self, part: Id, body: Id, (around, deep): (f64, f64), solver: &mut dyn Solver) -> Result<Outcome> {
        let q = self.part(part)?;
        let b = q.body(body).filter(|b| q.def_of(b).source == BodySource::Linked).ok_or(Error::Invalid(Refusal::BadClearance))?;
        let recipe = q.def_of(b).imprint.clone().ok_or(Error::Invalid(Refusal::BadClearance))?;
        let clearance = Clearance { around, deep };
        if !clearance.is_valid() {
            return Err(Error::Invalid(Refusal::BadClearance));
        }
        if clearance == recipe.clearance {
            return Ok(Outcome::default());
        }
        let placement = b.placement;
        let live = self.live_links();
        let seated: Vec<&Live> = live.iter().filter(|l| l.to == part && l.body == body).collect();
        let from = seated.iter().find(|l| l.pose.abs_diff_eq(placement, SAME)).or(seated.first());
        let (from, pose) = from.map(|l| (l.from, l.pose)).ok_or(Error::Invalid(Refusal::BadClearance))?;
        self.rebuild_tool((from, part, body), &pose, Some(clearance), &mut BTreeMap::new(), solver)?;
        self.recompute(part, None, solver)?;
        Ok(Outcome::default())
    }

    /// The linked bodies after an operation (`before` the document it was applied to): those
    /// seated between parts whose own bodies changed take the source's shape as it now is, where
    /// it now is (a stretch moves a receiver and reshapes its source at once). Each link showing
    /// the source where the body now lies takes its own pose as its seat: through a mirrored
    /// receiver, one change of the source is another change in its axes. A receiver reshaped
    /// carries the sources seated in it wherever they now are: what it now is cannot be read
    /// while the job is recorded, and its box before misjudges a source a stretch carried past
    /// it (a shelf raised by legs made taller).
    pub(crate) fn update_imprints(&mut self, before: &Document, solver: &mut dyn Solver) -> Result<()> {
        let changed: BTreeSet<Id> = self
            .components
            .iter()
            .filter_map(|(id, c)| {
                let ComponentKind::Part(p) = &c.kind else { return None };
                if before.components.get(id).is_none_or(|b| Arc::ptr_eq(b, c)) {
                    return None;
                }
                let old = before.part(*id).ok()?;
                (own_bodies(p) != own_bodies(old)).then_some(*id)
            })
            .collect();
        if changed.is_empty() {
            return Ok(());
        }
        /// The link a machining is built from.
        struct Target {
            from: Id,
            pose: DAffine3,
            /// It is the one the body was placed by.
            on_body: bool,
        }
        let mut targets: BTreeMap<(Id, Id), Target> = BTreeMap::new();
        /// A link a body is rebuilt for, the source where it now is in the receiver's axes.
        struct Seated {
            holder: Id,
            link: Id,
            from: Id,
            pose: DAffine3,
        }
        let mut seated: BTreeMap<(Id, Id), Vec<Seated>> = BTreeMap::new();
        for holder in self.link_holders() {
            let (Ok(a), Ok(old)) = (self.assembly(holder), before.assembly(holder)) else { continue };
            for l in a.links() {
                let Some(was) = old.links().iter().find(|o| o.id == l.id) else { continue };
                let Ok((from, to, pose)) = self.link_pose(holder, l) else { continue };
                if !(changed.contains(&from) || changed.contains(&to))
                    || before.link_state(holder, was) != LinkState::Seated
                    || self.part(to).map_or(true, |q| q.body(l.body).is_none())
                {
                    continue;
                }
                let placed = before.link_pose(holder, was).ok().and_then(|(_, t, _)| Some(before.part(t).ok()?.body(was.body)?.placement));
                let on_body = placed.is_some_and(|p| p.abs_diff_eq(was.seat, SAME));
                seated.entry((to, l.body)).or_default().push(Seated { holder, link: l.id, from, pose });
                let target = Target { from, pose, on_body };
                match targets.entry((to, l.body)) {
                    std::collections::btree_map::Entry::Vacant(e) => {
                        e.insert(target);
                    }
                    std::collections::btree_map::Entry::Occupied(mut e) if on_body && !e.get().on_body => {
                        e.insert(target);
                    }
                    _ => {}
                }
            }
        }
        let mut own = BTreeMap::new();
        let mut reshaped = BTreeSet::new();
        for ((to, body), t) in targets {
            self.rebuild_tool((t.from, to, body), &t.pose, None, &mut own, solver)?;
            for Seated { holder, link, from, pose } in seated.remove(&(to, body)).unwrap_or_default() {
                let there = pose.abs_diff_eq(t.pose, SAME)
                    || (from == t.from && self.part(from).is_ok_and(|p| same_place(p.shape(), &pose, p.shape(), &t.pose)));
                if there && let Some(l) = self.assembly_mut(holder)?.links_mut().iter_mut().find(|l| l.id == link) {
                    l.seat = pose;
                }
            }
            reshaped.insert(to);
        }
        for part in reshaped {
            self.recompute(part, None, solver)?;
        }
        Ok(())
    }
}

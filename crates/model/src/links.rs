//! Linked bodies (docs/decisions/0055-linked-bodies-through-imprints.md): a part imprints its final
//! solid in another, as a linked body of that part. The link between them is held by an assembly
//! holding both occurrences, by paths relative to it: a copy of that assembly keeps its links.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use skraft_kernel::Solid;

use crate::document::{BodySource, ComponentKind, Document, EntityPath, Id};
use crate::ops::{Outcome, Solver};
use crate::{Error, FastMap, FastSet, Refusal, Result, Warning};

/// How far (each term of the transform, mm for its translation) a source may lie from its seat
/// and still sit in it.
const SEATED: f64 = 1e-6;

/// An occurrence seated in a linked body of another. Several links may share one body: the rails
/// entering the legs of one definition, each by a link of its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub id: Id,
    /// The occurrences leading from the assembly holding the link to the source part.
    pub source: Vec<Id>,
    /// The occurrences leading from that assembly to the receiving part.
    pub receiver: Vec<Id>,
    /// The linked body: a body at the root of the receiving part.
    pub body: Id,
    /// The source's axes in the receiver's when it sits in `body`. Per link: read back from a
    /// body shared by mirrored receivers, it would give a mirrored source.
    pub seat: DAffine3,
}

/// Where a link stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// The source lies on its seat.
    Seated,
    /// Everything is there, the source elsewhere.
    Off,
    /// A path or the linked body is gone.
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
    /// The sources, seated before it, that the operation moved.
    pub sources: Vec<LinkSource>,
}

/// A link's source: the occurrence imprinting, in the assembly it lies in. Links held
/// at different levels name the same source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LinkSource {
    pub assembly: Id,
    pub occurrence: Id,
}

/// What the document's links need the user to see.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinkStatus {
    /// The sources with a link off.
    pub off: BTreeSet<LinkSource>,
    /// The linked bodies, by part and id, with a link broken and none left alive: frozen.
    pub broken: BTreeSet<(Id, Id)>,
    /// The components holding one or the other, at any depth.
    holding: FastSet<Id>,
}

impl LinkStatus {
    /// Is the occurrence `occurrence` of `assembly` a source off?
    pub fn is_off(&self, assembly: Id, occurrence: Id) -> bool {
        self.off.contains(&LinkSource { assembly, occurrence })
    }

    /// Does `component` hold a source off or a linked body broken, at any depth?
    pub fn holds(&self, component: Id) -> bool {
        self.holding.contains(&component)
    }
}

/// Who shapes whom through the document's links, for the interface to say so.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinkMap {
    /// Each linked body with a source, by part and id: the part it takes its shape from.
    sources: BTreeMap<(Id, Id), Id>,
    /// Each occurrence imprinting: the parts it shapes. Per occurrence, not per part: of two
    /// copies of a part, the one whose link was broken or removed shapes nothing more.
    receivers: BTreeMap<LinkSource, BTreeSet<Id>>,
}

impl LinkMap {
    /// The part the linked body `body` of `part` takes its shape from.
    pub fn source_of(&self, part: Id, body: Id) -> Option<Id> {
        self.sources.get(&(part, body)).copied()
    }

    /// The parts the occurrence `source` shapes.
    pub fn receivers_of(&self, source: LinkSource) -> Option<&BTreeSet<Id>> {
        self.receivers.get(&source)
    }
}

impl Document {
    /// The part `path` leads to from the assembly `assembly`, and its axes in that assembly's.
    pub(crate) fn through(&self, assembly: Id, path: &[Id]) -> Result<(Id, DAffine3)> {
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

    /// The source part, the receiving part and the source's axes in the receiver's.
    pub(crate) fn link_pose(&self, assembly: Id, link: &Link) -> Result<(Id, Id, DAffine3)> {
        let (from, from_t) = self.through(assembly, &link.source)?;
        let (to, to_t) = self.through(assembly, &link.receiver)?;
        Ok((from, to, to_t.inverse() * from_t))
    }

    /// Where `link`, held by the assembly `assembly`, stands.
    pub fn link_state(&self, assembly: Id, link: &Link) -> LinkState {
        let Ok((from, to, pose)) = self.link_pose(assembly, link) else { return LinkState::Broken };
        let (Ok(p), Ok(q)) = (self.part(from), self.part(to)) else { return LinkState::Broken };
        let Some(b) = q.body(link.body).filter(|b| q.def_of(b).source == BodySource::Linked) else { return LinkState::Broken };
        // On its seat, the body there: made at this pose, or shared from another showing the
        // source in the same place (a mirrored receiver).
        let there = b.placement.abs_diff_eq(pose, SEATED) || same_place(p.shape(), &pose, p.shape(), &b.placement);
        if pose.abs_diff_eq(link.seat, SEATED) && there { LinkState::Seated } else { LinkState::Off }
    }

    /// The source of `link` in the axes of the assembly holding it.
    fn source_place(&self, assembly: Id, link: &Link) -> Option<DAffine3> {
        self.through(assembly, &link.source).ok().map(|(_, t)| t)
    }

    /// Does the source of `link`, at `pose` in the receiver's axes, still reach into the
    /// receiver's box? Read from the record, no kernel: conservative on curved faces.
    pub(crate) fn source_enters(&self, assembly: Id, link: &Link, pose: &DAffine3) -> bool {
        let Ok((from, to, _)) = self.link_pose(assembly, link) else { return false };
        let Some((lo, hi)) = self.component_bounds(to) else { return false };
        self.part(from).is_ok_and(|p| crate::imprints::reaches(p.shape(), pose, lo, hi))
    }

    /// The linked bodies after an operation (`before` the document it was applied to): each
    /// goes with the sources that were seated in it and moved, in every copy of its part, its
    /// links' seats with it. A receiver moved drives nothing; a source leaving its receiver
    /// leaves the body; different moves asked of one body leave it where it is. Decided on
    /// transforms and on `before`'s solids alone: the same when the job is recorded and replayed.
    pub(crate) fn follow_links(&mut self, before: &Document, solver: &mut dyn Solver) -> Result<Followed> {
        let mut asks: BTreeMap<(Id, Id), Vec<DAffine3>> = BTreeMap::new();
        let mut sources = BTreeSet::new();
        for (id, c) in &self.components {
            let ComponentKind::Assembly(a) = &c.kind else { continue };
            if a.links().is_empty() {
                continue;
            }
            let Ok(old) = before.assembly(*id) else { continue };
            let old: FastMap<Id, &Link> = old.links().iter().map(|l| (l.id, l)).collect();
            for l in a.links() {
                let Some(was) = old.get(&l.id) else { continue };
                let Ok((_, to, pose)) = self.link_pose(*id, l) else { continue };
                if pose.abs_diff_eq(l.seat, SEATED) || before.link_state(*id, was) != LinkState::Seated {
                    continue;
                }
                let (Some(a0), Some(a1)) = (before.source_place(*id, was), self.source_place(*id, l)) else { continue };
                if a0.abs_diff_eq(a1, SEATED) {
                    continue;
                }
                sources.extend(self.link_source(*id, l));
                let there = self.part(to).is_ok_and(|q| q.body(l.body).is_some());
                if there && before.source_enters(*id, was, &pose) {
                    asks.entry((to, l.body)).or_default().push(pose * l.seat.inverse());
                }
            }
        }
        let mut followed = Followed { sources: sources.into_iter().collect(), ..Followed::default() };
        let mut changed = BTreeSet::new();
        for ((part, body), moves) in asks {
            if !moves.iter().all(|d| d.abs_diff_eq(moves[0], SEATED)) {
                followed.conflicts.push((part, body));
                continue;
            }
            self.place_linked_body(part, body, &moves[0])?;
            changed.insert(part);
            followed.moved.push((part, body));
        }
        for part in changed {
            self.recompute(part, None, solver)?;
        }
        Ok(followed)
    }

    /// The source of `link`, held by `holder`: the last occurrence of its path, where it lies.
    pub(crate) fn link_source(&self, holder: Id, link: &Link) -> Option<LinkSource> {
        let (last, above) = link.source.split_last()?;
        let (assembly, _) = self.assembly_through(holder, above)?;
        self.assembly(assembly).ok()?.occurrence(*last)?;
        Some(LinkSource { assembly, occurrence: *last })
    }

    /// The source occurrence of the linked body `body` of the part `receiver` leads to (occurrences
    /// from the model), by its path from the model: the link held lowest above the receiver.
    /// `None` without one (the body not linked, its link gone or broken).
    pub fn link_source_path(&self, receiver: &[Id], body: Id) -> Option<EntityPath> {
        // The assembly each prefix of `receiver` leads to, the model first.
        let mut holders = vec![self.root()];
        for &i in &receiver[..receiver.len().saturating_sub(1)] {
            let next = self.assembly(*holders.last()?).ok()?.occurrence(i)?.component;
            holders.push(next);
        }
        holders.iter().enumerate().rev().find_map(|(k, &holder)| {
            let a = self.assembly(holder).ok()?;
            let l = a.links().iter().find(|l| l.body == body && l.receiver == receiver[k..])?;
            self.link_source(holder, l)?;
            let (last, above) = l.source.split_last()?;
            Some(EntityPath::new(receiver[..k].iter().chain(above).copied().collect(), *last))
        })
    }

    /// The assembly `path` leads to from `holder`, and its axes in `holder`'s.
    fn assembly_through(&self, holder: Id, path: &[Id]) -> Option<(Id, DAffine3)> {
        let (mut at, mut t) = (holder, DAffine3::IDENTITY);
        for &i in path {
            let o = self.assembly(at).ok()?.occurrence(i)?;
            t *= o.transform;
            at = o.component;
        }
        self.assembly(at).ok()?;
        Some((at, t))
    }

    /// What the links need the user to see (docs/model/bodies.md#links).
    pub fn link_status(&self) -> LinkStatus {
        let mut status = LinkStatus::default();
        let (mut alive, mut lost) = (FastSet::default(), Vec::new());
        for holder in self.link_holders() {
            let Ok(a) = self.assembly(holder) else { continue };
            for l in a.links() {
                let Ok((to, _)) = self.through(holder, &l.receiver) else { continue };
                match self.link_state(holder, l) {
                    LinkState::Broken => lost.push((to, l.body)),
                    state => {
                        alive.insert((to, l.body));
                        if state == LinkState::Off {
                            status.off.extend(self.link_source(holder, l));
                        }
                    }
                }
            }
        }
        status.broken = lost.into_iter().filter(|k| !alive.contains(k) && self.part(k.0).is_ok_and(|q| q.body(k.1).is_some())).collect();
        status.holding = status.off.iter().map(|s| s.assembly).chain(status.broken.iter().map(|b| b.0)).collect();
        self.spread_holding(&mut status.holding);
        status
    }

    /// `holding` with the assemblies above its components, up to the model: each pass reaches one
    /// assembly higher.
    pub(crate) fn spread_holding(&self, holding: &mut FastSet<Id>) {
        for _ in 0..crate::MAX_DEPTH {
            let above: Vec<Id> = self
                .components
                .iter()
                .filter(|(id, c)| {
                    !holding.contains(*id)
                        && matches!(&c.kind, ComponentKind::Assembly(a) if a.occurrences().iter().any(|o| holding.contains(&o.component)))
                })
                .map(|(id, _)| *id)
                .collect();
            if above.is_empty() {
                break;
            }
            holding.extend(above);
        }
    }

    /// Who shapes whom: the links whose two parts and linked body are there.
    pub fn link_map(&self) -> LinkMap {
        let mut map = LinkMap::default();
        for holder in self.link_holders() {
            let Ok(a) = self.assembly(holder) else { continue };
            for l in a.links() {
                let Ok((from, to, _)) = self.link_pose(holder, l) else { continue };
                if self.part(to).is_ok_and(|q| q.body(l.body).is_some_and(|b| q.def_of(b).source == BodySource::Linked)) {
                    map.sources.entry((to, l.body)).or_insert(from);
                    if let Some(source) = self.link_source(holder, l) {
                        map.receivers.entry(source).or_default().insert(to);
                    }
                }
            }
        }
        map
    }

    /// `bodies` of `part` without its linked ones, which follow their sources: what an edit of
    /// their shape or place applies to. Refused when it leaves none of them.
    pub(crate) fn editable(&self, part: Id, bodies: Vec<Id>) -> Result<Vec<Id>> {
        let p = self.part(part)?;
        let kept: Vec<Id> =
            bodies.iter().copied().filter(|id| p.body(*id).is_none_or(|b| p.def_of(b).source != BodySource::Linked)).collect();
        if kept.is_empty() && !bodies.is_empty() {
            return Err(Error::Invalid(Refusal::LinkedBody));
        }
        Ok(kept)
    }

    /// Each linked body named after its source part. One that lost its source keeps the last.
    pub(crate) fn settle_link_names(&mut self) {
        let mut names: BTreeMap<(Id, Id), String> = BTreeMap::new();
        for holder in self.link_holders() {
            let Ok(a) = self.assembly(holder) else { continue };
            for l in a.links() {
                let Ok((from, to, _)) = self.link_pose(holder, l) else { continue };
                let (Ok(c), Ok(q)) = (self.component(from), self.part(to)) else { continue };
                let Some(b) = q.body(l.body).filter(|b| q.def_of(b).source == BodySource::Linked) else { continue };
                let d = q.def_of(b);
                if !d.named || d.name != c.name {
                    names.entry((to, d.id)).or_insert_with(|| c.name.clone());
                }
            }
        }
        for ((part, def), name) in names {
            if let Some(d) = self.part_mut(part).ok().and_then(|q| q.defs.iter_mut().find(|d| d.id == def)) {
                d.set_name(Some(name));
            }
        }
    }

    /// Takes away the links with nothing left to show: their receiver or their linked body gone.
    /// One whose source is gone stays: its body is told frozen.
    pub(crate) fn drop_dead_links(&mut self) {
        for id in self.link_holders() {
            let Ok(a) = self.assembly(id) else { continue };
            let live = |l: &Link| self.through(id, &l.receiver).is_ok_and(|(to, _)| self.part(to).is_ok_and(|q| q.body(l.body).is_some()));
            let kept: Vec<Link> = a.links().iter().filter(|l| live(l)).cloned().collect();
            if kept.len() != a.links().len()
                && let Ok(a) = self.assembly_mut(id)
            {
                *a.links_mut() = kept;
            }
        }
    }

    /// Where each link of the source `source` wants it, in the axes of the assembly it lies
    /// in, from the seat in its receiver.
    fn seat_targets(&self, source: LinkSource) -> Vec<DAffine3> {
        let mut out = Vec::new();
        for holder in self.link_holders() {
            let Ok(a) = self.assembly(holder) else { continue };
            for l in a.links().iter().filter(|l| self.link_source(holder, l) == Some(source)) {
                let Some((_, above)) = l.source.split_last().and_then(|(_, up)| self.assembly_through(holder, up)) else { continue };
                let Ok((_, to_t)) = self.through(holder, &l.receiver) else { continue };
                out.push(above.inverse() * to_t * l.seat);
            }
        }
        out
    }

    /// The occurrences `source` is seated in, as sources are named.
    fn seat_receivers(&self, source: LinkSource) -> Vec<LinkSource> {
        let mut out = Vec::new();
        for holder in self.link_holders() {
            let Ok(a) = self.assembly(holder) else { continue };
            for l in a.links().iter().filter(|l| self.link_source(holder, l) == Some(source)) {
                let Some((last, above)) = l.receiver.split_last() else { continue };
                if let Some((assembly, _)) = self.assembly_through(holder, above) {
                    out.push(LinkSource { assembly, occurrence: *last });
                }
            }
        }
        out
    }

    /// « Replacer » as a chain: each source placed once the parts it enters are where they stay,
    /// so that a source entering another one replaced lands on that one's new place (a frame of
    /// copies, one rail moved in its plane: the others follow round). `None`, nothing changed,
    /// when some cannot be placed so (seats disagreeing, a closed cycle) or when it would leave
    /// a source off that was not.
    fn reseat_in_chain(&self, sources: &[LinkSource]) -> Option<Document> {
        let mut doc = self.clone();
        let off_before = self.link_status().off;
        let mut pending: Vec<LinkSource> = sources.to_vec();
        while !pending.is_empty() {
            let ready: Vec<LinkSource> =
                pending.iter().copied().filter(|s| doc.seat_receivers(*s).iter().all(|r| !pending.contains(r))).collect();
            if ready.is_empty() {
                return None;
            }
            for s in ready {
                let targets = doc.seat_targets(s);
                let first = *targets.first()?;
                if !targets.iter().all(|t| t.abs_diff_eq(first, SEATED)) {
                    return None;
                }
                doc.assembly_mut(s.assembly).ok()?.occurrence_mut(s.occurrence)?.transform = first;
                pending.retain(|p| *p != s);
            }
        }
        let off = doc.link_status().off;
        let fine = off.iter().all(|s| off_before.contains(s) && !sources.contains(s));
        fine.then_some(doc)
    }

    /// [`crate::Operation::ReseatSources`]: each source goes where its seats say, when they all
    /// agree; the others stay where they are, and the warning says by how much they disagree.
    /// Sources entering each other are placed in a chain when that seats them all.
    pub(crate) fn reseat_sources(&mut self, sources: &[LinkSource]) -> Result<Outcome> {
        if let Some(doc) = self.reseat_in_chain(sources) {
            *self = doc;
            return Ok(Outcome::default());
        }
        let (mut moves, mut names, mut gap) = (Vec::new(), Vec::new(), 0.0_f64);
        for s in sources {
            let targets = self.seat_targets(*s);
            let Some(first) = targets.first() else { continue };
            if targets.iter().all(|t| t.abs_diff_eq(*first, SEATED)) {
                moves.push((*s, *first));
            } else {
                gap = targets.iter().fold(gap, |g, t| g.max(t.translation.distance(first.translation)));
                let o = self.assembly(s.assembly)?.occurrence(s.occurrence).ok_or(Error::NotFound(s.occurrence))?;
                names.push(self.component(o.component)?.name.clone());
            }
        }
        // A source may be another's receiver: moved, it changes that one's seat. Each placed
        // where it is not seated after all (a closed cycle of copies) goes back and is told.
        let mut was = Vec::with_capacity(moves.len());
        for (s, t) in &moves {
            let o = self.assembly_mut(s.assembly)?.occurrence_mut(s.occurrence).ok_or(Error::NotFound(s.occurrence))?;
            was.push(std::mem::replace(&mut o.transform, *t));
        }
        let mut kept = vec![true; moves.len()];
        loop {
            let stuck: Vec<(usize, f64)> = (0..moves.len())
                .filter(|&k| kept[k])
                .filter_map(|k| {
                    let far =
                        self.seat_targets(moves[k].0).iter().map(|t| t.translation.distance(moves[k].1.translation)).fold(0.0, f64::max);
                    let off = self.seat_targets(moves[k].0).iter().any(|t| !t.abs_diff_eq(moves[k].1, SEATED));
                    off.then_some((k, far))
                })
                .collect();
            if stuck.is_empty() {
                break;
            }
            for (k, far) in stuck {
                kept[k] = false;
                gap = gap.max(far);
                let s = moves[k].0;
                if let Some(o) = self.assembly_mut(s.assembly)?.occurrence_mut(s.occurrence) {
                    o.transform = was[k];
                }
                let o = self.assembly(s.assembly)?.occurrence(s.occurrence).ok_or(Error::NotFound(s.occurrence))?;
                names.push(self.component(o.component)?.name.clone());
            }
        }
        let warning = (!names.is_empty()).then(|| Warning::NotReseated { names, gap: self.settings.units.format(gap) });
        Ok(Outcome { warning, ..Outcome::default() })
    }

    /// [`crate::Operation::SeparateReceivers`]: each part receiving these sources made a part of
    /// its own; then, in it and in the copies left, each linked body goes back under its sources
    /// when none is seated in it and they agree.
    pub(crate) fn separate_receivers(&mut self, sources: &[LinkSource], solver: &mut dyn Solver) -> Result<Outcome> {
        let mut routes = BTreeSet::new();
        for holder in self.link_holders() {
            let a = self.assembly(holder)?;
            for l in a.links() {
                if self.link_source(holder, l).is_some_and(|s| sources.contains(&s)) && self.link_state(holder, l) != LinkState::Broken {
                    routes.insert((holder, l.receiver.clone()));
                }
            }
        }
        let mut touched = BTreeSet::new();
        for (holder, path) in routes {
            let Ok((old, _)) = self.through(holder, &path) else { continue };
            let mut at = holder;
            for &occurrence in &path {
                self.apply_in_place(&crate::Operation::MakeUnique { context: at, occurrence }, solver)?;
                at = self.assembly(at)?.occurrence(occurrence).ok_or(Error::NotFound(occurrence))?.component;
            }
            touched.extend([old, at]);
        }
        let mut asks: BTreeMap<(Id, Id), (bool, Vec<DAffine3>)> = BTreeMap::new();
        for holder in self.link_holders() {
            let a = self.assembly(holder)?;
            for l in a.links() {
                let Ok((_, to, pose)) = self.link_pose(holder, l) else { continue };
                if !touched.contains(&to) {
                    continue;
                }
                let ask = asks.entry((to, l.body)).or_default();
                match self.link_state(holder, l) {
                    LinkState::Seated => ask.0 = true,
                    LinkState::Off => ask.1.push(pose * l.seat.inverse()),
                    LinkState::Broken => {}
                }
            }
        }
        let mut changed = BTreeSet::new();
        for ((part, body), (seated, moves)) in asks {
            let Some(d) = moves.first().copied().filter(|d| !seated && moves.iter().all(|m| m.abs_diff_eq(*d, SEATED))) else { continue };
            self.place_linked_body(part, body, &d)?;
            changed.insert(part);
        }
        for part in changed {
            self.recompute(part, None, solver)?;
        }
        Ok(Outcome::default())
    }

    /// [`crate::Operation::BringLinkedBody`]: the linked bodies of the source's links off, each
    /// brought under it, in every copy of its part; its other sources are then off.
    pub(crate) fn bring_linked_body(&mut self, source: LinkSource, solver: &mut dyn Solver) -> Result<Outcome> {
        let mut asks: BTreeMap<(Id, Id), Vec<DAffine3>> = BTreeMap::new();
        for holder in self.link_holders() {
            let a = self.assembly(holder)?;
            for l in a.links().iter().filter(|l| self.link_source(holder, l) == Some(source)) {
                if self.link_state(holder, l) != LinkState::Off {
                    continue;
                }
                let (_, to, pose) = self.link_pose(holder, l)?;
                asks.entry((to, l.body)).or_default().push(pose * l.seat.inverse());
            }
        }
        if asks.values().any(|m| !m.iter().all(|d| d.abs_diff_eq(m[0], SEATED))) {
            return Err(Error::Invalid(Refusal::LinkConflict));
        }
        let mut changed = BTreeSet::new();
        for ((part, body), moves) in asks {
            self.place_linked_body(part, body, &moves[0])?;
            changed.insert(part);
        }
        for part in changed {
            self.recompute(part, None, solver)?;
        }
        Ok(Outcome::default())
    }

    /// The linked body `body` of `part` moved by `d`, in the part's axes, its links' seats with it.
    fn place_linked_body(&mut self, part: Id, body: Id, d: &DAffine3) -> Result<()> {
        if let Some(b) = self.part_mut(part)?.bodies.iter_mut().find(|b| b.id == body) {
            b.placement = *d * b.placement;
        }
        self.map_seats(part, Some(body), d);
        Ok(())
    }

    /// [`crate::Operation::RemoveLinked`]: these linked bodies, each by its part and id, taken
    /// away from their parts; their links go with them.
    pub(crate) fn remove_linked(&mut self, bodies: &[(Id, Id)], solver: &mut dyn Solver) -> Result<Outcome> {
        let parts: BTreeSet<Id> = bodies.iter().map(|(part, _)| *part).collect();
        for part in parts {
            let q = self.part(part)?;
            let gone: Vec<Id> = q
                .bodies
                .iter()
                .filter(|b| bodies.contains(&(part, b.id)) && q.def_of(b).source == BodySource::Linked)
                .map(|b| b.id)
                .collect();
            if gone.is_empty() {
                continue;
            }
            let q = self.part_mut(part)?;
            q.bodies.retain(|b| !gone.contains(&b.id));
            q.drop_unused_defs();
            self.recompute(part, None, solver)?;
        }
        Ok(Outcome::default())
    }

    /// [`crate::Operation::Unlink`]: the bodies `bodies` of `part` become plain bodies, their
    /// shape and place as they are, their links gone.
    pub(crate) fn unlink_bodies(&mut self, part: Id, bodies: &[Id]) -> Result<Outcome> {
        let q = self.part_mut(part)?;
        let defs: Vec<Id> = q.bodies.iter().filter(|b| bodies.contains(&b.id)).map(|b| b.def).collect();
        for d in q.defs.iter_mut().filter(|d| defs.contains(&d.id)) {
            d.source = BodySource::Local;
        }
        for holder in self.link_holders() {
            let a = self.assembly(holder)?;
            let goes = |l: &Link| bodies.contains(&l.body) && self.through(holder, &l.receiver).is_ok_and(|(to, _)| to == part);
            let kept: Vec<Link> = a.links().iter().filter(|l| !goes(l)).cloned().collect();
            if kept.len() != a.links().len() {
                *self.assembly_mut(holder)?.links_mut() = kept;
            }
        }
        Ok(Outcome::default())
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

    /// The axes of the source part `part` changed by `axes` (what its occurrences' transforms were
    /// multiplied by): its links' seats, and their linked bodies, kept in its axes. Nothing moves.
    pub(crate) fn map_source_axes(&mut self, part: Id, axes: &DAffine3, solver: &mut dyn Solver) -> Result<()> {
        let mut bodies = BTreeSet::new();
        for id in self.link_holders() {
            let a = self.assembly(id)?;
            let reached: Vec<usize> =
                (0..a.links().len()).filter(|&k| self.through(id, &a.links()[k].source).is_ok_and(|(from, _)| from == part)).collect();
            for &k in &reached {
                if let Ok((to, _)) = self.through(id, &a.links()[k].receiver) {
                    bodies.insert((to, a.links()[k].body));
                }
            }
            let a = self.assembly_mut(id)?;
            for k in reached {
                a.links_mut()[k].seat *= *axes;
            }
        }
        let into = axes.inverse();
        for (to, body) in bodies {
            let q = self.part(to)?;
            let Some(b) = q.body(body) else { continue };
            let (def, solid) = (b.def, q.solid_of(b).clone());
            let solid = solver.transform(&solid, &into)?;
            let q = self.part_mut(to)?;
            if let Some(d) = q.defs.iter_mut().find(|d| d.id == def) {
                d.solid = solid;
            }
            if let Some(b) = q.bodies.iter_mut().find(|b| b.id == body) {
                b.placement *= *axes;
            }
        }
        Ok(())
    }

    /// The assemblies holding links.
    pub(crate) fn link_holders(&self) -> Vec<Id> {
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
    pub(crate) fn settle_link_holders(&mut self) {
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
}

/// Do the solids `a` and `b`, placed, lie in the same place: their vertices the same points,
/// whatever the placements (a mirrored receiver sees a symmetric body through a mirror)? A solid
/// without vertices (a sphere, a torus) only by the same placement.
pub(crate) fn same_place(a: &Solid, ta: &DAffine3, b: &Solid, tb: &DAffine3) -> bool {
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

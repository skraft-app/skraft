//! The alert between linked parts (docs/model/bodies.md#links): two parts seated in one receiver
//! that share material. Their final solids are compared, so a joint each has cut in the other is
//! no overlap. Only the kernel can tell, so it is asked in the background; what it answered is
//! kept while the same solids stand the same way.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use skraft_kernel::{Job, Output, Solid};

use crate::document::{Document, Id, MAX_DEPTH};
use crate::imprints::{boxes_meet, placed_box};
use crate::links::{LinkSource, LinkState};
use crate::ops::Recorder;
use crate::{FastMap, FastSet};

/// How far two poses may differ and be one.
const SAME: f64 = 1e-6;

/// Two sources seated in the part `receiver`, sharing material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinkOverlap {
    pub first: LinkSource,
    pub second: LinkSource,
    pub receiver: Id,
}

/// The overlaps the kernel found in the document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinkOverlaps {
    pairs: BTreeSet<LinkOverlap>,
    /// The assemblies holding one of these sources, at any depth.
    holding: FastSet<Id>,
}

impl LinkOverlaps {
    pub fn pairs(&self) -> &BTreeSet<LinkOverlap> {
        &self.pairs
    }

    /// What `source` overlaps, and in which part.
    pub fn of(&self, source: LinkSource) -> impl Iterator<Item = (LinkSource, Id)> + '_ {
        self.pairs.iter().filter_map(move |o| match (o.first == source, o.second == source) {
            (true, _) => Some((o.second, o.receiver)),
            (_, true) => Some((o.first, o.receiver)),
            _ => None,
        })
    }

    /// Does `component` hold a source overlapping another, at any depth?
    pub fn holds(&self, component: Id) -> bool {
        self.holding.contains(&component)
    }
}

/// Do these two solids, the first placed by `pose` in the second's axes, share material?
#[derive(Clone, Debug)]
struct Question {
    first: Arc<Solid>,
    second: Arc<Solid>,
    pose: DAffine3,
}

impl Question {
    fn same(&self, other: &Question) -> bool {
        Arc::ptr_eq(&self.first, &other.first) && Arc::ptr_eq(&self.second, &other.second) && self.pose.abs_diff_eq(other.pose, SAME)
    }
}

/// The kernel's last answers. Holding the solids keeps their addresses theirs.
#[derive(Clone, Debug, Default)]
pub struct OverlapMemo(Vec<(Question, bool)>);

#[derive(Clone, Copy, Debug)]
enum Answer {
    Known(bool),
    Task(usize),
}

/// The kernel's question about the document's overlaps.
#[derive(Clone, Debug, Default)]
pub struct OverlapProbe {
    /// What to run; nothing to ask when it holds no task.
    pub job: Job,
    asked: Vec<(Question, Answer)>,
    /// Each pair whose boxes meet, and its question in `asked`.
    pairs: Vec<(LinkOverlap, usize)>,
}

impl OverlapProbe {
    /// Does it ask the same of the same pairs? Then an answer on its way serves for `other`.
    pub fn asks_the_same(&self, other: &OverlapProbe) -> bool {
        self.pairs == other.pairs
            && self.asked.len() == other.asked.len()
            && self.asked.iter().zip(&other.asked).all(|((a, _), (b, _))| a.same(b))
    }
}

/// A source seated in a receiver, from the model.
struct Entering {
    path: Vec<Id>,
    part: Id,
    t: DAffine3,
    source: LinkSource,
}

impl Document {
    /// The sources seated under the assembly `at`, by receiving part and its path from the model:
    /// each copy of an assembly holding links counts.
    fn entering(&self, at: Id, prefix: &mut Vec<Id>, t: DAffine3, out: &mut BTreeMap<(Id, Vec<Id>), Vec<Entering>>) {
        if prefix.len() > MAX_DEPTH {
            return;
        }
        let Ok(a) = self.assembly(at) else { return };
        for l in a.links() {
            if self.link_state(at, l) != LinkState::Seated {
                continue;
            }
            let (Ok((part, from)), Ok((to, _)), Some(source)) =
                (self.through(at, &l.source), self.through(at, &l.receiver), self.link_source(at, l))
            else {
                continue;
            };
            let path = [prefix.as_slice(), &l.source].concat();
            let list = out.entry((to, [prefix.as_slice(), &l.receiver].concat())).or_default();
            if !list.iter().any(|e| e.path == path) {
                list.push(Entering { path, part, t: t * from, source });
            }
        }
        for o in a.occurrences() {
            if self.assembly(o.component).is_ok() {
                prefix.push(o.id);
                self.entering(o.component, prefix, t * o.transform, out);
                prefix.pop();
            }
        }
    }

    /// What to ask the kernel about the sources sharing a receiver: the pairs whose boxes meet,
    /// but for what `memo` already knows.
    pub fn overlap_probe(&self, memo: &OverlapMemo) -> OverlapProbe {
        let mut probe = OverlapProbe::default();
        if self.link_holders().is_empty() {
            return probe;
        }
        let mut entering = BTreeMap::new();
        self.entering(self.root(), &mut Vec::new(), DAffine3::IDENTITY, &mut entering);
        let mut recorder = Recorder::default();
        let mut bounds: FastMap<Id, Option<(DVec3, DVec3)>> = FastMap::default();
        for ((receiver, _), list) in &entering {
            for (i, a) in list.iter().enumerate() {
                for b in &list[i + 1..] {
                    let mut bounds_of = |e: &Entering| *bounds.entry(e.part).or_insert_with(|| self.component_bounds(e.part));
                    let (Some(x), Some(y)) = (bounds_of(a), bounds_of(b)) else { continue };
                    if !boxes_meet(&placed_box(&a.t, x), &placed_box(&b.t, y)) {
                        continue;
                    }
                    let (Ok(p), Ok(q)) = (self.part(a.part), self.part(b.part)) else { continue };
                    let question = Question { first: p.shape().clone(), second: q.shape().clone(), pose: b.t.inverse() * a.t };
                    let at = probe.asked.iter().position(|(x, _)| x.same(&question)).unwrap_or_else(|| {
                        let answer = match memo.0.iter().find(|(x, _)| x.same(&question)) {
                            Some((_, shared)) => Answer::Known(*shared),
                            None => Answer::Task(recorder.contact(&question.first, &question.second, question.pose)),
                        };
                        probe.asked.push((question, answer));
                        probe.asked.len() - 1
                    });
                    let (first, second) = if a.source <= b.source { (a.source, b.source) } else { (b.source, a.source) };
                    probe.pairs.push((LinkOverlap { first, second, receiver: *receiver }, at));
                }
            }
        }
        probe.job = recorder.into_job();
        probe
    }

    /// The overlaps, from the kernel's `outputs` for `probe`, and what to remember of them.
    pub fn overlaps_of(&self, probe: &OverlapProbe, outputs: &[Output]) -> (LinkOverlaps, OverlapMemo) {
        let shared: Vec<bool> = probe
            .asked
            .iter()
            .map(|(_, answer)| match answer {
                Answer::Known(shared) => *shared,
                Answer::Task(task) => matches!(outputs.get(*task), Some(Output::Contact(true))),
            })
            .collect();
        let pairs: BTreeSet<LinkOverlap> = probe.pairs.iter().filter(|(_, at)| shared[*at]).map(|(o, _)| *o).collect();
        let mut holding: FastSet<Id> = pairs.iter().flat_map(|o| [o.first.assembly, o.second.assembly]).collect();
        self.spread_holding(&mut holding);
        let memo = probe.asked.iter().zip(shared).map(|((question, _), s)| (question.clone(), s)).collect();
        (LinkOverlaps { pairs, holding }, OverlapMemo(memo))
    }

    /// The name of a link's source as its row shows it: the copy's own, else its component's.
    pub fn link_source_name(&self, source: LinkSource) -> String {
        let Some(o) = self.assembly(source.assembly).ok().and_then(|a| a.occurrence(source.occurrence)) else { return String::new() };
        if o.name.is_empty() { self.component(o.component).map_or_else(|_| String::new(), |c| c.name.clone()) } else { o.name.clone() }
    }
}

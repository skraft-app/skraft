//! Stretching a part or an assembly (docs/ARCHITECTURE.md §6 sexies), after OpenCutList's
//! split and apply workers (`common_stretch_split_worker.rb`, `common_stretch_apply_worker.rb`).
//!
//! The stretch is given in the axes of the component the target occurrence places. Walking
//! down from it, each occurrence is
//! - **moved** when its content lies in sections that move alike: only its placement changes,
//!   its component is not touched; a component locked along the axis (its `Behavior`) is a
//!   block, always moved — with the section holding its origin, even across a cutter;
//! - else **stretched**: a part has its recipe stretched (`stretch_part`), an assembly has its
//!   occurrences walked in turn — each component in its own axes, the stretch brought there
//!   ([`Stretch::in_frame`]). Its **origin follows its anchor**, the section holding the end of
//!   its extent nearest the origin (OCL's `section_def` of a split container): the occurrence
//!   moves as that section, the content by the rest; every occurrence of the component the walk
//!   does not reach moves as much in its own axes (OCL's back translation of extern instances),
//!   showing the same change.
//!
//! What a component becomes is set by what the stretch does to **each of its pieces** — the
//! move of each vertex, edge and face of its parts, of each occurrence moved whole, of each
//! guide (OCL's md5 of section deltas) — along its axis turned the canonical way, from the
//! first piece's move: wherever the cutters fall between the same pieces, and for a copy turned
//! half round or mirrored, the same change. Two occurrences taking the same keep one component
//! — the one that differs only by a move along the axis moved by it; otherwise each change is a
//! component of its own (§2.5). Under
//! "all occurrences" the change most occurrences take is made in place (the occurrences
//! elsewhere show it), the others go to copies — all of them when the tree also reaches the
//! component unchanged (an occurrence only moved, or below one: OCL's `make_unique_c`); under
//! "only this one" the path is made the target's own first, and a change goes to a copy
//! whenever the component shows elsewhere too.

use crate::{FastMap, FastSet};

use glam::{DAffine3, DVec3};
use solidsketch_kernel::Stretch;

#[cfg(doc)]
use crate::document::StretchMask;
use crate::document::{ComponentKind, Document, EntityPath, GuideShape, Id, MAX_DEPTH, Occurrence};
use crate::{Error, Result, Scope};

/// A part stretched by an operation: the part as it is after it (the same, or a copy), the part
/// it was, and the stretch it took in its axes. What a preview draws in its stead.
#[derive(Clone, Debug, PartialEq)]
pub struct StretchedPart {
    pub part: Id,
    pub from: Id,
    pub stretch: Stretch,
}

/// A component's change: the component, and what the stretch does to each piece of it —
/// rounded, its axis made to point the canonical way, taken from the first piece's move (OCL's
/// md5 of section deltas, signed by `_axis_sign`). Two stretches with one key give one shape up
/// to a move along the axis: wherever the cutters fall between the same pieces, and for a copy
/// turned half round or mirrored, stretched alike.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    component: Id,
    numbers: Vec<i64>,
}

/// The stretch within `lo..=hi` along its axis: the cutters outside dropped, with the moves of
/// the sections they bounded.
fn within(s: &Stretch, lo: f64, hi: f64) -> Stretch {
    let (a, b) = (s.section(lo), s.section(hi));
    Stretch::new(s.axis(), s.cuts()[a..b].to_vec(), s.moves()[a..=b].to_vec()).unwrap_or_else(|_| s.clone())
}

/// The stretch `s` with every move less `m`: what it does relative to a section moving by `m`.
fn relative(s: &Stretch, m: f64) -> Stretch {
    Stretch::new(s.axis(), s.cuts().to_vec(), s.moves().iter().map(|x| x - m).collect()).unwrap_or_else(|_| s.clone())
}

/// The move of the anchor of something spanning `lo..=hi` along `s`'s axis, `0` its origin:
/// the move of the section holding the end nearest the origin (OCL's `section_def` of a split
/// container). The origin follows it.
fn anchor_move(s: &Stretch, lo: f64, hi: f64) -> f64 {
    s.move_at(if lo.abs() <= hi.abs() { lo } else { hi })
}

/// Widens `spans[i]` by `lo..=hi` moved by `shift`.
fn add_span(spans: &mut [Option<(f64, f64)>], i: usize, (lo, hi): (f64, f64), shift: f64) {
    let (lo, hi) = (lo + shift, hi + shift);
    spans[i] = Some(spans[i].map_or((lo, hi), |(a, b)| (a.min(lo), b.max(hi))));
}

/// Where the stretch of a whole tree is decided, then made.
struct Walk<'a> {
    scope: Scope,
    /// The document before the stretch: copies are made from it.
    before: &'a Document,
    /// Per change, how many times the tree reaches it (through every occurrence above);
    /// per component, its changes in the order met.
    counts: FastMap<Key, usize>,
    changes: FastMap<Id, Vec<Key>>,
    /// Per component, how many times the whole document places it.
    placed: FastMap<Id, usize>,
    /// The components the tree reaches somewhere unchanged — within an occurrence it only
    /// moves: none of their changes may be made in place (OCL counts moved instances among the
    /// stretched ones, and makes a split unique when they are not all split alike).
    kept: FastSet<Id>,
    /// The component each change went to, and the first move (canonical) of the stretch it took.
    made: FastMap<Key, (Id, f64)>,
    /// Several occurrences stretched: a refusal in a part names it, even at the top.
    several: bool,
    stretched: Vec<StretchedPart>,
    /// The occurrences the walk placed, by the assembly (as it is after) holding them.
    walked: FastSet<(Id, Id)>,
    /// Per component changed in place, how far its origin moved in its axes: its occurrences
    /// the walk did not reach move as much (OCL's back translation of extern instances).
    anchored: FastMap<Id, DVec3>,
}

impl Document {
    /// Is `component` locked along the axis of `s` (in its axes, [`StretchMask::locks_along`])?
    /// Then it is a block: never stretched, moved with the section holding its origin
    /// (ARCHITECTURE.md §6 sexies).
    fn blocked(&self, component: Id, s: &Stretch) -> bool {
        self.component(component).is_ok_and(|c| c.behavior.no_stretch.locks_along(s.axis()))
    }

    /// The move of `component` as a whole by `s` (in its axes), spanning `lo..=hi` along its
    /// axis: a block's, as its origin's section (OCL checks a lock before the bounds); else
    /// when every section it reaches moves alike. `None`: it is stretched.
    fn whole_move(&self, component: Id, s: &Stretch, lo: f64, hi: f64) -> Option<f64> {
        if self.blocked(component, s) { Some(s.move_at(0.0)) } else { s.rigid_move(lo, hi) }
    }

    /// The extent of `component` along `s`'s axis, in its axes (conservative for curved faces);
    /// `None` when it holds nothing.
    fn extent_along(&self, component: Id, s: &Stretch, depth: usize) -> Option<(f64, f64)> {
        if depth > MAX_DEPTH {
            return None;
        }
        match &self.component(component).ok()?.kind {
            ComponentKind::Part(p) => s.extent(p.shape()),
            ComponentKind::Assembly(a) => a
                .occurrences()
                .iter()
                .filter_map(|o| self.placed_extent(o.component, s, &o.transform, depth + 1))
                .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1))),
        }
    }

    /// The extent of `component` placed by `t` in the axes `s` is given in, along its axis.
    fn placed_extent(&self, component: Id, s: &Stretch, t: &DAffine3, depth: usize) -> Option<(f64, f64)> {
        let shift = s.axis().dot(t.translation);
        let (lo, hi) = self.extent_along(component, &s.in_frame(t), depth)?;
        Some((lo + shift, hi + shift))
    }

    /// The occurrences `occurrences` of the assembly `context` leads into, each with its
    /// placement in the axes `frame` places in the context's: what a stretch in those axes
    /// applies to. Refused when there is none, or one is not there.
    fn stretched_occurrences(&self, context: &[Id], occurrences: &[Id], frame: &DAffine3) -> Result<Vec<(Occurrence, DAffine3)>> {
        let a = self.assembly(self.context_of(context)?)?;
        if occurrences.is_empty() {
            return Err(Error::Invalid(crate::Refusal::NothingStretched));
        }
        let to_frame = frame.inverse();
        occurrences
            .iter()
            .map(|id| {
                let o = a.occurrence(*id).ok_or(Error::NotFound(*id))?;
                Ok((o.clone(), to_frame * o.transform))
            })
            .collect()
    }

    /// The bounds of what `component` holds, in its axes (exact for what is flat or round about
    /// an axis, conservative otherwise); `None` when it holds nothing.
    pub(crate) fn component_bounds(&self, component: Id) -> Option<(DVec3, DVec3)> {
        let mut lo = DVec3::ZERO;
        let mut hi = DVec3::ZERO;
        for k in 0..3 {
            let s = Stretch::new(DVec3::AXES[k], Vec::new(), vec![0.0]).ok()?;
            (lo[k], hi[k]) = self.extent_along(component, &s, 0)?;
        }
        Some((lo, hi))
    }

    /// The bounds of what the occurrences `occurrences` of the context `context` place, in the
    /// axes `frame` places in the context's (exact for what is flat or round about an axis,
    /// conservative otherwise): the box a stretch works on.
    pub fn stretch_bounds(&self, context: &[Id], occurrences: &[Id], frame: &DAffine3) -> Result<Option<(DVec3, DVec3)>> {
        let occs = self.stretched_occurrences(context, occurrences, frame)?;
        let mut lo = DVec3::ZERO;
        let mut hi = DVec3::ZERO;
        for k in 0..3 {
            let s = Stretch::new(DVec3::AXES[k], Vec::new(), vec![0.0]).map_err(solidsketch_kernel::Error::from)?;
            let extent =
                occs.iter().filter_map(|(o, t)| self.placed_extent(o.component, &s, t, 0)).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)));
            let Some((a, b)) = extent else { return Ok(None) };
            (lo[k], hi[k]) = (a, b);
        }
        Ok(Some((lo, hi)))
    }

    /// Per section of the cutters `cuts` along `axis` (in the axes `frame` places in the
    /// context's), the extent along the axis of the matter wholly in it, over the occurrences
    /// `occurrences` of the context — what a compression must keep apart (OpenCutList's section
    /// bounds): an occurrence wholly in a section counts whole, a part across a cutter by its
    /// edges in one section ([`Stretch::section_spans`]).
    pub fn stretch_spans(
        &self,
        context: &[Id],
        occurrences: &[Id],
        frame: &DAffine3,
        axis: DVec3,
        cuts: &[f64],
    ) -> Result<Vec<Option<(f64, f64)>>> {
        let occs = self.stretched_occurrences(context, occurrences, frame)?;
        let s = Stretch::new(axis, cuts.to_vec(), vec![0.0; cuts.len() + 1]).map_err(solidsketch_kernel::Error::from)?;
        let mut spans = vec![None; cuts.len() + 1];
        self.collect_occurrence_spans(occs.iter().map(|(o, t)| (o.component, *t)), &s, 0.0, &mut spans, 0);
        Ok(spans)
    }

    /// Adds to `spans` (along the target's axis) what `component` holds, `s` in its axes and
    /// `shift` from its axes' positions to the target's.
    fn collect_spans(&self, component: Id, s: &Stretch, shift: f64, spans: &mut [Option<(f64, f64)>], depth: usize) {
        let Ok(c) = self.component(component) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        match &c.kind {
            ComponentKind::Part(p) => {
                for (i, span) in s.section_spans(p.shape()).into_iter().enumerate() {
                    if let Some(span) = span {
                        add_span(spans, i, span, shift);
                    }
                }
            }
            ComponentKind::Assembly(a) => {
                self.collect_occurrence_spans(a.occurrences().iter().map(|o| (o.component, o.transform)), s, shift, spans, depth + 1);
            }
        }
    }

    /// [`Document::collect_spans`] for components placed in `s`'s axes: whole in a section, or
    /// by what they hold.
    fn collect_occurrence_spans(
        &self,
        placed: impl Iterator<Item = (Id, DAffine3)>,
        s: &Stretch,
        shift: f64,
        spans: &mut [Option<(f64, f64)>],
        depth: usize,
    ) {
        for (component, t) in placed {
            let local = s.in_frame(&t);
            let at = s.axis().dot(t.translation);
            let Some((lo, hi)) = self.extent_along(component, &local, depth) else { continue };
            let (i, j) = (local.section(lo), local.section(hi));
            if self.blocked(component, &local) {
                // A block: in the section of its origin, all of it (OCL adds a locked
                // container's bounds to that section's).
                add_span(spans, local.section(0.0), (lo, hi), shift + at);
            } else if i == j {
                add_span(spans, i, (lo, hi), shift + at);
            } else {
                self.collect_spans(component, &local, shift + at, spans, depth);
            }
        }
    }

    /// Stretches what the occurrences `occurrences` of the context `context` place by
    /// `stretch`, given in the axes `frame` places in the context's; the scope deciding what
    /// else shows it (module doc). The context's own component is changed in place. Returns the
    /// parts stretched.
    pub(crate) fn stretch_tree(
        &mut self,
        context: &[Id],
        occurrences: &[Id],
        frame: &DAffine3,
        stretch: &Stretch,
        scope: Scope,
        stretch_part: &mut dyn FnMut(&mut Document, Id, &Stretch) -> Result<()>,
    ) -> Result<Vec<StretchedPart>> {
        self.stretched_occurrences(context, occurrences, frame)?;
        if !crate::ops::is_rigid(frame) {
            return Err(Error::Invalid(crate::Refusal::NotRigid));
        }
        // Only these: the context made its own once (OpenCutList's make unique); below, a change
        // goes to a copy whenever its component shows elsewhere too.
        if scope == Scope::This
            && let Some(path) = EntityPath::from_chain(context)
        {
            self.separate(&path)?;
        }
        let at = self.context_of(context)?;
        let before = self.clone();
        let occs = before.stretched_occurrences(context, occurrences, frame)?;
        let mut walk = Walk {
            scope,
            before: &before,
            counts: FastMap::default(),
            changes: FastMap::default(),
            placed: FastMap::default(),
            kept: FastSet::default(),
            made: FastMap::default(),
            several: occs.len() > 1,
            stretched: Vec::new(),
            walked: FastSet::default(),
            anchored: FastMap::default(),
        };
        before.count_placed(before.root, &mut walk.placed);
        walk.count_occurrences(&occs, stretch, 0);
        self.make_occurrences(&mut walk, at, &occs, stretch, frame, stretch_part, 0)?;
        self.move_unreached(&walk)?;
        Ok(walk.stretched)
    }

    /// Moves the occurrences the walk did not reach of each component changed in place by how
    /// far its origin moved: they show the same change in their own axes.
    fn move_unreached(&mut self, walk: &Walk<'_>) -> Result<()> {
        for (c, v) in &walk.anchored {
            let holders: Vec<(Id, Vec<Id>)> = self
                .components
                .values()
                .filter_map(|x| {
                    let a = x.as_assembly()?;
                    let ids: Vec<Id> = a
                        .occurrences()
                        .iter()
                        .filter(|o| o.component == *c && !walk.walked.contains(&(x.id, o.id)))
                        .map(|o| o.id)
                        .collect();
                    (!ids.is_empty()).then_some((x.id, ids))
                })
                .collect();
            for (holder, ids) in holders {
                let a = self.assembly_mut(holder)?;
                for id in ids {
                    if let Some(o) = a.occurrence_mut(id) {
                        o.transform *= DAffine3::from_translation(*v);
                    }
                }
            }
        }
        Ok(())
    }

    /// The document as the stretch would leave it, without its geometry: occurrences moved,
    /// components copied as the scope says, but no part changed — with the parts that would be,
    /// and how. What a preview draws, each stretched part from its former mesh deformed.
    #[allow(clippy::too_many_arguments)]
    pub fn stretch_sketch(
        &self,
        context: &[Id],
        occurrences: &[Id],
        frame: &DAffine3,
        axis: DVec3,
        cuts: &[f64],
        moves: &[f64],
        scope: Scope,
    ) -> Result<(Document, Vec<StretchedPart>)> {
        let s = Stretch::new(axis, cuts.to_vec(), moves.to_vec()).map_err(solidsketch_kernel::Error::from)?;
        let mut doc = self.clone();
        let parts = doc.stretch_tree(context, occurrences, frame, &s, scope, &mut |_, _, _| Ok(()))?;
        Ok((doc, parts))
    }

    /// The key of the change of `c` by the stretch `s` (in its axes), the axis it is signed by,
    /// and the first piece's move along it: what the stretch adds to the key's own — a move
    /// along that axis.
    fn signature(&self, c: Id, s: &Stretch, depth: usize) -> (Key, DVec3, f64) {
        // Along the axis pointing the canonical way: the same stretch, turned over.
        let a = s.axis();
        let flip = [a.x, a.y, a.z].into_iter().find(|x| x.abs() > 1e-6).is_some_and(|x| x < 0.0);
        let turned = flip.then(|| {
            let cuts = s.cuts().iter().rev().map(|c| -c).collect();
            let moves = s.moves().iter().rev().map(|m| -m).collect();
            Stretch::new(-a, cuts, moves).unwrap_or_else(|_| s.clone())
        });
        let s = turned.as_ref().unwrap_or(s);
        let mut moves = Vec::new();
        self.piece_moves(c, s, &mut moves, depth);
        let first = moves.first().copied().unwrap_or(0.0);
        let round = |x: f64| (x * 1e6).round() as i64;
        let axis = s.axis();
        let numbers = [axis.x, axis.y, axis.z].into_iter().chain(moves.iter().map(|m| m - first)).map(round).collect();
        (Key { component: c, numbers }, axis, first)
    }

    /// Adds to `out` what the stretch `s` (in `c`'s axes) does to each piece of `c`, as moves
    /// along its axis: a part's blank, its machinings' tools, its guides; an assembly's
    /// occurrences — moved whole, or their own pieces — and its guides.
    fn piece_moves(&self, c: Id, s: &Stretch, out: &mut Vec<f64>, depth: usize) {
        let Ok(component) = self.component(c) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        match &component.kind {
            ComponentKind::Part(p) => {
                out.extend(s.piece_moves(&p.blank));
                for m in &p.machinings {
                    out.extend(s.in_frame(&m.placement).piece_moves(&m.tool));
                }
            }
            ComponentKind::Assembly(a) => {
                for o in a.occurrences() {
                    let local = s.in_frame(&o.transform);
                    let Some((lo, hi)) = self.extent_along(o.component, &local, depth + 1) else { continue };
                    match self.whole_move(o.component, &local, lo, hi) {
                        Some(m) => out.push(m),
                        None => self.piece_moves(o.component, &within(&local, lo, hi), out, depth + 1),
                    }
                }
            }
        }
        let a = s.axis();
        for g in &component.guides {
            match g.shape {
                GuideShape::Line { origin, dir } if dir.dot(a).abs() < 1e-9 => out.push(s.move_at(a.dot(origin))),
                GuideShape::Circle { center, normal, .. } if normal.cross(a).length() < 1e-9 => out.push(s.move_at(a.dot(center))),
                _ => {}
            }
        }
    }

    /// How many times the document places each component below `component` (every path):
    /// worked out over the distinct assemblies (each one's contents counted once, in runs),
    /// then carried down from `component` in topological order — not by visiting every
    /// placement.
    pub(crate) fn count_placed(&self, component: Id, placed: &mut FastMap<Id, usize>) {
        // Each assembly reached: its components, with how many occurrences of each.
        let mut direct: FastMap<Id, Vec<(Id, usize)>> = FastMap::default();
        // Reverse post-order of the depth-first walk: holders before what they hold.
        let mut order = Vec::new();
        self.gather_contents(component, &mut direct, &mut order, 0);
        let mut times: FastMap<Id, usize> = FastMap::from_iter([(component, 1)]);
        for a in order.iter().rev() {
            let n = times.get(a).copied().unwrap_or(0);
            for (c, k) in &direct[a] {
                *times.entry(*c).or_default() += n * k;
                *placed.entry(*c).or_default() += n * k;
            }
        }
    }

    fn gather_contents(&self, assembly: Id, direct: &mut FastMap<Id, Vec<(Id, usize)>>, order: &mut Vec<Id>, depth: usize) {
        if depth > MAX_DEPTH || direct.contains_key(&assembly) {
            return;
        }
        let Ok(a) = self.assembly(assembly) else { return };
        // Runs of the same component counted before one lookup each.
        let mut at: FastMap<Id, usize> = FastMap::default();
        let mut counts: Vec<(Id, usize)> = Vec::new();
        let mut occs = a.occurrences().iter().peekable();
        while let Some(o) = occs.next() {
            let mut run = 1;
            while occs.next_if(|n| n.component == o.component).is_some() {
                run += 1;
            }
            match at.get(&o.component) {
                Some(i) => counts[*i].1 += run,
                None => {
                    at.insert(o.component, counts.len());
                    counts.push((o.component, run));
                }
            }
        }
        direct.insert(assembly, counts.clone());
        for (c, _) in &counts {
            if matches!(self.component(*c).map(|x| &x.kind), Ok(ComponentKind::Assembly(_))) {
                self.gather_contents(*c, direct, order, depth + 1);
            }
        }
        order.push(assembly);
    }

    /// Makes the change of `c` by the stretch `s` (in its axes, relative to its anchor, which
    /// moves by `anchor`): in place, or in a copy (module doc). Returns the component that holds
    /// it, and how far (in its axes) an occurrence of it must move to show `s` — its anchor's
    /// move, and the rest when the change was made by a stretch alike up to a move.
    #[allow(clippy::too_many_arguments)]
    fn make(
        &mut self,
        walk: &mut Walk<'_>,
        c: Id,
        s: &Stretch,
        anchor: f64,
        stretch_part: &mut dyn FnMut(&mut Document, Id, &Stretch) -> Result<()>,
        depth: usize,
    ) -> Result<(Id, DVec3)> {
        let to_anchor = s.axis() * anchor;
        let (key, axis, first) = walk.before.signature(c, s, depth);
        if let Some((id, made_first)) = walk.made.get(&key) {
            return Ok((*id, to_anchor + axis * (first - made_first)));
        }
        let changes = walk.changes.get(&c).cloned().unwrap_or_default();
        // The change most of the tree's occurrences take (the first met on a tie).
        let main = changes.iter().max_by_key(|k| (walk.counts[*k], std::cmp::Reverse(changes.iter().position(|x| x == *k)))).cloned();
        let reached: usize = changes.iter().map(|k| walk.counts[k]).sum();
        let elsewhere = walk.placed.get(&c).copied().unwrap_or(0) > reached;
        let in_place = main.as_ref() == Some(&key) && !walk.kept.contains(&c) && (walk.scope == Scope::All || !elsewhere);
        let id = if in_place { c } else { self.copy_from(walk.before, c)? };
        walk.made.insert(key, (id, first));
        if in_place && anchor.abs() > 1e-12 {
            walk.anchored.insert(c, to_anchor);
        }
        match &walk.before.component(c)?.kind {
            ComponentKind::Part(_) => {
                // Below the target, the refusal names the part.
                // Below the top, or with several stretched, the refusal names the part.
                stretch_part(self, id, s).map_err(|e| match depth {
                    0 if !walk.several => e,
                    _ => Error::InPart { part: walk.before.component(c).map(|c| c.name.clone()).unwrap_or_default(), error: Box::new(e) },
                })?;
                walk.stretched.push(StretchedPart { part: id, from: c, stretch: s.clone() });
            }
            ComponentKind::Assembly(a) => {
                let occs: Vec<_> = a.occurrences().iter().map(|o| (o.clone(), o.transform)).collect();
                self.make_occurrences(walk, id, &occs, s, &DAffine3::IDENTITY, stretch_part, depth + 1)?;
                self.stretch_guides(id, s)?;
            }
        }
        Ok((id, to_anchor))
    }

    /// Stretches the occurrences `occs` of the assembly `id` (each with its placement in the
    /// axes of `s`, which `frame` places in the assembly's): moved when in sections that move
    /// alike, else their component made by [`Document::make`].
    #[allow(clippy::too_many_arguments)]
    fn make_occurrences(
        &mut self,
        walk: &mut Walk<'_>,
        id: Id,
        occs: &[(Occurrence, DAffine3)],
        s: &Stretch,
        frame: &DAffine3,
        stretch_part: &mut dyn FnMut(&mut Document, Id, &Stretch) -> Result<()>,
        depth: usize,
    ) -> Result<()> {
        for (o, t) in occs {
            let local = s.in_frame(t);
            let Some((lo, hi)) = walk.before.extent_along(o.component, &local, depth) else { continue };
            let (transform, component) = match walk.before.whole_move(o.component, &local, lo, hi) {
                Some(m) => (*frame * DAffine3::from_translation(s.axis() * m) * *t, o.component),
                None => {
                    // The origin follows its anchor; the content moves relative to it.
                    let local = within(&local, lo, hi);
                    let anchor = anchor_move(&local, lo, hi);
                    let (child, v) = self.make(walk, o.component, &relative(&local, anchor), anchor, stretch_part, depth)?;
                    (o.transform * DAffine3::from_translation(v), child)
                }
            };
            walk.walked.insert((id, o.id));
            if let Some(occ) = self.assembly_mut(id)?.occurrence_mut(o.id) {
                (occ.transform, occ.component) = (transform, component);
            }
        }
        Ok(())
    }

    /// A copy of the component `c` as it is in `before` (ids of its occurrences, machinings and
    /// guides kept, §2.6, and its behavior), under a name of its own.
    fn copy_from(&mut self, before: &Document, c: Id) -> Result<Id> {
        let src = before.component(c)?.clone();
        let copy = self.insert_component(&src.name, src.named, src.kind);
        let c = self.component_mut(copy)?;
        (c.guides, c.behavior) = (src.guides, src.behavior);
        Ok(copy)
    }

    /// The guides of `component` follow their section: a line square to the axis or a circle
    /// about it; a line along it stays, an oblique one has no section of its own.
    pub(crate) fn stretch_guides(&mut self, component: Id, s: &Stretch) -> Result<()> {
        let a = s.axis();
        for g in &mut self.component_mut(component)?.guides {
            let at = match g.shape {
                GuideShape::Line { origin, dir } if dir.dot(a).abs() < 1e-9 => Some(origin),
                GuideShape::Circle { center, normal, .. } if normal.cross(a).length() < 1e-9 => Some(center),
                _ => None,
            };
            if let Some(at) = at {
                g.shape = g.shape.transformed(&DAffine3::from_translation(a * s.move_at(a.dot(at))));
            }
        }
        Ok(())
    }
}

impl Walk<'_> {
    /// Counts the change of `c` by the stretch `s` (in its axes) and those below it, once per
    /// time the tree reaches them.
    fn count(&mut self, c: Id, s: &Stretch, depth: usize) {
        let key = self.before.signature(c, s, depth).0;
        *self.counts.entry(key.clone()).or_default() += 1;
        let changes = self.changes.entry(c).or_default();
        if !changes.contains(&key) {
            changes.push(key);
        }
        let Ok(a) = self.before.assembly(c) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        let occs: Vec<_> = a.occurrences().iter().map(|o| (o.clone(), o.transform)).collect();
        self.count_occurrences(&occs, s, depth + 1);
    }

    /// Counts the changes of the occurrences `occs` (each with its placement in `s`'s axes)
    /// that the stretch does not only move.
    fn count_occurrences(&mut self, occs: &[(Occurrence, DAffine3)], s: &Stretch, depth: usize) {
        for (o, t) in occs {
            let local = s.in_frame(t);
            let Some((lo, hi)) = self.before.extent_along(o.component, &local, depth) else { continue };
            if self.before.whole_move(o.component, &local, lo, hi).is_none() {
                self.count(o.component, &within(&local, lo, hi), depth);
            } else {
                self.keep(o.component, depth);
            }
        }
    }

    /// Marks `c` and all it holds as reached unchanged.
    fn keep(&mut self, c: Id, depth: usize) {
        if depth > MAX_DEPTH || !self.kept.insert(c) {
            return;
        }
        let Ok(a) = self.before.assembly(c) else { return };
        let below: Vec<Id> = a.occurrences().iter().map(|o| o.component).collect();
        for b in below {
            self.keep(b, depth + 1);
        }
    }
}

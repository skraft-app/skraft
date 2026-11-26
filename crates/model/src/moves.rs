//! Bodies moved in a part's tree (docs/model/bodies.md#moving-bodies, decision 0044): from a
//! level of the part — its root, a group or a body node — to another, or out of the part as a
//! new one. The world place is kept. Inside the part a body keeps its id and its definition,
//! and the names of the body nodes above both levels follow it (`[g1, b]` becomes `[g2, b]`);
//! a node it leaves loses the edges it took on it (in error, said). A group shared by several
//! copies, left or entered, is made unique first. A body keeps what it does: its sign is
//! changed when the signs on its way differ (out of a machining group, into one).

use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::sync::Arc;

use glam::DAffine3;

use crate::document::{Body, BodyDef, Document, EntityPath, Id, Occurrence, Part, Sign};
use crate::ops::{BodiesTo, Operation, Outcome, Scope, Solver, Warning, note_refused, relocate};
use crate::{Error, Refusal, Result};

/// The bodies of the level `path` of `p` (the bodies leading to a group from the part's root;
/// empty: its root).
fn level<'a>(p: &'a Part, path: &[Id]) -> Result<&'a [Body]> {
    match path.last() {
        None => Ok(&p.bodies),
        Some(_) => {
            let (chain, _) = p.group_chain(path)?;
            let def = *chain.last().expect("a group path is not empty");
            Ok(p.def(def).map_or(&[][..], |d| d.children()))
        }
    }
}

/// [`level`], to change; its failures with it.
fn level_mut<'a>(p: &'a mut Part, path: &[Id]) -> Result<(&'a mut Vec<Body>, &'a mut BTreeSet<Id>)> {
    match path.last() {
        None => Ok((&mut p.bodies, &mut p.failed)),
        Some(_) => {
            let (chain, _) = p.group_chain(path)?;
            let def = *chain.last().expect("a group path is not empty");
            let g = p.defs.iter_mut().find(|d| d.id == def).and_then(|d| d.group.as_mut()).ok_or(Error::NotFound(def))?;
            Ok((&mut g.children, &mut g.failed))
        }
    }
}

/// What the level `path` does to the part: adds (positive) or takes away (negative), the signs
/// of the groups on its way multiplied.
fn effective(p: &Part, path: &[Id]) -> Result<Sign> {
    let mut sign = Sign::Positive;
    let mut bodies: &[Body] = &p.bodies;
    for id in path {
        let b = bodies.iter().find(|b| b.id == *id).ok_or(Error::NotFound(*id))?;
        if b.sign == Sign::Negative {
            sign = flip(sign);
        }
        bodies = p.def_of(b).children();
    }
    Ok(sign)
}

/// The other of adding and taking away; an intersection stays one, it limits wherever it goes
/// (docs/decisions/0045-intersection-a-third-sign.md).
fn flip(s: Sign) -> Sign {
    match s {
        Sign::Positive => Sign::Negative,
        Sign::Negative => Sign::Positive,
        Sign::Intersect => Sign::Intersect,
    }
}

/// The groups `path` goes through, from the part's root (each a path, the shortest first).
fn prefixes(path: &[Id]) -> impl Iterator<Item = Vec<Id>> + '_ {
    (1..=path.len()).map(|n| path[..n].to_vec())
}

/// The body nodes of `p` in error, by definition.
fn refused(p: &Part) -> BTreeSet<Id> {
    p.defs.iter().filter(|d| d.group.as_ref().is_some_and(|g| g.refused.is_some())).map(|d| d.id).collect()
}

impl Document {
    /// [`Operation::MoveBodies`].
    pub(crate) fn move_bodies(
        &mut self,
        from: &EntityPath,
        bodies: &[Id],
        to: &BodiesTo,
        copy: bool,
        scope: Scope,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        if bodies.is_empty() {
            return Err(Error::Invalid(Refusal::NoBodies));
        }
        self.resolve_part(from)?;
        if scope == Scope::This {
            self.separate(from)?;
        }
        let (part, p, _) = self.resolve_part(from)?;
        let here = level(p, &from.group)?;
        if let Some(b) = bodies.iter().find(|b| !here.iter().any(|h| h.id == **b)) {
            return Err(Error::NotFound(*b));
        }
        let before = refused(p);
        let mut outcome = match to {
            BodiesTo::Level(to) => self.move_in_part(part, from, bodies, to, copy, solver)?,
            BodiesTo::NewPart(occurrences) => self.move_to_new_part(from, bodies, occurrences, copy, scope, solver)?,
            // Another occurrence of the same part: its level.
            BodiesTo::Part(to) if self.resolve_part(to)?.0 == part => self.move_in_part(part, from, bodies, &to.group, copy, solver)?,
            BodiesTo::Part(to) => self.move_to_part(from, bodies, to, copy, scope, solver)?,
        };
        // The nodes it left in error, said.
        let p = self.part(part)?;
        let in_error: Vec<String> =
            p.defs.iter().filter(|d| !before.contains(&d.id) && refused(p).contains(&d.id)).map(|d| d.name.clone()).collect();
        if let Some(Warning::Moved { in_error: e, .. }) = &mut outcome.warning {
            *e = in_error;
        } else if !in_error.is_empty() {
            outcome.warning = Some(Warning::Moved { made_unique: Vec::new(), in_error });
        }
        if matches!(&outcome.warning, Some(Warning::Moved { made_unique, in_error }) if made_unique.is_empty() && in_error.is_empty()) {
            outcome.warning = None;
        }
        Ok(outcome)
    }

    /// The group the bodies `path` leads to, made its own when its definition is shared by
    /// several bodies of `part`: its children's ids, old → new; none when it is not shared (or
    /// `path` is the root).
    fn own_group(&mut self, part: Id, path: &[Id]) -> Result<Option<Vec<(Id, Id)>>> {
        let Some(&g) = path.last() else { return Ok(None) };
        let p = self.part(part)?;
        let def = p.any_body(g).ok_or(Error::NotFound(g))?.def;
        if p.copies_of(def) < 2 {
            return Ok(None);
        }
        let d = p.def(def).cloned().ok_or(Error::NotFound(def))?;
        let fresh = self.fresh_id();
        let (copy, renamed) = self.renamed_children_map(BodyDef { id: fresh, ..d });
        let p = self.part_mut(part)?;
        if let Some(b) = p.any_body_mut(g) {
            b.def = copy.id;
        }
        p.defs.push(copy);
        Ok(Some(renamed))
    }

    /// The move to another level of the same part.
    fn move_in_part(
        &mut self,
        part: Id,
        from: &EntityPath,
        bodies: &[Id],
        to: &[Id],
        copy: bool,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        let (mut src, mut dst, mut bodies) = (from.group.clone(), to.to_vec(), bodies.to_vec());
        if src == dst {
            return Ok(Outcome::default());
        }
        // Into one of the bodies moved: into itself.
        if dst.iter().any(|g| bodies.contains(g)) {
            return Err(Error::Cycle);
        }
        self.part(part)?.group_chain(&dst)?;
        // Paths from the part's root, old → new: what the names of the nodes on the way follow.
        let mut relocated: Vec<(Vec<Id>, Vec<Id>)> = Vec::new();
        let mut made_unique = Vec::new();
        for leaving in [true, false] {
            let path = if leaving { src.clone() } else { dst.clone() };
            let Some(renamed) = self.own_group(part, &path)? else { continue };
            made_unique.push(*path.last().expect("a shared group"));
            let n = path.len();
            let follow = |q: &mut Vec<Id>| {
                if q.len() > n
                    && q[..n] == path[..]
                    && let Some((_, new)) = renamed.iter().find(|(old, _)| *old == q[n])
                {
                    q[n] = *new;
                }
            };
            follow(&mut src);
            follow(&mut dst);
            if leaving {
                for b in &mut bodies {
                    if let Some((_, new)) = renamed.iter().find(|(old, _)| old == b) {
                        *b = *new;
                    }
                }
            }
            relocated.extend(
                renamed
                    .iter()
                    .map(|&(old, new)| (path.iter().copied().chain([old]).collect(), path.iter().copied().chain([new]).collect())),
            );
        }
        // Placed in the axes of where they go, each keeping what it does.
        let p = self.part(part)?;
        let ((_, src_t), (_, dst_t)) = (p.group_chain(&src)?, p.group_chain(&dst)?);
        let into: DAffine3 = dst_t.inverse() * src_t;
        let turn = effective(p, &src)? != effective(p, &dst)?;
        let moving: Vec<Body> = level(p, &src)?.iter().filter(|b| bodies.contains(&b.id)).cloned().collect();
        let mut placed = Vec::with_capacity(moving.len());
        for b in moving {
            let id = if copy { self.fresh_id() } else { b.id };
            let sign = if turn { flip(b.sign) } else { b.sign };
            placed.push(Body { id, sign, placement: into * b.placement, ..b });
        }
        let p = self.part_mut(part)?;
        if !copy {
            let (children, failed) = level_mut(p, &src)?;
            children.retain(|b| !bodies.contains(&b.id));
            failed.retain(|f| !bodies.contains(f));
            relocated.extend(bodies.iter().map(|&b| (src.iter().copied().chain([b]).collect(), dst.iter().copied().chain([b]).collect())));
        }
        level_mut(p, &dst)?.0.extend(placed.iter().cloned());
        // The names of every group on the way of either level follow the bodies that stay under
        // it; a node a body left keeps its names, lost (in error).
        let mut levels: Vec<Vec<Id>> = prefixes(&src).chain(prefixes(&dst)).collect();
        levels.sort();
        levels.dedup();
        for at in &levels {
            let moves: Vec<(Vec<Id>, Vec<Id>)> = relocated
                .iter()
                .filter(|(old, new)| old.starts_with(at) && new.starts_with(at))
                .map(|(old, new)| (old[at.len()..].to_vec(), new[at.len()..].to_vec()))
                .collect();
            if moves.is_empty() {
                continue;
            }
            let (chain, _) = p.group_chain(at)?;
            let def = *chain.last().expect("a group path is not empty");
            if let Some(g) = p.defs.iter_mut().find(|d| d.id == def).and_then(|d| d.group.as_mut()) {
                relocate(g, &[], &moves);
            }
        }
        // The groups on the way computed again, the deepest first, then the part.
        levels.sort_by_key(|l| Reverse(l.len()));
        for at in &levels {
            let p = self.part(part)?;
            let (chain, _) = p.group_chain(at)?;
            let def = *chain.last().expect("a group path is not empty");
            let Some(group) = p.def(def).and_then(|d| d.group.clone()) else { continue };
            let defs = p.defs.clone();
            let (solid, group, inner) = Self::computed_group(&defs, group, &[], solver)?;
            let p = self.part_mut(part)?;
            note_refused(&mut p.defs, inner);
            if let Some(d) = p.defs.iter_mut().find(|d| d.id == def) {
                d.solid = solid;
                d.group = Some(group);
            }
        }
        let p = self.part_mut(part)?;
        p.drop_unused_defs();
        self.recompute(part, None, solver)?;
        let p = self.part(part)?;
        let made_unique: Vec<String> = made_unique.iter().filter_map(|g| p.any_body(*g)).map(|b| p.def_of(b).name.clone()).collect();
        Ok(Outcome {
            bodies: placed.iter().map(|b| b.id).collect(),
            warning: Some(Warning::Moved { made_unique, in_error: Vec::new() }),
            ..Outcome::default()
        })
    }

    /// The move out of the part: a new part in the assembly `occurrences` leads to (the part's
    /// own: « Sortir de la pièce »), as bodies pasted out of a part (docs/tools/copy-paste.md),
    /// then those left taken away.
    fn move_to_new_part(
        &mut self,
        from: &EntityPath,
        bodies: &[Id],
        occurrences: &[Id],
        copy: bool,
        scope: Scope,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        let (_, p, _) = self.resolve_part(from)?;
        let turn = effective(p, &from.group)? == Sign::Negative;
        let mut fragment = self.bodies_fragment(from, bodies)?;
        if turn {
            fragment.bodies.iter_mut().for_each(|b| b.sign = flip(b.sign));
        }
        let context = self.context_of(occurrences)?;
        let transform = self.transform_of(occurrences)?.inverse();
        let outcome = self.apply_in_place(&Operation::PasteAsPart { context, fragment: Arc::new(fragment), transform }, solver)?;
        if !copy {
            self.apply_in_place(&Operation::DeleteBodies { target: from.clone(), bodies: bodies.to_vec(), scope }, solver)?;
        }
        Ok(outcome)
    }

    /// The move into another part: bodies pasted into it, at the level `to` names, each keeping
    /// what it does there; then taken away from where they were.
    fn move_to_part(
        &mut self,
        from: &EntityPath,
        bodies: &[Id],
        to: &EntityPath,
        copy: bool,
        scope: Scope,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        let (_, p, _) = self.resolve_part(from)?;
        let src = effective(p, &from.group)?;
        let (_, q, _) = self.resolve_part(to)?;
        let dst = effective(q, &to.group)?;
        let mut fragment = self.bodies_fragment(from, bodies)?;
        if src != dst {
            fragment.bodies.iter_mut().for_each(|b| b.sign = flip(b.sign));
        }
        let (_, _, world) = self.resolve_container(to)?;
        let transform = world.inverse() * fragment.world;
        let outcome = self.apply_in_place(
            &Operation::PasteBodies { target: to.clone(), fragment: Arc::new(fragment), transform, scope: Scope::All },
            solver,
        )?;
        if !copy {
            self.apply_in_place(&Operation::DeleteBodies { target: from.clone(), bodies: bodies.to_vec(), scope }, solver)?;
        }
        Ok(outcome)
    }

    /// [`Operation::MoveParts`]: the occurrences pasted into the part as bodies (positive, a
    /// part keeps adding; negative where the level takes away), then taken away from their
    /// assembly. A part into itself, or into a part an assembly moved holds, is refused.
    pub(crate) fn move_parts(
        &mut self,
        from: &[Id],
        occurrences: &[Id],
        to: &EntityPath,
        copy: bool,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        if occurrences.is_empty() {
            return Err(Error::Invalid(Refusal::NoBodies));
        }
        let context = self.context_of(from)?;
        let moved: Vec<Id> = self.occurrences_of(context, occurrences)?.iter().map(|o| o.component).collect();
        let (part, q, _) = self.resolve_part(to)?;
        // The target itself moved, or inside an assembly moved.
        let n = from.len();
        let inside = |o: &Id| to.occurrences.len() > n && to.occurrences[..n] == *from && to.occurrences[n] == *o;
        if moved.contains(&part) || occurrences.iter().any(inside) || (to.occurrences == from && occurrences.contains(&to.entity)) {
            return Err(Error::Cycle);
        }
        let negative = effective(q, &to.group)? == Sign::Negative;
        let fragment = self.entities_fragment(from, occurrences)?;
        let (_, _, world) = self.resolve_container(to)?;
        let outcome = self.apply_in_place(
            &Operation::PasteParts { target: to.clone(), fragment: Arc::new(fragment), transform: world.inverse(), scope: Scope::All },
            solver,
        )?;
        if negative && !outcome.bodies.is_empty() {
            let set = Operation::SetSign { target: to.clone(), bodies: outcome.bodies.clone(), sign: Sign::Negative, scope: Scope::All };
            self.apply_in_place(&set, solver)?;
        }
        if !copy {
            self.apply_in_place(&Operation::Delete { context, entities: occurrences.to_vec() }, solver)?;
        }
        Ok(outcome)
    }

    /// [`Operation::MoveOccurrences`]: out of their assembly into another, their world place
    /// kept; that assembly's definition changes (every copy of it). Nothing to do into the
    /// assembly they lie in.
    pub(crate) fn move_occurrences(&mut self, from: &[Id], occurrences: &[Id], to: &[Id], copy: bool) -> Result<Outcome> {
        if occurrences.is_empty() {
            return Err(Error::Invalid(Refusal::NoBodies));
        }
        let (source, into) = (self.context_of(from)?, self.context_of(to)?);
        let moving: Vec<Occurrence> = self.occurrences_of(source, occurrences)?;
        if source == into {
            return Ok(Outcome::default());
        }
        // Into one of them, or an assembly one of them holds.
        if moving.iter().any(|o| self.contains_component(o.component, into)) {
            return Err(Error::Cycle);
        }
        let carry: DAffine3 = self.transform_of(to)?.inverse() * self.transform_of(from)?;
        let mut placed = Vec::with_capacity(moving.len());
        for o in moving {
            let id = if copy { self.fresh_id() } else { o.id };
            placed.push(Occurrence { id, transform: carry * o.transform, ..o });
        }
        if !copy {
            self.assembly_mut(source)?.occurrences_mut().retain(|o| !occurrences.contains(&o.id));
        }
        let created: Vec<Id> = placed.iter().map(|o| o.id).collect();
        self.assembly_mut(into)?.occurrences_mut().extend(placed);
        if !copy {
            self.links_moved(from, occurrences, to)?;
        }
        Ok(Outcome { created, ..Outcome::default() })
    }
}

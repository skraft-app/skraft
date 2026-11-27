//! Booleans between parts (docs/model/bodies.md#booleans-between-parts): targets and tools,
//! several of each as OpenCutList's solid tools (`common_solid_boolean_apply_worker.rb`), in one
//! operation.
//!
//! - Each target takes the tools that **reach** it: those whose bounds meet its own (a box test
//!   on the record, conservative, no kernel). A union joins every tool to its single target.
//! - **Scope** (docs/model/data-model.md#copies-and-propagation): targets of one part that the
//!   operation leaves identical in their own axes (the same tools, placed alike) keep one part;
//!   those that diverge get parts of their own, the change most of them take made in place —
//!   under « only this one », only when the targets are every placement of their part.
//! - **Keep the targets**: each result goes to a new part beside its target, left as it was.
//!   **Keep the tools**: their occurrences stay. A tool that reaches no target always stays.
//! - **Drop unused**: the tools' parts left without occurrence go, even named; without it they
//!   stay, unused, until purged (docs/ui/components-library.md).

use glam::{DAffine3, DVec3};
use skraft_kernel::Stretch;

use crate::document::{Body, BodyDef, Document, EntityPath, Id, Occurrence, Part, Sign, Visibility};
use crate::ops::Solver;
use crate::{Boolean, Error, FastMap, Outcome, Refusal, Result, Scope};

/// What a boolean keeps (its options, docs/tools/solid-tools.md).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Keep {
    pub targets: bool,
    pub tools: bool,
    pub drop_unused: bool,
}

/// A tool as it was before the operation changed anything.
struct ToolPart {
    path: EntityPath,
    part: Id,
    q: Part,
    /// Part to world.
    world: DAffine3,
    /// The part's name, when the user gave it (it names the body it becomes).
    named: Option<String>,
    /// Its occurrence's name, the body's own.
    name: String,
}

/// The tools reaching a target — each by the first tool of its shape — and how they lie in its
/// axes: what tells two targets' results apart (rounded to the micrometre's thousandth).
type Signature = Vec<(usize, [i64; 12])>;

fn rounded(t: &DAffine3) -> [i64; 12] {
    let mut out = [0; 12];
    for (o, x) in out.iter_mut().zip(t.to_cols_array()) {
        *o = (x * 1e6).round() as i64;
    }
    out
}

impl Document {
    /// [`crate::Operation::Boolean`] (module doc).
    pub(crate) fn booleans(
        &mut self,
        targets: &[EntityPath],
        tools: &[EntityPath],
        op: Boolean,
        scope: Scope,
        keep: Keep,
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        if targets.is_empty() || tools.is_empty() {
            return Err(Error::Invalid(Refusal::NoOperand));
        }
        let mut targets = dedup(targets);
        let tools = dedup(tools);
        if op == Boolean::Union && targets.len() > 1 {
            return Err(Error::Invalid(Refusal::UnionTargets));
        }
        if targets.iter().any(|t| tools.contains(t)) {
            return Err(Error::Invalid(Refusal::SelfBoolean));
        }
        // The tools as they are, before any target changes (a tool may be a copy of a target's
        // part).
        let tools = tools.iter().map(|p| self.tool_part(p)).collect::<Result<Vec<_>>>()?;
        // Tools of one shape cut alike, copies or not (two drills drawn alike).
        let alike: Vec<usize> = (0..tools.len())
            .map(|i| {
                let shape = tools[i].q.shape();
                (0..i).find(|&j| tools[j].part == tools[i].part || **tools[j].q.shape() == **shape).unwrap_or(i)
            })
            .collect();
        // What reaches each target, read before anything moves.
        let mut reached = Vec::with_capacity(targets.len());
        let mut signatures = Vec::with_capacity(targets.len());
        let mut parts = Vec::with_capacity(targets.len());
        for t in &targets {
            let (part, _, world) = self.resolve_part(t)?;
            let to_target = world.inverse();
            let r: Vec<usize> = (0..tools.len())
                .filter(|&i| op == Boolean::Union || self.reaches(part, &(to_target * tools[i].world), tools[i].part))
                .collect();
            let mut sig: Signature = r.iter().map(|&i| (alike[i], rounded(&(to_target * tools[i].world)))).collect();
            sig.sort_unstable();
            reached.push(r);
            signatures.push(sig);
            parts.push(part);
        }
        if reached.iter().all(Vec::is_empty) {
            return Err(Error::Invalid(Refusal::NothingReached));
        }
        // The targets reached by no tool are left alone.
        let acting: Vec<usize> = (0..targets.len()).filter(|&k| !reached[k].is_empty()).collect();
        if keep.targets {
            self.put_beside(&mut targets, &acting, &parts, &signatures)?;
            for &k in &acting {
                parts[k] = self.resolve_part(&targets[k])?.0;
            }
        }
        self.split_targets(&targets, &acting, &parts, &signatures, scope)?;
        // One change per part (the targets left sharing one share its result).
        let mut done = Vec::new();
        for &k in &acting {
            let (part, _, _) = self.resolve_part(&targets[k])?;
            if done.contains(&part) {
                continue;
            }
            done.push(part);
            let with: Vec<&ToolPart> = reached[k].iter().map(|&i| &tools[i]).collect();
            self.combine(&targets[k], &with, op, solver)?;
        }
        // The tools that took part leave the model, unless kept.
        let used: Vec<&ToolPart> =
            tools.iter().enumerate().filter(|(i, _)| reached.iter().any(|r| r.contains(i))).map(|(_, t)| t).collect();
        if !keep.tools {
            for t in &used {
                let ctx = self.context_of(&t.path.occurrences)?;
                self.assembly_mut(ctx)?.occurrences_mut().retain(|o| o.id != t.path.entity);
            }
        }
        if keep.drop_unused {
            for t in &used {
                if self.occurrence_count(t.part) == 0 {
                    self.components.remove(&t.part);
                }
            }
        }
        Ok(Outcome { created: acting.iter().map(|&k| targets[k].entity).collect(), ..Outcome::default() })
    }

    fn tool_part(&self, path: &EntityPath) -> Result<ToolPart> {
        let (part, q, world) = self.resolve_part(path)?;
        let named = self.component(part).ok().filter(|c| c.named).map(|c| c.name.clone());
        let name = self.resolve(path)?.name.clone();
        Ok(ToolPart { path: path.clone(), part, q: q.clone(), world, named, name })
    }

    /// Do the bounds of `tool` placed by `t` in the axes of `part` meet those of `part`?
    fn reaches(&self, part: Id, t: &DAffine3, tool: Id) -> bool {
        (0..3).all(|k| {
            let Ok(s) = Stretch::new(DVec3::AXES[k], Vec::new(), vec![0.0]) else { return true };
            let (Some(a), Some(b)) = (self.extent_along(part, &s, 0), self.placed_extent(tool, &s, t, 0)) else { return false };
            a.0 <= b.1 + 1e-6 && b.0 <= a.1 + 1e-6
        })
    }

    /// Each target acting replaced by a new occurrence beside it, of a copy of its part: the
    /// targets of one part reached alike share one copy.
    fn put_beside(&mut self, targets: &mut [EntityPath], acting: &[usize], parts: &[Id], signatures: &[Signature]) -> Result<()> {
        let mut copies: Vec<((Id, &Signature), Id)> = Vec::new();
        for &k in acting {
            let key = (parts[k], &signatures[k]);
            let copy = match copies.iter().find(|(c, _)| *c == key) {
                Some((_, copy)) => *copy,
                None => {
                    let copy = self.copy_component(parts[k])?;
                    copies.push((key, copy));
                    copy
                }
            };
            let o = self.resolve(&targets[k])?.clone();
            let id = self.fresh_id();
            let ctx = self.context_of(&targets[k].occurrences)?;
            self.assembly_mut(ctx)?.occurrences_mut().push(Occurrence { id, component: copy, visibility: Visibility::Shown, ..o });
            targets[k] = EntityPath::new(targets[k].occurrences.clone(), id);
        }
        Ok(())
    }

    /// Makes parts of their own for the targets whose results differ from the others' of their
    /// part, and for all of them under « only this one » unless they are every placement of
    /// their part (module doc).
    fn split_targets(
        &mut self,
        targets: &[EntityPath],
        acting: &[usize],
        parts: &[Id],
        signatures: &[Signature],
        scope: Scope,
    ) -> Result<()> {
        // Per part, its groups of targets reached alike, in the order picked.
        let mut by_part: Vec<(Id, Vec<Vec<usize>>)> = Vec::new();
        for &k in acting {
            let groups = match by_part.iter_mut().find(|(p, _)| *p == parts[k]) {
                Some((_, g)) => g,
                None => {
                    by_part.push((parts[k], Vec::new()));
                    &mut by_part.last_mut().expect("just pushed").1
                }
            };
            match groups.iter_mut().find(|g| signatures[g[0]] == signatures[k]) {
                Some(g) => g.push(k),
                None => groups.push(vec![k]),
            }
        }
        let mut placed = FastMap::default();
        if scope == Scope::This {
            self.count_placed(self.root, &mut placed);
        }
        for (part, groups) in by_part {
            let all: usize = groups.iter().map(Vec::len).sum();
            let in_place = match scope {
                Scope::All => true,
                Scope::This => placed.get(&part).copied().unwrap_or(0) == all,
            };
            // The largest group (the first of the largest) keeps the part.
            let kept = in_place.then(|| (0..groups.len()).rev().max_by_key(|&g| groups[g].len())).flatten();
            for (g, group) in groups.iter().enumerate() {
                if Some(g) == kept {
                    continue;
                }
                self.separate(&targets[group[0]])?;
                let (own, _, _) = self.resolve_part(&targets[group[0]])?;
                for &k in &group[1..] {
                    self.separate_onto(&targets[k], own)?;
                }
            }
        }
        Ok(())
    }

    /// Makes the assemblies on `path`'s way its own, as [`Document::separate`], then points its
    /// occurrence at `part`.
    pub(crate) fn separate_onto(&mut self, path: &EntityPath, part: Id) -> Result<()> {
        if !path.occurrences.is_empty() {
            self.separate(&EntityPath::new(
                path.occurrences[..path.occurrences.len() - 1].to_vec(),
                path.occurrences[path.occurrences.len() - 1],
            ))?;
        }
        let ctx = self.context_of(&path.occurrences)?;
        if let Some(o) = self.assembly_mut(ctx)?.occurrence_mut(path.entity) {
            o.component = part;
        }
        Ok(())
    }

    /// Before a union, `part`'s own bodies put in one positive group at the identity when a
    /// machining is among them: every negative cuts every positive of its group, the tools
    /// joined to the part included (docs/model/bodies.md#booleans-between-parts). The group's
    /// result is the part as it is, `shape` (no kernel); it takes the part's name, when the user
    /// gave it. A part without machining at its top level is left as it is.
    fn group_machined(&mut self, part: Id, shape: std::sync::Arc<skraft_kernel::Solid>) -> Result<()> {
        if !self.part(part)?.bodies.iter().any(|b| b.sign == Sign::Negative) {
            return Ok(());
        }
        let named = self.component(part).ok().filter(|c| c.named).map(|c| c.name.clone());
        let (group, body) = (self.fresh_id(), self.fresh_id());
        let p = self.part_mut(part)?;
        let children = std::mem::take(&mut p.bodies);
        let failed = std::mem::take(&mut p.failed);
        let mut def = BodyDef::group(group, shape.clone(), children, failed);
        if named.is_some() {
            def.set_name(named);
        }
        p.defs.push(def);
        p.bodies.push(Body::new(body, group, Sign::Positive, DAffine3::IDENTITY));
        // Its group alone at the identity, the part is plain: its shape is the group's.
        p.shape = None;
        Ok(())
    }

    /// `target op (∪ tools)` on the target's part, the tools brought into its axes.
    fn combine(&mut self, target: &EntityPath, tools: &[&ToolPart], op: Boolean, solver: &mut dyn Solver) -> Result<()> {
        let (part, p, world) = self.resolve_part(target)?;
        let shape = p.shape().clone();
        let to_target = world.inverse();
        match op {
            // Each tool a body of the target: negative (a machining), or positive. A tool that
            // is its single positive body (a shape drawn) is taken whole, its profile with it:
            // still editable; any other comes as a group of copies of its bodies, its shape the
            // group's (docs/model/bodies.md#body-groups). Added as they are: the part as it is
            // cut by the negatives, or joined to it — its machinings first kept off the tools.
            Boolean::Difference | Boolean::Union => {
                if op == Boolean::Union {
                    self.group_machined(part, shape)?;
                }
                let mut ids = Vec::with_capacity(tools.len());
                for t in tools {
                    let (defs, mut body) = self.tool_as_body(&t.q, &(to_target * t.world), t.named.clone());
                    if op == Boolean::Difference {
                        body.sign = Sign::Negative;
                    }
                    if !t.name.is_empty() {
                        body.name = t.name.clone();
                    }
                    ids.push(body.id);
                    let p = self.part_mut(part)?;
                    p.defs.extend(defs);
                    p.bodies.push(body);
                }
                self.recompute(part, Some(&ids), solver)?;
            }
            // The target limited by the tools' union, the tools kept (docs/decisions/0045-intersection-a-third-sign.md):
            // one tool an intersection itself; several, the children of an intersection group (their
            // union limits, as the tool's `(∪A) ∩ (∪B)`), each as a union's or a difference's are.
            Boolean::Intersection => {
                let mut bodies = Vec::with_capacity(tools.len());
                for t in tools {
                    let (defs, mut body) = self.tool_as_body(&t.q, &(to_target * t.world), t.named.clone());
                    if !t.name.is_empty() {
                        body.name = t.name.clone();
                    }
                    self.part_mut(part)?.defs.extend(defs);
                    bodies.push(body);
                }
                let intersect = match bodies.len() {
                    1 => Body { sign: Sign::Intersect, ..bodies.remove(0) },
                    _ => {
                        let p = self.part(part)?;
                        let (solid, failed, faces) = Self::group_result(&p.defs, &bodies, solver)?;
                        let mut def = BodyDef::group(self.fresh_id(), solid, bodies, failed);
                        def.group = def.group.take().map(|g| g.with_faces(&def.solid, faces));
                        let body = Body::new(self.fresh_id(), def.id, Sign::Intersect, DAffine3::IDENTITY);
                        self.part_mut(part)?.defs.push(def);
                        body
                    }
                };
                let id = intersect.id;
                self.part_mut(part)?.bodies.push(intersect);
                self.recompute(part, Some(&[id]), solver)?;
            }
        }
        Ok(())
    }
}

/// `paths`, each once, in their order.
fn dedup(paths: &[EntityPath]) -> Vec<EntityPath> {
    let mut out: Vec<EntityPath> = Vec::with_capacity(paths.len());
    for p in paths {
        if !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

//! « Imprimer » (docs/decisions/0055-linked-bodies-through-imprints.md): the parts chosen make
//! imprints of the bodies that enter other parts — the tenon grown by a clearance, deeper, run
//! back past its shoulder — and link them to those parts. An imprint made of a body follows it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use skraft_kernel::{Clearance, Solid, Stretch};

use crate::document::{Body, BodyDef, BodySource, ComponentKind, Document, Id, MAX_DEPTH, Part, Sign};
use crate::links::{Link, same_place};
use crate::ops::{Outcome, Solver};
use crate::{Error, FastMap, FastSet, Refusal, Result};

/// How an imprint is made of the body `of` of its part: it follows that body's place and shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImprintRecipe {
    pub of: Id,
    pub clearance: Clearance,
}

/// A part placed under the assembly a plan looks in.
struct Placed {
    path: Vec<Id>,
    part: Id,
    t: DAffine3,
    /// Its box in that assembly's axes.
    world: Option<(DVec3, DVec3)>,
}

/// One link « Imprimer » would make.
#[derive(Clone, Debug)]
struct PlannedLink {
    source: Vec<Id>,
    part: Id,
    /// The imprint, or (`exists` false) the body to make one of.
    body: Id,
    exists: bool,
    receiver: Vec<Id>,
    pose: DAffine3,
    slot: usize,
}

/// One machining of a receiving part: the links sharing it.
#[derive(Clone, Debug)]
struct Slot {
    to: Id,
    solid: Arc<Solid>,
    pose: DAffine3,
    /// Which way its imprint enters, in the receiver's axes.
    axis: Option<DVec3>,
    /// A linked body already there.
    body: Option<Id>,
    reached: BTreeSet<Vec<Id>>,
}

/// What « Imprimer » would do, for its confirmation.
#[derive(Clone, Debug, Default)]
pub struct ImprintPlan {
    links: Vec<PlannedLink>,
    slots: Vec<Slot>,
    /// The receiving parts with a machining that some of their copies have no source for.
    pub partial: Vec<Id>,
}

impl ImprintPlan {
    /// How many links it makes.
    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    /// How many machinings it adds to the receiving parts.
    pub fn machining_count(&self) -> usize {
        self.slots.iter().filter(|s| s.body.is_none()).count()
    }

    /// The parts receiving a machining.
    pub fn receivers(&self) -> Vec<Id> {
        self.slots.iter().map(|s| s.to).collect::<BTreeSet<_>>().into_iter().collect()
    }
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
fn placed_box(t: &DAffine3, (lo, hi): (DVec3, DVec3)) -> (DVec3, DVec3) {
    (0..8).fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(a, b), i| {
        let c =
            DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
        let p = t.transform_point3(c);
        (a.min(p), b.max(p))
    })
}

fn boxes_meet(a: &(DVec3, DVec3), b: &(DVec3, DVec3)) -> bool {
    (0..3).all(|k| a.0[k] < b.1[k] && a.1[k] > b.0[k])
}

/// From the face where `b` joins the rest of its part towards its other end, in `b`'s axes:
/// the face it shares most with another positive body. None when it touches none.
pub(crate) fn shoulder_axis(p: &Part, b: &Body) -> Option<DVec3> {
    const ON: f64 = 1e-6;
    let solid = p.solid_of(b);
    let points = &solid.record().vertices;
    if points.is_empty() {
        return None;
    }
    let centre = points.iter().fold(DVec3::ZERO, |c, v| c + DVec3::from_array(v.point)) / points.len() as f64;
    let faces = solid.planar_faces();
    let mut best: Option<(f64, DVec3)> = None;
    for o in p.positives().filter(|o| o.id != b.id) {
        let t = b.placement.inverse() * o.placement;
        for other in p.solid_of(o).planar_faces() {
            let normal = t.transform_vector3(other.plane.normal);
            let at = t.transform_point3(other.plane.point);
            let there: Vec<DVec3> = other.vertices.iter().map(|v| t.transform_point3(*v)).collect();
            for f in &faces {
                if f.plane.normal.dot(normal).abs() < 1.0 - 1e-9 || (at - f.plane.point).dot(f.plane.normal).abs() > ON {
                    continue;
                }
                let bound = |list: &[DVec3]| list.iter().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(a, c), v| (a.min(*v), c.max(*v)));
                let (a, c) = (bound(&f.vertices), bound(&there));
                let d = a.1.min(c.1) - a.0.max(c.0);
                if d.min_element() < -ON {
                    continue;
                }
                // The two largest extents of what they share, in a plane.
                let area = d.x * d.y + d.y * d.z + d.z * d.x;
                if area > ON && best.is_none_or(|(most, _)| area > most) {
                    let mid = f.vertices.iter().fold(DVec3::ZERO, |m, v| m + *v) / f.vertices.len().max(1) as f64;
                    let outward = if (mid - centre).dot(f.plane.normal) >= 0.0 { f.plane.normal } else { -f.plane.normal };
                    best = Some((area, -outward));
                }
            }
        }
    }
    best.map(|(_, axis)| axis.normalize())
}

/// The clearance of an imprint of `b`: run back by its own length from its shoulder; without a
/// shoulder, around it alone.
fn clearance_of(p: &Part, b: &Body, around: f64, deep: f64) -> Clearance {
    let axis = shoulder_axis(p, b);
    let back = axis
        .and_then(|a| Stretch::new(a, Vec::new(), vec![0.0]).ok())
        .and_then(|s| s.extent(p.solid_of(b)))
        .map_or(0.0, |(lo, hi)| hi - lo);
    Clearance { axis, around, deep: if axis.is_some() { deep } else { 0.0 }, back }
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
                    let world = doc.component_bounds(o.component).map(|b| placed_box(&placed, b));
                    out.push(Placed { path: path.clone(), part: o.component, t: placed, world });
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

    /// The parts `component` is or holds.
    fn parts_in(&self, component: Id, out: &mut BTreeSet<Id>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        match self.component(component).map(|c| &c.kind) {
            Ok(ComponentKind::Part(_)) => {
                out.insert(component);
            }
            Ok(ComponentKind::Assembly(a)) => a.occurrences().iter().for_each(|o| self.parts_in(o.component, out, depth + 1)),
            Err(_) => {}
        }
    }

    /// What « Imprimer » would do for the occurrences `selected` of `context`: every occurrence
    /// of the parts they are or hold is a source. Its imprints if it has some, else its bodies
    /// that reach into another part, each linked to the parts it reaches. Shared machinings:
    /// the same place in copies of one part is one body.
    pub fn imprint_plan(&self, context: Id, selected: &[Id]) -> Result<ImprintPlan> {
        let a = self.assembly(context)?;
        let mut chosen = BTreeSet::new();
        for id in selected {
            let o = a.occurrence(*id).ok_or(Error::NotFound(*id))?;
            self.parts_in(o.component, &mut chosen, 0);
        }
        let placed = self.placed_under(context);
        // What is linked already: not linked again.
        let mut linked: FastSet<(Id, Id, Id)> = FastSet::default();
        // The model's own links too: `components()` leaves it out.
        for c in self.components.values() {
            if let ComponentKind::Assembly(h) = &c.kind {
                for l in h.links() {
                    if let (Some(s), Some(r)) = (l.source.last(), l.receiver.last()) {
                        linked.insert((*s, l.imprint, *r));
                    }
                }
            }
        }
        let mut axes: FastMap<(Id, Id), Option<DVec3>> = FastMap::default();
        let mut plan = ImprintPlan::default();
        for s in placed.iter().filter(|s| chosen.contains(&s.part)) {
            let p = self.part(s.part)?;
            let imprints: Vec<&Body> = p.bodies.iter().filter(|b| b.sign == Sign::Imprint).collect();
            let exists = !imprints.is_empty();
            let bodies: Vec<&Body> =
                if exists { imprints } else { p.positives().filter(|b| p.def_of(b).source == BodySource::Local).collect() };
            for b in bodies {
                let solid = p.solid_of(b);
                let Some(extent) = solid_box(solid) else { continue };
                let body_box = placed_box(&(s.t * b.placement), extent);
                let axis = *axes.entry((s.part, b.id)).or_insert_with(|| match (exists, &p.def_of(b).imprint) {
                    (true, Some(r)) => r.clearance.axis,
                    (true, None) => None,
                    (false, _) => shoulder_axis(p, b),
                });
                for r in &placed {
                    if r.path == s.path || !r.world.as_ref().is_some_and(|w| boxes_meet(&body_box, w)) {
                        continue;
                    }
                    let Some((lo, hi)) = self.component_bounds(r.part) else { continue };
                    let pose = r.t.inverse() * s.t * b.placement;
                    if !reaches(solid, &pose, lo, hi) {
                        continue;
                    }
                    if exists && linked.contains(&(*s.path.last().expect("a path"), b.id, *r.path.last().expect("a path"))) {
                        continue;
                    }
                    let along = axis.map(|x| pose.transform_vector3(x));
                    let same_way = |o: Option<DVec3>| match (o, along) {
                        (Some(x), Some(y)) => x.distance(y) < 1e-6,
                        (None, None) => true,
                        _ => false,
                    };
                    let slot = plan
                        .slots
                        .iter()
                        .position(|sl| sl.to == r.part && same_way(sl.axis) && same_place(solid, &pose, &sl.solid, &sl.pose))
                        .unwrap_or_else(|| {
                            // An imprint already there may have its body in this place.
                            let q = self.part(r.part).ok();
                            let body = q.filter(|_| exists).and_then(|q| {
                                q.negatives()
                                    .find(|x| {
                                        q.def_of(x).source == BodySource::Linked && same_place(solid, &pose, q.solid_of(x), &x.placement)
                                    })
                                    .map(|x| x.id)
                            });
                            plan.slots.push(Slot { to: r.part, solid: solid.clone(), pose, axis: along, body, reached: BTreeSet::new() });
                            plan.slots.len() - 1
                        });
                    plan.slots[slot].reached.insert(r.path.clone());
                    plan.links.push(PlannedLink {
                        source: s.path.clone(),
                        part: s.part,
                        body: b.id,
                        exists,
                        receiver: r.path.clone(),
                        pose,
                        slot,
                    });
                }
            }
        }
        let partial: BTreeSet<Id> =
            plan.slots.iter().filter(|sl| placed.iter().filter(|x| x.part == sl.to).count() > sl.reached.len()).map(|sl| sl.to).collect();
        plan.partial = partial.into_iter().collect();
        Ok(plan)
    }

    /// [`crate::Operation::Imprint`].
    pub(crate) fn imprint(&mut self, context: Id, sources: &[Id], around: f64, deep: f64, solver: &mut dyn Solver) -> Result<Outcome> {
        if !around.is_finite() || !deep.is_finite() {
            return Err(Error::Invalid(Refusal::BadClearance));
        }
        let plan = self.imprint_plan(context, sources)?;
        if plan.links.is_empty() {
            return Err(Error::Invalid(Refusal::NothingToImprint));
        }
        // The imprints to make, each once for its part.
        let mut made: BTreeMap<(Id, Id), (Id, Arc<Solid>)> = BTreeMap::new();
        for l in plan.links.iter().filter(|l| !l.exists) {
            if made.contains_key(&(l.part, l.body)) {
                continue;
            }
            let p = self.part(l.part)?;
            let b = p.body(l.body).ok_or(Error::NotFound(l.body))?.clone();
            let clearance = clearance_of(p, &b, around, deep);
            let (of_solid, recipe) = (p.solid_of(&b).clone(), p.def_of(&b).profile.as_ref().map(|f| f.primitive()));
            let solid = solver.imprint(&of_solid, recipe.as_ref(), &clearance)?;
            let (def, id) = (self.fresh_id(), self.fresh_id());
            let d = BodyDef { imprint: Some(ImprintRecipe { of: b.id, clearance }), ..BodyDef::new(def, solid.clone()) };
            let q = self.part_mut(l.part)?;
            q.defs.push(d);
            q.bodies.push(Body::new(id, def, Sign::Imprint, b.placement));
            self.recompute(l.part, Some(&[id]), solver)?;
            made.insert((l.part, l.body), (id, solid));
        }
        // The machinings, one per slot, and a link for each.
        let mut bodies: Vec<Option<Id>> = plan.slots.iter().map(|s| s.body).collect();
        let mut added: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
        let mut links = Vec::with_capacity(plan.links.len());
        for l in &plan.links {
            let (imprint, solid) = match made.get(&(l.part, l.body)) {
                Some((id, solid)) => (*id, solid.clone()),
                None => {
                    let p = self.part(l.part)?;
                    (l.body, p.solid_of(p.body(l.body).ok_or(Error::NotFound(l.body))?).clone())
                }
            };
            let body = match bodies[l.slot] {
                Some(b) => b,
                None => {
                    let to = plan.slots[l.slot].to;
                    let (def, id) = (self.fresh_id(), self.fresh_id());
                    let q = self.part_mut(to)?;
                    q.defs.push(BodyDef { source: BodySource::Linked, ..BodyDef::new(def, solid) });
                    q.bodies.push(Body::new(id, def, Sign::Negative, l.pose));
                    added.entry(to).or_default().push(id);
                    bodies[l.slot] = Some(id);
                    id
                }
            };
            links.push(Link { id: self.fresh_id(), source: l.source.clone(), imprint, receiver: l.receiver.clone(), body, seat: l.pose });
        }
        for (part, ids) in added {
            self.recompute(part, Some(&ids), solver)?;
        }
        let count = links.len();
        self.assembly_mut(context)?.links_mut().extend(links);
        self.settle_link_holders();
        Ok(Outcome { imprinted: (plan.machining_count(), count), ..Outcome::default() })
    }

    /// [`crate::Operation::SetClearance`]: the imprint's clearance changed, its tool built
    /// again from the body it is made of.
    pub(crate) fn set_clearance(
        &mut self,
        part: Id,
        body: Id,
        (around, deep, back): (f64, f64, f64),
        solver: &mut dyn Solver,
    ) -> Result<Outcome> {
        let p = self.part(part)?;
        let b = p.body(body).filter(|b| b.sign == Sign::Imprint).ok_or(Error::Invalid(Refusal::BadClearance))?;
        let def = b.def;
        let recipe = p.def_of(b).imprint.clone().ok_or(Error::Invalid(Refusal::BadClearance))?;
        let of = p.body(recipe.of).ok_or(Error::Invalid(Refusal::BadClearance))?;
        let clearance = Clearance { around, deep, back, ..recipe.clearance };
        if !clearance.is_valid() {
            return Err(Error::Invalid(Refusal::BadClearance));
        }
        if clearance == recipe.clearance {
            return Ok(Outcome::default());
        }
        let (of_solid, primitive) = (p.solid_of(of).clone(), p.def_of(of).profile.as_ref().map(|f| f.primitive()));
        let solid = solver.imprint(&of_solid, primitive.as_ref(), &clearance)?;
        if let Some(d) = self.part_mut(part)?.defs.iter_mut().find(|d| d.id == def) {
            d.solid = solid;
            d.imprint = Some(ImprintRecipe { clearance, ..recipe });
        }
        Ok(Outcome::default())
    }

    /// The imprints after an operation (`before` the document it was applied to): each goes
    /// with the body it is made of, and is built again when that body's shape changed. One whose
    /// body is gone stays as it is, made of nothing any more.
    pub(crate) fn update_imprints(&mut self, before: &Document, solver: &mut dyn Solver) -> Result<()> {
        let parts: Vec<Id> = self
            .components
            .iter()
            .filter(|(id, c)| {
                before.components.get(*id).is_none_or(|b| !Arc::ptr_eq(b, c))
                    && matches!(&c.kind, ComponentKind::Part(p) if p.defs.iter().any(|d| d.imprint.is_some()))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in parts {
            let Ok(old) = before.part(id) else { continue };
            let p = self.part(id)?.clone();
            let mut rebuilt = BTreeSet::new();
            for b in p.bodies.iter().filter(|b| b.sign == Sign::Imprint) {
                let Some(recipe) = p.def_of(b).imprint.clone() else { continue };
                let Some(of) = p.body(recipe.of) else {
                    if let Some(d) = self.part_mut(id)?.defs.iter_mut().find(|d| d.id == b.def) {
                        d.imprint = None;
                    }
                    continue;
                };
                let Some(was) = old.body(recipe.of) else { continue };
                // Moved with it, unless the operation moved the imprint itself.
                if old.body(b.id).is_some_and(|x| x.placement == b.placement) && of.placement != was.placement {
                    let moved = of.placement * was.placement.inverse();
                    if let Some(x) = self.part_mut(id)?.bodies.iter_mut().find(|x| x.id == b.id) {
                        x.placement = moved * x.placement;
                    }
                }
                if !Arc::ptr_eq(p.solid_of(of), old.solid_of(was)) && rebuilt.insert(b.def) {
                    let primitive = p.def_of(of).profile.as_ref().map(|f| f.primitive());
                    let solid = solver.imprint(p.solid_of(of), primitive.as_ref(), &recipe.clearance)?;
                    if let Some(d) = self.part_mut(id)?.defs.iter_mut().find(|d| d.id == b.def) {
                        d.solid = solid;
                    }
                }
            }
        }
        Ok(())
    }
}

/// The solid's box, from the record.
fn solid_box(s: &Solid) -> Option<(DVec3, DVec3)> {
    let mut out = (DVec3::ZERO, DVec3::ZERO);
    for k in 0..3 {
        let axis = Stretch::new(DVec3::AXES[k], Vec::new(), vec![0.0]).ok()?;
        (out.0[k], out.1[k]) = axis.extent(s)?;
    }
    Some(out)
}

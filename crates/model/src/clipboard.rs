//! Copy and paste (docs/tools/copy-paste.md): what a selection copies, as a fragment
//! that [`Operation::Paste`](crate::Operation::Paste) or
//! [`Operation::PasteBodies`](crate::Operation::PasteBodies) places again.
//!
//! A fragment shares the source's solids (`Arc`): copying costs nearly nothing, whatever the
//! model's size.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::DAffine3;
use serde::{Deserialize, Serialize};

use crate::document::{
    Assembly, Body, BodyDef, Component, ComponentKind, Document, EntityPath, Guide, GuidePath, GuideShape, Id, Sign, is_identity, used_defs,
};
use crate::{Error, Result};

/// Bodies and guides copied from an open part, in the part's axes, with the definitions of
/// the bodies (pasted into the same part, its bodies are copies of them).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyFragment {
    pub defs: Vec<BodyDef>,
    pub bodies: Vec<Body>,
    pub guides: Vec<GuideShape>,
    /// The world transformation of the part copied from: pasted out of a part, the new part
    /// takes its axes (docs/tools/copy-paste.md, across the part's boundary).
    #[serde(default = "identity", skip_serializing_if = "is_identity")]
    pub world: DAffine3,
}

impl Default for BodyFragment {
    fn default() -> Self {
        BodyFragment { defs: Vec::new(), bodies: Vec::new(), guides: Vec::new(), world: DAffine3::IDENTITY }
    }
}

impl BodyFragment {
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty() && self.guides.is_empty()
    }
}

fn identity() -> DAffine3 {
    DAffine3::IDENTITY
}

impl Document {
    /// What copying `ids` — occurrences and guides of the assembly `instances` leads into —
    /// copies: a document whose root holds those occurrences and guides, in **world** axes,
    /// with the components they use and the materials they are painted with, under their ids
    /// here (pasting into this document again uses them, docs/tools/copy-paste.md).
    pub fn entities_fragment(&self, instances: &[Id], ids: &[Id]) -> Result<Document> {
        let context = self.context_of(instances)?;
        let world = self.transform_of(instances)?;
        let (guides, entities) = self.split_guides(context, ids)?;
        let occurrences = self.occurrences_of(context, &entities)?;
        let occurrences = occurrences.into_iter().map(|o| crate::Occurrence { transform: world * o.transform, ..o }).collect();
        let shown = self.component(context)?;
        let guides = shown
            .guides
            .iter()
            .filter(|g| guides.contains(&g.id))
            .map(|g| Guide { id: g.id, shape: g.shape.transformed(&world) })
            .collect();
        self.fragment(occurrences, guides, self.next_id)
    }

    /// What copying the occurrences `paths` names and the guides `guides` names — at any depth
    /// (« Parts », docs/tools/selection.md) — copies, as [`Document::entities_fragment`]: each
    /// in world axes, side by side in the fragment's root; an id met twice (copies of one
    /// assembly keep their occurrences' ids) drawn afresh.
    pub fn paths_fragment(&self, paths: &[EntityPath], guides: &[GuidePath]) -> Result<Document> {
        let mut next = self.next_id;
        let mut seen = BTreeSet::new();
        let mut fresh = |id: Id| {
            if seen.insert(id) {
                return id;
            }
            next += 1;
            Id(next - 1)
        };
        let mut occurrences = Vec::new();
        for p in paths {
            let world = self.transform_of(&p.instances)?;
            let mut o = self.resolve(p)?.clone();
            o.transform = world * o.transform;
            o.id = fresh(o.id);
            occurrences.push(o);
        }
        let mut shapes = Vec::new();
        for g in guides {
            let (_, world) = self.component_at(&g.instances)?;
            let shape = self.resolve_guide(g)?.shape.transformed(&world);
            shapes.push(Guide { id: fresh(g.guide), shape });
        }
        self.fragment(occurrences, shapes, next)
    }

    /// A fragment whose root holds `occurrences` and `guides` (in world axes), with the
    /// components they use at any depth and the materials they are painted with; ids counted
    /// from `next_id`.
    fn fragment(&self, occurrences: Vec<crate::Occurrence>, guides: Vec<Guide>, next_id: u64) -> Result<Document> {
        // The components the occurrences use, at any depth, and their materials.
        let mut components = BTreeSet::new();
        let mut materials: BTreeSet<Id> = occurrences.iter().filter_map(|o| o.material).collect();
        let mut stack: Vec<Id> = occurrences.iter().map(|o| o.component).collect();
        while let Some(id) = stack.pop() {
            if !components.insert(id) {
                continue;
            }
            match &self.component(id)?.kind {
                ComponentKind::Assembly(a) => {
                    materials.extend(a.occurrences().iter().filter_map(|o| o.material));
                    stack.extend(a.occurrences().iter().map(|o| o.component));
                }
                ComponentKind::Part(p) => materials.extend(p.faces.values().copied()),
            }
        }

        let root = self.component(self.root)?;
        let top = Component {
            id: self.root,
            name: root.name.clone(),
            named: true,
            kind: ComponentKind::Assembly(Assembly::new(occurrences)),
            guides,
            behavior: Default::default(),
        };
        let mut all: BTreeMap<Id, Arc<Component>> = components.iter().map(|id| (*id, self.components[id].clone())).collect();
        all.insert(self.root, Arc::new(top));
        Ok(Document {
            components: all,
            root: self.root,
            next_id,
            settings: self.settings.clone(),
            materials: materials.iter().filter_map(|id| self.materials.get(id).map(|m| (*id, m.clone()))).collect(),
        })
    }

    /// What copying `ids` — bodies and guides of the part `target` places — copies, in the
    /// part's axes.
    pub fn bodies_fragment(&self, target: &EntityPath, ids: &[Id]) -> Result<BodyFragment> {
        // In an open group, its bodies (docs/model/bodies.md#body-groups), in its axes.
        let (part, p, world) = self.resolve_container(target)?;
        let (guides, bodies) = self.split_guides(part, ids)?;
        let bodies: Vec<Body> = bodies.iter().map(|id| p.body(*id).cloned().ok_or(Error::NotFound(*id))).collect::<Result<_>>()?;
        let used = used_defs(&p.defs, &bodies);
        let defs = p.defs.iter().filter(|d| used.contains(&d.id)).cloned().collect();
        // The part's guides, in the axes of what is open (a group's, docs/model/bodies.md#body-groups).
        let into = self.resolve_part(target)?.2.inverse() * world;
        let into = into.inverse();
        let guides = self.component(part)?.guides.iter().filter(|g| guides.contains(&g.id)).map(|g| g.shape.transformed(&into)).collect();
        Ok(BodyFragment { defs, bodies, guides, world })
    }

    /// This document — an entities' fragment ([`Document::entities_fragment`]) — as the bodies
    /// it pastes into an open part, in world axes: one positive body per part placed, at any
    /// depth (hidden occurrences left out). A part that is its single positive body gives it
    /// whole, its profile with it; any other comes as a group of its bodies (docs/model/bodies.md « Body
    /// groups »), its shape the group's, under the part's id: copies of a part share it.
    pub fn parts_as_bodies(&self) -> Result<BodyFragment> {
        let mut out = BodyFragment::default();
        let root = self.component(self.root)?;
        out.guides = root.guides.iter().map(|g| g.shape).collect();
        // A part's occurrence's name: the body's own (docs/model/bodies.md#body-names).
        let mut stack: Vec<(Id, DAffine3, &str)> = vec![(self.root, DAffine3::IDENTITY, "")];
        while let Some((id, t, occurrence)) = stack.pop() {
            let c = self.component(id)?;
            // A part the user named names the body it becomes, unless that one has a name
            // (docs/model/bodies.md#body-names).
            let named = c.named.then(|| c.name.clone());
            match &c.kind {
                ComponentKind::Assembly(a) => {
                    // Reversed: popped in the assembly's order.
                    for o in a.occurrences().iter().rev().filter(|o| !o.hidden) {
                        stack.push((o.component, t * o.transform, &o.name));
                    }
                }
                ComponentKind::Part(p) => {
                    let used = used_defs(&p.defs, &p.bodies);
                    for d in p.defs.iter().filter(|d| used.contains(&d.id)) {
                        if !out.defs.iter().any(|o| o.id == d.id) {
                            out.defs.push(d.clone());
                        }
                    }
                    let (def, placement) = match p.is_plain() {
                        true => (p.bodies[0].def, t * p.bodies[0].placement),
                        false => {
                            if !out.defs.iter().any(|d| d.id == id) {
                                out.defs.push(BodyDef::group(id, p.shape().clone(), p.bodies.clone(), p.failed.clone()));
                            }
                            (id, t)
                        }
                    };
                    if let Some(d) = out.defs.iter_mut().find(|d| d.id == def)
                        && !d.named
                        && named.is_some()
                    {
                        d.set_name(named);
                    }
                    let mut body = Body::new(Id(out.bodies.len() as u64), def, Sign::Positive, placement);
                    body.name = match (occurrence, p.is_plain()) {
                        ("", true) => p.bodies[0].name.clone(),
                        (name, _) => name.to_owned(),
                    };
                    out.bodies.push(body);
                }
            }
        }
        Ok(out)
    }
}

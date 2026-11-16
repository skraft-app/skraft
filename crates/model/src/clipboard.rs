//! Copy and paste (docs/ARCHITECTURE.md §6 decies): what a selection copies, as a fragment
//! that [`Operation::Paste`](crate::Operation::Paste) or
//! [`Operation::PasteBodies`](crate::Operation::PasteBodies) places again.
//!
//! A fragment shares the source's solids (`Arc`): copying costs nearly nothing, whatever the
//! model's size.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::DAffine3;
use serde::{Deserialize, Serialize};

use crate::document::{Assembly, Body, BodyDef, Component, ComponentKind, Document, EntityPath, Guide, GuideShape, Id, Sign, is_identity};
use crate::{Error, Result};

/// Bodies and guides copied from an open part, in the part's axes, with the definitions of
/// the bodies (pasted into the same part, its bodies are copies of them).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyFragment {
    pub defs: Vec<BodyDef>,
    pub bodies: Vec<Body>,
    pub guides: Vec<GuideShape>,
    /// The world transformation of the part copied from: pasted out of a part, the new part
    /// takes its axes (§6 decies, across the part's boundary).
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

/// Parts copied, as the bodies they paste into an open part as (§6 decies, across the part's
/// boundary).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartsAsBodies {
    /// In world axes, as the parts were copied; every body positive.
    pub fragment: BodyFragment,
    /// Did a part lose its bodies, flattened into its final shape?
    pub flattened: bool,
}

fn identity() -> DAffine3 {
    DAffine3::IDENTITY
}

impl Document {
    /// What copying `ids` — occurrences and guides of the assembly `instances` leads into —
    /// copies: a document whose root holds those occurrences and guides, in **world** axes,
    /// with the components they use and the materials they are painted with, under their ids
    /// here (pasting into this document again uses them, §6 decies).
    pub fn entities_fragment(&self, instances: &[Id], ids: &[Id]) -> Result<Document> {
        let context = self.context_of(instances)?;
        let world = self.transform_of(instances)?;
        let (guides, entities) = self.split_guides(context, ids)?;
        let occurrences = self.occurrences_of(context, &entities)?;

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
        let shown = self.component(context)?;
        let top = Component {
            id: self.root,
            name: root.name.clone(),
            named: true,
            kind: ComponentKind::Assembly(Assembly::new(
                occurrences.into_iter().map(|o| crate::Occurrence { transform: world * o.transform, ..o }).collect(),
            )),
            guides: shown
                .guides
                .iter()
                .filter(|g| guides.contains(&g.id))
                .map(|g| Guide { id: g.id, shape: g.shape.transformed(&world) })
                .collect(),
            behavior: Default::default(),
        };
        let mut all: BTreeMap<Id, Arc<Component>> = components.iter().map(|id| (*id, self.components[id].clone())).collect();
        all.insert(self.root, Arc::new(top));
        Ok(Document {
            components: all,
            root: self.root,
            next_id: self.next_id,
            settings: self.settings.clone(),
            materials: materials.iter().filter_map(|id| self.materials.get(id).map(|m| (*id, m.clone()))).collect(),
        })
    }

    /// What copying `ids` — bodies and guides of the part `target` places — copies, in the
    /// part's axes.
    pub fn bodies_fragment(&self, target: &EntityPath, ids: &[Id]) -> Result<BodyFragment> {
        let (part, p, _) = self.resolve_part(target)?;
        let (guides, bodies) = self.split_guides(part, ids)?;
        let bodies: Vec<Body> = bodies.iter().map(|id| p.body(*id).cloned().ok_or(Error::NotFound(*id))).collect::<Result<_>>()?;
        let defs = p.defs.iter().filter(|d| bodies.iter().any(|b| b.def == d.id)).cloned().collect();
        let guides = self.component(part)?.guides.iter().filter(|g| guides.contains(&g.id)).map(|g| g.shape).collect();
        let world = self.resolve_part(target)?.2;
        Ok(BodyFragment { defs, bodies, guides, world })
    }

    /// This document — an entities' fragment ([`Document::entities_fragment`]) — as the bodies
    /// it pastes into an open part: one positive body per part placed, at any depth (hidden
    /// occurrences left out), in world axes. A part that is its single positive body gives
    /// it whole, its profile with it; any other is flattened, its final shape one free body.
    /// Copies of a part share one definition: its own (plain), else under the part's id.
    pub fn parts_as_bodies(&self) -> Result<PartsAsBodies> {
        let mut out = PartsAsBodies::default();
        let root = self.component(self.root)?;
        out.fragment.guides = root.guides.iter().map(|g| g.shape).collect();
        let mut stack: Vec<(Id, DAffine3)> = vec![(self.root, DAffine3::IDENTITY)];
        while let Some((id, t)) = stack.pop() {
            match &self.component(id)?.kind {
                ComponentKind::Assembly(a) => {
                    // Reversed: popped in the assembly's order.
                    for o in a.occurrences().iter().rev().filter(|o| !o.hidden) {
                        stack.push((o.component, t * o.transform));
                    }
                }
                ComponentKind::Part(p) => {
                    let (def, placement) = match p.is_plain() {
                        true => (p.def_of(&p.bodies[0]).clone(), t * p.bodies[0].placement),
                        false => {
                            out.flattened = true;
                            (BodyDef::new(id, p.shape().clone()), t)
                        }
                    };
                    let body = Body::new(Id(out.fragment.bodies.len() as u64), def.id, Sign::Positive, placement);
                    if !out.fragment.defs.iter().any(|d| d.id == def.id) {
                        out.fragment.defs.push(def);
                    }
                    out.fragment.bodies.push(body);
                }
            }
        }
        Ok(out)
    }
}

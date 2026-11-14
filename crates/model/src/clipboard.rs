//! Copy and paste (docs/ARCHITECTURE.md §6 decies): what a selection copies, as a fragment
//! that [`Operation::Paste`](crate::Operation::Paste) or
//! [`Operation::PasteMachinings`](crate::Operation::PasteMachinings) places again.
//!
//! A fragment shares the source's solids (`Arc`): copying costs nearly nothing, whatever the
//! model's size.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::document::{Assembly, Component, ComponentKind, Document, EntityPath, Guide, GuideShape, Id, Machining};
use crate::{Error, Result};

/// Machinings and guides copied from an open part, in the part's axes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MachiningFragment {
    pub machinings: Vec<Machining>,
    pub guides: Vec<GuideShape>,
}

impl MachiningFragment {
    pub fn is_empty(&self) -> bool {
        self.machinings.is_empty() && self.guides.is_empty()
    }
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

    /// What copying `ids` — machinings and guides of the part `target` places — copies, in the
    /// part's axes.
    pub fn machinings_fragment(&self, target: &EntityPath, ids: &[Id]) -> Result<MachiningFragment> {
        let (part, p, _) = self.resolve_part(target)?;
        let (guides, machinings) = self.split_guides(part, ids)?;
        let machinings = machinings.iter().map(|id| p.machining(*id).cloned().ok_or(Error::NotFound(*id))).collect::<Result<_>>()?;
        let guides = self.component(part)?.guides.iter().filter(|g| guides.contains(&g.id)).map(|g| g.shape).collect();
        Ok(MachiningFragment { machinings, guides })
    }
}

//! A STEP file read by the kernel ([`StepImport`]) as a document fragment
//! (docs/ARCHITECTURE.md §8 ter), which [`Operation::Insert`](crate::Operation::Insert) then
//! places in the model.
//!
//! A product with one body and nothing else is a part; a product with several bodies, or
//! bodies and products, is an assembly holding one part per body (a woodworker's component
//! of several boards is several parts); a product of products is an assembly. A body's
//! colour becomes a material (one per colour) on the occurrences placing its part.

use std::collections::HashMap;
use std::sync::Arc;

use glam::DAffine3;
use skraft_i18n::t;
use skraft_kernel::StepImport;

use crate::document::{Assembly, ComponentKind, Document, Id, Material, Occurrence};

/// What became of a STEP file's content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StepReport {
    pub parts: usize,
    pub assemblies: usize,
    /// Bodies left out, with the reason.
    pub excluded: usize,
}

impl Document {
    /// The document of a STEP file read by the kernel: its root (named `name`) holds what
    /// stands at the file's top.
    pub fn from_step(step: &StepImport, name: &str) -> (Document, StepReport) {
        let mut doc = Document::new();
        let mut report = StepReport { excluded: step.excluded.len(), ..Default::default() };
        let root = doc.root;
        if let Ok(c) = doc.component_mut(root) {
            c.name = name.to_owned();
        }
        let mut materials: HashMap<[u8; 3], Id> = HashMap::new();
        let mut material = |doc: &mut Document, color: Option<[u8; 3]>| {
            color.map(|c| {
                *materials.entry(c).or_insert_with(|| {
                    let id = doc.fresh_id();
                    let hex = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
                    doc.materials.insert(id, Material { name: t!("default_name.step_color", hex = hex), color: c });
                    id
                })
            })
        };
        // Products come children first (the kernel numbers a product after what it places):
        // each one's component, and the material its occurrences take (a single body's).
        let mut component_of: Vec<Option<(Id, Option<Id>)>> = vec![None; step.products.len()];
        for (k, p) in step.products.iter().enumerate() {
            let product_name = if p.name.is_empty() { None } else { Some(p.name.clone()) };
            let mut occurrences: Vec<Occurrence> = Vec::new();
            if p.children.is_empty()
                && let [body] = p.bodies.as_slice()
            {
                let base = product_name.clone().or_else(|| Some(body.name.clone()).filter(|n| !n.is_empty()));
                let part = doc.plain_part(Arc::new(body.solid.clone()));
                let id = doc.insert_component(
                    &base.clone().unwrap_or_else(|| t!("default_name.part")),
                    base.is_some(),
                    ComponentKind::Part(part),
                );
                report.parts += 1;
                component_of[k] = Some((id, material(&mut doc, body.color)));
                continue;
            }
            for (b, body) in p.bodies.iter().enumerate() {
                let base =
                    Some(body.name.clone()).filter(|n| !n.is_empty()).or_else(|| product_name.as_ref().map(|n| format!("{n} {}", b + 1)));
                let part = doc.plain_part(Arc::new(body.solid.clone()));
                let part = doc.insert_component(
                    &base.clone().unwrap_or_else(|| t!("default_name.part")),
                    base.is_some(),
                    ComponentKind::Part(part),
                );
                report.parts += 1;
                let material = material(&mut doc, body.color);
                let id = doc.fresh_id();
                occurrences.push(Occurrence {
                    id,
                    name: String::new(),
                    component: part,
                    transform: DAffine3::IDENTITY,
                    material,
                    hidden: false,
                });
            }
            for c in &p.children {
                if let Some(Some((component, material))) = component_of.get(c.product) {
                    let id = doc.fresh_id();
                    occurrences.push(Occurrence {
                        id,
                        name: c.name.clone(),
                        component: *component,
                        transform: c.transform,
                        material: *material,
                        hidden: false,
                    });
                }
            }
            if occurrences.is_empty() {
                continue;
            }
            let id = doc.insert_component(
                &product_name.clone().unwrap_or_else(|| t!("default_name.assembly")),
                product_name.is_some(),
                ComponentKind::Assembly(Assembly::new(occurrences)),
            );
            report.assemblies += 1;
            component_of[k] = Some((id, None));
        }
        let mut top = Vec::new();
        for r in &step.roots {
            if let Some(Some((component, material))) = component_of.get(r.product) {
                let id = doc.fresh_id();
                top.push(Occurrence {
                    id,
                    name: r.name.clone(),
                    component: *component,
                    transform: r.transform,
                    material: *material,
                    hidden: false,
                });
            }
        }
        if let Ok(a) = doc.assembly_mut(root) {
            *a.occurrences_mut() = top;
        }
        doc.settle_names(None);
        doc.purge();
        (doc, report)
    }
}

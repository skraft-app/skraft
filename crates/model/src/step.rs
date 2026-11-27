//! A STEP file read by the kernel ([`StepImport`]) as a document fragment
//! (docs/io/step-import.md), which [`Operation::Insert`](crate::Operation::Insert) then
//! places in the model.
//!
//! A product with one body and nothing else is a part; a product with several bodies, or bodies and
//! products, is an assembly holding one part per body (a component made of several solids — a
//! cabinet of boards, a frame of tubes — is several parts); a product of products is an assembly. A
//! body's colour becomes a finish (one per colour) of its part; a product's material
//! designation a material (generic, one per name) of its parts; a product bought a component
//! bought (docs/decisions/0057-materials-on-the-definition-typed-sources.md).

use std::collections::HashMap;
use std::sync::Arc;

use glam::DAffine3;
use skraft_i18n::t;
use skraft_kernel::StepImport;

use crate::document::{Assembly, ComponentKind, Document, Id, Material, MaterialKind, Occurrence, Source};

/// The colour of a material a STEP file names: a neutral grey, the file saying nothing of it.
const MATERIAL_COLOR: [u8; 3] = [190, 190, 190];

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
        let mut finishes: HashMap<[u8; 3], Id> = HashMap::new();
        let mut finish = |doc: &mut Document, color: Option<[u8; 3]>| {
            color.map(|c| {
                *finishes.entry(c).or_insert_with(|| {
                    let id = doc.fresh_id();
                    let hex = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
                    doc.materials.insert(id, Material::new(t!("default_name.step_color", hex = hex), c));
                    id
                })
            })
        };
        let mut materials: HashMap<String, Id> = HashMap::new();
        let mut material = |doc: &mut Document, name: &Option<String>| {
            name.as_ref().map(|n| {
                *materials.entry(n.clone()).or_insert_with(|| {
                    let id = doc.fresh_id();
                    doc.materials.insert(id, Material::of_kind(n.clone(), MATERIAL_COLOR, Some(MaterialKind::Generic)));
                    id
                })
            })
        };
        let source = |p: &skraft_kernel::StepProduct| if p.bought { Source::Bought } else { Source::Made };
        // Products come children first (the kernel numbers a product after what it places):
        // each one's component. A body's colour is its part's material.
        let mut component_of: Vec<Option<Id>> = vec![None; step.products.len()];
        for (k, p) in step.products.iter().enumerate() {
            let product_name = if p.name.is_empty() { None } else { Some(p.name.clone()) };
            let mut occurrences: Vec<Occurrence> = Vec::new();
            if p.children.is_empty()
                && let [body] = p.bodies.as_slice()
            {
                let base = product_name.clone().or_else(|| Some(body.name.clone()).filter(|n| !n.is_empty()));
                let mut part = doc.plain_part(Arc::new(body.solid.clone()));
                part.finish = finish(&mut doc, body.color);
                part.material = material(&mut doc, &p.material);
                let id = doc.insert_component(
                    &base.clone().unwrap_or_else(|| t!("default_name.part")),
                    base.is_some(),
                    ComponentKind::Part(part),
                );
                if let Ok(c) = doc.component_mut(id) {
                    c.source = source(p);
                }
                report.parts += 1;
                component_of[k] = Some(id);
                continue;
            }
            for (b, body) in p.bodies.iter().enumerate() {
                let base =
                    Some(body.name.clone()).filter(|n| !n.is_empty()).or_else(|| product_name.as_ref().map(|n| format!("{n} {}", b + 1)));
                let mut part = doc.plain_part(Arc::new(body.solid.clone()));
                part.finish = finish(&mut doc, body.color);
                part.material = material(&mut doc, &p.material);
                let part = doc.insert_component(
                    &base.clone().unwrap_or_else(|| t!("default_name.part")),
                    base.is_some(),
                    ComponentKind::Part(part),
                );
                report.parts += 1;
                let id = doc.fresh_id();
                occurrences.push(Occurrence {
                    id,
                    name: String::new(),
                    component: part,
                    transform: DAffine3::IDENTITY,
                    hidden: false,
                    props: Default::default(),
                });
            }
            for c in &p.children {
                if let Some(Some(component)) = component_of.get(c.product) {
                    let id = doc.fresh_id();
                    occurrences.push(Occurrence {
                        id,
                        name: c.name.clone(),
                        component: *component,
                        transform: c.transform,
                        hidden: false,
                        props: Default::default(),
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
            if let Ok(c) = doc.component_mut(id) {
                c.source = source(p);
            }
            report.assemblies += 1;
            component_of[k] = Some(id);
        }
        let mut top = Vec::new();
        for r in &step.roots {
            if let Some(Some(component)) = component_of.get(r.product) {
                let id = doc.fresh_id();
                top.push(Occurrence {
                    id,
                    name: r.name.clone(),
                    component: *component,
                    transform: r.transform,
                    hidden: false,
                    props: Default::default(),
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

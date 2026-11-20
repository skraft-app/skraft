//! A SketchUp file read by the kernel ([`SketchUpImport`]) as a document fragment
//! (docs/io/sketchup-import.md), which [`Operation::Insert`](crate::Operation::Insert) then
//! places in the model.
//!
//! Each definition at a scale becomes a component: its faces a **part** (built by the kernel,
//! kept only if closed), its instances an **assembly**; a definition holding both becomes an
//! assembly whose first occurrence is the part of its faces. Instances become occurrences, by
//! the rigid transform the kernel kept on them (their scale is in the geometry).

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::DAffine3;
use skraft_i18n::t;

use crate::Result;
use crate::document::{Assembly, Behavior, ComponentKind, Document, Id, Material, Occurrence, StretchMask};
use skraft_kernel::SketchUpImport;

/// How many of the slowest builds the report keeps.
const SLOWEST: usize = 15;

// ---- the report --------------------------------------------------------------------------

/// What an import did.
#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    pub parts: usize,
    pub assemblies: usize,
    /// Occurrences in the document (each counted once, in its assembly).
    pub occurrences: usize,
    /// Parts placed in the scene (through every path).
    pub placed_parts: usize,
    /// Faces of all the parts built.
    pub faces: usize,
    /// Faces of the parts built painted with a material of their own.
    pub painted: usize,
    /// Definitions (at a given scale) whose faces gave no solid, with the reason.
    pub excluded: Vec<(String, String)>,
    /// Instances dropped: hidden by an excluded part (an assembly emptied by it) or naming
    /// an unknown definition.
    pub dropped_instances: usize,
    /// The slowest builds: time, name, faces, built or not.
    pub slowest: Vec<(Duration, String, usize, bool)>,
    /// What the file reader noticed (records skipped…), in English.
    pub diagnostics: Vec<String>,
    /// Reading the file, building the solids (the kernel's clock), making the components.
    pub read: Duration,
    pub build: Duration,
    pub assemble: Duration,
}

impl Document {
    /// The document of a SketchUp file read by the kernel: its root (named `name`) holds what
    /// stands at the model's top. Definitions whose faces make no closed solid are left out
    /// and reported, with the instances only they filled.
    pub fn from_sketchup(skp: &SketchUpImport, name: &str) -> Result<(Document, ImportReport)> {
        let mut report = ImportReport {
            dropped_instances: skp.dropped,
            diagnostics: skp.diagnostics.clone(),
            read: skp.read,
            build: skp.build,
            ..Default::default()
        };
        let t = Instant::now();
        let mut doc = Document::new();
        let root_id = doc.root;
        doc.component_mut(root_id)?.name = name.to_owned();
        let materials: Vec<Id> = skp
            .materials
            .iter()
            .map(|m| {
                let id = doc.fresh_id();
                let mut material = Material::new(m.name.clone(), m.color);
                // Its texture, its image among the assets: laid at its real size, the image's
                // width along the part's length (docs/render/look.md#textures).
                if let Some(t) = &m.texture
                    && let Some(asset) = crate::Asset::new(t.image.clone())
                {
                    let texture = crate::Texture { image: asset.id(), size: t.size, rotation: 0.0 };
                    if texture.is_valid() {
                        doc.assets.insert(texture.image.clone(), asset);
                        material.texture = Some(texture);
                    }
                }
                doc.materials.insert(id, material);
                id
            })
            .collect();
        let name_of = |def: Option<usize>| def.map_or_else(|| t!("default_name.model"), |d| skp.definitions[d].name.clone());
        let named_def = |def: Option<usize>| def.is_some_and(|d| !skp.definitions[d].group);
        let behavior = |def: Option<usize>| Behavior {
            no_stretch: StretchMask(def.map_or(0, |d| skp.definitions[d].no_scale_mask & StretchMask::ALL)),
            cutters: None,
        };
        // The part of each request, or none.
        let mut times: Vec<(Duration, String, usize, bool)> =
            skp.parts.iter().map(|p| (p.time, name_of(p.definition), p.faces, p.solid.is_ok())).collect();
        times.sort_by_key(|t| std::cmp::Reverse(t.0));
        times.truncate(SLOWEST);
        report.slowest = times;
        let mut part_ids: Vec<Option<Id>> = Vec::with_capacity(skp.parts.len());
        for p in &skp.parts {
            match &p.solid {
                Ok(solid) => {
                    report.faces += p.faces;
                    report.painted += p.painted.len();
                    report.parts += 1;
                    let name = if p.loose { t!("default_name.loose_faces", name = name_of(p.definition)) } else { name_of(p.definition) };
                    let mut part = doc.plain_part(Arc::new(solid.clone()));
                    // Its painted faces, over the occurrences' material.
                    part.faces.extend(p.painted.iter().filter_map(|&(face, m)| Some((face, *materials.get(m)?))));
                    let id = doc.insert_component(&name, named_def(p.definition), ComponentKind::Part(part));
                    doc.component_mut(id)?.behavior = behavior(p.definition);
                    part_ids.push(Some(id));
                }
                Err(why) => {
                    report.excluded.push((name_of(p.definition), why.clone()));
                    part_ids.push(None);
                }
            }
        }
        // Nodes come children first (the kernel numbers a node after those it places): one
        // pass bottom-up. A node is its part alone when it has no children, else an assembly.
        let mut component_of: Vec<Option<Id>> = vec![None; skp.nodes.len()];
        for n in 0..skp.nodes.len() {
            if n == skp.root {
                continue;
            }
            let node = &skp.nodes[n];
            let part = node.part.and_then(|p| part_ids[p]);
            let mut occurrences = Vec::new();
            if let Some(p) = part
                && !node.children.is_empty()
            {
                occurrences.push((p, DAffine3::IDENTITY, String::new(), None));
            }
            for c in &node.children {
                match component_of[c.node] {
                    Some(id) => occurrences.push((id, c.transform, c.name.clone(), c.material)),
                    None => report.dropped_instances += 1,
                }
            }
            component_of[n] = if node.children.is_empty() {
                part
            } else if occurrences.is_empty() {
                None
            } else {
                let mut list = Vec::with_capacity(occurrences.len());
                for (component, transform, name, material) in occurrences {
                    let id = doc.fresh_id();
                    let material = material.and_then(|m| materials.get(m).copied());
                    list.push(Occurrence { id, name, component, transform, material, hidden: false, props: Default::default() });
                }
                let assembly = Assembly::new(list);
                report.assemblies += 1;
                let id = doc.insert_component(&name_of(node.definition), named_def(node.definition), ComponentKind::Assembly(assembly));
                doc.component_mut(id)?.behavior = behavior(node.definition);
                Some(id)
            };
        }
        // The model's own entities go straight into the root.
        let root_node = &skp.nodes[skp.root];
        let mut top = Vec::new();
        if let Some(p) = root_node.part.and_then(|p| part_ids[p]) {
            top.push(Occurrence {
                id: Id(0),
                name: String::new(),
                component: p,
                transform: DAffine3::IDENTITY,
                material: None,
                hidden: false,
                props: Default::default(),
            });
        }
        for c in &root_node.children {
            match component_of[c.node] {
                Some(component) => {
                    let material = c.material.and_then(|m| materials.get(m).copied());
                    top.push(Occurrence {
                        id: Id(0),
                        name: c.name.clone(),
                        component,
                        transform: c.transform,
                        material,
                        hidden: false,
                        props: Default::default(),
                    });
                }
                None => report.dropped_instances += 1,
            }
        }
        for o in &mut top {
            o.id = doc.fresh_id();
        }
        *doc.assembly_mut(root_id)?.occurrences_mut() = top;
        // Parts nothing places any more (only under excluded assemblies) are dropped.
        let used: std::collections::HashSet<Id> =
            doc.components.values().filter_map(|c| c.as_assembly()).flat_map(|a| a.occurrences().iter().map(|o| o.component)).collect();
        let unused: Vec<Id> = doc.components.keys().copied().filter(|id| *id != root_id && !used.contains(id)).collect();
        for id in unused {
            if let Some(c) = doc.components.remove(&id) {
                if c.is_part() {
                    report.parts -= 1;
                } else {
                    report.assemblies -= 1;
                }
            }
        }
        report.occurrences = doc.components.values().filter_map(|c| c.as_assembly()).map(|a| a.occurrences().len()).sum();
        report.placed_parts = doc.placed_parts().len();
        report.assemble = t.elapsed();
        doc.settle_names(None);
        Ok((doc, report))
    }
}

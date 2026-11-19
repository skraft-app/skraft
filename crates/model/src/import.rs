//! Importing a SketchUp model (docs/io/sketchup-import.md), from the JSON that
//! `scripts/sketchup/export.rb` writes inside SketchUp.
//!
//! Each SketchUp definition becomes a component: its faces a **part** (a planar-faced exact
//! B-rep, kept only if closed), its instances an **assembly**; a definition holding both
//! becomes an assembly whose first occurrence is the part of its faces. Instances become
//! occurrences, their transformation split into the rigid T of the occurrence and a scale
//! (or a shear) **baked into the geometry**: a definition placed at another scale gives
//! another component, as a scaled part is another part here (docs/model/data-model.md). Curves stay the
//! segments SketchUp draws: a hole of 24 segments is 24 planar faces.
//!
//! The solids are built by a function the caller gives (the kernel's
//! `Solid::from_polyhedron`), in parallel: this crate does not hold the kernel itself.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use glam::{DAffine3, DMat3, DVec3};
use serde::Deserialize;
use skraft_i18n::t;

use crate::document::{Assembly, Behavior, ComponentKind, Document, Id, Material, Occurrence, StretchMask};
use crate::{Error, FileError, Result};
use skraft_kernel::{PolyFace, Polyhedron, Solid};

/// The `format` field of an export.
pub const SKETCHUP_FORMAT: &str = "skraft-sketchup-export";
/// The export version this build reads.
pub const SKETCHUP_VERSION: u32 = 1;
/// How many of the slowest builds the report keeps.
const SLOWEST: usize = 15;

// ---- the export (scripts/sketchup/export.rb) --------------------------------------------

#[derive(Deserialize)]
struct Export {
    format: String,
    version: u32,
    #[serde(default)]
    materials: Vec<SkMaterial>,
    #[serde(default)]
    definitions: Vec<SkDefinition>,
    root: SkEntities,
}

#[derive(Deserialize)]
struct SkMaterial {
    name: String,
    color: [u8; 3],
}

#[derive(Deserialize)]
struct SkDefinition {
    name: String,
    /// A group's definition: its name is SketchUp's own, not the user's.
    #[serde(default)]
    group: bool,
    /// SketchUp's `Behavior#no_scale_mask`.
    #[serde(default)]
    no_scale_mask: u8,
    #[serde(flatten)]
    entities: SkEntities,
}

/// Visible faces and instances of an entities collection, in mm.
#[derive(Deserialize)]
struct SkEntities {
    #[serde(default)]
    vertices: Vec<[f64; 3]>,
    #[serde(default)]
    faces: Vec<SkFace>,
    #[serde(default)]
    instances: Vec<SkInstance>,
}

#[derive(Deserialize)]
struct SkFace {
    normal: [f64; 3],
    /// Vertex indices, the outer loop first.
    loops: Vec<Vec<u32>>,
}

#[derive(Deserialize)]
struct SkInstance {
    definition: usize,
    #[serde(default)]
    name: String,
    /// Column-major 4×4 (SketchUp's `Transformation#to_a`), the translation in mm.
    transform: [f64; 16],
    #[serde(default)]
    material: Option<usize>,
}

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
    /// Definitions (at a given scale) whose faces gave no solid, with the reason.
    pub excluded: Vec<(String, String)>,
    /// Instances dropped: hidden by an excluded part (an assembly emptied by it) or naming
    /// an unknown definition.
    pub dropped_instances: usize,
    /// The slowest builds: time, name, faces, built or not.
    pub slowest: Vec<(Duration, String, usize, bool)>,
    pub parse: Duration,
    pub build: Duration,
    pub assemble: Duration,
}

// ---- the plan: definitions at a scale ---------------------------------------------------

/// A definition at a scale (`a`, its own axes to the parent's, after the rigid T).
struct Node {
    def: Option<usize>,
    /// The part of its faces (index into `Plan::parts`).
    part: Option<usize>,
    children: Vec<Child>,
}

struct Child {
    node: usize,
    transform: DAffine3,
    name: String,
    material: Option<usize>,
}

struct PartRequest {
    def: Option<usize>,
    a: DMat3,
    /// Its definition holds instances too: the part is named apart from its assembly.
    loose: bool,
}

#[derive(Default)]
struct Plan {
    nodes: Vec<Node>,
    parts: Vec<PartRequest>,
    memo: HashMap<(Option<usize>, [i64; 9]), usize>,
    dropped: usize,
}

/// `m` as a rigid part and the rest: `m = r · d`. When its columns are square to each other,
/// `r` is them made unit (a rotation, maybe a mirror) and `d` the scale along each; else `r` is
/// the identity and `d` all of `m` (a shear, baked into the geometry).
fn split(m: DMat3) -> (DMat3, DMat3) {
    let c = [m.x_axis, m.y_axis, m.z_axis];
    let l = c.map(|v| v.length());
    let square = |a: DVec3, b: DVec3, la: f64, lb: f64| a.dot(b).abs() <= 1e-9 * la * lb;
    if l.iter().all(|x| *x > 1e-12) && square(c[0], c[1], l[0], l[1]) && square(c[1], c[2], l[1], l[2]) && square(c[2], c[0], l[2], l[0]) {
        let snap = |x: f64| if (x - 1.0).abs() < 1e-9 { 1.0 } else { x };
        let r = DMat3::from_cols(c[0] / l[0], c[1] / l[1], c[2] / l[2]);
        (r, DMat3::from_diagonal(DVec3::new(snap(l[0]), snap(l[1]), snap(l[2]))))
    } else {
        (DMat3::IDENTITY, m)
    }
}

fn key(a: &DMat3) -> [i64; 9] {
    a.to_cols_array().map(|x| (x * 1e9).round() as i64)
}

impl Plan {
    /// The node of `def` (None: the model's own entities) at the scale `a`.
    fn node(&mut self, export: &Export, def: Option<usize>, a: DMat3, depth: usize) -> usize {
        if let Some(&n) = self.memo.get(&(def, key(&a))) {
            return n;
        }
        let entities = match def {
            Some(d) => &export.definitions[d].entities,
            None => &export.root,
        };
        let part = (!entities.faces.is_empty()).then(|| {
            self.parts.push(PartRequest { def, a, loose: !entities.instances.is_empty() });
            self.parts.len() - 1
        });
        let mut children = Vec::with_capacity(entities.instances.len());
        for inst in &entities.instances {
            if inst.definition >= export.definitions.len() || depth >= crate::MAX_DEPTH {
                self.dropped += 1;
                continue;
            }
            let t = DAffine3::from_mat4(glam::DMat4::from_cols_array(&inst.transform));
            // A · (L p + t) = (A L) p + A t; A L = R D: the occurrence is (R, A t), the child
            // definition taken at D.
            let (r, d) = split(a * t.matrix3);
            let node = self.node(export, Some(inst.definition), d, depth + 1);
            let transform = DAffine3 { matrix3: r, translation: a * t.translation };
            children.push(Child { node, transform, name: inst.name.clone(), material: inst.material });
        }
        self.nodes.push(Node { def, part, children });
        let n = self.nodes.len() - 1;
        self.memo.insert((def, key(&a)), n);
        n
    }
}

/// The polyhedron of `entities`' faces, through `a`.
fn polyhedron(entities: &SkEntities, a: DMat3) -> Polyhedron {
    // Normals go through the inverse transpose; their side stays outwards.
    let normal_map = a.inverse().transpose();
    Polyhedron {
        vertices: entities.vertices.iter().map(|p| a * DVec3::from_array(*p)).collect(),
        faces: entities
            .faces
            .iter()
            .map(|f| PolyFace { normal: normal_map * DVec3::from_array(f.normal), loops: f.loops.clone() })
            .collect(),
    }
}

/// A request built: the solid, or why not; its faces and the time it took.
type Built = std::result::Result<(Solid, usize, Duration), (String, usize, Duration)>;

/// Builds every request with `build`, on all cores (each thread with the stack SolidCore
/// needs).
fn build_all<F>(export: &Export, requests: &[PartRequest], build: &F) -> Vec<Built>
where
    F: Fn(&Polyhedron) -> skraft_kernel::Result<Solid> + Sync,
{
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(requests.len().max(1));
    let mut out: Vec<Option<Built>> = (0..requests.len()).map(|_| None).collect();
    let done: Vec<Vec<(usize, Built)>> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .filter_map(|_| {
                std::thread::Builder::new()
                    .stack_size(skraft_kernel::KERNEL_STACK)
                    .spawn_scoped(s, || {
                        let mut mine = Vec::new();
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(r) = requests.get(i) else { break };
                            let entities = match r.def {
                                Some(d) => &export.definitions[d].entities,
                                None => &export.root,
                            };
                            let faces = entities.faces.len();
                            let t = Instant::now();
                            let solid = build(&polyhedron(entities, r.a)).map_err(|e| e.to_string());
                            mine.push((i, solid.map(|s| (s, faces, t.elapsed())).map_err(|e| (e, faces, t.elapsed()))));
                        }
                        mine
                    })
                    .ok()
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap_or_default()).collect()
    });
    for (i, r) in done.into_iter().flatten() {
        out[i] = Some(r);
    }
    out.into_iter().map(|r| r.unwrap_or_else(|| Err(("not built".into(), 0, Duration::ZERO)))).collect()
}

impl Document {
    /// A new document from a SketchUp export (`text`, its JSON), the parts' solids built by
    /// `build`. Definitions whose faces make no closed solid are left out and reported.
    pub fn from_sketchup<F>(text: &str, build: &F) -> Result<(Document, ImportReport)>
    where
        F: Fn(&Polyhedron) -> skraft_kernel::Result<Solid> + Sync,
    {
        let unreadable = |e: String| Error::File(FileError::Unreadable(e));
        let t0 = Instant::now();
        let export: Export = serde_json::from_str(text).map_err(|e| unreadable(e.to_string()))?;
        if export.format != SKETCHUP_FORMAT {
            return Err(Error::File(FileError::NotSkraft(export.format)));
        }
        if export.version > SKETCHUP_VERSION {
            return Err(Error::File(FileError::TooNew(export.version)));
        }
        let mut report = ImportReport { parse: t0.elapsed(), ..Default::default() };

        let t1 = Instant::now();
        let mut plan = Plan::default();
        let root = plan.node(&export, None, DMat3::IDENTITY, 0);
        let built = build_all(&export, &plan.parts, build);
        report.dropped_instances = plan.dropped;
        report.build = t1.elapsed();

        let t2 = Instant::now();
        let mut doc = Document::new();
        let materials: Vec<Id> = export
            .materials
            .iter()
            .map(|m| {
                let id = doc.fresh_id();
                doc.materials.insert(id, Material::new(m.name.clone(), m.color));
                id
            })
            .collect();
        let name_of = |def: Option<usize>| def.map_or_else(|| t!("default_name.model"), |d| export.definitions[d].name.clone());
        let named_def = |def: Option<usize>| def.is_some_and(|d| !export.definitions[d].group);
        let behavior = |def: Option<usize>| Behavior {
            no_stretch: StretchMask(def.map_or(0, |d| export.definitions[d].no_scale_mask & StretchMask::ALL)),
            cutters: None,
        };
        // The part of each request, or none.
        let mut part_ids: Vec<Option<Id>> = Vec::with_capacity(built.len());
        let mut times: Vec<(Duration, String, usize, bool)> = built
            .iter()
            .zip(&plan.parts)
            .map(|(b, r)| match b {
                Ok((_, faces, t)) => (*t, name_of(r.def), *faces, true),
                Err((_, faces, t)) => (*t, name_of(r.def), *faces, false),
            })
            .collect();
        times.sort_by_key(|t| std::cmp::Reverse(t.0));
        times.truncate(SLOWEST);
        report.slowest = times;
        for (r, b) in plan.parts.iter().zip(built) {
            match b {
                Ok((solid, faces, _)) => {
                    report.faces += faces;
                    report.parts += 1;
                    let name = if r.loose { t!("default_name.loose_faces", name = name_of(r.def)) } else { name_of(r.def) };
                    let part = doc.plain_part(Arc::new(solid));
                    let id = doc.insert_component(&name, named_def(r.def), ComponentKind::Part(part));
                    doc.component_mut(id)?.behavior = behavior(r.def);
                    part_ids.push(Some(id));
                }
                Err((why, _, _)) => {
                    report.excluded.push((name_of(r.def), why));
                    part_ids.push(None);
                }
            }
        }
        // Nodes come children first (`Plan::node` pushes a node after its children): one
        // pass bottom-up. A node is its part alone when it has no children, else an assembly.
        let mut component_of: Vec<Option<Id>> = vec![None; plan.nodes.len()];
        for n in 0..plan.nodes.len() {
            if n == root {
                continue;
            }
            let node = &plan.nodes[n];
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
                let id = doc.insert_component(&name_of(node.def), named_def(node.def), ComponentKind::Assembly(assembly));
                doc.component_mut(id)?.behavior = behavior(node.def);
                Some(id)
            };
        }
        // The model's own entities go straight into the root.
        let root_node = &plan.nodes[root];
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
                    top.push(Occurrence { id: Id(0), name: c.name.clone(), component, transform: c.transform, material, hidden: false, props: Default::default() });
                }
                None => report.dropped_instances += 1,
            }
        }
        for o in &mut top {
            o.id = doc.fresh_id();
        }
        let root_id = doc.root;
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
        report.assemble = t2.elapsed();
        doc.settle_names(None);
        Ok((doc, report))
    }
}

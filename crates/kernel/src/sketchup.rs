//! A SketchUp file (`.skp`) read by the kernel (docs/io/sketchup-import.md): what the
//! model then turns into components ([`SketchUpImport`]).
//!
//! The file is read by skpwright, a clean-room reader of every `.skp` release (3 to 2027).
//! Each definition placed is taken **at each scale it is placed at**: an instance's
//! transformation is split into a rigid part (rotation, maybe a mirror, and the translation)
//! that stays on the instance and a scale (or a shear) baked into the definition's geometry,
//! as a scaled part is another part here (docs/model/data-model.md). The faces of a definition
//! at a scale make one solid ([`Solid::from_polyhedron`]), kept only if closed. Curves stay the
//! segments SketchUp draws: a hole of 24 segments is 24 planar faces.
//!
//! Materials come with their textures (image and real size), on instances and on faces: a face
//! takes the material of its side facing out, found again in the built solid by its vertices.
//! Hidden entities and those on hidden tags are left out; so are edges alone, images, texts
//! and dimensions.

use std::time::Duration;

use glam::DAffine3;
use serde::{Deserialize, Serialize};

use crate::Solid;

/// A SketchUp model read: its definitions taken at their scales (the nodes), their faces built.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SketchUpImport {
    /// The definitions placed, as the file names them.
    pub definitions: Vec<SketchUpDefinition>,
    /// The materials painted on instances.
    pub materials: Vec<SketchUpMaterial>,
    /// Definitions at a scale, each after the nodes it places.
    pub nodes: Vec<SketchUpNode>,
    /// The node of the model's own entities.
    pub root: usize,
    /// The faces of a node, built.
    pub parts: Vec<SketchUpPart>,
    /// Instances dropped: of a definition the file does not link, or nested too deep.
    pub dropped: usize,
    /// What the reader noticed (records skipped, a fallback taken), in English.
    pub diagnostics: Vec<String>,
    /// Reading the file, then building the solids (zero in the browser, which has no clock
    /// here).
    pub read: Duration,
    pub build: Duration,
}

/// A component or group definition.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SketchUpDefinition {
    pub name: String,
    /// A group's: its name is SketchUp's own, not the user's.
    pub group: bool,
    /// SketchUp's `Behavior#no_scale_mask` (bits: X, Y, Z, XY, XZ, YZ, uniform).
    pub no_scale_mask: u8,
}

/// A material: its name and colour (a texture's average colour), its texture.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SketchUpMaterial {
    pub name: String,
    pub color: [u8; 3],
    pub texture: Option<SketchUpTexture>,
    /// OpenCutList's type of it (`ladb_opencutlist` › `type`: 1 solid wood, 2 sheet good, 3
    /// dimensional, 4 edge, 5 hardware, 6 veneer, 7 machining), when it has one.
    #[serde(default)]
    pub ocl_type: Option<u8>,
}

/// A material's texture: its image (PNG or JPEG, as the file holds it) and the real size it is
/// laid at (mm: width, height).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SketchUpTexture {
    #[serde(with = "crate::job::serde_bytes_compat")]
    pub image: Vec<u8>,
    pub size: [f64; 2],
}

/// A definition (`None`: the model's own entities) at a scale.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SketchUpNode {
    pub definition: Option<usize>,
    /// The part of its faces (index into [`SketchUpImport::parts`]), when it has faces.
    pub part: Option<usize>,
    pub children: Vec<SketchUpChild>,
}

/// An instance: the node it places, by a rigid transform (mm) — its scale is in the node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SketchUpChild {
    pub node: usize,
    pub transform: DAffine3,
    pub name: String,
    /// Index into [`SketchUpImport::materials`].
    pub material: Option<usize>,
}

/// The faces of a node, built: the solid, or why they make none.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SketchUpPart {
    pub definition: Option<usize>,
    /// Its node places instances too: the part is named apart from the assembly.
    pub loose: bool,
    pub faces: usize,
    pub solid: Result<Solid, String>,
    /// Its faces painted: (index of the face in the solid's record, index into
    /// [`SketchUpImport::materials`]), the material of the side facing out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub painted: Vec<(u32, usize)>,
    /// How long it took (zero in the browser).
    pub time: Duration,
}

#[cfg(feature = "engine")]
pub(crate) mod read {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    use glam::{DAffine3, DMat3, DVec3};
    use skpwright::{DefinitionKind, Model};

    use super::{SketchUpChild, SketchUpDefinition, SketchUpImport, SketchUpMaterial, SketchUpNode, SketchUpPart, SketchUpTexture};
    use solidcore_record::brep::Loop;

    use crate::polyhedron::untangled;
    use crate::{Error, PolyFace, Polyhedron, Progress, Result, Solid};

    /// Millimetres in an inch: the file's unit.
    const MM_PER_INCH: f64 = 25.4;
    /// The deepest nesting followed; deeper is treated as a cycle (the model's `MAX_DEPTH`).
    const MAX_DEPTH: usize = 64;

    /// A clock where there is one: `std::time::Instant` panics in the browser.
    #[derive(Clone, Copy)]
    struct Clock(#[cfg(not(target_arch = "wasm32"))] std::time::Instant);

    impl Clock {
        fn start() -> Clock {
            Clock(
                #[cfg(not(target_arch = "wasm32"))]
                std::time::Instant::now(),
            )
        }

        fn elapsed(self) -> std::time::Duration {
            #[cfg(not(target_arch = "wasm32"))]
            return self.0.elapsed();
            #[cfg(target_arch = "wasm32")]
            std::time::Duration::ZERO
        }
    }

    /// The visible faces (mm, in the definition's axes) and instances of an entity list.
    #[derive(Default)]
    struct Entities {
        vertices: Vec<DVec3>,
        faces: Vec<PolyFace>,
        /// Each face's front and back material, into `SketchUpImport::materials`.
        paint: Vec<[Option<usize>; 2]>,
        instances: Vec<Instance>,
    }

    struct Instance {
        /// Index into `Source::entities` (and `SketchUpImport::definitions`).
        definition: usize,
        name: String,
        transform: DAffine3,
        material: Option<usize>,
    }

    /// The file's entity lists as the plan walks them: the model's own, then the definitions
    /// in the order first placed.
    struct Source {
        root: Entities,
        entities: Vec<Entities>,
    }

    /// Reads a `.skp` file: see the module. Reports [`Progress::Reading`], then the solids
    /// built as they are ([`Progress::Bodies`]).
    pub(crate) fn import(bytes: &[u8], report: &(dyn Fn(Progress) + Sync)) -> Result<SketchUpImport> {
        report(Progress::Reading);
        let clock = Clock::start();
        let model = crate::engine::guarded("sketchup", || {
            Model::parse(bytes).map_err(|e| Error::Kernel { call: "sketchup".into(), message: e.to_string() })
        })?;
        let mut out = SketchUpImport {
            diagnostics: model.diagnostics.iter().filter(|d| d.is_desync()).map(|d| format!("{d:?}")).collect(),
            ..Default::default()
        };
        let ocl = ocl_types_in_xml(bytes);
        let source = source(&model, &ocl, &mut out);
        out.read = clock.elapsed();

        // The plan: each definition at each of its scales, children first.
        let clock = Clock::start();
        let mut plan = Plan::default();
        out.root = plan.node(&source, None, DMat3::IDENTITY, 0);
        out.dropped += plan.dropped;
        out.nodes = plan.nodes;

        // Their faces built, on all cores.
        let total = plan.parts.len() as u32;
        report(Progress::Bodies { done: 0, total });
        let done = AtomicU32::new(0);
        out.parts = crate::engine::parallel_map(&plan.parts, |r| {
            let entities = r.definition.map_or(&source.root, |d| &source.entities[d]);
            let t = Clock::start();
            let poly = polyhedron(entities, r.scale);
            let solid = Solid::from_polyhedron(&poly).map_err(|e| e.to_string());
            let painted = solid.as_ref().map(|s| painted(entities, &poly, s)).unwrap_or_default();
            report(Progress::Bodies { done: done.fetch_add(1, Ordering::Relaxed) + 1, total });
            SketchUpPart { definition: r.definition, loose: r.loose, faces: entities.faces.len(), solid, painted, time: t.elapsed() }
        });
        // Cancelled: the solids left were refused at once; the job fails.
        if solidcore_ops::cancel::is_cancelled() {
            return Err(Error::Cancelled);
        }
        out.build = clock.elapsed();
        Ok(out)
    }

    /// The model's entity lists, definitions taken as they are first placed; its definitions
    /// and the materials of its instances into `out`.
    fn source(model: &Model, ocl: &HashMap<String, u8>, out: &mut SketchUpImport) -> Source {
        let visible_layer = |slot: u16| model.layer_of(slot).is_none_or(|l| l.visible);
        // A definition's run (its entities) by the key instances name it with.
        let run_of: HashMap<usize, usize> = model.geometry.iter().enumerate().filter_map(|(i, r)| r.def_index.map(|d| (d, i))).collect();
        let definition_of: HashMap<u32, usize> = model.definition_links.iter().copied().collect();
        let material_of: HashMap<u16, usize> = model.material_links.iter().copied().collect();
        // The names of instances by persistent id: a definition's instances carry none.
        let name_of: HashMap<u32, &str> =
            model.instances.iter().filter(|i| i.pid != 0).filter_map(|i| Some((i.pid, i.name.as_deref()?))).collect();

        let mut index: HashMap<u32, usize> = HashMap::new();
        let queue: std::cell::RefCell<Vec<u32>> = Default::default();
        let materials: std::cell::RefCell<HashMap<usize, usize>> = Default::default();
        // A material slot (0: none) into `out.materials`, added when first used.
        let material_index = |slot: u16, out: &mut SketchUpImport| -> Option<usize> {
            let m = *material_of.get(&slot).filter(|_| slot != 0)?;
            Some(*materials.borrow_mut().entry(m).or_insert_with(|| {
                let mut record = material_record(&model.materials[m]);
                record.ocl_type = ocl_type(model, m).or_else(|| ocl.get(&record.name).copied());
                out.materials.push(record);
                out.materials.len() - 1
            }))
        };
        let faces_of = |r: usize, out: &mut SketchUpImport| faces_of(model, r, &mut |slot| material_index(slot, out));
        let mut placed = |defref: u32, transform: &[f64; 13], material: u16, name: String, out: &mut SketchUpImport| -> Option<Instance> {
            let Some(&k) = definition_of.get(&defref) else {
                out.dropped += 1;
                return None;
            };
            let def = &model.definitions[k];
            if def.kind == DefinitionKind::Image {
                return None;
            }
            let definition = *index.entry(defref).or_insert_with(|| {
                out.definitions.push(SketchUpDefinition {
                    name: def.name.clone(),
                    group: def.kind == DefinitionKind::Group,
                    // The u32 after the behaviour bits: the mask, checked against SketchUp.
                    no_scale_mask: (def.behaviour.extra & 0x7f) as u8,
                });
                queue.borrow_mut().push(defref);
                out.definitions.len() - 1
            });
            let material = material_index(material, out);
            Some(Instance { definition, name, transform: affine(transform), material })
        };

        // The model's own entities: the run of no definition, and the instances outside every
        // definition's run.
        let mut root = model.geometry.iter().position(|r| r.def_index.is_none()).map(|r| faces_of(r, out)).unwrap_or_default();
        for i in &model.instances {
            if model.geometry.iter().any(|r| r.start <= i.offset && i.offset < r.end)
                || i.hidden.unwrap_or(false)
                || !visible_layer(i.layer.unwrap_or(0))
            {
                continue;
            }
            if let Some(inst) = placed(i.defref, &i.transform, i.material.unwrap_or(0), i.name.clone().unwrap_or_default(), out) {
                root.instances.push(inst);
            }
        }
        // The definitions, as they are placed (a queue: placing one may place others).
        let mut entities = Vec::new();
        let mut next = 0;
        loop {
            // The borrow ends here: placing the definition's instances queues others.
            let Some(defref) = queue.borrow().get(next).copied() else { break };
            next += 1;
            let mut e = Entities::default();
            if let Some(&r) = run_of.get(&(defref as usize)) {
                e = faces_of(r, out);
                for p in &model.geometry[r].placed {
                    if p.hidden || !visible_layer(p.layer) {
                        continue;
                    }
                    let name = name_of.get(&p.pid).map_or_else(String::new, |n| (*n).to_owned());
                    if let Some(inst) = placed(p.defref, &p.transform, p.material, name, out) {
                        e.instances.push(inst);
                    }
                }
            }
            entities.push(e);
        }
        Source { root, entities }
    }

    /// The visible faces of run `r`, in mm, with their materials (`material`: a slot into
    /// `SketchUpImport::materials`).
    fn faces_of(model: &Model, r: usize, material: &mut dyn FnMut(u16) -> Option<usize>) -> Entities {
        let visible_layer = |slot: u16| model.layer_of(slot).is_none_or(|l| l.visible);
        let mesh = &model.geometry[r].mesh;
        let mut e = Entities::default();
        let mut at: HashMap<u32, u32> = HashMap::new();
        for f in &mesh.faces {
            if f.hidden || !visible_layer(f.layer) {
                continue;
            }
            let mut ix = |v: u32| {
                *at.entry(v).or_insert_with(|| {
                    // The file's own inches when kept: metres lose the last bits.
                    let p = match mesh.vertices_in.get(v as usize) {
                        Some(p) => DVec3::from_array(*p) * MM_PER_INCH,
                        None => DVec3::from_array(mesh.vertices.get(v as usize).copied().unwrap_or_default()) * 1000.0,
                    };
                    e.vertices.push(p);
                    e.vertices.len() as u32 - 1
                })
            };
            let loops = std::iter::once(&f.outer).chain(&f.holes).map(|l| l.iter().map(|&v| ix(v)).collect()).collect();
            e.faces.push(PolyFace { normal: DVec3::from_array(f.normal), loops });
            e.paint.push([f.front_material, f.back_material].map(|s| s.and_then(&mut *material)));
        }
        e
    }

    /// The faces of `solid` (built from `poly`, of `entities`) painted, by their index in its
    /// record: a face found by its vertices (the polyhedron's faces become the record's one to
    /// one), painted with the material of its side facing out — a face SketchUp holds reversed
    /// shows its back.
    fn painted(entities: &Entities, poly: &Polyhedron, solid: &Solid) -> Vec<(u32, usize)> {
        if entities.paint.iter().all(|p| p == &[None, None]) {
            return Vec::new();
        }
        let Ok((oriented, _)) = poly.oriented() else { return Vec::new() };
        // A face's key: its vertices, rounded to 0.1 µm, sorted.
        let key = |points: &mut dyn Iterator<Item = DVec3>| -> Vec<[i64; 3]> {
            let mut k: Vec<[i64; 3]> = points.map(|p| (p * 1e4).round().to_array().map(|x| x as i64)).collect();
            k.sort_unstable();
            k.dedup();
            k
        };
        let outward: HashMap<Vec<[i64; 3]>, DVec3> =
            oriented.faces.iter().map(|f| (key(&mut f.loops.iter().flatten().map(|&i| oriented.vertices[i as usize])), f.normal)).collect();
        let r = solid.record();
        let record: HashMap<Vec<[i64; 3]>, u32> = r
            .faces
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let vertices = f.loops.iter().flat_map(|l| match l {
                    Loop::Edges { edges } => edges.iter().flat_map(|e| r.edges[e.edge as usize].vertices).collect::<Vec<_>>(),
                    Loop::Vertex { vertex } => vec![*vertex],
                });
                (key(&mut vertices.map(|v| DVec3::from_array(r.vertices[v as usize].point))), i as u32)
            })
            .collect();
        let mut out = Vec::new();
        for (f, paint) in poly.faces.iter().zip(&entities.paint) {
            if paint == &[None, None] {
                continue;
            }
            let k = key(&mut f.loops.iter().flat_map(|l| untangled(l)).flatten().map(|i| poly.vertices[i as usize]));
            let (Some(n), Some(&face)) = (outward.get(&k), record.get(&k)) else { continue };
            if let Some(m) = paint[if f.normal.dot(*n) >= 0.0 { 0 } else { 1 }] {
                out.push((face, m));
            }
        }
        out
    }

    /// SketchUp's 13 values as an affine map in mm: a 3×3 whose first row is `t[0..3]`, the
    /// translation in inches, a homogeneous weight dividing it all.
    fn affine(t: &[f64; 13]) -> DAffine3 {
        let w = if t[12] != 0.0 { t[12] } else { 1.0 };
        DAffine3 {
            matrix3: DMat3::from_cols(DVec3::new(t[0], t[3], t[6]), DVec3::new(t[1], t[4], t[7]), DVec3::new(t[2], t[5], t[8])) / w,
            translation: DVec3::new(t[9], t[10], t[11]) * (MM_PER_INCH / w),
        }
    }

    /// OpenCutList's types of the materials of a 2026 file, by name: SketchUp 2026 keeps a
    /// material's dictionaries in its `materials/<name>/material.xml`, which skpwright 0.6 does
    /// not read (it reads those of the records, as older files hold them).
    fn ocl_types_in_xml(bytes: &[u8]) -> HashMap<String, u8> {
        let mut out = HashMap::new();
        if skpwright::detect_container(bytes) == skpwright::Container::Stream {
            return out;
        }
        let Ok(doc) = skpwright::Document::open(bytes) else { return out };
        for name in doc.entry_names().into_iter().filter(|n| n.starts_with("materials/") && n.ends_with("/material.xml")) {
            let Some(xml) = doc.entry(name).and_then(|x| std::str::from_utf8(x).ok()) else { continue };
            if let (Some(material), Some(ocl)) = (xml_material_name(xml), xml_ocl_type(xml)) {
                out.insert(material, ocl);
            }
        }
        out
    }

    /// The `name` attribute of a material XML's `material` element, unescaped.
    fn xml_material_name(xml: &str) -> Option<String> {
        let tag = &xml[xml.find(":material ")?..];
        let tag = &tag[..tag.find('>')?];
        let start = tag.find(" name=\"")? + 7;
        let value = &tag[start..start + tag[start..].find('"')?];
        Some(value.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"))
    }

    /// The value of `type` in a material XML's dictionary `ladb_opencutlist` (1 to 7).
    fn xml_ocl_type(xml: &str) -> Option<u8> {
        let dictionary = &xml[xml.find("AttributeDictionary name=\"ladb_opencutlist\"")?..];
        let dictionary = &dictionary[..dictionary.find("AttributeDictionary>").unwrap_or(dictionary.len())];
        let attribute = &dictionary[dictionary.find("key=\"type\"")?..];
        let value = &attribute[attribute.find('>')? + 1..];
        let value = value[..value.find('<')?].trim();
        let n: i64 = value.parse().ok().or_else(|| value.parse::<f64>().ok().map(|x| x as i64))?;
        u8::try_from(n).ok().filter(|t| (1..=7).contains(t))
    }

    /// OpenCutList's type of material `m` (its dictionary `ladb_opencutlist`, key `type`: an
    /// integer, or its text from older releases), when known (1 to 7).
    fn ocl_type(model: &Model, m: usize) -> Option<u8> {
        let owner = model.dictionaries.iter().find(|a| a.owner == skpwright::Owner::Material(m))?;
        let dictionary = owner.dictionaries.iter().find(|d| d.name == "ladb_opencutlist")?;
        let (_, value) = dictionary.entries.iter().find(|(k, _)| k == "type")?;
        let n = match value {
            skpwright::Value::Int(n) => i64::from(*n),
            skpwright::Value::Double(x) => *x as i64,
            skpwright::Value::Float(x) => *x as i64,
            skpwright::Value::Text(t) => t.trim().parse().ok()?,
            _ => return None,
        };
        u8::try_from(n).ok().filter(|t| (1..=7).contains(t))
    }

    fn material_record(m: &skpwright::Material) -> SketchUpMaterial {
        match m {
            skpwright::Material::Solid { name, rgba, .. } => {
                SketchUpMaterial { name: name.clone(), color: [rgba[0], rgba[1], rgba[2]], texture: None, ocl_type: None }
            }
            skpwright::Material::Textured { name, avg_rgba, image_bytes, applied_size_in, .. } => {
                let rgba = avg_rgba.unwrap_or([200, 200, 200, 255]);
                // A texture needs its image and the size it is laid at.
                let texture = image_bytes
                    .as_ref()
                    .zip(*applied_size_in)
                    .filter(|(_, (w, h))| *w > 0.0 && *h > 0.0)
                    .map(|(image, (w, h))| SketchUpTexture { image: image.clone(), size: [w * MM_PER_INCH, h * MM_PER_INCH] });
                SketchUpMaterial { name: name.clone(), color: [rgba[0], rgba[1], rgba[2]], texture, ocl_type: None }
            }
        }
    }

    // ---- the plan: definitions at a scale ------------------------------------------------

    struct PartRequest {
        definition: Option<usize>,
        /// Its own axes to the parent's, after the rigid part of the instance.
        scale: DMat3,
        loose: bool,
    }

    #[derive(Default)]
    struct Plan {
        nodes: Vec<SketchUpNode>,
        parts: Vec<PartRequest>,
        memo: HashMap<(Option<usize>, [i64; 9]), usize>,
        dropped: usize,
    }

    /// `m` as a rigid part and the rest: `m = r · d`. When its columns are square to each
    /// other, `r` is them made unit (a rotation, maybe a mirror) and `d` the scale along each;
    /// else `r` is the identity and `d` all of `m` (a shear, baked into the geometry).
    fn split(m: DMat3) -> (DMat3, DMat3) {
        let c = [m.x_axis, m.y_axis, m.z_axis];
        let l = c.map(|v| v.length());
        let square = |a: DVec3, b: DVec3, la: f64, lb: f64| a.dot(b).abs() <= 1e-9 * la * lb;
        if l.iter().all(|x| *x > 1e-12)
            && square(c[0], c[1], l[0], l[1])
            && square(c[1], c[2], l[1], l[2])
            && square(c[2], c[0], l[2], l[0])
        {
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
        /// The node of `definition` (None: the model's own entities) at the scale `a`.
        fn node(&mut self, source: &Source, definition: Option<usize>, a: DMat3, depth: usize) -> usize {
            if let Some(&n) = self.memo.get(&(definition, key(&a))) {
                return n;
            }
            let entities = definition.map_or(&source.root, |d| &source.entities[d]);
            let part = (!entities.faces.is_empty()).then(|| {
                self.parts.push(PartRequest { definition, scale: a, loose: !entities.instances.is_empty() });
                self.parts.len() - 1
            });
            let mut children = Vec::with_capacity(entities.instances.len());
            for inst in &entities.instances {
                if depth >= MAX_DEPTH {
                    self.dropped += 1;
                    continue;
                }
                // A · (L p + t) = (A L) p + A t; A L = R D: the instance is (R, A t), the child
                // definition taken at D.
                let (r, d) = split(a * inst.transform.matrix3);
                let node = self.node(source, Some(inst.definition), d, depth + 1);
                let transform = DAffine3 { matrix3: r, translation: a * inst.transform.translation };
                children.push(SketchUpChild { node, transform, name: inst.name.clone(), material: inst.material });
            }
            self.nodes.push(SketchUpNode { definition, part, children });
            let n = self.nodes.len() - 1;
            self.memo.insert((definition, key(&a)), n);
            n
        }
    }

    /// The polyhedron of `entities`' faces, through `a`.
    fn polyhedron(entities: &Entities, a: DMat3) -> Polyhedron {
        // Normals go through the inverse transpose; their side stays outwards.
        let normal_map = a.inverse().transpose();
        Polyhedron {
            vertices: entities.vertices.iter().map(|p| a * *p).collect(),
            faces: entities.faces.iter().map(|f| PolyFace { normal: normal_map * f.normal, loops: f.loops.clone() }).collect(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// SketchUp's 13 values: a 3×3 by rows, the translation in inches, the weight.
        #[test]
        fn transforms_read_by_rows_in_inches_over_the_weight() {
            // A quarter turn about Z (x → y), moved 1 inch along X, weight 2 (halves it all).
            let t = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 2.0];
            let a = affine(&t);
            assert!((a.transform_vector3(DVec3::X) - DVec3::new(0.0, 0.5, 0.0)).length() < 1e-12);
            assert!((a.translation - DVec3::new(12.7, 0.0, 0.0)).length() < 1e-12);
        }
    }
}

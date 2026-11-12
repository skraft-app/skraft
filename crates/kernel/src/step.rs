//! Importing a STEP file (AP203 / AP214 / AP242): its bodies built as exact solids, its
//! product structure — products, the products they contain, where — and the colour of each
//! body, as plain data the model turns into components (docs/ARCHITECTURE.md §8 ter).
//!
//! The kernel does the whole reading ([`Task::ImportStep`](crate::Task::ImportStep)): the file
//! is parsed once, off the UI thread, by SolidCore's STEP reader (`step-import`), which maps
//! the B-rep bodies; the structure and the colours are read here from the same parsed file;
//! the bodies are built by `step-brep`, in parallel where threads exist.

use glam::DAffine3;
use serde::{Deserialize, Serialize};

use crate::Solid;

/// What a STEP file holds, ready for the model.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StepImport {
    /// Every product (or shape without a product) that holds bodies or other products.
    pub products: Vec<StepProduct>,
    /// What stands at the top of the file, in its axes (mm).
    pub roots: Vec<StepInstance>,
    /// Bodies left out: open (surfaces only) or failing to build.
    pub excluded: Vec<StepExcluded>,
    /// What the reader noticed (units missing, constructs not understood…), in English.
    pub diagnostics: Vec<String>,
}

/// A product: its own bodies, and the products it places.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StepProduct {
    pub name: String,
    pub bodies: Vec<StepBody>,
    pub children: Vec<StepInstance>,
}

/// A solid body of a product, in the product's axes (mm).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepBody {
    /// The body's own name (often empty).
    pub name: String,
    pub solid: Solid,
    /// sRGB, from the body's style (or the one colour all its faces share).
    pub color: Option<[u8; 3]>,
}

/// A product placed: `transform` takes its axes into its parent's (rigid, mm).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepInstance {
    /// Index into [`StepImport::products`].
    pub product: usize,
    /// The instance's name (`NEXT_ASSEMBLY_USAGE_OCCURRENCE`'s), maybe empty.
    pub name: String,
    pub transform: DAffine3,
}

/// A body not imported.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepExcluded {
    pub product: String,
    pub body: String,
    pub reason: String,
}

#[cfg(feature = "engine")]
pub(crate) mod read {
    use std::collections::{HashMap, HashSet};

    use glam::{DAffine3, DMat3, DVec3};
    use step_import::model as sm;
    use step_import::part21::{Instance, Limits, Param, Record, StepFile};

    use super::{StepBody, StepExcluded, StepImport, StepInstance, StepProduct};
    use crate::{Error, Result, Solid};

    fn err(message: impl Into<String>) -> Error {
        Error::Kernel { call: "step".into(), message: message.into() }
    }

    /// Short entity names some AP203 writers use (as `step_import::import` normalises them).
    const ALIASES: [(&str, &str); 11] = [
        ("FCSRF", "FACE_SURFACE"),
        ("FCOTBN", "FACE_OUTER_BOUND"),
        ("PLYLP", "POLY_LOOP"),
        ("CRTPNT", "CARTESIAN_POINT"),
        ("DRCTN", "DIRECTION"),
        ("A2PL3D", "AXIS2_PLACEMENT_3D"),
        ("CLSSHL", "CLOSED_SHELL"),
        ("BSSWK", "B_SPLINE_SURFACE_WITH_KNOTS"),
        ("BSCWK", "B_SPLINE_CURVE_WITH_KNOTS"),
        ("TRMCRV", "TRIMMED_CURVE"),
        ("CMCRSG", "COMPOSITE_CURVE"),
    ];

    /// Reads a whole STEP file: see the module.
    pub(crate) fn import(bytes: &[u8]) -> Result<StepImport> {
        let mut text = text_of(bytes);
        for (short, long) in ALIASES {
            if text.contains(&format!("={short}(")) {
                text = text.replace(&format!("={short}("), &format!("={long}("));
            }
        }
        let limits = Limits::default();
        let file = step_import::part21::parse(text.as_bytes(), &limits).map_err(|e| err(e.to_string()))?;
        let model = crate::engine::guarded("step", || Ok(step_import::map::map_file(&file, &limits)))?;
        let mut out = StepImport::default();
        // The reader says it leaves assembly instancing aside: applied here, not repeated.
        out.diagnostics.extend(model.diagnostics.iter().filter(|d| !d.message.contains("assembly instance")).map(|d| d.message.clone()));
        let structure = Structure::read(&file, &model, &mut out.diagnostics);
        let colors = Colors::read(&file, &model);

        // Every body a product uses, built once, on all cores.
        let bodies: HashMap<u64, &sm::Body> = model.bodies.iter().map(|b| (b.step_id, b)).collect();
        let mut wanted: Vec<u64> = structure.nodes.iter().flat_map(|n| n.bodies.iter().copied()).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let built: HashMap<u64, Result<Solid>> = crate::engine::parallel_map(&wanted, |id| {
            let body = bodies[id];
            if body.kind == sm::BodyKind::ShellModel || body.shells.iter().any(|s| !s.closed) {
                return (*id, Err(err("open: surfaces only")));
            }
            (*id, Solid::from_step_body(body))
        })
        .into_iter()
        .collect();
        for (id, why) in &structure.skipped {
            out.excluded.push(StepExcluded { product: String::new(), body: format!("#{id}"), reason: why.clone() });
        }

        for node in &structure.nodes {
            let mut product = StepProduct { name: node.name.clone(), ..Default::default() };
            for id in &node.bodies {
                let name = bodies[id].name.clone().unwrap_or_default();
                match &built[id] {
                    Ok(solid) => product.bodies.push(StepBody { name, solid: solid.clone(), color: colors.of(bodies[id]) }),
                    Err(e) => out.excluded.push(StepExcluded { product: node.name.clone(), body: name, reason: e.to_string() }),
                }
            }
            product.children = node.children.clone();
            out.products.push(product);
        }
        out.roots = structure.roots;
        Ok(out)
    }

    /// The file's bytes as text. Part 21 wants plain ASCII (other characters escaped as
    /// `\X\hh` / `\X2\…\X0\`), but many writers put them raw: as UTF-8, or in the system's
    /// 8-bit code page (Open CASCADE 7.1, older CAD on Windows). UTF-8 is kept wherever
    /// it is valid; any other byte is read as Windows-1252 (a superset of ISO 8859-1).
    pub(crate) fn text_of(bytes: &[u8]) -> String {
        if let Ok(s) = std::str::from_utf8(bytes) {
            return s.to_owned();
        }
        // 0x80–0x9F in Windows-1252; the five holes are kept as their C1 control.
        const HIGH: [char; 32] = [
            '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', //
            '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
        ];
        let mut out = String::with_capacity(bytes.len() + bytes.len() / 8);
        for chunk in bytes.utf8_chunks() {
            out.push_str(chunk.valid());
            out.extend(chunk.invalid().iter().map(|&b| match b {
                0x80..=0x9F => HIGH[(b - 0x80) as usize],
                _ => b as char,
            }));
        }
        out
    }

    // ---- reading the file ---------------------------------------------------------------

    /// The record of `id` named `name` (simple, or that part of a complex instance).
    fn record<'a>(file: &'a StepFile, id: u64, name: &str) -> Option<&'a Record> {
        file.entities.get(&id)?.record(name)
    }

    fn simple(file: &StepFile, id: u64) -> Option<&Record> {
        match file.entities.get(&id)? {
            Instance::Simple(r) => Some(r),
            Instance::Complex(_) => None,
        }
    }

    fn param_ref(r: &Record, i: usize) -> Option<u64> {
        r.params.get(i)?.as_ref_id()
    }

    fn param_refs(r: &Record, i: usize) -> Vec<u64> {
        r.params.get(i).and_then(Param::as_list).map(|l| l.iter().filter_map(Param::as_ref_id).collect()).unwrap_or_default()
    }

    fn param_str(r: &Record, i: usize) -> String {
        match r.params.get(i) {
            Some(Param::Str(s)) => decode(s).trim().to_owned(),
            _ => String::new(),
        }
    }

    /// A Part 21 string with its control directives decoded: `\X2\…\X0\` (UTF-16),
    /// `\X\hh` (ISO 8859-1), `\S\c` (the upper half of ISO 8859-1), `\\`.
    pub(crate) fn decode(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = String::with_capacity(s.len());
        let mut i = 0;
        let hex = |h: &[u8]| std::str::from_utf8(h).ok().and_then(|h| u32::from_str_radix(h, 16).ok());
        while i < b.len() {
            if b[i] == b'\\' {
                if b[i..].starts_with(b"\\X2\\") || b[i..].starts_with(b"\\X4\\") {
                    let width = if b[i + 2] == b'2' { 4 } else { 8 };
                    let start = i + 4;
                    let Some(end) = s[start..].find("\\X0\\").map(|e| start + e) else { break };
                    let units: Vec<u32> = b[start..end].chunks(width).filter_map(hex).collect();
                    if width == 4 {
                        let units: Vec<u16> = units.iter().map(|u| *u as u16).collect();
                        out.push_str(&String::from_utf16_lossy(&units));
                    } else {
                        out.extend(units.iter().filter_map(|u| char::from_u32(*u)));
                    }
                    i = end + 4;
                    continue;
                }
                if b[i..].starts_with(b"\\X\\") && i + 5 <= b.len() {
                    if let Some(c) = hex(&b[i + 3..i + 5]).and_then(char::from_u32) {
                        out.push(c);
                    }
                    i += 5;
                    continue;
                }
                if b[i..].starts_with(b"\\S\\") && i + 4 <= b.len() {
                    if let Some(c) = char::from_u32(b[i + 3] as u32 + 128) {
                        out.push(c);
                    }
                    i += 4;
                    continue;
                }
                if b[i..].starts_with(b"\\\\") {
                    out.push('\\');
                    i += 2;
                    continue;
                }
            }
            // A plain character (the text is UTF-8: copy the whole char).
            let c = s[i..].chars().next().unwrap_or('?');
            out.push(c);
            i += c.len_utf8();
        }
        out
    }

    /// The length unit of a unit entity, in mm.
    fn unit_mm(file: &StepFile, id: u64, depth: usize) -> Option<f64> {
        if depth > 8 {
            return None;
        }
        let inst = file.entities.get(&id)?;
        if let Some(si) = inst.record("SI_UNIT") {
            let factor = match si.params.first().and_then(Param::as_enum) {
                None => 1.0,
                Some("MILLI") => 1e-3,
                Some("CENTI") => 1e-2,
                Some("DECI") => 1e-1,
                Some("MICRO") => 1e-6,
                Some("KILO") => 1e3,
                Some(_) => return None,
            };
            return Some(factor * 1000.0);
        }
        if let Some(cb) = inst.record("CONVERSION_BASED_UNIT") {
            let m = file.entities.get(&param_ref(cb, 1)?)?;
            let m = m.record("LENGTH_MEASURE_WITH_UNIT").or_else(|| m.record("MEASURE_WITH_UNIT"))?;
            return Some(m.params.first()?.as_f64()? * unit_mm(file, param_ref(m, 1)?, depth + 1)?);
        }
        None
    }

    /// The length unit of a representation context, in mm (1 when it says none).
    fn context_mm(file: &StepFile, ctx: u64) -> f64 {
        let Some(g) = record(file, ctx, "GLOBAL_UNIT_ASSIGNED_CONTEXT") else { return 1.0 };
        param_refs(g, 0)
            .into_iter()
            .find(|u| file.entities.get(u).is_some_and(|i| i.record("LENGTH_UNIT").is_some()))
            .and_then(|u| unit_mm(file, u, 0))
            .unwrap_or(1.0)
    }

    /// A representation: (items, context).
    fn representation(file: &StepFile, id: u64) -> Option<(Vec<u64>, u64)> {
        let reps: Vec<&Record> = match file.entities.get(&id)? {
            Instance::Simple(r) => vec![r],
            Instance::Complex(rs) => rs.iter().collect(),
        };
        reps.into_iter()
            .filter(|r| r.name.ends_with("REPRESENTATION") && r.params.len() == 3)
            .find_map(|r| Some((param_refs(r, 1), param_ref(r, 2)?)))
    }

    fn direction(file: &StepFile, id: Option<u64>) -> Option<DVec3> {
        let r = simple(file, id?)?;
        if r.name != "DIRECTION" {
            return None;
        }
        let l = r.params.get(1)?.as_list()?;
        let c: Vec<f64> = l.iter().filter_map(Param::as_f64).collect();
        (c.len() == 3).then(|| DVec3::new(c[0], c[1], c[2]).normalize_or_zero()).filter(|d| *d != DVec3::ZERO)
    }

    /// An `AXIS2_PLACEMENT_3D` as the frame it places (from its axes to its representation's),
    /// its origin scaled by `mm`.
    fn placement(file: &StepFile, id: u64, mm: f64) -> Option<DAffine3> {
        let r = simple(file, id)?;
        if r.name != "AXIS2_PLACEMENT_3D" {
            return None;
        }
        let p = simple(file, param_ref(r, 1)?)?;
        let c: Vec<f64> = p.params.get(1)?.as_list()?.iter().filter_map(Param::as_f64).collect();
        let origin = DVec3::new(*c.first()?, *c.get(1)?, c.get(2).copied().unwrap_or(0.0)) * mm;
        let z = direction(file, param_ref(r, 2)).unwrap_or(DVec3::Z);
        let x = direction(file, param_ref(r, 3)).unwrap_or(DVec3::X);
        // X made square to Z (ISO 10303-42), then Y = Z × X.
        let mut x = (x - z * x.dot(z)).normalize_or_zero();
        if x == DVec3::ZERO {
            x = z.any_orthonormal_vector();
        }
        Some(DAffine3 { matrix3: DMat3::from_cols(x, z.cross(x), z), translation: origin })
    }

    // ---- the product structure ------------------------------------------------------------

    /// A product, or a shape no product owns but a `MAPPED_ITEM` places.
    struct Node {
        name: String,
        bodies: Vec<u64>,
        children: Vec<StepInstance>,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    enum Key {
        /// A `PRODUCT_DEFINITION`.
        Product(u64),
        /// A representation of no product.
        Shape(u64),
    }

    #[derive(Default)]
    struct Structure {
        nodes: Vec<Node>,
        roots: Vec<StepInstance>,
        /// Bodies the reader skipped (unsupported entities).
        skipped: Vec<(u64, String)>,
    }

    /// The links between entities, read once.
    struct Graph<'a> {
        file: &'a StepFile,
        bodies: HashSet<u64>,
        /// A product definition's shape representations.
        reps_of: HashMap<u64, Vec<u64>>,
        /// The product definition a representation belongs to.
        owner: HashMap<u64, u64>,
        /// Representations joined without a transformation (`SHAPE_REPRESENTATION_RELATIONSHIP`):
        /// the same axes.
        joined: HashMap<u64, Vec<u64>>,
        /// Per parent definition: (child definition, instance name, the relationship placing it).
        usages: HashMap<u64, Vec<(u64, String, Option<u64>)>>,
        /// Definitions placed in another.
        used: HashSet<u64>,
    }

    impl<'a> Graph<'a> {
        fn read(file: &'a StepFile, model: &sm::Model) -> Graph<'a> {
            let mut g = Graph {
                file,
                bodies: model.bodies.iter().map(|b| b.step_id).collect(),
                reps_of: HashMap::new(),
                owner: HashMap::new(),
                joined: HashMap::new(),
                usages: HashMap::new(),
                used: HashSet::new(),
            };
            // The NAUO each product-definition shape stands for, and the relationship placing it.
            let mut placed_by: HashMap<u64, u64> = HashMap::new();
            for inst in file.entities.values() {
                if let Some(r) = inst.record("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")
                    && let (Some(rr), Some(pds)) = (param_ref(r, 0), param_ref(r, 1))
                    && let Some(pds) = record(file, pds, "PRODUCT_DEFINITION_SHAPE")
                    && let Some(usage) = param_ref(pds, 2)
                {
                    placed_by.insert(usage, rr);
                }
            }
            for (&id, inst) in &file.entities {
                if let Some(r) = inst.record("SHAPE_DEFINITION_REPRESENTATION")
                    && let (Some(pds), Some(rep)) = (param_ref(r, 0), param_ref(r, 1))
                    && let Some(pds) = record(file, pds, "PRODUCT_DEFINITION_SHAPE")
                    && let Some(def) = param_ref(pds, 2)
                    && record(file, def, "PRODUCT_DEFINITION").is_some()
                {
                    g.reps_of.entry(def).or_default().push(rep);
                    g.owner.entry(rep).or_insert(def);
                }
                // A plain relationship joins two representations; one with a transformation
                // (a complex instance) places a child, read through its usage.
                if let Instance::Simple(r) = inst
                    && r.name == "SHAPE_REPRESENTATION_RELATIONSHIP"
                    && let (Some(a), Some(b)) = (param_ref(r, 2), param_ref(r, 3))
                {
                    g.joined.entry(a).or_default().push(b);
                    g.joined.entry(b).or_default().push(a);
                }
                if let Some(r) = inst.record("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
                    && let (Some(parent), Some(child)) = (param_ref(r, 3), param_ref(r, 4))
                {
                    // Its name only: its id ("NAUO1") means nothing to the user.
                    let name = param_str(r, 1);
                    g.usages.entry(parent).or_default().push((child, name, placed_by.get(&id).copied()));
                    g.used.insert(child);
                }
            }
            g
        }

        /// The representations sharing axes with `reps` (themselves included).
        fn closure(&self, reps: &[u64]) -> Vec<u64> {
            let mut seen: HashSet<u64> = reps.iter().copied().collect();
            let mut out: Vec<u64> = reps.to_vec();
            let mut k = 0;
            while k < out.len() {
                for &n in self.joined.get(&out[k]).map(Vec::as_slice).unwrap_or(&[]) {
                    if seen.insert(n) {
                        out.push(n);
                    }
                }
                k += 1;
            }
            out
        }

        fn name_of(&self, def: u64) -> String {
            let product = record(self.file, def, "PRODUCT_DEFINITION")
                .and_then(|pd| param_ref(pd, 2))
                .and_then(|f| self.file.entities.get(&f))
                .and_then(|f| match f {
                    Instance::Simple(r) => Some(r),
                    Instance::Complex(rs) => rs.iter().find(|r| r.name.starts_with("PRODUCT_DEFINITION_FORMATION")),
                })
                .and_then(|f| param_ref(f, 2))
                .and_then(|p| record(self.file, p, "PRODUCT"));
            match product {
                Some(p) => Some(param_str(p, 1)).filter(|n| !n.is_empty()).unwrap_or_else(|| param_str(p, 0)),
                None => String::new(),
            }
        }

        /// From `child`'s axes to `parent`'s, by a `REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION`
        /// whose `ITEM_DEFINED_TRANSFORMATION` gives a frame in each.
        fn relationship(&self, rr: u64, child_reps: &[u64]) -> Option<DAffine3> {
            let rel = record(self.file, rr, "REPRESENTATION_RELATIONSHIP")?;
            let (rep1, rep2) = (param_ref(rel, 2)?, param_ref(rel, 3)?);
            let wt = record(self.file, rr, "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")?;
            let idt = record(self.file, param_ref(wt, 0)?, "ITEM_DEFINED_TRANSFORMATION")?;
            let mm = |rep: u64| representation(self.file, rep).map_or(1.0, |(_, ctx)| context_mm(self.file, ctx));
            let f1 = placement(self.file, param_ref(idt, 2)?, mm(rep1))?;
            let f2 = placement(self.file, param_ref(idt, 3)?, mm(rep2))?;
            // rep_1 is the child's (the recommended practice); some writers swap them.
            Some(if child_reps.contains(&rep2) && !child_reps.contains(&rep1) { f1 * f2.inverse() } else { f2 * f1.inverse() })
        }
    }

    impl Structure {
        fn read(file: &StepFile, model: &sm::Model, diagnostics: &mut Vec<String>) -> Structure {
            let g = Graph::read(file, model);
            let mut s = Structure {
                skipped: model.skipped.iter().map(|k| (k.step_id, format!("{}: {}", k.kind, k.reason))).collect(),
                ..Default::default()
            };
            let mut memo: HashMap<Key, Option<usize>> = HashMap::new();
            let mut reached: HashSet<u64> = HashSet::new();
            // Top: the definitions no other places, in file order.
            let mut tops: Vec<u64> = g.reps_of.keys().chain(g.usages.keys()).copied().filter(|d| !g.used.contains(d)).collect();
            tops.sort_unstable();
            tops.dedup();
            for def in tops {
                if let Some(n) = s.node(&g, Key::Product(def), 0, &mut memo, &mut reached, diagnostics) {
                    s.roots.push(StepInstance { product: n, name: String::new(), transform: DAffine3::IDENTITY });
                }
            }
            // Bodies no product reaches (a file without products, or loose shapes): one more
            // node at the top.
            let loose: Vec<u64> = model.bodies.iter().map(|b| b.step_id).filter(|id| !reached.contains(id)).collect();
            if !loose.is_empty() {
                s.nodes.push(Node { name: String::new(), bodies: loose, children: Vec::new() });
                s.roots.push(StepInstance { product: s.nodes.len() - 1, name: String::new(), transform: DAffine3::IDENTITY });
            }
            s
        }

        /// The node of `key`, children first; `None` when it holds nothing.
        fn node(
            &mut self,
            g: &Graph,
            key: Key,
            depth: usize,
            memo: &mut HashMap<Key, Option<usize>>,
            reached: &mut HashSet<u64>,
            diagnostics: &mut Vec<String>,
        ) -> Option<usize> {
            if let Some(n) = memo.get(&key) {
                return *n;
            }
            if depth > 64 {
                diagnostics.push("product structure deeper than 64 levels: cut".into());
                return None;
            }
            // Guards a cycle: a node is "nothing" while it is being read.
            memo.insert(key, None);
            let (name, own) = match key {
                Key::Product(def) => (g.name_of(def), g.reps_of.get(&def).cloned().unwrap_or_default()),
                Key::Shape(rep) => (String::new(), vec![rep]),
            };
            let reps = g.closure(&own);
            let mut bodies = Vec::new();
            let mut children = Vec::new();
            // Children by assembly usage.
            let mut child_defs: HashSet<u64> = HashSet::new();
            if let Key::Product(def) = key {
                for (child, inst_name, rr) in g.usages.get(&def).cloned().unwrap_or_default() {
                    child_defs.insert(child);
                    let child_reps = g.closure(g.reps_of.get(&child).map(Vec::as_slice).unwrap_or(&[]));
                    let transform = match rr.map(|rr| g.relationship(rr, &child_reps)) {
                        Some(Some(t)) => t,
                        Some(None) => {
                            diagnostics.push(format!("#{child}: placement not understood; placed at the origin"));
                            DAffine3::IDENTITY
                        }
                        None => DAffine3::IDENTITY,
                    };
                    if let Some(n) = self.node(g, Key::Product(child), depth + 1, memo, reached, diagnostics) {
                        children.push(StepInstance { product: n, name: inst_name, transform });
                    }
                }
            }
            for rep in &reps {
                let Some((items, ctx)) = representation(g.file, *rep) else { continue };
                for item in items {
                    if g.bodies.contains(&item) {
                        bodies.push(item);
                        reached.insert(item);
                    } else if let Some(mi) = record(g.file, item, "MAPPED_ITEM") {
                        // A shape placed: `target · origin⁻¹` from the mapped representation's
                        // axes to this one's.
                        let Some(map) = param_ref(mi, 1).and_then(|m| record(g.file, m, "REPRESENTATION_MAP")) else { continue };
                        let (Some(origin), Some(mapped)) = (param_ref(map, 0), param_ref(map, 1)) else { continue };
                        let mapped_mm = representation(g.file, mapped).map_or(1.0, |(_, c)| context_mm(g.file, c));
                        let target = param_ref(mi, 2).and_then(|t| placement(g.file, t, context_mm(g.file, ctx)));
                        let origin = placement(g.file, origin, mapped_mm);
                        let (Some(target), Some(origin)) = (target, origin) else {
                            diagnostics.push(format!("#{item}: mapped item placement not understood; skipped"));
                            continue;
                        };
                        let child = match g.owner.get(&mapped) {
                            // Already placed by an assembly usage: not twice.
                            Some(def) if child_defs.contains(def) => continue,
                            Some(def) => Key::Product(*def),
                            None => Key::Shape(mapped),
                        };
                        if let Some(n) = self.node(g, child, depth + 1, memo, reached, diagnostics) {
                            children.push(StepInstance { product: n, name: String::new(), transform: target * origin.inverse() });
                        }
                    }
                }
            }
            let n = (!bodies.is_empty() || !children.is_empty()).then(|| {
                self.nodes.push(Node { name, bodies, children });
                self.nodes.len() - 1
            });
            memo.insert(key, n);
            n
        }
    }

    // ---- colours --------------------------------------------------------------------------

    /// The colour styles of the file, by the item they style.
    struct Colors {
        of_item: HashMap<u64, [u8; 3]>,
    }

    impl Colors {
        fn read(file: &StepFile, _model: &sm::Model) -> Colors {
            let mut of_item = HashMap::new();
            for inst in file.entities.values() {
                let Some(r) = inst.record("STYLED_ITEM").or_else(|| inst.record("OVER_RIDING_STYLED_ITEM")) else { continue };
                let Some(item) = param_ref(r, 2) else { continue };
                if let Some(c) = param_refs(r, 1).into_iter().find_map(|s| colour(file, s, 0)) {
                    of_item.insert(item, c);
                }
            }
            Colors { of_item }
        }

        /// A body's colour: its own, its shell's, or the one all its styled faces share.
        fn of(&self, body: &sm::Body) -> Option<[u8; 3]> {
            if let Some(c) = self.of_item.get(&body.step_id).or_else(|| body.shells.iter().find_map(|s| self.of_item.get(&s.step_id))) {
                return Some(*c);
            }
            let mut faces = body.faces.iter().filter_map(|f| self.of_item.get(&f.step_id));
            let first = *faces.next()?;
            faces.all(|c| *c == first).then_some(first)
        }
    }

    /// The first surface colour reached from a presentation style.
    fn colour(file: &StepFile, id: u64, depth: usize) -> Option<[u8; 3]> {
        if depth > 10 {
            return None;
        }
        let inst = file.entities.get(&id)?;
        if let Some(c) = inst.record("COLOUR_RGB") {
            let v: Vec<f64> = c.params.iter().skip(1).filter_map(Param::as_f64).collect();
            let byte = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
            return (v.len() == 3).then(|| [byte(v[0]), byte(v[1]), byte(v[2])]);
        }
        if let Some(c) = inst.record("DRAUGHTING_PRE_DEFINED_COLOUR") {
            return match param_str(c, 0).as_str() {
                "red" => Some([255, 0, 0]),
                "green" => Some([0, 255, 0]),
                "blue" => Some([0, 0, 255]),
                "yellow" => Some([255, 255, 0]),
                "magenta" => Some([255, 0, 255]),
                "cyan" => Some([0, 255, 255]),
                "black" => Some([0, 0, 0]),
                "white" => Some([255, 255, 255]),
                _ => None,
            };
        }
        let records: Vec<&Record> = match inst {
            Instance::Simple(r) => vec![r],
            Instance::Complex(rs) => rs.iter().collect(),
        };
        // Lines' colours are not the faces'.
        if records.iter().any(|r| r.name == "CURVE_STYLE" || r.name == "POINT_STYLE") {
            return None;
        }
        let refs = records.iter().flat_map(|r| r.params.iter()).flat_map(|p| match p {
            Param::Ref(id) => vec![*id],
            Param::List(l) => l.iter().filter_map(Param::as_ref_id).collect(),
            _ => Vec::new(),
        });
        refs.into_iter().find_map(|r| colour(file, r, depth + 1))
    }

    impl Solid {
        /// A STEP body as an exact solid, built by `step-brep` (from its topology, else
        /// stitched); refused unless closed with every face.
        pub(crate) fn from_step_body(body: &sm::Body) -> Result<Solid> {
            crate::engine::guarded("step body", || {
                let options = step_brep::BuildOptions { mode: step_brep::BuildMode::Strict, ..Default::default() };
                let mut built = step_brep::build_body(body, options).map_err(|e| err(e.message))?;
                if let Some(f) = built.face_failures.first() {
                    return Err(err(format!("{} faces left out (face #{}: {})", built.face_failures.len(), f.face, f.reason)));
                }
                if built.path == step_brep::BuildPath::Stitch && !step_brep::is_closed_solid(&built.brep) {
                    return Err(err("not a closed solid"));
                }
                let mut k = crate::engine::kernel();
                crate::engine::to_record(&mut k, &mut built.brep, "step body")
            })
        }
    }
}

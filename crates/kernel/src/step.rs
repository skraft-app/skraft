//! Importing a STEP file (AP203 / AP214 / AP242): its bodies built as exact solids, its
//! product structure — products, the products they contain, where — and the colour of each
//! body, as plain data the model turns into components (docs/io/step-import.md).
//!
//! The kernel does the whole reading ([`Task::ImportStep`](crate::Task::ImportStep)), off the UI
//! thread, with SolidCore's STEP readers: the file is parsed once (`solidcore_record`'s Part 21
//! parser), its product structure and styles read as data (`step::product`) and walked here into
//! products and placed instances; each body is read from its topology straight into a record
//! (`solidcore_step::topology`), in parallel where threads exist.

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
    use std::sync::atomic::{AtomicU32, Ordering};

    use glam::{DAffine3, DMat3, DVec3};
    use solidcore::geom::step_matrix::{self, Matrix4};
    use solidcore_ops::pcurve::{TrimFace, Trims};
    use solidcore_record::brep::Curve2;
    use solidcore_record::step::p21::{self, Exchange, Param};
    use solidcore_record::step::product::{self, Colour, Frame, ItemKind, OccurrenceKind, ProductStructure, Transformation};
    use solidcore_step::{topology, units};

    use super::{StepBody, StepExcluded, StepImport, StepInstance, StepProduct};
    use crate::{Error, Progress, Result, Solid};

    fn err(message: impl Into<String>) -> Error {
        Error::Kernel { call: "step".into(), message: message.into() }
    }

    /// Reads a whole STEP file: see the module. Reports [`Progress::Reading`], then the bodies
    /// read as they are.
    pub(crate) fn import(bytes: &[u8], report: &(dyn Fn(Progress) + Sync)) -> Result<StepImport> {
        report(Progress::Reading);
        let text = text_of(bytes);
        let ex = crate::engine::guarded("step", || p21::parse(&text).map_err(|e| err(e.to_string())))?;
        let mut out = StepImport::default();
        let structure = match product::read(&ex) {
            Ok(ps) => Some(ps),
            Err(e) => {
                out.diagnostics.push(format!("product structure not read ({e}): bodies only"));
                None
            }
        };
        let structure = Structure::read(&ex, structure.as_ref(), &mut out.diagnostics);
        let colors = Colors::read(&ex, structure.styles.as_slice());

        // Every body a product uses, read once, on all cores.
        let mut wanted: Vec<(u64, Option<u64>)> =
            structure.nodes.iter().flat_map(|n| n.bodies.iter().map(|b| (b.entity, b.context))).collect();
        wanted.sort_unstable();
        wanted.dedup_by_key(|(id, _)| *id);
        let total = wanted.len() as u32;
        report(Progress::Bodies { done: 0, total });
        let done = AtomicU32::new(0);
        let built: HashMap<u64, Result<(Solid, Vec<u64>)>> = crate::engine::parallel_map(&wanted, |&(id, context)| {
            let body = read_body(&ex, id, context);
            report(Progress::Bodies { done: done.fetch_add(1, Ordering::Relaxed) + 1, total });
            (id, body)
        })
        .into_iter()
        .collect();
        // Cancelled: the bodies left were refused at once; the job fails.
        if solidcore_ops::cancel::is_cancelled() {
            return Err(Error::Cancelled);
        }

        for node in &structure.nodes {
            let mut product = StepProduct { name: node.name.clone(), ..Default::default() };
            for b in &node.bodies {
                match &built[&b.entity] {
                    Ok((solid, faces)) => {
                        product.bodies.push(StepBody { name: b.name.clone(), solid: solid.clone(), color: colors.of(&ex, b.entity, faces) })
                    }
                    Err(e) => out.excluded.push(StepExcluded { product: node.name.clone(), body: b.name.clone(), reason: e.to_string() }),
                }
            }
            product.children = node.children.clone();
            out.products.push(product);
        }
        out.roots = structure.roots;
        Ok(out)
    }

    /// One body: its record from the file's topology, made the kernel's (refused unless closed),
    /// and the file's face of each of its faces (for the colours).
    fn read_body(ex: &Exchange, item: u64, context: Option<u64>) -> Result<(Solid, Vec<u64>)> {
        crate::engine::guarded("step body", || {
            if solidcore_ops::cancel::is_cancelled() {
                return Err(Error::Cancelled);
            }
            let context = context.map(|c| units::context(ex, c)).unwrap_or_default();
            let read = topology::read_solid_with(ex, item, context, &KernelTrims).map_err(err)?;
            if read.sheet {
                return Err(err("open: surfaces only"));
            }
            let mut k = crate::engine::kernel();
            let mut b = crate::engine::to_kernel(&mut k, &read.brep, "step body")?;
            Ok((crate::engine::to_record(&mut k, &mut b, "step body")?, read.face_origin))
        })
    }

    /// The pcurves the reader has no closed form for, made by the kernel as it makes its own
    /// trims (SMLib's): the face alone, a sheet whose uses without a pcurve go to the kernel bare
    /// (`to_kernel_with_bare_uses`), comes back with the kernel's.
    struct KernelTrims;

    /// How far, as a fraction of its interval, the kernel may move an edge's ends (setting them
    /// on its vertices again) for its pcurve to be put back on the file's interval.
    const MOVED_INTERVAL: f64 = 1e-3;

    impl Trims for KernelTrims {
        fn trims(&self, face: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String> {
            use solidcore_record::brep as rb;
            // The record's canonical order: edges as the loops first use them, vertices as those
            // edges first use them.
            let mut edge_at: Vec<Option<u32>> = vec![None; face.edges.len()];
            let mut edges_in_order: Vec<usize> = Vec::new();
            let mut uses: Vec<Vec<rb::EdgeUseRef>> = Vec::new();
            let mut bare = Vec::new();
            let mut loops = Vec::with_capacity(face.loops.len());
            for (li, l) in face.loops.iter().enumerate() {
                let mut oriented = Vec::with_capacity(l.len());
                for (p, u) in l.iter().enumerate() {
                    let old = u.edge as usize;
                    let e = face.edges.get(old).ok_or("edge out of range")?;
                    let new = match edge_at[old] {
                        Some(n) => n,
                        None => {
                            edges_in_order.push(old);
                            uses.push(Vec::new());
                            let n = (edges_in_order.len() - 1) as u32;
                            edge_at[old] = Some(n);
                            n
                        }
                    };
                    uses[new as usize].push(rb::EdgeUseRef { face: 0, r#loop: li as u32, position: p as u32, upper: !u.forward });
                    let pcurve = match &u.pcurve {
                        Some(c) => c.clone(),
                        None => {
                            bare.push((0, li as u32, p as u32));
                            // A placeholder on the edge's interval: the kernel makes the trim.
                            let [a, b] = e.interval;
                            Curve2::Spline { knots: vec![a, a, b, b], points: vec![[0.0; 2]; 2] }
                        }
                    };
                    oriented.push(rb::OrientedEdge { edge: new, forward: u.forward, pcurve });
                }
                loops.push(rb::Loop::Edges { edges: oriented });
            }
            if face.edges.iter().any(|e| e.vertices.iter().any(|&v| v as usize >= face.vertices.len())) {
                return Err("vertex out of range".into());
            }
            let mut vertex_at: Vec<Option<u32>> = vec![None; face.vertices.len()];
            let mut vertices = Vec::new();
            let mut edges = Vec::with_capacity(edges_in_order.len());
            for (new, &old) in edges_in_order.iter().enumerate() {
                let e = &face.edges[old];
                let ends = e.vertices.map(|v| {
                    let v = v as usize;
                    *vertex_at[v].get_or_insert_with(|| {
                        vertices.push(rb::Vertex { point: face.vertices[v], tolerance: face.tolerance.max(e.tolerance) });
                        (vertices.len() - 1) as u32
                    })
                });
                edges.push(rb::Edge {
                    curve: e.curve.clone(),
                    nurbs: e.nurbs.cloned(),
                    interval: e.interval,
                    vertices: ends,
                    tolerance: e.tolerance,
                    uses: std::mem::take(&mut uses[new]),
                });
            }
            let sheet = rb::Brep {
                version: rb::VERSION,
                tolerance: face.tolerance,
                regions: vec![rb::Region { shells: vec![0], void: false }],
                shells: vec![rb::Shell::Faces { faces: vec![rb::OrientedFace { face: 0, forward: true }], wires: Vec::new() }],
                faces: vec![rb::Face { surface: face.surface.clone(), nurbs: face.nurbs.cloned(), domain: face.domain, loops }],
                edges,
                vertices,
            };
            bare.sort_unstable();
            let mut k = crate::engine::kernel();
            let (mut b, sent) =
                solidcore_convert::brep::to_kernel_with_bare_uses(&mut k, &sheet, true, &bare).map_err(|e| e.to_string())?;
            let (back, came) = solidcore_convert::brep::to_record(&mut k, &mut b).map_err(|e| e.to_string())?;
            let [back_face] = back.faces.as_slice() else { return Err("the kernel made several faces of one".into()) };
            // Each use found again by its edge (through the kernel's) and its direction.
            let mut out = Vec::with_capacity(face.loops.len());
            for l in &face.loops {
                let mut pcs = Vec::with_capacity(l.len());
                for u in l {
                    let new = edge_at[u.edge as usize].ok_or("edge order")? as usize;
                    let id = sent.edges.get(new).ok_or("edge order")?;
                    let j = came.edges.iter().position(|e| e == id).ok_or("the kernel dropped an edge")?;
                    let (want, got) = (face.edges[u.edge as usize].interval, back.edges[j].interval);
                    let moved = (want[0] - got[0]).abs().max((want[1] - got[1]).abs());
                    let span = (want[1] - want[0]).abs();
                    // The kernel may set an edge's ends on its vertices again, a hair inside: its
                    // pcurve, on that interval, is put back on ours by a linear change of parameter
                    // (its ends stay at the vertices; the reader checks it lifts onto the edge).
                    if moved > MOVED_INTERVAL * span.max(1e-12) {
                        let e = &face.edges[u.edge as usize];
                        let kind = |c: &solidcore_record::brep::Curve| {
                            format!("{c:?}").chars().take_while(|c| c.is_alphanumeric()).collect::<String>()
                        };
                        return Err(format!(
                            "the kernel moved an edge's interval: {want:?} → {got:?} ({}{}; back: {}{})",
                            kind(e.curve),
                            e.nurbs.map_or(String::new(), |n| format!(" as {}", kind(n))),
                            kind(&back.edges[j].curve),
                            back.edges[j].nurbs.as_ref().map_or(String::new(), |n| format!(" as {}", kind(n)))
                        ));
                    }
                    let pc = back_face
                        .loops
                        .iter()
                        .find_map(|bl| match bl {
                            rb::Loop::Edges { edges } => edges.iter().find(|o| o.edge as usize == j && o.forward == u.forward),
                            rb::Loop::Vertex { .. } => None,
                        })
                        .ok_or("the kernel dropped an edge use")?;
                    let mut pcurve = pc.pcurve.clone();
                    if moved > 0.0 && got[1] > got[0] {
                        let knots = match &mut pcurve {
                            Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. } => knots,
                        };
                        for k in knots.iter_mut() {
                            *k = want[0] + (*k - got[0]) * (want[1] - want[0]) / (got[1] - got[0]);
                        }
                    }
                    pcs.push(pcurve);
                }
                out.push(pcs);
            }
            Ok(out)
        }
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

    // ---- placements -------------------------------------------------------------------------

    /// SolidCore's row-vector matrix (rows 0..3 the images of X, Y, Z, row 3 the translation) as
    /// a column-vector affine.
    fn affine(m: &Matrix4) -> DAffine3 {
        let row = |i: usize| DVec3::new(m[i][0], m[i][1], m[i][2]);
        DAffine3 { matrix3: DMat3::from_cols(row(0), row(1), row(2)), translation: row(3) }
    }

    /// A placement or an operator as the frame it places (from its axes to its representation's),
    /// made by the kernel (`step_matrix`).
    fn frame(f: &Frame) -> Option<DAffine3> {
        let m = match f {
            Frame::Placement(p) => step_matrix::frame_matrix(p.origin, p.axis, p.ref_direction)?,
            Frame::Operator(o) => step_matrix::operator_matrix(o.axis1, o.axis2, o.axis3, o.origin, o.scale, o.scale2, o.scale3)?,
        };
        Some(affine(&m))
    }

    // ---- the product structure ----------------------------------------------------------------

    /// A body a node lists: its entity, its name, and the representation context it is read in.
    struct NodeBody {
        entity: u64,
        name: String,
        context: Option<u64>,
    }

    /// A product, or a shape no product owns but a `MAPPED_ITEM` places.
    struct Node {
        name: String,
        bodies: Vec<NodeBody>,
        children: Vec<StepInstance>,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    enum Key {
        /// A definition (index into [`ProductStructure::definitions`]).
        Product(usize),
        /// A representation of no product (index into [`ProductStructure::representations`]).
        Shape(usize),
    }

    #[derive(Default)]
    struct Structure {
        nodes: Vec<Node>,
        roots: Vec<StepInstance>,
        styles: Vec<product::StyledItem>,
    }

    /// What the walk needs from the structure, worked out once.
    struct Graph<'a> {
        ps: &'a ProductStructure,
        /// Per definition: the representations of its shape and those joined to them without a
        /// transformation (the same axes).
        closure: Vec<Vec<usize>>,
        /// Per definition: its child occurrences, in file order.
        children: Vec<Vec<usize>>,
        /// The definition a representation belongs to.
        owner: HashMap<usize, usize>,
    }

    impl<'a> Graph<'a> {
        fn new(ps: &'a ProductStructure) -> Graph<'a> {
            let mut children = vec![Vec::new(); ps.definitions.len()];
            for (i, o) in ps.occurrences.iter().enumerate() {
                // A higher usage overrides a placement deeper down; it places nothing itself.
                if o.kind != OccurrenceKind::SpecifiedHigherUsage
                    && let Some(c) = children.get_mut(o.parent)
                {
                    c.push(i);
                }
            }
            let mut owner = HashMap::new();
            for (d, def) in ps.definitions.iter().enumerate() {
                for &r in &def.shapes {
                    owner.entry(r).or_insert(d);
                }
            }
            let closure = ps.definitions.iter().map(|d| Graph::joined(ps, &d.shapes)).collect();
            Graph { ps, closure, children, owner }
        }

        /// `reps` and the representations joined to them without a transformation.
        fn joined(ps: &ProductStructure, reps: &[usize]) -> Vec<usize> {
            let mut seen: HashSet<usize> = reps.iter().copied().collect();
            let mut out = reps.to_vec();
            let mut k = 0;
            while k < out.len() {
                let r = out[k];
                for l in ps.links.iter().filter(|l| l.transformation.is_none()) {
                    let other = if l.rep_1 == r {
                        l.rep_2
                    } else if l.rep_2 == r {
                        l.rep_1
                    } else {
                        continue;
                    };
                    if seen.insert(other) {
                        out.push(other);
                    }
                }
                k += 1;
            }
            out
        }

        fn name_of(&self, def: usize) -> String {
            let Some(p) = self.ps.definitions.get(def).and_then(|d| self.ps.products.get(d.product)) else { return String::new() };
            Some(p.name.trim().to_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| p.id.trim().to_owned())
        }

        /// From the child's axes to its parent's, by the occurrence's first placing relationship
        /// (the identity when it has none).
        fn occurrence(&self, occurrence: usize) -> Option<DAffine3> {
            let o = &self.ps.occurrences[occurrence];
            let Some(&l) = o.placements.first() else { return Some(DAffine3::IDENTITY) };
            let link = self.ps.links.get(l)?;
            match link.transformation.as_ref() {
                None => Some(DAffine3::IDENTITY),
                Some(Transformation::Frame(f)) => frame(f),
                Some(Transformation::Items { item_1, item_2, .. }) => {
                    let (f1, f2) = (frame(item_1)?, frame(item_2)?);
                    // rep_1 is the child's (the recommended practice); some writers swap them.
                    let child = self.closure.get(o.child).map(Vec::as_slice).unwrap_or(&[]);
                    Some(if child.contains(&link.rep_2) && !child.contains(&link.rep_1) { f1 * f2.inverse() } else { f2 * f1.inverse() })
                }
            }
        }
    }

    impl Structure {
        fn read(ex: &Exchange, ps: Option<&ProductStructure>, diagnostics: &mut Vec<String>) -> Structure {
            let mut s = Structure::default();
            let mut reached: HashSet<u64> = HashSet::new();
            if let Some(ps) = ps {
                s.styles = ps.styles.clone();
                let g = Graph::new(ps);
                let mut memo: HashMap<Key, Option<usize>> = HashMap::new();
                for &def in &ps.roots {
                    if let Some(n) = s.node(&g, Key::Product(def), 0, &mut memo, &mut reached, diagnostics) {
                        s.roots.push(StepInstance { product: n, name: String::new(), transform: DAffine3::IDENTITY });
                    }
                }
                // Representations of bodies no product reaches: their bodies are loose.
                let mut loose = Vec::new();
                for rep in &ps.representations {
                    for item in &rep.items {
                        if matches!(item.kind, ItemKind::Body(_)) && reached.insert(item.entity) {
                            loose.push(NodeBody { entity: item.entity, name: item.name.trim().to_owned(), context: rep.context });
                        }
                    }
                }
                s.push_loose(loose);
            }
            // Topology outside any representation (a file of bare geometry).
            let bare: Vec<NodeBody> = topology::roots(ex)
                .into_iter()
                .filter(|id| !reached.contains(id))
                .map(|id| NodeBody { entity: id, name: entity_name(ex, id), context: None })
                .collect();
            s.push_loose(bare);
            s
        }

        /// One more node at the top for bodies no product holds.
        fn push_loose(&mut self, bodies: Vec<NodeBody>) {
            if !bodies.is_empty() {
                self.nodes.push(Node { name: String::new(), bodies, children: Vec::new() });
                self.roots.push(StepInstance { product: self.nodes.len() - 1, name: String::new(), transform: DAffine3::IDENTITY });
            }
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
            if depth > product::MAX_ASSEMBLY_DEPTH {
                diagnostics.push(format!("product structure deeper than {} levels: cut", product::MAX_ASSEMBLY_DEPTH));
                return None;
            }
            // Guards a cycle: a node is "nothing" while it is being read.
            memo.insert(key, None);
            let (name, reps) = match key {
                Key::Product(def) => (g.name_of(def), g.closure.get(def).cloned().unwrap_or_default()),
                Key::Shape(rep) => (String::new(), Graph::joined(g.ps, &[rep])),
            };
            let mut bodies = Vec::new();
            let mut children = Vec::new();
            // Children by assembly usage.
            let mut child_defs: HashSet<usize> = HashSet::new();
            if let Key::Product(def) = key {
                for &o in g.children.get(def).map(Vec::as_slice).unwrap_or(&[]) {
                    let occ = &g.ps.occurrences[o];
                    child_defs.insert(occ.child);
                    let transform = g.occurrence(o).unwrap_or_else(|| {
                        diagnostics.push(format!("#{}: placement not understood; placed at the origin", occ.entity));
                        DAffine3::IDENTITY
                    });
                    if let Some(n) = self.node(g, Key::Product(occ.child), depth + 1, memo, reached, diagnostics) {
                        // Its name only: its id ("NAUO1") means nothing to the user.
                        children.push(StepInstance { product: n, name: occ.name.trim().to_owned(), transform });
                    }
                }
            }
            for &r in &reps {
                let Some(rep) = g.ps.representations.get(r) else { continue };
                for item in &rep.items {
                    match &item.kind {
                        ItemKind::Body(_) => {
                            reached.insert(item.entity);
                            bodies.push(NodeBody { entity: item.entity, name: item.name.trim().to_owned(), context: rep.context });
                        }
                        ItemKind::Mapped { map, target } => {
                            // A shape placed: `target · origin⁻¹` from the mapped representation's
                            // axes to this one's.
                            let Some(map) = g.ps.maps.get(*map) else { continue };
                            let (Some(target), Some(origin)) = (frame(target), frame(&map.origin)) else {
                                diagnostics.push(format!("#{}: mapped item placement not understood; skipped", item.entity));
                                continue;
                            };
                            let child = match g.owner.get(&map.representation) {
                                // Already placed by an assembly usage: not twice.
                                Some(def) if child_defs.contains(def) => continue,
                                Some(def) => Key::Product(*def),
                                None => Key::Shape(map.representation),
                            };
                            if let Some(n) = self.node(g, child, depth + 1, memo, reached, diagnostics) {
                                children.push(StepInstance { product: n, name: String::new(), transform: target * origin.inverse() });
                            }
                        }
                        ItemKind::Other(_) => {}
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

    /// An entity's `name` (its first parameter, when a string).
    fn entity_name(ex: &Exchange, id: u64) -> String {
        match ex.get(id).and_then(|e| e.params().first()) {
            Some(Param::Str(s)) => s.trim().to_owned(),
            _ => String::new(),
        }
    }

    // ---- colours ------------------------------------------------------------------------------

    /// The surface colours of the file's styles, by the item they style.
    struct Colors {
        of_item: HashMap<u64, [u8; 3]>,
    }

    impl Colors {
        fn read(_ex: &Exchange, styles: &[product::StyledItem]) -> Colors {
            let byte = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
            let rgb = |c: &Colour| match c {
                Colour::Rgb { rgb, .. } => Some(*rgb),
                Colour::PreDefined { rgb, .. } => *rgb,
            };
            let mut of_item = HashMap::new();
            // In file order: an overriding style, later, wins.
            for s in styles {
                // Lines' colours are not the faces'.
                if let Some(c) = s.surface.iter().find_map(|st| st.fill.as_ref().or(st.rendering.as_ref()).and_then(rgb)) {
                    of_item.insert(s.item, c.map(byte));
                }
            }
            Colors { of_item }
        }

        /// A body's colour: its own, its shells', or the one all its styled faces share.
        fn of(&self, ex: &Exchange, body: u64, faces: &[u64]) -> Option<[u8; 3]> {
            if let Some(c) = self.of_item.get(&body) {
                return Some(*c);
            }
            // The shells a solid or a surface model lists.
            let mut shells = Vec::new();
            if let Some(e) = ex.get(body) {
                for p in e.records.iter().flat_map(|r| r.params.iter()) {
                    match p {
                        Param::Ref(id) => shells.push(*id),
                        Param::List(l) => shells.extend(l.iter().filter_map(Param::as_ref_id)),
                        _ => {}
                    }
                }
            }
            if let Some(c) = shells.iter().find_map(|s| self.of_item.get(s)) {
                return Some(*c);
            }
            let mut colours = faces.iter().filter_map(|f| self.of_item.get(f));
            let first = *colours.next()?;
            colours.all(|c| *c == first).then_some(first)
        }
    }
}

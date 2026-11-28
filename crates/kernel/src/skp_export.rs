//! Exporting a model as a SketchUp file (docs/io/export.md,
//! docs/decisions/0059-exporters-step-exact-skp-meshed.md): the products the model gathers as
//! component definitions placed by instances, each part's final shape as faces — a planar face
//! one polygon with its holes, a curved one triangles whose inner edges are softened and
//! smoothed — written by skpwright as a release 2017 or 2026 file, off the UI thread.

use serde::{Deserialize, Serialize};

use crate::ExportProduct;

/// What to write: products, the top one, the release, how fine curved faces are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SkpExport {
    /// Every product written; placements may mirror (SketchUp keeps them).
    pub products: Vec<ExportProduct>,
    /// Index of the top product: without a body of its own, its children stand at the model's
    /// top; else it stands there itself.
    pub root: usize,
    pub release: SkpRelease,
    pub facets: Facets,
}

/// The SketchUp release a file is written for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkpRelease {
    /// Read by SketchUp Make 2017 and every later release.
    #[default]
    R2017,
    /// The current container, read by SketchUp 2021 and later.
    R2026,
}

/// How finely curved faces are cut into facets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facets {
    Coarse,
    /// As Skraft displays them.
    #[default]
    Normal,
    Fine,
}

#[cfg(feature = "engine")]
pub(crate) mod write {
    use std::collections::HashMap;

    use glam::{DAffine3, DVec3};
    use skpwright::{Definition, GeometryRun, Material, Mesh, MeshEdge, MeshFace, Model, PlacedInstance};
    use solidcore_record::brep::Surface;
    use solidcore_tess::{DISPLAY_ANGLE, MeshTolerance};

    use super::{Facets, SkpExport, SkpRelease};
    use crate::{Error, ExportBody, Paint, Result, Solid};

    const MM_PER_INCH: f64 = 25.4;

    fn err(message: impl Into<String>) -> Error {
        Error::Kernel { call: "skp export".into(), message: message.into() }
    }

    impl Facets {
        /// The tolerance a body `size` across is meshed at.
        fn tolerance(self, size: f64) -> MeshTolerance {
            let normal = MeshTolerance::display_of(size);
            match self {
                Facets::Coarse => MeshTolerance { chord: normal.chord * 4.0, angle: Some(DISPLAY_ANGLE * 2.0) },
                Facets::Normal => normal,
                Facets::Fine => MeshTolerance { chord: normal.chord / 4.0, angle: Some(DISPLAY_ANGLE / 2.0) },
            }
        }
    }

    /// A placement as SketchUp stores it: the 3×3 by rows, the translation in inches, w.
    pub(crate) fn transform13(t: &DAffine3) -> [f64; 13] {
        let (x, y, z) = (t.matrix3.x_axis, t.matrix3.y_axis, t.matrix3.z_axis);
        let o = t.translation / MM_PER_INCH;
        [x.x, y.x, z.x, x.y, y.y, z.y, x.z, y.z, z.z, o.x, o.y, o.z, 1.0]
    }

    /// The SketchUp materials of an export: one per paint, by slot (from 1).
    #[derive(Default)]
    struct Materials {
        slots: HashMap<Paint, u16>,
        list: Vec<Material>,
    }

    impl Materials {
        fn slot(&mut self, p: &Paint) -> u16 {
            if let Some(&s) = self.slots.get(p) {
                return s;
            }
            // SketchUp names a material once: a second colour of a name is numbered.
            let taken = |n: &str| self.list.iter().any(|m| matches!(m, Material::Solid { name, .. } if name == n));
            let mut name = if p.name.is_empty() { format!("#{:02X}{:02X}{:02X}", p.rgb[0], p.rgb[1], p.rgb[2]) } else { p.name.clone() };
            let base = name.clone();
            let mut k = 2;
            while taken(&name) {
                name = format!("{base} #{k}");
                k += 1;
            }
            self.list.push(Material::Solid { name, rgba: [p.rgb[0], p.rgb[1], p.rgb[2], 255], opacity: 1.0, colorize_type: 0 });
            let s = self.list.len() as u16;
            self.slots.insert(p.clone(), s);
            s
        }
    }

    /// The bytes of `export` as a SketchUp file, and what its release could not hold; the
    /// bodies' records found by `input`, meshed in parallel.
    pub(crate) fn export<'a>(export: &SkpExport, input: &dyn Fn(u32) -> Result<&'a Solid>) -> Result<(Vec<u8>, Vec<String>)> {
        let products = &export.products;
        let root = products.get(export.root).ok_or_else(|| err("no top product"))?;
        let mut materials = Materials::default();

        // Each part's body meshed once, on every core.
        let bodies: Vec<(&ExportBody, &Solid)> =
            products.iter().flat_map(|p| &p.bodies).map(|b| Ok((b, input(b.solid)?))).collect::<Result<_>>()?;
        let facets = export.facets;
        let meshes = crate::parallel_map(&bodies, |(_, s)| mesh(s, facets));
        let mut meshed: HashMap<*const ExportBody, Mesh> = HashMap::new();
        for ((b, _), m) in bodies.iter().zip(meshes) {
            meshed.insert(*b as *const ExportBody, m?);
        }

        // A definition per product, but the top one when its children stand at the model's top.
        let top_is_model = root.bodies.is_empty();
        let mut model = Model::default();
        let mut def_of: Vec<Option<u32>> = vec![None; products.len()];
        let mut names: HashMap<String, u32> = HashMap::new();
        for (i, p) in products.iter().enumerate() {
            if i == export.root && top_is_model {
                continue;
            }
            let k = model.definitions.len();
            let map = k as u32 + 1;
            // SketchUp names a definition once.
            let n = names.entry(p.name.clone()).or_insert(0);
            *n += 1;
            let name = if *n == 1 { p.name.clone() } else { format!("{}#{}", p.name, n) };
            model.definitions.push(Definition { name, map_index: Some(map as usize), ..Definition::default() });
            model.definition_links.push((map, k));
            def_of[i] = Some(map);
        }

        // An instance's name goes by its persistent id, in `Model::instances`.
        let mut named: Vec<skpwright::Instance> = Vec::new();
        let mut placed = |materials: &mut Materials, child: usize, t: &DAffine3, name: &str| -> Option<PlacedInstance> {
            let c = products.get(child)?;
            // A part's paint on its instance, as OpenCutList reads it.
            let material = c.bodies.first().and_then(|b| b.color.as_ref()).map_or(0, |p| materials.slot(p));
            let pid = if name.is_empty() {
                0
            } else {
                let pid = 1_000_000 + named.len() as u32;
                named.push(skpwright::Instance {
                    pid,
                    definition: None,
                    defref: 0,
                    offset: 0,
                    translation_m: [0.0; 3],
                    transform: [0.0; 13],
                    material: None,
                    name: Some(name.to_owned()),
                    is_group: None,
                    hidden: None,
                    layer: None,
                    slot: None,
                    guid: String::new(),
                    casts_shadows: None,
                    receives_shadows: None,
                    locked: None,
                });
                pid
            };
            Some(PlacedInstance { pid, defref: def_of[child]?, transform: transform13(t), material, ..PlacedInstance::default() })
        };

        for (i, p) in products.iter().enumerate() {
            let Some(map) = def_of[i] else { continue };
            let mut mesh = Mesh::default();
            for b in &p.bodies {
                let mut m = meshed.remove(&(b as *const ExportBody)).unwrap_or_default();
                paint_faces(&mut m, b, &mut materials);
                append(&mut mesh, m);
            }
            let placed = p.children.iter().filter_map(|(c, t, n)| placed(&mut materials, *c, t, n)).collect();
            model.geometry.push(GeometryRun { def_index: Some(map as usize), mesh, placed, ..GeometryRun::default() });
        }
        let top: Vec<PlacedInstance> = if top_is_model {
            root.children.iter().filter_map(|(c, t, n)| placed(&mut materials, *c, t, n)).collect()
        } else {
            placed(&mut materials, export.root, &DAffine3::IDENTITY, "").into_iter().collect()
        };
        model.geometry.push(GeometryRun { def_index: None, placed: top, ..GeometryRun::default() });
        model.instances = named;
        model.material_links = (1..=materials.list.len()).map(|s| (s as u16, s - 1)).collect();
        model.materials = materials.list;

        crate::engine::guarded("skp export", || {
            let written = match export.release {
                SkpRelease::R2017 => model.to_release(skpwright::Release::R2017),
                SkpRelease::R2026 => model.to_archive(skpwright::ArchiveTarget::ByContent),
            };
            written.map_err(|e| err(e.0))
        })
    }

    /// The faces of `m` painted on their own (`b.faces`, by record face) take their slot.
    fn paint_faces(m: &mut Mesh, b: &ExportBody, materials: &mut Materials) {
        // `mesh` leaves each face's record index in its `pid`, cleared here.
        let paints: HashMap<u32, &Paint> = b.faces.iter().map(|(f, p)| (*f, p)).collect();
        for f in &mut m.faces {
            if let Some(p) = paints.get(&f.pid) {
                f.front_material = Some(materials.slot(p));
            }
            f.pid = 0;
        }
    }

    /// `m` added to `to`, its indices moved past `to`'s vertices.
    fn append(to: &mut Mesh, m: Mesh) {
        let base = to.vertices.len() as u32;
        to.vertices.extend(m.vertices);
        to.vertex_pids.extend(m.vertex_pids);
        to.faces.extend(m.faces.into_iter().map(|mut f| {
            f.outer.iter_mut().for_each(|v| *v += base);
            f.holes.iter_mut().flatten().for_each(|v| *v += base);
            f
        }));
        to.edges.extend(m.edges.into_iter().map(|mut e| {
            e.v0 += base;
            e.v1 += base;
            e
        }));
    }

    /// The faces of `solid` as SketchUp draws them (metres): a planar face one polygon, its
    /// holes in it; a curved face its facets, the edges between them softened and smoothed.
    /// Each face's record index is left in its `pid` ([`paint_faces`]).
    pub(crate) fn mesh(solid: &Solid, facets: Facets) -> Result<Mesh> {
        let record = solid.record();
        if record.faces.is_empty() {
            return Ok(Mesh::default());
        }
        let size = solid.vertex_bounds().map(|(lo, hi)| (hi - lo).length()).filter(|s| *s > 1e-6).unwrap_or(100.0);
        let tess = crate::engine::guarded("mesh", || {
            let (plan, _) = solidcore_tess::plan(record, &facets.tolerance(size), size).map_err(|e| Error::Mesh(e.to_string()))?;
            Ok(plan.evaluate().0)
        })?;

        // The facets' corners welded: each face's own copies of its boundary points made one.
        let mut weld: HashMap<[i64; 3], u32> = HashMap::new();
        let mut vertices: Vec<[f64; 3]> = Vec::new();
        let ids: Vec<u32> = tess
            .positions
            .iter()
            .map(|p| {
                let key = [p.x, p.y, p.z].map(|c| (c * 1e6).round() as i64);
                *weld.entry(key).or_insert_with(|| {
                    vertices.push([p.x / 1000.0, p.y / 1000.0, p.z / 1000.0]);
                    (vertices.len() - 1) as u32
                })
            })
            .collect();
        let point = |v: u32| DVec3::from_array(vertices[v as usize]);

        // Triangles by record face, degenerate ones left out.
        let mut by_face: HashMap<u32, Vec<[u32; 3]>> = HashMap::new();
        for (t, f) in tess.triangles.iter().zip(&tess.tri_face) {
            let t = t.map(|i| ids[i as usize]);
            if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                by_face.entry(*f).or_default().push(t);
            }
        }
        let mut faces: Vec<MeshFace> = Vec::new();
        // Per face of the mesh: its record face, and whether that one is curved.
        let mut owner: Vec<(u32, bool)> = Vec::new();
        let mut order: Vec<u32> = by_face.keys().copied().collect();
        order.sort_unstable();
        for f in order {
            let tris = &by_face[&f];
            let planar =
                record.faces.get(f as usize).is_some_and(|x| matches!(x.surface, Surface::Plane { .. } | Surface::PlaneThrough { .. }));
            if planar && let Some(face) = polygon(tris, &point) {
                faces.push(MeshFace { pid: f, ..face });
                owner.push((f, false));
                continue;
            }
            for t in tris {
                let n = (point(t[1]) - point(t[0])).cross(point(t[2]) - point(t[0])).normalize_or_zero();
                if n == DVec3::ZERO {
                    continue;
                }
                faces.push(MeshFace { pid: f, outer: t.to_vec(), normal: n.to_array(), ..MeshFace::default() });
                owner.push((f, !planar));
            }
        }

        // Every edge of every ring, once; soft and smooth between two facets of one curved face.
        let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (k, f) in faces.iter().enumerate() {
            for ring in std::iter::once(&f.outer).chain(&f.holes) {
                for i in 0..ring.len() {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    edges.entry((a.min(b), a.max(b))).or_default().push(k);
                }
            }
        }
        let mut keys: Vec<(u32, u32)> = edges.keys().copied().collect();
        keys.sort_unstable();
        let edges = keys
            .into_iter()
            .map(|(a, b)| {
                let users = &edges[&(a, b)];
                let soft = users.len() == 2 && owner[users[0]].1 && owner[users[0]] == owner[users[1]];
                MeshEdge { v0: a, v1: b, soft, smooth: soft, ..MeshEdge::default() }
            })
            .collect();
        let n = vertices.len();
        Ok(Mesh { vertices, vertex_pids: vec![0; n], faces, edges, vertices_in: Vec::new() })
    }

    /// A planar face's triangles as one polygon: its boundary (the edges one triangle uses)
    /// chained into loops, the widest the outer one (counter-clockwise about the face's normal,
    /// as the triangles turn), the others its holes, turned the same way. None when the
    /// boundary does not close.
    fn polygon(tris: &[[u32; 3]], point: &impl Fn(u32) -> DVec3) -> Option<MeshFace> {
        let mut used: HashMap<(u32, u32), u32> = HashMap::new();
        for t in tris {
            for i in 0..3 {
                *used.entry((t[i], t[(i + 1) % 3])).or_insert(0) += 1;
            }
        }
        let mut next: HashMap<u32, Vec<u32>> = HashMap::new();
        for (&(a, b), _) in used.iter().filter(|((a, b), _)| !used.contains_key(&(*b, *a))) {
            next.entry(a).or_default().push(b);
        }
        let mut loops: Vec<Vec<u32>> = Vec::new();
        let mut starts: Vec<u32> = next.keys().copied().collect();
        starts.sort_unstable();
        for s in starts {
            while next.get(&s).is_some_and(|n| !n.is_empty()) {
                let mut ring = vec![s];
                let mut at = s;
                loop {
                    let b = next.get_mut(&at)?.pop()?;
                    if b == s {
                        break;
                    }
                    ring.push(b);
                    at = b;
                    if ring.len() > tris.len() * 3 {
                        return None;
                    }
                }
                if ring.len() >= 3 {
                    loops.push(ring);
                }
            }
        }
        // Newell's area vector of each loop.
        let area = |r: &[u32]| (0..r.len()).fold(DVec3::ZERO, |s, i| s + point(r[i]).cross(point(r[(i + 1) % r.len()]))) * 0.5;
        let widest = (0..loops.len()).max_by(|a, b| area(&loops[*a]).length().total_cmp(&area(&loops[*b]).length()))?;
        let outer = loops.swap_remove(widest);
        let normal = area(&outer).normalize_or_zero();
        if normal == DVec3::ZERO {
            return None;
        }
        // A hole's boundary runs clockwise about the normal: turned to run as the outer one.
        let holes = loops.into_iter().map(|mut r| {
            r.reverse();
            r
        });
        Some(MeshFace { outer, holes: holes.collect(), normal: normal.to_array(), ..MeshFace::default() })
    }
}

#[cfg(all(test, feature = "engine"))]
mod tests {
    use std::sync::Arc;

    use glam::{DAffine3, DVec3};

    use super::*;
    use crate::{ExportBody, Job, MIRROR, Output, Paint, Solid, Task};

    fn body(solid: u32, color: Option<Paint>) -> ExportBody {
        ExportBody { name: "Planche".into(), solid, mirrored: false, color, faces: vec![] }
    }

    fn written(export: SkpExport, inputs: Vec<Arc<Solid>>) -> Vec<u8> {
        let job = Job { inputs, tasks: vec![Task::ExportSkp { export: Box::new(export) }] };
        match crate::run(&job).unwrap().pop() {
            Some(Output::SkpFile { bytes, .. }) => bytes,
            o => panic!("{o:?}"),
        }
    }

    /// A board placed twice, once mirrored, and a holed plate, in both releases: read back by
    /// the importer with their volumes where they were placed, the board's paint on its
    /// instances.
    #[test]
    fn parts_go_out_as_components_read_back_in_place() {
        let board = Arc::new(Solid::new_box(DVec3::ZERO, DVec3::new(100.0, 20.0, 10.0)).unwrap());
        let hole = crate::Primitive::Cylinder { origin: DVec3::new(50.0, 50.0, -1.0), radius: 10.0, height: 12.0 }.build().unwrap();
        let plate = Solid::new_box(DVec3::ZERO, DVec3::new(100.0, 100.0, 10.0)).unwrap();
        let plate = Arc::new(plate.boolean(&hole, crate::BooleanOp::Difference).unwrap());
        let oak = Paint::new("Chêne", [180, 140, 90]);
        for release in [SkpRelease::R2017, SkpRelease::R2026] {
            let export = SkpExport {
                products: vec![
                    ExportProduct {
                        name: "Meuble".into(),
                        bodies: vec![],
                        children: vec![
                            (1, DAffine3::from_translation(DVec3::new(500.0, 0.0, 0.0)), "Droite".into()),
                            (1, DAffine3::from_translation(DVec3::new(-50.0, 0.0, 0.0)) * MIRROR, "Gauche".into()),
                            (2, DAffine3::from_translation(DVec3::new(0.0, 300.0, 0.0)), String::new()),
                        ],
                    },
                    ExportProduct { name: "Planche".into(), bodies: vec![body(0, Some(oak.clone()))], children: vec![] },
                    ExportProduct { name: "Plaque".into(), bodies: vec![body(1, None)], children: vec![] },
                ],
                root: 0,
                release,
                facets: Facets::Normal,
            };
            let bytes = written(export, vec![board.clone(), plate.clone()]);
            let model = skpwright::Model::parse(&bytes).unwrap();
            assert_eq!(model.definitions.len(), 2, "{release:?}");
            // The plate: one face with a hole on top, the hole's wall soft facets.
            let run = model.geometry.iter().find(|r| r.mesh.faces.iter().any(|f| !f.holes.is_empty())).expect("a face with a hole");
            assert!(run.mesh.edges.iter().any(|e| e.soft && e.smooth), "{release:?}");

            let read = crate::run(&Job { inputs: vec![], tasks: vec![Task::ImportSketchUp { bytes }] }).unwrap();
            let Some(Output::SketchUp(skp)) = read.into_iter().next() else { panic!() };
            let solids: Vec<&Solid> = skp.parts.iter().map(|p| p.solid.as_ref().expect("a closed solid")).collect();
            let mut volumes: Vec<f64> = solids.iter().map(|s| s.volume().unwrap()).collect();
            volumes.sort_by(f64::total_cmp);
            let expected = 100.0 * 100.0 * 10.0 - std::f64::consts::PI * 100.0 * 10.0;
            assert!((volumes[0] - 20000.0).abs() < 1e-3, "{release:?} {volumes:?}");
            assert!((volumes[1] - expected).abs() / expected < 5e-3, "{release:?} {volumes:?}");
        }
    }
}

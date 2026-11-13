//! A solid bounded by planar polygons, as a mesh modeller holds it (a SketchUp definition):
//! shared vertices, faces of one outer loop and maybe holes. [`Solid::from_polyhedron`] turns
//! it into an exact B-rep through SolidCore's STEP topology reader (`solidcore_step::topology`),
//! the faces handed to it as the STEP entities of a B-rep (in memory, never as text): its
//! topology as given, a plane through each polygon, a line along each edge.

use glam::DVec3;

/// Planar faces over shared vertices (mm).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polyhedron {
    pub vertices: Vec<DVec3>,
    pub faces: Vec<PolyFace>,
}

/// A planar face: its loops by vertex index, the outer one first, then its holes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolyFace {
    /// The face's normal (any length), outwards for a solid. Only its side counts: the loops
    /// are turned to agree with it, whichever way they were given.
    pub normal: DVec3,
    pub loops: Vec<Vec<u32>>,
}

/// Twice the area vector of the polygon `ids` (Newell): normal to it, by the right hand.
fn newell(vertices: &[DVec3], ids: &[u32]) -> DVec3 {
    let p = |i: u32| vertices.get(i as usize).copied().unwrap_or(DVec3::ZERO);
    let mut n = DVec3::ZERO;
    for (k, &a) in ids.iter().enumerate() {
        let (a, b) = (p(a), p(ids[(k + 1) % ids.len()]));
        n += DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    n
}

impl Polyhedron {
    /// The same faces ready to build: consecutive repeated vertices dropped, loops of fewer
    /// than three vertices dropped, each outer loop turning about its face's normal and each
    /// hole against it; then, lump by lump, the faces turned alike across their edges (a
    /// reversed face set right) and all of them turned if the lump is inside out (a negative
    /// volume). Also the volume. `Err` names what makes it no solid: open, non-manifold, not
    /// orientable, flat.
    pub fn oriented(&self) -> Result<(Polyhedron, f64), String> {
        let n = self.vertices.len();
        let mut faces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            let mut loops = Vec::with_capacity(f.loops.len());
            for (k, l) in f.loops.iter().enumerate() {
                if l.iter().any(|&i| i as usize >= n) {
                    return Err(format!("vertex index out of range ({n} vertices)"));
                }
                let mut l: Vec<u32> = l.clone();
                l.dedup();
                while l.len() > 1 && l.first() == l.last() {
                    l.pop();
                }
                if l.len() < 3 {
                    if k == 0 {
                        break; // A degenerate outer loop: the face is dropped.
                    }
                    continue;
                }
                let along = newell(&self.vertices, &l).dot(f.normal);
                if (k == 0) == (along < 0.0) {
                    l.reverse();
                }
                loops.push(l);
            }
            if !loops.is_empty() {
                faces.push(PolyFace { normal: f.normal, loops });
            }
        }
        if faces.len() < 4 {
            return Err(format!("{} faces: not a solid", faces.len()));
        }
        // Closed and manifold: each edge between exactly two faces. Checked here, cheaply,
        // because the kernel's fallback for a body its topology does not close (stitching by
        // geometry) takes up to a minute on a few thousand faces.
        let mut uses: std::collections::HashMap<(u32, u32), Vec<(usize, bool)>> = std::collections::HashMap::new();
        for (fi, f) in faces.iter().enumerate() {
            for l in &f.loops {
                for (k, &a) in l.iter().enumerate() {
                    let b = l[(k + 1) % l.len()];
                    uses.entry((a.min(b), a.max(b))).or_default().push((fi, a < b));
                }
            }
        }
        if let Some(u) = uses.values().find(|u| u.len() != 2) {
            return Err(if u.len() == 1 {
                "open: an edge has one face".into()
            } else {
                format!("non-manifold: an edge has {} faces", u.len())
            });
        }
        // The faces turned alike, from one face to the next across their edges (SketchUp may
        // hold a reversed face in a solid): two faces run their common edge each its own way.
        let mut neighbours: Vec<Vec<(usize, bool)>> = vec![Vec::new(); faces.len()];
        for u in uses.values() {
            let ((f, df), (g, dg)) = (u[0], u[1]);
            if f == g {
                return Err("non-manifold: a face meets itself".into());
            }
            // Same way: one of the two must turn.
            neighbours[f].push((g, df == dg));
            neighbours[g].push((f, df == dg));
        }
        let mut flip: Vec<Option<bool>> = vec![None; faces.len()];
        let mut volume_total = 0.0;
        let area_dot = |f: &PolyFace| {
            let area: DVec3 = f.loops.iter().map(|l| newell(&self.vertices, l)).sum::<DVec3>() * 0.5;
            area.dot(self.vertices[f.loops[0][0] as usize])
        };
        for start in 0..faces.len() {
            if flip[start].is_some() {
                continue;
            }
            // One lump: its faces turned alike, then all of them turned if it is inside out.
            flip[start] = Some(false);
            let mut lump = vec![start];
            let mut k = 0;
            while k < lump.len() {
                let f = lump[k];
                k += 1;
                let mine = flip[f].unwrap_or(false);
                for &(g, turn) in &neighbours[f] {
                    let want = mine ^ turn;
                    match flip[g] {
                        None => {
                            flip[g] = Some(want);
                            lump.push(g);
                        }
                        Some(had) if had != want => return Err("not orientable".into()),
                        Some(_) => {}
                    }
                }
            }
            // V = 1/3 Σ (area vector · a point of the face); a hole's vector counts against.
            let signed = |f: usize| if flip[f] == Some(true) { -area_dot(&faces[f]) } else { area_dot(&faces[f]) };
            let volume = lump.iter().map(|&f| signed(f)).sum::<f64>() / 3.0;
            if !volume.is_finite() || volume.abs() < 1e-9 {
                return Err("no volume".into());
            }
            if volume < 0.0 {
                for &f in &lump {
                    flip[f] = flip[f].map(|b| !b);
                }
            }
            volume_total += volume.abs();
        }
        for (f, turn) in faces.iter_mut().zip(flip) {
            if turn == Some(true) {
                f.normal = -f.normal;
                for l in &mut f.loops {
                    l.reverse();
                }
            }
        }
        Ok((Polyhedron { vertices: self.vertices.clone(), faces }, volume_total))
    }
}

#[cfg(feature = "engine")]
mod build {
    use std::collections::HashMap;

    use glam::DVec3;
    use solidcore_record::step::p21::{Entity, Exchange, Param, Record};
    use solidcore_step::topology;

    use super::{Polyhedron, newell};
    use crate::{Error, Result, Solid};

    fn err(message: impl Into<String>) -> Error {
        Error::Kernel { call: "polyhedron".into(), message: message.into() }
    }

    /// The STEP entities of a B-rep, numbered as they are added.
    #[derive(Default)]
    struct Entities(Exchange);

    impl Entities {
        fn add(&mut self, name: &str, params: Vec<Param>) -> u64 {
            let id = self.0.entities.len() as u64 + 1;
            self.0.entities.insert(id, Entity { records: vec![Record { name: name.into(), params }] });
            id
        }
        fn point(&mut self, p: DVec3) -> u64 {
            self.add("CARTESIAN_POINT", vec![label(), reals(p)])
        }
        fn direction(&mut self, d: DVec3) -> u64 {
            self.add("DIRECTION", vec![label(), reals(d)])
        }
    }

    fn label() -> Param {
        Param::Str(String::new())
    }

    fn reals(v: DVec3) -> Param {
        Param::List(v.to_array().into_iter().map(Param::Real).collect())
    }

    fn refs(ids: impl IntoIterator<Item = u64>) -> Param {
        Param::List(ids.into_iter().map(Param::Ref).collect())
    }

    fn flag(b: bool) -> Param {
        Param::Enum(if b { "T" } else { "F" }.into())
    }

    impl Polyhedron {
        /// These faces (already [`Polyhedron::oriented`]) as a STEP B-rep: a plane per face, a
        /// line per edge shared by its two faces, a closed shell per set of faces joined by
        /// edges. The B-rep's entity, and the entities.
        fn exchange(&self) -> Result<(u64, Exchange)> {
            let mut x = Entities::default();
            let vertices: Vec<u64> = self
                .vertices
                .iter()
                .map(|p| {
                    let pt = x.point(*p);
                    x.add("VERTEX_POINT", vec![label(), Param::Ref(pt)])
                })
                .collect();
            let mut edges: HashMap<(u32, u32), u64> = HashMap::new();
            // Per face: the edges (by ends) it uses, to join faces into shells.
            let mut face_ids = Vec::with_capacity(self.faces.len());
            let mut face_edges: Vec<Vec<(u32, u32)>> = Vec::with_capacity(self.faces.len());
            for (fi, f) in self.faces.iter().enumerate() {
                let outer = &f.loops[0];
                let o = self.vertices[outer[0] as usize];
                let z = newell(&self.vertices, outer).normalize_or_zero();
                // X along the first edge not along Z, made square to it.
                let dx = outer
                    .iter()
                    .skip(1)
                    .map(|&i| {
                        let d = self.vertices[i as usize] - o;
                        (d - z * d.dot(z)).normalize_or_zero()
                    })
                    .find(|x| *x != DVec3::ZERO)
                    .unwrap_or(DVec3::ZERO);
                if z == DVec3::ZERO || dx == DVec3::ZERO {
                    return Err(err(format!("face {fi}: degenerate polygon")));
                }
                let (po, pz, px) = (x.point(o), x.direction(z), x.direction(dx));
                let axes = x.add("AXIS2_PLACEMENT_3D", vec![label(), Param::Ref(po), Param::Ref(pz), Param::Ref(px)]);
                let plane = x.add("PLANE", vec![label(), Param::Ref(axes)]);
                let mut bounds = Vec::with_capacity(f.loops.len());
                let mut used = Vec::new();
                for (k, l) in f.loops.iter().enumerate() {
                    let mut oriented = Vec::with_capacity(l.len());
                    for (j, &s) in l.iter().enumerate() {
                        let e = l[(j + 1) % l.len()];
                        let key = (s.min(e), s.max(e));
                        let edge = match edges.get(&key) {
                            Some(&id) => id,
                            None => {
                                let (p, q) = (self.vertices[key.0 as usize], self.vertices[key.1 as usize]);
                                let len = (q - p).length();
                                if len < 1e-9 {
                                    return Err(err(format!("face {fi}: zero-length edge")));
                                }
                                let (start, dir) = (x.point(p), x.direction((q - p) / len));
                                let vector = x.add("VECTOR", vec![label(), Param::Ref(dir), Param::Real(len)]);
                                let line = x.add("LINE", vec![label(), Param::Ref(start), Param::Ref(vector)]);
                                let [a, b] = [key.0, key.1].map(|v| Param::Ref(vertices[v as usize]));
                                let id = x.add("EDGE_CURVE", vec![label(), a, b, Param::Ref(line), flag(true)]);
                                edges.insert(key, id);
                                id
                            }
                        };
                        used.push(key);
                        oriented.push(x.add("ORIENTED_EDGE", vec![label(), Param::Derived, Param::Derived, Param::Ref(edge), flag(s == key.0)]));
                    }
                    let lp = x.add("EDGE_LOOP", vec![label(), refs(oriented)]);
                    bounds.push(x.add(if k == 0 { "FACE_OUTER_BOUND" } else { "FACE_BOUND" }, vec![label(), Param::Ref(lp), flag(true)]));
                }
                face_ids.push(x.add("ADVANCED_FACE", vec![label(), refs(bounds), Param::Ref(plane), flag(true)]));
                face_edges.push(used);
            }
            // One shell per set of faces joined by edges (a SketchUp definition may hold two
            // separate boxes): each its own lump.
            let mut parent: Vec<usize> = (0..face_ids.len()).collect();
            fn find(parent: &mut [usize], mut i: usize) -> usize {
                while parent[i] != i {
                    parent[i] = parent[parent[i]];
                    i = parent[i];
                }
                i
            }
            let mut first_face: HashMap<(u32, u32), usize> = HashMap::new();
            for (fi, used) in face_edges.iter().enumerate() {
                for key in used {
                    match first_face.get(key) {
                        None => {
                            first_face.insert(*key, fi);
                        }
                        Some(&other) => {
                            let (a, b) = (find(&mut parent, fi), find(&mut parent, other));
                            parent[a] = b;
                        }
                    }
                }
            }
            let mut shell_of: HashMap<usize, usize> = HashMap::new();
            let mut shells: Vec<Vec<u64>> = Vec::new();
            for (fi, &face) in face_ids.iter().enumerate() {
                let root = find(&mut parent, fi);
                let s = *shell_of.entry(root).or_insert_with(|| {
                    shells.push(Vec::new());
                    shells.len() - 1
                });
                shells[s].push(face);
            }
            let shells: Vec<u64> = shells.into_iter().map(|faces| x.add("CLOSED_SHELL", vec![label(), refs(faces)])).collect();
            // One shell: a solid; several: a model of closed shells, read as one body of lumps.
            let body = match shells.as_slice() {
                [one] => x.add("MANIFOLD_SOLID_BREP", vec![label(), Param::Ref(*one)]),
                _ => x.add("SHELL_BASED_SURFACE_MODEL", vec![label(), refs(shells)]),
            };
            Ok((body, x.0))
        }
    }

    impl Solid {
        /// The exact solid bounded by these planar faces: read from their topology by
        /// SolidCore's STEP reader, refused unless it makes a closed solid with every face.
        pub fn from_polyhedron(poly: &Polyhedron) -> Result<Solid> {
            let (poly, _volume) = poly.oriented().map_err(err)?;
            let (body, ex) = poly.exchange()?;
            crate::engine::guarded("polyhedron", || {
                // Lines on planes: every pcurve has its closed form, no kernel trims needed.
                let read = topology::read_solid(&ex, body, Default::default()).map_err(err)?;
                if read.sheet {
                    return Err(err("not a closed solid"));
                }
                let mut k = crate::engine::kernel();
                let mut b = crate::engine::to_kernel(&mut k, &read.brep, "polyhedron")?;
                crate::engine::to_record(&mut k, &mut b, "polyhedron")
            })
        }
    }
}

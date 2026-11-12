//! A solid bounded by planar polygons, as a mesh modeller holds it (a SketchUp definition):
//! shared vertices, faces of one outer loop and maybe holes. [`Solid::from_polyhedron`] turns
//! it into an exact B-rep through SolidCore's STEP body builder (`step-brep`), the way a
//! STEP `FACETED_BREP` is read: its topology as given, its planes through its polygons.

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
    use step_import::model as sm;

    use super::{Polyhedron, newell};
    use crate::{Error, Result, Solid};

    fn err(message: impl Into<String>) -> Error {
        Error::Kernel { call: "polyhedron".into(), message: message.into() }
    }

    impl Polyhedron {
        /// The neutral STEP body of these faces (already [`Polyhedron::oriented`]): one closed
        /// shell, a plane per face, a line per edge shared by its two faces.
        fn body(&self) -> Result<sm::Body> {
            let a = |v: DVec3| v.to_array();
            let vertices: Vec<sm::Vertex> =
                self.vertices.iter().enumerate().map(|(i, p)| sm::Vertex { step_id: i as u64 + 1, point: a(*p) }).collect();
            let mut edges: Vec<sm::Edge> = Vec::new();
            let mut by_ends: HashMap<(u32, u32), usize> = HashMap::new();
            let mut faces = Vec::with_capacity(self.faces.len());
            for (fi, f) in self.faces.iter().enumerate() {
                let outer = &f.loops[0];
                let o = self.vertices[outer[0] as usize];
                let z = newell(&self.vertices, outer).normalize_or_zero();
                // X along the first edge not along Z, made square to it.
                let x = outer
                    .iter()
                    .skip(1)
                    .map(|&i| {
                        let d = self.vertices[i as usize] - o;
                        (d - z * d.dot(z)).normalize_or_zero()
                    })
                    .find(|x| *x != DVec3::ZERO)
                    .unwrap_or(DVec3::ZERO);
                if z == DVec3::ZERO || x == DVec3::ZERO {
                    return Err(err(format!("face {fi}: degenerate polygon")));
                }
                let frame = sm::Frame { origin: a(o), x: a(x), y: a(z.cross(x)), z: a(z) };
                let mut loops = Vec::with_capacity(f.loops.len());
                for (k, l) in f.loops.iter().enumerate() {
                    let mut oriented = Vec::with_capacity(l.len());
                    for (j, &s) in l.iter().enumerate() {
                        let e = l[(j + 1) % l.len()];
                        let key = (s.min(e), s.max(e));
                        let edge = match by_ends.get(&key) {
                            Some(&i) => i,
                            None => {
                                let (p, q) = (self.vertices[key.0 as usize], self.vertices[key.1 as usize]);
                                let len = (q - p).length();
                                if len < 1e-9 {
                                    return Err(err(format!("face {fi}: zero-length edge")));
                                }
                                edges.push(sm::Edge {
                                    step_id: 0,
                                    start: key.0 as usize,
                                    end: key.1 as usize,
                                    curve: sm::Curve::Line { origin: a(p), dir: a((q - p) / len), magnitude: len },
                                    same_sense: true,
                                    t_start: 0.0,
                                    t_end: 1.0,
                                });
                                by_ends.insert(key, edges.len() - 1);
                                edges.len() - 1
                            }
                        };
                        oriented.push(sm::OrientedEdge { edge, forward: s == key.0 });
                    }
                    loops.push(sm::Loop { outer: k == 0, orientation: true, bound: sm::Bound::Edges(oriented) });
                }
                faces.push(sm::Face { step_id: fi as u64 + 1, surface: sm::Surface::Plane { frame }, same_sense: true, loops });
            }
            // One shell per set of faces joined by edges (a SketchUp definition may hold two
            // separate boxes): each its own lump.
            let mut parent: Vec<usize> = (0..faces.len()).collect();
            fn find(parent: &mut [usize], mut i: usize) -> usize {
                while parent[i] != i {
                    parent[i] = parent[parent[i]];
                    i = parent[i];
                }
                i
            }
            let mut first_face: Vec<Option<usize>> = vec![None; edges.len()];
            for (fi, f) in faces.iter().enumerate() {
                for l in &f.loops {
                    let sm::Bound::Edges(es) = &l.bound else { continue };
                    for oe in es {
                        match first_face[oe.edge] {
                            None => first_face[oe.edge] = Some(fi),
                            Some(other) => {
                                let (a, b) = (find(&mut parent, fi), find(&mut parent, other));
                                parent[a] = b;
                            }
                        }
                    }
                }
            }
            let mut shell_of: HashMap<usize, usize> = HashMap::new();
            let mut shells: Vec<sm::Shell> = Vec::new();
            for fi in 0..faces.len() {
                let root = find(&mut parent, fi);
                let s = *shell_of.entry(root).or_insert_with(|| {
                    shells.push(sm::Shell { step_id: 0, closed: true, orientation: true, faces: Vec::new() });
                    shells.len() - 1
                });
                shells[s].faces.push(fi);
            }
            Ok(sm::Body {
                step_id: 0,
                name: None,
                kind: sm::BodyKind::ManifoldSolid,
                unit_mm: 1.0,
                uncertainty_mm: None,
                shells,
                faces,
                edges,
                vertices,
            })
        }
    }

    impl Solid {
        /// The exact solid bounded by these planar faces: built from their topology by
        /// SolidCore's STEP body builder, refused unless it makes a closed solid with every
        /// face (a face the builder leaves out, an open or non-manifold shell: an error).
        pub fn from_polyhedron(poly: &Polyhedron) -> Result<Solid> {
            let (poly, _volume) = poly.oriented().map_err(err)?;
            let body = poly.body()?;
            crate::engine::guarded("polyhedron", || {
                // The topology alone: `oriented` checked it closes, and planar faces need no
                // seam. No stitched fallback (slow), no volume check (the integration costs).
                let mut built = step_brep::build_from_topology(&body, step_brep::BuildOptions::default()).map_err(|e| err(e.message))?;
                if let Some(f) = built.face_failures.first() {
                    return Err(err(format!("{} faces left out (face {}: {})", built.face_failures.len(), f.face, f.reason)));
                }
                let mut k = crate::engine::kernel();
                crate::engine::to_record(&mut k, &mut built.brep, "polyhedron")
            })
        }
    }
}

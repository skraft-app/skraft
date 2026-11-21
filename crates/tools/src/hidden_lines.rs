//! A drawing view's lines found on the display meshes (docs/ui/drawing.md#lines, decision
//! 0048): the parts' edges and the silhouettes of their curved faces, seen parallel along a
//! frame, split where the front faces of every part hide them.
//!
//! The front-facing triangles are the occluders, sorted from the nearest, in a grid over the
//! sheet; each segment is tested against the occluders of the cells it crosses, nearest first,
//! and stops as soon as it is hidden whole or the next occluder lies behind it.

use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::Mesh;

use skraft_model::FastMap;

/// A view's lines in its frame (sheet x, y in model units): polylines, visible and hidden.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewLines {
    pub visible: Vec<Vec<DVec2>>,
    pub hidden: Vec<Vec<DVec2>>,
}

impl ViewLines {
    /// Their extent (none when empty).
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        self.visible.iter().chain(&self.hidden).flatten().fold(None, |b, p| Some(b.map_or((*p, *p), |(a, z)| (a.min(*p), z.max(*p)))))
    }

    /// The segments' count.
    pub fn segments(&self) -> usize {
        self.visible.iter().chain(&self.hidden).map(|p| p.len().saturating_sub(1)).sum()
    }
}

/// A frame to look along: `forward` from the eye into the scene, `right` and `up` the sheet's.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub right: DVec3,
    pub up: DVec3,
    pub forward: DVec3,
}

impl Look {
    fn project(&self, p: DVec3) -> DVec3 {
        DVec3::new(p.dot(self.right), p.dot(self.up), p.dot(self.forward))
    }
}

/// A front-facing triangle on the sheet: its corners (counterclockwise), its depth plane
/// (depth = g.x x + g.y y + g.z), its nearest depth.
struct Occluder {
    p: [DVec2; 3],
    g: DVec3,
    near: f64,
}

/// A segment to draw: its ends on the sheet with their depths, the polyline it belongs to.
struct Seg {
    a: DVec3,
    b: DVec3,
    chain: u32,
}

/// The lines of `parts` (each mesh, part to world), seen along `look`.
pub fn hidden_lines(parts: &[(Arc<Mesh>, DAffine3)], look: &Look) -> ViewLines {
    let mut occ = Vec::new();
    let mut segs = Vec::new();
    let mut chain = 0u32;
    let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    let mut inner: FastMap<*const Mesh, Vec<(u32, u32, u32, u32)>> = FastMap::default();
    for (mesh, place) in parts {
        let m = &**mesh;
        let t = *place * DAffine3::from_translation(m.origin);
        let flip = t.matrix3.determinant() < 0.0;
        let pts: Vec<DVec3> = m.positions.iter().map(|p| look.project(t.transform_point3(DVec3::from(p.map(f64::from))))).collect();
        for p in &pts {
            lo = lo.min(p.truncate());
            hi = hi.max(p.truncate());
        }
        // The cosine of each triangle's normal to the eye: counterclockwise on the sheet (as the
        // eye sees it) is front.
        let facing: Vec<f64> = m
            .indices
            .chunks_exact(3)
            .map(|tri| {
                let [a, b, c] = [pts[tri[0] as usize], pts[tri[1] as usize], pts[tri[2] as usize]];
                let area = (b - a).truncate().perp_dot((c - a).truncate());
                let area3 = (b - a).cross(c - a).length().max(1e-300);
                if flip { -area / area3 } else { area / area3 }
            })
            .collect();
        for (tri, &cos) in m.indices.chunks_exact(3).zip(&facing) {
            // Edge-on triangles (f32 positions: noise ~1e-7) hide nothing.
            if cos <= 1e-4 {
                continue;
            }
            let [a, b, c] = [pts[tri[0] as usize], pts[tri[1] as usize], pts[tri[2] as usize]];
            let (e1, e2) = (b - a, c - a);
            let den = e1.x * e2.y - e2.x * e1.y;
            if den.abs() < 1e-18 {
                continue;
            }
            let gx = (e1.z * e2.y - e2.z * e1.y) / den;
            let gy = (e2.z * e1.x - e1.z * e2.x) / den;
            let g = DVec3::new(gx, gy, a.z - gx * a.x - gy * a.y);
            let p = if den > 0.0 { [a.truncate(), b.truncate(), c.truncate()] } else { [a.truncate(), c.truncate(), b.truncate()] };
            occ.push(Occluder { p, g, near: a.z.min(b.z).min(c.z) });
        }
        // The part's edges, one chain each.
        for poly in &m.edges {
            for w in poly.windows(2) {
                let p = |v: [f32; 3]| look.project(t.transform_point3(DVec3::from(v.map(f64::from))));
                segs.push(Seg { a: p(w[0]), b: p(w[1]), chain });
            }
            chain += 1;
        }
        // The silhouettes: edges inside one B-rep face whose two triangles face either way.
        let edges = inner.entry(Arc::as_ptr(mesh)).or_insert_with(|| inner_edges(m));
        for &(t0, t1, i, j) in edges.iter() {
            if (facing[t0 as usize] > 0.0) != (facing[t1 as usize] > 0.0) {
                segs.push(Seg { a: pts[i as usize], b: pts[j as usize], chain });
                chain += 1;
            }
        }
    }
    if segs.is_empty() {
        return ViewLines::default();
    }
    // Nearest first: a cell's list, filled in this order, is sorted by depth.
    occ.sort_by(|a, b| a.near.total_cmp(&b.near));
    let grid = Grid::build(&occ, lo, hi);
    let size = (hi - lo).max_element().max(1e-9);
    // Occluders grown a little (no cracks between neighbours); a segment on a face's plane is
    // not behind it.
    let (grow, epsz) = (size * 1e-7, size * 1e-4);
    // Chunks of whole chains, so their pieces join again.
    let mut chunks: Vec<&[Seg]> = Vec::new();
    let mut start = 0;
    for i in 1..=segs.len() {
        if i == segs.len() || (i - start >= 2048 && segs[i].chain != segs[i - 1].chain) {
            chunks.push(&segs[start..i]);
            start = i;
        }
    }
    let parts = map_chunks(&chunks, |chunk| {
        let mut out = ViewLines::default();
        let mut cells = Vec::new();
        let mut hid: Vec<(f64, f64)> = Vec::new();
        // The open polyline of each kind, with its chain.
        let mut open: [Option<(u32, Vec<DVec2>)>; 2] = [None, None];
        for s in *chunk {
            grid.cells_along(s.a.truncate(), s.b.truncate(), &mut cells);
            let far = s.a.z.max(s.b.z) + epsz;
            hid.clear();
            // Each cell's occluders nearest first, till one lies behind the segment (an occluder
            // in two cells tested twice: no matter).
            'cells: for &c in &cells {
                for &i in grid.cell(c) {
                    let o = &occ[i as usize];
                    if o.near > far {
                        break; // the rest of the cell lies behind it
                    }
                    if let Some(iv) = hidden_by(s, o, grow, epsz) {
                        hid.push(iv);
                        if covered(&mut hid) {
                            break 'cells;
                        }
                    }
                }
            }
            merge(&mut hid);
            // Pieces shorter than the depth's tolerance take their neighbours' look (no specks
            // where an edge hidden leaves a corner seen).
            let len = s.a.truncate().distance(s.b.truncate());
            absorb(&mut hid, if len > 0.0 { epsz / len } else { 1.0 });
            let at = |t: f64| s.a.truncate().lerp(s.b.truncate(), t);
            let mut t = 0.0;
            let mut piece = |from: f64, to: f64, visible: bool| {
                if to - from <= 1e-9 {
                    return;
                }
                let k = usize::from(!visible);
                let (a, b) = (at(from), at(to));
                match &mut open[k] {
                    Some((c, line)) if *c == s.chain && line.last().is_some_and(|l| l.distance_squared(a) < 1e-18) => line.push(b),
                    slot => {
                        if let Some((_, line)) = slot.take() {
                            if visible { out.visible.push(line) } else { out.hidden.push(line) }
                        }
                        *slot = Some((s.chain, vec![a, b]));
                    }
                }
            };
            for &(a, b) in &hid {
                piece(t, a, true);
                piece(a, b, false);
                t = b;
            }
            piece(t, 1.0, true);
        }
        for (k, slot) in open.into_iter().enumerate() {
            if let Some((_, line)) = slot {
                if k == 0 { out.visible.push(line) } else { out.hidden.push(line) }
            }
        }
        out
    });
    let mut lines = ViewLines::default();
    for p in parts {
        lines.visible.extend(p.visible);
        lines.hidden.extend(p.hidden);
    }
    lines
}

/// `f` over the chunks: on every core natively, one after the other on the web (no threads;
/// the kernel's pool lives in its worker there).
fn map_chunks<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    #[cfg(not(target_arch = "wasm32"))]
    return skraft_kernel::parallel_map(items, f);
    #[cfg(target_arch = "wasm32")]
    items.iter().map(f).collect()
}

/// Sorts and joins the intervals; covering `[0, 1]` whole?
fn covered(hid: &mut Vec<(f64, f64)>) -> bool {
    merge(hid);
    hid.len() == 1 && hid[0].0 <= 1e-9 && hid[0].1 >= 1.0 - 1e-9
}

/// The hidden intervals with what lies within `min` of an end or of each other joined, and
/// those shorter than `min` dropped.
fn absorb(hid: &mut Vec<(f64, f64)>, min: f64) {
    for iv in hid.iter_mut() {
        if iv.0 < min {
            iv.0 = 0.0;
        }
        if iv.1 > 1.0 - min {
            iv.1 = 1.0;
        }
    }
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(hid.len());
    for &(a, b) in hid.iter() {
        match out.last_mut() {
            Some(m) if a - m.1 < min => m.1 = m.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out.retain(|(a, b)| b - a >= min);
    *hid = out;
}

fn merge(hid: &mut Vec<(f64, f64)>) {
    if hid.len() < 2 {
        return;
    }
    hid.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(hid.len());
    for &(a, b) in hid.iter() {
        match out.last_mut() {
            Some(m) if a <= m.1 => m.1 = m.1.max(b),
            _ => out.push((a, b)),
        }
    }
    *hid = out;
}

/// The parameter interval of `s` inside `o` (grown by `grow`), and behind its plane by more
/// than `epsz`.
fn hidden_by(s: &Seg, o: &Occluder, grow: f64, epsz: f64) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let (a, b) = (s.a.truncate(), s.b.truncate());
    let mut clip = |f0: f64, f1: f64| -> bool {
        if f0 < 0.0 && f1 < 0.0 {
            return false;
        }
        if f0 < 0.0 || f1 < 0.0 {
            let t = f0 / (f0 - f1);
            if f0 < 0.0 { lo = lo.max(t) } else { hi = hi.min(t) }
        }
        lo < hi
    };
    for i in 0..3 {
        let (p, q) = (o.p[i], o.p[(i + 1) % 3]);
        let e = q - p;
        let len = e.length();
        let f = |v: DVec2| e.perp_dot(v - p) / len + grow;
        if !clip(f(a), f(b)) {
            return None;
        }
    }
    let h = |v: DVec3| v.z - (o.g.x * v.x + o.g.y * v.y + o.g.z) - epsz;
    if !clip(h(s.a), h(s.b)) {
        return None;
    }
    Some((lo, hi))
}

/// Each edge inside one B-rep face: its two triangles and its two vertices.
fn inner_edges(m: &Mesh) -> Vec<(u32, u32, u32, u32)> {
    let key = |i: u32| {
        let p = m.positions[i as usize];
        [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]
    };
    let mut ids: FastMap<[u32; 3], u32> = FastMap::default();
    let vid: Vec<u32> = (0..m.positions.len() as u32)
        .map(|i| {
            let n = ids.len() as u32;
            *ids.entry(key(i)).or_insert(n)
        })
        .collect();
    let mut seen: FastMap<(u32, u32), (u32, u32, u32)> = FastMap::default();
    let mut inner = Vec::new();
    for (t, tri) in m.indices.chunks_exact(3).enumerate() {
        for k in 0..3 {
            let (i, j) = (tri[k], tri[(k + 1) % 3]);
            let (a, b) = (vid[i as usize], vid[j as usize]);
            match seen.remove(&(a.min(b), a.max(b))) {
                Some((t0, i0, j0)) => {
                    if m.tri_face.get(t0 as usize) == m.tri_face.get(t) {
                        inner.push((t0, t as u32, i0, j0));
                    }
                }
                None => {
                    seen.insert((a.min(b), a.max(b)), (t as u32, i, j));
                }
            }
        }
    }
    inner
}

/// The occluders by the cells of a grid over the sheet, each cell's in depth order.
struct Grid {
    lo: DVec2,
    cell: f64,
    n: [usize; 2],
    start: Vec<u32>,
    items: Vec<u32>,
}

impl Grid {
    fn build(occ: &[Occluder], lo: DVec2, hi: DVec2) -> Grid {
        let size = (hi - lo).max(DVec2::splat(1e-9));
        let target = ((occ.len() as f64).sqrt() * 1.5).clamp(16.0, 4096.0);
        let cell = size.max_element() / target;
        let n = [((size.x / cell) as usize + 1).min(8192), ((size.y / cell) as usize + 1).min(8192)];
        let range = |o: &Occluder| {
            let (a, b) = (o.p[0].min(o.p[1]).min(o.p[2]), o.p[0].max(o.p[1]).max(o.p[2]));
            let c = |v: DVec2| {
                let q = (v - lo) / cell;
                [(q.x.max(0.0) as usize).min(n[0] - 1), (q.y.max(0.0) as usize).min(n[1] - 1)]
            };
            (c(a), c(b))
        };
        let mut count = vec![0u32; n[0] * n[1] + 1];
        for o in occ {
            let (a, b) = range(o);
            for y in a[1]..=b[1] {
                for x in a[0]..=b[0] {
                    count[y * n[0] + x + 1] += 1;
                }
            }
        }
        for i in 1..count.len() {
            count[i] += count[i - 1];
        }
        let mut fill = count.clone();
        let mut items = vec![0u32; *count.last().unwrap_or(&0) as usize];
        for (i, o) in occ.iter().enumerate() {
            let (a, b) = range(o);
            for y in a[1]..=b[1] {
                for x in a[0]..=b[0] {
                    let c = y * n[0] + x;
                    items[fill[c] as usize] = i as u32;
                    fill[c] += 1;
                }
            }
        }
        Grid { lo, cell, n, start: count, items }
    }

    /// The occluders of cell `c`, nearest first.
    fn cell(&self, c: usize) -> &[u32] {
        &self.items[self.start[c] as usize..self.start[c + 1] as usize]
    }

    /// The cells the segment `a`–`b` crosses (and those it runs along).
    fn cells_along(&self, a: DVec2, b: DVec2, out: &mut Vec<usize>) {
        out.clear();
        let qa = (a - self.lo) / self.cell;
        let qb = (b - self.lo) / self.cell;
        let steps = ((qb - qa).abs().max_element().ceil() as usize * 2).max(1);
        let mut last = usize::MAX;
        for s in 0..=steps {
            let q = qa.lerp(qb, s as f64 / steps as f64);
            // The cells around the sample: a segment on a cell border sees both sides.
            for (dx, dy) in [(0.0, 0.0), (0.5, 0.0), (-0.5, 0.0), (0.0, 0.5), (0.0, -0.5)] {
                let x = ((q.x + dx).max(0.0) as usize).min(self.n[0] - 1);
                let y = ((q.y + dy).max(0.0) as usize).min(self.n[1] - 1);
                let c = y * self.n[0] + x;
                if c == last {
                    continue;
                }
                last = c;
                out.push(c);
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box `lo`–`hi` as a mesh: 12 triangles (counterclockwise from outside), its 12 edges.
    fn cube(lo: DVec3, hi: DVec3) -> Arc<Mesh> {
        let c = |i: usize| {
            [
                if i & 1 == 0 { lo.x } else { hi.x } as f32,
                if i & 2 == 0 { lo.y } else { hi.y } as f32,
                if i & 4 == 0 { lo.z } else { hi.z } as f32,
            ]
        };
        let positions: Vec<[f32; 3]> = (0..8).map(c).collect();
        // Faces: -z, +z, -y, +y, -x, +x, each as two triangles, outward.
        let quads: [[u32; 4]; 6] = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let mut indices = Vec::new();
        let mut tri_face = Vec::new();
        for (f, q) in quads.iter().enumerate() {
            indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
            tri_face.extend([f as u32, f as u32]);
        }
        let edges: Vec<Vec<[f32; 3]>> = (0..8usize)
            .flat_map(|i| [1usize, 2, 4].into_iter().filter(move |b| i & b == 0).map(move |b| (i, i | b)))
            .map(|(a, b)| vec![c(a), c(b)])
            .collect();
        let edge_ids = (0..edges.len() as u32).collect();
        Arc::new(Mesh { origin: DVec3::ZERO, normals: vec![[0.0, 0.0, 1.0]; 8], positions, indices, tri_face, edges, edge_ids })
    }

    fn front() -> Look {
        Look { right: DVec3::X, up: DVec3::Z, forward: DVec3::Y }
    }

    fn length(lines: &[Vec<DVec2>]) -> f64 {
        lines.iter().flat_map(|l| l.windows(2)).map(|w| w[0].distance(w[1])).sum()
    }

    #[test]
    fn a_box_from_the_front_shows_its_outline_and_hides_its_back() {
        let lines = hidden_lines(&[(cube(DVec3::ZERO, DVec3::new(100.0, 50.0, 20.0)), DAffine3::IDENTITY)], &front());
        // The front face's outline seen: 2 × (100 + 20); the back face's edges and the four
        // edges along y (points, seen end on) hidden or of no length.
        assert!((length(&lines.visible) - 240.0).abs() < 1e-6, "{}", length(&lines.visible));
        assert!((length(&lines.hidden) - 240.0).abs() < 1e-6, "the back face's, under the front's: {}", length(&lines.hidden));
    }

    #[test]
    fn a_box_hides_the_one_behind_it_but_not_what_sticks_out() {
        let near = (cube(DVec3::ZERO, DVec3::new(100.0, 10.0, 100.0)), DAffine3::IDENTITY);
        // Behind it, taller: its top 50 above the near one's.
        let far = (cube(DVec3::new(25.0, 50.0, 0.0), DVec3::new(75.0, 60.0, 150.0)), DAffine3::IDENTITY);
        let lines = hidden_lines(&[near, far], &front());
        let seen = length(&lines.visible);
        // The near box's front outline (400), the far one's top 50 above it: its top edge (50)
        // and its sides' upper halves (2 × 50).
        assert!((seen - 550.0).abs() < 1e-3, "{seen}");
        assert!(lines.bounds().is_some_and(|(lo, hi)| (hi - lo - DVec2::new(100.0, 150.0)).length() < 1e-9));
    }

    #[test]
    fn an_iso_view_joins_pieces_into_polylines() {
        let iso = {
            let f = DVec3::new(-1.0, 1.0, -1.0).normalize();
            let r = f.cross(DVec3::Z).normalize();
            Look { right: r, up: r.cross(f), forward: f }
        };
        let lines = hidden_lines(&[(cube(DVec3::ZERO, DVec3::splat(10.0)), DAffine3::IDENTITY)], &iso);
        // A cube seen iso: 9 edges seen, 3 hidden (meeting at the far corner).
        assert_eq!(lines.visible.len(), 9, "{:?}", lines.visible);
        assert_eq!(lines.hidden.len(), 3);
    }
}

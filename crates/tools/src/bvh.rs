//! A bounding volume hierarchy over world boxes (ARCHITECTURE.md §7 bis): what lets the scene
//! answer a ray or a region without testing every placement.
//!
//! Built whole from its items' boxes (a linear BVH): the boxes ordered along a Morton curve
//! through their centres, one scale on the three axes so that the curve follows space, then
//! that order cut where the highest bit that differs among its codes turns on (each node is
//! a cell of an octree, its items near one another) down to a few items per leaf, each
//! node's bounds gathered from its children's; nodes are laid out depth first in one array
//! (an inner node's left child follows it). Leaves hold their items' boxes, tested before
//! an item is handed out. Items are only indices: what they stand for, and the exact tests,
//! stay with the caller.

use glam::DVec3;

/// Items per leaf, at most.
const LEAF: usize = 4;

/// The largest coordinate along one axis of the Morton curve (21 bits).
const MORTON_MAX: f64 = ((1u64 << 21) - 1) as f64;

pub(crate) type Aabb = (DVec3, DVec3);

#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Aabb,
    /// A leaf: its first item in `items`; an inner node: its right child.
    index: u32,
    /// A leaf: its number of items; 0: an inner node.
    count: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Bvh {
    nodes: Vec<Node>,
    /// The items (index, world box), leaf by leaf.
    items: Vec<(u32, Aabb)>,
}

impl Bvh {
    /// The hierarchy of `items` (index, world box).
    pub(crate) fn build(items: impl IntoIterator<Item = (u32, Aabb)>) -> Bvh {
        let items: Vec<(u32, Aabb)> = items.into_iter().collect();
        let mut bvh = Bvh { nodes: Vec::with_capacity(items.len()), items: Vec::with_capacity(items.len()) };
        if items.is_empty() {
            return bvh;
        }
        // Ordered along a Morton curve through the boxes' centres, the same scale on every
        // axis: a cell of the curve is a cube, however long and flat the scene.
        let centre = |b: &Aabb| (b.0 + b.1) * 0.5;
        let (lo, hi) = items.iter().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), (_, b)| (lo.min(centre(b)), hi.max(centre(b))));
        let scale = MORTON_MAX / (hi - lo).max_element().max(1e-12);
        let mut order: Vec<(u64, u32)> = items
            .iter()
            .enumerate()
            .map(|(k, (_, b))| {
                let q = ((centre(b) - lo) * scale).clamp(DVec3::ZERO, DVec3::splat(MORTON_MAX));
                (morton(q.x as u64) | morton(q.y as u64) << 1 | morton(q.z as u64) << 2, k as u32)
            })
            .collect();
        order.sort_unstable();
        let codes: Vec<u64> = order.iter().map(|(c, _)| *c).collect();
        let sorted: Vec<(u32, Aabb)> = order.into_iter().map(|(_, k)| items[k as usize]).collect();
        bvh.split(&codes, &sorted);
        bvh
    }

    /// The node of `items` (in Morton order, with their `codes`) and those below it; returns
    /// its bounds.
    fn split(&mut self, codes: &[u64], items: &[(u32, Aabb)]) -> Aabb {
        let at = self.nodes.len();
        if items.len() <= LEAF {
            let bounds = enclose(items);
            self.nodes.push(Node { bounds, index: self.items.len() as u32, count: items.len() as u32 });
            self.items.extend_from_slice(items);
            return bounds;
        }
        // Cut where the highest bit that differs among the codes turns on: both sides are
        // cells of the octree. The codes all equal (centres in one cell): halved.
        let (first, last) = (codes[0], codes[codes.len() - 1]);
        let mid = if first == last {
            items.len() / 2
        } else {
            let bit = 63 - (first ^ last).leading_zeros();
            codes.partition_point(|c| c >> bit & 1 == 0)
        };
        self.nodes.push(Node { bounds: items[0].1, index: 0, count: 0 });
        let a = self.split(&codes[..mid], &items[..mid]);
        self.nodes[at].index = self.nodes.len() as u32;
        let b = self.split(&codes[mid..], &items[mid..]);
        let bounds = union(a, b);
        self.nodes[at].bounds = bounds;
        bounds
    }

    /// The same hierarchy over the items' boxes as they now are (`bounds` of each item that
    /// was given a box): each node's gathered again from its children's, the deepest first.
    /// The tree stays as it was built, so it serves well only while the boxes change little.
    pub(crate) fn refit(&mut self, bounds: impl Fn(u32) -> Aabb) {
        // Children follow their parent in the array: backwards, they come first.
        for n in (0..self.nodes.len()).rev() {
            let node = self.nodes[n];
            let b = if node.count > 0 {
                let items = &mut self.items[node.index as usize..(node.index + node.count) as usize];
                for (i, b) in items.iter_mut() {
                    *b = bounds(*i);
                }
                enclose(items)
            } else {
                union(self.nodes[n + 1].bounds, self.nodes[node.index as usize].bounds)
            };
            self.nodes[n].bounds = b;
        }
    }

    /// A leaf's items.
    fn leaf(&self, node: &Node) -> &[(u32, Aabb)] {
        &self.items[node.index as usize..(node.index + node.count) as usize]
    }

    /// Calls `item` with each item whose box `accept`s, the nodes holding it accepted too:
    /// `accept` must hold of a box whenever it holds of a box inside it.
    pub(crate) fn visit(&self, mut accept: impl FnMut(&Aabb) -> bool, mut item: impl FnMut(u32)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0u32];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n as usize];
            if !accept(&node.bounds) {
                continue;
            }
            if node.count > 0 {
                // A leaf of one: its box is the node's, accepted already.
                if node.count == 1 {
                    item(self.items[node.index as usize].0);
                    continue;
                }
                for (i, b) in self.leaf(node) {
                    if accept(b) {
                        item(*i);
                    }
                }
            } else {
                stack.push(node.index);
                stack.push(n + 1);
            }
        }
    }

    /// Calls `item` with the items whose box the ray `o + t·d` (t ≥ `t_min`) meets, nearest
    /// boxes first; each call returns how far along the ray hits still matter (`f64::INFINITY`
    /// at first): boxes beyond are skipped.
    pub(crate) fn along_ray(&self, o: DVec3, d: DVec3, t_min: f64, mut item: impl FnMut(u32) -> f64) {
        let Some(t) = self.nodes.first().and_then(|n| ray_box_entry(o, d, &n.bounds, t_min)) else { return };
        let mut far = f64::INFINITY;
        let mut stack = vec![(0u32, t)];
        while let Some((n, entry)) = stack.pop() {
            if entry > far {
                continue;
            }
            let node = &self.nodes[n as usize];
            if node.count > 0 {
                // The leaf's boxes the ray meets before `far`, nearest first.
                let mut near = [(0.0, 0u32); LEAF];
                let mut k = 0;
                for (i, b) in self.leaf(node) {
                    if let Some(t) = ray_box_entry(o, d, b, t_min)
                        && t <= far
                    {
                        near[k] = (t, *i);
                        k += 1;
                    }
                }
                let near = &mut near[..k];
                near.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
                for &(t, i) in near.iter() {
                    if t > far {
                        break;
                    }
                    far = far.min(item(i));
                }
                continue;
            }
            let (a, b) = (n + 1, node.index);
            let ta = ray_box_entry(o, d, &self.nodes[a as usize].bounds, t_min);
            let tb = ray_box_entry(o, d, &self.nodes[b as usize].bounds, t_min);
            // The nearer one popped first.
            match (ta, tb) {
                (Some(ta), Some(tb)) if ta <= tb => stack.extend([(b, tb), (a, ta)]),
                (Some(ta), Some(tb)) => stack.extend([(a, ta), (b, tb)]),
                (Some(ta), None) => stack.push((a, ta)),
                (None, Some(tb)) => stack.push((b, tb)),
                (None, None) => {}
            }
        }
    }
}

/// The box around two.
fn union(a: Aabb, b: Aabb) -> Aabb {
    (a.0.min(b.0), a.1.max(b.1))
}

/// The box around some items' (at least one).
fn enclose(items: &[(u32, Aabb)]) -> Aabb {
    items[1..].iter().fold(items[0].1, |a, (_, b)| union(a, *b))
}

/// `x`'s 21 low bits spread to every third bit (a Morton code's share of one axis).
fn morton(x: u64) -> u64 {
    let mut x = x & 0x1f_ffff;
    x = (x | x << 32) & 0x1f00000000ffff;
    x = (x | x << 16) & 0x1f0000ff0000ff;
    x = (x | x << 8) & 0x100f00f00f00f00f;
    x = (x | x << 4) & 0x10c30c30c30c30c3;
    x = (x | x << 2) & 0x1249249249249249;
    x
}

/// Where the ray `o + t·d` (t ≥ `t_min`) enters the box (slab test, slightly padded), if it
/// meets it: `t_min` when it starts inside (−∞ for a parallel projection's ray).
pub(crate) fn ray_box_entry(o: DVec3, d: DVec3, (lo, hi): &Aabb, t_min: f64) -> Option<f64> {
    let pad = ((*hi - *lo).length() * 1e-6).max(1e-9);
    let (lo, hi) = (*lo - pad, *hi + pad);
    let (mut t0, mut t1) = (t_min, f64::INFINITY);
    for k in 0..3 {
        if d[k].abs() < 1e-300 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boxes scattered over a few hundred millimetres, some overlapping, some stacked.
    fn boxes(n: u32) -> Vec<Aabb> {
        let mut seed = 12345u64;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) * 500.0
        };
        (0..n)
            .map(|i| {
                let lo = if i % 7 == 0 { DVec3::ZERO } else { DVec3::new(next(), next(), next()) };
                (lo, lo + DVec3::new(next() * 0.1 + 1.0, next() * 0.1 + 1.0, next() * 0.1 + 1.0))
            })
            .collect()
    }

    #[test]
    fn visiting_finds_what_a_scan_finds() {
        let bs = boxes(1000);
        let bvh = Bvh::build(bs.iter().enumerate().map(|(i, b)| (i as u32, *b)));
        let region = (DVec3::splat(100.0), DVec3::splat(250.0));
        let meets = |(lo, hi): &Aabb| lo.cmple(region.1).all() && hi.cmpge(region.0).all();
        // Only the items whose own box is accepted.
        let mut found = Vec::new();
        bvh.visit(meets, |i| found.push(i));
        found.sort();
        let scan: Vec<u32> = (0..bs.len() as u32).filter(|i| meets(&bs[*i as usize])).collect();
        assert!(!scan.is_empty());
        assert_eq!(found, scan);
    }

    #[test]
    fn a_ray_finds_the_nearest_box_as_a_scan_does() {
        let bs = boxes(1000);
        let bvh = Bvh::build(bs.iter().enumerate().map(|(i, b)| (i as u32, *b)));
        for (o, d) in [
            (DVec3::new(-100.0, 200.0, 250.0), DVec3::X),
            (DVec3::new(250.0, 250.0, 900.0), -DVec3::Z),
            (DVec3::splat(-50.0), DVec3::ONE.normalize()),
            (DVec3::new(0.5, 0.5, -10.0), DVec3::Z),
        ] {
            let mut best: Option<(f64, u32)> = None;
            let mut tested = 0;
            bvh.along_ray(o, d, 0.0, |i| {
                tested += 1;
                // Only boxes the ray meets, before the nearest hit so far.
                let t = ray_box_entry(o, d, &bs[i as usize], 0.0).expect("a box the ray meets");
                assert!(best.is_none_or(|(b, _)| t <= b));
                if best.is_none_or(|(b, _)| t < b) {
                    best = Some((t, i));
                }
                best.map_or(f64::INFINITY, |(b, _)| b)
            });
            let scan = (0..bs.len() as u32)
                .filter_map(|i| ray_box_entry(o, d, &bs[i as usize], 0.0).map(|t| (t, i)))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            assert_eq!(best.map(|b| b.0), scan.map(|b| b.0), "{o} {d}");
            assert!(tested < 10, "{tested} tested");
        }
    }

    #[test]
    fn a_refit_hierarchy_finds_what_a_scan_finds() {
        let mut bs = boxes(500);
        let mut bvh = Bvh::build(bs.iter().enumerate().map(|(i, b)| (i as u32, *b)));
        // Every box moved and grown a little, some a lot (the items' own boxes too).
        for (i, b) in bs.iter_mut().enumerate() {
            let shift = DVec3::new((i % 7) as f64, (i % 5) as f64 * 2.0, 0.0) * if i % 50 == 0 { 40.0 } else { 1.0 };
            *b = (b.0 + shift, b.1 + shift + DVec3::splat(0.5));
        }
        bvh.refit(|i| bs[i as usize]);
        let region = (DVec3::splat(150.0), DVec3::splat(300.0));
        let meets = |(lo, hi): &Aabb| lo.cmple(region.1).all() && hi.cmpge(region.0).all();
        let mut found = Vec::new();
        bvh.visit(meets, |i| found.push(i));
        found.sort();
        let scan: Vec<u32> = (0..bs.len() as u32).filter(|i| meets(&bs[*i as usize])).collect();
        assert!(!scan.is_empty());
        assert_eq!(found, scan);
    }

    /// Build and queries over a few scenes where the index is tested: per ray and per region,
    /// how many items reach the caller and how long it takes (ignored; run with
    /// `cargo test -p solidsketch-tools --release bench_bvh -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn bench_bvh() {
        use std::time::Instant;
        let mut seed = 777u64;
        let mut rnd = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let cube = |lo: DVec3, s: f64| (lo, lo + DVec3::splat(s));
        // 40 000 cubes of 20 on a 200 × 200 grid, every 100.
        let grid: Vec<Aabb> = (0..40_000).map(|i| cube(DVec3::new((i % 200) as f64, (i / 200) as f64, 0.0) * 100.0, 20.0)).collect();
        // 10 000 cubes under 20 plates as wide as the grid (bench_mixed_sizes).
        let mut mixed: Vec<Aabb> = (0..10_000).map(|i| cube(DVec3::new((i % 100) as f64, (i / 100) as f64, 0.0) * 100.0, 20.0)).collect();
        mixed.extend((0..20).map(|k| {
            let lo = DVec3::new(-50.0, -50.0, 40.0 + k as f64 * 30.0);
            (lo, lo + DVec3::new(10_000.0, 10_000.0, 5.0))
        }));
        // 20 000 boards (18 thick, 200 to 2 000 long) in a long, low and shallow room.
        let room = DVec3::new(20_000.0, 1_500.0, 2_400.0);
        let boards: Vec<Aabb> = (0..20_000)
            .map(|_| {
                let mut size = DVec3::new(200.0 + 1_800.0 * rnd(), 100.0 + 500.0 * rnd(), 18.0);
                size = match (rnd() * 3.0) as u32 {
                    0 => size,
                    1 => DVec3::new(size.z, size.y, size.x),
                    _ => DVec3::new(size.x, size.z, size.y),
                };
                let lo = DVec3::new(rnd(), rnd(), rnd()) * (room - size).max(DVec3::ZERO);
                (lo, lo + size)
            })
            .collect();
        for (name, bs) in [("grid 40k", &grid), ("mixed 10k + plates", &mixed), ("boards 20k", &boards)] {
            let t = Instant::now();
            let bvh = Bvh::build(bs.iter().enumerate().map(|(i, b)| (i as u32, *b)));
            let build = t.elapsed().as_secs_f64() * 1e3;
            let (lo, hi) = bs.iter().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), b| (lo.min(b.0), hi.max(b.1)));
            let span = hi - lo;
            // Rays from above and aside towards random points of the scene, the nearest box kept.
            let rays: Vec<(DVec3, DVec3)> = (0..20_000)
                .map(|k| {
                    let target = lo + DVec3::new(rnd(), rnd(), rnd()) * span;
                    let eye = if k % 2 == 0 {
                        lo + DVec3::new(rnd() * span.x, -span.y - 1_000.0, span.z * 2.0 + 1_000.0)
                    } else {
                        hi + DVec3::new(1_000.0, 1_000.0, 1_000.0) + span * 0.5
                    };
                    (eye, (target - eye).normalize())
                })
                .collect();
            let (mut called, mut hits) = (0usize, 0usize);
            let t = Instant::now();
            for &(o, d) in &rays {
                let mut best = f64::INFINITY;
                bvh.along_ray(o, d, 0.0, |i| {
                    called += 1;
                    if let Some(t) = ray_box_entry(o, d, &bs[i as usize], 0.0) {
                        best = best.min(t);
                    }
                    best
                });
                hits += best.is_finite() as usize;
            }
            let ray_us = t.elapsed().as_secs_f64() * 1e6 / rays.len() as f64;
            // Regions of a twentieth of the scene across.
            let (mut region_called, mut found) = (0usize, 0usize);
            let t = Instant::now();
            for _ in 0..2_000 {
                let a = lo + DVec3::new(rnd(), rnd(), rnd()) * span;
                let region = (a, a + span * 0.05);
                let meets = |(lo, hi): &Aabb| lo.cmple(region.1).all() && hi.cmpge(region.0).all();
                bvh.visit(meets, |i| {
                    region_called += 1;
                    found += meets(&bs[i as usize]) as usize;
                });
            }
            let region_us = t.elapsed().as_secs_f64() * 1e6 / 2_000.0;
            eprintln!(
                "{name:<20} build {build:>6.2} ms | ray {ray_us:>6.2} µs, {:>6.1} items called ({:.0} % hit) | region {region_us:>6.2} µs, {:>6.1} items called for {:>6.1} found",
                called as f64 / rays.len() as f64,
                hits as f64 * 100.0 / rays.len() as f64,
                region_called as f64 / 2_000.0,
                found as f64 / 2_000.0,
            );
        }
    }

    /// Boxes all centred alike (one Morton code), or along one thin axis: halved, and found.
    #[test]
    fn same_centres_and_a_flat_scene() {
        let nested: Vec<Aabb> = (1..=50).map(|k| (DVec3::splat(-(k as f64)), DVec3::splat(k as f64))).collect();
        let row: Vec<Aabb> =
            (0..500).map(|k| (DVec3::new(k as f64 * 10.0, 0.0, 0.0), DVec3::new(k as f64 * 10.0 + 5.0, 1.0, 1.0))).collect();
        for bs in [nested, row] {
            let bvh = Bvh::build(bs.iter().enumerate().map(|(i, b)| (i as u32, *b)));
            assert!(bvh.nodes.iter().all(|n| n.count as usize <= LEAF));
            let region = (DVec3::new(20.0, -2.0, -2.0), DVec3::new(42.0, 2.0, 2.0));
            let meets = |(lo, hi): &Aabb| lo.cmple(region.1).all() && hi.cmpge(region.0).all();
            let mut found = Vec::new();
            bvh.visit(meets, |i| found.push(i));
            found.sort();
            let scan: Vec<u32> = (0..bs.len() as u32).filter(|i| meets(&bs[*i as usize])).collect();
            assert!(!scan.is_empty());
            assert_eq!(found, scan);
        }
    }

    #[test]
    fn empty_and_single() {
        let empty = Bvh::build([]);
        empty.visit(|_| true, |_| panic!());
        empty.along_ray(DVec3::ZERO, DVec3::X, 0.0, |_| panic!());
        let one = Bvh::build([(7, (DVec3::ONE, DVec3::splat(2.0)))]);
        let mut seen = Vec::new();
        one.along_ray(DVec3::ZERO, DVec3::ONE.normalize(), 0.0, |i| {
            seen.push(i);
            f64::INFINITY
        });
        assert_eq!(seen, vec![7]);
    }
}

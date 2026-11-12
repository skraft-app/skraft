//! A bounding volume hierarchy over world boxes (ARCHITECTURE.md §7 bis): what lets the scene
//! answer a ray or a region without testing every placement.
//!
//! Built whole from its items' boxes (a linear BVH): the boxes ordered along a Morton curve
//! through their centres, then that order halved down to a few items per leaf, each node's
//! bounds gathered from its children's; nodes are laid out depth first in one array (an
//! inner node's left child follows it). Items are only indices: what they stand for, and
//! the exact tests, stay with the caller.

use glam::DVec3;

/// Items per leaf, at most.
const LEAF: usize = 4;

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
    items: Vec<u32>,
}

impl Bvh {
    /// The hierarchy of `items` (index, world box).
    pub(crate) fn build(items: impl IntoIterator<Item = (u32, Aabb)>) -> Bvh {
        let items: Vec<(u32, Aabb)> = items.into_iter().collect();
        let mut bvh = Bvh { nodes: Vec::with_capacity(2 * items.len() / LEAF + 1), items: Vec::with_capacity(items.len()) };
        if items.is_empty() {
            return bvh;
        }
        // Ordered along a Morton curve through the boxes' centres: halving that order halves
        // space, without sorting again at each level.
        let centre = |b: &Aabb| (b.0 + b.1) * 0.5;
        let (lo, hi) = items.iter().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), (_, b)| (lo.min(centre(b)), hi.max(centre(b))));
        let scale = (hi - lo).max(DVec3::splat(1e-12)).recip() * ((1u64 << 21) - 1) as f64;
        let mut order: Vec<(u64, u32)> = items
            .iter()
            .enumerate()
            .map(|(k, (_, b))| {
                let q = ((centre(b) - lo) * scale).max(DVec3::ZERO);
                (morton(q.x as u64) | morton(q.y as u64) << 1 | morton(q.z as u64) << 2, k as u32)
            })
            .collect();
        order.sort_unstable();
        let sorted: Vec<(u32, Aabb)> = order.into_iter().map(|(_, k)| items[k as usize]).collect();
        bvh.split(&sorted);
        bvh
    }

    /// The node of `items` (in Morton order) and those below it; returns its bounds.
    fn split(&mut self, items: &[(u32, Aabb)]) -> Aabb {
        let at = self.nodes.len();
        if items.len() <= LEAF {
            let bounds = items.iter().skip(1).fold(items[0].1, |(lo, hi), (_, b)| (lo.min(b.0), hi.max(b.1)));
            self.nodes.push(Node { bounds, index: self.items.len() as u32, count: items.len() as u32 });
            self.items.extend(items.iter().map(|(i, _)| *i));
            return bounds;
        }
        self.nodes.push(Node { bounds: items[0].1, index: 0, count: 0 });
        let (left, right) = items.split_at(items.len() / 2);
        let a = self.split(left);
        self.nodes[at].index = self.nodes.len() as u32;
        let b = self.split(right);
        let bounds = (a.0.min(b.0), a.1.max(b.1));
        self.nodes[at].bounds = bounds;
        bounds
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
                for &i in &self.items[node.index as usize..(node.index + node.count) as usize] {
                    item(i);
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
                for &i in &self.items[node.index as usize..(node.index + node.count) as usize] {
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
        let mut found = Vec::new();
        bvh.visit(meets, |i| {
            if meets(&bs[i as usize]) {
                found.push(i)
            }
        });
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
                if let Some(t) = ray_box_entry(o, d, &bs[i as usize], 0.0)
                    && best.is_none_or(|(b, _)| t < b)
                {
                    best = Some((t, i));
                }
                best.map_or(f64::INFINITY, |(b, _)| b)
            });
            let scan = (0..bs.len() as u32)
                .filter_map(|i| ray_box_entry(o, d, &bs[i as usize], 0.0).map(|t| (t, i)))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            assert_eq!(best.map(|b| b.0), scan.map(|b| b.0), "{o} {d}");
            assert!(tested < bs.len() / 4, "{tested} tested");
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

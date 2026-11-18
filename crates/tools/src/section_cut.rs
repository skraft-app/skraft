//! Section cut lines (docs/ARCHITECTURE.md §7 quater): the outline of what the section plane
//! cuts, from the display meshes — so they meet the caps the renderer draws from the same
//! triangles.

use std::cell::RefCell;
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use skraft_kernel::Mesh;
use skraft_model::{FastMap, Id, Section};

use crate::scene::DisplayScene;

/// One definition's cut by a plane, in its axes: by solid, its segments.
pub type DefinitionCut = Arc<Vec<(Id, Vec<[DVec3; 2]>)>>;

/// What a cut is kept by: the definition, its mesh's revision, the plane in its axes (the
/// bits of its normal and offset).
type Key = (Id, u64, [u64; 4]);

/// The cuts of the definitions drawn, kept from one frame to the next while used: a still
/// plane cuts nothing again, a moving one only what it crosses.
#[derive(Default)]
pub struct SectionCuts {
    /// This frame's, and the last frame's not used yet.
    kept: RefCell<(FastMap<Key, DefinitionCut>, FastMap<Key, DefinitionCut>)>,
}

impl SectionCuts {
    /// Starts a frame: the cuts the last one did not use are dropped at the end of this one.
    pub fn next_frame(&self) {
        let mut kept = self.kept.borrow_mut();
        let (now, last) = &mut *kept;
        *last = std::mem::take(now);
    }

    /// The cut of `def`'s mesh placed by `transform` (definition to world) by `section`, in
    /// the definition's axes; none when the plane misses it or it has no mesh.
    pub fn cut(&self, scene: &DisplayScene, def: Id, transform: &DAffine3, section: &Section) -> Option<DefinitionCut> {
        let dm = scene.definition_mesh(def)?;
        let (normal, offset) = local_plane(transform, section);
        let key = (def, dm.revision, [normal.x.to_bits(), normal.y.to_bits(), normal.z.to_bits(), offset.to_bits()]);
        let mut kept = self.kept.borrow_mut();
        let (now, last) = &mut *kept;
        if let Some(c) = now.get(&key) {
            return Some(c.clone());
        }
        let cut = match last.remove(&key) {
            Some(c) => c,
            None => {
                let mut parts = Vec::new();
                for p in &dm.parts {
                    if p.bounds.is_some_and(|(lo, hi)| !straddles(lo, hi, normal, offset)) {
                        continue;
                    }
                    let mut segments = Vec::new();
                    slice(&p.mesh, normal, offset, &mut segments);
                    if !segments.is_empty() {
                        parts.push((p.solid, segments));
                    }
                }
                Arc::new(parts)
            }
        };
        now.insert(key, cut.clone());
        Some(cut)
    }
}

/// The plane `section` in the axes `transform` maps to the world: `normal · x + offset > 0`
/// for a point `x` cut away (`normal` not unit under a scale).
fn local_plane(transform: &DAffine3, section: &Section) -> (DVec3, f64) {
    let n = section.normal;
    (transform.matrix3.transpose() * n, n.dot(transform.translation - section.origin))
}

/// Can the plane cut the box `lo..hi`: has it points on both sides?
fn straddles(lo: DVec3, hi: DVec3, normal: DVec3, offset: f64) -> bool {
    let centre = normal.dot((lo + hi) * 0.5) + offset;
    let reach = normal.abs().dot((hi - lo) * 0.5);
    // A little slack: a mesh may stray from its solid's exact bounds by rounding.
    let slack = reach * 1e-9 + 1e-6;
    centre - reach <= slack && centre + reach > -slack
}

/// Adds to `out` the segments where the plane `normal · x + offset = 0` cuts `mesh`'s
/// triangles, in the mesh's definition axes. A vertex on the plane counts with the side kept
/// (as the renderer's cut, `> 0` cut away), so an edge on the plane gives one segment, not one
/// per triangle along it; a triangle touching the plane at a point gives none.
pub fn slice(mesh: &Mesh, normal: DVec3, offset: f64, out: &mut Vec<[DVec3; 2]>) {
    let shift = normal.dot(mesh.origin) + offset;
    let at = |i: u32| DVec3::from(mesh.positions[i as usize].map(f64::from));
    let d: Vec<f64> = mesh.positions.iter().map(|p| normal.dot(DVec3::from(p.map(f64::from))) + shift).collect();
    for tri in mesh.indices.chunks_exact(3) {
        let tri = [tri[0], tri[1], tri[2]];
        let cut = tri.map(|i| d[i as usize] > 0.0);
        if cut[0] == cut[1] && cut[1] == cut[2] {
            continue;
        }
        // The two sides crossing the plane.
        let mut ends = [DVec3::ZERO; 2];
        let mut n = 0;
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            if cut[k] != cut[(k + 1) % 3] {
                let (da, db) = (d[a as usize], d[b as usize]);
                ends[n] = at(a).lerp(at(b), da / (da - db));
                n += 1;
            }
        }
        if ends[0] != ends[1] {
            out.push(ends.map(|p| p + mesh.origin));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DQuat;

    /// The cube `-1..1` around `origin`, its triangles turned outwards.
    fn cube(origin: DVec3) -> Mesh {
        let positions: Vec<[f32; 3]> = (0..8)
            .map(|i| [if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 }])
            .collect();
        // Each side as two triangles: x−, x+, y−, y+, z−, z+.
        let quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
        let indices = quads.iter().flat_map(|[a, b, c, d]| [*a, *b, *c, *a, *c, *d]).collect();
        Mesh { origin, positions, indices, ..Default::default() }
    }

    fn length(segments: &[[DVec3; 2]]) -> f64 {
        segments.iter().map(|[a, b]| a.distance(*b)).sum()
    }

    #[test]
    fn a_plane_across_a_cube_cuts_its_outline() {
        let mut out = Vec::new();
        // x = 10.25 (the cube around x = 10).
        slice(&cube(DVec3::new(10.0, 0.0, 0.0)), DVec3::X, -10.25, &mut out);
        assert_eq!(out.len(), 8, "two triangles per side crossed");
        assert!((length(&out) - 8.0).abs() < 1e-9);
        assert!(out.iter().flatten().all(|p| (p.x - 10.25).abs() < 1e-9));
    }

    #[test]
    fn a_plane_on_a_side_cuts_it_once_or_not_at_all() {
        let mut out = Vec::new();
        // On the side x = −1, the rest cut away: its outline, once.
        slice(&cube(DVec3::ZERO), DVec3::X, 1.0, &mut out);
        assert!((length(&out) - 8.0).abs() < 1e-9, "{out:?}");
        // On the side x = 1, the rest kept: nothing cut.
        out.clear();
        slice(&cube(DVec3::ZERO), DVec3::X, -1.0, &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn the_plane_in_a_placements_axes_cuts_the_same_points() {
        let t = DAffine3::from_rotation_translation(DQuat::from_euler(glam::EulerRot::XYZ, 0.3, -1.1, 2.0), DVec3::new(5.0, -7.0, 3.0));
        let section = Section { origin: DVec3::new(1.0, 2.0, -4.0), normal: DVec3::new(0.2, -0.5, 0.8).normalize() };
        let (n, offset) = local_plane(&t, &section);
        for x in [DVec3::ZERO, DVec3::new(3.0, -2.0, 1.0), DVec3::new(-9.0, 4.0, 6.0)] {
            let d = (t.transform_point3(x) - section.origin).dot(section.normal);
            assert!((n.dot(x) + offset - d).abs() < 1e-9);
        }
        // A box straddles the plane only if it has points on both sides.
        assert!(straddles(DVec3::splat(-1.0), DVec3::splat(1.0), DVec3::X, 0.5));
        assert!(!straddles(DVec3::splat(-1.0), DVec3::splat(1.0), DVec3::X, 1.5));
        assert!(!straddles(DVec3::splat(-1.0), DVec3::splat(1.0), DVec3::X, -1.5));
    }
}

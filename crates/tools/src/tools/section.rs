//! Section plane (§7 quater): click a face, the plane lies on it; move the cursor to sink it
//! into the model along the face's normal (or type the depth); click again to place it. What
//! lies outside of it — towards the face's outside — is cut away from the view. The cut shows
//! while the plane is being placed.

use glam::{DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::{Operation, Section};

use super::{Command, Cx, Preview, PreviewLine, Tool, ToolInput, ToolKind};
use crate::vcb::{Vcb, VcbKind, VcbValue};

#[derive(Clone, Copy, Debug)]
struct Placing {
    /// The point clicked on the face, and the face's outward normal (world).
    origin: DVec3,
    normal: DVec3,
    /// How deep into the model the plane is (mm, along -normal).
    depth: f64,
}

impl Placing {
    fn section(&self) -> Section {
        Section { origin: self.origin - self.normal * self.depth, normal: self.normal }
    }
}

#[derive(Default)]
pub struct SectionTool {
    /// The face under the cursor: a point and its normal facing the eye (world).
    hover: Option<(DVec3, DVec3)>,
    placing: Option<Placing>,
}

impl SectionTool {
    fn commit(&mut self, cx: &Cx<'_>) -> Vec<Command> {
        let Some(p) = self.placing.take() else { return vec![] };
        let mut settings = cx.doc.settings().clone();
        settings.display.section = Some(p.section());
        settings.display.section_active = true;
        vec![Command::Apply(Operation::SetSettings(settings))]
    }
}

/// How far along the line `origin + t·dir` (unit) the ray through `px` passes closest.
fn along(cx: &Cx<'_>, px: DVec2, origin: DVec3, dir: DVec3) -> f64 {
    let r = cx.ray(px);
    let w = r.origin - origin;
    let (b, d, e) = (dir.dot(r.dir), dir.dot(w), r.dir.dot(w));
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-9 {
        return 0.0; // looking along the line
    }
    (d - b * e) / denom
}

/// The polygon where the plane through `origin` (unit `normal`) crosses the box `lo`–`hi`, its
/// corners in order around it; empty when the plane misses the box.
fn plane_in_box(origin: DVec3, normal: DVec3, lo: DVec3, hi: DVec3) -> Vec<DVec3> {
    let corner = |i: usize| {
        DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z })
    };
    let side = |p: DVec3| (p - origin).dot(normal);
    // The 12 edges: corners differing by one bit.
    let mut points: Vec<DVec3> = Vec::new();
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit != 0 {
                continue;
            }
            let (a, b) = (corner(i), corner(i | bit));
            let (da, db) = (side(a), side(b));
            if (da > 0.0) == (db > 0.0) && da != 0.0 && db != 0.0 {
                continue;
            }
            let t = if da == db { 0.0 } else { da / (da - db) };
            let p = a + (b - a) * t;
            if !points.iter().any(|q| (*q - p).length_squared() < 1e-12) {
                points.push(p);
            }
        }
    }
    if points.len() < 3 {
        return Vec::new();
    }
    // In order around their centre, in the plane's axes.
    let centre = points.iter().sum::<DVec3>() / points.len() as f64;
    let u = normal.any_orthonormal_vector();
    let v = normal.cross(u);
    let angle = |p: &DVec3| (*p - centre).dot(v).atan2((*p - centre).dot(u));
    points.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
    points
}

impl Tool for SectionTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Section
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => match &mut self.placing {
                Some(p) => p.depth = -along(cx, *px, p.origin, p.normal),
                None => self.hover = cx.pick(*px).map(|h| (h.point, h.normal)),
            },
            ToolInput::Click { px, .. } => {
                if self.placing.is_some() {
                    return self.commit(cx);
                }
                if let Some(h) = cx.pick(*px) {
                    self.placing = Some(Placing { origin: h.point, normal: h.normal, depth: 0.0 });
                }
            }
            ToolInput::Value(VcbValue::Lengths(v)) => {
                if let (Some(p), Some(d)) = (&mut self.placing, v.first()) {
                    p.depth = *d;
                    return self.commit(cx);
                }
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let (origin, normal, section) = match (&self.placing, &self.hover) {
            (Some(p), _) => (p.section().origin, p.normal, Some(p.section())),
            (None, Some((o, n))) => (*o, *n, None),
            (None, None) => return Preview::default(),
        };
        let color = super::PICK;
        // The plane where it crosses the model's box (padded): it stays where the geometry
        // is, and does not reach behind the eye when the plane is seen edge on. A plane past
        // the box (sunk too deep) shows as a square, sized for the model.
        let bounds = cx.scene.bounds();
        let size = bounds.map_or(100.0, |(lo, hi)| (hi - lo).length().max(10.0));
        let corners = bounds
            .map(|(lo, hi)| {
                let pad = DVec3::splat(size * 0.05);
                plane_in_box(origin, normal, lo - pad, hi + pad)
            })
            .filter(|c| c.len() >= 3)
            .unwrap_or_else(|| {
                let half = size * 0.35;
                let u = normal.any_orthonormal_vector();
                let v = normal.cross(u);
                let c = |a: f64, b: f64| origin + (u * a + v * b) * half;
                vec![c(-1.0, -1.0), c(1.0, -1.0), c(1.0, 1.0), c(-1.0, 1.0)]
            });
        let n = corners.len();
        let mut lines: Vec<PreviewLine> = (0..n).map(|k| PreviewLine { a: corners[k], b: corners[(k + 1) % n], color }).collect();
        // Its normal: towards what is cut away.
        lines.push(PreviewLine { a: origin, b: origin + normal * size * 0.1, color });
        Preview { lines, section, ..Default::default() }
    }

    fn status(&self) -> String {
        match self.placing {
            None => t!("tool.section.hint.face"),
            Some(_) => t!("tool.section.hint.depth"),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        self.placing.map(|p| Vcb::new(t!("tool.section.vcb.depth"), VcbKind::Lengths(vec![p.depth])))
    }

    fn busy(&self) -> bool {
        self.placing.is_some()
    }

    fn reset(&mut self) {
        self.placing = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plane_crosses_a_box_as_a_polygon() {
        let (lo, hi) = (DVec3::ZERO, DVec3::splat(10.0));
        // Parallel to a face: a square at the plane's height.
        let sq = plane_in_box(DVec3::new(3.0, 3.0, 4.0), DVec3::Z, lo, hi);
        assert_eq!(sq.len(), 4);
        assert!(sq.iter().all(|p| (p.z - 4.0).abs() < 1e-9));
        // Through the centre, along the diagonal: a hexagon.
        let hex = plane_in_box(DVec3::splat(5.0), DVec3::ONE.normalize(), lo, hi);
        assert_eq!(hex.len(), 6);
        // In order: consecutive corners lie on a common face of the box (no crossing).
        for k in 0..hex.len() {
            let (a, b) = (hex[k], hex[(k + 1) % hex.len()]);
            let shared = (0..3).any(|i| {
                let (x, y) = (a[i], b[i]);
                (x.abs() < 1e-9 && y.abs() < 1e-9) || ((x - 10.0).abs() < 1e-9 && (y - 10.0).abs() < 1e-9)
            });
            assert!(shared, "{a} – {b} cross the box");
        }
        // Past the box: nothing.
        assert!(plane_in_box(DVec3::splat(20.0), DVec3::Z, lo, hi).is_empty());
    }
}

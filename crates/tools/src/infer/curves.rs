//! The curves of the scene in the world: edges placed by their occurrence (arcs maybe
//! scaled into ellipses), guide circles; as drawn, as references, their point nearest the
//! cursor.

use std::f64::consts::TAU;

use glam::{DAffine3, DVec2, DVec3};
use skraft_kernel::SnapCurve;
use skraft_model::{EntityPath, GuideShape};

use super::{Curve, EdgeRef, Query};
use crate::camera::{Projector, Ray};
use crate::scene::{DisplayScene, Owner, Piece, SceneGuide};

/// A guide circle as an arc (a whole one), in the world.
pub(super) fn guide_arc(center: DVec3, normal: DVec3, radius: f64) -> SnapCurve {
    let x = normal.any_orthonormal_vector();
    SnapCurve::Arc { center, x, y: normal.cross(x), radius, sweep: TAU }
}

/// The guides the tool's filter keeps (not those it moves).
pub(super) fn kept_guides<'a>(scene: &'a DisplayScene, q: &Query<'_>) -> Vec<&'a SceneGuide> {
    scene.guides().iter().filter(|g| (q.keep)(&Owner { path: &EntityPath::new(g.path.clone(), g.id), piece: Piece::Guide(g.id) })).collect()
}

/// The guide circles among `guides`, each with its arc.
pub(super) fn guide_arcs<'a>(guides: &'a [&SceneGuide]) -> impl Iterator<Item = (GuideShape, SnapCurve)> + 'a {
    guides.iter().filter_map(|g| match g.shape {
        GuideShape::Circle { center, normal, radius } => Some((g.shape, guide_arc(center, normal, radius))),
        GuideShape::Line { .. } => None,
    })
}

/// A guide circle as a curve drawn.
pub(super) fn guide_circle(center: DVec3, normal: DVec3, radius: f64) -> Curve {
    Curve::Guide(GuideShape::Circle { center, normal, radius })
}

/// The curve `curve` (placed by `t`) as drawn, in the world.
pub(super) fn drawn(curve: &SnapCurve, t: &DAffine3) -> Curve {
    match *curve {
        SnapCurve::Segment { a, b } => Curve::Segment(t.transform_point3(a), t.transform_point3(b)),
        SnapCurve::Arc { center, x, y, radius, sweep } => Curve::Arc(PlacedArc::new(t, center, x, y, radius, sweep).samples()),
        SnapCurve::Polyline { ref points, .. } => Curve::Arc(points.iter().map(|p| t.transform_point3(*p)).collect()),
    }
}

/// The curve `curve` (placed by `t`) at its point `p` (world), as a reference.
pub(super) fn curve_ref(curve: &SnapCurve, t: &DAffine3, p: DVec3) -> Option<EdgeRef> {
    match *curve {
        SnapCurve::Segment { a, b } => t.transform_vector3(b - a).try_normalize().map(EdgeRef::Straight),
        SnapCurve::Arc { center, x, y, radius, sweep } => {
            let arc = PlacedArc::new(t, center, x, y, radius, sweep);
            let PlacedArc { c, n, rx, .. } = arc;
            let d = p - c;
            let radial = (d - n * d.dot(n)).try_normalize()?;
            // An arc scaled into an ellipse has no one radius.
            let radius = if arc.is_circle() { rx } else { (d - n * d.dot(n)).length() };
            Some(EdgeRef::Arc { tangent: n.cross(radial), radial, center: c, radius })
        }
        SnapCurve::Polyline { .. } => None,
    }
}

/// The point of `curve` (placed by `t`) nearest the cursor, in the world.
pub(super) fn on_curve(curve: &SnapCurve, t: &DAffine3, ray: &Ray, screen: &Projector, px: DVec2) -> Option<DVec3> {
    match *curve {
        SnapCurve::Segment { a, b } => Some(on_segment(ray, t.transform_point3(a), t.transform_point3(b))),
        SnapCurve::Arc { center, x, y, radius, sweep } => {
            let arc = PlacedArc::new(t, center, x, y, radius, sweep);
            let dn = ray.dir.dot(arc.n);
            if dn.abs() > 1e-3 && arc.is_circle() {
                // Seen from the side of its plane: where the ray's line meets the plane (a parallel
                // projection may start past it: the line, not the half-line), radially.
                let q = ray.origin + ray.dir * ((arc.c - ray.origin).dot(arc.n) / dn);
                let mut a = arc.angle_of(q);
                if a > sweep {
                    // Past the arc's end: the nearer end.
                    a = if a - sweep < TAU - a { sweep } else { 0.0 };
                }
                return Some(arc.at(a));
            }
            // Edge on (or unevenly scaled): sample it.
            on_polyline(&arc.samples(), ray, screen, px)
        }
        SnapCurve::Polyline { ref points, .. } => {
            let pts: Vec<DVec3> = points.iter().map(|p| t.transform_point3(*p)).collect();
            on_polyline(&pts, ray, screen, px)
        }
    }
}

/// An arc placed in the world: a placement may scale it, unevenly into an ellipse.
#[derive(Clone, Copy)]
pub(super) struct PlacedArc {
    pub(super) c: DVec3,
    /// Unit axes of the arc's plane, from the placed `x` and `y`.
    pub(super) x: DVec3,
    pub(super) y: DVec3,
    /// Unit normal.
    pub(super) n: DVec3,
    /// Radius along each axis.
    pub(super) rx: f64,
    pub(super) ry: f64,
    pub(super) sweep: f64,
}

impl PlacedArc {
    /// How many chords stand for an arc that has to be sampled.
    const SAMPLES: usize = 64;

    pub(super) fn new(t: &DAffine3, center: DVec3, x: DVec3, y: DVec3, radius: f64, sweep: f64) -> Self {
        let (xw, yw) = (t.transform_vector3(x), t.transform_vector3(y));
        let (x, y) = (xw.normalize(), yw.normalize());
        PlacedArc { c: t.transform_point3(center), x, y, n: x.cross(y), rx: xw.length() * radius, ry: yw.length() * radius, sweep }
    }

    /// Still a circle (evenly scaled).
    pub(super) fn is_circle(&self) -> bool {
        (self.rx - self.ry).abs() <= 1e-9 * self.rx.max(1.0)
    }

    /// The point at angle `a`.
    pub(super) fn at(&self, a: f64) -> DVec3 {
        self.c + self.x * (self.rx * a.cos()) + self.y * (self.ry * a.sin())
    }

    /// The angle of `p` around the centre, in [0, 2π).
    pub(super) fn angle_of(&self, p: DVec3) -> f64 {
        let d = p - self.c;
        let a = d.dot(self.y).atan2(d.dot(self.x));
        if a < 0.0 { a + TAU } else { a }
    }

    /// [`Self::SAMPLES`] + 1 points along it, ends included.
    pub(super) fn samples(&self) -> Vec<DVec3> {
        (0..=Self::SAMPLES).map(|i| self.at(self.sweep * i as f64 / Self::SAMPLES as f64)).collect()
    }
}

pub(super) fn on_polyline(pts: &[DVec3], ray: &Ray, screen: &Projector, px: DVec2) -> Option<DVec3> {
    pts.windows(2)
        .map(|w| on_segment(ray, w[0], w[1]))
        .filter_map(|p| Some((screen.project(p)?.distance(px), p)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
}

/// The point of the segment `[a, b]` nearest the ray's line.
pub(super) fn on_segment(ray: &Ray, a: DVec3, b: DVec3) -> DVec3 {
    let u = b - a;
    let len2 = u.length_squared();
    if len2 < 1e-24 {
        return a;
    }
    let w = a - ray.origin;
    let (du, dw, uw) = (ray.dir.dot(u), ray.dir.dot(w), u.dot(w));
    let denom = len2 - du * du; // |u|² (1 - cos²): zero when parallel to the ray
    let s = if denom.abs() < 1e-12 * len2 { 0.0 } else { (du * dw - uw) / denom };
    a + u * s.clamp(0.0, 1.0)
}

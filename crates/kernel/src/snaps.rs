//! What the tools can infer on a solid (docs/tools/inferences.md): its visible vertices,
//! its visible edges as exact curves where the record names them (segments, circular arcs),
//! and its planar faces' exact planes. Everything in the solid's coordinates, `f64`.
//!
//! Built from the record, with the mesher's edge polylines for what has no simple exact form
//! (splines, intersection curves) and to tell which way an arc runs.

// Building snaps follows tessellation, which only the `engine` has.
#![cfg_attr(not(feature = "engine"), allow(dead_code))]

use glam::DVec3;
use serde::{Deserialize, Serialize};
use solidcore_record::brep::{Brep as Record, Curve, Surface};

/// Snap features of one solid.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snaps {
    /// Endpoints of visible edges (B-rep vertices), each once.
    pub vertices: Vec<DVec3>,
    /// Visible edges (seams between pieces of one smooth surface left out).
    pub edges: Vec<SnapEdge>,
    /// Per B-rep face: its plane, when the face is planar.
    pub planes: Vec<Option<Plane>>,
}

/// A plane: a point on it and its unit normal (sign as the surface has it).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub point: DVec3,
    pub normal: DVec3,
}

impl Plane {
    /// The point of the plane closest to `p`.
    pub fn project(&self, p: DVec3) -> DVec3 {
        p - self.normal * (p - self.point).dot(self.normal)
    }
}

/// One visible B-rep edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapEdge {
    /// Index of the edge in the record.
    pub id: u32,
    pub curve: SnapCurve,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SnapCurve {
    /// A straight edge from `a` to `b`.
    Segment { a: DVec3, b: DVec3 },
    /// A circular arc `center + radius (cos θ x + sin θ y)`, θ ∈ [0, sweep] (a full circle when
    /// `sweep` is 2π). `x` and `y` are orthonormal.
    Arc { center: DVec3, x: DVec3, y: DVec3, radius: f64, sweep: f64 },
    /// Anything else, sampled; `center` for an ellipse.
    Polyline { points: Vec<DVec3>, center: Option<DVec3> },
}

const TAU: f64 = std::f64::consts::TAU;

impl SnapCurve {
    /// Is the arc a whole circle?
    pub fn is_closed_arc(&self) -> bool {
        matches!(self, SnapCurve::Arc { sweep, .. } if *sweep > TAU - 1e-9)
    }

    /// The point of an arc at angle `t`.
    pub fn arc_point(center: DVec3, x: DVec3, y: DVec3, radius: f64, t: f64) -> DVec3 {
        center + (x * t.cos() + y * t.sin()) * radius
    }

    /// The midpoint, for a segment or an open arc.
    pub fn midpoint(&self) -> Option<DVec3> {
        match *self {
            SnapCurve::Segment { a, b } => Some((a + b) * 0.5),
            SnapCurve::Arc { center, x, y, radius, sweep } if sweep < TAU - 1e-9 => {
                Some(Self::arc_point(center, x, y, radius, sweep * 0.5))
            }
            _ => None,
        }
    }

    /// The points at one and two thirds, for a segment or an open arc (along its length).
    pub fn thirds(&self) -> Option<[DVec3; 2]> {
        match *self {
            SnapCurve::Segment { a, b } => Some([a + (b - a) / 3.0, a + (b - a) * 2.0 / 3.0]),
            SnapCurve::Arc { center, x, y, radius, sweep } if sweep < TAU - 1e-9 => {
                Some([1.0, 2.0].map(|k| Self::arc_point(center, x, y, radius, sweep * k / 3.0)))
            }
            _ => None,
        }
    }

    /// The ends, for a segment, an open arc or an open polyline (none for a closed curve).
    pub fn ends(&self) -> Vec<DVec3> {
        match *self {
            SnapCurve::Segment { a, b } => vec![a, b],
            SnapCurve::Arc { center, x, y, radius, sweep } if sweep < TAU - 1e-9 => {
                vec![Self::arc_point(center, x, y, radius, 0.0), Self::arc_point(center, x, y, radius, sweep)]
            }
            SnapCurve::Polyline { ref points, .. } if points.len() > 1 && points[0] != points[points.len() - 1] => {
                vec![points[0], points[points.len() - 1]]
            }
            _ => Vec::new(),
        }
    }

    /// The centre, for an arc, a circle or an ellipse.
    pub fn center(&self) -> Option<DVec3> {
        match *self {
            SnapCurve::Arc { center, .. } => Some(center),
            SnapCurve::Polyline { center, .. } => center,
            SnapCurve::Segment { .. } => None,
        }
    }
}

fn v3(p: [f64; 3]) -> DVec3 {
    DVec3::from_array(p)
}

impl Snaps {
    /// Snap features of `record`. `polylines[i]` is the mesher's polyline of edge `i` and
    /// `visible[i]` whether the viewport draws it.
    pub(crate) fn build(record: &Record, polylines: &[Vec<DVec3>], visible: &[bool]) -> Self {
        let mut vertex_used = vec![false; record.vertices.len()];
        let mut edges = Vec::new();
        for (i, e) in record.edges.iter().enumerate() {
            if !visible.get(i).copied().unwrap_or(false) {
                continue;
            }
            let poly = polylines.get(i).map(Vec::as_slice).unwrap_or(&[]);
            let [s, t] = e.vertices;
            let (Some(vs), Some(vt)) = (record.vertices.get(s as usize), record.vertices.get(t as usize)) else {
                continue;
            };
            let (a, b) = (v3(vs.point), v3(vt.point));
            let closed = s == t;
            let curve = match &e.curve {
                Curve::Line { .. } if !closed => Some(SnapCurve::Segment { a, b }),
                Curve::Polyline { points } if points.len() == 2 && !closed => Some(SnapCurve::Segment { a, b }),
                Curve::Circle { frame, radius } => {
                    let o = v3(frame.origin);
                    let n = v3(frame.x_axis).cross(v3(frame.y_axis)).normalize_or_zero();
                    arc(o, n, *radius, a, poly, closed)
                }
                Curve::Ellipse { frame, .. } if poly.len() >= 2 => {
                    Some(SnapCurve::Polyline { points: poly.to_vec(), center: Some(v3(frame.origin)) })
                }
                _ => None,
            };
            let curve = match curve {
                Some(c) => c,
                None if poly.len() >= 2 => SnapCurve::Polyline { points: poly.to_vec(), center: None },
                None => continue,
            };
            // The vertex of a closed edge is only where the curve was cut: not a point to infer.
            if !closed {
                vertex_used[s as usize] = true;
                vertex_used[t as usize] = true;
            }
            edges.push(SnapEdge { id: i as u32, curve });
        }
        let vertices = record.vertices.iter().zip(&vertex_used).filter(|(_, used)| **used).map(|(v, _)| v3(v.point)).collect();
        let planes = record.faces.iter().map(|f| plane_of(&f.surface)).collect();
        Snaps { vertices, edges, planes }
    }
}

/// The arc of the circle (`center`, unit normal `n`, `radius`) that starts at `start` and runs
/// as `poly` does; a whole circle when `closed`.
fn arc(center: DVec3, n: DVec3, radius: f64, start: DVec3, poly: &[DVec3], closed: bool) -> Option<SnapCurve> {
    if n == DVec3::ZERO || radius <= 0.0 {
        return None;
    }
    let flat = |p: DVec3| {
        let d = p - center;
        d - n * d.dot(n)
    };
    let x = flat(start).try_normalize()?;
    let mut y = n.cross(x);
    if closed {
        return Some(SnapCurve::Arc { center, x, y, radius, sweep: TAU });
    }
    // Which way, and how far: add up the turns along the mesher's polyline (it may start at
    // either end; only the sweep's magnitude and sense matter, measured from `start`).
    let angle = |p: DVec3| {
        let d = flat(p);
        d.dot(y).atan2(d.dot(x))
    };
    let (first, last) = (*poly.first()?, *poly.last()?);
    let reversed = (first - start).length_squared() > (last - start).length_squared();
    let pts: Vec<DVec3> = if reversed { poly.iter().rev().copied().collect() } else { poly.to_vec() };
    let mut sweep = 0.0;
    let mut prev = angle(pts[0]);
    for p in &pts[1..] {
        let a = angle(*p);
        let mut d = a - prev;
        if d > std::f64::consts::PI {
            d -= TAU;
        } else if d < -std::f64::consts::PI {
            d += TAU;
        }
        sweep += d;
        prev = a;
    }
    if sweep < 0.0 {
        y = -y;
        sweep = -sweep;
    }
    (sweep > 1e-9).then_some(SnapCurve::Arc { center, x, y, radius, sweep: sweep.min(TAU) })
}

pub(crate) fn plane_of(s: &Surface) -> Option<Plane> {
    let (point, normal) = match s {
        Surface::Plane { origin, u_axis, v_axis } => (v3(*origin), v3(*u_axis).cross(v3(*v_axis))),
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let o = v3(*origin);
            (o, (v3(*u_point) - o).cross(v3(*v_point) - o))
        }
        _ => return None,
    };
    normal.try_normalize().map(|normal| Plane { point, normal })
}

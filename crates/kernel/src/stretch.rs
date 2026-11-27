//! Stretching a solid along an axis (docs/tools/stretch.md), by rewriting its record:
//! no kernel call, so it runs anywhere (the web UI too).
//!
//! **Cutters** — planes across the axis — split space into **sections**, each translated along
//! the axis by its own **move**. What lies in one section (or in sections that move alike) is
//! translated with it. What crosses a cutter between sections that move apart must be invariant
//! along the axis, and is lengthened or shortened:
//! - an edge: a line parallel to the axis (its interval grows, its pcurves with it), or a
//!   straight degree-1 spline;
//! - a face: a plane containing the axis direction (its pcurves shift in (u, v), planes being
//!   affine), or a cylinder whose axis is the stretch axis (its NURBS net, linear in the height
//!   along u or v, has its points moved and that parameter's knots shifted) — or a cone of half
//!   angle 0, as the kernel writes a mirrored cylinder — or a NURBS net extruded along the axis,
//!   as the kernel writes a prism's arc sides (stretched like a cylinder's net).
//!
//! Anything else across a cutter refuses the stretch ([`StretchError`]), as OpenCutList refuses
//! a curve crossing a cutter. The result is only checked by the kernel when it reads the record
//! back (the recipe task does).
//!
//! Extents along the axis are taken from the vertices and the curves, plus the surface for faces
//! whose boundary does not hold their extremes (spheres, tori, splines): exact for lines and
//! arcs of circles and ellipses, conservative otherwise (a control net's hull), so a doubt
//! refuses rather than tears.

use std::f64::consts::{PI, TAU};

use glam::{DAffine3, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use skraft_i18n::t;
use solidcore_record::brep::blend::{BlendDefinition, BlendDefinitionRecord, NurbsCurveRecord, NurbsSurfaceRecord, SupportRecord};
use solidcore_record::brep::{Brep as Record, Curve, Curve2, Edge, Face, Frame, Loop, Surface};

use crate::Solid;

/// Positions closer than this to a cutter (mm) belong to the section below it; moves closer than
/// this are equal.
const EPS: f64 = 1e-6;

/// How parallel (sine of the angle) a line or a cylinder's axis must be to the stretch axis, and
/// how perpendicular a plane's normal.
const PARALLEL: f64 = 1e-9;

/// Why a stretch cannot be applied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StretchError {
    /// The stretch itself is malformed: a null axis, cutters not increasing, a move per section
    /// missing, a number not finite.
    Invalid,
    /// This edge (index in the record) crosses a cutter without being a line along the axis.
    EdgeAcross(u32),
    /// This face crosses a cutter without being a plane, a cylinder or an extrusion along the axis.
    FaceAcross(u32),
    /// This face crosses a cutter but its parameters are not of a form the stretch rewrites.
    Unsupported(u32),
    /// The compression would make something crossing a cutter vanish or turn inside out.
    Collapsed,
}

impl std::fmt::Display for StretchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            StretchError::Invalid => t!("error.stretch.invalid"),
            StretchError::EdgeAcross(_) => t!("error.stretch.edge_across"),
            StretchError::FaceAcross(_) => t!("error.stretch.face_across"),
            StretchError::Unsupported(_) => t!("error.stretch.unsupported"),
            StretchError::Collapsed => t!("error.stretch.collapsed"),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for StretchError {}

/// A piecewise translation along an axis: `cuts` (increasing positions along the axis, as
/// `axis · p`) split space into `cuts.len() + 1` sections, section `i` moved by `moves[i]` along
/// the axis. Read back from data (a job crossing to the web worker), it is checked again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawStretch", into = "RawStretch")]
pub struct Stretch {
    axis: DVec3,
    cuts: Vec<f64>,
    moves: Vec<f64>,
}

/// A [`Stretch`] as data, unchecked.
#[derive(Serialize, Deserialize)]
struct RawStretch {
    axis: DVec3,
    cuts: Vec<f64>,
    moves: Vec<f64>,
}

impl TryFrom<RawStretch> for Stretch {
    type Error = StretchError;

    fn try_from(r: RawStretch) -> Result<Self, StretchError> {
        Stretch::new(r.axis, r.cuts, r.moves)
    }
}

impl From<Stretch> for RawStretch {
    fn from(s: Stretch) -> Self {
        RawStretch { axis: s.axis, cuts: s.cuts, moves: s.moves }
    }
}

impl Stretch {
    /// `axis` is normalised; `cuts` must increase strictly, `moves` hold one move per section.
    pub fn new(axis: DVec3, cuts: Vec<f64>, moves: Vec<f64>) -> Result<Self, StretchError> {
        let axis = axis.try_normalize().ok_or(StretchError::Invalid)?;
        let increasing = cuts.windows(2).all(|w| w[1] > w[0] + EPS);
        let finite = cuts.iter().chain(&moves).all(|x| x.is_finite());
        if !increasing || !finite || moves.len() != cuts.len() + 1 {
            return Err(StretchError::Invalid);
        }
        Ok(Stretch { axis, cuts, moves })
    }

    pub fn axis(&self) -> DVec3 {
        self.axis
    }

    pub fn cuts(&self) -> &[f64] {
        &self.cuts
    }

    pub fn moves(&self) -> &[f64] {
        &self.moves
    }

    /// The section of a position along the axis: one on a cutter is in the section below it.
    pub fn section(&self, x: f64) -> usize {
        self.cuts.iter().take_while(|&&c| x > c + EPS).count()
    }

    /// The move of a position along the axis.
    pub fn move_at(&self, x: f64) -> f64 {
        self.moves[self.section(x)]
    }

    /// The move of something spanning `lo..=hi` along the axis, if it moves as one piece: every
    /// section it reaches moves alike.
    pub fn rigid_move(&self, lo: f64, hi: f64) -> Option<f64> {
        let (a, b) = (self.section(lo), self.section(hi));
        let m = self.moves[a];
        self.moves[a..=b].iter().all(|x| (x - m).abs() <= EPS).then_some(m)
    }

    /// The same stretch seen from a solid placed by `placement` (its frame → this stretch's
    /// frame, rigid, a mirror allowed): a machining's tool is stretched in its own frame.
    pub fn in_frame(&self, placement: &DAffine3) -> Stretch {
        let axis = (placement.matrix3.transpose() * self.axis).normalize();
        let shift = self.axis.dot(placement.translation);
        Stretch { axis, cuts: self.cuts.iter().map(|c| c - shift).collect(), moves: self.moves.clone() }
    }

    /// The extent of a solid along the axis (conservative for curved faces), `None` if it holds
    /// nothing.
    pub fn extent(&self, solid: &Solid) -> Option<(f64, f64)> {
        let r = solid.record();
        let edges: Vec<Span> = r.edges.iter().map(|e| self.edge_span(r, e)).collect();
        let span = (0..r.faces.len()).map(|i| self.face_span(r, i, &edges)).fold(Span::EMPTY, Span::union);
        let span = r.vertices.iter().fold(span, |s, v| s.add(self.axis.dot(v3(v.point))));
        (span.lo <= span.hi).then_some((span.lo, span.hi))
    }

    /// Per section, the extent along the axis of the edges lying wholly in it (`None` for a
    /// section without any): the matter a compression must keep apart (OpenCutList's section
    /// bounds). Edges crossing a cutter are left out: they are what lengthens or shortens.
    pub fn section_spans(&self, solid: &Solid) -> Vec<Option<(f64, f64)>> {
        let r = solid.record();
        let mut spans = vec![Span::EMPTY; self.moves.len()];
        for e in &r.edges {
            let s = self.edge_span(r, e);
            let (a, b) = (self.section(s.lo), self.section(s.hi));
            if a == b {
                spans[a] = spans[a].union(s);
            }
        }
        spans.into_iter().map(|s| (s.lo <= s.hi).then_some((s.lo, s.hi))).collect()
    }

    /// What the stretch does to a solid, piece by piece: the move of each vertex, then of
    /// each end of each edge's and each face's extent along the axis. Two stretches giving one
    /// list (up to a move of all) give one solid (up to that move): whatever is not here — where
    /// exactly the cutters fall — does not change the result.
    pub fn piece_moves(&self, solid: &Solid) -> Vec<f64> {
        let r = solid.record();
        let edges: Vec<Span> = r.edges.iter().map(|e| self.edge_span(r, e)).collect();
        let faces = (0..r.faces.len()).map(|i| self.face_span(r, i, &edges));
        let ends = edges.iter().copied().chain(faces).filter(|s| s.lo <= s.hi).flat_map(|s| [self.move_at(s.lo), self.move_at(s.hi)]);
        r.vertices.iter().map(|v| self.move_at(self.axis.dot(v3(v.point)))).chain(ends).collect()
    }

    /// The solid stretched.
    pub fn apply(&self, solid: &Solid) -> Result<Solid, StretchError> {
        let r0 = solid.record();
        let mut r = r0.clone();
        let a = self.axis;

        let vertex_moves: Vec<f64> = r0.vertices.iter().map(|v| self.move_at(a.dot(v3(v.point)))).collect();
        for (v, m) in r.vertices.iter_mut().zip(&vertex_moves) {
            v.point = (v3(v.point) + a * *m).to_array();
        }

        // Edges: translated, or lines along the axis lengthened.
        let spans: Vec<Span> = r0.edges.iter().map(|e| self.edge_span(r0, e)).collect();
        let mut kinds = Vec::with_capacity(r.edges.len());
        for (i, e) in r.edges.iter_mut().enumerate() {
            match self.rigid_move(spans[i].lo, spans[i].hi) {
                Some(m) => {
                    translate_curve(&mut e.curve, a * m);
                    if let Some(n) = &mut e.nurbs {
                        translate_curve(n, a * m);
                    }
                    kinds.push(EdgeKind::Rigid(m));
                }
                None => {
                    // The line stays; the parameter of each end shifts with its vertex.
                    let per_t = match straight(&e.curve) {
                        Some(d) if e.nurbs.is_none() && d.length_squared() > 0.0 && d.normalize().cross(a).length() <= PARALLEL => d,
                        _ => return Err(StretchError::EdgeAcross(i as u32)),
                    };
                    let per_mm = a.dot(per_t) / per_t.length_squared();
                    let [m0, m1] = e.vertices.map(|v| vertex_moves[v as usize]);
                    let old = e.interval;
                    let new = [old[0] + m0 * per_mm, old[1] + m1 * per_mm];
                    if (new[1] - new[0]) * per_t.length() <= EPS {
                        return Err(StretchError::Collapsed);
                    }
                    // A two-point polyline and a degree-1 spline are bounded by their points and
                    // knots: they follow the interval's ends, the parametrisation kept (SolidCore
                    // writes a prism's upright edges as such splines).
                    match &mut e.curve {
                        Curve::Polyline { points } => e.curve = Curve::Line { start: points[0], end: points[1] },
                        Curve::Spline { knots, points } => {
                            let p0 = v3(points[0]) - per_t * knots[1];
                            let (k0, k1) = (knots[1] + new[0] - old[0], knots[2] + new[1] - old[1]);
                            *knots = vec![k0, k0, k1, k1];
                            *points = vec![(p0 + per_t * k0).to_array(), (p0 + per_t * k1).to_array()];
                        }
                        _ => {}
                    }
                    e.interval = new;
                    kinds.push(EdgeKind::Across { old, new, moves: [m0, m1] });
                }
            }
        }

        // Faces: translated, or planes, cylinders and extrusions along the axis lengthened.
        for fi in 0..r.faces.len() {
            let span = self.face_span(r0, fi, &spans);
            let f = &mut r.faces[fi];
            if let Some(m) = self.rigid_move(span.lo, span.hi) {
                translate_surface(&mut f.surface, a * m);
                if let Some(n) = &mut f.nurbs {
                    translate_surface(n, a * m);
                }
                continue;
            }
            let map = FaceMap::of(f, a, fi as u32)?;
            for lp in &mut f.loops {
                let Loop::Edges { edges } = lp else { continue };
                for oe in edges {
                    match kinds[oe.edge as usize] {
                        EdgeKind::Rigid(m) => shift_pcurve(&mut oe.pcurve, map.shift(m)),
                        EdgeKind::Across { old, new, moves } => {
                            let Curve2::Spline { knots, points } = &mut oe.pcurve else { return Err(StretchError::Unsupported(fi as u32)) };
                            if points.len() != 2 || knots.len() != 4 {
                                return Err(StretchError::Unsupported(fi as u32));
                            }
                            for (p, m) in points.iter_mut().zip(moves) {
                                let s = map.shift(m);
                                p[0] += s.x;
                                p[1] += s.y;
                            }
                            for k in knots.iter_mut() {
                                *k = remap(*k, old, new);
                            }
                        }
                    }
                }
            }
            map.finish(self, f)?;
        }
        Ok(Solid::from_record(r))
    }

    /// An edge's extent along the axis: its vertices and its curve, a circle's or an ellipse's
    /// only along the arc the edge runs on.
    fn edge_span(&self, r: &Record, e: &Edge) -> Span {
        let ends = e.vertices.map(|v| v3(r.vertices[v as usize].point));
        let span = ends.iter().fold(Span::EMPTY, |s, p| s.add(self.axis.dot(*p)));
        let curve = match &e.curve {
            Curve::Circle { frame, radius } => self.arc_span(e, frame, [*radius, *radius], ends),
            Curve::Ellipse { frame, radii } => self.arc_span(e, frame, *radii, ends),
            c => self.curve_span(c),
        };
        span.union(curve)
    }

    /// The extent along the axis of the arc of a circle or an ellipse an edge runs on, `t` its
    /// angle (`origin + r₀ cos t · x + r₁ sin t · y`, SolidCore's `Curve`), from start to end
    /// as `t` grows. The arc is the edge's interval when the edge is parameterised on the conic;
    /// with a NURBS form, it runs between the ends on the side of that form's first inner control
    /// point (a conic's control points lie within its arc's angles). The whole conic for a closed
    /// edge or in doubt.
    fn arc_span(&self, e: &Edge, frame: &Frame, radii: [f64; 2], ends: [DVec3; 2]) -> Span {
        let a = self.axis;
        let whole = conic_span(a, frame, radii);
        let (o, x, y) = (v3(frame.origin), v3(frame.x_axis), v3(frame.y_axis));
        let angle = |p: DVec3| {
            let (u, v) = ((p - o).dot(x) / radii[0], (p - o).dot(y) / radii[1]);
            (u.hypot(v) > 1e-9).then(|| v.atan2(u))
        };
        // Counter-clockwise from one angle to another, in [0, 2π).
        let turn = |from: f64, to: f64| (to - from).rem_euclid(TAU);
        let arc = match &e.nurbs {
            None => Some((e.interval[0], e.interval[1] - e.interval[0])),
            Some(_) if e.vertices[0] == e.vertices[1] => None,
            Some(n) => {
                let inner = match n {
                    Curve::Spline { points, .. } if points.len() >= 3 => Some(v3(points[1])),
                    Curve::RationalSpline { homogeneous, .. } if homogeneous.len() >= 3 => Some(dehomogenise(&homogeneous[1])),
                    _ => None,
                };
                match (angle(ends[0]), angle(ends[1]), inner.and_then(angle)) {
                    (Some(t0), Some(t1), Some(tm)) if turn(t0, tm) < turn(t0, t1) => Some((t0, turn(t0, t1))),
                    (Some(t0), Some(t1), Some(_)) => Some((t1, turn(t1, t0))),
                    _ => None,
                }
            }
        };
        let Some((start, length)) = arc.filter(|&(_, l)| l < TAU) else { return whole };
        // a · p = c + R cos(t − φ): its extremes at φ and φ + π, where the arc reaches them.
        let (ax, ay) = (radii[0] * a.dot(x), radii[1] * a.dot(y));
        let phi = ay.atan2(ax);
        let at = |t: f64| a.dot(o) + ax * t.cos() + ay * t.sin();
        let mut span = Span::EMPTY.add(at(start)).add(at(start + length));
        if turn(start, phi) <= length {
            span = span.add(whole.hi);
        }
        if turn(start, phi + PI) <= length {
            span = span.add(whole.lo);
        }
        span
    }

    /// A curve's extent along the axis, beyond its vertices: whole circles and ellipses, control
    /// hulls of splines. Nothing for a straight curve: its vertices bound it (a mirrored line's
    /// spline has its control points at ±1234567).
    fn curve_span(&self, c: &Curve) -> Span {
        let a = self.axis;
        if straight(c).is_some() {
            return Span::EMPTY;
        }
        match c {
            Curve::Line { .. } => Span::EMPTY,
            Curve::Circle { frame, radius } => conic_span(a, frame, [*radius, *radius]),
            Curve::Ellipse { frame, radii } => conic_span(a, frame, *radii),
            Curve::Polyline { points } | Curve::Spline { points, .. } => points.iter().fold(Span::EMPTY, |s, p| s.add(a.dot(v3(*p)))),
            Curve::RationalSpline { homogeneous, .. } => homogeneous.iter().fold(Span::EMPTY, |s, h| s.add(a.dot(dehomogenise(h)))),
            Curve::SurfaceIntersection { leader, tolerance, .. } => self.curve_span(leader).grow(*tolerance),
        }
    }

    /// A face's extent along the axis: its boundary, plus its surface where the boundary does not
    /// hold its extremes. On a plane, a cylinder or a cone, a · p is linear along each straight line
    /// of the surface, so its extremes lie on the boundary.
    fn face_span(&self, r: &Record, fi: usize, edges: &[Span]) -> Span {
        let f = &r.faces[fi];
        let a = self.axis;
        let mut span = Span::EMPTY;
        for lp in &f.loops {
            match lp {
                Loop::Edges { edges: es } => span = es.iter().fold(span, |s, oe| s.union(edges[oe.edge as usize])),
                Loop::Vertex { vertex } => span = span.add(a.dot(v3(r.vertices[*vertex as usize].point))),
            }
        }
        let surface = match &f.surface {
            Surface::Plane { .. } | Surface::PlaneThrough { .. } | Surface::Cylinder { .. } | Surface::Cone { .. } => Span::EMPTY,
            _ => surface_span(a, f.parameters()),
        };
        span.union(surface)
    }
}

/// What became of an edge: translated by a move, or a line lengthened from `old` to `new` (its
/// interval), its ends moved by `moves`.
#[derive(Clone, Copy)]
enum EdgeKind {
    Rigid(f64),
    Across { old: [f64; 2], new: [f64; 2], moves: [f64; 2] },
}

/// How a face crossing a cutter maps a move to a shift of its (u, v).
enum FaceMap {
    /// A plane: `u_axis`, `v_axis` (not necessarily orthonormal), and the stretch axis.
    Plane { u: DVec3, v: DVec3, a: DVec3 },
    /// A surface extruded along the axis — a cylinder, or a NURBS net extruded (a prism's arc
    /// side): its parameter `dir` (0: u, 1: v) grows by `rate` per mm of height along its `z`;
    /// `s` = a · z; heights are measured along `z` from `origin`: `height(t) = h0 + (t - t0) /
    /// rate`.
    Extrusion { z: DVec3, origin: DVec3, s: f64, dir: usize, rate: f64, t0: f64, h0: f64, net: Net },
}

/// Where an extruded face holds a NURBS net to stretch with it.
#[derive(Clone, Copy, PartialEq)]
enum Net {
    /// None: an analytic cylinder.
    None,
    /// In its `nurbs`, beside its cylinder (SMLib's form).
    Shadow,
    /// As its surface (SolidCore writes a prism's arc sides so).
    Surface,
}

/// A NURBS net extruded along `z`, as SMLib and SolidCore write one: degree 1 along one of its
/// parameters (four knots), two points of equal weights that way. Along v (SMLib's cylinders, a
/// prism's arc sides): rows of two; along u (a round's cone of half angle 0 out of a boolean,
/// seen on a user's part): two rows, each pair apart
/// along `z` alike. Its `(dir, t0, h0, rate)` ([`FaceMap::Extrusion`]), the heights along `z`.
fn extruded_net(z: DVec3, u_knots: &[f64], v_knots: &[f64], homogeneous: &[Vec<[f64; 4]>]) -> Option<(usize, f64, f64, f64)> {
    let paired = |p: &[f64; 4], q: &[f64; 4]| (p[3] - q[3]).abs() <= 1e-12 * p[3].abs();
    let (dir, knots, p, q) =
        if !homogeneous.is_empty() && v_knots.len() == 4 && homogeneous.iter().all(|row| row.len() == 2 && paired(&row[0], &row[1])) {
            (1, v_knots, homogeneous[0][0], homogeneous[0][1])
        } else if let [r0, r1] = homogeneous
            && u_knots.len() == 4
            && !r0.is_empty()
            && r0.len() == r1.len()
        {
            let d = dehomogenise(&r1[0]) - dehomogenise(&r0[0]);
            let along = r0.iter().zip(r1).all(|(p, q)| paired(p, q) && (dehomogenise(q) - dehomogenise(p) - d).length() <= EPS);
            if !along || d.cross(z).length() > PARALLEL * d.length() {
                return None;
            }
            (0, u_knots, r0[0], r1[0])
        } else {
            return None;
        };
    let (h0, h1) = (z.dot(dehomogenise(&p)), z.dot(dehomogenise(&q)));
    let (t0, t1) = (knots[0], knots[3]);
    if (h1 - h0).abs() <= EPS || (t1 - t0).abs() <= 0.0 {
        return None;
    }
    Some((dir, t0, h0, (t1 - t0) / (h1 - h0)))
}

impl FaceMap {
    fn of(f: &Face, a: DVec3, fi: u32) -> Result<FaceMap, StretchError> {
        let across = StretchError::FaceAcross(fi);
        let unsupported = StretchError::Unsupported(fi);
        match &f.surface {
            Surface::Plane { u_axis, v_axis, .. } | Surface::PlaneThrough { u_point: u_axis, v_point: v_axis, .. } => {
                let (u, v) = match &f.surface {
                    Surface::PlaneThrough { origin, .. } => (v3(*u_axis) - v3(*origin), v3(*v_axis) - v3(*origin)),
                    _ => (v3(*u_axis), v3(*v_axis)),
                };
                let n = u.cross(v).try_normalize().ok_or(unsupported.clone())?;
                if n.dot(a).abs() > PARALLEL {
                    return Err(across);
                }
                if f.nurbs.is_some() {
                    return Err(unsupported);
                }
                Ok(FaceMap::Plane { u, v, a })
            }
            // A mirrored cylinder comes back from the kernel as a cone of half angle 0.
            Surface::Cylinder { frame, .. } | Surface::Cone { frame, half_angle: 0.0, .. } => {
                let z = v3(frame.x_axis).cross(v3(frame.y_axis)).normalize();
                if z.cross(a).length() > PARALLEL {
                    return Err(across);
                }
                let origin = v3(frame.origin);
                let s = a.dot(z);
                match &f.nurbs {
                    // `origin + v z + …`: v is the height from the origin.
                    None => Ok(FaceMap::Extrusion { z, origin, s, dir: 1, rate: 1.0, t0: 0.0, h0: z.dot(origin), net: Net::None }),
                    Some(Surface::RationalSpline { u_knots, v_knots, homogeneous, .. }) => {
                        let (dir, t0, h0, rate) = extruded_net(z, u_knots, v_knots, homogeneous).ok_or(unsupported)?;
                        Ok(FaceMap::Extrusion { z, origin, s, dir, rate, t0, h0, net: Net::Shadow })
                    }
                    Some(_) => Err(unsupported),
                }
            }
            // A NURBS net extruded along the axis: each row's two points apart along it, by the
            // same height.
            Surface::RationalSpline { u_knots, v_knots, homogeneous, .. } if f.nurbs.is_none() => {
                let Some(first) = homogeneous.first().filter(|row| row.len() == 2) else { return Err(across) };
                let origin = dehomogenise(&first[0]);
                let d = dehomogenise(&first[1]) - origin;
                let along = |row: &Vec<[f64; 4]>| row.len() == 2 && (dehomogenise(&row[1]) - dehomogenise(&row[0]) - d).length() <= EPS;
                let z = d.try_normalize().filter(|z| z.cross(a).length() <= PARALLEL).ok_or(across.clone())?;
                if !homogeneous.iter().all(along) {
                    return Err(across);
                }
                let (_, t0, h0, rate) = extruded_net(z, u_knots, v_knots, homogeneous).filter(|n| n.0 == 1).ok_or(unsupported)?;
                Ok(FaceMap::Extrusion { z, origin, s: a.dot(z), dir: 1, rate, t0, h0, net: Net::Surface })
            }
            _ => Err(across),
        }
    }

    /// The (u, v) shift of a point moved by `m` along the axis.
    fn shift(&self, m: f64) -> DVec2 {
        match self {
            FaceMap::Plane { u, v, a } => {
                // m a lies in the plane: solve m a = α u + β v by its Gram system.
                let d = *a * m;
                let (uu, uv, vv) = (u.dot(*u), u.dot(*v), v.dot(*v));
                let (du, dv) = (d.dot(*u), d.dot(*v));
                let det = uu * vv - uv * uv;
                DVec2::new((du * vv - dv * uv) / det, (dv * uu - du * uv) / det)
            }
            FaceMap::Extrusion { s, dir, rate, .. } => {
                let mut d = DVec2::ZERO;
                d[*dir] = m * s * rate;
                d
            }
        }
    }

    /// The domain (and an extrusion's net) after the pcurves moved.
    fn finish(&self, stretch: &Stretch, f: &mut Face) -> Result<(), StretchError> {
        match self {
            FaceMap::Plane { .. } => {
                let mut lo = DVec2::splat(f64::INFINITY);
                let mut hi = DVec2::splat(f64::NEG_INFINITY);
                for lp in &f.loops {
                    let Loop::Edges { edges } = lp else { continue };
                    for oe in edges {
                        for p in pcurve_points(&oe.pcurve) {
                            lo = lo.min(p);
                            hi = hi.max(p);
                        }
                    }
                }
                if !(hi.x - lo.x > EPS && hi.y - lo.y > EPS) {
                    return Err(StretchError::Collapsed);
                }
                f.domain = [[lo.x, hi.x], [lo.y, hi.y]];
            }
            FaceMap::Extrusion { z, origin, s, dir, rate, t0, h0, net } => {
                let a = stretch.axis;
                // Position along the stretch axis of the point at height h on the line along `z` through
                // `origin`.
                let along = |h: f64| a.dot(*origin + *z * (h - z.dot(*origin)));
                let height = |t: f64| h0 + (t - t0) / rate;
                let dt = |t: f64| stretch.move_at(along(height(t))) * s * rate;
                let [d0, d1] = f.domain[*dir];
                let domain = [d0 + dt(d0), d1 + dt(d1)];
                if (domain[1] - domain[0]) / rate.abs() <= EPS {
                    return Err(StretchError::Collapsed);
                }
                f.domain[*dir] = domain;
                if *net != Net::None {
                    let net = if *net == Net::Shadow { f.nurbs.as_mut() } else { Some(&mut f.surface) };
                    let Some(Surface::RationalSpline { u_knots, v_knots, homogeneous, .. }) = net else { unreachable!() };
                    let knots = if *dir == 0 { u_knots } else { v_knots };
                    let (k0, k1) = (knots[0], knots[3]);
                    let (n0, n1) = (k0 + dt(k0), k1 + dt(k1));
                    knots[..2].fill(n0);
                    knots[2..].fill(n1);
                    for row in homogeneous.iter_mut() {
                        for h in row.iter_mut() {
                            let m = stretch.move_at(a.dot(dehomogenise(h)));
                            let w = h[3];
                            for i in 0..3 {
                                h[i] += a[i] * m * w;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// A closed interval along the axis; empty when `lo > hi`.
#[derive(Clone, Copy, Debug)]
struct Span {
    lo: f64,
    hi: f64,
}

impl Span {
    const EMPTY: Span = Span { lo: f64::INFINITY, hi: f64::NEG_INFINITY };
    const ALL: Span = Span { lo: f64::NEG_INFINITY, hi: f64::INFINITY };

    fn add(self, x: f64) -> Span {
        Span { lo: self.lo.min(x), hi: self.hi.max(x) }
    }

    fn union(self, o: Span) -> Span {
        Span { lo: self.lo.min(o.lo), hi: self.hi.max(o.hi) }
    }

    fn grow(self, d: f64) -> Span {
        Span { lo: self.lo - d, hi: self.hi + d }
    }
}

/// The extent along `a` of a whole circle or ellipse.
fn conic_span(a: DVec3, frame: &Frame, radii: [f64; 2]) -> Span {
    let c = a.dot(v3(frame.origin));
    let half = (radii[0] * a.dot(v3(frame.x_axis))).hypot(radii[1] * a.dot(v3(frame.y_axis)));
    Span { lo: c - half, hi: c + half }
}

/// The extent along `a` of a whole surface, conservatively: a sphere, a torus or a ball's
/// sphere; a spline's control hull; everything for a revolution (never treated as rigid
/// across a cutter that moves).
fn surface_span(a: DVec3, s: &Surface) -> Span {
    match s {
        Surface::Sphere { frame, radius } => Span { lo: a.dot(v3(frame.origin)), hi: a.dot(v3(frame.origin)) }.grow(*radius),
        Surface::Torus { frame, major_radius, minor_radius, .. } => {
            Span { lo: a.dot(v3(frame.origin)), hi: a.dot(v3(frame.origin)) }.grow(major_radius + minor_radius)
        }
        Surface::BallPatch { center, radius } => Span { lo: a.dot(v3(*center)), hi: a.dot(v3(*center)) }.grow(*radius),
        Surface::Spline { points, .. } => points.iter().flatten().fold(Span::EMPTY, |s, p| s.add(a.dot(v3(*p)))),
        Surface::RationalSpline { homogeneous, .. } => homogeneous.iter().flatten().fold(Span::EMPTY, |s, h| s.add(a.dot(dehomogenise(h)))),
        _ => Span::ALL,
    }
}

fn translate_curve(c: &mut Curve, d: DVec3) {
    match c {
        Curve::Line { start, end } => {
            translate(start, d);
            translate(end, d);
        }
        Curve::Circle { frame, .. } | Curve::Ellipse { frame, .. } => translate(&mut frame.origin, d),
        Curve::Polyline { points } | Curve::Spline { points, .. } => points.iter_mut().for_each(|p| translate(p, d)),
        Curve::RationalSpline { homogeneous, .. } => homogeneous.iter_mut().for_each(|h| translate_homogeneous(h, d)),
        Curve::SurfaceIntersection { surfaces, leader, .. } => {
            surfaces.iter_mut().for_each(|s| translate_surface(s, d));
            translate_curve(leader, d);
        }
    }
}

fn translate_surface(s: &mut Surface, d: DVec3) {
    match s {
        Surface::Plane { origin, .. } => translate(origin, d),
        Surface::PlaneThrough { origin, u_point, v_point } => {
            translate(origin, d);
            translate(u_point, d);
            translate(v_point, d);
        }
        Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } | Surface::Sphere { frame, .. } | Surface::Torus { frame, .. } => {
            translate(&mut frame.origin, d)
        }
        Surface::BallPatch { center, .. } => translate(center, d),
        Surface::Spline { points, .. } => points.iter_mut().flatten().for_each(|p| translate(p, d)),
        Surface::RationalSpline { homogeneous, .. } => homogeneous.iter_mut().flatten().for_each(|h| translate_homogeneous(h, d)),
        // Column-major affine: the translation is the last column.
        Surface::PlacedRevolution { matrix, .. } => {
            for i in 0..3 {
                matrix[3][i] += d[i] * matrix[3][3];
            }
        }
        // The face's `nurbs` is the net, translated with it; the definition the kernel rebuilds
        // and certifies the net against must move as well.
        Surface::Blend { definition } => match definition.as_mut() {
            BlendDefinition::Procedural(r) => translate_blend(r, d),
            BlendDefinition::ExactRound(r) => {
                r.supports.iter_mut().for_each(|s| translate_support(s, d));
                if let Some(l) = &mut r.leader {
                    translate_nurbs_curve(l, d);
                }
                translate(&mut r.inputs.centre, d);
            }
        },
    }
}

/// A procedural blend's definition translated: its supports and its leader. The seeds are (u, v)
/// on the supports and the radius law a function of the leader's parameter: neither moves.
fn translate_blend(r: &mut BlendDefinitionRecord, d: DVec3) {
    r.supports.iter_mut().for_each(|s| translate_support(s, d));
    translate_nurbs_curve(&mut r.leader, d);
}

fn translate_support(s: &mut SupportRecord, d: DVec3) {
    match s {
        SupportRecord::Nurbs { form, .. } => translate_nurbs_surface(form, d),
        SupportRecord::Blend(b) => {
            translate_blend(&mut b.definition, d);
            translate_nurbs_surface(&mut b.net, d);
        }
    }
}

/// Control points stored as `(w x, w y, w z)` when rational: each moves by its weight times `d`.
fn translate_weighted(points: &mut [[f64; 3]], weights: Option<&[f64]>, d: DVec3) {
    for (i, p) in points.iter_mut().enumerate() {
        translate(p, d * weights.map_or(1.0, |w| w[i]));
    }
}

fn translate_nurbs_surface(n: &mut NurbsSurfaceRecord, d: DVec3) {
    translate_weighted(&mut n.points, n.weights.as_deref(), d);
}

fn translate_nurbs_curve(n: &mut NurbsCurveRecord, d: DVec3) {
    // Only a curve in space moves (a leader); a plane curve is in parameters.
    if n.dim == 3 && n.points_3d {
        translate_weighted(&mut n.points, n.weights.as_deref(), d);
    }
}

fn shift_pcurve(c: &mut Curve2, s: DVec2) {
    match c {
        Curve2::Spline { points, .. } => points.iter_mut().for_each(|p| {
            p[0] += s.x;
            p[1] += s.y;
        }),
        Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter_mut().for_each(|h| {
            h[0] += s.x * h[2];
            h[1] += s.y * h[2];
        }),
    }
}

fn pcurve_points(c: &Curve2) -> Vec<DVec2> {
    match c {
        Curve2::Spline { points, .. } => points.iter().map(|p| DVec2::from_array(*p)).collect(),
        Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().map(|h| DVec2::new(h[0] / h[2], h[1] / h[2])).collect(),
    }
}

/// A straight curve's derivative along its parameter: a line, or a two-point polyline or
/// degree-1 spline (the kernel writes a mirrored line as such).
fn straight(c: &Curve) -> Option<DVec3> {
    match c {
        Curve::Line { start, end } => Some(v3(*end) - v3(*start)),
        Curve::Polyline { points } if points.len() == 2 => Some(v3(points[1]) - v3(points[0])),
        Curve::Spline { knots, points } if points.len() == 2 && knots.len() == 4 && knots[2] > knots[1] => {
            Some((v3(points[1]) - v3(points[0])) / (knots[2] - knots[1]))
        }
        _ => None,
    }
}

/// `k` on the interval `old` mapped affinely onto `new`.
fn remap(k: f64, old: [f64; 2], new: [f64; 2]) -> f64 {
    new[0] + (k - old[0]) * (new[1] - new[0]) / (old[1] - old[0])
}

fn translate(p: &mut [f64; 3], d: DVec3) {
    *p = (v3(*p) + d).to_array();
}

fn translate_homogeneous(h: &mut [f64; 4], d: DVec3) {
    let w = h[3];
    for i in 0..3 {
        h[i] += d[i] * w;
    }
}

fn dehomogenise(h: &[f64; 4]) -> DVec3 {
    DVec3::new(h[0], h[1], h[2]) / h[3]
}

fn v3(p: [f64; 3]) -> DVec3 {
    DVec3::from_array(p)
}

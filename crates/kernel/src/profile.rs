//! Planar profiles: the outline of a [`crate::Primitive::Prism`] (docs/tools/prism.md
//! ) or the half profile of a [`crate::Primitive::Revolution`] (docs/tools/turned-bodies.md), a closed loop of lines and arcs in its XY plane. Each [`Corner`] carries the
//! bulge of the side leaving it, as DXF's LWPOLYLINE: the sides' ends are shared, so the
//! outline cannot open. Here: the sides, the area, the clean-up, the checks the drawing tool
//! makes before the kernel does, the box, the offset, the fillets, and points along it for
//! the previews.
//!
//! A corner may also carry a fillet radius (docs/tools/prism.md#the-shape-kept-while-editing): the
//! outline keeps the sharp corner and [`rounded`] computes the arcs wherever its shape is
//! needed, so a fillet follows every edit and stays tangent. The functions here work on the
//! outline they are given, fillets left alone; call [`rounded`] first for the shape.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// How close (mm) two points are the same one, and how far off a line or an arc a point
/// still lies on it.
const SAME: f64 = 1e-6;
/// Below this bulge, a side is straight.
const FLAT: f64 = 1e-9;

/// A corner of an outline and the side leaving it: straight (`bulge` 0) or an arc of sweep
/// 4·atan(`bulge`), turning counter-clockwise when positive. `fillet`: the radius rounding the
/// corner when both its sides are straight (0: sharp), applied by [`rounded`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corner {
    pub p: DVec2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bulge: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fillet: f64,
}

fn is_zero(b: &f64) -> bool {
    *b == 0.0
}

impl Corner {
    /// A corner whose side leaving it is straight.
    pub fn new(p: DVec2) -> Corner {
        Corner { p, bulge: 0.0, fillet: 0.0 }
    }

    /// A corner whose side leaving it is an arc of `bulge`.
    pub fn arc(p: DVec2, bulge: f64) -> Corner {
        Corner { p, bulge, fillet: 0.0 }
    }

    /// A sharp corner rounded by a fillet of radius `r`, its side leaving it straight.
    pub fn filleted(p: DVec2, r: f64) -> Corner {
        Corner { p, bulge: 0.0, fillet: r }
    }

    fn straight(&self) -> bool {
        self.bulge.abs() < FLAT
    }
}

/// The outline of the polygon `points` (all its sides straight).
pub fn polygon(points: &[DVec2]) -> Vec<Corner> {
    points.iter().map(|p| Corner::new(*p)).collect()
}

/// The bulge of an arc of signed sweep `sweep` (radians).
pub fn bulge_of(sweep: f64) -> f64 {
    (sweep / 4.0).tan()
}

/// The bulge of the arc from `a` to `b` through its middle `m` (0: straight, or no chord).
pub fn bulge_through(a: DVec2, m: DVec2, b: DVec2) -> f64 {
    let l = a.distance(b);
    if l < SAME {
        return 0.0;
    }
    // tan(sweep / 4) = 2·sagitta / chord; the middle on the chord's right: turning left.
    -2.0 * (b - a).perp_dot(m - a) / (l * l)
}

/// A side of an outline, from `a` to `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Side {
    Line {
        a: DVec2,
        b: DVec2,
    },
    /// About `c`, of radius `r`, from the angle `start` by the signed `sweep` (positive:
    /// counter-clockwise).
    Arc {
        a: DVec2,
        b: DVec2,
        c: DVec2,
        r: f64,
        start: f64,
        sweep: f64,
    },
}

impl Side {
    /// The side from `a` to `b` of `bulge`.
    pub fn new(a: DVec2, b: DVec2, bulge: f64) -> Side {
        let l = a.distance(b);
        if bulge.abs() < FLAT || l < SAME {
            return Side::Line { a, b };
        }
        let sweep = 4.0 * bulge.atan();
        let r = l * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
        // From the chord's middle to the centre: on the left of the chord for a positive bulge.
        let u = (b - a) / l;
        let left = DVec2::new(-u.y, u.x);
        let sagitta = bulge.abs() * l / 2.0;
        let c = (a + b) / 2.0 + left * (r - sagitta) * bulge.signum();
        let start = (a - c).to_angle();
        Side::Arc { a, b, c, r, start, sweep }
    }

    pub fn a(&self) -> DVec2 {
        match *self {
            Side::Line { a, .. } | Side::Arc { a, .. } => a,
        }
    }

    pub fn b(&self) -> DVec2 {
        match *self {
            Side::Line { b, .. } | Side::Arc { b, .. } => b,
        }
    }

    /// The point at `t` ∈ [0, 1] along it.
    pub fn at(&self, t: f64) -> DVec2 {
        match *self {
            Side::Line { a, b } => a.lerp(b, t),
            Side::Arc { c, r, start, sweep, .. } => c + DVec2::from_angle(start + sweep * t) * r,
        }
    }

    /// The unit direction it runs in at its start, and at its end.
    pub fn tangents(&self) -> (DVec2, DVec2) {
        match *self {
            Side::Line { a, b } => {
                let u = (b - a).normalize_or_zero();
                (u, u)
            }
            Side::Arc { start, sweep, .. } => {
                let turn = |angle: f64| DVec2::from_angle(angle).perp() * sweep.signum();
                (turn(start), turn(start + sweep))
            }
        }
    }

    /// Its length.
    pub fn length(&self) -> f64 {
        match *self {
            Side::Line { a, b } => a.distance(b),
            Side::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }

    /// Is `p` on it (within `tol`)?
    fn holds(&self, p: DVec2, tol: f64) -> bool {
        match *self {
            Side::Line { a, b } => {
                let d = b - a;
                let l2 = d.length_squared();
                if l2 <= 0.0 {
                    return p.distance(a) <= tol;
                }
                let (l, t) = (l2.sqrt(), (p - a).dot(d) / l2);
                t >= -tol / l && t <= 1.0 + tol / l && (a + d * t.clamp(0.0, 1.0)).distance(p) <= tol
            }
            Side::Arc { c, r, start, sweep, .. } => {
                if (p.distance(c) - r).abs() > tol {
                    return false;
                }
                // The angle from the start, the way it turns, within its sweep.
                let along = ((p - c).to_angle() - start) * sweep.signum();
                let along = along.rem_euclid(TAU);
                let slack = tol / r;
                along <= sweep.abs() + slack || along >= TAU - slack
            }
        }
    }

    /// Points along it from `a` to `b`, both included, an arc turning by `step` radians at most
    /// between two.
    pub fn points(&self, step: f64) -> Vec<DVec2> {
        let mut out = Vec::new();
        self.sample(step.max(1e-3), &mut out);
        out.push(self.b());
        out
    }

    /// Points along it, `a` first, `b` not: one for a line, enough for an arc to turn by
    /// `step` radians at most between two.
    fn sample(&self, step: f64, out: &mut Vec<DVec2>) {
        match *self {
            Side::Line { a, .. } => out.push(a),
            Side::Arc { a, sweep, .. } => {
                let n = ((sweep.abs() / step).ceil() as usize).clamp(1, 1024);
                // The corner itself first, not as recomputed from the centre.
                out.push(a);
                out.extend((1..n).map(|k| self.at(k as f64 / n as f64)));
            }
        }
    }
}

/// The sides of the outline `p`, the last one closing it.
pub fn sides(p: &[Corner]) -> Vec<Side> {
    let n = p.len();
    (0..n).map(|i| Side::new(p[i].p, p[(i + 1) % n].p, p[i].bulge)).collect()
}

/// Twice the signed area of the outline `p` (positive: counter-clockwise).
pub fn signed_area2(p: &[Corner]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let chord = p[i].p.perp_dot(p[(i + 1) % n].p);
            match Side::new(p[i].p, p[(i + 1) % n].p, p[i].bulge) {
                Side::Line { .. } => chord,
                // The circular segment between the chord and the arc, on its side.
                Side::Arc { r, sweep, .. } => chord + sweep.signum() * r * r * (sweep.abs() - sweep.abs().sin()),
            }
        })
        .sum()
}

/// The same outline run the other way round.
pub fn reversed(p: &[Corner]) -> Vec<Corner> {
    let n = p.len();
    // The side from the k-th corner of the reversed outline is the one into it, turned.
    (0..n).map(|k| Corner { p: p[n - 1 - k].p, bulge: -p[(2 * n - 2 - k) % n].bulge, fillet: p[n - 1 - k].fillet }).collect()
}

/// The outline counter-clockwise.
pub fn counter_clockwise(p: &[Corner]) -> Vec<Corner> {
    if signed_area2(p) < 0.0 { reversed(p) } else { p.to_vec() }
}

/// `p` with its sides of no length and its corners lying straight between two straight sides
/// dropped: the outline's true corners.
pub fn cleaned(p: &[Corner]) -> Vec<Corner> {
    let mut out = p.to_vec();
    loop {
        let n = out.len();
        if n < 2 {
            return out;
        }
        // A side of no length: its corner goes, the side before running on to the next one.
        if let Some(i) = (0..n).find(|&i| out[i].p.distance(out[(i + 1) % n].p) <= SAME) {
            out.remove(i);
            continue;
        }
        if n < 3 {
            return out;
        }
        // A corner straight between two straight sides (not folded back).
        let straight = (0..n).find(|&i| {
            let (a, b, c) = (out[(i + n - 1) % n], out[i], out[(i + 1) % n]);
            if !a.straight() || !b.straight() {
                return false;
            }
            let len = a.p.distance(c.p);
            len > SAME
                && ((c.p - a.p).perp_dot(b.p - a.p) / len).abs() <= SAME
                && (b.p - a.p).dot(c.p - a.p) > 0.0
                && (b.p - c.p).dot(a.p - c.p) > 0.0
        });
        match straight {
            Some(i) => {
                out.remove(i);
            }
            None => return out,
        }
    }
}

/// Where the lines or circles carrying `x` and `y` cross (a line's both ways, an arc's whole
/// circle).
fn carriers_meet(x: &Side, y: &Side) -> Vec<DVec2> {
    match (*x, *y) {
        (Side::Line { a, b }, Side::Line { a: c, b: d }) => {
            let (u, v) = (b - a, d - c);
            let det = u.perp_dot(v);
            if det.abs() <= 1e-12 * u.length() * v.length() {
                return Vec::new();
            }
            vec![a + u * ((c - a).perp_dot(v) / det)]
        }
        (Side::Line { a, b }, Side::Arc { c, r, .. }) | (Side::Arc { c, r, .. }, Side::Line { a, b }) => line_circle(a, b - a, c, r),
        (Side::Arc { c: c1, r: r1, .. }, Side::Arc { c: c2, r: r2, .. }) => circle_circle(c1, r1, c2, r2),
    }
}

/// Where the line through `a` along `u` meets the circle about `c` of radius `r`.
fn line_circle(a: DVec2, u: DVec2, c: DVec2, r: f64) -> Vec<DVec2> {
    let l = u.length();
    if l <= 0.0 {
        return Vec::new();
    }
    let u = u / l;
    let foot = a + u * (c - a).dot(u);
    let h2 = r * r - foot.distance_squared(c);
    if h2 < -(SAME * r) {
        return Vec::new();
    }
    let h = h2.max(0.0).sqrt();
    if h <= SAME * 1e-3 { vec![foot] } else { vec![foot - u * h, foot + u * h] }
}

/// Where the circles about `c1` and `c2` meet.
fn circle_circle(c1: DVec2, r1: f64, c2: DVec2, r2: f64) -> Vec<DVec2> {
    let d = c1.distance(c2);
    if d <= SAME || d > r1 + r2 + SAME || d < (r1 - r2).abs() - SAME {
        return Vec::new();
    }
    let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
    let h = (r1 * r1 - x * x).max(0.0).sqrt();
    let u = (c2 - c1) / d;
    let m = c1 + u * x;
    if h <= SAME * 1e-3 { vec![m] } else { vec![m - u.perp() * h, m + u.perp() * h] }
}

/// Do `x` and `y` overlap along one another (two lines along one line, two arcs of one
/// circle), beyond a point?
fn overlap(x: &Side, y: &Side) -> bool {
    match (*x, *y) {
        (Side::Line { a, b }, Side::Line { a: c, b: d }) => {
            let Some(u) = (b - a).try_normalize() else { return false };
            let off = |p: DVec2| (p - a).perp_dot(u).abs();
            if off(c) > SAME || off(d) > SAME {
                return false;
            }
            let along = |p: DVec2| (p - a).dot(u);
            let (s0, s1) = (along(c).min(along(d)), along(c).max(along(d)));
            s1.min(a.distance(b)) - s0.max(0.0) > SAME
        }
        (Side::Arc { c: c1, r: r1, .. }, Side::Arc { c: c2, r: r2, .. }) if c1.distance(c2) <= SAME && (r1 - r2).abs() <= SAME => {
            // The middle of the shorter inside the other.
            let (s, l) = if x.length() <= y.length() { (x, y) } else { (y, x) };
            l.holds(s.at(0.5), SAME) || s.holds(l.at(0.5), SAME)
        }
        _ => false,
    }
}

/// Is `p` a simple outline — two corners at least, an area, no two sides meeting but
/// neighbours at their shared corner, and none folded back there?
pub fn is_simple(p: &[Corner]) -> bool {
    let n = p.len();
    if n < 2 || signed_area2(p).abs() <= SAME * SAME {
        return false;
    }
    let s = sides(p);
    // The outline's size, for the tolerance of the crossings.
    let tol = SAME * (1.0 + bounds(p).map_or(0.0, |(lo, hi)| (hi - lo).max_element()));
    for i in 0..n {
        for j in i + 1..n {
            let (x, y) = (&s[i], &s[j]);
            if overlap(x, y) {
                return false;
            }
            // The corners they share (neighbours share one, both when there are two sides).
            let mut shared = Vec::new();
            if j == i + 1 {
                shared.push(x.b());
            }
            if i == 0 && j == n - 1 {
                shared.push(x.a());
            }
            for q in &shared {
                // Folded back: leaving the corner the way it was come into.
                let (into, out) = if *q == x.b() { (x.tangents().1, y.tangents().0) } else { (y.tangents().1, x.tangents().0) };
                if into.dot(out) < -1.0 + 1e-9 {
                    return false;
                }
            }
            let crossing = carriers_meet(x, y)
                .into_iter()
                .any(|q| x.holds(q, tol) && y.holds(q, tol) && shared.iter().all(|c| c.distance(q) > 1e3 * tol));
            if crossing {
                return false;
            }
            // Touching at a corner they do not share.
            if shared.is_empty() && [x.a(), x.b()].iter().any(|q| y.holds(*q, tol))
                || shared.is_empty() && [y.a(), y.b()].iter().any(|q| x.holds(*q, tol))
            {
                return false;
            }
        }
    }
    true
}

/// Can the half profile `p` be turned about its Y axis (a revolution's, docs/tools/turned-bodies.md): a simple
/// outline, its fillets made arcs, all on the side of positive X (touching the axis or not)?
pub fn is_turnable(p: &[Corner]) -> bool {
    let r = rounded(p);
    is_simple(&r) && bounds(&r).is_some_and(|(lo, _)| lo.x >= -SAME)
}

/// The box around the outline, its arcs' furthest points included.
pub fn bounds(p: &[Corner]) -> Option<(DVec2, DVec2)> {
    let mut lo = DVec2::INFINITY;
    let mut hi = DVec2::NEG_INFINITY;
    for s in sides(p) {
        lo = lo.min(s.a());
        hi = hi.max(s.a());
        if let Side::Arc { c, r, .. } = s {
            for k in 0..4 {
                let q = c + DVec2::from_angle(k as f64 * FRAC_PI_2) * r;
                if s.holds(q, SAME) {
                    lo = lo.min(q);
                    hi = hi.max(q);
                }
            }
        }
    }
    lo.is_finite().then_some((lo, hi))
}

/// Points along the outline, the arcs turning by `step` radians at most between two (the
/// previews), each corner among them, closing back to the first implicitly.
pub fn sample(p: &[Corner], step: f64) -> Vec<DVec2> {
    let mut out = Vec::new();
    for s in sides(p) {
        s.sample(step.max(1e-3), &mut out);
    }
    out
}

/// Why an outline has no offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetError {
    /// Nothing is left of it (an inward offset past its middle).
    Empty,
    /// The offset outline would cross itself (an inward offset pinching a neck, an outward one
    /// closing a notch), or two of its sides no longer meet.
    Crossed,
}

/// A side of an outline moved by the offset: the line or circle carrying it, and where its
/// ends went.
#[derive(Clone, Copy, Debug)]
struct Moved {
    /// A side running the same way as the original, along the new carrier (an arc's sweep is
    /// the original's, only its sign matters).
    side: Side,
    from: DVec2,
    to: DVec2,
}

/// The simple outline `p` offset by `d` (positive: outward), its corners sharp, as SolidCraft's
/// sketch offset (`crates/sketch/src/offset.rs`): each line moved along its outward normal, each
/// arc made the concentric one, the corners where neighbouring sides meet (the nearest
/// crossing to where their ends went). A side the offset turns over (inward, a short side between
/// two converging ones; outward, a notch's bottom) or an arc it shrinks to nothing is taken out
/// and its neighbours meet instead, as Clipper's miter offset does (OpenCutList's): a trapezoid
/// can become a triangle. Counter-clockwise; refused when the result crosses itself or nothing
/// is left.
pub fn offset(p: &[Corner], d: f64) -> Result<Vec<Corner>, OffsetError> {
    let p = cleaned(p);
    if !is_simple(&p) {
        return Err(OffsetError::Crossed);
    }
    let p = counter_clockwise(&p);
    if d == 0.0 {
        return Ok(p);
    }
    // Outward is on the right of a counter-clockwise outline.
    let mut moved: Vec<Moved> = Vec::with_capacity(p.len());
    for s in sides(&p) {
        match s {
            Side::Line { a, b } => {
                let u = (b - a).normalize();
                let n = DVec2::new(u.y, -u.x) * d;
                moved.push(Moved { side: Side::Line { a: a + n, b: b + n }, from: a + n, to: b + n });
            }
            Side::Arc { a, b, c, r, start, sweep } => {
                // Turning left (counter-clockwise), its centre is inside: outward grows it.
                let r2 = r + d * sweep.signum();
                if r2 <= SAME {
                    continue;
                }
                let (from, to) = (c + (a - c).normalize() * r2, c + (b - c).normalize() * r2);
                moved.push(Moved { side: Side::Arc { a: from, b: to, c, r: r2, start, sweep }, from, to });
            }
        }
    }
    let gone = || if d < 0.0 { OffsetError::Empty } else { OffsetError::Crossed };
    loop {
        let m = moved.len();
        if m < 2 {
            return Err(OffsetError::Empty);
        }
        // Each corner: where the side before meets the side after.
        let mut corners = Vec::with_capacity(m);
        for i in 0..m {
            let (x, y) = (&moved[(i + m - 1) % m], &moved[i]);
            if x.to.distance(y.from) <= SAME {
                corners.push(x.to);
                continue;
            }
            let guess = (x.to + y.from) / 2.0;
            let met = carriers_meet(&x.side, &y.side).into_iter().min_by(|a, b| a.distance(guess).total_cmp(&b.distance(guess)));
            match met {
                Some(q) => corners.push(q),
                // Neighbours made parallel by a side taken out: inward, the outline has
                // collapsed; outward, a notch has closed.
                None => return Err(gone()),
            }
        }
        // How far each side runs from its corner to the next, the way it ran: negative, turned
        // over.
        let run = |i: usize| -> f64 {
            let (a, b) = (corners[i], corners[(i + 1) % m]);
            match moved[i].side {
                Side::Line { a: s, b: e } => (b - a).dot((e - s).normalize()),
                Side::Arc { c, r, sweep, .. } => {
                    let turned = ((b - c).to_angle() - (a - c).to_angle()) * sweep.signum();
                    let turned = turned.rem_euclid(TAU);
                    // Past the original sweep by more than half a turn: it went round backwards.
                    if turned > sweep.abs() + PI { (turned - TAU) * r } else { turned * r }
                }
            }
        };
        match (0..m).filter(|&i| run(i) < -SAME).min_by(|&a, &b| run(a).total_cmp(&run(b))) {
            Some(i) => {
                moved.remove(i);
            }
            None => {
                let out: Vec<Corner> = (0..m)
                    .map(|i| match moved[i].side {
                        Side::Line { .. } => Corner::new(corners[i]),
                        Side::Arc { c, sweep, .. } => {
                            let turned =
                                (((corners[(i + 1) % m] - c).to_angle() - (corners[i] - c).to_angle()) * sweep.signum()).rem_euclid(TAU);
                            Corner::arc(corners[i], bulge_of(turned * sweep.signum()))
                        }
                    })
                    .collect();
                let out = cleaned(&out);
                if out.len() < 2 || signed_area2(&out) <= SAME * SAME {
                    return Err(OffsetError::Empty);
                }
                return if is_simple(&out) { Ok(out) } else { Err(OffsetError::Crossed) };
            }
        }
    }
}

/// Where the line through `a` along `u` meets the one through `b` along `v` (none: parallel).
fn lines_meet(a: DVec2, u: DVec2, b: DVec2, v: DVec2) -> Option<DVec2> {
    let det = u.perp_dot(v);
    if det.abs() <= 1e-12 * u.length() * v.length() {
        return None;
    }
    Some(a + u * ((b - a).perp_dot(v) / det))
}

/// The outline `p` with some of its straight sides translated, each keeping its direction
/// (docs/tools/prism.md#the-shape-kept-while-editing): `shifts`, the sides (by the index of the
/// corner they leave) and their moves. A corner between a moved side and a straight side left in
/// place slides along the latter; between two moved sides, it is where they meet; next to an
/// arc, it moves with its straight side (the arc's end follows). Fillets and bulges are kept.
/// None when a moved side can no longer meet its neighbour (parallel).
pub fn shift_sides(p: &[Corner], shifts: &[(usize, DVec2)]) -> Option<Vec<Corner>> {
    let n = p.len();
    let along = |s: usize| p[(s + 1) % n].p - p[s].p;
    let shift = |s: usize| shifts.iter().find(|(i, _)| *i == s && p[s].straight()).map(|(_, v)| *v);
    let mut out = p.to_vec();
    for (k, corner) in out.iter_mut().enumerate() {
        let (into, from) = ((k + n - 1) % n, k);
        corner.p = match (shift(into), shift(from)) {
            (None, None) => continue,
            (Some(vi), Some(vo)) => match lines_meet(p[k].p + vi, along(into), p[k].p + vo, along(from)) {
                Some(q) => q,
                // Two moved sides along one line: they must move alike.
                None if vi.distance(vo) <= SAME => p[k].p + vi,
                None => return None,
            },
            (Some(v), None) | (None, Some(v)) => {
                let (moved, other) = if shift(into).is_some() { (into, from) } else { (from, into) };
                if p[other].straight() { lines_meet(p[k].p + v, along(moved), p[k].p, along(other))? } else { p[k].p + v }
            }
        };
    }
    Some(out)
}

/// The outline `p` with its corner `i` moved to `q`, its straight sides keeping their
/// direction: each is translated through `q` ([`shift_sides`]) — a rectangle stays one, a
/// trapezoid keeps its angles. Next to an arc, the corner simply goes to `q`. When that would
/// move every corner (a triangle: a mere scaling), the corner moves alone, its sides pivoting.
pub fn corner_moved(p: &[Corner], i: usize, q: DVec2) -> Option<Vec<Corner>> {
    let n = p.len();
    let d = q - p[i].p;
    let mut shifts = Vec::with_capacity(2);
    for s in [(i + n - 1) % n, i] {
        if p[s].straight() {
            let normal = (p[(s + 1) % n].p - p[s].p).try_normalize()?.perp();
            shifts.push((s, normal * normal.dot(d)));
        }
    }
    let mut out = shift_sides(p, &shifts)?;
    out[i].p = q;
    if d.length() > SAME && out.iter().zip(p).all(|(a, b)| a.p.distance(b.p) > SAME) {
        out = p.to_vec();
        out[i].p = q;
    }
    Some(out)
}

/// The outline `p` with its straight side `i` moved by `d` across itself (along its normal,
/// `d`'s component along the side ignored), its neighbours extended or shortened
/// ([`shift_sides`]).
pub fn side_moved(p: &[Corner], i: usize, d: DVec2) -> Option<Vec<Corner>> {
    let n = p.len();
    let normal = (p[(i + 1) % n].p - p[i].p).try_normalize()?.perp();
    shift_sides(p, &[(i, normal * normal.dot(d))])
}

/// How close to 1 the dot product of two unit tangents makes them one (an arc tangent to its
/// neighbour).
const TANGENT: f64 = 1e-9;

/// The signed angle from `u` to `v`.
fn angle_to(u: DVec2, v: DVec2) -> f64 {
    u.perp_dot(v).atan2(u.dot(v))
}

/// `moved` — `base` with some corners moved, the same corners in the same order — with its arcs
/// fitted again as they lay in `base` (docs/tools/prism.md#the-shape-kept-while-editing), read
/// there, no solver:
/// - a half circle tangent to two parallel straight sides (a rounded end, a slot's end) stays
///   tangent to both: the end that did not move slides along its side to face the other, the
///   radius becoming half the gap;
/// - an arc tangent to a straight neighbour keeps that tangency (at its start if both, unless
///   only its end moved): the one arc from that end, along that side, to its other end;
/// - an arc tangent to nothing keeps its sagitta (the rise over its chord).
///
/// Arcs tangent to arcs may lose it (the sketch's job, later).
pub fn rearced(base: &[Corner], moved: &[Corner]) -> Vec<Corner> {
    let n = base.len();
    let mut out = moved.to_vec();
    if n != moved.len() || n < 2 {
        return out;
    }
    let bs = sides(base);
    let dir = |p: &[Corner], s: usize| (p[(s + 1) % n].p - p[s].p).try_normalize();
    for j in 0..n {
        if base[j].straight() {
            continue;
        }
        let (prev, next) = ((j + n - 1) % n, (j + 1) % n);
        let (t_a, t_b) = bs[j].tangents();
        let tangent_in = n > 2 && base[prev].straight() && bs[prev].tangents().1.dot(t_a) > 1.0 - TANGENT;
        let tangent_out = n > 2 && base[next].straight() && t_b.dot(bs[next].tangents().0) > 1.0 - TANGENT;
        let a_moved = out[j].p.distance(base[j].p) > SAME;
        let b_moved = out[next].p.distance(base[next].p) > SAME;
        if tangent_in && tangent_out && t_a.dot(t_b) < -1.0 + TANGENT {
            // A half circle between parallel sides: the still end faces the moved one.
            if b_moved && !a_moved {
                if let Some(u) = dir(&out, next) {
                    let d = out[next].p - out[j].p;
                    out[j].p += u * u.dot(d);
                }
            } else if let Some(u) = dir(&out, prev) {
                let d = out[j].p - out[next].p;
                out[next].p += u * u.dot(d);
            }
            continue;
        }
        let (a, b) = (out[j].p, out[next].p);
        let chord = b - a;
        if chord.length() <= SAME {
            continue;
        }
        let from_start = tangent_in && (a_moved || !b_moved || !tangent_out);
        out[j].bulge = if from_start {
            // The chord leaves the tangent at half the sweep.
            dir(&out, prev).map_or(out[j].bulge, |t| bulge_of(2.0 * angle_to(t, chord)))
        } else if tangent_out {
            dir(&out, next).map_or(out[j].bulge, |t| bulge_of(2.0 * angle_to(chord, t)))
        } else {
            // The sagitta kept: bulge = 2·sagitta / chord.
            let l0 = base[j].p.distance(base[next].p);
            base[j].bulge * l0 / chord.length()
        };
    }
    out
}

/// The centre and radius of `p` when it is a whole circle (every side an arc of one circle: a
/// cylinder's disc).
pub fn circle(p: &[Corner]) -> Option<(DVec2, f64)> {
    let s = sides(p);
    let Some(Side::Arc { c, r, .. }) = s.first().copied() else { return None };
    s.iter()
        .all(|x| matches!(*x, Side::Arc { c: c2, r: r2, .. } if c2.distance(c) <= SAME * (1.0 + r) && (r2 - r).abs() <= SAME * (1.0 + r)))
        .then_some((c, r))
}

/// The whole circle `p` ([`circle`]) of radius `r` instead, about the same centre.
pub fn with_radius(p: &[Corner], r: f64) -> Option<Vec<Corner>> {
    let (c, r0) = circle(p)?;
    (r > SAME).then(|| p.iter().map(|k| Corner { p: c + (k.p - c) * (r / r0), ..*k }).collect())
}

/// A corner `i` a fillet can round (both its sides straight, a true corner): the unit bisector
/// pointing inside its angle, half that angle, and how far along each side a fillet of radius 1
/// starts.
fn fillet_frame(p: &[Corner], i: usize) -> Option<(DVec2, f64, f64)> {
    let n = p.len();
    if n < 3 {
        return None;
    }
    let before = (i + n - 1) % n;
    if !p[before].straight() || !p[i].straight() {
        return None;
    }
    let u_in = (p[i].p - p[before].p).try_normalize()?;
    let u_out = (p[(i + 1) % n].p - p[i].p).try_normalize()?;
    let turn = angle_to(u_in, u_out);
    if turn.abs() < 1e-9 || turn.abs() > PI - 1e-9 {
        return None;
    }
    let half = (PI - turn.abs()) / 2.0;
    Some(((u_out - u_in).normalize(), half, (turn.abs() / 2.0).tan()))
}

/// Where the middle of a fillet of radius `r` on the corner `i` lies (on its bisector, inside);
/// none for a corner no fillet can round.
pub fn fillet_middle(p: &[Corner], i: usize, r: f64) -> Option<DVec2> {
    let (bisector, half, _) = fillet_frame(p, i)?;
    Some(p[i].p + bisector * r * (1.0 / half.sin() - 1.0))
}

/// The radius of the fillet on the corner `i` whose middle lies `d` from the corner along its
/// bisector (inside).
pub fn fillet_through(p: &[Corner], i: usize, d: f64) -> Option<f64> {
    let (_, half, _) = fillet_frame(p, i)?;
    Some(d.max(0.0) / (1.0 / half.sin() - 1.0))
}

/// The bisector pointing inside the corner `i`, for a corner a fillet can round.
pub fn fillet_bisector(p: &[Corner], i: usize) -> Option<DVec2> {
    fillet_frame(p, i).map(|f| f.0)
}

/// The largest fillet the corner `i` takes, its neighbours' fillets kept (what is left of each
/// side once the fillet at its other end has taken its share).
pub fn fillet_max(p: &[Corner], i: usize) -> Option<f64> {
    let n = p.len();
    let (_, _, tan_i) = fillet_frame(p, i)?;
    let used = |k: usize| fillet_frame(p, k).map_or(0.0, |(_, _, t)| t * p[k].fillet);
    let (before, next) = ((i + n - 1) % n, (i + 1) % n);
    let room_in = p[i].p.distance(p[before].p) - used(before);
    let room_out = p[i].p.distance(p[next].p) - used(next);
    Some((room_in.min(room_out) / tan_i).max(0.0))
}

/// `p` with a fillet of radius `r` on every corner between two straight sides (0: none), the
/// others left as they are.
pub fn with_fillets(p: &[Corner], r: f64) -> Vec<Corner> {
    let n = p.len();
    (0..n)
        .map(|i| {
            let c = p[i];
            let fillet = if r > SAME && c.straight() && p[(i + n - 1) % n].straight() { r } else { 0.0 };
            Corner { fillet, ..c }
        })
        .collect()
}

/// The outline `p` with its fillets made arcs: each corner between two straight sides that
/// carries a radius is rounded by an arc of it, tangent to both; a corner whose sides are too
/// short for it (the fillets at both ends of a side counted, in the outline's order) stays
/// sharp. The result carries no radius.
pub fn rounded(p: &[Corner]) -> Vec<Corner> {
    let p = cleaned(p);
    let n = p.len();
    let sharp = |c: &Corner| Corner { fillet: 0.0, ..*c };
    if n < 3 || p.iter().all(|c| c.fillet <= SAME) {
        return p.iter().map(sharp).collect();
    }
    let len = |i: usize| p[i].p.distance(p[(i + 1) % n].p);
    // Per corner: how far along each side the fillet starts, and its signed turn.
    let mut cut: Vec<Option<(f64, f64)>> = vec![None; n];
    for i in 0..n {
        let before = (i + n - 1) % n;
        let r = p[i].fillet;
        if r <= SAME || !p[before].straight() || !p[i].straight() {
            continue;
        }
        let u_in = (p[i].p - p[before].p).normalize();
        let u_out = (p[(i + 1) % n].p - p[i].p).normalize();
        let turn = u_in.perp_dot(u_out).atan2(u_in.dot(u_out));
        if turn.abs() < 1e-9 || turn.abs() > PI - 1e-9 {
            continue;
        }
        let t = r * (turn.abs() / 2.0).tan();
        let used_in = cut[before].map_or(0.0, |c| c.0);
        let used_out = if i == n - 1 { cut[0].map_or(0.0, |c| c.0) } else { 0.0 };
        if t <= len(before) - used_in + SAME && t <= len(i) - used_out + SAME {
            cut[i] = Some((t, turn));
        }
    }
    let mut out = Vec::with_capacity(2 * n);
    for i in 0..n {
        match cut[i] {
            Some((t, turn)) => {
                let before = (i + n - 1) % n;
                let u_in = (p[i].p - p[before].p).normalize();
                let u_out = (p[(i + 1) % n].p - p[i].p).normalize();
                out.push(Corner::arc(p[i].p - u_in * t, bulge_of(turn)));
                out.push(Corner::new(p[i].p + u_out * t));
            }
            None => out.push(sharp(&p[i])),
        }
    }
    cleaned(&out)
}

/// `p` with each arc tangent to two straight sides that are not parallel — a fillet, however it
/// was drawn — made the radius of the sharp corner where those sides meet (docs/tools/prism.md: a fillet
/// is a radius). [`rounded`] gives the same outline back.
pub fn recognized(p: &[Corner]) -> Vec<Corner> {
    let mut out = cleaned(p);
    'search: loop {
        let n = out.len();
        if n < 4 {
            return out;
        }
        for i in 0..n {
            let (before, next) = ((i + n - 1) % n, (i + 1) % n);
            if out[i].straight() || !out[before].straight() || !out[next].straight() {
                continue;
            }
            let Side::Arc { a, b, r, sweep, .. } = Side::new(out[i].p, out[next].p, out[i].bulge) else { continue };
            if sweep.abs() >= PI - 1e-9 {
                continue;
            }
            let (Some(u_in), Some(u_out)) = ((a - out[before].p).try_normalize(), (out[(i + 2) % n].p - b).try_normalize()) else {
                continue;
            };
            let (t_a, t_b) = Side::new(a, b, out[i].bulge).tangents();
            // Tangent at both ends: the sides run on along the arc's tangents.
            if t_a.dot(u_in) < 1.0 - 1e-9 || t_b.dot(u_out) < 1.0 - 1e-9 {
                continue;
            }
            let det = u_in.perp_dot(u_out);
            if det.abs() < 1e-9 {
                continue;
            }
            // Where the two sides meet: along the side in, beyond the arc's start.
            let q = a + u_in * ((b - a).perp_dot(u_out) / det);
            // The fillet of that corner gives the arc back.
            let t = r * (sweep.abs() / 2.0).tan();
            if (q - u_in * t).distance(a) > SAME * (1.0 + r) || (q + u_out * t).distance(b) > SAME * (1.0 + r) {
                continue;
            }
            out[i] = Corner::filleted(q, r);
            out.remove(next);
            continue 'search;
        }
        return out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn poly(ps: &[(f64, f64)]) -> Vec<Corner> {
        polygon(&ps.iter().map(|&(x, y)| v(x, y)).collect::<Vec<_>>())
    }

    fn fillet(p: &[Corner], r: f64) -> Vec<Corner> {
        rounded(&with_fillets(p, r))
    }

    fn same(a: &[Corner], b: &[Corner]) -> bool {
        let n = a.len();
        n == b.len()
            && (0..n).any(|k| (0..n).all(|i| a[(i + k) % n].p.distance(b[i].p) < 1e-9 && (a[(i + k) % n].bulge - b[i].bulge).abs() < 1e-9))
    }

    #[test]
    fn simple_or_not() {
        let square = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        assert!(is_simple(&square));
        assert!(is_simple(&reversed(&square)));
        assert!(signed_area2(&square) > 0.0 && signed_area2(&reversed(&square)) < 0.0);
        assert!(!is_simple(&poly(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)])), "a bow tie");
        assert!(is_simple(&poly(&[(0.0, 0.0), (20.0, 0.0), (20.0, 5.0), (5.0, 5.0), (5.0, 20.0), (0.0, 20.0)])), "an L");
        assert!(!is_simple(&poly(&[(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 0.0), (0.0, 10.0)])), "a corner on a side");
        assert!(!is_simple(&poly(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)])), "flat");
    }

    #[test]
    fn arcs_area_box_and_crossings() {
        // A disc of radius 5 as two half circles.
        let disc = [Corner::arc(v(-5.0, 0.0), -1.0), Corner::arc(v(5.0, 0.0), -1.0)];
        // Clockwise as given (each half turning right).
        assert!((signed_area2(&disc) + 2.0 * PI * 25.0).abs() < 1e-9, "{}", signed_area2(&disc));
        assert!(is_simple(&disc));
        let (lo, hi) = bounds(&disc).unwrap();
        assert!(lo.distance(v(-5.0, -5.0)) < 1e-9 && hi.distance(v(5.0, 5.0)) < 1e-9, "{lo} {hi}");
        // A square with its top bulging out (a half circle): area 100 + π·25/2.
        let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
        assert!((signed_area2(&tomb) / 2.0 - (100.0 + PI * 12.5)).abs() < 1e-9);
        assert!(is_simple(&tomb));
        assert!((bounds(&tomb).unwrap().1.y - 15.0).abs() < 1e-9);
        // The same top bulging in so far that it crosses the bottom.
        let crossed = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 2.0), -1.0), Corner::new(v(0.0, 2.0))];
        assert!(!is_simple(&crossed));
        // Reversed, the same outline.
        let back = reversed(&tomb);
        assert!((signed_area2(&back) + signed_area2(&tomb)).abs() < 1e-9);
        assert!(same(&reversed(&back), &tomb));
    }

    #[test]
    fn bulge_through_its_middle() {
        // A half circle from (0,0) to (10,0) through (5,-5): turning left (its middle on the right).
        assert!((bulge_through(v(0.0, 0.0), v(5.0, -5.0), v(10.0, 0.0)) - 1.0).abs() < 1e-12);
        assert!((bulge_through(v(0.0, 0.0), v(5.0, 5.0), v(10.0, 0.0)) + 1.0).abs() < 1e-12);
        let s = Side::new(v(0.0, 0.0), v(10.0, 0.0), 0.3);
        assert!((bulge_through(v(0.0, 0.0), s.at(0.5), v(10.0, 0.0)) - 0.3).abs() < 1e-12);
        assert_eq!(bulge_through(v(0.0, 0.0), v(5.0, 0.0), v(10.0, 0.0)), 0.0);
        let pts = s.points(0.1);
        assert!(pts.len() > 3 && pts[0] == v(0.0, 0.0) && pts[pts.len() - 1] == v(10.0, 0.0));
    }

    #[test]
    fn cleaned_drops_repeats_and_straight_corners() {
        let p = poly(&[(0.0, 0.0), (0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]);
        assert!(same(&cleaned(&p), &poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)])));
        let spike = poly(&[(0.0, 0.0), (10.0, 0.0), (5.0, 0.0), (5.0, 5.0)]);
        assert_eq!(cleaned(&spike).len(), 4);
        assert!(!is_simple(&cleaned(&spike)));
        // A corner between an arc and a line stays.
        let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
        assert_eq!(cleaned(&tomb).len(), 4);
    }

    #[test]
    fn offset_moves_the_sides_with_sharp_corners() {
        let square = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        assert!(same(&offset(&square, 2.0).unwrap(), &poly(&[(-2.0, -2.0), (12.0, -2.0), (12.0, 12.0), (-2.0, 12.0)])));
        assert!(same(&offset(&reversed(&square), -2.0).unwrap(), &poly(&[(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)])));
        let l = poly(&[(0.0, 0.0), (20.0, 0.0), (20.0, 5.0), (5.0, 5.0), (5.0, 20.0), (0.0, 20.0)]);
        assert!(offset(&l, 1.0).unwrap().iter().any(|c| c.p.distance(v(6.0, 6.0)) < 1e-9));
        assert_eq!(offset(&square, -5.0), Err(OffsetError::Empty));
        assert_eq!(offset(&square, -6.0), Err(OffsetError::Empty));
    }

    #[test]
    fn offset_takes_out_the_sides_it_turns_over() {
        // A tall trapezoid whose short top side vanishes inward (at 1.62): a triangle.
        let t = poly(&[(0.0, 0.0), (20.0, 0.0), (11.0, 18.0), (9.0, 18.0)]);
        assert_eq!(offset(&t, -1.0).unwrap().len(), 4);
        let o = offset(&t, -3.0).unwrap();
        assert_eq!(o.len(), 3, "{o:?}");
        assert!(signed_area2(&o) > 0.0);
        let u = poly(&[(0.0, 0.0), (30.0, 0.0), (30.0, 20.0), (20.0, 20.0), (20.0, 10.0), (10.0, 10.0), (10.0, 20.0), (0.0, 20.0)]);
        assert!(offset(&u, 2.0).is_ok());
        assert_eq!(offset(&u, 6.0), Err(OffsetError::Crossed));
        let h = poly(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 4.0),
            (20.0, 4.0),
            (20.0, 0.0),
            (30.0, 0.0),
            (30.0, 10.0),
            (20.0, 10.0),
            (20.0, 6.0),
            (10.0, 6.0),
            (10.0, 10.0),
            (0.0, 10.0),
        ]);
        assert!(offset(&h, -0.5).is_ok());
        assert!(offset(&h, -1.5).is_err());
    }

    #[test]
    fn offset_makes_arcs_concentric() {
        // A square with its top a half circle out (radius 5 about (5, 10)): outward by 1, the
        // arc's radius is 6, its ends where the moved sides meet it.
        let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
        let o = offset(&tomb, 1.0).unwrap();
        assert!(
            same(
                &o,
                &[Corner::new(v(-1.0, -1.0)), Corner::new(v(11.0, -1.0)), Corner::arc(v(11.0, 10.0), 1.0), Corner::new(v(-1.0, 10.0))]
            ),
            "{o:?}"
        );
        // Inward by 1: radius 4.
        let i = offset(&tomb, -1.0).unwrap();
        assert!(
            same(&i, &[Corner::new(v(1.0, 1.0)), Corner::new(v(9.0, 1.0)), Corner::arc(v(9.0, 10.0), 1.0), Corner::new(v(1.0, 10.0))]),
            "{i:?}"
        );
        // A disc: its radius moves.
        let disc = [Corner::arc(v(-5.0, 0.0), 1.0), Corner::arc(v(5.0, 0.0), 1.0)];
        let o = offset(&disc, 2.0).unwrap();
        assert!((bounds(&o).unwrap().1 - v(7.0, 7.0)).length() < 1e-9, "{o:?}");
        assert_eq!(offset(&disc, -5.0), Err(OffsetError::Empty));
        // A notch rounded inward (an arc turning right): outward it shrinks.
        let notch = [
            Corner::new(v(0.0, 0.0)),
            Corner::new(v(20.0, 0.0)),
            Corner::new(v(20.0, 10.0)),
            Corner::arc(v(15.0, 10.0), -1.0),
            Corner::new(v(5.0, 10.0)),
            Corner::new(v(0.0, 10.0)),
        ];
        assert!(is_simple(&notch));
        let o = offset(&notch, 1.0).unwrap();
        // The arc now of radius 4 about (10, 10), meeting the top moved to y = 11 at 10 ± √15.
        assert!(o.iter().any(|c| c.p.distance(v(10.0 + 15f64.sqrt(), 11.0)) < 1e-9 && c.bulge < -1.0), "{o:?}");
        assert!(is_simple(&o));
    }

    #[test]
    fn fillet_rounds_the_straight_corners() {
        let square = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        let f = fillet(&square, 2.0);
        assert_eq!(f.len(), 8);
        assert!(is_simple(&f));
        // Area: the square less four corners of (4 − π)·r².
        assert!((signed_area2(&f) / 2.0 - (100.0 - 4.0 * (4.0 - PI))).abs() < 1e-9, "{}", signed_area2(&f) / 2.0);
        assert!(f.iter().any(|c| c.p.distance(v(2.0, 0.0)) < 1e-9 && c.straight()));
        assert!(f.iter().any(|c| c.p.distance(v(8.0, 0.0)) < 1e-9 && (c.bulge - bulge_of(FRAC_PI_2)).abs() < 1e-12));
        // Radius 5: the sides used up, four quarter circles — a disc.
        let d = fillet(&square, 5.0);
        assert_eq!(d.len(), 4, "{d:?}");
        assert!((signed_area2(&d) / 2.0 - PI * 25.0).abs() < 1e-9);
        // Too big: the corners stay (the first takes what it can, the others not).
        let f = fillet(&square, 6.0);
        assert!(f.iter().filter(|c| !c.straight()).count() <= 2, "{f:?}");
        assert!(is_simple(&f));
        // Clockwise: the fillets turn right.
        let f = fillet(&reversed(&square), 2.0);
        assert!(f.iter().all(|c| c.bulge <= 0.0) && is_simple(&f));
        // A concave corner is rounded too.
        let l = poly(&[(0.0, 0.0), (20.0, 0.0), (20.0, 5.0), (5.0, 5.0), (5.0, 20.0), (0.0, 20.0)]);
        let f = fillet(&l, 1.0);
        assert_eq!(f.len(), 12);
        assert!(f.iter().any(|c| c.bulge < 0.0), "the inner corner turns right");
    }

    #[test]
    fn fillets_are_radii_on_the_corners() {
        let square = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        // One corner rounded, the others sharp.
        let mut one = square.clone();
        one[2].fillet = 3.0;
        let r = rounded(&one);
        assert_eq!(r.len(), 5);
        assert!(r.iter().all(|c| c.fillet == 0.0));
        assert!(r.iter().any(|c| c.p.distance(v(10.0, 7.0)) < 1e-9 && (c.bulge - bulge_of(FRAC_PI_2)).abs() < 1e-12));
        // Moving the sharp corner moves the fillet with it, still tangent, still radius 3.
        let mut wider = one.clone();
        wider[1].p.x = 20.0;
        wider[2].p.x = 20.0;
        let r = rounded(&wider);
        assert!(r.iter().any(|c| c.p.distance(v(20.0, 7.0)) < 1e-9 && !c.straight()));
        // Too short a side: sharp, and the radius kept for when there is room again.
        let mut narrow = one.clone();
        narrow[2].p.y = 2.0;
        narrow[3].p.y = 2.0;
        assert_eq!(rounded(&narrow).len(), 4);
        assert_eq!(narrow[2].fillet, 3.0);
        // A radius next to an arc is not applied.
        let tomb = [
            Corner::new(v(0.0, 0.0)),
            Corner::filleted(v(10.0, 0.0), 2.0),
            Corner::arc(v(10.0, 10.0), 1.0),
            Corner::filleted(v(0.0, 10.0), 2.0),
        ];
        let r = rounded(&tomb);
        assert_eq!(r.iter().filter(|c| !c.straight()).count(), 2, "{r:?}");
        // Reversed, each corner keeps its radius.
        let back = reversed(&one);
        assert_eq!(back.iter().find(|c| c.p == v(10.0, 10.0)).unwrap().fillet, 3.0);
        assert!((signed_area2(&rounded(&back)) + signed_area2(&rounded(&one))).abs() < 1e-9);
        // with_fillets skips corners next to an arc.
        let w = with_fillets(
            &[Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))],
            2.0,
        );
        assert_eq!(w.iter().map(|c| c.fillet).collect::<Vec<_>>(), vec![2.0, 2.0, 0.0, 0.0]);
    }

    #[test]
    fn tangent_arcs_are_recognized_as_fillets() {
        let square = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        let f = fillet(&square, 2.0);
        let back = recognized(&f);
        assert!(same(&back.iter().map(|c| Corner { fillet: 0.0, ..*c }).collect::<Vec<_>>(), &square), "{back:?}");
        assert!(back.iter().all(|c| (c.fillet - 2.0).abs() < 1e-9));
        assert!(same(&rounded(&back), &f));
        // Clockwise and concave, the same.
        let l = poly(&[(0.0, 0.0), (20.0, 0.0), (20.0, 5.0), (5.0, 5.0), (5.0, 20.0), (0.0, 20.0)]);
        let f = fillet(&reversed(&l), 1.0);
        let back = recognized(&f);
        assert_eq!(back.len(), 6);
        assert!(same(&rounded(&back), &f));
        // A half circle between parallel sides, an arc not tangent, a disc: left as they are.
        let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
        assert!(same(&recognized(&tomb), &tomb));
        let bent = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 0.2), Corner::new(v(0.0, 10.0))];
        assert!(same(&recognized(&bent), &bent));
        let disc = [Corner::arc(v(-5.0, 0.0), 1.0), Corner::arc(v(5.0, 0.0), 1.0)];
        assert!(same(&recognized(&disc), &disc));
    }

    #[test]
    fn a_fillet_is_saved_only_when_there_is_one() {
        let c = serde_json::to_string(&Corner::new(v(1.0, 2.0))).unwrap();
        assert!(!c.contains("fillet"), "{c}");
        let f: Corner = serde_json::from_str(&serde_json::to_string(&Corner::filleted(v(1.0, 2.0), 3.0)).unwrap()).unwrap();
        assert_eq!(f, Corner::filleted(v(1.0, 2.0), 3.0));
    }

    #[test]
    fn sides_keep_their_direction() {
        let rect = poly(&[(0.0, 0.0), (600.0, 0.0), (600.0, 300.0), (0.0, 300.0)]);
        // The far corner moved: a rectangle still, 700 × 350.
        let r = corner_moved(&rect, 2, v(700.0, 350.0)).unwrap();
        assert!(same(&r, &poly(&[(0.0, 0.0), (700.0, 0.0), (700.0, 350.0), (0.0, 350.0)])), "{r:?}");
        // The top side moved up 50 (its along part ignored): 600 × 350.
        let r = side_moved(&rect, 2, v(80.0, 50.0)).unwrap();
        assert!(same(&r, &poly(&[(0.0, 0.0), (600.0, 0.0), (600.0, 350.0), (0.0, 350.0)])), "{r:?}");
        // A trapezoid keeps its angles: its top right corner moved right, the slanted side
        // translated, the top extended, the bottom right corner sliding along the bottom.
        let trap = poly(&[(0.0, 0.0), (100.0, 0.0), (80.0, 50.0), (20.0, 50.0)]);
        let t = corner_moved(&trap, 2, v(90.0, 50.0)).unwrap();
        assert!(same(&t, &poly(&[(0.0, 0.0), (110.0, 0.0), (90.0, 50.0), (20.0, 50.0)])), "{t:?}");
        // A triangle: keeping its sides' directions would scale it, every corner moving; the
        // corner moves alone instead.
        let tri = poly(&[(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)]);
        let t = corner_moved(&tri, 1, v(110.0, 10.0)).unwrap();
        assert!(same(&t, &poly(&[(0.0, 0.0), (110.0, 10.0), (0.0, 100.0)])), "{t:?}");
        // Along its base, one corner stays: the directions are kept, the hypotenuse translated.
        let t = corner_moved(&tri, 1, v(120.0, 0.0)).unwrap();
        assert!(same(&t, &poly(&[(0.0, 0.0), (120.0, 0.0), (0.0, 120.0)])), "{t:?}");
        // Fillets are carried along.
        let mut round = rect.clone();
        round[2].fillet = 20.0;
        assert_eq!(corner_moved(&round, 2, v(700.0, 350.0)).unwrap()[2].fillet, 20.0);
        // Next to an arc: the arc's end follows the straight side.
        let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
        let t = side_moved(&tomb, 1, v(5.0, 0.0)).unwrap();
        assert!(t[1].p.distance(v(15.0, 0.0)) < 1e-9 && t[2].p.distance(v(15.0, 10.0)) < 1e-9, "{t:?}");
        assert_eq!(t[2].bulge, 1.0);
    }

    #[test]
    fn arcs_fitted_again_as_they_lay() {
        // A tomb: a half circle on top, tangent to both upright sides.
        let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
        // The right side moved out by 4: the half circle grows to radius 7, still tangent.
        let t = rearced(&tomb, &side_moved(&tomb, 1, v(4.0, 0.0)).unwrap());
        assert!(t[2].p.distance(v(14.0, 10.0)) < 1e-9 && t[3].p.distance(v(0.0, 10.0)) < 1e-9, "{t:?}");
        assert!((t[2].bulge - 1.0).abs() < 1e-12);
        // Its top right corner moved up alone (free): the other end follows up its side.
        let mut m = tomb.to_vec();
        m[2].p = v(10.0, 15.0);
        let t = rearced(&tomb, &m);
        assert!(t[3].p.distance(v(0.0, 15.0)) < 1e-9 && (t[2].bulge - 1.0).abs() < 1e-12, "{t:?}");
        assert!(is_simple(&t));

        // An arc tangent to the side before it only (an arc tangent, drawn): that side moved
        // across, the arc still leaves it tangent.
        let hook = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 0.3), Corner::new(v(0.0, 20.0))];
        let base_t = sides(&hook)[2].tangents().0;
        assert!(base_t.dot(v(0.0, 1.0)) < 1.0 - 1e-6, "not tangent to the right side");
        let hook = {
            // Make it tangent to the right side (going up) at (10, 10), ending at (0, 20).
            let mut h = hook.to_vec();
            h[2].bulge = bulge_of(2.0 * angle_to(v(0.0, 1.0), v(-10.0, 10.0)));
            h
        };
        assert!((sides(&hook)[2].tangents().0.dot(v(0.0, 1.0)) - 1.0).abs() < 1e-12);
        let t = rearced(&hook, &side_moved(&hook, 1, v(5.0, 0.0)).unwrap());
        assert!(t[2].p.distance(v(15.0, 10.0)) < 1e-9);
        assert!((sides(&t)[2].tangents().0.dot(v(0.0, 1.0)) - 1.0).abs() < 1e-9, "still tangent: {t:?}");
        assert!(sides(&t)[2].b().distance(v(0.0, 20.0)) < 1e-9);

        // A bent rail (tangent to nothing): its sagitta kept when the chord grows.
        let rail = [Corner::new(v(0.0, 0.0)), Corner::new(v(100.0, 0.0)), Corner::arc(v(100.0, 50.0), 0.2), Corner::new(v(0.0, 50.0))];
        let sag = |p: &[Corner]| sides(p)[2].at(0.5).distance((p[2].p + p[3].p) / 2.0);
        let t = rearced(&rail, &side_moved(&rail, 1, v(50.0, 0.0)).unwrap());
        assert!((sag(&t) - sag(&rail)).abs() < 1e-9, "{} {}", sag(&t), sag(&rail));

        // Nothing moved: nothing changes.
        assert!(same(&rearced(&rail, &rail), &rail));
    }

    #[test]
    fn a_whole_circle_has_a_radius() {
        let disc = [Corner::arc(v(5.0, 0.0), 1.0), Corner::arc(v(-5.0, 0.0), 1.0)];
        let (c, r) = circle(&disc).unwrap();
        assert!(c.length() < 1e-9 && (r - 5.0).abs() < 1e-9);
        let big = with_radius(&disc, 8.0).unwrap();
        assert!((circle(&big).unwrap().1 - 8.0).abs() < 1e-9);
        let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
        assert!(circle(&tomb).is_none());
        // Half circles of two circles: not one.
        assert!(circle(&[Corner::arc(v(5.0, 0.0), 1.0), Corner::arc(v(-5.0, 0.0), 0.5)]).is_none());
    }

    #[test]
    fn a_fillet_by_its_middle() {
        let square = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        // A right angle: the middle of an R2 fillet is 2(√2 − 1) in along the bisector.
        let m = fillet_middle(&square, 2, 2.0).unwrap();
        let d = 2.0 * (2f64.sqrt() - 1.0);
        assert!(m.distance(v(10.0, 10.0) - v(1.0, 1.0).normalize() * d) < 1e-12, "{m}");
        assert!((fillet_through(&square, 2, d).unwrap() - 2.0).abs() < 1e-12);
        // It lies on the arc rounded() makes.
        let mut one = square.clone();
        one[2].fillet = 2.0;
        let arc = sides(&rounded(&one)).into_iter().find(|s| matches!(s, Side::Arc { .. })).unwrap();
        assert!(arc.at(0.5).distance(m) < 1e-9);
        // The room: 10 alone, 5 with the neighbours at 5.
        assert!((fillet_max(&square, 2).unwrap() - 10.0).abs() < 1e-12);
        let mut all = square.clone();
        all[1].fillet = 5.0;
        all[3].fillet = 5.0;
        assert!((fillet_max(&all, 2).unwrap() - 5.0).abs() < 1e-12);
        // Not a corner a fillet rounds: next to an arc.
        let tomb = [Corner::new(v(0.0, 0.0)), Corner::new(v(10.0, 0.0)), Corner::arc(v(10.0, 10.0), 1.0), Corner::new(v(0.0, 10.0))];
        assert!(fillet_middle(&tomb, 2, 1.0).is_none() && fillet_middle(&tomb, 1, 1.0).is_some());
    }
}

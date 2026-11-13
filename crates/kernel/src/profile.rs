//! Planar profiles: the outline of a [`crate::Primitive::Prism`] (docs/ARCHITECTURE.md
//! §6 nonies), a closed loop of lines and arcs in its XY plane. Each [`Corner`] carries the
//! bulge of the side leaving it, as DXF's LWPOLYLINE: the sides' ends are shared, so the
//! outline cannot open. Here: the sides, the area, the clean-up, the checks the drawing tool
//! makes before the kernel does, the box, the offset, the fillets, and points along it for
//! the previews.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// How close (mm) two points are the same one, and how far off a line or an arc a point
/// still lies on it.
const SAME: f64 = 1e-6;
/// Below this bulge, a side is straight.
const FLAT: f64 = 1e-9;

/// A corner of an outline and the side leaving it: straight (`bulge` 0) or an arc of sweep
/// 4·atan(`bulge`), turning counter-clockwise when positive.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corner {
    pub p: DVec2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bulge: f64,
}

fn is_zero(b: &f64) -> bool {
    *b == 0.0
}

impl Corner {
    /// A corner whose side leaving it is straight.
    pub fn new(p: DVec2) -> Corner {
        Corner { p, bulge: 0.0 }
    }

    /// A corner whose side leaving it is an arc of `bulge`.
    pub fn arc(p: DVec2, bulge: f64) -> Corner {
        Corner { p, bulge }
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
    (0..n).map(|k| Corner { p: p[n - 1 - k].p, bulge: -p[(2 * n - 2 - k) % n].bulge }).collect()
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

/// The outline `p` with every corner between two straight sides rounded by an arc of radius
/// `r`, tangent to both; a corner whose sides are too short for it (the fillets at both ends of
/// a side counted, in the outline's order) stays sharp.
pub fn fillet(p: &[Corner], r: f64) -> Vec<Corner> {
    let p = cleaned(p);
    let n = p.len();
    if r <= SAME || n < 3 {
        return p;
    }
    let len = |i: usize| p[i].p.distance(p[(i + 1) % n].p);
    // Per corner: how far along each side the fillet starts, and its signed turn.
    let mut cut: Vec<Option<(f64, f64)>> = vec![None; n];
    for i in 0..n {
        let before = (i + n - 1) % n;
        if !p[before].straight() || !p[i].straight() {
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
            None => out.push(p[i]),
        }
    }
    cleaned(&out)
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
}

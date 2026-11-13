//! Planar profiles: the outline of a [`crate::Primitive::Prism`] (docs/ARCHITECTURE.md
//! §6 nonies), a closed polygon in its XY plane. The checks the drawing tool makes before
//! the kernel does, the clean-up both share, and the outline's offset.

use glam::DVec2;

/// How close (mm) two corners are the same one, and how far off the line through its
/// neighbours a corner still lies on it.
const SAME: f64 = 1e-6;

/// Twice the signed area of the closed polygon `p` (positive: counter-clockwise).
pub fn signed_area2(p: &[DVec2]) -> f64 {
    (0..p.len()).map(|i| p[i].perp_dot(p[(i + 1) % p.len()])).sum()
}

/// `p` with its repeated corners (consecutive, and the last on the first) and its corners
/// lying on the line through their neighbours dropped: the polygon's true corners.
pub fn cleaned(p: &[DVec2]) -> Vec<DVec2> {
    let mut out: Vec<DVec2> = Vec::with_capacity(p.len());
    for &q in p {
        if out.last().is_none_or(|l| l.distance(q) > SAME) {
            out.push(q);
        }
    }
    while out.len() > 1 && out[0].distance(out[out.len() - 1]) <= SAME {
        out.pop();
    }
    // Straight corners, until none is left (dropping one can straighten another).
    loop {
        let n = out.len();
        if n < 3 {
            return out;
        }
        let straight = (0..n).find(|&i| {
            let (a, b, c) = (out[(i + n - 1) % n], out[i], out[(i + 1) % n]);
            let len = a.distance(c);
            // Off the line `a`–`c` by less than SAME, and between its ends.
            len > SAME && ((c - a).perp_dot(b - a) / len).abs() <= SAME && (b - a).dot(c - a) > 0.0 && (b - c).dot(a - c) > 0.0
        });
        match straight {
            Some(i) => {
                out.remove(i);
            }
            None => return out,
        }
    }
}

/// Is `p` a simple polygon — at least three corners, an area, no two of its sides meeting but
/// neighbours at their shared corner?
pub fn is_simple(p: &[DVec2]) -> bool {
    let n = p.len();
    if n < 3 || signed_area2(p).abs() <= SAME * SAME {
        return false;
    }
    let side = |i: usize| (p[i], p[(i + 1) % n]);
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = side(i);
            let (c, d) = side(j);
            let neighbours = j == i + 1 || (i == 0 && j == n - 1);
            if neighbours {
                // Neighbours meet at their shared corner only: not folded back over each other.
                let (u, v) = if j == i + 1 { (a - b, d - b) } else { (b - a, c - a) };
                if u.perp_dot(v).abs() <= SAME * u.length() * v.length() && u.dot(v) > 0.0 {
                    return false;
                }
                continue;
            }
            if segments_meet(a, b, c, d) {
                return false;
            }
        }
    }
    true
}

/// Do the segments `a`–`b` and `c`–`d` meet (touching counts)?
fn segments_meet(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let side = |p: DVec2, q: DVec2, r: DVec2| {
        let s = (q - p).perp_dot(r - p);
        if s.abs() <= SAME * (q - p).length().max(1.0) { 0 } else { s.signum() as i32 }
    };
    let on = |p: DVec2, q: DVec2, r: DVec2| {
        r.x >= p.x.min(q.x) - SAME && r.x <= p.x.max(q.x) + SAME && r.y >= p.y.min(q.y) - SAME && r.y <= p.y.max(q.y) + SAME
    };
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if d1 * d2 < 0 && d3 * d4 < 0 {
        return true;
    }
    (d1 == 0 && on(c, d, a)) || (d2 == 0 && on(c, d, b)) || (d3 == 0 && on(a, b, c)) || (d4 == 0 && on(a, b, d))
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

/// The simple polygon `p` offset by `d` (positive: outward), its corners sharp, as SolidCraft's
/// sketch offset (`crates/sketch/src/offset.rs`): each side moved along its outward normal, the
/// corners where neighbouring sides meet. A side the offset turns over (inward, a short side
/// between two converging ones; outward, a notch's bottom) is taken out and its neighbours meet
/// instead, as Clipper's miter offset does (OpenCutList's): a trapezoid can become a triangle.
/// Counter-clockwise; refused when the result crosses itself or nothing is left.
pub fn offset(p: &[DVec2], d: f64) -> Result<Vec<DVec2>, OffsetError> {
    let mut p = cleaned(p);
    if !is_simple(&p) {
        return Err(OffsetError::Crossed);
    }
    if signed_area2(&p) < 0.0 {
        p.reverse();
    }
    if d == 0.0 {
        return Ok(p);
    }
    // The sides' lines, moved: a point and the unit direction (outward is on the right of a
    // counter-clockwise outline).
    let n = p.len();
    let mut lines: Vec<(DVec2, DVec2)> = (0..n)
        .map(|i| {
            let u = (p[(i + 1) % n] - p[i]).normalize();
            (p[i] + DVec2::new(u.y, -u.x) * d, u)
        })
        .collect();
    loop {
        let m = lines.len();
        if m < 3 {
            return Err(OffsetError::Empty);
        }
        // Each corner: where the side before meets the side after.
        let mut corners = Vec::with_capacity(m);
        for i in 0..m {
            let (a, u) = lines[(i + m - 1) % m];
            let (b, v) = lines[i];
            let det = u.perp_dot(v);
            // Neighbours made parallel by a side taken out: inward, the outline has collapsed;
            // outward, a notch has closed.
            if det.abs() <= 1e-12 {
                return Err(if d < 0.0 { OffsetError::Empty } else { OffsetError::Crossed });
            }
            corners.push(a + u * ((b - a).perp_dot(v) / det));
        }
        // The side most turned over, if any: taken out, and the corners found again.
        let length = |i: usize| (corners[(i + 1) % m] - corners[i]).dot(lines[i].1);
        match (0..m).filter(|&i| length(i) < -SAME).min_by(|&a, &b| length(a).total_cmp(&length(b))) {
            Some(i) => {
                lines.remove(i);
            }
            None => {
                let out = cleaned(&corners);
                if out.len() < 3 || signed_area2(&out) <= SAME * SAME {
                    return Err(OffsetError::Empty);
                }
                return if is_simple(&out) { Ok(out) } else { Err(OffsetError::Crossed) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn simple_or_not() {
        let square = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        assert!(is_simple(&square));
        let mut cw = square;
        cw.reverse();
        assert!(is_simple(&cw));
        assert!(signed_area2(&square) > 0.0 && signed_area2(&cw) < 0.0);
        // A bow tie crosses itself.
        assert!(!is_simple(&[v(0.0, 0.0), v(10.0, 10.0), v(10.0, 0.0), v(0.0, 10.0)]));
        // An L, concave, is simple.
        assert!(is_simple(&[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 5.0), v(5.0, 5.0), v(5.0, 20.0), v(0.0, 20.0)]));
        // A corner touching another side.
        assert!(!is_simple(&[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(10.0, 0.0), v(0.0, 10.0)]));
        // Flat.
        assert!(!is_simple(&[v(0.0, 0.0), v(10.0, 0.0), v(20.0, 0.0)]));
    }

    #[test]
    fn cleaned_drops_repeats_and_straight_corners() {
        let p = [v(0.0, 0.0), v(0.0, 0.0), v(5.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0), v(0.0, 0.0)];
        assert_eq!(cleaned(&p), vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)]);
        // A spike folded back is not straight: kept (and refused as not simple).
        let spike = [v(0.0, 0.0), v(10.0, 0.0), v(5.0, 0.0), v(5.0, 5.0)];
        assert_eq!(cleaned(&spike).len(), 4);
        assert!(!is_simple(&cleaned(&spike)));
    }

    fn same(a: &[DVec2], b: &[DVec2]) -> bool {
        a.len() == b.len() && (0..a.len()).any(|k| (0..a.len()).all(|i| a[(i + k) % a.len()].distance(b[i]) < 1e-9))
    }

    #[test]
    fn offset_moves_the_sides_with_sharp_corners() {
        let square = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        assert!(same(&offset(&square, 2.0).unwrap(), &[v(-2.0, -2.0), v(12.0, -2.0), v(12.0, 12.0), v(-2.0, 12.0)]));
        // Either way round: the same outline, counter-clockwise.
        let mut cw = square;
        cw.reverse();
        assert!(same(&offset(&cw, -2.0).unwrap(), &[v(2.0, 2.0), v(8.0, 2.0), v(8.0, 8.0), v(2.0, 8.0)]));
        // An L, outward: the inner corner moves in.
        let l = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 5.0), v(5.0, 5.0), v(5.0, 20.0), v(0.0, 20.0)];
        let o = offset(&l, 1.0).unwrap();
        assert!(o.iter().any(|p| p.distance(v(6.0, 6.0)) < 1e-9), "{o:?}");
        // Past the middle: nothing left.
        assert_eq!(offset(&square, -5.0), Err(OffsetError::Empty));
        assert_eq!(offset(&square, -6.0), Err(OffsetError::Empty));
    }

    #[test]
    fn offset_takes_out_the_sides_it_turns_over() {
        // A tall trapezoid whose short top side vanishes inward (at 1.62): a triangle.
        let t = [v(0.0, 0.0), v(20.0, 0.0), v(11.0, 18.0), v(9.0, 18.0)];
        assert_eq!(offset(&t, -1.0).unwrap().len(), 4);
        let o = offset(&t, -3.0).unwrap();
        assert_eq!(o.len(), 3, "{o:?}");
        assert!(signed_area2(&o) > 0.0);
        // A U, outward past its notch's half width: the notch closes, refused.
        let u = [v(0.0, 0.0), v(30.0, 0.0), v(30.0, 20.0), v(20.0, 20.0), v(20.0, 10.0), v(10.0, 10.0), v(10.0, 20.0), v(0.0, 20.0)];
        assert!(offset(&u, 2.0).is_ok());
        assert_eq!(offset(&u, 6.0), Err(OffsetError::Crossed));
        // A dumbbell pinched inward past its neck: refused.
        let h = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 4.0), v(20.0, 4.0), v(20.0, 0.0), v(30.0, 0.0), v(30.0, 10.0), v(20.0, 10.0), v(20.0, 6.0), v(10.0, 6.0), v(10.0, 10.0), v(0.0, 10.0)];
        assert!(offset(&h, -0.5).is_ok());
        assert!(offset(&h, -1.5).is_err());
    }
}

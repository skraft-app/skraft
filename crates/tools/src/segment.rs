//! A segment on a plane (the screen, a sheet): the point of it nearest a point, whether it meets
//! a rectangle.

use glam::DVec2;

/// Below this squared length a segment is a point (a chord seen end on).
const POINT: f64 = 1e-12;

/// The point of `a`–`b` nearest `p`: its parameter (0 at `a`, 1 at `b`), and its distance to `p`.
pub fn nearest(p: DVec2, a: DVec2, b: DVec2) -> (f64, f64) {
    let ab = b - a;
    let length2 = ab.length_squared();
    let t = if length2 < POINT { 0.0 } else { ((p - a).dot(ab) / length2).clamp(0.0, 1.0) };
    (t, p.distance(a + ab * t))
}

/// Does `a`–`b` meet the rectangle (Liang–Barsky clipping)?
pub fn meets_rect(a: DVec2, b: DVec2, (lo, hi): (DVec2, DVec2)) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for k in 0..2 {
        for (p, q) in [(-d[k], a[k] - lo[k]), (d[k], hi[k] - a[k])] {
            if p == 0.0 {
                if q < 0.0 {
                    return false;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
                if t0 > t1 {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_point_stays_on_the_segment() {
        let (a, b) = (DVec2::ZERO, DVec2::new(10.0, 0.0));
        assert_eq!(nearest(DVec2::new(4.0, 3.0), a, b), (0.4, 3.0));
        assert_eq!(nearest(DVec2::new(-3.0, 4.0), a, b), (0.0, 5.0));
        assert_eq!(nearest(DVec2::new(13.0, 4.0), a, b), (1.0, 5.0));
    }

    #[test]
    fn a_segment_seen_end_on_is_its_first_point() {
        let a = DVec2::new(1.0, 1.0);
        assert_eq!(nearest(DVec2::new(4.0, 5.0), a, a + DVec2::splat(1e-9)).0, 0.0);
        assert_eq!(nearest(DVec2::new(4.0, 5.0), a, a), (0.0, 5.0));
    }

    #[test]
    fn a_segment_meets_a_rectangle_it_enters_or_crosses() {
        let rect = (DVec2::ZERO, DVec2::splat(10.0));
        assert!(meets_rect(DVec2::splat(2.0), DVec2::splat(3.0), rect), "inside");
        assert!(meets_rect(DVec2::new(-5.0, 5.0), DVec2::new(15.0, 5.0), rect), "across");
        assert!(meets_rect(DVec2::new(5.0, 5.0), DVec2::new(5.0, 20.0), rect), "out of it");
        assert!(!meets_rect(DVec2::new(-5.0, 12.0), DVec2::new(15.0, 12.0), rect), "above");
        assert!(!meets_rect(DVec2::new(-4.0, 8.0), DVec2::new(3.0, 15.0), rect), "past a corner");
    }
}

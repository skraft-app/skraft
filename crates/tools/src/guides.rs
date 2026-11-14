//! Drawing guides (docs/ARCHITECTURE.md §6 quinquies): an infinite line drawn across the whole
//! view (to its vanishing point in perspective), a circle as a polyline. Both are handed to
//! the renderer as [`Stroke`]s it dashes on screen, dashes of [`DASH_PX`] anchored on the
//! guide (a line's origin, a circle's first point), so they hold still as the view pans.

use std::f64::consts::TAU;

use glam::{DVec2, DVec3};
use solidsketch_model::GuideShape;

use crate::camera::Camera;

/// A dash, and the gap after it, in pixels.
pub const DASH_PX: f64 = 6.0;
/// Chords of a guide circle.
const CIRCLE_SEGMENTS: usize = 128;
/// How far past the view's sides a line is drawn, in pixels: its ends stay out of sight.
const MARGIN_PX: f64 = 4.0;

/// A straight stretch of a guide (world), dashed on screen by the renderer: the dash pattern
/// (a dash of [`DASH_PX`], a gap as long) is at `phase` pixels at `a`, running towards `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub a: DVec3,
    pub b: DVec3,
    /// In `0..2·DASH_PX`.
    pub phase: f64,
}

/// The guide `shape` (world) as the renderer dashes it.
pub fn strokes(camera: &Camera, size: DVec2, shape: &GuideShape) -> Vec<Stroke> {
    let proj = camera.projector(size);
    let period = 2.0 * DASH_PX;
    let mut out = Vec::new();
    for run in runs(camera, size, shape) {
        let screen: Vec<DVec2> = run.iter().filter_map(|p| proj.project(*p)).collect();
        if screen.len() != run.len() {
            continue;
        }
        // A line's pattern is anchored on its origin, where that is in front; a circle's (and
        // a line's otherwise) on its first point drawn.
        let mut phase = match *shape {
            GuideShape::Line { origin, .. } if in_front(camera, origin) => proj.project(origin).map_or(0.0, |o| {
                let u = (screen[1] - screen[0]).normalize_or_zero();
                (screen[0] - o).dot(u)
            }),
            _ => 0.0,
        };
        for (w, s) in run.windows(2).zip(screen.windows(2)) {
            out.push(Stroke { a: w[0], b: w[1], phase: phase.rem_euclid(period) });
            phase += s[0].distance(s[1]);
        }
    }
    out
}

/// The part of the line `origin + t·dir`, `t` in `range` (ends may be infinite), a `size`
/// viewport shows, kept off the eye as guides are (see [`Camera::visible_segment`]).
pub fn visible_line(camera: &Camera, size: DVec2, origin: DVec3, dir: DVec3, range: (f64, f64)) -> Option<(DVec3, DVec3)> {
    camera.visible_segment(size, origin, dir, range, near(camera), MARGIN_PX)
}

/// The guide as drawn: runs of points (world) in front of the eye, a line across the view.
fn runs(camera: &Camera, size: DVec2, shape: &GuideShape) -> Vec<Vec<DVec3>> {
    match *shape {
        GuideShape::Line { origin, dir } => {
            visible_line(camera, size, origin, dir, (f64::NEG_INFINITY, f64::INFINITY)).map(|(a, b)| vec![a, b]).into_iter().collect()
        }
        GuideShape::Circle { center, normal, radius } => {
            let points = circle(center, normal, radius);
            // Cut where it passes behind the eye: each run in front on its own.
            let mut runs = Vec::new();
            let mut run: Vec<DVec3> = Vec::new();
            for w in points.windows(2) {
                match clip_in_front(camera, w[0], w[1]) {
                    Some((a, b)) if run.last().is_some_and(|l| (*l - a).length() <= 1e-9) => run.push(b),
                    Some((a, b)) => runs.push(std::mem::replace(&mut run, vec![a, b])),
                    None => runs.push(std::mem::take(&mut run)),
                }
            }
            runs.push(run);
            runs.retain(|r| r.len() >= 2);
            runs
        }
    }
}

/// The guide as the screen sees it: runs of points (world, pixels).
fn screen_runs(camera: &Camera, size: DVec2, shape: &GuideShape) -> Vec<Vec<(DVec3, DVec2)>> {
    let proj = camera.projector(size);
    runs(camera, size, shape)
        .into_iter()
        .map(|r| r.iter().filter_map(|p| proj.project(*p).map(|s| (*p, s))).collect::<Vec<_>>())
        .filter(|r| r.len() >= 2)
        .collect()
}

/// How far the guide passes from the pixel `px` on screen, and its point there (world).
pub fn distance(camera: &Camera, size: DVec2, shape: &GuideShape, px: DVec2) -> Option<(f64, DVec3)> {
    let mut best: Option<(f64, DVec3)> = None;
    for run in screen_runs(camera, size, shape) {
        for w in run.windows(2) {
            let ((a, sa), (b, sb)) = (w[0], w[1]);
            let d = sb - sa;
            let t = if d.length_squared() < 1e-12 { 0.0 } else { ((px - sa).dot(d) / d.length_squared()).clamp(0.0, 1.0) };
            let dist = (sa + d * t).distance(px);
            if best.is_none_or(|b| dist < b.0) {
                best = Some((dist, a.lerp(b, t)));
            }
        }
    }
    // A line: its point nearest the cursor's ray, exactly (the chord is no straight measure of
    // depth in perspective).
    if let (Some((dist, _)), GuideShape::Line { origin, dir }) = (best, shape) {
        best = Some((dist, camera.ray(px, size).closest_on_line(*origin, *dir)));
    }
    best
}

/// Does the guide meet the rectangle `lo`–`hi` (pixels), or lie inside it entirely? A line,
/// infinite, is never inside one.
pub fn in_rect(camera: &Camera, size: DVec2, shape: &GuideShape, rect: (DVec2, DVec2)) -> (bool, bool) {
    let runs = screen_runs(camera, size, shape);
    let inside = |p: DVec2| p.cmpge(rect.0).all() && p.cmple(rect.1).all();
    let finite = matches!(shape, GuideShape::Circle { .. });
    let all = finite && !runs.is_empty() && runs.iter().flatten().all(|(_, s)| inside(*s));
    let any = runs.iter().any(|r| r.windows(2).any(|w| segment_meets_rect(w[0].1, w[1].1, rect)));
    (all, any)
}

/// Does the segment `a`–`b` meet the rectangle (Liang–Barsky clipping)?
fn segment_meets_rect(a: DVec2, b: DVec2, (lo, hi): (DVec2, DVec2)) -> bool {
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

/// The points of a circle, the first repeated last.
pub fn circle(center: DVec3, normal: DVec3, radius: f64) -> Vec<DVec3> {
    let u = normal.any_orthonormal_vector();
    let v = normal.cross(u);
    (0..=CIRCLE_SEGMENTS)
        .map(|k| {
            let a = TAU * k as f64 / CIRCLE_SEGMENTS as f64;
            center + (u * a.cos() + v * a.sin()) * radius
        })
        .collect()
}

/// How far in front of a perspective eye guides start (a fraction of its focus distance):
/// dashes nearer would be specks.
fn near(camera: &Camera) -> f64 {
    camera.focus * 0.02
}

/// Is `p` where guides are drawn: past [`near`] in perspective, anywhere in parallel?
fn in_front(camera: &Camera, p: DVec3) -> bool {
    camera.is_parallel() || (p - camera.eye).dot(camera.forward()) >= near(camera)
}

/// The part of the segment `a`–`b` past [`near`] in perspective; all of it in parallel.
fn clip_in_front(camera: &Camera, a: DVec3, b: DVec3) -> Option<(DVec3, DVec3)> {
    if camera.is_parallel() {
        return Some((a, b));
    }
    let f = camera.forward();
    let (da, db) = ((a - camera.eye).dot(f) - near(camera), (b - camera.eye).dot(f) - near(camera));
    match (da >= 0.0, db >= 0.0) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        (true, false) => Some((a, a + (b - a) * (da / (da - db)))),
        (false, true) => Some((a + (b - a) * (da / (da - db)), b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::camera::{StandardView, ViewArea};

    fn top() -> (Camera, DVec2) {
        let size = DVec2::new(1000.0, 1000.0);
        let mut c = Camera::default();
        c.set_parallel(true);
        c.set_standard_view(
            StandardView::Top,
            glam::DMat3::IDENTITY,
            Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))),
            ViewArea::full(size),
        );
        (c, size)
    }

    fn on_screen(c: &Camera, size: DVec2, p: DVec3) -> DVec2 {
        c.project(p, size).unwrap()
    }

    #[test]
    fn a_line_crosses_the_whole_view() {
        let (c, size) = top();
        let line = GuideShape::Line { origin: DVec3::new(0.0, 50.0, 0.0), dir: DVec3::X };
        let s = strokes(&c, size, &line);
        assert_eq!(s.len(), 1);
        let (a, b) = (on_screen(&c, size, s[0].a), on_screen(&c, size, s[0].b));
        // On the line, from just past one side of the view to just past the other.
        assert!((s[0].a.y - 50.0).abs() < 1e-9 && (s[0].b.y - 50.0).abs() < 1e-9);
        let (l, r) = (a.x.min(b.x), a.x.max(b.x));
        assert!((l + MARGIN_PX).abs() < 1e-3 && (r - size.x - MARGIN_PX).abs() < 1e-3, "{l} {r}");
        // The pattern is anchored on the origin: a dash starts there.
        let o = on_screen(&c, size, DVec3::new(0.0, 50.0, 0.0));
        let at_origin = s[0].phase + a.distance(o);
        assert!(at_origin.rem_euclid(2.0 * DASH_PX).min(2.0 * DASH_PX - at_origin.rem_euclid(2.0 * DASH_PX)) < 1e-6, "{at_origin}");
    }

    #[test]
    fn a_line_holds_its_dashes_as_the_view_pans() {
        let (mut c, size) = top();
        let line = GuideShape::Line { origin: DVec3::new(0.0, 50.0, 0.0), dir: DVec3::X };
        let dash_at = |c: &Camera, x: f64| {
            let s = strokes(c, size, &line)[0];
            let a = on_screen(c, size, s.a);
            let p = on_screen(c, size, DVec3::new(x, 50.0, 0.0));
            let u = (on_screen(c, size, s.b) - a).normalize();
            (s.phase + (p - a).dot(u)).rem_euclid(2.0 * DASH_PX)
        };
        let before = dash_at(&c, 123.0);
        c.eye += DVec3::new(37.0, 0.0, 0.0);
        let after = dash_at(&c, 123.0);
        assert!((before - after).abs() < 1e-6, "{before} {after}");
    }

    #[test]
    fn a_receding_line_runs_to_its_vanishing_point() {
        let size = DVec2::new(1200.0, 800.0);
        let c = Camera::default();
        // Along the view, off to the side of the eye.
        let origin = c.eye + c.forward() * c.focus + c.right() * 100.0;
        let line = GuideShape::Line { origin, dir: c.forward() };
        let s = strokes(&c, size, &line);
        assert_eq!(s.len(), 1);
        let vanishing = on_screen(&c, size, c.eye + c.forward() * 1e3 * c.focus);
        let far = [s[0].a, s[0].b].into_iter().map(|p| on_screen(&c, size, p)).map(|p| p.distance(vanishing)).fold(f64::MAX, f64::min);
        let centre = size / 2.0;
        assert!(vanishing.distance(centre) < 1e-6);
        assert!(far < 1.0, "{far}");
        // The near end is off the screen.
        let near_end = [s[0].a, s[0].b]
            .into_iter()
            .map(|p| on_screen(&c, size, p))
            .max_by(|p, q| p.distance(centre).total_cmp(&q.distance(centre)))
            .unwrap();
        assert!(near_end.x < 0.0 || near_end.x > size.x || near_end.y < 0.0 || near_end.y > size.y, "{near_end}");
    }

    #[test]
    fn a_line_out_of_view_is_not_drawn() {
        let (c, size) = top();
        let line = GuideShape::Line { origin: DVec3::new(0.0, 1e6, 0.0), dir: DVec3::X };
        assert!(strokes(&c, size, &line).is_empty());
        // Seen end on, in parallel (the top view stops short of the zenith).
        let line = GuideShape::Line { origin: DVec3::ZERO, dir: DVec3::Z };
        assert!(strokes(&c, size, &line).is_empty());
    }

    #[test]
    fn a_guide_is_picked_near_it_on_screen() {
        let (c, size) = top();
        let line = GuideShape::Line { origin: DVec3::new(0.0, 50.0, 0.0), dir: DVec3::X };
        let px = c.project(DVec3::new(100.0, 56.0, 0.0), size).unwrap();
        let (d, p) = distance(&c, size, &line, px).unwrap();
        let per_px = c.world_per_pixel(size, DVec3::ZERO);
        assert!((d - 6.0 / per_px).abs() < 1e-6, "{d}");
        assert!((p - DVec3::new(100.0, 50.0, 0.0)).length() < 1e-6, "{p}");
        // Far from the model, anywhere on screen.
        let far = c.project(DVec3::new(-990.0, 50.0, 0.0), size).unwrap();
        assert!(distance(&c, size, &line, far).unwrap().0 < 1e-6);
        // A circle inside a rectangle; a line only crosses one.
        let circle = GuideShape::Circle { center: DVec3::ZERO, normal: DVec3::Z, radius: 100.0 };
        let r = |a: DVec3, b: DVec3| {
            let (a, b) = (c.project(a, size).unwrap(), c.project(b, size).unwrap());
            (a.min(b), a.max(b))
        };
        assert_eq!(in_rect(&c, size, &circle, r(DVec3::splat(-150.0), DVec3::splat(150.0))), (true, true));
        assert_eq!(in_rect(&c, size, &circle, r(DVec3::new(50.0, -10.0, 0.0), DVec3::new(150.0, 10.0, 0.0))), (false, true));
        assert_eq!(in_rect(&c, size, &line, r(DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 10.0, 0.0))), (false, false));
        assert_eq!(in_rect(&c, size, &line, r(DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 100.0, 0.0))), (false, true));
        // Not even in one as large as the view.
        assert_eq!(in_rect(&c, size, &line, (DVec2::splat(-10.0), size + 10.0)), (false, true));
    }

    #[test]
    fn a_circle_goes_round() {
        let (c, size) = top();
        let circle = GuideShape::Circle { center: DVec3::new(10.0, 20.0, 5.0), normal: DVec3::Z, radius: 300.0 };
        let s = strokes(&c, size, &circle);
        assert_eq!(s.len(), CIRCLE_SEGMENTS);
        assert!(s.iter().all(|s| ((s.a - DVec3::new(10.0, 20.0, 5.0)).length() - 300.0).abs() < 1.0));
        // The pattern runs on from chord to chord.
        let w = (on_screen(&c, size, s[0].b) - on_screen(&c, size, s[0].a)).length();
        assert!((s[1].phase - (s[0].phase + w).rem_euclid(2.0 * DASH_PX)).abs() < 1e-6);
    }
}

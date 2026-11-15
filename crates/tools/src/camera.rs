//! The camera: SketchUp navigation (the frame of the last standard view keeps its Z up),
//! perspective or parallel, in `f64`.
//!
//! It gives the renderer a `f64` view and a **reversed-depth** projection (ARCHITECTURE.md
//! §7 bis), and the tools a picking [`Ray`] per pixel.

use glam::{DMat3, DMat4, DVec2, DVec3, Mat4};

/// How close to its vanishing point an infinite line is drawn, in pixels.
pub const VANISHING_PX: f64 = 0.25;
/// How far from the view's direction (radians) a line is still seen end on, in parallel: a
/// point, not drawn. The standard views stop 0.01° short of the zenith ([`MAX_PITCH`]).
const END_ON: f64 = 1e-3;

/// A half-line from the camera through a pixel. `t_min` is where hits start counting (minus
/// infinity in parallel projection, where the scene may lie behind the eye plane).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: DVec3,
    /// Unit length.
    pub dir: DVec3,
    pub t_min: f64,
}

impl Ray {
    pub fn at(&self, t: f64) -> DVec3 {
        self.origin + self.dir * t
    }

    /// Where the ray meets the plane through `p` with normal `n` (`None` if parallel or
    /// before `t_min`).
    pub fn hit_plane(&self, p: DVec3, n: DVec3) -> Option<DVec3> {
        let d = self.dir.dot(n);
        if d.abs() < 1e-12 {
            return None;
        }
        let t = (p - self.origin).dot(n) / d;
        (t >= self.t_min).then(|| self.at(t))
    }

    /// The point of the line `p + s·dir` closest to this ray.
    pub fn closest_on_line(&self, p: DVec3, dir: DVec3) -> DVec3 {
        let dir = dir.normalize();
        let w = p - self.origin;
        let b = self.dir.dot(dir);
        let denom = 1.0 - b * b;
        if denom < 1e-12 {
            return p;
        }
        let s = (b * self.dir.dot(w) - dir.dot(w)) / denom;
        p + dir * s
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// Vertical field of view, radians.
    Perspective { fov_y: f64 },
    /// Visible height of the view, mm.
    Parallel { height: f64 },
}

/// A standard view (SketchUp's Camera > Standard Views).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandardView {
    Top,
    Front,
    Right,
    Back,
    Left,
    Iso,
}

/// Steepest pitch: looking straight down would lose "up".
const MAX_PITCH: f64 = 89.99 * std::f64::consts::PI / 180.0;
const DEFAULT_FOV: f64 = 35.0 * std::f64::consts::PI / 180.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    pub eye: DVec3,
    /// Heading of the forward direction around the frame's +Z (0 = its +X), radians.
    pub yaw: f64,
    /// Elevation of the forward direction above the frame's XY plane (positive looks up),
    /// radians.
    pub pitch: f64,
    pub projection: Projection,
    /// Distance to what the camera looks at (orbit pivot, zoom target): sets the near plane.
    pub focus: f64,
    /// The frame `yaw` and `pitch` are measured in, to world (orthonormal, maybe mirrored): the
    /// frame of the last standard view. Its Z stays up on screen, as the orbit turns about it.
    pub frame: DMat3,
}

/// The viewport, and the part of it that panels leave visible (pixels, origin top left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewArea {
    pub size: DVec2,
    pub free_min: DVec2,
    pub free_max: DVec2,
}

impl ViewArea {
    /// Nothing covers the viewport.
    pub fn full(size: DVec2) -> Self {
        ViewArea { size, free_min: DVec2::ZERO, free_max: size }
    }
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Camera {
            eye: DVec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            projection: Projection::Perspective { fov_y: DEFAULT_FOV },
            focus: 1.0,
            frame: DMat3::IDENTITY,
        };
        c.set_standard_view(
            StandardView::Iso,
            DMat3::IDENTITY,
            Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))),
            ViewArea::full(DVec2::new(1200.0, 800.0)),
        );
        c
    }
}

/// The forward direction of a heading `yaw` around +Z (0 = +X) and an elevation `pitch`.
fn level_forward(yaw: f64, pitch: f64) -> DVec3 {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    DVec3::new(cp * cy, cp * sy, sp)
}

/// The right direction of a heading `yaw`: horizontal.
fn level_right(yaw: f64) -> DVec3 {
    let (sy, cy) = yaw.sin_cos();
    DVec3::new(sy, -cy, 0.0)
}

/// A frame's rotation part made orthonormal, keeping its Z, then its X, and its handedness (a
/// mirrored frame stays left-handed). `None` when degenerate.
pub fn orthonormal_frame(m: DMat3) -> Option<DMat3> {
    let z = m.z_axis.try_normalize()?;
    let x = (m.x_axis - z * m.x_axis.dot(z)).try_normalize()?;
    let y = z.cross(x) * if m.determinant() < 0.0 { -1.0 } else { 1.0 };
    Some(DMat3::from_cols(x, y, z))
}

impl Camera {
    pub fn forward(&self) -> DVec3 {
        self.frame * level_forward(self.yaw, self.pitch)
    }

    pub fn right(&self) -> DVec3 {
        self.forward().cross(self.up())
    }

    pub fn up(&self) -> DVec3 {
        let f = level_forward(self.yaw, self.pitch);
        self.frame * level_right(self.yaw).cross(f)
    }

    pub fn is_parallel(&self) -> bool {
        matches!(self.projection, Projection::Parallel { .. })
    }

    /// World to camera.
    pub fn view(&self) -> DMat4 {
        DMat4::look_to_rh(self.eye, self.forward(), self.up())
    }

    /// Camera to clip, reversed depth. `bounds`: the scene's (for the parallel depth range).
    pub fn proj(&self, aspect: f64, bounds: Option<(DVec3, DVec3)>) -> Mat4 {
        match self.projection {
            Projection::Perspective { fov_y } => {
                let near = (self.focus * 1e-3).clamp(1e-3, 100.0);
                Mat4::perspective_infinite_reverse_rh(fov_y as f32, aspect as f32, near as f32)
            }
            Projection::Parallel { height } => {
                let (hh, hw) = (height / 2.0, height / 2.0 * aspect);
                // Depth range: the scene's extent along the view, padded.
                let (mut near, mut far) = (-1.0, 1.0);
                if let Some((lo, hi)) = bounds {
                    let f = self.forward();
                    let (mut a, mut b) = (f64::MAX, f64::MIN);
                    for i in 0..8 {
                        let c = DVec3::new(
                            if i & 1 == 0 { lo.x } else { hi.x },
                            if i & 2 == 0 { lo.y } else { hi.y },
                            if i & 4 == 0 { lo.z } else { hi.z },
                        );
                        let d = (c - self.eye).dot(f);
                        a = a.min(d);
                        b = b.max(d);
                    }
                    let pad = (b - a).max(1.0) * 0.05;
                    (near, far) = (a - pad, b + pad);
                }
                // Reversed: the near plane maps to 1, the far plane to 0.
                Mat4::orthographic_rh(-hw as f32, hw as f32, -hh as f32, hh as f32, far as f32, near as f32)
            }
        }
    }

    /// The ray through the pixel `px` of a `size` viewport (origin top left).
    pub fn ray(&self, px: DVec2, size: DVec2) -> Ray {
        let (x, y) = ndc(px, size);
        let aspect = size.x / size.y.max(1.0);
        let (f, r, u) = (self.forward(), self.right(), self.up());
        match self.projection {
            Projection::Perspective { fov_y } => {
                let t = (fov_y / 2.0).tan();
                Ray { origin: self.eye, dir: (f + r * (x * t * aspect) + u * (y * t)).normalize(), t_min: 0.0 }
            }
            Projection::Parallel { height } => {
                let hh = height / 2.0;
                Ray { origin: self.eye + r * (x * hh * aspect) + u * (y * hh), dir: f, t_min: f64::NEG_INFINITY }
            }
        }
    }

    /// Orbits by a pointer drag of `delta` pixels around `pivot` (SketchUp: the point under
    /// the cursor when the orbit began), keeping the frame's Z up. As in SketchUp, dragging right turns
    /// the model to the right and dragging down tilts it towards the viewer (the camera rises).
    /// Pitch is clamped short of the zenith, at the top (dragging down) as at the bottom.
    pub fn orbit(&mut self, delta: DVec2, pivot: DVec3) {
        const SPEED: f64 = 0.006;
        // The eye relative to the pivot, in the camera's frame…
        let rel = self.eye - pivot;
        let (r, u, f) = (rel.dot(self.right()), rel.dot(self.up()), rel.dot(self.forward()));
        // In a mirrored frame, turning its way shows as turning the other way.
        self.yaw -= delta.x * SPEED * self.frame.determinant().signum();
        self.pitch = (self.pitch - delta.y * SPEED).clamp(-MAX_PITCH, MAX_PITCH);
        // …kept in the new frame: the pivot stays where it was on screen.
        self.eye = pivot + self.right() * r + self.up() * u + self.forward() * f;
        self.focus = (pivot - self.eye).dot(self.forward()).abs().max(1e-3);
    }

    /// Pans by a pointer drag of `delta` pixels, so the point at `depth_point` follows the
    /// cursor.
    pub fn pan(&mut self, delta: DVec2, size: DVec2, depth_point: DVec3) {
        let per_px = self.world_per_pixel(size, depth_point);
        self.eye += (-self.right() * delta.x + self.up() * delta.y) * per_px;
    }

    /// Zooms by `steps` (positive: closer) towards the pixel `px`; `target` is the scene
    /// point under it, if any.
    pub fn zoom(&mut self, steps: f64, px: DVec2, size: DVec2, target: Option<DVec3>) {
        let factor = 0.85f64.powf(steps);
        let (f, r, u) = (self.forward(), self.right(), self.up());
        match self.projection {
            Projection::Perspective { .. } => {
                let ray = self.ray(px, size);
                let p = target.unwrap_or_else(|| ray.at(self.focus));
                self.eye = p + (self.eye - p) * factor;
                self.focus = ((p - self.eye).dot(f)).abs().max(1e-3);
            }
            Projection::Parallel { height } => {
                let (x, y) = ndc(px, size);
                let hh = height / 2.0;
                let aspect = size.x / size.y.max(1.0);
                let offset = r * (x * hh * aspect) + u * (y * hh);
                self.projection = Projection::Parallel { height: height * factor };
                self.eye += offset * (1.0 - factor);
            }
        }
    }

    /// Frames `bounds` in a viewport of aspect `aspect`.
    pub fn zoom_extents(&mut self, bounds: (DVec3, DVec3), area: ViewArea) {
        let (lo, hi) = bounds;
        let center = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(1.0);
        let f = self.forward();
        let size = area.size.max(DVec2::ONE);
        let free = (area.free_max - area.free_min).max(DVec2::ONE);
        match self.projection {
            Projection::Perspective { fov_y } => {
                // Half-angles the free rectangle covers, from the view axis.
                let t = (fov_y / 2.0).tan() / size.y;
                let half = (t * free.y).atan().min((t * free.x).atan());
                let dist = radius / half.sin();
                self.eye = center - f * dist;
                self.focus = dist;
            }
            Projection::Parallel { .. } => {
                self.projection = Projection::Parallel { height: 2.0 * radius * size.y / free.x.min(free.y) };
                self.eye = center - f * (radius * 3.0);
                self.focus = radius * 3.0;
            }
        }
        // Slide the camera so the centre lands in the middle of the free rectangle.
        let offset = (area.free_min + area.free_max) * 0.5 - size * 0.5;
        let wpp = self.world_per_pixel(size, center);
        self.eye += (-self.right() * offset.x + self.up() * offset.y) * wpp;
    }

    /// Switches projection, keeping the same framing at the focus distance.
    pub fn set_parallel(&mut self, parallel: bool) {
        match (self.projection, parallel) {
            (Projection::Perspective { fov_y }, true) => {
                self.projection = Projection::Parallel { height: 2.0 * self.focus * (fov_y / 2.0).tan() };
            }
            (Projection::Parallel { height }, false) => {
                let target = self.eye + self.forward() * self.focus;
                let dist = height / 2.0 / (DEFAULT_FOV / 2.0).tan();
                self.projection = Projection::Perspective { fov_y: DEFAULT_FOV };
                self.eye = target - self.forward() * dist;
                self.focus = dist;
            }
            _ => {}
        }
    }

    /// Looks along a standard direction of `frame` (the active frame's axes, to world: see
    /// [`orthonormal_frame`]), framing `bounds` when given. Its Z is up on screen: the camera
    /// orbits about that Z until the next standard view. A mirrored frame is seen as it
    /// is drawn (the top view of a left-handed frame has its X to the left).
    pub fn set_standard_view(&mut self, view: StandardView, frame: DMat3, bounds: Option<(DVec3, DVec3)>, area: ViewArea) {
        use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        let (yaw, pitch) = match view {
            StandardView::Top => (FRAC_PI_2, -MAX_PITCH),
            StandardView::Front => (FRAC_PI_2, 0.0),
            StandardView::Right => (PI, 0.0),
            StandardView::Back => (-FRAC_PI_2, 0.0),
            StandardView::Left => (0.0, 0.0),
            StandardView::Iso => (3.0 * FRAC_PI_4, -(1.0f64 / 2.0f64.sqrt()).atan()),
        };
        let target = self.eye + self.forward() * self.focus;
        self.frame = orthonormal_frame(frame).unwrap_or(DMat3::IDENTITY);
        self.yaw = yaw;
        self.pitch = pitch;
        match bounds {
            Some(b) => self.zoom_extents(b, area),
            None => self.eye = target - self.forward() * self.focus,
        }
    }

    /// The size of a pixel at the depth of `p`.
    pub fn world_per_pixel(&self, size: DVec2, p: DVec3) -> f64 {
        match self.projection {
            Projection::Perspective { fov_y } => {
                let d = (p - self.eye).dot(self.forward()).max(1e-3);
                2.0 * d * (fov_y / 2.0).tan() / size.y.max(1.0)
            }
            Projection::Parallel { height } => height / size.y.max(1.0),
        }
    }

    /// Where the world point `p` lands in a `size` viewport (pixels, origin top left), if in
    /// front of the camera.
    pub fn project(&self, p: DVec3, size: DVec2) -> Option<DVec2> {
        self.projector(size).project(p)
    }

    /// The part of the line `origin + t·dir`, `t` in `range` (ends may be infinite), a `size`
    /// viewport shows: clipped to the view's sides, grown by `margin` pixels, and in
    /// perspective to `near` in front of the eye. An end running off to infinity stops where
    /// it is within [`VANISHING_PX`] of its vanishing point. `None` when nothing of it shows,
    /// or when a parallel view sees it end on.
    pub fn visible_segment(
        &self,
        size: DVec2,
        origin: DVec3,
        dir: DVec3,
        range: (f64, f64),
        near: f64,
        margin: f64,
    ) -> Option<(DVec3, DVec3)> {
        let len = dir.length();
        if len < 1e-12 {
            return None;
        }
        let d = dir / len;
        let (mut t0, mut t1) = (range.0 * len, range.1 * len);
        let (f, r, u) = (self.forward(), self.right(), self.up());
        let o = origin - self.eye;
        let (ox, oy, oz) = (o.dot(r), o.dot(u), o.dot(f));
        let (dx, dy, dz) = (d.dot(r), d.dot(u), d.dot(f));
        let (gx, gy) = (1.0 + 2.0 * margin / size.x.max(1.0), 1.0 + 2.0 * margin / size.y.max(1.0));
        let aspect = size.x / size.y.max(1.0);
        // The half-spaces `a + b·t ≥ 0` the view is made of.
        let half_spaces: Vec<(f64, f64)> = match self.projection {
            Projection::Perspective { fov_y } => {
                let (tx, ty) = ((fov_y / 2.0).tan() * aspect * gx, (fov_y / 2.0).tan() * gy);
                vec![
                    (oz - near, dz),
                    (tx * oz - ox, tx * dz - dx),
                    (tx * oz + ox, tx * dz + dx),
                    (ty * oz - oy, ty * dz - dy),
                    (ty * oz + oy, ty * dz + dy),
                ]
            }
            Projection::Parallel { height } => {
                // Seen end on, give or take the standard views stopping short of the zenith.
                if dx.hypot(dy) < END_ON {
                    return None;
                }
                let (hw, hh) = (height / 2.0 * aspect * gx, height / 2.0 * gy);
                vec![(hw - ox, -dx), (hw + ox, dx), (hh - oy, -dy), (hh + oy, dy)]
            }
        };
        for (a, b) in half_spaces {
            if b.abs() < 1e-15 {
                if a < 0.0 {
                    return None;
                }
            } else if b > 0.0 {
                t0 = t0.max(-a / b);
            } else {
                t1 = t1.min(-a / b);
            }
        }
        // Perspective only: an end left infinite runs to its vanishing point (the sides keep
        // it in view only if it recedes, dz ≠ 0). At depth z it is `|cross| / (|dz|·z)` from
        // it (in units of depth): it stops at the depth where that is VANISHING_PX.
        if let Projection::Perspective { fov_y } = self.projection
            && t0 < t1
            && (t0.is_infinite() || t1.is_infinite())
            && dz.abs() > 1e-15
        {
            let focal_px = size.y.max(1.0) / (2.0 * (fov_y / 2.0).tan());
            let cross = (ox * dz - dx * oz).hypot(oy * dz - dy * oz);
            let far_t = (focal_px * cross / (dz.abs() * VANISHING_PX) - oz) / dz;
            if t1.is_infinite() {
                t1 = far_t;
            } else {
                t0 = far_t;
            }
        }
        (t0.is_finite() && t1.is_finite() && t0 < t1).then(|| (origin + d * t0, origin + d * t1))
    }

    /// Projects many points into a `size` viewport (the camera's axes computed once): in
    /// `f64`, the exact inverse of [`Camera::ray`] — not through [`Camera::proj`], an `f32`
    /// matrix for the GPU, which would move a point by a millionth of the view (a world point
    /// projected then picked back must land on itself).
    pub fn projector(&self, size: DVec2) -> Projector {
        let aspect = size.x / size.y.max(1.0);
        let (forward, right, up) = (self.forward(), self.right(), self.up());
        let half = match self.projection {
            Projection::Perspective { fov_y } => (fov_y / 2.0).tan(),
            Projection::Parallel { height } => height / 2.0,
        };
        Projector { eye: self.eye, forward, right: right / (half * aspect), up: up / half, parallel: self.is_parallel(), size }
    }
}

/// World to viewport pixels, for one camera and viewport.
#[derive(Clone, Copy, Debug)]
pub struct Projector {
    eye: DVec3,
    forward: DVec3,
    /// The camera's right and up, scaled so that the view's edges are at ±1 (at depth 1 in
    /// perspective).
    right: DVec3,
    up: DVec3,
    parallel: bool,
    size: DVec2,
}

impl Projector {
    /// Where the world point `p` lands (pixels, origin top left), if in front of the camera.
    pub fn project(&self, p: DVec3) -> Option<DVec2> {
        let d = p - self.eye;
        let (x, y) = (d.dot(self.right), d.dot(self.up));
        let (x, y) = if self.parallel {
            (x, y)
        } else {
            let depth = d.dot(self.forward);
            if depth <= 0.0 {
                return None;
            }
            (x / depth, y / depth)
        };
        Some(DVec2::new((x + 1.0) * 0.5 * self.size.x, (1.0 - y) * 0.5 * self.size.y))
    }
}

fn ndc(px: DVec2, size: DVec2) -> (f64, f64) {
    (2.0 * px.x / size.x.max(1.0) - 1.0, 1.0 - 2.0 * px.y / size.y.max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: DVec2 = DVec2::new(800.0, 600.0);

    #[test]
    fn ray_through_projected_point() {
        // Near the origin and 100 km away: a point projected then cast back lands on itself,
        // to the precision of f64 (the control protocol clicks world points, §7 septies).
        for far in [DVec3::ZERO, DVec3::new(1.0e8, -3.0e7, 2.0e5)] {
            for parallel in [false, true] {
                let mut c = Camera::default();
                c.eye += far;
                c.set_parallel(parallel);
                let p = c.eye + c.forward() * c.focus + DVec3::new(120.0, -40.0, 75.0);
                let px = c.project(p, SIZE).unwrap();
                let r = c.ray(px, SIZE);
                let q = r.closest_on_line(p, DVec3::Z);
                assert!((q - p).length() < 1e-9 * p.length().max(1.0), "parallel {parallel}: {q} vs {p}");
            }
        }
    }

    #[test]
    fn projecting_agrees_with_what_the_gpu_draws() {
        for parallel in [false, true] {
            let mut c = Camera::default();
            c.set_parallel(parallel);
            let gpu = c.proj(SIZE.x / SIZE.y, None).as_dmat4() * c.view();
            for p in [DVec3::new(120.0, -40.0, 75.0), DVec3::new(-900.0, 300.0, 10.0), DVec3::ZERO] {
                let clip = gpu * p.extend(1.0);
                let ndc = clip.truncate() / clip.w;
                let drawn = DVec2::new((ndc.x + 1.0) * 0.5 * SIZE.x, (1.0 - ndc.y) * 0.5 * SIZE.y);
                let px = c.project(p, SIZE).unwrap();
                assert!((px - drawn).length() < 0.01, "parallel {parallel}: {px} vs {drawn}");
            }
        }
    }

    #[test]
    fn orbit_keeps_pivot_on_screen_and_z_up() {
        let mut c = Camera::default();
        let pivot = DVec3::new(100.0, 50.0, 0.0);
        let before = c.project(pivot, SIZE).unwrap();
        c.orbit(DVec2::new(120.0, -45.0), pivot);
        let after = c.project(pivot, SIZE).unwrap();
        assert!((before - after).length() < 1e-6, "{before} → {after}");
        assert!(c.up().z > 0.0);
        assert!(c.right().z.abs() < 1e-12);
    }

    #[test]
    fn dragging_down_raises_the_camera() {
        // SketchUp: drag the mouse down, the camera moves up and looks further down.
        let mut c = Camera::default();
        let (z, pitch) = (c.eye.z, c.pitch);
        c.orbit(DVec2::new(0.0, 50.0), c.eye + c.forward() * c.focus);
        assert!(c.eye.z > z && c.pitch < pitch, "eye z {z} → {}, pitch {pitch} → {}", c.eye.z, c.pitch);
    }

    #[test]
    fn zoom_keeps_target_under_cursor() {
        for parallel in [false, true] {
            let mut c = Camera::default();
            c.set_parallel(parallel);
            let px = DVec2::new(600.0, 200.0);
            let target = c.ray(px, SIZE).at(c.focus * 0.7);
            c.zoom(2.0, px, SIZE, Some(target));
            let now = c.project(target, SIZE).unwrap();
            assert!((now - px).length() < 1e-3, "parallel {parallel}: {now}");
        }
    }

    #[test]
    fn pan_follows_cursor() {
        let mut c = Camera::default();
        let p = c.eye + c.forward() * c.focus;
        let before = c.project(p, SIZE).unwrap();
        c.pan(DVec2::new(30.0, -12.0), SIZE, p);
        let after = c.project(p, SIZE).unwrap();
        assert!((after - before - DVec2::new(30.0, -12.0)).length() < 1e-3, "{before} → {after}");
    }

    #[test]
    fn top_view_has_y_up_on_screen() {
        let mut c = Camera::default();
        c.set_standard_view(
            StandardView::Top,
            glam::DMat3::IDENTITY,
            Some((DVec3::ZERO, DVec3::splat(100.0))),
            ViewArea::full(DVec2::splat(600.0)),
        );
        assert!(c.up().y > 0.99 && c.forward().z < -0.99);
    }

    const AREA: ViewArea = ViewArea { size: SIZE, free_min: DVec2::ZERO, free_max: SIZE };
    const BOX: (DVec3, DVec3) = (DVec3::ZERO, DVec3::splat(100.0));

    #[test]
    fn views_of_a_frame_are_the_world_s_turned() {
        let tilted = DMat3::from_rotation_x(30f64.to_radians()) * DMat3::from_rotation_z(20f64.to_radians());
        for frame in [DMat3::from_rotation_z(30f64.to_radians()), tilted] {
            for view in [StandardView::Top, StandardView::Front, StandardView::Right, StandardView::Iso] {
                let (mut world, mut c) = (Camera::default(), Camera::default());
                world.set_standard_view(view, DMat3::IDENTITY, Some(BOX), AREA);
                c.set_standard_view(view, frame, Some(BOX), AREA);
                assert!((c.forward() - frame * world.forward()).length() < 1e-9, "{view:?}");
                assert!((c.up() - frame * world.up()).length() < 1e-9, "{view:?}");
            }
        }
    }

    #[test]
    fn views_of_a_tilted_frame_have_its_z_up() {
        let frame = DMat3::from_rotation_x(30f64.to_radians()) * DMat3::from_rotation_z(20f64.to_radians());
        let mut c = Camera::default();
        c.set_standard_view(StandardView::Front, frame, Some(BOX), AREA);
        assert!((c.forward() - frame.y_axis).length() < 1e-9 && (c.up() - frame.z_axis).length() < 1e-9);
        assert!((c.right() - frame.x_axis).length() < 1e-9);
        c.set_standard_view(StandardView::Top, frame, Some(BOX), AREA);
        assert!(c.forward().dot(-frame.z_axis) > 0.999 && c.up().dot(frame.y_axis) > 0.999);
        // The framing and the ray still agree with the projection.
        let p = DVec3::new(30.0, 60.0, 10.0);
        let px = c.project(p, SIZE).unwrap();
        let r = c.ray(px, SIZE);
        let d = (p - r.origin).cross(r.dir).length();
        assert!(d < 1e-6 * c.focus, "{d}");
    }

    #[test]
    fn a_mirrored_frame_is_seen_as_drawn() {
        // Left-handed: X mirrored. Its top view looks down its Z, Y up: its X runs left.
        let frame = DMat3::from_cols(-DVec3::X, DVec3::Y, DVec3::Z);
        let mut c = Camera::default();
        c.set_standard_view(StandardView::Top, frame, Some(BOX), AREA);
        assert!(c.forward().z < -0.99 && c.up().y > 0.99 && c.right().x > 0.99);
        assert!(c.right().dot(frame.x_axis) < -0.99);
        // Tilted as well.
        let frame = DMat3::from_rotation_x(40f64.to_radians()) * frame;
        c.set_standard_view(StandardView::Top, frame, Some(BOX), AREA);
        assert!(c.forward().dot(-frame.z_axis) > 0.999 && c.right().dot(frame.x_axis) < -0.99);
    }

    #[test]
    fn orbiting_keeps_the_frame_s_z_up_without_a_jump() {
        let tilted = DMat3::from_rotation_y(25f64.to_radians());
        let mirrored = tilted * DMat3::from_cols(-DVec3::X, DVec3::Y, DVec3::Z);
        let mut way = None;
        for frame in [DMat3::IDENTITY, tilted, mirrored] {
            let mut c = Camera::default();
            c.set_standard_view(StandardView::Iso, frame, Some(BOX), AREA);
            let pivot = c.eye + c.forward() * c.focus;
            let (forward, up) = (c.forward(), c.up());
            // A point between the eye and the pivot, in the middle of the view.
            let near = pivot - forward * c.focus * 0.5;
            let before = c.project(near, SIZE).unwrap();
            c.orbit(DVec2::new(1.0, 0.0), pivot);
            // A tiny drag, a tiny turn: no jump.
            assert!(c.forward().dot(forward) > 0.9999 && c.up().dot(up) > 0.9999);
            // The frame's Z stays up: the right direction stays in its XY plane.
            assert!(c.right().dot(frame.z_axis).abs() < 1e-12 && c.up().dot(frame.z_axis) > 0.0);
            // The pivot stays put, and the model turns the same way on screen in every frame.
            assert!((c.project(pivot, SIZE).unwrap() - SIZE / 2.0).length() < 1e-3);
            let moved = (c.project(near, SIZE).unwrap().x - before.x).signum();
            assert_eq!(*way.get_or_insert(moved), moved, "{frame}");
        }
    }

    #[test]
    fn orthonormal_frame_drops_scale_and_shear() {
        let m = DMat3::from_cols(DVec3::new(2.0, 0.1, 0.0), DVec3::new(0.0, 3.0, 0.0), DVec3::new(0.0, 0.0, -0.5));
        let f = orthonormal_frame(m).unwrap();
        assert!((f.determinant() - m.determinant().signum()).abs() < 1e-12);
        assert!((f.z_axis + DVec3::Z).length() < 1e-12 && f.x_axis.dot(DVec3::X) > 0.99);
        assert!(orthonormal_frame(DMat3::ZERO).is_none());
    }

    #[test]
    fn zoom_extents_centres_in_the_free_area() {
        let bounds = (DVec3::splat(-50.0), DVec3::splat(50.0));
        // A panel covers the right 300 px.
        let area = ViewArea { size: SIZE, free_min: DVec2::ZERO, free_max: DVec2::new(500.0, 600.0) };
        for parallel in [false, true] {
            let mut c = Camera::default();
            c.set_parallel(parallel);
            c.zoom_extents(bounds, area);
            let p = c.project(DVec3::ZERO, SIZE).unwrap();
            assert!((p - DVec2::new(250.0, 300.0)).length() < 1.0, "{p}");
            // The corners stay inside the free area.
            for k in 0..8 {
                let corner = DVec3::new(
                    if k & 1 == 0 { -50.0 } else { 50.0 },
                    if k & 2 == 0 { -50.0 } else { 50.0 },
                    if k & 4 == 0 { -50.0 } else { 50.0 },
                );
                let q = c.project(corner, SIZE).unwrap();
                assert!(q.x > -0.5 && q.x < 500.5 && q.y > -0.5 && q.y < 600.5, "{q}");
            }
        }
    }
}

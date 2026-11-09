//! The camera: SketchUp navigation (Z always up), perspective or parallel, in `f64`.
//!
//! It gives the renderer a `f64` view and a **reversed-depth** projection (ARCHITECTURE.md
//! §7 bis), and the tools a picking [`Ray`] per pixel.

use glam::{DMat4, DVec2, DVec3, Mat4};

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
    /// Heading of the forward direction around +Z (0 = +X), radians.
    pub yaw: f64,
    /// Elevation of the forward direction (positive looks up), radians.
    pub pitch: f64,
    pub projection: Projection,
    /// Distance to what the camera looks at (orbit pivot, zoom target): sets the near plane.
    pub focus: f64,
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
        };
        c.set_standard_view(StandardView::Iso, Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))), ViewArea::full(DVec2::new(1200.0, 800.0)));
        c
    }
}

impl Camera {
    pub fn forward(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        DVec3::new(cp * cy, cp * sy, sp)
    }

    pub fn right(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        DVec3::new(sy, -cy, 0.0)
    }

    pub fn up(&self) -> DVec3 {
        self.right().cross(self.forward())
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
    /// the cursor when the orbit began), keeping Z up. As in SketchUp, dragging right turns
    /// the model to the right and dragging down tilts it towards the viewer (the camera rises).
    /// Pitch is clamped short of the zenith, at the top (dragging down) as at the bottom.
    pub fn orbit(&mut self, delta: DVec2, pivot: DVec3) {
        const SPEED: f64 = 0.006;
        // The eye relative to the pivot, in the camera's frame…
        let rel = self.eye - pivot;
        let (r, u, f) = (rel.dot(self.right()), rel.dot(self.up()), rel.dot(self.forward()));
        self.yaw -= delta.x * SPEED;
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

    /// Looks along a standard direction, framing `bounds` when given.
    pub fn set_standard_view(&mut self, view: StandardView, bounds: Option<(DVec3, DVec3)>, area: ViewArea) {
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

    /// Projects many points into a `size` viewport (the matrices computed once).
    pub fn projector(&self, size: DVec2) -> Projector {
        Projector {
            view_proj: self.proj(size.x / size.y.max(1.0), None).as_dmat4() * self.view(),
            parallel: self.is_parallel(),
            size,
        }
    }
}

/// World to viewport pixels, for one camera and viewport.
#[derive(Clone, Copy, Debug)]
pub struct Projector {
    view_proj: DMat4,
    parallel: bool,
    size: DVec2,
}

impl Projector {
    /// Where the world point `p` lands (pixels, origin top left), if in front of the camera.
    pub fn project(&self, p: DVec3) -> Option<DVec2> {
        let clip = self.view_proj * p.extend(1.0);
        if clip.w <= 0.0 && !self.parallel {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(DVec2::new((ndc.x + 1.0) * 0.5 * self.size.x, (1.0 - ndc.y) * 0.5 * self.size.y))
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
        for parallel in [false, true] {
            let mut c = Camera::default();
            c.set_parallel(parallel);
            let p = DVec3::new(120.0, -40.0, 75.0);
            let px = c.project(p, SIZE).unwrap();
            let r = c.ray(px, SIZE);
            let q = r.closest_on_line(p, DVec3::Z);
            assert!((q - p).length() < 1e-6 * c.focus.max(1.0), "parallel {parallel}: {q} vs {p}");
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
        c.set_standard_view(StandardView::Top, Some((DVec3::ZERO, DVec3::splat(100.0))), ViewArea::full(DVec2::splat(600.0)));
        assert!(c.up().y > 0.99 && c.forward().z < -0.99);
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
                let corner = DVec3::new(if k & 1 == 0 { -50.0 } else { 50.0 }, if k & 2 == 0 { -50.0 } else { 50.0 }, if k & 4 == 0 { -50.0 } else { 50.0 });
                let q = c.project(corner, SIZE).unwrap();
                assert!(q.x > -0.5 && q.x < 500.5 && q.y > -0.5 && q.y < 600.5, "{q}");
            }
        }
    }
}

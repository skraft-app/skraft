//! Navigation, as SketchUp does it: orbit and pan about the point under the cursor, zoom,
//! standard views.

use glam::{DVec2, DVec3};

use super::Editor;
use crate::camera::StandardView;

impl Editor {
    // ---- navigation (SketchUp) -----------------------------------------------------------------

    /// Starts an orbit or a pan at `px`: the point under the cursor becomes the pivot.
    pub fn nav_begin(&mut self, px: DVec2) {
        let ray = self.camera.ray(px, self.size);
        self.pivot = Some(
            self.scene
                .pick(&ray, |_| true)
                .map(|h| h.point)
                .unwrap_or_else(|| self.camera.eye + self.camera.forward() * self.camera.focus),
        );
    }

    pub(super) fn pivot(&self) -> DVec3 {
        self.pivot.unwrap_or_else(|| self.camera.eye + self.camera.forward() * self.camera.focus)
    }

    pub fn orbit(&mut self, delta: DVec2) {
        let p = self.pivot();
        self.camera.orbit(delta, p);
    }

    pub fn pan(&mut self, delta: DVec2) {
        let p = self.pivot();
        self.camera.pan(delta, self.size, p);
    }

    /// Zooms by `steps` (positive: closer) towards the scene under `px`.
    pub fn zoom(&mut self, px: DVec2, steps: f64) {
        let ray = self.camera.ray(px, self.size);
        let target = self.scene.pick(&ray, |_| true).map(|h| h.point);
        self.camera.zoom(steps, px, self.size, target);
    }

    /// Centres the view on the scene point under `px` (SketchUp's middle double-click).
    pub fn recenter(&mut self, px: DVec2) {
        let ray = self.camera.ray(px, self.size);
        if let Some(h) = self.scene.pick(&ray, |_| true) {
            let d = (h.point - self.camera.eye).dot(self.camera.forward());
            self.camera.eye = h.point - self.camera.forward() * d;
            self.camera.focus = d.abs().max(1e-3);
        }
    }

    pub fn zoom_extents(&mut self) {
        let b = self.scene.bounds().unwrap_or((DVec3::splat(-1000.0), DVec3::splat(1000.0)));
        self.camera.zoom_extents(b, self.view_area());
    }

    pub fn standard_view(&mut self, v: StandardView) {
        let b = self.scene.bounds().or(Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))));
        self.camera.set_standard_view(v, b, self.view_area());
    }

    pub fn set_parallel(&mut self, parallel: bool) {
        self.camera.set_parallel(parallel);
    }
}

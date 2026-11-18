//! Navigation: orbit and pan about a pivot, zoom (wheel, drag, window), standard views, and
//! the views gone through (previous / next). Which gesture does what is the app's navigation
//! style (docs/ui/navigation-styles.md); the editor only moves the camera.

use glam::{DAffine3, DVec2, DVec3};

use super::Editor;
use crate::camera::StandardView;
use crate::navigation::Pivot;
use crate::scene::ContextIndex;

/// How many views the history keeps.
const VIEWS_KEPT: usize = 50;

impl Editor {
    // ---- navigation ----------------------------------------------------------------------------

    /// Starts an orbit or a pan at `px`, about the point under the cursor (SketchUp).
    pub fn nav_begin(&mut self, px: DVec2) {
        self.nav_begin_about(px, Pivot::Aimed);
    }

    /// Starts an orbit or a pan at `px`, about `pivot` as the style has it: the camera's view
    /// so far is kept for « previous view ».
    pub fn nav_begin_about(&mut self, px: DVec2, pivot: Pivot) {
        self.view_changing();
        let target = self.camera.eye + self.camera.forward() * self.camera.focus;
        self.pivot = Some(match pivot {
            Pivot::Aimed => {
                let ray = self.camera.ray(px, self.size);
                self.scene.pick(&ray, |_| true).map(|h| h.point).unwrap_or(target)
            }
            Pivot::SelectionOrModel => {
                self.selection_center().or_else(|| self.scene.bounds().map(|(lo, hi)| (lo + hi) * 0.5)).unwrap_or(target)
            }
            Pivot::ScreenCentre => target,
        });
    }

    /// The centre of the box bounding the selection (world), if it has occurrences.
    fn selection_center(&self) -> Option<DVec3> {
        let ctx = ContextIndex::new(self.document(), &self.context)?;
        let mut bounds: Option<(DVec3, DVec3)> = None;
        for &id in &self.selection {
            let Some(b) = self.content_bounds(&ctx, id, DAffine3::IDENTITY, 0.0) else { continue };
            for c in b.corners() {
                bounds = Some(bounds.map_or((c, c), |(lo, hi)| (lo.min(c), hi.max(c))));
            }
        }
        bounds.map(|(lo, hi)| (lo + hi) * 0.5)
    }

    pub(crate) fn pivot(&self) -> DVec3 {
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

    /// Fills the view with the rectangle from `a` to `b` (pixels), the view's direction kept.
    /// The depth is the scene's under its centre, else the camera's target.
    pub fn zoom_window(&mut self, a: DVec2, b: DVec2) {
        self.view_changing();
        let centre = (a + b) * 0.5;
        let ray = self.camera.ray(centre, self.size);
        let depth = self.scene.pick(&ray, |_| true).map(|h| h.point).unwrap_or_else(|| ray.at(self.camera.focus));
        self.camera.zoom_window(a, b, self.size, depth);
    }

    /// Centres the view on the scene point under `px` (SketchUp's middle double-click).
    pub fn recenter(&mut self, px: DVec2) {
        let ray = self.camera.ray(px, self.size);
        if let Some(h) = self.scene.pick(&ray, |_| true) {
            self.view_changing();
            let d = (h.point - self.camera.eye).dot(self.camera.forward());
            self.camera.eye = h.point - self.camera.forward() * d;
            self.camera.focus = d.abs().max(1e-3);
        }
    }

    pub fn zoom_extents(&mut self) {
        self.view_changing();
        let b = self.scene.bounds().unwrap_or((DVec3::splat(-1000.0), DVec3::splat(1000.0)));
        self.camera.zoom_extents(b, self.view_area());
    }

    pub fn standard_view(&mut self, v: StandardView) {
        self.view_changing();
        let b = self.scene.bounds().or(Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))));
        // In the active frame (docs/model/data-model.md): the open part's axes, else the context's.
        self.camera.set_standard_view(v, self.axes_transform().matrix3, b, self.view_area());
    }

    pub fn set_parallel(&mut self, parallel: bool) {
        self.view_changing();
        self.camera.set_parallel(parallel);
    }

    // ---- the views gone through ------------------------------------------------------------------

    /// The view shown becomes the history's current one, unless it already is.
    fn record_view(&mut self) {
        if self.views.get(self.view_index) == Some(&self.camera) {
            return;
        }
        self.views.truncate(self.view_index + 1);
        self.views.push(self.camera.clone());
        if self.views.len() > VIEWS_KEPT {
            self.views.remove(0);
        }
        self.view_index = self.views.len() - 1;
    }

    /// The camera is about to move: the view shown is kept, and the views after it (gone back
    /// from) are dropped, as a browser does. The view the move ends on is kept at the next one,
    /// or at « previous view ».
    pub fn view_changing(&mut self) {
        self.record_view();
        self.views.truncate(self.view_index + 1);
    }

    /// Back to the view before (« Vue précédente »): whether there was one.
    pub fn previous_view(&mut self) -> bool {
        self.record_view();
        if self.view_index == 0 {
            return false;
        }
        self.view_index -= 1;
        self.camera = self.views[self.view_index].clone();
        true
    }

    /// Forward to the view gone back from (« Vue suivante »): whether there was one.
    pub fn next_view(&mut self) -> bool {
        // Moved since going back: nothing after it any more.
        if self.views.get(self.view_index) != Some(&self.camera) || self.view_index + 1 >= self.views.len() {
            return false;
        }
        self.view_index += 1;
        self.camera = self.views[self.view_index].clone();
        true
    }

    /// The views kept and the index of the one shown (tests).
    #[cfg(test)]
    pub(crate) fn views(&self) -> (&[crate::camera::Camera], usize) {
        (&self.views, self.view_index)
    }
}

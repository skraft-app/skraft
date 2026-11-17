//! Rotate (SketchUp's protractor): click the centre — the axis is the normal of the face under
//! it, else the context's Z; the arrow keys lock it (→ red X, ← green Y, ↑ blue Z, ↓ free) —
//! then a reference direction (0°), then the angle. Inside the protractor, both snap to its
//! 15° graduations, and the radii reach them; past it, to what is inferred (axes, edges'
//! parallels, features), and the radii reach the cursor. The angle can be
//! typed in degrees (`360/7`, `@/2`: half the angle so far). A tap of the copy modifier (Option on macOS, Ctrl elsewhere) toggles
//! copying; right after a copy, `6x` makes a circular array of 6 at the same angle and `/6`
//! spreads 6 copies over it. What turns is picked first (§6 septendecies, [`Picker`]): the
//! selection, else a click, Shift clicks or a rectangle; once turned, the selection is let go
//! and the tool picks again.

use super::DragClicks;
use super::pick::Picker;
use crate::infer::Inference;
use crate::scene::Owner;
use crate::vcb::{self, Vcb, VcbKind, VcbValue};
use glam::{DAffine3, DQuat, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::{EntityPath, Id};

use super::{
    BLUE, Command, Cx, GREEN, INK, Key, LastCopy, Motion, OptionValue, Preview, PreviewLabel, PreviewLine, RED, Tool, ToolInput, ToolKind,
    ToolOption, dashed, guide_color,
};

/// The protractor's graduations, which the cursor snaps to inside it (radians).
const SNAP_STEP: f64 = std::f64::consts::PI / 12.0;
/// Radius of the protractor, in pixels.
const PROTRACTOR_PX: f64 = 70.0;

#[derive(Default)]
pub struct RotateTool {
    picker: Picker,
    /// The selection is taken, as the last input left it (else the tool picks).
    taken: bool,
    state: State,
    copy: bool,
    moving: Vec<Id>,
    /// The parts moved at any depth (« Parts », §6 septendecies).
    moving_paths: Vec<EntityPath>,
    /// Where the protractor would go, and its axis, before the first click.
    cursor: Option<(DVec3, DVec3)>,
    snap: Option<Inference>,
    /// The copy just made, which a typed count turns into an array.
    last: Option<LastCopy>,
}

#[derive(Default, Clone, Copy)]
enum State {
    #[default]
    Center,
    Reference {
        center: DVec3,
        axis: DVec3,
        /// The direction to the cursor, and how far it is.
        dir: Option<(DVec3, f64)>,
    },
    Angle {
        center: DVec3,
        axis: DVec3,
        reference: DVec3,
        /// How far from the centre the reference was clicked.
        reference_reach: f64,
        angle: f64,
        /// How far from the centre the cursor is.
        reach: f64,
    },
}

/// The axes' axis `k` (the open part's, else the context's), in the world.
fn context_axis(cx: &Cx<'_>, k: usize) -> DVec3 {
    cx.axis(k)
}

/// The rotation of `angle` about the line through `center` along `axis` (world).
pub fn rotation_about(center: DVec3, axis: DVec3, angle: f64) -> DAffine3 {
    DAffine3::from_translation(center)
        * DAffine3::from_quat(DQuat::from_axis_angle(axis.normalize(), angle))
        * DAffine3::from_translation(-center)
}

impl RotateTool {
    /// The centre and axis the cursor gives before the first click: the point inferred, and
    /// the normal of the face under the cursor (else the context's Z), unless an axis is locked.
    fn center_at(&self, cx: &Cx<'_>, px: DVec2) -> Option<(DVec3, DVec3, Inference)> {
        let inf = cx.infer(px, |_| true, None)?;
        let axis = match (cx.axis_lock, &inf.hit) {
            (Some(k), _) => context_axis(cx, k),
            (None, Some(h)) => h.normal,
            (None, None) => context_axis(cx, 2),
        };
        Some((inf.point, axis, inf))
    }

    /// The direction from `center` to the cursor, in the protractor's plane, and how far the
    /// cursor is. Inside the protractor, the direction snaps to its graduations, counted from
    /// `zero`, and reaches them (the protractor's radius). Past it: towards a feature inferred (its projection on the plane) or along an
    /// axis or an edge's parallel lying in the plane, else where the cursor's ray meets the
    /// plane. While turning, what turns is not inferred on, unless copying: then the original
    /// stays in place.
    fn direction(&self, cx: &Cx<'_>, px: DVec2, center: DVec3, axis: DVec3, zero: DVec3) -> (Option<(DVec3, f64)>, Option<Inference>) {
        let in_plane = |p: DVec3| {
            let d = p - center;
            d - axis * d.dot(axis)
        };
        let hit = cx.ray(px).hit_plane(center, axis).map(in_plane);
        let r = protractor_radius(cx, center);
        if let Some(h) = hit.filter(|h| h.length() < r) {
            let Some(d) = h.try_normalize() else { return (None, None) };
            let v = axis.cross(zero);
            let a = (v.dot(d).atan2(zero.dot(d)) / SNAP_STEP).round() * SNAP_STEP;
            return (Some((zero * a.cos() + v * a.sin(), r)), None);
        }
        let turning = matches!(self.state, State::Angle { .. });
        let keep = |o: &Owner<'_>| self.copy || !(turning && (cx.is_moving(o, &self.moving) || cx.is_moving_deep(o, &self.moving_paths)));
        let inf = cx.infer_in_plane(px, keep, (center, axis), center).filter(|i| i.kind.is_feature());
        let d = match &inf {
            Some(i) => Some(in_plane(i.point)),
            None => hit,
        };
        let dir = d.and_then(|d| d.try_normalize().map(|n| (n, d.length())));
        (dir, inf.filter(|_| dir.is_some()))
    }

    /// Where the protractor's graduations start before a reference is given: the context's X
    /// laid in its plane (its Y when the axis is X).
    fn zero(cx: &Cx<'_>, axis: DVec3) -> DVec3 {
        [0, 1, 2]
            .into_iter()
            .find_map(|k| {
                let a = context_axis(cx, k);
                (a - axis * a.dot(axis)).try_normalize().filter(|_| a.dot(axis).abs() < 0.9999)
            })
            .unwrap_or_else(|| axis.any_orthonormal_vector())
    }

    fn finish(&mut self, cx: &Cx<'_>, center: DVec3, axis: DVec3, angle: f64) -> Vec<Command> {
        let world = rotation_about(center, axis, angle);
        let entities = std::mem::take(&mut self.moving);
        let paths = std::mem::take(&mut self.moving_paths);
        let moved = if paths.is_empty() { cx.moved() } else { cx.moved_deep(&paths) };
        let copy = self.copy;
        self.reset();
        let op = if copy {
            let last = LastCopy::new(moved, entities, Motion::Rotate { center, axis, angle });
            let op = last.op.clone();
            self.last = Some(last);
            op
        } else {
            moved.transform(entities, world)
        };
        // Turned, the selection is let go: the tool picks again (OCL's restart).
        vec![Command::Apply(op), Command::Select(Vec::new())]
    }

    /// Is the tool picking what it turns (§6 septendecies)?
    fn picking(&self) -> bool {
        matches!(self.state, State::Center) && !self.taken
    }

    fn input_state(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        self.picker.shift_released(cx);
        let picking = matches!(self.state, State::Center) && !self.picker.taken(cx);
        match (input, self.state) {
            (ToolInput::Key { key: Key::Copy, .. }, _) => self.copy = !self.copy,
            (ToolInput::Value(VcbValue::Count(r)), State::Center) => {
                let Some(last) = &mut self.last else { return vec![] };
                return last.retype(*r);
            }
            // Picking (§6 septendecies): what turns becomes the selection.
            _ if picking => {
                self.snap = None;
                self.cursor = None;
                let (out, took) = self.picker.input(input, cx);
                if took {
                    self.last = None;
                }
                return out;
            }
            (ToolInput::Hover { px }, State::Center) => {
                let c = self.center_at(cx, *px);
                self.cursor = c.as_ref().map(|(p, a, _)| (*p, *a));
                self.snap = c.map(|(_, _, i)| i);
            }
            (ToolInput::Hover { px }, State::Reference { center, axis, .. }) => {
                let (dir, snap) = self.direction(cx, *px, center, axis, Self::zero(cx, axis));
                self.state = State::Reference { center, axis, dir };
                self.snap = snap;
            }
            (ToolInput::Hover { px }, State::Angle { center, axis, reference, reference_reach, .. }) => {
                // The graduations count from the reference.
                let (dir, snap) = self.direction(cx, *px, center, axis, reference);
                if let Some((d, reach)) = dir {
                    let angle = axis.dot(reference.cross(d)).atan2(reference.dot(d));
                    self.state = State::Angle { center, axis, reference, reference_reach, angle, reach };
                }
                self.snap = snap;
            }
            (ToolInput::Click { px, .. }, State::Center) => {
                self.last = None;
                let Some((center, axis, _)) = self.center_at(cx, *px) else { return vec![] };
                self.moving = self.picker.selection(cx);
                self.moving_paths = self.picker.deep(cx);
                self.state = State::Reference { center, axis, dir: None };
            }
            (ToolInput::Click { .. }, State::Reference { center, axis, dir }) => {
                let Some((reference, reach)) = dir else {
                    return vec![Command::Say(t!("tool.rotate.say.too_close"))];
                };
                self.state = State::Angle { center, axis, reference, reference_reach: reach, angle: 0.0, reach };
            }
            (ToolInput::Click { .. }, State::Angle { center, axis, angle, .. }) => {
                return self.finish(cx, center, axis, angle);
            }
            (ToolInput::Value(VcbValue::Angle(angle)), State::Reference { center, axis, .. } | State::Angle { center, axis, .. }) => {
                return self.finish(cx, center, axis, *angle);
            }
            _ => {}
        }
        vec![]
    }

    fn color(&self, cx: &Cx<'_>, axis: DVec3) -> [f32; 4] {
        [(0, RED), (1, GREEN), (2, BLUE)].into_iter().find(|(k, _)| context_axis(cx, *k).dot(axis).abs() > 0.9999).map_or(INK, |(_, c)| c)
    }
}

/// The protractor's radius at `center`, in the world.
fn protractor_radius(cx: &Cx<'_>, center: DVec3) -> f64 {
    cx.camera.world_per_pixel(cx.size, center) * PROTRACTOR_PX
}

impl Tool for RotateTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Rotate
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        let out = self.input_state(input, cx);
        self.taken = self.picker.taken(cx);
        out
    }
    fn preview(&self, cx: &Cx<'_>) -> Preview {
        // Its graduations start from `u`, in its plane.
        let protractor = |center: DVec3, axis: DVec3, u: DVec3, color: [f32; 4]| -> Vec<PreviewLine> {
            let r = protractor_radius(cx, center);
            let v = axis.cross(u);
            const N: usize = 48;
            let p = |i: usize| {
                let a = i as f64 / N as f64 * std::f64::consts::TAU;
                center + (u * a.cos() + v * a.sin()) * r
            };
            let mut lines: Vec<PreviewLine> = (0..N).map(|i| PreviewLine { a: p(i), b: p(i + 1), color, bold: false }).collect();
            // Ticks every 15°.
            for k in 0..24 {
                let a = k as f64 * SNAP_STEP;
                let d = u * a.cos() + v * a.sin();
                let inner = if k % 6 == 0 { 0.8 } else { 0.9 };
                lines.push(PreviewLine { a: center + d * r * inner, b: center + d * r, color, bold: false });
            }
            lines
        };
        match self.state {
            State::Center if self.picking() => {
                let mut p = Preview::default();
                self.picker.preview(&mut p);
                p
            }
            State::Center => Preview {
                lines: self.cursor.map(|(c, a)| protractor(c, a, Self::zero(cx, a), self.color(cx, a))).unwrap_or_default(),
                inference: self.snap.clone(),
                ..Default::default()
            },
            // The radii from the centre, dashed, over everything as the protractor. The one to
            // the cursor is drawn last.
            State::Reference { center, axis, dir } => {
                let mut lines = protractor(center, axis, Self::zero(cx, axis), self.color(cx, axis));
                let color = self.snap.as_ref().map_or(INK, |i| guide_color(i.kind));
                if let Some((d, reach)) = dir {
                    dashed(cx.camera, cx.size, &mut lines, center, center + d * reach, color);
                }
                Preview { lines, inference: self.snap.clone(), ..Default::default() }
            }
            State::Angle { center, axis, reference, reference_reach, angle, reach } => {
                let mut lines = protractor(center, axis, reference, self.color(cx, axis));
                let r = protractor_radius(cx, center) * 1.4;
                let now = DQuat::from_axis_angle(axis, angle) * reference;
                let color = self.snap.as_ref().map_or(INK, |i| guide_color(i.kind));
                dashed(cx.camera, cx.size, &mut lines, center, center + reference * reference_reach, INK);
                dashed(cx.camera, cx.size, &mut lines, center, center + now * reach, color);
                // The angle, between the two radii, just past the protractor.
                let mid = DQuat::from_axis_angle(axis, angle / 2.0) * reference;
                // Not turned yet: only while the angle is typed.
                let (a, b) = (center + mid * r * 0.6, center + mid * r * 1.1);
                let text = vcb::format_angle(angle);
                let labels = vec![PreviewLabel { a, b, text, color, slot: Some(0), hidden: angle.abs() <= 1e-9, ..Default::default() }];
                Preview {
                    lines,
                    labels,
                    marks: vec![center],
                    ghost: Some(rotation_about(center, axis, angle)),
                    inference: self.snap.clone(),
                    ..Default::default()
                }
            }
        }
    }

    fn status(&self) -> String {
        let hint = match self.state {
            State::Center if self.last.is_some() && !self.picker.busy() => return t!("tool.rotate.hint.array"),
            State::Center if self.picking() => self.picker.status(t!("tool.rotate.hint.pick"), t!("tool.rotate.hint.gather")),
            State::Center => t!("tool.rotate.hint.center", copy_key = super::copy_key()),
            State::Reference { .. } => return t!("tool.rotate.hint.reference"),
            State::Angle { .. } => t!("tool.rotate.hint.angle"),
        };
        super::with_copying(hint, self.copy)
    }

    fn vcb(&self) -> Option<Vcb> {
        match self.state {
            State::Center => self.last.as_ref().map(|_| Vcb::new(t!("tool.rotate.vcb.copies"), VcbKind::Count)),
            State::Reference { .. } => Some(Vcb::new(t!("tool.rotate.vcb.angle"), VcbKind::Angle(0.0))),
            State::Angle { angle, .. } => Some(Vcb::new(t!("tool.rotate.vcb.angle"), VcbKind::Angle(angle))),
        }
    }

    fn copying(&self) -> bool {
        self.copy
    }

    fn options(&self, _: &Cx<'_>) -> Vec<ToolOption> {
        vec![ToolOption {
            key: "copy",
            label: t!("tool.rotate.option.copy"),
            tip: t!("tool.rotate.option.copy_tip", copy_key = super::copy_key()),
            value: OptionValue::Toggle(self.copy),
            choices: Vec::new(),
            saved: false,
            icon: None,
        }]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        if let ("copy", OptionValue::Toggle(on)) = (key, value) {
            self.copy = *on;
        }
    }

    /// Picking, a drag draws a selection rectangle; then, it places the centre at the press,
    /// the reference at the release.
    fn drag_clicks(&self) -> DragClicks {
        match self.state {
            State::Center if self.picking() => DragClicks::No,
            State::Center => DragClicks::PressAndRelease,
            _ => DragClicks::Release,
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.state, State::Center) || self.picker.busy()
    }

    fn picks_first(&self) -> bool {
        true
    }

    fn picks_parts(&self) -> bool {
        true
    }

    fn leave(&mut self) {
        self.picker.leave();
    }

    fn back(&mut self) {
        if !self.picker.back() {
            self.reset();
        }
    }

    fn reset(&mut self) {
        self.picker = Picker::default();
        self.state = State::Center;
        self.copy = false;
        self.last = None;
        self.moving.clear();
        self.moving_paths.clear();
        self.snap = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_keeps_the_centre() {
        let c = DVec3::new(10.0, 20.0, 0.0);
        let r = rotation_about(c, DVec3::Z, 1.0);
        assert!((r.transform_point3(c) - c).length() < 1e-12);
        let p = r.transform_point3(c + DVec3::X);
        assert!((p - (c + DVec3::new(1f64.cos(), 1f64.sin(), 0.0))).length() < 1e-12);
    }
}

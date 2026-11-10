//! Rotate (SketchUp's protractor): click the centre — the axis is the normal of the face under
//! it, else the context's Z; the arrow keys lock it (→ red X, ← green Y, ↑ blue Z, ↓ free) —
//! then a reference direction (0°), then the angle, which snaps to 15° steps. The angle can be
//! typed in degrees (`360/7`, `@/2`: half the angle so far). A tap of the copy modifier (Option on macOS, Ctrl elsewhere) toggles
//! copying; right after a copy, `6x` makes a circular array of 6 at the same angle and `/6`
//! spreads 6 copies over it. With nothing selected, the first click takes the entity under
//! the cursor.

use glam::{DAffine3, DQuat, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::Id;
use crate::scene::Owner;
use crate::infer::Inference;
use crate::vcb::{Vcb, VcbKind, VcbValue};

use super::{BLUE, Command, Cx, GREEN, INK, Key, LastCopy, Motion, OptionValue, Preview, PreviewLine, RED, Tool, ToolInput, ToolKind, ToolOption, cross, guide_color};

/// Snapping step and how close the angle must be to snap (radians).
const SNAP_STEP: f64 = std::f64::consts::PI / 12.0;
const SNAP_WITHIN: f64 = 2.0 * std::f64::consts::PI / 180.0;
/// Radius of the protractor, in pixels.
const PROTRACTOR_PX: f64 = 70.0;

#[derive(Default)]
pub struct RotateTool {
    state: State,
    copy: bool,
    moving: Vec<Id>,
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
    Reference { center: DVec3, axis: DVec3, dir: Option<DVec3> },
    Angle { center: DVec3, axis: DVec3, reference: DVec3, angle: f64 },
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

    /// The direction from `center` to the cursor, in the protractor's plane: towards a feature
    /// inferred (its projection on the plane) or along an axis or an edge's parallel lying in
    /// the plane, else where the cursor's ray meets the plane. While turning, what turns is not
    /// inferred on.
    fn direction(&self, cx: &Cx<'_>, px: DVec2, center: DVec3, axis: DVec3) -> (Option<DVec3>, Option<Inference>) {
        let turning = matches!(self.state, State::Angle { .. });
        let keep = |o: &Owner<'_>| !(turning && cx.is_moving(o, &self.moving));
        let inf = cx.infer_in_plane(px, keep, (center, axis), center).filter(|i| i.kind.is_feature());
        let p = match &inf {
            Some(i) => Some(i.point),
            None => cx.ray(px).hit_plane(center, axis),
        };
        let dir = p.and_then(|p| {
            let d = p - center;
            (d - axis * d.dot(axis)).try_normalize()
        });
        (dir, inf.filter(|_| dir.is_some()))
    }

    fn finish(&mut self, cx: &Cx<'_>, center: DVec3, axis: DVec3, angle: f64) -> Vec<Command> {
        let world = rotation_about(center, axis, angle);
        let entities = std::mem::take(&mut self.moving);
        let copy = self.copy;
        self.reset();
        let op = if copy {
            let last = LastCopy::new(cx.moved(), entities, Motion::Rotate { center, axis, angle });
            let op = last.op.clone();
            self.last = Some(last);
            op
        } else {
            cx.moved().transform(entities, world)
        };
        vec![Command::Apply(op)]
    }

    fn color(&self, cx: &Cx<'_>, axis: DVec3) -> [f32; 4] {
        [(0, RED), (1, GREEN), (2, BLUE)]
            .into_iter()
            .find(|(k, _)| context_axis(cx, *k).dot(axis).abs() > 0.9999)
            .map_or(INK, |(_, c)| c)
    }
}

impl Tool for RotateTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Rotate
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match (input, self.state) {
            (ToolInput::Hover { px }, State::Center) => {
                let c = self.center_at(cx, *px);
                self.cursor = c.as_ref().map(|(p, a, _)| (*p, *a));
                self.snap = c.map(|(_, _, i)| i);
            }
            (ToolInput::Hover { px }, State::Reference { center, axis, .. }) => {
                let (dir, snap) = self.direction(cx, *px, center, axis);
                self.state = State::Reference { center, axis, dir };
                self.snap = snap;
            }
            (ToolInput::Hover { px }, State::Angle { center, axis, reference, .. }) => {
                let (dir, snap) = self.direction(cx, *px, center, axis);
                if let Some(d) = dir {
                    let mut angle = axis.dot(reference.cross(d)).atan2(reference.dot(d));
                    // Steps of 15°, unless a feature gives the angle.
                    let snapped = (angle / SNAP_STEP).round() * SNAP_STEP;
                    if snap.is_none() && (angle - snapped).abs() < SNAP_WITHIN {
                        angle = snapped;
                    }
                    self.state = State::Angle { center, axis, reference, angle };
                }
                self.snap = snap;
            }
            (ToolInput::Key { key: Key::Copy, .. }, _) => self.copy = !self.copy,
            (ToolInput::Click { px, .. }, State::Center) => {
                self.last = None;
                let mut out = vec![];
                self.moving = cx.selection.to_vec();
                if self.moving.is_empty() {
                    let inf = cx.infer(*px, |_| true, None);
                    match cx.entity_under(*px, inf.as_ref()) {
                        Some(id) => {
                            self.moving = vec![id];
                            out.push(Command::Select(vec![id]));
                        }
                        None => return vec![Command::Say(t!("tool.rotate.say.nothing"))],
                    }
                }
                let Some((center, axis, _)) = self.center_at(cx, *px) else { return out };
                self.state = State::Reference { center, axis, dir: None };
                return out;
            }
            (ToolInput::Click { .. }, State::Reference { center, axis, dir }) => {
                let Some(reference) = dir else {
                    return vec![Command::Say(t!("tool.rotate.say.too_close"))];
                };
                self.state = State::Angle { center, axis, reference, angle: 0.0 };
            }
            (ToolInput::Click { .. }, State::Angle { center, axis, angle, .. }) => {
                return self.finish(cx, center, axis, angle);
            }
            (ToolInput::Value(VcbValue::Count(r)), State::Center) => {
                let Some(last) = &mut self.last else { return vec![] };
                return last.retype(*r);
            }
            (
                ToolInput::Value(VcbValue::Angle(angle)),
                State::Reference { center, axis, .. } | State::Angle { center, axis, .. },
            ) => {
                return self.finish(cx, center, axis, *angle);
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let protractor = |center: DVec3, axis: DVec3, color: [f32; 4]| -> Vec<PreviewLine> {
            let r = cx.camera.world_per_pixel(cx.size, center) * PROTRACTOR_PX;
            let u = axis.any_orthonormal_vector();
            let v = axis.cross(u);
            const N: usize = 48;
            let p = |i: usize| {
                let a = i as f64 / N as f64 * std::f64::consts::TAU;
                center + (u * a.cos() + v * a.sin()) * r
            };
            let mut lines: Vec<PreviewLine> = (0..N).map(|i| PreviewLine { a: p(i), b: p(i + 1), color }).collect();
            // Ticks every 15°.
            for k in 0..24 {
                let a = k as f64 * SNAP_STEP;
                let d = u * a.cos() + v * a.sin();
                let inner = if k % 6 == 0 { 0.8 } else { 0.9 };
                lines.push(PreviewLine { a: center + d * r * inner, b: center + d * r, color });
            }
            lines
        };
        let radius = |c: DVec3| cx.camera.world_per_pixel(cx.size, c) * PROTRACTOR_PX * 1.4;
        match self.state {
            State::Center => Preview {
                lines: self.cursor.map(|(c, a)| protractor(c, a, self.color(cx, a))).unwrap_or_default(),
                inference: self.snap.clone(),
                ..Default::default()
            },
            State::Reference { center, axis, dir } => {
                let mut lines = protractor(center, axis, self.color(cx, axis));
                if let Some(d) = dir {
                    let color = self.snap.as_ref().map_or(INK, |i| guide_color(i.kind));
                    lines.push(PreviewLine { a: center, b: center + d * radius(center), color });
                }
                Preview { lines, inference: self.snap.clone(), ..Default::default() }
            }
            State::Angle { center, axis, reference, angle } => {
                let mut lines = protractor(center, axis, self.color(cx, axis));
                let r = radius(center);
                let now = DQuat::from_axis_angle(axis, angle) * reference;
                lines.push(PreviewLine { a: center, b: center + reference * r, color: INK });
                let color = self.snap.as_ref().map_or(INK, |i| guide_color(i.kind));
                lines.push(PreviewLine { a: center, b: center + now * r, color });
                lines.extend(cross(cx, center, INK));
                Preview {
                    lines,
                    ghost: Some(rotation_about(center, axis, angle)),
                    inference: self.snap.clone(),
                    ..Default::default()
                }
            }
        }
    }

    fn status(&self) -> String {
        let hint = match self.state {
            State::Center if self.last.is_some() => return t!("tool.rotate.hint.array"),
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
        }]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        if let ("copy", OptionValue::Toggle(on)) = (key, value) {
            self.copy = *on;
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.state, State::Center)
    }

    fn reset(&mut self) {
        self.state = State::Center;
        self.copy = false;
        self.last = None;
        self.moving.clear();
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

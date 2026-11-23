//! « Cadrer la vue » (docs/ui/drawing.md#framing-a-view, decision 0051): a view's subject seen
//! large over the stage, turned by the pointer or put on a standard direction, what its frame will
//! hold unmasked. It works on a copy of the view's look: neither the 3D view's camera nor its
//! selection moves, and validating is one undoable change.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use glam::{DAffine3, DMat4, DQuat, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{Direction, Document, ElementKind, Frame, Id, Look, Operation, Perspective, Subject, View, ViewKind};
use skraft_tools::Editor;
use skraft_tools::camera::{Camera, Projection};

use crate::sheet::{Placed, SheetState, ViewSlot, Wanted};
use crate::view_picker::first_path;

/// The picture's slot: no view has this id, so the view's own picture stays as it is.
const SLOT: ViewSlot = (Id(u64::MAX), None);
/// Radians per point dragged, the 3D view's.
const ORBIT_SPEED: f64 = 0.006;
/// Room around the subject's sphere, and how far the wheel zooms.
const MARGIN: f64 = 1.08;
const ZOOM: (f64, f64) = (0.05, 20.0);
const MAX_PICTURE: f32 = 4096.0;
const DIRECTIONS: [(Direction, &str); 7] = [
    (Direction::Front, "front"),
    (Direction::Back, "back"),
    (Direction::Left, "left"),
    (Direction::Right, "right"),
    (Direction::Top, "top"),
    (Direction::Bottom, "bottom"),
    (Direction::Iso, "iso"),
];

/// A view being framed (never saved).
#[derive(Clone, Debug)]
pub struct ViewFramer {
    /// The base view changed.
    view: Id,
    look: Look,
    /// Its subject's parts, by definition, and the corners of their boxes, in its axes.
    parts: Vec<(Id, Vec<Placed>)>,
    corners: Vec<DVec3>,
    centre: DVec3,
    radius: f64,
    zoom: f64,
    pan: DVec2,
    /// What the picture last asked for shows.
    stamp: Option<u64>,
}

/// The base view of `id`: itself, or the one it is projected from.
fn base_of(e: &Editor, mut id: Id) -> Option<Id> {
    let d = e.document().drawing();
    for _ in 0..64 {
        match &d.element(id)?.kind {
            ElementKind::View(View { kind: ViewKind::Base { .. }, .. }) => return Some(id),
            ElementKind::View(View { kind: ViewKind::Projected { base, .. }, .. }) => id = *base,
            _ => return None,
        }
    }
    None
}

/// Opens the framing of view `id` (a projected view's: of its base).
pub fn open(e: &Editor, st: &mut SheetState, id: Id) -> Result<(), String> {
    let view = base_of(e, id).ok_or_else(|| t!("drawing.no_view_selected"))?;
    let Some(ElementKind::View(View { kind: ViewKind::Base { subject, look, .. }, .. })) =
        e.document().drawing().element(view).map(|el| &el.kind)
    else {
        return Err(t!("drawing.no_view_selected"));
    };
    let mut by_part: HashMap<Id, Vec<Placed>> = HashMap::new();
    let mut corners = Vec::new();
    for p in e.document().subject_parts(subject) {
        if let Some((lo, hi)) = e.scene().bounds_of(&p.shape) {
            corners.extend((0..8).map(|i| {
                let c = DVec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                );
                p.transform.transform_point3(c)
            }));
        }
        by_part.entry(p.part).or_default().push((p.transform, p.material));
    }
    let Some((lo, hi)) =
        corners.iter().fold(None, |b: Option<(DVec3, DVec3)>, c| Some(b.map_or((*c, *c), |(a, z)| (a.min(*c), z.max(*c)))))
    else {
        return Err(t!("drawing.framer.empty"));
    };
    let mut parts: Vec<_> = by_part.into_iter().collect();
    parts.sort_by_key(|(p, _)| *p);
    st.framer = Some(ViewFramer {
        view,
        look: look.clone(),
        parts,
        corners,
        centre: (lo + hi) / 2.0,
        radius: ((hi - lo).length() / 2.0).max(1e-3),
        zoom: 1.0,
        pan: DVec2::ZERO,
        stamp: None,
    });
    Ok(())
}

/// `frame` turned by a drag of `delta` points, as the 3D view's orbit: about Z, then about its
/// right, stopping short of going over the top. Also the rotation made.
pub fn orbit(frame: Frame, delta: DVec2) -> (Frame, DQuat) {
    let yaw = DQuat::from_rotation_z(-delta.x * ORBIT_SPEED);
    let (right, up, forward) = (yaw * frame.right, yaw * frame.up, yaw * frame.forward);
    let pitch = DQuat::from_axis_angle(right.normalize(), -delta.y * ORBIT_SPEED);
    let tilted = pitch * up;
    if tilted.z < 0.0 && tilted.z < up.z {
        return (Frame { right, up, forward }, yaw);
    }
    (Frame { right, up: tilted, forward: pitch * forward }, pitch * yaw)
}

/// A look along `forward`, `up`, from a camera in axes `to_subject` takes to the subject's.
fn look_in(to_subject: DAffine3, forward: DVec3, up: DVec3, perspective: Option<(DVec3, f64)>) -> Look {
    Look::Camera {
        forward: to_subject.transform_vector3(forward).normalize(),
        up: to_subject.transform_vector3(up).normalize(),
        perspective: perspective.map(|(eye, fov_y)| Perspective { eye: to_subject.transform_point3(eye), fov_y }),
    }
}

/// The 3D camera as a look at `subject`: a component's view is in its own axes, so the camera is
/// taken there through its first occurrence.
pub fn camera_look(doc: &Document, subject: &Subject, camera: &Camera) -> Look {
    let to_subject = match subject {
        Subject::Component(c) => first_path(doc, doc.root(), *c, &mut Vec::new())
            .and_then(|p| {
                let chain: Vec<Id> = p.instances.iter().copied().chain([p.entity]).collect();
                doc.transform_of(&chain).ok()
            })
            .map_or(DAffine3::IDENTITY, |t| t.inverse()),
        _ => DAffine3::IDENTITY,
    };
    let perspective = match camera.projection {
        Projection::Perspective { fov_y } => Some((camera.eye, fov_y)),
        _ => None,
    };
    look_in(to_subject, camera.forward(), camera.up(), perspective)
}

impl ViewFramer {
    /// Subject axes to eye and the projection (reversed depth), `half` wide and high.
    fn camera(&self, frame: &Frame, at: DVec2, half: DVec2) -> (DMat4, glam::Mat4) {
        let depth = self.centre.dot(frame.forward);
        let point = frame.right * at.x + frame.up * at.y + frame.forward * depth;
        let back = 2.0 * self.radius + 1.0;
        let eye = point - frame.forward * back;
        let view = glam::dcamera::rh::view::look_to_mat4(eye, frame.forward, frame.up);
        let (near, far) = ((back - self.radius * 1.05).max(1e-3), back + self.radius * 1.05);
        let proj = glam::camera::rh::proj::directx::orthographic(
            -half.x as f32,
            half.x as f32,
            -half.y as f32,
            half.y as f32,
            far as f32,
            near as f32,
        );
        (view, proj)
    }

    fn turn(&mut self, frame: Frame, delta: DVec2) {
        let (turned, q) = orbit(frame, delta);
        let perspective = self.look.frame().1.map(|p| Perspective { eye: self.centre + q * (p.eye - self.centre), ..p });
        self.look = Look::Camera { forward: turned.forward, up: turned.up, perspective };
    }

    /// The view with this look, if it changes it.
    fn change(&self, e: &Editor) -> Option<Operation> {
        let ElementKind::View(view) = &e.document().drawing().element(self.view)?.kind else { return None };
        let ViewKind::Base { subject, look, scale, at } = &view.kind else { return None };
        (*look != self.look).then(|| Operation::SetElement {
            element: self.view,
            kind: ElementKind::View(View {
                kind: ViewKind::Base { subject: subject.clone(), look: self.look.clone(), scale: *scale, at: *at },
                ..view.clone()
            }),
        })
    }
}

/// The stage while a view is framed, over `room`: what it asks the renderer to draw.
pub fn ui(
    ui: &mut egui::Ui,
    room: egui::Rect,
    e: &mut Editor,
    st: &mut SheetState,
    ready: &HashMap<ViewSlot, egui::TextureId>,
) -> Vec<Wanted> {
    let Some(mut f) = st.framer.take() else { return Vec::new() };
    if base_of(e, f.view) != Some(f.view) {
        return Vec::new();
    }
    let response = ui.interact(room, egui::Id::unique("view_framer"), egui::Sense::click_and_drag());
    crate::handles::publish(|| "view_framer".into(), room);
    let frame = f.look.frame().0;
    let height = f.radius * MARGIN * f.zoom;
    let per_unit = room.height() as f64 / (2.0 * height);
    let drag = response.drag_delta();
    let delta = DVec2::new(drag.x as f64, drag.y as f64);
    if delta != DVec2::ZERO {
        if response.dragged_by(egui::PointerButton::Secondary) {
            f.pan += DVec2::new(-delta.x, delta.y) / per_unit;
        } else {
            f.turn(frame, delta);
        }
    }
    if response.hovered() {
        let (wheel, pinch) = ui.input(|i| (i.smooth_scroll_delta.y as f64, i.zoom_delta() as f64));
        f.zoom = (f.zoom * 0.85f64.powf(wheel / 40.0) / pinch).clamp(ZOOM.0, ZOOM.1);
        let grabbing = response.dragged();
        ui.ctx().set_cursor_icon(if grabbing { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    let frame = f.look.frame().0;
    let height = f.radius * MARGIN * f.zoom;
    let per_unit = room.height() as f64 / (2.0 * height);
    let half = DVec2::new(height * (room.width() / room.height().max(1.0)) as f64, height);
    let at = f.centre.dot(frame.right) * DVec2::X + f.centre.dot(frame.up) * DVec2::Y + f.pan;

    // The picture.
    let ppp = ui.ctx().pixels_per_point();
    let side = |points: f32| (points * ppp).round().clamp(8.0, MAX_PICTURE) as u32;
    let size = [side(room.width()), side(room.height())];
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (e.history().revision(), size).hash(&mut h);
    for v in [frame.right, frame.up, frame.forward] {
        v.to_array().map(f64::to_bits).hash(&mut h);
    }
    at.to_array().map(f64::to_bits).hash(&mut h);
    half.to_array().map(f64::to_bits).hash(&mut h);
    let stamp = h.finish();
    let painter = ui.painter().with_clip_rect(room);
    painter.rect_filled(room, 0.0, egui::Color32::WHITE);
    if f.stamp.is_some()
        && let Some(texture) = ready.get(&SLOT)
    {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(*texture, room, uv, egui::Color32::WHITE);
    }
    let mut wanted = Vec::new();
    if f.stamp != Some(stamp) || !ready.contains_key(&SLOT) {
        f.stamp = Some(stamp);
        wanted.push(Wanted { slot: SLOT, size, camera: f.camera(&frame, at, half), parts: f.parts.clone(), lines: None, stamp });
        ui.ctx().request_repaint();
    }

    // What the view's frame will hold: the rest masked.
    let (lo, hi) = f.corners.iter().fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), c| {
        let q = frame.project(*c).truncate();
        (lo.min(q), hi.max(q))
    });
    let on_screen = |q: DVec2| room.center() + egui::vec2(((q.x - at.x) * per_unit) as f32, ((at.y - q.y) * per_unit) as f32);
    let window = egui::Rect::from_two_pos(on_screen(lo), on_screen(hi));
    let mask = egui::Color32::from_black_alpha(46);
    for r in [
        egui::Rect::from_min_max(room.min, egui::pos2(room.max.x, window.min.y)),
        egui::Rect::from_min_max(egui::pos2(room.min.x, window.max.y), room.max),
        egui::Rect::from_min_max(egui::pos2(room.min.x, window.min.y), egui::pos2(window.min.x, window.max.y)),
        egui::Rect::from_min_max(egui::pos2(window.max.x, window.min.y), egui::pos2(room.max.x, window.max.y)),
    ] {
        if r.is_positive() {
            painter.rect_filled(r, 0.0, mask);
        }
    }
    painter.rect_stroke(window, 0.0, egui::Stroke::new(1.5, ui.visuals().selection.stroke.color), egui::StrokeKind::Outside);

    // The bar.
    let (mut done, mut cancel) = (false, false);
    egui::Area::new(egui::Id::unique("view_framer_bar"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(room.center_bottom() - egui::vec2(0.0, 14.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::window(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (d, key) in DIRECTIONS {
                        let on = f.look == Look::Standard(d);
                        let b = ui.selectable_label(on, crate::panels::direction_label(d));
                        crate::handles::publish(|| format!("view_framer.dir:{key}"), b.rect);
                        if b.clicked() {
                            f.look = Look::Standard(d);
                            (f.zoom, f.pan) = (1.0, DVec2::ZERO);
                        }
                    }
                    ui.separator();
                    let c = ui.button(t!("naming.cancel"));
                    crate::handles::publish(|| "view_framer.cancel".into(), c.rect);
                    cancel |= c.clicked();
                    let ok = ui.add(egui::Button::new(t!("drawing.framer.ok")).fill(ui.visuals().selection.bg_fill));
                    crate::handles::publish(|| "view_framer.ok".into(), ok.rect);
                    done |= ok.clicked();
                });
            });
        });
    let free = ui.ctx().memory(|m| m.focused().is_none());
    done |= free && ui.input(|i| i.key_pressed(egui::Key::Enter));
    cancel |= free && ui.input(|i| i.key_pressed(egui::Key::Escape));
    if cancel {
        return wanted;
    }
    if done {
        if let Some(op) = f.change(e) {
            let _ = e.apply(op);
        }
        return wanted;
    }
    st.framer = Some(f);
    wanted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_down_raises_the_eye_and_stops_short_of_going_over() {
        let front = Frame::of(Direction::Front);
        let (down, _) = orbit(front, DVec2::new(0.0, 100.0));
        assert!(down.forward.z < -0.1, "the eye rose: it looks down");
        assert!(down.up.z > 0.0);
        let top = Frame::of(Direction::Top);
        let (over, _) = orbit(top, DVec2::new(0.0, 100.0));
        assert!((over.forward - top.forward).length() < 1e-9, "not past the top");
        let (back, _) = orbit(top, DVec2::new(0.0, -100.0));
        assert!(back.forward.z > top.forward.z && back.up.z > 0.0, "down from the top");
    }

    #[test]
    fn dragging_across_keeps_the_view_level() {
        let (turned, _) = orbit(Frame::of(Direction::Front), DVec2::new(80.0, 0.0));
        assert!(turned.right.z.abs() < 1e-12 && turned.forward.z.abs() < 1e-12);
    }

    #[test]
    fn a_camera_is_taken_into_a_turned_occurrence_axes() {
        // An occurrence turned a quarter about Z: what the world sees along +Y, its axes see
        // along +X... its inverse brings the camera there.
        let placed = DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
        let Look::Camera { forward, up, perspective } =
            look_in(placed.inverse(), DVec3::Y, DVec3::Z, Some((DVec3::new(0.0, -10.0, 0.0), 0.7)))
        else {
            unreachable!()
        };
        assert!((forward - DVec3::X).length() < 1e-9 && (up - DVec3::Z).length() < 1e-9);
        assert!((perspective.unwrap().eye - DVec3::new(-10.0, 0.0, 0.0)).length() < 1e-9);
    }
}

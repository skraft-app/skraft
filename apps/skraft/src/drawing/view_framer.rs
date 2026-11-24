//! « Cadrer la vue » (docs/ui/drawing.md#framing-a-view, decisions 0051, 0052): a view's frame
//! held still over the stage and its subject moved inside it: turned about the point pressed,
//! zoomed at the pointer, slid, or seen from elsewhere. It works on a copy of the view: neither
//! the 3D view's camera nor its selection moves, and validating is one undoable change.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{DAffine3, DMat4, DQuat, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{Crop, Direction, Document, ElementKind, Frame, Id, Look, Operation, Perspective, Subject, View, ViewKind};
use skraft_tools::Editor;
use skraft_tools::camera::{Camera, Projection};

use crate::drawing::sheet::{Placed, SCALES, SheetState, ViewSlot, Wanted, nearest_scale, scale_label, window_matrix};
use crate::drawing::view_picker::first_path;
use crate::kit::widgets::{Confirm, confirm_buttons};

/// The picture's slot: no view has this id, so the view's own picture stays as it is.
const SLOT: ViewSlot = (Id(u64::MAX), None);
/// Radians per point dragged, the 3D view's.
const ORBIT_SPEED: f64 = 0.006;
const MAX_PICTURE: f32 = 4096.0;
/// The wheel's points that double the size seen, and how long after it stops a parallel view's
/// scale settles on a standard one (seconds).
const WHEEL_DOUBLES: f64 = 400.0;
const SETTLE_S: f64 = 0.25;
/// What stays of the subject in the frame, at least: this much of its size, or of the frame's.
const KEPT: f64 = 0.25;
/// The film a focal length is read on (millimetres high), the focal a perspective starts at,
/// and the range offered.
const FILM_HEIGHT: f64 = 24.0;
const DEFAULT_FOCAL: f64 = 50.0;
const FOCAL: (f64, f64) = (14.0, 200.0);
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
    /// The texture images they show.
    images: Arc<[String]>,
    corners: Vec<DVec3>,
    centre: DVec3,
    radius: f64,
    /// The view's scale, and whether views are projected from it (then it stays parallel).
    scale: f64,
    projected: bool,
    /// Its frame, paper millimetres from the point of the sheet its subject hangs on.
    crop: Crop,
    /// Parallel, nothing placed by hand: validated, its frame is the subject's box again.
    auto: bool,
    /// The frame and state it had before going to perspective, for coming back.
    parallel: Option<(Crop, bool)>,
    /// The middle of its frame when opened: the frame stays there on the sheet.
    start_mid: DVec2,
    /// The point of the subject the drag turns about.
    pivot: Option<DVec3>,
    /// The wheel under way in parallel: when it last turned, the point of the paper under it,
    /// and the scale and state it started from (back on that scale, nothing was changed).
    zooming: Option<(f64, DVec2, f64, bool)>,
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
    let Some(ElementKind::View(v)) = e.document().drawing().element(view).map(|el| &el.kind) else {
        return Err(t!("drawing.no_view_selected"));
    };
    let ViewKind::Base { subject, look, scale, .. } = &v.kind else {
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
    let images = crate::drawing::sheet::images_of(e, &parts);
    let mut f = ViewFramer {
        view,
        look: look.clone(),
        parts,
        images,
        corners,
        centre: (lo + hi) / 2.0,
        radius: ((hi - lo).length() / 2.0).max(1e-3),
        scale: *scale,
        projected: e.document().drawing().has_projected_views(view),
        crop: Crop { lo: -DVec2::ONE, hi: DVec2::ONE },
        auto: v.crop.is_none() && look.frame().1.is_none(),
        parallel: None,
        start_mid: DVec2::ZERO,
        pivot: None,
        zooming: None,
        stamp: None,
    };
    f.crop = v.crop.unwrap_or_else(|| f.full());
    f.start_mid = f.crop.middle();
    st.framer = Some(f);
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
fn look_in(to_subject: DAffine3, forward: DVec3, up: DVec3, perspective: Option<(DVec3, f64)>, size: DVec2) -> Look {
    Look::Camera {
        forward: to_subject.transform_vector3(forward).normalize(),
        up: to_subject.transform_vector3(up).normalize(),
        perspective: perspective.map(|(eye, fov_y)| Perspective { eye: to_subject.transform_point3(eye), fov_y, size }),
    }
}

/// The 3D camera as a look at `subject`: a component's view is in its own axes, so the camera is
/// taken there through its first occurrence.
pub fn camera_look(doc: &Document, subject: &Subject, camera: &Camera, size: DVec2) -> Look {
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
    look_in(to_subject, camera.forward(), camera.up(), perspective, size)
}

/// The eye seeing every one of `corners` within a frame of `aspect`, opening `fov_y`, along
/// `frame`, centred across on their box.
fn eye_fitting(corners: &[DVec3], frame: &Frame, fov_y: f64, aspect: f64) -> DVec3 {
    let seen: Vec<DVec3> = corners.iter().map(|c| frame.project(*c)).collect();
    let (lo, hi) = seen.iter().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(a, z), q| (a.min(*q), z.max(*q)));
    let middle = (lo + hi) / 2.0;
    let t = (fov_y / 2.0).tan();
    let depth =
        seen.iter().map(|q| q.z - ((q.x - middle.x).abs() / (t * aspect)).max((q.y - middle.y).abs() / t)).fold(f64::INFINITY, f64::min);
    frame.right * middle.x + frame.up * middle.y + frame.forward * depth
}

/// The opening of a focal length (millimetres, on a 24 mm high frame), and back.
fn fov_of(focal: f64) -> f64 {
    2.0 * (FILM_HEIGHT / 2.0 / focal).atan()
}

fn focal_of(fov_y: f64) -> f64 {
    FILM_HEIGHT / 2.0 / (fov_y / 2.0).tan()
}

/// The distance along `dir` from `o` to triangle `a`, `b`, `c`, if it is hit.
fn hit_triangle(o: DVec3, dir: DVec3, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let s = o - a;
    let u = s.dot(p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(e2.dot(q) / det).filter(|t| *t > 0.0)
}

/// Does the ray from `o` along `dir` meet box `lo`–`hi`?
fn hits_box(o: DVec3, dir: DVec3, lo: DVec3, hi: DVec3) -> bool {
    let (mut near, mut far) = (0.0f64, f64::MAX);
    for k in 0..3 {
        if dir[k].abs() < 1e-14 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return false;
            }
            continue;
        }
        let (a, b) = ((lo[k] - o[k]) / dir[k], (hi[k] - o[k]) / dir[k]);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    near <= far
}

/// How the stage shows the frame: as large as it fits, still.
#[derive(Clone, Copy)]
struct Stage {
    room: egui::Rect,
    window: egui::Rect,
    /// Points per paper millimetre.
    per_mm: f64,
    /// The middle of the frame on the paper.
    mid: DVec2,
}

impl Stage {
    fn of(crop: Crop, room: egui::Rect) -> Stage {
        // Clear of the bar at the bottom.
        let inner = egui::Rect::from_min_max(room.min + egui::vec2(24.0, 24.0), room.max - egui::vec2(24.0, 112.0));
        let inner = if inner.is_positive() { inner } else { room };
        let size = crop.size().max(DVec2::splat(1e-6));
        let per_mm = (inner.width() as f64 / size.x).min(inner.height() as f64 / size.y);
        let window = egui::Rect::from_center_size(inner.center(), egui::vec2((size.x * per_mm) as f32, (size.y * per_mm) as f32));
        Stage { room, window, per_mm, mid: crop.middle() }
    }

    /// A point on screen, in paper millimetres from the point the subject hangs on.
    fn to_paper(self, p: egui::Pos2) -> DVec2 {
        let c = self.window.center();
        self.mid + DVec2::new((p.x - c.x) as f64, (c.y - p.y) as f64) / self.per_mm
    }
}

impl ViewFramer {
    /// The box of the subject's corners across `frame`.
    fn extent(&self, frame: &Frame) -> (DVec2, DVec2) {
        self.corners.iter().fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), c| {
            let q = frame.project(*c).truncate();
            (lo.min(q), hi.max(q))
        })
    }

    fn box_mid(&self, frame: &Frame) -> DVec2 {
        let (lo, hi) = self.extent(frame);
        (lo + hi) / 2.0
    }

    /// The frame showing the whole subject (a perspective's: its own).
    fn full(&self) -> Crop {
        let (frame, lens) = self.look.frame();
        let half = match lens {
            Some(p) => p.size / 2.0,
            None => {
                let (lo, hi) = self.extent(&frame);
                ((hi - lo) * self.scale / 2.0).max(DVec2::splat(0.5))
            }
        };
        Crop { lo: -half, hi: half }
    }

    /// Subject axes to eye, and the projection (reversed depth), of what `stage` shows.
    fn camera(&self, stage: &Stage) -> (DMat4, glam::Mat4) {
        let (frame, lens) = self.look.frame();
        let (lo, hi) = (stage.to_paper(stage.room.left_bottom()), stage.to_paper(stage.room.right_top()));
        if let Some(p) = lens {
            let from_eye = (self.centre - p.eye).dot(frame.forward);
            let near = (from_eye - self.radius * 1.05).max(from_eye.abs() * 1e-3).max(1e-3);
            let view = glam::dcamera::rh::view::look_to_mat4(p.eye, frame.forward, frame.up);
            let aspect = p.size.x / p.size.y;
            let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(p.fov_y as f32, aspect as f32, near as f32);
            return (view, window_matrix(lo / (p.size / 2.0), hi / (p.size / 2.0)) * proj);
        }
        let at = self.box_mid(&frame) + (lo + hi) / 2.0 / self.scale;
        let half = (hi - lo) / 2.0 / self.scale;
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

    /// The ray under a point of the stage, in the subject's axes.
    fn ray(&self, stage: &Stage, p: egui::Pos2) -> (DVec3, DVec3) {
        let (frame, lens) = self.look.frame();
        let q = stage.to_paper(p);
        match lens {
            Some(l) => {
                let t = (l.fov_y / 2.0).tan();
                let tangent = q / (l.size / 2.0) * DVec2::new(t * l.size.x / l.size.y, t);
                (l.eye, (frame.right * tangent.x + frame.up * tangent.y + frame.forward).normalize())
            }
            None => {
                let m = self.box_mid(&frame) + q / self.scale;
                let depth = self.centre.dot(frame.forward) - 2.0 * self.radius - 1.0;
                (frame.right * m.x + frame.up * m.y + frame.forward * depth, frame.forward)
            }
        }
    }

    /// The point of the subject under `p`; off it, on the plane through its centre.
    fn point_at(&self, e: &Editor, stage: &Stage, p: egui::Pos2) -> DVec3 {
        let (o, dir) = self.ray(stage, p);
        let mut best: Option<f64> = None;
        for (id, places) in &self.parts {
            let Some(dm) = e.scene().definition_mesh(*id) else { continue };
            for pm in &dm.parts {
                let m = &pm.mesh;
                for (place, _) in places {
                    if let Some((lo, hi)) = pm.bounds {
                        let (mut a, mut z) = (DVec3::INFINITY, DVec3::NEG_INFINITY);
                        for i in 0..8 {
                            let c = DVec3::new(
                                if i & 1 == 0 { lo.x } else { hi.x },
                                if i & 2 == 0 { lo.y } else { hi.y },
                                if i & 4 == 0 { lo.z } else { hi.z },
                            );
                            let c = place.transform_point3(c);
                            (a, z) = (a.min(c), z.max(c));
                        }
                        if !hits_box(o, dir, a, z) {
                            continue;
                        }
                    }
                    let t = *place * DAffine3::from_translation(m.origin);
                    let at = |i: u32| t.transform_point3(DVec3::from(m.positions[i as usize].map(f64::from)));
                    for tri in m.indices.chunks_exact(3) {
                        if let Some(d) = hit_triangle(o, dir, at(tri[0]), at(tri[1]), at(tri[2]))
                            && best.is_none_or(|b| d < b)
                        {
                            best = Some(d);
                        }
                    }
                }
            }
        }
        let forward = self.look.frame().0.forward;
        let d = best.unwrap_or_else(|| {
            let d = (self.centre - o).dot(forward) / dir.dot(forward);
            if d > 0.0 { d } else { self.radius }
        });
        o + dir * d
    }

    fn set_lens(&mut self, lens: Perspective) {
        if let Look::Camera { perspective, .. } = &mut self.look {
            *perspective = Some(lens);
        }
    }

    /// Where the subject lies on the paper, from the point it hangs on; none with the eye among it.
    fn subject_on_paper(&self) -> Option<(DVec2, DVec2)> {
        let (frame, lens) = self.look.frame();
        let Some(p) = lens else {
            let (lo, hi) = self.extent(&frame);
            let half = (hi - lo) * self.scale / 2.0;
            return Some((-half, half));
        };
        let to_paper = p.size.y / (2.0 * (p.fov_y / 2.0).tan());
        let (mut lo, mut hi) = (DVec2::INFINITY, DVec2::NEG_INFINITY);
        for c in &self.corners {
            let q = frame.project(*c - p.eye);
            if q.z <= 1e-6 {
                return None;
            }
            let on_paper = q.truncate() / q.z * to_paper;
            (lo, hi) = (lo.min(on_paper), hi.max(on_paper));
        }
        Some((lo, hi))
    }

    /// The frame brought back, if need be, so that a part of the subject stays in it.
    fn keep_in_frame(&mut self) {
        let Some((lo, hi)) = self.subject_on_paper() else { return };
        let kept = ((hi - lo) * KEPT).min(self.crop.size() * KEPT);
        let mut d = DVec2::ZERO;
        for k in 0..2 {
            if self.crop.lo[k] > hi[k] - kept[k] {
                d[k] = hi[k] - kept[k] - self.crop.lo[k];
            } else if self.crop.hi[k] < lo[k] + kept[k] {
                d[k] = lo[k] + kept[k] - self.crop.hi[k];
            }
        }
        self.crop = Crop { lo: self.crop.lo + d, hi: self.crop.hi + d };
    }

    /// The picture slid by a drag of `delta` points, the point of view unchanged.
    fn slide(&mut self, delta: DVec2, stage: &Stage) {
        self.auto = false;
        let d = DVec2::new(delta.x, -delta.y) / stage.per_mm;
        self.crop = Crop { lo: self.crop.lo - d, hi: self.crop.hi - d };
        self.keep_in_frame();
    }

    /// The eye moved across by a drag of `delta` points; in parallel it is the same as sliding.
    fn move_eye(&mut self, delta: DVec2, stage: &Stage) {
        let (frame, lens) = self.look.frame();
        let Some(p) = lens else { return self.slide(delta, stage) };
        let from_eye = (self.centre - p.eye).dot(frame.forward).abs().max(1e-3);
        let unit = 2.0 * from_eye * (p.fov_y / 2.0).tan() / (p.size.y * stage.per_mm);
        self.set_lens(Perspective { eye: p.eye + (frame.up * delta.y - frame.right * delta.x) * unit, ..p });
        self.keep_in_frame();
    }

    /// Turned by a drag of `delta` points about the point pressed, which stays where it is in the
    /// frame; the frame keeps its size.
    fn turn(&mut self, delta: DVec2) {
        let (frame, lens) = self.look.frame();
        let (turned, q) = orbit(frame, delta);
        let pivot = self.pivot.unwrap_or(self.centre);
        if let Some(p) = lens {
            let lens = Perspective { eye: pivot + q * (p.eye - pivot), ..p };
            self.look = Look::Camera { forward: turned.forward, up: turned.up, perspective: Some(lens) };
        } else {
            let before = (frame.project(pivot).truncate() - self.box_mid(&frame)) * self.scale;
            self.look = Look::Camera { forward: turned.forward, up: turned.up, perspective: None };
            let after = (turned.project(pivot).truncate() - self.box_mid(&turned)) * self.scale;
            self.crop = Crop { lo: self.crop.lo + after - before, hi: self.crop.hi + after - before };
        }
        self.keep_in_frame();
    }

    /// At scale `s`, the point `q` of the paper (at the present scale) staying where it is in the
    /// frame; where that point is now.
    fn set_scale(&mut self, s: f64, q: DVec2) -> DVec2 {
        let k = s / self.scale;
        let moved = q * (k - 1.0);
        self.crop = Crop { lo: self.crop.lo + moved, hi: self.crop.hi + moved };
        self.scale = s;
        q * k
    }

    /// The wheel at `p`, at time `now`: the eye nearer the point there; in parallel a scale
    /// about it, which settles on a standard one when the wheel stops.
    fn zoom_at(&mut self, e: &Editor, stage: &Stage, p: egui::Pos2, wheel: f64, now: f64) {
        let k = 2f64.powf(wheel / WHEEL_DOUBLES);
        if let Some(l) = self.look.frame().1 {
            let target = self.point_at(e, stage, p);
            self.set_lens(Perspective { eye: target + (l.eye - target) / k, ..l });
            self.keep_in_frame();
            return;
        }
        let (largest, smallest) = (SCALES[0], SCALES[SCALES.len() - 1]);
        let s = (self.scale * k).clamp(smallest, largest);
        let q = stage.to_paper(p);
        if s != self.scale {
            let (from, was_auto) = self.zooming.map_or((self.scale, self.auto), |z| (z.2, z.3));
            self.auto = false;
            let q = self.set_scale(s, q);
            self.zooming = Some((now, q, from, was_auto));
            self.keep_in_frame();
        }
    }

    /// The scale on the nearest standard one, once the wheel has stopped (or `now`, by `force`).
    fn settle(&mut self, now: f64, force: bool) {
        let Some((at, q, from, was_auto)) = self.zooming else { return };
        if force || now - at >= SETTLE_S {
            let s = nearest_scale(self.scale);
            self.set_scale(s, q);
            self.zooming = None;
            self.auto = was_auto && s == from;
            self.keep_in_frame();
        }
    }

    /// The whole subject in the frame again, the frame keeping its size: in parallel at the
    /// largest standard scale that holds it. An automatic view stays so only if nothing changes.
    fn show_all(&mut self) {
        self.zooming = None;
        let (frame, lens) = self.look.frame();
        let size = self.crop.size();
        if let Some(p) = lens {
            let eye = eye_fitting(&self.corners, &frame, p.fov_y, size.x / size.y);
            self.set_lens(Perspective { eye, size, ..p });
            self.crop = self.full();
            return;
        }
        let (lo, hi) = self.extent(&frame);
        let whole = (hi - lo).max(DVec2::splat(1e-9));
        let fit = (size.x / whole.x).min(size.y / whole.y);
        if (fit / self.scale - 1.0).abs() > 1e-6 {
            self.scale = SCALES.iter().copied().find(|s| *s <= fit).unwrap_or(*SCALES.last().unwrap_or(&self.scale));
        }
        self.auto = self.auto && (self.full().size() - size).length() < 1e-6;
        self.crop = Crop { lo: -size / 2.0, hi: size / 2.0 };
    }

    /// The frame around the subject, the scale (or the eye) kept: a parallel view is automatic
    /// again.
    fn fit_frame(&mut self) {
        self.zooming = None;
        if self.look.frame().1.is_none() {
            self.auto = true;
            self.crop = self.full();
        } else if let Some((lo, hi)) = self.subject_on_paper() {
            let hi = hi.max(lo + DVec2::splat(5.0));
            self.crop = Crop { lo, hi };
        }
    }

    /// To perspective, its frame the one the view has now, the subject filling it.
    fn switch_to_perspective(&mut self) {
        let (frame, lens) = self.look.frame();
        if lens.is_some() {
            return;
        }
        self.zooming = None;
        self.parallel = Some((self.crop, self.auto));
        let size = self.crop.size().max(DVec2::splat(10.0));
        let fov_y = fov_of(DEFAULT_FOCAL);
        let eye = eye_fitting(&self.corners, &frame, fov_y, size.x / size.y);
        self.look = Look::Camera { forward: frame.forward, up: frame.up, perspective: Some(Perspective { eye, fov_y, size }) };
        self.auto = false;
        self.crop = self.full();
    }

    /// Back to parallel, with the frame it had before perspective; else the same frame, centred.
    fn switch_to_parallel(&mut self) {
        let frame = self.look.frame().0;
        let size = self.crop.size();
        self.look = Look::Camera { forward: frame.forward, up: frame.up, perspective: None };
        (self.crop, self.auto) = self.parallel.take().unwrap_or((Crop { lo: -size / 2.0, hi: size / 2.0 }, false));
        self.keep_in_frame();
    }

    /// Looking along a standard direction, in the projection it is in, the subject centred in the
    /// frame, which keeps its size.
    fn look_along(&mut self, d: Direction) {
        self.zooming = None;
        let size = self.crop.size();
        match self.look.frame().1 {
            None => {
                self.look = Look::Standard(d);
                self.crop = Crop { lo: -size / 2.0, hi: size / 2.0 };
            }
            Some(p) => {
                let frame = Frame::of(d);
                let eye = eye_fitting(&self.corners, &frame, p.fov_y, size.x / size.y);
                self.look = Look::Camera { forward: frame.forward, up: frame.up, perspective: Some(Perspective { eye, size, ..p }) };
                self.crop = self.full();
            }
        }
    }

    /// Another focal length, the eye moved so that the subject keeps its size in the frame.
    fn set_focal(&mut self, focal: f64) {
        if let Some(p) = self.look.frame().1 {
            let fov_y = fov_of(focal);
            let k = (p.fov_y / 2.0).tan() / (fov_y / 2.0).tan();
            self.set_lens(Perspective { eye: self.centre + (p.eye - self.centre) * k, fov_y, ..p });
        }
    }

    /// The view as framed, if that changes it: an automatic one takes its subject's box, about
    /// the place its frame was; a free one keeps its frame where it is on the sheet.
    fn change(&self, e: &Editor) -> Option<Operation> {
        let ElementKind::View(view) = &e.document().drawing().element(self.view)?.kind else { return None };
        let ViewKind::Base { subject, at, .. } = &view.kind else { return None };
        let whole = if self.look.frame().1.is_some() {
            let full = self.full();
            (self.crop.lo - full.lo).length() < 1e-6 && (self.crop.hi - full.hi).length() < 1e-6
        } else {
            self.auto
        };
        let crop = (!whole).then_some(self.crop);
        let middle = crop.map_or(DVec2::ZERO, |c| c.middle());
        let kind =
            ViewKind::Base { subject: subject.clone(), look: self.look.clone(), scale: self.scale, at: *at + self.start_mid - middle };
        let new = View { kind, crop, ..view.clone() };
        (new != *view).then(|| Operation::SetElement { element: self.view, kind: ElementKind::View(new) })
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
    crate::kit::handles::publish(|| "view_framer".into(), room);
    let stage = Stage::of(f.crop, room);
    if response.drag_started() {
        f.pivot = ui.input(|i| i.pointer.press_origin()).map(|p| f.point_at(e, &stage, p));
    }
    let drag = response.drag_delta();
    let delta = DVec2::new(drag.x as f64, drag.y as f64);
    if delta != DVec2::ZERO {
        if ui.input(|i| i.modifiers.shift) {
            f.move_eye(delta, &stage);
        } else if response.dragged_by(egui::PointerButton::Secondary) {
            f.slide(delta, &stage);
        } else {
            f.turn(delta);
        }
    }
    if response.hovered() {
        let (wheel, pinch, at) = ui.input(|i| (i.smooth_scroll_delta.y as f64, i.zoom_delta() as f64, i.pointer.hover_pos()));
        let wheel = wheel + pinch.ln() * WHEEL_DOUBLES / std::f64::consts::LN_2;
        if wheel != 0.0
            && let Some(p) = at
        {
            f.zoom_at(e, &stage, p, wheel, ui.input(|i| i.time));
        }
        let grabbing = response.dragged();
        ui.ctx().set_cursor_icon(if grabbing { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    f.settle(ui.input(|i| i.time), false);
    if f.zooming.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }
    let stage = Stage::of(f.crop, room);
    let camera = f.camera(&stage);

    // The picture.
    let ppp = ui.ctx().pixels_per_point();
    let side = |points: f32| (points * ppp).round().clamp(8.0, MAX_PICTURE) as u32;
    let size = [side(room.width()), side(room.height())];
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (e.history().revision(), size).hash(&mut h);
    camera.0.to_cols_array().map(f64::to_bits).hash(&mut h);
    camera.1.to_cols_array().map(f32::to_bits).hash(&mut h);
    let stamp = h.finish();
    let painter = ui.painter().with_clip_rect(room);
    painter.rect_filled(room, 0.0, egui::Color32::WHITE);
    if f.stamp.is_some()
        && let Some(texture) = ready.get(&SLOT)
    {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(*texture, room, uv, egui::Color32::WHITE);
    }
    // Asked for at every frame: the viewport draws it when its stamp changes or a texture comes.
    let wanted = vec![Wanted { slot: SLOT, size, camera, parts: f.parts.clone(), lines: None, images: f.images.clone(), stamp }];
    if f.stamp != Some(stamp) || !ready.contains_key(&SLOT) {
        f.stamp = Some(stamp);
        ui.ctx().request_repaint();
    }

    // What the view's frame will hold: the rest masked.
    let window = stage.window;
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
        .fixed_pos(room.center_bottom() - egui::vec2(0.0, 12.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::window(ui.style()).show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 6.0);
                let (frame, lens) = f.look.frame();
                ui.horizontal(|ui| {
                    for (d, key) in DIRECTIONS {
                        let along = Frame::of(d).forward;
                        let on = f.look == Look::Standard(d) || (lens.is_some() && (frame.forward - along).length() < 1e-6);
                        let b = ui.selectable_label(on, crate::workspaces::common::cards::direction_label(d));
                        crate::kit::handles::publish(|| format!("view_framer.dir:{key}"), b.rect);
                        if b.clicked() {
                            f.look_along(d);
                        }
                    }
                    ui.separator();
                    let parallel = ui.selectable_label(lens.is_none(), t!("drawing.framer.parallel"));
                    crate::kit::handles::publish(|| "view_framer.parallel".into(), parallel.rect);
                    if parallel.clicked() && lens.is_some() {
                        f.switch_to_parallel();
                    }
                    let label = egui::Button::selectable(lens.is_some(), t!("drawing.framer.perspective"));
                    let perspective = ui.add_enabled(!f.projected, label).on_disabled_hover_text(t!("drawing.framer.has_projections"));
                    crate::kit::handles::publish_enabled(|| "view_framer.perspective".into(), perspective.rect, !f.projected);
                    if perspective.clicked() {
                        f.switch_to_perspective();
                    }
                });
                ui.horizontal(|ui| {
                    let all = ui.button(t!("drawing.framer.show_all"));
                    crate::kit::handles::publish(|| "view_framer.show_all".into(), all.rect);
                    if all.clicked() {
                        f.show_all();
                    }
                    let fit = ui.button(t!("drawing.framer.fit_frame"));
                    crate::kit::handles::publish(|| "view_framer.fit_frame".into(), fit.rect);
                    if fit.clicked() {
                        f.fit_frame();
                    }
                    ui.separator();
                    if let Some(p) = lens {
                        let mut focal = focal_of(p.fov_y);
                        ui.spacing_mut().slider_width = 90.0;
                        // Shown rounded, never rounded itself: opening the view would change its focal length.
                        let slider = egui::Slider::new(&mut focal, FOCAL.0..=FOCAL.1)
                            .logarithmic(true)
                            .suffix(" mm")
                            .custom_formatter(|v, _| format!("{v:.0}"));
                        let changed = ui.add(slider);
                        crate::kit::handles::publish(|| "view_framer.focal".into(), changed.rect);
                        if changed.changed() {
                            f.set_focal(focal);
                        }
                    } else if f.zooming.is_some() {
                        ui.label(if f.scale >= 1.0 { format!("≈ {:.1}:1", f.scale) } else { format!("≈ 1:{:.1}", 1.0 / f.scale) });
                    } else {
                        ui.label(scale_label(f.scale));
                    }
                    match confirm_buttons(ui, "view_framer", t!("drawing.framer.ok"), true, t!("naming.cancel")) {
                        Some(Confirm::Ok) => done = true,
                        Some(Confirm::Cancel) => cancel = true,
                        None => {}
                    }
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
        f.settle(0.0, true);
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

    fn cube_framer(look: Look) -> ViewFramer {
        let mut corners = Vec::new();
        for i in 0..8 {
            corners.push(DVec3::new(
                if i & 1 == 0 { 0.0 } else { 900.0 },
                if i & 2 == 0 { 0.0 } else { 300.0 },
                if i & 4 == 0 { 0.0 } else { 500.0 },
            ));
        }
        let auto = look.frame().1.is_none();
        let mut f = ViewFramer {
            view: Id(1),
            look,
            parts: Vec::new(),
            images: Default::default(),
            corners,
            centre: DVec3::new(450.0, 150.0, 250.0),
            radius: 550.0,
            scale: 0.2,
            projected: false,
            crop: Crop { lo: -DVec2::ONE, hi: DVec2::ONE },
            auto,
            parallel: None,
            start_mid: DVec2::ZERO,
            pivot: None,
            zooming: None,
            stamp: None,
        };
        f.crop = f.full();
        f
    }

    fn room() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 800.0))
    }

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
            look_in(placed.inverse(), DVec3::Y, DVec3::Z, Some((DVec3::new(0.0, -10.0, 0.0), 0.7)), DVec2::new(100.0, 80.0))
        else {
            unreachable!()
        };
        assert!((forward - DVec3::X).length() < 1e-9 && (up - DVec3::Z).length() < 1e-9);
        assert!((perspective.unwrap().eye - DVec3::new(-10.0, 0.0, 0.0)).length() < 1e-9);
    }

    #[test]
    fn the_eye_found_sees_every_corner_within_the_frame() {
        let f = cube_framer(Look::Standard(Direction::Iso));
        let frame = Frame::of(Direction::Iso);
        let (fov_y, aspect) = (fov_of(50.0), 1.5);
        let eye = eye_fitting(&f.corners, &frame, fov_y, aspect);
        let t = (fov_y / 2.0).tan();
        let mut tight = 0.0f64;
        for c in &f.corners {
            let q = frame.project(*c - eye);
            assert!(q.z > 0.0, "in front of the eye");
            let reach = (q.x.abs() / (q.z * t * aspect)).max(q.y.abs() / (q.z * t));
            assert!(reach <= 1.0 + 1e-9);
            tight = tight.max(reach);
        }
        assert!(tight > 0.8, "and not far off: {tight}");
    }

    #[test]
    fn a_focal_length_reads_back() {
        assert!((focal_of(fov_of(35.0)) - 35.0).abs() < 1e-9);
    }

    #[test]
    fn turning_about_a_point_keeps_it_where_it_is_in_the_frame() {
        // Parallel.
        let mut f = cube_framer(Look::Standard(Direction::Front));
        let stage = Stage::of(f.crop, room());
        f.slide(DVec2::new(30.0, 10.0), &stage);
        let pivot = DVec3::new(800.0, 0.0, 400.0);
        f.pivot = Some(pivot);
        let in_frame = |f: &ViewFramer| {
            let frame = f.look.frame().0;
            (frame.project(pivot).truncate() - f.box_mid(&frame)) * f.scale - f.crop.lo
        };
        let before = in_frame(&f);
        f.turn(DVec2::new(70.0, 40.0));
        assert!((in_frame(&f) - before).length() < 1e-9);
        // In perspective: its place seen from the eye.
        let mut f = cube_framer(Look::Standard(Direction::Front));
        f.switch_to_perspective();
        f.pivot = Some(pivot);
        let seen = |f: &ViewFramer| {
            let (frame, lens) = f.look.frame();
            let c = frame.project(pivot - lens.unwrap().eye);
            DVec2::new(c.x / c.z, c.y / c.z)
        };
        let before = seen(&f);
        f.turn(DVec2::new(70.0, 40.0));
        assert!((seen(&f) - before).length() < 1e-9);
    }

    #[test]
    fn the_wheel_zooms_gently_at_the_pointer_then_settles_on_a_standard_scale() {
        let mut f = cube_framer(Look::Standard(Direction::Front));
        let e = Editor::new(Document::new(), Box::new(NoKernel));
        let stage = Stage::of(f.crop, room());
        let p = egui::pos2(620.0, 330.0);
        let q = stage.to_paper(p);
        let from_window = q - f.crop.lo;
        let model = q / f.scale;
        f.zoom_at(&e, &stage, p, 40.0, 1.0);
        assert!(f.scale > 0.2 && f.scale < 0.22, "a notch is a small step: {}", f.scale);
        assert!((model * f.scale - f.crop.lo - from_window).length() < 1e-9, "the same place in the frame");
        f.settle(1.1, false);
        assert!(f.zooming.is_some(), "not while it turns");
        f.settle(1.3, false);
        assert_eq!(f.scale, 0.2, "the nearest standard scale");
        assert!(f.auto, "back on its scale, nothing was changed");
        assert!((model * f.scale - f.crop.lo - from_window).length() < 1e-9, "and still there");
    }

    #[test]
    fn the_subject_cannot_leave_the_frame() {
        let mut f = cube_framer(Look::Standard(Direction::Front));
        let stage = Stage::of(f.crop, room());
        f.slide(DVec2::new(100000.0, -100000.0), &stage);
        let (lo, hi) = f.subject_on_paper().unwrap();
        assert!(f.crop.lo.cmplt(hi).all() && f.crop.hi.cmpgt(lo).all(), "a part of it stays");
    }

    #[test]
    fn turning_keeps_the_frame_and_an_automatic_view_stays_so() {
        let mut f = cube_framer(Look::Standard(Direction::Front));
        let size = f.crop.size();
        f.pivot = Some(f.centre);
        f.turn(DVec2::new(90.0, 40.0));
        assert!(f.auto, "only turned");
        assert!((f.crop.size() - size).length() < 1e-9, "the frame did not move");
        let stage = Stage::of(f.crop, room());
        f.slide(DVec2::new(5.0, 0.0), &stage);
        assert!(!f.auto, "placed by hand: free");
        f.fit_frame();
        assert!(f.auto && (f.crop.size() - f.full().size()).length() < 1e-9, "automatic again, around the subject");
    }

    #[test]
    fn show_all_keeps_the_frame_and_takes_the_scale_that_holds_the_subject() {
        let mut f = cube_framer(Look::Standard(Direction::Front));
        let size = f.crop.size();
        f.auto = false;
        f.scale = 0.5;
        f.show_all();
        assert_eq!(f.scale, 0.2, "the subject held again");
        assert!(!f.auto, "free stays free");
        assert!((f.crop.size() - size).length() < 1e-9, "in the same frame");
        assert!(f.crop.middle().length() < 1e-9);
    }

    struct NoKernel;
    impl skraft_model::kernel::KernelService for NoKernel {
        fn submit(&mut self, _: skraft_model::kernel::JobId, _: skraft_model::kernel::Job) {}
        fn poll(&mut self) -> Vec<(skraft_model::kernel::JobId, skraft_model::kernel::JobResult)> {
            Vec::new()
        }
    }
}

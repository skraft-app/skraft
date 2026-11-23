//! The box of what Move and Rotate take (docs/tools/inferences.md#selection-box): its eight
//! corners are points to infer on and, for Rotate, the face of it under the cursor offers four
//! grips, which start a rotation about the axis through the face's centre, along its normal.
//! One part keeps its own axes; several share one box in the context's.

use glam::{DQuat, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{EntityPath, Id};

use super::handle::{Axes, Target};
use super::{Cx, INK};
use crate::OrientedBox;
use crate::draw::{Drawer, Pen};
use crate::infer::{APERTURE_PX, InferKind, Inference};
use crate::scene::Owner;

/// How far inside the face's sides the grips stand (pixels), at most half way to its centre.
const GRIP_INSET_PX: f64 = 28.0;
/// A grip nearer than this to the face's centre on screen is not offered (pixels).
const GRIP_MIN_PX: f64 = 12.0;
/// The radius of a grip's curved arrow (pixels), and its half angle (radians).
const GRIP_ARC_RADIUS_PX: f64 = 20.0;
const GRIP_ARC_HALF: f64 = 1.0;
/// A head's length (pixels), its half width over its length, and how far back on the curve
/// (radians) its direction is taken, so that it follows the bend.
const GRIP_HEAD_PX: f64 = 5.0;
const GRIP_HEAD_SPREAD: f64 = 0.65;
const GRIP_HEAD_BACK: f64 = 0.35;
/// How far from a grip's arrow, on screen, the cursor takes it (pixels).
const GRIP_HIT_PX: f64 = 8.0;
/// The chords of half a grip's arc.
const GRIP_ARC_STEPS: i32 = 6;
/// The light edge under a grip, readable on any face (points).
const GRIP_HALO_WIDTH: f32 = 4.5;
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// The face of the box under the cursor (world).
#[derive(Clone, Debug)]
struct Face {
    center: DVec3,
    /// Unit, out of the box.
    normal: DVec3,
    corners: [DVec3; 4],
    grips: Vec<DVec3>,
}

/// What the cursor aims at on the face.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Aim {
    Center,
    Grip(DVec3),
}

/// What the box hangs on: the scene's generation, the context, whether bodies are shown, the
/// selection and the parts selected at any depth.
type Key = (u64, Vec<Id>, bool, Vec<Id>, Vec<EntityPath>);

/// The box of the selection, kept while what it hangs on stays, and what the cursor aims at.
#[derive(Default)]
pub(super) struct BoxGrips {
    key: Option<Key>,
    target: Option<OrientedBox>,
    several: bool,
    corners: Vec<DVec3>,
    face: Option<Face>,
    aim: Option<Aim>,
}

impl BoxGrips {
    fn sync(&mut self, cx: &Cx<'_>, ids: &[Id], deep: &[EntityPath]) {
        let key = (cx.scene.generation(), cx.context.to_vec(), cx.machinings_shown(), ids.to_vec(), deep.to_vec());
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let target = if deep.is_empty() { Target::of(cx, ids, Axes::Entity) } else { Target::of_deep(cx, deep, Axes::Entity) };
        self.target = target.map(|t| OrientedBox { frame: t.frame, lo: t.lo, hi: t.hi });
        self.corners = self.target.map_or_else(Vec::new, |b| b.corners().to_vec());
        self.several = ids.len() + deep.len() > 1;
        self.key = Some(key);
    }

    /// The point inferred under `px`, the box's corners among the candidates (Move): no face,
    /// no grip.
    pub fn infer_corners(&mut self, cx: &Cx<'_>, px: DVec2, ids: &[Id], deep: &[EntityPath]) -> Option<Inference> {
        self.sync(cx, ids, deep);
        cx.infer_on_box(px, &self.corners)
    }

    /// The point inferred under `px`, the box's corners and the centre of the face under the
    /// cursor among the candidates (Rotate); `None` on a grip (or where nothing is inferred).
    pub fn infer(&mut self, cx: &Cx<'_>, px: DVec2, ids: &[Id], deep: &[EntityPath]) -> Option<Inference> {
        self.sync(cx, ids, deep);
        let inf = cx.infer_on_box(px, &self.corners);
        self.face = self.target.as_ref().and_then(|b| face_at(cx, b, px, ids, deep));
        self.aim = None;
        let Some(face) = &self.face else { return inf };
        let Some(screen) = cx.camera.project(face.center, cx.size) else { return inf };
        // A point of the geometry keeps the cursor unless it is nearer to an arrow.
        let point = inf.as_ref().filter(|i| i.kind.is_point()).map(|i| cx.camera.project(i.point, cx.size).map_or(0.0, |s| s.distance(px)));
        let mut best = point.unwrap_or(f64::INFINITY);
        if point.is_none() && screen.distance(px) <= APERTURE_PX {
            best = screen.distance(px);
            self.aim = Some(Aim::Center);
        }
        // An axis locked keeps the rotation on it: no grip gives another.
        if cx.axis_lock.is_none() {
            for &g in &face.grips {
                let dist = arrow_distance(cx, face, g, px);
                if dist <= GRIP_HIT_PX && dist < best {
                    best = dist;
                    self.aim = Some(Aim::Grip(g));
                }
            }
        }
        match self.aim {
            Some(Aim::Center) => Some(Inference::on_box(face.center, InferKind::Center, inf.and_then(|i| i.hit))),
            Some(Aim::Grip(_)) => None,
            None => inf,
        }
    }

    /// The rotation the grip aimed at starts: the face's centre, its normal, the grip.
    pub fn turn(&self) -> Option<(DVec3, DVec3, DVec3)> {
        match (self.aim, &self.face) {
            (Some(Aim::Grip(g)), Some(f)) => Some((f.center, f.normal, g)),
            _ => None,
        }
    }

    /// The face's centre and normal, when the cursor is on the centre.
    pub fn center(&self) -> Option<(DVec3, DVec3)> {
        match (self.aim, &self.face) {
            (Some(Aim::Center), Some(f)) => Some((f.center, f.normal)),
            _ => None,
        }
    }

    /// The tooltip on a grip.
    pub fn tip(&self) -> Option<String> {
        self.turn().map(|_| t!("common.rotate"))
    }

    pub fn clear(&mut self) {
        self.face = None;
        self.aim = None;
    }

    /// The box shared by several, the face under the cursor and its grips.
    pub fn draw(&self, d: &mut Drawer<'_>, cx: &Cx<'_>) {
        if self.several
            && let Some(b) = &self.target
        {
            d.pen(Pen::new(INK).dotted()).oriented_box(b, Default::default());
        }
        let Some(f) = &self.face else { return };
        let color = super::rotate::axis_color(cx, f.normal);
        d.pen(Pen::new(color).dashed().over()).polygon(&f.corners);
        for &g in &f.grips {
            let aimed = self.aim == Some(Aim::Grip(g));
            let halo = Pen::new(WHITE).width(GRIP_HALO_WIDTH).over();
            let pen = Pen::new(color).over().bold_if(aimed);
            grip_arc(d, cx, f, g, halo);
            grip_arc(d, cx, f, g, pen);
        }
        // Aimed at, the rotation it starts: the protractor, its zero at the grip.
        if let Some((c, axis, g)) = self.turn() {
            super::rotate::protractor(d, cx, c, axis, (g - c).normalize(), color);
            d.pen(Pen::dimension(color)).line(c, g).mark(c);
        }
    }
}

/// The curve of the arrow of the grip `g` of the face `f` (world), and the centre of that curve.
fn arc_points(cx: &Cx<'_>, f: &Face, g: DVec3) -> (Vec<DVec3>, DVec3) {
    let radial = (g - f.center).normalize() * GRIP_ARC_RADIUS_PX * cx.camera.world_per_pixel(cx.size, g);
    let center = g - radial;
    let points = (-GRIP_ARC_STEPS..=GRIP_ARC_STEPS)
        .map(|i| center + DQuat::from_axis_angle(f.normal, GRIP_ARC_HALF * i as f64 / GRIP_ARC_STEPS as f64) * radial)
        .collect();
    (points, center)
}

/// How far `px` is from the arrow of the grip `g`, on screen (pixels).
fn arrow_distance(cx: &Cx<'_>, f: &Face, g: DVec3, px: DVec2) -> f64 {
    let (points, _) = arc_points(cx, f, g);
    let screen: Vec<DVec2> = points.iter().filter_map(|p| cx.camera.project(*p, cx.size)).collect();
    screen
        .windows(2)
        .map(|w| {
            let (a, ab) = (w[0], w[1] - w[0]);
            let t = if ab.length_squared() > 0.0 { ((px - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
            (a + ab * t).distance(px)
        })
        .fold(f64::INFINITY, f64::min)
}

/// The grip `g` of the face `f`: a curved double arrow, hollow towards the face's centre, its
/// curve fixed on screen (the face's own radius would draw it nearly straight): it reads as a
/// turn either way.
fn grip_arc(d: &mut Drawer<'_>, cx: &Cx<'_>, f: &Face, g: DVec3, pen: Pen) {
    let wpp = cx.camera.world_per_pixel(cx.size, g);
    let (points, center) = arc_points(cx, f, g);
    let radial = g - center;
    let at = |a: f64| center + DQuat::from_axis_angle(f.normal, a) * radial;
    d.pen(pen).curve(&points, false);
    // The heads lie in the face, along the curve's end.
    let head = GRIP_HEAD_PX * wpp;
    for way in [1.0, -1.0] {
        let tip = at(way * GRIP_ARC_HALF);
        let along = (tip - at(way * (GRIP_ARC_HALF - GRIP_HEAD_BACK))).normalize();
        let across = f.normal.cross(along);
        d.line(tip - along * head + across * head * GRIP_HEAD_SPREAD, tip);
        d.line(tip - along * head - across * head * GRIP_HEAD_SPREAD, tip);
    }
}

/// The face of the box `b` the ray at `px` enters by, unless something else stands before it.
fn face_at(cx: &Cx<'_>, b: &OrientedBox, px: DVec2, ids: &[Id], deep: &[EntityPath]) -> Option<Face> {
    let ray = cx.ray(px);
    let inv = b.frame.inverse();
    let (o, d) = (inv.transform_point3(ray.origin), inv.transform_vector3(ray.dir));
    let (mut enter, mut leave) = (f64::NEG_INFINITY, f64::INFINITY);
    let mut by = None;
    for k in 0..3 {
        if d[k].abs() < 1e-12 {
            if o[k] < b.lo[k] || o[k] > b.hi[k] {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((b.lo[k] - o[k]) / d[k], (b.hi[k] - o[k]) / d[k]);
        if t0.min(t1) > enter {
            enter = t0.min(t1);
            by = Some((k, d[k] < 0.0));
        }
        leave = leave.min(t0.max(t1));
    }
    let (k, high) = by?;
    if enter > leave || enter < ray.t_min {
        return None;
    }
    let at = ray.origin + ray.dir * enter;
    let tol = cx.camera.world_per_pixel(cx.size, at) * 3.0;
    let other = |o: &Owner<'_>| !(cx.is_moving(o, ids) || cx.is_moving_deep(o, deep));
    if cx.scene.pick_inferable(&ray, other).is_some_and(|h| h.t < enter - tol) {
        return None;
    }
    Some(face(cx, b, k, high))
}

/// The face of `b` across its axis `k`, on its high side or its low one.
fn face(cx: &Cx<'_>, b: &OrientedBox, k: usize, high: bool) -> Face {
    let (u, v) = ((k + 1) % 3, (k + 2) % 3);
    let mid = (b.lo + b.hi) * 0.5;
    let half = (b.hi - b.lo) * 0.5;
    let mut c = mid;
    c[k] = if high { b.hi[k] } else { b.lo[k] };
    let along = |a: usize, by: f64| {
        let mut p = DVec3::ZERO;
        p[a] = by;
        p
    };
    let world = |p: DVec3| b.frame.transform_point3(p);
    let center = world(c);
    let normal = b.frame.matrix3.col(k).normalize() * if high { 1.0 } else { -1.0 };
    let corners =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(su, sv)| world(c + along(u, su * half[u]) + along(v, sv * half[v])));
    let inset = cx.camera.world_per_pixel(cx.size, center) * GRIP_INSET_PX;
    let screen = cx.camera.project(center, cx.size);
    let far_enough = |p: DVec3| match (screen, cx.camera.project(p, cx.size)) {
        (Some(a), Some(b)) => a.distance(b) >= GRIP_MIN_PX,
        _ => false,
    };
    let grips = [(u, 1.0), (u, -1.0), (v, 1.0), (v, -1.0)]
        .into_iter()
        .map(|(a, s)| world(c + along(a, s * (half[a] - inset).max(half[a] * 0.5))))
        .filter(|p| far_enough(*p))
        .collect();
    Face { center, normal, corners, grips }
}

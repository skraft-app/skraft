//! Profile: edits the blank profile of the open part (docs/ARCHITECTURE.md §6 nonies, step 3).
//! The outline is drawn on the cap facing the eye with grips — its corners, its arcs' middles,
//! its straight sides' middles (to insert a corner) and the cap's middle (the thickness). A
//! grip is taken by a click and dropped by another (SketchUp's Move; a drag works too), Esc
//! dropping the gesture; each gesture is one `Operation::SetProfile`, under the open part's
//! scope.
//!
//! - A corner follows the cursor, inferred with the lines through where it was and brought
//!   onto the cap's plane; its sides follow, an arc keeping its bulge. Typed: a distance along
//!   the move, or a point.
//! - A side's middle inserts a corner there, which then moves as a corner.
//! - An arc's middle sets its bulge (the arc through the cursor), snapping to the half circle
//!   and to straight within 10 px. Typed: its height (sagitta).
//! - The thickness grip moves that cap along the part's Z. Typed: the thickness.
//! - Delete over a corner takes it away.
//!
//! An outline that would cross itself is refused, said in the status bar.

use glam::{DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::kernel::profile::{self, Corner, Side};
use solidsketch_model::{BlankProfile, EntityPath, Operation};

use super::{
    BLUE, Command, Cx, INK, Key, MAGENTA, PICK, PointShape, Preview, PreviewLine, PreviewPoint, Tool, ToolInput, ToolKind, dashed,
};
use crate::infer::Inference;
use crate::vcb::{PointRef, Vcb, VcbKind, VcbValue};

/// How near a grip (pixels) the cursor takes it.
const GRIP_PX: f64 = 10.0;
/// How near the half circle or the chord (pixels) an arc's middle snaps to it.
const SNAP_PX: f64 = 10.0;
/// The smallest length or thickness (mm).
const MIN: f64 = 1e-3;
/// How far an arc turns between two points of its preview (radians).
const STEP: f64 = 0.06;

/// A grip of the outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grip {
    Corner(usize),
    /// The middle of a straight side: a corner inserted there.
    Insert(usize),
    /// The middle of an arc: its bulge.
    Bulge(usize),
    Thickness,
}

#[derive(Default)]
pub struct ProfileTool {
    state: State,
    /// The cap the outline is drawn on while a grip is taken: the top (z0 + height) or the
    /// bottom (z0). Otherwise the cap facing the eye ([`ProfileTool::cap`]).
    top: bool,
    snap: Option<Inference>,
}

#[derive(Default, Clone)]
enum State {
    #[default]
    Idle,
    /// The grip under the cursor.
    Over(Grip),
    /// A grip taken: the profile as it was (a corner inserted already for a side's middle), the
    /// grip moved (an inserted corner as a corner), where it was (axes), the profile now, and
    /// whether it is dragged (dropped on release).
    Moving { grip: Grip, from: DVec3, base: BlankProfile, now: BlankProfile, drag: bool, world: (DVec3, DVec3) },
}

/// The open part's path and profile.
fn open(cx: &Cx<'_>) -> Option<(EntityPath, BlankProfile)> {
    let path = cx.open_part_path()?;
    let (_, part, _) = cx.doc.resolve_part(&path).ok()?;
    Some((path, part.profile.clone()?))
}

/// The height of the cap in use.
fn cap_z(b: &BlankProfile, top: bool) -> f64 {
    if top { b.z0 + b.height } else { b.z0 }
}

/// Where a grip is (axes).
fn grip_at(b: &BlankProfile, top: bool, g: Grip) -> DVec3 {
    let z = cap_z(b, top);
    let n = b.profile.len();
    match g {
        Grip::Corner(i) => b.profile[i].p.extend(z),
        Grip::Insert(i) | Grip::Bulge(i) => Side::new(b.profile[i].p, b.profile[(i + 1) % n].p, b.profile[i].bulge).at(0.5).extend(z),
        Grip::Thickness => {
            let (lo, hi) = profile::bounds(&b.profile).unwrap_or_default();
            ((lo + hi) / 2.0).extend(z)
        }
    }
}

/// Every grip of the outline.
fn grips(b: &BlankProfile) -> Vec<Grip> {
    let n = b.profile.len();
    let mut out: Vec<Grip> = (0..n).map(Grip::Corner).collect();
    out.extend((0..n).map(|i| if b.profile[i].bulge == 0.0 { Grip::Insert(i) } else { Grip::Bulge(i) }));
    out.push(Grip::Thickness);
    out
}

/// The profile with a corner inserted in the middle of its straight side `i`: the new corner is
/// `i + 1`.
fn inserted(b: &BlankProfile, i: usize) -> BlankProfile {
    let n = b.profile.len();
    let mid = (b.profile[i].p + b.profile[(i + 1) % n].p) / 2.0;
    let mut p = b.profile.clone();
    p.insert(i + 1, Corner::new(mid));
    BlankProfile { profile: p, ..b.clone() }
}

/// The profile without its corner `i`, the side before it running on straight to the next one.
fn removed(b: &BlankProfile, i: usize) -> BlankProfile {
    let n = b.profile.len();
    let mut p = b.profile.clone();
    p.remove(i);
    let before = (i + n - 1) % n;
    let before = if before > i { before - 1 } else { before };
    if let Some(c) = p.get_mut(before) {
        c.bulge = 0.0;
    }
    BlankProfile { profile: p, ..b.clone() }
}

impl ProfileTool {
    /// The cap the outline is drawn on: the one the gesture under way took, else the one facing
    /// the eye now — the view's middle looking down the part's Z sees its top — from the very
    /// first frame, whatever the cursor.
    fn cap(&self, cx: &Cx<'_>) -> bool {
        match self.state {
            State::Moving { .. } => self.top,
            _ => cx.axes_ray(cx.size / 2.0).dir.z <= 0.0,
        }
    }

    /// The grip under `px`, the nearest within reach.
    fn grip_under(&self, cx: &Cx<'_>, b: &BlankProfile, px: DVec2) -> Option<Grip> {
        grips(b)
            .into_iter()
            .filter_map(|g| Some((g, cx.camera.project(cx.to_world(grip_at(b, self.cap(cx), g)), cx.size)?.distance(px))))
            .filter(|(_, d)| *d <= GRIP_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    }

    /// The profile with the grip moved by the cursor at `px`, and what it snapped to.
    fn moved(&self, cx: &Cx<'_>, px: DVec2, grip: Grip, from: DVec3, base: &BlankProfile) -> Option<(BlankProfile, Option<Inference>)> {
        let z = cap_z(base, self.top);
        let n = base.profile.len();
        match grip {
            Grip::Corner(i) | Grip::Insert(i) => {
                let i = if matches!(grip, Grip::Insert(_)) { i + 1 } else { i };
                let plane = (cx.to_world(DVec3::new(0.0, 0.0, z)), cx.axis(2));
                let inf = cx.infer_from(px, |_| true, Some(plane), cx.to_world(from), cx.lock())?;
                let q = cx.to_axes(inf.point);
                let mut b = base.clone();
                b.profile[i].p = q.truncate();
                Some((b, Some(inf)))
            }
            Grip::Bulge(i) => {
                let (a, c) = (base.profile[i].p, base.profile[(i + 1) % n].p);
                let q = cx.axes_ray(px).hit_plane(DVec3::new(0.0, 0.0, z), DVec3::Z)?.truncate();
                let l = a.distance(c);
                let u = (c - a).try_normalize()?;
                let left = u.perp();
                let mut s = (q - a).dot(left);
                // Near straight, or the half circle: that.
                let on_screen = |p: DVec2| cx.camera.project(cx.to_world(p.extend(z)), cx.size).map(|w| w.distance(px));
                let mid = (a + c) / 2.0;
                if on_screen(mid).is_some_and(|d| d <= SNAP_PX) {
                    s = 0.0;
                } else if on_screen(mid + left * (l / 2.0) * s.signum()).is_some_and(|d| d <= SNAP_PX) {
                    s = (l / 2.0) * s.signum();
                }
                let mut b = base.clone();
                b.profile[i].bulge = if s.abs() < MIN { 0.0 } else { profile::bulge_through(a, mid + left * s, c) };
                Some((b, None))
            }
            Grip::Thickness => {
                // The closest point of the part's Z through the grip to the cursor's ray.
                let zc = cx.axes_ray(px).closest_on_line(from, DVec3::Z).z;
                let mut b = base.clone();
                if self.top {
                    b.height = (zc - base.z0).max(MIN);
                } else {
                    let top = base.z0 + base.height;
                    b.z0 = zc.min(top - MIN);
                    b.height = top - b.z0;
                }
                Some((b, None))
            }
        }
    }

    /// The gesture done with `now`: the operation, or why not.
    fn apply(&mut self, cx: &Cx<'_>, now: BlankProfile) -> Vec<Command> {
        let Some((target, _)) = open(cx) else {
            self.state = State::Idle;
            return vec![];
        };
        if !now.is_valid() {
            return vec![Command::Say(t!("tool.profile.say.crossed"))];
        }
        self.state = State::Idle;
        self.snap = None;
        vec![Command::Apply(Operation::SetProfile { target, profile: now, scope: cx.part_scope })]
    }

    /// Takes the grip `grip` of `b`.
    fn take(&mut self, b: &BlankProfile, grip: Grip, drag: bool) {
        let base = match grip {
            Grip::Insert(i) => inserted(b, i),
            _ => b.clone(),
        };
        let from = match grip {
            Grip::Insert(i) => grip_at(&base, self.top, Grip::Corner(i + 1)),
            g => grip_at(&base, self.top, g),
        };
        self.state = State::Moving { grip, from, now: base.clone(), base, drag, world: (DVec3::ZERO, DVec3::ZERO) };
    }
}

impl Tool for ProfileTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Profile
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        let Some((_, b)) = open(cx) else {
            self.state = State::Idle;
            return vec![];
        };
        // The cap facing the eye, kept while a grip is taken.
        self.top = self.cap(cx);
        match (input, self.state.clone()) {
            (ToolInput::Hover { px }, State::Idle | State::Over(_)) => {
                self.state = self.grip_under(cx, &b, *px).map_or(State::Idle, State::Over);
            }
            (ToolInput::Click { px, .. }, State::Idle | State::Over(_)) => match self.grip_under(cx, &b, *px) {
                Some(g) => self.take(&b, g, false),
                // Off the part: as Select's click off (§2.9).
                None => return vec![Command::ClickOff(*px)],
            },
            (ToolInput::DragStart { px, .. }, State::Idle | State::Over(_)) => {
                if let Some(g) = self.grip_under(cx, &b, *px) {
                    self.take(&b, g, true);
                }
            }
            (ToolInput::Hover { px } | ToolInput::Drag { px }, State::Moving { grip, from, base, drag, .. }) => {
                if let Some((now, snap)) = self.moved(cx, *px, grip, from, &base) {
                    self.snap = snap.filter(|i| i.kind != crate::infer::InferKind::OnPlane);
                    // A corner's start and where it is, in the world (typed points).
                    let to = match grip {
                        Grip::Corner(i) => now.profile[i].p.extend(from.z),
                        Grip::Insert(i) => now.profile[i + 1].p.extend(from.z),
                        _ => from,
                    };
                    let world = (cx.to_world(from), cx.to_world(to));
                    self.state = State::Moving { grip, from, base, now, drag, world };
                }
            }
            (ToolInput::Click { px, .. }, State::Moving { grip, from, base, drag: false, .. })
            | (ToolInput::DragEnd { px, .. }, State::Moving { grip, from, base, drag: true, .. }) => {
                if let Some((now, _)) = self.moved(cx, *px, grip, from, &base) {
                    return self.apply(cx, now);
                }
            }
            (ToolInput::Key { key: Key::Delete, .. }, State::Over(Grip::Corner(i))) => return self.apply(cx, removed(&b, i)),
            (ToolInput::Value(v), State::Moving { grip, from, base, now, .. }) => {
                let n = base.profile.len();
                let typed = match (grip, v) {
                    (Grip::Corner(_) | Grip::Insert(_), VcbValue::Lengths(v)) => {
                        let i = if let Grip::Insert(i) = grip {
                            i + 1
                        } else if let Grip::Corner(i) = grip {
                            i
                        } else {
                            0
                        };
                        let d = (now.profile[i].p - from.truncate()).try_normalize();
                        d.map(|d| {
                            let mut b = base.clone();
                            b.profile[i].p = from.truncate() + d * v[0];
                            b
                        })
                    }
                    (Grip::Corner(_) | Grip::Insert(_), VcbValue::Point(w)) => {
                        let i = if let Grip::Insert(i) = grip {
                            i + 1
                        } else if let Grip::Corner(i) = grip {
                            i
                        } else {
                            0
                        };
                        let mut b = base.clone();
                        b.profile[i].p = cx.to_axes(*w).truncate();
                        Some(b)
                    }
                    (Grip::Bulge(i), VcbValue::Lengths(v)) => {
                        let (a, c) = (base.profile[i].p, base.profile[(i + 1) % n].p);
                        let left = (c - a).normalize().perp();
                        // The side the arc bulges to now.
                        let side = if now.profile[i].bulge > 0.0 { -1.0 } else { 1.0 };
                        let mut b = base.clone();
                        b.profile[i].bulge = profile::bulge_through(a, (a + c) / 2.0 + left * v[0].abs() * side, c);
                        Some(b)
                    }
                    (Grip::Thickness, VcbValue::Lengths(v)) if v[0].abs() >= MIN => {
                        let mut b = base.clone();
                        if self.top {
                            b.height = v[0].abs();
                        } else {
                            b.z0 = base.z0 + base.height - v[0].abs();
                            b.height = v[0].abs();
                        }
                        Some(b)
                    }
                    _ => None,
                };
                if let Some(now) = typed {
                    return self.apply(cx, now);
                }
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let Some((_, part)) = open(cx) else { return Preview::default() };
        let (b, grip) = match &self.state {
            State::Moving { now, grip, .. } => (now.clone(), Some(*grip)),
            _ => (part.clone(), None),
        };
        let mut lines = Vec::new();
        let mut points = Vec::new();
        let line = |a: DVec3, c: DVec3, color| PreviewLine { a: cx.to_world(a), b: cx.to_world(c), color, bold: false };
        // The outline on the cap, and on the other one with the sides while moving.
        let ring = |z: f64| {
            let pts = profile::sample(&b.profile, STEP);
            let n = pts.len();
            (0..n).map(move |i| (pts[i].extend(z), pts[(i + 1) % n].extend(z))).collect::<Vec<_>>()
        };
        let top = self.cap(cx);
        let z = cap_z(&b, top);
        lines.extend(ring(z).into_iter().map(|(a, c)| PreviewLine { bold: true, ..line(a, c, PICK) }));
        if grip.is_some() {
            let other = cap_z(&b, !top);
            lines.extend(ring(other).into_iter().map(|(a, c)| line(a, c, PICK)));
            lines.extend(b.profile.iter().map(|c| line(c.p.extend(z), c.p.extend(other), PICK)));
            // Where it was: the outline as it is, dashed.
            for (a, c) in profile::sample(&part.profile, STEP).windows(2).map(|w| (w[0], w[1])) {
                let zp = cap_z(&part, top);
                dashed(cx.camera, cx.size, &mut lines, cx.to_world(a.extend(zp)), cx.to_world(c.extend(zp)), INK);
            }
        }
        let over = match &self.state {
            State::Over(g) => Some(*g),
            State::Moving { grip, .. } => Some(match grip {
                Grip::Insert(i) => Grip::Corner(i + 1),
                g => *g,
            }),
            State::Idle => None,
        };
        for g in grips(&b) {
            let at = cx.to_world(grip_at(&b, top, g));
            let hot = over == Some(g);
            let (radius, shape, stroke) = match g {
                Grip::Corner(_) => (5.0, PointShape::Round, INK),
                Grip::Insert(_) => (3.0, PointShape::Round, INK),
                Grip::Bulge(_) => (5.0, PointShape::Diamond, MAGENTA),
                Grip::Thickness => (5.0, PointShape::Round, BLUE),
            };
            let radius = if hot { radius + 2.0 } else { radius };
            points.push(PreviewPoint {
                at,
                radius,
                fill: Some(if hot { PICK } else { [1.0, 1.0, 1.0, 1.0] }),
                stroke: Some(stroke),
                shape,
            });
        }
        Preview { lines, points, inference: self.snap.clone(), hide_selection: true, ..Default::default() }
    }

    fn status(&self) -> String {
        match &self.state {
            State::Moving { grip: Grip::Bulge(_), .. } => t!("tool.profile.hint.bulge"),
            State::Moving { grip: Grip::Thickness, .. } => t!("tool.profile.hint.thickness"),
            State::Moving { .. } => t!("tool.profile.hint.corner"),
            _ => t!("tool.profile.hint.idle"),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        let State::Moving { grip, from, base, now, world, .. } = &self.state else { return None };
        let n = base.profile.len();
        match grip {
            Grip::Corner(i) | Grip::Insert(i) => {
                let i = if matches!(grip, Grip::Insert(_)) { i + 1 } else { *i };
                let to = now.profile[i].p.extend(from.z);
                Some(Vcb {
                    point: Some(PointRef { from: world.0, to: world.1 }),
                    ..Vcb::new(t!("tool.profile.vcb.distance"), VcbKind::Lengths(vec![from.distance(to)]))
                })
            }
            Grip::Bulge(i) => {
                let s = Side::new(now.profile[*i].p, now.profile[(i + 1) % n].p, now.profile[*i].bulge);
                let mid = (s.a() + s.b()) / 2.0;
                Some(Vcb::new(t!("tool.profile.vcb.bulge"), VcbKind::Lengths(vec![s.at(0.5).distance(mid)])))
            }
            Grip::Thickness => Some(Vcb::new(t!("tool.profile.vcb.thickness"), VcbKind::Lengths(vec![now.height]))),
        }
    }

    fn busy(&self) -> bool {
        matches!(self.state, State::Moving { .. })
    }

    fn takes_delete(&self) -> bool {
        matches!(self.state, State::Over(Grip::Corner(_)))
    }

    fn leave(&mut self) {
        if !matches!(self.state, State::Moving { .. }) {
            self.state = State::Idle;
        }
    }

    fn back(&mut self) {
        self.state = State::Idle;
        self.snap = None;
    }

    fn reset(&mut self) {
        self.state = State::Idle;
        self.snap = None;
    }
}

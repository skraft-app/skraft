//! Profile: edits the blank profile of the open part (docs/ARCHITECTURE.md §6 nonies, step 3).
//! The outline is drawn on the cap facing the eye with grips — its corners, its arcs' middles,
//! its straight sides' middles (to insert a corner) and the cap's middle (the thickness). A
//! grip is taken by a click and dropped by another (SketchUp's Move; a drag works too), Esc
//! dropping the gesture; each gesture is one `Operation::SetProfile`, under the open part's
//! scope.
//!
//! - The sides keep their direction (« The shape kept while editing »): a corner follows the
//!   cursor, inferred with the lines through where it was and brought onto the cap's plane, its
//!   two sides translated through it, the corners beyond sliding along their sides (a rectangle
//!   stays one); a straight side's middle moves that side across itself, its neighbours
//!   extended. Typed: a distance along the move, or a point.
//! - Free, the copy modifier tapped (before or during the gesture, dropped once done): a corner
//!   moves alone, its sides following; a side's middle inserts a corner there, which moves
//!   alone.
//! - Either way the arcs are fitted again as they lay ([`profile::rearced`]): a half circle
//!   between parallel sides stays tangent to both, an arc tangent to a straight side stays so,
//!   any other keeps its sagitta. A whole circle (a cylinder's) has a single grip, its radius.
//! - Fillets: hovering a sharp corner between two straight sides shows a small grip on its
//!   bisector, just inside; dragged, it rounds the corner (typed: the radius). A rounded corner's
//!   grip sits at its arc's middle, always shown; brought back onto the corner (10 px) it makes
//!   it sharp again, as Delete over it does. The radius stops where the side runs out.
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
    BLUE, Command, Cx, INK, Key, MAGENTA, OptionValue, PICK, PointShape, Preview, PreviewLabel, PreviewLine, PreviewPoint, Tool, ToolInput,
    ToolKind, ToolOption, copy_key, dashed,
};
use crate::infer::Inference;
use crate::vcb::{self, PointRef, Vcb, VcbKind, VcbValue};

/// How near a grip (pixels) the cursor takes it.
const GRIP_PX: f64 = 10.0;
/// How near the half circle or the chord (pixels) an arc's middle snaps to it.
const SNAP_PX: f64 = 10.0;
/// How far inside a sharp corner (pixels) its fillet grip shows.
const FILLET_INSET_PX: f64 = 18.0;
/// The smallest length or thickness (mm).
const MIN: f64 = 1e-3;
/// How far an arc turns between two points of its preview (radians).
const STEP: f64 = 0.06;

/// A grip of the outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grip {
    Corner(usize),
    /// The middle of a straight side: the side moved across itself, or (free) a corner
    /// inserted there.
    Side(usize),
    /// The middle of an arc: its bulge.
    Bulge(usize),
    /// A whole circle's radius (its only grip but the thickness).
    Radius,
    /// The fillet of a corner: at its arc's middle, or just inside a sharp corner hovered.
    Fillet(usize),
    Thickness,
}

#[derive(Default)]
pub struct ProfileTool {
    state: State,
    /// The cap the outline is drawn on while a grip is taken: the top (z0 + height) or the
    /// bottom (z0). Otherwise the cap facing the eye ([`ProfileTool::cap`]).
    top: bool,
    snap: Option<Inference>,
    /// Free moves (the copy modifier tapped): a corner alone, a corner inserted in a side.
    free: bool,
    /// How far inside a sharp corner its fillet grip shows (axes, [`FILLET_INSET_PX`]).
    inset: f64,
}

#[derive(Default, Clone)]
enum State {
    #[default]
    Idle,
    /// The grip under the cursor.
    Over(Grip),
    /// A grip taken: the profile as it was (a corner inserted already for a side's middle taken
    /// free), the grip, where it was (axes), the profile now, whether it is dragged (dropped
    /// on release), where the grip went (world, typed points) and the cursor's last place.
    Moving { grip: Grip, from: DVec3, base: BlankProfile, now: BlankProfile, drag: bool, world: (DVec3, DVec3), px: Option<DVec2> },
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

/// Is the fillet of the corner `i` made (a radius that fits)?
fn rounded_at(b: &BlankProfile, i: usize) -> bool {
    let r = b.profile[i].fillet;
    r > 0.0 && profile::fillet_max(&b.profile, i).is_some_and(|m| r <= m + 1e-9)
}

/// Where a grip is (axes); a sharp corner's fillet grip `inset` inside it.
fn grip_at(b: &BlankProfile, top: bool, g: Grip, inset: f64) -> DVec3 {
    let z = cap_z(b, top);
    let n = b.profile.len();
    match g {
        Grip::Corner(i) => b.profile[i].p.extend(z),
        Grip::Side(i) | Grip::Bulge(i) => Side::new(b.profile[i].p, b.profile[(i + 1) % n].p, b.profile[i].bulge).at(0.5).extend(z),
        Grip::Radius => b.profile[0].p.extend(z),
        Grip::Fillet(i) => {
            let p = &b.profile;
            let at = if rounded_at(b, i) {
                profile::fillet_middle(p, i, p[i].fillet)
            } else {
                profile::fillet_bisector(p, i).map(|u| p[i].p + u * inset)
            };
            at.unwrap_or(p[i].p).extend(z)
        }
        Grip::Thickness => {
            let (lo, hi) = profile::bounds(&b.outline()).unwrap_or_default();
            ((lo + hi) / 2.0).extend(z)
        }
    }
}

/// From each corner a fillet rounds, the stretches of its two sides the fillet cuts off.
fn fillet_stubs(b: &BlankProfile) -> Vec<(DVec2, DVec2)> {
    let p = &b.profile;
    let n = p.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (before, next) = ((i + n - 1) % n, (i + 1) % n);
        if p[i].fillet <= 0.0 || p[i].bulge != 0.0 || p[before].bulge != 0.0 {
            continue;
        }
        let (Some(u_in), Some(u_out)) = ((p[i].p - p[before].p).try_normalize(), (p[next].p - p[i].p).try_normalize()) else {
            continue;
        };
        let turn = u_in.perp_dot(u_out).atan2(u_in.dot(u_out)).abs();
        let t = p[i].fillet * (turn / 2.0).tan();
        let (l_in, l_out) = (p[i].p.distance(p[before].p), p[i].p.distance(p[next].p));
        out.push((p[i].p, p[i].p - u_in * t.min(l_in)));
        out.push((p[i].p, p[i].p + u_out * t.min(l_out)));
    }
    out
}

/// Every grip of the outline: the fillets' of the rounded corners, and of the corner `hover`
/// (hovered, itself or its fillet grip).
fn grips(b: &BlankProfile, hover: Option<usize>) -> Vec<Grip> {
    if profile::circle(&b.profile).is_some() {
        return vec![Grip::Radius, Grip::Thickness];
    }
    let n = b.profile.len();
    let mut out: Vec<Grip> = (0..n).map(Grip::Corner).collect();
    out.extend((0..n).map(|i| if b.profile[i].bulge == 0.0 { Grip::Side(i) } else { Grip::Bulge(i) }));
    out.extend(
        (0..n).filter(|&i| rounded_at(b, i) || hover == Some(i) && profile::fillet_bisector(&b.profile, i).is_some()).map(Grip::Fillet),
    );
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

/// How far inside a sharp corner its fillet grip shows, at this zoom (axes).
fn inset(cx: &Cx<'_>, b: &BlankProfile, top: bool) -> f64 {
    let (lo, hi) = profile::bounds(&b.outline()).unwrap_or_default();
    cx.camera.world_per_pixel(cx.size, cx.to_world(((lo + hi) / 2.0).extend(cap_z(b, top)))) * FILLET_INSET_PX
}

impl ProfileTool {
    /// The corner hovered, by itself or its fillet grip (its fillet grip then shows).
    fn hovered(&self) -> Option<usize> {
        match self.state {
            State::Over(Grip::Corner(i) | Grip::Fillet(i)) | State::Moving { grip: Grip::Fillet(i), .. } => Some(i),
            _ => None,
        }
    }

    /// The corner a free move takes (an inserted corner for a side's middle), if the gesture
    /// moves one corner alone.
    fn lone(&self, grip: Grip) -> Option<usize> {
        match grip {
            Grip::Corner(i) if self.free => Some(i),
            Grip::Side(i) if self.free => Some(i + 1),
            _ => None,
        }
    }

    /// `base` with the grip `grip` brought to `q` (axes, on the cap): a corner alone (free) or
    /// with its sides keeping their direction, a side across itself from `from`.
    fn placed(&self, grip: Grip, from: DVec3, base: &BlankProfile, q: DVec2) -> Option<BlankProfile> {
        let profile = match (grip, self.lone(grip)) {
            (_, Some(i)) => {
                let mut p = base.profile.clone();
                p[i].p = q;
                p
            }
            (Grip::Corner(i), None) => profile::corner_moved(&base.profile, i, q)?,
            (Grip::Side(i), None) => profile::side_moved(&base.profile, i, q - from.truncate())?,
            (Grip::Radius, _) => {
                let (c, _) = profile::circle(&base.profile)?;
                return Some(BlankProfile { profile: profile::with_radius(&base.profile, c.distance(q).max(MIN))?, ..base.clone() });
            }
            _ => return None,
        };
        Some(BlankProfile { profile: profile::rearced(&base.profile, &profile), ..base.clone() })
    }

    /// Where the grip moved is in `now` (axes): a corner, or a side's middle.
    /// The length the grip taken changes, as the measurements box takes it ([`Tool::vcb`]):
    /// the way a corner or a side went (dashed), an arc's bulge, a fillet's or a circle's
    /// radius, the thickness. `b` is the profile now.
    fn labels(&self, cx: &Cx<'_>, b: &BlankProfile, top: bool, inset: f64, lines: &mut Vec<PreviewLine>) -> Vec<PreviewLabel> {
        let State::Moving { grip, from, .. } = &self.state else { return Vec::new() };
        let label = |a: DVec3, c: DVec3, color| {
            let len = a.distance(c);
            let text = vcb::format(len, cx.units());
            PreviewLabel { a: cx.to_world(a), b: cx.to_world(c), text, color, slot: Some(0), hidden: len < MIN, ..Default::default() }
        };
        let (p, z) = (&b.profile, cap_z(b, top));
        let n = p.len();
        let label = match *grip {
            Grip::Corner(_) | Grip::Side(_) => {
                let to = self.grip_now(*grip, b, from.z);
                dashed(cx.camera, cx.size, lines, cx.to_world(*from), cx.to_world(to), INK);
                label(*from, to, INK)
            }
            Grip::Bulge(i) => {
                let s = Side::new(p[i].p, p[(i + 1) % n].p, p[i].bulge);
                label(((s.a() + s.b()) / 2.0).extend(z), s.at(0.5).extend(z), MAGENTA)
            }
            // From the fillet's middle to its centre, on the bisector.
            Grip::Fillet(i) => {
                let r = p[i].fillet;
                let m = profile::fillet_middle(p, i, r).unwrap_or(p[i].p);
                let u = profile::fillet_bisector(p, i).unwrap_or_default();
                label(m.extend(z), (m + u * r).extend(z), MAGENTA)
            }
            Grip::Radius => {
                let c = profile::circle(p).map_or(p[0].p, |(c, _)| c);
                label(c.extend(z), grip_at(b, top, Grip::Radius, inset), INK)
            }
            Grip::Thickness => {
                let g = grip_at(b, top, Grip::Thickness, inset).truncate();
                label(g.extend(b.z0), g.extend(b.z0 + b.height), BLUE)
            }
        };
        vec![label]
    }

    fn grip_now(&self, grip: Grip, now: &BlankProfile, z: f64) -> DVec3 {
        match (grip, self.lone(grip)) {
            (_, Some(i)) => now.profile[i].p.extend(z),
            (g, None) => grip_at(now, self.top, g, self.inset),
        }
    }

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
        grips(b, self.hovered())
            .into_iter()
            .filter_map(|g| Some((g, cx.camera.project(cx.to_world(grip_at(b, self.cap(cx), g, self.inset)), cx.size)?.distance(px))))
            .filter(|(_, d)| *d <= GRIP_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    }

    /// The profile with the grip moved by the cursor at `px`, and what it snapped to.
    fn moved(&self, cx: &Cx<'_>, px: DVec2, grip: Grip, from: DVec3, base: &BlankProfile) -> Option<(BlankProfile, Option<Inference>)> {
        let z = cap_z(base, self.top);
        let n = base.profile.len();
        match grip {
            Grip::Corner(_) | Grip::Side(_) | Grip::Radius => {
                let plane = (cx.to_world(DVec3::new(0.0, 0.0, z)), cx.axis(2));
                let inf = cx.infer_from(px, |_| true, Some(plane), cx.to_world(from), cx.lock())?;
                let q = cx.to_axes(inf.point).truncate();
                Some((self.placed(grip, from, base, q)?, Some(inf)))
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
            Grip::Fillet(i) => {
                let p = &base.profile;
                let corner = p[i].p;
                let q = cx.axes_ray(px).hit_plane(DVec3::new(0.0, 0.0, z), DVec3::Z)?.truncate();
                let near_corner = cx.camera.project(cx.to_world(corner.extend(z)), cx.size).is_some_and(|w| w.distance(px) <= SNAP_PX);
                let r = if near_corner {
                    0.0
                } else {
                    let d = (q - corner).dot(profile::fillet_bisector(p, i)?);
                    profile::fillet_through(p, i, d)?.min(profile::fillet_max(p, i)?)
                };
                let mut b = base.clone();
                b.profile[i].fillet = if r < MIN { 0.0 } else { r };
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
        self.done();
        vec![Command::Apply(Operation::SetProfile { target, profile: now, scope: cx.part_scope })]
    }

    /// Takes the grip `grip` of `b`.
    fn take(&mut self, b: &BlankProfile, grip: Grip, drag: bool) {
        let base = match grip {
            Grip::Side(i) if self.free => inserted(b, i),
            _ => b.clone(),
        };
        let from = self.grip_now(grip, &base, cap_z(&base, self.top));
        self.state = State::Moving { grip, from, now: base.clone(), base, drag, world: (DVec3::ZERO, DVec3::ZERO), px: None };
    }

    /// Follows the cursor at `px` with the grip taken.
    fn follow(&mut self, cx: &Cx<'_>, px: DVec2) {
        let State::Moving { grip, from, base, drag, .. } = self.state.clone() else { return };
        if let Some((now, snap)) = self.moved(cx, px, grip, from, &base) {
            self.snap = snap.filter(|i| i.kind != crate::infer::InferKind::OnPlane);
            let world = (cx.to_world(from), cx.to_world(self.grip_now(grip, &now, from.z)));
            self.state = State::Moving { grip, from, base, now, drag, world, px: Some(px) };
        }
    }

    /// The gesture under way taken again from `b` (free toggled), following the cursor.
    fn retake(&mut self, cx: &Cx<'_>, b: &BlankProfile) {
        let State::Moving { grip, drag, px, .. } = self.state.clone() else { return };
        self.take(b, grip, drag);
        if let Some(px) = px {
            self.follow(cx, px);
        }
    }

    /// The gesture is over: free moves drop.
    fn done(&mut self) {
        self.state = State::Idle;
        self.snap = None;
        self.free = false;
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
        self.inset = inset(cx, &b, self.top);
        match (input, self.state.clone()) {
            (ToolInput::Key { key: Key::Copy, .. }, state) => {
                self.free = !self.free;
                if matches!(state, State::Moving { .. }) {
                    self.retake(cx, &b);
                }
            }
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
            (ToolInput::Hover { px } | ToolInput::Drag { px }, State::Moving { .. }) => self.follow(cx, *px),
            (ToolInput::Click { px, .. }, State::Moving { grip, from, base, drag: false, .. })
            | (ToolInput::DragEnd { px, .. }, State::Moving { grip, from, base, drag: true, .. }) => {
                if let Some((now, _)) = self.moved(cx, *px, grip, from, &base) {
                    return self.apply(cx, now);
                }
            }
            (ToolInput::Key { key: Key::Delete, .. }, State::Over(Grip::Corner(i))) => return self.apply(cx, removed(&b, i)),
            (ToolInput::Key { key: Key::Delete, .. }, State::Over(Grip::Fillet(i))) => {
                let mut sharp = b.clone();
                sharp.profile[i].fillet = 0.0;
                return self.apply(cx, sharp);
            }
            (ToolInput::Value(VcbValue::Lengths(v)), State::Moving { grip: Grip::Fillet(i), base, .. }) => {
                let r = v[0].abs();
                if profile::fillet_max(&base.profile, i).is_none_or(|m| r > m + 1e-9) {
                    return vec![Command::Say(t!("tool.profile.say.fillet_too_big"))];
                }
                let mut b = base.clone();
                b.profile[i].fillet = if r < MIN { 0.0 } else { r };
                return self.apply(cx, b);
            }
            (ToolInput::Value(v), State::Moving { grip, from, base, now, .. }) => {
                let n = base.profile.len();
                let typed = match (grip, v) {
                    // A distance along the move made so far.
                    (Grip::Corner(_) | Grip::Side(_), VcbValue::Lengths(v)) => {
                        let o = from.truncate();
                        let d = (self.grip_now(grip, &now, from.z).truncate() - o).try_normalize();
                        d.and_then(|d| self.placed(grip, from, &base, o + d * v[0]))
                    }
                    (Grip::Corner(_) | Grip::Side(_), VcbValue::Point(w)) => self.placed(grip, from, &base, cx.to_axes(*w).truncate()),
                    (Grip::Radius, VcbValue::Lengths(v)) if v[0].abs() >= MIN => {
                        profile::with_radius(&base.profile, v[0].abs()).map(|profile| BlankProfile { profile, ..base.clone() })
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
        // The outline on the cap (its fillets made arcs), and on the other one with the sides
        // while moving.
        let outline = b.outline();
        let ring = |z: f64| {
            let pts = profile::sample(&outline, STEP);
            let n = pts.len();
            (0..n).map(move |i| (pts[i].extend(z), pts[(i + 1) % n].extend(z))).collect::<Vec<_>>()
        };
        let top = self.cap(cx);
        let z = cap_z(&b, top);
        lines.extend(ring(z).into_iter().map(|(a, c)| PreviewLine { bold: true, ..line(a, c, PICK) }));
        if grip.is_some() {
            let other = cap_z(&b, !top);
            lines.extend(ring(other).into_iter().map(|(a, c)| line(a, c, PICK)));
            lines.extend(outline.iter().map(|c| line(c.p.extend(z), c.p.extend(other), PICK)));
            // Where it was: the outline as it is, dashed.
            for (a, c) in profile::sample(&part.outline(), STEP).windows(2).map(|w| (w[0], w[1])) {
                let zp = cap_z(&part, top);
                dashed(cx.camera, cx.size, &mut lines, cx.to_world(a.extend(zp)), cx.to_world(c.extend(zp)), INK);
            }
        }
        // A rounded corner's grip is off the outline: its sides run on to it, dashed.
        for (a, c) in fillet_stubs(&b) {
            dashed(cx.camera, cx.size, &mut lines, cx.to_world(a.extend(z)), cx.to_world(c.extend(z)), INK);
        }
        let over = match &self.state {
            State::Over(g) => Some(*g),
            State::Moving { grip, .. } => Some(match self.lone(*grip) {
                Some(i) => Grip::Corner(i),
                None => *grip,
            }),
            State::Idle => None,
        };
        let inset = inset(cx, &b, top);
        for g in grips(&b, self.hovered()) {
            let at = cx.to_world(grip_at(&b, top, g, inset));
            let hot = over == Some(g);
            let (radius, shape, stroke) = match g {
                Grip::Corner(_) => (5.0, PointShape::Round, INK),
                Grip::Side(_) => (3.0, PointShape::Round, INK),
                Grip::Bulge(_) => (5.0, PointShape::Diamond, MAGENTA),
                Grip::Radius => (5.0, PointShape::Diamond, INK),
                Grip::Fillet(_) => (4.0, PointShape::Round, MAGENTA),
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
        let labels = self.labels(cx, &b, top, inset, &mut lines);
        let tip = self.free.then(|| t!("tool.profile.free"));
        Preview { lines, points, labels, inference: self.snap.clone(), tip, hide_selection: true, ..Default::default() }
    }

    fn status(&self) -> String {
        let key = copy_key();
        let hint = match &self.state {
            State::Moving { grip: Grip::Bulge(_), .. } => t!("tool.profile.hint.bulge"),
            State::Moving { grip: Grip::Thickness, .. } => t!("tool.profile.hint.thickness"),
            State::Moving { grip: Grip::Radius, .. } => t!("tool.profile.hint.radius"),
            State::Moving { grip: Grip::Fillet(_), .. } => t!("tool.profile.hint.fillet"),
            State::Moving { grip: Grip::Side(_), .. } if !self.free => t!("tool.profile.hint.side", copy_key = key),
            State::Moving { .. } => t!("tool.profile.hint.corner", copy_key = key),
            _ => t!("tool.profile.hint.idle", copy_key = key),
        };
        if self.free { format!("{hint} {}", t!("tool.profile.freely")) } else { hint }
    }

    fn options(&self, _: &Cx<'_>) -> Vec<ToolOption> {
        vec![ToolOption {
            key: "free",
            label: t!("tool.profile.option.free"),
            tip: t!("tool.profile.option.free_tip", copy_key = copy_key()),
            value: OptionValue::Toggle(self.free),
            choices: Vec::new(),
            saved: false,
            icon: None,
        }]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        if let ("free", OptionValue::Toggle(on)) = (key, value) {
            self.free = *on;
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        let State::Moving { grip, from, base, now, world, .. } = &self.state else { return None };
        let n = base.profile.len();
        match grip {
            Grip::Corner(_) | Grip::Side(_) => {
                let to = self.grip_now(*grip, now, from.z);
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
            Grip::Fillet(i) => Some(Vcb::new(t!("tool.profile.vcb.fillet"), VcbKind::Lengths(vec![now.profile[*i].fillet]))),
            Grip::Radius => {
                let r = profile::circle(&now.profile).map_or(0.0, |(_, r)| r);
                Some(Vcb::new(t!("tool.profile.vcb.radius"), VcbKind::Lengths(vec![r])))
            }
            Grip::Thickness => Some(Vcb::new(t!("tool.profile.vcb.thickness"), VcbKind::Lengths(vec![now.height]))),
        }
    }

    fn busy(&self) -> bool {
        matches!(self.state, State::Moving { .. })
    }

    fn takes_delete(&self) -> bool {
        matches!(self.state, State::Over(Grip::Corner(_) | Grip::Fillet(_)))
    }

    fn leave(&mut self) {
        if !matches!(self.state, State::Moving { .. }) {
            self.state = State::Idle;
        }
    }

    fn back(&mut self) {
        self.done();
    }

    fn reset(&mut self) {
        self.done();
    }
}

//! Profile: edits the profile of a body of the open part (docs/tools/prism.md,
//! docs/model/bodies.md), in that body's axes: the one double-clicked (in Select or here, while the bodies
//! show), else its first profiled positive body.
//! The outline is drawn where the profile lies — an extrusion's bottom cap (its `z0`), a turned
//! body's XZ plane — whatever the view, with grips — its corners, its arcs' middles,
//! its straight sides' middles (to insert a corner). The thickness is the Stretch tool's (along
//! the body's Z, docs/model/bodies.md). A grip is taken by a click and dropped by another (SketchUp's Move; a drag works too), Esc
//! dropping the gesture; each gesture is one `Operation::SetProfile`, under the open part's
//! scope.
//!
//! - The sides keep their direction (« The shape kept while editing »): a corner follows the
//!   cursor, inferred with the lines through where it was and brought onto the cap's plane, its
//!   two sides translated through it, the corners beyond sliding along their sides (a rectangle
//!   stays one); a straight side's middle moves that side across itself, its neighbours
//!   extended. Typed: a distance along the move, or a point.
//!   Its grips say so: a corner's a square, a straight side's middle a bar along the side.
//! - Free, the copy modifier tapped (before or during the gesture, dropped once done): a corner
//!   moves alone, its sides following; a side's middle inserts a corner there, which moves
//!   alone. Their grips are round.
//! - Either way the arcs are fitted again as they lay ([`profile::rearced`]): a half circle
//!   between parallel sides stays tangent to both, an arc tangent to a straight side stays so,
//!   any other keeps its sagitta. A whole circle (a cylinder's) has a single grip, its radius.
//! - Fillets: hovering a sharp corner between two straight sides shows a small grip on its
//!   bisector, just inside; dragged, it rounds the corner (typed: the radius). A rounded corner's
//!   grip sits at its arc's middle, always shown; brought back onto the corner (10 px) it makes
//!   it sharp again, as Delete over it does. The radius stops where the side runs out.
//! - An arc's middle sets its bulge (the arc through the cursor), snapping to the half circle
//!   and to straight within 10 px. Typed: its height (sagitta).
//! - Delete over a corner takes it away.
//!
//! An outline that would cross itself is refused, said in the status bar.
//!
//! A turned body's half profile (docs/tools/turned-bodies.md) is edited the same way in its XZ plane, with two
//! rules: a corner or a side
//! is stopped on the axis, never beyond it; a corner on the axis slides along it, and a side
//! lying on the axis stays there (no grip across it unless free). An arc bulging across the
//! axis is refused.

use glam::{DAffine3, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::kernel::profile::{self, Corner, Side};
use skraft_model::{BodyProfile, EntityPath, Id, Operation};

use super::{
    Command, Cx, INK, Key, MAGENTA, OptionValue, PICK, PointShape, Preview, Tool, ToolInput, ToolKind, ToolOption, WHITE, copy_key,
};
use crate::draw::{Drawer, Drawing, Pen};
use crate::infer::Inference;
use crate::vcb::{PointRef, Vcb, VcbKind, VcbValue};

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
    /// A whole circle's radius (its only grip).
    Radius,
    /// The fillet of a corner: at its arc's middle, or just inside a sharp corner hovered.
    Fillet(usize),
}

#[derive(Default)]
pub struct ProfileTool {
    state: State,
    snap: Option<Inference>,
    /// Free moves (the copy modifier tapped): a corner alone, a corner inserted in a side.
    free: bool,
    /// How far inside a sharp corner its fillet grip shows (axes, [`FILLET_INSET_PX`]).
    inset: f64,
    /// The body whose profile is edited; none: the open part's first profiled positive body.
    body: Option<Id>,
}

/// How near the axis (mm) a turned profile's corner lies on it.
const ON_AXIS: f64 = 1e-6;

#[derive(Default, Clone)]
enum State {
    #[default]
    Idle,
    /// The grip under the cursor.
    Over(Grip),
    /// A grip taken: the profile as it was (a corner inserted already for a side's middle taken
    /// free), the grip, where it was (axes), the profile now, whether it is dragged (dropped
    /// on release), where the grip went (world, typed points) and the cursor's last place.
    Moving { grip: Grip, from: DVec3, base: BodyProfile, now: BodyProfile, drag: bool, world: (DVec3, DVec3), px: Option<DVec2> },
}

/// The open part's path, the body `body` (none: its first profiled positive one), its
/// profile and its placement in the part's axes.
fn open(cx: &Cx<'_>, body: Option<Id>) -> Option<(EntityPath, Id, BodyProfile, DAffine3)> {
    let path = cx.open_part_path()?;
    let (_, part, _) = cx.doc.resolve_container(&path).ok()?;
    let b = match body {
        Some(id) => part.body(id)?,
        None => part.profiled()?,
    };
    Some((path, b.id, part.profile_of(b)?.clone(), b.placement))
}

/// The frame the profile is edited in (in the part's axes): the body's, for a turned body its
/// XZ plane made the frame's XY (the profile's `(x, y)` its `(x, z)`).
fn edit_frame(b: &BodyProfile, placement: DAffine3) -> DAffine3 {
    if !b.is_turned() {
        return placement;
    }
    placement * DAffine3::from_cols(DVec3::X, DVec3::Z, DVec3::NEG_Y, DVec3::ZERO)
}

/// Do the corners `i` and `i + 1` of a turned profile both lie on the axis (the side between
/// them on it)?
fn on_axis_side(b: &BodyProfile, i: usize) -> bool {
    let n = b.profile.len();
    b.is_turned() && b.profile[i].p.x.abs() < ON_AXIS && b.profile[(i + 1) % n].p.x.abs() < ON_AXIS && b.profile[i].bulge == 0.0
}

/// `cx` with its axes moved by `t` (in them): the tools' frame becomes a body's.
fn framed<'a>(cx: &Cx<'a>, t: DAffine3) -> Cx<'a> {
    Cx { axes_in_context: cx.axes_in_context * t, ..cx.clone() }
}

/// The height the profile lies at: an extrusion's bottom cap (0 in a turned body's frame).
fn base_z(b: &BodyProfile) -> f64 {
    b.extrusion().map_or(0.0, |(z0, _)| z0)
}

/// Is the fillet of the corner `i` made (a radius that fits)?
fn rounded_at(b: &BodyProfile, i: usize) -> bool {
    let r = b.profile[i].fillet;
    r > 0.0 && profile::fillet_max(&b.profile, i).is_some_and(|m| r <= m + 1e-9)
}

/// Where a grip is (axes); a sharp corner's fillet grip `inset` inside it.
fn grip_at(b: &BodyProfile, g: Grip, inset: f64) -> DVec3 {
    let z = base_z(b);
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
    }
}

/// From each corner a fillet rounds, the stretches of its two sides the fillet cuts off.
fn fillet_stubs(b: &BodyProfile) -> Vec<(DVec2, DVec2)> {
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
fn grips(b: &BodyProfile, hover: Option<usize>) -> Vec<Grip> {
    if profile::circle(&b.profile).is_some() {
        return vec![Grip::Radius];
    }
    let n = b.profile.len();
    let mut out: Vec<Grip> = (0..n).map(Grip::Corner).collect();
    out.extend((0..n).map(|i| if b.profile[i].bulge == 0.0 { Grip::Side(i) } else { Grip::Bulge(i) }));
    out.extend(
        (0..n).filter(|&i| rounded_at(b, i) || hover == Some(i) && profile::fillet_bisector(&b.profile, i).is_some()).map(Grip::Fillet),
    );
    out
}

/// The profile with a corner inserted in the middle of its straight side `i`: the new corner is
/// `i + 1`.
fn inserted(b: &BodyProfile, i: usize) -> BodyProfile {
    let n = b.profile.len();
    let mid = (b.profile[i].p + b.profile[(i + 1) % n].p) / 2.0;
    let mut p = b.profile.clone();
    p.insert(i + 1, Corner::new(mid));
    BodyProfile { profile: p, ..b.clone() }
}

/// The profile without its corner `i`, the side before it running on straight to the next one.
fn removed(b: &BodyProfile, i: usize) -> BodyProfile {
    let n = b.profile.len();
    let mut p = b.profile.clone();
    p.remove(i);
    let before = (i + n - 1) % n;
    let before = if before > i { before - 1 } else { before };
    if let Some(c) = p.get_mut(before) {
        c.bulge = 0.0;
    }
    BodyProfile { profile: p, ..b.clone() }
}

/// How far inside a sharp corner its fillet grip shows, at this zoom (axes).
fn inset(cx: &Cx<'_>, b: &BodyProfile) -> f64 {
    let (lo, hi) = profile::bounds(&b.outline()).unwrap_or_default();
    cx.camera.world_per_pixel(cx.size, cx.to_world(((lo + hi) / 2.0).extend(base_z(b)))) * FILLET_INSET_PX
}

impl ProfileTool {
    /// The tool on the body `id` of the open part.
    pub fn of_body(id: Id) -> Self {
        ProfileTool { body: Some(id), ..Default::default() }
    }

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
    fn placed(&self, grip: Grip, from: DVec3, base: &BodyProfile, q: DVec2) -> Option<BodyProfile> {
        if !base.is_turned() {
            return self.placed_anywhere(grip, from, base, q);
        }
        // A turned profile: a corner on the axis slides along it; a side on it stays.
        let mut q = q;
        match (grip, self.lone(grip)) {
            (_, Some(i)) | (Grip::Corner(i), None) if base.profile[i].p.x.abs() < ON_AXIS => q.x = 0.0,
            (Grip::Side(i), None) if on_axis_side(base, i) => return None,
            _ => {}
        }
        let turnable = |b: &BodyProfile| profile::is_turnable(&b.profile);
        if let Some(b) = self.placed_anywhere(grip, from, base, q).filter(turnable) {
            return Some(b);
        }
        // Across the axis: stopped on it, as far along the move as it stays on its side.
        let o = from.truncate();
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..40 {
            let m = (lo + hi) / 2.0;
            if self.placed_anywhere(grip, from, base, o + (q - o) * m).is_some_and(|b| turnable(&b)) {
                lo = m;
            } else {
                hi = m;
            }
        }
        self.placed_anywhere(grip, from, base, o + (q - o) * lo).filter(turnable)
    }

    /// [`ProfileTool::placed`], whatever the axis.
    fn placed_anywhere(&self, grip: Grip, from: DVec3, base: &BodyProfile, q: DVec2) -> Option<BodyProfile> {
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
                return Some(BodyProfile { profile: profile::with_radius(&base.profile, c.distance(q).max(MIN))?, ..base.clone() });
            }
            _ => return None,
        };
        Some(BodyProfile { profile: profile::rearced(&base.profile, &profile), ..base.clone() })
    }

    /// Where the grip moved is in `now` (axes): a corner, or a side's middle.
    /// The length the grip taken changes, as the measurements box takes it ([`Tool::vcb`]):
    /// the way a corner or a side went (dashed), an arc's bulge, a fillet's or a circle's
    /// radius. `b` is the profile now.
    fn lengths(&self, cx: &Cx<'_>, d: &mut Drawer<'_>, b: &BodyProfile, inset: f64) {
        let State::Moving { grip, from, px, .. } = &self.state else { return };
        let px = *px;
        let (p, z) = (&b.profile, base_z(b));
        let n = p.len();
        let dashed = Pen::dimension(INK);
        match *grip {
            Grip::Corner(_) | Grip::Side(_) => {
                let to = self.grip_now(*grip, b, from.z);
                d.pen(dashed).line(*from, to).length_always(*from, to, INK, Some(0));
            }
            Grip::Bulge(i) => {
                let s = Side::new(p[i].p, p[(i + 1) % n].p, p[i].bulge);
                d.length_always(((s.a() + s.b()) / 2.0).extend(z), s.at(0.5).extend(z), MAGENTA, Some(0));
            }
            // From the fillet's middle to its centre, on the bisector.
            Grip::Fillet(i) => {
                let r = p[i].fillet;
                let m = profile::fillet_middle(p, i, r).unwrap_or(p[i].p);
                let u = profile::fillet_bisector(p, i).unwrap_or_default();
                d.length_always(m.extend(z), (m + u * r).extend(z), MAGENTA, Some(0));
            }
            // From the centre towards the cursor, the radius long, dashed.
            Grip::Radius => {
                let Some((c, r)) = profile::circle(p) else { return };
                let toward = px
                    .and_then(|px| cx.axes_ray(px).hit_plane(DVec3::new(0.0, 0.0, z), DVec3::Z))
                    .and_then(|q| (q.truncate() - c).try_normalize());
                let to = toward.map_or(grip_at(b, Grip::Radius, inset), |u| (c + u * r).extend(z));
                d.pen(dashed).line(c.extend(z), to).length_always(c.extend(z), to, INK, Some(0));
            }
        }
    }

    fn grip_now(&self, grip: Grip, now: &BodyProfile, z: f64) -> DVec3 {
        match (grip, self.lone(grip)) {
            (_, Some(i)) => now.profile[i].p.extend(z),
            (g, None) => grip_at(now, g, self.inset),
        }
    }

    /// The grips shown: a side lying on a turned profile's axis has none, unless free.
    fn grips_of(&self, b: &BodyProfile) -> Vec<Grip> {
        let mut out = grips(b, self.hovered());
        out.retain(|g| !matches!(g, Grip::Side(i) if !self.free && on_axis_side(b, *i)));
        out
    }

    /// The grip under `px`, the nearest within reach.
    fn grip_under(&self, cx: &Cx<'_>, b: &BodyProfile, px: DVec2) -> Option<Grip> {
        self.grips_of(b)
            .into_iter()
            .filter_map(|g| Some((g, cx.camera.project(cx.to_world(grip_at(b, g, self.inset)), cx.size)?.distance(px))))
            .filter(|(_, d)| *d <= GRIP_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    }

    /// The profile with the grip moved by the cursor at `px`, and what it snapped to.
    fn moved(&self, cx: &Cx<'_>, px: DVec2, grip: Grip, from: DVec3, base: &BodyProfile) -> Option<(BodyProfile, Option<Inference>)> {
        let z = base_z(base);
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
        }
    }

    /// The gesture done with `now`: the operation, or why not.
    fn apply(&mut self, cx: &Cx<'_>, now: BodyProfile) -> Vec<Command> {
        let Some((target, body, _, _)) = open(cx, self.body) else {
            self.state = State::Idle;
            return vec![];
        };
        if !now.is_valid() {
            return vec![Command::Say(t!("tool.profile.say.crossed"))];
        }
        self.done();
        vec![Command::Apply(Operation::SetProfile { target, body, profile: now, scope: cx.part_scope })]
    }

    /// Takes the grip `grip` of `b`.
    fn take(&mut self, b: &BodyProfile, grip: Grip, drag: bool) {
        let base = match grip {
            Grip::Side(i) if self.free => inserted(b, i),
            _ => b.clone(),
        };
        let from = self.grip_now(grip, &base, base_z(&base));
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
    fn retake(&mut self, cx: &Cx<'_>, b: &BodyProfile) {
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

impl ProfileTool {
    /// [`Tool::input`], `cx` in the body's axes.
    fn input_in(&mut self, input: &ToolInput, cx: &Cx<'_>, b: BodyProfile) -> Vec<Command> {
        self.inset = inset(cx, &b);
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
                // Off the part: as Select's click off (docs/model/opening-a-part.md).
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
                        profile::with_radius(&base.profile, v[0].abs()).map(|profile| BodyProfile { profile, ..base.clone() })
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

    /// [`Tool::preview`], `cx` in the body's axes.
    fn preview_in(&self, cx: &Cx<'_>, part: BodyProfile) -> Preview {
        let (b, grip) = match &self.state {
            State::Moving { now, grip, .. } => (now.clone(), Some(*grip)),
            _ => (part.clone(), None),
        };
        let mut drawing = Drawing::default();
        let d = &mut cx.axes_drawer(&mut drawing);
        // The outline where it lies (its fillets made arcs), and on the top cap with the sides
        // while moving.
        let outline = b.outline();
        let ring = |z: f64| profile::sample(&outline, STEP).into_iter().map(|p| p.extend(z)).collect::<Vec<_>>();
        let z = base_z(&b);
        let pick = Pen::new(PICK).over();
        d.pen(pick.bold()).curve(&ring(z), true);
        // While moving, an extrusion's top cap and sides too.
        if let (Some(_), Some((z0, height))) = (grip, b.extrusion()) {
            let other = z0 + height;
            d.pen(pick).curve(&ring(other), true);
            d.segments(outline.iter().map(|c| (c.p.extend(z), c.p.extend(other))));
        }
        // While moving, where it was: the outline as it is, dashed.
        let dashed = Pen::new(INK).over().dashed();
        if grip.is_some() {
            let zp = base_z(&part);
            let was: Vec<DVec3> = profile::sample(&part.outline(), STEP).into_iter().map(|p| p.extend(zp)).collect();
            d.pen(dashed).curve(&was, true);
        }
        // A rounded corner's grip is off the outline: its sides run on to it, dashed.
        d.pen(dashed).segments(fillet_stubs(&b).into_iter().map(|(a, c)| (a.extend(z), c.extend(z))));
        let over = match &self.state {
            State::Over(g) => Some(*g),
            State::Moving { grip, .. } => Some(match self.lone(*grip) {
                Some(i) => Grip::Corner(i),
                None => *grip,
            }),
            State::Idle => None,
        };
        let inset = inset(cx, &b);
        for g in self.grips_of(&b) {
            let at = grip_at(&b, g, inset);
            let hot = over == Some(g);
            // Free, a corner and a side's middle move alone: round; else a corner keeps its
            // sides' lines (a square) and a side moves across itself (a bar along it).
            let (radius, shape, stroke) = match g {
                Grip::Corner(_) if self.free => (5.0, PointShape::Round, INK),
                Grip::Side(_) if self.free => (3.0, PointShape::Round, INK),
                Grip::Corner(_) => (4.0, PointShape::Square, INK),
                Grip::Side(i) => {
                    let (a, c) = (b.profile[i].p.extend(z), b.profile[(i + 1) % b.profile.len()].p.extend(z));
                    (2.5, PointShape::Bar { along: (c - a) / 2.0 }, INK)
                }
                Grip::Bulge(_) => (5.0, PointShape::Diamond, MAGENTA),
                Grip::Radius => (5.0, PointShape::Diamond, INK),
                Grip::Fillet(_) => (4.0, PointShape::Round, MAGENTA),
            };
            let radius = if hot { radius + 2.0 } else { radius };
            d.grip(at, radius, Some(if hot { PICK } else { WHITE }), Some(stroke), shape);
        }
        self.lengths(cx, d, &b, inset);
        let tip = self.free.then(|| t!("tool.profile.free"));
        Preview { drawing, inference: self.snap.clone(), tip, hide_selection: true, ..Default::default() }
    }
}

impl Tool for ProfileTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Profile
    }

    fn profile_body(&self, cx: &Cx<'_>) -> Option<Id> {
        open(cx, self.body).map(|(_, id, ..)| id)
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        // A double-click on another profiled body edits it.
        if let (ToolInput::DoubleClick { px, .. }, false) = (input, self.busy())
            && let Some(body) = cx.profiled_body_at(*px)
        {
            return vec![Command::EditProfile(body)];
        }
        let Some((_, _, b, t)) = open(cx, self.body) else {
            self.state = State::Idle;
            return vec![];
        };
        self.input_in(input, &framed(cx, edit_frame(&b, t)), b)
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let Some((_, _, b, t)) = open(cx, self.body) else { return Preview::default() };
        self.preview_in(&framed(cx, edit_frame(&b, t)), b)
    }

    fn status(&self) -> String {
        let key = copy_key();
        let hint = match &self.state {
            State::Moving { grip: Grip::Bulge(_), .. } => t!("tool.profile.hint.bulge"),
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
            swatch: None,
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

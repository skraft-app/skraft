//! Prism: click the corners of an outline, close it, then pull its height (docs/
//! ARCHITECTURE.md §6 nonies), after OpenCutList's `SmartDrawPolygonActionHandler`. The part
//! made is a prism, its profile kept in its blank's recipe.
//!
//! - The first point is the box's (the plane aimed at, the arrows locking it, the copy modifier
//!   held for a face's centroid and Shift tapped then for its whole outline, the other modifier
//!   to measure from a corner). A plane locked there stays for the whole outline.
//! - Then each click adds a corner, inferred with the lines through the last one; the arrows
//!   lock an axis through it. The plane follows OCL's: through the first corner as the box's
//!   second corner sets it, then through the first two and the cursor's point, set from three
//!   corners on (or from the first arc). From three corners, the cursor snaps to a placed corner
//!   near it, and a click there closes the outline (as a double-click, or Enter); from two, it
//!   snaps to the foot of a placed corner on the line it draws (OCL's alignment).
//! - Sides: the choice « Tracé » (Tab) — a line, an arc tangent to the side before, or an arc
//!   by two points (its end, then its bulge, SketchUp's). SolidCraft's gesture switches lines
//!   and tangent arcs: leave the last corner, come back onto it and rest there. The side closing
//!   the outline follows the mode.
//! - The copy modifier tapped reverses the way corners are added (before the first instead of
//!   after the last); Shift tapped adds a guide from the last corner towards the cursor.
//! - Esc drops an arc under way, else takes away the last corner placed; from the height, back
//!   to the corners.
//! - The height is the box's.
//! - The "Offset" option (the box's and the cylinder's, `3x` at any step) moves the outline
//!   made ([`profile::offset`], as SolidCraft's sketch offset, a side it turns over taken out as
//!   Clipper does), then the "Fillet" option (`5r`) rounds its corners between straight sides,
//!   recorded as radii on them ([`profile::with_fillets`], made arcs by [`profile::rounded`]):
//!   the outline drawn is dashed then, the one made solid.

use std::f64::consts::PI;

use super::*;
use crate::tools::{PointShape, PreviewPoint};
use crate::vcb::PointRef;
use solidsketch_model::{GuideShape, Operation};

/// The option: new corners go before the first one (OCL's `measure_reversed`, saved).
const REVERSED: &str = "reversed";
/// The choice: what the next side is (not saved: each outline starts with lines).
const TRACE: &str = "trace";
const LINE: &str = "line";
const TANGENT: &str = "tangent";
const ARC: &str = "arc";
/// How near a placed corner (pixels) the cursor snaps to it.
const CORNER_PX: f64 = 10.0;
/// How near the foot of a placed corner (pixels) the cursor snaps to it (OCL's 30 px aperture,
/// across).
const ALIGN_PX: f64 = 15.0;
/// SolidCraft's switch to tangent arcs: how far the cursor leaves the last corner first, how
/// near it comes back, how still it rests there (pixels) and how long (seconds).
const AWAY_PX: f64 = 20.0;
const ON_PX: f64 = 8.0;
const STILL_PX: f64 = 2.0;
const REST_S: f64 = 0.35;

/// What the next side is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Line,
    /// An arc tangent to the side before.
    Tangent,
    /// An arc by its end, then its bulge.
    Arc,
}

impl Mode {
    fn value(self) -> &'static str {
        match self {
            Mode::Line => LINE,
            Mode::Tangent => TANGENT,
            Mode::Arc => ARC,
        }
    }
}

#[derive(Default)]
pub struct PrismTool {
    state: State,
    first: First,
    mode: Mode,
    /// The cursor's point while placing corners.
    cursor: Option<Cursor>,
    snap: Option<Inference>,
    /// The saved options, as the editor last gave them.
    pull_centered: bool,
    reversed: bool,
    /// Where a click just closed the outline: the double-click that may follow there is that
    /// click's, not a call for the last height.
    closed_at: Option<DVec2>,
    /// The cursor resting on the last corner (SolidCraft's switch).
    rest: Rest,
}

/// Where the cursor has been against the last corner: away from it since it was placed (or
/// the mode switched), and resting on it since when.
#[derive(Clone, Copy, Debug, Default)]
struct Rest {
    left: bool,
    since: Option<(DVec2, f64)>,
}

#[derive(Default, Clone)]
enum State {
    #[default]
    First,
    Corners(Corners),
    /// The outline closed: the profile (in `f`'s plane, from its origin), the corner it was
    /// closed on (axes), and the height so far.
    Height {
        corners: Corners,
        f: Frame,
        profile: Vec<Corner>,
        end: DVec3,
        h: f64,
    },
}

/// The corners placed (axes coordinates) and the plane they set.
#[derive(Clone, Debug)]
struct Corners {
    /// Along the outline.
    along: Vec<DVec3>,
    /// Per corner, the middle of the side leaving it when an arc (the last: the closing side).
    mids: Vec<Option<DVec3>>,
    /// In the order they were placed (Esc takes the last away).
    placed: Vec<DVec3>,
    /// The plane's normal so far (towards the eye); set from three corners on, from the first
    /// arc, or when locked.
    n: DVec3,
    /// The normal locked at the first point (the arrows, ↓), for the whole outline.
    locked: Option<DVec3>,
    /// The direction the first click gave (an edge aimed at): the sides of the second corner's
    /// plane.
    dir: Option<DVec3>,
    /// An arc by two points under way: its end is set, its bulge follows the cursor.
    pending: Option<Pending>,
}

/// The end of an arc by two points, the plane it is drawn in, and whether it closes the outline.
#[derive(Clone, Copy, Debug)]
struct Pending {
    end: DVec3,
    n: DVec3,
    closes: bool,
}

/// The cursor's point while placing corners: where it is (axes), the plane's normal with it,
/// the placed corner it snaps to (an index along the outline), or the corner whose foot on the
/// line drawn it snaps to; for an arc, its middle; a tangent arc ending straight behind.
#[derive(Clone, Copy, Debug)]
struct Cursor {
    p: DVec3,
    /// The anchor and the point, in the world (the measurements box's typed points).
    world: (DVec3, DVec3),
    n: DVec3,
    on_corner: Option<usize>,
    aligned: Option<DVec3>,
    mid: Option<DVec3>,
    behind: bool,
}

impl Corners {
    fn new(p: DVec3, n: DVec3, locked: Option<DVec3>, dir: Option<DVec3>) -> Corners {
        Corners { along: vec![p], mids: vec![None], placed: vec![p], n: locked.unwrap_or(n), locked, dir, pending: None }
    }

    fn arcs(&self) -> bool {
        self.mids.iter().any(Option::is_some)
    }

    /// Is the plane set (from three corners, an arc, or locked)?
    fn plane_set(&self) -> bool {
        self.locked.is_some() || self.along.len() >= 3 || self.arcs()
    }

    /// The plane's frame, on the first corner.
    fn frame(&self) -> Frame {
        Frame::new(self.along[0], self.n, None)
    }

    /// The corner new sides start from: the last one, or the first, reversed.
    fn anchor(&self, reversed: bool) -> DVec3 {
        if reversed { self.along[0] } else { self.along[self.along.len() - 1] }
    }

    /// The other end of the outline: where it closes back to.
    fn other_end(&self, reversed: bool) -> DVec3 {
        if reversed { self.along[self.along.len() - 1] } else { self.along[0] }
    }

    /// The side from the corner `i` to the next one, in the plane.
    fn side(&self, f: &Frame, i: usize) -> profile::Side {
        let n = self.along.len();
        side_in(f, self.along[i], self.mids[i], self.along[(i + 1) % n])
    }

    /// The unit direction (axes) the outline runs into the anchor: along the side ending there,
    /// or, reversed, back along the side leaving the first corner. None with no side yet.
    fn incoming(&self, reversed: bool) -> Option<DVec3> {
        let k = self.along.len();
        if k < 2 {
            return None;
        }
        let f = self.frame();
        let t = if reversed { -self.side(&f, 0).tangents().0 } else { self.side(&f, k - 2).tangents().1 };
        (f.u * t.x + f.v * t.y).try_normalize()
    }

    /// Adds the corner `p` (with the plane `n`), the side reaching it an arc through `mid` or
    /// straight.
    fn add(&mut self, p: DVec3, n: DVec3, mid: Option<DVec3>, reversed: bool) {
        // The plane follows the cursor's until set.
        let set = self.plane_set();
        if reversed {
            self.along.insert(0, p);
            self.mids.insert(0, mid);
        } else {
            let k = self.mids.len();
            self.mids[k - 1] = mid;
            self.along.push(p);
            self.mids.push(None);
        }
        self.placed.push(p);
        if !set {
            self.n = n;
        }
    }

    /// Takes the last corner placed away; `false` when none is left.
    fn remove_last(&mut self) -> bool {
        if let Some(p) = self.placed.pop()
            && let Some(i) = self.along.iter().position(|q| *q == p)
        {
            let n = self.along.len();
            // The side into it now runs on to the next corner: straight again.
            let before = (i + n - 1) % n;
            self.along.remove(i);
            self.mids.remove(i);
            if !self.along.is_empty() {
                let before = if before > i { before - 1 } else { before.min(self.along.len() - 1) };
                self.mids[before] = None;
            }
        }
        !self.along.is_empty()
    }

    /// The outline in the plane of `f`, closed by its last side.
    fn outline(&self, f: &Frame) -> Vec<Corner> {
        let n = self.along.len();
        (0..n).map(|i| Corner::arc(local2(f, self.along[i]), bulge_in(f, self.along[i], self.mids[i], self.along[(i + 1) % n]))).collect()
    }
}

/// `p` in the plane of `f`.
fn local2(f: &Frame, p: DVec3) -> DVec2 {
    f.local(p).truncate()
}

/// The point `p` of the plane of `f`.
fn at3(f: &Frame, p: DVec2) -> DVec3 {
    f.at(p.extend(0.0))
}

/// The bulge of the side from `a` to `b` through `mid` (straight without), in the plane of `f`.
fn bulge_in(f: &Frame, a: DVec3, mid: Option<DVec3>, b: DVec3) -> f64 {
    mid.map_or(0.0, |m| profile::bulge_through(local2(f, a), local2(f, m), local2(f, b)))
}

/// The side from `a` to `b` through `mid`, in the plane of `f`.
fn side_in(f: &Frame, a: DVec3, mid: Option<DVec3>, b: DVec3) -> profile::Side {
    profile::Side::new(local2(f, a), local2(f, b), bulge_in(f, a, mid, b))
}

/// Points along the side from `a` to `b` through `mid` (axes).
fn side_points(f: &Frame, a: DVec3, mid: Option<DVec3>, b: DVec3) -> Vec<DVec3> {
    side_in(f, a, mid, b).points(STEP).into_iter().map(|p| at3(f, p)).collect()
}

/// The middle of the arc from `a`, leaving along `t` (unit) in the plane of normal `n`, to `q`:
/// none when `q` lies straight ahead (a line); `Err` straight behind, where no arc goes.
fn tangent_mid(a: DVec3, t: DVec3, q: DVec3, n: DVec3) -> Result<Option<DVec3>, ()> {
    let d = q - a;
    if d.length() < MIN {
        return Ok(None);
    }
    let phi = n.dot(t.cross(d)).atan2(t.dot(d));
    if phi.abs() > PI - 1e-6 {
        return Err(());
    }
    if phi.abs() < 1e-9 {
        return Ok(None);
    }
    // Turning by φ from its tangent to its chord, the arc sweeps 2φ: its bulge is tan(φ / 2).
    let f = Frame::new(a, n, None);
    let s = profile::Side::new(DVec2::ZERO, local2(&f, q), (phi / 2.0).tan());
    Ok(Some(at3(&f, s.at(0.5))))
}

/// A face's outline: each corner, with the middle of the side leaving it when an arc.
type Outline3 = Vec<(DVec3, Option<DVec3>)>;

/// The ordered outer outline of the planar face `face` (world), its sides straight or arcs: the
/// corners, each with the middle of the side leaving it when an arc; its sides chained into
/// loops, the one of largest area. A disc is two half circles. None when a side is another curve.
fn outer_loop(face: &Face) -> Option<Outline3> {
    const SAME: f64 = 1e-3;
    if face.sides.is_empty() && face.arcs.is_empty() {
        let (c, r) = face.disc?;
        let u = face.normal.any_orthonormal_vector();
        let v = face.normal.cross(u);
        return Some(vec![(c + u * r, Some(c + v * r)), (c - u * r, Some(c - v * r))]);
    }
    let mut sides: Vec<(DVec3, DVec3, Option<DVec3>)> =
        face.sides.iter().map(|(a, b)| (*a, *b, None)).chain(face.arcs.iter().map(|(a, b, m)| (*a, *b, Some(*m)))).collect();
    let mut best: Option<(f64, Outline3)> = None;
    while let Some((a, b, m)) = sides.pop() {
        let mut lp = vec![(a, m)];
        let mut end = b;
        while end.distance(a) > SAME {
            let i = sides.iter().position(|(p, q, _)| p.distance(end) < SAME || q.distance(end) < SAME)?;
            let (p, q, m) = sides.swap_remove(i);
            lp.push((end, m));
            end = if p.distance(end) < SAME { q } else { p };
        }
        // The area of the polygon through the corners and the arcs' middles, to tell the outer
        // loop.
        let pts: Vec<DVec3> = lp.iter().flat_map(|(p, m)| std::iter::once(*p).chain(*m)).collect();
        let area = (0..pts.len()).map(|i| pts[i].cross(pts[(i + 1) % pts.len()])).sum::<DVec3>().dot(face.normal).abs();
        if pts.len() >= 3 && best.as_ref().is_none_or(|(a, _)| area > *a) {
            best = Some((area, lp));
        }
    }
    // Every corner of the face on a side (none on another curve).
    let (_, lp) = best?;
    let on = |c: &DVec3| {
        face.sides.iter().any(|(a, b)| a.distance(*c) < SAME || b.distance(*c) < SAME)
            || face.arcs.iter().any(|(a, b, _)| a.distance(*c) < SAME || b.distance(*c) < SAME)
    };
    face.corners.iter().all(on).then_some(lp)
}

/// Why the offset refuses the outline, for the status bar.
fn offset_refused(e: profile::OffsetError) -> String {
    match e {
        profile::OffsetError::Empty => t!("tool.shape.say.offset_empty"),
        profile::OffsetError::Crossed => t!("tool.prism.say.offset_crossed"),
    }
}

/// The outline made from the one drawn: offset, then its fillets' radius on its corners
/// (§ module doc). Its shape: [`profile::rounded`].
fn made(cx: &Cx<'_>, drawn: &[Corner]) -> Result<Vec<Corner>, profile::OffsetError> {
    let o = profile::offset(drawn, cx.length(OFFSET))?;
    Ok(profile::with_fillets(&o, cx.length(FILLET)))
}

/// Does the outline made differ from the one drawn (an offset or a fillet)?
fn changed(cx: &Cx<'_>) -> bool {
    cx.length(OFFSET) != 0.0 || cx.length(FILLET) > 0.0
}

/// The middle of a profile's box (in its plane).
fn middle(p: &[Corner]) -> DVec2 {
    profile::bounds(p).map_or(DVec2::ZERO, |(lo, hi)| (lo + hi) / 2.0)
}

impl PrismTool {
    /// The cursor's point under `px` while placing corners (§ module doc), and what it snapped to.
    fn point_at(&self, cx: &Cx<'_>, px: DVec2, c: &Corners) -> Option<(Cursor, Option<Inference>)> {
        let a = c.anchor(self.reversed);
        let k = c.along.len();
        let ray = cx.axes_ray(px).dir;
        let cursor = |p: DVec3, n: DVec3, on_corner, aligned| Cursor {
            p,
            world: (cx.to_world(a), cx.to_world(p)),
            n,
            on_corner,
            aligned,
            mid: None,
            behind: false,
        };
        // The arrow keys' axis, unless it leaves a plane already set.
        let lock = cx.lock().filter(|l| !c.plane_set() || dir_to_axes(cx, l.dir).dot(c.n).abs() < SQUARE);
        // A placed corner near the cursor, once the outline can close (on the locked axis, its
        // foot).
        if self.closable(c) {
            let near = c
                .along
                .iter()
                .enumerate()
                .filter_map(|(i, p)| Some((i, cx.camera.project(cx.to_world(*p), cx.size)?.distance(px))))
                .filter(|(_, d)| *d <= CORNER_PX)
                .min_by(|x, y| x.1.total_cmp(&y.1));
            if let Some((i, _)) = near {
                let p = c.along[i];
                let p = match lock {
                    Some(l) => {
                        let d = dir_to_axes(cx, l.dir);
                        a + d * (p - a).dot(d)
                    }
                    None => p,
                };
                return Some((cursor(p, c.n, Some(i), None), None));
            }
        }
        let plane = (cx.to_world(a), dir_to_world(cx, c.n));
        let inf = cx.infer_from(px, |_| true, Some(plane), cx.to_world(a), lock)?;
        let q = cx.to_axes(inf.point);
        let (mut p, n, snap) = match (k, c.locked) {
            // A plane locked: the point brought onto it.
            (_, Some(n)) => (Frame::new(a, n, None).flatten(q), n, Some(inf)),
            // Set (an arc drawn already): brought onto it.
            _ if c.plane_set() => (Frame::new(a, c.n, None).flatten(q), c.n, Some(inf)),
            // One corner: the box's second corner's plane through it.
            (1, None) => {
                let f = Frame::new(c.along[0], c.n, c.dir);
                let (g, p, snap) = second_plane(cx, px, &f, c.dir, inf);
                (p, g.n, snap)
            }
            // Two: the plane through both and the cursor's point.
            _ => {
                let n = (c.along[1] - c.along[0]).cross(q - c.along[0]).try_normalize().map_or(c.n, |n| facing(n, ray));
                (q, n, Some(inf))
            }
        };
        let mut snap = snap.filter(|i| i.kind != InferKind::OnPlane);
        // From two corners, off any feature: the foot of a placed corner on the line drawn.
        let mut aligned = None;
        if k >= 2
            && !snap.as_ref().is_some_and(|i| i.kind.is_feature())
            && let Some(d) = (p - a).try_normalize()
        {
            let feet = c.along.iter().filter(|q| **q != a).map(|q| (*q, a + d * (*q - a).dot(d)));
            let near = feet
                .filter_map(|(q, f)| Some((q, f, cx.camera.project(cx.to_world(f), cx.size)?.distance(px))))
                .filter(|(_, _, dist)| *dist <= ALIGN_PX)
                .min_by(|x, y| x.2.total_cmp(&y.2));
            if let Some((q, f, _)) = near {
                (p, aligned, snap) = (f, Some(q), None);
            }
        }
        Some((cursor(p, n, None, aligned), snap))
    }

    /// Can a click on a placed corner close the outline now: three corners, or two and an arc
    /// (drawn, or the next side's)?
    fn closable(&self, c: &Corners) -> bool {
        c.along.len() >= 3 || c.along.len() == 2 && (c.arcs() || self.mode != Mode::Line)
    }

    /// The cursor under `px` for the side the mode draws: an arc's middle with it, or, an arc by
    /// two points under way, its bulge.
    fn cursor_at(&self, cx: &Cx<'_>, px: DVec2, c: &Corners) -> Option<(Cursor, Option<Inference>)> {
        let a = c.anchor(self.reversed);
        if let Some(pend) = c.pending {
            return self.bulge_at(cx, px, a, pend).map(|cur| (cur, None));
        }
        let (mut cur, snap) = self.point_at(cx, px, c)?;
        if self.mode == Mode::Tangent
            && let Some(t) = c.incoming(self.reversed)
        {
            // On a placed corner, the side closing the outline.
            let to = if cur.on_corner.is_some() { c.other_end(self.reversed) } else { cur.p };
            match tangent_mid(a, t, to, cur.n) {
                Ok(mid) => cur.mid = mid,
                Err(()) => cur.behind = true,
            }
        }
        Some((cur, snap))
    }

    /// The bulge of an arc by two points from `a` to the end set: the cursor's point on its
    /// plane, its distance from the chord the arc's height; near the half circle, that.
    fn bulge_at(&self, cx: &Cx<'_>, px: DVec2, a: DVec3, pend: Pending) -> Option<Cursor> {
        let q = pend.end;
        let chord = q - a;
        let l = chord.length();
        let u = chord.try_normalize()?;
        let left = pend.n.cross(u);
        let p = cx.axes_ray(px).hit_plane(a, pend.n)?;
        let mut s = (p - a).dot(left);
        let half = a + chord / 2.0 + left * (l / 2.0) * s.signum();
        if cx.camera.project(cx.to_world(half), cx.size).is_some_and(|h| h.distance(px) <= SQUARE_PX) {
            s = (l / 2.0) * s.signum();
        }
        let mid = (s.abs() >= MIN).then(|| a + chord / 2.0 + left * s);
        Some(Cursor { p: q, world: (cx.to_world(a), cx.to_world(q)), n: pend.n, on_corner: None, aligned: None, mid, behind: false })
    }

    /// A corner placed at `p` (with the plane `n`), the side to it through `mid`: added, the
    /// locks dropped.
    fn add(&mut self, mut c: Corners, p: DVec3, n: DVec3, mid: Option<DVec3>) -> Vec<Command> {
        if p.distance(c.anchor(self.reversed)) < MIN {
            self.state = State::Corners(c);
            return vec![];
        }
        c.add(p, n, mid, self.reversed);
        self.state = State::Corners(c);
        self.cursor = None;
        self.snap = None;
        self.rest = Rest::default();
        vec![Command::ClearLocks]
    }

    /// The outline closed on the corner `end`, its closing side through `mid`: on to the height,
    /// or a word if it cannot be.
    fn close(&mut self, mut c: Corners, end: DVec3, mid: Option<DVec3>) -> Vec<Command> {
        let k = c.mids.len();
        c.mids[k - 1] = mid;
        c.pending = None;
        if c.along.len() < 2 || c.along.len() == 2 && !c.arcs() {
            c.mids[k - 1] = None;
            self.state = State::Corners(c);
            return vec![Command::Say(t!("tool.prism.say.too_few"))];
        }
        let f = c.frame();
        let outline = profile::cleaned(&c.outline(&f));
        if !profile::is_simple(&outline) {
            c.mids[k - 1] = None;
            self.state = State::Corners(c);
            return vec![Command::Say(t!("tool.prism.say.crossed"))];
        }
        self.state = State::Height { corners: c, f, profile: outline, end, h: 0.0 };
        self.cursor = None;
        self.snap = None;
        vec![Command::ClearLocks]
    }

    /// The side closing the outline in the current mode: a tangent arc's middle, or for an arc
    /// by two points, its bulge still to come (`Err` with the end set).
    fn closing(&self, c: &Corners) -> Result<Option<DVec3>, Option<Pending>> {
        let (a, to) = (c.anchor(self.reversed), c.other_end(self.reversed));
        match self.mode {
            Mode::Line => Ok(None),
            Mode::Tangent => match c.incoming(self.reversed) {
                Some(t) => tangent_mid(a, t, to, c.n).map_err(|()| None),
                None => Ok(None),
            },
            Mode::Arc => Err(Some(Pending { end: to, n: c.n, closes: true })),
        }
    }

    /// Closes the outline (Enter, a double-click) with the mode's closing side.
    fn close_now(&mut self, mut c: Corners) -> Vec<Command> {
        let end = c.anchor(self.reversed);
        match self.closing(&c) {
            Ok(mid) => self.close(c, end, mid),
            Err(Some(pend)) if self.closable(&c) => {
                c.pending = Some(pend);
                self.state = State::Corners(c);
                vec![]
            }
            Err(Some(_)) => self.close(c, end, None),
            Err(None) => {
                self.state = State::Corners(c);
                vec![Command::Say(t!("tool.prism.say.behind"))]
            }
        }
    }

    /// A click at the cursor `cur` while placing corners.
    fn clicked(&mut self, mut c: Corners, cur: Cursor) -> Vec<Command> {
        // The bulge of an arc by two points: the arc made.
        if let Some(pend) = c.pending {
            if pend.closes {
                let end = c.anchor(self.reversed);
                return self.close(c, end, cur.mid);
            }
            c.pending = None;
            return self.add(c, pend.end, pend.n, cur.mid);
        }
        let on_corner = cur.on_corner.filter(|&i| cur.p == c.along[i]);
        if self.mode == Mode::Arc {
            // The end of an arc by two points: its bulge next.
            let pend = match on_corner {
                Some(_) => Pending { end: c.other_end(self.reversed), n: c.n, closes: true },
                None if cur.p.distance(c.anchor(self.reversed)) < MIN => {
                    self.state = State::Corners(c);
                    return vec![];
                }
                None => Pending { end: cur.p, n: cur.n, closes: false },
            };
            c.pending = Some(pend);
            self.state = State::Corners(c);
            return vec![Command::ClearLocks];
        }
        if cur.behind {
            self.state = State::Corners(c);
            return vec![Command::Say(t!("tool.prism.say.behind"))];
        }
        if let Some(i) = on_corner {
            let end = c.along[i];
            return self.close(c, end, cur.mid);
        }
        self.add(c, cur.p, cur.n, cur.mid)
    }

    /// The prism on `profile` (in `f`'s plane), pulled to `h`.
    fn finish(&mut self, cx: &Cx<'_>, f: Frame, outline: Vec<Corner>, h: f64) -> Vec<Command> {
        let (z0, z1) = span(h, self.pull_centered);
        if z1 - z0 < MIN {
            return vec![Command::Say(t!("tool.prism.say.empty"))];
        }
        // The outline made (§ module doc); the step stays when it cannot be.
        let outline = match made(cx, &outline) {
            Ok(o) => o,
            Err(e) => return vec![Command::Say(offset_refused(e))],
        };
        self.state = State::First;
        let (primitive, placement) = if f.aligned() {
            // In the axes: the corners as clicked, no rounding through a frame.
            (Primitive::Prism { origin: f.o + DVec3::Z * z0, profile: outline, height: z1 - z0 }, DAffine3::IDENTITY)
        } else {
            (Primitive::Prism { origin: DVec3::new(0.0, 0.0, z0), profile: outline, height: z1 - z0 }, f.placement())
        };
        vec![Command::Apply(cx.add_primitive(primitive, placement)), Command::RememberHeight(z1 - z0)]
    }

    /// The height step ends at `h`.
    fn pulled(&mut self, cx: &Cx<'_>, h: f64) -> Vec<Command> {
        let State::Height { f, profile, .. } = &self.state else { return vec![] };
        let (f, profile) = (*f, profile.clone());
        if h.abs() < MIN {
            if let State::Height { h: hh, .. } = &mut self.state {
                *hh = h;
            }
            return vec![Command::Say(t!("tool.prism.say.empty"))];
        }
        self.finish(cx, f, profile, h)
    }

    /// The guide from the anchor towards the cursor (Shift tapped).
    fn guide(&self, cx: &Cx<'_>, c: &Corners) -> Vec<Command> {
        let Some(cur) = self.cursor else { return vec![] };
        let a = c.anchor(self.reversed);
        let (wa, wp) = (cx.to_world(a), cx.to_world(cur.p));
        let Some(dir) = (wp - wa).try_normalize() else { return vec![] };
        let (component, frame) = cx.guide_owner();
        let shape = GuideShape::Line { origin: wa, dir };
        vec![Command::Apply(Operation::AddGuide { component, shape: shape.transformed(&frame.inverse()) })]
    }

    /// SolidCraft's switch between lines and tangent arcs: the cursor at `px` against the last
    /// corner, now; switched after a rest on it, once it has been away.
    fn follow_rest(&mut self, cx: &Cx<'_>, px: DVec2, c: &Corners) {
        let switchable = matches!(self.mode, Mode::Line | Mode::Tangent) && c.along.len() >= 2 && c.pending.is_none();
        let Some(at) = cx.camera.project(cx.to_world(c.anchor(self.reversed)), cx.size).filter(|_| switchable) else {
            self.rest = Rest::default();
            return;
        };
        let off = at.distance(px);
        if off >= AWAY_PX {
            self.rest.left = true;
        }
        if off > ON_PX || !self.rest.left {
            self.rest.since = None;
            return;
        }
        match self.rest.since {
            Some((p, t)) if p.distance(px) <= STILL_PX => {
                if cx.now - t >= REST_S {
                    self.mode = if self.mode == Mode::Line { Mode::Tangent } else { Mode::Line };
                    self.rest = Rest::default();
                }
            }
            _ => self.rest.since = Some((px, cx.now)),
        }
    }

    /// The radius of the tangent arc the cursor gives, and its turn from the tangent.
    fn tangent_radius(&self, c: &Corners, cur: &Cursor) -> Option<(f64, f64, DVec3)> {
        let a = c.anchor(self.reversed);
        let t = c.incoming(self.reversed)?;
        let d = cur.p - a;
        let phi = cur.n.dot(t.cross(d)).atan2(t.dot(d));
        (phi.abs() > 1e-9).then(|| (d.length() / (2.0 * phi.sin().abs()), phi, t))
    }

    /// A value typed while placing corners.
    fn typed(&mut self, cx: &Cx<'_>, c: Corners, v: &VcbValue) -> Vec<Command> {
        let a = c.anchor(self.reversed);
        let Some(cur) = self.cursor else {
            self.state = State::Corners(c);
            return vec![];
        };
        // A typed point, brought onto the plane once it is set.
        let onto = |q: DVec3| if c.plane_set() { Frame::new(c.along[0], c.n, None).flatten(q) } else { q };
        match (v, c.pending, self.mode) {
            // The bulge's height, the way the cursor gives.
            (VcbValue::Lengths(v), Some(pend), _) => {
                let (q, l) = (pend.end, pend.end.distance(a));
                let side = cur.mid.map_or(1.0, |m| (m - (a + q) / 2.0).dot(pend.n.cross((q - a) / l)).signum());
                let mid = (v[0].abs() >= MIN).then(|| (a + q) / 2.0 + pend.n.cross((q - a) / l) * v[0].abs() * side);
                let cur = Cursor { mid, ..cur };
                self.clicked(c, cur)
            }
            // The tangent arc's radius, the cursor keeping its turn.
            (VcbValue::Lengths(v), None, Mode::Tangent) => match self.tangent_radius(&c, &cur) {
                Some((_, phi, t)) if v[0].abs() >= MIN => {
                    let n = cur.n;
                    let d = t * phi.cos() + n.cross(t) * phi.sin();
                    let q = a + d * (2.0 * v[0].abs() * phi.sin().abs());
                    let mid = tangent_mid(a, t, q, n).ok().flatten();
                    self.add(c, q, n, mid)
                }
                _ => {
                    self.state = State::Corners(c);
                    vec![]
                }
            },
            // A length along the cursor's direction: the next corner, or an arc's end.
            (VcbValue::Lengths(v), None, _) => {
                let Some(d) = (cur.p - a).try_normalize() else {
                    self.state = State::Corners(c);
                    return vec![];
                };
                let q = a + d * v[0];
                let cur = Cursor { p: q, on_corner: None, mid: None, ..cur };
                let cur = match (self.mode, c.incoming(self.reversed)) {
                    (Mode::Tangent, Some(t)) => Cursor { mid: tangent_mid(a, t, q, cur.n).ok().flatten(), ..cur },
                    _ => cur,
                };
                self.clicked(c, cur)
            }
            (VcbValue::Point(w), None, _) => {
                let q = onto(cx.to_axes(*w));
                let mut cur = Cursor { p: q, on_corner: None, mid: None, behind: false, ..cur };
                if self.mode == Mode::Tangent
                    && let Some(t) = c.incoming(self.reversed)
                {
                    match tangent_mid(a, t, q, cur.n) {
                        Ok(mid) => cur.mid = mid,
                        Err(()) => cur.behind = true,
                    }
                }
                self.clicked(c, cur)
            }
            _ => {
                self.state = State::Corners(c);
                vec![]
            }
        }
    }
}

impl Tool for PrismTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Prism
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        self.pull_centered = cx.toggle(PULL_CENTERED);
        self.reversed = cx.toggle(REVERSED);
        // Moved away from where the outline was closed.
        if let ToolInput::Hover { px } = input
            && self.closed_at.is_some_and(|c| c.distance(*px) > 0.5)
        {
            self.closed_at = None;
        }
        let state = std::mem::take(&mut self.state);
        match (input, state) {
            // ---- the first point
            (ToolInput::Key { key: Key::Shift, .. }, State::First) if cx.copy_down => {
                // The whole face (its outer outline) as the profile, the height next.
                let face = self.first.at.as_ref().and_then(|s| s.face.as_ref());
                match face.map(|face| (face, outer_loop(face))) {
                    Some((face, Some(lp))) => {
                        let n = dir_to_axes(cx, face.normal);
                        let along: Vec<DVec3> = lp.iter().map(|(p, _)| cx.to_axes(*p)).collect();
                        let mids = lp.iter().map(|(_, m)| m.map(|m| cx.to_axes(m))).collect();
                        let c = Corners { placed: along.clone(), along, mids, n, locked: Some(n), dir: None, pending: None };
                        let (end, closing) = (c.along[0], c.mids[c.mids.len() - 1]);
                        self.first.from_corner = false;
                        return self.close(c, end, closing);
                    }
                    Some((_, None)) => return vec![Command::Say(t!("tool.prism.say.curved_face"))],
                    None => {}
                }
            }
            (ToolInput::Key { key, .. }, State::First) => return first_key(*key, &self.first).into_iter().collect(),
            (ToolInput::Hover { px }, State::First) => self.first.hover(cx, *px),
            (ToolInput::Click { px, .. }, State::First) => {
                if let Some(s) = start_at(cx, *px) {
                    self.state = State::Corners(Corners::new(s.f.o, s.f.n, locked_normal(cx), s.dir));
                    self.first.from_corner = false;
                    self.mode = Mode::Line;
                    self.rest = Rest::default();
                    return vec![Command::ClearLocks];
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), State::First) => {
                if let Some((f, dir)) = self.first.typed(v) {
                    self.state = State::Corners(Corners::new(f.o, f.n, locked_normal(cx), dir));
                    self.first.from_corner = false;
                    self.mode = Mode::Line;
                    self.rest = Rest::default();
                    return vec![Command::ClearLocks];
                }
            }

            // ---- the corners
            (ToolInput::Key { key: Key::Copy, .. }, State::Corners(c)) => {
                self.state = State::Corners(c);
                return vec![Command::SetOption { key: REVERSED, value: OptionValue::Toggle(!self.reversed) }];
            }
            (ToolInput::Key { key: Key::Shift, .. }, State::Corners(c)) => {
                let commands = self.guide(cx, &c);
                self.state = State::Corners(c);
                return commands;
            }
            (ToolInput::Key { key: Key::Enter, .. }, State::Corners(c)) if c.pending.is_none() => return self.close_now(c),
            (ToolInput::Hover { px }, State::Corners(c)) => {
                self.follow_rest(cx, *px, &c);
                if let Some((cur, snap)) = self.cursor_at(cx, *px, &c) {
                    self.cursor = Some(cur);
                    self.snap = snap;
                }
                self.state = State::Corners(c);
            }
            (ToolInput::Click { px, .. }, State::Corners(c)) => {
                let Some((cur, _)) = self.cursor_at(cx, *px, &c) else {
                    self.state = State::Corners(c);
                    return vec![];
                };
                let closes =
                    c.pending.is_none_or(|p| p.closes) && (c.pending.is_some() || cur.on_corner.is_some_and(|i| cur.p == c.along[i]));
                let commands = self.clicked(c, cur);
                if closes && matches!(self.state, State::Height { .. }) {
                    self.closed_at = Some(*px);
                }
                return commands;
            }
            // A double-click: its second click has closed the outline already (or the corner is
            // added and that is all, under three).
            (ToolInput::DoubleClick { px, .. }, State::Corners(c))
                if c.along.len() >= 3 && c.pending.is_none() && self.mode != Mode::Arc =>
            {
                self.closed_at = Some(*px);
                return self.close_now(c);
            }
            (ToolInput::Value(v), State::Corners(c)) => return self.typed(cx, c, v),

            // ---- the height
            (ToolInput::Key { key: Key::Copy, .. }, state @ State::Height { .. }) => {
                self.state = state;
                return vec![Command::SetOption { key: PULL_CENTERED, value: OptionValue::Toggle(!self.pull_centered) }];
            }
            (ToolInput::Hover { px }, State::Height { corners, f, profile, end, .. }) => {
                let m = middle(&profile);
                let (h, snap) = pull_at(cx, *px, &f, end, f.at(m.extend(0.0)), self.pull_centered);
                // No height yet (the cursor still on the plane): no way either, for a typed one.
                let h = if h.abs() < MIN { 0.0 } else { h };
                self.snap = snap;
                self.state = State::Height { corners, f, profile, end, h };
            }
            (ToolInput::Click { .. }, state @ State::Height { .. }) => {
                let State::Height { h, .. } = state else { unreachable!() };
                self.state = state;
                if self.closed_at.is_some() {
                    return vec![];
                }
                return self.pulled(cx, h);
            }
            (ToolInput::DoubleClick { .. }, state @ State::Height { .. }) => {
                self.state = state;
                if self.closed_at.take().is_some() {
                    return vec![];
                }
                if let Some(last) = cx.last_height {
                    return self.pulled(cx, pull_of(last, self.pull_centered));
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), state @ State::Height { .. }) => {
                self.state = state;
                return self.pulled(cx, pull_of(v[0], self.pull_centered));
            }
            (_, state) => self.state = state,
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let mut lines = Vec::new();
        let mut labels = Vec::new();
        let mut marks = Vec::new();
        let mut points = Vec::new();
        let mut inference = self.snap.clone();
        let mut tip = None;
        let corner = |p: DVec3, shape| PreviewPoint { at: cx.to_world(p), radius: 4.0, fill: Some(INK), stroke: None, shape };
        let pairs = |pts: Vec<DVec3>| pts.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>();
        match &self.state {
            State::First => {
                inference = self.first.inference();
                if let Some(s) = &self.first.at {
                    // OCL's small trapezoid at the cursor, in the plane it would take: its first
                    // corner on the cursor, 40 × 20 px along the plane's red and green sides.
                    // With an offset, the one drawn dashed and the one made solid, 10 % larger or
                    // smaller (OCL's `_preview_shape_start`).
                    let f = s.f;
                    let w = pixels(cx, f.o, GAUGE_PX);
                    let color = plane_color(cx, &f);
                    let trapezoid = |off: f64, tip: f64| {
                        let at = |x: f64, y: f64| f.at(DVec3::new(-off + x * (w + 2.0 * off), -off + y * (w / 2.0 + 2.0 * off), 0.0));
                        let c = [at(0.0, 0.0), at(tip, 0.0), at(0.5, 1.0), at(0.0, 1.0)];
                        (0..4).map(move |i| (c[i], c[(i + 1) % 4]))
                    };
                    let off = cx.length(OFFSET);
                    if off != 0.0 {
                        segments(cx, &mut lines, trapezoid(0.0, 1.0), color, true);
                    }
                    let (grow, tip) = match off.partial_cmp(&0.0) {
                        Some(std::cmp::Ordering::Greater) => (w * 0.1, 1.1),
                        Some(std::cmp::Ordering::Less) => (-w * 0.1, 0.9),
                        _ => (0.0, 1.0),
                    };
                    segments(cx, &mut lines, trapezoid(grow, tip), color, false);
                    if snapped(&inference) {
                        mark(cx, &mut marks, f.o);
                    }
                }
                self.first.preview(cx, &mut lines, &mut labels, &mut marks);
            }
            State::Corners(c) => {
                let n = self.cursor.map_or(c.n, |cur| cur.n);
                let f = if c.plane_set() { c.frame() } else { Frame::new(c.along[0], n, None) };
                let color = if c.plane_set() || c.along.len() >= 2 { plane_color(cx, &f) } else { INK };
                // With an offset or a fillet, the outline drawn is dashed and the one made solid.
                let dash = changed(cx);
                let k = c.along.len();
                for i in 0..k - 1 {
                    segments(cx, &mut lines, pairs(side_points(&f, c.along[i], c.mids[i], c.along[i + 1])), color, dash);
                }
                points.extend(c.along.iter().map(|p| corner(*p, PointShape::Round)));
                if let Some(cur) = self.cursor {
                    let a = c.anchor(self.reversed);
                    let to = c.other_end(self.reversed);
                    let along = axis_color(cur.p - a);
                    // The measurements box takes the tangent arc's radius, not the side's length.
                    let length = (!(self.mode == Mode::Tangent && c.incoming(self.reversed).is_some())).then_some(0);
                    match c.pending {
                        // An arc by two points: its chord, and the arc through the cursor's bulge.
                        Some(pend) => {
                            dashed(cx.camera, cx.size, &mut lines, cx.to_world(a), cx.to_world(pend.end), INK);
                            segments(cx, &mut lines, pairs(side_points(&f, a, cur.mid, pend.end)), color, dash);
                            if let Some(m) = cur.mid {
                                mark(cx, &mut marks, m);
                                label(cx, &mut labels, (a + pend.end) / 2.0, m, MAGENTA, Some(0));
                            }
                        }
                        None if self.mode == Mode::Arc => {
                            dashed(cx.camera, cx.size, &mut lines, cx.to_world(a), cx.to_world(cur.p), along.unwrap_or(color));
                            label(cx, &mut labels, a, cur.p, along.unwrap_or(INK), length);
                        }
                        None if cur.mid.is_some() && cur.on_corner.is_none() => {
                            segments(cx, &mut lines, pairs(side_points(&f, a, cur.mid, cur.p)), color, dash);
                            // The tangent arc's radius, from its centre to the cursor.
                            if length.is_none()
                                && let Some((r, phi, t)) = self.tangent_radius(c, &cur)
                                && let Some(left) = cur.n.cross(t).try_normalize()
                            {
                                let centre = a + left * r * phi.signum();
                                dashed(cx.camera, cx.size, &mut lines, cx.to_world(centre), cx.to_world(cur.p), INK);
                                label(cx, &mut labels, centre, cur.p, MAGENTA, Some(0));
                            }
                        }
                        None => {
                            segments(cx, &mut lines, [(a, cur.p)], along.unwrap_or(color), dash);
                            label(cx, &mut labels, a, cur.p, along.unwrap_or(INK), length);
                        }
                    }
                    // Where the outline would close: from the cursor, or, on a placed corner (a
                    // click there closes it; right after a click, the corner just placed), the
                    // closing side itself — a tangent arc's in that mode.
                    if c.pending.is_none() && k >= 2 {
                        let (from, mid) = match cur.on_corner {
                            Some(_) if self.mode == Mode::Tangent => (a, cur.mid),
                            Some(_) => (a, None),
                            None => (cur.p, None),
                        };
                        // Solid as the other sides (dashed with them, under an offset or a fillet).
                        if from.distance(to) >= MIN {
                            segments(cx, &mut lines, pairs(side_points(&f, from, mid, to)), color, dash);
                        }
                    }
                    // The outline made, as it would close with the cursor.
                    if dash {
                        let mut d = c.clone();
                        if c.pending.is_none() && cur.on_corner.is_none() && !cur.behind && self.mode != Mode::Arc {
                            d.add(cur.p, c.n, cur.mid, self.reversed);
                        } else if let Some(pend) = c.pending.filter(|p| !p.closes) {
                            d.add(pend.end, c.n, cur.mid, self.reversed);
                        }
                        let drawn = d.outline(&f);
                        if drawn.len() >= 3
                            && let Ok(o) = made(cx, &drawn)
                        {
                            segments(cx, &mut lines, ring(&f, &profile::rounded(&o), 0.0), color, false);
                        }
                    }
                    if let Some(i) = cur.on_corner {
                        points.push(PreviewPoint { radius: 6.0, ..corner(c.along[i], PointShape::Diamond) });
                    }
                    if let Some(q) = cur.aligned {
                        dashed(cx.camera, cx.size, &mut lines, cx.to_world(q), cx.to_world(cur.p), MAGENTA);
                        points.push(PreviewPoint { stroke: Some(MAGENTA), ..corner(q, PointShape::Round) });
                    }
                    if snapped(&inference) || cur.aligned.is_some() {
                        mark(cx, &mut marks, cur.p);
                    }
                    // The mode by the cursor, for an arc (the inference's tip, on a bare point if
                    // nothing is inferred).
                    let word = match self.mode {
                        Mode::Line => None,
                        Mode::Tangent => Some(t!("tool.prism.option.tangent")),
                        Mode::Arc => Some(t!("tool.prism.option.arc")),
                    };
                    if let Some(word) = word {
                        tip = Some(word);
                        let at = cx.to_world(if c.pending.is_some() { cur.mid.unwrap_or(cur.p) } else { cur.p });
                        inference = inference.or(Some(Inference {
                            point: at,
                            kind: InferKind::OnPlane,
                            path: None,
                            hit: None,
                            line: None,
                            line2: None,
                            edge: None,
                            curve: None,
                            crossed: Vec::new(),
                        }));
                    }
                }
            }
            State::Height { f, profile, end, h, .. } => {
                let color = plane_color(cx, f);
                let (z0, z1) = span(*h, self.pull_centered);
                let prism = |lines: &mut Vec<PreviewLine>, p: &[Corner], dash: bool| {
                    segments(cx, lines, ring(f, p, z0), color, dash);
                    segments(cx, lines, ring(f, p, z1), color, dash);
                    segments(cx, lines, p.iter().map(|c| (f.at(c.p.extend(z0)), f.at(c.p.extend(z1)))), color, dash);
                };
                // With an offset or a fillet, the outline drawn dashed and the prism made solid.
                let dash = changed(cx);
                prism(&mut lines, profile, dash);
                if dash && let Ok(o) = made(cx, profile) {
                    prism(&mut lines, &profile::rounded(&o), false);
                }
                let e = local2(f, *end);
                label(cx, &mut labels, f.at(e.extend(z0)), f.at(e.extend(z1)), BLUE, Some(0));
            }
        }
        Preview { lines, labels, marks, points, inference, tip, ..Default::default() }
    }

    fn status(&self) -> String {
        match &self.state {
            State::First => t!("tool.prism.hint.first", copy_key = copy_key(), alt_key = alt_key()),
            State::Corners(c) if c.pending.is_some() => t!("tool.prism.hint.bulge"),
            State::Corners(_) if self.mode == Mode::Tangent => t!("tool.prism.hint.tangent", copy_key = copy_key()),
            State::Corners(_) if self.mode == Mode::Arc => t!("tool.prism.hint.arc_end", copy_key = copy_key()),
            State::Corners(_) => t!("tool.prism.hint.corners", copy_key = copy_key()),
            State::Height { .. } => t!("tool.shape.hint.height", copy_key = copy_key()),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        match &self.state {
            State::First => self.first.vcb(),
            State::Corners(c) => {
                let a = c.anchor(self.reversed);
                let point = self.cursor.map(|cur| PointRef { from: cur.world.0, to: cur.world.1 });
                match (c.pending, self.mode, self.cursor) {
                    (Some(pend), _, cur) => {
                        let s = cur.and_then(|cur| cur.mid).map_or(0.0, |m| m.distance((a + pend.end) / 2.0));
                        Some(Vcb::new(t!("tool.prism.vcb.bulge"), VcbKind::Lengths(vec![s])))
                    }
                    (None, Mode::Tangent, Some(cur)) if c.incoming(self.reversed).is_some() => {
                        let r = self.tangent_radius(c, &cur).map_or(0.0, |(r, _, _)| r);
                        Some(Vcb { point, ..Vcb::new(t!("tool.prism.vcb.radius"), VcbKind::Lengths(vec![r])) })
                    }
                    _ => {
                        let len = self.cursor.map_or(0.0, |cur| (cur.p - a).length());
                        Some(Vcb { point, ..Vcb::new(t!("tool.prism.vcb.length"), VcbKind::Lengths(vec![len])) })
                    }
                }
            }
            State::Height { h, .. } => Some(Vcb::new(t!("tool.prism.vcb.height"), VcbKind::Lengths(vec![whole(*h, self.pull_centered)]))),
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.state, State::First)
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let choice =
            |value: &'static str, label: String, tip: String, icon: &'static str| OptionChoice { value, label, tip, icon: Some(icon) };
        let trace = ToolOption {
            key: TRACE,
            label: t!("tool.prism.option.trace"),
            tip: t!("tool.prism.option.trace_tip"),
            value: OptionValue::Choice(self.mode.value().into()),
            choices: vec![
                choice(LINE, t!("tool.prism.option.line"), t!("tool.prism.option.line_tip"), "trace_line"),
                choice(TANGENT, t!("tool.prism.option.tangent"), t!("tool.prism.option.tangent_tip"), "trace_tangent"),
                choice(ARC, t!("tool.prism.option.arc"), t!("tool.prism.option.arc_tip"), "trace_arc"),
            ],
            saved: false,
            icon: None,
        };
        let reversed = toggle(
            REVERSED,
            t!("tool.prism.option.reversed"),
            t!("tool.prism.option.reversed_tip", copy_key = copy_key()),
            cx.toggle(REVERSED),
            true,
            "measure_reversed",
        );
        [offset_option(cx), fillet_option(cx), trace, reversed].into_iter().chain(shared_options(cx, &self.first)).collect()
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        match (key, value) {
            (REVERSED, OptionValue::Toggle(on)) => self.reversed = *on,
            (PULL_CENTERED, OptionValue::Toggle(on)) => self.pull_centered = *on,
            (FROM_CORNER, OptionValue::Toggle(on)) => self.first.from_corner = *on,
            (TRACE, OptionValue::Choice(v)) => {
                self.mode = match v.as_str() {
                    TANGENT => Mode::Tangent,
                    ARC => Mode::Arc,
                    _ => Mode::Line,
                };
                // An arc by two points under way is dropped with its mode.
                if let State::Corners(c) = &mut self.state
                    && self.mode != Mode::Arc
                {
                    c.pending = None;
                }
                self.rest = Rest { left: false, since: None };
            }
            _ => {}
        }
    }

    fn down_locks_face(&self) -> bool {
        matches!(self.state, State::First)
    }

    fn waiting(&self) -> bool {
        self.rest.since.is_some()
    }

    fn length_suffix(&self, suffix: char) -> Option<&'static str> {
        match suffix {
            'x' => Some(OFFSET),
            'r' => Some(FILLET),
            _ => None,
        }
    }

    fn leave(&mut self) {
        self.first.leave();
        self.cursor = None;
        self.snap = None;
        self.rest.since = None;
    }

    fn back(&mut self) {
        self.state = match std::mem::take(&mut self.state) {
            State::Height { corners, .. } => State::Corners(corners),
            State::Corners(mut c) if c.pending.is_some() => {
                c.pending = None;
                State::Corners(c)
            }
            State::Corners(mut c) => {
                if c.remove_last() {
                    State::Corners(c)
                } else {
                    State::First
                }
            }
            State::First => State::First,
        };
        self.cursor = None;
        self.snap = None;
    }

    fn reset(&mut self) {
        self.state = State::First;
        self.cursor = None;
        self.snap = None;
        self.mode = Mode::Line;
        self.rest = Rest::default();
    }
}

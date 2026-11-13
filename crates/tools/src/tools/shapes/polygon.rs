//! Polygon: click the corners of an outline, close it, then pull its height (docs/
//! ARCHITECTURE.md §6 nonies), after OpenCutList's `SmartDrawPolygonActionHandler`. The part
//! made is a prism, its profile kept in its blank's recipe.
//!
//! - The first point is the box's (the plane aimed at, the arrows locking it, the copy modifier
//!   held for a face's centroid and Shift tapped then for its whole outline, the other modifier
//!   to measure from a corner). A plane locked there stays for the whole outline.
//! - Then each click adds a corner, inferred with the lines through the last one; the arrows
//!   lock an axis through it. The plane follows OCL's: through the first corner as the box's
//!   second corner sets it, then through the first two and the cursor's point, set from three
//!   corners on. From three corners, the cursor snaps to a placed corner near it, and a click
//!   there closes the outline (as a double-click, or Enter); from two, it snaps to the foot of
//!   a placed corner on the line it draws (OCL's alignment).
//! - The copy modifier tapped reverses the way corners are added (before the first instead of
//!   after the last); Shift tapped adds a guide from the last corner towards the cursor.
//! - Esc takes away the last corner placed; from the height, back to the corners.
//! - The height is the box's.
//! - The "Offset" option (the box's and the cylinder's, `3x` at any step) moves the outline
//!   made, its corners sharp ([`profile::offset`], as SolidCraft's sketch offset, a side it turns
//!   over taken out as Clipper does): the outline drawn is dashed then, the one made solid.

use super::*;
use crate::tools::{PointShape, PreviewPoint};
use crate::vcb::PointRef;
use solidsketch_model::kernel::profile;
use solidsketch_model::{GuideShape, Operation};

/// The option: new corners go before the first one (OCL's `measure_reversed`, saved).
const REVERSED: &str = "reversed";
/// How near a placed corner (pixels) the cursor snaps to it.
const CORNER_PX: f64 = 10.0;
/// How near the foot of a placed corner (pixels) the cursor snaps to it (OCL's 30 px aperture,
/// across).
const ALIGN_PX: f64 = 15.0;

#[derive(Default)]
pub struct PolygonTool {
    state: State,
    first: First,
    /// The cursor's point while placing corners.
    cursor: Option<Cursor>,
    snap: Option<Inference>,
    /// The saved options, as the editor last gave them.
    pull_centered: bool,
    reversed: bool,
    /// Where a click just closed the outline: the double-click that may follow there is that
    /// click's, not a call for the last height.
    closed_at: Option<DVec2>,
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
        profile: Vec<DVec2>,
        end: DVec3,
        h: f64,
    },
}

/// The corners placed (axes coordinates) and the plane they set.
#[derive(Clone, Debug)]
struct Corners {
    /// Along the outline.
    along: Vec<DVec3>,
    /// In the order they were placed (Esc takes the last away).
    placed: Vec<DVec3>,
    /// The plane's normal so far (towards the eye); set from three corners on, or when locked.
    n: DVec3,
    /// The normal locked at the first point (the arrows, ↓), for the whole outline.
    locked: Option<DVec3>,
    /// The direction the first click gave (an edge aimed at): the sides of the second corner's
    /// plane.
    dir: Option<DVec3>,
}

/// The cursor's point while placing corners: where it is (axes), the plane's normal with it,
/// the placed corner it snaps to (an index along the outline), or the corner whose foot on the
/// line drawn it snaps to.
#[derive(Clone, Copy, Debug)]
struct Cursor {
    p: DVec3,
    /// The anchor and the point, in the world (the measurements box's typed points).
    world: (DVec3, DVec3),
    n: DVec3,
    on_corner: Option<usize>,
    aligned: Option<DVec3>,
}

impl Corners {
    fn new(p: DVec3, n: DVec3, locked: Option<DVec3>, dir: Option<DVec3>) -> Corners {
        Corners { along: vec![p], placed: vec![p], n: locked.unwrap_or(n), locked, dir }
    }

    /// Is the plane set (from three corners, or locked)?
    fn plane_set(&self) -> bool {
        self.locked.is_some() || self.along.len() >= 3
    }

    /// The corner new lines start from: the last one, or the first, reversed.
    fn anchor(&self, reversed: bool) -> DVec3 {
        if reversed { self.along[0] } else { self.along[self.along.len() - 1] }
    }

    /// The other end of the outline: where it closes back to.
    fn other_end(&self, reversed: bool) -> DVec3 {
        if reversed { self.along[self.along.len() - 1] } else { self.along[0] }
    }

    fn add(&mut self, p: DVec3, n: DVec3, reversed: bool) {
        if reversed {
            self.along.insert(0, p);
        } else {
            self.along.push(p);
        }
        self.placed.push(p);
        if self.locked.is_none() {
            self.n = n;
        }
    }

    /// Takes the last corner placed away; `false` when none is left.
    fn remove_last(&mut self) -> bool {
        if let Some(p) = self.placed.pop()
            && let Some(i) = self.along.iter().position(|q| *q == p)
        {
            self.along.remove(i);
        }
        !self.along.is_empty()
    }
}

/// The ordered outer outline of the planar face `face` (world), when its sides are all
/// straight: its sides chained into loops, the one of largest area.
fn outer_loop(face: &Face) -> Option<Vec<DVec3>> {
    const SAME: f64 = 1e-3;
    let mut sides = face.sides.clone();
    let mut best: Option<(f64, Vec<DVec3>)> = None;
    while let Some((a, b)) = sides.pop() {
        let mut lp = vec![a];
        let mut end = b;
        while end.distance(a) > SAME {
            lp.push(end);
            let i = sides.iter().position(|(p, q)| p.distance(end) < SAME || q.distance(end) < SAME)?;
            let (p, q) = sides.swap_remove(i);
            end = if p.distance(end) < SAME { q } else { p };
        }
        let area = (0..lp.len()).map(|i| lp[i].cross(lp[(i + 1) % lp.len()])).sum::<DVec3>().dot(face.normal).abs();
        if lp.len() >= 3 && best.as_ref().is_none_or(|(a, _)| area > *a) {
            best = Some((area, lp));
        }
    }
    // Every corner of the face on a straight side: none on an arc.
    let (_, lp) = best?;
    face.corners.iter().all(|c| face.sides.iter().any(|(a, b)| a.distance(*c) < SAME || b.distance(*c) < SAME)).then_some(lp)
}

/// Why the offset refuses the outline, for the status bar.
fn offset_refused(e: profile::OffsetError) -> String {
    match e {
        profile::OffsetError::Empty => t!("tool.shape.say.offset_empty"),
        profile::OffsetError::Crossed => t!("tool.polygon.say.offset_crossed"),
    }
}

/// The middle of a profile's box (in its plane).
fn middle(profile: &[DVec2]) -> DVec2 {
    let (lo, hi) = profile.iter().fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(l, h), p| (l.min(*p), h.max(*p)));
    (lo + hi) / 2.0
}

impl PolygonTool {
    /// The cursor's point under `px` while placing corners (§ module doc), and what it snapped to.
    fn point_at(&self, cx: &Cx<'_>, px: DVec2, c: &Corners) -> Option<(Cursor, Option<Inference>)> {
        let a = c.anchor(self.reversed);
        let k = c.along.len();
        let ray = cx.axes_ray(px).dir;
        // The arrow keys' axis, unless it leaves a plane already set.
        let lock = cx.lock().filter(|l| !c.plane_set() || dir_to_axes(cx, l.dir).dot(c.n).abs() < SQUARE);
        // From three corners on: a placed corner near the cursor (on the locked axis, its foot).
        if k >= 3 {
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
                return Some((Cursor { p, world: (cx.to_world(a), cx.to_world(p)), n: c.n, on_corner: Some(i), aligned: None }, None));
            }
        }
        let plane = (cx.to_world(a), dir_to_world(cx, c.n));
        let inf = cx.infer_from(px, |_| true, Some(plane), cx.to_world(a), lock)?;
        let q = cx.to_axes(inf.point);
        let (mut p, n, snap) = match (k, c.locked) {
            // A plane locked: the point brought onto it.
            (_, Some(n)) => (Frame::new(a, n, None).flatten(q), n, Some(inf)),
            // One corner: the box's second corner's plane through it.
            (1, None) => {
                let f = Frame::new(c.along[0], c.n, c.dir);
                let (g, p, snap) = second_plane(cx, px, &f, c.dir, inf);
                (p, g.n, snap)
            }
            // Two: the plane through both and the cursor's point.
            (2, None) => {
                let n = (c.along[1] - c.along[0]).cross(q - c.along[0]).try_normalize().map_or(c.n, |n| facing(n, ray));
                (q, n, Some(inf))
            }
            // Set: brought onto it.
            _ => (Frame::new(a, c.n, None).flatten(q), c.n, Some(inf)),
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
        Some((Cursor { p, world: (cx.to_world(a), cx.to_world(p)), n, on_corner: None, aligned }, snap))
    }

    /// A click (or a typed point) at `p`, with the plane `n`: a corner added, the locks dropped.
    fn add(&mut self, mut c: Corners, p: DVec3, n: DVec3) -> Vec<Command> {
        if p.distance(c.anchor(self.reversed)) < MIN {
            self.state = State::Corners(c);
            return vec![];
        }
        c.add(p, n, self.reversed);
        self.state = State::Corners(c);
        self.cursor = None;
        self.snap = None;
        vec![Command::ClearLocks]
    }

    /// The outline closed on the corner `end`: on to the height, or a word if it cannot be.
    fn close(&mut self, c: Corners, end: DVec3) -> Vec<Command> {
        if c.along.len() < 3 {
            self.state = State::Corners(c);
            return vec![Command::Say(t!("tool.polygon.say.too_few"))];
        }
        let f = Frame::new(c.along[0], c.n, None);
        let outline: Vec<DVec2> = c.along.iter().map(|p| f.local(*p).truncate()).collect();
        let outline = profile::cleaned(&outline);
        if !profile::is_simple(&outline) {
            self.state = State::Corners(c);
            return vec![Command::Say(t!("tool.polygon.say.crossed"))];
        }
        self.state = State::Height { corners: c, f, profile: outline, end, h: 0.0 };
        self.cursor = None;
        self.snap = None;
        vec![Command::ClearLocks]
    }

    /// The prism on `profile` (in `f`'s plane), pulled to `h`.
    fn finish(&mut self, cx: &Cx<'_>, f: Frame, outline: Vec<DVec2>, h: f64) -> Vec<Command> {
        let (z0, z1) = span(h, self.pull_centered);
        if z1 - z0 < MIN {
            return vec![Command::Say(t!("tool.polygon.say.empty"))];
        }
        // The outline moved by the offset (§ module doc); the step stays when it cannot be.
        let outline = match profile::offset(&outline, cx.length(OFFSET)) {
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
            return vec![Command::Say(t!("tool.polygon.say.empty"))];
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
}

impl Tool for PolygonTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Polygon
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
                        let along: Vec<DVec3> = lp.iter().map(|p| cx.to_axes(*p)).collect();
                        let c = Corners { placed: along.clone(), along, n, locked: Some(n), dir: None };
                        let end = c.along[0];
                        self.first.from_corner = false;
                        return self.close(c, end);
                    }
                    Some((_, None)) => return vec![Command::Say(t!("tool.polygon.say.curved_face"))],
                    None => {}
                }
            }
            (ToolInput::Key { key, .. }, State::First) => return first_key(*key, &self.first).into_iter().collect(),
            (ToolInput::Hover { px }, State::First) => self.first.hover(cx, *px),
            (ToolInput::Click { px, .. }, State::First) => {
                if let Some(s) = start_at(cx, *px) {
                    self.state = State::Corners(Corners::new(s.f.o, s.f.n, locked_normal(cx), s.dir));
                    self.first.from_corner = false;
                    return vec![Command::ClearLocks];
                }
            }
            (ToolInput::Value(VcbValue::Lengths(v)), State::First) => {
                if let Some((f, dir)) = self.first.typed(v) {
                    self.state = State::Corners(Corners::new(f.o, f.n, locked_normal(cx), dir));
                    self.first.from_corner = false;
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
            (ToolInput::Key { key: Key::Enter, .. }, State::Corners(c)) => {
                let end = c.anchor(self.reversed);
                return self.close(c, end);
            }
            (ToolInput::Hover { px }, State::Corners(c)) => {
                if let Some((cur, snap)) = self.point_at(cx, *px, &c) {
                    self.cursor = Some(cur);
                    self.snap = snap;
                }
                self.state = State::Corners(c);
            }
            (ToolInput::Click { px, .. }, State::Corners(c)) => {
                let Some((cur, _)) = self.point_at(cx, *px, &c) else {
                    self.state = State::Corners(c);
                    return vec![];
                };
                if let Some(i) = cur.on_corner
                    && cur.p == c.along[i]
                {
                    let end = c.along[i];
                    self.closed_at = Some(*px);
                    return self.close(c, end);
                }
                return self.add(c, cur.p, cur.n);
            }
            // A double-click: its second click has closed the outline already (or the corner is
            // added and that is all, under three).
            (ToolInput::DoubleClick { px, .. }, State::Corners(c)) if c.along.len() >= 3 => {
                let end = c.anchor(self.reversed);
                self.closed_at = Some(*px);
                return self.close(c, end);
            }
            (ToolInput::Value(VcbValue::Lengths(v)), State::Corners(c)) => {
                let a = c.anchor(self.reversed);
                let Some(cur) = self.cursor else {
                    self.state = State::Corners(c);
                    return vec![];
                };
                let Some(d) = (cur.p - a).try_normalize() else {
                    self.state = State::Corners(c);
                    return vec![];
                };
                return self.add(c, a + d * v[0], cur.n);
            }
            (ToolInput::Value(VcbValue::Point(w)), State::Corners(c)) => {
                let q = cx.to_axes(*w);
                let n = self.cursor.map_or(c.n, |cur| cur.n);
                let p = if c.plane_set() { Frame::new(c.along[0], c.n, None).flatten(q) } else { q };
                return self.add(c, p, n);
            }

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
        let corner = |p: DVec3, shape| PreviewPoint { at: cx.to_world(p), radius: 4.0, fill: Some(INK), stroke: None, shape };
        match &self.state {
            State::First => {
                inference = self.first.inference();
                if let Some(s) = &self.first.at {
                    // A small pentagon at the cursor, in the plane it would take.
                    let f = s.f;
                    let r = pixels(cx, f.o, GAUGE_PX) / 2.0;
                    let at = |i: usize| {
                        let a = std::f64::consts::FRAC_PI_2 + i as f64 / 5.0 * std::f64::consts::TAU;
                        f.at(DVec3::new(r * a.cos(), r * (a.sin() - 1.0), 0.0))
                    };
                    let color = plane_color(cx, &f);
                    lines.extend((0..5).map(|i| line(cx, at(i), at(i + 1), color)));
                    if snapped(&inference) {
                        mark(cx, &mut marks, f.o);
                    }
                }
                self.first.preview(cx, &mut lines, &mut labels, &mut marks);
            }
            State::Corners(c) => {
                let f = Frame::new(c.along[0], self.cursor.map_or(c.n, |cur| cur.n), None);
                let color = if c.plane_set() || c.along.len() >= 2 { plane_color(cx, &f) } else { INK };
                // With an offset, the outline drawn is dashed and the one made solid (the box's).
                let off = cx.length(OFFSET);
                segments(cx, &mut lines, c.along.windows(2).map(|w| (w[0], w[1])), color, off != 0.0);
                if off != 0.0 {
                    // The outline as it would close with the cursor's point, offset.
                    let mut closed = c.along.clone();
                    if let Some(cur) = self.cursor.filter(|cur| cur.on_corner.is_none()) {
                        if self.reversed {
                            closed.insert(0, cur.p);
                        } else {
                            closed.push(cur.p);
                        }
                    }
                    let local: Vec<DVec2> = closed.iter().map(|p| f.local(*p).truncate()).collect();
                    if local.len() >= 3
                        && let Ok(o) = profile::offset(&local, off)
                    {
                        let at = |p: DVec2| f.at(p.extend(0.0));
                        segments(cx, &mut lines, (0..o.len()).map(|i| (at(o[i]), at(o[(i + 1) % o.len()]))), color, false);
                    }
                }
                points.extend(c.along.iter().map(|p| corner(*p, PointShape::Round)));
                if let Some(cur) = self.cursor {
                    let a = c.anchor(self.reversed);
                    let along = axis_color(cur.p - a);
                    segments(cx, &mut lines, [(a, cur.p)], along.unwrap_or(color), off != 0.0);
                    label(cx, &mut labels, a, cur.p, along.unwrap_or(INK));
                    // Where the outline would close: from the cursor, or, on a placed corner (a
                    // click there closes it; right after a click, the corner just placed), the
                    // closing side itself.
                    let from = if cur.on_corner.is_some() { a } else { cur.p };
                    let to = c.other_end(self.reversed);
                    if c.along.len() >= 2 && from.distance(to) >= MIN {
                        dashed(cx.camera, cx.size, &mut lines, cx.to_world(from), cx.to_world(to), color);
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
                }
            }
            State::Height { f, profile, end, h, .. } => {
                let color = plane_color(cx, f);
                let (z0, z1) = span(*h, self.pull_centered);
                let at = |p: DVec2, z: f64| f.at(p.extend(z));
                let prism = |lines: &mut Vec<PreviewLine>, profile: &[DVec2], dash: bool| {
                    let n = profile.len();
                    let sides = (0..n).flat_map(|i| {
                        let (p, q) = (profile[i], profile[(i + 1) % n]);
                        [(at(p, z0), at(q, z0)), (at(p, z1), at(q, z1)), (at(p, z0), at(p, z1))]
                    });
                    segments(cx, lines, sides, color, dash);
                };
                // With an offset, the outline drawn dashed and the prism made solid.
                let off = cx.length(OFFSET);
                prism(&mut lines, profile, off != 0.0);
                if off != 0.0
                    && let Ok(o) = profile::offset(profile, off)
                {
                    prism(&mut lines, &o, false);
                }
                let e = f.local(*end).truncate();
                label(cx, &mut labels, at(e, z0), at(e, z1), BLUE);
            }
        }
        Preview { lines, labels, marks, points, inference, ..Default::default() }
    }

    fn status(&self) -> String {
        match &self.state {
            State::First => t!("tool.polygon.hint.first", copy_key = copy_key(), alt_key = alt_key()),
            State::Corners(_) => t!("tool.polygon.hint.corners", copy_key = copy_key()),
            State::Height { .. } => t!("tool.shape.hint.height", copy_key = copy_key()),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        match &self.state {
            State::First => self.first.vcb(),
            State::Corners(c) => {
                let a = c.anchor(self.reversed);
                let len = self.cursor.map_or(0.0, |cur| (cur.p - a).length());
                Some(Vcb {
                    point: self.cursor.map(|cur| PointRef { from: cur.world.0, to: cur.world.1 }),
                    ..Vcb::new(t!("tool.polygon.vcb.length"), VcbKind::Lengths(vec![len]))
                })
            }
            State::Height { h, .. } => Some(Vcb::new(t!("tool.polygon.vcb.height"), VcbKind::Lengths(vec![whole(*h, self.pull_centered)]))),
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.state, State::First)
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let reversed = toggle(
            REVERSED,
            t!("tool.polygon.option.reversed"),
            t!("tool.polygon.option.reversed_tip", copy_key = copy_key()),
            cx.toggle(REVERSED),
            true,
            "measure_reversed",
        );
        [offset_option(cx), reversed].into_iter().chain(shared_options(cx, &self.first)).collect()
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        match (key, value) {
            (REVERSED, OptionValue::Toggle(on)) => self.reversed = *on,
            (PULL_CENTERED, OptionValue::Toggle(on)) => self.pull_centered = *on,
            (FROM_CORNER, OptionValue::Toggle(on)) => self.first.from_corner = *on,
            _ => {}
        }
    }

    fn down_locks_face(&self) -> bool {
        matches!(self.state, State::First)
    }

    fn leave(&mut self) {
        self.first.leave();
        self.cursor = None;
        self.snap = None;
    }

    fn back(&mut self) {
        self.state = match std::mem::take(&mut self.state) {
            State::Height { corners, .. } => State::Corners(corners),
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
    }
}

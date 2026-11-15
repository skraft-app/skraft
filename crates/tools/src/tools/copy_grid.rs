//! Copy along a grid (ARCHITECTURE.md §6 duodecies), OpenCutList's
//! `SmartHandleCopyGridActionHandler` as faithfully as the app allows.
//!
//! - **Pick** as the line ([`Picker`]).
//! - **Handle** (one step): from the centre of the box, on the plane through it normal to the
//!   blue axis of the axes chosen, or to the axis the arrow keys lock; the cursor taken on that
//!   plane (its ray, off the geometry; the point inferred, projected, on it). The grid's red
//!   side is the axes' red one, or their green one when the plane is normal to red (OCL's
//!   `_get_handle_axes`). Each side stops at the least length the measure allows.
//! - **Typed**: two distances `x; y` (what the measure says), two counts `3x; /4` (an empty one
//!   is one copy); `/1; /1` by default. The copy modifier tapped toggles the mirror (one copy
//!   per side only): the side copies mirrored, the corner one turned half a turn.
//! - **Then**: the copies made, two distances or two counts typed again redo them, in their
//!   place in the history; a click picks something else.

use super::DragClicks;
use glam::{DAffine3, DMat3, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::Operation;

use super::handle::{
    MIRROR, Measure, Memo, Picker, Target, axis_color, draw_axes, draw_box, draw_measure, measure, minimums, mirror_badge, reflection,
};
use super::rotate::rotation_about;
use super::{
    Command, Cx, INK, Key, OptionValue, PointShape, Preview, PreviewLabel, PreviewPoint, Tool, ToolInput, ToolKind, ToolOption, copy_key,
};
use crate::infer::{InferKind, Inference};
use crate::vcb::{self, Repeat, Vcb, VcbKind, VcbValue};

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// The grid's axes in the target's (columns: its red side, its green side, the normal), for the
/// plane normal to the target's axis `k` (OCL's `_get_handle_axes`: the red axis, or the green
/// one when it is the normal, then normal × red).
fn handle_axes(k: usize) -> DMat3 {
    let z = DVec3::AXES[k];
    let x = if k == 0 { DVec3::Y } else { DVec3::X };
    DMat3::from_cols(x, z.cross(x), z)
}

/// The target's axis a grid axis runs along (for its colour).
fn axis_of(v: DVec3) -> usize {
    (0..3).fold(0, |b, k| if v[k].abs() > v[b].abs() { k } else { b })
}

/// The grid, in its axes (OCL's `_get_move_def`, in « handle » space): the box's centre `mps`,
/// the far corner copy's centre `mpe`, the measure rectangle `dps`–`dpe` and the least measure
/// along each side.
#[derive(Clone, Copy, Debug)]
struct GridDef {
    mps: DVec3,
    mpe: DVec3,
    dps: DVec3,
    dpe: DVec3,
    dmin: DVec2,
    /// From the centre of a copy to its outer corner, in the plane (OCL's `ve`).
    ve: DVec3,
}

/// The grid's axes `h` to the target's, and the target's box in them.
fn handle_box(t: &Target, h: &DMat3) -> (DVec3, DVec3) {
    let inv = h.transpose();
    let (mut lo, mut hi) = (DVec3::INFINITY, DVec3::NEG_INFINITY);
    for i in 0..8 {
        let c = DVec3::new([t.lo.x, t.hi.x][i & 1], [t.lo.y, t.hi.y][(i >> 1) & 1], [t.lo.z, t.hi.z][(i >> 2) & 1]);
        let p = inv * c;
        (lo, hi) = (lo.min(p), hi.max(p));
    }
    (lo, hi)
}

fn factor(r: Repeat) -> f64 {
    match r {
        Repeat::Divide(n) => n as f64,
        Repeat::Times(_) => 1.0,
    }
}

fn count(r: Repeat) -> u32 {
    match r {
        Repeat::Times(n) | Repeat::Divide(n) => n,
    }
}

/// The grid to the point `end` (the grid's axes), `r` copies along each side, measured as
/// `measure` says, each side held at the least length keeping the copies apart.
fn grid_def(t: &Target, h: &DMat3, end: DVec3, r: [Repeat; 2], measure: Measure) -> GridDef {
    let (lo, hi) = handle_box(t, h);
    let c = (lo + hi) * 0.5;
    let half = (hi - lo) * 0.5;
    let mut ev = DVec2::new(end.x - c.x, end.y - c.y);
    let mut vs = DVec3::ZERO;
    let mut dmin = DVec2::ZERO;
    for i in 0..2 {
        if ev[i].abs() < 1e-9 {
            ev[i] = 1.0;
        }
        let s = ev[i].signum();
        let (dl, dm) = minimums(half[i], factor(r[i]), measure);
        if ev[i].abs() < dl {
            ev[i] = s * dl;
        }
        vs[i] = -s * half[i];
        dmin[i] = dm;
    }
    let lpe = c + DVec3::new(ev.x, ev.y, 0.0);
    let (mpe, dps) = match measure {
        Measure::Outside => (lpe + vs, c + vs),
        Measure::Centered => (lpe, c),
        Measure::Inside => (lpe - vs, c - vs),
    };
    GridDef { mps: c, mpe, dps, dpe: lpe, dmin, ve: -vs }
}

/// The world transform of the grid's cell `(x, y)` (OCL's `_copy_grid_entity`): moved by its
/// step; with the mirror (one copy per side), the side copies mirrored across the plane
/// through their centre normal to their side, the corner one turned half a turn about the
/// normal.
fn cell(t: &Target, h: &DMat3, d: &GridDef, r: [Repeat; 2], mirror: bool, x: u32, y: u32) -> DAffine3 {
    let mv = d.mpe - d.mps;
    let step = |i: usize| if matches!(r[i], Repeat::Divide(_)) { mv[i] / count(r[i]) as f64 } else { mv[i] };
    let mvu = DVec3::new(step(0) * x as f64, step(1) * y as f64, 0.0);
    let tr = t.translation(*h * mvu);
    let (nx, ny) = (count(r[0]), count(r[1]));
    if !mirror || nx != 1 || ny != 1 || (x == 0 && y == 0) {
        return tr;
    }
    let mp = t.world(*h * (d.mps + mvu));
    let world = |v: DVec3| t.frame.transform_vector3(*h * v).normalize();
    match (x == nx, y == ny) {
        (true, true) => rotation_about(mp, world(DVec3::Z), std::f64::consts::PI) * tr,
        (true, false) => reflection(mp, world(DVec3::X)) * tr,
        _ => reflection(mp, world(DVec3::Y)) * tr,
    }
}

/// Every copy's world transform, the source's cell left out.
fn placements(t: &Target, h: &DMat3, d: &GridDef, r: [Repeat; 2], mirror: bool) -> Vec<DAffine3> {
    let (nx, ny) = (count(r[0]), count(r[1]));
    (0..=nx).flat_map(|x| (0..=ny).map(move |y| (x, y))).filter(|c| *c != (0, 0)).map(|(x, y)| cell(t, h, d, r, mirror, x, y)).collect()
}

/// The copies just made: a value typed then redoes them.
#[derive(Clone, Debug)]
struct Last {
    target: Target,
    k: usize,
    end: DVec3,
    repeat: [Repeat; 2],
    size: DVec2,
    op: Operation,
}

#[derive(Clone)]
pub struct CopyGridTool {
    picker: Picker,
    memo: Memo,
    armed: bool,
    /// The cursor's point, in the grid's axes, and the target's axis the plane is normal to.
    end: Option<(DVec3, usize)>,
    snap: Option<Inference>,
    repeat: [Repeat; 2],
    last: Option<Last>,
    aimed: bool,
    /// The rectangle's sides, as the last input left them.
    size: DVec2,
}

impl Default for CopyGridTool {
    fn default() -> Self {
        CopyGridTool {
            picker: Picker::default(),
            memo: Memo::default(),
            armed: true,
            end: None,
            snap: None,
            repeat: [Repeat::Divide(1); 2],
            last: None,
            aimed: false,
            size: DVec2::ZERO,
        }
    }
}

fn measure_of(cx: &Cx<'_>) -> Measure {
    measure(cx, Measure::Outside)
}

impl CopyGridTool {
    fn target(&self, cx: &Cx<'_>) -> Option<Target> {
        if !self.armed || self.picker.gathering {
            return None;
        }
        self.memo.target(cx)
    }

    /// The plane's normal: the locked axis, else blue.
    fn normal(cx: &Cx<'_>) -> usize {
        cx.axis_lock.unwrap_or(2)
    }

    /// The cursor on the plane (OCL's `_snap_handle`): its ray's point on it off the geometry,
    /// else the point inferred projected on it.
    fn aim(&mut self, cx: &Cx<'_>, t: &Target, px: DVec2) {
        let k = Self::normal(cx);
        let inf = cx.infer(px, |_| true, None);
        let on = inf.filter(|i| i.kind != InferKind::OnPlane);
        let c = t.world(t.center());
        let n = t.dir(k);
        let p = match &on {
            Some(i) => Some(i.point - n * (i.point - c).dot(n)),
            None => cx.ray(px).hit_plane(c, n),
        };
        if let Some(p) = p {
            self.end = Some((handle_axes(k).transpose() * t.local(p), k));
        }
        self.snap = on;
    }

    fn operation(cx: &Cx<'_>, t: &Target, k: usize, end: DVec3, r: [Repeat; 2]) -> Operation {
        let h = handle_axes(k);
        let d = grid_def(t, &h, end, r, measure_of(cx));
        t.operation(DAffine3::IDENTITY, &placements(t, &h, &d, r, cx.toggle(MIRROR)))
    }

    fn size(cx: &Cx<'_>, t: &Target, k: usize, end: DVec3, r: [Repeat; 2]) -> DVec2 {
        let d = grid_def(t, &handle_axes(k), end, r, measure_of(cx));
        DVec2::new((d.dpe.x - d.dps.x).abs(), (d.dpe.y - d.dps.y).abs())
    }

    /// The end point for typed sides (OCL's `_read_handle`), or why they are refused.
    fn typed_end(cx: &Cx<'_>, t: &Target, k: usize, end: DVec3, r: [Repeat; 2], sides: &[f64]) -> Result<DVec3, String> {
        let d = grid_def(t, &handle_axes(k), end, r, measure_of(cx));
        let dv = d.dpe - d.dps;
        let mut out = d.dps;
        for i in 0..2 {
            let side = sides.get(i).copied().unwrap_or(dv[i]).abs();
            if side < d.dmin[i] - 1e-9 {
                let f = |v: f64| vcb::format(v, cx.units());
                return Err(t!("tool.handle.say.min_distance", value = f(side), min = f(d.dmin[i])));
            }
            out[i] += side.max(d.dmin[i]) * if dv[i] < 0.0 { -1.0 } else { 1.0 };
        }
        Ok(out)
    }

    /// Typed counts: a missing one is one copy (OCL's `['*', 1]`).
    fn counts(c: &[Option<Repeat>]) -> [Repeat; 2] {
        [0, 1].map(|i| c.get(i).copied().flatten().unwrap_or(Repeat::Times(1)))
    }

    fn mirror_warning(cx: &Cx<'_>, r: [Repeat; 2]) -> Option<Command> {
        (cx.toggle(MIRROR) && (count(r[0]) != 1 || count(r[1]) != 1)).then(|| Command::Say(t!("tool.handle.say.mirror_one")))
    }

    fn finish(&mut self, cx: &Cx<'_>, target: Target, k: usize, end: DVec3) -> Vec<Command> {
        let op = Self::operation(cx, &target, k, end, self.repeat);
        let size = Self::size(cx, &target, k, end, self.repeat);
        self.last = Some(Last { target, k, end, repeat: self.repeat, size, op: op.clone() });
        self.armed = false;
        self.end = None;
        self.snap = None;
        vec![Command::Apply(op), Command::Select(Vec::new())]
    }

    fn retype(&mut self, cx: &Cx<'_>, v: &VcbValue) -> Vec<Command> {
        let Some(last) = &mut self.last else { return vec![] };
        let (end, repeat) = match v {
            VcbValue::Counts(c) => (last.end, Self::counts(c)),
            VcbValue::Lengths(l) => match Self::typed_end(cx, &last.target, last.k, last.end, last.repeat, l) {
                Ok(end) => (end, last.repeat),
                Err(say) => return vec![Command::Say(say)],
            },
            _ => return vec![],
        };
        let op = Self::operation(cx, &last.target, last.k, end, repeat);
        let previous = std::mem::replace(&mut last.op, op.clone());
        (last.end, last.repeat) = (end, repeat);
        last.size = Self::size(cx, &last.target, last.k, end, repeat);
        let mut out = vec![Command::Replace { previous: Box::new(previous), op: Box::new(op) }, Command::Select(Vec::new())];
        out.extend(Self::mirror_warning(cx, repeat));
        out
    }
}

impl Tool for CopyGridTool {
    fn kind(&self) -> ToolKind {
        ToolKind::CopyGrid
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        if self.picker.gathering && !cx.shift_down {
            self.picker.gathering = false;
            self.armed = true;
            self.last = None;
        }
        if let ToolInput::Key { key: Key::Copy, .. } = input {
            let on = !cx.toggle(MIRROR);
            let mut out = vec![Command::SetOption { key: MIRROR, value: OptionValue::Toggle(on) }];
            if on && (count(self.repeat[0]) != 1 || count(self.repeat[1]) != 1) {
                out.push(Command::Say(t!("tool.handle.say.mirror_one")));
            }
            return out;
        }
        let target = self.target(cx);
        self.aimed = target.is_some();
        let Some(t) = target else {
            self.end = None;
            self.snap = None;
            if let ToolInput::Value(v) = input {
                return self.retype(cx, v);
            }
            let (out, took) = self.picker.input(input, cx);
            if took {
                self.armed = true;
                self.last = None;
            }
            return out;
        };
        let k = self.end.map_or(Self::normal(cx), |e| e.1);
        let end = self.end.map_or_else(|| handle_axes(k).transpose() * t.center(), |e| e.0);
        match input {
            ToolInput::Hover { px } => self.aim(cx, &t, *px),
            ToolInput::Click { .. } if self.end.is_some() => return self.finish(cx, t, k, end),
            ToolInput::Value(VcbValue::Counts(c)) => {
                self.repeat = Self::counts(c);
                let out = Self::mirror_warning(cx, self.repeat).into_iter().collect();
                if let Some((end, k)) = self.end {
                    self.size = Self::size(cx, &t, k, end, self.repeat);
                }
                return out;
            }
            ToolInput::Value(VcbValue::Lengths(l)) => {
                return match Self::typed_end(cx, &t, k, end, self.repeat, l) {
                    Ok(end) => self.finish(cx, t, k, end),
                    Err(say) => vec![Command::Say(say)],
                };
            }
            _ => {}
        }
        if let Some((end, k)) = self.end {
            self.size = Self::size(cx, &t, k, end, self.repeat);
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let mut p = Preview::default();
        let Some(t) = self.target(cx) else {
            p.hover = self.picker.hovered;
            p.marquee = self.picker.marquee.clone();
            return p;
        };
        draw_axes(cx, &mut p.scene_lines, &t, |_| false, None);
        let Some((end, k)) = self.end else {
            draw_box(cx, &mut p.scene_lines, t.frame, t.lo, t.hi, INK);
            return p;
        };
        let h = handle_axes(k);
        let r = self.repeat;
        let mirror = cx.toggle(MIRROR);
        let d = grid_def(&t, &h, end, r, measure_of(cx));
        let color = axis_color(k);
        let world = |q: DVec3| t.world(h * q);
        // The measure rectangle, dashed, seen through what lies between.
        let (a, b) = (d.dps, d.dpe);
        let corners = [a, DVec3::new(b.x, a.y, a.z), DVec3::new(b.x, b.y, a.z), DVec3::new(a.x, b.y, a.z)].map(world);
        for i in 0..4 {
            draw_measure(cx, &mut p.lines, corners[i], corners[(i + 1) % 4], color);
        }
        // The boxes at the grid's corners (the source's in ink), their centres and measure points.
        let border = [r[0], r[1]].map(|r| if matches!(r, Repeat::Divide(_)) { count(r) } else { 1 });
        let mut cells: Vec<(u32, u32)> = vec![(0, 0), (border[0], 0), (0, border[1]), (border[0], border[1])];
        cells.dedup();
        let dv = d.dpe - d.dps;
        for (x, y) in cells {
            let c = cell(&t, &h, &d, r, mirror, x, y);
            draw_box(cx, &mut p.scene_lines, c * t.frame, t.lo, t.hi, if (x, y) == (0, 0) { INK } else { color });
            p.marks.push(c.transform_point3(world(d.mps)));
            let dp = d.dps + DVec3::new(dv.x * x.min(1) as f64, dv.y * y.min(1) as f64, 0.0);
            p.points.push(PreviewPoint { at: world(dp), radius: 4.0, fill: Some(WHITE), stroke: Some(color), shape: PointShape::Round });
        }
        // The sides' lengths, along the rectangle's sides from its low corner.
        let lo = a.min(b);
        let hi = a.max(b);
        let size = hi - lo;
        for (i, (from, to)) in [(lo, DVec3::new(hi.x, lo.y, lo.z)), (lo, DVec3::new(lo.x, hi.y, lo.z))].into_iter().enumerate() {
            let text = vcb::format(size[i], cx.units());
            let color = axis_color(axis_of(h.col(i)));
            p.labels.push(PreviewLabel {
                a: world(from),
                b: world(to),
                text,
                color,
                slot: Some(i),
                hidden: size[i] <= 1e-9,
                ..Default::default()
            });
        }
        // The mirror's badge past the corner copy (OCL's), in black.
        if mirror && count(r[0]) == 1 && count(r[1]) == 1 {
            let ve = t.frame.transform_vector3(h * d.ve);
            if let Some(dir) = ve.try_normalize() {
                p.points.push(mirror_badge(cx, world(d.mpe), dir, ve.length(), BLACK));
            }
        }
        p.copies = placements(&t, &h, &d, r, mirror);
        p.inference = self.snap.clone();
        p
    }

    fn status(&self) -> String {
        if self.aimed {
            return t!("tool.copy_grid.hint.place", copy_key = copy_key());
        }
        if self.last.is_some() && self.picker.marquee.is_none() && !self.picker.gathering {
            return t!("tool.copy_grid.hint.again");
        }
        self.picker.status(t!("tool.copy_grid.hint.pick"))
    }

    fn vcb(&self) -> Option<Vcb> {
        let label = t!("tool.copy_grid.vcb.size");
        let vcb = |s: DVec2| Some(Vcb::new(label.clone(), VcbKind::Lengths(vec![s.x, s.y])).counts(2));
        if self.aimed {
            return vcb(if self.end.is_some() { self.size } else { DVec2::ZERO });
        }
        vcb(self.last.as_ref()?.size)
    }

    fn copying(&self) -> bool {
        true
    }

    /// Picking, a drag draws a selection rectangle; then, it places where it is released.
    fn drag_clicks(&self) -> DragClicks {
        if self.aimed { DragClicks::Release } else { DragClicks::No }
    }

    fn busy(&self) -> bool {
        self.aimed || self.picker.marquee.is_some() || self.picker.gathering || self.last.is_some()
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        super::handle::options(cx, &[Measure::Outside, Measure::Centered, Measure::Inside], true)
    }

    fn set_option(&mut self, key: &str, _: &OptionValue) {
        if key == super::handle::AXES {
            self.end = None;
            self.snap = None;
        }
    }

    fn leave(&mut self) {
        self.end = None;
        self.snap = None;
        self.picker.hovered = None;
    }

    fn back(&mut self) {
        if self.picker.marquee.take().is_some() {
            return;
        }
        self.armed = false;
        self.end = None;
        self.snap = None;
        self.last = None;
        // Nothing aimed at any more: no longer busy (a quick tool then hands back).
        self.aimed = false;
    }

    fn reset(&mut self) {
        *self = CopyGridTool::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Moved;
    use solidsketch_model::Id;

    fn target() -> Target {
        Target {
            entities: vec![Id(1)],
            moved: Moved::Occurrences { context: Id(0), frame: DAffine3::IDENTITY },
            frame: DAffine3::IDENTITY,
            lo: DVec3::ZERO,
            hi: DVec3::new(100.0, 40.0, 20.0),
        }
    }

    #[test]
    fn the_grid_s_axes_follow_its_normal() {
        assert_eq!(handle_axes(2), DMat3::IDENTITY);
        let h = handle_axes(0);
        assert_eq!((h.col(0), h.col(1), h.col(2)), (DVec3::Y, DVec3::Z, DVec3::X));
        let h = handle_axes(1);
        assert_eq!((h.col(0), h.col(1), h.col(2)), (DVec3::X, -DVec3::Z, DVec3::Y));
    }

    #[test]
    fn each_side_measures_and_stops_as_a_line() {
        let t = target();
        let h = handle_axes(2);
        let one = [Repeat::Divide(1); 2];
        // Edge to edge: 500 × 300 from the source's corner.
        let d = grid_def(&t, &h, DVec3::new(500.0, 300.0, 0.0), one, Measure::Outside);
        assert!((d.dpe - d.dps - DVec3::new(500.0, 300.0, 0.0)).length() < 1e-9, "{d:?}");
        assert!((d.mpe - DVec3::new(450.0, 280.0, 10.0)).length() < 1e-9, "{d:?}");
        assert_eq!(d.dmin, DVec2::new(200.0, 80.0));
        // Too near: held where the copies touch.
        let d = grid_def(&t, &h, DVec3::new(60.0, -10.0, 0.0), one, Measure::Outside);
        assert!((d.mpe - DVec3::new(150.0, -20.0, 10.0)).length() < 1e-9, "{d:?}");
        let copies = placements(&t, &h, &d, [Repeat::Times(2), Repeat::Divide(1)], false);
        assert_eq!(copies.len(), 5);
    }

    #[test]
    fn the_mirror_turns_the_corner_and_mirrors_the_sides() {
        let t = target();
        let h = handle_axes(2);
        let one = [Repeat::Divide(1); 2];
        let d = grid_def(&t, &h, DVec3::new(500.0, 300.0, 0.0), one, Measure::Outside);
        let side = cell(&t, &h, &d, one, true, 1, 0);
        assert!(side.matrix3.determinant() < 0.0);
        // The source's left face (x = 0) goes to the copy's right (x = 500).
        assert!((side.transform_point3(DVec3::new(0.0, 10.0, 5.0)).x - 500.0).abs() < 1e-9);
        let corner = cell(&t, &h, &d, one, true, 1, 1);
        assert!(corner.matrix3.determinant() > 0.0);
        assert!((corner.transform_point3(DVec3::ZERO) - DVec3::new(500.0, 300.0, 0.0)).length() < 1e-9);
    }
}

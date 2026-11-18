//! Distribute (docs/tools/copy-array.md), OpenCutList's `SmartHandleDistributeActionHandler`
//! as faithfully as the app allows.
//!
//! - **Pick** as the copy tools ([`Picker`]).
//! - **Two points** (OCL's two steps): a click sets the start point, a second the end point —
//!   the inner faces of two uprights, say; Esc at the second goes back to the first. The end
//!   runs along the axis through the start whose line passes nearest the cursor on screen (off
//!   the geometry), else along the axis the way to the cursor mostly goes; the arrow keys lock
//!   one. With the copy modifier held, it goes from the start along that axis up to the first
//!   thing in the way (OCL's raytest).
//! - **The pieces**: what is distributed moves to the first slot, its copies take the next
//!   ones, spread evenly between the two points — centre to centre, or with even gaps between
//!   them and the points (between the parts, OCL's default). While the tool works, what is
//!   distributed is drawn where it goes only.
//! - **Typed**: a count `5x` (5 pieces in all) or `/5` (5 intervals: 4 pieces), a distance (the
//!   end point along the line), a list of spacings `30; 50` (the first gaps fixed, the rest
//!   spread; `;; 30` fixes the last); then, the pieces placed, any of them again redoes it.
//! - **Double-click**: the last distribute again, on what is picked now.

use super::DragClicks;
use glam::{DAffine3, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::Operation;

use super::handle::{
    Measure, Memo, Target, axis_color, count_step, draw_axes, draw_boxes, draw_measure, half_along, measure, nearest_axis_on_screen,
    stepped,
};
use super::pick::Picker;
use super::{Command, Cx, PointShape, Preview, Tool, ToolInput, ToolKind, ToolOption, copy_key};
use crate::camera::Ray;
use crate::draw::{Ends, Pen};
use crate::infer::{InferKind, Inference};
use crate::scene::Owner;
use crate::vcb::{Repeat, Vcb, VcbKind, VcbValue};

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const GREY: [f32; 4] = [0.35, 0.35, 0.35, 1.0];

/// A distribute, in the target's axes: from `start` to `end` (on a line along an axis),
/// `number` pieces, the first `spacings` set (OCL's `@spacings`, `None` an empty slot).
#[derive(Clone, Debug, PartialEq)]
struct Params {
    start: DVec3,
    end: DVec3,
    number: u32,
    spacings: Vec<Option<f64>>,
}

/// Where the pieces go (OCL's `_get_move_def`), in the target's axes: the measure line `lps` →
/// `lpe` through the box's centre, and each piece's centre.
#[derive(Clone, Debug)]
struct Slots {
    lps: DVec3,
    lpe: DVec3,
    centers: Vec<DVec3>,
}

/// The slots for `p`, measured as `measure` says; none when the two points meet along the line.
fn slots(t: &Target, p: &Params, measure: Measure) -> Option<Slots> {
    let v = p.end - p.start;
    if v.length() < 1e-9 || p.number == 0 {
        return None;
    }
    let u = v.normalize();
    let c = t.center();
    let lps = c + u * (p.start - c).dot(u);
    let lpe = c + u * (p.end - c).dot(u);
    let h = if measure == Measure::Centered { 0.0 } else { half_along(t.half(), u) };
    let (mps, mpe) = (lps - u * h, lpe + u * h);
    let total = (mpe - mps).length();
    let n = p.number as usize;
    // The spacings set: the first ones, then (unless all are) the last ones, read backwards.
    let mut first: Vec<f64> = p.spacings.iter().map_while(|s| *s).collect();
    let mut last: Vec<f64> = if first.len() == p.spacings.len() { Vec::new() } else { p.spacings.iter().rev().map_while(|s| *s).collect() };
    if first.len() > n {
        first.truncate(n);
        last.clear();
    } else if first.len() + last.len() > n {
        last.truncate(n - first.len());
    }
    let middle = n - first.len() - last.len();
    let size = 2.0 * h;
    let mut from = 0.0;
    let mut at: Vec<f64> = first
        .iter()
        .map(|s| {
            from += s + size;
            from
        })
        .collect();
    let mut to = total;
    let ends: Vec<f64> = last
        .iter()
        .map(|s| {
            to -= s + size;
            to
        })
        .collect();
    at.extend((1..=middle).map(|i| from + (to - from) * i as f64 / (middle + 1) as f64));
    at.extend(ends);
    Some(Slots { lps, lpe, centers: at.into_iter().map(|l| mps + u * l).collect() })
}

/// The pieces just placed: a value typed then places them again.
#[derive(Clone, Debug)]
struct Last {
    target: Target,
    params: Params,
    /// The operation applied last for them (the one a new value replaces).
    op: Operation,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// The start point to click.
    Start,
    /// The end point to click, from `start` (the target's axes).
    End { start: DVec3 },
}

#[derive(Clone)]
pub struct DistributeTool {
    picker: Picker,
    memo: Memo,
    state: State,
    /// The cursor's point (the target's axes), on the line from the start in `End`, and the
    /// axis it runs along.
    cursor: Option<(DVec3, usize)>,
    /// Where the copy modifier's ray stopped (world), if it did.
    ray_hit: Option<DVec3>,
    snap: Option<Inference>,
    number: u32,
    spacings: Vec<Option<f64>>,
    last: Option<Last>,
    /// The last distribute, which a double-click repeats on what is picked now (OCL's previous
    /// handler's).
    previous: Option<Params>,
    aimed: bool,
}

impl Default for DistributeTool {
    fn default() -> Self {
        DistributeTool {
            picker: Picker::default(),
            memo: Memo::default(),
            state: State::Start,
            cursor: None,
            ray_hit: None,
            snap: None,
            number: 1,
            spacings: Vec::new(),
            last: None,
            previous: None,
            aimed: false,
        }
    }
}

fn measure_of(cx: &Cx<'_>) -> Measure {
    measure(cx, Measure::Inside)
}

impl DistributeTool {
    fn target(&self, cx: &Cx<'_>) -> Option<Target> {
        if !self.picker.aims() {
            return None;
        }
        self.memo.target(cx)
    }

    /// The point inferred under `px`, what is distributed left out (drawn where it goes only).
    fn infer(cx: &Cx<'_>, t: &Target, px: DVec2) -> Option<Inference> {
        cx.infer(px, |o: &Owner<'_>| !cx.is_moving(o, &t.entities) && !cx.is_moving_deep(o, cx.deep), None)
            .filter(|i| i.kind != InferKind::OnPlane)
    }

    /// The start point under `px` (OCL's `_snap_handle_start`: the point inferred, else on the
    /// ground).
    fn aim_start(&mut self, cx: &Cx<'_>, t: &Target, px: DVec2) {
        let inf = Self::infer(cx, t, px);
        let p = inf.as_ref().map(|i| i.point).or_else(|| {
            let (o, n) = cx.ground();
            cx.ray(px).hit_plane(o, n)
        });
        self.cursor = p.map(|p| (t.local(p), 0));
        self.snap = inf;
    }

    /// The end point under `px` (OCL's `_snap_handle`): on the axis through the start whose
    /// line passes nearest the cursor on screen (off the geometry; or the locked one), else the
    /// point inferred projected on the axis the way to it mostly goes; with the copy modifier
    /// held, where that way first meets something.
    fn aim_end(&mut self, cx: &Cx<'_>, t: &Target, start: DVec3, px: DVec2) {
        let inf = Self::infer(cx, t, px);
        let from = t.world(start);
        let (end, k) = match &inf {
            None => {
                let k = cx.axis_lock.unwrap_or_else(|| nearest_axis_on_screen(cx, t, from, px));
                (t.local(cx.ray(px).closest_on_line(from, t.dir(k))), k)
            }
            Some(i) => {
                let v = t.local(i.point) - start;
                let k = cx.axis_lock.unwrap_or_else(|| (0..3).fold(0, |b, k| if v[k].abs() > v[b].abs() + 1e-12 { k } else { b }));
                (start + DVec3::AXES[k] * v[k], k)
            }
        };
        self.ray_hit = None;
        let mut end = end;
        if cx.copy_down
            && let Some(dir) = t.frame.transform_vector3(end - start).try_normalize()
        {
            let ray = Ray { origin: from, dir, t_min: 1e-3 };
            if let Some(h) = cx.scene.pick(&ray, |p| !cx.in_selection(p)) {
                end = t.local(h.point);
                self.ray_hit = Some(h.point);
            }
        }
        self.cursor = Some((end, k));
        self.snap = inf.filter(|_| self.ray_hit.is_none());
    }

    fn params(&self, start: DVec3, end: DVec3) -> Params {
        Params { start, end, number: self.number, spacings: self.spacings.clone() }
    }

    /// The operation: what is distributed moved to the first slot, copies at the others.
    fn operation(cx: &Cx<'_>, t: &Target, p: &Params) -> Option<Operation> {
        let s = slots(t, p, measure_of(cx))?;
        let moves: Vec<DAffine3> = s.centers.iter().map(|m| t.translation(*m - t.center())).collect();
        Some(t.operation(moves[0], &moves[1..]))
    }

    fn finish(&mut self, cx: &Cx<'_>, target: Target, params: Params) -> Vec<Command> {
        let Some(op) = Self::operation(cx, &target, &params) else { return vec![] };
        self.previous = Some(params.clone());
        self.last = Some(Last { target, params, op: op.clone() });
        self.picker.drop();
        self.state = State::Start;
        self.cursor = None;
        self.snap = None;
        self.ray_hit = None;
        vec![Command::Apply(op), Command::Select(Vec::new())]
    }

    /// A count typed: the pieces, or the refusal.
    fn count(r: Repeat) -> Result<u32, String> {
        match r {
            Repeat::Times(n) => Ok(n),
            Repeat::Divide(n) if n >= 2 => Ok(n - 1),
            Repeat::Divide(n) => Err(t!("tool.distribute.say.intervals", value = n)),
        }
    }

    /// The end point a typed distance gives, along the line from `start` to `end`.
    fn typed_end(t: &Target, p: &Params, d: f64, cx: &Cx<'_>) -> Option<DVec3> {
        let s = slots(t, p, measure_of(cx))?;
        let u = (s.lpe - s.lps).try_normalize()?;
        Some(p.start + u * d.abs())
    }

    /// A value typed once the pieces are placed: they are placed again.
    fn retype(&mut self, cx: &Cx<'_>, v: &VcbValue) -> Vec<Command> {
        let Some(last) = &mut self.last else { return vec![] };
        let mut p = last.params.clone();
        match v {
            VcbValue::Counts(c) => match c.first().copied().flatten().map(Self::count) {
                Some(Ok(n)) => p.number = n,
                Some(Err(say)) => return vec![Command::Say(say)],
                None => return vec![],
            },
            VcbValue::Spacings(s) => p.spacings = s.clone(),
            VcbValue::Lengths(l) => match Self::typed_end(&last.target, &p, l[0], cx) {
                Some(end) => p.end = end,
                None => return vec![],
            },
            _ => return vec![],
        }
        let Some(op) = Self::operation(cx, &last.target, &p) else { return vec![] };
        let previous = std::mem::replace(&mut last.op, op.clone());
        last.params = p.clone();
        (self.number, self.spacings) = (p.number, p.spacings.clone());
        self.previous = Some(p);
        vec![Command::Replace { previous: Box::new(previous), op: Box::new(op) }, Command::Select(Vec::new())]
    }
}

impl Tool for DistributeTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Distribute
    }

    /// OCL's `STATE_SELECT`; then the arrow.
    fn cursor(&self, cx: &Cx<'_>) -> Option<crate::tools::Cursor> {
        Some(if self.picking() { self.picker.cursor(cx, crate::tools::Cursor::SelectDistribute) } else { crate::tools::Cursor::Select })
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        if self.picker.shift_released(cx) {
            self.last = None;
        }
        let target = self.target(cx);
        self.aimed = target.is_some();
        let Some(t) = target else {
            self.state = State::Start;
            self.cursor = None;
            self.snap = None;
            if let ToolInput::Value(v) = input {
                return self.retype(cx, v);
            }
            let (out, took) = self.picker.input(input, cx);
            if took {
                self.last = None;
            }
            return out;
        };
        // The last distribute again (OCL's double-click), on what is picked now.
        if let ToolInput::DoubleClick { .. } = input {
            let Some(p) = self.previous.clone() else { return vec![Command::Say(t!("tool.distribute.say.nothing_to_repeat"))] };
            (self.number, self.spacings) = (p.number, p.spacings.clone());
            return self.finish(cx, t, p);
        }
        // Shift with + / − at the second point: one piece more or fewer (OCL's).
        if let (State::End { .. }, Some(by)) = (self.state, count_step(input)) {
            self.number = stepped(self.number, by);
            return vec![];
        }
        match (self.state, input) {
            (State::Start, ToolInput::Hover { px }) => self.aim_start(cx, &t, *px),
            (State::Start, ToolInput::Click { .. }) => {
                if let Some((start, _)) = self.cursor {
                    self.state = State::End { start };
                    self.cursor = None;
                }
            }
            (State::End { start }, ToolInput::Hover { px }) => self.aim_end(cx, &t, start, *px),
            (State::End { start }, ToolInput::Click { .. }) => {
                if let Some((end, _)) = self.cursor {
                    let p = self.params(start, end);
                    return self.finish(cx, t, p);
                }
            }
            (State::End { .. }, ToolInput::Value(VcbValue::Counts(c))) => match c.first().copied().flatten().map(Self::count) {
                Some(Ok(n)) => self.number = n,
                Some(Err(say)) => return vec![Command::Say(say)],
                None => {}
            },
            (State::End { .. }, ToolInput::Value(VcbValue::Spacings(s))) => self.spacings = s.clone(),
            (State::End { start }, ToolInput::Value(VcbValue::Lengths(l))) => {
                let end = self.cursor.map_or(start + DVec3::X, |c| c.0);
                if let Some(end) = Self::typed_end(&t, &self.params(start, end), l[0], cx) {
                    let p = self.params(start, end);
                    return self.finish(cx, t, p);
                }
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let mut p = Preview::default();
        let Some(t) = self.target(cx) else {
            self.picker.preview(&mut p);
            return p;
        };
        // What is distributed is drawn where it goes only: in place until then.
        p.ghost = Some(DAffine3::IDENTITY);
        p.inference = self.snap.clone();
        let draw = &mut cx.drawer(&mut p.drawing);
        let State::End { start } = self.state else {
            draw_axes(draw, &t, |_| false, None);
            draw_boxes(draw, &t, &[]);
            if let Some(c) = self.cursor {
                draw.mark(t.world(c.0));
            }
            return p;
        };
        let from = t.world(start);
        draw.mark(from);
        let Some((end, k)) = self.cursor else { return p };
        let params = self.params(start, end);
        let Some(s) = slots(&t, &params, measure_of(cx)) else { return p };
        let color = axis_color(k);
        let moves: Vec<DAffine3> = s.centers.iter().map(|m| t.translation(*m - t.center())).collect();
        draw_axes(draw, &t, |j| j == k, None);
        draw_boxes(draw, &t, &moves);
        let (a, b) = (t.world(s.lps), t.world(s.lpe));
        // From the points clicked to the measure line, which runs through the box's centre.
        let cursor = self.ray_hit.unwrap_or_else(|| self.snap.as_ref().map_or(t.world(end), |i| i.point));
        draw.pen(Pen::new(GREY).over().dashed()).line(from, a).line(cursor, b);
        if let Some(hit) = self.ray_hit {
            draw.pen(Pen::new(color).over().bold()).edge(from, hit, Ends::End);
        }
        // The measure, dashed, seen through what lies between.
        draw_measure(cx, draw, a, b, color);
        for at in [a, b] {
            draw.grip(at, 4.0, Some(WHITE), Some(color), PointShape::Round);
        }
        draw.mark(t.world(t.center())).length_always(a, b, color, Some(0));
        p.ghost = moves.first().copied();
        p.copies = moves[1..].to_vec();
        p
    }

    fn status(&self) -> String {
        match (self.aimed, self.state) {
            (true, State::Start) => t!("tool.distribute.hint.start"),
            (true, State::End { .. }) => t!("tool.distribute.hint.end", copy_key = copy_key()),
            (false, _) if self.last.is_some() && !self.picker.busy() => {
                t!("tool.distribute.hint.again")
            }
            (false, _) => self.picker.status(t!("tool.distribute.hint.pick"), t!("tool.handle.hint.gather")),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        let label = t!("tool.distribute.vcb.distance");
        let vcb = |d: f64| Some(Vcb::new(label.clone(), VcbKind::Lengths(vec![d])).counts(1).spacings());
        if self.aimed {
            let State::End { start } = self.state else { return None };
            let d = self.cursor.map_or(0.0, |(end, _)| (end - start).length());
            return vcb(d);
        }
        let last = self.last.as_ref()?;
        vcb((last.params.end - last.params.start).length())
    }

    /// At the second point (OCL's `STATE_HANDLE`).
    fn takes_counts(&self) -> bool {
        self.aimed && matches!(self.state, State::End { .. })
    }

    /// Picking, a drag draws a selection rectangle; then, it goes from the start (the press)
    /// to the end (the release).
    fn drag_clicks(&self) -> DragClicks {
        match self.state {
            _ if !self.aimed => DragClicks::No,
            State::Start => DragClicks::PressAndRelease,
            State::End { .. } => DragClicks::Release,
        }
    }

    fn busy(&self) -> bool {
        self.aimed || self.picker.busy() || self.last.is_some()
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        super::handle::options(cx, &[Measure::Inside, Measure::Centered], false)
    }

    fn set_option(&mut self, key: &str, _: &super::OptionValue) {
        // Other axes: the points are taken again (OCL keeps the start; ours lies in the axes).
        if key == super::handle::AXES {
            self.state = State::Start;
            self.cursor = None;
        }
    }

    fn leave(&mut self) {
        self.cursor = None;
        self.snap = None;
        self.ray_hit = None;
        self.picker.leave();
    }

    fn picks_first(&self) -> bool {
        true
    }

    /// Nothing aimed at: a click picks what to copy (also once copies are made).
    fn picking(&self) -> bool {
        !self.aimed
    }

    fn picks_parts(&self) -> bool {
        true
    }

    fn dropped(&self) -> bool {
        self.picker.dropped()
    }

    fn back(&mut self) {
        if self.picker.back() {
            return;
        }
        if let State::End { .. } = self.state {
            self.state = State::Start;
            self.cursor = None;
            return;
        }
        self.picker.drop();
        self.cursor = None;
        self.snap = None;
        self.last = None;
        // Nothing aimed at any more: no longer busy (a quick tool then hands back).
        self.aimed = false;
    }

    fn reset(&mut self) {
        let previous = self.previous.take();
        *self = DistributeTool { previous, ..DistributeTool::default() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Moved;
    use skraft_model::Id;

    /// A shelf 18 thick along X, its centre at x = 9.
    fn target() -> Target {
        Target {
            entities: vec![Id(1)],
            moved: Moved::Occurrences { context: Id(0), frame: DAffine3::IDENTITY },
            frame: DAffine3::IDENTITY,
            lo: DVec3::ZERO,
            hi: DVec3::new(18.0, 300.0, 20.0),
            filled: false,
        }
    }

    fn xs(t: &Target, p: &Params, m: Measure) -> Vec<f64> {
        slots(t, p, m).unwrap().centers.iter().map(|c| c.x).collect()
    }

    fn near(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn pieces_are_spread_with_even_gaps_or_centres() {
        let t = target();
        // Between two uprights' faces at x = 100 and 700: 600 of room.
        let p = Params { start: DVec3::new(100.0, 0.0, 0.0), end: DVec3::new(700.0, 0.0, 0.0), number: 2, spacings: vec![] };
        // Between the parts: three gaps of (600 − 36) / 3 = 188.
        assert!(
            near(&xs(&t, &p, Measure::Inside), &[100.0 + 188.0 + 9.0, 100.0 + 2.0 * 188.0 + 27.0]),
            "{:?}",
            xs(&t, &p, Measure::Inside)
        );
        // Centre to centre: 200 apart.
        assert!(near(&xs(&t, &p, Measure::Centered), &[300.0, 500.0]));
        // One piece: in the middle.
        let one = Params { number: 1, ..p.clone() };
        assert!(near(&xs(&t, &one, Measure::Inside), &[400.0]));
    }

    #[test]
    fn spacings_fix_the_first_or_the_last_gaps() {
        let t = target();
        let p = Params { start: DVec3::new(100.0, 0.0, 0.0), end: DVec3::new(700.0, 0.0, 0.0), number: 3, spacings: vec![Some(30.0)] };
        // The first gap 30, the other two pieces spread over what is left.
        let x = xs(&t, &p, Measure::Inside);
        assert!((x[0] - 139.0).abs() < 1e-9, "{x:?}");
        let gap = (700.0 - 148.0 - 36.0) / 3.0;
        assert!(near(&x, &[139.0, 148.0 + gap + 9.0, 148.0 + 2.0 * gap + 27.0]), "{x:?}");
        // `;; 50`: the last gap 50.
        let p = Params { spacings: vec![None, None, Some(50.0)], ..p };
        let x = xs(&t, &p, Measure::Inside);
        assert!((x[2] - (700.0 - 50.0 - 9.0)).abs() < 1e-9, "{x:?}");
        // More spacings than pieces: the first ones only.
        let p = Params { number: 1, spacings: vec![Some(10.0), Some(20.0)], ..p };
        assert!(near(&xs(&t, &p, Measure::Inside), &[119.0]));
    }

    #[test]
    fn counts_and_their_refusal() {
        assert_eq!(DistributeTool::count(Repeat::Times(5)), Ok(5));
        assert_eq!(DistributeTool::count(Repeat::Divide(5)), Ok(4));
        assert!(DistributeTool::count(Repeat::Divide(1)).is_err());
    }
}

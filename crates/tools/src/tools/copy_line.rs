//! Copy along a line (ARCHITECTURE.md §6 duodecies), OpenCutList's
//! `SmartHandleCopyLineActionHandler` as faithfully as the app allows.
//!
//! - **Pick** (OCL's `SmartSelectActionHandler`): the selection when the tool starts, else a
//!   click, Shift clicks or a rectangle ([`Picker`]) — occurrences of the active context, or
//!   bodies of the open part, with the guides selected.
//! - **Handle** (OCL's one step: no first click): from the centre of the box of what is copied,
//!   in the axes the option chooses. Off the geometry, the cursor runs along the axis whose line
//!   passes nearest it on screen; on it, along the axis of the box face the way to it leaves by;
//!   the arrow keys lock an axis. The cursor stops at the least distance the measure allows (the
//!   copies never overlap).
//! - **Typed**: a distance (what the measure option says: edge to edge, centre to centre,
//!   between the copies), a count `5x` (5 copies at the step) or `/5` (5 copies over the way;
//!   `/1` by default). The copy modifier tapped toggles the mirror (one copy only).
//! - **Then**: the copies made, a distance or a count typed again redoes them, in their place in
//!   the history (OCL's `STATE_HANDLE_COPIES`); a click picks something else.

use super::DragClicks;
use glam::{DAffine3, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::Operation;

use super::handle::{
    MIRROR, Measure, Memo, Picker, Target, axis_color, draw_axes, draw_boxes, draw_measure, half_along, leaving_axis, measure, minimums,
    mirror_badge, nearest_axis_on_screen, reflection,
};
use super::{
    Command, Cx, Key, OptionValue, PointShape, Preview, PreviewLabel, PreviewPoint, Tool, ToolInput, ToolKind, ToolOption, copy_key,
};
use crate::infer::{InferKind, Inference};
use crate::vcb::{self, Repeat, Vcb, VcbKind, VcbValue};

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Where the copies go, worked out in the target's axes (OCL's `_get_move_def`): from the
/// centre of the box, `mps`, the last copy's centre `mpe`; the measure from `dps` to `dpe`,
/// along the unit `u`; the least measure `dmin`.
#[derive(Clone, Copy, Debug)]
struct LineDef {
    u: DVec3,
    mps: DVec3,
    mpe: DVec3,
    dps: DVec3,
    dpe: DVec3,
    dmin: f64,
}

/// The way to the point `end` (the target's axes) for `repeat` copies, measured as `measure`
/// says, held at the least length that keeps the copies apart.
fn line_def(t: &Target, end: DVec3, repeat: Repeat, measure: Measure) -> LineDef {
    let c = t.center();
    let mut ev = end - c;
    if ev.length() < 1e-9 {
        ev = DVec3::X;
    }
    let u = ev.normalize();
    let h = half_along(t.half(), u);
    let f = match repeat {
        Repeat::Divide(n) => n as f64,
        Repeat::Times(_) => 1.0,
    };
    let (dlmin, dmin) = minimums(h, f, measure);
    let lpe = c + u * ev.length().max(dlmin);
    let (mpe, dps) = match measure {
        Measure::Outside => (lpe - u * h, c - u * h),
        Measure::Centered => (lpe, c),
        Measure::Inside => (lpe + u * h, c + u * h),
    };
    LineDef { u, mps: c, mpe, dps, dpe: lpe, dmin }
}

/// The copies' world transforms: `repeat` steps along the way; the one copy mirrored across the
/// plane through its centre normal to the way when `mirror` (OCL's scaling by −1 then half turn
/// about the way: that reflection).
fn placements(t: &Target, d: &LineDef, repeat: Repeat, mirror: bool) -> Vec<DAffine3> {
    let mv = d.mpe - d.mps;
    let (n, step) = match repeat {
        Repeat::Times(n) => (n, mv),
        Repeat::Divide(n) => (n, mv / n as f64),
    };
    (1..=n)
        .map(|i| {
            let tr = t.translation(step * i as f64);
            if mirror && n == 1 { reflection(t.world(d.mpe), t.frame.transform_vector3(d.u).normalize()) * tr } else { tr }
        })
        .collect()
}

fn count(r: Repeat) -> u32 {
    match r {
        Repeat::Times(n) | Repeat::Divide(n) => n,
    }
}

/// The copies just made: a value typed then redoes them.
#[derive(Clone, Debug)]
struct Last {
    target: Target,
    end: DVec3,
    repeat: Repeat,
    /// The measure they were made at.
    distance: f64,
    /// The operation applied last for them (the one a new value replaces).
    op: Operation,
}

#[derive(Clone)]
pub struct CopyLineTool {
    picker: Picker,
    memo: Memo,
    /// The selection is what is copied (when the tool starts, and once picked); else a click
    /// picks.
    armed: bool,
    /// The cursor's point, in the target's axes, and the axis it runs along.
    end: Option<(DVec3, usize)>,
    snap: Option<Inference>,
    repeat: Repeat,
    last: Option<Last>,
    /// There was something to copy at the last input (for the hints).
    aimed: bool,
    /// The measure shown, as the last input left it.
    distance: f64,
}

impl Default for CopyLineTool {
    fn default() -> Self {
        CopyLineTool {
            picker: Picker::default(),
            memo: Memo::default(),
            armed: true,
            end: None,
            snap: None,
            repeat: Repeat::Divide(1),
            last: None,
            aimed: false,
            distance: 0.0,
        }
    }
}

impl CopyLineTool {
    fn target(&self, cx: &Cx<'_>) -> Option<Target> {
        if !self.armed || self.picker.gathering {
            return None;
        }
        self.memo.target(cx)
    }

    /// The cursor's point (OCL's `_snap_handle`): off the geometry, on the axis through the box's
    /// centre whose line passes nearest the cursor on screen (or the locked one), at its point
    /// nearest the cursor's ray; on the geometry, the point projected on the axis the way to it
    /// leaves the box by.
    fn aim(&mut self, cx: &Cx<'_>, t: &Target, px: glam::DVec2) {
        let inf = cx.infer(px, |_| true, None);
        let c = t.center();
        let start = t.world(c);
        let on = inf.filter(|i| i.kind != InferKind::OnPlane);
        let (end, k) = match &on {
            None => {
                let k = cx.axis_lock.unwrap_or_else(|| nearest_axis_on_screen(cx, t, start, px));
                (t.local(cx.ray(px).closest_on_line(start, t.dir(k))), k)
            }
            Some(i) => {
                let v = t.local(i.point) - c;
                let k = cx.axis_lock.unwrap_or_else(|| leaving_axis(t.half(), v));
                (c + DVec3::AXES[k] * v[k], k)
            }
        };
        // At the centre (the cursor's ray along the axis, seen end on): along the axis still.
        let end = if (end - c).length() < 1e-9 { c + DVec3::AXES[k] * 1e-6 } else { end };
        self.end = Some((end, k));
        self.snap = on;
    }

    fn operation(cx: &Cx<'_>, t: &Target, end: DVec3, repeat: Repeat) -> Operation {
        let d = line_def(t, end, repeat, measure(cx, Measure::Outside));
        t.operation(DAffine3::IDENTITY, &placements(t, &d, repeat, cx.toggle(MIRROR)))
    }

    /// The end point for a typed distance, or why it is refused.
    fn typed_end(cx: &Cx<'_>, t: &Target, end: DVec3, repeat: Repeat, distance: f64) -> Result<DVec3, String> {
        let d = line_def(t, end, repeat, measure(cx, Measure::Outside));
        let distance = distance.abs();
        if distance < d.dmin - 1e-9 {
            let f = |v: f64| vcb::format(v, cx.units());
            return Err(t!("tool.handle.say.min_distance", value = f(distance), min = f(d.dmin)));
        }
        Ok(d.dps + d.u * distance)
    }

    fn finish(&mut self, cx: &Cx<'_>, target: Target, end: DVec3) -> Vec<Command> {
        let op = Self::operation(cx, &target, end, self.repeat);
        let distance = Self::measured(cx, &target, end, self.repeat);
        self.last = Some(Last { target, end, repeat: self.repeat, distance, op: op.clone() });
        self.armed = false;
        self.end = None;
        self.snap = None;
        // OCL lets the copies go: the next click picks something else.
        vec![Command::Apply(op), Command::Select(Vec::new())]
    }

    /// A value typed once the copies are made: they are made again.
    fn retype(&mut self, cx: &Cx<'_>, v: &VcbValue) -> Vec<Command> {
        let Some(last) = &mut self.last else { return vec![] };
        let (end, repeat) = match v {
            VcbValue::Counts(c) => match c.first() {
                Some(Some(r)) => (last.end, *r),
                _ => return vec![],
            },
            VcbValue::Lengths(l) => match Self::typed_end(cx, &last.target, last.end, last.repeat, l[0]) {
                Ok(end) => (end, last.repeat),
                Err(say) => return vec![Command::Say(say)],
            },
            _ => return vec![],
        };
        let op = Self::operation(cx, &last.target, end, repeat);
        let previous = std::mem::replace(&mut last.op, op.clone());
        (last.end, last.repeat) = (end, repeat);
        last.distance = Self::measured(cx, &last.target, end, repeat);
        let mut out = vec![Command::Replace { previous: Box::new(previous), op: Box::new(op) }, Command::Select(Vec::new())];
        out.extend(Self::mirror_warning(cx, count(repeat)));
        out
    }

    /// The measure for the end point `end`.
    fn measured(cx: &Cx<'_>, t: &Target, end: DVec3, repeat: Repeat) -> f64 {
        let d = line_def(t, end, repeat, measure(cx, Measure::Outside));
        (d.dpe - d.dps).length()
    }

    /// OCL's warning: the mirror makes one copy only.
    fn mirror_warning(cx: &Cx<'_>, n: u32) -> Option<Command> {
        (cx.toggle(MIRROR) && n != 1).then(|| Command::Say(t!("tool.handle.say.mirror_one")))
    }
}

impl Tool for CopyLineTool {
    fn kind(&self) -> ToolKind {
        ToolKind::CopyLine
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        // Shift released: the selection made is taken.
        if self.picker.gathering && !cx.shift_down {
            self.picker.gathering = false;
            self.armed = true;
            self.last = None;
        }
        // The copy modifier tapped: the mirror (OCL's Alt / Command).
        if let ToolInput::Key { key: Key::Copy, .. } = input {
            let on = !cx.toggle(MIRROR);
            let mut out = vec![Command::SetOption { key: MIRROR, value: OptionValue::Toggle(on) }];
            if on && count(self.repeat) != 1 {
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
        let end = self.end.map_or(t.center(), |e| e.0);
        match input {
            ToolInput::Hover { px } => {
                self.aim(cx, &t, *px);
                if let Some((end, _)) = self.end {
                    self.distance = Self::measured(cx, &t, end, self.repeat);
                }
            }
            ToolInput::Click { .. } if self.end.is_some() => return self.finish(cx, t, end),
            ToolInput::Value(VcbValue::Counts(c)) => {
                if let Some(Some(r)) = c.first() {
                    self.repeat = *r;
                    if let Some((end, _)) = self.end {
                        self.distance = Self::measured(cx, &t, end, self.repeat);
                    }
                    return Self::mirror_warning(cx, count(*r)).into_iter().collect();
                }
            }
            ToolInput::Value(VcbValue::Lengths(v)) => {
                return match Self::typed_end(cx, &t, end, self.repeat, v[0]) {
                    Ok(end) => self.finish(cx, t, end),
                    Err(say) => vec![Command::Say(say)],
                };
            }
            _ => {}
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
        let Some((end, k)) = self.end else {
            draw_axes(cx, &mut p.scene_lines, &t, |_| false, None);
            draw_boxes(cx, &mut p.scene_lines, &t, &[], [0.0; 4]);
            return p;
        };
        let d = line_def(&t, end, self.repeat, measure(cx, Measure::Outside));
        let copies = placements(&t, &d, self.repeat, cx.toggle(MIRROR));
        let color = axis_color(k);
        draw_axes(cx, &mut p.scene_lines, &t, |j| j == k, None);
        draw_boxes(cx, &mut p.scene_lines, &t, &copies, color);
        // The measure, dashed, seen through what lies between, its length at its middle.
        let (a, b) = (t.world(d.dps), t.world(d.dpe));
        draw_measure(cx, &mut p.lines, a, b, color);
        for at in [a, b] {
            p.points.push(PreviewPoint { at, radius: 4.0, fill: Some(WHITE), stroke: Some(color), shape: PointShape::Round });
        }
        p.marks = vec![t.world(d.mps), t.world(d.mpe)];
        let len = (b - a).length();
        let text = vcb::format(len, cx.units());
        p.labels.push(PreviewLabel { a, b, text, color, slot: Some(0), hidden: len <= 1e-9, ..Default::default() });
        // The mirror's badge past the copy (OCL's), in the axis's colour.
        if cx.toggle(MIRROR) && count(self.repeat) == 1 {
            let dir = t.frame.transform_vector3(d.u).normalize();
            p.points.push(mirror_badge(cx, t.world(d.mpe), dir, half_along(t.half(), d.u), color));
        }
        p.copies = copies;
        p.inference = self.snap.clone();
        p
    }

    fn status(&self) -> String {
        if self.aimed {
            return t!("tool.copy_line.hint.place", copy_key = copy_key());
        }
        if self.last.is_some() && self.picker.marquee.is_none() && !self.picker.gathering {
            return t!("tool.copy_line.hint.again");
        }
        self.picker.status(t!("tool.copy_line.hint.pick"))
    }

    fn vcb(&self) -> Option<Vcb> {
        let label = t!("tool.copy_line.vcb.distance");
        if self.aimed {
            // Before any move, the cursor's point is the box's centre.
            let distance = self.end.map_or(0.0, |_| self.distance);
            return Some(Vcb::new(label, VcbKind::Lengths(vec![distance])).counts(1));
        }
        self.last.as_ref().map(|l| Vcb::new(label, VcbKind::Lengths(vec![l.distance])).counts(1))
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
        // Other axes, another box: the handle starts again from its centre.
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
        *self = CopyLineTool::default();
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
    fn the_measure_says_where_the_copies_go() {
        let t = target();
        let end = DVec3::new(500.0, 20.0, 10.0);
        // Edge to edge: 500 from the source's back to the copy's front.
        let d = line_def(&t, end, Repeat::Divide(1), Measure::Outside);
        assert!(((d.dpe - d.dps).length() - 500.0).abs() < 1e-9);
        assert!((d.mpe - DVec3::new(450.0, 20.0, 10.0)).length() < 1e-9);
        assert!((d.dmin - 200.0).abs() < 1e-9);
        // Centre to centre.
        let d = line_def(&t, end, Repeat::Divide(1), Measure::Centered);
        assert!((d.mpe - end).length() < 1e-9);
        // Between the copies: the gap.
        let d = line_def(&t, end, Repeat::Divide(1), Measure::Inside);
        assert!((d.mpe - DVec3::new(550.0, 20.0, 10.0)).length() < 1e-9);
        assert!(((d.dpe - d.dps).length() - 400.0).abs() < 1e-9);
    }

    #[test]
    fn the_cursor_stops_where_the_copies_would_overlap() {
        let t = target();
        let d = line_def(&t, DVec3::new(60.0, 20.0, 10.0), Repeat::Divide(3), Measure::Outside);
        // Three copies over the way, each 100 long, with the source: 400 at least.
        assert!(((d.dpe - d.dps).length() - 400.0).abs() < 1e-9, "{:?}", d);
        let copies = placements(&t, &d, Repeat::Divide(3), false);
        let xs: Vec<f64> = copies.iter().map(|c| c.translation.x).collect();
        assert!(xs.iter().zip([100.0, 200.0, 300.0]).all(|(x, e)| (x - e).abs() < 1e-9), "{xs:?}");
    }

    #[test]
    fn times_repeats_the_step_and_the_mirror_reflects_one_copy() {
        let t = target();
        let d = line_def(&t, DVec3::new(250.0, 20.0, 10.0), Repeat::Times(3), Measure::Centered);
        let xs: Vec<f64> = placements(&t, &d, Repeat::Times(3), false).iter().map(|c| c.translation.x).collect();
        assert!(xs.iter().zip([200.0, 400.0, 600.0]).all(|(x, e)| (x - e).abs() < 1e-9), "{xs:?}");
        let d = line_def(&t, DVec3::new(250.0, 20.0, 10.0), Repeat::Divide(1), Measure::Centered);
        let [m] = placements(&t, &d, Repeat::Divide(1), true)[..] else { panic!() };
        assert!(m.matrix3.determinant() < 0.0);
        // The source's back face (x = 0) goes to the copy's far side (x = 300).
        assert!((m.transform_point3(DVec3::new(0.0, 5.0, 5.0)) - DVec3::new(300.0, 5.0, 5.0)).length() < 1e-9);
    }
}

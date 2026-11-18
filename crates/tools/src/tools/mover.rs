//! Move: click a start point, then a destination, both inferred (§6 ter): the destination
//! follows the red, green and blue axes from the start, and edges' parallels; the arrow keys
//! lock an axis (→ red X, ← green Y, ↑ blue Z, ↓ unlock) and Shift holds the line inferred. A
//! tap of the copy modifier (Option on macOS, Ctrl elsewhere), before or while moving, toggles
//! copying; right after a copy, `5x` makes an array of 5 at the same step and `/5` spreads 5
//! copies over the same distance. A typed distance finishes the move along the current
//! direction (`@/2`: half the way so far), a typed point at it (`[x;y;z]` in the context's
//! axes, `<dx;dy;dz>` from the start). What is moved is picked first (§6 septendecies, [`Picker`]):
//! the selection, else a click, Shift clicks or a rectangle; once moved, the selection is let
//! go and the tool picks again.
//!
//! Paste at the cursor (§6 decies) carries what was just pasted in place with this tool, held
//! by a corner ([`MoveTool::carrying`]): the click puts the paste, moved, in the paste's
//! place in the history (one step), without copying.

use super::DragClicks;
use super::pick::Picker;
use crate::scene::Owner;
use glam::{DAffine3, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{EntityPath, Id, Operation};

use super::{Command, Cx, INK, Key, LastCopy, Motion, OptionValue, Preview, Tool, ToolInput, ToolKind, ToolOption, guide_color};
use crate::draw::{Drawing, Ends, Pen};
use crate::infer::Inference;
use crate::vcb::{PointRef, Vcb, VcbKind, VcbValue};

#[derive(Default)]
pub struct MoveTool {
    picker: Picker,
    /// The selection is taken, as the last input left it (else the tool picks).
    taken: bool,
    state: State,
    copy: bool,
    /// The entities moved (the selection taken).
    moving: Vec<Id>,
    /// The parts moved at any depth (« Parts », §6 septendecies).
    moving_paths: Vec<EntityPath>,
    cursor: Option<DVec3>,
    snap: Option<Inference>,
    /// The copy just made, which a typed count turns into an array.
    last: Option<LastCopy>,
    /// The paste carried: the click replaces it by itself moved.
    pasted: Option<Box<Operation>>,
}

#[derive(Default, Clone, Copy)]
enum State {
    #[default]
    Start,
    Moving {
        from: DVec3,
        to: DVec3,
    },
}

/// The axes' axis `k`, in the world.
fn axis(cx: &Cx<'_>, k: usize) -> DVec3 {
    cx.axis(k)
}

impl MoveTool {
    /// Carries `entities`, just pasted by `pasted`, from `from`: already moving.
    pub fn carrying(entities: Vec<Id>, from: DVec3, pasted: Operation) -> Self {
        MoveTool { state: State::Moving { from, to: from }, moving: entities, pasted: Some(Box::new(pasted)), ..Default::default() }
    }

    /// The destination under `px`: inferred on the scene (ignoring what moves, unless copying:
    /// then the original stays in place) and on the lines through `from`, held on the locked
    /// line if any, else on the horizontal plane through `from`.
    fn target(&self, cx: &Cx<'_>, px: DVec2, from: DVec3) -> (DVec3, Option<Inference>) {
        let keep = |o: &Owner<'_>| self.copy || !(cx.is_moving(o, &self.moving) || cx.is_moving_deep(o, &self.moving_paths));
        match cx.infer_from(px, keep, Some((from, axis(cx, 2))), from, cx.lock()) {
            Some(i) => (i.point, Some(i)),
            None => (from, None),
        }
    }

    fn finish(&mut self, cx: &Cx<'_>, from: DVec3, to: DVec3) -> Vec<Command> {
        self.state = State::Start;
        let entities = std::mem::take(&mut self.moving);
        let paths = std::mem::take(&mut self.moving_paths);
        let moved = if paths.is_empty() { cx.moved() } else { cx.moved_deep(&paths) };
        self.snap = None;
        if let Some(pasted) = self.pasted.take() {
            let op = cx.moved().paste(&pasted, DAffine3::from_translation(to - from));
            return vec![Command::Replace { previous: pasted, op: Box::new(op) }];
        }
        // Moved, the selection is let go: the tool picks again (OCL's restart).
        let op = if self.copy {
            let last = LastCopy::new(moved, entities, Motion::Translate(to - from));
            let op = last.op.clone();
            self.last = Some(last);
            op
        } else {
            moved.transform(entities, DAffine3::from_translation(to - from))
        };
        self.copy = false;
        self.snap = None;
        vec![Command::Apply(op), Command::Select(Vec::new())]
    }
}

impl MoveTool {
    /// Is the tool picking what it moves (§6 septendecies)?
    fn picking(&self) -> bool {
        matches!(self.state, State::Start) && self.pasted.is_none() && !self.taken
    }

    fn input_state(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        self.picker.shift_released(cx);
        let picking = matches!(self.state, State::Start) && self.pasted.is_none() && !self.picker.taken(cx);
        match (input, self.state) {
            // A paste carried is placed, not copied.
            (ToolInput::Key { key: Key::Copy, .. }, _) if self.pasted.is_none() => self.copy = !self.copy,
            (ToolInput::Value(VcbValue::Count(r)), State::Start) => {
                let Some(last) = &mut self.last else { return vec![] };
                return last.retype(*r);
            }
            // Picking (§6 septendecies): what moves becomes the selection.
            _ if picking => {
                self.snap = None;
                self.cursor = None;
                let (out, took) = self.picker.input(input, cx);
                if took {
                    self.last = None;
                }
                return out;
            }
            (ToolInput::Hover { px }, State::Start) => {
                self.snap = cx.infer(*px, |_| true, None);
                self.cursor = self.snap.as_ref().map(|i| i.point);
            }
            (ToolInput::Hover { px }, State::Moving { from, .. }) => {
                let (to, snap) = self.target(cx, *px, from);
                self.state = State::Moving { from, to };
                self.snap = snap;
            }
            (ToolInput::Click { px, .. }, State::Start) => {
                self.last = None;
                let Some(from) = cx.infer(*px, |_| true, None).map(|i| i.point) else { return vec![] };
                self.moving = self.picker.selection(cx);
                self.moving_paths = self.picker.deep(cx);
                self.state = State::Moving { from, to: from };
            }
            (ToolInput::Click { .. }, State::Moving { from, to }) => return self.finish(cx, from, to),
            (ToolInput::Value(VcbValue::Point(p)), State::Moving { from, .. }) => return self.finish(cx, from, *p),
            (ToolInput::Value(VcbValue::Lengths(v)), State::Moving { from, to }) => {
                let d = v[0];
                // Along the locked line, on the cursor's side; else towards the cursor.
                let dir = match cx.lock().map(|l| l.dir) {
                    Some(d) if (to - from).dot(d) < 0.0 => -d,
                    Some(d) => d,
                    None => (to - from).try_normalize().unwrap_or(axis(cx, 0)),
                };
                return self.finish(cx, from, from + dir * d);
            }
            _ => {}
        }
        vec![]
    }
}

impl Tool for MoveTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Move
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        let out = self.input_state(input, cx);
        self.taken = self.picker.taken(cx);
        out
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        match self.state {
            State::Start if self.picking() => {
                let mut p = Preview::default();
                self.picker.preview(&mut p);
                p
            }
            State::Start => {
                let mut drawing = Drawing::default();
                if let Some(c) = self.cursor {
                    cx.drawer(&mut drawing).mark(c);
                }
                Preview { drawing, inference: self.snap.clone(), ..Default::default() }
            }
            State::Moving { from, to } => {
                let color = self.snap.as_ref().map_or(INK, |i| guide_color(i.kind));
                let mut drawing = Drawing::default();
                let d = &mut cx.drawer(&mut drawing);
                // Seen through what lies between: dashed behind faces; the head over everything,
                // readable at the cursor; its length at its middle (not moved yet: only while
                // the distance is typed).
                d.pen(Pen::new(color).x_ray()).line(from, to);
                d.pen(Pen::new(color).over()).arrow_heads(from, to, Ends::End);
                d.length_always(from, to, color, Some(0)).mark(from);
                Preview { drawing, ghost: Some(DAffine3::from_translation(to - from)), inference: self.snap.clone(), ..Default::default() }
            }
        }
    }

    fn status(&self) -> String {
        let hint = match self.state {
            State::Start if self.last.is_some() && !self.picker.busy() => return t!("tool.move.hint.array"),
            State::Start if self.picking() => self.picker.status(t!("tool.move.hint.pick"), t!("tool.move.hint.gather")),
            State::Start => t!("tool.move.hint.start", copy_key = super::copy_key()),
            State::Moving { .. } if self.pasted.is_some() => return t!("tool.move.hint.paste"),
            State::Moving { .. } => t!("tool.move.hint.destination"),
        };
        super::with_copying(hint, self.copy)
    }

    fn vcb(&self) -> Option<Vcb> {
        match self.state {
            State::Moving { from, to } => Some(Vcb {
                point: Some(PointRef { from, to }),
                ..Vcb::new(t!("tool.move.vcb.distance"), VcbKind::Lengths(vec![(to - from).length()]))
            }),
            State::Start => self.last.as_ref().map(|_| Vcb::new(t!("tool.move.vcb.copies"), VcbKind::Count)),
        }
    }

    fn copying(&self) -> bool {
        self.copy
    }

    fn options(&self, _: &Cx<'_>) -> Vec<ToolOption> {
        vec![ToolOption {
            key: "copy",
            label: t!("tool.move.option.copy"),
            tip: t!("tool.move.option.copy_tip", copy_key = super::copy_key()),
            value: OptionValue::Toggle(self.copy),
            choices: Vec::new(),
            saved: false,
            icon: None,
            swatch: None,
        }]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        if let ("copy", OptionValue::Toggle(on)) = (key, value)
            && self.pasted.is_none()
        {
            self.copy = *on;
        }
    }

    /// Picking, a drag draws a selection rectangle; then, it moves from the press to the
    /// release (SketchUp's).
    fn drag_clicks(&self) -> DragClicks {
        match self.state {
            State::Start if self.picking() => DragClicks::No,
            State::Start => DragClicks::PressAndRelease,
            State::Moving { .. } => DragClicks::Release,
        }
    }

    fn busy(&self) -> bool {
        matches!(self.state, State::Moving { .. }) || self.picker.busy()
    }

    fn picks_first(&self) -> bool {
        true
    }

    fn picks_parts(&self) -> bool {
        true
    }

    fn leave(&mut self) {
        self.picker.leave();
    }

    fn back(&mut self) {
        if !self.picker.back() {
            self.reset();
        }
    }

    fn reset(&mut self) {
        self.picker = Picker::default();
        self.state = State::Start;
        self.copy = false;
        self.last = None;
        self.moving.clear();
        self.moving_paths.clear();
        self.snap = None;
        self.pasted = None;
    }
}

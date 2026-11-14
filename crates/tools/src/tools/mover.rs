//! Move: click a start point, then a destination, both inferred (§6 ter): the destination
//! follows the red, green and blue axes from the start, and edges' parallels; the arrow keys
//! lock an axis (→ red X, ← green Y, ↑ blue Z, ↓ unlock) and Shift holds the line inferred. A
//! tap of the copy modifier (Option on macOS, Ctrl elsewhere), before or while moving, toggles
//! copying; right after a copy, `5x` makes an array of 5 at the same step and `/5` spreads 5
//! copies over the same distance. A typed distance finishes the move along the current
//! direction (`@/2`: half the way so far), a typed point at it (`[x;y;z]` in the context's
//! axes, `<dx;dy;dz>` from the start). With nothing selected, the first click takes the entity under the cursor
//! (SketchUp).
//!
//! Paste at the cursor (§6 decies) carries what was just pasted in place with this tool, held
//! by a corner ([`MoveTool::carrying`]): the click puts the paste, moved, in the paste's
//! place in the history (one step), without copying.

use crate::scene::Owner;
use glam::{DAffine3, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::{Id, Operation};

use super::{
    Command, Cx, INK, Key, LastCopy, Motion, OptionValue, Preview, PreviewLabel, PreviewLine, Tool, ToolInput, ToolKind, ToolOption,
    guide_color,
};
use crate::infer::Inference;
use crate::vcb::{self, PointRef, Vcb, VcbKind, VcbValue};

#[derive(Default)]
pub struct MoveTool {
    state: State,
    copy: bool,
    /// The entities moved (the selection, or what the first click took).
    moving: Vec<Id>,
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
        let keep = |o: &Owner<'_>| self.copy || !cx.is_moving(o, &self.moving);
        match cx.infer_from(px, keep, Some((from, axis(cx, 2))), from, cx.lock()) {
            Some(i) => (i.point, Some(i)),
            None => (from, None),
        }
    }

    fn finish(&mut self, cx: &Cx<'_>, from: DVec3, to: DVec3) -> Vec<Command> {
        self.state = State::Start;
        let entities = std::mem::take(&mut self.moving);
        self.snap = None;
        if let Some(pasted) = self.pasted.take() {
            let op = cx.moved().paste(&pasted, DAffine3::from_translation(to - from));
            return vec![Command::Replace { previous: pasted, op: Box::new(op) }];
        }
        let op = if self.copy {
            let last = LastCopy::new(cx.moved(), entities, Motion::Translate(to - from));
            let op = last.op.clone();
            self.last = Some(last);
            op
        } else {
            cx.moved().transform(entities, DAffine3::from_translation(to - from))
        };
        self.copy = false;
        self.snap = None;
        vec![Command::Apply(op)]
    }
}

impl Tool for MoveTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Move
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match (input, self.state) {
            (ToolInput::Hover { px }, State::Start) => {
                self.snap = cx.infer(*px, |_| true, None);
                self.cursor = self.snap.as_ref().map(|i| i.point);
            }
            (ToolInput::Hover { px }, State::Moving { from, .. }) => {
                let (to, snap) = self.target(cx, *px, from);
                self.state = State::Moving { from, to };
                self.snap = snap;
            }
            // A paste carried is placed, not copied.
            (ToolInput::Key { key: Key::Copy, .. }, _) if self.pasted.is_none() => self.copy = !self.copy,
            (ToolInput::Click { px, .. }, State::Start) => {
                self.last = None;
                let inf = cx.infer(*px, |_| true, None);
                let mut out = vec![];
                self.moving = cx.selection.to_vec();
                if self.moving.is_empty() {
                    // What is under the cursor, else what the inferred point is on.
                    match cx.entity_under(*px, inf.as_ref()) {
                        Some(id) => {
                            self.moving = vec![id];
                            out.push(Command::Select(vec![id]));
                        }
                        None => return vec![Command::Say(t!("tool.move.say.nothing"))],
                    }
                }
                let Some(from) = inf.map(|i| i.point) else { return out };
                self.state = State::Moving { from, to: from };
                return out;
            }
            (ToolInput::Click { .. }, State::Moving { from, to }) => return self.finish(cx, from, to),
            (ToolInput::Value(VcbValue::Count(r)), State::Start) => {
                let Some(last) = &mut self.last else { return vec![] };
                return last.retype(*r);
            }
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

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        match self.state {
            State::Start => Preview { marks: self.cursor.into_iter().collect(), inference: self.snap.clone(), ..Default::default() },
            State::Moving { from, to } => {
                let color = self.snap.as_ref().map_or(INK, |i| guide_color(i.kind));
                // Seen through what lies between: dashed behind faces; its length at its middle.
                let xray = vec![PreviewLine { a: from, b: to, color, bold: false }];
                // Not moved yet: only while the distance is typed.
                let len = (to - from).length();
                let text = vcb::format(len, cx.units());
                let labels = vec![PreviewLabel { a: from, b: to, text, color, slot: Some(0), hidden: len <= 1e-9, ..Default::default() }];
                Preview {
                    // The head over everything: it stays readable at the cursor.
                    lines: super::arrow_head(cx.camera, cx.size, from, to, color),
                    xray,
                    labels,
                    marks: vec![from],
                    ghost: Some(DAffine3::from_translation(to - from)),
                    inference: self.snap.clone(),
                    ..Default::default()
                }
            }
        }
    }

    fn status(&self) -> String {
        let hint = match self.state {
            State::Start if self.last.is_some() => return t!("tool.move.hint.array"),
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
        }]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        if let ("copy", OptionValue::Toggle(on)) = (key, value)
            && self.pasted.is_none()
        {
            self.copy = *on;
        }
    }

    fn busy(&self) -> bool {
        matches!(self.state, State::Moving { .. })
    }

    fn reset(&mut self) {
        self.state = State::Start;
        self.copy = false;
        self.last = None;
        self.moving.clear();
        self.snap = None;
        self.pasted = None;
    }
}

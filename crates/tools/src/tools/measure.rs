//! The tape measure (docs/ARCHITECTURE.md §6 quinquies): click a point, the length to the point
//! hovered shows on the segment and in the measurements box. With the option "Create guides"
//! on, it draws a guide instead, of the kind the option "Guide" says: Auto — parallel to a
//! straight edge, concentric to an arc; Line — parallel to a straight edge, tangent to an arc,
//! else through two points; Circle — concentric to an arc, else centred on the point clicked.
//! Where curves cross, each gives its guide, and the fallback one more: the other modifier
//! turns them over after the first click.

use glam::{DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::{GuideShape, Operation};

use super::{
    BLUE, Command, Cx, GREEN, INK, Key, MAGENTA, OptionChoice, OptionValue, Preview, PreviewLabel, RED, Tool, ToolInput, ToolKind,
    ToolOption, alt_key, copy_key,
};
use crate::guides;
use crate::infer::{EdgeRef, InferKind, Inference};
use crate::vcb::{self, Vcb, VcbKind, VcbValue};

/// The saved option: an edge starts a guide (on unless turned off).
const GUIDES: &str = "guides";
/// The saved choice: the kind of guide drawn (Auto unless set).
const SHAPE: &str = "guide";
const AUTO: &str = "auto";
const LINE: &str = "line";
const CIRCLE: &str = "circle";

/// Shorter than this is nothing (mm).
const MIN: f64 = 1e-6;
/// The cursor this close to the edge's line (mm) gives no side to offset a typed guide to.
const SIDE: f64 = 1e-3;
/// A length is written on its segment only when it is this long on screen (pixels).
const LABEL_PX: f64 = 30.0;

/// The kind of guide the tape draws (the option "Guide").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// What the edge clicked gives: a line from a straight edge, a circle from an arc; from
    /// anything else, a measure.
    Auto,
    /// A line: parallel to a straight edge, tangent to an arc, else through two points.
    Line,
    /// A circle: concentric to an arc, else centred on the point clicked.
    Circle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// Waiting for the first click.
    Idle,
    /// Measuring from a point (world).
    Measure { from: DVec3 },
    /// A guide parallel to a straight edge: the point clicked on it and its direction (world,
    /// unit); the plane it is offset in when the cursor points at nothing (and the
    /// perpendicular inferred lies in), see [`offset_plane`]. Tangent to an arc there: the
    /// tangent's direction, in the arc's plane.
    Parallel { at: DVec3, dir: DVec3, plane: Option<(DVec3, DVec3)>, tangent: bool },
    /// A guide concentric to an arc: the point clicked on it, its centre, normal (unit) and
    /// radius (world). A circle centred on the point clicked is one to a circle of radius
    /// zero there.
    Concentric { at: DVec3, center: DVec3, normal: DVec3, radius: f64 },
    /// A guide line through the point clicked and the next one (world).
    Through { from: DVec3 },
}

pub struct MeasureTool {
    state: State,
    /// The point under the cursor.
    hover: Option<Inference>,
    /// The option "Create guides", as last read.
    guides: bool,
    /// The option "Guide", as last read.
    shape: Shape,
    /// The first click, kept to start over when an option changes in between.
    first: Option<Inference>,
    /// What the first click may start, each with the curve it refers to (see
    /// [`MeasureTool::starts`]); the one `state` is.
    starts: Vec<(Option<EdgeRef>, State)>,
    pick: usize,
    /// The cursor, last hovered (pixels): the point again when the guide changes.
    px: Option<DVec2>,
    /// An option changed in between: start over from the first click at the next input.
    restart: bool,
}

impl Default for MeasureTool {
    fn default() -> Self {
        MeasureTool {
            state: State::Idle,
            hover: None,
            guides: true,
            shape: Shape::Auto,
            first: None,
            starts: Vec::new(),
            pick: 0,
            px: None,
            restart: false,
        }
    }
}

/// Does an edge start a guide (the saved option, on by default)?
fn guides_on(cx: &Cx<'_>) -> bool {
    !matches!(cx.options.get(GUIDES), Some(OptionValue::Toggle(false)))
}

/// The kind of guide (the saved choice, Auto by default).
fn shape_of(cx: &Cx<'_>) -> Shape {
    shape_from(cx.choice(SHAPE).unwrap_or(AUTO))
}

fn shape_from(value: &str) -> Shape {
    match value {
        LINE => Shape::Line,
        CIRCLE => Shape::Circle,
        _ => Shape::Auto,
    }
}

impl MeasureTool {
    /// The point the cursor gives at `px`, lines inferred from the first point.
    fn infer(&self, cx: &Cx<'_>, px: DVec2) -> Option<Inference> {
        match self.state {
            State::Idle => cx.infer(px, |_| true, None),
            State::Measure { from } | State::Through { from } => cx.infer_from(px, |_| true, None, from, cx.lock()),
            State::Parallel { at, plane, .. } => cx.infer_from(px, |_| true, plane, at, cx.lock()),
            State::Concentric { at, center, normal, .. } => {
                cx.infer_from(px, |_| true, Some((center, normal)), at, cx.lock()).map(|mut i| {
                    // Anywhere off the arc's plane: the point on the plane under the cursor.
                    if !i.kind.is_feature()
                        && let Some(p) = cx.ray(px).hit_plane(center, normal)
                    {
                        i.point = p;
                    }
                    i
                })
            }
        }
    }

    /// What the first click at `inf` may start, each with the curve it refers to, and the one
    /// it does: a guide from the curve clicked, or one from each curve crossing there (ending
    /// there, at a vertex), then the fallback (a measure, a line through two points, a circle
    /// centred there). At a point, the guide from the edge last hovered when it is one of them
    /// (`Cx::reference`), else — and always in Auto, which measures from a point — the fallback.
    fn starts(&self, cx: &Cx<'_>, inf: &Inference) -> (Vec<(Option<EdgeRef>, State)>, usize) {
        let at = inf.point;
        if !self.guides {
            return (vec![(None, State::Measure { from: at })], 0);
        }
        // On an edge, a guide or an axis as drawn: a guide offset from it.
        let on_edge = match inf.kind {
            InferKind::OnEdge | InferKind::OnGuide => true,
            InferKind::OnLine(l) => l.is_fixed(),
            _ => false,
        };
        let curves: Vec<EdgeRef> = if on_edge { inf.edge.into_iter().collect() } else { inf.crossed.clone() };
        let guides: Vec<(EdgeRef, State)> = curves.into_iter().filter_map(|e| Some((e, self.guide_from(cx, inf, e)?))).collect();
        let pick = if on_edge && !guides.is_empty() {
            0
        } else if self.shape == Shape::Auto {
            guides.len()
        } else {
            cx.reference.and_then(|r| guides.iter().position(|(e, _)| same_curve(*e, r))).unwrap_or(guides.len())
        };
        let mut starts: Vec<(Option<EdgeRef>, State)> = guides.into_iter().map(|(e, s)| (Some(e), s)).collect();
        let fallback = match self.shape {
            Shape::Auto => State::Measure { from: at },
            Shape::Line => State::Through { from: at },
            // Centred on the point clicked, in the face clicked, else level.
            Shape::Circle => {
                let normal = inf.hit.as_ref().map_or_else(|| cx.axis(2), |h| h.normal);
                State::Concentric { at, center: at, normal, radius: 0.0 }
            }
        };
        starts.push((None, fallback));
        (starts, pick)
    }

    /// The guide of the kind chosen the curve `edge` gives, through the point clicked at `inf`.
    fn guide_from(&self, cx: &Cx<'_>, inf: &Inference, edge: EdgeRef) -> Option<State> {
        let at = inf.point;
        match (self.shape, edge) {
            (Shape::Auto | Shape::Line, EdgeRef::Straight(dir)) => {
                Some(State::Parallel { at, dir, plane: offset_plane(cx, inf, dir), tangent: false })
            }
            (Shape::Auto | Shape::Circle, EdgeRef::Arc { tangent, radial, center, radius }) if radius > MIN => {
                Some(State::Concentric { at, center, normal: radial.cross(tangent).normalize(), radius })
            }
            // Tangent to the arc: offset along its radius, in its plane.
            (Shape::Line, EdgeRef::Arc { tangent, radial, center, .. }) => {
                Some(State::Parallel { at, dir: tangent, plane: Some((center, radial.cross(tangent).normalize())), tangent: true })
            }
            _ => None,
        }
    }

    /// Starts from the first click `inf`: the guide it gives by default.
    fn begin(&mut self, cx: &Cx<'_>, inf: Inference) {
        (self.starts, self.pick) = self.starts(cx, &inf);
        self.state = self.starts[self.pick].1;
        self.first = Some(inf);
    }

    /// The guide through `p` (world), its offset from the edge (signed: a circle shrunk is
    /// negative), and the segment that offset runs along: for a line, from the point clicked,
    /// square to the edge, to the guide; for a circle, along the radius through the cursor.
    fn guide_through(&self, p: DVec3) -> Option<(GuideShape, f64, (DVec3, DVec3))> {
        match self.state {
            State::Parallel { at, dir, .. } => {
                let off = (p - at) - dir * (p - at).dot(dir);
                Some((GuideShape::Line { origin: p, dir }, off.length(), (at, at + off)))
            }
            State::Concentric { at, center, normal, radius } => {
                let q = p - normal * (p - center).dot(normal);
                let r = (q - center).length();
                // Along the radius through the cursor; at the centre, the point clicked's.
                let out = (q - center).try_normalize().or_else(|| (at - center).try_normalize()).unwrap_or(DVec3::ZERO);
                Some((GuideShape::Circle { center, normal, radius: r }, r - radius, (center + out * radius, q)))
            }
            State::Through { from } => {
                let dir = (p - from).try_normalize()?;
                Some((GuideShape::Line { origin: from, dir }, (p - from).length(), (from, p)))
            }
            _ => None,
        }
    }

    /// The guide at the typed offset `d` towards where the cursor is (a negative one, the
    /// other way).
    fn guide_at(&self, d: f64) -> Result<GuideShape, String> {
        let hovered = self.hover.as_ref().and_then(|h| self.guide_through(h.point));
        match self.state {
            State::Parallel { at, dir, plane, .. } => {
                // No offset: the guide along the edge, through the point clicked.
                if d.abs() < MIN {
                    return Ok(GuideShape::Line { origin: at, dir });
                }
                // The side the cursor is on; none yet: square to the edge in the face clicked.
                let side = hovered
                    .filter(|(_, off, _)| *off > SIDE)
                    .and_then(|(_, _, (foot, p))| (p - foot).try_normalize())
                    .or_else(|| plane.and_then(|(_, n)| n.cross(dir).try_normalize()))
                    .ok_or_else(|| t!("tool.measure.say.no_side"))?;
                Ok(GuideShape::Line { origin: at + side * d, dir })
            }
            State::Concentric { center, normal, radius, .. } => {
                let sign = if hovered.is_some_and(|(_, off, _)| off < -SIDE) { -1.0 } else { 1.0 };
                let r = radius + sign * d;
                if r < MIN {
                    return Err(t!("tool.measure.say.no_radius"));
                }
                Ok(GuideShape::Circle { center, normal, radius: r })
            }
            _ => Err(String::new()),
        }
    }

    /// The commands adding the guide `shape` (world) to the active context, back to the first
    /// click.
    fn add(&mut self, cx: &Cx<'_>, shape: GuideShape) -> Vec<Command> {
        self.reset();
        let (component, frame) = cx.guide_owner();
        vec![Command::Apply(Operation::AddGuide { component, shape: shape.transformed(&frame.inverse()) })]
    }

    /// The length the measurements box shows now.
    fn length(&self) -> f64 {
        let Some(p) = self.hover.as_ref().map(|h| h.point) else { return 0.0 };
        match self.state {
            State::Idle => 0.0,
            State::Measure { from } | State::Through { from } => (p - from).length(),
            _ => self.guide_through(p).map_or(0.0, |(_, off, _)| off.abs()),
        }
    }
}

/// Are `a` and `b` the same curve (seen at different points of it)?
fn same_curve(a: EdgeRef, b: EdgeRef) -> bool {
    match (a, b) {
        (EdgeRef::Straight(a), EdgeRef::Straight(b)) => a.dot(b).abs() > 1.0 - 1e-9,
        (
            EdgeRef::Arc { tangent: t, radial: r, center: c, radius },
            EdgeRef::Arc { tangent: t2, radial: r2, center: c2, radius: radius2 },
        ) => (c - c2).length() < SIDE && (radius - radius2).abs() < SIDE && r.cross(t).dot(r2.cross(t2)).abs() > 1.0 - 1e-9,
        _ => false,
    }
}

/// The plane a guide parallel to the edge `dir` (world, unit) clicked at `inf` is offset in
/// when the cursor points at nothing: the horizontal one through the point clicked (the
/// axes'), unless the edge is upright — then the face clicked, else the upright plane through
/// the edge facing the eye.
fn offset_plane(cx: &Cx<'_>, inf: &Inference, dir: DVec3) -> Option<(DVec3, DVec3)> {
    let up = cx.axis(2);
    if dir.dot(up).abs() < 1.0 - 1e-6 {
        return Some((inf.point, up));
    }
    let facing = || {
        let f = cx.camera.forward();
        (f - dir * f.dot(dir)).try_normalize().map(|n| (inf.point, n))
    };
    inf.hit.as_ref().map(|h| (h.point, h.normal)).or_else(facing)
}

/// The colour of the axis (of the axes tools draw in) `v` runs along, else ink.
fn axis_color(cx: &Cx<'_>, v: DVec3) -> [f32; 4] {
    let Some(v) = v.try_normalize() else { return INK };
    match (0..3).find(|k| cx.axis(*k).dot(v).abs() > 1.0 - 1e-9) {
        Some(0) => RED,
        Some(1) => GREEN,
        Some(_) => BLUE,
        None => INK,
    }
}

/// The length of `a`–`b` written along it, when long enough on screen.
fn label(cx: &Cx<'_>, out: &mut Vec<PreviewLabel>, a: DVec3, b: DVec3, color: [f32; 4]) {
    let len = (b - a).length();
    if len >= MIN && len > cx.camera.world_per_pixel(cx.size, (a + b) * 0.5) * LABEL_PX {
        out.push(PreviewLabel { a, b, text: vcb::format(len, cx.units()), color });
    }
}

impl Tool for MeasureTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Measure
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        self.guides = guides_on(cx);
        self.shape = shape_of(cx);
        if std::mem::take(&mut self.restart)
            && self.state != State::Idle
            && let Some(first) = self.first.clone()
        {
            self.begin(cx, first);
        }
        match input {
            ToolInput::Hover { px } => {
                self.px = Some(*px);
                self.hover = self.infer(cx, *px);
            }
            ToolInput::Click { px, .. } => {
                self.hover = self.infer(cx, *px);
                let Some(inf) = self.hover.clone() else { return vec![] };
                match self.state {
                    State::Idle => self.begin(cx, inf),
                    State::Measure { .. } => self.reset(),
                    State::Through { .. } => {
                        let Some((shape, _, _)) = self.guide_through(inf.point) else {
                            return vec![Command::Say(t!("tool.measure.say.no_line"))];
                        };
                        return self.add(cx, shape);
                    }
                    State::Parallel { .. } | State::Concentric { .. } => {
                        let Some((shape, _, _)) = self.guide_through(inf.point) else { return vec![] };
                        // A guide may lie on its edge (a zero offset); a circle needs a radius.
                        if let GuideShape::Circle { radius, .. } = shape
                            && radius < MIN
                        {
                            return vec![Command::Say(t!("tool.measure.say.no_radius"))];
                        }
                        return self.add(cx, shape);
                    }
                }
            }
            ToolInput::Value(VcbValue::Lengths(v)) => {
                if let (State::Parallel { .. } | State::Concentric { .. }, Some(d)) = (self.state, v.first()) {
                    return match self.guide_at(*d) {
                        Ok(shape) => self.add(cx, shape),
                        Err(e) => vec![Command::Say(e)],
                    };
                }
            }
            // The other modifier tapped after the first click: the next guide it may start.
            ToolInput::Key { key: Key::Alt, .. } if self.state != State::Idle && self.starts.len() > 1 => {
                self.pick = (self.pick + 1) % self.starts.len();
                self.state = self.starts[self.pick].1;
                self.hover = self.px.and_then(|px| self.infer(cx, px));
            }
            // The copy modifier tapped: guides or not (SketchUp's Ctrl).
            ToolInput::Key { key: Key::Copy, .. } => {
                return vec![Command::SetOption { key: GUIDES, value: OptionValue::Toggle(!self.guides) }];
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let mut lines = Vec::new();
        let mut guide_strokes = Vec::new();
        let mut labels = Vec::new();
        let mut marks = Vec::new();
        let inference = self.hover.clone();
        let Some(p) = inference.as_ref().map(|i| i.point) else { return Preview::default() };
        // The curve the guide refers to, in magenta: the one picked; before the first click, at
        // a point where several cross, the one it would take.
        let reference = match (&self.first, &inference) {
            (Some(first), _) if self.state != State::Idle => self.starts.get(self.pick).and_then(|s| s.0).map(|e| (first.point, e)),
            (_, Some(hover)) if !hover.crossed.is_empty() => {
                let (starts, pick) = self.starts(cx, hover);
                starts[pick].0.map(|e| (hover.point, e))
            }
            _ => None,
        };
        if let Some((at, edge)) = reference {
            let shape = match edge {
                EdgeRef::Straight(dir) => GuideShape::Line { origin: at, dir },
                EdgeRef::Arc { tangent, radial, center, radius } => {
                    GuideShape::Circle { center, normal: radial.cross(tangent).normalize(), radius }
                }
            };
            guide_strokes.extend(guides::strokes(cx.camera, cx.size, &shape).into_iter().map(|s| (s, MAGENTA)));
        }
        match self.state {
            State::Idle => {}
            State::Measure { from } | State::Through { from } => {
                if let (State::Through { .. }, Some((shape, _, _))) = (self.state, self.guide_through(p)) {
                    guide_strokes.extend(guides::strokes(cx.camera, cx.size, &shape).into_iter().map(|s| (s, INK)));
                }
                let color = inference.as_ref().map_or(INK, |i| super::guide_color(i.kind));
                super::dashed(cx.camera, cx.size, &mut lines, from, p, color);
                label(cx, &mut labels, from, p, color);
                marks.push(from);
            }
            State::Parallel { at, .. } | State::Concentric { at, .. } => {
                if let Some((shape, _, (a, b))) = self.guide_through(p) {
                    let valid = !matches!(shape, GuideShape::Circle { radius, .. } if radius < MIN);
                    if valid {
                        guide_strokes.extend(guides::strokes(cx.camera, cx.size, &shape).into_iter().map(|s| (s, INK)));
                    }
                    // The offset in the colour of the axis it runs along, else ink.
                    let color = axis_color(cx, b - a);
                    super::dashed(cx.camera, cx.size, &mut lines, a, b, color);
                    label(cx, &mut labels, a, b, color);
                }
                marks.push(at);
            }
        }
        Preview { lines, guides: guide_strokes, labels, marks, inference, ..Default::default() }
    }

    fn status(&self) -> String {
        let hint = match self.state {
            State::Idle if !self.guides => t!("tool.measure.hint.first_measure_only", copy_key = copy_key()),
            State::Idle => match self.shape {
                Shape::Auto => t!("tool.measure.hint.first", copy_key = copy_key()),
                Shape::Line => t!("tool.measure.hint.first_line", copy_key = copy_key()),
                Shape::Circle => t!("tool.measure.hint.first_circle", copy_key = copy_key()),
            },
            State::Measure { .. } => t!("tool.measure.hint.measure"),
            State::Through { .. } => t!("tool.measure.hint.through"),
            State::Concentric { radius, .. } if radius < MIN => t!("tool.measure.hint.centered"),
            State::Concentric { .. } => t!("tool.measure.hint.concentric"),
            State::Parallel { tangent: true, .. } => t!("tool.measure.hint.tangent"),
            State::Parallel { .. } => t!("tool.measure.hint.parallel"),
        };
        if self.state != State::Idle && self.starts.len() > 1 {
            format!("{hint} · {}", t!("tool.measure.hint.other", alt_key = alt_key()))
        } else {
            hint
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        let label = match self.state {
            State::Idle => return None,
            State::Measure { .. } | State::Through { .. } => t!("tool.measure.vcb.length"),
            State::Concentric { radius, .. } if radius < MIN => t!("tool.measure.vcb.radius"),
            State::Parallel { .. } | State::Concentric { .. } => t!("tool.measure.vcb.offset"),
        };
        Some(Vcb::new(label, VcbKind::Lengths(vec![self.length()])))
    }

    fn busy(&self) -> bool {
        self.state != State::Idle
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let choice = |value, label, tip| OptionChoice { value, label, tip };
        let shape = match shape_of(cx) {
            Shape::Auto => AUTO,
            Shape::Line => LINE,
            Shape::Circle => CIRCLE,
        };
        vec![
            ToolOption {
                key: GUIDES,
                label: t!("tool.measure.option.guides"),
                tip: t!("tool.measure.option.guides_tip", copy_key = copy_key()),
                value: OptionValue::Toggle(guides_on(cx)),
                choices: Vec::new(),
                saved: true,
            },
            ToolOption {
                key: SHAPE,
                label: t!("tool.measure.option.shape"),
                tip: t!("tool.measure.option.shape_tip"),
                value: OptionValue::Choice(shape.into()),
                choices: vec![
                    choice(AUTO, t!("tool.measure.option.auto"), t!("tool.measure.option.auto_tip")),
                    choice(LINE, t!("tool.measure.option.line"), t!("tool.measure.option.line_tip")),
                    choice(CIRCLE, t!("tool.measure.option.circle"), t!("tool.measure.option.circle_tip")),
                ],
                saved: true,
            },
        ]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        match (key, value) {
            (GUIDES, OptionValue::Toggle(on)) => self.guides = *on,
            (SHAPE, OptionValue::Choice(v)) => self.shape = shape_from(v),
            _ => return,
        }
        // Changed in between: what the first click starts again, at the next input (the
        // editor hovers again at once).
        self.restart = true;
    }

    fn leave(&mut self) {
        self.hover = None;
    }

    fn reset(&mut self) {
        self.state = State::Idle;
        self.first = None;
        self.starts.clear();
        self.pick = 0;
    }
}

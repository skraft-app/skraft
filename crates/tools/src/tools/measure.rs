//! The tape measure (docs/ARCHITECTURE.md §6 quinquies): click a point, the length to the point
//! hovered shows on the segment and in the measurements box. Started on an edge, with the
//! option "Create guides" on, it draws a guide instead: parallel to a straight edge,
//! concentric to an arc, through the point clicked next or at the offset typed.

use glam::{DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::{GuideShape, Operation};

use super::{BLUE, Command, Cx, GREEN, INK, Key, OptionValue, Preview, PreviewLabel, RED, Tool, ToolInput, ToolKind, ToolOption, copy_key};
use crate::guides;
use crate::infer::{EdgeRef, InferKind, Inference};
use crate::vcb::{self, Vcb, VcbKind, VcbValue};

/// The saved option: an edge starts a guide (on unless turned off).
const GUIDES: &str = "guides";

/// Shorter than this is nothing (mm).
const MIN: f64 = 1e-6;
/// The cursor this close to the edge's line (mm) gives no side to offset a typed guide to.
const SIDE: f64 = 1e-3;
/// A length is written on its segment only when it is this long on screen (pixels).
const LABEL_PX: f64 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// Waiting for the first click.
    Idle,
    /// Measuring from a point (world).
    Measure { from: DVec3 },
    /// A guide parallel to a straight edge: the point clicked on it and its direction (world,
    /// unit); the plane it is offset in when the cursor points at nothing (and the
    /// perpendicular inferred lies in), see [`offset_plane`].
    Parallel { at: DVec3, dir: DVec3, plane: Option<(DVec3, DVec3)> },
    /// A guide concentric to an arc: the point clicked on it, its centre, normal (unit) and
    /// radius (world).
    Concentric { at: DVec3, center: DVec3, normal: DVec3, radius: f64 },
}

pub struct MeasureTool {
    state: State,
    /// The point under the cursor.
    hover: Option<Inference>,
    /// The option "Create guides", as last read.
    guides: bool,
}

impl Default for MeasureTool {
    fn default() -> Self {
        MeasureTool { state: State::Idle, hover: None, guides: true }
    }
}

/// Does an edge start a guide (the saved option, on by default)?
fn guides_on(cx: &Cx<'_>) -> bool {
    !matches!(cx.options.get(GUIDES), Some(OptionValue::Toggle(false)))
}

impl MeasureTool {
    /// The point the cursor gives at `px`, lines inferred from the first point.
    fn infer(&self, cx: &Cx<'_>, px: DVec2) -> Option<Inference> {
        match self.state {
            State::Idle => cx.infer(px, |_| true, None),
            State::Measure { from } => cx.infer_from(px, |_| true, None, from, cx.lock()),
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

    /// What the first click at `inf` starts.
    fn start(&self, cx: &Cx<'_>, inf: &Inference) -> State {
        // On an edge, a guide or an axis as drawn: a guide offset from it.
        let on_edge = match inf.kind {
            InferKind::OnEdge | InferKind::OnGuide => true,
            InferKind::OnLine(l) => l.is_fixed(),
            _ => false,
        };
        if self.guides && on_edge {
            match inf.edge {
                Some(EdgeRef::Straight(dir)) => return State::Parallel { at: inf.point, dir, plane: offset_plane(cx, inf, dir) },
                Some(EdgeRef::Arc { tangent, radial, center, radius }) if radius > MIN => {
                    let normal = radial.cross(tangent).normalize();
                    return State::Concentric { at: inf.point, center, normal, radius };
                }
                _ => {}
            }
        }
        State::Measure { from: inf.point }
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
                let out = (q - center).try_normalize().unwrap_or_else(|| (at - center).normalize());
                Some((GuideShape::Circle { center, normal, radius: r }, r - radius, (center + out * radius, q)))
            }
            _ => None,
        }
    }

    /// The guide at the typed offset `d` towards where the cursor is (a negative one, the
    /// other way).
    fn guide_at(&self, d: f64) -> Result<GuideShape, String> {
        let hovered = self.hover.as_ref().and_then(|h| self.guide_through(h.point));
        match self.state {
            State::Parallel { at, dir, plane } => {
                // The side the cursor is on; none yet: square to the edge in the face clicked.
                let side = hovered
                    .filter(|(_, off, _)| *off > SIDE)
                    .and_then(|(_, _, (foot, p))| (p - foot).try_normalize())
                    .or_else(|| plane.and_then(|(_, n)| n.cross(dir).try_normalize()))
                    .ok_or_else(|| t!("tool.measure.say.no_offset"))?;
                if d.abs() < MIN {
                    return Err(t!("tool.measure.say.no_offset"));
                }
                Ok(GuideShape::Line { origin: at + side * d, dir })
            }
            State::Concentric { center, normal, radius, .. } => {
                let sign = if hovered.is_some_and(|(_, off, _)| off < -SIDE) { -1.0 } else { 1.0 };
                if d.abs() < MIN {
                    return Err(t!("tool.measure.say.no_offset"));
                }
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
        self.state = State::Idle;
        let (component, frame) = cx.guide_owner();
        vec![Command::Apply(Operation::AddGuide { component, shape: shape.transformed(&frame.inverse()) })]
    }

    /// The length the measurements box shows now.
    fn length(&self) -> f64 {
        let Some(p) = self.hover.as_ref().map(|h| h.point) else { return 0.0 };
        match self.state {
            State::Idle => 0.0,
            State::Measure { from } => (p - from).length(),
            _ => self.guide_through(p).map_or(0.0, |(_, off, _)| off.abs()),
        }
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
        match input {
            ToolInput::Hover { px } => self.hover = self.infer(cx, *px),
            ToolInput::Click { px, .. } => {
                self.hover = self.infer(cx, *px);
                let Some(inf) = self.hover.clone() else { return vec![] };
                match self.state {
                    State::Idle => self.state = self.start(cx, &inf),
                    State::Measure { .. } => self.state = State::Idle,
                    State::Parallel { .. } | State::Concentric { .. } => {
                        let Some((shape, off, _)) = self.guide_through(inf.point) else { return vec![] };
                        if off.abs() < MIN {
                            return vec![Command::Say(t!("tool.measure.say.no_offset"))];
                        }
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
        match self.state {
            State::Idle => {}
            State::Measure { from } => {
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
        match self.state {
            State::Idle if self.guides => t!("tool.measure.hint.first", copy_key = copy_key()),
            State::Idle => t!("tool.measure.hint.first_measure_only", copy_key = copy_key()),
            State::Measure { .. } => t!("tool.measure.hint.measure"),
            State::Parallel { .. } | State::Concentric { .. } => t!("tool.measure.hint.guide"),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        let label = match self.state {
            State::Idle => return None,
            State::Measure { .. } => t!("tool.measure.vcb.length"),
            State::Parallel { .. } | State::Concentric { .. } => t!("tool.measure.vcb.offset"),
        };
        Some(Vcb::new(label, VcbKind::Lengths(vec![self.length()])))
    }

    fn busy(&self) -> bool {
        self.state != State::Idle
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        vec![ToolOption {
            key: GUIDES,
            label: t!("tool.measure.option.guides"),
            tip: t!("tool.measure.option.guides_tip", copy_key = copy_key()),
            value: OptionValue::Toggle(guides_on(cx)),
            choices: Vec::new(),
            saved: true,
        }]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        if let (GUIDES, OptionValue::Toggle(on)) = (key, value) {
            self.guides = *on;
            // Turned off while drawing a guide: a measure from the point clicked.
            if !on && let State::Parallel { at, .. } | State::Concentric { at, .. } = self.state {
                self.state = State::Measure { from: at };
            }
        }
    }

    fn leave(&mut self) {
        self.hover = None;
    }

    fn reset(&mut self) {
        self.state = State::Idle;
    }
}

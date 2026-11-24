//! « Réorienter » (OpenCutList's SmartAxes, docs/tools/reorient.md): gives the part or assembly
//! clicked new axes, nothing moving in the world. Four modes, Tab going to the next: swap length
//! and width, swap front and back, adapt the axes to a face and an edge, move the origin. As
//! SmartAxes, it shows the target **as it is**: its axes, arrows and box; what a click would give
//! is drawn over it in magenta: the turn of the two swaps, « Adapter »'s axes, « Origine »'s.

use glam::{DAffine3, DMat3, DQuat, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{Behavior, EntityPath, Id, Operation, Subject};

use super::face::Face;
use super::{
    BLUE, Command, Cursor, Cx, GREEN, INK, MAGENTA, OptionChoice, OptionValue, Preview, RED, Tool, ToolInput, ToolKind, ToolOption, WHITE,
    alt_key,
};
use crate::draw::{Brush, Drawer, Drawing, Ends, Pen, Stipple};
use crate::infer::Inference;

/// The saved choice: the mode.
pub const MODE: &str = "mode";
const SWAP_LENGTH_WIDTH: &str = "swap_length_width";
const SWAP_FRONT_BACK: &str = "swap_front_back";
const ADAPT: &str = "adapt";
const ORIGIN: &str = "origin";

/// An edge this close to the cursor (pixels) is the one aimed at.
const EDGE_PX: f64 = 8.0;
/// The arrows' line width (points), OpenCutList's.
const ARROW_WIDTH: f32 = 2.0;
/// The picked edge's.
const EDGE_WIDTH: f32 = 4.0;
/// The axes helper's length and section (pixels).
const AXES_PX: (f64, f64) = (90.0, 3.0);
/// The arrows on the part as it is, OpenCutList's.
const ARROW: [f32; 4] = WHITE;
/// The quarter turn's arc: its radius, its distance to the face's sides, the radius under which
/// it no longer fits (pixels).
const TURN_RADIUS_PX: f64 = 24.0;
const TURN_INSET_PX: f64 = 10.0;
const TURN_MIN_PX: f64 = 8.0;
/// The half turn's ring: its radius (pixels), the opening left at the back of red, and where it
/// stands along the arrow (the arrow's tip).
const RING_RADIUS_PX: f64 = 20.0;
const RING_GAP: f64 = 80.0 * std::f64::consts::PI / 180.0;
const RING_ALONG: f64 = 0.9;
/// A turn's white outline.
const TURN_HALO_WIDTH: f32 = 4.5;
/// A turn seen edge on is opened up to this sine, to stay readable.
const TURN_MIN_OPENING: f64 = 0.35;

/// What is read of the target once, while it stays as it is.
#[derive(Clone, Debug)]
struct Target {
    /// The component's address and the document's next id: either changes with it.
    key: (usize, u64),
    /// Its box in its axes.
    bounds: Option<(DVec3, DVec3)>,
    /// How many times it is placed in the model.
    count: u64,
}

/// What the cursor aims at, and what a click would do.
#[derive(Clone, Debug)]
struct Aim {
    mode: &'static str,
    reversed: bool,
    /// The occurrence clicked, its component, and the component's axes to world.
    path: EntityPath,
    component: Id,
    to_world: DAffine3,
    /// From the new axes to the current ones.
    axes: DAffine3,
    target: Target,
    /// « Adapter »: the component's box in the new axes, the face's triangles and the edge red
    /// is taken from (world).
    adapted: Option<Adapted>,
    inference: Option<Inference>,
}

#[derive(Clone, Debug)]
struct Adapted {
    bounds: Option<(DVec3, DVec3)>,
    triangles: Vec<DVec3>,
    edge: (DVec3, DVec3),
}

#[derive(Default)]
pub struct ReorientTool {
    px: Option<DVec2>,
    aim: Option<Aim>,
    mode: Option<&'static str>,
}

fn mode_of(value: Option<&str>) -> &'static str {
    match value {
        Some(SWAP_FRONT_BACK) => SWAP_FRONT_BACK,
        Some(ADAPT) => ADAPT,
        Some(ORIGIN) => ORIGIN,
        _ => SWAP_LENGTH_WIDTH,
    }
}

/// The occurrence a click on `path` reaches: the part open, a part at any depth under « Parts »,
/// else the active context's occurrence.
fn reached(path: &EntityPath, cx: &Cx<'_>) -> Option<EntityPath> {
    if cx.open_part.is_some() {
        return cx.open_part_path().map(|p| p.part_path());
    }
    if cx.parts {
        return Some(path.part_path()).filter(|p| p.instances.starts_with(cx.context));
    }
    cx.selectable(path).map(|id| EntityPath::new(cx.context.to_vec(), id))
}

/// The box of `component` in the axes `axes` go from, on its display meshes (a curved face has
/// few vertices), the vertices of what is not meshed yet.
fn bounds_in(component: Id, axes: &DAffine3, cx: &Cx<'_>) -> Option<(DVec3, DVec3)> {
    let into = axes.inverse();
    let mut out: Option<(DVec3, DVec3)> = None;
    let mut add = |p: DVec3| out = Some(out.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
    for placed in cx.doc.subject_parts(&Subject::Component(component)) {
        let t = into * placed.transform;
        match cx.scene.definition_mesh(placed.part) {
            Some(dm) if dm.parts.iter().any(|p| !p.mesh.positions.is_empty()) => {
                for part in &dm.parts {
                    for [x, y, z] in &part.mesh.positions {
                        add(t.transform_point3(part.mesh.origin + DVec3::new(*x as f64, *y as f64, *z as f64)));
                    }
                }
            }
            _ => placed.shape.vertex_points().into_iter().for_each(|v| add(t.transform_point3(v))),
        }
    }
    out
}

/// What is known of `component`: `known`'s when it has not changed.
fn target_of(component: Id, known: Option<&Target>, cx: &Cx<'_>) -> Option<Target> {
    let c = cx.doc.component(component).ok()?;
    let key = (std::ptr::from_ref(c) as usize, cx.doc.next_id());
    if let Some(t) = known.filter(|t| t.key == key) {
        return Some(t.clone());
    }
    let exact = c.as_part().and_then(|p| cx.scene.bounds_of(p.shape()));
    let bounds = exact.or_else(|| bounds_in(component, &DAffine3::IDENTITY, cx));
    let count = cx.doc.counts_in_model().get(&component).copied().unwrap_or(1);
    Some(Target { key, bounds, count })
}

/// The axes on the face `hit` is on, in the component's: blue its normal, red the side nearest
/// the cursor, else its longest (the nearest to the current red on a tie), the face on green's
/// side.
fn adapted(face: &Face, px: DVec2, to_world: &DAffine3, reversed: bool, cx: &Cx<'_>) -> Option<(DMat3, (DVec3, DVec3))> {
    let near = |(a, b): &(DVec3, DVec3)| {
        let (p, q) = (cx.camera.project(*a, cx.size)?, cx.camera.project(*b, cx.size)?);
        let k = ((px - p).dot(q - p) / (q - p).length_squared().max(1e-9)).clamp(0.0, 1.0);
        Some(p.lerp(q, k).distance(px))
    };
    let closest = face
        .sides
        .iter()
        .filter_map(|s| near(s).filter(|d| *d < EDGE_PX).map(|d| (s, d)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| *s);
    let into = to_world.inverse();
    let direction = |(a, b): &(DVec3, DVec3)| into.transform_vector3(*b - *a).normalize_or_zero();
    let longest = || {
        let most = face.sides.iter().map(|(a, b)| a.distance(*b)).fold(0.0, f64::max);
        face.sides
            .iter()
            .filter(|(a, b)| (a.distance(*b) - most).abs() < 1e-4)
            .min_by(|p, q| direction(q).x.abs().total_cmp(&direction(p).x.abs()))
            .copied()
    };
    let edge = closest.or_else(longest)?;
    let z = into.transform_vector3(face.normal).normalize_or_zero();
    let mut x = direction(&edge);
    if x == DVec3::ZERO || z == DVec3::ZERO {
        return None;
    }
    // The face on the side of green.
    let centre = face.corners.iter().fold(DVec3::ZERO, |s, c| s + into.transform_point3(*c)) / face.corners.len().max(1) as f64;
    if (centre - into.transform_point3(edge.0)).dot(z.cross(x)) < 0.0 {
        x = -x;
    }
    if reversed {
        x = -x;
    }
    Some((DMat3::from_cols(x, z.cross(x), z), edge))
}

fn aim(px: DVec2, mode: &'static str, known: Option<&Aim>, cx: &Cx<'_>) -> Option<Aim> {
    let hit = cx.pick(px);
    // A corner on the part's outline is a point of it, though the ray may just miss its faces.
    let found = (mode == ORIGIN).then(|| cx.infer(px, |_| true, None)).flatten();
    let aimed = found.as_ref().and_then(|i| i.path.clone()).or_else(|| hit.as_ref().map(|h| h.path.clone()))?;
    let path = reached(&aimed, cx)?;
    let occurrence = cx.doc.resolve(&path).ok()?;
    let component = occurrence.component;
    let to_world = cx.doc.transform_of(&path.instances).ok()? * occurrence.transform;
    // The copy modifier is the tree's (« élément parent », docs/tools/selection.md).
    let reversed = cx.alt_down;
    let target = target_of(component, known.filter(|k| k.component == component).map(|k| &k.target), cx)?;
    let (mut face_axes, mut inference) = (None, None);
    let axes = match mode {
        SWAP_LENGTH_WIDTH if reversed => DAffine3::from_mat3(DMat3::from_cols(DVec3::Y, -DVec3::X, DVec3::Z)),
        SWAP_LENGTH_WIDTH => DAffine3::from_mat3(DMat3::from_cols(-DVec3::Y, DVec3::X, DVec3::Z)),
        SWAP_FRONT_BACK => DAffine3::from_mat3(DMat3::from_cols(DVec3::X, -DVec3::Y, -DVec3::Z)),
        ADAPT => {
            let face = Face::at(cx.scene, hit.as_ref()?)?;
            let (m, edge) = adapted(&face, px, &to_world, reversed, cx)?;
            let axes = DAffine3::from_mat3(m);
            // The same axes on the same target: their box is known.
            let same = known.filter(|k| k.component == component && k.target.key == target.key && k.axes == axes);
            let bounds = match same.and_then(|k| k.adapted.as_ref()) {
                Some(a) => a.bounds,
                None => bounds_in(component, &axes, cx),
            };
            face_axes = Some(Adapted { bounds, triangles: face.triangles().iter().flatten().copied().collect(), edge });
            axes
        }
        _ => {
            let found = found?;
            let at = to_world.inverse().transform_point3(found.point);
            inference = Some(found);
            DAffine3::from_translation(at)
        }
    };
    Some(Aim { mode, reversed, path, component, to_world, axes, target, adapted: face_axes, inference })
}

/// Does `m` leave the axes on one another, a quarter turn?
fn is_quarter_turn(m: &DMat3) -> bool {
    [m.x_axis, m.y_axis, m.z_axis].iter().all(|v| (v.abs().max_element() - 1.0).abs() < 1e-9)
}

fn axes_helper(d: &mut Drawer<'_>) {
    d.axes(DVec3::ZERO, AXES_PX.0, AXES_PX.1, [INK.into(), RED.into(), GREEN.into(), BLUE.into()]);
}

/// The part as it is (SmartTool's `_preview_part`): an arrow along its length on its back,
/// dashed, and on its front, and its box dotted.
fn draw_current(d: &mut Drawer<'_>, (lo, hi): (DVec3, DVec3)) {
    let (u, v) = (DVec3::X * (hi.x - lo.x), DVec3::Y * (hi.y - lo.y));
    d.pen(Pen::new(ARROW).width(ARROW_WIDTH).stipple(Stipple::ShortDashes)).arrow(lo, u, v);
    d.pen(Pen::new(ARROW).width(ARROW_WIDTH)).arrow(DVec3::new(lo.x, lo.y, hi.z), u, v);
    d.pen(Pen::new(INK).stipple(Stipple::Dotted)).box_(lo, hi);
}

/// What « Adapter » would give (`SmartAxesAdaptAxesActionHandler`): the front arrow filled, its
/// box dashed, in magenta.
fn draw_adapted(d: &mut Drawer<'_>, (lo, hi): (DVec3, DVec3)) {
    let (front, u, v) = (DVec3::new(lo.x, lo.y, hi.z), DVec3::X * (hi.x - lo.x), DVec3::Y * (hi.y - lo.y));
    d.brush(Brush::new(translucent(MAGENTA, 0.35)).over()).fill_arrow(front, u, v);
    d.pen(Pen::new(MAGENTA).width(ARROW_WIDTH).over()).arrow(front, u, v);
    d.pen(Pen::new(MAGENTA).stipple(Stipple::ShortDashes)).box_(lo, hi);
}

/// The rotation tilting the plane of `normal` (facing `eye`) just enough to be seen open.
fn opened(normal: DVec3, eye: DVec3) -> DQuat {
    let facing = normal.dot(eye);
    let side = (eye - normal * facing).normalize_or_zero();
    if facing >= TURN_MIN_OPENING || side == DVec3::ZERO {
        return DQuat::IDENTITY;
    }
    let s = TURN_MIN_OPENING;
    DQuat::from_rotation_arc(normal, normal * (1.0 - s * s).sqrt() + side * s)
}

/// The curved arrow of a turn, magenta on a white outline.
fn turn(d: &mut Drawer<'_>, center: DVec3, axis: DVec3, from: DVec3, angle: f64, ends: Ends) {
    for pen in [Pen::new(ARROW).width(TURN_HALO_WIDTH).over(), Pen::new(MAGENTA).width(ARROW_WIDTH).over()] {
        d.pen(pen).turn_arrow(center, axis, from, angle, ends);
    }
}

/// The turn a swap gives, in the component's axes, on the side of the box facing the eye. A
/// quarter turn: an arc in the corner red sweeps past, from the arrow's tip to the side it will
/// point at. A half turn has no direction: a ring with two heads about red, at the arrow's tip.
fn draw_turn(d: &mut Drawer<'_>, a: &Aim, (lo, hi): (DVec3, DVec3), cx: &Cx<'_>) {
    let eye = -a.to_world.inverse().transform_vector3(cx.camera.forward()).normalize_or_zero();
    let z = if eye.z >= 0.0 { hi.z } else { lo.z };
    let wpp_at = |p: DVec3| cx.camera.world_per_pixel(cx.size, a.to_world.transform_point3(p));
    match a.mode {
        SWAP_LENGTH_WIDTH => {
            let way = if a.reversed { 1.0 } else { -1.0 };
            let corner = DVec3::new(hi.x, if a.reversed { hi.y } else { lo.y }, z);
            let wpp = wpp_at(corner);
            let radius = TURN_RADIUS_PX.min((hi.x - lo.x).min(hi.y - lo.y) / wpp - TURN_INSET_PX);
            if radius < TURN_MIN_PX {
                return;
            }
            let off = (TURN_INSET_PX + radius) * wpp;
            let center = corner + DVec3::new(-off, -way * off, 0.0);
            let tilt = opened(if eye.z >= 0.0 { DVec3::Z } else { -DVec3::Z }, eye);
            turn(d, center, tilt * DVec3::Z, tilt * DVec3::X * radius * wpp, way * std::f64::consts::FRAC_PI_2, Ends::End);
        }
        SWAP_FRONT_BACK => {
            let center = DVec3::new(lo.x + RING_ALONG * (hi.x - lo.x), (lo.y + hi.y) / 2.0, z);
            let wpp = wpp_at(center);
            let tilt = opened(if eye.x >= 0.0 { DVec3::X } else { -DVec3::X }, eye);
            let axis = tilt * DVec3::X;
            // The opening at the back of red: the ring's two heads on either side of it.
            let back = (-eye).reject_from_normalized(axis).normalize_or(tilt * DVec3::Y);
            let from = DQuat::from_axis_angle(axis, RING_GAP / 2.0) * back * RING_RADIUS_PX * wpp;
            turn(d, center, axis, from, std::f64::consts::TAU - RING_GAP, Ends::Both);
        }
        _ => {}
    }
}

fn translucent(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

/// The tooltip (`_preview_action_tooltip`, `more_entities`): its name, material and size, and how
/// many occurrences the change reaches.
fn tip(a: &Aim, cx: &Cx<'_>) -> Option<String> {
    let c = cx.doc.component(a.component).ok()?;
    let mut lines = vec![c.name.clone()];
    if let Some(m) = cx.occurrence_material(&a.path).and_then(|m| cx.doc.material(m).ok()) {
        lines.push(m.name.clone());
    }
    if let (true, Some((lo, hi))) = (c.is_part(), a.target.bounds) {
        let (s, units) = (hi - lo, cx.units());
        lines.push(format!("{} × {} × {}{}", units.number(s.x), units.number(s.y), units.number(s.z), units.suffix()));
    }
    if a.target.count > 1 {
        lines.push(t!("tool.reorient.more_occurrences", count = a.target.count));
    }
    Some(lines.join("\n"))
}

impl ReorientTool {
    fn current(&self, cx: &Cx<'_>) -> Option<Aim> {
        let mode = mode_of(cx.choice(MODE));
        match &self.aim {
            Some(a) if a.mode == mode && a.reversed == cx.alt_down => Some(a.clone()),
            known => self.px.and_then(|px| aim(px, mode, known.as_ref(), cx)),
        }
    }
}

impl Tool for ReorientTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Reorient
    }

    fn cursor(&self, _cx: &Cx<'_>) -> Option<Cursor> {
        Some(Cursor::SelectPart)
    }

    fn started(&mut self, cx: &Cx<'_>) {
        self.mode = Some(mode_of(cx.choice(MODE)));
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        let mode = mode_of(cx.choice(MODE));
        self.mode = Some(mode);
        match input {
            ToolInput::Hover { px } => {
                self.px = Some(*px);
                self.aim = aim(*px, mode, self.aim.as_ref(), cx);
            }
            ToolInput::Click { px, .. } => {
                self.px = Some(*px);
                let Some(a) = aim(*px, mode, None, cx) else {
                    self.aim = None;
                    return match (mode, cx.pick(*px)) {
                        (ADAPT, Some(_)) => vec![Command::Say(t!("tool.reorient.not_flat"))],
                        _ => vec![],
                    };
                };
                self.aim = None;
                if a.axes.abs_diff_eq(DAffine3::IDENTITY, 1e-9) {
                    return vec![Command::Say(t!("tool.reorient.already"))];
                }
                let settings = cx.doc.component(a.component).is_ok_and(|c| c.behavior != Behavior::default());
                let mut commands = vec![Command::Apply(Operation::ChangeAxes { component: a.component, axes: a.axes })];
                if settings && !is_quarter_turn(&a.axes.matrix3) {
                    commands.push(Command::Say(t!("tool.reorient.settings_reset")));
                }
                return commands;
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let Some(a) = self.current(cx) else { return Preview::default() };
        let mut drawing = Drawing::default();
        // As it is. « Adapter » shows no current axes: they would stand among the new ones.
        {
            let d = &mut cx.drawer(&mut drawing).with_frame(a.to_world);
            if a.mode != ADAPT {
                axes_helper(d);
            }
            if let Some(b) = a.target.bounds {
                draw_current(d, b);
                draw_turn(d, &a, b, cx);
            }
        }
        // What a click would give.
        match (&a.adapted, a.mode) {
            (Some(adapted), _) => {
                {
                    let d = &mut cx.drawer(&mut drawing);
                    d.brush(Brush::new(translucent(MAGENTA, 0.2)).over()).fill_triangles(&adapted.triangles);
                    d.pen(Pen::new(MAGENTA).width(EDGE_WIDTH).over()).line(adapted.edge.0, adapted.edge.1);
                }
                let d = &mut cx.drawer(&mut drawing).with_frame(a.to_world * a.axes);
                if let Some(b) = adapted.bounds {
                    draw_adapted(d, b);
                }
                axes_helper(d);
            }
            (None, ORIGIN) => axes_helper(&mut cx.drawer(&mut drawing).with_frame(a.to_world * a.axes)),
            _ => {}
        }
        let hover = (!cx.parts && cx.open_part.is_none()).then_some(a.path.entity);
        let hover_solid = hover.is_none().then(|| a.path.clone());
        let tip = tip(&a, cx);
        Preview { hover, hover_solid, propagate: Some(a.path), inference: a.inference, tip, drawing, ..Default::default() }
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let choice = |value, label, tip| OptionChoice { value, label, tip, icon: None };
        vec![ToolOption {
            key: MODE,
            label: t!("tool.reorient.option.mode"),
            tip: t!("tool.reorient.option.mode_tip"),
            value: OptionValue::Choice(mode_of(cx.choice(MODE)).into()),
            choices: vec![
                choice(SWAP_LENGTH_WIDTH, t!("tool.reorient.option.swap_length_width"), t!("tool.reorient.option.swap_length_width_tip")),
                choice(SWAP_FRONT_BACK, t!("tool.reorient.option.swap_front_back"), t!("tool.reorient.option.swap_front_back_tip")),
                choice(ADAPT, t!("tool.reorient.option.adapt"), t!("tool.reorient.option.adapt_tip")),
                choice(ORIGIN, t!("tool.reorient.option.origin"), t!("tool.reorient.option.origin_tip")),
            ],
            saved: true,
            icon: None,
            swatch: None,
        }]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        if key == MODE
            && let OptionValue::Choice(v) = value
        {
            self.mode = Some(mode_of(Some(v)));
        }
    }

    fn status(&self) -> String {
        match self.mode.unwrap_or(SWAP_LENGTH_WIDTH) {
            SWAP_FRONT_BACK => t!("tool.reorient.hint.swap_front_back"),
            ADAPT => t!("tool.reorient.hint.adapt", alt_key = alt_key()),
            ORIGIN => t!("tool.reorient.hint.origin"),
            _ => t!("tool.reorient.hint.swap_length_width", alt_key = alt_key()),
        }
    }

    fn busy(&self) -> bool {
        false
    }

    fn picks_parts(&self) -> bool {
        true
    }

    fn leave(&mut self) {
        self.px = None;
        self.aim = None;
    }

    fn reset(&mut self) {
        self.px = None;
        self.aim = None;
    }
}

//! What the copy tools share (ARCHITECTURE.md §6 duodecies), after OpenCutList's Smart Handle
//! (`SmartHandleActionHandler`): picking what to copy, its box in the chosen axes, the measure,
//! the mirror, and drawing boxes and axes. The bounds are the Stretch's too.

use std::cell::RefCell;

use glam::{DAffine3, DMat3, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{Body, EntityPath, Id, Occurrence, Part};

use super::{BLUE, Cx, GREEN, Key, Moved, OptionChoice, OptionValue, PointShape, RED, ToolInput, ToolOption, copy_key};
use crate::draw::{Drawer, Ends, Pen};
use crate::vcb::Repeat;

/// Shift with `+` / `−` (OCL's SmartHandle): the count one more or one fewer, never below one,
/// `x` or `/` kept; `None` for any other input.
pub(super) fn count_step(input: &ToolInput) -> Option<i64> {
    match input {
        ToolInput::Key { key: Key::More, .. } => Some(1),
        ToolInput::Key { key: Key::Fewer, .. } => Some(-1),
        _ => None,
    }
}

/// `n` stepped by `by`, at least one.
pub(super) fn stepped(n: u32, by: i64) -> u32 {
    (n as i64 + by).clamp(1, u32::MAX as i64) as u32
}

/// `r` with its count stepped by `by`.
pub(super) fn repeat_stepped(r: Repeat, by: i64) -> Repeat {
    match r {
        Repeat::Times(n) => Repeat::Times(stepped(n, by)),
        Repeat::Divide(n) => Repeat::Divide(stepped(n, by)),
    }
}

/// The axes option (saved): « active » the model's, « context » the active context's (the
/// default, as OCL's handle preset), « entity » the one occurrence's or body's own.
pub(super) const AXES: &str = "axes";
pub(super) const ACTIVE: &str = "active";
pub(super) const CONTEXT: &str = "context";
pub(super) const ENTITY: &str = "entity";
/// What a distance measures (saved): « outside » edge to edge (the default), « centered »
/// centre to centre, « inside » between the copies.
pub(super) const MEASURE: &str = "measure";
pub(super) const OUTSIDE: &str = "outside";
pub(super) const CENTERED: &str = "centered";
pub(super) const INSIDE: &str = "inside";
/// The copy mirrored (saved; one copy only).
pub(super) const MIRROR: &str = "mirror";

/// How far past the box the axes are drawn (pixels).
const AXES_PX: f64 = 50.0;
/// Below this many copies their boxes are drawn.
const MAX_BOXES: usize = 100;
/// How far past the mirrored copy its badge is (pixels; OCL's 40), and its half size.
const BADGE_GAP_PX: f64 = 40.0;
const BADGE_PX: f32 = 11.0;

/// Does `m` only swap and turn over axes (each column one ±1)? Then a box stays a box, exact.
pub(super) fn square(m: &DMat3) -> bool {
    m.to_cols_array().iter().all(|x| x.abs() < 1e-9 || (x.abs() - 1.0).abs() < 1e-9)
}

/// The corners of the box `lo`–`hi`.
fn corners(lo: DVec3, hi: DVec3) -> [DVec3; 8] {
    std::array::from_fn(|i| DVec3::new([lo.x, hi.x][i & 1], [lo.y, hi.y][(i >> 1) & 1], [lo.z, hi.z][(i >> 2) & 1]))
}

/// A running box.
struct Bounds(DVec3, DVec3);

impl Bounds {
    fn new() -> Self {
        Bounds(DVec3::INFINITY, DVec3::NEG_INFINITY)
    }

    fn add(&mut self, p: DVec3) {
        self.0 = self.0.min(p);
        self.1 = self.1.max(p);
    }

    /// A display mesh's vertices placed by `t`.
    fn add_mesh(&mut self, m: &crate::scene::DefinitionMesh, t: DAffine3) {
        for pm in &m.parts {
            for v in &pm.mesh.positions {
                self.add(t.transform_point3(pm.mesh.origin + DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64)));
            }
        }
    }

    fn get(self) -> Option<(DVec3, DVec3)> {
        (self.0.x <= self.1.x).then_some((self.0, self.1))
    }
}

/// The bounds, in the axes `in_context` places in the context's, of what the occurrences
/// `occs` of the context `context` hold: a part's exact box when it lies square to the axes
/// (read from the document: there before its mesh is), else its display mesh's vertices (its
/// box's corners would swell it); an assembly's parts as the scene places them.
pub(crate) fn occurrences_bounds(cx: &Cx<'_>, context: &[Id], occs: &[&Occurrence], in_context: DAffine3) -> Option<(DVec3, DVec3)> {
    let to_world = cx.doc.transform_of(context).ok()?;
    let mut b = Bounds::new();
    let mut add_part = |def: Id, t: DAffine3| {
        let mesh = cx.scene.definition_mesh(def).filter(|m| !square(&t.matrix3) && m.parts.iter().any(|p| !p.mesh.positions.is_empty()));
        if let Some(m) = mesh {
            b.add_mesh(m, t);
        } else if let Some((lo, hi)) =
            cx.doc.part(def).ok().and_then(|p| cx.scene.bounds_of(p.shape()).or_else(|| p.shape().vertex_bounds()))
        {
            corners(lo, hi).into_iter().for_each(|c| b.add(t.transform_point3(c)));
        }
    };
    let to_target = in_context.inverse();
    let world_to_target = (to_world * in_context).inverse();
    for o in occs {
        if cx.doc.part(o.component).is_ok() {
            add_part(o.component, to_target * o.transform);
            continue;
        }
        let chain: Vec<Id> = context.iter().copied().chain([o.id]).collect();
        for (def, places) in cx.scene.placements() {
            for p in places.iter().filter(|p| p.path.starts_with(&chain)) {
                add_part(def, world_to_target * p.transform);
            }
        }
    }
    b.get()
}

/// The bounds, in the axes `in_part` places in the part's, of its bodies `bodies` (§2.11): a
/// body's exact box when square to the axes, else its mesh's vertices.
pub(crate) fn bodies_bounds(cx: &Cx<'_>, part: &Part, bodies: &[&Body], in_part: DAffine3) -> Option<(DVec3, DVec3)> {
    let to_target = in_part.inverse();
    let mut b = Bounds::new();
    for body in bodies {
        let t = to_target * body.placement;
        let key = cx.recipe.and_then(|r| r.body(body.id)).map(|r| r.key);
        let mesh = key.and_then(|k| cx.scene.definition_mesh(k)).filter(|_| !square(&t.matrix3));
        if let Some(m) = mesh {
            b.add_mesh(m, t);
        } else if let Some((lo, hi)) = cx.scene.bounds_of(part.solid_of(body)).or_else(|| part.solid_of(body).vertex_bounds()) {
            corners(lo, hi).into_iter().for_each(|c| b.add(t.transform_point3(c)));
        }
    }
    b.get()
}

/// The axes the copies are laid along (OCL's axes option).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Axes {
    /// The model's.
    Active,
    /// The active context's (in an open part: the part's).
    Context,
    /// The one occurrence's (or body's) own; the context's for several.
    Entity,
}

/// The axes option now (the context's by default, as OCL's).
pub(super) fn axes(cx: &Cx<'_>) -> Axes {
    match cx.choice(AXES) {
        Some(ACTIVE) => Axes::Active,
        Some(ENTITY) => Axes::Entity,
        _ => Axes::Context,
    }
}

/// What a distance measures (OCL's `copy_measure_type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Measure {
    /// From the far edge of the source to the far edge of the last copy.
    Outside,
    /// From centre to centre.
    Centered,
    /// The gap between the source and the first copy… to the last.
    Inside,
}

/// The measure option now, `default` when never set (OCL's presets: edge to edge for the
/// copies, between the parts for Distribute).
pub(super) fn measure(cx: &Cx<'_>, default: Measure) -> Measure {
    match cx.choice(MEASURE) {
        Some(OUTSIDE) => Measure::Outside,
        Some(CENTERED) => Measure::Centered,
        Some(INSIDE) => Measure::Inside,
        _ => default,
    }
}

/// What is copied — occurrences of the active context, or bodies of the open part, with the
/// guides selected with them —, in which frame the operation is, and its box in the axes chosen.
#[derive(Clone, Debug)]
pub(super) struct Target {
    /// The entities selected (guides too).
    pub entities: Vec<Id>,
    pub moved: Moved,
    /// The axes chosen, to world.
    pub frame: DAffine3,
    /// The box, in `frame`.
    pub lo: DVec3,
    pub hi: DVec3,
}

impl Target {
    /// The selection `ids`, in the axes `axes`; none without an occurrence (or a body) in it.
    pub fn of(cx: &Cx<'_>, ids: &[Id], axes: Axes) -> Option<Target> {
        let to_world = |in_context: DAffine3| (in_context, cx.context_transform * in_context);
        let (frame, lo, hi) = if cx.machinings_shown() {
            let (_, part, part_to_world) = cx.doc.resolve_container(&cx.open_part_path()?).ok()?;
            let bodies: Vec<&Body> = ids.iter().filter_map(|id| part.body(*id)).collect();
            let in_part = match (axes, &bodies[..]) {
                (_, []) => return None,
                (Axes::Entity, [b]) => b.placement,
                (Axes::Active, _) => part_to_world.inverse(),
                _ => DAffine3::IDENTITY,
            };
            let (lo, hi) = bodies_bounds(cx, &part, &bodies, in_part)?;
            (part_to_world * in_part, lo, hi)
        } else {
            if cx.open_part.is_some() {
                return None;
            }
            let a = cx.doc.assembly(cx.context_def).ok()?;
            let occs: Vec<&Occurrence> = ids.iter().filter_map(|id| a.occurrence(*id)).collect();
            let in_context = match (axes, &occs[..]) {
                (_, []) => return None,
                (Axes::Entity, [o]) => o.transform,
                (Axes::Active, _) => cx.context_transform.inverse(),
                _ => DAffine3::IDENTITY,
            };
            let (lo, hi) = occurrences_bounds(cx, cx.context, &occs, in_context)?;
            let (_, frame) = to_world(in_context);
            (frame, lo, hi)
        };
        Some(Target { entities: ids.to_vec(), moved: cx.moved(), frame, lo, hi })
    }

    /// The parts `paths` selected at any depth (« Parts », ARCHITECTURE.md §6 septendecies), in the
    /// axes `axes`: each from its own assembly, their box in those axes.
    pub fn of_deep(cx: &Cx<'_>, paths: &[EntityPath], axes: Axes) -> Option<Target> {
        if cx.open_part.is_some() || paths.is_empty() {
            return None;
        }
        let moved = cx.moved_deep(paths);
        let Moved::Deep { groups, .. } = &moved else { return None };
        let frame = match (axes, paths) {
            (Axes::Entity, [p]) => cx.doc.transform_of(&p.instances).ok()? * cx.doc.resolve(p).ok()?.transform,
            (Axes::Active, _) => DAffine3::IDENTITY,
            _ => cx.context_transform,
        };
        let mut b: Option<(DVec3, DVec3)> = None;
        for g in groups {
            let a = cx.doc.assembly(cx.doc.context_of(&g.parent).ok()?).ok()?;
            let occs: Vec<&Occurrence> = g.entities.iter().filter_map(|id| a.occurrence(*id)).collect();
            let in_parent = g.frame.inverse() * frame;
            let Some((lo, hi)) = occurrences_bounds(cx, &g.parent, &occs, in_parent) else { continue };
            b = Some(b.map_or((lo, hi), |(l, h)| (l.min(lo), h.max(hi))));
        }
        let (lo, hi) = b?;
        Some(Target { entities: Vec::new(), moved, frame, lo, hi })
    }

    pub fn center(&self) -> DVec3 {
        (self.lo + self.hi) * 0.5
    }

    pub fn half(&self) -> DVec3 {
        (self.hi - self.lo) * 0.5
    }

    /// The axis `k` of the frame, in the world (unit).
    pub fn dir(&self, k: usize) -> DVec3 {
        self.frame.matrix3.col(k).normalize()
    }

    /// A point of the frame, in the world.
    pub fn world(&self, p: DVec3) -> DVec3 {
        self.frame.transform_point3(p)
    }

    /// A world point, in the frame.
    pub fn local(&self, p: DVec3) -> DVec3 {
        self.frame.inverse().transform_point3(p)
    }

    /// The world motion moving by `v` in the frame.
    pub fn translation(&self, v: DVec3) -> DAffine3 {
        DAffine3::from_translation(self.frame.transform_vector3(v))
    }

    /// The operation: the copies at the world transforms `placements`, the sources moved by
    /// `moved` (world).
    pub fn operation(&self, moved: DAffine3, placements: &[DAffine3]) -> skraft_model::Operation {
        self.moved.copies(self.entities.clone(), moved, placements)
    }
}

/// How far from the box's centre its surface lies along the unit `u` (in its axes): the half
/// box seen along a line (OCL's `evs`, from the face the line leaves the box by).
pub(super) fn half_along(half: DVec3, u: DVec3) -> f64 {
    (0..3).filter(|k| u[*k].abs() > 1e-12).map(|k| half[k] / u[k].abs()).fold(f64::INFINITY, f64::min).min(f64::MAX)
}

/// The least lengths along a way for `f` copies of a box `h` deep along it (half), measured as
/// `measure` says (OCL's `dlmin`, `dmin`): where the cursor stops (from the box's centre), and
/// the least measure typed.
pub(super) fn minimums(h: f64, f: f64, measure: Measure) -> (f64, f64) {
    match measure {
        Measure::Outside => (h * (2.0 * f + 1.0), h * (2.0 * f + 2.0)),
        Measure::Centered => (h * 2.0 * f, h * 2.0 * f),
        Measure::Inside => (h * (2.0 * f - 1.0), h * (2.0 * f - 2.0)),
    }
}

/// The axis of the box (in its axes) the vector `v` from its centre leaves it by (OCL's test of
/// the bottom, left and front planes): its face's axis; X for no vector.
pub(super) fn leaving_axis(half: DVec3, v: DVec3) -> usize {
    let ratio = |k: usize| v[k].abs() / half[k].max(1e-9);
    (0..3).fold(0, |best, k| if ratio(k) > ratio(best) + 1e-12 { k } else { best })
}

/// The world reflection across the plane through `at` normal to the unit `n`.
pub(super) fn reflection(at: DVec3, n: DVec3) -> DAffine3 {
    let m = DMat3::IDENTITY - 2.0 * DMat3::from_cols(n * n.x, n * n.y, n * n.z);
    DAffine3::from_translation(at) * DAffine3::from_mat3(m) * DAffine3::from_translation(-at)
}

/// The mirror's badge (OCL's): past the point `at` (world) along the unit `dir`, `reach` (world)
/// then 40 pixels further, on `background`.
pub(super) fn mirror_badge(d: &mut Drawer<'_>, at: DVec3, dir: DVec3, reach: f64, background: [f32; 4]) {
    let gap = d.px(at, BADGE_GAP_PX);
    d.grip(at + dir * (reach + gap), BADGE_PX, Some(background), Some([1.0, 1.0, 1.0, 1.0]), PointShape::Mirror);
}

/// The axis `k`'s colour.
pub(super) fn axis_color(k: usize) -> [f32; 4] {
    [RED, GREEN, BLUE][k]
}

/// A measure line `a`–`b`, dashed, over everything (seen through what lies between); bold
/// while an axis is locked.
pub(super) fn draw_measure(cx: &Cx<'_>, d: &mut Drawer<'_>, a: DVec3, b: DVec3, color: [f32; 4]) {
    d.pen(Pen::new(color).over().dashed().bold_if(cx.axis_lock.is_some())).line(a, b);
}

/// The box `lo`–`hi` placed by `to_world`, dashed, in the scene, its front over it (OCL's
/// `shell_on_top`).
pub(super) fn draw_box(d: &mut Drawer<'_>, to_world: DAffine3, lo: DVec3, hi: DVec3, color: [f32; 4]) {
    d.in_frame(to_world, |d| {
        d.pen(Pen::new(color).dashed()).box_(lo, hi);
    });
}

/// The boxes of the copies at the world transforms `copies` (not too many), in `color`, and
/// the source's in ink.
pub(super) fn draw_boxes(d: &mut Drawer<'_>, target: &Target, copies: &[DAffine3], color: [f32; 4]) {
    draw_box(d, target.frame, target.lo, target.hi, super::INK);
    if copies.len() <= MAX_BOXES {
        for c in copies {
            draw_box(d, *c * target.frame, target.lo, target.hi, color);
        }
    }
}

/// The axes of the target through its centre, past its box, in the scene, with arrows at both
/// ends; but those `skip` says (OCL's `_preview_edit_axes`). Bold for a locked one, its heads
/// too.
pub(super) fn draw_axes(d: &mut Drawer<'_>, target: &Target, skip: impl Fn(usize) -> bool, locked: Option<usize>) {
    let c = target.world(target.center());
    let pad = d.px(c, AXES_PX);
    for k in (0..3).filter(|k| !skip(*k)) {
        let reach = target.half()[k] + pad;
        let (a, b) = (c - target.dir(k) * reach, c + target.dir(k) * reach);
        let color = axis_color(k);
        d.pen(Pen::new(color).bold_if(locked == Some(k))).edge(a, b, Ends::Both);
    }
}

/// The axis whose line through `start` (world) passes nearest the cursor on screen.
pub(super) fn nearest_axis_on_screen(cx: &Cx<'_>, target: &Target, start: DVec3, px: DVec2) -> usize {
    let len = cx.camera.world_per_pixel(cx.size, start) * 100.0;
    let Some(s) = cx.camera.project(start, cx.size) else { return 0 };
    let distance = |k: usize| {
        let Some(e) = cx.camera.project(start + target.dir(k) * len, cx.size) else { return f64::INFINITY };
        let d = e - s;
        if d.length() < 1e-9 {
            return f64::INFINITY;
        }
        let n = DVec2::new(-d.y, d.x).normalize();
        (px - s).dot(n).abs()
    };
    (0..3).fold(0, |best, k| if distance(k) < distance(best) { k } else { best })
}

/// The target worked out last, and what from (asked again at each input and preview, costly
/// for many occurrences: their bounds).
#[derive(Default)]
pub(super) struct Memo(RefCell<Option<(MemoKey, Option<Target>)>>);

#[derive(Clone, Debug, PartialEq)]
struct MemoKey {
    generation: u64,
    context: Vec<Id>,
    open_part: Option<Id>,
    shown: bool,
    selection: Vec<Id>,
    deep: Vec<EntityPath>,
    axes: Axes,
}

impl Memo {
    pub fn target(&self, cx: &Cx<'_>) -> Option<Target> {
        let key = MemoKey {
            generation: cx.scene.generation(),
            context: cx.context.to_vec(),
            open_part: cx.open_part,
            shown: cx.machinings_shown(),
            selection: cx.selection.to_vec(),
            deep: cx.deep.to_vec(),
            axes: axes(cx),
        };
        if let Some((k, t)) = &*self.0.borrow()
            && *k == key
        {
            return t.clone();
        }
        let t = if key.deep.is_empty() { Target::of(cx, &key.selection, key.axes) } else { Target::of_deep(cx, &key.deep, key.axes) };
        *self.0.borrow_mut() = Some((key, t.clone()));
        t
    }
}

impl Clone for Memo {
    fn clone(&self) -> Self {
        Memo(RefCell::new(self.0.borrow().clone()))
    }
}

/// The options the copy tools share: the measure (`measures`: those offered, the first the
/// default), the axes and, when `mirror`, the mirror.
pub(super) fn options(cx: &Cx<'_>, measures: &[Measure], mirror: bool) -> Vec<ToolOption> {
    let choice = |value: &'static str, label: String, tip: String, icon: Option<&'static str>| OptionChoice { value, label, tip, icon };
    let measure_choice = |m: Measure| match m {
        Measure::Outside => {
            choice(OUTSIDE, t!("tool.handle.option.outside"), t!("tool.handle.option.outside_tip"), Some("copy_measure_outside"))
        }
        Measure::Centered => {
            choice(CENTERED, t!("tool.handle.option.centered"), t!("tool.handle.option.centered_tip"), Some("copy_measure_centered"))
        }
        Measure::Inside => {
            choice(INSIDE, t!("tool.handle.option.inside"), t!("tool.handle.option.inside_tip"), Some("copy_measure_inside"))
        }
    };
    let measure_value = match measure(cx, measures[0]) {
        Measure::Outside => OUTSIDE,
        Measure::Centered => CENTERED,
        Measure::Inside => INSIDE,
    };
    let mut out = vec![
        ToolOption {
            key: MEASURE,
            label: t!("tool.handle.option.measure"),
            tip: t!("tool.handle.option.measure_tip"),
            value: OptionValue::Choice(measure_value.into()),
            choices: measures.iter().map(|m| measure_choice(*m)).collect(),
            saved: true,
            icon: None,
            swatch: None,
        },
        ToolOption {
            key: AXES,
            label: t!("tool.handle.option.axes"),
            tip: t!("tool.handle.option.axes_tip"),
            value: OptionValue::Choice(
                match axes(cx) {
                    Axes::Active => ACTIVE,
                    Axes::Context => CONTEXT,
                    Axes::Entity => ENTITY,
                }
                .into(),
            ),
            choices: vec![
                choice(ACTIVE, t!("tool.handle.option.active"), t!("tool.handle.option.active_tip"), Some("axes_active")),
                choice(CONTEXT, t!("tool.handle.option.context"), t!("tool.handle.option.context_tip"), Some("axes_context")),
                choice(ENTITY, t!("tool.handle.option.entity"), t!("tool.handle.option.entity_tip"), Some("axes_entity")),
            ],
            saved: true,
            icon: None,
            swatch: None,
        },
    ];
    if mirror {
        out.push(ToolOption {
            key: MIRROR,
            label: t!("tool.handle.option.mirror"),
            tip: t!("tool.handle.option.mirror_tip", copy_key = copy_key()),
            value: OptionValue::Toggle(cx.toggle(MIRROR)),
            choices: Vec::new(),
            saved: true,
            icon: Some("mirror"),
            swatch: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_half_box_along_a_line_and_the_face_it_leaves_by() {
        let half = DVec3::new(50.0, 20.0, 10.0);
        assert!((half_along(half, DVec3::X) - 50.0).abs() < 1e-12);
        assert!((half_along(half, -DVec3::Y) - 20.0).abs() < 1e-12);
        let u = DVec3::new(1.0, 1.0, 0.0).normalize();
        assert!((half_along(half, u) - 20.0 * 2f64.sqrt()).abs() < 1e-9);
        assert_eq!(leaving_axis(half, DVec3::new(100.0, 30.0, 0.0)), 0);
        assert_eq!(leaving_axis(half, DVec3::new(10.0, 30.0, 0.0)), 1);
        assert_eq!(leaving_axis(half, DVec3::new(10.0, 3.0, -20.0)), 2);
        assert_eq!(leaving_axis(half, DVec3::ZERO), 0);
    }

    #[test]
    fn a_reflection_mirrors_across_its_plane() {
        let r = reflection(DVec3::new(100.0, 0.0, 0.0), DVec3::X);
        assert!((r.transform_point3(DVec3::new(90.0, 5.0, 7.0)) - DVec3::new(110.0, 5.0, 7.0)).length() < 1e-12);
        assert!(r.matrix3.determinant() < 0.0);
        assert!(skraft_model_rigid(&r));
    }

    fn skraft_model_rigid(t: &DAffine3) -> bool {
        let m = t.matrix3;
        (m.transpose() * m).abs_diff_eq(DMat3::IDENTITY, 1e-12)
    }
}

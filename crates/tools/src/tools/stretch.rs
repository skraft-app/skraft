//! Stretch (ARCHITECTURE.md §6 sexies), OpenCutList's Smart Reshape stretch
//! (`SmartReshapeStretchActionHandler`) as faithfully as the app allows.
//!
//! - **Pick** (OCL's `SmartSelectActionHandler`): what to stretch — the open part, the
//!   occurrences (of parts or assemblies) selected in the active context, else the one clicked,
//!   as the selection takes it. Several are taken with Shift held — each click adds or removes
//!   one, the set stretched once Shift is released — or by a rectangle (left to right: what it
//!   encloses, right to left: what it touches).
//! - **Axes** (OCL's option): the occurrence's own (entity; the context's for several), the
//!   context's, or the model's (active). Several share one box in them.
//! - **Grips and cutters** (OCL's `STATE_STRETCH_START`): a grip at the centre of each face of
//!   the part's box, a point drawn over everything and taken near it on screen, seen or not
//!   (30 px for the active axis's, 10 for the others'). Hovering a grip makes its axis the
//!   active one (the arrow keys lock it); the active axis is drawn dashed between its grips, in
//!   its colour, the others in grey. The active axis's cutters are translucent planes across the
//!   box, 50 px wider than it, drawn in the scene (hidden behind faces): pressed and dragged
//!   along the axis, out of the box to remove one; with the copy modifier held a click adds one
//!   where the cursor is, with the other modifier held a click removes the one under it. A click on a grip starts the stretch (or press, drag
//!   and release); a click on nothing lets the part go.
//! - **Interior handles** (OCL's `_get_interior_handle_defs`): with two cutters or more on the
//!   active axis, each section between two of them has one — its stretch of the line between the
//!   grips, taken anywhere along it (20 px), drawn bold with arrows when hovered. It is grabbed
//!   where an inferred point that tells a place along the axis is (a diamond), else at its point
//!   nearest the cursor's ray; Shift held keeps it and takes its start anywhere, projected on
//!   its line. Moved, that section goes by the move, the others by a share falling to 0 at
//!   both ends: the overall length stays, so the measure is the offset and « centred » is moot.
//! - **Stretch** (OCL's `STATE_STRETCH`): the part drawn stretched, alone, with the measure line
//!   and its length; the cursor near the start point snaps back to it, else an inferred point
//!   that tells a place along the axis (a point, an edge across it, a face facing it), else the
//!   closest point of the axis to the cursor's ray. Shift holds the last measure, the copy
//!   modifier tapped centres, the other one tapped makes it a separate part; a double-click takes
//!   the last measure again; a typed one ends it. Then back to the grips, on the same part.
//!
//! Cutters are kept per part and axis, as ratios of the box, for the session (OCL keeps them on
//! the definition, saved in the model); several occurrences share one set, the last used.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::kernel::Solid;
use solidsketch_model::{EntityPath, Id, Operation, Scope};

use super::select::{marquee_hits, toggled};
use super::{
    BLUE, Command, Cx, GREEN, Key, Marquee, OptionChoice, OptionValue, Preview, PreviewFill, PreviewLabel, PreviewLine, PreviewPoint, RED,
    StretchPreview, Tool, ToolInput, ToolKind, ToolOption, alt_key, copy_key, dashed,
};
use crate::camera::Ray;
use crate::infer::{EdgeRef, InferKind, Inference};
use crate::vcb::{self, Vcb, VcbKind, VcbValue};

/// Both ends move, as far each (saved).
const CENTERED: &str = "centered";
/// What the measure is (saved): the overall dimension, or the offset of the face pulled.
const MEASURE: &str = "measure";
const OVERALL: &str = "overall";
const OFFSET: &str = "offset";
/// What the stretch changes (saved, without an open part: the open part has its own).
const SCOPE: &str = "scope";
const ALL: &str = "all";
const THIS: &str = "this";
/// The axes stretched along (saved, OCL's): the model's, the context's, the occurrence's own.
const AXES: &str = "axes";
const ACTIVE: &str = "active";
const CONTEXT: &str = "context";
const ENTITY: &str = "entity";

/// How near a grip the cursor takes it (pixels): the active axis's, the others'.
const GRIP_PX: f64 = 30.0;
const OTHER_GRIP_PX: f64 = 10.0;
/// How near a cutter's outline the cursor takes it (pixels).
const CUTTER_PX: f64 = 20.0;
/// How near an interior handle the cursor takes it (pixels).
const HANDLE_PX: f64 = 20.0;
/// An interior handle's arrowheads (pixels).
const ARROW_PX: f64 = 10.0;
/// How far the cutters stand out of the box (pixels).
const INFLATE_PX: f64 = 50.0;
/// How near the start point the cursor snaps back to it (pixels).
const START_PX: f64 = 40.0;
/// A drag shorter than this (pixels, both ways) is a click.
const MIN_MARQUEE: f64 = 3.0;
/// What a compression leaves between the matter of two sections (mm), as OpenCutList does.
const KEEP: f64 = 1.0;
/// Shorter than this (mm), nothing.
const MIN: f64 = 1e-3;
/// Closer than this, two cutters are one; at or past it from an end, a cutter is removed (ratio).
const SAME_CUT: f64 = 1e-3;

/// The grey of the inactive axes and of a cutter about to go.
const GREY: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

thread_local! {
    /// The cutters of each part, per axis, for the session; under `None`, those several
    /// occurrences share (OCL's `@@last_cutters_data`). The tools run on one thread; each test
    /// on its own.
    static CUTTERS: RefCell<HashMap<Option<Id>, [Vec<f64>; 3]>> = RefCell::default();
}

fn load_cutters(key: Option<Id>) -> [Vec<f64>; 3] {
    CUTTERS.with_borrow(|c| c.get(&key).cloned()).unwrap_or_else(|| [vec![0.5], vec![0.5], vec![0.5]])
}

fn store_cutters(key: Option<Id>, cutters: &[Vec<f64>; 3]) {
    CUTTERS.with_borrow_mut(|c| c.insert(key, cutters.clone()));
}

/// The axes a stretch is along (OCL's axes option).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axes {
    /// The model's: the root's.
    Active,
    /// The active context's.
    Context,
    /// The occurrence's own; the context's for several.
    Entity,
}

/// The axes option now (the occurrence's own by default, as OCL's stretch).
fn axes(cx: &Cx<'_>) -> Axes {
    match cx.choice(AXES) {
        Some(ACTIVE) => Axes::Active,
        Some(CONTEXT) => Axes::Context,
        _ => Axes::Entity,
    }
}

/// Does `m` only swap and turn over axes (each column one ±1)? Then a box stays a box, exact.
fn square(m: &glam::DMat3) -> bool {
    m.to_cols_array().iter().all(|x| x.abs() < 1e-9 || (x.abs() - 1.0).abs() < 1e-9)
}

/// Ratios sorted, inside ]0, 1[ and apart; none left, one in the middle (OCL's `_load_cutters`).
fn sanitized(mut ratios: Vec<f64>) -> Vec<f64> {
    ratios.retain(|r| (SAME_CUT..=1.0 - SAME_CUT).contains(r));
    ratios.sort_by(f64::total_cmp);
    ratios.dedup_by(|a, b| (*a - *b).abs() < SAME_CUT);
    if ratios.is_empty() { vec![0.5] } else { ratios }
}

/// What is stretched — occurrences of parts or assemblies of one context: the context's path,
/// the occurrences, the axes stretched along and the bounds, in them, of what they hold (exact
/// for what lies square to them, else from the display meshes).
#[derive(Clone, Debug)]
struct Target {
    context: Vec<Id>,
    occurrences: Vec<Id>,
    /// The one occurrence's component (its cutters are its own); none for several.
    component: Option<Id>,
    /// The stretch's axes in the context's (the operation's frame), and to world.
    in_context: DAffine3,
    frame: DAffine3,
    lo: DVec3,
    hi: DVec3,
}

impl Target {
    fn of(cx: &Cx<'_>, context: Vec<Id>, occurrences: Vec<Id>, axes: Axes) -> Option<Target> {
        let a = cx.doc.assembly(cx.doc.context_of(&context).ok()?).ok()?;
        let occs = occurrences.iter().map(|id| a.occurrence(*id)).collect::<Option<Vec<_>>>()?;
        let to_world = cx.doc.transform_of(&context).ok()?;
        let in_context = match (axes, &occs[..]) {
            (_, []) => return None,
            (Axes::Entity, [o]) => o.transform,
            (Axes::Active, _) => to_world.inverse(),
            _ => DAffine3::IDENTITY,
        };
        let component = match &occs[..] {
            [o] => Some(o.component),
            _ => None,
        };
        let frame = to_world * in_context;
        let exact = |shape: &Arc<Solid>| cx.scene.bounds_of(shape).or_else(|| shape.vertex_bounds());
        let mut b = (DVec3::INFINITY, DVec3::NEG_INFINITY);
        // A part placed by `t` in the axes: its exact box when square to them, else (its box
        // would swell) its display mesh's vertices.
        let mut add_part = |def: Id, t: DAffine3| {
            let mut add = |w: DVec3| b = (b.0.min(w), b.1.max(w));
            let mesh =
                cx.scene.definition_mesh(def).filter(|m| !square(&t.matrix3) && m.parts.iter().any(|p| !p.mesh.positions.is_empty()));
            if let Some(m) = mesh {
                for pm in &m.parts {
                    for v in &pm.mesh.positions {
                        add(t.transform_point3(pm.mesh.origin + DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64)));
                    }
                }
            } else if let Some((lo, hi)) = cx.doc.part(def).ok().and_then(|p| exact(p.shape())) {
                for i in 0..8 {
                    add(t.transform_point3(DVec3::new([lo.x, hi.x][i & 1], [lo.y, hi.y][(i >> 1) & 1], [lo.z, hi.z][(i >> 2) & 1])));
                }
            }
        };
        let to_target = in_context.inverse();
        for o in &occs {
            if cx.doc.part(o.component).is_ok() {
                // From the document: there before its mesh is.
                add_part(o.component, to_target * o.transform);
                continue;
            }
            // What an assembly holds, as the scene places it.
            let chain: Vec<Id> = context.iter().copied().chain([o.id]).collect();
            let to_target = frame.inverse();
            for (def, places) in cx.scene.placements() {
                for p in places.iter().filter(|p| p.path.starts_with(&chain)) {
                    add_part(def, to_target * p.transform);
                }
            }
        }
        let (lo, hi) = (b.0.x <= b.1.x).then_some(b)?;
        Some(Target { context, occurrences, component, in_context, frame, lo, hi })
    }

    /// The paths of the occurrences stretched.
    fn paths(&self) -> Vec<EntityPath> {
        self.occurrences.iter().map(|o| EntityPath::new(self.context.clone(), *o)).collect()
    }

    /// The one occurrence's path, if only one.
    fn single(&self) -> Option<EntityPath> {
        match self.occurrences[..] {
            [o] => Some(EntityPath::new(self.context.clone(), o)),
            _ => None,
        }
    }

    fn length(&self, k: usize) -> f64 {
        self.hi[k] - self.lo[k]
    }

    /// The axis `k` of the part, in the world (unit).
    fn dir(&self, k: usize) -> DVec3 {
        self.frame.matrix3.col(k).normalize()
    }

    /// The ray in the part's axes.
    fn ray(&self, cx: &Cx<'_>, px: DVec2) -> Ray {
        let r = cx.ray(px);
        let inv = self.frame.inverse();
        Ray { origin: inv.transform_point3(r.origin), dir: inv.transform_vector3(r.dir).normalize(), t_min: r.t_min }
    }

    /// The centre of the face of axis `k` on `side` (part's axes).
    fn grip(&self, k: usize, side: f64) -> DVec3 {
        let mut c = (self.lo + self.hi) / 2.0;
        c[k] = if side > 0.0 { self.hi[k] } else { self.lo[k] };
        c
    }

    /// Where the ratio `r` of axis `k` is along it.
    fn at(&self, k: usize, r: f64) -> f64 {
        self.lo[k] + r * self.length(k)
    }

    fn world(&self, p: DVec3) -> DVec3 {
        self.frame.transform_point3(p)
    }

    /// The cutter of axis `k` at `at`, a rectangle across the box standing `inflate` out of it
    /// (world corners).
    fn plane(&self, k: usize, at: f64, inflate: f64) -> [DVec3; 4] {
        let (u, v) = ((k + 1) % 3, (k + 2) % 3);
        let (lo, hi) = (self.lo - DVec3::splat(inflate), self.hi + DVec3::splat(inflate));
        [(lo[u], lo[v]), (hi[u], lo[v]), (hi[u], hi[v]), (lo[u], hi[v])].map(|(a, b)| {
            let mut p = DVec3::ZERO;
            (p[k], p[u], p[v]) = (at, a, b);
            self.world(p)
        })
    }

    /// The box's stretchable axes: those it has a length along.
    fn axes(&self) -> impl Iterator<Item = usize> + '_ {
        (0..3).filter(|k| self.length(*k) > MIN)
    }
}

/// The share of the move of each of `n` sections, the grip on `side` (OpenCutList's profile):
/// from 0 at the anchored end to 1 at the grip, proportional to the section's rank; centred,
/// from −1 to 1. Times the move and the side, each section's displacement along the axis.
fn shares(n: usize, side: f64, centered: bool) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let t = if n == 1 {
                1.0
            } else if side > 0.0 {
                i as f64 / (n - 1) as f64
            } else {
                (n - 1 - i) as f64 / (n - 1) as f64
            };
            side * if centered { 2.0 * t - 1.0 } else { t }
        })
        .collect()
}

/// The share of the move of each of `n` sections, the interior section `index` (in 1..n−1)
/// moved (OCL's `interior_t_coefs`): from 0 at the first end to 1 at that section, back to 0 at
/// the other end, each gap taking an equal part on its side; the overall length stays.
fn interior_shares(n: usize, index: usize) -> Vec<f64> {
    let gaps = n.saturating_sub(1) as f64;
    let i = index as f64;
    (0..n).map(|j| j as f64).map(|j| if j <= i { j / i } else { (gaps - j) / (gaps - i) }).collect()
}

/// The range of the move `d` that keeps [`KEEP`] between the matter of the sections a
/// compression brings together (`spans`, per section, as [`Stretch::section_spans`]).
fn move_range(spans: &[Option<(f64, f64)>], shares: &[f64]) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    let filled: Vec<(usize, (f64, f64))> = spans.iter().enumerate().filter_map(|(i, s)| s.map(|s| (i, s))).collect();
    for w in filled.windows(2) {
        let ((a, (_, a_hi)), (b, (b_lo, _))) = (w[0], w[1]);
        let room = (b_lo - a_hi - KEEP).max(0.0);
        // The gap closes by (share_a − share_b)·d.
        let closing = shares[a] - shares[b];
        if closing > 0.0 {
            hi = hi.min(room / closing);
        } else if closing < 0.0 {
            lo = lo.max(room / closing);
        }
    }
    (lo, hi)
}

/// The cutters after a stretch (OCL's `StretchDef#cutter_ratios`), as ratios of the box it
/// leaves (`lo..hi` along the axis): one in the middle of each gap between the matter of two
/// sections that hold some (`spans`, before the stretch), once moved (`moves`) — so that each
/// stays between the same vertices, however hard the compression. A run of empty sections is
/// one gap (one cutter); matter that overlaps across a cutter leaves none there.
fn kept_cutters(spans: &[Option<(f64, f64)>], moves: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    let filled: Vec<(usize, (f64, f64))> = spans.iter().enumerate().filter_map(|(i, s)| s.map(|s| (i, s))).collect();
    filled
        .windows(2)
        .filter_map(|w| {
            let ((a, (_, a_hi)), (b, (b_lo, _))) = (w[0], w[1]);
            let (end, start) = (a_hi + moves[a], b_lo + moves[b]);
            (start > end + MIN && hi > lo).then(|| ((end + start) / 2.0 - lo) / (hi - lo))
        })
        .collect()
}

/// The distance from `p` to the segment `a`–`b` (pixels).
fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// Is `p` inside the convex polygon `poly` (either winding; not when it is flat)?
fn inside(p: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    let signs: Vec<f64> = (0..n).map(|i| (poly[(i + 1) % n] - poly[i]).perp_dot(p - poly[i])).collect();
    signs.iter().all(|s| *s > 0.0) || signs.iter().all(|s| *s < 0.0)
}

fn axis_color(k: usize) -> [f32; 4] {
    [RED, GREEN, BLUE][k]
}

fn translucent(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

/// The scope a stretch takes now: the open part's, else the tool's.
fn scope(cx: &Cx<'_>) -> Scope {
    if cx.open_part.is_some() {
        cx.part_scope
    } else if cx.choice(SCOPE) == Some(THIS) {
        Scope::This
    } else {
        Scope::All
    }
}

/// Does the inferred point tell a place along `dir` (OCL's `_stretch_ip_stable_along?`): a
/// point, an edge across it, a face facing it? Elsewhere it slides along the axis.
fn stable_along(i: &Inference, dir: DVec3) -> bool {
    if i.kind.is_point() {
        return true;
    }
    match i.kind {
        InferKind::OnEdge => matches!(i.edge, Some(EdgeRef::Straight(e)) if e.dot(dir).abs() < 1e-6),
        InferKind::OnFace => i.hit.as_ref().is_some_and(|h| h.normal.cross(dir).length() < 1e-6),
        _ => false,
    }
}

/// What the cursor is over, the grips shown.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Over {
    Nothing,
    /// The grip of axis `k` on `side`.
    Grip {
        k: usize,
        side: f64,
    },
    /// An interior handle of the active axis.
    Handle(Handle),
    /// The cutter of this index, on the active axis.
    Cutter(usize),
}

/// An interior handle taken (OCL's `_get_interior_handle_defs`): the section `index` (in
/// 1..n−1, between the cutters `index − 1` and `index`), grabbed `at` (the part's axes, on the
/// line between the axis's grips); `magnetized`: from an inferred point that tells a place along
/// the axis; `from`: that point when away from the line (the handle held by Shift).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Handle {
    index: usize,
    at: DVec3,
    magnetized: bool,
    from: Option<DVec3>,
}

/// What a click does among the grips, by the modifiers held (OCL's cutter states).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    /// The copy modifier held: a click adds a cutter.
    Add,
    /// The other modifier held: a click removes one.
    Remove,
}

#[derive(Clone, Debug)]
enum State {
    /// The grips and cutters of the target, if any (none: a click picks a part).
    Grips,
    /// The cutter `index` of axis `k` dragged, from `from` (the part's axes, on its plane).
    Cutter { k: usize, index: usize, from: DVec3 },
    /// The grip on `side` of axis `k` pulled by `d` (outward), within `range`; `drag`: started by
    /// a drag, ended by its release. Or the interior handle `handle` moved by `d` along the
    /// axis (`side` 1).
    /// `spans`: the matter wholly in each section, along the axis, before the stretch.
    Stretch {
        target: Box<Target>,
        k: usize,
        side: f64,
        handle: Option<Handle>,
        d: f64,
        range: (f64, f64),
        spans: Vec<Option<(f64, f64)>>,
        drag: bool,
    },
}

pub struct StretchTool {
    state: State,
    /// The occurrence clicked, when nothing is open nor selected.
    picked: Option<EntityPath>,
    /// The occurrence under the cursor, when none is picked.
    hovered: Option<EntityPath>,
    /// Shift held after a Shift click or rectangle (OCL's `STATE_SELECT_MULTIPLE`): the
    /// selection is being made, taken once Shift is released.
    gathering: bool,
    /// The selection rectangle dragged, when nothing is taken yet.
    marquee: Option<Marquee>,
    /// The active axis (0 red, 1 green, 2 blue): the last grip hovered's.
    axis: Option<usize>,
    /// The cutters of `cutters_of` (a component, or none: several occurrences'), per axis, as
    /// increasing ratios of its box in ]0, 1[.
    cutters: [Vec<f64>; 3],
    cutters_of: Option<Option<Id>>,
    over: Over,
    mode: Mode,
    /// The ratio a click would add a cutter at ([`Mode::Add`]).
    adding: Option<f64>,
    /// A part was there to stretch at the last input (for the hints).
    aimed: bool,
    snap: Option<Inference>,
    /// The last measures applied, per kind (a double-click or Shift takes it again): an overall
    /// dimension, a signed offset.
    last_overall: Option<f64>,
    last_offset: Option<f64>,
    /// Shift was down when the stretch started: it holds the last measure only once released
    /// and pressed again.
    shift_ignored: bool,
    /// The saved options, as the editor last gave them.
    centered: bool,
    overall: bool,
}

impl Default for StretchTool {
    fn default() -> Self {
        StretchTool {
            state: State::Grips,
            picked: None,
            hovered: None,
            gathering: false,
            marquee: None,
            axis: None,
            cutters: [vec![0.5], vec![0.5], vec![0.5]],
            cutters_of: None,
            over: Over::Nothing,
            mode: Mode::Normal,
            adding: None,
            aimed: false,
            snap: None,
            last_overall: None,
            last_offset: None,
            shift_ignored: false,
            centered: false,
            overall: true,
        }
    }
}

impl StretchTool {
    /// What the tool works on: the one stretched, the open part, the occurrences selected, the
    /// one picked.
    fn target(&self, cx: &Cx<'_>) -> Option<Target> {
        if let State::Stretch { target, .. } = &self.state {
            return Some((**target).clone());
        }
        let axes = axes(cx);
        if let Some(id) = cx.open_part {
            return Target::of(cx, cx.context.to_vec(), vec![id], axes);
        }
        let selected = self.selected(cx);
        if !selected.is_empty() {
            return Target::of(cx, cx.context.to_vec(), selected, axes);
        }
        let p = self.picked.as_ref()?;
        Target::of(cx, p.instances.clone(), vec![p.entity], axes)
    }

    /// The occurrences (of parts or assemblies) selected in the context — none while the
    /// selection is being made.
    fn selected(&self, cx: &Cx<'_>) -> Vec<Id> {
        if self.gathering {
            return Vec::new();
        }
        cx.selection.iter().copied().filter(|id| cx.doc.resolve(&EntityPath::new(cx.context.to_vec(), *id)).is_ok()).collect()
    }

    /// The occurrences of the context `ids` names (not its guides).
    fn occurrences(cx: &Cx<'_>, ids: Vec<Id>) -> Vec<Id> {
        ids.into_iter().filter(|id| cx.doc.resolve(&EntityPath::new(cx.context.to_vec(), *id)).is_ok()).collect()
    }

    /// The cutters of `target`'s component (or several occurrences'), loaded when it changes.
    fn sync_cutters(&mut self, target: &Target) {
        if self.cutters_of != Some(target.component) {
            self.cutters = load_cutters(target.component);
            self.cutters_of = Some(target.component);
        }
    }

    fn set_cutters(&mut self, k: usize, ratios: Vec<f64>) {
        self.cutters[k] = sanitized(ratios);
        if let Some(key) = self.cutters_of {
            store_cutters(key, &self.cutters);
        }
    }

    /// The active axis: the arrow keys', else the last grip hovered's, if stretchable.
    fn active(&self, cx: &Cx<'_>, target: &Target) -> Option<usize> {
        cx.axis_lock.or(self.axis).filter(|k| target.length(*k) > MIN)
    }

    /// The grip under the cursor: the nearest on screen within its reach, the nearer the eye
    /// first when two coincide (a face's and its opposite's, seen along the axis).
    fn grip_at(&self, cx: &Cx<'_>, target: &Target, px: DVec2) -> Option<(usize, f64)> {
        let active = self.active(cx, target);
        let depth = |p: DVec3| (p - cx.camera.eye).dot(cx.camera.forward());
        target
            .axes()
            .filter(|k| cx.axis_lock.is_none_or(|l| l == *k))
            .flat_map(|k| [(k, -1.0), (k, 1.0)])
            .filter_map(|(k, side)| {
                let p = target.world(target.grip(k, side));
                let d = cx.camera.project(p, cx.size)?.distance(px);
                let reach = if active == Some(k) { GRIP_PX } else { OTHER_GRIP_PX };
                (d <= reach).then_some(((k, side), (d * 4.0).round(), depth(p)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
            .map(|(g, _, _)| g)
    }

    /// [`INFLATE_PX`] in model length at the box's centre.
    fn inflate(cx: &Cx<'_>, target: &Target) -> f64 {
        cx.camera.world_per_pixel(cx.size, target.world((target.lo + target.hi) / 2.0)) * INFLATE_PX
    }

    /// The cutter of axis `k` under the cursor: inside its plane on screen, or near its outline.
    fn cutter_at(&self, cx: &Cx<'_>, target: &Target, k: usize, px: DVec2) -> Option<usize> {
        let inflate = Self::inflate(cx, target);
        self.cutters[k].iter().position(|r| {
            let corners = target.plane(k, target.at(k, *r), inflate);
            let Some(c) = corners.iter().map(|w| cx.camera.project(*w, cx.size)).collect::<Option<Vec<_>>>() else { return false };
            inside(px, &c) || (0..4).any(|i| segment_distance(px, c[i], c[(i + 1) % 4]) <= CUTTER_PX)
        })
    }

    /// The ratio along `k` a new cutter takes at the cursor (OCL's `_snap_stretch_cutter_add`):
    /// on the plane through the box's centre along the axis and its larger other side.
    fn ratio_to_add(cx: &Cx<'_>, target: &Target, k: usize, px: DVec2) -> f64 {
        let (u, v) = ((k + 1) % 3, (k + 2) % 3);
        let normal = if target.length(u) > target.length(v) { DVec3::AXES[v] } else { DVec3::AXES[u] };
        let c = (target.lo + target.hi) / 2.0;
        let ray = target.ray(cx, px);
        let denom = ray.dir.dot(normal);
        let p = if denom.abs() > 1e-12 { ray.at((c - ray.origin).dot(normal) / denom) } else { ray.origin };
        ((p[k] - target.lo[k]) / target.length(k)).clamp(0.0, 1.0)
    }

    /// The interior handles of axis `k` (OCL's `_get_interior_handle_defs`): per section between
    /// two cutters, its index and its segment on the line between the grips (the part's axes).
    fn handles(&self, target: &Target, k: usize) -> Vec<(usize, DVec3, DVec3)> {
        let point = |r: f64| {
            let mut c = (target.lo + target.hi) / 2.0;
            c[k] = target.at(k, r);
            c
        };
        self.cutters[k].windows(2).enumerate().map(|(i, w)| (i + 1, point(w[0]), point(w[1]))).collect()
    }

    /// The interior handle of axis `k` under the cursor, grabbed where an inferred point that
    /// tells a place along the axis is (projected on its segment), else at the segment's point
    /// nearest the cursor's ray (OCL's `_get_segment_grab_point`).
    fn handle_at(&self, cx: &Cx<'_>, target: &Target, k: usize, px: DVec2) -> Option<Handle> {
        let screen = |p: DVec3| cx.camera.project(target.world(p), cx.size);
        let (index, a, b) = self.handles(target, k).into_iter().find(|(_, a, b)| {
            let (Some(sa), Some(sb)) = (screen(*a), screen(*b)) else { return false };
            segment_distance(px, sa, sb) <= HANDLE_PX
        })?;
        let clamp = |mut p: DVec3| {
            let mut on = a;
            on[k] = p[k].clamp(a[k], b[k]);
            p = on;
            p
        };
        let inv = target.frame.inverse();
        let inferred = cx.infer(px, |_| true, None).filter(|i| stable_along(i, target.dir(k)));
        Some(match inferred {
            Some(i) => Handle { index, at: clamp(inv.transform_point3(i.point)), magnetized: true, from: None },
            None => Handle { index, at: clamp(target.ray(cx, px).closest_on_line(a, DVec3::AXES[k])), magnetized: false, from: None },
        })
    }

    /// The handle held by Shift (OCL's `_snap_stretch_start_locked_interior_handle`): its start
    /// taken anywhere, an inferred point that tells a place along the axis projected on its
    /// line (and shown), else the line's point nearest the cursor's ray.
    fn hold_handle(&mut self, cx: &Cx<'_>, target: &Target, k: usize, mut h: Handle, px: DVec2) {
        let inv = target.frame.inverse();
        let on_line = |p: DVec3| {
            let mut q = h.at;
            q[k] = p[k];
            q
        };
        self.snap = cx.infer(px, |_| true, None).filter(|i| stable_along(i, target.dir(k)));
        match &self.snap {
            Some(i) => {
                let p = inv.transform_point3(i.point);
                (h.at, h.magnetized, h.from) = (on_line(p), true, Some(p));
            }
            None => (h.at, h.magnetized, h.from) = (on_line(target.ray(cx, px).closest_on_line(h.at, DVec3::AXES[k])), false, None),
        }
        self.over = Over::Handle(h);
    }

    /// The grips' hover: a grip, else an interior handle or a cutter of the active axis (or
    /// where one would be added), by the mode. Shift holds the interior handle hovered.
    fn hover_grips(&mut self, cx: &Cx<'_>, target: &Target, px: DVec2) {
        self.adding = None;
        self.snap = None;
        if let (true, Mode::Normal, Over::Handle(h), Some(k)) = (cx.shift_down, self.mode, self.over, self.active(cx, target)) {
            self.hold_handle(cx, target, k, h, px);
            return;
        }
        self.over = Over::Nothing;
        if self.mode != Mode::Remove
            && let Some((k, side)) = self.grip_at(cx, target, px)
        {
            if cx.axis_lock.is_none() {
                self.axis = Some(k);
            }
            if self.mode == Mode::Normal {
                self.over = Over::Grip { k, side };
                return;
            }
        }
        let Some(k) = self.active(cx, target) else { return };
        if self.mode == Mode::Add {
            self.adding = Some(Self::ratio_to_add(cx, target, k, px));
        } else if let Some(h) = self.handle_at(cx, target, k, px).filter(|_| self.mode == Mode::Normal) {
            self.over = Over::Handle(h);
        } else if let Some(i) = self.cutter_at(cx, target, k, px) {
            self.over = Over::Cutter(i);
        }
    }

    /// The stretch of the grip on `side` of `k` (or of the interior handle `handle`) starts, if
    /// the part can be stretched there.
    fn start(&mut self, cx: &Cx<'_>, target: Target, k: usize, side: f64, handle: Option<Handle>, drag: bool) -> Vec<Command> {
        let cuts: Vec<f64> = self.cutters[k].iter().map(|r| target.at(k, *r)).collect();
        let shares = self.shares(cuts.len() + 1, side, handle);
        // Refused now rather than at the end: the operation for a 1 mm pull, prepared.
        if let Err(e) = cx.doc.prepare(&self.operation(cx, &target, k, side, handle, 1.0)) {
            return vec![Command::Say(e.to_string())];
        }
        let spans =
            cx.doc.stretch_spans(&target.context, &target.occurrences, &target.in_context, DVec3::AXES[k], &cuts).unwrap_or_default();
        let range = move_range(&spans, &shares);
        if let Some(path) = target.single() {
            self.picked = Some(path);
        }
        self.over = Over::Nothing;
        self.snap = None;
        self.shift_ignored = cx.shift_down;
        self.state = State::Stretch { target: Box::new(target), k, side, handle, d: 0.0, range, spans, drag };
        vec![]
    }

    /// Each section's share of the move: the interior handle's profile, else the grip's.
    fn shares(&self, n: usize, side: f64, handle: Option<Handle>) -> Vec<f64> {
        match handle {
            Some(h) => interior_shares(n, h.index),
            None => shares(n, side, self.centered),
        }
    }

    /// The operation stretching `target`'s grip on `side` of `k` by `d` (outward), or moving its
    /// interior handle `handle` by `d` along the axis.
    fn operation(&self, cx: &Cx<'_>, target: &Target, k: usize, side: f64, handle: Option<Handle>, d: f64) -> Operation {
        let cuts: Vec<f64> = self.cutters[k].iter().map(|r| target.at(k, *r)).collect();
        let moves = self.shares(cuts.len() + 1, side, handle).into_iter().map(|s| s * d).collect();
        Operation::Stretch {
            context: target.context.clone(),
            occurrences: target.occurrences.clone(),
            frame: target.in_context,
            axis: DVec3::AXES[k],
            cuts,
            moves,
            scope: scope(cx),
        }
    }

    /// Is the measure the overall dimension? Not for an interior handle, which keeps it: the
    /// offset then, whatever the option.
    fn overall(&self) -> bool {
        self.overall && !matches!(self.state, State::Stretch { handle: Some(_), .. })
    }

    /// The stretch done by `d`: the operation, the cutters set again in the middle of the gaps
    /// between the matter ([`kept_cutters`]); back to the grips, on the same part (the axis kept: an interior handle leaves the
    /// cursor away from the grips).
    fn finish(&mut self, cx: &Cx<'_>, d: f64) -> Vec<Command> {
        let overall = self.overall();
        let measure = match &self.state {
            State::Stretch { target, k, .. } => self.measure(target, *k, d),
            _ => d,
        };
        let State::Stretch { target, k, side, handle, spans, .. } = std::mem::replace(&mut self.state, State::Grips) else {
            return vec![];
        };
        if d.abs() < MIN {
            return vec![];
        }
        let op = self.operation(cx, &target, k, side, handle, d);
        let Operation::Stretch { moves, .. } = &op else { unreachable!() };
        let (lo, hi) = (target.lo[k] + moves[0], target.hi[k] + moves[moves.len() - 1]);
        self.set_cutters(k, kept_cutters(&spans, moves, lo, hi));
        self.axis = Some(k);
        if overall {
            self.last_overall = Some(measure);
        } else {
            self.last_offset = Some(d);
        }
        vec![Command::Apply(op)]
    }

    /// A move asked for (typed, or taken again), within the range, else said why not.
    fn finish_checked(&mut self, cx: &Cx<'_>, d: f64) -> Vec<Command> {
        let State::Stretch { target, k, range, .. } = &self.state else { return vec![] };
        if d < range.0 - 1e-9 || d > range.1 + 1e-9 {
            let say = if self.overall() {
                let (value, min) = (self.measure(target, *k, d), self.measure(target, *k, range.0));
                t!("tool.stretch.say.min_length", value = self.format(cx, value), min = self.format(cx, min))
            } else {
                // An interior handle compresses either way.
                let max = if d < 0.0 { range.0.abs() } else { range.1 };
                t!("tool.stretch.say.max_compression", value = self.format(cx, d.abs()), max = self.format(cx, max))
            };
            return vec![Command::Say(say)];
        }
        self.finish(cx, d)
    }

    fn format(&self, cx: &Cx<'_>, v: f64) -> String {
        vcb::format(v, cx.units())
    }

    /// The move the cursor gives (OCL's `_snap_stretch`): back to the start near it; else an
    /// inferred point that tells a place along the axis, projected on it; else the closest point
    /// of the axis to the cursor's ray. Shift holds the last measure, the way the cursor's
    /// (once released, when it was down at the start).
    fn pull_at(&mut self, cx: &Cx<'_>, px: DVec2) {
        let (overall, centered) = (self.overall(), self.centered);
        let last = if overall { self.last_overall } else { self.last_offset };
        if !cx.shift_down {
            self.shift_ignored = false;
        }
        let shift = cx.shift_down && !self.shift_ignored;
        let State::Stretch { target, k, side, handle, d, range, .. } = &mut self.state else { return };
        let (k, side) = (*k, *side);
        let start = handle.map_or_else(|| target.grip(k, side), |h| h.at);
        let (o, dir) = (target.world(start), target.dir(k));
        // The opposite face, where the end would flatten the part; an interior handle has none.
        let opposite = handle.is_none().then(|| target.world(target.grip(k, -side)));
        self.snap = None;
        let near_start = cx.camera.project(o, cx.size).is_some_and(|at| at.distance(px) <= START_PX);
        let mut moved = if near_start {
            0.0
        } else {
            // Not on what the preview draws elsewhere — the parts stretched, or what of the
            // assemblies is stretched or moved (and, under the scope « all », their other
            // occurrences): they are not where they were.
            let paths = target.paths();
            let inf = cx.infer(px, |w| !paths.contains(w.path) && !cx.previewed.contains_key(w.path), None);
            match inf.filter(|i| stable_along(i, dir) && opposite.is_none_or(|op| (i.point - op).dot(dir).abs() > MIN)) {
                Some(i) => {
                    let m = (i.point - o).dot(dir) * side;
                    self.snap = Some(i);
                    m
                }
                None => (cx.ray(px).closest_on_line(o, dir) - o).dot(dir) * side,
            }
        };
        if shift && let Some(last) = last {
            moved = if overall {
                (last - target.length(k)) / if centered { 2.0 } else { 1.0 }
            } else {
                last.abs() * if moved < 0.0 { -1.0 } else { 1.0 }
            };
            self.snap = None;
        }
        *d = moved.clamp(range.0, range.1);
    }

    /// What the measure shows for the move `d`: the overall dimension, or the offset.
    fn measure(&self, target: &Target, k: usize, d: f64) -> f64 {
        if self.overall() { target.length(k) + d * if self.centered { 2.0 } else { 1.0 } } else { d }
    }

    /// The interior handle taken (OCL's `_preview_stretch_start`): its segment bold, with arrows
    /// at both ends, over everything; where it is grabbed, a diamond when magnetized; held away
    /// from it, a dashed line from the point it is taken from.
    fn preview_handle(&self, cx: &Cx<'_>, target: &Target, k: usize, h: Handle, p: &mut Preview) {
        let color = axis_color(k);
        let Some((_, a, b)) = self.handles(target, k).into_iter().find(|(i, _, _)| *i == h.index) else { return };
        let (a, b) = (target.world(a), target.world(b));
        p.lines.push(PreviewLine { a, b, color, bold: true });
        let dir = target.dir(k);
        let across = dir.cross(cx.camera.forward()).try_normalize().unwrap_or(DVec3::ZERO);
        for (tip, back) in [(a, dir), (b, -dir)] {
            let s = cx.camera.world_per_pixel(cx.size, tip) * ARROW_PX;
            for side in [-0.5, 0.5] {
                p.lines.push(PreviewLine { a: tip, b: tip + back * s + across * side * s, color, bold: true });
            }
        }
        let at = target.world(h.at);
        let radius = if h.magnetized { 6.0 } else { 5.0 };
        p.points.push(PreviewPoint { at, radius, fill: Some(color), stroke: Some(WHITE), diamond: h.magnetized });
        if let Some(from) = h.from.map(|f| target.world(f)).filter(|f| (*f - at).length() > MIN) {
            dashed(cx.camera, cx.size, &mut p.lines, from, at, color);
        }
    }

    /// The grips' drawing: the axes between their grips (not while a cutter moves), the active
    /// axis's cutters (in the scene: hidden behind the part's faces).
    fn preview_grips(&self, cx: &Cx<'_>, target: &Target, p: &mut Preview, moving: Option<(usize, usize)>) {
        let active = match moving {
            Some((k, _)) => Some(k),
            None => self.active(cx, target),
        };
        if moving.is_none() {
            for k in target.axes().filter(|k| cx.axis_lock.is_none_or(|l| l == *k)) {
                let (a, b) = (target.world(target.grip(k, -1.0)), target.world(target.grip(k, 1.0)));
                let color = if active == Some(k) { axis_color(k) } else { GREY };
                dashed(cx.camera, cx.size, &mut p.lines, a, b, color);
                for (side, at) in [(-1.0, a), (1.0, b)] {
                    let fill = if self.over == (Over::Grip { k, side }) { color } else { WHITE };
                    let radius = if active == Some(k) { 5.0 } else { 4.0 };
                    p.points.push(PreviewPoint { at, radius, fill: Some(fill), stroke: Some(color), diamond: false });
                }
            }
        }
        // The active axis's cutters: translucent planes across the box, in the scene.
        let Some(k) = active else { return };
        if moving.is_none()
            && let Over::Handle(h) = self.over
        {
            self.preview_handle(cx, target, k, h, p);
        }
        let inflate = Self::inflate(cx, target);
        let color = axis_color(k);
        let ratios: Vec<f64> = self.cutters[k].iter().copied().chain(self.adding).collect();
        for (i, r) in ratios.iter().enumerate() {
            let adding = self.adding.is_some() && i == ratios.len() - 1;
            let dragged = moving == Some((k, i));
            let picked = self.over == Over::Cutter(i) || dragged;
            let going = (picked && self.mode == Mode::Remove) || (dragged && !(SAME_CUT..=1.0 - SAME_CUT).contains(r));
            let c = target.plane(k, target.at(k, *r), inflate);
            let ink = if going { GREY } else { color };
            p.fills.push(PreviewFill { corners: c.to_vec(), color: translucent(ink, if picked && !going { 0.6 } else { 0.3 }) });
            for j in 0..4 {
                if going || adding {
                    dashed(cx.camera, cx.size, &mut p.scene_lines, c[j], c[(j + 1) % 4], ink);
                } else {
                    p.scene_lines.push(PreviewLine { a: c[j], b: c[(j + 1) % 4], color: ink, bold: picked });
                }
            }
            p.points.push(PreviewPoint { at: (c[0] + c[2]) / 2.0, radius: 3.0, fill: Some(ink), stroke: None, diamond: false });
        }
    }

    /// What the input does in the grips' state.
    fn input_grips(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        let Some(target) = self.target(cx) else {
            return self.input_pick(input, cx);
        };
        self.hovered = None;
        self.sync_cutters(&target);
        match input {
            ToolInput::Hover { px } => self.hover_grips(cx, &target, *px),
            ToolInput::Click { px, .. } => {
                self.hover_grips(cx, &target, *px);
                let active = self.active(cx, &target);
                match (self.mode, self.over, active) {
                    (Mode::Add, _, Some(k)) => {
                        if let Some(r) = self.adding {
                            let mut ratios = self.cutters[k].clone();
                            ratios.push(r);
                            self.set_cutters(k, ratios);
                        }
                    }
                    (Mode::Remove, Over::Cutter(i), Some(k)) => {
                        let mut ratios = self.cutters[k].clone();
                        ratios.remove(i);
                        self.set_cutters(k, ratios);
                    }
                    (Mode::Normal, Over::Grip { k, side }, _) => return self.start(cx, target, k, side, None, false),
                    (Mode::Normal, Over::Handle(h), Some(k)) => return self.start(cx, target, k, 1.0, Some(h), false),
                    // A click on nothing lets the part go.
                    (Mode::Normal, Over::Nothing, _) => {
                        self.picked = None;
                        if !self.selected(cx).is_empty() {
                            return vec![Command::Select(Vec::new())];
                        }
                    }
                    _ => {}
                }
            }
            ToolInput::DragStart { px, .. } => {
                self.hover_grips(cx, &target, *px);
                match (self.mode, self.over) {
                    (Mode::Normal, Over::Grip { k, side }) => return self.start(cx, target, k, side, None, true),
                    (Mode::Normal, Over::Handle(h)) => {
                        let Some(k) = self.active(cx, &target) else { return vec![] };
                        return self.start(cx, target, k, 1.0, Some(h), true);
                    }
                    (Mode::Normal, Over::Cutter(index)) => {
                        let Some(k) = self.active(cx, &target) else { return vec![] };
                        // Where the cursor's ray meets the cutter's plane, else on the axis.
                        let at = target.at(k, self.cutters[k][index]);
                        let ray = target.ray(cx, *px);
                        let from = if ray.dir[k].abs() > 1e-9 {
                            ray.at((at - ray.origin[k]) / ray.dir[k])
                        } else {
                            let mut c = (target.lo + target.hi) / 2.0;
                            c[k] = at;
                            c
                        };
                        if let Some(path) = target.single() {
                            self.picked = Some(path);
                        }
                        self.state = State::Cutter { k, index, from };
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        vec![]
    }

    /// Nothing taken yet (OCL's `SmartSelectActionHandler`): the occurrence of the active
    /// context under the cursor — a part, or the assembly holding it, as the selection takes it
    /// — picked by a click; Shift held, clicks add or remove occurrences to the selection, taken
    /// once Shift is released; a drag draws a rectangle, its occurrences selected.
    fn input_pick(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        let under = |px: DVec2| cx.pick(px).and_then(|h| cx.selectable(&h.path));
        match input {
            ToolInput::Hover { px } => {
                self.hovered = under(*px).map(|id| EntityPath::new(cx.context.to_vec(), id));
            }
            ToolInput::Click { px, mods } if mods.toggle => {
                if let Some(id) = under(*px) {
                    self.gathering = true;
                    return vec![Command::Select(toggled(cx.selection, &[id]))];
                }
            }
            ToolInput::Click { .. } => self.picked = self.hovered.take(),
            ToolInput::DragStart { px, .. } => {
                self.hovered = None;
                self.marquee = Some(Marquee::new(*px, *px));
            }
            ToolInput::Drag { px } => {
                if let Some(m) = &mut self.marquee {
                    *m = Marquee::new(m.from, *px);
                    m.hits = Self::occurrences(cx, marquee_hits(cx, m.from, m.to, m.crossing));
                }
            }
            ToolInput::DragEnd { px, mods } => {
                let Some(m) = self.marquee.take() else { return vec![] };
                if ((*px - m.from).abs()).max_element() < MIN_MARQUEE {
                    return self.input_pick(&ToolInput::Click { px: *px, mods: *mods }, cx);
                }
                let m = Marquee::new(m.from, *px);
                let hits = Self::occurrences(cx, marquee_hits(cx, m.from, m.to, m.crossing));
                if mods.toggle {
                    self.gathering = true;
                    return vec![Command::Select(toggled(cx.selection, &hits))];
                }
                return vec![Command::Select(hits)];
            }
            _ => {}
        }
        vec![]
    }

    fn input_state(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        // The other modifier tapped: a separate part or not (OCL's "make unique"); not in an
        // open part, whose scope is the part's.
        if let ToolInput::Key { key: Key::Alt, .. } = input
            && cx.open_part.is_none()
            && !matches!(self.state, State::Cutter { .. })
        {
            let next = if scope(cx) == Scope::This { ALL } else { THIS };
            return vec![Command::SetOption { key: SCOPE, value: OptionValue::Choice(next.into()) }];
        }
        match (&self.state, input) {
            (State::Grips, _) => return self.input_grips(input, cx),

            // ---- a cutter dragged along the axis: its ratio, the ends removing it when dropped
            (State::Cutter { k, index, from }, ToolInput::Drag { px }) => {
                let (k, index, from) = (*k, *index, *from);
                if let Some(target) = self.target(cx) {
                    let p = target.ray(cx, *px).closest_on_line(from, DVec3::AXES[k]);
                    if let Some(r) = self.cutters[k].get_mut(index) {
                        *r = ((p[k] - target.lo[k]) / target.length(k)).clamp(0.0, 1.0);
                    }
                }
            }
            (State::Cutter { k, .. }, ToolInput::DragEnd { .. }) => {
                let k = *k;
                self.state = State::Grips;
                let ratios = self.cutters[k].clone();
                self.set_cutters(k, ratios);
            }

            // ---- the stretch
            (State::Stretch { .. }, ToolInput::Hover { px } | ToolInput::Drag { px }) => self.pull_at(cx, *px),
            (State::Stretch { d, drag: false, .. }, ToolInput::Click { .. })
            | (State::Stretch { d, drag: true, .. }, ToolInput::DragEnd { .. }) => {
                let d = *d;
                return self.finish(cx, d);
            }
            (State::Stretch { target, k, .. }, ToolInput::DoubleClick { .. }) => {
                let (target, k) = ((**target).clone(), *k);
                let d = if self.overall() {
                    self.last_overall.map(|l| (l - target.length(k)) / if self.centered { 2.0 } else { 1.0 })
                } else {
                    self.last_offset
                };
                if let Some(d) = d {
                    return self.finish_checked(cx, d);
                }
            }
            // Centred: not for an interior handle, which keeps both ends.
            (State::Stretch { handle: None, .. }, ToolInput::Key { key: Key::Copy, .. }) => {
                return vec![Command::SetOption { key: CENTERED, value: OptionValue::Toggle(!self.centered) }];
            }
            (State::Stretch { target, k, handle, d, .. }, ToolInput::Value(VcbValue::Lengths(v))) => {
                let (target, k) = ((**target).clone(), *k);
                if self.overall() && v[0] < 0.0 {
                    return vec![Command::Say(t!("tool.stretch.say.invalid_length", value = self.format(cx, v[0])))];
                }
                // An interior handle goes the way the cursor takes it (along the axis without a
                // move yet).
                let way = if handle.is_some() && *d < 0.0 { -1.0 } else { 1.0 };
                let d = if self.overall() { (v[0] - target.length(k)) / if self.centered { 2.0 } else { 1.0 } } else { v[0] * way };
                return self.finish_checked(cx, d);
            }
            _ => {}
        }
        vec![]
    }
}

impl Tool for StretchTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Stretch
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        // Shift released: the selection made is taken.
        if self.gathering && !cx.shift_down {
            self.gathering = false;
        }
        self.centered = cx.toggle(CENTERED);
        self.overall = cx.choice(MEASURE) != Some(OFFSET);
        self.mode = if cx.copy_down {
            Mode::Add
        } else if cx.alt_down {
            Mode::Remove
        } else {
            Mode::Normal
        };
        let commands = self.input_state(input, cx);
        self.aimed = self.target(cx).is_some();
        commands
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let mut p = Preview::default();
        let Some(target) = self.target(cx) else {
            p.hover_solid = self.hovered.clone();
            p.marquee = self.marquee.clone();
            return p;
        };
        match &self.state {
            State::Grips => self.preview_grips(cx, &target, &mut p, None),
            State::Cutter { k, index, .. } => self.preview_grips(cx, &target, &mut p, Some((*k, *index))),
            State::Stretch { k, side, handle, d, .. } => {
                let (k, side, handle, d) = (*k, *side, *handle, *d);
                let cuts: Vec<f64> = self.cutters[k].iter().map(|r| target.at(k, *r)).collect();
                let moves: Vec<f64> = self.shares(cuts.len() + 1, side, handle).into_iter().map(|s| s * d).collect();
                // The measure line (OCL's lps → lpe): from the opposite face, moved when centred,
                // for the overall dimension; from the grip (or the handle's point) for the offset.
                let color = axis_color(k);
                let grip = handle.map_or_else(|| target.grip(k, side), |h| h.at);
                let mut end = grip;
                end[k] += side * d;
                let start = if self.overall() {
                    let mut s = target.grip(k, -side);
                    s[k] -= if self.centered { side * d } else { 0.0 };
                    s
                } else {
                    grip
                };
                let (a, b) = (target.world(start), target.world(end));
                dashed(cx.camera, cx.size, &mut p.lines, a, b, color);
                for at in [a, b] {
                    p.points.push(PreviewPoint { at, radius: 4.0, fill: Some(WHITE), stroke: Some(color), diamond: false });
                }
                p.points.push(PreviewPoint { at: target.world(grip), radius: 4.0, fill: Some(color), stroke: None, diamond: false });
                if (b - a).length() > MIN {
                    p.labels.push(PreviewLabel { a, b, text: self.format(cx, self.measure(&target, k, d).abs()), color });
                }
                p.stretch = Some(StretchPreview {
                    context: target.context.clone(),
                    occurrences: target.occurrences.clone(),
                    frame: target.in_context,
                    all: scope(cx) == Scope::All,
                    axis: DVec3::AXES[k],
                    cuts,
                    moves,
                });
            }
        }
        p.inference = self.snap.clone();
        p.propagate = target.single().filter(|_| scope(cx) == Scope::All);
        p
    }

    fn status(&self) -> String {
        match (&self.state, self.aimed, self.mode) {
            (State::Grips, false, _) => match &self.marquee {
                Some(m) if m.crossing => t!("tool.select.hint.crossing"),
                Some(_) => t!("tool.select.hint.window"),
                None if self.gathering => t!("tool.stretch.hint.gather"),
                None => t!("tool.stretch.hint.pick"),
            },
            (State::Grips, true, Mode::Add) => t!("tool.stretch.hint.add"),
            (State::Grips, true, Mode::Remove) => t!("tool.stretch.hint.remove"),
            (State::Grips, true, Mode::Normal) if matches!(self.over, Over::Handle(_)) => {
                t!("tool.stretch.hint.interior", copy_key = copy_key(), alt_key = alt_key())
            }
            (State::Grips, true, Mode::Normal) => t!("tool.stretch.hint.grip", copy_key = copy_key(), alt_key = alt_key()),
            (State::Cutter { .. }, _, _) => t!("tool.stretch.hint.cutter"),
            (State::Stretch { handle: Some(_), .. }, _, _) => t!("tool.stretch.hint.stretch_interior", alt_key = alt_key()),
            (State::Stretch { .. }, _, _) => t!("tool.stretch.hint.stretch", copy_key = copy_key(), alt_key = alt_key()),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        let State::Stretch { target, k, d, .. } = &self.state else { return None };
        let label = if self.overall() { t!("tool.stretch.vcb.overall") } else { t!("tool.stretch.vcb.offset") };
        Some(Vcb::new(label, VcbKind::Lengths(vec![self.measure(target, *k, *d)])))
    }

    fn busy(&self) -> bool {
        !matches!(self.state, State::Grips) || self.picked.is_some() || self.marquee.is_some() || self.gathering
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let target = self.target(cx);
        let choice = |value: &'static str, label: String, tip: String| OptionChoice { value, label, tip };
        let mut options = vec![
            ToolOption {
                key: MEASURE,
                label: t!("tool.stretch.option.measure"),
                tip: t!("tool.stretch.option.measure_tip"),
                value: OptionValue::Choice(if cx.choice(MEASURE) == Some(OFFSET) { OFFSET } else { OVERALL }.into()),
                choices: vec![
                    choice(OVERALL, t!("tool.stretch.option.overall"), t!("tool.stretch.option.overall_tip")),
                    choice(OFFSET, t!("tool.stretch.option.offset"), t!("tool.stretch.option.offset_tip")),
                ],
                saved: true,
            },
            ToolOption {
                key: AXES,
                label: t!("tool.stretch.option.axes"),
                tip: t!("tool.stretch.option.axes_tip"),
                value: OptionValue::Choice(
                    match axes(cx) {
                        Axes::Active => ACTIVE,
                        Axes::Context => CONTEXT,
                        Axes::Entity => ENTITY,
                    }
                    .into(),
                ),
                choices: vec![
                    choice(ACTIVE, t!("tool.stretch.option.active"), t!("tool.stretch.option.active_tip")),
                    choice(CONTEXT, t!("tool.stretch.option.context"), t!("tool.stretch.option.context_tip")),
                    choice(ENTITY, t!("tool.stretch.option.entity"), t!("tool.stretch.option.entity_tip")),
                ],
                saved: true,
            },
            ToolOption {
                key: CENTERED,
                label: t!("tool.stretch.option.centered"),
                tip: t!("tool.stretch.option.centered_tip", copy_key = copy_key()),
                value: OptionValue::Toggle(cx.toggle(CENTERED)),
                choices: Vec::new(),
                saved: true,
            },
        ];
        // The open part has its own scope, in the bar already.
        if cx.open_part.is_none() {
            let all = match target.as_ref().and_then(|t| t.single()).map(|p| cx.copies_shown(&p)) {
                Some(n) if n > 1 => t!("tool.boolean.option.all_count", count = n),
                Some(_) => t!("tool.boolean.option.all_one"),
                None => t!("tool.boolean.option.all"),
            };
            options.push(ToolOption {
                key: SCOPE,
                label: t!("tool.boolean.option.scope"),
                tip: t!("tool.stretch.option.scope_tip", alt_key = alt_key()),
                value: OptionValue::Choice(if scope(cx) == Scope::This { THIS } else { ALL }.into()),
                choices: vec![
                    choice(ALL, all, t!("tool.boolean.option.all_tip")),
                    choice(THIS, t!("tool.boolean.option.this"), t!("tool.stretch.option.this_tip")),
                ],
                saved: true,
            });
        }
        options
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        match (key, value) {
            (CENTERED, OptionValue::Toggle(on)) => self.centered = *on,
            (MEASURE, OptionValue::Choice(v)) => self.overall = v != OFFSET,
            // Other axes, another box: back to its grips (OCL's `onToolActionOptionStored`).
            (AXES, _) => {
                self.state = State::Grips;
                self.over = Over::Nothing;
                self.snap = None;
            }
            _ => {}
        }
    }

    fn leave(&mut self) {
        self.over = Over::Nothing;
        self.adding = None;
        self.hovered = None;
        self.snap = None;
    }

    fn back(&mut self) {
        match self.state {
            State::Grips if self.marquee.is_some() => self.marquee = None,
            State::Grips => self.picked = None,
            _ => self.state = State::Grips,
        }
        self.snap = None;
    }

    fn reset(&mut self) {
        self.state = State::Grips;
        self.picked = None;
        self.marquee = None;
        self.gathering = false;
        self.snap = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_follow_the_grip() {
        assert_eq!(shares(2, 1.0, false), vec![0.0, 1.0]);
        assert_eq!(shares(2, -1.0, false), vec![-1.0, -0.0]);
        assert_eq!(shares(3, 1.0, false), vec![0.0, 0.5, 1.0]);
        assert_eq!(shares(3, 1.0, true), vec![-1.0, 0.0, 1.0]);
        assert_eq!(shares(2, -1.0, true), vec![-1.0, 1.0]);
    }

    #[test]
    fn compression_keeps_the_sections_apart() {
        // A board 0–600 cut at 300: its ends' matter at 0 and 600.
        let spans = [Some((0.0, 0.0)), Some((600.0, 600.0))];
        assert_eq!(move_range(&spans, &shares(2, 1.0, false)), (-599.0, f64::INFINITY));
        assert_eq!(move_range(&spans, &shares(2, 1.0, true)), (-299.5, f64::INFINITY));
        // Nothing whole in a section: no limit from it.
        assert_eq!(move_range(&[Some((0.0, 10.0)), None], &[0.0, 1.0]), (f64::NEG_INFINITY, f64::INFINITY));
    }

    #[test]
    fn an_interior_handle_keeps_both_ends() {
        assert_eq!(interior_shares(4, 1), vec![0.0, 1.0, 0.5, 0.0]);
        assert_eq!(interior_shares(4, 2), vec![0.0, 0.5, 1.0, 0.0]);
        // Matter at 0, 250 and 600 (cut at 200 and 300): either way a gap closes.
        let spans = [Some((0.0, 0.0)), Some((250.0, 250.0)), Some((600.0, 600.0))];
        assert_eq!(move_range(&spans, &interior_shares(3, 1)), (-249.0, 349.0));
    }

    #[test]
    fn cutters_stay_between_the_same_matter() {
        // A board 0–600, holes at 100, 250 (to 255) and 500 (from 495), cut at 300.
        let spans = [Some((0.0, 255.0)), Some((495.0, 600.0))];
        // Pulled 200: the cutter in the middle of the gap, 255–695.
        assert_eq!(kept_cutters(&spans, &[0.0, 200.0], 0.0, 800.0), vec![475.0 / 800.0]);
        // Compressed 239 (as far as it goes): still past the hole at 250, at 255.5.
        assert_eq!(kept_cutters(&spans, &[0.0, -239.0], 0.0, 361.0), vec![255.5 / 361.0]);
        // Two cutters with nothing between them: one, in the gap; overlapping matter: none.
        assert_eq!(kept_cutters(&[Some((0.0, 100.0)), None, Some((300.0, 400.0))], &[0.0, 50.0, 100.0], 0.0, 500.0), vec![0.5]);
        assert!(kept_cutters(&[Some((0.0, 350.0)), Some((250.0, 400.0))], &[0.0, 0.0], 0.0, 400.0).is_empty());
    }

    #[test]
    fn cutters_are_sanitized() {
        assert_eq!(sanitized(vec![0.7, 0.2, 0.2000001, 1.0, 0.0]), vec![0.2, 0.7]);
        assert_eq!(sanitized(vec![1.0]), vec![0.5]);
    }
}

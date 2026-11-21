//! The « Mise en plan » workspace's stage (docs/ui/drawing.md, decision 0048): a sheet of
//! the document's drawing on its paper, panned and zoomed, its views' frames, its elements;
//! the sheets' tabs under it.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{DAffine3, DMat4, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{
    DimKind, Dimension, Direction, Drawing, ElementKind, Entity, EntityPath, Found, Id, Look, Operation, Orient, Paper, Placing, Resolved,
    Sheet, Side, Subject, View, ViewKind, ViewStyle,
};
use skraft_theme::Theme;
use skraft_tools::Editor;

use crate::viewport;

/// A document's sheet shown and how it is seen (not saved).
#[derive(Clone, Debug, Default)]
pub struct SheetState {
    /// The sheet shown: none, the first.
    pub current: Option<Id>,
    /// The sheet point at the stage's centre, in millimetres.
    pan: DVec2,
    /// Points per millimetre of paper.
    zoom: f64,
    /// The sheet framed whole last; another (or none) frames the shown one again.
    fitted: Option<Id>,
    /// The elements picked on the sheet (a click, Shift+click, a rectangle), what the commands
    /// change.
    pub selected: Vec<Id>,
    /// What a click on the sheet does.
    pub tool: SheetTool,
    /// The selection being dragged, from where (sheet millimetres).
    moving: Option<DVec2>,
    /// Where the selection rectangle being drawn starts (screen points).
    marquee: Option<egui::Pos2>,
    /// The pointer over the sheet last frame (screen points), for the status bar.
    pointer: Option<egui::Pos2>,
    /// The handle being dragged.
    grip: Option<Grip>,
    /// The views' frames on screen last frame, bottom first: what a press lands on.
    frames: Vec<(Id, egui::Rect)>,
    /// What each view shows, for the document's revision and the meshes' generation.
    views: ViewsCache,
    /// The layer what is drawn goes on (none, or one gone: the first not shared).
    pub active_layer: Option<Id>,
    /// Where the rectangle or line being drawn starts (sheet millimetres).
    sketch: Option<DVec2>,
    /// A command the sheet asks the app to run (a tab's menu).
    pub command: Option<&'static str>,
    /// What a dimension's tool picked so far: the view, the anchors.
    picks: Vec<(Id, skraft_model::Anchor)>,
}

/// The layer what is drawn goes on: the one chosen if it is still there, shown and not locked,
/// else the first such one not shared.
pub fn active_layer(d: &Drawing, st: &SheetState) -> Option<Id> {
    let usable = |l: &&skraft_model::Layer| !l.hidden && !l.locked;
    st.active_layer.and_then(|id| d.layer(id)).filter(usable).or_else(|| d.layers.iter().filter(|l| !l.shared).find(usable)).map(|l| l.id)
}

/// The element picked on the sheet shown, if it is still there.
pub fn selected_element<'a>(e: &'a Editor, st: &SheetState) -> Option<&'a skraft_model::Element> {
    match selection(e, st).as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// The elements picked on the sheet shown that are still there.
pub fn selection<'a>(e: &'a Editor, st: &SheetState) -> Vec<&'a skraft_model::Element> {
    let d = e.document().drawing();
    let Some(sheet) = st.shown(d) else { return Vec::new() };
    d.elements_of(sheet).filter(|el| st.selected.contains(&el.id)).collect()
}

/// The views among them.
pub fn selected_views<'a>(e: &'a Editor, st: &SheetState) -> Vec<(Id, &'a View)> {
    selection(e, st)
        .into_iter()
        .filter_map(|el| match &el.kind {
            ElementKind::View(v) => Some((el.id, v)),
            _ => None,
        })
        .collect()
}

/// Is element `id` carried by another of `moved` (a view projected from one, a dimension
/// through one), so that it is not moved itself?
fn carried(d: &Drawing, id: Id, moved: &[Id]) -> bool {
    let by = match d.element(id).map(|el| &el.kind) {
        Some(ElementKind::View(View { kind: ViewKind::Projected { base, .. }, .. })) => *base,
        Some(ElementKind::Dimension(dim)) => dim.view,
        _ => return false,
    };
    moved.contains(&by) || carried(d, by, moved)
}

/// The elements of `moved` that move themselves, and what they become moved by `offset`.
fn moves(d: &Drawing, moved: &[Id], offset: DVec2) -> Vec<(Id, ElementKind)> {
    moved
        .iter()
        .filter(|id| !carried(d, **id, moved))
        .filter_map(|id| d.element(*id).map(|el| (*id, moved_element(&el.kind, offset))))
        .collect()
}

/// The texts' fields (docs/ui/drawing.md#fields): what they say on this sheet.
pub struct Fields<'a> {
    pub document: &'a str,
    pub sheet: &'a str,
    pub number: usize,
    pub count: usize,
    pub scale: Option<f64>,
}

/// The fields a text may hold, in the « Insérer un champ » menu's order.
pub const FIELDS: &[&str] = &["document", "date", "sheet", "sheet.number", "sheet.count", "scale"];

impl Fields<'_> {
    /// `text` with its `{…}` fields filled (an unknown one left as it is).
    pub fn fill(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            match tail.find('}') {
                Some(j) => {
                    let key = &tail[1..j];
                    match self.value(key) {
                        Some(v) => out.push_str(&v),
                        None => out.push_str(&tail[..=j]),
                    }
                    rest = &tail[j + 1..];
                }
                None => {
                    out.push_str(tail);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    fn value(&self, key: &str) -> Option<String> {
        Some(match key {
            "document" => self.document.to_owned(),
            "date" => today(),
            "sheet" => self.sheet.to_owned(),
            "sheet.number" => self.number.to_string(),
            "sheet.count" => self.count.to_string(),
            "scale" => self.scale.map(scale_label).unwrap_or_default(),
            _ => return None,
        })
    }
}

/// Today's date, day/month/year (UTC).
fn today() -> String {
    let secs = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    // Howard Hinnant's civil_from_days.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d:02}/{m:02}/{y}")
}

/// What each view shows (its parts, its box, what its lines are computed from), kept while
/// the document and its meshes stay as they are: a sheet drawn again computes none of it.
#[derive(Clone, Debug, Default)]
struct ViewsCache {
    key: (u64, u64),
    views: HashMap<Id, Arc<ViewData>>,
}

/// What a view shows.
struct ViewData {
    /// Its subject's box in its frame (x, y, depth); none while no part's box is known.
    bx3: Option<(DVec3, DVec3)>,
    /// Per part (its definition): its placements in the subject's axes and their materials.
    parts: Vec<(Id, Vec<Placed>)>,
    lines: Option<crate::view_lines::LinesJob>,
    /// The parts it shows, placed in its subject's axes.
    placed: Vec<skraft_model::PlacedPart>,
    /// Its vertices and edges on the sheet, made when a dimension's tool asks.
    snaps: std::sync::OnceLock<Snaps>,
}

/// An edge a dimension's tool may pick: its placed part, its index, its shape in the part's
/// axes, as a polyline in the part's axes and on the sheet.
type SnapEdge = (usize, u32, skraft_model::kernel::topo::EdgeShape, Vec<DVec3>, Vec<DVec2>);

/// What a dimension's tool picks in a view: its parts' vertices and edges on the sheet.
#[derive(Default)]
struct Snaps {
    /// (placed part, vertex, its point in the part's axes, on the sheet).
    vertices: Vec<(usize, u32, DVec3, DVec2)>,
    /// (placed part, edge, its shape in the part's axes, as a polyline in the part's axes and
    /// on the sheet).
    edges: Vec<SnapEdge>,
}

impl ViewData {
    /// The centre of its box in its frame (what its frame's centre stands for).
    fn centre(&self) -> Option<DVec2> {
        self.bx3.map(|(lo, hi)| ((lo + hi) / 2.0).truncate())
    }

    /// Its vertices and edges on the sheet.
    fn snaps(&self, e: &Editor, r: &Resolved<'_>) -> &Snaps {
        self.snaps.get_or_init(|| {
            let Some(centre) = self.centre() else { return Snaps::default() };
            let on_sheet = |p: DVec3| r.at + (r.frame.project(p).truncate() - centre) * r.scale;
            let mut s = Snaps::default();
            for (k, p) in self.placed.iter().enumerate() {
                let Ok(part) = e.document().part(p.part) else { continue };
                let shape = part.shape();
                for (v, q) in shape.vertex_points().into_iter().enumerate() {
                    s.vertices.push((k, v as u32, q, on_sheet(p.transform.transform_point3(q))));
                }
                for i in 0..shape.edge_count() as u32 {
                    let Some(es) = shape.edge_shape(i) else { continue };
                    let pts: Vec<DVec3> = match es {
                        skraft_model::kernel::topo::EdgeShape::Circle { centre, normal, radius, a, b } => {
                            // Its arc sampled, from a to b about its normal (a whole turn when closed).
                            let x = (a - centre).normalize_or_zero();
                            let y = normal.cross(x);
                            let end = (b - centre).normalize_or_zero();
                            let mut sweep = end.dot(y).atan2(end.dot(x));
                            if sweep <= 1e-9 {
                                sweep += std::f64::consts::TAU;
                            }
                            (0..=24)
                                .map(|j| centre + (x * (sweep * j as f64 / 24.0).cos() + y * (sweep * j as f64 / 24.0).sin()) * radius)
                                .collect()
                        }
                        other => {
                            let (a, b) = other.ends();
                            vec![a, b]
                        }
                    };
                    let sheet: Vec<DVec2> = pts.iter().map(|q| on_sheet(p.transform.transform_point3(*q))).collect();
                    s.edges.push((k, i, es, pts, sheet));
                }
            }
            s
        })
    }
}

impl std::fmt::Debug for ViewData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewData").field("bx3", &self.bx3).field("parts", &self.parts.len()).finish()
    }
}

impl ViewsCache {
    /// What view `id` (`r`) shows.
    fn get(&mut self, e: &Editor, id: Id, r: &Resolved<'_>) -> Arc<ViewData> {
        let key = (e.history().revision(), e.scene().generation());
        if key != self.key {
            self.key = key;
            self.views.clear();
        }
        self.views
            .entry(id)
            .or_insert_with(|| {
                let placed = e.document().subject_parts(r.subject);
                let mut by_part: HashMap<Id, Vec<Placed>> = HashMap::new();
                for p in &placed {
                    by_part.entry(p.part).or_default().push((p.transform, p.material));
                }
                let mut parts: Vec<_> = by_part.into_iter().collect();
                parts.sort_by_key(|(p, _)| *p);
                Arc::new(ViewData {
                    bx3: box_of(e, r, &placed),
                    parts,
                    lines: crate::view_lines::LinesJob::of(e, r, &placed),
                    placed,
                    snaps: std::sync::OnceLock::new(),
                })
            })
            .clone()
    }
}

/// What a click on the sheet does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SheetTool {
    /// Picks a view; a drag moves it, elsewhere pans.
    #[default]
    Select,
    /// Places views projected from `base` on the side the pointer is (« Vue projetée »).
    Project {
        base: Id,
    },
    /// A click puts a text there (« Texte »).
    Text,
    /// A drag draws a rectangle (« Rectangle »), a line (« Trait »).
    Rect,
    Line,
    /// Dimensions (docs/ui/drawing.md#dimensions): linear (two vertices, then its line),
    /// radius and diameter (a circular edge, then its text), angle (two straight edges, then
    /// its arc).
    Linear,
    Radius,
    Diameter,
    Angle,
}

impl SheetTool {
    /// The tool a command (`drawing.tool_*`) starts.
    pub fn of(id: &str) -> Option<SheetTool> {
        match id {
            "drawing.tool_text" => Some(SheetTool::Text),
            "drawing.tool_rect" => Some(SheetTool::Rect),
            "drawing.tool_line" => Some(SheetTool::Line),
            "drawing.dim_linear" => Some(SheetTool::Linear),
            "drawing.dim_radius" => Some(SheetTool::Radius),
            "drawing.dim_diameter" => Some(SheetTool::Diameter),
            "drawing.dim_angle" => Some(SheetTool::Angle),
            _ => None,
        }
    }
}

/// The paper's margin around a sheet framed whole, as a share of its size.
const FIT_MARGIN: f64 = 1.08;
/// Standard scales, paper over model, largest first.
pub const SCALES: &[f64] = &[
    10.0,
    5.0,
    2.0,
    1.0,
    1.0 / 2.0,
    1.0 / 5.0,
    1.0 / 10.0,
    1.0 / 20.0,
    1.0 / 25.0,
    1.0 / 50.0,
    1.0 / 100.0,
    1.0 / 200.0,
    1.0 / 500.0,
    1.0 / 1000.0,
    1.0 / 2000.0,
    1.0 / 5000.0,
];

/// How near a handle the pointer takes it (points), and its square's side.
const GRIP_PT: f32 = 6.0;
const GRIP_SIDE: f32 = 7.0;

/// A handle of the element picked alone (docs/ui/drawing.md#handles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    /// A box's (rectangle, image) corner or side middle: the sides it moves along x and y
    /// (-1 the left or bottom one, 1 the right or top one, 0 none).
    Side(i8, i8),
    /// A line's start (0) or end (1).
    End(u8),
    /// A base view frame's corner: its scale.
    Scale(i8, i8),
    /// A text's top right: its height.
    Size,
    /// A dimension's line (linear) or text (radius, diameter) or arc (angle).
    Dim,
}

impl Handle {
    /// The pointer over it.
    fn cursor(self) -> egui::CursorIcon {
        use egui::CursorIcon as C;
        match self {
            Handle::Side(0, _) => C::ResizeVertical,
            Handle::Side(_, 0) => C::ResizeHorizontal,
            Handle::Side(x, y) | Handle::Scale(x, y) if x == y => C::ResizeNeSw,
            Handle::Side(..) | Handle::Scale(..) => C::ResizeNwSe,
            Handle::Size => C::ResizeVertical,
            Handle::End(_) | Handle::Dim => C::Move,
        }
    }
}

/// A handle taken: the element as it was, where the handle was and where it was taken (sheet
/// millimetres), its box then.
#[derive(Clone, Debug)]
struct Grip {
    element: Id,
    handle: Handle,
    start: DVec2,
    from: DVec2,
    kind: ElementKind,
    lo: DVec2,
    hi: DVec2,
}

/// The handles of element `el` (`frame`: its box on the sheet as last drawn), on the sheet.
fn handles(
    e: &Editor,
    d: &Drawing,
    views: &mut ViewsCache,
    el: &skraft_model::Element,
    frame: Option<(DVec2, DVec2)>,
) -> Vec<(Handle, DVec2)> {
    let pick = |s: i8, lo: f64, hi: f64| match s {
        -1 => lo,
        0 => (lo + hi) / 2.0,
        _ => hi,
    };
    match &el.kind {
        ElementKind::Rect { min, max } | ElementKind::Image { min, max, .. } => {
            let mut out = Vec::new();
            for sy in [-1i8, 0, 1] {
                for sx in [-1i8, 0, 1] {
                    if sx != 0 || sy != 0 {
                        out.push((Handle::Side(sx, sy), DVec2::new(pick(sx, min.x, max.x), pick(sy, min.y, max.y))));
                    }
                }
            }
            out
        }
        ElementKind::Line { from, to } => vec![(Handle::End(0), *from), (Handle::End(1), *to)],
        ElementKind::View(View { kind: ViewKind::Base { .. }, .. }) => frame.map_or_else(Vec::new, |(lo, hi)| {
            [(-1i8, -1i8), (1, -1), (1, 1), (-1, 1)]
                .into_iter()
                .map(|(sx, sy)| (Handle::Scale(sx, sy), DVec2::new(pick(sx, lo.x, hi.x), pick(sy, lo.y, hi.y))))
                .collect()
        }),
        ElementKind::Text { .. } => frame.map_or_else(Vec::new, |(_, hi)| vec![(Handle::Size, hi)]),
        ElementKind::Dimension(dim) => view_map(e, d, views, dim.view)
            .map_or_else(Vec::new, |(r, c)| vec![(Handle::Dim, crate::dims::shape(e.document(), &r, c, dim).grip)]),
        _ => Vec::new(),
    }
}

/// Box `min`–`max` with handle (`sx`, `sy`) at `p`: the sides it holds move there, the others
/// stay; `keep` (a corner): its proportions kept, the opposite corner fixed.
fn resized(min: DVec2, max: DVec2, sx: i8, sy: i8, p: DVec2, keep: bool) -> (DVec2, DVec2) {
    if keep && sx != 0 && sy != 0 {
        let o = DVec2::new(if sx < 0 { max.x } else { min.x }, if sy < 0 { max.y } else { min.y });
        let size = (max - min).max(DVec2::splat(1e-9));
        let d = p - o;
        let k = (d.x.abs() / size.x).max(d.y.abs() / size.y);
        let c = o + DVec2::new(d.x.signum() * size.x * k, d.y.signum() * size.y * k);
        return (o.min(c), o.max(c));
    }
    let (mut lo, mut hi) = (min, max);
    match sx {
        -1 => lo.x = p.x,
        1 => hi.x = p.x,
        _ => {}
    }
    match sy {
        -1 => lo.y = p.y,
        1 => hi.y = p.y,
        _ => {}
    }
    (lo.min(hi), lo.max(hi))
}

/// The standard scale nearest `s` (as a ratio: their logarithms).
fn nearest_scale(s: f64) -> f64 {
    let s = s.max(1e-9);
    SCALES.iter().copied().min_by(|a, b| (a.ln() - s.ln()).abs().total_cmp(&(b.ln() - s.ln()).abs())).unwrap_or(1.0)
}

/// The element of grip `g` with its handle at `p` (sheet millimetres); `shift` keeps a box's
/// proportions.
fn regrip(g: &Grip, p: DVec2, shift: bool) -> ElementKind {
    match (&g.kind, g.handle) {
        (ElementKind::Rect { min, max }, Handle::Side(sx, sy)) => {
            let (min, max) = resized(*min, *max, sx, sy, p, shift);
            ElementKind::Rect { min, max }
        }
        (ElementKind::Image { asset, min, max }, Handle::Side(sx, sy)) => {
            let (min, max) = resized(*min, *max, sx, sy, p, shift);
            ElementKind::Image { asset: asset.clone(), min, max }
        }
        (ElementKind::Line { to, .. }, Handle::End(0)) => ElementKind::Line { from: p, to: *to },
        (ElementKind::Line { from, .. }, Handle::End(_)) => ElementKind::Line { from: *from, to: p },
        // A view's scale: as far out as the pointer is from its centre, the nearest standard one.
        (ElementKind::View(v), Handle::Scale(..)) => match &v.kind {
            ViewKind::Base { subject, look, scale, at } => {
                let half = ((g.hi - g.lo) / 2.0).max(DVec2::splat(1e-9));
                let k = ((p.x - at.x).abs() / half.x).max((p.y - at.y).abs() / half.y);
                let kind = ViewKind::Base { subject: subject.clone(), look: look.clone(), scale: nearest_scale(scale * k), at: *at };
                ElementKind::View(View { kind, ..v.clone() })
            }
            ViewKind::Projected { .. } => g.kind.clone(),
        },
        // A text's height: its top where the pointer is, its baseline staying.
        (ElementKind::Text { at, text, size }, Handle::Size) => {
            let k = (p.y - at.y) / (g.hi.y - at.y).max(1e-9);
            let size = ((size * k).clamp(0.5, 200.0) * 10.0).round() / 10.0;
            ElementKind::Text { at: *at, text: text.clone(), size }
        }
        (kind @ ElementKind::Dimension(_), Handle::Dim) => moved_element(kind, p - g.start),
        (kind, _) => kind.clone(),
    }
}

/// An element, its box on the sheet, its handles.
type Picked = (skraft_model::Element, (DVec2, DVec2), Vec<(Handle, DVec2)>);

/// The element picked alone, its box as last drawn (sheet millimetres) and its handles —
/// none on a locked layer's (not in `frames`).
fn picked_handles(e: &Editor, st: &mut SheetState, pv: &Paperview) -> Option<Picked> {
    let el = selected_element(e, st)?.clone();
    let r = st.frames.iter().find(|(id, _)| *id == el.id)?.1;
    let frame = (pv.to_sheet(r.left_bottom()), pv.to_sheet(r.right_top()));
    let hs = handles(e, e.document().drawing(), &mut st.views, &el, Some(frame));
    Some((el, frame, hs))
}

/// The handle of the element picked alone under `p` (screen points), the nearest.
fn handle_at(e: &Editor, st: &mut SheetState, pv: &Paperview, p: egui::Pos2) -> Option<Grip> {
    let (el, (lo, hi), hs) = picked_handles(e, st, pv)?;
    let (handle, start, _) = hs
        .into_iter()
        .map(|(h, q)| (h, q, pv.to_screen(q).distance(p)))
        .filter(|(_, _, dist)| *dist <= GRIP_PT)
        .min_by(|a, b| a.2.total_cmp(&b.2))?;
    Some(Grip { element: el.id, handle, start, from: pv.to_sheet(p), kind: el.kind, lo, hi })
}

impl Grip {
    /// Where its handle goes for the pointer at `p` (sheet millimetres): as far from where it
    /// was as the pointer from where it took it.
    fn target(&self, p: DVec2) -> DVec2 {
        self.start + (p - self.from)
    }

    /// Does its handle snap (a box's side or corner, a line's end)? The point the line's other
    /// end is, to align with.
    fn snaps(&self) -> Option<Option<DVec2>> {
        match (&self.kind, self.handle) {
            (_, Handle::Side(..)) => Some(None),
            (ElementKind::Line { to, .. }, Handle::End(0)) => Some(Some(*to)),
            (ElementKind::Line { from, .. }, Handle::End(_)) => Some(Some(*from)),
            _ => None,
        }
    }
}

/// Where the pointer aims on a sheet (docs/ui/drawing.md#snapping).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Aim {
    /// The point, sheet millimetres.
    at: DVec2,
    /// Snapped on a point (an element's corner or end, a view's vertex or frame corner).
    point: bool,
    /// The point it is aligned with along the sheet's x (the same y), along its y (the same x).
    along_x: Option<DVec2>,
    along_y: Option<DVec2>,
}

/// The points of sheet `sheet` a point snaps on, and those it aligns with: rectangles' and
/// images' corners, lines' ends, views' frame corners (both); views' vertices (snapped on only:
/// so many would align everywhere); not `skip`'s, nor a hidden layer's.
fn snap_points(e: &Editor, d: &Drawing, views: &mut ViewsCache, sheet: Id, skip: Option<Id>) -> (Vec<DVec2>, Vec<DVec2>) {
    let (mut on, mut align) = (Vec::new(), Vec::new());
    let Some(s) = d.sheet(sheet) else { return (on, align) };
    let corners = |lo: DVec2, hi: DVec2| [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
    for el in d.elements_of(s) {
        if Some(el.id) == skip || d.layer(el.layer).is_some_and(|l| l.hidden) {
            continue;
        }
        match &el.kind {
            ElementKind::Rect { min, max } | ElementKind::Image { min, max, .. } => align.extend(corners(*min, *max)),
            ElementKind::Line { from, to } => align.extend([*from, *to]),
            ElementKind::View(_) => {
                let Some(r) = d.resolve(el.id) else { continue };
                let data = views.get(e, el.id, &r);
                let (lo, hi) = frame_of(&r, data.bx3.map(|(lo, hi)| (lo.truncate(), hi.truncate())));
                align.extend(corners(lo, hi));
                on.extend(data.snaps(e, &r).vertices.iter().map(|v| v.3));
            }
            _ => {}
        }
    }
    on.extend(align.iter().copied());
    (on, align)
}

/// Where a pointer at `p` aims (sheet millimetres, `reach` the snapping distance): the nearest
/// point within reach; else aligned, along each axis apart, with the nearest point within
/// reach of it (or `from`, where the shape started); else on the grid (step `grid`, 0: none).
fn aim_at(on: &[DVec2], align: &[DVec2], p: DVec2, reach: f64, from: Option<DVec2>, grid: f64) -> Aim {
    let nearest = on.iter().map(|q| (*q, q.distance(p))).filter(|(_, dist)| *dist <= reach).min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((q, _)) = nearest {
        return Aim { at: q, point: true, ..Aim::default() };
    }
    let mut aim = Aim { at: p, ..Aim::default() };
    let cands = || align.iter().copied().chain(from);
    // The same x (aligned along y): the nearest across, then the nearest along.
    let across = |a: f64, b: f64| (a - b).abs();
    if let Some(q) = cands()
        .filter(|q| across(q.x, p.x) <= reach)
        .min_by(|a, b| across(a.x, p.x).total_cmp(&across(b.x, p.x)).then(across(a.y, p.y).total_cmp(&across(b.y, p.y))))
    {
        aim.at.x = q.x;
        aim.along_y = Some(q);
    } else if grid > 0.0 {
        aim.at.x = (p.x / grid).round() * grid;
    }
    if let Some(q) = cands()
        .filter(|q| across(q.y, p.y) <= reach)
        .min_by(|a, b| across(a.y, p.y).total_cmp(&across(b.y, p.y)).then(across(a.x, p.x).total_cmp(&across(b.x, p.x))))
    {
        aim.at.y = q.y;
        aim.along_x = Some(q);
    } else if grid > 0.0 {
        aim.at.y = (p.y / grid).round() * grid;
    }
    aim
}

/// Where the pointer at `p` (sheet millimetres) aims for the shape being drawn or the handle
/// being dragged; `lock` (Shift, a line): its direction from where it started kept horizontal
/// or vertical, the nearer.
#[allow(clippy::too_many_arguments)]
fn aim_for(e: &Editor, st: &mut SheetState, sheet: Id, zoom: f64, p: DVec2, skip: Option<Id>, from: Option<DVec2>, lock: bool) -> Aim {
    let d = e.document().drawing();
    let grid = d.sheet(sheet).map_or(0.0, |s| s.grid);
    let (on, align) = snap_points(e, d, &mut st.views, sheet, skip);
    let reach = SNAP_PT as f64 / zoom;
    match from {
        Some(f) if lock => {
            // Along the axis through its start: only the other coordinate snaps.
            let horizontal = (p.x - f.x).abs() >= (p.y - f.y).abs();
            let mut aim = aim_at(&[], &align, p, reach, None, grid);
            if horizontal {
                aim.at.y = f.y;
                aim.along_x = Some(f);
            } else {
                aim.at.x = f.x;
                aim.along_y = Some(f);
            }
            aim
        }
        _ => aim_at(&on, &align, p, reach, from, grid),
    }
}

/// How near a point the pointer snaps on it (points).
const SNAP_PT: f32 = 8.0;

/// An aim's marks: the point snapped on circled, the alignments dotted — along the sheet's x
/// red, along its y green, as the 3D view's axes.
fn draw_aim(painter: &egui::Painter, pv: &Paperview, aim: &Aim) {
    let at = pv.to_screen(aim.at);
    if aim.point {
        painter.circle_stroke(at, 5.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 120, 30)));
    }
    for (q, color) in [(aim.along_x, egui::Color32::from_rgb(210, 50, 50)), (aim.along_y, egui::Color32::from_rgb(40, 160, 60))] {
        if let Some(q) = q {
            painter.extend(egui::Shape::dashed_line(&[pv.to_screen(q), at], egui::Stroke::new(1.0, color), 3.0, 3.0));
        }
    }
}

impl SheetState {
    /// The sheet shown: `current` if it still is, else the first.
    pub fn shown<'a>(&self, d: &'a Drawing) -> Option<&'a Sheet> {
        self.current.and_then(|id| d.sheet(id)).or_else(|| d.sheets.first())
    }

    /// Frames the sheet whole at the next frame.
    pub fn refit(&mut self) {
        self.fitted = None;
    }

    /// The status bar's hint on the sheet (docs/ui/drawing.md#placing-views): what the tool
    /// does at its step.
    pub fn status(&self) -> String {
        let back = || t!("drawing.hint.back");
        match self.tool {
            SheetTool::Select if self.grip.is_some() => t!("drawing.hint.grip"),
            SheetTool::Select => match (self.marquee, self.pointer) {
                (Some(from), Some(p)) if p.x < from.x => t!("tool.select.hint.crossing"),
                (Some(_), _) => t!("tool.select.hint.window"),
                _ if self.moving.is_some() => t!("drawing.hint.moving"),
                _ => t!("drawing.hint.select"),
            },
            SheetTool::Project { .. } => t!("drawing.hint.project"),
            SheetTool::Text => t!("drawing.hint.text"),
            SheetTool::Rect if self.sketch.is_some() => t!("drawing.hint.rect_second"),
            SheetTool::Line if self.sketch.is_some() => t!("drawing.hint.line_second"),
            SheetTool::Rect => t!("drawing.hint.rect"),
            SheetTool::Line => t!("drawing.hint.line"),
            SheetTool::Linear => match self.picks.len() {
                0 => t!("drawing.hint.linear_first"),
                1 => format!("{} · {}", t!("drawing.hint.linear_second"), back()),
                _ => format!("{} · {}", t!("drawing.hint.linear_place"), back()),
            },
            SheetTool::Radius | SheetTool::Diameter => match self.picks.len() {
                0 => t!("drawing.hint.circle_pick"),
                _ => format!("{} · {}", t!("drawing.hint.dim_place"), back()),
            },
            SheetTool::Angle => match self.picks.len() {
                0 => t!("drawing.hint.angle_first"),
                1 => format!("{} · {}", t!("drawing.hint.angle_second"), back()),
                _ => format!("{} · {}", t!("drawing.hint.dim_place"), back()),
            },
        }
    }

    /// `tool` started, what the last one was doing dropped.
    pub fn set_tool(&mut self, tool: SheetTool) {
        self.tool = tool;
        self.picks.clear();
        self.sketch = None;
    }
}

/// The box of a view's subject seen in its frame, in model units: sheet x, y before scaling, and
/// depth (along its forward); its parts' exact boxes' corners, projected. None while nothing is
/// known of them.
pub fn subject_box3(e: &Editor, r: &Resolved<'_>) -> Option<(DVec3, DVec3)> {
    box_of(e, r, &e.document().subject_parts(r.subject))
}

/// The box of `placed` seen in `r`'s frame.
fn box_of(e: &Editor, r: &Resolved<'_>, placed: &[skraft_model::PlacedPart]) -> Option<(DVec3, DVec3)> {
    let mut b: Option<(DVec3, DVec3)> = None;
    for p in placed {
        let Some((lo, hi)) = e.scene().bounds_of(&p.shape) else { continue };
        for i in 0..8 {
            let c = DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let q = r.frame.project(p.transform.transform_point3(c));
            b = Some(b.map_or((q, q), |(a, z)| (a.min(q), z.max(q))));
        }
    }
    b
}

/// [`subject_box3`] on the sheet's plane.
pub fn subject_box(e: &Editor, r: &Resolved<'_>) -> Option<(DVec2, DVec2)> {
    subject_box3(e, r).map(|(lo, hi)| (lo.truncate(), hi.truncate()))
}

/// A view's frame on the sheet, in millimetres: its subject's box (`bx`, seen in its frame) at
/// its scale, centred on where it is placed; a small square while the box is not known.
pub fn frame_of(r: &Resolved<'_>, bx: Option<(DVec2, DVec2)>) -> (DVec2, DVec2) {
    let half = bx.map_or(DVec2::splat(10.0), |(lo, hi)| (hi - lo) * r.scale / 2.0);
    (r.at - half, r.at + half)
}

/// [`frame_of`] its subject's box.
pub fn view_frame(e: &Editor, r: &Resolved<'_>) -> (DVec2, DVec2) {
    frame_of(r, subject_box(e, r))
}

/// A part placed in a view's subject: its transform there, its material.
pub type Placed = (DAffine3, Option<Id>);

/// A view's picture the 3D renderer is asked for: its subject's parts seen in its frame (the
/// *Rendered* and *Hybrid* styles) and its lines over them (*Lines*, *Hybrid*), framed exactly
/// on its box (what its frame on the sheet shows), at `size` pixels; drawn again when `stamp`
/// changes.
pub struct Wanted {
    pub view: Id,
    pub size: [u32; 2],
    /// Subject axes to eye, and the projection (reversed depth).
    pub camera: (DMat4, glam::Mat4),
    /// Per part (its definition): its placements in the subject's axes and their materials.
    pub parts: Vec<(Id, Vec<Placed>)>,
    pub lines: Option<PictureLines>,
    pub stamp: u64,
}

/// A view's lines in its picture: in its frame (`right`, `up`, model units), laid at `depth`
/// along `forward`; the widths and dashes in points.
pub struct PictureLines {
    pub lines: Arc<skraft_tools::hidden_lines::ViewLines>,
    pub axes: [DVec3; 3],
    pub depth: f64,
    /// The hidden ones drawn too (*Lines*), dashed.
    pub hidden: bool,
    pub width: f32,
    pub hidden_width: f32,
    pub dash: [f32; 2],
}

/// Line widths on paper (millimetres): visible, hidden; the hidden lines' dash and gap.
const LINE_MM: f64 = 0.35;
const HIDDEN_MM: f64 = 0.18;
const DASH_MM: [f64; 2] = [2.0, 1.0];

/// The largest side of a view's picture, in pixels.
const MAX_PICTURE: f64 = 4096.0;

/// The camera seeing the box `(lo, hi)` (in `r`'s frame: x, y, depth) exactly across, parallel.
fn picture_camera(r: &Resolved<'_>, (lo, hi): (DVec3, DVec3)) -> (DMat4, glam::Mat4) {
    let f = &r.frame;
    let c = (lo + hi) / 2.0;
    let half = ((hi - lo) / 2.0).max(DVec3::splat(1e-3));
    let centre = f.right * c.x + f.up * c.y + f.forward * c.z;
    let back = half.z + half.x.max(half.y) + 1.0;
    let eye = centre - f.forward * back;
    let view = DMat4::look_to_rh(eye, f.forward, f.up);
    let (near, far) = ((back - half.z * 1.05 - 1.0).max(1e-3), back + half.z * 1.05 + 1.0);
    // Reversed depth: near → 1, far → 0.
    let proj = glam::Mat4::orthographic_rh(-half.x as f32, half.x as f32, -half.y as f32, half.y as f32, far as f32, near as f32);
    (view, proj)
}

/// A side of a picture in pixels, in steps of a fifth (a zoom redraws it only past a step).
fn picture_side(px: f64) -> u32 {
    let step = 1.2f64;
    let q = step.powf((px.max(8.0).ln() / step.ln()).round());
    q.min(MAX_PICTURE).round() as u32
}

/// The picture view `id` (`style`, showing `data`) asks for, its frame `rect` on screen, its
/// lines (`lines`, as last computed) over its parts.
fn wanted_picture(
    id: Id,
    style: ViewStyle,
    r: &Resolved<'_>,
    data: &ViewData,
    lines: Option<Arc<skraft_tools::hidden_lines::ViewLines>>,
    rect: egui::Rect,
    ppp: f32,
) -> Option<Wanted> {
    let b = data.bx3?;
    let size = [picture_side((rect.width() * ppp) as f64), picture_side((rect.height() * ppp) as f64)];
    // Points per paper millimetre in the picture.
    let frame_mm = (b.1.x - b.0.x).max(1e-9) * r.scale;
    let ppm = size[0] as f64 / ppp as f64 / frame_mm;
    let f = &r.frame;
    let lines = lines.filter(|_| style != ViewStyle::Rendered).map(|l| PictureLines {
        lines: l,
        axes: [f.right, f.up, f.forward],
        // In front of every part.
        depth: b.0.z - 1.0,
        hidden: style == ViewStyle::Lines,
        width: (LINE_MM * ppm).max(1.0) as f32,
        hidden_width: (HIDDEN_MM * ppm).max(0.6) as f32,
        dash: [(DASH_MM[0] * ppm).max(3.0) as f32, (DASH_MM[1] * ppm).max(2.0) as f32],
    });
    let parts = if style == ViewStyle::Lines { Vec::new() } else { data.parts.clone() };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (e_key(r), size, id.0, style as u8, lines.as_ref().map(|l| Arc::as_ptr(&l.lines) as usize)).hash(&mut h);
    data.parts.len().hash(&mut h);
    for (p, places) in &data.parts {
        p.0.hash(&mut h);
        for (t, m) in places {
            t.to_cols_array().map(f64::to_bits).hash(&mut h);
            m.map(|m| m.0).hash(&mut h);
        }
    }
    Some(Wanted { view: id, size, camera: picture_camera(r, b), parts, lines, stamp: h.finish() })
}

/// What a view puts on paper (the PDF's, docs/ui/drawing.md#pdf): its frame (sheet
/// millimetres), its picture asked for (its parts, no lines: *Rendered*, *Hybrid*), its lines
/// computed now (*Lines*, *Hybrid*) in its frame, the centre of its box there.
pub struct Printed {
    pub frame: (DVec2, DVec2),
    pub picture: Option<Wanted>,
    pub lines: Option<skraft_tools::hidden_lines::ViewLines>,
    pub centre: DVec2,
}

/// What view `id` (`r`, drawn in `style`) puts on paper at `dpi`; none while its box is not
/// known.
pub fn printed(e: &Editor, id: Id, style: ViewStyle, r: &Resolved<'_>, dpi: f64) -> Option<Printed> {
    let placed = e.document().subject_parts(r.subject);
    let b = box_of(e, r, &placed)?;
    let frame = frame_of(r, Some((b.0.truncate(), b.1.truncate())));
    let picture = (style != ViewStyle::Lines).then(|| {
        let px = |mm: f64| (mm * dpi / 25.4).round().clamp(8.0, 8192.0) as u32;
        let mut by_part: HashMap<Id, Vec<Placed>> = HashMap::new();
        for p in &placed {
            by_part.entry(p.part).or_default().push((p.transform, p.material));
        }
        let mut parts: Vec<_> = by_part.into_iter().collect();
        parts.sort_by_key(|(p, _)| *p);
        let size = [px(frame.1.x - frame.0.x), px(frame.1.y - frame.0.y)];
        Wanted { view: id, size, camera: picture_camera(r, b), parts, lines: None, stamp: 0 }
    });
    let lines = if style == ViewStyle::Rendered {
        None
    } else {
        crate::view_lines::LinesJob::of(e, r, &placed).map(crate::view_lines::LinesJob::run)
    };
    Some(Printed { frame, picture, lines, centre: ((b.0 + b.1) / 2.0).truncate() })
}

/// What of a view's frame changes its picture (its direction).
fn e_key(r: &Resolved<'_>) -> [u64; 9] {
    let f = &r.frame;
    let mut k = [0u64; 9];
    for (i, v) in [f.right, f.up, f.forward].iter().enumerate() {
        k[3 * i..3 * i + 3].copy_from_slice(&v.to_array().map(f64::to_bits));
    }
    k
}

/// The largest standard scale showing a subject of `size` (model units) within `room` (paper
/// millimetres); the smallest when even that is too large.
fn scale_to_fit(size: DVec2, room: DVec2) -> f64 {
    let fit = (room.x / size.x.max(1e-9)).min(room.y / size.y.max(1e-9));
    SCALES.iter().copied().find(|s| *s <= fit).unwrap_or(*SCALES.last().unwrap())
}

/// What a new view of the editor's selection shows: the part open, else the one occurrence
/// selected (its component, in its own axes), else the occurrences selected where they stand,
/// else the model.
fn subject_of_selection(e: &Editor) -> Subject {
    if let Some(part) = e.open_part() {
        return Subject::Component(part);
    }
    let paths: Vec<EntityPath> = e.selection().iter().map(|id| EntityPath::new(e.context().to_vec(), *id)).collect();
    match paths.as_slice() {
        [] => Subject::Model,
        [one] => e.document().resolve(one).map_or(Subject::Model, |o| Subject::Component(o.component)),
        _ => Subject::Occurrences(paths),
    }
}

/// A new sheet, A3 landscape, shown.
pub fn new_sheet(e: &mut Editor, st: &mut SheetState) {
    if let Ok(Some(o)) = e.apply(Operation::AddSheet { name: None, paper: Paper::A3 }) {
        st.current = o.sheet;
    }
}

/// A view from the front of what is selected (or the model), at the largest standard scale
/// fitting half the paper, at the sheet's centre.
pub fn add_view(e: &mut Editor, st: &mut SheetState) -> Result<(), String> {
    let sheet = st.shown(e.document().drawing()).ok_or_else(|| t!("drawing.no_sheet"))?;
    let (sheet_id, paper) = (sheet.id, sheet.paper);
    let n = sheet.elements.iter().filter(|e| matches!(e.kind, ElementKind::View(_))).count() + 1;
    let subject = subject_of_selection(e);
    let look = Look::Standard(Direction::Front);
    let (frame, perspective) = look.frame();
    let at = paper.size() / 2.0;
    // Its scale from its box, seen at scale 1.
    let probe = Resolved { subject: &subject, frame, perspective, scale: 1.0, at };
    let scale = subject_box(e, &probe).map_or(1.0, |(lo, hi)| scale_to_fit(hi - lo, paper.size() / 2.0));
    let view = View { name: t!("drawing.view_name", n = n), kind: ViewKind::Base { subject, look, scale, at }, style: ViewStyle::Lines };
    e.apply(Operation::AddElement { at: Placing { sheet: sheet_id, layer: None }, element: ElementKind::View(view) }).map(|_| ())
}

/// The view picked on the sheet shown, if it is still there.
pub fn selected_view<'a>(e: &'a Editor, st: &SheetState) -> Option<(Id, &'a View)> {
    selected_element(e, st).and_then(|el| match &el.kind {
        ElementKind::View(v) => Some((el.id, v)),
        _ => None,
    })
}

/// Can a view be projected from the view picked (a parallel one)?
pub fn can_project(e: &Editor, st: &SheetState) -> bool {
    selected_view(e, st).is_some_and(|(id, _)| e.document().drawing().resolve(id).is_some_and(|r| r.perspective.is_none()))
}

/// « Vue projetée » from the view picked.
pub fn project_view(e: &Editor, st: &mut SheetState) -> Result<(), String> {
    if !can_project(e, st) {
        return Err(t!("drawing.no_view_selected"));
    }
    let base = selected_view(e, st).map_or(Id(0), |(id, _)| id);
    st.set_tool(SheetTool::Project { base });
    Ok(())
}

/// Deletes the elements picked (a view: with the views projected from it).
pub fn delete_selected(e: &mut Editor, st: &mut SheetState) -> Result<(), String> {
    let elements: Vec<Id> = selection(e, st).iter().map(|el| el.id).collect();
    if elements.is_empty() {
        return Err(t!("drawing.no_view_selected"));
    }
    st.selected.clear();
    e.apply(Operation::DeleteElements { elements }).map(|_| ())
}

/// The element `kind` moved by `offset` on the sheet.
fn moved_element(kind: &ElementKind, offset: DVec2) -> ElementKind {
    match kind {
        ElementKind::View(v) => ElementKind::View(View { kind: moved(&v.kind, offset), ..v.clone() }),
        ElementKind::Text { at, text, size } => ElementKind::Text { at: *at + offset, text: text.clone(), size: *size },
        ElementKind::Line { from, to } => ElementKind::Line { from: *from + offset, to: *to + offset },
        ElementKind::Rect { min, max } => ElementKind::Rect { min: *min + offset, max: *max + offset },
        ElementKind::Image { asset, min, max } => ElementKind::Image { asset: asset.clone(), min: *min + offset, max: *max + offset },
        // A dimension's line moved across itself, its text or arc with the pointer.
        ElementKind::Dimension(dim) => {
            let kind = match &dim.kind {
                DimKind::Linear { a, b, orient, offset: o } => {
                    let across = match orient {
                        Orient::Vertical => offset.x,
                        _ => offset.y,
                    };
                    DimKind::Linear { a: a.clone(), b: b.clone(), orient: *orient, offset: o + across }
                }
                DimKind::Radius { edge, diameter, at } => DimKind::Radius { edge: edge.clone(), diameter: *diameter, at: *at + offset },
                DimKind::Angle { a, b, at } => DimKind::Angle { a: a.clone(), b: b.clone(), at: *at + offset },
            };
            ElementKind::Dimension(Dimension { view: dim.view, kind })
        }
    }
}

/// Dimension `dim` as drawn now, its view's box computed afresh (the PDF's).
pub fn dim_shape(e: &Editor, dim: &Dimension) -> Option<crate::dims::DimShape> {
    let d = e.document().drawing();
    let r = d.resolve(dim.view)?;
    let (lo, hi) = subject_box3(e, &r)?;
    Some(crate::dims::shape(e.document(), &r, ((lo + hi) / 2.0).truncate(), dim))
}

/// How a view maps its model onto the sheet: its resolution and its box's centre.
fn view_map<'a>(e: &Editor, d: &'a Drawing, views: &mut ViewsCache, view: Id) -> Option<(Resolved<'a>, DVec2)> {
    let r = d.resolve(view)?;
    let centre = views.get(e, view, &r).centre()?;
    Some((r, centre))
}

/// A point of a view's subject on the sheet.
fn on_sheet(r: &Resolved<'_>, centre: DVec2, p: DVec3) -> DVec2 {
    r.at + (r.frame.project(p).truncate() - centre) * r.scale
}

/// The distance from `p` to the polyline `line`, and the point of it there (index of its
/// segment, parameter along it).
fn near_polyline(line: &[DVec2], p: DVec2) -> (f64, usize, f64) {
    let mut best = (f64::MAX, 0, 0.0);
    for (i, w) in line.windows(2).enumerate() {
        let ab = w[1] - w[0];
        let t = ((p - w[0]).dot(ab) / ab.length_squared().max(1e-300)).clamp(0.0, 1.0);
        let d = (w[0] + ab * t).distance(p);
        if d < best.0 {
            best = (d, i, t);
        }
    }
    best
}

/// What a dimension's tool picks at `p` (sheet millimetres) within `reach`: a vertex
/// (linear) or an edge (a circle's: radius, diameter; a straight one: angle) of a view on
/// sheet `sheet` (`only`: that view's); its view, its anchor, and where it is on the sheet.
#[allow(clippy::too_many_arguments)]
fn pick_at(
    e: &Editor,
    d: &Drawing,
    views: &mut ViewsCache,
    sheet: Id,
    tool: SheetTool,
    p: DVec2,
    reach: f64,
    only: Option<Id>,
) -> Option<(Id, skraft_model::Anchor, Vec<DVec2>)> {
    use skraft_model::kernel::topo::EdgeShape;
    let s = d.sheet(sheet)?;
    let ids: Vec<Id> = d
        .elements_of(s)
        .filter(|el| matches!(el.kind, ElementKind::View(_)) && d.layer(el.layer).is_none_or(|l| !l.hidden))
        .map(|el| el.id)
        .filter(|id| only.is_none_or(|o| o == *id))
        .collect();
    let mut best: Option<(f64, Id, skraft_model::Anchor, Vec<DVec2>)> = None;
    for id in ids {
        let Some(r) = d.resolve(id) else { continue };
        let data = views.get(e, id, &r);
        let snaps = data.snaps(e, &r);
        if tool == SheetTool::Linear {
            for (k, v, q, at) in &snaps.vertices {
                let dist = at.distance(p);
                if dist <= reach && best.as_ref().is_none_or(|b| dist < b.0) {
                    let Some(anchor) = e.document().anchor(&data.placed[*k], Entity::Vertex, *v, *q) else { continue };
                    best = Some((dist, id, anchor, vec![*at]));
                }
            }
        } else {
            for (k, i, shape, local, line) in &snaps.edges {
                let fits = match shape {
                    EdgeShape::Circle { normal, .. } => {
                        matches!(tool, SheetTool::Radius | SheetTool::Diameter) && normal.dot(r.frame.forward).abs() > 0.999
                    }
                    EdgeShape::Line { .. } => tool == SheetTool::Angle,
                    EdgeShape::Other { .. } => false,
                };
                if !fits {
                    continue;
                }
                let (dist, seg, t) = near_polyline(line, p);
                if dist <= reach && best.as_ref().is_none_or(|b| dist < b.0) {
                    let at = local[seg].lerp(local[(seg + 1).min(local.len() - 1)], t);
                    let Some(anchor) = e.document().anchor(&data.placed[*k], Entity::Edge, *i, at) else { continue };
                    best = Some((dist, id, anchor, line.clone()));
                }
            }
        }
    }
    best.map(|(_, id, a, m)| (id, a, m))
}

/// Along what and how far off a linear dimension between `sa` and `sb` goes for the pointer
/// at `p` (sheet millimetres): past the points' span across → horizontal (or vertical);
/// within it → aligned.
fn linear_place(sa: DVec2, sb: DVec2, p: DVec2) -> (Orient, f64) {
    let (lo, hi) = (sa.min(sb), sa.max(sb));
    let mid = (sa + sb) / 2.0;
    let out_y = (p.y - hi.y).max(lo.y - p.y);
    let out_x = (p.x - hi.x).max(lo.x - p.x);
    if out_y > 0.0 && out_y >= out_x {
        (Orient::Horizontal, p.y - mid.y)
    } else if out_x > 0.0 {
        (Orient::Vertical, p.x - mid.x)
    } else {
        let n = (sb - sa).normalize_or(DVec2::X).perp();
        (Orient::Aligned, (p - sa).dot(n))
    }
}

/// The dimension the picks and the pointer at `p` make (none while it needs more picks).
fn dimension_of(
    e: &Editor,
    d: &Drawing,
    views: &mut ViewsCache,
    tool: SheetTool,
    picks: &[(Id, skraft_model::Anchor)],
    p: DVec2,
) -> Option<Dimension> {
    let (view, _) = picks.first()?;
    let (r, centre) = view_map(e, d, views, *view)?;
    let found = |a: &skraft_model::Anchor| e.document().find(r.subject, a);
    let kind = match (tool, picks) {
        (SheetTool::Linear, [(_, a), (_, b)]) => {
            let (sa, sb) = (on_sheet(&r, centre, found(a)?.point()), on_sheet(&r, centre, found(b)?.point()));
            let (orient, offset) = linear_place(sa, sb, p);
            DimKind::Linear { a: a.clone(), b: b.clone(), orient, offset }
        }
        (SheetTool::Radius | SheetTool::Diameter, [(_, edge)]) => {
            let c = on_sheet(&r, centre, found(edge)?.point());
            DimKind::Radius { edge: edge.clone(), diameter: tool == SheetTool::Diameter, at: p - c }
        }
        (SheetTool::Angle, [(_, a), (_, b)]) => {
            let line = |x: &skraft_model::Anchor| match found(x)? {
                Found::Edge(skraft_model::kernel::topo::EdgeShape::Line { a, b }) => {
                    Some((on_sheet(&r, centre, a), on_sheet(&r, centre, b)))
                }
                _ => None,
            };
            let ((p1, q1), (p2, q2)) = (line(a)?, line(b)?);
            let (d1, d2) = ((q1 - p1).normalize_or(DVec2::X), (q2 - p2).normalize_or(DVec2::Y));
            let den = d1.perp_dot(d2);
            let o = if den.abs() < 1e-9 { p1 } else { p1 + d1 * ((p2 - p1).perp_dot(d2) / den) };
            DimKind::Angle { a: a.clone(), b: b.clone(), at: p - o }
        }
        _ => return None,
    };
    Some(Dimension { view: *view, kind })
}

/// How many picks a dimension's tool takes before its last click places it.
fn picks_needed(tool: SheetTool) -> usize {
    match tool {
        SheetTool::Radius | SheetTool::Diameter => 1,
        _ => 2,
    }
}

/// A dimension's shape drawn on the sheet: its lines, arrows and text (red when an orphan);
/// its box on screen.
fn draw_dim(painter: &egui::Painter, pv: &Paperview, s: &crate::dims::DimShape) -> egui::Rect {
    let color = if s.orphan { egui::Color32::from_rgb(210, 40, 40) } else { egui::Color32::from_gray(25) };
    let stroke = egui::Stroke::new((0.18 * pv.zoom).max(1.0) as f32, color);
    for [a, b] in &s.lines {
        painter.line_segment([pv.to_screen(*a), pv.to_screen(*b)], stroke);
    }
    for tri in &s.arrows {
        painter.add(egui::Shape::convex_polygon(tri.iter().map(|q| pv.to_screen(*q)).collect(), color, egui::Stroke::NONE));
    }
    let font = egui::FontId::proportional((crate::dims::TEXT_MM * pv.zoom).max(1.0) as f32);
    let galley = painter.layout_no_wrap(s.text.clone(), font, color);
    // Turned about its middle; the screen's y down.
    let angle = -s.text_angle as f32;
    let rot = egui::emath::Rot2::from_angle(angle);
    let centre = pv.to_screen(s.text_at);
    let pos = centre - rot * (galley.size() / 2.0);
    painter.add(egui::epaint::TextShape::new(pos, galley, color).with_angle(angle));
    let (lo, hi) = s.bounds();
    pv.rect(lo, hi).expand(4.0)
}

/// The view `kind` moved by `offset` on the sheet: a base view anywhere, a projected one along
/// its side only (it stays aligned).
fn moved(kind: &ViewKind, offset: DVec2) -> ViewKind {
    match kind {
        ViewKind::Base { subject, look, scale, at } => {
            ViewKind::Base { subject: subject.clone(), look: look.clone(), scale: *scale, at: *at + offset }
        }
        ViewKind::Projected { base, side, distance } => {
            ViewKind::Projected { base: *base, side: *side, distance: (distance + offset.dot(side.dir())).max(0.0) }
        }
    }
}

/// Where a view projected from `base` goes for the pointer at `p` (sheet millimetres): the
/// side it is on, past the base view's frame, and the distance between the frames' centres.
fn projection_at(e: &Editor, d: &Drawing, base: Id, p: DVec2) -> Option<(Side, f64)> {
    let r = d.resolve(base)?;
    let (lo, hi) = view_frame(e, &r);
    let half = ((hi - lo) / 2.0).max(DVec2::ONE);
    let off = p - r.at;
    if off.x.abs() <= half.x && off.y.abs() <= half.y {
        return None;
    }
    Some(if off.x.abs() / half.x >= off.y.abs() / half.y {
        (if off.x > 0.0 { Side::Right } else { Side::Left }, off.x.abs())
    } else {
        (if off.y > 0.0 { Side::Above } else { Side::Below }, off.y.abs())
    })
}

/// The style command `id` (`drawing.style_lines`, `_rendered`, `_hybrid`) stands for.
pub fn style_of(id: &str) -> Option<ViewStyle> {
    match id {
        "drawing.style_lines" => Some(ViewStyle::Lines),
        "drawing.style_rendered" => Some(ViewStyle::Rendered),
        "drawing.style_hybrid" => Some(ViewStyle::Hybrid),
        _ => None,
    }
}

/// The view picked drawn in `style`.
pub fn set_style(e: &mut Editor, st: &SheetState, style: ViewStyle) -> Result<(), String> {
    let views = selected_views(e, st);
    if views.is_empty() {
        return Err(t!("drawing.no_view_selected"));
    }
    let ops: Vec<Operation> = views
        .into_iter()
        .filter(|(_, v)| v.style != style)
        .map(|(id, v)| Operation::SetElement { element: id, kind: ElementKind::View(View { style, ..v.clone() }) })
        .collect();
    match ops.len() {
        0 => Ok(()),
        1 => e.apply(ops.into_iter().next().unwrap()).map(|_| ()),
        _ => e.apply(Operation::Batch(ops)).map(|_| ()),
    }
}

/// Sheet millimetres (y up) to screen points, and back.
#[derive(Clone, Copy)]
struct Paperview {
    centre: egui::Pos2,
    pan: DVec2,
    zoom: f64,
}

impl Paperview {
    fn to_screen(self, p: DVec2) -> egui::Pos2 {
        let d = (p - self.pan) * self.zoom;
        egui::pos2(self.centre.x + d.x as f32, self.centre.y - d.y as f32)
    }
    fn to_sheet(self, p: egui::Pos2) -> DVec2 {
        self.pan + DVec2::new((p.x - self.centre.x) as f64, (self.centre.y - p.y) as f64) / self.zoom
    }
    fn rect(self, a: DVec2, b: DVec2) -> egui::Rect {
        egui::Rect::from_two_pos(self.to_screen(a), self.to_screen(b))
    }
}

/// The height of the sheets' tabs at the stage's bottom (the side panels stop above them).
pub fn tabs_height(style: &egui::Style) -> f32 {
    style.spacing.interact_size.y + 16.0
}

/// The stage: the sheet shown on its paper over `rect`, panned (a drag) and zoomed (the
/// wheel, about the pointer; a double click frames it whole), and the sheets' tabs at the
/// bottom.
#[allow(clippy::too_many_arguments)]
pub fn ui(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    top: f32,
    e: &mut Editor,
    st: &mut SheetState,
    lines: &mut crate::view_lines::LinesCache,
    theme: &Theme,
    ready: &HashMap<Id, egui::TextureId>,
    title: &str,
) -> Vec<Wanted> {
    let mut wanted = Vec::new();
    let ink = viewport::color(theme.ui.view_text);
    ui.painter().rect_filled(rect, 0.0, viewport::color(theme.ui.view));
    crate::handles::publish(|| "sheet".into(), rect);
    let tabs_height = tabs_height(ui.style());
    let paper_room = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y - tabs_height));
    // Framed whole in what the ribbon leaves.
    let fit_room = egui::Rect::from_min_max(egui::pos2(rect.min.x, top.min(paper_room.max.y - 50.0)), paper_room.max);
    let shown = st.shown(e.document().drawing()).map(|s| (s.id, s.paper));
    if let Some((id, paper)) = shown {
        if st.fitted != Some(id) || st.zoom <= 0.0 {
            st.zoom = (fit_room.width() as f64 / (paper.width * FIT_MARGIN)).min(fit_room.height() as f64 / (paper.height * FIT_MARGIN));
            // The paper's centre at the room's.
            let shift = fit_room.center() - paper_room.center();
            st.pan = paper.size() / 2.0 - DVec2::new(shift.x as f64, -shift.y as f64) / st.zoom;
            st.fitted = Some(id);
        }
        st.current = Some(id);
        let response = ui.interact(paper_room, egui::Id::unique("sheet_stage"), egui::Sense::click_and_drag());
        let centre = paper_room.center();
        let pv = Paperview { centre, pan: st.pan, zoom: st.zoom };
        // A press with Select on an element drags the selection (it alone when it was not in
        // it), elsewhere draws a selection rectangle; with Rectangle or Line it starts the
        // shape. The middle or right button's drag pans.
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let shift = ui.input(|i| i.modifiers.shift);
        st.pointer = pointer;
        // From where the button went down, not where the drag was recognised.
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
        {
            // A handle of the element picked alone first.
            let grip = if st.tool == SheetTool::Select { handle_at(e, st, &pv, p) } else { None };
            match st.tool {
                SheetTool::Select if grip.is_some() => st.grip = grip,
                SheetTool::Select => match st.frames.iter().rev().find(|(_, r)| r.contains(p)) {
                    Some((v, _)) => {
                        if !st.selected.contains(v) {
                            if !shift {
                                st.selected.clear();
                            }
                            st.selected.push(*v);
                        }
                        st.moving = Some(pv.to_sheet(p));
                    }
                    None => st.marquee = Some(p),
                },
                // A drag from where the button went down (the first click's point, if made).
                SheetTool::Rect | SheetTool::Line => {
                    if st.sketch.is_none() {
                        st.sketch = Some(aim_for(e, st, id, pv.zoom, pv.to_sheet(p), None, None, false).at);
                    }
                }
                _ => {}
            }
        }
        // Where the pointer aims, snapped (docs/ui/drawing.md#snapping): while a shape or a text
        // is put, or a box's or a line's handle dragged; Shift keeps a line horizontal or
        // vertical.
        let aim = {
            let grip = st.grip.clone();
            let line_end = grip.as_ref().is_some_and(|g| matches!(g.handle, Handle::End(_)));
            match (pointer, grip) {
                (Some(p), Some(g)) => {
                    g.snaps().map(|from| aim_for(e, st, id, pv.zoom, g.target(pv.to_sheet(p)), Some(g.element), from, shift && line_end))
                }
                (Some(p), None) if matches!(st.tool, SheetTool::Rect | SheetTool::Line | SheetTool::Text) => {
                    let lock = shift && st.tool == SheetTool::Line && st.sketch.is_some();
                    Some(aim_for(e, st, id, pv.zoom, pv.to_sheet(p), None, st.sketch, lock))
                }
                _ => None,
            }
        };
        // A handle's point: where it aims, else as far as the pointer went.
        let grip_to = |g: &Grip, p: egui::Pos2| aim.map_or_else(|| g.target(pv.to_sheet(p)), |a| a.at);
        let offset = match (st.moving, pointer) {
            (Some(from), Some(p)) => pv.to_sheet(p) - from,
            _ => DVec2::ZERO,
        };
        if response.dragged_by(egui::PointerButton::Middle) || response.dragged_by(egui::PointerButton::Secondary) {
            let d = response.drag_delta();
            st.pan -= DVec2::new(d.x as f64, -d.y as f64) / st.zoom;
        }
        // A handle let go: its element changed (one `SetElement`).
        if response.drag_stopped()
            && let Some(g) = st.grip.take()
            && let Some(p) = pointer
        {
            let kind = regrip(&g, grip_to(&g, p), shift);
            if kind != g.kind {
                let _ = e.apply(Operation::SetElement { element: g.element, kind });
            }
        }
        if response.drag_stopped() && st.moving.take().is_some() && offset != DVec2::ZERO {
            let mut ops: Vec<Operation> = moves(e.document().drawing(), &st.selected, offset)
                .into_iter()
                .map(|(element, kind)| Operation::SetElement { element, kind })
                .collect();
            let _ = match ops.len() {
                0 => Ok(None),
                1 => e.apply(ops.remove(0)),
                _ => e.apply(Operation::Batch(ops)),
            };
        }
        // The selection rectangle: left to right the elements it holds whole, right to left
        // those it touches (docs/tools/interaction.md#selection); Shift adds or removes them.
        if response.drag_stopped()
            && let Some(from) = st.marquee.take()
            && let Some(p) = pointer
        {
            let r = egui::Rect::from_two_pos(from, p);
            let window = p.x >= from.x;
            let hit: Vec<Id> =
                st.frames.iter().filter(|(_, f)| if window { r.contains_rect(*f) } else { r.intersects(*f) }).map(|(id, _)| *id).collect();
            if shift {
                for id in hit {
                    match st.selected.iter().position(|x| *x == id) {
                        Some(i) => {
                            st.selected.remove(i);
                        }
                        None => st.selected.push(id),
                    }
                }
            } else {
                st.selected = hit;
            }
        }
        // A rectangle or a line, on the active layer: two clicks (its corners, its ends) or a
        // drag, as the 3D view's shapes (docs/tools/interaction.md); under 0.5 mm, the second
        // point still to come.
        if matches!(st.tool, SheetTool::Rect | SheetTool::Line)
            && let Some(p) = pointer
            && (response.clicked() || response.drag_stopped_by(egui::PointerButton::Primary))
        {
            let to = aim.map_or_else(|| pv.to_sheet(p), |a| a.at);
            match st.sketch {
                None => st.sketch = Some(to),
                Some(from) if from.distance(to) > 0.5 => {
                    st.sketch = None;
                    let element = match st.tool {
                        SheetTool::Rect => ElementKind::Rect { min: from.min(to), max: from.max(to) },
                        _ => ElementKind::Line { from, to },
                    };
                    let layer = active_layer(e.document().drawing(), st);
                    if let Ok(Some(o)) = e.apply(Operation::AddElement { at: Placing { sheet: id, layer }, element }) {
                        st.selected = o.element.into_iter().collect();
                    }
                }
                Some(_) => {}
            }
        }
        if response.double_clicked() && st.tool == SheetTool::Select {
            st.refit();
        }
        if let Some(at) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll as f64 * 0.002).exp() * pinch as f64;
            if factor != 1.0 {
                let under = pv.to_sheet(at);
                st.zoom = (st.zoom * factor).clamp(0.05, 200.0);
                // The point under the pointer stays there.
                st.pan = under - DVec2::new((at.x - centre.x) as f64, (centre.y - at.y) as f64) / st.zoom;
            }
        }
        // Esc goes back one step of the tool: the shape or rectangle drawn dropped, the picks;
        // with Select the selection cleared; a tool stays (Space, `tool.select`, ends it), as in
        // the 3D view (docs/tools/interaction.md#selection). Not while a field is typed in.
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && ui.ctx().memory(|m| m.focused().is_none()) {
            if st.sketch.take().is_some() || st.marquee.take().is_some() || st.moving.take().is_some() || st.grip.take().is_some() {
            } else if !st.picks.is_empty() {
                st.picks.clear();
            } else if st.tool == SheetTool::Select {
                st.selected.clear();
            }
        }
        let pv = Paperview { centre, pan: st.pan, zoom: st.zoom };
        let painter = ui.painter().with_clip_rect(paper_room);
        let page = pv.rect(DVec2::ZERO, paper.size());
        painter.rect_filled(page.translate(egui::vec2(3.0, 4.0)), 0.0, egui::Color32::from_black_alpha(40));
        painter.rect_filled(page, 0.0, egui::Color32::WHITE);
        painter.rect_stroke(page, 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(160)), egui::StrokeKind::Outside);
        // Its grid, faint, while its lines are 6 points apart at least (not printed).
        let grid = e.document().drawing().sheet(id).map_or(0.0, |s| s.grid);
        if grid > 0.0 && grid * pv.zoom >= 6.0 {
            let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(232));
            let clip = painter.clip_rect().intersect(page);
            let (lo, hi) = (pv.to_sheet(clip.left_bottom()), pv.to_sheet(clip.right_top()));
            let mut x = (lo.x / grid).ceil() * grid;
            while x <= hi.x {
                let sx = pv.to_screen(DVec2::new(x, 0.0)).x;
                painter.line_segment([egui::pos2(sx, clip.top()), egui::pos2(sx, clip.bottom())], stroke);
                x += grid;
            }
            let mut y = (lo.y / grid).ceil() * grid;
            while y <= hi.y {
                let sy = pv.to_screen(DVec2::new(0.0, y)).y;
                painter.line_segment([egui::pos2(clip.left(), sy), egui::pos2(clip.right(), sy)], stroke);
                y += grid;
            }
        }
        // The element dragged drawn where it goes (a view's projections with it).
        let preview;
        let gripped = match (&st.grip, pointer) {
            (Some(g), Some(p)) => Some((g.element, regrip(g, grip_to(g, p), shift))),
            _ => None,
        };
        let drawing = match st.moving {
            _ if let Some((id, kind)) = gripped => {
                let mut d = e.document().drawing().clone();
                if let Some(el) = d.element_mut(id) {
                    el.kind = kind;
                }
                preview = d;
                &preview
            }
            Some(_) if offset != DVec2::ZERO => {
                let mut d = e.document().drawing().clone();
                for (v, kind) in moves(&d, &st.selected, offset) {
                    if let Some(el) = d.element_mut(v) {
                        el.kind = kind;
                    }
                }
                preview = d;
                &preview
            }
            _ => e.document().drawing(),
        };
        let fields = drawing.sheet(id).map(|s| Fields {
            document: title,
            sheet: &s.name,
            number: drawing.sheets.iter().position(|x| x.id == id).map_or(0, |i| i + 1),
            count: drawing.sheets.len(),
            scale: drawing.elements_of(s).find_map(|el| match &el.kind {
                ElementKind::View(View { kind: ViewKind::Base { scale, .. }, .. }) => Some(*scale),
                _ => None,
            }),
        });
        st.frames = draw_elements(&painter, &pv, e, drawing, id, &st.selected, ready, lines, &mut st.views, &mut wanted, fields.as_ref());
        // The handles of the element picked alone (Select), as it is drawn; the pointer over one
        // shows what it does.
        if st.tool == SheetTool::Select && st.moving.is_none() && st.marquee.is_none() {
            let shown = match &st.grip {
                Some(g) => drawing.element(g.element).cloned().map(|el| {
                    let frame = st
                        .frames
                        .iter()
                        .find(|(id, _)| *id == el.id)
                        .map(|(_, r)| (pv.to_sheet(r.left_bottom()), pv.to_sheet(r.right_top())));
                    handles(e, drawing, &mut st.views, &el, frame)
                }),
                None => picked_handles(e, st, &pv).map(|(_, _, hs)| hs),
            };
            let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 120, 200));
            for (h, q) in shown.unwrap_or_default() {
                let at = pv.to_screen(q);
                painter.rect(
                    egui::Rect::from_center_size(at, egui::vec2(GRIP_SIDE, GRIP_SIDE)),
                    0.0,
                    egui::Color32::WHITE,
                    stroke,
                    egui::StrokeKind::Middle,
                );
                let over = st.grip.as_ref().map(|g| g.handle == h).unwrap_or_else(|| pointer.is_some_and(|p| p.distance(at) <= GRIP_PT));
                if over {
                    ui.ctx().set_cursor_icon(h.cursor());
                }
            }
        }
        // The selection rectangle being drawn: solid left to right, dashed right to left.
        if let (Some(from), Some(p)) = (st.marquee, pointer) {
            let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 120, 200));
            let r = egui::Rect::from_two_pos(from, p);
            if p.x >= from.x {
                painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
            } else {
                let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&c, stroke, 5.0, 4.0));
            }
        }
        // What the pointer snaps on.
        if let Some(a) = &aim {
            draw_aim(&painter, &pv, a);
        }
        // The shape being drawn.
        if let (Some(from), Some(p)) = (st.sketch, pointer) {
            let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(70, 120, 200));
            let (a, b) = (pv.to_screen(from), aim.map_or(p, |a| pv.to_screen(a.at)));
            if st.tool == SheetTool::Rect {
                painter.rect_stroke(egui::Rect::from_two_pos(a, b), 0.0, stroke, egui::StrokeKind::Middle);
            } else {
                painter.line_segment([a, b], stroke);
            }
        }
        // The lines of views gone forgotten.
        let views: Vec<Id> = e
            .document()
            .drawing()
            .sheets
            .iter()
            .flat_map(|s| &s.elements)
            .filter(|el| matches!(el.kind, ElementKind::View(_)))
            .map(|el| el.id)
            .collect();
        lines.retain(&views);
        match st.tool {
            SheetTool::Select => {
                // A click picks what is under it (the last drawn first), elsewhere nothing; with
                // Shift it adds it or takes it off.
                if response.clicked() {
                    let at = response.interact_pointer_pos();
                    let hit = at.and_then(|p| st.frames.iter().rev().find(|(_, r)| r.contains(p)).map(|(v, _)| *v));
                    match (shift, hit) {
                        (true, Some(v)) => match st.selected.iter().position(|x| *x == v) {
                            Some(i) => {
                                st.selected.remove(i);
                            }
                            None => st.selected.push(v),
                        },
                        (true, None) => {}
                        (false, hit) => st.selected = hit.into_iter().collect(),
                    }
                }
            }
            SheetTool::Text => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                // A text there, picked, to be written in the entity info.
                if response.clicked()
                    && let Some(p) = response.interact_pointer_pos()
                    && let Ok(Some(o)) = e.apply(Operation::AddElement {
                        at: Placing { sheet: id, layer: active_layer(e.document().drawing(), st) },
                        element: ElementKind::Text {
                            at: aim.map_or_else(|| pv.to_sheet(p), |a| a.at),
                            text: t!("drawing.text_default"),
                            size: 3.5,
                        },
                    })
                {
                    st.selected = o.element.into_iter().collect();
                    st.tool = SheetTool::Select;
                }
            }
            SheetTool::Rect | SheetTool::Line => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
            tool @ (SheetTool::Linear | SheetTool::Radius | SheetTool::Diameter | SheetTool::Angle) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                if let Some(p) = response.hover_pos() {
                    let at = pv.to_sheet(p);
                    let mark = egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 120, 30));
                    if st.picks.len() < picks_needed(tool) {
                        // A vertex or an edge to pick, of the first pick's view once there is one.
                        let only = st.picks.first().map(|x| x.0);
                        let hit = pick_at(e, drawing, &mut st.views, id, tool, at, 8.0 / pv.zoom, only);
                        if let Some((_, _, marker)) = &hit {
                            match marker.as_slice() {
                                [q] => {
                                    painter.circle_stroke(pv.to_screen(*q), 5.0, mark);
                                }
                                line => {
                                    painter.add(egui::Shape::line(line.iter().map(|q| pv.to_screen(*q)).collect(), mark));
                                }
                            }
                        }
                        if response.clicked()
                            && let Some((v, a, _)) = hit
                        {
                            st.picks.push((v, a));
                        }
                    } else if let Some(dim) = dimension_of(e, drawing, &mut st.views, tool, &st.picks, at) {
                        // Where a click puts it.
                        if let Some((r, c)) = view_map(e, drawing, &mut st.views, dim.view) {
                            draw_dim(&painter, &pv, &crate::dims::shape(e.document(), &r, c, &dim));
                        }
                        if response.clicked() {
                            let layer = active_layer(e.document().drawing(), st);
                            st.picks.clear();
                            if let Ok(Some(o)) =
                                e.apply(Operation::AddElement { at: Placing { sheet: id, layer }, element: ElementKind::Dimension(dim) })
                            {
                                st.selected = o.element.into_iter().collect();
                            }
                        }
                    }
                }
            }
            SheetTool::Project { base } => {
                if drawing.resolve(base).is_none() {
                    st.tool = SheetTool::Select;
                } else if let Some(p) = response.hover_pos()
                    && let Some((side, distance)) = projection_at(e, drawing, base, pv.to_sheet(p))
                {
                    // Its frame where a click puts it.
                    let r = drawing.resolve(base).unwrap();
                    let ghost =
                        Resolved { frame: r.frame.projected(side, drawing.projection), at: r.at + side.dir() * distance, ..r.clone() };
                    let (lo, hi) = view_frame(e, &ghost);
                    let g = pv.rect(lo, hi);
                    let corners = [g.left_top(), g.right_top(), g.right_bottom(), g.left_bottom(), g.left_top()];
                    painter.extend(egui::Shape::dashed_line(&corners, egui::Stroke::new(1.0, egui::Color32::from_gray(120)), 4.0, 4.0));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    if response.clicked() {
                        let n =
                            drawing.sheet(id).map_or(0, |s| s.elements.iter().filter(|e| matches!(e.kind, ElementKind::View(_))).count())
                                + 1;
                        let view = View {
                            name: t!("drawing.view_name", n = n),
                            kind: ViewKind::Projected { base, side, distance },
                            style: drawing
                                .element(base)
                                .and_then(|el| match &el.kind {
                                    ElementKind::View(v) => Some(v.style),
                                    _ => None,
                                })
                                .unwrap_or_default(),
                        };
                        if let Ok(Some(o)) =
                            e.apply(Operation::AddElement { at: Placing { sheet: id, layer: None }, element: ElementKind::View(view) })
                        {
                            st.selected = o.element.into_iter().collect();
                        }
                    }
                }
            }
        }
    } else {
        let at = rect.center();
        ui.painter().text(at, egui::Align2::CENTER_CENTER, t!("drawing.no_sheet_hint"), egui::FontId::proportional(14.0), ink);
    }
    tabs(ui, egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - tabs_height), rect.max), e, st);
    wanted
}

/// A sheet's elements, bottom first: views as their frames, named; texts, lines, rectangles;
/// images as their frames for now.
#[allow(clippy::too_many_arguments)]
fn draw_elements(
    painter: &egui::Painter,
    pv: &Paperview,
    e: &Editor,
    d: &Drawing,
    sheet: Id,
    selected: &[Id],
    ready: &HashMap<Id, egui::TextureId>,
    lines: &mut crate::view_lines::LinesCache,
    views: &mut ViewsCache,
    wanted: &mut Vec<Wanted>,
    fields: Option<&Fields<'_>>,
) -> Vec<(Id, egui::Rect)> {
    let mut frames = Vec::new();
    let Some(s) = d.sheet(sheet) else { return frames };
    let ppp = painter.ctx().pixels_per_point();
    let paper_ink = egui::Color32::from_gray(30);
    let frame_ink = egui::Color32::from_rgb(70, 120, 200);
    let thin = egui::Stroke::new((0.25 * pv.zoom).max(1.0) as f32, paper_ink);
    let accent = egui::Stroke::new(2.0, frame_ink);
    for el in d.elements_of(s) {
        // A hidden layer's elements not drawn, a locked one's not picked.
        let layer = d.layer(el.layer);
        if layer.is_some_and(|l| l.hidden) {
            continue;
        }
        let pickable = layer.is_none_or(|l| !l.locked);
        match &el.kind {
            ElementKind::View(v) => {
                let Some(r) = d.resolve(el.id) else { continue };
                let data = views.get(e, el.id, &r);
                let (lo, hi) = frame_of(&r, data.bx3.map(|(lo, hi)| (lo.truncate(), hi.truncate())));
                let rect = pv.rect(lo, hi);
                // Its picture — its parts rendered, its lines over them, as the style says — as
                // last drawn, under its frame.
                if let Some(tex) = ready.get(&el.id) {
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    painter.image(*tex, rect, uv, egui::Color32::WHITE);
                }
                if painter.clip_rect().intersects(rect) {
                    // Its lines as last computed (new ones on their way when it changed).
                    let got = if v.style == ViewStyle::Rendered {
                        None
                    } else {
                        let (got, busy) = lines.get(el.id, data.lines.as_ref());
                        if busy {
                            painter.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                        }
                        got
                    };
                    if let Some(w) = wanted_picture(el.id, v.style, &r, &data, got, rect, ppp) {
                        wanted.push(w);
                    }
                }
                if pickable {
                    frames.push((el.id, rect));
                }
                if selected.contains(&el.id) {
                    painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, frame_ink), egui::StrokeKind::Outside);
                } else {
                    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
                    painter.extend(egui::Shape::dashed_line(&corners, egui::Stroke::new(1.0, frame_ink), 6.0, 4.0));
                }
                let scale = scale_label(r.scale);
                painter.text(
                    rect.left_top() + egui::vec2(0.0, -2.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{} · {scale}", v.name),
                    egui::FontId::proportional(11.0),
                    frame_ink,
                );
                crate::handles::publish(|| format!("sheet_view:{}", el.id.0), rect);
            }
            ElementKind::Dimension(dim) => {
                if let Some((r, centre)) = view_map(e, d, views, dim.view) {
                    let shape = crate::dims::shape(e.document(), &r, centre, dim);
                    let rect = draw_dim(painter, pv, &shape);
                    if pickable {
                        frames.push((el.id, rect));
                    }
                    if selected.contains(&el.id) {
                        painter.rect_stroke(rect, 2.0, accent, egui::StrokeKind::Outside);
                    }
                    crate::handles::publish(|| format!("sheet_element:{}", el.id.0), rect);
                }
            }
            other => {
                let r = match other {
                    ElementKind::Text { at, text, size } => {
                        let shown = fields.map_or_else(|| text.clone(), |f| f.fill(text));
                        let font = egui::FontId::proportional((size * pv.zoom).max(1.0) as f32);
                        painter.text(pv.to_screen(*at), egui::Align2::LEFT_BOTTOM, shown, font, paper_ink)
                    }
                    ElementKind::Line { from, to } => {
                        let (a, b) = (pv.to_screen(*from), pv.to_screen(*to));
                        painter.line_segment([a, b], thin);
                        egui::Rect::from_two_pos(a, b).expand(3.0)
                    }
                    ElementKind::Rect { min, max } => {
                        let r = pv.rect(*min, *max);
                        painter.rect_stroke(r, 0.0, thin, egui::StrokeKind::Middle);
                        r
                    }
                    ElementKind::Image { min, max, .. } => {
                        let r = pv.rect(*min, *max);
                        painter.rect_stroke(r, 0.0, thin, egui::StrokeKind::Middle);
                        painter.line_segment([r.left_top(), r.right_bottom()], thin);
                        painter.line_segment([r.left_bottom(), r.right_top()], thin);
                        r
                    }
                    ElementKind::View(_) | ElementKind::Dimension(_) => unreachable!(),
                };
                if pickable {
                    frames.push((el.id, r));
                }
                if selected.contains(&el.id) {
                    painter.rect_stroke(r.expand(2.0), 2.0, accent, egui::StrokeKind::Outside);
                }
                crate::handles::publish(|| format!("sheet_element:{}", el.id.0), r);
            }
        }
    }
    frames
}

/// « 1:10 », « 2:1 ».
pub fn scale_label(scale: f64) -> String {
    let n = |v: f64| if (v - v.round()).abs() < 1e-9 { format!("{}", v.round()) } else { format!("{v:.3}") };
    if scale >= 1.0 { format!("{}:1", n(scale)) } else { format!("1:{}", n(1.0 / scale)) }
}

/// The sheets' tabs along the bottom: one per sheet (a click shows it), then « + » (a new
/// sheet).
fn tabs(ui: &mut egui::Ui, rect: egui::Rect, e: &mut Editor, st: &mut SheetState) {
    let sheets: Vec<(Id, String)> = e.document().drawing().sheets.iter().map(|s| (s.id, s.name.clone())).collect();
    let shown = st.shown(e.document().drawing()).map(|s| s.id);
    let mut add = false;
    let mut template = None;
    let mut deleted = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(12.0, 8.0))), |ui| {
        ui.horizontal(|ui| {
            for (id, name) in &sheets {
                let tab = ui.selectable_label(shown == Some(*id), name);
                crate::handles::publish(|| format!("sheet_tab:{}", id.0), tab.rect);
                if tab.clicked() {
                    st.current = Some(*id);
                }
                tab.context_menu(|ui| {
                    if ui.button(t!("menu.drawing.save_template")).clicked() {
                        st.current = Some(*id);
                        st.command = Some("drawing.save_template");
                        ui.close();
                    }
                    if ui.button(t!("drawing.delete_sheet")).clicked() {
                        deleted = Some(*id);
                        ui.close();
                    }
                });
            }
            // « + »: a blank sheet, or one from a template (docs/ui/drawing.md#templates).
            let plus = ui.button("+").on_hover_text(t!("menu.drawing.new_sheet"));
            crate::handles::publish(|| "sheet_add".into(), plus.rect);
            egui::Popup::menu(&plus).style(crate::style::menu).show(|ui| {
                let blank = ui.add(crate::style::menu_item(t!("drawing.blank_sheet"), None, false));
                crate::handles::publish(|| "sheet_add:blank".into(), blank.rect);
                if blank.clicked() {
                    add = true;
                    ui.close();
                }
                ui.separator();
                for (k, tpl) in crate::templates::all().into_iter().enumerate() {
                    let item = ui.add(crate::style::menu_item(&tpl.name, None, false));
                    crate::handles::publish(|| format!("sheet_add:{k}"), item.rect);
                    if item.clicked() {
                        template = Some(tpl.document);
                        ui.close();
                    }
                }
            });
        });
    });
    if add {
        new_sheet(e, st);
    }
    if let Some(template) = template
        && let Ok(Some(o)) = e.apply(Operation::AddSheetFrom { template, name: None })
    {
        st.current = o.sheet;
    }
    if let Some(sheet) = deleted {
        let _ = e.apply(Operation::DeleteSheet { sheet });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_handle_moves_its_sides_and_a_corner_with_shift_keeps_proportions() {
        let (min, max) = (DVec2::new(0.0, 0.0), DVec2::new(40.0, 20.0));
        // The right side's middle: the width alone.
        assert_eq!(resized(min, max, 1, 0, DVec2::new(60.0, 99.0), false), (min, DVec2::new(60.0, 20.0)));
        // Past the opposite side: the box turned over, still min / max.
        assert_eq!(resized(min, max, 1, 0, DVec2::new(-10.0, 0.0), false), (DVec2::new(-10.0, 0.0), DVec2::new(0.0, 20.0)));
        // The top right corner with Shift: twice as wide, so twice as high.
        assert_eq!(resized(min, max, 1, 1, DVec2::new(80.0, 25.0), true), (min, DVec2::new(80.0, 40.0)));
    }

    #[test]
    fn a_point_snaps_on_a_point_then_aligns_then_takes_the_grid() {
        let corner = DVec2::new(10.0, 10.0);
        let (on, align) = (vec![corner], vec![corner]);
        // Within reach of the corner: on it.
        let a = aim_at(&on, &align, DVec2::new(11.0, 9.5), 2.0, None, 5.0);
        assert!(a.point && a.at == corner);
        // Far from it but with its x: aligned along y, its y on the grid.
        let a = aim_at(&on, &align, DVec2::new(10.6, 33.0), 2.0, None, 5.0);
        assert_eq!((a.at, a.along_y, a.along_x), (DVec2::new(10.0, 35.0), Some(corner), None));
        // Aligned with where the line started, along x.
        let from = DVec2::new(50.0, 20.0);
        let a = aim_at(&on, &align, DVec2::new(80.3, 21.0), 2.0, Some(from), 0.0);
        assert_eq!((a.at, a.along_x), (DVec2::new(80.3, 20.0), Some(from)));
        // Nothing near, no grid: where the pointer is.
        let p = DVec2::new(73.3, 61.7);
        assert_eq!(aim_at(&on, &align, p, 2.0, None, 0.0).at, p);
    }

    #[test]
    fn a_view_handle_takes_the_nearest_standard_scale() {
        assert_eq!(nearest_scale(0.11), 0.1);
        assert_eq!(nearest_scale(0.03), 1.0 / 25.0);
        assert_eq!(nearest_scale(100.0), 10.0);
    }

    #[test]
    fn a_text_handle_sets_its_height() {
        let kind = ElementKind::Text { at: DVec2::new(10.0, 10.0), text: "T".into(), size: 3.5 };
        let g = Grip {
            element: Id(1),
            handle: Handle::Size,
            start: DVec2::ZERO,
            from: DVec2::ZERO,
            kind,
            lo: DVec2::new(10.0, 10.0),
            hi: DVec2::new(20.0, 15.0),
        };
        // The box 5 mm high for 3.5: its top at 20, twice as high.
        assert_eq!(
            regrip(&g, DVec2::new(30.0, 20.0), false),
            ElementKind::Text { at: DVec2::new(10.0, 10.0), text: "T".into(), size: 7.0 }
        );
    }

    #[test]
    fn fields_are_filled_and_unknown_ones_kept() {
        let f = Fields { document: "Table.skraft", sheet: "Plan", number: 2, count: 3, scale: Some(0.1) };
        assert_eq!(f.fill("{document} · {sheet} {sheet.number}/{sheet.count} · {scale}"), "Table.skraft · Plan 2/3 · 1:10");
        assert_eq!(f.fill("{nope} and {"), "{nope} and {");
        assert_eq!(f.fill("{date}").len(), 10, "dd/mm/yyyy");
    }

    #[test]
    fn scales_are_standard_and_fit() {
        // A 2 m table in 210 mm: 1:10.
        assert_eq!(scale_to_fit(DVec2::new(2000.0, 750.0), DVec2::new(210.0, 148.0)), 0.1);
        // A 40 mm part in 210 mm: 5:1.
        assert_eq!(scale_to_fit(DVec2::new(40.0, 10.0), DVec2::new(210.0, 148.0)), 5.0);
        assert_eq!(scale_label(0.1), "1:10");
        assert_eq!(scale_label(5.0), "5:1");
        assert_eq!(scale_label(1.0), "1:1");
    }
}

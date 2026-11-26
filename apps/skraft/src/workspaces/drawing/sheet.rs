//! The « Mise en plan » workspace's stage (docs/ui/drawing.md, decisions 0048 and 0050): the
//! board of every sheet of the document's drawing, panned and zoomed, one of them active with its
//! views' frames and its elements to edit; the navigator (`drawing::cards::navigator`) choosing it.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{DAffine2, DAffine3, DMat4, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{
    Container, DimKind, Dimension, Direction, Drawing, ElementKind, Entity, EntityPath, Found, Id, Look, Operation, Orient, Paper, Place,
    Projection, Resolved, SeenBoard, Sheet, Side, Subject, View, ViewKind, ViewStyle,
};
use skraft_theme::{Inks, Theme};
use skraft_tools::Editor;

use crate::kit::overlay;
use crate::workspaces::common::viewport;
use crate::workspaces::drawing::dialogs::view_picker::{Purpose, ViewPicker, subject_of_paths};
use crate::workspaces::drawing::sheet::board::{Board, Column};

/// A document's sheet shown and how it is seen (the sheet and the camera kept in its file,
/// [`skraft_model::SeenBoard`]).
#[derive(Clone, Debug, Default)]
pub struct SheetState {
    /// The sheet shown: none, the first of the presentation shown.
    pub current: Option<Id>,
    /// Two fingers pan and pinch the board (docs/ui/touch.md).
    pub touch: crate::kit::touch::TwoFingers,
    /// A finger dragged from nothing with the select tool pans the board, until lifted.
    pub finger_pan: bool,
    /// The presentation shown: the sheet's, kept while it has none (docs/ui/drawing.md#navigator).
    presentation: Option<Id>,
    /// The navigator's rows unfolded, and the sheet they were last unfolded for.
    pub tree_open: skraft_model::FastSet<egui::Id>,
    pub tree_shown: Option<Id>,
    /// The board point at the stage's centre, in millimetres.
    pan: DVec2,
    /// Points per millimetre of paper.
    zoom: f64,
    /// What the camera is to frame at the next frame (docs/ui/drawing.md#framing).
    framing: Option<Framing>,
    /// The active sheet and its place on the board last frame: the camera follows it when the
    /// layout changes.
    follow: Option<(Id, DVec2)>,
    /// The sheet the last click made active: a double click then frames it.
    activated: Option<Id>,
    /// The elements picked on the sheet (a click, Shift+click, a rectangle), what the commands
    /// change.
    pub selected: Vec<Id>,
    /// What a click on the sheet does.
    pub tool: SheetTool,
    /// The selection being dragged.
    moving: Option<Moving>,
    /// Where the selection rectangle being drawn starts (screen points).
    marquee: Option<egui::Pos2>,
    /// The pointer over the sheet last frame (screen points), for the status bar.
    pointer: Option<egui::Pos2>,
    /// The handle being dragged.
    grip: Option<Grip>,
    /// The shape last made or the handle last let go, which a value typed changes
    /// (docs/ui/drawing.md#measurements); dropped once the element is changed otherwise.
    amend: Option<Grip>,
    /// What the measurements box shows: its label and the current value.
    measure: Option<(String, String)>,
    /// Where the shape being drawn last went: a value is typed with the pointer anywhere.
    sketch_to: Option<DVec2>,
    /// A value typed in the measurements box, to apply (the app puts it).
    pub typed: Option<String>,
    /// Esc was taken by the measurements box this frame (what was typed dropped).
    pub esc_eaten: bool,
    /// Why the value typed was refused, in the status bar until the next one.
    message: Option<String>,
    /// The views' frames on screen last frame, bottom first: what a press lands on.
    frames: Vec<(Id, egui::Rect)>,
    /// Their boxes in the axes of the level open, last frame.
    boxes: HashMap<Id, (DVec2, DVec2)>,
    /// The occurrences entered, from the sheet down (docs/ui/drawing.md#compositions): what they place
    /// is edited, the rest faded.
    pub open: Vec<Id>,
    /// The box of what the level open shows, in its axes: a click off it leaves it.
    open_box: Option<(DVec2, DVec2)>,
    /// The compositions entered last frame: one left empty goes.
    was_open: Vec<Id>,
    /// A definition to place on the sheet shown (the navigator's menu).
    pub place_composition: Option<Id>,
    /// What the navigator or the clipboard ask of the stage, done at its next frame (it has the
    /// editor, and the pointer in the level open).
    pub asked: Option<SheetAsk>,
    /// The library's compositions: search, rows and outlines.
    pub library: crate::workspaces::drawing::cards::library::CompositionsState,
    /// The composition being dragged from that card, placed where it is let go.
    pub carried: Option<Id>,
    /// The dialog choosing what a view shows, while open.
    pub picker: Option<ViewPicker>,
    /// The view being placed by [`SheetTool::Place`].
    placing: Option<Placing>,
    /// The view being framed (« Cadrer la vue »), over the stage.
    pub framer: Option<crate::workspaces::drawing::dialogs::view_framer::ViewFramer>,
    /// What the entity info shows while nothing is picked.
    pub info: InfoOn,
    /// What each view shows, for the document's revision and the meshes' generation.
    views: ViewsCache,
    /// Where the rectangle or line being drawn starts (sheet millimetres).
    sketch: Option<DVec2>,
    /// What a dimension's tool picked so far: the view, the anchors.
    picks: Vec<(Id, skraft_model::Anchor)>,
}

/// What the camera is asked to frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Framing {
    /// Every sheet.
    Board,
    /// A sheet whole.
    Sheet(Id),
    /// The column of a presentation without a sheet.
    Column(Id),
    /// A sheet, only if it is off screen.
    Reveal(Id),
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
    let Some(list) = st.container(d).and_then(|c| d.elements_in(c)) else { return Vec::new() };
    list.iter().filter(|el| st.selected.contains(&el.id)).collect()
}

/// What is picked, as a fragment to paste, with the box it covers in the level open.
pub fn copy_selected(e: &Editor, st: &SheetState) -> Option<(Arc<skraft_model::Document>, (DVec2, DVec2))> {
    let ids: Vec<Id> = selection(e, st).iter().map(|el| el.id).collect();
    let to = st.container(e.document().drawing())?;
    let fragment = e.document().elements_fragment(to, &ids).ok()?;
    let bounds = ids
        .iter()
        .filter_map(|id| st.boxes.get(id).copied())
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
        .unwrap_or((DVec2::ZERO, DVec2::ZERO));
    Some((Arc::new(fragment), bounds))
}

/// Everything in the level open picked, but what is hidden or locked.
pub fn select_all(e: &Editor, st: &mut SheetState) {
    let d = e.document().drawing();
    let list = st.container(d).and_then(|c| d.elements_in(c)).unwrap_or(&[]);
    st.selected = list.iter().filter(|el| !el.hidden && !el.locked).map(|el| el.id).collect();
}

/// How far box `lo`–`hi` must move to lie on `paper`; too large for it, centred on it.
fn offset_into(lo: DVec2, hi: DVec2, paper: Paper) -> DVec2 {
    let along = |lo: f64, hi: f64, room: f64| {
        if hi - lo > room {
            (room - lo - hi) / 2.0
        } else if lo < 0.0 {
            -lo
        } else if hi > room {
            room - hi
        } else {
            0.0
        }
    };
    DVec2::new(along(lo.x, hi.x, paper.width), along(lo.y, hi.y, paper.height))
}

/// The box of `el` in its container's axes; an occurrence has none of its own.
fn element_box(e: &Editor, el: &skraft_model::Element, projection: Projection) -> Option<(DVec2, DVec2)> {
    Some(match &el.kind {
        ElementKind::Rect { min, max } | ElementKind::Image { min, max, .. } => (*min, *max),
        ElementKind::Line { from, to } => (from.min(*to), from.max(*to)),
        ElementKind::Circle { center, radius } => (*center - DVec2::splat(*radius), *center + DVec2::splat(*radius)),
        ElementKind::Text { at, text, size } => {
            let columns = text.lines().map(|l| l.chars().count()).max().unwrap_or(1) as f64;
            let rows = text.lines().count().max(1) as f64;
            (*at, *at + DVec2::new(columns * size * 0.55, rows * size * 1.25))
        }
        ElementKind::View(_) => view_frame(e, &e.document().drawing().resolve_as(el.id, projection)?),
        ElementKind::Dimension(dim) => dim_shape(e, dim, projection)?.bounds(),
        ElementKind::Occurrence(_) => return None,
    })
}

/// A composition's outline in its axes, what stands for it in the card and while it is dragged:
/// its lines, its rectangles, a view's frame, a bar per line of text.
#[derive(Clone, Debug)]
pub struct Sketch {
    pub segments: Vec<(DVec2, DVec2)>,
    pub lo: DVec2,
    pub hi: DVec2,
}

impl Sketch {
    /// Its lines, as large as they fit in the middle of `rect`.
    pub fn paint_in(&self, painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
        let size = (self.hi - self.lo).max(DVec2::splat(1e-6));
        let scale = (rect.width() as f64 / size.x).min(rect.height() as f64 / size.y);
        let middle = (self.lo + self.hi) / 2.0;
        let at = |q: DVec2| rect.center() + egui::vec2(((q.x - middle.x) * scale) as f32, ((middle.y - q.y) * scale) as f32);
        painter.extend(self.segments.iter().map(|(a, b)| egui::Shape::line_segment([at(*a), at(*b)], stroke)));
    }
}

/// The outline of composition `id`, what is hidden left out; none when it shows nothing.
pub fn sketch_of(e: &Editor, id: Id, projection: Projection) -> Option<Sketch> {
    fn walk(e: &Editor, list: &[skraft_model::Element], to: DAffine2, projection: Projection, depth: usize, s: &mut Sketch) {
        let d = e.document().drawing();
        for el in list.iter().filter(|el| !el.hidden) {
            if let Some(i) = el.kind.occurrence() {
                if let Some(c) = d.composition(i.composition).filter(|_| depth < 32) {
                    walk(e, &c.elements, to * i.place.transform(None), projection, depth + 1, s);
                }
                continue;
            }
            let Some((lo, hi)) = element_box(e, el, projection) else { continue };
            let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)].map(|q| to.transform_point2(q));
            for q in corners {
                (s.lo, s.hi) = (s.lo.min(q), s.hi.max(q));
            }
            match &el.kind {
                ElementKind::Line { from, to: end } => s.segments.push((to.transform_point2(*from), to.transform_point2(*end))),
                ElementKind::Text { at, text, size } => {
                    for (k, line) in text.lines().enumerate() {
                        let y = at.y + size * (0.5 + 1.25 * (text.lines().count() - 1 - k) as f64);
                        let width = line.chars().count() as f64 * size * 0.55;
                        s.segments.push((to.transform_point2(DVec2::new(at.x, y)), to.transform_point2(DVec2::new(at.x + width, y))));
                    }
                }
                ElementKind::Circle { center, radius } => {
                    const SIDES: usize = 32;
                    let on = |k: usize| {
                        let a = k as f64 / SIDES as f64 * std::f64::consts::TAU;
                        to.transform_point2(*center + DVec2::from_angle(a) * *radius)
                    };
                    s.segments.extend((0..SIDES).map(|k| (on(k), on(k + 1))));
                }
                ElementKind::Dimension(_) => {}
                _ => s.segments.extend((0..4).map(|k| (corners[k], corners[(k + 1) % 4]))),
            }
        }
    }
    let c = e.document().drawing().composition(id)?;
    let mut s = Sketch { segments: Vec::new(), lo: DVec2::INFINITY, hi: DVec2::NEG_INFINITY };
    walk(e, &c.elements, DAffine2::IDENTITY, projection, 0, &mut s);
    (s.lo.x <= s.hi.x).then_some(s)
}

/// Is point `p` on the stage itself, no card over it?
fn on_stage(ctx: &egui::Context, room: egui::Rect, p: egui::Pos2) -> bool {
    room.contains(p) && ctx.layer_id_at(p).is_none_or(|l| matches!(l.order, egui::Order::Background | egui::Order::Tooltip))
}

/// Sheet `id` made active, the camera staying where it is.
fn make_active(st: &mut SheetState, board: &Board, id: Id) {
    st.show_sheet(id);
    st.info = InfoOn::Sheet;
    st.follow = board.sheet(id).map(|s| (id, s.origin));
}

/// May the elements `picked` leave their sheet? Not a projected view or a dimension without what
/// it hangs on (docs/ui/drawing.md#moving-between-sheets).
fn can_leave_sheet(d: &Drawing, picked: &[Id]) -> bool {
    !picked.is_empty() && picked.iter().all(|id| d.element(*id).is_some_and(|el| el.kind.hung_on().is_none() || carried(d, *id, picked)))
}

/// The selection, dragged by `offset` on sheet `from`, let go over sheet `to`: it moves there, where
/// it was let go on the board, what moved picked, that sheet active, in one step.
fn drop_on_sheet(e: &mut Editor, st: &mut SheetState, board: &Board, from: Id, to: Id, offset: DVec2) {
    let (Some(a), Some(b)) = (board.sheet(from), board.sheet(to)) else { return };
    let offset = a.origin + offset - b.origin;
    match e.apply(Operation::MoveElements { elements: st.selected.clone(), to, offset, copy: false }) {
        Ok(Some(o)) => {
            make_active(st, board, to);
            st.selected = o.elements;
        }
        Ok(None) => {}
        Err(m) => st.message = Some(m),
    }
}

/// The box on sheet `sheet` of the elements `ids` with their families, those anchored to the
/// paper left out (they keep their corner on any paper).
fn free_bounds(e: &Editor, sheet: Id, ids: &[Id]) -> Option<(DVec2, DVec2)> {
    let doc = e.document();
    let d = doc.drawing();
    let members = doc.families(Container::Sheet(sheet), ids).ok()?;
    let anchored = |id: Id| d.element(id).and_then(|el| el.kind.occurrence()).is_some_and(|i| i.place.corner.is_some());
    let mut out: Option<(DVec2, DVec2)> = None;
    for shown in d.placed(sheet) {
        let top = shown.top();
        if !members.contains(&top) || anchored(top) {
            continue;
        }
        let Some((lo, hi)) = element_box(e, shown.element, shown.projection) else { continue };
        for q in [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)].map(|q| shown.transform.transform_point2(q)) {
            out = Some(out.map_or((q, q), |(a, z)| (a.min(q), z.max(q))));
        }
    }
    out
}

/// Does what was asked of the stage; `centre`: the stage's on screen.
fn run_ask(e: &mut Editor, st: &mut SheetState, ask: SheetAsk, centre: egui::Pos2) {
    match ask {
        SheetAsk::NewSheet(presentation) => {
            st.show_presentation(e.document().drawing(), presentation);
            new_sheet(e, st);
        }
        SheetAsk::SheetFrom { template, sheet, name, presentation } => {
            st.show_presentation(e.document().drawing(), presentation);
            sheet_from(e, st, template, sheet, name);
        }
        SheetAsk::PresentationFrom(template) => {
            presentation_from(e, st, template);
        }
        SheetAsk::Paste { fragment, bounds, in_place, linked } => {
            let d = e.document().drawing();
            let Some(sheet) = st.shown(d).map(|s| s.id) else { return };
            let (to, place) = d.opened(sheet, &st.open).unwrap_or((Container::Sheet(sheet), DAffine2::IDENTITY));
            let aside = DVec2::new(10.0, -10.0);
            let offset = match (in_place, st.pointer) {
                (true, _) => DVec2::ZERO,
                // At the pointer while it is on the active sheet's paper.
                (false, Some(p)) => {
                    let origin = Board::of(d).sheet(sheet).map_or(DVec2::ZERO, |s| s.origin);
                    let pv = Paperview::new(centre, st.pan, st.zoom).at(origin);
                    let (on, paper) = (pv.to_sheet(p), d.sheet(sheet).map_or(DVec2::ZERO, |s| s.paper.size()));
                    if on.cmpge(DVec2::ZERO).all() && on.cmple(paper).all() {
                        pv.placed(place).to_sheet(p) - (bounds.0 + bounds.1) / 2.0
                    } else {
                        aside
                    }
                }
                (false, None) => aside,
            };
            match e.apply(Operation::PasteElements { fragment, to, offset, linked }) {
                Ok(Some(o)) => {
                    st.selected = o.elements;
                    st.amend = None;
                }
                Ok(None) => {}
                Err(m) => st.message = Some(m),
            }
        }
        SheetAsk::CopySheet { sheet, presentation, index } => match e.apply(Operation::CopySheet { sheet, presentation, index }) {
            Ok(Some(o)) => {
                if let Some(s) = o.sheet {
                    st.show_sheet(s);
                    st.info = InfoOn::Sheet;
                }
            }
            Ok(None) => {}
            Err(m) => st.message = Some(m),
        },
        SheetAsk::Drop { elements, to, copy } => {
            let d = e.document().drawing();
            let Some(Container::Sheet(from)) = elements.first().and_then(|id| d.container_of(*id)) else { return };
            let Some(paper) = d.sheet(to).map(|s| s.paper) else { return };
            let offset = if from == to {
                DVec2::new(5.0, -5.0)
            } else {
                free_bounds(e, from, &elements).map_or(DVec2::ZERO, |(lo, hi)| offset_into(lo, hi, paper))
            };
            match e.apply(Operation::MoveElements { elements, to, offset, copy }) {
                Ok(Some(o)) => {
                    st.show_sheet(to);
                    st.selected = o.elements;
                }
                Ok(None) => {}
                Err(m) => st.message = Some(m),
            }
        }
    }
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

/// What is asked of the stage for its next frame.
#[derive(Clone, Debug)]
pub enum SheetAsk {
    /// What was copied, at the pointer or `in_place`; `linked`: from this document, in a state
    /// it still knows.
    Paste { fragment: Arc<skraft_model::Document>, bounds: (DVec2, DVec2), in_place: bool, linked: bool },
    /// A blank sheet in presentation `.0`.
    NewSheet(Id),
    /// A sheet of `template` (`sheet`: none, its first) in `presentation`.
    SheetFrom { template: Arc<skraft_model::Document>, sheet: Option<Id>, name: Option<String>, presentation: Id },
    /// A presentation from a template.
    PresentationFrom(Arc<skraft_model::Document>),
    /// Elements dragged in the navigator onto sheet `to`, moved or copied.
    Drop { elements: Vec<Id>, to: Id, copy: bool },
    /// A copy of sheet `sheet` at `index` of `presentation`, made active.
    CopySheet { sheet: Id, presentation: Id, index: usize },
}

/// What the entity info shows while nothing is picked (docs/ui/drawing.md#entity-info).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InfoOn {
    #[default]
    Sheet,
    Presentation,
    Document,
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
    /// A click places the new view chosen in the dialog (« Nouvelle vue »).
    Place,
    /// A click puts a text there (« Texte »).
    Text,
    /// A drag draws a rectangle (« Rectangle »), a circle from its centre (« Cercle »), a line
    /// (« Trait »).
    Rect,
    Circle,
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
            "drawing.tool_circle" => Some(SheetTool::Circle),
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

use skraft_tools::vcb::{Vcb, VcbKind, VcbValue};

impl SheetState {
    /// The presentation shown: the sheet's, else the one last shown, else the first.
    pub fn presentation<'a>(&self, d: &'a Drawing) -> Option<&'a skraft_model::Presentation> {
        self.current
            .and_then(|s| d.presentation_of(s))
            .or_else(|| self.presentation.and_then(|p| d.presentation(p)))
            .or_else(|| d.presentations.first())
    }

    /// The sheet shown: `current` if it still is, else the first of the presentation shown.
    pub fn shown<'a>(&self, d: &'a Drawing) -> Option<&'a Sheet> {
        self.current.and_then(|id| d.sheet(id)).or_else(|| self.presentation(d)?.sheets.first())
    }

    /// Where its projected views go: the presentation shown's.
    pub fn projection(&self, d: &Drawing) -> Projection {
        self.presentation(d).map_or_else(Projection::default, |p| p.projection)
    }

    /// What holds what is edited now: the composition of the last occurrence entered, else the sheet
    /// shown.
    pub fn container(&self, d: &Drawing) -> Option<Container> {
        let s = self.shown(d)?;
        Some(d.opened(s.id, &self.open).map_or(Container::Sheet(s.id), |(c, _)| c))
    }

    /// Sheet `id` shown, what was being drawn on the other one dropped.
    pub fn show_sheet(&mut self, id: Id) {
        if self.current != Some(id) {
            self.current = Some(id);
            self.amend = None;
            self.picks.clear();
            self.sketch = None;
            self.selected.clear();
            self.open.clear();
            // What the pointer landed on was the other sheet's.
            self.frames.clear();
            self.boxes.clear();
            self.open_box = None;
            self.grip = None;
            self.moving = None;
            self.marquee = None;
        }
    }

    /// Occurrence `id`, in the level open, entered.
    pub fn enter(&mut self, id: Id) {
        self.open.push(id);
        self.selected.clear();
        self.amend = None;
        self.picks.clear();
        self.sketch = None;
    }

    /// One level up, the occurrence left picked.
    pub fn leave(&mut self) {
        if let Some(id) = self.open.pop() {
            self.selected = vec![id];
            self.amend = None;
            self.picks.clear();
            self.sketch = None;
        }
    }

    /// The levels open down to `path`, kept as far as they agree with it.
    pub fn open_to(&mut self, path: &[Id]) {
        let k = self.open.iter().zip(path).take_while(|(a, b)| a == b).count();
        if k < self.open.len() {
            self.open.truncate(k);
            self.selected.clear();
            self.amend = None;
            self.picks.clear();
            self.sketch = None;
        }
    }

    /// Presentation `id` shown, at its first sheet (none while it has none).
    pub fn show_presentation(&mut self, d: &Drawing, id: Id) {
        if self.presentation(d).is_some_and(|p| p.id == id) {
            return;
        }
        self.presentation = Some(id);
        match d.presentation(id).and_then(|p| p.sheets.first()) {
            Some(s) => self.show_sheet(s.id),
            None => {
                self.current = None;
                self.selected.clear();
                self.open.clear();
            }
        }
    }

    /// Is the camera still to frame something?
    pub fn framing_asked(&self) -> bool {
        self.framing.is_some()
    }

    /// What a file keeps of it; none while the board was never shown.
    pub fn seen(&self) -> Option<SeenBoard> {
        (self.zoom > 0.0).then_some(SeenBoard { sheet: self.current, presentation: self.presentation, pan: self.pan, zoom: self.zoom })
    }

    /// Back to what a file kept (it is untrusted): a sheet or presentation gone is left out, a
    /// camera that cannot be one leaves the board to be framed.
    pub fn restore(&mut self, d: &Drawing, seen: &SeenBoard) {
        if !seen.pan.is_finite() || !seen.zoom.is_finite() || seen.zoom <= 0.0 {
            return;
        }
        self.current = seen.sheet.filter(|id| d.sheet(*id).is_some());
        self.presentation = seen.presentation.filter(|id| d.presentation(*id).is_some());
        self.pan = seen.pan;
        self.zoom = seen.zoom.clamp(0.01, 200.0);
        // Already followed: else it would be brought on screen, the view kept lost.
        let board = Board::of(d);
        self.follow = self.shown(d).and_then(|s| board.sheet(s.id)).map(|s| (s.id, s.origin));
    }

    /// Frames the whole board at the next frame.
    pub fn refit(&mut self) {
        self.framing = Some(Framing::Board);
    }

    /// Frames sheet `id` whole at the next frame.
    pub fn frame_sheet(&mut self, id: Id) {
        self.framing = Some(Framing::Sheet(id));
    }

    /// Brings sheet `id` on screen at the next frame, if it is not.
    pub fn reveal_sheet(&mut self, id: Id) {
        self.framing = Some(Framing::Reveal(id));
    }

    /// Frames the sheet shown of presentation `id`, or its column while it has none.
    pub fn frame_presentation(&mut self, d: &Drawing, id: Id) {
        let shown = self.shown(d).filter(|s| d.presentation_of(s.id).is_some_and(|p| p.id == id));
        self.framing = Some(shown.map_or(Framing::Column(id), |s| Framing::Sheet(s.id)));
    }

    /// The status bar's hint on the sheet (docs/ui/drawing.md#placing-views): what the tool
    /// does at its step.
    pub fn status(&self) -> String {
        if self.framer.is_some() {
            return t!("drawing.hint.frame_view");
        }
        if let Some(m) = &self.message {
            return m.clone();
        }
        let back = || t!("drawing.hint.back");
        match self.tool {
            SheetTool::Select if self.grip.is_some() => t!("drawing.hint.grip"),
            SheetTool::Select => match (self.marquee, self.pointer) {
                (Some(from), Some(p)) if p.x < from.x => t!("tool.select.hint.crossing"),
                (Some(_), _) => t!("tool.select.hint.window"),
                _ if self.moving.is_some() => t!("drawing.hint.moving"),
                _ if !self.open.is_empty() => t!("drawing.hint.select_open"),
                _ => t!("drawing.hint.select"),
            },
            SheetTool::Project { .. } => t!("drawing.hint.project"),
            SheetTool::Place => t!("drawing.hint.place_view"),
            SheetTool::Text => t!("drawing.hint.text"),
            SheetTool::Rect if self.sketch.is_some() => t!("drawing.hint.rect_second"),
            SheetTool::Circle if self.sketch.is_some() => t!("drawing.hint.circle_second"),
            SheetTool::Line if self.sketch.is_some() => t!("drawing.hint.line_second"),
            SheetTool::Rect => t!("drawing.hint.rect"),
            SheetTool::Circle => t!("drawing.hint.circle"),
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

    /// What the measurements box shows (its label, the current value), if it takes a value.
    pub fn vcb(&self) -> Option<(String, String)> {
        self.measure.clone()
    }

    /// `tool` started, what the last one was doing dropped.
    pub fn set_tool(&mut self, tool: SheetTool) {
        self.amend = None;
        self.message = None;
        self.tool = tool;
        self.picks.clear();
        self.sketch = None;
        self.placing = None;
    }
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
    subject_of_paths(e.document(), &paths)
}

/// A new sheet, A3 landscape, in the presentation shown, shown.
pub fn new_sheet(e: &mut Editor, st: &mut SheetState) {
    let presentation = st.presentation(e.document().drawing()).map(|p| p.id);
    if let Ok(Some(o)) = e.apply(Operation::AddSheet { presentation, name: None, paper: Paper::A3 })
        && let Some(s) = o.sheet
    {
        st.show_sheet(s);
    }
}

/// A sheet from `template`'s `sheet` (none: its first) in the presentation shown, « Feuille n »
/// unless `name` is given and free there, shown. Was it made?
pub fn sheet_from(
    e: &mut Editor,
    st: &mut SheetState,
    template: Arc<skraft_model::Document>,
    sheet: Option<Id>,
    name: Option<String>,
) -> bool {
    let shown = st.presentation(e.document().drawing());
    // A name its presentation already has: « Feuille n » instead.
    let name = name.filter(|n| !shown.is_some_and(|p| p.sheets.iter().any(|s| s.name == *n)));
    let presentation = shown.map(|p| p.id);
    match e.apply(Operation::AddSheetFrom { template, sheet, presentation, name }) {
        Ok(Some(o)) => {
            if let Some(s) = o.sheet {
                st.show_sheet(s);
            }
            true
        }
        _ => false,
    }
}

/// A presentation from `template`, shown. Was it made?
pub fn presentation_from(e: &mut Editor, st: &mut SheetState, template: Arc<skraft_model::Document>) -> bool {
    match e.apply(Operation::AddPresentationFrom { template, name: None }) {
        Ok(Some(o)) => {
            if let Some(p) = o.presentation {
                st.show_presentation(e.document().drawing(), p);
            }
            true
        }
        _ => false,
    }
}

/// A new presentation with a first sheet, A3 landscape, shown.
pub fn new_presentation(e: &mut Editor, st: &mut SheetState) {
    if let Ok(Some(o)) = e.apply(Operation::AddPresentation { name: None, paper: Some(Paper::A3) }) {
        st.presentation = o.presentation;
        if let Some(s) = o.sheet {
            st.show_sheet(s);
        }
    }
}

/// A new view being placed: what it shows, the size of its box seen from the front (model units).
#[derive(Clone, Debug)]
pub struct Placing {
    subject: Subject,
    size: Option<DVec2>,
}

/// « Nouvelle vue »: the dialog asking what it shows, the 3D selection ticked in advance
/// (decision 0051).
pub fn add_view(e: &mut Editor, st: &mut SheetState) -> Result<(), String> {
    st.shown(e.document().drawing()).ok_or_else(|| t!("drawing.no_sheet"))?;
    let subject = subject_of_selection(e);
    st.picker = Some(ViewPicker::new(e.document(), Purpose::New, subject));
    Ok(())
}

/// The dialog's answer: a new view to place, or view `id` showing `subject`, its scale and place
/// kept.
pub fn picked(e: &mut Editor, st: &mut SheetState, purpose: Purpose, subject: Subject) -> Result<(), String> {
    match purpose {
        Purpose::New => {
            let (frame, perspective) = Look::Standard(Direction::Front).frame();
            let probe = Resolved { subject: &subject, frame, perspective, scale: 1.0, at: DVec2::ZERO, crop: None };
            let size = subject_box(e, &probe).map(|(lo, hi)| hi - lo);
            st.set_tool(SheetTool::Place);
            st.placing = Some(Placing { subject, size });
            Ok(())
        }
        Purpose::Change(id) => {
            let Some(ElementKind::View(view)) = e.document().drawing().element(id).map(|el| el.kind.clone()) else { return Ok(()) };
            let ViewKind::Base { subject: was, look, scale, at } = view.kind.clone() else { return Ok(()) };
            if was == subject {
                return Ok(());
            }
            let kind = ElementKind::View(View { kind: ViewKind::Base { subject, look, scale, at }, crop: None, ..view });
            e.apply(Operation::SetElement { element: id, kind }).map(|_| ())
        }
    }
}

/// The view picked on the sheet shown, if it is still there.
pub fn selected_view<'a>(e: &'a Editor, st: &SheetState) -> Option<(Id, &'a View)> {
    selected_element(e, st).and_then(|el| match &el.kind {
        ElementKind::View(v) => Some((el.id, v)),
        _ => None,
    })
}

/// « Cadrer la vue » on the view picked.
pub fn frame_view(e: &Editor, st: &mut SheetState) -> Result<(), String> {
    let id = selected_view(e, st).map(|(id, _)| id).ok_or_else(|| t!("drawing.no_view_selected"))?;
    crate::workspaces::drawing::dialogs::view_framer::open(e, st, id)
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
        ElementKind::Circle { center, radius } => ElementKind::Circle { center: *center + offset, radius: *radius },
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
        ElementKind::Occurrence(i) => {
            let mut i = i.clone();
            i.place.at += offset;
            ElementKind::Occurrence(i)
        }
    }
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
fn projection_at(e: &Editor, d: &Drawing, base: Id, p: DVec2, projection: Projection) -> Option<(Side, f64)> {
    let r = d.resolve_as(base, projection)?;
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

/// « 1:10 », « 2:1 ».
pub fn scale_label(scale: f64) -> String {
    let n = |v: f64| if (v - v.round()).abs() < 1e-9 { format!("{}", v.round()) } else { format!("{v:.3}") };
    if scale >= 1.0 { format!("{}:1", n(scale)) } else { format!("1:{}", n(1.0 / scale)) }
}

#[cfg(test)]
mod tests {
    use skraft_model::kernel::{Job, JobId, JobResult, KernelService};
    use skraft_model::{Corner, Document, Place, drawing::Occurrence};

    use super::*;

    /// No kernel: these sheets hold no solid.
    struct NoKernel;
    impl KernelService for NoKernel {
        fn submit(&mut self, _: JobId, _: Job) {}
        fn poll(&mut self) -> Vec<(JobId, JobResult)> {
            Vec::new()
        }
    }

    /// An A4 sheet holding a composition — a 40 × 20 rectangle left of its origin and a line — placed
    /// at 10 mm from the paper's bottom right corner.
    fn sheet_with_a_composition() -> (Editor, Id, Id) {
        let mut e = Editor::new(Document::new(), Box::new(NoKernel));
        let sheet = e.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A4 }).unwrap().unwrap().sheet.unwrap();
        let to = Container::Sheet(sheet);
        let mut add = |element| e.apply(Operation::AddElement { to, element }).unwrap().unwrap().element.unwrap();
        let rect = add(ElementKind::Rect { min: DVec2::new(-40.0, 0.0), max: DVec2::new(0.0, 20.0) });
        let line = add(ElementKind::Line { from: DVec2::new(-40.0, 10.0), to: DVec2::new(0.0, 10.0) });
        let group = Operation::MakeComposition { elements: vec![rect, line], name: Some("Cartouche".into()) };
        let i = e.apply(group).unwrap().unwrap().element.unwrap();
        let composition = e.document().drawing().element(i).unwrap().kind.occurrence().unwrap().composition;
        let place = Place::anchored(Corner::BottomRight, DVec2::new(-10.0, 10.0));
        let kind = ElementKind::Occurrence(Occurrence { composition, place, props: Default::default() });
        e.apply(Operation::SetElement { element: i, kind }).unwrap();
        (e, sheet, i)
    }

    /// Holds the kernel's jobs; [`settle`] runs them.
    #[derive(Clone, Default)]
    struct Hold {
        jobs: std::rc::Rc<std::cell::RefCell<Vec<(JobId, Job)>>>,
        ready: std::rc::Rc<std::cell::RefCell<Vec<(JobId, JobResult)>>>,
    }
    impl KernelService for Hold {
        fn submit(&mut self, id: JobId, job: Job) {
            self.jobs.borrow_mut().push((id, job));
        }
        fn poll(&mut self) -> Vec<(JobId, JobResult)> {
            std::mem::take(&mut *self.ready.borrow_mut())
        }
    }
    fn settle(e: &mut Editor, hold: &Hold) {
        loop {
            let jobs: Vec<(JobId, Job)> = std::mem::take(&mut *hold.jobs.borrow_mut());
            if jobs.is_empty() {
                break;
            }
            let results = skraft_kernel::parallel_map(&jobs, |(id, job)| (*id, skraft_kernel::run(job)));
            hold.ready.borrow_mut().extend(results);
            e.poll();
        }
    }

    /// The stamp of view `id`'s picture in `style`, at a fixed size: what changes it is the model.
    fn picture_stamp(e: &Editor, id: Id, style: ViewStyle) -> u64 {
        let r = e.document().drawing().resolve(id).unwrap();
        let data = ViewsCache::default().get(e, id, &r);
        wanted_picture((id, None), style, &r, &data, None, egui::vec2(400.0, 300.0), 2.0).unwrap().stamp
    }

    #[test]
    fn a_view_picture_is_drawn_again_when_the_model_changes() {
        let mut h = skraft_model::History::new(Document::new());
        let root = h.document().root();
        let material =
            h.apply(Operation::AddMaterial { name: "M".into(), color: [200, 40, 40], kind: Default::default() }).unwrap().material.unwrap();
        let other =
            h.apply(Operation::AddMaterial { name: "O".into(), color: [10, 10, 10], kind: Default::default() }).unwrap().material.unwrap();
        let primitive = skraft_model::Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(1000.0, 500.0, 300.0) };
        let made =
            h.apply(Operation::AddPrimitive { context: root, primitive, placement: DAffine3::IDENTITY, material: Some(material) }).unwrap();
        let occurrence = made.created[0];
        let hold = Hold::default();
        let mut e = Editor::new(h.document().clone(), Box::new(hold.clone()));
        settle(&mut e, &hold);
        let sheet = e.apply(Operation::AddPresentation { name: None, paper: Some(Paper::A3) }).unwrap().unwrap().sheet.unwrap();
        let look = skraft_model::Look::Camera { forward: DVec3::Y, up: DVec3::Z, perspective: None };
        let kind = ViewKind::Base { subject: skraft_model::Subject::Model, look, scale: 0.2, at: DVec2::new(200.0, 150.0) };
        let element = ElementKind::View(View { name: "V".into(), kind, style: ViewStyle::Rendered, crop: None });
        let id = e.apply(Operation::AddElement { to: Container::Sheet(sheet), element }).unwrap().unwrap().element.unwrap();
        settle(&mut e, &hold);
        let rendered = picture_stamp(&e, id, ViewStyle::Rendered);
        let lines = picture_stamp(&e, id, ViewStyle::Lines);

        // A material it does not use: nothing to draw again.
        e.apply(Operation::EditMaterial { material: other, name: "O".into(), color: [20, 20, 20] }).unwrap();
        settle(&mut e, &hold);
        assert_eq!(picture_stamp(&e, id, ViewStyle::Rendered), rendered);

        // The one it uses: its picture, not a picture of lines alone.
        e.apply(Operation::EditMaterial { material, name: "M".into(), color: [40, 200, 40] }).unwrap();
        settle(&mut e, &hold);
        let recoloured = picture_stamp(&e, id, ViewStyle::Rendered);
        assert_ne!(recoloured, rendered);
        assert_eq!(picture_stamp(&e, id, ViewStyle::Lines), lines);

        // Its shape, the part keeping its id and place.
        e.apply(Operation::Stretch {
            context: vec![],
            occurrences: vec![occurrence],
            frame: DAffine3::IDENTITY,
            axis: DVec3::X,
            cuts: vec![500.0],
            moves: vec![0.0, 20.0],
            scope: Default::default(),
            cutters: None,
        })
        .unwrap();
        settle(&mut e, &hold);
        assert_ne!(picture_stamp(&e, id, ViewStyle::Rendered), recoloured);
        assert_ne!(picture_stamp(&e, id, ViewStyle::Lines), lines, "its frame moved");
    }

    #[test]
    fn a_view_lists_the_texture_images_it_shows() {
        let mut h = skraft_model::History::new(Document::new());
        let root = h.document().root();
        let textured = |h: &mut skraft_model::History, byte: u8| {
            let material = h
                .apply(Operation::AddMaterial { name: format!("M{byte}"), color: [9, 9, 9], kind: Some(skraft_model::MaterialKind::Solid) })
                .unwrap()
                .material
                .unwrap();
            let image = skraft_model::Asset::new([&b"\x89PNG\r\n\x1a\n"[..], &[byte]].concat()).unwrap();
            let texture = skraft_model::Texture { image: image.id(), size: [100.0, 100.0], rotation: 0.0 };
            let id = texture.image.clone();
            h.apply(Operation::SetTexture { material, texture: Some(texture), image: Some(image), color: None }).unwrap();
            (material, id)
        };
        let (shown, image) = textured(&mut h, 1);
        textured(&mut h, 2);
        let primitive = skraft_model::Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) };
        h.apply(Operation::AddPrimitive { context: root, primitive, placement: DAffine3::IDENTITY, material: Some(shown) }).unwrap();
        let hold = Hold::default();
        let mut e = Editor::new(h.document().clone(), Box::new(hold.clone()));
        settle(&mut e, &hold);
        let placed = e.document().subject_parts(&skraft_model::Subject::Model);
        let parts: Vec<(Id, Vec<Placed>)> = placed.iter().map(|p| (p.part, vec![(p.transform, p.shown())])).collect();
        assert_eq!(&*images_of(&e, &parts), &[image], "the material it shows, not the other");
    }

    /// What a press lands on, the sheet drawn at a point per millimetre about the screen's origin.
    fn frames_of(e: &Editor, sheet: Id) -> Vec<(Id, egui::Rect)> {
        let ctx = egui::Context::default();
        let mut out = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            let pv = Paperview::new(egui::Pos2::ZERO, DVec2::ZERO, 1.0);
            let d = e.document().drawing();
            let placed = d.placed(sheet);
            let (mut lines, mut views, mut wanted) = (Default::default(), ViewsCache::default(), Vec::new());
            let level = Level { open: &[], place: DAffine2::IDENTITY, printed: false, accent: egui::Color32::BLUE };
            out = draw_elements(
                &painter,
                &pv,
                e,
                d,
                sheet,
                &placed,
                level,
                &[],
                &HashMap::new(),
                &mut lines,
                &mut views,
                &mut wanted,
                &mut HashSet::new(),
                None,
            )
            .frames;
        });
        // No renderer to take the font atlas.
        output.textures_delta.clear();
        out
    }

    #[test]
    fn an_occurrence_is_picked_whole_where_it_shows_its_composition() {
        let (mut e, sheet, i) = sheet_with_a_composition();
        // Its origin at (287, 10): the rectangle from (247, 10) to (287, 30), y down on screen,
        // and 3 points on each side for its line's reach.
        let frames = frames_of(&e, sheet);
        assert_eq!(frames.len(), 1, "one box for what it shows");
        assert_eq!(frames[0].0, i);
        let want = egui::Rect::from_min_max(egui::pos2(244.0, -30.0), egui::pos2(290.0, -10.0));
        assert!(frames[0].1.min.distance(want.min) < 1e-3 && frames[0].1.max.distance(want.max) < 1e-3, "{:?}", frames[0].1);
        // Locked: drawn, not picked. Hidden: neither.
        e.apply(Operation::SetElementState { elements: vec![i], hidden: None, locked: Some(true) }).unwrap();
        assert!(frames_of(&e, sheet).is_empty());
        e.apply(Operation::SetElementState { elements: vec![i], hidden: Some(true), locked: Some(false) }).unwrap();
        assert!(frames_of(&e, sheet).is_empty());
    }

    #[test]
    fn a_point_snaps_on_what_an_occurrence_shows() {
        let (e, sheet, i) = sheet_with_a_composition();
        let d = e.document().drawing();
        let (on, align) = snap_points(&e, d, &mut ViewsCache::default(), sheet, &[], &[]);
        for q in [DVec2::new(247.0, 10.0), DVec2::new(287.0, 30.0), DVec2::new(247.0, 20.0)] {
            assert!(on.iter().any(|p| p.distance(q) < 1e-9) && align.iter().any(|p| p.distance(q) < 1e-9), "{q}");
        }
        // Not on the occurrence being dragged.
        assert!(snap_points(&e, d, &mut ViewsCache::default(), sheet, &[], &[i]).0.is_empty());
    }

    #[test]
    fn an_anchored_occurrence_moves_from_its_corner() {
        let (e, _, i) = sheet_with_a_composition();
        let kind = &e.document().drawing().element(i).unwrap().kind;
        let moved = moved_element(kind, DVec2::new(-5.0, 2.0));
        let place = moved.occurrence().unwrap().place;
        assert_eq!((place.corner, place.at), (Some(Corner::BottomRight), DVec2::new(-15.0, 12.0)));
    }

    #[test]
    fn a_composition_entered_is_what_a_press_lands_on_in_its_own_axes() {
        let (mut e, sheet, i) = sheet_with_a_composition();
        // Something else on the sheet: faded, out of reach.
        e.apply(Operation::AddElement { to: Container::Sheet(sheet), element: ElementKind::Rect { min: DVec2::ZERO, max: DVec2::ONE } })
            .unwrap();
        let d = e.document().drawing();
        let (to, place) = d.opened(sheet, &[i]).unwrap();
        let list = d.elements_in(to).unwrap();
        let ctx = egui::Context::default();
        let mut drawn = Drawn::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            let pv = Paperview::new(egui::Pos2::ZERO, DVec2::ZERO, 1.0);
            let placed = d.placed(sheet);
            let (mut lines, mut views, mut wanted) = (Default::default(), ViewsCache::default(), Vec::new());
            let level = Level { open: &[i], place, printed: false, accent: egui::Color32::BLUE };
            drawn = draw_elements(
                &painter,
                &pv,
                &e,
                d,
                sheet,
                &placed,
                level,
                &[],
                &HashMap::new(),
                &mut lines,
                &mut views,
                &mut wanted,
                &mut HashSet::new(),
                None,
            );
        });
        output.textures_delta.clear();
        let ids: Vec<Id> = drawn.frames.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, list.iter().map(|el| el.id).collect::<Vec<_>>(), "what the composition holds, nothing else");
        assert_eq!(drawn.boxes[&list[0].id], (DVec2::new(-40.0, 0.0), DVec2::new(0.0, 20.0)), "in the composition's axes");
        // Its box a little wider than what it shows.
        assert_eq!(drawn.level_box, Some((DVec2::new(-42.0, -2.0), DVec2::new(2.0, 22.0))));
        // The pointer read in its axes: the paper's (287, 10) is its origin.
        let lv = Paperview::new(egui::Pos2::ZERO, DVec2::ZERO, 1.0).placed(place);
        assert!(lv.to_sheet(egui::pos2(287.0, -10.0)).length() < 1e-3);
    }

    #[test]
    fn levels_are_entered_and_left_the_occurrence_picked() {
        let (e, sheet, i) = sheet_with_a_composition();
        let d = e.document().drawing();
        let mut st = SheetState { current: Some(sheet), ..Default::default() };
        assert_eq!(st.container(d), Some(Container::Sheet(sheet)));
        st.selected = vec![i];
        edit_selected(&e, &mut st).unwrap();
        let composition = d.element(i).unwrap().kind.occurrence().unwrap().composition;
        assert_eq!((st.container(d), st.selected.len()), (Some(Container::Composition(composition)), 0));
        st.leave();
        assert_eq!((st.container(d), st.selected.as_slice()), (Some(Container::Sheet(sheet)), &[i][..]));
    }

    #[test]
    fn a_definition_placed_on_another_sheet_takes_its_corner() {
        let (mut e, _, i) = sheet_with_a_composition();
        let composition = e.document().drawing().element(i).unwrap().kind.occurrence().unwrap().composition;
        let other = e.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().unwrap().sheet.unwrap();
        let mut st = SheetState { current: Some(other), ..Default::default() };
        place_composition(&mut e, &mut st, composition);
        let d = e.document().drawing();
        let placed = st.selected[0];
        let p = d.element(placed).unwrap().kind.occurrence().unwrap().place;
        assert_eq!((p.corner, p.at), (Some(Corner::BottomRight), DVec2::new(-10.0, 10.0)));
        assert_eq!(d.container_of(placed), Some(Container::Sheet(other)));
        // Again on the same sheet: a little aside.
        place_composition(&mut e, &mut st, composition);
        let p = e.document().drawing().element(st.selected[0]).unwrap().kind.occurrence().unwrap().place;
        assert_eq!(p.at, DVec2::new(0.0, 0.0));
    }

    #[test]
    fn the_board_comes_back_from_what_a_file_keeps() {
        let (e, sheet, _) = sheet_with_a_composition();
        let d = e.document().drawing();
        assert_eq!(SheetState::default().seen(), None, "never shown: nothing to keep");
        let seen =
            SeenBoard { sheet: Some(sheet), presentation: d.presentation_of(sheet).map(|p| p.id), pan: DVec2::new(12.0, -3.0), zoom: 2.5 };
        let mut st = SheetState::default();
        st.restore(d, &seen);
        assert_eq!(st.seen(), Some(seen));
        // Kept as it is: the first frame does not bring the sheet back on screen.
        follow_active(&mut st, &Board::of(d), Some(sheet));
        assert_eq!((st.framing, st.pan), (None, seen.pan));

        let mut gone = SheetState::default();
        gone.restore(d, &SeenBoard { sheet: Some(Id(999_999)), presentation: Some(Id(999_998)), ..seen });
        assert_eq!((gone.current, gone.presentation), (None, None));
        let mut bad = SheetState::default();
        bad.restore(d, &SeenBoard { zoom: f64::NAN, ..seen });
        assert_eq!(bad.seen(), None, "left to be framed");
    }

    /// A real click at `at` at time `t` (seconds), through egui: was it taken from the tools?
    fn click_on_stage(ctx: &egui::Context, st: &mut SheetState, board: &Board, d: &Drawing, at: egui::Pos2, t: f64) -> bool {
        let stage = egui::Rect::from_center_size(egui::pos2(500.0, 500.0), egui::Vec2::splat(1000.0));
        let press =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let mut taken = false;
        for (k, event) in [egui::Event::PointerMoved(at), press(true), press(false)].into_iter().enumerate() {
            let raw = egui::RawInput { time: Some(t + k as f64 * 0.01), events: vec![event], ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| {
                let r = ui.interact(stage, egui::Id::unique("stage"), egui::Sense::click_and_drag());
                let active = st.shown(d).map(|s| s.id);
                taken |= activate(&r, st, board, stage.center(), active);
            });
            output.textures_delta.clear();
        }
        taken
    }

    #[test]
    fn a_click_makes_another_sheet_active_and_a_double_click_frames_it() {
        let (mut e, a, _) = sheet_with_a_composition();
        let b = e.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().unwrap().sheet.unwrap();
        let d = e.document().drawing();
        let board = Board::of(d);
        let (lo, hi) = board.bounds().unwrap();
        let mut st = SheetState { current: Some(a), zoom: 1.0, pan: (lo + hi) / 2.0, ..Default::default() };
        let view = Paperview::new(egui::pos2(500.0, 500.0), st.pan, 1.0);
        let middle = |id: Id| {
            let s = board.sheet(id).unwrap();
            view.at(s.origin).to_screen(s.size / 2.0)
        };
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        let pan = st.pan;
        // A click: active, the camera still, nothing for the tools.
        assert!(click_on_stage(&ctx, &mut st, &board, d, middle(b), 1.0));
        assert_eq!((st.current, st.framing, st.pan), (Some(b), None, pan));
        // The second of a double click frames it, and is not the tools' either.
        assert!(click_on_stage(&ctx, &mut st, &board, d, middle(b), 1.1));
        assert_eq!(st.framing, Some(Framing::Sheet(b)));
        st.framing = None;
        // Later, a click on the active sheet is the tools'.
        assert!(!click_on_stage(&ctx, &mut st, &board, d, middle(b), 3.0));
        assert_eq!((st.current, st.framing), (Some(b), None));
        // While a tool is under way its clicks are its own.
        st.sketch = Some(DVec2::ZERO);
        assert!(!click_on_stage(&ctx, &mut st, &board, d, middle(a), 5.0));
        assert_eq!(st.current, Some(b));
        st.sketch = None;
        // What the active sheet shows past its paper is picked before the sheet under it.
        st.frames = vec![(Id(99), egui::Rect::from_center_size(middle(a), egui::Vec2::splat(20.0)))];
        assert!(!click_on_stage(&ctx, &mut st, &board, d, middle(a), 7.0));
        assert_eq!(st.current, Some(b));
    }

    #[test]
    fn the_camera_follows_the_active_sheet_and_reveals_a_new_one() {
        let (mut e, a, _) = sheet_with_a_composition();
        let b = e.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().unwrap().sheet.unwrap();
        let board = Board::of(e.document().drawing());
        let at = board.sheet(b).unwrap().origin;
        let mut st = SheetState { current: Some(b), zoom: 1.0, follow: Some((b, at + DVec2::new(0.0, 100.0))), ..Default::default() };
        follow_active(&mut st, &board, Some(b));
        assert_eq!((st.pan, st.framing), (DVec2::new(0.0, -100.0), None), "moved with it");
        // Another one active, not by a click on it: brought on screen if it is not.
        follow_active(&mut st, &board, Some(a));
        assert_eq!(st.framing, Some(Framing::Reveal(a)));
    }

    #[test]
    fn a_projected_view_is_kept_per_projection_and_a_base_view_once() {
        let id = Id(5);
        let view = |kind| View { name: String::new(), kind, style: ViewStyle::Lines, crop: None };
        let projected = view(ViewKind::Projected { base: Id(4), side: Side::Left, distance: 10.0 });
        assert_ne!(slot_of(id, &projected, Projection::FirstAngle), slot_of(id, &projected, Projection::ThirdAngle));
        let base = view(ViewKind::Base { subject: Subject::Model, look: Look::Standard(Direction::Front), scale: 1.0, at: DVec2::ZERO });
        assert_eq!(slot_of(id, &base, Projection::FirstAngle), slot_of(id, &base, Projection::ThirdAngle));
    }

    #[test]
    fn pictures_over_the_budget_go_the_least_recently_seen_first() {
        let pictures = [('a', 40, 3), ('b', 40, 1), ('c', 40, 5), ('d', 40, 2)];
        assert!(over_budget(pictures.into_iter(), 5, 160).is_empty(), "they fit");
        assert_eq!(over_budget(pictures.into_iter(), 5, 100), ['b', 'd']);
        // What is on screen stays, however much it takes.
        assert_eq!(over_budget(pictures.map(|(k, b, _)| (k, b, 5)).into_iter(), 5, 10), Vec::<char>::new());
    }

    #[test]
    fn a_selection_let_go_over_another_sheet_moves_there() {
        let (mut e, a, title_block) = sheet_with_a_composition();
        let b = e.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().unwrap().sheet.unwrap();
        let min = DVec2::new(10.0, 10.0);
        let kind = ElementKind::Rect { min, max: DVec2::new(50.0, 30.0) };
        let rect = e.apply(Operation::AddElement { to: Container::Sheet(a), element: kind }).unwrap().unwrap().element.unwrap();
        let board = Board::of(e.document().drawing());
        let (from, to) = (board.sheet(a).unwrap().origin, board.sheet(b).unwrap().origin);
        let mut st = SheetState { current: Some(a), zoom: 1.0, selected: vec![rect, title_block], ..Default::default() };
        let pan = st.pan;
        drop_on_sheet(&mut e, &mut st, &board, a, b, DVec2::new(5.0, -300.0));
        let d = e.document().drawing();
        assert_eq!((st.current, st.follow, st.pan), (Some(b), Some((b, to)), pan), "active, the camera still");
        assert_eq!(d.container_of(rect), Some(Container::Sheet(b)));
        assert_eq!(st.selected.len(), 2, "what moved picked");
        // Where it was let go on the board.
        let Some(ElementKind::Rect { min: got, .. }) = d.element(rect).map(|el| &el.kind) else { panic!() };
        assert!(got.distance(from + min + DVec2::new(5.0, -300.0) - to) < 1e-9, "{got}");
        // Anchored, it keeps its corner.
        let p = d.element(title_block).unwrap().kind.occurrence().unwrap().place;
        assert_eq!((p.corner, p.at), (Some(Corner::BottomRight), DVec2::new(-10.0, 10.0)));
        e.undo();
        assert_eq!(e.document().drawing().container_of(rect), Some(Container::Sheet(a)), "one step");
    }

    #[test]
    fn a_projected_view_or_a_dimension_leaves_its_sheet_only_with_its_base() {
        let (mut e, a, title_block) = sheet_with_a_composition();
        let to = Container::Sheet(a);
        let base = View {
            name: "Base".into(),
            kind: ViewKind::Base { subject: Subject::Model, look: Look::Standard(Direction::Front), scale: 1.0, at: DVec2::ZERO },
            style: ViewStyle::Lines,
            crop: None,
        };
        let base = e.apply(Operation::AddElement { to, element: ElementKind::View(base) }).unwrap().unwrap().element.unwrap();
        let side = View {
            name: "Side".into(),
            kind: ViewKind::Projected { base, side: Side::Right, distance: 100.0 },
            style: ViewStyle::Lines,
            crop: None,
        };
        let side = e.apply(Operation::AddElement { to, element: ElementKind::View(side) }).unwrap().unwrap().element.unwrap();
        let d = e.document().drawing();
        assert!(!can_leave_sheet(d, &[]));
        assert!(can_leave_sheet(d, &[title_block]));
        assert!(can_leave_sheet(d, &[base]), "with what hangs on it");
        assert!(!can_leave_sheet(d, &[side]), "without its base");
        assert!(!can_leave_sheet(d, &[title_block, side]), "the whole selection stays");
        assert!(can_leave_sheet(d, &[base, side]));
    }

    #[test]
    fn a_sheet_copied_from_the_navigator_becomes_active() {
        let (mut e, sheet, _) = sheet_with_a_composition();
        let o = e.apply(Operation::AddPresentation { name: None, paper: None }).unwrap().unwrap();
        let presentation = o.presentation.unwrap();
        let mut st = SheetState { current: Some(sheet), zoom: 1.0, ..Default::default() };
        run_ask(&mut e, &mut st, SheetAsk::CopySheet { sheet, presentation, index: 0 }, egui::Pos2::ZERO);
        let d = e.document().drawing();
        let copy = d.presentation(presentation).unwrap().sheets[0].id;
        assert_ne!(copy, sheet);
        assert_eq!(st.current, Some(copy));
    }

    #[test]
    fn a_composition_carried_lands_in_the_level_open_or_right_on_another_sheet() {
        let (mut e, a, i) = sheet_with_a_composition();
        let b = e.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().unwrap().sheet.unwrap();
        let d = e.document().drawing();
        let board = Board::of(d);
        let composition = d.element(i).unwrap().kind.occurrence().unwrap().composition;
        let view = Paperview::new(egui::Pos2::ZERO, DVec2::ZERO, 1.0);
        let level = Some((a, Container::Composition(composition), view.at(board.sheet(a).unwrap().origin)));
        let (s, to, _) = carry_target(&board, view, level, a).unwrap();
        assert_eq!((s, to), (a, Container::Composition(composition)), "the level open on the active sheet");
        let (s, to, lv) = carry_target(&board, view, level, b).unwrap();
        assert_eq!((s, to, lv.origin), (b, Container::Sheet(b), board.sheet(b).unwrap().origin), "right on another");
    }

    #[test]
    fn a_box_is_brought_onto_a_smaller_paper() {
        let a4 = Paper::A4;
        let at = |x, y| DVec2::new(x, y);
        assert_eq!(offset_into(at(10.0, 10.0), at(50.0, 50.0), a4), DVec2::ZERO, "on it: left where it is");
        assert_eq!(offset_into(at(300.0, 250.0), at(400.0, 280.0), a4), at(-103.0, -70.0));
        assert_eq!(offset_into(at(-5.0, 10.0), at(20.0, 20.0), a4), at(5.0, 0.0));
        assert_eq!(offset_into(at(0.0, 0.0), at(397.0, 10.0), a4), at(-50.0, 0.0), "too wide: centred");
    }

    #[test]
    fn copying_on_a_sheet_pastes_at_the_pointer_in_the_level_open() {
        let (mut e, sheet, i) = sheet_with_a_composition();
        let mut st = SheetState { current: Some(sheet), zoom: 1.0, ..Default::default() };
        st.selected = vec![i];
        st.frames = vec![(i, egui::Rect::NOTHING)];
        st.boxes.insert(i, (DVec2::new(247.0, 10.0), DVec2::new(287.0, 30.0)));
        let (fragment, bounds) = copy_selected(&e, &st).unwrap();
        // The pointer at the paper's (100, 100), the stage's centre at its origin.
        st.pan = Board::of(e.document().drawing()).sheet(sheet).unwrap().origin;
        st.pointer = Some(egui::pos2(100.0, -100.0));
        run_ask(&mut e, &mut st, SheetAsk::Paste { fragment, bounds, in_place: false, linked: true }, egui::Pos2::ZERO);
        let d = e.document().drawing();
        assert_eq!(d.sheet(sheet).unwrap().elements.len(), 2);
        let copy = st.selected[0];
        assert_ne!(copy, i);
        let (a, b) = (d.element(i).unwrap().kind.occurrence().unwrap(), d.element(copy).unwrap().kind.occurrence().unwrap());
        assert_eq!(a.composition, b.composition, "the same composition");
        assert!(b.place.origin(Some(Paper::A4)).distance(DVec2::new(120.0, 90.0)) < 1e-9, "its box centred on the pointer");
        select_all(&e, &mut st);
        assert_eq!(st.selected.len(), 2);
    }

    #[test]
    fn what_a_placed_view_draws_turns_with_it() {
        let pv = Paperview::new(egui::pos2(100.0, 100.0), DVec2::ZERO, 2.0);
        assert!(!pv.turned() && pv.angle() == 0.0);
        let turned = pv.placed(DAffine2::from_angle_translation(std::f64::consts::FRAC_PI_2, DVec2::new(10.0, 0.0)));
        assert!(turned.turned());
        assert!((turned.angle() + std::f32::consts::FRAC_PI_2).abs() < 1e-6, "counterclockwise on paper, y down on screen");
        // (1, 0) turned to (0, 1), moved to (10, 1): 20 right of the centre, 2 up.
        assert!(turned.to_screen(DVec2::X).distance(egui::pos2(120.0, 98.0)) < 1e-3);
        // The box's top left corner (0, 1): at (9, 0).
        assert!(turned.quad(DVec2::ZERO, DVec2::new(2.0, 1.0))[0].distance(egui::pos2(118.0, 100.0)) < 1e-3);
        // What it covers on screen, whatever its turn: 2 wide × 4 high… at 2 points a millimetre.
        let r = turned.rect(DVec2::ZERO, DVec2::new(2.0, 1.0));
        assert!((r.size() - egui::vec2(2.0, 4.0)).length() < 1e-3, "{r:?}");
    }

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
    fn a_box_moved_snaps_on_a_point_then_aligns_then_takes_the_grid() {
        let (lo, hi) = (DVec2::ZERO, DVec2::new(10.0, 20.0));
        let free = (true, true);
        let q = DVec2::new(50.0, 50.0);
        // Its top right corner comes on the point.
        let (o, a) = aim_box(&[q], &[q], None, lo, hi, DVec2::new(39.0, 30.5), 2.0, 5.0, free);
        assert!(a.point);
        assert_eq!(o, DVec2::new(40.0, 30.0));
        // Its middle aligned along x, its low side on the grid along y.
        let (o, a) = aim_box(&[], &[q], None, lo, hi, DVec2::new(44.5, 3.2), 2.0, 5.0, free);
        assert_eq!(o, DVec2::new(45.0, 5.0));
        assert_eq!((a.along_y, a.along_x), (Some(q), None));
        // No point, no grid: as the pointer went.
        let free_offset = DVec2::new(3.3, 7.7);
        assert_eq!(aim_box(&[], &[], None, lo, hi, free_offset, 2.0, 0.0, free).0, free_offset);
        // Along x only: y left alone, and no point can take it.
        let (o, _) = aim_box(&[q], &[q], None, lo, hi, DVec2::new(39.0, 30.5), 2.0, 5.0, (true, false));
        assert_eq!(o, DVec2::new(40.0, 30.5));
    }

    #[test]
    fn a_box_moved_centres_on_the_paper_by_its_middle_only() {
        let (lo, hi) = (DVec2::ZERO, DVec2::new(10.0, 20.0));
        let c = Some(DVec2::new(100.0, 100.0));
        // Its middle comes on the centre.
        let (o, a) = aim_box(&[], &[], c, lo, hi, DVec2::new(94.0, 91.0), 2.0, 0.0, (true, true));
        assert_eq!(o, DVec2::new(95.0, 90.0));
        assert!(a.point);
        // Centred along x alone.
        let (o, a) = aim_box(&[], &[], c, lo, hi, DVec2::new(94.0, 40.0), 2.0, 0.0, (true, true));
        assert_eq!(o, DVec2::new(95.0, 40.0));
        assert_eq!((a.along_y, a.along_x), (c, None));
        // Its corner on the centre's line does not snap.
        let free = DVec2::new(99.0, 40.0);
        assert_eq!(aim_box(&[], &[], c, lo, hi, free, 2.0, 0.0, (true, true)).0, free);
    }

    #[test]
    fn a_text_handle_takes_the_grid_by_its_top() {
        let place = DAffine2::from_translation(DVec2::new(0.0, 3.0));
        // On the sheet 21.8 + 3 = 24.8: to 25.
        let q = top_on_grid(DVec2::new(7.0, 21.8), place, 5.0);
        assert!((q.y - 22.0).abs() < 1e-9 && q.x == 7.0);
        let free = DVec2::new(7.0, 21.8);
        assert_eq!(top_on_grid(free, place, 0.0), free);
    }

    #[test]
    fn a_circle_takes_its_radius_from_its_handle_or_a_value() {
        let units = skraft_model::Units::default();
        let g = sketch_grip(SheetTool::Circle, DVec2::new(10.0, 10.0), DVec2::new(13.0, 14.0));
        assert_eq!(g.kind, ElementKind::Circle { center: DVec2::new(10.0, 10.0), radius: 5.0 });
        assert_eq!(regrip(&g, DVec2::new(10.0, 18.0), false), ElementKind::Circle { center: DVec2::new(10.0, 10.0), radius: 8.0 });
        assert_eq!(typed_kind(&g, "12", &units).unwrap(), ElementKind::Circle { center: DVec2::new(10.0, 10.0), radius: 12.0 });
        assert!(typed_kind(&g, "0", &units).is_err());
    }

    #[test]
    fn a_value_typed_sizes_a_shape_the_way_it_goes() {
        let units = skraft_model::Units::default();
        // A rectangle going down and left from (10, 10): 100 × 50 that way.
        let g = sketch_grip(SheetTool::Rect, DVec2::new(10.0, 10.0), DVec2::new(0.0, 5.0));
        let kind = typed_kind(&g, "100;50", &units).unwrap();
        assert_eq!(kind, ElementKind::Rect { min: DVec2::new(-90.0, -40.0), max: DVec2::new(10.0, 10.0) });
        // A line: its length along where it goes, or its end relative to its start.
        let g = sketch_grip(SheetTool::Line, DVec2::ZERO, DVec2::new(3.0, 4.0));
        let ElementKind::Line { to, .. } = typed_kind(&g, "100", &units).unwrap() else { panic!() };
        assert!(to.distance(DVec2::new(60.0, 80.0)) < 1e-9);
        let ElementKind::Line { to, .. } = typed_kind(&g, "<30;40>", &units).unwrap() else { panic!() };
        assert!(to.distance(DVec2::new(30.0, 40.0)) < 1e-9);
        // Nonsense is refused.
        assert!(typed_kind(&g, "abc", &units).is_err());
    }

    #[test]
    fn values_are_in_the_documents_unit() {
        let units = skraft_model::Units { length: skraft_model::LengthUnit::Inch, ..Default::default() };
        let g = sketch_grip(SheetTool::Rect, DVec2::ZERO, DVec2::new(10.0, 10.0));
        let ElementKind::Rect { max, .. } = typed_kind(&g, "2;1", &units).unwrap() else { panic!() };
        assert!(max.distance(DVec2::new(50.8, 25.4)) < 1e-9);
        // An explicit unit wins.
        let ElementKind::Rect { max, .. } = typed_kind(&g, "10mm;1", &units).unwrap() else { panic!() };
        assert!(max.distance(DVec2::new(10.0, 25.4)) < 1e-9);
    }

    #[test]
    fn a_side_handle_takes_one_length_from_the_opposite_side() {
        let units = skraft_model::Units::default();
        let kind = ElementKind::Rect { min: DVec2::ZERO, max: DVec2::new(40.0, 20.0) };
        let g = Grip {
            element: Id(1),
            handle: Handle::Side(-1, 0),
            start: DVec2::ZERO,
            from: DVec2::ZERO,
            kind,
            lo: DVec2::ZERO,
            hi: DVec2::ONE,
        };
        assert_eq!(typed_kind(&g, "100", &units).unwrap(), ElementKind::Rect { min: DVec2::new(-60.0, 0.0), max: DVec2::new(40.0, 20.0) });
    }

    #[test]
    fn scales_are_typed_as_ratios() {
        assert_eq!(parse_scale("1:20"), Some(0.05));
        assert_eq!(parse_scale("2:1"), Some(2.0));
        assert_eq!(parse_scale("1/4"), Some(0.25));
        assert_eq!(parse_scale("0,5"), Some(0.5));
        assert_eq!(parse_scale("0:1"), None);
        assert_eq!(parse_scale("abc"), None);
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
        let f = Fields { document: "Table.skraft", sheet: "Plan", number: 2, count: 3, scale: Some(0.1), values: None };
        assert_eq!(f.fill("{document} · {sheet} {sheet.number}/{sheet.count} · {scale}", &[]), "Table.skraft · Plan 2/3 · 1:10");
        assert_eq!(f.fill("{nope} and {", &[]), "{nope} and {");
        assert_eq!(f.fill("{date}", &[]).len(), 10, "dd/mm/yyyy");
    }

    #[test]
    fn a_field_is_filled_from_the_nearest_value() {
        let (mut e, sheet, i) = sheet_with_a_composition();
        let text = |s: &str| Some(skraft_model::Value::Text(s.into()));
        let set = |on, value| Operation::SetField { on, key: "user.client".into(), value };
        let p = e.document().drawing().presentation_of(sheet).unwrap().id;
        e.apply(set(skraft_model::FieldOn::Presentation(p), text("Martin"))).unwrap();
        let fill = |e: &Editor, path: &[Id]| {
            let f = Fields { document: "", sheet: "", number: 1, count: 1, scale: None, values: Some((e.document(), sheet)) };
            f.fill("{client} {autre}", path)
        };
        assert_eq!(fill(&e, &[i]), "Martin {autre}", "from the presentation; one without a value left");
        e.apply(set(skraft_model::FieldOn::Occurrence(i), text("Dupont"))).unwrap();
        assert_eq!(fill(&e, &[i]), "Dupont {autre}");
        assert_eq!(fill(&e, &[]), "Martin {autre}", "not under the occurrence");
        assert_eq!(custom_fields(["client".to_owned(), "scale".to_owned(), "sheet.number".to_owned()]), vec!["client".to_owned()]);
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

mod board;
mod compositions;
mod dimensions;
mod fields;
mod grips;
pub(crate) mod lines;
mod paint;
mod pictures;
mod snap;
mod stage;
mod view_cache;

pub(crate) use compositions::*;
pub(crate) use dimensions::*;
pub(crate) use fields::*;
pub(crate) use grips::*;
use paint::*;
pub(crate) use pictures::*;
use snap::*;
pub(crate) use stage::*;
pub(crate) use view_cache::*;

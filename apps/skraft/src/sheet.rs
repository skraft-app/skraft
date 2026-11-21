//! The « Mise en plan » workspace's stage (docs/ui/drawing.md, decisions 0048 and 0050): the
//! board of every sheet of the document's drawing, panned and zoomed, one of them active with its
//! views' frames and its elements to edit; the navigator (`sheet_tree`) choosing it.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{DAffine2, DAffine3, DMat4, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::{
    Container, DimKind, Dimension, Direction, Drawing, ElementKind, Entity, EntityPath, Found, Id, Look, Operation, Orient, Paper, Place,
    Projection, Resolved, Sheet, Side, Subject, View, ViewKind, ViewStyle,
};
use skraft_theme::Theme;
use skraft_tools::Editor;

use crate::sheet_board::{Board, Column};
use crate::viewport;

/// A document's sheet shown and how it is seen (not saved).
#[derive(Clone, Debug, Default)]
pub struct SheetState {
    /// The sheet shown: none, the first of the presentation shown.
    pub current: Option<Id>,
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
    /// The selection being dragged, from where (sheet millimetres).
    moving: Option<DVec2>,
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
    /// The instances entered, from the sheet down (docs/ui/drawing.md#compositions): what they place
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
    /// The « Compositions » card's search, rows and outlines.
    pub library: crate::sheet_library::CompositionsState,
    /// The composition being dragged from that card, placed where it is let go.
    pub carried: Option<Id>,
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

/// The box of `el` in its container's axes; an instance has none of its own.
fn element_box(e: &Editor, el: &skraft_model::Element, projection: Projection) -> Option<(DVec2, DVec2)> {
    Some(match &el.kind {
        ElementKind::Rect { min, max } | ElementKind::Image { min, max, .. } => (*min, *max),
        ElementKind::Line { from, to } => (from.min(*to), from.max(*to)),
        ElementKind::Text { at, text, size } => {
            let columns = text.lines().map(|l| l.chars().count()).max().unwrap_or(1) as f64;
            let rows = text.lines().count().max(1) as f64;
            (*at, *at + DVec2::new(columns * size * 0.55, rows * size * 1.25))
        }
        ElementKind::View(_) => view_frame(e, &e.document().drawing().resolve_as(el.id, projection)?),
        ElementKind::Dimension(dim) => dim_shape(e, dim, projection)?.bounds(),
        ElementKind::Instance(_) => return None,
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
            if let Some(i) = el.kind.instance() {
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

/// Where a composition carried lands on sheet `under`: the level open on the active sheet
/// (`level`: the sheet, what is open, its view), right on the sheet on another.
fn carry_target(
    board: &Board,
    view: Paperview,
    level: Option<(Id, Container, Paperview)>,
    under: Id,
) -> Option<(Id, Container, Paperview)> {
    match level {
        Some(l) if l.0 == under => Some(l),
        _ => board.sheet(under).map(|s| (under, Container::Sheet(under), view.at(s.origin))),
    }
}

/// The composition dragged from the card: its outline at the pointer over a sheet, an instance
/// there, free, when the button is let go, that sheet becoming active; given up off the papers,
/// over a card, inside itself, or by Esc.
#[allow(clippy::too_many_arguments)]
fn carry_composition(
    ui: &egui::Ui,
    e: &mut Editor,
    st: &mut SheetState,
    room: egui::Rect,
    board: &Board,
    view: Paperview,
    level: Option<(Id, Container, Paperview)>,
) {
    let Some(composition) = st.carried else { return };
    let ctx = ui.ctx();
    let (pos, down, esc) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down(), i.key_pressed(egui::Key::Escape)));
    let d = e.document().drawing();
    if esc || d.composition(composition).is_none() {
        st.carried = None;
        return;
    }
    let over = pos.filter(|p| on_stage(ctx, room, *p));
    let target = over
        .and_then(|p| board.sheet_at(view.to_sheet(p)))
        .and_then(|under| carry_target(board, view, level, under))
        .filter(|(_, to, _)| !matches!(to, Container::Composition(x) if d.reaches(composition, *x)));
    // Outlined as the presentation it lands in projects its views.
    let projection = target.and_then(|(s, ..)| d.presentation_of(s)).map_or_else(|| st.projection(d), |p| p.projection);
    let Some(sketch) = st.library.sketch(e, composition, projection).cloned() else {
        if !down {
            st.carried = None;
        }
        return;
    };
    let at = over.zip(target).map(|(p, (_, _, lv))| lv.to_sheet(p) - (sketch.lo + sketch.hi) / 2.0);
    if let (Some(at), Some((_, _, lv))) = (at, target) {
        let stroke = egui::Stroke::new(1.0, ui.visuals().selection.stroke.color);
        let painter = ui.painter().with_clip_rect(room);
        painter
            .extend(sketch.segments.iter().map(|(a, b)| egui::Shape::line_segment([lv.to_screen(*a + at), lv.to_screen(*b + at)], stroke)));
    }
    if down {
        return;
    }
    st.carried = None;
    if let (Some(at), Some((sheet, to, _))) = (at, target) {
        let place = Place::free(at);
        let element = ElementKind::Instance(skraft_model::Instance { composition, place, props: Default::default() });
        match e.apply(Operation::AddElement { to, element }) {
            Ok(Some(o)) => {
                if level.map(|l| l.0) != Some(sheet) {
                    make_active(st, board, sheet);
                }
                st.selected = o.element.into_iter().collect();
            }
            Ok(None) => {}
            Err(m) => st.message = Some(m),
        }
    }
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
    let anchored = |id: Id| d.element(id).and_then(|el| el.kind.instance()).is_some_and(|i| i.place.corner.is_some());
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

/// The instances among them.
pub fn selected_instances<'a>(e: &'a Editor, st: &SheetState) -> Vec<(Id, &'a skraft_model::Instance)> {
    selection(e, st).into_iter().filter_map(|el| el.kind.instance().map(|i| (el.id, i))).collect()
}

/// The elements picked gathered into a composition placed there, picked.
pub fn compose_selected(e: &mut Editor, st: &mut SheetState) -> Result<(), String> {
    let elements: Vec<Id> = selection(e, st).iter().map(|el| el.id).collect();
    if elements.is_empty() {
        return Err(t!("drawing.no_view_selected"));
    }
    if let Some(o) = e.apply(Operation::MakeComposition { elements, name: None })? {
        st.selected = o.element.into_iter().collect();
    }
    Ok(())
}

/// The instances picked replaced by what their compositions hold, picked.
pub fn explode_selected(e: &mut Editor, st: &mut SheetState) -> Result<(), String> {
    let ops: Vec<Operation> = selected_instances(e, st).iter().map(|(id, _)| Operation::ExplodeInstance { instance: *id }).collect();
    let op = match ops.len() {
        0 => return Err(t!("drawing.no_view_selected")),
        1 => ops.into_iter().next().expect("one"),
        _ => Operation::Batch(ops),
    };
    if let Some(o) = e.apply(op)? {
        st.selected = o.elements;
    }
    Ok(())
}

/// The instances picked each given a composition of its own.
pub fn unique_selected(e: &mut Editor, st: &SheetState) -> Result<(), String> {
    let ops: Vec<Operation> = selected_instances(e, st).iter().map(|(id, _)| Operation::MakeInstanceUnique { instance: *id }).collect();
    match ops.len() {
        0 => Err(t!("drawing.no_view_selected")),
        1 => e.apply(ops.into_iter().next().expect("one")).map(|_| ()),
        _ => e.apply(Operation::Batch(ops)).map(|_| ()),
    }
}

/// The instance picked entered, its composition edited in place.
pub fn edit_selected(e: &Editor, st: &mut SheetState) -> Result<(), String> {
    let Some((id, _)) = selected_instances(e, st).first().copied() else { return Err(t!("drawing.no_view_selected")) };
    st.enter(id);
    Ok(())
}

/// Definition `composition` placed on the level open: where its first instance on a sheet is (a title
/// block at its corner), a little aside on the sheet already showing it.
fn place_composition(e: &mut Editor, st: &mut SheetState, composition: Id) {
    let d = e.document().drawing();
    let Some(s) = st.shown(d) else { return };
    let Some(to) = st.container(d) else { return };
    let on = |list: &[skraft_model::Element]| {
        list.iter().find_map(|el| el.kind.instance().filter(|i| i.composition == composition).map(|i| i.place))
    };
    let mut place = match on(d.elements_in(to).unwrap_or(&[])) {
        Some(p) => Place { at: p.at + DVec2::new(10.0, -10.0), ..p },
        None => d.sheets().find_map(|x| on(&x.elements)).unwrap_or_else(|| Place::free(s.paper.size() / 2.0)),
    };
    // A composition has no paper.
    if matches!(to, Container::Composition(_)) {
        place.corner = None;
    }
    let element = ElementKind::Instance(skraft_model::Instance { composition, place, props: Default::default() });
    if let Ok(Some(o)) = e.apply(Operation::AddElement { to, element }) {
        st.selected = o.element.into_iter().collect();
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

/// The texts' fields (docs/ui/drawing.md#fields): what they say on this sheet.
pub struct Fields<'a> {
    pub document: &'a str,
    pub sheet: &'a str,
    pub number: usize,
    pub count: usize,
    pub scale: Option<f64>,
    /// Where the other fields' values are looked for: the document, this sheet.
    pub values: Option<(&'a skraft_model::Document, Id)>,
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
}

/// What the entity info shows while nothing is picked (docs/ui/drawing.md#entity-info).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InfoOn {
    #[default]
    Sheet,
    Presentation,
    Document,
}

/// The fields among `names` that are given values: not those Skraft fills itself.
pub fn custom_fields(names: impl IntoIterator<Item = String>) -> Vec<String> {
    names.into_iter().filter(|n| !FIELDS.contains(&n.as_str()) && !n.trim().is_empty()).collect()
}

/// The fields a text may hold, in the « Insérer un champ » menu's order.
pub const FIELDS: &[&str] = &["document", "date", "sheet", "sheet.number", "sheet.count", "scale"];

impl Fields<'_> {
    /// `text` with its `{…}` fields filled, for an element under the instances `path` (one
    /// without a value left as it is).
    pub fn fill(&self, text: &str, path: &[Id]) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            match tail.find('}') {
                Some(j) => {
                    let key = &tail[1..j];
                    match self.value(key, path) {
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

    fn value(&self, key: &str, path: &[Id]) -> Option<String> {
        Some(match key {
            "document" => self.document.to_owned(),
            "date" => today(),
            "sheet" => self.sheet.to_owned(),
            "sheet.number" => self.number.to_string(),
            "sheet.count" => self.count.to_string(),
            "scale" => self.scale.map(scale_label).unwrap_or_default(),
            _ => {
                let (doc, sheet) = self.values?;
                doc.field(sheet, path, key)?.shown(&doc.settings().units)
            }
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
    /// By view and the direction it looks along: a composition's projected view looks its
    /// presentation's way (docs/ui/drawing.md#compositions).
    views: HashMap<(Id, [u64; 9]), Arc<ViewData>>,
}

/// What a view shows.
struct ViewData {
    /// Its subject's box in its frame (x, y, depth); none while no part's box is known.
    bx3: Option<(DVec3, DVec3)>,
    /// Per part (its definition): its placements in the subject's axes and their materials.
    parts: Vec<(Id, Vec<Placed>)>,
    lines: Option<Arc<crate::view_lines::LinesJob>>,
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
            .entry((id, e_key(r)))
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
                    lines: crate::view_lines::LinesJob::of(e, r, &placed).map(Arc::new),
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
    projection: Projection,
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
        ElementKind::Dimension(dim) => view_map(e, d, views, dim.view, projection)
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
/// none on a locked element (not in `frames`).
fn picked_handles(e: &Editor, st: &mut SheetState) -> Option<Picked> {
    let el = selected_element(e, st)?.clone();
    st.frames.iter().find(|(id, _)| *id == el.id)?;
    let frame = *st.boxes.get(&el.id)?;
    let d = e.document().drawing();
    let projection = st.projection(d);
    let hs = handles(e, d, &mut st.views, &el, Some(frame), projection);
    Some((el, frame, hs))
}

/// The handle of the element picked alone under `p` (screen points), the nearest.
fn handle_at(e: &Editor, st: &mut SheetState, pv: &Paperview, p: egui::Pos2) -> Option<Grip> {
    let (el, (lo, hi), hs) = picked_handles(e, st)?;
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

/// What the measurements box takes for grip `g` (docs/ui/drawing.md#measurements): lengths
/// (paper millimetres, shown and typed in the document's unit) or a view's scale.
enum Measure {
    Lengths(Vcb),
    Scale(f64),
}

use skraft_tools::vcb::{Vcb, VcbKind, VcbValue};

/// A box's corner its handle (`sx`, `sy`) does not move: where a typed size starts.
fn opposite(min: DVec2, max: DVec2, sx: i8, sy: i8) -> DVec2 {
    DVec2::new(if sx < 0 { max.x } else { min.x }, if sy < 0 { max.y } else { min.y })
}

/// The handle's other end for a line's end `k`, and its own.
fn line_ends(from: DVec2, to: DVec2, k: u8) -> (DVec2, DVec2) {
    if k == 0 { (to, from) } else { (from, to) }
}

/// What the measurements box measures for grip `g` (its element as `g.kind` is), signed as the
/// gesture goes.
fn measure_of(g: &Grip) -> Option<Measure> {
    let lengths = |label: String, v: Vec<f64>| Some(Measure::Lengths(Vcb::new(label, VcbKind::Lengths(v))));
    match (&g.kind, g.handle) {
        (ElementKind::Rect { min, max } | ElementKind::Image { min, max, .. }, Handle::Side(sx, sy)) => {
            let size = *max - *min;
            let (w, h) = (sx as f64 * size.x, sy as f64 * size.y);
            match (sx, sy) {
                (0, _) => lengths(t!("drawing.vcb.height"), vec![h]),
                (_, 0) => lengths(t!("drawing.vcb.width"), vec![w]),
                _ => lengths(t!("drawing.vcb.size"), vec![w, h]),
            }
        }
        (ElementKind::Line { from, to }, Handle::End(k)) => {
            let (o, end) = line_ends(*from, *to, k);
            let mut vcb = Vcb::new(t!("drawing.vcb.length"), VcbKind::Lengths(vec![o.distance(end)]));
            vcb.point = Some(skraft_tools::vcb::PointRef { from: o.extend(0.0), to: end.extend(0.0) });
            Some(Measure::Lengths(vcb))
        }
        (ElementKind::View(View { kind: ViewKind::Base { scale, .. }, .. }), Handle::Scale(..)) => Some(Measure::Scale(*scale)),
        (ElementKind::Text { size, .. }, Handle::Size) => lengths(t!("drawing.vcb.height"), vec![*size]),
        (ElementKind::Dimension(dim), Handle::Dim) => match &dim.kind {
            DimKind::Linear { offset, .. } => lengths(t!("drawing.vcb.distance"), vec![*offset]),
            DimKind::Radius { at, .. } => lengths(t!("drawing.vcb.distance"), vec![at.length()]),
            DimKind::Angle { at, .. } => lengths(t!("drawing.vcb.radius"), vec![at.length()]),
        },
        _ => None,
    }
}

/// A scale typed: `1:20`, `2:1`, `1/20` or a ratio (`0.05`).
fn parse_scale(text: &str) -> Option<f64> {
    let t = text.trim().replace(',', ".");
    let ratio = match t.split_once([':', '/']) {
        Some((a, b)) => a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?,
        None => t.parse::<f64>().ok()?,
    };
    (ratio.is_finite() && ratio > 0.0).then_some(ratio)
}

/// Grip `g`'s element with `text` typed for it: the box's size from its fixed corner, the
/// line's length along it (or its end at a point), the view's scale, the text's height, the
/// dimension's distance. The error is the status bar's.
fn typed_kind(g: &Grip, text: &str, units: &skraft_model::Units) -> Result<ElementKind, String> {
    let refused = || t!("drawing.vcb.refused", text = text.trim());
    let lengths = |vcb: &Vcb| match skraft_tools::vcb::read(vcb, text, units, &glam::DAffine3::IDENTITY)? {
        VcbValue::Lengths(v) => Ok(v),
        _ => Err(refused()),
    };
    let measure = measure_of(g).ok_or_else(refused)?;
    let vcb = match &measure {
        Measure::Lengths(v) => Some(v),
        Measure::Scale(_) => None,
    };
    match (&g.kind, g.handle, vcb) {
        (ElementKind::Rect { min, max } | ElementKind::Image { min, max, .. }, Handle::Side(sx, sy), Some(vcb)) => {
            let v = lengths(vcb)?;
            let o = opposite(*min, *max, sx, sy);
            let (mut lo, mut hi) = (*min, *max);
            let mut k = 0;
            if sx != 0 {
                (lo.x, hi.x) = (o.x.min(o.x + v[k]), o.x.max(o.x + v[k]));
                k += 1;
            }
            if sy != 0 {
                (lo.y, hi.y) = (o.y.min(o.y + v[k]), o.y.max(o.y + v[k]));
            }
            if hi.x - lo.x <= 0.0 || hi.y - lo.y <= 0.0 {
                return Err(refused());
            }
            Ok(match &g.kind {
                ElementKind::Image { asset, .. } => ElementKind::Image { asset: asset.clone(), min: lo, max: hi },
                _ => ElementKind::Rect { min: lo, max: hi },
            })
        }
        (ElementKind::Line { from, to }, Handle::End(k), Some(vcb)) => {
            let (o, end) = line_ends(*from, *to, k);
            let new = match skraft_tools::vcb::read(vcb, text, units, &glam::DAffine3::IDENTITY)? {
                VcbValue::Point(p) => p.truncate(),
                VcbValue::Lengths(v) => o + (end - o).normalize_or(DVec2::X) * v[0],
                _ => return Err(refused()),
            };
            if new.distance(o) <= 0.0 {
                return Err(refused());
            }
            Ok(if k == 0 { ElementKind::Line { from: new, to: o } } else { ElementKind::Line { from: o, to: new } })
        }
        (ElementKind::View(v), Handle::Scale(..), None) => match &v.kind {
            ViewKind::Base { subject, look, at, .. } => {
                let scale = parse_scale(text).ok_or_else(refused)?;
                let kind = ViewKind::Base { subject: subject.clone(), look: look.clone(), scale, at: *at };
                Ok(ElementKind::View(View { kind, ..v.clone() }))
            }
            ViewKind::Projected { .. } => Err(refused()),
        },
        (ElementKind::Text { at, text: words, .. }, Handle::Size, Some(vcb)) => {
            let size = lengths(vcb)?[0].abs();
            if !(0.5..=200.0).contains(&size) {
                return Err(refused());
            }
            Ok(ElementKind::Text { at: *at, text: words.clone(), size })
        }
        (ElementKind::Dimension(dim), Handle::Dim, Some(vcb)) => {
            let d = lengths(vcb)?[0];
            let kind = match &dim.kind {
                DimKind::Linear { a, b, orient, .. } => DimKind::Linear { a: a.clone(), b: b.clone(), orient: *orient, offset: d },
                DimKind::Radius { edge, diameter, at } => {
                    DimKind::Radius { edge: edge.clone(), diameter: *diameter, at: at.normalize_or(DVec2::X) * d.abs() }
                }
                DimKind::Angle { a, b, at } => DimKind::Angle { a: a.clone(), b: b.clone(), at: at.normalize_or(DVec2::X) * d.abs() },
            };
            Ok(ElementKind::Dimension(skraft_model::Dimension { view: dim.view, kind }))
        }
        _ => Err(refused()),
    }
}

/// The shape being drawn from `from` to `to` (a rectangle or a line), as a grip on its moving
/// corner or end — what the measurements box measures and a value typed makes.
fn sketch_grip(tool: SheetTool, from: DVec2, to: DVec2) -> Grip {
    let sign = |v: f64| if v < 0.0 { -1i8 } else { 1 };
    let (kind, handle) = match tool {
        SheetTool::Rect => {
            (ElementKind::Rect { min: from.min(to), max: from.max(to) }, Handle::Side(sign(to.x - from.x), sign(to.y - from.y)))
        }
        _ => (ElementKind::Line { from, to }, Handle::End(1)),
    };
    Grip { element: Id(0), handle, start: to, from: to, kind, lo: from.min(to), hi: from.max(to) }
}

/// Where the pointer aims on a sheet (docs/ui/drawing.md#snapping).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Aim {
    /// The point, in the axes of the level open (the sheet's, none open).
    at: DVec2,
    /// The same on the sheet, where its marks are drawn; `along_x` and `along_y` are too.
    mark: DVec2,
    /// Snapped on a point (an element's corner or end, a view's vertex or frame corner).
    point: bool,
    /// The point it is aligned with along the sheet's x (the same y), along its y (the same x).
    along_x: Option<DVec2>,
    along_y: Option<DVec2>,
}

/// The points of sheet `sheet` a point snaps on, and those it aligns with: rectangles' and
/// images' corners, lines' ends, views' frame corners (both); views' vertices (snapped on only:
/// so many would align everywhere), through its instances; not `skip`'s (right in the level
/// `open`), nor what is hidden.
fn snap_points(e: &Editor, d: &Drawing, views: &mut ViewsCache, sheet: Id, open: &[Id], skip: Option<Id>) -> (Vec<DVec2>, Vec<DVec2>) {
    let (mut on, mut align) = (Vec::new(), Vec::new());
    let Some(s) = d.sheet(sheet) else { return (on, align) };
    let corners = |lo: DVec2, hi: DVec2| [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
    for shown in d.placed(s.id) {
        if shown.hidden || skip.is_some_and(|s| shown.under(open) == Some(s)) {
            continue;
        }
        let t = shown.transform;
        let el = shown.element;
        match &el.kind {
            ElementKind::Rect { min, max } | ElementKind::Image { min, max, .. } => {
                align.extend(corners(*min, *max).map(|q| t.transform_point2(q)));
            }
            ElementKind::Line { from, to } => align.extend([*from, *to].map(|q| t.transform_point2(q))),
            ElementKind::View(_) => {
                let Some(r) = d.resolve_as(el.id, shown.projection) else { continue };
                let data = views.get(e, el.id, &r);
                let (lo, hi) = frame_of(&r, data.bx3.map(|(lo, hi)| (lo.truncate(), hi.truncate())));
                align.extend(corners(lo, hi).map(|q| t.transform_point2(q)));
                on.extend(data.snaps(e, &r).vertices.iter().map(|v| t.transform_point2(v.3)));
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

/// Where the pointer at `p` (in the axes of the level open) aims for the shape being drawn or
/// the handle being dragged, `from` in those axes too; `lock` (Shift, a line): its direction
/// from where it started kept horizontal or vertical, the nearer. Snapped on the sheet, its
/// points and grid, and given back in those axes.
#[allow(clippy::too_many_arguments)]
fn aim_for(e: &Editor, st: &mut SheetState, sheet: Id, zoom: f64, p: DVec2, skip: Option<Id>, from: Option<DVec2>, lock: bool) -> Aim {
    let d = e.document().drawing();
    let grid = d.sheet(sheet).map_or(0.0, |s| s.grid);
    let place = d.opened(sheet, &st.open).map_or(DAffine2::IDENTITY, |(_, t)| t);
    let (p, from) = (place.transform_point2(p), from.map(|f| place.transform_point2(f)));
    let open = st.open.clone();
    let (on, align) = snap_points(e, d, &mut st.views, sheet, &open, skip);
    let reach = SNAP_PT as f64 / zoom;
    let mut aim = aim_on_sheet(&on, &align, p, reach, from, grid, lock);
    aim.mark = aim.at;
    aim.at = place.inverse().transform_point2(aim.at);
    aim
}

/// [`aim_at`], `lock` keeping a line horizontal or vertical from `from`.
fn aim_on_sheet(on: &[DVec2], align: &[DVec2], p: DVec2, reach: f64, from: Option<DVec2>, grid: f64, lock: bool) -> Aim {
    match from {
        Some(f) if lock => {
            // Along the axis through its start: only the other coordinate snaps.
            let horizontal = (p.x - f.x).abs() >= (p.y - f.y).abs();
            let mut aim = aim_at(&[], align, p, reach, None, grid);
            if horizontal {
                aim.at.y = f.y;
                aim.along_x = Some(f);
            } else {
                aim.at.x = f.x;
                aim.along_y = Some(f);
            }
            aim
        }
        _ => aim_at(on, align, p, reach, from, grid),
    }
}

/// How near a point the pointer snaps on it (points).
const SNAP_PT: f32 = 8.0;

/// An aim's marks: the point snapped on circled, the alignments dotted — along the sheet's x
/// red, along its y green, as the 3D view's axes.
fn draw_aim(painter: &egui::Painter, pv: &Paperview, aim: &Aim) {
    let at = pv.to_screen(aim.mark);
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

    /// What holds what is edited now: the composition of the last instance entered, else the sheet
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

    /// Instance `id`, in the level open, entered.
    pub fn enter(&mut self, id: Id) {
        self.open.push(id);
        self.selected.clear();
        self.amend = None;
        self.picks.clear();
        self.sketch = None;
    }

    /// One level up, the instance left picked.
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

/// What a view's picture and lines are kept by: the view and, for a projected one, the projection
/// of the presentation it is drawn in. A composition's projected view may so look two ways on the
/// board at once, and a view whose direction changes keeps its last picture until the next.
pub type ViewSlot = (Id, Option<Projection>);

/// The slot of view `id` (`view`) drawn with `projection`.
fn slot_of(id: Id, view: &View, projection: Projection) -> ViewSlot {
    (id, matches!(view.kind, ViewKind::Projected { .. }).then_some(projection))
}

/// Of pictures (key, bytes, the frame they were last asked for), those to free for the rest to
/// fit in `budget`, the least recently asked first; none asked at frame `now`.
pub fn over_budget<K: Copy>(pictures: impl Iterator<Item = (K, u64, u64)>, now: u64, budget: u64) -> Vec<K> {
    let mut all: Vec<(K, u64, u64)> = pictures.collect();
    let mut total: u64 = all.iter().map(|p| p.1).sum();
    all.sort_by_key(|p| p.2);
    let mut out = Vec::new();
    for (key, bytes, seen) in all {
        if total <= budget || seen >= now {
            break;
        }
        total -= bytes;
        out.push(key);
    }
    out
}

/// A view's picture the 3D renderer is asked for: its subject's parts seen in its frame (the
/// *Rendered* and *Hybrid* styles) and its lines over them (*Lines*, *Hybrid*), framed exactly
/// on its box (what its frame on the sheet shows), at `size` pixels; drawn again when `stamp`
/// changes.
pub struct Wanted {
    pub slot: ViewSlot,
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
    let view = glam::dcamera::rh::view::look_to_mat4(eye, f.forward, f.up);
    let (near, far) = ((back - half.z * 1.05 - 1.0).max(1e-3), back + half.z * 1.05 + 1.0);
    // Reversed depth: near → 1, far → 0.
    let proj = glam::camera::rh::proj::directx::orthographic(
        -half.x as f32,
        half.x as f32,
        -half.y as f32,
        half.y as f32,
        far as f32,
        near as f32,
    );
    (view, proj)
}

/// A side of a picture in pixels, in steps of a fifth (a zoom redraws it only past a step).
fn picture_side(px: f64) -> u32 {
    let step = 1.2f64;
    let q = step.powf((px.max(8.0).ln() / step.ln()).round());
    q.min(MAX_PICTURE).round() as u32
}

/// The picture of view slot `slot` (`style`, showing `data`) asks for, its frame `frame` points
/// wide and high on screen (turned or not), its lines (`lines`, as last computed) over its parts.
fn wanted_picture(
    slot: ViewSlot,
    style: ViewStyle,
    r: &Resolved<'_>,
    data: &ViewData,
    lines: Option<Arc<skraft_tools::hidden_lines::ViewLines>>,
    frame: egui::Vec2,
    ppp: f32,
) -> Option<Wanted> {
    let b = data.bx3?;
    let size = [picture_side((frame.x * ppp) as f64), picture_side((frame.y * ppp) as f64)];
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
    (e_key(r), size, slot.0.0, style as u8, lines.as_ref().map(|l| Arc::as_ptr(&l.lines) as usize)).hash(&mut h);
    data.parts.len().hash(&mut h);
    for (p, places) in &data.parts {
        p.0.hash(&mut h);
        for (t, m) in places {
            t.to_cols_array().map(f64::to_bits).hash(&mut h);
            m.map(|m| m.0).hash(&mut h);
        }
    }
    Some(Wanted { slot, size, camera: picture_camera(r, b), parts, lines, stamp: h.finish() })
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
        Wanted { slot: (id, None), size, camera: picture_camera(r, b), parts, lines: None, stamp: 0 }
    });
    let lines = if style == ViewStyle::Rendered { None } else { crate::view_lines::LinesJob::of(e, r, &placed).map(|job| job.run()) };
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

/// A view from the front of what is selected (or the model), at the largest standard scale
/// fitting half the paper, at the sheet's centre.
pub fn add_view(e: &mut Editor, st: &mut SheetState) -> Result<(), String> {
    let sheet = st.shown(e.document().drawing()).ok_or_else(|| t!("drawing.no_sheet"))?;
    let (sheet_id, paper) = (sheet.id, sheet.paper);
    // On the level open, at the paper's centre.
    let d = e.document().drawing();
    let (to, place) = d.opened(sheet_id, &st.open).unwrap_or((Container::Sheet(sheet_id), DAffine2::IDENTITY));
    let n = d.elements_in(to).unwrap_or(&[]).iter().filter(|e| matches!(e.kind, ElementKind::View(_))).count() + 1;
    let subject = subject_of_selection(e);
    let look = Look::Standard(Direction::Front);
    let (frame, perspective) = look.frame();
    let at = place.inverse().transform_point2(paper.size() / 2.0);
    // Its scale from its box, seen at scale 1.
    let probe = Resolved { subject: &subject, frame, perspective, scale: 1.0, at };
    let scale = subject_box(e, &probe).map_or(1.0, |(lo, hi)| scale_to_fit(hi - lo, paper.size() / 2.0));
    let view = View { name: t!("drawing.view_name", n = n), kind: ViewKind::Base { subject, look, scale, at }, style: ViewStyle::Lines };
    e.apply(Operation::AddElement { to, element: ElementKind::View(view) }).map(|_| ())
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
        ElementKind::Instance(i) => {
            let mut i = i.clone();
            i.place.at += offset;
            ElementKind::Instance(i)
        }
    }
}

/// Dimension `dim` as drawn now, its view's box computed afresh (the PDF's).
pub fn dim_shape(e: &Editor, dim: &Dimension, projection: skraft_model::Projection) -> Option<crate::dims::DimShape> {
    let d = e.document().drawing();
    let r = d.resolve_as(dim.view, projection)?;
    let (lo, hi) = subject_box3(e, &r)?;
    Some(crate::dims::shape(e.document(), &r, ((lo + hi) / 2.0).truncate(), dim))
}

/// How a view maps its model onto the sheet: its resolution and its box's centre.
fn view_map<'a>(e: &Editor, d: &'a Drawing, views: &mut ViewsCache, view: Id, projection: Projection) -> Option<(Resolved<'a>, DVec2)> {
    let r = d.resolve_as(view, projection)?;
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
/// (linear) or an edge (a circle's: radius, diameter; a straight one: angle) of a view right
/// in `list`, what is edited (`only`: that view's; a dimension stays beside its view, so one in
/// an instance is dimensioned once in its composition), `p` in its axes; its view, its anchor, and
/// where it is.
#[allow(clippy::too_many_arguments)]
fn pick_at(
    e: &Editor,
    d: &Drawing,
    views: &mut ViewsCache,
    list: &[skraft_model::Element],
    projection: Projection,
    tool: SheetTool,
    p: DVec2,
    reach: f64,
    only: Option<Id>,
) -> Option<(Id, skraft_model::Anchor, Vec<DVec2>)> {
    use skraft_model::kernel::topo::EdgeShape;
    let ids: Vec<Id> = list
        .iter()
        .filter(|el| matches!(el.kind, ElementKind::View(_)) && !el.hidden)
        .map(|el| el.id)
        .filter(|id| only.is_none_or(|o| o == *id))
        .collect();
    let mut best: Option<(f64, Id, skraft_model::Anchor, Vec<DVec2>)> = None;
    for id in ids {
        let Some(r) = d.resolve_as(id, projection) else { continue };
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
    projection: Projection,
) -> Option<Dimension> {
    let (view, _) = picks.first()?;
    let (r, centre) = view_map(e, d, views, *view, projection)?;
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
    // Turned about its middle, with what places it; the screen's y down.
    let angle = -s.text_angle as f32 + pv.angle();
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

/// Sheet millimetres (y up) to screen points, and back; drawing in the axes of what `place`
/// puts on the sheet (an instance's content).
#[derive(Clone, Copy)]
struct Paperview {
    centre: egui::Pos2,
    /// The board point at `centre`.
    pan: DVec2,
    zoom: f64,
    /// The sheet's origin on the board.
    origin: DVec2,
    place: DAffine2,
}

impl Paperview {
    fn new(centre: egui::Pos2, pan: DVec2, zoom: f64) -> Self {
        Paperview { centre, pan, zoom, origin: DVec2::ZERO, place: DAffine2::IDENTITY }
    }
    /// This view drawing a sheet whose origin lies at `origin` on the board.
    fn at(self, origin: DVec2) -> Self {
        Paperview { origin, ..self }
    }
    /// This view drawing in the axes `place` puts on the sheet.
    fn placed(self, place: DAffine2) -> Self {
        Paperview { place, ..self }
    }
    fn to_screen(self, p: DVec2) -> egui::Pos2 {
        let d = (self.place.transform_point2(p) + self.origin - self.pan) * self.zoom;
        egui::pos2(self.centre.x + d.x as f32, self.centre.y - d.y as f32)
    }
    /// A screen point in the axes it draws in.
    fn to_sheet(self, p: egui::Pos2) -> DVec2 {
        let on_sheet = self.pan - self.origin + DVec2::new((p.x - self.centre.x) as f64, (self.centre.y - p.y) as f64) / self.zoom;
        self.place.inverse().transform_point2(on_sheet)
    }
    /// The corners of box `a`–`b` on screen: its top left first, then clockwise.
    fn quad(self, a: DVec2, b: DVec2) -> [egui::Pos2; 4] {
        let (lo, hi) = (a.min(b), a.max(b));
        [DVec2::new(lo.x, hi.y), hi, DVec2::new(hi.x, lo.y), lo].map(|p| self.to_screen(p))
    }
    /// What box `a`–`b` covers on screen.
    fn rect(self, a: DVec2, b: DVec2) -> egui::Rect {
        egui::Rect::from_points(&self.quad(a, b))
    }
    /// Is what it draws turned?
    fn turned(self) -> bool {
        self.place.matrix2.x_axis.y.abs() > 1e-12 || self.place.matrix2.x_axis.x < 0.0
    }
    /// How much what it draws is turned on screen (radians, the screen's y down); 0 when not.
    fn angle(self) -> f32 {
        if self.turned() { -self.place.matrix2.x_axis.y.atan2(self.place.matrix2.x_axis.x) as f32 } else { 0.0 }
    }
}

/// The least room (points, each way) the sheet is framed in.
const MIN_FIT_ROOM: f32 = 160.0;
/// Above the board's top when it is framed: the presentations' and the first sheets' names.
const NAMES_PT: f32 = 44.0;
/// A sheet's name needs this much gap above it (points).
const SHEET_NAME_PT: f32 = 18.0;
/// A sheet with less than this share of it in the room counts as off screen.
const REVEAL_SHARE: f32 = 0.25;
/// The active sheet's outline and what is picked.
const ACCENT: egui::Color32 = egui::Color32::from_rgb(70, 120, 200);

/// The camera keeps the active sheet where it is on screen when the layout moves it; a sheet
/// made active otherwise than by a click on it (a new one, the last one deleted) is brought on
/// screen.
fn follow_active(st: &mut SheetState, board: &Board, active: Option<Id>) {
    let now = active.and_then(|id| board.sheet(id)).map(|s| (s.id, s.origin));
    match (st.follow, now) {
        (Some((was, from)), Some((id, to))) if was == id => st.pan += to - from,
        (_, Some((id, _))) if st.framing.is_none() => st.framing = Some(Framing::Reveal(id)),
        _ => {}
    }
    st.follow = now;
}

/// What was asked to be framed, in `room` (what the panels leave of `stage`); asked again while
/// that room is not `known` yet.
fn apply_framing(st: &mut SheetState, board: &Board, known: bool, room: egui::Rect, stage: egui::Rect) {
    let Some(framing) = st.framing else { return };
    let (target, names) = match framing {
        Framing::Board => (board.bounds(), true),
        Framing::Sheet(id) => (board.sheet(id).map(|s| (s.origin, s.max())), false),
        Framing::Column(id) => (board.column(id).map(Column::bounds), true),
        Framing::Reveal(id) => {
            let Some(s) = board.sheet(id) else {
                st.framing = None;
                return;
            };
            let page = Paperview::new(stage.center(), st.pan, st.zoom).rect(s.origin, s.max());
            let seen = page.intersect(room);
            if st.zoom > 0.0 && seen.is_positive() && seen.area() >= page.area() * REVEAL_SHARE {
                st.framing = None;
                return;
            }
            (Some((s.origin, s.max())), false)
        }
    };
    if known {
        st.framing = None;
    }
    let Some((lo, hi)) = target else { return };
    let room = if names { egui::Rect::from_min_max(room.min + egui::vec2(0.0, NAMES_PT.min(room.height() / 2.0)), room.max) } else { room };
    let size = (hi - lo).max(DVec2::splat(1.0));
    st.zoom = (room.width() as f64 / (size.x * FIT_MARGIN)).min(room.height() as f64 / (size.y * FIT_MARGIN));
    // Its centre at the room's.
    let shift = room.center() - stage.center();
    st.pan = (lo + hi) / 2.0 - DVec2::new(shift.x as f64, -shift.y as f64) / st.zoom;
}

/// The middle or right button's drag pans the board, the wheel (or a pinch) zooms about the
/// pointer.
fn move_camera(ui: &egui::Ui, response: &egui::Response, st: &mut SheetState, centre: egui::Pos2) {
    if st.zoom <= 0.0 {
        return;
    }
    if response.dragged_by(egui::PointerButton::Middle) || response.dragged_by(egui::PointerButton::Secondary) {
        let d = response.drag_delta();
        st.pan -= DVec2::new(d.x as f64, -d.y as f64) / st.zoom;
    }
    if let Some(at) = response.hover_pos() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (scroll as f64 * 0.002).exp() * pinch as f64;
        if factor != 1.0 {
            let under = Paperview::new(centre, st.pan, st.zoom).to_sheet(at);
            st.zoom = (st.zoom * factor).clamp(0.01, 200.0);
            // The point under the pointer stays there.
            st.pan = under - DVec2::new((at.x - centre.x) as f64, (centre.y - at.y) as f64) / st.zoom;
        }
    }
}

/// A click on another sheet makes it active, the camera still, and a double click frames it
/// (docs/ui/drawing.md#the-active-sheet); a drag from it makes it active and goes on there. Not
/// while a tool is under way, nor over what the active sheet shows past its paper. Was the click
/// taken?
fn activate(response: &egui::Response, st: &mut SheetState, board: &Board, centre: egui::Pos2, active: Option<Id>) -> bool {
    if st.zoom <= 0.0 {
        return false;
    }
    let view = Paperview::new(centre, st.pan, st.zoom);
    let under_way = st.sketch.is_some() || !st.picks.is_empty() || matches!(st.tool, SheetTool::Project { .. });
    let other = |st: &SheetState, p: egui::Pos2| {
        if st.frames.iter().any(|(_, r)| r.expand(GRIP_PT).contains(p)) {
            return None;
        }
        board.sheet_at(view.to_sheet(p)).filter(|s| Some(*s) != active)
    };
    if response.clicked() {
        let first = st.activated.take();
        let at = response.interact_pointer_pos();
        let under = if under_way { None } else { at.and_then(|p| other(st, p)) };
        if response.double_clicked() {
            // Its first click made it active; or both came within one frame.
            if first.is_some() && at.and_then(|p| board.sheet_at(view.to_sheet(p))) == first {
                st.framing = first.map(Framing::Sheet);
                return true;
            }
            if let Some(id) = under {
                make_active(st, board, id);
                st.framing = Some(Framing::Sheet(id));
                return true;
            }
        }
        if let Some(id) = under {
            make_active(st, board, id);
            st.activated = Some(id);
            return true;
        }
    } else if response.drag_started_by(egui::PointerButton::Primary)
        && !under_way
        && let Some(id) = response.ctx.input(|i| i.pointer.press_origin()).and_then(|p| other(st, p))
    {
        make_active(st, board, id);
    }
    false
}

/// A sheet's white paper with its shadow, the active one outlined.
fn draw_paper(painter: &egui::Painter, page: egui::Rect, active: bool) {
    painter.rect_filled(page.translate(egui::vec2(3.0, 4.0)), 0.0, egui::Color32::from_black_alpha(40));
    painter.rect_filled(page, 0.0, egui::Color32::WHITE);
    let stroke = if active { egui::Stroke::new(2.0, ACCENT) } else { egui::Stroke::new(1.0, egui::Color32::from_gray(160)) };
    painter.rect_stroke(page, 0.0, stroke, egui::StrokeKind::Outside);
}

/// What the texts' fields say on sheet `id`, which shows `placed`.
fn fields_of<'a>(e: &'a Editor, d: &'a Drawing, id: Id, title: &'a str, placed: &[skraft_model::Placed<'_>]) -> Option<Fields<'a>> {
    d.sheet(id).map(|s| Fields {
        document: title,
        sheet: &s.name,
        number: d.sheet_number(id).map_or(0, |(n, _)| n),
        count: d.sheet_number(id).map_or(0, |(_, c)| c),
        // The first base view it shows, through its instances.
        scale: placed.iter().find_map(|p| match &p.element.kind {
            ElementKind::View(View { kind: ViewKind::Base { scale, .. }, .. }) => Some(*scale),
            _ => None,
        }),
        values: Some((e.document(), id)),
    })
}

/// The sheets other than `active`, as printed: clipped to their paper, without their views'
/// frames and names (docs/ui/drawing.md#the-active-sheet); those off the stage skipped. The
/// columns without a sheet say how to make one.
#[allow(clippy::too_many_arguments)]
fn draw_other_sheets(
    painter: &egui::Painter,
    view: Paperview,
    board: &Board,
    active: Option<Id>,
    e: &Editor,
    d: &Drawing,
    title: &str,
    ready: &HashMap<ViewSlot, egui::TextureId>,
    lines: &mut crate::view_lines::LinesCache,
    views: &mut ViewsCache,
    wanted: &mut Vec<Wanted>,
) {
    let clip = painter.clip_rect();
    let mut asked = HashSet::new();
    for column in &board.columns {
        if column.sheets.is_empty() {
            let (lo, hi) = column.bounds();
            let r = view.rect(lo, hi);
            let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            let ink = egui::Color32::from_gray(140);
            painter.extend(egui::Shape::dashed_line(&corners, egui::Stroke::new(1.0, ink), 6.0, 4.0));
            if r.width() >= 120.0 && r.height() >= 60.0 {
                let galley = painter.layout(t!("drawing.no_sheet_hint"), egui::FontId::proportional(12.0), ink, r.width() - 24.0);
                painter.galley(r.center() - galley.size() / 2.0, galley, ink);
            }
        }
        for s in &column.sheets {
            let pv = view.at(s.origin);
            let page = pv.rect(DVec2::ZERO, s.size);
            crate::handles::publish(|| format!("sheet_paper:{}", s.id.0), page);
            if Some(s.id) == active || !clip.intersects(page) {
                continue;
            }
            draw_paper(painter, page, false);
            let on_paper = painter.with_clip_rect(clip.intersect(page));
            let placed = d.placed(s.id);
            let fields = fields_of(e, d, s.id, title, &placed);
            let level = Level { open: &[], place: DAffine2::IDENTITY, printed: true };
            draw_elements(&on_paper, &pv, e, d, s.id, &placed, level, &[], ready, lines, views, wanted, &mut asked, fields.as_ref());
        }
    }
}

/// A name centred above `bottom`, cut at `max` points wide.
fn board_name(painter: &egui::Painter, bottom: egui::Pos2, text: &str, size: f32, color: egui::Color32, max: f32) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), egui::FontId::proportional(size), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max.max(1.0));
    let galley = painter.layout_job(job);
    painter.galley(bottom - egui::vec2(galley.size().x / 2.0, galley.size().y), galley, color);
}

/// The presentations' names on top of their columns, and the sheets' above them while their gap
/// has room.
fn draw_names(painter: &egui::Painter, view: Paperview, board: &Board, d: &Drawing, active: Option<Id>, ink: egui::Color32) {
    if view.zoom <= 0.0 {
        return;
    }
    let with_sheets = (board.gap * view.zoom) as f32 >= SHEET_NAME_PT;
    let zoom = view.zoom as f32;
    for column in &board.columns {
        if with_sheets {
            for s in &column.sheets {
                let Some(sheet) = d.sheet(s.id) else { continue };
                let top = view.to_screen(DVec2::new(s.origin.x + s.size.x / 2.0, s.max().y));
                let color = if Some(s.id) == active { ACCENT } else { ink.gamma_multiply(0.7) };
                board_name(painter, top - egui::vec2(0.0, 3.0), &sheet.name, 11.0, color, s.size.x as f32 * zoom);
            }
        }
        let Some(p) = d.presentation(column.presentation) else { continue };
        let top = view.to_screen(DVec2::new(column.centre, 0.0));
        let above = if with_sheets { SHEET_NAME_PT } else { 4.0 };
        let room = (column.width + board.gap) as f32 * zoom;
        board_name(painter, top - egui::vec2(0.0, above), &p.name, 14.0, ink, room);
    }
}

/// The stage: the board of every sheet over `rect`, the active one edited, panned (a drag) and
/// zoomed (the wheel, about the pointer); what is framed is framed in `fit`, what the ribbon and
/// the side panels leave free: none while it is not known yet.
#[allow(clippy::too_many_arguments)]
pub fn ui(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    fit: Option<egui::Rect>,
    e: &mut Editor,
    st: &mut SheetState,
    lines: &mut crate::view_lines::LinesCache,
    theme: &Theme,
    ready: &HashMap<ViewSlot, egui::TextureId>,
    title: &str,
) -> Vec<Wanted> {
    let mut wanted = Vec::new();
    let ink = viewport::color(theme.ui.view_text);
    ui.painter().rect_filled(rect, 0.0, viewport::color(theme.ui.view));
    crate::handles::publish(|| "sheet".into(), rect);
    let paper_room = rect;
    let centre = paper_room.center();
    if let Some(ask) = st.asked.take() {
        run_ask(e, st, ask, centre);
    }
    // Framed in what the panels leave; if they leave next to nothing, in the whole stage.
    let fit_room = fit.filter(|r| r.width() >= MIN_FIT_ROOM && r.height() >= MIN_FIT_ROOM).unwrap_or(rect);
    let board = Board::of(e.document().drawing());
    let response = ui.interact(paper_room, egui::Id::unique("sheet_stage"), egui::Sense::click_and_drag());
    if st.zoom <= 0.0 {
        st.framing = Some(Framing::Board);
    }
    let active = st.shown(e.document().drawing()).map(|s| s.id);
    follow_active(st, &board, active);
    apply_framing(st, &board, fit.is_some(), fit_room, paper_room);
    move_camera(ui, &response, st, centre);
    // A click that made another sheet active, or framed it, is not the tool's.
    let taken = activate(&response, st, &board, centre, active);
    let (clicked, double) = (response.clicked() && !taken, response.double_clicked() && !taken);
    let shown = st.shown(e.document().drawing()).map(|s| (s.id, s.paper));
    let painter = ui.painter().with_clip_rect(paper_room);
    // What the other sheets ask for, after what the active one does.
    let mut others = Vec::new();
    if let Some((id, paper)) = shown {
        let origin = board.sheet(id).map_or(DVec2::ZERO, |s| s.origin);
        st.current = Some(id);
        // The level open, as far as it still leads somewhere; what is picked: in it, neither
        // hidden nor locked. A composition left empty goes (as an empty assembly, data-model.md).
        let (to, place, projection) = {
            let d = e.document().drawing();
            st.presentation = d.presentation_of(id).map(|p| p.id);
            while !st.open.is_empty() && d.opened(id, &st.open).is_none() {
                st.open.pop();
            }
            let (to, place) = d.opened(id, &st.open).unwrap_or((Container::Sheet(id), DAffine2::IDENTITY));
            if let Some(list) = d.elements_in(to) {
                st.selected.retain(|v| list.iter().any(|el| el.id == *v && !el.hidden && !el.locked));
            }
            (to, place, st.projection(d))
        };
        let compositions: Vec<Id> =
            st.open.iter().filter_map(|i| e.document().drawing().element(*i)?.kind.instance().map(|i| i.composition)).collect();
        for b in std::mem::replace(&mut st.was_open, compositions.clone()) {
            if !compositions.contains(&b) && e.document().drawing().composition(b).is_some_and(|x| x.elements.is_empty()) {
                let _ = e.apply(Operation::DeleteComposition { composition: b });
            }
        }
        if let Some(b) = st.place_composition.take() {
            place_composition(e, st, b);
        }
        let pv = Paperview::new(centre, st.pan, st.zoom).at(origin);
        // In the axes of the level open: what is picked, moved and drawn there.
        let lv = pv.placed(place);
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
            let grip = if st.tool == SheetTool::Select { handle_at(e, st, &lv, p) } else { None };
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
                        st.moving = Some(lv.to_sheet(p));
                    }
                    None => st.marquee = Some(p),
                },
                // A drag from where the button went down (the first click's point, if made).
                SheetTool::Rect | SheetTool::Line if st.sketch.is_none() => {
                    st.sketch = Some(aim_for(e, st, id, pv.zoom, lv.to_sheet(p), None, None, false).at);
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
                    g.snaps().map(|from| aim_for(e, st, id, pv.zoom, g.target(lv.to_sheet(p)), Some(g.element), from, shift && line_end))
                }
                (Some(p), None) if matches!(st.tool, SheetTool::Rect | SheetTool::Line | SheetTool::Text) => {
                    let lock = shift && st.tool == SheetTool::Line && st.sketch.is_some();
                    Some(aim_for(e, st, id, pv.zoom, lv.to_sheet(p), None, st.sketch, lock))
                }
                _ => None,
            }
        };
        // A handle's point: where it aims, else as far as the pointer went.
        let grip_to = |g: &Grip, p: egui::Pos2| aim.map_or_else(|| g.target(lv.to_sheet(p)), |a| a.at);
        if response.clicked() || response.drag_started() {
            st.message = None;
            st.info = InfoOn::Sheet;
        }
        // The measurements box (docs/ui/drawing.md#measurements): the shape being drawn, the
        // handle dragged, else the shape last made or the handle last let go, while its element
        // is as it was left and picked alone.
        if st
            .amend
            .as_ref()
            .is_some_and(|a| st.selected != [a.element] || e.document().drawing().element(a.element).map(|el| &el.kind) != Some(&a.kind))
        {
            st.amend = None;
        }
        if let Some(a) = &aim {
            st.sketch_to = Some(a.at);
        }
        let measured = match (st.sketch, &st.grip, pointer) {
            (Some(from), _, _) if matches!(st.tool, SheetTool::Rect | SheetTool::Line) => {
                Some(sketch_grip(st.tool, from, st.sketch_to.unwrap_or(from)))
            }
            (_, Some(g), Some(p)) => Some(Grip { kind: regrip(g, grip_to(g, p), shift), ..g.clone() }),
            _ => st.amend.clone(),
        };
        let units = e.document().settings().units.clone();
        st.measure = measured.as_ref().and_then(measure_of).map(|m| match m {
            Measure::Lengths(v) => (v.label.clone(), skraft_tools::vcb::display(&v, &units)),
            Measure::Scale(s) => (t!("drawing.vcb.scale"), scale_label(s)),
        });
        // A value typed: the shape drawn so, or the element changed so (one operation).
        if let Some(text) = st.typed.take()
            && let Some(g) = &measured
        {
            st.message = None;
            match typed_kind(g, &text, &units) {
                Ok(kind) if g.element == Id(0) => {
                    st.sketch = None;
                    if let Ok(Some(o)) = e.apply(Operation::AddElement { to, element: kind.clone() }) {
                        st.selected = o.element.into_iter().collect();
                        st.amend = o.element.map(|element| Grip { element, kind, ..g.clone() });
                    }
                }
                Ok(kind) => {
                    if e.apply(Operation::SetElement { element: g.element, kind: kind.clone() }).is_ok() {
                        st.grip = None;
                        st.amend = Some(Grip { kind, ..g.clone() });
                    }
                }
                Err(m) => st.message = Some(m),
            }
        }
        let offset = match (st.moving, pointer) {
            (Some(from), Some(p)) => lv.to_sheet(p) - from,
            _ => DVec2::ZERO,
        };
        // The sheet the selection would move to, let go here.
        let drop_on = match (st.moving, pointer) {
            (Some(_), Some(p))
                if st.open.is_empty() && on_stage(ui.ctx(), paper_room, p) && can_leave_sheet(e.document().drawing(), &st.selected) =>
            {
                board.sheet_at(Paperview::new(centre, st.pan, st.zoom).to_sheet(p)).filter(|s| *s != id)
            }
            _ => None,
        };
        // A handle let go: its element changed (one `SetElement`).
        if response.drag_stopped()
            && let Some(g) = st.grip.take()
            && let Some(p) = pointer
        {
            let kind = regrip(&g, grip_to(&g, p), shift);
            if kind != g.kind {
                let _ = e.apply(Operation::SetElement { element: g.element, kind: kind.clone() });
            }
            st.amend = Some(Grip { kind, ..g });
        }
        if response.drag_stopped() && st.moving.take().is_some() && offset != DVec2::ZERO {
            if let Some(to) = drop_on {
                drop_on_sheet(e, st, &board, id, to, offset);
                ui.ctx().request_repaint();
            } else {
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
        // A rectangle or a line: two clicks (its corners, its ends) or a
        // drag, as the 3D view's shapes (docs/tools/interaction.md); under 0.5 mm, the second
        // point still to come.
        if matches!(st.tool, SheetTool::Rect | SheetTool::Line)
            && let Some(p) = pointer
            && (clicked || response.drag_stopped_by(egui::PointerButton::Primary))
        {
            let end = aim.map_or_else(|| lv.to_sheet(p), |a| a.at);
            match st.sketch {
                None => st.sketch = Some(end),
                Some(from) if from.distance(end) > 0.5 => {
                    st.sketch = None;
                    let element = match st.tool {
                        SheetTool::Rect => ElementKind::Rect { min: from.min(end), max: from.max(end) },
                        _ => ElementKind::Line { from, to: end },
                    };
                    if let Ok(Some(o)) = e.apply(Operation::AddElement { to, element }) {
                        st.selected = o.element.into_iter().collect();
                        st.amend = o.element.map(|element| Grip { element, ..sketch_grip(st.tool, from, end) });
                    }
                }
                Some(_) => {}
            }
        }
        // Esc goes back one step of the tool: the shape or rectangle drawn dropped, the picks;
        // with Select the selection cleared; a tool stays (Space, `tool.select`, ends it), as in
        // the 3D view (docs/tools/interaction.md#selection). Not while a field is typed in.
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape)) && ui.ctx().memory(|m| m.focused().is_none());
        if std::mem::take(&mut st.esc_eaten) {
        } else if esc {
            st.amend = None;
            st.message = None;
            if st.sketch.take().is_some() || st.marquee.take().is_some() || st.moving.take().is_some() || st.grip.take().is_some() {
            } else if !st.picks.is_empty() {
                st.picks.clear();
            } else if st.tool == SheetTool::Select {
                // The selection first, then one level up.
                if st.selected.is_empty() {
                    st.leave();
                } else {
                    st.selected.clear();
                }
            }
        }
        let page = pv.rect(DVec2::ZERO, paper.size());
        draw_paper(&painter, page, true);
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
        // The others as printed, under what the active sheet shows past its paper; from the
        // drawing as previewed, a composition being edited showing there too.
        let view = Paperview::new(centre, st.pan, st.zoom);
        draw_other_sheets(&painter, view, &board, Some(id), e, drawing, title, ready, lines, &mut st.views, &mut others);
        // The sheet the selection would land on.
        if let Some(s) = drop_on.filter(|_| st.moving.is_some()).and_then(|s| board.sheet(s)) {
            let target = view.at(s.origin).rect(DVec2::ZERO, s.size);
            painter.rect_stroke(target, 0.0, egui::Stroke::new(2.0, ACCENT.gamma_multiply(0.6)), egui::StrokeKind::Outside);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        let placed = drawing.placed(id);
        let fields = fields_of(e, drawing, id, title, &placed);
        let level = Level { open: &st.open, place, printed: false };
        let mut asked = HashSet::new();
        let drawn = draw_elements(
            &painter,
            &pv,
            e,
            drawing,
            id,
            &placed,
            level,
            &st.selected,
            ready,
            lines,
            &mut st.views,
            &mut wanted,
            &mut asked,
            fields.as_ref(),
        );
        others.retain(|w| !asked.contains(&w.slot));
        (st.frames, st.boxes, st.open_box) = (drawn.frames, drawn.boxes, drawn.level_box);
        // The handles of the element picked alone (Select), as it is drawn; the pointer over one
        // shows what it does.
        if st.tool == SheetTool::Select && st.moving.is_none() && st.marquee.is_none() {
            let shown = match &st.grip {
                Some(g) => drawing.element(g.element).cloned().map(|el| {
                    let frame = st.boxes.get(&el.id).copied();
                    handles(e, drawing, &mut st.views, &el, frame, projection)
                }),
                None => picked_handles(e, st).map(|(_, _, hs)| hs),
            };
            let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 120, 200));
            for (h, q) in shown.unwrap_or_default() {
                let at = lv.to_screen(q);
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
            let end = aim.map_or_else(|| lv.to_sheet(p), |a| a.at);
            if st.tool == SheetTool::Rect {
                painter.add(egui::Shape::closed_line(lv.quad(from, end).to_vec(), stroke));
            } else {
                painter.line_segment([lv.to_screen(from), lv.to_screen(end)], stroke);
            }
        }
        match st.tool {
            SheetTool::Select => {
                let at = response.interact_pointer_pos();
                let hit = at.and_then(|p| st.frames.iter().rev().find(|(_, r)| r.contains(p)).map(|(v, _)| *v));
                // A double click enters an instance; on the sheet's void it frames the sheet, off
                // its paper the whole board.
                if double {
                    let instance = hit.filter(|v| drawing.element(*v).is_some_and(|el| el.kind.instance().is_some()));
                    match (instance, hit) {
                        (Some(i), _) => st.enter(i),
                        (None, None) if st.open.is_empty() => {
                            let on_paper =
                                at.map(|p| pv.to_sheet(p)).is_some_and(|q| q.cmpge(DVec2::ZERO).all() && q.cmple(paper.size()).all());
                            if on_paper {
                                st.frame_sheet(id);
                            } else {
                                st.refit();
                            }
                        }
                        _ => {}
                    }
                } else if clicked
                    && hit.is_none()
                    && !shift
                    && !st.open.is_empty()
                    && !at.is_some_and(|p| {
                        let q = lv.to_sheet(p);
                        st.open_box.is_some_and(|(lo, hi)| q.cmpge(lo).all() && q.cmple(hi).all())
                    })
                {
                    // Off the box of the composition edited: one level up, as in the 3D view
                    // (docs/tools/interaction.md#selection).
                    st.leave();
                } else if clicked {
                    // A click picks what is under it (the last drawn first), elsewhere nothing;
                    // with Shift it adds it or takes it off.
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
                if clicked
                    && let Some(p) = response.interact_pointer_pos()
                    && let Ok(Some(o)) = e.apply(Operation::AddElement {
                        to,
                        element: ElementKind::Text {
                            at: aim.map_or_else(|| lv.to_sheet(p), |a| a.at),
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
                    let at = lv.to_sheet(p);
                    let mark = egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 120, 30));
                    if st.picks.len() < picks_needed(tool) {
                        // A vertex or an edge to pick, of the first pick's view once there is one.
                        let only = st.picks.first().map(|x| x.0);
                        let list = drawing.elements_in(to).unwrap_or(&[]);
                        let hit = pick_at(e, drawing, &mut st.views, list, projection, tool, at, 8.0 / pv.zoom, only);
                        if let Some((_, _, marker)) = &hit {
                            match marker.as_slice() {
                                [q] => {
                                    painter.circle_stroke(lv.to_screen(*q), 5.0, mark);
                                }
                                line => {
                                    painter.add(egui::Shape::line(line.iter().map(|q| lv.to_screen(*q)).collect(), mark));
                                }
                            }
                        }
                        if clicked && let Some((v, a, _)) = hit {
                            st.picks.push((v, a));
                        }
                    } else if let Some(dim) = dimension_of(e, drawing, &mut st.views, tool, &st.picks, at, projection) {
                        // Where a click puts it.
                        if let Some((r, c)) = view_map(e, drawing, &mut st.views, dim.view, projection) {
                            draw_dim(&painter, &lv, &crate::dims::shape(e.document(), &r, c, &dim));
                        }
                        if clicked {
                            st.picks.clear();
                            if let Ok(Some(o)) = e.apply(Operation::AddElement { to, element: ElementKind::Dimension(dim) }) {
                                st.selected = o.element.into_iter().collect();
                            }
                        }
                    }
                }
            }
            SheetTool::Project { base } => {
                if drawing.resolve_as(base, projection).is_none() || drawing.container_of(base) != Some(to) {
                    st.tool = SheetTool::Select;
                } else if let Some(p) = response.hover_pos()
                    && let Some((side, distance)) = projection_at(e, drawing, base, lv.to_sheet(p), projection)
                {
                    // Its frame where a click puts it.
                    let r = drawing.resolve_as(base, projection).unwrap();
                    let ghost = Resolved { frame: r.frame.projected(side, projection), at: r.at + side.dir() * distance, ..r.clone() };
                    let (lo, hi) = view_frame(e, &ghost);
                    let q = lv.quad(lo, hi);
                    let corners = [q[0], q[1], q[2], q[3], q[0]];
                    painter.extend(egui::Shape::dashed_line(&corners, egui::Stroke::new(1.0, egui::Color32::from_gray(120)), 4.0, 4.0));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    if clicked {
                        let n =
                            drawing.elements_in(to).unwrap_or(&[]).iter().filter(|e| matches!(e.kind, ElementKind::View(_))).count() + 1;
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
                        if let Ok(Some(o)) = e.apply(Operation::AddElement { to, element: ElementKind::View(view) }) {
                            st.selected = o.element.into_iter().collect();
                        }
                    }
                }
            }
        }
        carry_composition(ui, e, st, paper_room, &board, Paperview::new(centre, st.pan, st.zoom), Some((id, to, lv)));
    } else {
        let view = Paperview::new(centre, st.pan, st.zoom);
        draw_other_sheets(&painter, view, &board, None, e, e.document().drawing(), title, ready, lines, &mut st.views, &mut others);
        carry_composition(ui, e, st, paper_room, &board, view, None);
        if board.columns.is_empty() {
            let at = fit.unwrap_or(rect).center();
            ui.painter().text(at, egui::Align2::CENTER_CENTER, t!("drawing.no_sheet_hint"), egui::FontId::proportional(14.0), ink);
        }
        if double {
            st.refit();
        }
    }
    wanted.append(&mut others);
    // The lines of views gone forgotten; those asked for this frame started, the active sheet's
    // first.
    let views: HashSet<Id> = e.document().drawing().view_ids().collect();
    lines.retain(&views);
    lines.start();
    // Asked during this frame (a double click): done at the next, with no input to wake it.
    if st.framing.is_some() {
        ui.ctx().request_repaint();
    }
    draw_names(&painter, Paperview::new(centre, st.pan, st.zoom), &board, e.document().drawing(), shown.map(|s| s.0), ink);
    wanted
}

/// A picture on the four corners `quad` (its top left first, clockwise on screen).
fn draw_picture(painter: &egui::Painter, texture: egui::TextureId, quad: [egui::Pos2; 4]) {
    let mut mesh = egui::Mesh::with_texture(texture);
    let uvs = [egui::pos2(0.0, 0.0), egui::pos2(1.0, 0.0), egui::pos2(1.0, 1.0), egui::pos2(0.0, 1.0)];
    for (pos, uv) in quad.into_iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: egui::Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// `text` from its bottom left corner `at`, turned about it by `angle` (on screen); what it
/// covers on screen, and its size before it is turned.
fn turned_text(
    painter: &egui::Painter,
    at: egui::Pos2,
    angle: f32,
    text: String,
    font: egui::FontId,
    color: egui::Color32,
) -> (egui::Rect, egui::Vec2) {
    if angle == 0.0 {
        let r = painter.text(at, egui::Align2::LEFT_BOTTOM, text, font, color);
        return (r, r.size());
    }
    let galley = painter.layout_no_wrap(text, font, color);
    let size = galley.size();
    let rot = egui::emath::Rot2::from_angle(angle);
    let top_left = at + rot * egui::vec2(0.0, -size.y);
    let corners = [top_left, top_left + rot * egui::vec2(size.x, 0.0), top_left + rot * size, top_left + rot * egui::vec2(0.0, size.y)];
    painter.add(egui::epaint::TextShape::new(top_left, galley, color).with_angle(angle));
    (egui::Rect::from_points(&corners), size)
}

/// The outline of the box on `quad`, turned or not.
fn stroke_box(painter: &egui::Painter, pv: &Paperview, quad: [egui::Pos2; 4], stroke: egui::Stroke) {
    if pv.turned() {
        painter.add(egui::Shape::closed_line(quad.to_vec(), stroke));
    } else {
        painter.rect_stroke(egui::Rect::from_points(&quad), 0.0, stroke, egui::StrokeKind::Middle);
    }
}

/// The level edited: the instances entered, what takes their last composition's axes to the sheet.
#[derive(Clone, Copy)]
struct Level<'a> {
    open: &'a [Id],
    place: DAffine2,
    /// A sheet drawn as printed: without its views' frames and names, nothing to pick.
    printed: bool,
}

/// What [`draw_elements`] leaves for the pointer.
#[derive(Default)]
struct Drawn {
    /// What a press lands on, bottom first: what is right in the level open, on screen.
    frames: Vec<(Id, egui::Rect)>,
    /// The same in the level's axes.
    boxes: HashMap<Id, (DVec2, DVec2)>,
    /// All it shows there, a little wider: the box a click off leaves it.
    level_box: Option<(DVec2, DVec2)>,
}

/// How far the box of the composition edited stands off what it shows (paper millimetres).
const LEVEL_MARGIN: f64 = 2.0;

/// What a sheet shows through its instances (docs/ui/drawing.md#compositions), bottom first: views as
/// their frames, named; texts, lines, rectangles; images as their frames for now. With a composition
/// entered, what lies outside it first, faded under a veil of the paper's white, then what it
/// shows in its grey box. A press lands on what is right in the level, an instance whole by the
/// box of what it shows.
#[allow(clippy::too_many_arguments)]
fn draw_elements(
    painter: &egui::Painter,
    pv: &Paperview,
    e: &Editor,
    d: &Drawing,
    sheet: Id,
    placed: &[skraft_model::Placed<'_>],
    level: Level<'_>,
    selected: &[Id],
    ready: &HashMap<ViewSlot, egui::TextureId>,
    lines: &mut crate::view_lines::LinesCache,
    views: &mut ViewsCache,
    wanted: &mut Vec<Wanted>,
    // The views asked for so far: one placed twice asks for its picture once per slot.
    asked: &mut HashSet<ViewSlot>,
    fields: Option<&Fields<'_>>,
) -> Drawn {
    let mut out = Drawn::default();
    let Some(s) = d.sheet(sheet) else { return out };
    let ppp = painter.ctx().pixels_per_point();
    let paper_ink = egui::Color32::from_gray(30);
    let frame_ink = ACCENT;
    let thin = egui::Stroke::new((0.25 * pv.zoom).max(1.0) as f32, paper_ink);
    let accent = egui::Stroke::new(2.0, frame_ink);
    // Its frame, under what it holds.
    if let Some(b) = s.border {
        let m = DVec2::splat(b.margin);
        let stroke = egui::Stroke::new((b.width * pv.zoom).max(1.0) as f32, paper_ink);
        painter.rect_stroke(pv.rect(m, s.paper.size() - m), 0.0, stroke, egui::StrokeKind::Middle);
    }
    let list = d.opened(sheet, level.open).and_then(|(c, _)| d.elements_in(c)).unwrap_or(&[]);
    let tops: HashMap<Id, &skraft_model::Element> = list.iter().map(|el| (el.id, el)).collect();
    let to_level = level.place.inverse();
    let (outside, inside): (Vec<_>, Vec<_>) = placed.iter().partition(|p| p.under(level.open).is_none());
    let mut boxes: Vec<(Id, egui::Rect)> = Vec::new();
    // What is outside, the veil (`None`), what is inside.
    for shown in outside.iter().map(|p| Some(*p)).chain([None]).chain(inside.iter().map(|p| Some(*p))) {
        let Some(shown) = shown else {
            if !level.open.is_empty() {
                painter.rect_filled(pv.rect(DVec2::ZERO, s.paper.size()), 0.0, egui::Color32::from_white_alpha(190));
            }
            continue;
        };
        // Hidden, itself or by an instance above it.
        if shown.hidden {
            continue;
        }
        let el = shown.element;
        let under = shown.under(level.open);
        // Picked: only what is right in the level (an instance is outlined whole, below).
        let alone = !level.printed && under.is_some() && shown.path.len() == level.open.len();
        let picked = alone && selected.contains(&el.id);
        let pv = pv.placed(shown.transform);
        let (rect, (lo, hi)) = match &el.kind {
            ElementKind::View(v) => {
                let Some(r) = d.resolve_as(el.id, shown.projection) else { continue };
                let data = views.get(e, el.id, &r);
                let (lo, hi) = frame_of(&r, data.bx3.map(|(lo, hi)| (lo.truncate(), hi.truncate())));
                let quad = pv.quad(lo, hi);
                let rect = egui::Rect::from_points(&quad);
                // Its picture — its parts rendered, its lines over them, as the style says — as
                // last drawn, under its frame.
                let slot = slot_of(el.id, v, shown.projection);
                if let Some(tex) = ready.get(&slot) {
                    draw_picture(painter, *tex, quad);
                }
                if painter.clip_rect().intersects(rect) && asked.insert(slot) {
                    // Its lines as last computed (new ones on their way when it changed).
                    let got = if v.style == ViewStyle::Rendered {
                        None
                    } else {
                        let (got, busy) = lines.get(slot, data.lines.as_ref(), !level.printed);
                        if busy {
                            painter.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                        }
                        got
                    };
                    let size = (hi - lo) * pv.zoom;
                    if let Some(w) = wanted_picture(slot, v.style, &r, &data, got, egui::vec2(size.x as f32, size.y as f32), ppp) {
                        wanted.push(w);
                    }
                }
                if !level.printed {
                    if picked {
                        stroke_box(painter, &pv, quad, egui::Stroke::new(2.0, frame_ink));
                    } else {
                        let closed = [quad[0], quad[1], quad[2], quad[3], quad[0]];
                        painter.extend(egui::Shape::dashed_line(&closed, egui::Stroke::new(1.0, frame_ink), 6.0, 4.0));
                    }
                    let angle = pv.angle();
                    let label = quad[0] + egui::emath::Rot2::from_angle(angle) * egui::vec2(0.0, -2.0);
                    let name = format!("{} · {}", v.name, scale_label(r.scale));
                    turned_text(painter, label, angle, name, egui::FontId::proportional(11.0), frame_ink);
                }
                if alone {
                    crate::handles::publish(|| format!("sheet_view:{}", el.id.0), rect);
                }
                (rect, (lo, hi))
            }
            ElementKind::Dimension(dim) => {
                let Some(r) = d.resolve_as(dim.view, shown.projection) else { continue };
                let Some(centre) = views.get(e, dim.view, &r).centre() else { continue };
                let shape = crate::dims::shape(e.document(), &r, centre, dim);
                let rect = draw_dim(painter, &pv, &shape);
                if picked {
                    painter.rect_stroke(rect, 2.0, accent, egui::StrokeKind::Outside);
                }
                if alone {
                    crate::handles::publish(|| format!("sheet_element:{}", el.id.0), rect);
                }
                (rect, shape.bounds())
            }
            // `placed` goes through them.
            ElementKind::Instance(_) => continue,
            other => {
                let (r, b) = match other {
                    ElementKind::Text { at, text, size } => {
                        let words = fields.map_or_else(|| text.clone(), |f| f.fill(text, &shown.path));
                        let font = egui::FontId::proportional((size * pv.zoom).max(1.0) as f32);
                        let (r, on_screen) = turned_text(painter, pv.to_screen(*at), pv.angle(), words, font, paper_ink);
                        (r, (*at, *at + DVec2::new(on_screen.x as f64, on_screen.y as f64) / pv.zoom))
                    }
                    ElementKind::Line { from, to } => {
                        let (a, b) = (pv.to_screen(*from), pv.to_screen(*to));
                        painter.line_segment([a, b], thin);
                        (egui::Rect::from_two_pos(a, b).expand(3.0), (from.min(*to), from.max(*to)))
                    }
                    ElementKind::Rect { min, max } => {
                        let quad = pv.quad(*min, *max);
                        stroke_box(painter, &pv, quad, thin);
                        (egui::Rect::from_points(&quad), (*min, *max))
                    }
                    ElementKind::Image { min, max, .. } => {
                        let quad = pv.quad(*min, *max);
                        stroke_box(painter, &pv, quad, thin);
                        painter.line_segment([quad[0], quad[2]], thin);
                        painter.line_segment([quad[3], quad[1]], thin);
                        (egui::Rect::from_points(&quad), (*min, *max))
                    }
                    ElementKind::View(_) | ElementKind::Dimension(_) | ElementKind::Instance(_) => unreachable!(),
                };
                if picked {
                    painter.rect_stroke(r.expand(2.0), 2.0, accent, egui::StrokeKind::Outside);
                }
                if alone {
                    crate::handles::publish(|| format!("sheet_element:{}", el.id.0), r);
                }
                (r, b)
            }
        };
        let Some(top) = under else { continue };
        // What one instance shows comes in a row.
        match boxes.last_mut() {
            Some((id, r)) if *id == top => *r = r.union(rect),
            _ => boxes.push((top, rect)),
        }
        // Its box in the level's axes.
        let rel = to_level * shown.transform;
        for q in [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)].map(|q| rel.transform_point2(q)) {
            let b = out.boxes.entry(top).or_insert((q, q));
            *b = (b.0.min(q), b.1.max(q));
            out.level_box = Some(out.level_box.map_or((q, q), |(a, z)| (a.min(q), z.max(q))));
        }
    }
    // An instance: picked and named by the box of what it shows.
    for (id, r) in &boxes {
        if !level.printed && tops.get(id).is_some_and(|el| el.kind.instance().is_some()) {
            if selected.contains(id) {
                painter.rect_stroke(r.expand(2.0), 2.0, accent, egui::StrokeKind::Outside);
            }
            crate::handles::publish(|| format!("sheet_element:{}", id.0), *r);
        }
    }
    // The composition edited in its grey box.
    if level.open.is_empty() {
        out.level_box = None;
    } else if let Some((lo, hi)) = out.level_box {
        let m = DVec2::splat(LEVEL_MARGIN);
        out.level_box = Some((lo - m, hi + m));
        let q = pv.placed(level.place).quad(lo - m, hi + m);
        let closed = [q[0], q[1], q[2], q[3], q[0]];
        painter.extend(egui::Shape::dashed_line(&closed, egui::Stroke::new(1.0, egui::Color32::from_gray(130)), 5.0, 3.0));
    }
    // A locked one is drawn, not picked.
    boxes.retain(|(id, _)| tops.get(id).is_some_and(|el| !el.locked));
    out.frames = boxes;
    out
}

/// « 1:10 », « 2:1 ».
pub fn scale_label(scale: f64) -> String {
    let n = |v: f64| if (v - v.round()).abs() < 1e-9 { format!("{}", v.round()) } else { format!("{v:.3}") };
    if scale >= 1.0 { format!("{}:1", n(scale)) } else { format!("1:{}", n(1.0 / scale)) }
}

#[cfg(test)]
mod tests {
    use skraft_model::kernel::{Job, JobId, JobResult, KernelService};
    use skraft_model::{Corner, Document, Instance, Place};

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
        let composition = e.document().drawing().element(i).unwrap().kind.instance().unwrap().composition;
        let place = Place::anchored(Corner::BottomRight, DVec2::new(-10.0, 10.0));
        let kind = ElementKind::Instance(Instance { composition, place, props: Default::default() });
        e.apply(Operation::SetElement { element: i, kind }).unwrap();
        (e, sheet, i)
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
            let level = Level { open: &[], place: DAffine2::IDENTITY, printed: false };
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
    fn an_instance_is_picked_whole_where_it_shows_its_composition() {
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
    fn a_point_snaps_on_what_an_instance_shows() {
        let (e, sheet, i) = sheet_with_a_composition();
        let d = e.document().drawing();
        let (on, align) = snap_points(&e, d, &mut ViewsCache::default(), sheet, &[], None);
        for q in [DVec2::new(247.0, 10.0), DVec2::new(287.0, 30.0), DVec2::new(247.0, 20.0)] {
            assert!(on.iter().any(|p| p.distance(q) < 1e-9) && align.iter().any(|p| p.distance(q) < 1e-9), "{q}");
        }
        // Not on the instance being dragged.
        assert!(snap_points(&e, d, &mut ViewsCache::default(), sheet, &[], Some(i)).0.is_empty());
    }

    #[test]
    fn an_anchored_instance_moves_from_its_corner() {
        let (e, _, i) = sheet_with_a_composition();
        let kind = &e.document().drawing().element(i).unwrap().kind;
        let moved = moved_element(kind, DVec2::new(-5.0, 2.0));
        let place = moved.instance().unwrap().place;
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
            let level = Level { open: &[i], place, printed: false };
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
    fn levels_are_entered_and_left_the_instance_picked() {
        let (e, sheet, i) = sheet_with_a_composition();
        let d = e.document().drawing();
        let mut st = SheetState { current: Some(sheet), ..Default::default() };
        assert_eq!(st.container(d), Some(Container::Sheet(sheet)));
        st.selected = vec![i];
        edit_selected(&e, &mut st).unwrap();
        let composition = d.element(i).unwrap().kind.instance().unwrap().composition;
        assert_eq!((st.container(d), st.selected.len()), (Some(Container::Composition(composition)), 0));
        st.leave();
        assert_eq!((st.container(d), st.selected.as_slice()), (Some(Container::Sheet(sheet)), &[i][..]));
    }

    #[test]
    fn a_definition_placed_on_another_sheet_takes_its_corner() {
        let (mut e, _, i) = sheet_with_a_composition();
        let composition = e.document().drawing().element(i).unwrap().kind.instance().unwrap().composition;
        let other = e.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().unwrap().sheet.unwrap();
        let mut st = SheetState { current: Some(other), ..Default::default() };
        place_composition(&mut e, &mut st, composition);
        let d = e.document().drawing();
        let placed = st.selected[0];
        let p = d.element(placed).unwrap().kind.instance().unwrap().place;
        assert_eq!((p.corner, p.at), (Some(Corner::BottomRight), DVec2::new(-10.0, 10.0)));
        assert_eq!(d.container_of(placed), Some(Container::Sheet(other)));
        // Again on the same sheet: a little aside.
        place_composition(&mut e, &mut st, composition);
        let p = e.document().drawing().element(st.selected[0]).unwrap().kind.instance().unwrap().place;
        assert_eq!(p.at, DVec2::new(0.0, 0.0));
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
        let view = |kind| View { name: String::new(), kind, style: ViewStyle::Lines };
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
        let p = d.element(title_block).unwrap().kind.instance().unwrap().place;
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
        };
        let base = e.apply(Operation::AddElement { to, element: ElementKind::View(base) }).unwrap().unwrap().element.unwrap();
        let side =
            View { name: "Side".into(), kind: ViewKind::Projected { base, side: Side::Right, distance: 100.0 }, style: ViewStyle::Lines };
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
    fn a_composition_carried_lands_in_the_level_open_or_right_on_another_sheet() {
        let (mut e, a, i) = sheet_with_a_composition();
        let b = e.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().unwrap().sheet.unwrap();
        let d = e.document().drawing();
        let board = Board::of(d);
        let composition = d.element(i).unwrap().kind.instance().unwrap().composition;
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
        let (a, b) = (d.element(i).unwrap().kind.instance().unwrap(), d.element(copy).unwrap().kind.instance().unwrap());
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
        e.apply(set(skraft_model::FieldOn::Instance(i), text("Dupont"))).unwrap();
        assert_eq!(fill(&e, &[i]), "Dupont {autre}");
        assert_eq!(fill(&e, &[]), "Martin {autre}", "not under the instance");
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

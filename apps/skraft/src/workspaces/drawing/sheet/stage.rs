//! The stage: the board on screen, its camera, and what the pointer does on it.

use super::*;

/// The least room (points, each way) the sheet is framed in.
const MIN_FIT_ROOM: f32 = 160.0;

/// Above the board's top when it is framed: the presentations' and the first sheets' names.
const NAMES_PT: f32 = 54.0;

/// A sheet's name needs this much gap above it (points).
pub(super) const SHEET_NAME_PT: f32 = 18.0;

/// How far above its column's top a presentation's name stands, past the first sheet's name.
pub(super) const COLUMN_NAME_PT: f32 = 28.0;

/// A sheet with less than this share of it in the room counts as off screen.
const REVEAL_SHARE: f32 = 0.25;

/// The camera keeps the active sheet where it is on screen when the layout moves it; a sheet
/// made active otherwise than by a click on it (a new one, the last one deleted) is brought on
/// screen.
pub(super) fn follow_active(st: &mut SheetState, board: &Board, active: Option<Id>) {
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

/// The middle or right button's drag pans the board, the wheel (or a trackpad's pinch) zooms
/// about the pointer; two fingers pan it and pinch it about their middle, one from nothing with
/// the select tool pans it.
fn move_camera(ui: &egui::Ui, response: &egui::Response, st: &mut SheetState, centre: egui::Pos2, pinch: Option<crate::kit::touch::Pinch>) {
    if st.zoom <= 0.0 {
        return;
    }
    // The board's point under `at` stays there as the zoom changes by `factor`.
    let zoom_about = |st: &mut SheetState, at: egui::Pos2, factor: f64| {
        let under = Paperview::new(centre, st.pan, st.zoom).to_sheet(at);
        st.zoom = (st.zoom * factor).clamp(0.01, 200.0);
        st.pan = under - DVec2::new((at.x - centre.x) as f64, (centre.y - at.y) as f64) / st.zoom;
    };
    if let Some(p) = pinch {
        st.pan -= DVec2::new(p.translation.x as f64, -p.translation.y as f64) / st.zoom;
        zoom_about(st, p.center, p.zoom as f64);
        return;
    }
    if st.finger_pan && !response.dragged_by(egui::PointerButton::Primary) {
        st.finger_pan = false;
    }
    let buttons = [egui::PointerButton::Middle, egui::PointerButton::Secondary];
    if buttons.into_iter().any(|b| response.dragged_by(b)) || (st.finger_pan && response.dragged_by(egui::PointerButton::Primary)) {
        let d = response.drag_delta();
        st.pan -= DVec2::new(d.x as f64, -d.y as f64) / st.zoom;
    }
    // A touch's zoom is the pinch's, above.
    if ui.input(|i| i.any_touches()) {
        return;
    }
    if let Some(at) = response.hover_pos() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (scroll as f64 * 0.002).exp() * pinch as f64;
        if factor != 1.0 {
            zoom_about(st, at, factor);
        }
    }
}

/// A click on another sheet makes it active, the camera still, and a double click frames it
/// (docs/ui/drawing.md#the-active-sheet); a drag from it makes it active and goes on there. Not
/// while a tool is under way, nor over what the active sheet shows past its paper. Was the click
/// taken?
pub(super) fn activate(response: &egui::Response, st: &mut SheetState, board: &Board, centre: egui::Pos2, active: Option<Id>) -> bool {
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
    lines: &mut crate::workspaces::drawing::sheet::lines::LinesCache,
    theme: &Theme,
    ready: &HashMap<ViewSlot, egui::TextureId>,
    title: &str,
) -> Vec<Wanted> {
    let mut wanted = Vec::new();
    let ink = viewport::color(theme.ui.view_text);
    let aids = Aids::of(theme);
    ui.painter().rect_filled(rect, 0.0, viewport::color(theme.ui.view));
    crate::kit::handles::publish(|| "sheet".into(), rect);
    if st.framer.is_some() {
        let room = fit.filter(|r| r.width() >= MIN_FIT_ROOM && r.height() >= MIN_FIT_ROOM).unwrap_or(rect);
        return crate::workspaces::drawing::dialogs::view_framer::ui(ui, room, e, st, ready);
    }
    let paper_room = rect;
    let centre = paper_room.center();
    if let Some(ask) = st.asked.take() {
        run_ask(e, st, ask, centre);
    }
    // Framed in what the panels leave; if they leave next to nothing, in the whole stage.
    let fit_room = fit.filter(|r| r.width() >= MIN_FIT_ROOM && r.height() >= MIN_FIT_ROOM).unwrap_or(rect);
    let board = Board::of(e.document().drawing());
    // Two fingers navigate: the stage's tools sense nothing of them (docs/ui/touch.md).
    let (fingers, pinch) = st.touch.update(ui, paper_room);
    let sense = if fingers { egui::Sense::hover() } else { egui::Sense::click_and_drag() };
    let response = ui.interact(paper_room, egui::Id::unique("sheet_stage"), sense);
    if st.zoom <= 0.0 {
        st.framing = Some(Framing::Board);
    }
    let active = st.shown(e.document().drawing()).map(|s| s.id);
    follow_active(st, &board, active);
    apply_framing(st, &board, fit.is_some(), fit_room, paper_room);
    move_camera(ui, &response, st, centre, pinch);
    // A click that made another sheet active, or framed it, is not the tool's.
    let taken = activate(&response, st, &board, centre, active);
    let (clicked, double) = (response.clicked() && !taken, response.double_clicked() && !taken);
    let shown = st.shown(e.document().drawing()).map(|s| (s.id, s.paper));
    let painter = ui.painter().with_clip_rect(paper_room);
    let marked = st.presentation(e.document().drawing()).map(|p| p.id);
    // What the other sheets ask for, after what the active one does.
    let mut others = Vec::new();
    let stage =
        Stage { painter: &painter, response: &response, board: &board, theme, aids, ready, title, paper_room, centre, clicked, double };
    if let Some((id, paper)) = shown {
        let mut out = Asks { lines: &mut *lines, wanted: &mut wanted, others: &mut others };
        active_sheet(ui, e, st, stage, (id, paper), &mut out);
    } else {
        let view = Paperview::new(centre, st.pan, st.zoom);
        draw_other_sheets(&painter, view, &board, aids, None, e, e.document().drawing(), title, ready, lines, &mut st.views, &mut others);
        carry_composition(ui, e, st, paper_room, &board, view, None);
        if board.columns.is_empty() {
            // Wrapped in a narrow stage (a phone's), centred line by line.
            let room = fit.unwrap_or(rect);
            let mut job =
                egui::text::LayoutJob::simple(t!("drawing.no_sheet_hint"), egui::FontId::proportional(14.0), ink, room.width() - 32.0);
            job.halign = egui::Align::Center;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(room.center() - egui::vec2(0.0, galley.size().y / 2.0), galley, ink);
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
    draw_names(&painter, Paperview::new(centre, st.pan, st.zoom), &board, aids, e.document().drawing(), shown.map(|s| s.0), marked);
    wanted
}

/// What a frame of the stage draws with.
#[derive(Clone, Copy)]
struct Stage<'a> {
    painter: &'a egui::Painter,
    response: &'a egui::Response,
    board: &'a Board,
    theme: &'a Theme,
    aids: Aids,
    ready: &'a HashMap<ViewSlot, egui::TextureId>,
    title: &'a str,
    paper_room: egui::Rect,
    centre: egui::Pos2,
    /// A click or a double click that is the tool's (not one that made a sheet active).
    clicked: bool,
    double: bool,
}

/// What the frame asks for: the views' lines, the pictures of the active sheet and the others'.
struct Asks<'a> {
    lines: &'a mut crate::workspaces::drawing::sheet::lines::LinesCache,
    wanted: &'a mut Vec<Wanted>,
    others: &'a mut Vec<Wanted>,
}

/// The active sheet at this frame.
#[derive(Clone, Copy)]
struct OnSheet<'a> {
    stage: Stage<'a>,
    id: Id,
    paper: Paper,
    /// The level open, and what takes its axes to the sheet.
    to: Container,
    place: DAffine2,
    projection: Projection,
    pv: Paperview,
    /// In the axes of the level open: what is picked, moved and drawn there.
    lv: Paperview,
    pointer: Option<egui::Pos2>,
    shift: bool,
    grid: f64,
}

impl OnSheet<'_> {
    /// A handle's point: where it aims, else as far as the pointer went.
    fn grip_to(&self, aim: Option<Aim>, g: &Grip, p: egui::Pos2) -> DVec2 {
        aim.map_or_else(
            || {
                let q = g.target(self.lv.to_sheet(p));
                if g.handle == Handle::Size { top_on_grid(q, self.place, self.grid) } else { q }
            },
            |a| a.at,
        )
    }
}

/// The drawing as shown: `preview` while something is dragged.
fn shown_drawing<'a>(preview: &'a Option<Drawing>, e: &'a Editor) -> &'a Drawing {
    preview.as_ref().unwrap_or_else(|| e.document().drawing())
}

/// A frame of the active sheet: what the pointer did, then what is drawn, then the tool.
fn active_sheet(ui: &mut egui::Ui, e: &mut Editor, st: &mut SheetState, stage: Stage<'_>, (id, paper): (Id, Paper), out: &mut Asks<'_>) {
    let Stage { painter, response, board, aids, ready, title, paper_room, centre, .. } = stage;
    let origin = board.sheet(id).map_or(DVec2::ZERO, |s| s.origin);
    st.current = Some(id);
    let (to, place, projection) = open_level(e, st, id);
    let pv = Paperview::new(centre, st.pan, st.zoom).at(origin);
    // In the axes of the level open: what is picked, moved and drawn there.
    let lv = pv.placed(place);
    // A press with Select on an element drags the selection (it alone when it was not in
    // it), elsewhere draws a selection rectangle; with Rectangle, Circle or Line it starts
    // the shape. The middle or right button's drag pans.
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let shift = ui.input(|i| i.modifiers.shift);
    st.pointer = pointer;
    let paper_grid = e.document().drawing().sheet(id).map_or(0.0, |s| s.grid);
    let s = OnSheet { stage, id, paper, to, place, projection, pv, lv, pointer, shift, grid: paper_grid };
    start_drag(ui, e, st, &s);
    let aim = aim_now(e, st, &s);
    measure_now(e, st, &s, aim);
    let (drop_on, offset, move_aim) = drag_offset(ui, e, st, &s);
    end_drags(ui, e, st, &s, aim, (drop_on, offset));
    finish_sketch(e, st, &s, aim);
    escape_step(ui, st);
    let page = pv.rect(DVec2::ZERO, paper.size());
    draw_paper(painter, page, Some(aids.board));
    // Its grid, faint, while its lines are 6 points apart at least (not printed).
    let grid = e.document().drawing().sheet(id).map_or(0.0, |s| s.grid);
    draw_sheet_grid(painter, &pv, page, grid);
    // The element dragged drawn where it goes (a view's projections with it).
    let preview = previewed(e, st, &s, aim, offset);
    let drawing = shown_drawing(&preview, e);
    // The others as printed, under what the active sheet shows past its paper; from the
    // drawing as previewed, a composition being edited showing there too.
    let view = Paperview::new(centre, st.pan, st.zoom);
    draw_other_sheets(painter, view, board, aids, Some(id), e, drawing, title, ready, out.lines, &mut st.views, out.others);
    // The sheet the selection would land on.
    if let Some(s) = drop_on.filter(|_| st.moving.is_some()).and_then(|s| board.sheet(s)) {
        let target = view.at(s.origin).rect(DVec2::ZERO, s.size);
        painter.rect_stroke(target, 0.0, egui::Stroke::new(2.0, aids.board.gamma_multiply(0.6)), egui::StrokeKind::Outside);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let placed = drawing.placed(id);
    let fields = fields_of(e, drawing, id, title, &placed);
    let level = Level { open: &st.open, place, printed: false, accent: aids.paper };
    let mut asked = HashSet::new();
    let drawn = draw_elements(
        painter,
        &pv,
        e,
        drawing,
        id,
        &placed,
        level,
        &st.selected,
        ready,
        out.lines,
        &mut st.views,
        out.wanted,
        &mut asked,
        fields.as_ref(),
    );
    out.others.retain(|w| !asked.contains(&w.slot));
    (st.frames, st.boxes, st.open_box) = (drawn.frames, drawn.boxes, drawn.level_box);
    draw_aids(ui, e, st, &s, drawing, aim, move_aim);
    tool_frame(ui, e, st, &s, &preview, aim);
    carry_composition(ui, e, st, paper_room, board, Paperview::new(centre, st.pan, st.zoom), Some((id, to, lv)));
}

/// The level open, as far as it still leads somewhere, and what is picked in it.
fn open_level(e: &mut Editor, st: &mut SheetState, id: Id) -> (Container, DAffine2, Projection) {
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
        st.open.iter().filter_map(|i| e.document().drawing().element(*i)?.kind.occurrence().map(|i| i.composition)).collect();
    for b in std::mem::replace(&mut st.was_open, compositions.clone()) {
        if !compositions.contains(&b) && e.document().drawing().composition(b).is_some_and(|x| x.elements.is_empty()) {
            let _ = e.apply(Operation::DeleteComposition { composition: b });
        }
    }
    if let Some(b) = st.place_composition.take() {
        place_composition(e, st, b);
    }
    (to, place, projection)
}

/// A press with the primary button: a handle, a move, a selection rectangle, a shape.
fn start_drag(ui: &egui::Ui, e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>) {
    let OnSheet { stage, id, to, place, pv, lv, shift, .. } = *s;
    let Stage { painter, response, .. } = stage;
    // From where the button went down, not where the drag was recognised.
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(p) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
    {
        // A handle of the element picked alone first.
        let grip = if st.tool == SheetTool::Select { handle_at(e, st, &lv, p) } else { None };
        match st.tool {
            SheetTool::Select if grip.is_some() => st.grip = grip.map(|g| g.with_true_text_top(painter)),
            SheetTool::Select => match st.frames.iter().rev().find(|(_, r)| r.contains(p)) {
                Some((v, _)) => {
                    if !st.selected.contains(v) {
                        if !shift {
                            st.selected.clear();
                        }
                        st.selected.push(*v);
                    }
                    st.moving = Some(start_moving(e, st, id, to, place, lv.to_sheet(p)));
                }
                // A finger pans the board, as it turns the 3D view (docs/ui/touch.md).
                None if ui.input(|i| i.any_touches()) => st.finger_pan = true,
                None => st.marquee = Some(p),
            },
            // A drag from where the button went down (the first click's point, if made).
            SheetTool::Rect | SheetTool::Circle | SheetTool::Line if st.sketch.is_none() => {
                st.sketch = Some(aim_for(e, st, id, pv.zoom, lv.to_sheet(p), None, None, false).at);
            }
            _ => {}
        }
    }
}

/// Where the pointer aims, snapped (docs/ui/drawing.md#snapping): while a shape or a text
/// is put, or a box's or a line's handle dragged; Shift keeps a line horizontal or
/// vertical.
fn aim_now(e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>) -> Option<Aim> {
    let OnSheet { id, pv, lv, pointer, shift, .. } = *s;
    let grip = st.grip.clone();
    let line_end = grip.as_ref().is_some_and(|g| matches!(g.handle, Handle::End(_)));
    match (pointer, grip) {
        (Some(p), Some(g)) => {
            g.snaps().map(|from| aim_for(e, st, id, pv.zoom, g.target(lv.to_sheet(p)), Some(g.element), from, shift && line_end))
        }
        (Some(p), None) if matches!(st.tool, SheetTool::Rect | SheetTool::Circle | SheetTool::Line | SheetTool::Text) => {
            let lock = shift && st.tool == SheetTool::Line && st.sketch.is_some();
            Some(aim_for(e, st, id, pv.zoom, lv.to_sheet(p), None, st.sketch, lock))
        }
        _ => None,
    }
}

/// The measurements box, and a value typed in it.
fn measure_now(e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>, aim: Option<Aim>) {
    let OnSheet { stage, to, pointer, shift, .. } = *s;
    let response = stage.response;
    if response.clicked() || response.drag_started() {
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
        (Some(from), _, _) if matches!(st.tool, SheetTool::Rect | SheetTool::Circle | SheetTool::Line) => {
            Some(sketch_grip(st.tool, from, st.sketch_to.unwrap_or(from)))
        }
        (_, Some(g), Some(p)) => Some(Grip { kind: regrip(g, s.grip_to(aim, g, p), shift), ..g.clone() }),
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
            Err(m) => e.refuse(m),
        }
    }
}

/// How far what is moved goes: the sheet it would land on, its offset, what it snaps on.
fn drag_offset(ui: &egui::Ui, e: &Editor, st: &SheetState, s: &OnSheet<'_>) -> (Option<Id>, DVec2, Option<Aim>) {
    let OnSheet { stage, id, place, pv, lv, pointer, grid, .. } = *s;
    let Stage { board, paper_room, centre, .. } = stage;
    let dragged = match (&st.moving, pointer) {
        (Some(m), Some(p)) => lv.to_sheet(p) - m.from,
        _ => DVec2::ZERO,
    };
    // The sheet the selection would move to, let go here.
    let drop_on = match (&st.moving, pointer) {
        (Some(_), Some(p))
            if st.open.is_empty() && on_stage(ui.ctx(), paper_room, p) && can_leave_sheet(e.document().drawing(), &st.selected) =>
        {
            board.sheet_at(Paperview::new(centre, st.pan, st.zoom).to_sheet(p)).filter(|s| *s != id)
        }
        _ => None,
    };
    // Snapped on this sheet (docs/ui/drawing.md#snapping); carried over another, as it goes.
    let (offset, move_aim) = match &st.moving {
        Some(m) if pointer.is_some() && drop_on.is_none() => m.aim(place, dragged, SNAP_PT as f64 / pv.zoom, grid),
        _ => (dragged, None),
    };
    (drop_on, offset, move_aim)
}

/// A drag let go: a handle, a move, a selection rectangle.
fn end_drags(
    ui: &egui::Ui,
    e: &mut Editor,
    st: &mut SheetState,
    s: &OnSheet<'_>,
    aim: Option<Aim>,
    (drop_on, offset): (Option<Id>, DVec2),
) {
    let OnSheet { stage, id, pointer, shift, .. } = *s;
    let Stage { response, board, .. } = stage;
    // A handle let go: its element changed (one `SetElement`).
    if response.drag_stopped()
        && let Some(g) = st.grip.take()
        && let Some(p) = pointer
    {
        let kind = regrip(&g, s.grip_to(aim, &g, p), shift);
        if kind != g.kind {
            let _ = e.apply(Operation::SetElement { element: g.element, kind: kind.clone() });
        }
        st.amend = Some(Grip { kind, ..g });
    }
    if response.drag_stopped() && st.moving.take().is_some() && offset != DVec2::ZERO {
        if let Some(to) = drop_on {
            drop_on_sheet(e, st, board, id, to, offset);
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
}

/// A shape's point given, by a click or the end of a drag.
fn finish_sketch(e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>, aim: Option<Aim>) {
    let OnSheet { stage, to, lv, pointer, .. } = *s;
    let Stage { response, clicked, .. } = stage;
    // A rectangle, a circle or a line: two clicks (its corners, its centre and a point of it,
    // its ends) or a drag, as the 3D view's shapes (docs/tools/interaction.md); under 0.5 mm,
    // the second point still to come.
    if matches!(st.tool, SheetTool::Rect | SheetTool::Circle | SheetTool::Line)
        && let Some(p) = pointer
        && (clicked || response.drag_stopped_by(egui::PointerButton::Primary))
    {
        let end = aim.map_or_else(|| lv.to_sheet(p), |a| a.at);
        match st.sketch {
            None => st.sketch = Some(end),
            Some(from) if from.distance(end) > 0.5 => {
                st.sketch = None;
                let element = sketch_grip(st.tool, from, end).kind;
                if let Ok(Some(o)) = e.apply(Operation::AddElement { to, element }) {
                    st.selected = o.element.into_iter().collect();
                    st.amend = o.element.map(|element| Grip { element, ..sketch_grip(st.tool, from, end) });
                }
            }
            Some(_) => {}
        }
    }
}

fn escape_step(ui: &egui::Ui, st: &mut SheetState) {
    // Esc goes back one step of the tool: the shape or rectangle drawn dropped, the picks;
    // with Select the selection cleared; a tool stays (Space, `tool.select`, ends it), as in
    // the 3D view (docs/tools/interaction.md#selection). Not while a field is typed in.
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape)) && ui.ctx().memory(|m| m.focused().is_none()) && st.picker.is_none();
    if std::mem::take(&mut st.esc_eaten) {
    } else if esc {
        st.amend = None;
        if st.sketch.take().is_some() || st.marquee.take().is_some() || st.moving.take().is_some() || st.grip.take().is_some() {
        } else if !st.picks.is_empty() {
            st.picks.clear();
        } else if st.tool == SheetTool::Place {
            st.set_tool(SheetTool::Select);
        } else if st.tool == SheetTool::Select {
            // The selection first, then one level up.
            if st.selected.is_empty() {
                st.leave();
            } else {
                st.selected.clear();
            }
        }
    }
}

/// The sheet's grid, faint, while its lines are 6 points apart at least (not printed).
fn draw_sheet_grid(painter: &egui::Painter, pv: &Paperview, page: egui::Rect, grid: f64) {
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
}

/// The drawing with what is dragged where it goes; none while nothing is.
fn previewed(e: &Editor, st: &SheetState, s: &OnSheet<'_>, aim: Option<Aim>, offset: DVec2) -> Option<Drawing> {
    let OnSheet { pointer, shift, .. } = *s;
    let gripped = match (&st.grip, pointer) {
        (Some(g), Some(p)) => Some((g.element, regrip(g, s.grip_to(aim, g, p), shift))),
        _ => None,
    };
    match st.moving {
        _ if let Some((id, kind)) = gripped => {
            let mut d = e.document().drawing().clone();
            if let Some(el) = d.element_mut(id) {
                el.kind = kind;
            }
            Some(d)
        }
        Some(_) if offset != DVec2::ZERO => {
            let mut d = e.document().drawing().clone();
            for (v, kind) in moves(&d, &st.selected, offset) {
                if let Some(el) = d.element_mut(v) {
                    el.kind = kind;
                }
            }
            Some(d)
        }
        _ => None,
    }
}

/// What is drawn over the elements: handles, the selection rectangle, snaps, the shape drawn.
fn draw_aids(ui: &egui::Ui, e: &Editor, st: &mut SheetState, s: &OnSheet<'_>, drawing: &Drawing, aim: Option<Aim>, move_aim: Option<Aim>) {
    let OnSheet { stage, projection, pv, lv, pointer, .. } = *s;
    let Stage { painter, theme, aids, .. } = stage;
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
        let stroke = egui::Stroke::new(1.0, aids.paper);
        for (h, q) in shown.unwrap_or_default() {
            let at = lv.to_screen(q);
            overlay::square_grip(painter, at, GRIP_SIDE, egui::Color32::WHITE, stroke);
            let over = st.grip.as_ref().map(|g| g.handle == h).unwrap_or_else(|| pointer.is_some_and(|p| p.distance(at) <= GRIP_PT));
            if over {
                ui.ctx().set_cursor_icon(h.cursor());
            }
        }
    }
    // The selection rectangle being drawn: solid left to right, dashed right to left.
    if let (Some(from), Some(p)) = (st.marquee, pointer) {
        overlay::marquee(painter, egui::Rect::from_two_pos(from, p), p.x < from.x, theme);
    }
    // What the pointer snaps on.
    if let Some(a) = aim.as_ref().or(move_aim.as_ref()) {
        draw_aim(painter, &pv, a);
    }
    // The shape being drawn.
    if let (Some(from), Some(p)) = (st.sketch, pointer) {
        let stroke = egui::Stroke::new(1.5, aids.paper);
        let end = aim.map_or_else(|| lv.to_sheet(p), |a| a.at);
        if st.tool == SheetTool::Rect {
            painter.add(egui::Shape::closed_line(lv.quad(from, end).to_vec(), stroke));
        } else if st.tool == SheetTool::Circle {
            let (c, q) = (lv.to_screen(from), lv.to_screen(end));
            painter.circle_stroke(c, c.distance(q), stroke);
            overlay::dashed(painter, &[c, q], egui::Stroke::new(1.0, stroke.color), overlay::ALIGNMENT);
        } else {
            painter.line_segment([lv.to_screen(from), lv.to_screen(end)], stroke);
        }
    }
}

/// What the active tool does with this frame's pointer.
fn tool_frame(ui: &egui::Ui, e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>, preview: &Option<Drawing>, aim: Option<Aim>) {
    match st.tool {
        SheetTool::Select => select_tool(e, st, s, preview),
        SheetTool::Text => text_tool(ui, e, st, s, aim),
        SheetTool::Rect | SheetTool::Circle | SheetTool::Line => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
        tool @ (SheetTool::Linear | SheetTool::Radius | SheetTool::Diameter | SheetTool::Angle) => {
            dimension_tool(ui, e, st, s, preview, tool)
        }
        SheetTool::Place => place_tool(ui, e, st, s, preview),
        SheetTool::Project { base } => project_tool(ui, e, st, s, preview, base),
    }
}

fn select_tool(e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>, preview: &Option<Drawing>) {
    let OnSheet { stage, id, paper, pv, lv, shift, .. } = *s;
    let Stage { response, clicked, double, .. } = stage;
    let drawing = shown_drawing(preview, e);
    let at = response.interact_pointer_pos();
    let hit = at.and_then(|p| st.frames.iter().rev().find(|(_, r)| r.contains(p)).map(|(v, _)| *v));
    // A double click enters an occurrence; on the sheet's void it frames the sheet, off
    // its paper the whole board.
    if double {
        let occurrence = hit.filter(|v| drawing.element(*v).is_some_and(|el| el.kind.occurrence().is_some()));
        match (occurrence, hit) {
            (Some(i), _) => st.enter(i),
            // A view: its point of view (decision 0051).
            (None, Some(v)) if drawing.element(v).is_some_and(|el| matches!(el.kind, ElementKind::View(_))) => {
                if let Err(why) = crate::workspaces::drawing::dialogs::view_framer::open(e, st, v) {
                    e.refuse(why);
                }
            }
            (None, None) if st.open.is_empty() => {
                let on_paper = at.map(|p| pv.to_sheet(p)).is_some_and(|q| q.cmpge(DVec2::ZERO).all() && q.cmple(paper.size()).all());
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

fn text_tool(ui: &egui::Ui, e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>, aim: Option<Aim>) {
    let OnSheet { stage, to, lv, .. } = *s;
    let Stage { response, clicked, .. } = stage;
    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    // A text there, picked, to be written in the entity info.
    if clicked
        && let Some(p) = response.interact_pointer_pos()
        && let Ok(Some(o)) = e.apply(Operation::AddElement {
            to,
            element: ElementKind::Text { at: aim.map_or_else(|| lv.to_sheet(p), |a| a.at), text: t!("drawing.text_default"), size: 3.5 },
        })
    {
        st.selected = o.element.into_iter().collect();
        st.tool = SheetTool::Select;
    }
}

fn dimension_tool(ui: &egui::Ui, e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>, preview: &Option<Drawing>, tool: SheetTool) {
    let OnSheet { stage, to, projection, pv, lv, .. } = *s;
    let Stage { painter, response, clicked, .. } = stage;
    let drawing = shown_drawing(preview, e);
    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    if let Some(p) = response.hover_pos() {
        let at = lv.to_sheet(p);
        let mark = egui::Stroke::new(2.0, viewport::color(Inks::LIGHT.pick));
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
                draw_dim(painter, &lv, &crate::workspaces::drawing::dims::shape(e.document(), &r, c, &dim));
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

fn place_tool(ui: &egui::Ui, e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>, preview: &Option<Drawing>) {
    let OnSheet { stage, id, paper, to, lv, .. } = *s;
    let Stage { painter, response, board, centre, clicked, .. } = stage;
    let drawing = shown_drawing(preview, e);
    let board_view = Paperview::new(centre, st.pan, st.zoom);
    let over = response.hover_pos().filter(|p| board.sheet_at(board_view.to_sheet(*p)) == Some(id));
    match (st.placing.clone(), over) {
        (None, _) => st.tool = SheetTool::Select,
        (Some(placing), Some(p)) => {
            // Its frame where a click puts it, at the largest scale fitting half the paper.
            let at = lv.to_sheet(p);
            let scale = placing.size.map_or(1.0, |s| scale_to_fit(s, paper.size() / 2.0));
            let half = placing.size.map_or(DVec2::splat(10.0), |s| s * scale / 2.0);
            let q = lv.quad(at - half, at + half);
            overlay::dashed_quad(painter, q, egui::Stroke::new(1.0, GHOST_INK), overlay::GHOST);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if clicked {
                let n = drawing.elements_in(to).unwrap_or(&[]).iter().filter(|e| matches!(e.kind, ElementKind::View(_))).count() + 1;
                let look = Look::Standard(Direction::Front);
                let kind = ViewKind::Base { subject: placing.subject, look, scale, at };
                let view = View { name: t!("drawing.view_name", n = n), kind, style: ViewStyle::Lines, crop: None };
                if let Ok(Some(o)) = e.apply(Operation::AddElement { to, element: ElementKind::View(view) }) {
                    st.selected = o.element.into_iter().collect();
                }
                st.set_tool(SheetTool::Select);
            }
        }
        _ => {}
    }
}

fn project_tool(ui: &egui::Ui, e: &mut Editor, st: &mut SheetState, s: &OnSheet<'_>, preview: &Option<Drawing>, base: Id) {
    let OnSheet { stage, to, projection, lv, .. } = *s;
    let Stage { painter, response, clicked, .. } = stage;
    let drawing = shown_drawing(preview, e);
    if drawing.resolve_as(base, projection).is_none() || drawing.container_of(base) != Some(to) {
        st.tool = SheetTool::Select;
    } else if let Some(p) = response.hover_pos()
        && let Some((side, distance)) = projection_at(e, drawing, base, lv.to_sheet(p), projection)
    {
        // Its frame where a click puts it.
        let r = drawing.resolve_as(base, projection).unwrap();
        let ghost = Resolved { frame: r.frame.projected(side, projection), at: r.at + side.dir() * distance, crop: None, ..r.clone() };
        let (lo, hi) = view_frame(e, &ghost);
        let q = lv.quad(lo, hi);
        overlay::dashed_quad(painter, q, egui::Stroke::new(1.0, GHOST_INK), overlay::GHOST);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        if clicked {
            let n = drawing.elements_in(to).unwrap_or(&[]).iter().filter(|e| matches!(e.kind, ElementKind::View(_))).count() + 1;
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
                crop: None,
            };
            if let Ok(Some(o)) = e.apply(Operation::AddElement { to, element: ElementKind::View(view) }) {
                st.selected = o.element.into_iter().collect();
            }
        }
    }
}

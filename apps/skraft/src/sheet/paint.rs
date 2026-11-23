//! Painting the board and its sheets, their elements and aids.

use super::*;

/// Sheet millimetres (y up) to screen points, and back; drawing in the axes of what `place`
/// puts on the sheet (an instance's content).
#[derive(Clone, Copy)]
pub(super) struct Paperview {
    pub(super) centre: egui::Pos2,
    /// The board point at `centre`.
    pub(super) pan: DVec2,
    pub(super) zoom: f64,
    /// The sheet's origin on the board.
    pub(super) origin: DVec2,
    pub(super) place: DAffine2,
}

impl Paperview {
    pub(super) fn new(centre: egui::Pos2, pan: DVec2, zoom: f64) -> Self {
        Paperview { centre, pan, zoom, origin: DVec2::ZERO, place: DAffine2::IDENTITY }
    }
    /// This view drawing a sheet whose origin lies at `origin` on the board.
    pub(super) fn at(self, origin: DVec2) -> Self {
        Paperview { origin, ..self }
    }
    /// This view drawing in the axes `place` puts on the sheet.
    pub(super) fn placed(self, place: DAffine2) -> Self {
        Paperview { place, ..self }
    }
    pub(super) fn to_screen(self, p: DVec2) -> egui::Pos2 {
        let d = (self.place.transform_point2(p) + self.origin - self.pan) * self.zoom;
        egui::pos2(self.centre.x + d.x as f32, self.centre.y - d.y as f32)
    }
    /// A screen point in the axes it draws in.
    pub(super) fn to_sheet(self, p: egui::Pos2) -> DVec2 {
        let on_sheet = self.pan - self.origin + DVec2::new((p.x - self.centre.x) as f64, (self.centre.y - p.y) as f64) / self.zoom;
        self.place.inverse().transform_point2(on_sheet)
    }
    /// The corners of box `a`–`b` on screen: its top left first, then clockwise.
    pub(super) fn quad(self, a: DVec2, b: DVec2) -> [egui::Pos2; 4] {
        let (lo, hi) = (a.min(b), a.max(b));
        [DVec2::new(lo.x, hi.y), hi, DVec2::new(hi.x, lo.y), lo].map(|p| self.to_screen(p))
    }
    /// What box `a`–`b` covers on screen.
    pub(super) fn rect(self, a: DVec2, b: DVec2) -> egui::Rect {
        egui::Rect::from_points(&self.quad(a, b))
    }
    /// Is what it draws turned?
    pub(super) fn turned(self) -> bool {
        self.place.matrix2.x_axis.y.abs() > 1e-12 || self.place.matrix2.x_axis.x < 0.0
    }
    /// How much what it draws is turned on screen (radians, the screen's y down); 0 when not.
    pub(super) fn angle(self) -> f32 {
        if self.turned() { -self.place.matrix2.x_axis.y.atan2(self.place.matrix2.x_axis.x) as f32 } else { 0.0 }
    }
}

/// A dimension that lost what it measures, on the paper.
pub(super) const PAPER_ERROR: egui::Color32 = egui::Color32::from_rgb(210, 40, 40);

/// A frame showing where a click would place a view.
pub(super) const GHOST_INK: egui::Color32 = egui::Color32::from_gray(120);

/// The colours of the sheet's aids, by the ground they stand on (docs/ui/drawing.md#the-board).
#[derive(Clone, Copy)]
pub(super) struct Aids {
    /// On the board, the view's ground of the mode: the active sheet's outline and name.
    pub(super) board: egui::Color32,
    /// A text on a fill of `board`.
    on_board: egui::Color32,
    /// A text right on the board.
    pub(super) text: egui::Color32,
    /// On the paper, white in either mode: what is picked, handles, what is being drawn.
    pub(super) paper: egui::Color32,
}

impl Aids {
    pub(super) fn of(theme: &Theme) -> Self {
        Aids {
            board: viewport::color(theme.ui.accent),
            on_board: viewport::color(theme.ui.panel),
            text: viewport::color(theme.ui.view_text),
            paper: viewport::color(theme.ui.paper_accent),
        }
    }
}

/// A sheet's white paper with its shadow, the active one outlined.
pub(super) fn draw_paper(painter: &egui::Painter, page: egui::Rect, active: Option<egui::Color32>) {
    painter.rect_filled(page.translate(egui::vec2(3.0, 4.0)), 0.0, egui::Color32::from_black_alpha(40));
    painter.rect_filled(page, 0.0, egui::Color32::WHITE);
    let stroke = active.map_or(egui::Stroke::new(1.0, egui::Color32::from_gray(160)), |accent| egui::Stroke::new(2.0, accent));
    painter.rect_stroke(page, 0.0, stroke, egui::StrokeKind::Outside);
}

/// The sheets other than `active`, as printed: clipped to their paper, without their views'
/// frames and names (docs/ui/drawing.md#the-active-sheet); those off the stage skipped. The
/// columns without a sheet say how to make one.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_other_sheets(
    painter: &egui::Painter,
    view: Paperview,
    board: &Board,
    aids: Aids,
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
            let ink = egui::Color32::from_gray(140);
            overlay::dashed_rect(painter, r, egui::Stroke::new(1.0, ink), overlay::FRAME);
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
            draw_paper(painter, page, None);
            let on_paper = painter.with_clip_rect(clip.intersect(page));
            let placed = d.placed(s.id);
            let fields = fields_of(e, d, s.id, title, &placed);
            let level = Level { open: &[], place: DAffine2::IDENTITY, printed: true, accent: aids.paper };
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

/// The height of a marked presentation's name.
const COLUMN_PILL_PT: f32 = 22.0;

/// A marked column: its name on the selection colour, where an unmarked one stands, and a dotted
/// frame broken around it, whose sides stay within half the gap to the next column.
fn draw_marked_column(
    painter: &egui::Painter,
    view: Paperview,
    board: &Board,
    aids: Aids,
    column: &crate::sheet_board::Column,
    name: &str,
    (above, room): (f32, f32),
) {
    let (lo, hi) = column.bounds();
    let r = view.rect(lo, hi);
    let zoom = view.zoom as f32;
    let margin = (board.gap as f32 * zoom * 0.5).max(6.0).min(board.column_gap as f32 * zoom * 0.45);
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), egui::FontId::proportional(14.0), aids.on_board);
    job.wrap = egui::text::TextWrapping::truncate_at_width((room - 16.0).max(1.0));
    let galley = painter.layout_job(job);
    let middle = r.top() - above - galley.size().y / 2.0;
    let frame = egui::Rect::from_min_max(egui::pos2(r.left() - margin, middle), egui::pos2(r.right() + margin, r.bottom() + margin));
    let pill = egui::Rect::from_center_size(egui::pos2(frame.center().x, middle), egui::vec2(galley.size().x + 16.0, COLUMN_PILL_PT));
    let (left, right) = (pill.left().max(frame.left()), pill.right().min(frame.right()));
    let path = [
        egui::pos2(right, middle),
        frame.right_top(),
        frame.right_bottom(),
        frame.left_bottom(),
        frame.left_top(),
        egui::pos2(left, middle),
    ];
    overlay::dashed(painter, &path, egui::Stroke::new(1.5, aids.board), overlay::SELECTION);
    painter.rect_filled(pill, 4.0, aids.board);
    painter.galley(pill.center() - galley.size() / 2.0, galley, aids.on_board);
}

/// The presentations' names on top of their columns, the one of `marked` standing out, and the
/// sheets' above them while their gap has room.
pub(super) fn draw_names(
    painter: &egui::Painter,
    view: Paperview,
    board: &Board,
    aids: Aids,
    d: &Drawing,
    active: Option<Id>,
    marked: Option<Id>,
) {
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
                let color = if Some(s.id) == active { aids.board } else { aids.text.gamma_multiply(0.7) };
                board_name(painter, top - egui::vec2(0.0, 3.0), &sheet.name, 11.0, color, s.size.x as f32 * zoom);
            }
        }
        let Some(p) = d.presentation(column.presentation) else { continue };
        let top = view.to_screen(DVec2::new(column.centre, 0.0));
        let above = if with_sheets { COLUMN_NAME_PT } else { 4.0 };
        let room = (column.width + board.column_gap) as f32 * zoom;
        if Some(column.presentation) == marked {
            draw_marked_column(painter, view, board, aids, column, &p.name, (above, room));
        } else {
            board_name(painter, top - egui::vec2(0.0, above), &p.name, 14.0, aids.text, room);
        }
    }
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
pub(super) struct Level<'a> {
    pub(super) open: &'a [Id],
    pub(super) place: DAffine2,
    /// A sheet drawn as printed: without its views' frames and names, nothing to pick.
    pub(super) printed: bool,
    /// Its views' frames and what is picked ([`Aids::paper`]).
    pub(super) accent: egui::Color32,
}

/// What [`draw_elements`] leaves for the pointer.
#[derive(Default)]
pub(super) struct Drawn {
    /// What a press lands on, bottom first: what is right in the level open, on screen.
    pub(super) frames: Vec<(Id, egui::Rect)>,
    /// The same in the level's axes.
    pub(super) boxes: HashMap<Id, (DVec2, DVec2)>,
    /// All it shows there, a little wider: the box a click off leaves it.
    pub(super) level_box: Option<(DVec2, DVec2)>,
}

/// How far the box of the composition edited stands off what it shows (paper millimetres).
const LEVEL_MARGIN: f64 = 2.0;

/// What a sheet shows through its instances (docs/ui/drawing.md#compositions), bottom first: views as
/// their frames, named; texts, lines, rectangles; images as their frames for now. With a composition
/// entered, what lies outside it first, faded under a veil of the paper's white, then what it
/// shows in its grey box. A press lands on what is right in the level, an instance whole by the
/// box of what it shows.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_elements(
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
    let frame_ink = level.accent;
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
                        overlay::dashed_quad(painter, quad, egui::Stroke::new(1.0, frame_ink), overlay::FRAME);
                    }
                    let angle = pv.angle();
                    let label = quad[0] + egui::emath::Rot2::from_angle(angle) * egui::vec2(0.0, -2.0);
                    let shown_scale = if r.perspective.is_some() { t!("drawing.no_scale") } else { scale_label(r.scale) };
                    let name = format!("{} · {}", v.name, shown_scale);
                    turned_text(painter, label, angle, name, egui::FontId::proportional(11.0), frame_ink);
                }
                if alone {
                    crate::handles::publish(|| format!("sheet_view:{}", el.id.0), rect);
                }
                (rect, (lo, hi))
            }
            ElementKind::Dimension(dim) => {
                let Some(r) = d.resolve_as(dim.view, shown.projection) else { continue };
                // Kept, not drawn: a perspective has no measure to read (decision 0051).
                if r.perspective.is_some() {
                    continue;
                }
                let Some(centre) = views.get(e, dim.view, &r).centre() else { continue };
                let shape = crate::dims::shape(e.document(), &r, centre, dim);
                let rect = draw_dim(painter, &pv, &shape);
                if picked {
                    painter.rect_stroke(rect, 0.0, accent, egui::StrokeKind::Middle);
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
                    ElementKind::Circle { center, radius } => {
                        let (c, r) = (pv.to_screen(*center), (*radius * pv.zoom) as f32);
                        painter.circle_stroke(c, r, thin);
                        let half = DVec2::splat(*radius);
                        (egui::Rect::from_center_size(c, egui::Vec2::splat(2.0 * r)), (*center - half, *center + half))
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
                // On its box, as a view's frame.
                if picked {
                    stroke_box(painter, &pv, pv.quad(b.0, b.1), accent);
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
                painter.rect_stroke(*r, 0.0, accent, egui::StrokeKind::Middle);
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
        overlay::dashed_quad(painter, q, egui::Stroke::new(1.0, egui::Color32::from_gray(130)), overlay::LEVEL);
    }
    // A locked one is drawn, not picked.
    boxes.retain(|(id, _)| tops.get(id).is_some_and(|el| !el.locked));
    out.frames = boxes;
    out
}

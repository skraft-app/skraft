//! What the pointer snaps on while drawing or moving on a sheet.

use super::*;

/// Where the pointer aims on a sheet (docs/ui/drawing.md#snapping).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Aim {
    /// The point, in the axes of the level open (the sheet's, none open).
    pub(super) at: DVec2,
    /// The same on the sheet, where its marks are drawn; `along_x` and `along_y` are too.
    pub(super) mark: DVec2,
    /// Snapped on a point (an element's corner or end, a view's vertex or frame corner).
    pub(super) point: bool,
    /// The point it is aligned with along the sheet's x (the same y), along its y (the same x).
    pub(super) along_x: Option<DVec2>,
    pub(super) along_y: Option<DVec2>,
}

/// The points of sheet `sheet` a point snaps on, and those it aligns with: rectangles' and
/// images' corners, circles' centres and quadrants, lines' ends, views' frame corners (both);
/// views' vertices (snapped on only: so many would align everywhere), through its occurrences; not
/// those of `skip` (right in the level `open`), nor what is hidden.
pub(super) fn snap_points(
    e: &Editor,
    d: &Drawing,
    views: &mut ViewsCache,
    sheet: Id,
    open: &[Id],
    skip: &[Id],
) -> (Vec<DVec2>, Vec<DVec2>) {
    let (mut on, mut align) = (Vec::new(), Vec::new());
    let Some(s) = d.sheet(sheet) else { return (on, align) };
    let corners = |lo: DVec2, hi: DVec2| [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
    for shown in d.placed(s.id) {
        if shown.hidden || shown.under(open).is_some_and(|u| skip.contains(&u)) {
            continue;
        }
        let t = shown.transform;
        let el = shown.element;
        match &el.kind {
            ElementKind::Rect { min, max } | ElementKind::Image { min, max, .. } => {
                align.extend(corners(*min, *max).map(|q| t.transform_point2(q)));
            }
            ElementKind::Line { from, to } => align.extend([*from, *to].map(|q| t.transform_point2(q))),
            ElementKind::Circle { center, radius } => {
                let r = *radius;
                let points = [DVec2::ZERO, DVec2::X * r, DVec2::Y * r, DVec2::NEG_X * r, DVec2::NEG_Y * r];
                align.extend(points.map(|q| t.transform_point2(*center + q)));
            }
            ElementKind::View(_) => {
                let Some(r) = d.resolve_as(el.id, shown.projection) else { continue };
                let data = views.get(e, el.id, &r);
                let (lo, hi) = frame_of(&r, data.bx3.map(|(lo, hi)| (lo.truncate(), hi.truncate())));
                align.extend(corners(lo, hi).map(|q| t.transform_point2(q)));
                // Its vertices are placed as seen parallel.
                if r.perspective.is_none() {
                    on.extend(data.snaps(e, &r).vertices.iter().map(|v| t.transform_point2(v.3)));
                }
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
pub(super) fn aim_at(on: &[DVec2], align: &[DVec2], p: DVec2, reach: f64, from: Option<DVec2>, grid: f64) -> Aim {
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
pub(super) fn aim_for(
    e: &Editor,
    st: &mut SheetState,
    sheet: Id,
    zoom: f64,
    p: DVec2,
    skip: Option<Id>,
    from: Option<DVec2>,
    lock: bool,
) -> Aim {
    let d = e.document().drawing();
    let grid = d.sheet(sheet).map_or(0.0, |s| s.grid);
    let place = d.opened(sheet, &st.open).map_or(DAffine2::IDENTITY, |(_, t)| t);
    let (p, from) = (place.transform_point2(p), from.map(|f| place.transform_point2(f)));
    let open = st.open.clone();
    let (on, align) = snap_points(e, d, &mut st.views, sheet, &open, skip.as_slice());
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

/// The selection being dragged (docs/ui/drawing.md#snapping): what it snaps with and on, taken
/// once when the drag starts (nothing else changes meanwhile).
#[derive(Clone, Debug)]
pub(super) struct Moving {
    /// Where it was taken, in the axes of the level open.
    pub(super) from: DVec2,
    /// Its box on the sheet as it was, if it has one.
    pub(super) bounds: Option<(DVec2, DVec2)>,
    /// The sheet's points it snaps on and aligns with, what moves with it left out.
    pub(super) on: Vec<DVec2>,
    pub(super) align: Vec<DVec2>,
    /// The paper's centre, which its box's middle snaps on.
    pub(super) centre: Option<DVec2>,
    /// The sheet's axes it can move along (a projected view, a linear dimension: one).
    pub(super) free: (bool, bool),
    /// The level open is turned from the sheet.
    pub(super) turned: bool,
}

/// The axes (of the level) the elements `moved` can go along together.
fn free_axes(d: &Drawing, moved: &[Id]) -> (bool, bool) {
    let mut free = (true, true);
    for id in moved.iter().filter(|id| !carried(d, **id, moved)) {
        match d.element(*id).map(|el| &el.kind) {
            Some(ElementKind::View(View { kind: ViewKind::Projected { side, .. }, .. })) => {
                let along_x = side.dir().x != 0.0;
                free = (free.0 && along_x, free.1 && !along_x);
            }
            Some(ElementKind::Dimension(Dimension { kind: DimKind::Linear { orient, .. }, .. })) => {
                let along_x = matches!(orient, Orient::Vertical);
                free = (free.0 && along_x, free.1 && !along_x);
            }
            _ => {}
        }
    }
    free
}

/// The selection taken at `from` (axes of the level `to`, put on the sheet by `place`).
pub(super) fn start_moving(e: &Editor, st: &mut SheetState, sheet: Id, to: Container, place: DAffine2, from: DVec2) -> Moving {
    let d = e.document().drawing();
    let mut bounds: Option<(DVec2, DVec2)> = None;
    for (lo, hi) in st.selected.iter().filter_map(|id| st.boxes.get(id)) {
        for q in [*lo, DVec2::new(hi.x, lo.y), *hi, DVec2::new(lo.x, hi.y)].map(|q| place.transform_point2(q)) {
            bounds = Some(bounds.map_or((q, q), |(a, z)| (a.min(q), z.max(q))));
        }
    }
    // What moves with it is no target: a view's projections and dimensions.
    let skip = e.document().families(to, &st.selected).unwrap_or_else(|_| st.selected.clone());
    let open = st.open.clone();
    let (on, align) = snap_points(e, d, &mut st.views, sheet, &open, &skip);
    let turned = place.matrix2.x_axis.y.abs() > 1e-12 || place.matrix2.x_axis.x < 0.0;
    let centre = d.sheet(sheet).map(|s| s.paper.size() / 2.0);
    Moving { from, bounds, on, align, centre, free: free_axes(d, &st.selected), turned }
}

impl Moving {
    /// The offset (in the level's axes) for the pointer gone `offset` from where it took it, and
    /// what it snapped on. An axis it cannot go along in a turned level: not snapped at all.
    pub(super) fn aim(&self, place: DAffine2, offset: DVec2, reach: f64, grid: f64) -> (DVec2, Option<Aim>) {
        let Some((lo, hi)) = self.bounds else { return (offset, None) };
        if self.turned && self.free != (true, true) {
            return (offset, None);
        }
        let on_sheet = place.transform_vector2(offset);
        let (snapped, aim) = aim_box(&self.on, &self.align, self.centre, lo, hi, on_sheet, reach, grid, self.free);
        // Unchanged, exactly: no rounding through the level's axes.
        let out = if snapped == on_sheet { offset } else { place.inverse().transform_vector2(snapped) };
        (out, Some(aim))
    }
}

/// Box `lo`–`hi` dragged by `offset`, snapped (all on the sheet): one of its corners or its
/// centre on the nearest point within reach, its centre alone on the paper's `centre`; else, each
/// axis apart, one of its sides or its middle aligned with the nearest point within reach, its
/// middle alone with `centre`; else its low side on the grid (step `grid`, 0: none). `free`: the
/// axes it can move along. The offset, and the marks to draw.
#[allow(clippy::too_many_arguments)]
pub(super) fn aim_box(
    on: &[DVec2],
    align: &[DVec2],
    centre: Option<DVec2>,
    lo: DVec2,
    hi: DVec2,
    offset: DVec2,
    reach: f64,
    grid: f64,
    free: (bool, bool),
) -> (DVec2, Aim) {
    let (lo, hi) = (lo + offset, hi + offset);
    let middle = (lo + hi) / 2.0;
    if free == (true, true) {
        let refs = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y), middle];
        let best = centre
            .map(|q| (middle, q, middle.distance(q)))
            .into_iter()
            .chain(refs.iter().flat_map(|r| on.iter().map(move |q| (*r, *q, r.distance(*q)))))
            .filter(|(_, _, dist)| *dist <= reach)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((r, q, _)) = best {
            return (offset + q - r, Aim { at: q, mark: q, point: true, ..Aim::default() });
        }
    }
    // The point of `align` nearest one of `refs` along `of`, then nearest the middle across it.
    let nearest = |refs: [f64; 3], of: fn(DVec2) -> f64, across: fn(DVec2) -> f64| {
        centre
            .map(|q| (refs[1], q, (refs[1] - of(q)).abs()))
            .into_iter()
            .chain(refs.iter().flat_map(|r| align.iter().map(move |q| (*r, *q, (r - of(*q)).abs()))))
            .filter(|(_, _, dist)| *dist <= reach)
            .min_by(|a, b| a.2.total_cmp(&b.2).then((across(a.1) - across(middle)).abs().total_cmp(&(across(b.1) - across(middle)).abs())))
    };
    let on_grid = |v: f64| if grid > 0.0 { (v / grid).round() * grid - v } else { 0.0 };
    let mut out = offset;
    let mut aim = Aim::default();
    let mut mark = middle;
    if free.0 {
        if let Some((r, q, _)) = nearest([lo.x, middle.x, hi.x], |v| v.x, |v| v.y) {
            out.x += q.x - r;
            mark.x = q.x;
            aim.along_y = Some(q);
        } else {
            let step = on_grid(lo.x);
            out.x += step;
            mark.x += step;
        }
    }
    if free.1 {
        if let Some((r, q, _)) = nearest([lo.y, middle.y, hi.y], |v| v.y, |v| v.x) {
            out.y += q.y - r;
            mark.y = q.y;
            aim.along_x = Some(q);
        } else {
            let step = on_grid(lo.y);
            out.y += step;
            mark.y += step;
        }
    }
    aim.at = mark;
    aim.mark = mark;
    (out, aim)
}

/// A text's height handle at `q` (axes of the level `place` puts on the sheet), its top on the
/// sheet's grid (step `grid`, 0: none). A text is never turned.
pub(super) fn top_on_grid(q: DVec2, place: DAffine2, grid: f64) -> DVec2 {
    if grid <= 0.0 {
        return q;
    }
    let y = place.transform_point2(q).y;
    DVec2::new(q.x, q.y + (y / grid).round() * grid - y)
}

/// How near a point the pointer snaps on it (points).
pub(super) const SNAP_PT: f32 = 8.0;

/// An aim's marks: the point snapped on circled, the alignments dotted — along the sheet's x
/// red, along its y green, as the 3D view's axes.
pub(super) fn draw_aim(painter: &egui::Painter, pv: &Paperview, aim: &Aim) {
    let at = pv.to_screen(aim.mark);
    if aim.point {
        painter.circle_stroke(at, 5.0, egui::Stroke::new(2.0, viewport::color(Inks::LIGHT.pick)));
    }
    for (q, ink) in [(aim.along_x, Inks::LIGHT.axis_x), (aim.along_y, Inks::LIGHT.axis_y)] {
        if let Some(q) = q {
            overlay::dashed(painter, &[pv.to_screen(q), at], egui::Stroke::new(1.0, viewport::color(ink)), overlay::ALIGNMENT);
        }
    }
}

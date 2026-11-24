//! Dimensions on a sheet: picked on a view, placed, drawn.

use super::*;

/// Dimension `dim` as drawn now, its view's box computed afresh (the PDF's).
pub fn dim_shape(e: &Editor, dim: &Dimension, projection: skraft_model::Projection) -> Option<crate::drawing::dims::DimShape> {
    let d = e.document().drawing();
    let r = d.resolve_as(dim.view, projection).filter(|r| r.perspective.is_none())?;
    let (lo, hi) = subject_box3(e, &r)?;
    Some(crate::drawing::dims::shape(e.document(), &r, ((lo + hi) / 2.0).truncate(), dim))
}

/// How a view maps its model onto the sheet: its resolution and its box's centre.
pub(super) fn view_map<'a>(
    e: &Editor,
    d: &'a Drawing,
    views: &mut ViewsCache,
    view: Id,
    projection: Projection,
) -> Option<(Resolved<'a>, DVec2)> {
    let r = d.resolve_as(view, projection)?;
    let centre = views.get(e, view, &r).centre()?;
    Some((r, centre))
}

/// A point of a view's subject on the sheet.
pub(super) fn on_sheet(r: &Resolved<'_>, centre: DVec2, p: DVec3) -> DVec2 {
    r.at + (r.frame.project(p).truncate() - centre) * r.scale
}

/// The distance from `p` to the polyline `line`, and the point of it there (index of its
/// segment, parameter along it).
fn near_polyline(line: &[DVec2], p: DVec2) -> (f64, usize, f64) {
    let mut best = (f64::MAX, 0, 0.0);
    for (i, w) in line.windows(2).enumerate() {
        let (t, d) = skraft_tools::segment::nearest(p, w[0], w[1]);
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
pub(super) fn pick_at(
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
        let Some(r) = d.resolve_as(id, projection).filter(|r| r.perspective.is_none()) else { continue };
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
pub(super) fn linear_place(sa: DVec2, sb: DVec2, p: DVec2) -> (Orient, f64) {
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
pub(super) fn dimension_of(
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
pub(super) fn picks_needed(tool: SheetTool) -> usize {
    match tool {
        SheetTool::Radius | SheetTool::Diameter => 1,
        _ => 2,
    }
}

/// A dimension's shape drawn on the sheet: its lines, arrows and text (red when an orphan);
/// its box on screen.
pub(super) fn draw_dim(painter: &egui::Painter, pv: &Paperview, s: &crate::drawing::dims::DimShape) -> egui::Rect {
    let color = if s.orphan { PAPER_ERROR } else { egui::Color32::from_gray(25) };
    let stroke = egui::Stroke::new((0.18 * pv.zoom).max(1.0) as f32, color);
    for [a, b] in &s.lines {
        painter.line_segment([pv.to_screen(*a), pv.to_screen(*b)], stroke);
    }
    for tri in &s.arrows {
        painter.add(egui::Shape::convex_polygon(tri.iter().map(|q| pv.to_screen(*q)).collect(), color, egui::Stroke::NONE));
    }
    let font = egui::FontId::proportional((crate::drawing::dims::TEXT_MM * pv.zoom).max(1.0) as f32);
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

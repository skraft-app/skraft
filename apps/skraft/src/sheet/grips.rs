//! The handles of what is picked on a sheet, and what dragging or typing on them does.

use super::*;

/// How near a handle the pointer takes it (points), and its square's side.
pub(super) const GRIP_PT: f32 = 6.0;

pub(super) const GRIP_SIDE: f32 = 7.0;

/// A handle of the element picked alone (docs/ui/drawing.md#handles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Handle {
    /// A box's (rectangle, image) corner or side middle: the sides it moves along x and y
    /// (-1 the left or bottom one, 1 the right or top one, 0 none).
    Side(i8, i8),
    /// A line's start (0) or end (1).
    End(u8),
    /// A circle's point on its right, left, top or bottom (its direction from the centre): its
    /// radius.
    Round(i8, i8),
    /// A base view frame's corner: its scale.
    Scale(i8, i8),
    /// A text's top right: its height.
    Size,
    /// A dimension's line (linear) or text (radius, diameter) or arc (angle).
    Dim,
}

impl Handle {
    /// The pointer over it.
    pub(super) fn cursor(self) -> egui::CursorIcon {
        use egui::CursorIcon as C;
        match self {
            Handle::Side(0, _) | Handle::Round(0, _) => C::ResizeVertical,
            Handle::Side(_, 0) | Handle::Round(..) => C::ResizeHorizontal,
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
pub(super) struct Grip {
    pub(super) element: Id,
    pub(super) handle: Handle,
    pub(super) start: DVec2,
    pub(super) from: DVec2,
    pub(super) kind: ElementKind,
    pub(super) lo: DVec2,
    pub(super) hi: DVec2,
}

/// The handles of element `el` (`frame`: its box on the sheet as last drawn), on the sheet.
pub(super) fn handles(
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
        ElementKind::Circle { center, radius } => [(1i8, 0i8), (0, 1), (-1, 0), (0, -1)]
            .into_iter()
            .map(|(x, y)| (Handle::Round(x, y), *center + DVec2::new(x as f64, y as f64) * *radius))
            .collect(),
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
pub(super) fn resized(min: DVec2, max: DVec2, sx: i8, sy: i8, p: DVec2, keep: bool) -> (DVec2, DVec2) {
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
pub(crate) fn nearest_scale(s: f64) -> f64 {
    let s = s.max(1e-9);
    SCALES.iter().copied().min_by(|a, b| (a.ln() - s.ln()).abs().total_cmp(&(b.ln() - s.ln()).abs())).unwrap_or(1.0)
}

/// The least side a perspective view's frame is cropped to (paper millimetres).
const MIN_VIEW_MM: f64 = 5.0;

/// The least radius a circle's handle leaves it (paper millimetres).
const MIN_RADIUS: f64 = 0.01;

/// The element of grip `g` with its handle at `p` (sheet millimetres); `shift` keeps a box's
/// proportions.
pub(super) fn regrip(g: &Grip, p: DVec2, shift: bool) -> ElementKind {
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
        (ElementKind::Circle { center, .. }, Handle::Round(..)) => {
            ElementKind::Circle { center: *center, radius: center.distance(p).max(MIN_RADIUS) }
        }
        // With Shift, a parallel view's scale: as far out as the pointer is from its middle, the
        // nearest standard one.
        (ElementKind::View(v), Handle::Scale(sx, sy)) => match &v.kind {
            ViewKind::Base { subject, look, scale, at } if shift && look.frame().1.is_none() => {
                let (half, middle) = (((g.hi - g.lo) / 2.0).max(DVec2::splat(1e-9)), (g.lo + g.hi) / 2.0);
                let k = ((p.x - middle.x).abs() / half.x).max((p.y - middle.y).abs() / half.y);
                let new = nearest_scale(scale * k);
                let kind = ViewKind::Base { subject: subject.clone(), look: look.clone(), scale: new, at: *at };
                ElementKind::View(View { kind, crop: v.crop.map(|c| c.scaled(new / scale)), ..v.clone() })
            }
            // Its frame: this corner moved, the opposite one and the picture still (decision 0052).
            ViewKind::Base { at, .. } => {
                let (lo, hi) = resized(g.lo, g.hi, sx, sy, p, false);
                let hi = hi.max(lo + DVec2::splat(MIN_VIEW_MM));
                ElementKind::View(View { crop: Some(skraft_model::Crop { lo: lo - *at, hi: hi - *at }), ..v.clone() })
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
pub(super) type Picked = (skraft_model::Element, (DVec2, DVec2), Vec<(Handle, DVec2)>);

/// The element picked alone, its box as last drawn (sheet millimetres) and its handles —
/// none on a locked element (not in `frames`).
pub(super) fn picked_handles(e: &Editor, st: &mut SheetState) -> Option<Picked> {
    let el = selected_element(e, st)?.clone();
    st.frames.iter().find(|(id, _)| *id == el.id)?;
    let frame = *st.boxes.get(&el.id)?;
    let d = e.document().drawing();
    let projection = st.projection(d);
    let hs = handles(e, d, &mut st.views, &el, Some(frame), projection);
    Some((el, frame, hs))
}

/// The handle of the element picked alone under `p` (screen points), the nearest.
pub(super) fn handle_at(e: &Editor, st: &mut SheetState, pv: &Paperview, p: egui::Pos2) -> Option<Grip> {
    let (el, (lo, hi), hs) = picked_handles(e, st)?;
    let (handle, start, _) = hs
        .into_iter()
        .map(|(h, q)| (h, q, pv.to_screen(q).distance(p)))
        .filter(|(_, _, dist)| *dist <= GRIP_PT)
        .min_by(|a, b| a.2.total_cmp(&b.2))?;
    Some(Grip { element: el.id, handle, start, from: pv.to_sheet(p), kind: el.kind, lo, hi })
}

impl Grip {
    /// A text's height handle with its top where the font's proportions put it: drawn small, a
    /// text's box is rounded to the pixel, and scaling from it would miss the grid.
    pub(super) fn with_true_text_top(mut self, painter: &egui::Painter) -> Self {
        if let (ElementKind::Text { at, text, size }, Handle::Size) = (&self.kind, self.handle) {
            const LARGE: f32 = 100.0;
            let galley = painter.layout_no_wrap(text.clone(), egui::FontId::proportional(LARGE), egui::Color32::BLACK);
            let top = at.y + size * (galley.size().y / LARGE) as f64;
            (self.start.y, self.hi.y) = (top, top);
        }
        self
    }

    /// Where its handle goes for the pointer at `p` (sheet millimetres): as far from where it
    /// was as the pointer from where it took it.
    pub(super) fn target(&self, p: DVec2) -> DVec2 {
        self.start + (p - self.from)
    }

    /// Does its handle snap (a box's side or corner, a line's end)? The point the line's other
    /// end is, to align with.
    pub(super) fn snaps(&self) -> Option<Option<DVec2>> {
        match (&self.kind, self.handle) {
            (_, Handle::Side(..)) => Some(None),
            (ElementKind::Line { to, .. }, Handle::End(0)) => Some(Some(*to)),
            (ElementKind::Line { from, .. }, Handle::End(_)) => Some(Some(*from)),
            (ElementKind::Circle { center, .. }, Handle::Round(..)) => Some(Some(*center)),
            _ => None,
        }
    }
}

/// What the measurements box takes for grip `g` (docs/ui/drawing.md#measurements): lengths
/// (paper millimetres, shown and typed in the document's unit) or a view's scale.
pub(super) enum Measure {
    Lengths(Vcb),
    Scale(f64),
}

/// A box's corner its handle (`sx`, `sy`) does not move: where a typed size starts.
pub(super) fn opposite(min: DVec2, max: DVec2, sx: i8, sy: i8) -> DVec2 {
    DVec2::new(if sx < 0 { max.x } else { min.x }, if sy < 0 { max.y } else { min.y })
}

/// The handle's other end for a line's end `k`, and its own.
fn line_ends(from: DVec2, to: DVec2, k: u8) -> (DVec2, DVec2) {
    if k == 0 { (to, from) } else { (from, to) }
}

/// What the measurements box measures for grip `g` (its element as `g.kind` is), signed as the
/// gesture goes.
pub(super) fn measure_of(g: &Grip) -> Option<Measure> {
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
        (ElementKind::Circle { radius, .. }, Handle::Round(..)) => lengths(t!("drawing.vcb.radius"), vec![*radius]),
        (ElementKind::View(View { kind: ViewKind::Base { scale, look, .. }, .. }), Handle::Scale(..)) if look.frame().1.is_none() => {
            Some(Measure::Scale(*scale))
        }
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
pub(super) fn parse_scale(text: &str) -> Option<f64> {
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
pub(super) fn typed_kind(g: &Grip, text: &str, units: &skraft_model::Units) -> Result<ElementKind, String> {
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
        (ElementKind::Circle { center, .. }, Handle::Round(..), Some(vcb)) => {
            let radius = lengths(vcb)?[0].abs();
            if radius <= 0.0 {
                return Err(refused());
            }
            Ok(ElementKind::Circle { center: *center, radius })
        }
        (ElementKind::View(v), Handle::Scale(..), None) => match &v.kind {
            ViewKind::Base { subject, look, at, scale: old } => {
                let scale = parse_scale(text).ok_or_else(refused)?;
                let kind = ViewKind::Base { subject: subject.clone(), look: look.clone(), scale, at: *at };
                Ok(ElementKind::View(View { kind, crop: v.crop.map(|c| c.scaled(scale / old)), ..v.clone() }))
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

/// The shape being drawn from `from` to `to` (a rectangle, a circle from its centre, a line), as
/// a grip on its moving corner, edge or end — what the measurements box measures and a value typed makes.
pub(super) fn sketch_grip(tool: SheetTool, from: DVec2, to: DVec2) -> Grip {
    let sign = |v: f64| if v < 0.0 { -1i8 } else { 1 };
    let (kind, handle) = match tool {
        SheetTool::Rect => {
            (ElementKind::Rect { min: from.min(to), max: from.max(to) }, Handle::Side(sign(to.x - from.x), sign(to.y - from.y)))
        }
        SheetTool::Circle => (ElementKind::Circle { center: from, radius: from.distance(to) }, Handle::Round(1, 0)),
        _ => (ElementKind::Line { from, to }, Handle::End(1)),
    };
    Grip { element: Id(0), handle, start: to, from: to, kind, lo: from.min(to), hi: from.max(to) }
}

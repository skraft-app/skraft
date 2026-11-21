//! A drawing's dimensions as drawn (docs/ui/drawing.md#dimensions): from what the model is
//! now, the segments, arrows and text a dimension shows on its sheet — the same for the
//! sheet on screen and for the PDF.

use glam::{DVec2, DVec3};
use skraft_model::kernel::topo::EdgeShape;
use skraft_model::{Anchor, DimKind, Dimension, Document, Found, Orient, Resolved};

/// Sizes on paper (millimetres): the text's height, an arrow's length and half width, the
/// gap between a point and its extension line, how far an extension line passes the
/// dimension line.
pub const TEXT_MM: f64 = 3.5;
const ARROW_MM: f64 = 2.5;
const ARROW_HALF_MM: f64 = 0.6;
const GAP_MM: f64 = 1.0;
const PAST_MM: f64 = 1.5;

/// A dimension as drawn, in sheet millimetres.
#[derive(Clone, Debug, Default)]
pub struct DimShape {
    pub lines: Vec<[DVec2; 2]>,
    /// Filled triangles, the tip first.
    pub arrows: Vec<[DVec2; 3]>,
    pub text: String,
    /// The text's middle, its baseline's direction (radians, counterclockwise).
    pub text_at: DVec2,
    pub text_angle: f64,
    /// An anchor not found: drawn where it was, in the orphans' colour.
    pub orphan: bool,
    /// The value measured (model millimetres, or degrees), none for an orphan.
    pub value: Option<f64>,
    /// Where its handle is (docs/ui/drawing.md#handles): a linear one's line middle, a
    /// radius's text point, an angle's arc middle.
    pub grip: DVec2,
}

impl DimShape {
    /// Its extent on the sheet.
    pub fn bounds(&self) -> (DVec2, DVec2) {
        let pts = self.lines.iter().flatten().chain(self.arrows.iter().flatten()).chain(std::iter::once(&self.text_at));
        pts.fold((DVec2::MAX, DVec2::MIN), |(l, h), p| (l.min(*p), h.max(*p)))
    }
}

/// An arrow with its tip at `tip`, pointing along `dir`.
fn arrow(tip: DVec2, dir: DVec2) -> [DVec2; 3] {
    let d = dir.normalize_or_zero();
    let n = d.perp();
    let base = tip - d * ARROW_MM;
    [tip, base + n * ARROW_HALF_MM, base - n * ARROW_HALF_MM]
}

/// An angle made readable: its text never upside down.
fn readable(a: f64) -> f64 {
    let mut a = a;
    while a > std::f64::consts::FRAC_PI_2 + 1e-9 {
        a -= std::f64::consts::PI;
    }
    while a < -std::f64::consts::FRAC_PI_2 + 1e-9 {
        a += std::f64::consts::PI;
    }
    a
}

/// Dimension `dim` through view `r` (its subject's box centred at `centre`, in its frame), as
/// the model is now.
pub fn shape(doc: &Document, r: &Resolved<'_>, centre: DVec2, dim: &Dimension) -> DimShape {
    let units = &doc.settings().units;
    let f = &r.frame;
    let on_sheet = |p: DVec3| r.at + (f.project(p).truncate() - centre) * r.scale;
    // An anchor found, or where it was picked (an orphan).
    let mut orphan = false;
    let mut find = |a: &Anchor| match doc.find(r.subject, a) {
        Some(found) => Some(found),
        None => {
            orphan = true;
            None
        }
    };
    let mut s = DimShape::default();
    match &dim.kind {
        DimKind::Linear { a, b, orient, offset } => {
            let (pa, pb) = (find(a).map(|x| x.point()), find(b).map(|x| x.point()));
            let (pa3, pb3) = (pa.unwrap_or(a.at), pb.unwrap_or(b.at));
            let (sa, sb) = (on_sheet(pa3), on_sheet(pb3));
            let d3 = pb3 - pa3;
            let mid = (sa + sb) / 2.0;
            // Each point's foot on the dimension line, the line's direction, the value.
            let (fa, fb, along, value) = match orient {
                Orient::Horizontal => {
                    let y = mid.y + offset;
                    (DVec2::new(sa.x, y), DVec2::new(sb.x, y), DVec2::X, d3.dot(f.right).abs())
                }
                Orient::Vertical => {
                    let x = mid.x + offset;
                    (DVec2::new(x, sa.y), DVec2::new(x, sb.y), DVec2::Y, d3.dot(f.up).abs())
                }
                Orient::Aligned => {
                    let t = (sb - sa).normalize_or(DVec2::X);
                    let n = t.perp();
                    (sa + n * *offset, sb + n * *offset, t, DVec2::new(d3.dot(f.right), d3.dot(f.up)).length())
                }
            };
            // Extension lines from a little off each point to a little past the line.
            for (p, foot) in [(sa, fa), (sb, fb)] {
                let dir = (foot - p).normalize_or_zero();
                if (foot - p).length() > GAP_MM {
                    s.lines.push([p + dir * GAP_MM, foot + dir * PAST_MM]);
                }
            }
            s.lines.push([fa, fb]);
            s.grip = (fa + fb) / 2.0;
            let out = (fb - fa).normalize_or(along);
            s.arrows.push(arrow(fa, -out));
            s.arrows.push(arrow(fb, out));
            let angle = readable(out.y.atan2(out.x));
            let normal = DVec2::from_angle(angle).perp();
            s.text_at = (fa + fb) / 2.0 + normal * (TEXT_MM * 0.8);
            s.text_angle = angle;
            s.value = Some(value);
            s.text = units.number(value);
        }
        DimKind::Radius { edge, diameter, at } => {
            let circle = find(edge).and_then(|found| match found {
                Found::Edge(EdgeShape::Circle { centre, radius, .. }) => Some((centre, radius)),
                _ => None,
            });
            let (c3, radius) = circle.unwrap_or((edge.at, 0.0));
            if circle.is_none() {
                orphan = true;
            }
            let c = on_sheet(c3);
            let rs = radius * r.scale;
            let dir = at.normalize_or(DVec2::X);
            let text_at = c + *at;
            s.grip = text_at;
            let rim = c + dir * rs;
            if *diameter {
                let back = c - dir * rs;
                let end = if at.length() > rs { text_at } else { rim };
                s.lines.push([back, end]);
                s.arrows.push(arrow(rim, dir));
                s.arrows.push(arrow(back, -dir));
            } else {
                let end = if at.length() > rs { text_at } else { rim };
                s.lines.push([c, end]);
                s.arrows.push(arrow(rim, dir));
            }
            let value = if *diameter { 2.0 * radius } else { radius };
            s.value = circle.map(|_| value);
            let mark = if *diameter { "Ø" } else { "R" };
            s.text = format!("{mark} {}", units.number(value));
            s.text_at = text_at + dir.perp() * (TEXT_MM * 0.8);
            s.text_angle = readable(dir.y.atan2(dir.x));
        }
        DimKind::Angle { a, b, at } => {
            let line = |found: Option<Found>, anchor: &Anchor| match found {
                Some(Found::Edge(EdgeShape::Line { a: p, b: q })) => Some((on_sheet(p), on_sheet(q))),
                _ => {
                    let _ = anchor;
                    None
                }
            };
            let (fa, fb) = (find(a), find(b));
            let (la, lb) = (line(fa, a), line(fb, b));
            let (Some((p1, q1)), Some((p2, q2))) = (la, lb) else {
                s.text = "?".into();
                let p = on_sheet(a.at);
                s.text_at = p + *at;
                s.grip = s.text_at;
                s.orphan = true;
                return s;
            };
            // Where the lines meet (or the first's start when parallel).
            let (d1, d2) = ((q1 - p1).normalize_or(DVec2::X), (q2 - p2).normalize_or(DVec2::Y));
            let den = d1.perp_dot(d2);
            let o = if den.abs() < 1e-9 { p1 } else { p1 + d1 * ((p2 - p1).perp_dot(d2) / den) };
            // Each ray towards the side the arc is on.
            let want = at.normalize_or(DVec2::X);
            let r1 = if d1.dot(want) >= 0.0 { d1 } else { -d1 };
            let r2 = if d2.dot(want) >= 0.0 { d2 } else { -d2 };
            let (a1, mut a2) = (r1.y.atan2(r1.x), r2.y.atan2(r2.x));
            // The short way from the first to the second.
            while a2 - a1 > std::f64::consts::PI {
                a2 -= std::f64::consts::TAU;
            }
            while a2 - a1 < -std::f64::consts::PI {
                a2 += std::f64::consts::TAU;
            }
            let radius = at.length().max(ARROW_MM * 2.0);
            let n = 32;
            let pts: Vec<DVec2> = (0..=n).map(|i| o + DVec2::from_angle(a1 + (a2 - a1) * i as f64 / n as f64) * radius).collect();
            for w in pts.windows(2) {
                s.lines.push([w[0], w[1]]);
            }
            let turn = (a2 - a1).signum();
            s.arrows.push(arrow(pts[0], DVec2::from_angle(a1).perp() * -turn));
            s.arrows.push(arrow(pts[n], DVec2::from_angle(a2).perp() * turn));
            // The extension of each edge out to the arc.
            for (p, q, ray) in [(p1, q1, r1), (p2, q2, r2)] {
                let far = if (p - o).dot(ray) > (q - o).dot(ray) { p } else { q };
                if (far - o).length() < radius {
                    s.lines.push([far + ray * GAP_MM, o + ray * (radius + PAST_MM)]);
                }
            }
            let value = (a2 - a1).abs().to_degrees();
            s.value = Some(value);
            s.text = format!("{}°", skraft_i18n::number(&format!("{}", (value * 10.0).round() / 10.0)));
            let mid = (a1 + a2) / 2.0;
            s.grip = o + DVec2::from_angle(mid) * radius;
            s.text_at = o + DVec2::from_angle(mid) * (radius + TEXT_MM);
            s.text_angle = readable(mid + std::f64::consts::FRAC_PI_2);
        }
    }
    s.orphan = orphan;
    if orphan && !s.text.ends_with('?') {
        s.text.push_str(" ?");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_angles_are_never_upside_down() {
        assert!((readable(std::f64::consts::PI) - 0.0).abs() < 1e-9);
        assert!((readable(-std::f64::consts::FRAC_PI_2) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!((readable(0.3) - 0.3).abs() < 1e-12);
    }

    #[test]
    fn an_arrow_points_at_its_tip() {
        let [tip, a, b] = arrow(DVec2::ZERO, DVec2::X);
        assert_eq!(tip, DVec2::ZERO);
        assert!(a.x < 0.0 && b.x < 0.0 && (a.y + b.y).abs() < 1e-12);
    }
}

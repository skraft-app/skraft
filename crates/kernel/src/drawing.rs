//! A drawing view's exact lines of one part (docs/ui/drawing.md#lines, decision 0048):
//! SolidCore's hidden lines of placed bodies (`solidcore_ops::hlr`), the part alone in its
//! scene.

use glam::{DAffine3, DVec2, DVec3};
use solidcore_ops::hlr as h;
use solidcore_record::{Seg2, Vec3 as V};

use crate::Solid;

/// The part's lines in the view's frame (`right`, `up`, model units): visible and hidden
/// polylines; the hidden pieces lying under visible ones left out.
pub type PartLines = (Vec<Vec<DVec2>>, Vec<Vec<DVec2>>);

/// The lines of `solid` placed by `place` (part to the view's axes), seen along `forward`
/// with `right` and `up` the sheet's. A placement that is not rigid (a mirror) is applied to
/// the solid first.
///
/// # Errors
/// SolidCore's refusal (a body or a scene it does not draw, a budget), as text.
pub fn part_lines(solid: &Solid, place: &DAffine3, right: DVec3, up: DVec3, forward: DVec3) -> Result<PartLines, String> {
    let m = place.matrix3;
    let rigid = (m.transpose() * m - glam::DMat3::IDENTITY).to_cols_array().iter().all(|v| v.abs() < 1e-9) && m.determinant() > 0.0;
    let placed;
    let (body, place) = if rigid {
        (solid, *place)
    } else {
        placed = solid.transformed(place).map_err(|e| e.to_string())?;
        (&placed, DAffine3::IDENTITY)
    };
    let opts = h::Options { threads: 1, ..Default::default() };
    let def = h::prepare(&body.record, &opts).map_err(|e| format!("{e:?}"))?;
    let v = |p: DVec3| V { x: p.x, y: p.y, z: p.z };
    let view = h::View::new(v(forward), v(up)).map_err(|e| format!("{e:?}"))?;
    let back = |p: V| DVec3::new(p.x, p.y, p.z);
    let (vx, vy) = (back(view.x), back(view.y));
    let to_frame = |p: [f64; 2]| {
        let w = vx * p[0] + vy * p[1];
        DVec2::new(w.dot(right), w.dot(up))
    };
    let scene = [h::Placement {
        body: 0,
        rotation: [v(place.matrix3.x_axis), v(place.matrix3.y_axis), v(place.matrix3.z_axis)],
        translation: v(place.translation),
    }];
    let d = h::hidden_lines(&[def], &scene, &view, &opts, &|| false).map_err(|e| format!("{e:?}"))?;
    let (mut visible, mut hidden) = (Vec::new(), Vec::new());
    for line in &d.lines {
        for piece in line.pieces.iter().filter(|p| !p.covered) {
            let into = match piece.visibility {
                h::Visibility::Visible => &mut visible,
                // An uncertain piece drawn as hidden: never over what is seen.
                h::Visibility::Hidden | h::Visibility::Uncertain => &mut hidden,
            };
            let mut poly: Vec<DVec2> = Vec::new();
            for seg in &piece.geometry {
                for p in sample(seg) {
                    let q = to_frame(p);
                    if poly.last().is_none_or(|l: &DVec2| l.distance_squared(q) > 1e-18) {
                        poly.push(q);
                    }
                }
            }
            if poly.len() > 1 {
                into.push(poly);
            }
        }
    }
    Ok((visible, hidden))
}

/// Points along a segment of the drawing: its ends for a line, 24 pieces for a curve.
fn sample(s: &Seg2) -> Vec<[f64; 2]> {
    let p = |v: &solidcore_record::Vec2| [v.x, v.y];
    let n = 24;
    let lerp = |a: [f64; 2], b: [f64; 2], t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let casteljau = |ctrl: &[[f64; 2]], t: f64| {
        let mut c = ctrl.to_vec();
        while c.len() > 1 {
            c = c.windows(2).map(|w| lerp(w[0], w[1], t)).collect();
        }
        c[0]
    };
    match s {
        Seg2::Line { a, b } => vec![p(a), p(b)],
        Seg2::Arc { center, radius, start, sweep } => (0..=n)
            .map(|i| {
                let t = start + sweep * i as f64 / n as f64;
                [center.x + radius * t.cos(), center.y + radius * t.sin()]
            })
            .collect(),
        Seg2::Cubic { p0, p1, p2, p3 } => (0..=n).map(|i| casteljau(&[p(p0), p(p1), p(p2), p(p3)], i as f64 / n as f64)).collect(),
        Seg2::Conic { a, apex, b, w } => (0..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t);
                let s = b0 + b1 + b2;
                [(b0 * a.x + b1 * apex.x + b2 * b.x) / s, (b0 * a.y + b1 * apex.y + b2 * b.y) / s]
            })
            .collect(),
        Seg2::Bezier { n: deg, p: ctrl } => {
            let c: Vec<[f64; 2]> = ctrl[..=*deg as usize].iter().map(p).collect();
            (0..=n).map(|i| casteljau(&c, i as f64 / n as f64)).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Primitive;

    #[test]
    fn a_box_from_the_front_shows_its_outline() {
        let job = crate::Job {
            inputs: Vec::new(),
            tasks: vec![crate::Task::Primitive {
                primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 50.0, 20.0) },
                placement: DAffine3::IDENTITY,
            }],
        };
        let Some(crate::Output::Solid { solid, .. }) = crate::run(&job).unwrap().pop() else { panic!("no solid") };
        let (visible, hidden) = part_lines(&solid, &DAffine3::IDENTITY, DVec3::X, DVec3::Z, DVec3::Y).unwrap();
        let length = |ls: &[Vec<DVec2>]| ls.iter().flat_map(|l| l.windows(2)).map(|w| w[0].distance(w[1])).sum::<f64>();
        assert!((length(&visible) - 240.0).abs() < 1e-6, "{}", length(&visible));
        // The back face's edges lie under the front's: covered, not drawn.
        assert!(length(&hidden) < 1e-6, "{}", length(&hidden));
        let (lo, hi) = visible.iter().flatten().fold((DVec2::MAX, DVec2::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
        assert!((hi - lo - DVec2::new(100.0, 20.0)).length() < 1e-9);
    }
}

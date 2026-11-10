//! The measurements box (SketchUp's VCB): values typed while a tool runs.
//!
//! The tool says what it takes now ([`Vcb`]: lengths, an angle, a count of copies, and
//! whether a point does too) with its current values; the editor reads what is typed with
//! [`solidsketch_model::measure`] — expressions (`2m*(2+3)`, `min(@; 300)`), `@` for the current
//! value, `;` lists with `=` to repeat, points `[x;y;z]` and `<dx;dy;dz>` — and gives the tool a
//! [`VcbValue`]. A bare number is in the document's unit; `,` and `.` are both decimal points.

use glam::{DAffine3, DVec3};
use solidsketch_model::{Units, measure};

/// What the measurements box takes now.
#[derive(Clone, Debug, PartialEq)]
pub struct Vcb {
    /// Its label (« Distance »).
    pub label: &'static str,
    pub kind: VcbKind,
    /// A typed point is taken too: `[x;y;z]` in the context's axes, `<dx;dy;dz>` along them
    /// from `from`; `to` is the point the cursor gives (world).
    pub point: Option<PointRef>,
}

impl Vcb {
    pub fn new(label: &'static str, kind: VcbKind) -> Self {
        Vcb { label, kind, point: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum VcbKind {
    /// Lengths (mm), as many as given, at their current values, signed: a typed one keeps
    /// its sign (the gesture's direction). `@` is a value's current one.
    Lengths(Vec<f64>),
    /// An angle (radians), at its current value.
    Angle(f64),
    /// A number of copies (`5x`, `/5`).
    Count,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointRef {
    pub from: DVec3,
    pub to: DVec3,
}

/// What was typed, read.
#[derive(Clone, Debug, PartialEq)]
pub enum VcbValue {
    /// One per current value, signed.
    Lengths(Vec<f64>),
    /// Radians.
    Angle(f64),
    Count(Repeat),
    /// World.
    Point(DVec3),
}

/// Reads `text` for `vcb`; `context` is the context's frame (to world), for points. The error
/// is the status bar's message.
pub fn read(vcb: &Vcb, text: &str, units: &Units, context: &DAffine3) -> Result<VcbValue, String> {
    let refused = |e: &dyn std::fmt::Display| format!("{} — « {} » : {e}", vcb.label, text.trim());
    if let Some(r) = vcb.point {
        let inv = context.inverse();
        let (to, from) = (inv.transform_point3(r.to), inv.transform_point3(r.from));
        if let Some(p) = measure::point(text, units, to, from) {
            return p.map(|p| VcbValue::Point(context.transform_point3(p))).map_err(|e| refused(&e));
        }
    }
    match &vcb.kind {
        VcbKind::Lengths(current) => measure::lengths(text, units, current).map(VcbValue::Lengths).map_err(|e| refused(&e)),
        VcbKind::Angle(current) => {
            measure::angle(text, current.to_degrees()).map(|a| VcbValue::Angle(a.to_radians())).map_err(|e| refused(&e))
        }
        VcbKind::Count => repeat(text).map(VcbValue::Count).ok_or_else(|| refused(&"taper 5x ou /5")),
    }
}

/// The value shown in the box while nothing is typed.
pub fn display(kind: &VcbKind, units: &Units) -> String {
    match kind {
        VcbKind::Lengths(v) => v.iter().map(|&x| format(x.abs(), units)).collect::<Vec<_>>().join("; "),
        VcbKind::Angle(a) => {
            let d = (a.to_degrees() * 10.0).round() / 10.0;
            let d = if d == 0.0 { 0.0 } else { d };
            format!("{d}°").replace('.', ",")
        }
        VcbKind::Count => String::new(),
    }
}

/// A count typed after a copy (SketchUp): `5x`, `x5`, `*5`, `5*` repeat it 5 times; `/5`
/// or `5/` spread 5 copies over its way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Times(u32),
    Divide(u32),
}

pub fn repeat(s: &str) -> Option<Repeat> {
    let t: String = s.trim().to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    let n = |x: &str| x.parse::<u32>().ok().filter(|n| (1..=solidsketch_model::MAX_ARRAY).contains(n));
    let strip = |marks: &[char]| marks.iter().find_map(|m| t.strip_suffix(*m).or_else(|| t.strip_prefix(*m)));
    if let Some(x) = strip(&['x', '*']) {
        return n(x).map(Repeat::Times);
    }
    strip(&['/']).and_then(n).map(Repeat::Divide)
}

/// A length (mm) for display in `units`, without needless decimals.
pub fn format(v: f64, units: &Units) -> String {
    units.format(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    use solidsketch_model::LengthUnit;

    #[test]
    fn reads_for_the_tool() {
        let mm = Units::default();
        let id = DAffine3::IDENTITY;
        let base = Vcb::new("Dimensions", VcbKind::Lengths(vec![100.0, -50.0]));
        assert_eq!(read(&base, "600=", &mm, &id), Ok(VcbValue::Lengths(vec![600.0, -600.0])));
        assert_eq!(read(&base, ";@*2", &mm, &id), Ok(VcbValue::Lengths(vec![100.0, -100.0])));
        assert_eq!(read(&base, "2m*3m", &mm, &id), Err("Dimensions — « 2m*3m » : unités incompatibles".into()));
        let angle = Vcb::new("Angle", VcbKind::Angle(90f64.to_radians()));
        assert_eq!(read(&angle, "@/2", &mm, &id), Ok(VcbValue::Angle(45f64.to_radians())));
        let count = Vcb::new("Copies", VcbKind::Count);
        assert_eq!(read(&count, "5x", &mm, &id), Ok(VcbValue::Count(Repeat::Times(5))));
        assert!(read(&count, "5", &mm, &id).is_err());
    }

    #[test]
    fn points_are_in_the_context_s_axes() {
        let mm = Units::default();
        // A context shifted by (1000, 0, 0) and turned a quarter about Z.
        let context = DAffine3::from_translation(DVec3::new(1000.0, 0.0, 0.0)) * DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
        let from = context.transform_point3(DVec3::new(10.0, 0.0, 0.0));
        let to = context.transform_point3(DVec3::new(10.0, 40.0, 0.0));
        let mut v = Vcb::new("Distance", VcbKind::Lengths(vec![40.0]));
        v.point = Some(PointRef { from, to });
        let close = |r: Result<VcbValue, String>, p: DVec3| match r {
            Ok(VcbValue::Point(q)) => (q - context.transform_point3(p)).length() < 1e-9,
            _ => false,
        };
        assert!(close(read(&v, "[0;0;0]", &mm, &context), DVec3::ZERO));
        assert!(close(read(&v, "<5;0;0>", &mm, &context), DVec3::new(15.0, 0.0, 0.0)));
        assert!(close(read(&v, "<;@/2;>", &mm, &context), DVec3::new(10.0, 20.0, 0.0)));
        assert_eq!(read(&v, "@/2", &mm, &context), Ok(VcbValue::Lengths(vec![20.0])));
    }

    #[test]
    fn displays() {
        let mm = Units::default();
        assert_eq!(display(&VcbKind::Lengths(vec![12.5, -300.0]), &mm), "12,5 mm; 300 mm");
        assert_eq!(display(&VcbKind::Angle(-22.5f64.to_radians()), &mm), "-22,5°");
        let cm = Units { length: LengthUnit::Centimeter, ..Default::default() };
        assert_eq!(display(&VcbKind::Lengths(vec![250.0]), &cm), "25 cm");
    }

    #[test]
    fn repeats() {
        assert_eq!(repeat("5x"), Some(Repeat::Times(5)));
        assert_eq!(repeat(" X 3"), Some(Repeat::Times(3)));
        assert_eq!(repeat("*4"), Some(Repeat::Times(4)));
        assert_eq!(repeat("/6"), Some(Repeat::Divide(6)));
        assert_eq!(repeat("6/"), Some(Repeat::Divide(6)));
        assert_eq!(repeat("0x"), None);
        assert_eq!(repeat("250"), None);
    }
}

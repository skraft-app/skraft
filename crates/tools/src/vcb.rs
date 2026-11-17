//! The measurements box (SketchUp's VCB): values typed while a tool runs.
//!
//! The tool says what it takes now ([`Vcb`]: lengths, an angle, a count of copies, and
//! whether a point does too) with its current values; the editor reads what is typed with
//! [`solidsketch_model::measure`] — expressions (`2m*(2+3)`, `min(@; 300)`), `@` for the current
//! value, `;` lists with `=` to repeat, points `[x;y;z]` and `<dx;dy;dz>` — and gives the tool a
//! [`VcbValue`]. Every list typed (lengths, counts, spacings, a point's coordinates) is read by
//! [`measure::list`], values beyond those taken left out. A bare number is in the document's
//! unit; `,` and `.` are both decimal points, unless `,` is the system's list separator.

use glam::{DAffine3, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::{Units, measure};

/// What the measurements box takes now.
#[derive(Clone, Debug, PartialEq)]
pub struct Vcb {
    /// Its label (the move's « Distance »).
    pub label: String,
    pub kind: VcbKind,
    /// A typed point is taken too: `[x;y;z]` in the context's axes, `<dx;dy;dz>` along them
    /// from `from`; `to` is the point the cursor gives (world).
    pub point: Option<PointRef>,
    /// How many of the last lengths may be left out (the height typed with the base): they
    /// are given only if typed, and not shown.
    pub optional: usize,
    /// How many counts (`5x`, `/5`, separated by `;`) are taken too, instead of the lengths
    /// (the copy tools, §6 duodecies).
    pub counts: usize,
    /// A list of spacings is taken too (Distribute, §6 duodecies): two lengths or more, `;`
    /// between them, empty slots left unset.
    pub spacings: bool,
}

impl Vcb {
    pub fn new(label: impl Into<String>, kind: VcbKind) -> Self {
        Vcb { label: label.into(), kind, point: None, optional: 0, counts: 0, spacings: false }
    }

    /// The same, taking up to `n` counts too.
    pub fn counts(self, n: usize) -> Self {
        Vcb { counts: n, ..self }
    }

    /// The same, taking a list of spacings too.
    pub fn spacings(self) -> Self {
        Vcb { spacings: true, ..self }
    }

    /// The same, the last `n` lengths optional.
    pub fn optional(self, n: usize) -> Self {
        Vcb { optional: n, ..self }
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
    /// One per current value, signed; optional ones only if typed.
    Lengths(Vec<f64>),
    /// Radians.
    Angle(f64),
    Count(Repeat),
    /// Counts typed for a tool taking them with lengths ([`Vcb::counts`]): one per slot, `None`
    /// where left empty.
    Counts(Vec<Option<Repeat>>),
    /// Spacings typed for a tool taking them ([`Vcb::spacings`]), in order, `None` where left
    /// empty; lengths, never negative.
    Spacings(Vec<Option<f64>>),
    /// World.
    Point(DVec3),
}

/// Reads `text` for `vcb`; `context` is the context's frame (to world), for points. The error
/// is the status bar's message.
pub fn read(vcb: &Vcb, text: &str, units: &Units, context: &DAffine3) -> Result<VcbValue, String> {
    let refused = |e: &dyn std::fmt::Display| t!("tool.vcb.refused", label = vcb.label, text = text.trim(), error = e);
    if let Some(c) = counts(text, vcb.counts) {
        return Ok(VcbValue::Counts(c));
    }
    if let Some(items) = spacings(vcb, text) {
        let read = |i: &measure::Item| (!i.value.is_empty()).then(|| measure::length(&i.value, units, 0.0).map(f64::abs)).transpose();
        return items.iter().map(read).collect::<Result<Vec<_>, _>>().map(VcbValue::Spacings).map_err(|e| refused(&e));
    }
    if let Some(r) = vcb.point {
        let inv = context.inverse();
        let (to, from) = (inv.transform_point3(r.to), inv.transform_point3(r.from));
        if let Some(p) = measure::point(text, units, to, from) {
            return p.map(|p| VcbValue::Point(context.transform_point3(p))).map_err(|e| refused(&e));
        }
    }
    match &vcb.kind {
        VcbKind::Lengths(current) => {
            let mut v = measure::lengths(text, units, current).map_err(|e| refused(&e))?;
            let typed = measure::list(text).len();
            v.truncate(typed.max(current.len().saturating_sub(vcb.optional)));
            Ok(VcbValue::Lengths(v))
        }
        VcbKind::Angle(current) => {
            measure::angle(text, current.to_degrees()).map(|a| VcbValue::Angle(a.to_radians())).map_err(|e| refused(&e))
        }
        VcbKind::Count => repeat(text).map(VcbValue::Count).ok_or_else(|| refused(&t!("tool.vcb.count_expected"))),
    }
}

/// The value shown in the box while nothing is typed (without the optional lengths).
pub fn display(vcb: &Vcb, units: &Units) -> String {
    match &vcb.kind {
        VcbKind::Lengths(v) => {
            let shown = &v[..v.len().saturating_sub(vcb.optional)];
            measure::join(shown.iter().map(|&x| format(x.abs(), units)))
        }
        VcbKind::Angle(a) => format_angle(*a),
        VcbKind::Count => String::new(),
    }
}

/// What the length written by a value (its bubble in the preview) shows of a text typed in
/// the box: the item typed for it, and what it reads as when that says more (`2m*3`, then
/// `= 6 000 mm` greyed; not for `600`).
#[derive(Clone, Debug, PartialEq)]
pub struct Typed {
    /// The item typed (empty for a value repeated by `=`).
    pub text: String,
    /// What it reads as, when that says more (shown after it as `= <result>`).
    pub result: Option<String>,
    /// The value being typed (the last item): the caret is there.
    pub active: bool,
}

/// What `text`, typed in the box, gives each of `vcb`'s values to show (by index, `None`
/// where it keeps its current value): the item typed for it (a `;` moves on to the next), with
/// what it reads as; a value repeated by `=` shows `= <result>`. An item that does not read
/// (yet) shows as typed. A point or a count typed is for the box only: nothing.
pub fn typed(vcb: &Vcb, text: &str, units: &Units) -> Vec<Option<Typed>> {
    let t = text.trim();
    let point = t.starts_with('[') || t.starts_with('<') || spacings(vcb, text).is_some() || counts(text, vcb.counts).is_some();
    match &vcb.kind {
        VcbKind::Lengths(current) if !point => {
            let items = measure::list(text);
            let last = items.last().map_or(0, |i| i.typed);
            let typed = |(slot, item): (usize, &measure::Item)| {
                let value = item.value.as_str();
                let read = (!value.is_empty()).then(|| measure::length(value, units, current[slot]).ok()).flatten();
                let read = read.map(|v| format(v.abs(), units));
                if item.repeat {
                    read.map(|r| Typed { text: String::new(), result: Some(r), active: false })
                } else {
                    let active = item.typed == last;
                    (!value.is_empty() || active).then(|| Typed {
                        text: value.to_owned(),
                        result: read.filter(|r| says_more(value, r)),
                        active,
                    })
                }
            };
            let mut out: Vec<_> = items.iter().take(current.len()).enumerate().map(typed).collect();
            out.resize(current.len(), None);
            out
        }
        VcbKind::Angle(current) if !point => {
            let result = measure::angle(t, current.to_degrees()).ok().map(|a| format_angle(a.to_radians()));
            let result = result.filter(|r| says_more(t.trim_end_matches('°').trim_end(), r));
            vec![Some(Typed { text: t.to_owned(), result, active: true })]
        }
        VcbKind::Lengths(current) => vec![None; current.len()],
        _ => Vec::new(),
    }
}

/// Does the result `shown` say more than the item `typed`: not when that is a bare number, or
/// the result itself (spaces aside).
fn says_more(typed: &str, shown: &str) -> bool {
    let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    typed.replace(',', ".").parse::<f64>().is_err() && bare(typed) != bare(shown)
}

/// A count typed after a copy (SketchUp): `5x`, `x5`, `*5`, `5*` repeat it 5 times; `/5`
/// or `5/` spread 5 copies over its way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Times(u32),
    Divide(u32),
}

/// `n` counts, a list ([`measure::list`]: `5x=` is `5x;5x`; an empty slot: `None`, those
/// beyond `n` left out), at least one typed, all those taken reading; none for `n` zero.
pub fn counts(text: &str, n: usize) -> Option<Vec<Option<Repeat>>> {
    let items = measure::list(text);
    let items = &items[..items.len().min(n)];
    if items.iter().all(|i| i.value.is_empty()) {
        return None;
    }
    let mut out =
        items.iter().map(|i| if i.value.is_empty() { Some(None) } else { repeat(&i.value).map(Some) }).collect::<Option<Vec<_>>>()?;
    out.resize(n, None);
    Some(out)
}

/// The spacings typed for a tool taking them ([`Vcb::spacings`]): a list of two values or
/// more (`10=` is two).
fn spacings(vcb: &Vcb, text: &str) -> Option<Vec<measure::Item>> {
    vcb.spacings.then(|| measure::list(text)).filter(|items| items.len() > 1)
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

/// An angle (radians), in degrees to a tenth: `-22,5°`, `~ 22,5°` when rounded.
pub fn format_angle(a: f64) -> String {
    let d = (a.to_degrees() * 10.0).round() / 10.0;
    // Below 1e-4° (under 2 µm at a metre, as precise as the lengths), float noise.
    let approx = if (d - a.to_degrees()).abs() > 1e-4 { solidsketch_model::APPROX } else { "" };
    let d = if d == 0.0 { 0.0 } else { d };
    solidsketch_i18n::number(&format!("{approx}{d}°"))
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
        // An optional height: given only if typed (`600==`: a cube).
        let with_height = Vcb::new("Dimensions", VcbKind::Lengths(vec![100.0, -50.0, 0.0])).optional(1);
        assert_eq!(read(&with_height, "600;400", &mm, &id), Ok(VcbValue::Lengths(vec![600.0, -400.0])));
        assert_eq!(read(&with_height, "600", &mm, &id), Ok(VcbValue::Lengths(vec![600.0, -50.0])));
        assert_eq!(read(&with_height, "600==", &mm, &id), Ok(VcbValue::Lengths(vec![600.0, -600.0, 600.0])));
        assert_eq!(read(&with_height, ";;30", &mm, &id), Ok(VcbValue::Lengths(vec![100.0, -50.0, 30.0])));
        // Beyond the values taken: left out.
        assert_eq!(read(&with_height, "1;2;3;4", &mm, &id), Ok(VcbValue::Lengths(vec![1.0, -2.0, 3.0])));
        assert_eq!(read(&base, "7====", &mm, &id), Ok(VcbValue::Lengths(vec![7.0, -7.0])));
    }

    #[test]
    fn every_list_repeats_with_equals() {
        let mm = Units::default();
        let id = DAffine3::IDENTITY;
        // Counts (Copy along grid).
        let grid = Vcb::new("", VcbKind::Lengths(vec![100.0, 50.0])).counts(2);
        let times = |n| Some(Repeat::Times(n));
        assert_eq!(read(&grid, "5x=", &mm, &id), Ok(VcbValue::Counts(vec![times(5), times(5)])));
        assert_eq!(read(&grid, ";3x", &mm, &id), Ok(VcbValue::Counts(vec![None, times(3)])));
        assert_eq!(read(&grid, "5x;3x;2x", &mm, &id), Ok(VcbValue::Counts(vec![times(5), times(3)])));
        assert_eq!(read(&grid, "5x;3x;oops", &mm, &id), Ok(VcbValue::Counts(vec![times(5), times(3)])), "left out, not read");
        // Spacings (Distribute): `10=` is two.
        let distribute = Vcb::new("", VcbKind::Lengths(vec![100.0])).counts(1).spacings();
        assert_eq!(read(&distribute, "10==;20", &mm, &id), Ok(VcbValue::Spacings(vec![Some(10.0), Some(10.0), Some(10.0), Some(20.0)])));
        assert_eq!(read(&distribute, "10=", &mm, &id), Ok(VcbValue::Spacings(vec![Some(10.0), Some(10.0)])));
        assert_eq!(read(&distribute, "40", &mm, &id), Ok(VcbValue::Lengths(vec![40.0])));
        // On a system writing `12.5`, `,` separates too, and lists are written with it.
        solidsketch_i18n::with_separators('.', None, || {
            assert_eq!(read(&grid, "5x,3x", &mm, &id), Ok(VcbValue::Counts(vec![times(5), times(3)])));
            assert_eq!(read(&grid, "12.5,3", &mm, &id), Ok(VcbValue::Lengths(vec![12.5, 3.0])));
            assert_eq!(read(&grid, "60;4", &mm, &id), Ok(VcbValue::Lengths(vec![60.0, 4.0])), "`;` always");
            let tenths = Units { decimals: 1, ..Default::default() };
            assert_eq!(display(&Vcb::new("", VcbKind::Lengths(vec![12.5, 300.0])), &tenths), "12.5, 300");
        });
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
        let mm = Units { decimals: 2, symbol: true, ..Default::default() };
        let lengths = |v: Vec<f64>| Vcb::new("", VcbKind::Lengths(v));
        assert_eq!(display(&lengths(vec![12.5, -300.0]), &mm), "12,5 mm; 300 mm");
        assert_eq!(display(&lengths(vec![12.5, -300.0, 0.0]).optional(1), &mm), "12,5 mm; 300 mm");
        assert_eq!(display(&Vcb::new("", VcbKind::Angle(-22.5f64.to_radians())), &mm), "-22,5°");
        assert_eq!(display(&Vcb::new("", VcbKind::Angle(22.54f64.to_radians())), &mm), "~ 22,5°");
        assert_eq!(display(&lengths(vec![12.345]), &mm), "~ 12,35 mm");
        let cm = Units { length: LengthUnit::Centimeter, ..mm };
        assert_eq!(display(&lengths(vec![250.0]), &cm), "25 cm");
    }

    #[test]
    fn typed_for_the_bubbles() {
        let mm = Units { decimals: 2, symbol: true, ..Default::default() };
        let base = Vcb::new("", VcbKind::Lengths(vec![100.0, -50.0, 0.0])).optional(1);
        // Written as the bubble shows it, the result after `|`.
        let shown = |text: &str| -> Vec<Option<(String, bool)>> {
            let written = |t: Typed| t.result.map_or(t.text.clone(), |r| format!("{}|= {r}", t.text));
            typed(&base, text, &mm).into_iter().map(|t| t.map(|t| (t.active, written(t))).map(|(active, w)| (w, active))).collect()
        };
        let some = |s: &str, active: bool| Some((s.to_owned(), active));
        // A bare number: as typed; `;` moves on to the next bubble, empty, with the caret.
        assert_eq!(shown("600"), vec![some("600", true), None, None]);
        assert_eq!(shown("600;"), vec![some("600", false), some("", true), None]);
        assert_eq!(shown("600; 2*@"), vec![some("600", false), some("2*@|= 100 mm", true), None]);
        // An empty item keeps its value; another unit says more.
        assert_eq!(shown(";40cm"), vec![None, some("40cm|= 400 mm", true), None]);
        assert_eq!(shown("600mm"), vec![some("600mm", true), None, None]);
        // What does not read (yet): as typed.
        assert_eq!(shown("2m*"), vec![some("2m*", true), None, None]);
        // `=` repeats: the next ones show the result.
        assert_eq!(shown("600="), vec![some("600", true), some("|= 600 mm", false), None]);
        assert_eq!(shown("@/2=="), vec![some("@/2|= 50 mm", true), some("|= 25 mm", false), some("|= 0 mm", false)]);
        // Beyond the values: left out.
        assert_eq!(shown("1;2;3;4"), vec![some("1", false), some("2", false), some("3", false)]);
        assert_eq!(shown("7===="), vec![some("7", true), some("|= 7 mm", false), some("|= 7 mm", false)]);
        assert_eq!(shown("1=;"), vec![some("1", false), some("|= 1 mm", false), some("", true)]);
        // A point: for the box only.
        assert_eq!(shown("[0;0;0]"), vec![None, None, None]);
        let angle = Vcb::new("", VcbKind::Angle(90f64.to_radians()));
        let one = |text: &str| typed(&angle, text, &mm).into_iter().map(|t| t.map(|t| (t.text, t.result))).collect::<Vec<_>>();
        let r = |t: &str, r: Option<&str>| vec![Some((t.to_owned(), r.map(str::to_owned)))];
        assert_eq!(one("45"), r("45", None));
        assert_eq!(one("45°"), r("45°", None));
        assert_eq!(one("@/2"), r("@/2", Some("45°")));
        assert!(typed(&Vcb::new("", VcbKind::Count), "5x", &mm).is_empty());
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

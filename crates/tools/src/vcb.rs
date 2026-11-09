//! The measurements box (SketchUp's VCB): lengths typed while a tool runs.
//!
//! `250`, `25cm`, `1,5m`, `2.5 m`, `-40`; several values separated by `;` (`300;200`). A
//! bare number is in the document's unit. Both `.` and `,` are decimal points (French keyboards), so
//! `;` is the only list separator.

use solidsketch_model::Units;

/// Parses one length, in mm; a bare number is in `units`.
pub fn length(s: &str, units: &Units) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    if s.is_empty() {
        return None;
    }
    let split = s.find(|c: char| c.is_ascii_alphabetic() || c == '"' || c == '\'').unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let v: f64 = num.trim().parse().ok()?;
    let scale = match unit.trim() {
        "" => units.length.mm(),
        "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "km" => 1e6,
        "\"" | "in" => 25.4,
        "'" | "ft" => 304.8,
        _ => return None,
    };
    v.is_finite().then_some(v * scale)
}

/// Parses `;`-separated lengths; an empty item is `None` (keep the current value).
pub fn lengths(s: &str, units: &Units) -> Option<Vec<Option<f64>>> {
    s.split(';')
        .map(|item| if item.trim().is_empty() { Some(None) } else { length(item, units).map(Some) })
        .collect()
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
    fn parses() {
        let mm = Units::default();
        assert_eq!(length("250", &mm), Some(250.0));
        assert_eq!(length("1,5m", &mm), Some(1500.0));
        assert_eq!(length(" 2.5 cm ", &mm), Some(25.0));
        assert_eq!(length("-40", &mm), Some(-40.0));
        assert_eq!(length("1\"", &mm), Some(25.4));
        assert_eq!(length("abc", &mm), None);
        assert_eq!(lengths("300;;2m", &mm), Some(vec![Some(300.0), None, Some(2000.0)]));
        assert_eq!(format(12.5, &mm), "12,5 mm");
        assert_eq!(format(300.0, &mm), "300 mm");
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

    #[test]
    fn bare_numbers_are_in_the_document_unit() {
        let cm = Units { length: LengthUnit::Centimeter, ..Default::default() };
        assert_eq!(length("25", &cm), Some(250.0));
        assert_eq!(length("25mm", &cm), Some(25.0));
        assert_eq!(format(250.0, &cm), "25 cm");
    }
}

//! Values typed for a field — the measurements box, a property — read with [`crate::expr`]:
//! lengths and angles, lists of them, points. As in OpenCutList's `UserTextHelper`:
//!
//! - `@` is the field's current value: `@/2`, `@+10`, `min(@; 300)`;
//! - `;` separates values (`600;400`), an empty one keeps its current value (`;400`), `=`
//!   repeats one (`600=` is `600;600`, `50==;20` is `50;50;50;20`), as [`list`] reads them —
//!   every list typed goes through it. `,` separates them too on a system writing `12.5`
//!   (its list separator, [`solidsketch_i18n::list_separator`]); otherwise `,` and `.` are
//!   both decimal points. A list only splits outside parentheses, so `min(@; 300);400` is
//!   two values. Values beyond those a field takes are left out;
//! - `[x;y;z]` is a point in absolute coordinates, `<dx;dy;dz>` a point relative to another;
//!   `@` is then the coordinate the cursor gives.
//!
//! A bare number is in the document's unit: expressions are computed in it (so `@*3` and
//! `@ - 2` read alike) and the result converted to mm.

use glam::DVec3;

use crate::Units;
use crate::expr::{self, ExprError, Literal, Quantity, Result, Value};

/// A length in mm. `@` is `current`'s magnitude and the result takes `current`'s sign (a
/// typed length keeps the direction of the gesture: `-40` goes the other way); empty, it is
/// `current`.
pub fn length(text: &str, units: &Units, current: f64) -> Result<f64> {
    let sign = if current < 0.0 { -1.0 } else { 1.0 };
    if text.trim().is_empty() {
        return Ok(current);
    }
    Ok(sign * coordinate(text, units, current.abs())?)
}

/// A length in mm, `@` being `current` as is, sign included (a coordinate); empty, `current`.
pub fn coordinate(text: &str, units: &Units, current: f64) -> Result<f64> {
    if text.trim().is_empty() {
        return Ok(current);
    }
    let unit = units.length.mm();
    let v = expr::evaluate(
        text,
        &|l: &Literal| match l.unit {
            None => Ok(Value::bare(l.number)),
            Some(m) => m.mm().map(|mm| Value::measure(l.number * mm / unit)).ok_or_else(|| ExprError::WrongUnit(l.text.clone())),
        },
        &|name: &str| variable(name, current / unit),
    )?;
    result(v, Quantity::Length).map(|n| n * unit)
}

/// An angle in degrees, `@` being `current` (degrees); a bare number is in degrees, `°` may
/// follow it. Empty, `current`.
pub fn angle(text: &str, current: f64) -> Result<f64> {
    if text.trim().is_empty() {
        return Ok(current);
    }
    let v = expr::evaluate(
        text,
        &|l: &Literal| match l.unit {
            None => Ok(Value::bare(l.number)),
            Some(expr::Mark::Degree) => Ok(Value::measure(l.number)),
            Some(_) => Err(ExprError::WrongUnit(l.text.clone())),
        },
        &|name: &str| variable(name, current),
    )?;
    result(v, Quantity::Angle)
}

/// `;`-separated lengths (mm), one per current value (see [`length`] for each); those not
/// typed keep their current value, those beyond are left out.
pub fn lengths(text: &str, units: &Units, current: &[f64]) -> Result<Vec<f64>> {
    let items = list(text);
    current.iter().enumerate().map(|(i, &c)| length(items.get(i).map_or("", |i| i.value.as_str()), units, c)).collect()
}

/// A typed point, if `text` is one (`None` otherwise): `[x;y;z]` absolute, `<dx;dy;dz>` from
/// `relative_to`, both in the same coordinates as `target`, the point the cursor gives. A
/// coordinate not typed is `target`'s; `@` is `target`'s, from the point's origin.
pub fn point(text: &str, units: &Units, target: DVec3, relative_to: DVec3) -> Option<Result<DVec3>> {
    let t = text.trim();
    let (inner, origin) = match t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        Some(s) => (s, DVec3::ZERO),
        None => (t.strip_prefix('<')?.strip_suffix('>')?, relative_to),
    };
    let mut p = target;
    for (k, item) in list(inner).iter().take(3).enumerate() {
        match coordinate(&item.value, units, target[k] - origin[k]) {
            Ok(v) => p[k] = origin[k] + v,
            Err(e) => return Some(Err(e)),
        }
    }
    Some(Ok(p))
}

/// A value of a typed list ([`list`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// As typed, trimmed, without the `=` repeating it; empty: keeps the current value.
    pub value: String,
    /// Which of the items typed it is (the one before it repeated by `=` keeps its index).
    pub typed: usize,
    /// A repeat of the one before (by `=`), not typed itself.
    pub repeat: bool,
}

/// The values of a typed list — every one goes through it, as OpenCutList's
/// `_split_user_text`: split on `;`, and on the system's list separator when it is `,`
/// ([`solidsketch_i18n::list_separator`]), outside parentheses and brackets; trimmed; `=`
/// after a value repeats it (`50=;-12` → `50`, `50`, `-12`; `50==` → three `50`). An item of
/// `=` alone is no repeat (it does not read). Never empty: `""` is one empty value.
pub fn list(text: &str) -> Vec<Item> {
    let comma = solidsketch_i18n::list_separator() == ',';
    let mut typed = Vec::new();
    let mut depth = 0i32;
    let mut item = String::new();
    for c in text.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ';' | ',' if depth <= 0 && (c == ';' || comma) => {
                typed.push(std::mem::take(&mut item));
                continue;
            }
            _ => {}
        }
        item.push(c);
    }
    typed.push(item);
    let mut out = Vec::with_capacity(typed.len());
    for (i, item) in typed.iter().enumerate() {
        let item = item.trim();
        let value = item.trim_end_matches('=').trim_end();
        let repeats = if value.is_empty() { 0 } else { item.len() - item.trim_end_matches('=').len() };
        let value = if repeats > 0 { value } else { item };
        out.extend((0..=repeats).map(|r| Item { value: value.to_owned(), typed: i, repeat: r > 0 }));
    }
    out
}

/// What [`list`] gives, written back: its values joined by the list separator.
pub fn join<S: AsRef<str>>(values: impl IntoIterator<Item = S>) -> String {
    let separator = format!("{} ", solidsketch_i18n::list_separator());
    values.into_iter().map(|v| v.as_ref().to_owned()).collect::<Vec<_>>().join(&separator)
}

fn variable(name: &str, current: f64) -> Result<Value> {
    if name.is_empty() { Ok(Value::measure(current)) } else { Err(ExprError::UnknownVariable(name.into())) }
}

/// The number of a measure: a factor (`@/@`) is refused, a bare number is the measure.
fn result(v: Value, what: Quantity) -> Result<f64> {
    if v.dimension == Some(0) {
        return Err(ExprError::NotA(what));
    }
    if !v.number.is_finite() {
        return Err(ExprError::ZeroDivision);
    }
    Ok(v.number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LengthUnit;

    fn mm() -> Units {
        Units::default()
    }

    fn cm() -> Units {
        Units { length: LengthUnit::Centimeter, ..Default::default() }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn lengths_in_the_document_unit() {
        assert_eq!(length("250", &mm(), 0.0), Ok(250.0));
        assert_eq!(length("25", &cm(), 0.0), Ok(250.0));
        assert_eq!(length("25mm", &cm(), 0.0), Ok(25.0));
        assert_eq!(length("1,5m", &mm(), 0.0), Ok(1500.0));
        assert!(close(length("1\"", &mm(), 0.0).unwrap(), 25.4));
        // @ is the current value; a bare number in a product is a factor, whatever the unit.
        assert_eq!(length("@*3", &cm(), 120.0), Ok(360.0));
        assert_eq!(length("@-2", &cm(), 120.0), Ok(100.0));
        assert_eq!(length("@/2", &mm(), 300.0), Ok(150.0));
        assert_eq!(length("", &mm(), 300.0), Ok(300.0));
        assert_eq!(length("2m*3m", &mm(), 0.0), Err(ExprError::InvalidDimension));
        assert_eq!(length("@/@", &mm(), 10.0), Err(ExprError::NotA(Quantity::Length)));
        assert_eq!(length("30°", &mm(), 0.0), Err(ExprError::WrongUnit("30°".into())));
    }

    #[test]
    fn a_typed_length_keeps_the_gesture_s_direction() {
        assert_eq!(length("50", &mm(), -10.0), Ok(-50.0));
        assert_eq!(length("-50", &mm(), -10.0), Ok(50.0));
        assert_eq!(length("@+5", &mm(), -10.0), Ok(-15.0));
        assert_eq!(length("", &mm(), -10.0), Ok(-10.0));
        assert_eq!(coordinate("@+5", &mm(), -10.0), Ok(-5.0));
    }

    #[test]
    fn angles() {
        assert_eq!(angle("90", 0.0), Ok(90.0));
        assert_eq!(angle("-45°", 0.0), Ok(-45.0));
        assert_eq!(angle("22,5", 0.0), Ok(22.5));
        assert_eq!(angle("360/7", 0.0).map(|a| (a * 1e6).round()), Ok((360.0 / 7.0 * 1e6f64).round()));
        assert_eq!(angle("90-@", 30.0), Ok(60.0));
        assert_eq!(angle("@/2", 30.0), Ok(15.0));
        assert_eq!(angle("30mm", 0.0), Err(ExprError::WrongUnit("30mm".into())));
        assert!(angle("abc", 0.0).is_err());
    }

    fn values(text: &str) -> Vec<String> {
        list(text).into_iter().map(|i| i.value).collect()
    }

    #[test]
    fn lists() {
        assert_eq!(values("50"), ["50"]);
        assert_eq!(values(""), [""]);
        assert_eq!(values(";50"), ["", "50"]);
        assert_eq!(values("50=;-12"), ["50", "50", "-12"]);
        assert_eq!(values("50=="), ["50", "50", "50"]);
        assert_eq!(values(" @/2 = ; 3"), ["@/2", "@/2", "3"]);
        assert_eq!(values("min(@; 300);400"), ["min(@; 300)", "400"]);
        assert_eq!(values("10,5=;2"), ["10,5", "10,5", "2"], "`,` a decimal point");
        assert_eq!(values("=;="), ["=", "="], "a lone `=` is no repeat");
        assert_eq!(values("5x=;/3"), ["5x", "5x", "/3"]);
        let typed = |text: &str| list(text).into_iter().map(|i| (i.typed, i.repeat)).collect::<Vec<_>>();
        assert_eq!(typed("1=;2;3=="), [(0, false), (0, true), (1, false), (2, false), (2, true), (2, true)]);
        assert_eq!(join(["1 mm", "2 mm"]), "1 mm; 2 mm");
        assert_eq!(lengths("60cm;0,4m", &mm(), &[1.0, 1.0]), Ok(vec![600.0, 400.0]));
        assert_eq!(lengths("300;;", &mm(), &[1.0, 2.0, 3.0]), Ok(vec![300.0, 2.0, 3.0]));
        assert_eq!(lengths("600=", &mm(), &[1.0, -2.0]), Ok(vec![600.0, -600.0]));
        assert_eq!(lengths(";@*2", &mm(), &[10.0, 20.0]), Ok(vec![10.0, 40.0]));
        // Beyond the values taken: left out.
        assert_eq!(lengths("1;2;3", &mm(), &[0.0, 0.0]), Ok(vec![1.0, 2.0]));
        assert_eq!(lengths("1;x", &mm(), &[0.0]), Ok(vec![1.0]));
        assert_eq!(lengths("7====", &mm(), &[0.0, 0.0]), Ok(vec![7.0, 7.0]));
    }

    #[test]
    fn lists_on_a_system_writing_12_point_5() {
        solidsketch_i18n::with_separators('.', None, || {
            assert_eq!(values("10,5"), ["10", "5"]);
            assert_eq!(values("10;5, 2="), ["10", "5", "2", "2"]);
            assert_eq!(values("min(@, 300),400"), ["min(@, 300)", "400"]);
            assert_eq!(lengths("12.5,3", &mm(), &[0.0, 0.0]), Ok(vec![12.5, 3.0]));
            assert_eq!(join(["1", "2"]), "1, 2");
            let target = DVec3::ZERO;
            assert_eq!(point("[1,2,3]", &mm(), target, target), Some(Ok(DVec3::new(1.0, 2.0, 3.0))));
        });
        // A system's list separator other than `,` (Windows': `;` with `.`): `,` is a decimal point.
        solidsketch_i18n::with_separators('.', Some(';'), || assert_eq!(values("10,5;2"), ["10,5", "2"]));
    }

    #[test]
    fn points() {
        let target = DVec3::new(100.0, -200.0, 300.0);
        let from = DVec3::new(10.0, 10.0, 10.0);
        assert_eq!(point("50", &mm(), target, from), None);
        assert_eq!(point("[1;2;3]", &cm(), target, from), Some(Ok(DVec3::new(10.0, 20.0, 30.0))));
        assert_eq!(point("<1;2;3>", &mm(), target, from), Some(Ok(DVec3::new(11.0, 12.0, 13.0))));
        // Not typed: the cursor's; `@`: the cursor's, from the origin of the point.
        assert_eq!(point("[;;0]", &mm(), target, from), Some(Ok(DVec3::new(100.0, -200.0, 0.0))));
        assert_eq!(point("<@/2;@;0>", &mm(), target, from), Some(Ok(DVec3::new(55.0, -200.0, 10.0))));
        assert_eq!(point("[50=;]", &mm(), target, from), Some(Ok(DVec3::new(50.0, 50.0, 300.0))));
        assert_eq!(point("[1;2;3;4]", &mm(), target, from), Some(Ok(DVec3::new(1.0, 2.0, 3.0))));
        assert!(matches!(point("[1;x;3]", &mm(), target, from), Some(Err(_))));
    }
}

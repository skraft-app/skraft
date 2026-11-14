//! Values typed for a field — the measurements box, a property — read with [`crate::expr`]:
//! lengths and angles, lists of them, points. As in OpenCutList's `UserTextHelper`:
//!
//! - `@` is the field's current value: `@/2`, `@+10`, `min(@; 300)`;
//! - `;` separates values (`600;400`), an empty one keeps its current value (`;400`), `=`
//!   repeats one (`600=` is `600;600`, `50==;20` is `50;50;50;20`). `,` and `.` are both
//!   decimal points, so `;` is the only separator; it only splits outside parentheses, so
//!   `min(@; 300);400` is two values;
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

/// `;`-separated lengths (mm), at most one per current value (see [`length`] for each);
/// those not typed keep their current value.
pub fn lengths(text: &str, units: &Units, current: &[f64]) -> Result<Vec<f64>> {
    let items = split(text);
    if items.len() > current.len() {
        return Err(ExprError::TooMany(current.len()));
    }
    current.iter().enumerate().map(|(i, &c)| length(items.get(i).map_or("", String::as_str), units, c)).collect()
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
    let items = split(inner);
    if items.len() > 3 {
        return Some(Err(ExprError::TooMany(3)));
    }
    let mut p = target;
    for (k, item) in items.iter().enumerate() {
        match coordinate(item, units, target[k] - origin[k]) {
            Ok(v) => p[k] = origin[k] + v,
            Err(e) => return Some(Err(e)),
        }
    }
    Some(Ok(p))
}

/// The values of a `;`-separated list, split outside parentheses, trimmed; `=` after one
/// repeats it (`50=;-12` → `50`, `50`, `-12`).
pub fn split(text: &str) -> Vec<String> {
    items(text)
        .into_iter()
        .flat_map(|item| {
            let (value, repeats) = repeated(&item);
            if repeats > 0 { vec![value.to_owned(); repeats + 1] } else { vec![item] }
        })
        .collect()
}

/// The items of a `;`-separated list as typed: split outside parentheses, trimmed, their `=`
/// kept (`50=;-12` → `50=`, `-12`).
pub fn items(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut item = String::new();
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ';' if depth <= 0 => {
                items.push(std::mem::take(&mut item));
                continue;
            }
            _ => {}
        }
        item.push(c);
    }
    items.push(item);
    items.into_iter().map(|item| item.trim().to_owned()).collect()
}

/// An item of a list (as [`items`] gives it) without the `=` that repeat it, and how many
/// more times it is repeated; an item of `=` alone is no repeat.
pub fn repeated(item: &str) -> (&str, usize) {
    let value = item.trim_end_matches('=').trim_end();
    let repeats = item.len() - item.trim_end_matches('=').len();
    if repeats > 0 && !value.is_empty() { (value, repeats) } else { (item, 0) }
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

    #[test]
    fn lists() {
        assert_eq!(split("50"), ["50"]);
        assert_eq!(split(";50"), ["", "50"]);
        assert_eq!(split("50=;-12"), ["50", "50", "-12"]);
        assert_eq!(split("50=="), ["50", "50", "50"]);
        assert_eq!(split(" @/2 = ; 3"), ["@/2", "@/2", "3"]);
        assert_eq!(split("min(@; 300);400"), ["min(@; 300)", "400"]);
        assert_eq!(lengths("60cm;0,4m", &mm(), &[1.0, 1.0]), Ok(vec![600.0, 400.0]));
        assert_eq!(lengths("300;;", &mm(), &[1.0, 2.0, 3.0]), Ok(vec![300.0, 2.0, 3.0]));
        assert_eq!(lengths("600=", &mm(), &[1.0, -2.0]), Ok(vec![600.0, -600.0]));
        assert_eq!(lengths(";@*2", &mm(), &[10.0, 20.0]), Ok(vec![10.0, 40.0]));
        assert_eq!(lengths("1;2;3", &mm(), &[0.0, 0.0]), Err(ExprError::TooMany(2)));
        assert_eq!(lengths("1;2", &mm(), &[0.0]), Err(ExprError::TooMany(1)));
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
        assert_eq!(point("[1;2;3;4]", &mm(), target, from), Some(Err(ExprError::TooMany(3))));
        assert!(matches!(point("[1;x;3]", &mm(), target, from), Some(Err(_))));
    }
}

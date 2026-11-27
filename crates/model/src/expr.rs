//! Expressions typed for a value: `2m*(2+3)`, `@/2`, `@ - 5mm`, `min(@ - 5mm; 20mm)`,
//! `floor(@ - 5mm; 12mm; 15mm; 20mm)`. A port of OpenCutList's `LengthExpressionUtils`: the
//! arithmetic is here, the caller says how a literal and a variable read ([`evaluate`]), and
//! [`crate::measure`] reads lengths, angles, lists and points with it.
//!
//! Values carry a dimension, checked strictly: 1 for a length (or an angle), 0 for a factor,
//! `None` for a bare number, which is either — the caller computes in the unit a bare number
//! is in, so `@*3` and `@ - 2` both read. `2m*3m` and `@/@` are refused.

use std::fmt;

use skraft_i18n::t;

/// A unit mark after a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Mm,
    Cm,
    M,
    Yd,
    /// `in`, `"`, `″`.
    In,
    /// `ft`, `'`, `′`.
    Ft,
    /// `°`.
    Degree,
}

impl Mark {
    /// Millimetres in one of this unit, `None` for an angle.
    pub fn mm(self) -> Option<f64> {
        match self {
            Mark::Mm => Some(1.0),
            Mark::Cm => Some(10.0),
            Mark::M => Some(1000.0),
            Mark::Yd => Some(914.4),
            Mark::In => Some(25.4),
            Mark::Ft => Some(304.8),
            Mark::Degree => None,
        }
    }
}

/// A number as typed, with its unit if any. Feet and inches (`1'6"`, `1'6`) are read as
/// inches.
#[derive(Clone, Debug, PartialEq)]
pub struct Literal {
    pub number: f64,
    pub unit: Option<Mark>,
    pub text: String,
}

/// A value: its number and dimension (1 a length or angle, 0 a factor, `None` a bare number).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Value {
    pub number: f64,
    pub dimension: Option<u8>,
}

impl Value {
    pub fn bare(number: f64) -> Self {
        Value { number, dimension: None }
    }

    pub fn measure(number: f64) -> Self {
        Value { number, dimension: Some(1) }
    }
}

/// What a field takes, for [`ExprError::NotA`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    Length,
    Angle,
}

/// Why an expression can't be evaluated. The messages are the status bar's.
#[derive(Clone, Debug, PartialEq)]
pub enum ExprError {
    /// What could not be read, from where it starts.
    Syntax(String),
    MissingOperand,
    MissingCloseParenthesis,
    UnexpectedCloseParenthesis,
    ZeroDivision,
    /// `2m*3m`, `@ + 2°`.
    InvalidDimension,
    /// A literal with a unit that is not the field's: `30mm` for an angle.
    WrongUnit(String),
    /// The result is a factor (`@/@`), not what the field takes.
    NotA(Quantity),
    UnknownVariable(String),
    /// `floor`, `ceil`: no step fits.
    NoMatchingValue(&'static str),
    /// A function needs at least two values.
    ArgumentCount(&'static str),
    /// `min(8,12)`: a decimal comma where `;` was meant.
    CommaSeparator(&'static str),
    /// More nested parentheses, signs or calls than [`MAX_DEPTH`].
    TooDeep,
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&match self {
            ExprError::Syntax(text) => t!("error.expr.syntax", text = text),
            ExprError::MissingOperand => t!("error.expr.missing_operand"),
            ExprError::MissingCloseParenthesis => t!("error.expr.missing_close_parenthesis"),
            ExprError::UnexpectedCloseParenthesis => t!("error.expr.unexpected_close_parenthesis"),
            ExprError::ZeroDivision => t!("error.expr.zero_division"),
            ExprError::InvalidDimension => t!("error.expr.invalid_dimension"),
            ExprError::WrongUnit(text) => t!("error.expr.wrong_unit", text = text),
            ExprError::NotA(Quantity::Length) => t!("error.expr.not_a_length"),
            ExprError::NotA(Quantity::Angle) => t!("error.expr.not_an_angle"),
            ExprError::UnknownVariable(name) => t!("error.expr.unknown_variable", name = name),
            ExprError::NoMatchingValue(function) => t!("error.expr.no_matching_value", function = function),
            ExprError::ArgumentCount(function) => t!("error.expr.argument_count", function = function),
            ExprError::CommaSeparator(function) => t!("error.expr.comma_separator", function = function),
            ExprError::TooDeep => t!("error.expr.too_deep"),
        })
    }
}

impl std::error::Error for ExprError {}

pub type Result<T> = std::result::Result<T, ExprError>;

/// The value of `text`. `literal` reads a number as typed (its dimension: `None` bare, 1 with
/// a unit of the field's, else [`ExprError::WrongUnit`]); `variable` reads `@name` (`""` for a
/// lone `@`).
pub fn evaluate(text: &str, literal: &dyn Fn(&Literal) -> Result<Value>, variable: &dyn Fn(&str) -> Result<Value>) -> Result<Value> {
    let tokens = tokenize(text, literal, variable)?;
    if tokens.is_empty() {
        return Err(ExprError::MissingOperand);
    }
    let mut p = Parser { tokens: &tokens, i: 0, depth: 0 };
    let v = p.sum()?;
    p.end()?;
    Ok(v)
}

/// Does `text` use a variable?
pub fn uses_variables(text: &str) -> bool {
    text.contains('@')
}

// ---- tokens ------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Func {
    Min,
    Max,
    Floor,
    Ceil,
}

impl Func {
    fn name(self) -> &'static str {
        match self {
            Func::Min => "min",
            Func::Max => "max",
            Func::Floor => "floor",
            Func::Ceil => "ceil",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// A literal or a variable, and whether it was typed with a comma (`8,12`).
    Val(Value, bool),
    /// `+ - * / ( ) ;`
    Op(char),
    Func(Func),
}

/// How far a value may miss a step and still take it (floor, ceil).
const STEP_TOLERANCE: f64 = 1e-6;

/// The arguments of a function are separated by `;`, or by `,` right after a value or a
/// parenthesis: `min(10mm, 20mm)`. A `,` is otherwise a decimal point: `min(10,5; 20)`.
const SEPARATOR: char = ';';

fn tokenize(text: &str, literal: &dyn Fn(&Literal) -> Result<Value>, variable: &dyn Fn(&str) -> Result<Value>) -> Result<Vec<Tok>> {
    let c: Vec<char> = text.chars().collect();
    let rest = |i: usize| c[i..].iter().collect::<String>();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == ',' && matches!(tokens.last(), Some(Tok::Val(..) | Tok::Op(')'))) {
            tokens.push(Tok::Op(SEPARATOR));
            i += 1;
        } else if let Some((lit, end)) = read_literal(&c, i)? {
            let v = literal(&lit)?;
            tokens.push(Tok::Val(v, lit.text.contains(',')));
            i = end;
        } else if ch == '@' {
            let mut j = i + 1;
            if j < c.len() && (c[j].is_ascii_alphabetic() || c[j] == '_') {
                while j < c.len() && (c[j].is_ascii_alphanumeric() || c[j] == '_') {
                    j += 1;
                }
            }
            let name: String = c[i + 1..j].iter().collect();
            tokens.push(Tok::Val(variable(&name)?, false));
            i = j;
        } else if ch.is_ascii_alphabetic() {
            let mut j = i;
            while j < c.len() && c[j].is_ascii_alphabetic() {
                j += 1;
            }
            let name: String = c[i..j].iter().collect::<String>().to_lowercase();
            let mut k = j;
            while k < c.len() && c[k].is_whitespace() {
                k += 1;
            }
            let func = match name.as_str() {
                "min" => Func::Min,
                "max" => Func::Max,
                "floor" => Func::Floor,
                "ceil" => Func::Ceil,
                _ => return Err(ExprError::Syntax(rest(i))),
            };
            if c.get(k) != Some(&'(') {
                return Err(ExprError::Syntax(rest(i)));
            }
            tokens.push(Tok::Func(func));
            i = j;
        } else if "+-*/();".contains(ch) {
            tokens.push(Tok::Op(ch));
            i += 1;
        } else {
            return Err(ExprError::Syntax(rest(i)));
        }
    }
    Ok(tokens)
}

/// Is `ch` a decimal point: `.`, and `,` unless it is the system's list separator
/// ([`skraft_i18n::list_separator`]: `,` then separates values, `10,5` is two).
fn decimal_point(ch: char) -> bool {
    ch == '.' || (ch == ',' && skraft_i18n::list_separator() != ',')
}

/// The literal at `i`, if a number starts there, and where it ends.
fn read_literal(c: &[char], i: usize) -> Result<Option<(Literal, usize)>> {
    let starts = c[i].is_ascii_digit() || (decimal_point(c[i]) && c.get(i + 1).is_some_and(|d| d.is_ascii_digit()));
    if !starts {
        return Ok(None);
    }
    let text = |end: usize| c[i..end].iter().collect::<String>().trim().to_owned();
    // Feet and inches: 1'6", 1' 6 1/2, 1ft6in. A number after feet is inches.
    let (whole, j) = digits(c, i);
    if j > i {
        let k = skip_ws(c, j);
        if let Some((Mark::Ft, k)) = mark(c, k) {
            let k = skip_ws(c, k);
            if let Some((inches, mut end)) = number(c, k)? {
                let m = skip_ws(c, end);
                if let Some((Mark::In, m)) = mark(c, m) {
                    end = m;
                }
                let number = whole * 12.0 + inches;
                return Ok(Some((Literal { number, unit: Some(Mark::In), text: text(end) }, end)));
            }
        }
    }
    let Some((number, end)) = number(c, i)? else { return Ok(None) };
    let m = skip_ws(c, end);
    Ok(Some(match mark(c, m) {
        Some((unit, m)) => (Literal { number, unit: Some(unit), text: text(m) }, m),
        None => (Literal { number, unit: None, text: text(end) }, end),
    }))
}

fn skip_ws(c: &[char], mut i: usize) -> usize {
    while i < c.len() && c[i].is_whitespace() {
        i += 1;
    }
    i
}

/// The digits at `i`, as a number, and where they end (`i` if none).
fn digits(c: &[char], i: usize) -> (f64, usize) {
    let mut j = i;
    while j < c.len() && c[j].is_ascii_digit() {
        j += 1;
    }
    (c[i..j].iter().collect::<String>().parse().unwrap_or(0.0), j)
}

/// A number at `i`: a whole and a fraction (`1 1/2`), a fraction (`3/4`), a decimal with `.`
/// or `,` (`2,5`, `.5`).
fn number(c: &[char], i: usize) -> Result<Option<(f64, usize)>> {
    let fraction = |i: usize| -> Option<(f64, f64, usize)> {
        let (n, j) = digits(c, i);
        if j == i || c.get(j) != Some(&'/') {
            return None;
        }
        let (d, k) = digits(c, j + 1);
        (k > j + 1).then_some((n, d, k))
    };
    let (whole, j) = digits(c, i);
    if j > i {
        // 1 1/2
        let k = skip_ws(c, j);
        if k > j
            && let Some((n, d, end)) = fraction(k)
        {
            return if d == 0.0 { Err(ExprError::ZeroDivision) } else { Ok(Some((whole + n / d, end))) };
        }
        // 3/4
        if let Some((n, d, end)) = fraction(i) {
            return if d == 0.0 { Err(ExprError::ZeroDivision) } else { Ok(Some((n / d, end))) };
        }
    }
    // 2,5 · 2.5 · .5
    let mut end = j;
    let mut points = 0;
    while end < c.len() && decimal_point(c[end]) && c.get(end + 1).is_some_and(|d| d.is_ascii_digit()) {
        points += 1;
        end = digits(c, end + 1).1;
    }
    if end == i {
        return Ok(None);
    }
    let s: String = c[i..end].iter().map(|&ch| if ch == ',' { '.' } else { ch }).collect();
    if points > 1 {
        return Err(ExprError::Syntax(c[i..end].iter().collect()));
    }
    Ok(s.parse().ok().map(|v| (v, end)))
}

/// The unit mark at `i`, and where it ends. Word marks can't run into a letter (`2min`).
fn mark(c: &[char], i: usize) -> Option<(Mark, usize)> {
    match c.get(i)? {
        '"' | '″' => return Some((Mark::In, i + 1)),
        '\'' | '′' => return Some((Mark::Ft, i + 1)),
        '°' => return Some((Mark::Degree, i + 1)),
        _ => {}
    }
    for (word, m) in [("mm", Mark::Mm), ("cm", Mark::Cm), ("m", Mark::M), ("yd", Mark::Yd), ("in", Mark::In), ("ft", Mark::Ft)] {
        let n = word.len();
        let matches = c.len() >= i + n && c[i..i + n].iter().zip(word.chars()).all(|(a, b)| a.to_ascii_lowercase() == b);
        if matches && !c.get(i + n).is_some_and(|ch| ch.is_alphabetic()) {
            return Some((m, i + n));
        }
    }
    None
}

// ---- parser ------------------------------------------------------------------------------------

/// The dimension of an operation on `a` and `b`: unchecked if either is a bare number, else
/// what `rule` gives, refused if it gives nothing.
fn dimension(a: Option<u8>, b: Option<u8>, rule: impl Fn(u8, u8) -> Option<u8>) -> Result<Option<u8>> {
    match (a, b) {
        (Some(a), Some(b)) => rule(a, b).map(Some).ok_or(ExprError::InvalidDimension),
        _ => Ok(None),
    }
}

/// How deep parentheses, signs and function calls may nest: far beyond any typed value, far
/// below what overflows the stack (the parser recurses, and a text can come from a control
/// client, docs/control-protocol.md).
const MAX_DEPTH: usize = 64;

struct Parser<'a> {
    tokens: &'a [Tok],
    i: usize,
    /// How many [`Parser::factor`] calls are open.
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.i)
    }

    fn op(&self) -> Option<char> {
        match self.peek() {
            Some(Tok::Op(c)) => Some(*c),
            _ => None,
        }
    }

    fn end(&self) -> Result<()> {
        match self.peek() {
            None => Ok(()),
            Some(Tok::Op(')')) => Err(ExprError::UnexpectedCloseParenthesis),
            Some(_) => Err(ExprError::MissingOperand),
        }
    }

    /// sum = product (('+' | '-') product)*
    fn sum(&mut self) -> Result<Value> {
        let mut v = self.product()?;
        while let Some(o @ ('+' | '-')) = self.op() {
            self.i += 1;
            let w = self.product()?;
            let d = dimension(v.dimension, w.dimension, |a, b| (a == b).then_some(a))?;
            let number = if o == '+' { v.number + w.number } else { v.number - w.number };
            v = Value { number, dimension: d.or(v.dimension).or(w.dimension) };
        }
        Ok(v)
    }

    /// product = factor (('*' | '/') factor)*
    fn product(&mut self) -> Result<Value> {
        let mut v = self.factor()?;
        while let Some(o @ ('*' | '/')) = self.op() {
            self.i += 1;
            let w = self.factor()?;
            v = if o == '*' {
                let d = dimension(v.dimension, w.dimension, |a, b| (a + b <= 1).then_some(a + b))?;
                Value { number: v.number * w.number, dimension: d.or(product_dimension(v.dimension, w.dimension)) }
            } else {
                if w.number == 0.0 {
                    return Err(ExprError::ZeroDivision);
                }
                let d = dimension(v.dimension, w.dimension, |a, b| a.checked_sub(b))?;
                Value { number: v.number / w.number, dimension: d.or(quotient_dimension(v.dimension, w.dimension)) }
            };
        }
        Ok(v)
    }

    /// factor = ('+' | '-') factor | '(' sum ')' | function '(' sum (';' sum)+ ')' | value
    ///
    /// Every recursion of the grammar goes through here, so the depth is counted here.
    fn factor(&mut self) -> Result<Value> {
        if self.depth == MAX_DEPTH {
            return Err(ExprError::TooDeep);
        }
        self.depth += 1;
        let v = self.factor_inner();
        self.depth -= 1;
        v
    }

    fn factor_inner(&mut self) -> Result<Value> {
        match self.peek().cloned() {
            None | Some(Tok::Op('*' | '/' | SEPARATOR)) => Err(ExprError::MissingOperand),
            Some(Tok::Op(')')) => Err(ExprError::UnexpectedCloseParenthesis),
            Some(Tok::Op(o @ ('+' | '-'))) => {
                self.i += 1;
                let v = self.factor()?;
                Ok(if o == '-' { Value { number: -v.number, ..v } } else { v })
            }
            Some(Tok::Op('(')) => {
                self.i += 1;
                let v = self.sum()?;
                if self.op() != Some(')') {
                    return Err(ExprError::MissingCloseParenthesis);
                }
                self.i += 1;
                Ok(v)
            }
            Some(Tok::Func(f)) => {
                self.i += 1;
                self.function(f)
            }
            Some(Tok::Val(v, _)) => {
                self.i += 1;
                Ok(v)
            }
            Some(Tok::Op(c)) => Err(ExprError::Syntax(c.to_string())),
        }
    }

    /// A function's call, from its `(`.
    fn function(&mut self, f: Func) -> Result<Value> {
        let start = self.i;
        let mut args = Vec::new();
        loop {
            self.i += 1; // Past '(' or ';'
            args.push(self.sum()?);
            if self.op() != Some(SEPARATOR) {
                break;
            }
        }
        if self.op() != Some(')') {
            return Err(ExprError::MissingCloseParenthesis);
        }
        if args.len() < 2 {
            let comma = self.tokens[start..self.i].iter().any(|t| matches!(t, Tok::Val(_, true)));
            return Err(if comma { ExprError::CommaSeparator(f.name()) } else { ExprError::ArgumentCount(f.name()) });
        }
        self.i += 1;
        let mut d = args[0].dimension;
        for a in &args[1..] {
            d = dimension(d, a.dimension, |a, b| (a == b).then_some(a))?.or(d).or(a.dimension);
        }
        let n: Vec<f64> = args.iter().map(|a| a.number).collect();
        let steps = &n[1..];
        let number = match f {
            Func::Min => Some(n.iter().copied().fold(f64::INFINITY, f64::min)),
            Func::Max => Some(n.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
            Func::Floor => steps.iter().copied().filter(|s| *s <= n[0] + STEP_TOLERANCE).reduce(f64::max),
            Func::Ceil => steps.iter().copied().filter(|s| *s >= n[0] - STEP_TOLERANCE).reduce(f64::min),
        };
        let number = number.ok_or(ExprError::NoMatchingValue(f.name()))?;
        Ok(Value { number, dimension: d })
    }
}

/// The dimension of a product when one side is bare: the other's, if it is a length (a bare
/// number then is a factor); unknown otherwise.
fn product_dimension(a: Option<u8>, b: Option<u8>) -> Option<u8> {
    match (a, b) {
        (Some(1), None) | (None, Some(1)) => Some(1),
        _ => None,
    }
}

/// The same for a quotient: a length over a bare number is a length; a bare number over a
/// length can only be a length too (a factor over a length has no dimension), so a factor.
fn quotient_dimension(a: Option<u8>, b: Option<u8>) -> Option<u8> {
    match (a, b) {
        (Some(1), None) => Some(1),
        (None, Some(1)) => Some(0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lengths in mm: a bare number is mm, `@` is 100.
    fn eval(text: &str) -> Result<Value> {
        evaluate(
            text,
            &|l| match l.unit {
                None => Ok(Value::bare(l.number)),
                Some(m) => m.mm().map(|mm| Value::measure(l.number * mm)).ok_or_else(|| ExprError::WrongUnit(l.text.clone())),
            },
            &|name| if name.is_empty() { Ok(Value::measure(100.0)) } else { Err(ExprError::UnknownVariable(name.into())) },
        )
    }

    fn n(text: &str) -> f64 {
        eval(text).unwrap_or_else(|e| panic!("{text}: {e}")).number
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A text nested past [`MAX_DEPTH`] is refused, not a stack overflow (issue #8).
    #[test]
    fn nesting_depth() {
        let deep = 100_000;
        for text in ["(".repeat(deep), "-".repeat(deep), "min(".repeat(deep), format!("{}1{}", "(".repeat(deep), ")".repeat(deep))] {
            assert_eq!(eval(&text).unwrap_err(), ExprError::TooDeep);
        }
        let fine = MAX_DEPTH - 1;
        assert_eq!(n(&format!("{}2{}", "(".repeat(fine), ")".repeat(fine))), 2.0);
        assert_eq!(n(&format!("{}2", "-".repeat(fine))), if fine % 2 == 0 { 2.0 } else { -2.0 });
    }

    #[test]
    fn literals() {
        assert_eq!(n("50"), 50.0);
        assert_eq!(n("2,5"), 2.5);
        assert_eq!(n("2.5cm"), 25.0);
        assert_eq!(n("2,5 m"), 2500.0);
        assert_eq!(n(".5"), 0.5);
        assert_eq!(n("3/4"), 0.75);
        assert!(close(n("1 1/2in"), 38.1));
        assert!(close(n("1'6\""), 18.0 * 25.4));
        assert!(close(n("1'6"), 18.0 * 25.4));
        assert!(close(n("2ft"), 609.6));
        assert!(close(n("1yd"), 914.4));
        assert_eq!(n("1″"), 25.4);
        assert_eq!(eval("1,2,3"), Err(ExprError::Syntax("1,2,3".into())));
        assert_eq!(eval("3/0"), Err(ExprError::ZeroDivision));
        assert!(matches!(eval("30°"), Err(ExprError::WrongUnit(_))));
    }

    #[test]
    fn arithmetic() {
        assert_eq!(n("2m*(2+3)"), 10000.0);
        assert_eq!(n("600-2*19"), 562.0);
        assert_eq!(n("-5"), -5.0);
        assert_eq!(n("+10"), 10.0);
        assert_eq!(n("10 - -5"), 15.0);
        assert_eq!(n("600 / 2"), 300.0);
        assert_eq!(n("12/4/3"), 1.0);
    }

    #[test]
    fn variables() {
        assert_eq!(n("@"), 100.0);
        assert_eq!(n("@+10"), 110.0);
        assert_eq!(n("@-15"), 85.0);
        assert_eq!(n("@*3"), 300.0);
        assert_eq!(n("@/2"), 50.0);
        assert_eq!(n("3*@"), 300.0);
        assert_eq!(n("@ - 2cm"), 80.0);
        assert_eq!(eval("@foo"), Err(ExprError::UnknownVariable("foo".into())));
        assert!(uses_variables("@/2") && !uses_variables("50"));
    }

    #[test]
    fn dimensions_are_strict() {
        assert_eq!(eval("2m*3m"), Err(ExprError::InvalidDimension));
        assert_eq!(eval("@*@"), Err(ExprError::InvalidDimension));
        assert_eq!(eval("10/2mm"), Ok(Value::measure(5.0))); // a fraction literal: 10/2 mm
        assert_eq!(eval("10/(2mm)").map(|v| v.dimension), Ok(Some(0)));
        assert_eq!(eval("@/@").map(|v| v.dimension), Ok(Some(0)));
        assert_eq!(eval("2m/1m").map(|v| v.dimension), Ok(Some(0)));
        assert_eq!(eval("@ + 2").map(|v| v.dimension), Ok(Some(1)));
        assert_eq!(eval("@ * (2m/1m)").map(|v| (v.number, v.dimension)), Ok((200.0, Some(1))));
        assert_eq!(eval("2 * 3").map(|v| v.dimension), Ok(None));
        assert_eq!(eval("2m + 2m/1m"), Err(ExprError::InvalidDimension));
    }

    #[test]
    fn functions() {
        assert_eq!(n("min(10; 20)"), 10.0);
        assert_eq!(n("max(10mm, 2cm)"), 20.0);
        assert_eq!(n("min(10,5; 20)"), 10.5);
        assert_eq!(n("MIN (1;2;3)"), 1.0);
        assert_eq!(n("floor(@ - 5; 12; 15; 20; 100)"), 20.0);
        assert_eq!(n("ceil(@ - 5; 12; 15; 200)"), 200.0);
        assert_eq!(n("2*max(@;150)"), 300.0);
        assert_eq!(eval("floor(5; 12; 15)"), Err(ExprError::NoMatchingValue("floor")));
        assert_eq!(eval("min(8,12)"), Err(ExprError::CommaSeparator("min")));
        // On a system writing `12.5`, `,` separates.
        skraft_i18n::with_separators('.', None, || {
            assert_eq!(n("min(8,12)"), 8.0);
            assert!(eval("2,5").is_err(), "not a decimal point");
            assert_eq!(n("min(10.5, 20)"), 10.5);
        });
        assert_eq!(eval("min(8)"), Err(ExprError::ArgumentCount("min")));
        assert_eq!(eval("min(1m; 2m/1m)"), Err(ExprError::InvalidDimension));
        assert!(matches!(eval("sin(3)"), Err(ExprError::Syntax(_))));
    }

    /// OpenCutList's cases (test/TC_Ladb_Utils_LengthExpressionUtils.rb).
    #[test]
    fn opencutlist_cases() {
        for (text, v) in [
            ("--5", 5.0),
            ("3*-2", -6.0),
            ("-(5+5)", -10.0),
            ("2+3*4", 14.0),
            ("(2+3)*4", 20.0),
            ("8 / 2 / 2", 2.0),
            ("  50  ", 50.0),
            ("-@", -100.0),
            ("10*(4+6)*10", 1000.0),
            ("min(10; 20; 5)", 5.0),
            ("MAX(5; 3) * 5", 25.0),
            ("max(min(10; 20); 30)", 30.0),
            ("-max(10; 5)", -10.0),
            ("min(10, 20)", 10.0),
            ("floor(14; 12; 15; 20)", 12.0),
            ("floor(15; 12; 15; 20)", 15.0),
            ("ceil(18; 12; 15; 20)", 20.0),
            ("ceil(30 - 15; 20; 15; 12)", 15.0),
            ("floor(100; 12)", 12.0),
        ] {
            assert!(close(n(text), v), "{text}");
        }
        assert_eq!(eval("()"), Err(ExprError::UnexpectedCloseParenthesis));
        assert_eq!(eval("min(10;)"), Err(ExprError::UnexpectedCloseParenthesis));
        assert_eq!(eval("min(10; 20"), Err(ExprError::MissingCloseParenthesis));
        assert_eq!(eval("10; 20"), Err(ExprError::MissingOperand));
        assert_eq!(eval("5/(2-2)"), Err(ExprError::ZeroDivision));
        assert!(matches!(eval("5 exit"), Err(ExprError::Syntax(_))));
        assert!(matches!(eval("sqrt(10; 20)"), Err(ExprError::Syntax(_))));
        assert_eq!(eval("ceil(30; 12; 15)"), Err(ExprError::NoMatchingValue("ceil")));
        assert_eq!(eval("min(@; 2)").map(|v| v.dimension), Ok(Some(1))); // a bare number adapts
    }

    #[test]
    fn syntax_errors() {
        assert_eq!(eval(""), Err(ExprError::MissingOperand));
        assert_eq!(eval("2+"), Err(ExprError::MissingOperand));
        assert_eq!(eval("*2"), Err(ExprError::MissingOperand));
        assert_eq!(eval("(2+3"), Err(ExprError::MissingCloseParenthesis));
        assert_eq!(eval("2+3)"), Err(ExprError::UnexpectedCloseParenthesis));
        assert_eq!(eval("2 3"), Err(ExprError::MissingOperand));
        assert_eq!(eval("10mm, 20mm"), Err(ExprError::MissingOperand));
        assert_eq!(eval("abc"), Err(ExprError::Syntax("abc".into())));
        assert_eq!(eval("2 # 3"), Err(ExprError::Syntax("# 3".into())));
    }
}

//! Typed properties (docs/model/properties.md): values the user, an import or an extension
//! attaches to the model's entities, each value written with its kind; the document's
//! definitions, optional, give a key its kind, label and choices, and check its values.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::document::{Component, ComponentKind, Document, Id};
use crate::{Error, FileError, Refusal, Result};

/// A property's value, its kind with it: `{"length": 18.0}`, `{"money": {"amount": 12.5,
/// "currency": "EUR"}}`. Lengths in mm, areas in mm², volumes in mm³, angles in radians, masses
/// in kg: shown and typed in the document's units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Bool(bool),
    Int(i64),
    Real(f64),
    Text(String),
    Length(f64),
    Area(f64),
    Volume(f64),
    Angle(f64),
    Mass(f64),
    /// An amount in a currency (its ISO 4217 code, « EUR »).
    Money {
        amount: f64,
        currency: String,
    },
    /// sRGB.
    Color([u8; 3]),
    /// One of its definition's choices.
    Choice(String),
    List(Vec<Value>),
}

/// The kind of a value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Bool,
    Int,
    Real,
    Text,
    Length,
    Area,
    Volume,
    Angle,
    Mass,
    Money,
    Color,
    Choice,
    /// A list of values of one kind.
    List(Box<ValueKind>),
}

impl Value {
    /// Its kind; a list's, that of its first element (a text's when empty).
    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Bool(_) => ValueKind::Bool,
            Value::Int(_) => ValueKind::Int,
            Value::Real(_) => ValueKind::Real,
            Value::Text(_) => ValueKind::Text,
            Value::Length(_) => ValueKind::Length,
            Value::Area(_) => ValueKind::Area,
            Value::Volume(_) => ValueKind::Volume,
            Value::Angle(_) => ValueKind::Angle,
            Value::Mass(_) => ValueKind::Mass,
            Value::Money { .. } => ValueKind::Money,
            Value::Color(_) => ValueKind::Color,
            Value::Choice(_) => ValueKind::Choice,
            Value::List(l) => ValueKind::List(Box::new(l.first().map_or(ValueKind::Text, Value::kind))),
        }
    }

    /// Well formed: its numbers finite, a currency named, a list's elements of one kind.
    pub fn is_valid(&self) -> bool {
        match self {
            Value::Real(x) | Value::Length(x) | Value::Area(x) | Value::Volume(x) | Value::Angle(x) | Value::Mass(x) => x.is_finite(),
            Value::Money { amount, currency } => amount.is_finite() && !currency.trim().is_empty(),
            Value::List(l) => l.iter().all(|v| v.is_valid() && !matches!(v, Value::List(_)) && v.kind() == l[0].kind()),
            _ => true,
        }
    }

    /// Of the kind `kind`, and among `choices` for a choice.
    fn fits(&self, kind: &ValueKind, choices: &[String]) -> bool {
        match (self, kind) {
            (Value::List(l), ValueKind::List(k)) => l.iter().all(|v| v.fits(k, choices)),
            (Value::Choice(c), ValueKind::Choice) => choices.contains(c),
            _ => self.kind() == *kind,
        }
    }
}

/// An entity's properties, by key.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Props(BTreeMap<String, Value>);

impl Props {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Sets `key` (`None`: removes it).
    pub fn set(&mut self, key: &str, value: Option<Value>) {
        match value {
            Some(v) => self.0.insert(key.to_owned(), v),
            None => self.0.remove(key),
        };
    }

    /// The same, the keys of `renamed` (old → new) renamed.
    pub(crate) fn renamed(&self, renamed: &BTreeMap<String, String>) -> Props {
        Props(self.0.iter().map(|(k, v)| (renamed.get(k).unwrap_or(k).clone(), v.clone())).collect())
    }

    pub(crate) fn has_any(&self, keys: &BTreeMap<String, String>) -> bool {
        keys.keys().any(|k| self.0.contains_key(k))
    }
}

impl FromIterator<(String, Value)> for Props {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(iter: I) -> Self {
        Props(iter.into_iter().collect())
    }
}

/// A key well formed: `<namespace>.<name>`, the namespace in lowercase ASCII letters, digits,
/// `_` or `-` (`user`, `skp`, an extension's), the name not empty, no control character.
pub fn is_key(key: &str) -> bool {
    let Some((ns, name)) = key.split_once('.') else { return false };
    !ns.is_empty()
        && ns.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        && !name.trim().is_empty()
        && !key.chars().any(char::is_control)
}

/// What kind of entity holds properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// The model itself: the root assembly.
    Model,
    Part,
    Assembly,
    Occurrence,
    BodyDef,
    Body,
    Material,
    Guide,
}

/// A property's definition in the document: the kind of its values, how it is shown, where it
/// applies. Values of a key without one are typed all the same, shown as they are.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropDef {
    /// Its name as shown (« Prix »); empty: the key's name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub kind: ValueKind,
    /// The values of a choice (of a list of choices).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// The value an entity without one is shown with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// The kinds of entities it applies to; empty: all.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub applies_to: BTreeSet<EntityKind>,
}

impl PropDef {
    pub fn new(kind: ValueKind) -> Self {
        PropDef { label: String::new(), kind, choices: Vec::new(), default: None, applies_to: BTreeSet::new() }
    }

    /// Well formed: a choice with choices, none empty or twice, its default fitting it.
    pub fn is_valid(&self) -> bool {
        let choice = matches!(&self.kind, ValueKind::Choice) || matches!(&self.kind, ValueKind::List(k) if **k == ValueKind::Choice);
        let mut seen = BTreeSet::new();
        let choices = self.choices.iter().all(|c| !c.trim().is_empty() && seen.insert(c));
        !matches!(&self.kind, ValueKind::List(k) if matches!(**k, ValueKind::List(_)))
            && choice == !self.choices.is_empty()
            && choices
            && self.default.as_ref().is_none_or(|d| self.accepts(d))
    }

    /// Does `value` fit it (its kind, its choices)?
    pub fn accepts(&self, value: &Value) -> bool {
        value.is_valid() && value.fits(&self.kind, &self.choices)
    }

    /// Does it apply to an entity of `kind`?
    pub fn applies(&self, kind: EntityKind) -> bool {
        self.applies_to.is_empty() || self.applies_to.contains(&kind)
    }
}

/// The entity whose properties an operation changes (`Operation::SetProps`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropTarget {
    /// A part or an assembly — the root: the model.
    Component(Id),
    /// An occurrence of the assembly `context`.
    Occurrence {
        context: Id,
        occurrence: Id,
    },
    /// A body definition of the part `part`: every body of it.
    BodyDef {
        part: Id,
        def: Id,
    },
    /// A body of the part `part` (its own, or a group's).
    Body {
        part: Id,
        body: Id,
    },
    Material(Id),
    /// A guide of `component`.
    Guide {
        component: Id,
        guide: Id,
    },
}


impl Component {
    /// Every properties it holds — its own, its guides', its occurrences' (an assembly), its
    /// body definitions' and bodies' (a part) —, with the kind of their entity; `root`: the
    /// model's root, its own properties the model's.
    fn each_props(&self, root: bool) -> Vec<(EntityKind, &Props)> {
        let own = match (&self.kind, root) {
            (_, true) => EntityKind::Model,
            (ComponentKind::Part(_), _) => EntityKind::Part,
            (ComponentKind::Assembly(_), _) => EntityKind::Assembly,
        };
        let mut out = vec![(own, &self.props)];
        out.extend(self.guides.iter().map(|g| (EntityKind::Guide, &g.props)));
        match &self.kind {
            ComponentKind::Assembly(a) => out.extend(a.occurrences().iter().map(|o| (EntityKind::Occurrence, &o.props))),
            ComponentKind::Part(p) => {
                out.extend(p.defs.iter().map(|d| (EntityKind::BodyDef, &d.props)));
                let children = p.defs.iter().flat_map(|d| d.children());
                out.extend(p.bodies.iter().chain(children).map(|b| (EntityKind::Body, &b.props)));
            }
        }
        out
    }

    /// The same, every properties' keys of `renamed` (old → new) renamed.
    fn rename_props(&mut self, renamed: &BTreeMap<String, String>) {
        let r = |p: &mut Props| *p = p.renamed(renamed);
        r(&mut self.props);
        self.guides.iter_mut().for_each(|g| r(&mut g.props));
        match &mut self.kind {
            ComponentKind::Assembly(a) => a.occurrences_mut().iter_mut().for_each(|o| r(&mut o.props)),
            ComponentKind::Part(p) => {
                for d in &mut p.defs {
                    r(&mut d.props);
                    if let Some(g) = &mut d.group {
                        g.children.iter_mut().for_each(|b| r(&mut b.props));
                    }
                }
                p.bodies.iter_mut().for_each(|b| r(&mut b.props));
            }
        }
    }
}

impl Document {
    /// The definitions of the properties, by key.
    pub fn prop_defs(&self) -> &BTreeMap<String, PropDef> {
        &self.prop_defs
    }

    /// The kind of the entity `target`.
    pub fn prop_kind(&self, target: &PropTarget) -> Result<EntityKind> {
        self.props(target)?;
        Ok(match target {
            PropTarget::Component(id) if *id == self.root => EntityKind::Model,
            PropTarget::Component(id) => match self.component(*id)?.kind {
                ComponentKind::Part(_) => EntityKind::Part,
                ComponentKind::Assembly(_) => EntityKind::Assembly,
            },
            PropTarget::Occurrence { .. } => EntityKind::Occurrence,
            PropTarget::BodyDef { .. } => EntityKind::BodyDef,
            PropTarget::Body { .. } => EntityKind::Body,
            PropTarget::Material(_) => EntityKind::Material,
            PropTarget::Guide { .. } => EntityKind::Guide,
        })
    }

    /// The properties of the entity `target`.
    pub fn props(&self, target: &PropTarget) -> Result<&Props> {
        let missing = |id: Id| Error::NotFound(id);
        Ok(match target {
            PropTarget::Component(id) => &self.component(*id)?.props,
            PropTarget::Occurrence { context, occurrence } => {
                &self.assembly(*context)?.occurrences().iter().find(|o| o.id == *occurrence).ok_or(missing(*occurrence))?.props
            }
            PropTarget::BodyDef { part, def } => &self.part(*part)?.def(*def).ok_or(missing(*def))?.props,
            PropTarget::Body { part, body } => &self.part(*part)?.any_body(*body).ok_or(missing(*body))?.props,
            PropTarget::Material(id) => &self.materials.get(id).ok_or(missing(*id))?.props,
            PropTarget::Guide { component, guide } => {
                &self.component(*component)?.guides.iter().find(|g| g.id == *guide).ok_or(missing(*guide))?.props
            }
        })
    }

    fn props_mut(&mut self, target: &PropTarget) -> Result<&mut Props> {
        let missing = |id: Id| Error::NotFound(id);
        Ok(match target {
            PropTarget::Component(id) => &mut self.component_mut(*id)?.props,
            PropTarget::Occurrence { context, occurrence } => {
                &mut self.assembly_mut(*context)?.occurrence_mut(*occurrence).ok_or(missing(*occurrence))?.props
            }
            PropTarget::BodyDef { part, def } => {
                &mut self.part_mut(*part)?.defs.iter_mut().find(|d| d.id == *def).ok_or(missing(*def))?.props
            }
            PropTarget::Body { part, body } => &mut self.part_mut(*part)?.any_body_mut(*body).ok_or(missing(*body))?.props,
            PropTarget::Material(id) => &mut self.materials.get_mut(id).ok_or(missing(*id))?.props,
            PropTarget::Guide { component, guide } => {
                &mut self.component_mut(*component)?.guides.iter_mut().find(|g| g.id == *guide).ok_or(missing(*guide))?.props
            }
        })
    }

    /// Every entity's properties, with its kind.
    pub(crate) fn each_props(&self) -> impl Iterator<Item = (EntityKind, &Props)> {
        let components = self.components.values().flat_map(|c| c.each_props(c.id == self.root));
        components.chain(self.materials.values().map(|m| (EntityKind::Material, &m.props)))
    }

    /// Does `value` of `key` fit where it is (an entity of `kind`): well formed, and as its
    /// definition says if it has one?
    fn fits(&self, kind: EntityKind, key: &str, value: &Value) -> bool {
        value.is_valid() && self.prop_defs.get(key).is_none_or(|d| d.accepts(value) && d.applies(kind))
    }

    /// `Operation::SetProps`: each key of `props` set, or removed (`None`).
    pub(crate) fn set_props(&mut self, target: &PropTarget, props: &BTreeMap<String, Option<Value>>) -> Result<()> {
        let kind = self.prop_kind(target)?;
        for (key, value) in props {
            if !is_key(key) {
                return Err(Error::Invalid(Refusal::BadPropKey));
            }
            if let Some(v) = value
                && !self.fits(kind, key, v)
            {
                return Err(Error::Invalid(Refusal::BadPropValue));
            }
        }
        let p = self.props_mut(target)?;
        for (key, value) in props {
            p.set(key, value.clone());
        }
        Ok(())
    }

    /// `Operation::SetPropDef`: `key` defined as `def`, or not any more (`None`: its values
    /// kept as they are). Refused when values of it do not fit the new definition.
    pub(crate) fn set_prop_def(&mut self, key: &str, def: Option<&PropDef>) -> Result<()> {
        if !is_key(key) {
            return Err(Error::Invalid(Refusal::BadPropKey));
        }
        match def {
            Some(d) => {
                if !d.is_valid() {
                    return Err(Error::Invalid(Refusal::BadPropDef));
                }
                let fits = |(kind, p): (EntityKind, &Props)| p.get(key).is_none_or(|v| d.accepts(v) && d.applies(kind));
                if !self.each_props().all(fits) {
                    return Err(Error::Invalid(Refusal::PropDefContradicted));
                }
                self.prop_defs.insert(key.to_owned(), d.clone());
            }
            None => {
                self.prop_defs.remove(key);
            }
        }
        Ok(())
    }

    /// The properties well formed (a file's check): keys, values, definitions, every value as
    /// its definition says.
    pub(crate) fn check_props(&self) -> Result<()> {
        let bad = |key: &str| Error::File(FileError::BadProp(key.to_owned()));
        if let Some(k) = self.prop_defs.iter().find(|(k, d)| !is_key(k) || !d.is_valid()).map(|(k, _)| k) {
            return Err(bad(k));
        }
        for (kind, props) in self.each_props() {
            if let Some(k) = props.iter().find(|(k, v)| !is_key(k) || !self.fits(kind, k, v)).map(|(k, _)| k) {
                return Err(bad(k));
            }
        }
        Ok(())
    }

    /// Takes in the definitions `defs` of properties coming from elsewhere (a paste, an
    /// import), with `values` — the properties coming with them and the kinds of their
    /// entities: a key defined here otherwise, or whose value does not fit the definition here,
    /// is renamed (`key_2`, `key_3`…), its definition taken under the new key. The keys renamed,
    /// old → new.
    pub(crate) fn adopt_prop_defs<'a>(
        &mut self,
        defs: &BTreeMap<String, PropDef>,
        values: impl Iterator<Item = (EntityKind, &'a Props)>,
    ) -> BTreeMap<String, String> {
        let mut conflicts: BTreeSet<&str> = BTreeSet::new();
        for (key, def) in defs {
            if self.prop_defs.get(key).is_some_and(|here| here != def) {
                conflicts.insert(key);
            }
        }
        for (kind, props) in values {
            for (key, v) in props.iter() {
                if !defs.contains_key(key) && !self.fits(kind, key, v) {
                    conflicts.insert(key);
                }
            }
        }
        let mut renamed = BTreeMap::new();
        for key in conflicts {
            let free = |k: &String| !self.prop_defs.contains_key(k) && !defs.contains_key(k);
            let new = (2..).map(|n| format!("{key}_{n}")).find(free).expect("a free key");
            renamed.insert(key.to_owned(), new);
        }
        for (key, def) in defs {
            let key = renamed.get(key).unwrap_or(key);
            self.prop_defs.entry(key.clone()).or_insert_with(|| def.clone());
        }
        renamed
    }

    /// This document, every properties' keys of `renamed` (old → new) renamed, its
    /// definitions with them.
    pub(crate) fn with_props_renamed(&self, renamed: &BTreeMap<String, String>) -> Document {
        let mut out = self.clone();
        for c in out.components.values_mut() {
            if c.each_props(false).iter().any(|(_, p)| p.has_any(renamed)) {
                Arc::make_mut(c).rename_props(renamed);
            }
        }
        for m in out.materials.values_mut() {
            m.props = m.props.renamed(renamed);
        }
        out.prop_defs = self.prop_defs.iter().map(|(k, d)| (renamed.get(k).unwrap_or(k).clone(), d.clone())).collect();
        out
    }
}

impl Document {
    /// [`Document::adopt_prop_defs`] for bodies pasted: the fragment as it is, or its keys
    /// renamed; the keys renamed.
    pub(crate) fn adopt_fragment_props<'f>(
        &mut self,
        fragment: &'f crate::BodyFragment,
    ) -> (std::borrow::Cow<'f, crate::BodyFragment>, BTreeMap<String, String>) {
        let renamed = self.adopt_prop_defs(&fragment.prop_defs, fragment.each_props());
        match renamed.is_empty() {
            true => (std::borrow::Cow::Borrowed(fragment), renamed),
            false => (std::borrow::Cow::Owned(fragment.with_props_renamed(&renamed)), renamed),
        }
    }
}

// ---- shown and typed (docs/model/properties.md#values) ----------------------------------------

use skraft_i18n::t;

use crate::{Units, measure};

/// The currency a money typed without one takes, when nothing else gives it.
pub const DEFAULT_CURRENCY: &str = "EUR";

/// `x` without needless decimals (at most `decimals`), as the locale writes it.
fn plain(x: f64, decimals: usize) -> String {
    let text = format!("{x:.decimals$}");
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text };
    skraft_i18n::number(if text == "-0" { "0" } else { &text })
}

impl Value {
    /// As shown: in the document's units (`18 mm`, `~ 12,5 cm`, `0,25 m²`, `45°`), a money
    /// with its currency (`12,5 EUR`), a colour in hexadecimal, a list joined.
    pub fn shown(&self, units: &Units) -> String {
        self.text(units, false)
    }

    /// As edited: the same, to the precision of the lengths shown, never marked rounded.
    pub fn exact(&self, units: &Units) -> String {
        self.text(units, true)
    }

    fn text(&self, units: &Units, exact: bool) -> String {
        let unit = units.length.mm();
        let digits = if exact { 9 } else { 6 };
        let symbol = units.length.symbol();
        match self {
            Value::Bool(b) => if *b { t!("property.yes") } else { t!("property.no") },
            Value::Int(n) => n.to_string(),
            Value::Real(x) => plain(*x, digits),
            Value::Text(s) | Value::Choice(s) => s.clone(),
            Value::Length(mm) if exact => format!("{}{}", units.exact_number(*mm), units.suffix()),
            Value::Length(mm) => units.format(*mm),
            Value::Area(a) => format!("{} {symbol}²", plain(a / (unit * unit), digits)),
            Value::Volume(v) => format!("{} {symbol}³", plain(v / (unit * unit * unit), digits)),
            Value::Angle(r) => format!("{}°", plain(r.to_degrees(), digits)),
            Value::Mass(kg) => format!("{} kg", plain(*kg, digits)),
            Value::Money { amount, currency } => format!("{} {currency}", plain(*amount, if exact { digits } else { 2 })),
            Value::Color([r, g, b]) => format!("#{r:02X}{g:02X}{b:02X}"),
            Value::List(l) => measure::join(l.iter().map(|v| v.text(units, exact))),
        }
    }

    /// A value of `kind` read from `text` as a field types it (docs/model/properties.md): numbers
    /// as the measurements box reads them, in the document's units, `@` being `current`'s; a
    /// money's currency after its amount (else `current`'s, else [`DEFAULT_CURRENCY`]); a
    /// colour `#RRGGBB`; a list `;`-separated. None when it does not read (a choice is checked
    /// by its definition, not here).
    pub fn typed(kind: &ValueKind, text: &str, units: &Units, current: Option<&Value>) -> Option<Value> {
        let text = text.trim();
        let unit = units.length.mm();
        let at = |f: &dyn Fn(&Value) -> Option<f64>| current.and_then(f).unwrap_or(0.0);
        let number = |current: f64| measure::number(text, current).ok();
        Some(match kind {
            ValueKind::Bool => match text.to_lowercase().as_str() {
                "1" | "true" => Value::Bool(true),
                "0" | "false" => Value::Bool(false),
                t if t == t!("property.yes").to_lowercase() => Value::Bool(true),
                t if t == t!("property.no").to_lowercase() => Value::Bool(false),
                _ => return None,
            },
            ValueKind::Int => {
                let n = number(at(&|v| if let Value::Int(n) = v { Some(*n as f64) } else { None }))?;
                (n.fract() == 0.0 && n.abs() < 9.0e15).then_some(Value::Int(n as i64))?
            }
            ValueKind::Real => Value::Real(number(at(&|v| if let Value::Real(x) = v { Some(*x) } else { None }))?),
            ValueKind::Text => Value::Text(text.to_owned()),
            ValueKind::Choice => (!text.is_empty()).then(|| Value::Choice(text.to_owned()))?,
            ValueKind::Length => {
                let current = at(&|v| if let Value::Length(x) = v { Some(*x) } else { None });
                Value::Length(measure::coordinate(text, units, current).ok()?)
            }
            ValueKind::Area => {
                let f = unit * unit;
                Value::Area(number(at(&|v| if let Value::Area(x) = v { Some(*x / f) } else { None }))? * f)
            }
            ValueKind::Volume => {
                let f = unit * unit * unit;
                Value::Volume(number(at(&|v| if let Value::Volume(x) = v { Some(*x / f) } else { None }))? * f)
            }
            ValueKind::Angle => {
                let current = at(&|v| if let Value::Angle(r) = v { Some(r.to_degrees()) } else { None });
                Value::Angle(measure::angle(text, current).ok()?.to_radians())
            }
            ValueKind::Mass => {
                let text = text.strip_suffix("kg").unwrap_or(text);
                Value::Mass(measure::number(text, at(&|v| if let Value::Mass(x) = v { Some(*x) } else { None })).ok()?)
            }
            ValueKind::Money => {
                let (amount, currency) = match text.rfind(|c: char| !c.is_alphabetic() && c != '€' && c != '$') {
                    Some(i) if i + 1 < text.len() => (&text[..=i], text[i + 1..].trim()),
                    _ if text.chars().all(char::is_alphabetic) => return None,
                    _ => (text, ""),
                };
                let currency = match currency {
                    "" => match current {
                        Some(Value::Money { currency, .. }) => currency.clone(),
                        _ => DEFAULT_CURRENCY.to_owned(),
                    },
                    "€" => "EUR".to_owned(),
                    "$" => "USD".to_owned(),
                    c => c.to_uppercase(),
                };
                let current = at(&|v| if let Value::Money { amount, .. } = v { Some(*amount) } else { None });
                Value::Money { amount: measure::number(amount, current).ok()?, currency }
            }
            ValueKind::Color => {
                let hex = text.strip_prefix('#').unwrap_or(text);
                let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
                (hex.len() == 6).then_some(())?;
                Value::Color([byte(0)?, byte(2)?, byte(4)?])
            }
            ValueKind::List(k) => {
                if text.is_empty() {
                    return Some(Value::List(Vec::new()));
                }
                let items = measure::list(text);
                Value::List(items.iter().map(|i| Value::typed(k, &i.value, units, None)).collect::<Option<_>>()?)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LengthUnit;

    #[test]
    fn values_are_shown_and_typed_in_the_document_s_units() {
        let cm = Units { length: LengthUnit::Centimeter, ..Default::default() };
        let mm = Units::default();
        let typed = |k: ValueKind, s: &str, u: &Units| Value::typed(&k, s, u, None);
        assert_eq!(typed(ValueKind::Length, "2", &cm), Some(Value::Length(20.0)));
        assert_eq!(typed(ValueKind::Length, "18mm", &cm), Some(Value::Length(18.0)));
        assert_eq!(Value::Length(20.0).exact(&cm), Value::typed(&ValueKind::Length, "2", &cm, None).unwrap().exact(&cm));
        assert_eq!(typed(ValueKind::Area, "2", &cm), Some(Value::Area(200.0)));
        assert!(Value::Area(200.0).shown(&cm).ends_with("cm²"));
        assert_eq!(typed(ValueKind::Volume, "1", &cm), Some(Value::Volume(1000.0)));
        assert_eq!(typed(ValueKind::Angle, "90", &mm), Some(Value::Angle(std::f64::consts::FRAC_PI_2)));
        assert_eq!(typed(ValueKind::Int, "3*4", &mm), Some(Value::Int(12)));
        assert_eq!(typed(ValueKind::Int, "1/2", &mm), None);
        assert_eq!(typed(ValueKind::Mass, "2.5 kg", &mm), Some(Value::Mass(2.5)));
        let money = |a: f64, c: &str| Some(Value::Money { amount: a, currency: c.into() });
        assert_eq!(typed(ValueKind::Money, "12.5", &mm), money(12.5, "EUR"));
        assert_eq!(typed(ValueKind::Money, "12.5 usd", &mm), money(12.5, "USD"));
        assert_eq!(typed(ValueKind::Money, "12 €", &mm), money(12.0, "EUR"));
        assert_eq!(Value::typed(&ValueKind::Money, "@*2", &mm, money(5.0, "CHF").as_ref()), money(10.0, "CHF"));
        assert_eq!(typed(ValueKind::Money, "EUR", &mm), None);
        assert_eq!(typed(ValueKind::Color, "#C89664", &mm), Some(Value::Color([200, 150, 100])));
        assert_eq!(Value::Color([200, 150, 100]).shown(&mm), "#C89664");
        assert_eq!(typed(ValueKind::Color, "C8966", &mm), None);
        assert_eq!(
            typed(ValueKind::List(Box::new(ValueKind::Length)), "18; 2=", &mm),
            Some(Value::List(vec![Value::Length(18.0), Value::Length(2.0), Value::Length(2.0)]))
        );
        assert_eq!(typed(ValueKind::Length, "abc", &mm), None);
        // `@` is the current value.
        assert_eq!(Value::typed(&ValueKind::Length, "@+2", &mm, Some(&Value::Length(18.0))), Some(Value::Length(20.0)));
    }
}

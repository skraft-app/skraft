//! The settings a document carries in its file (docs/ARCHITECTURE.md §8): how it is displayed
//! and the units it is measured in. The app's preferences give the values of new documents.
//!
//! Every field has a default, so a file written before a setting existed still loads.

use glam::DVec3;
use serde::{Deserialize, Serialize};
use solidsketch_i18n::t;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub display: Display,
    pub units: Units,
}

/// The display style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub grid: bool,
    pub axes: bool,
    /// The outline of curved faces (§7 quater).
    pub silhouettes: bool,
    /// The section plane, if one was placed; shown while `section_active`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<Section>,
    pub section_active: bool,
    /// The construction guides (§6 quinquies): drawn, inferred on and picked only when shown.
    pub guides: bool,
    /// While a part is open or an assembly edited, the rest of the model is hidden (neither
    /// drawn, picked nor inferred on) instead of faded.
    pub hide_rest_of_model: bool,
    /// The occurrences hidden by the user are drawn faded, picked and inferred on, as if
    /// shown (to find them, show them again).
    pub show_hidden: bool,
}

impl Default for Display {
    fn default() -> Self {
        Display {
            grid: false,
            axes: true,
            silhouettes: true,
            section: None,
            section_active: false,
            guides: true,
            hide_rest_of_model: false,
            show_hidden: false,
        }
    }
}

impl Display {
    /// The section plane, when it is shown.
    pub fn active_section(&self) -> Option<Section> {
        self.section.filter(|_| self.section_active)
    }
}

/// A section plane, in the model's (root) axes: what lies on the side `normal` points to is
/// cut away from the view (§7 quater).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub origin: DVec3,
    /// Unit.
    pub normal: DVec3,
}

impl Section {
    /// Is the world point `p` cut away?
    pub fn cuts(&self, p: DVec3) -> bool {
        (p - self.origin).dot(self.normal) > 0.0
    }
}

/// How lengths are typed and shown. Lengths are always stored in millimetres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Units {
    /// The unit of a bare number and of displayed lengths.
    pub length: LengthUnit,
    /// Decimals shown (trailing zeros dropped).
    pub decimals: u8,
    /// The lengths shown are followed by the unit's symbol (`12,5 mm`, else `12,5`).
    pub symbol: bool,
}

impl Default for Units {
    fn default() -> Self {
        Units { length: LengthUnit::Millimeter, decimals: 0, symbol: false }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LengthUnit {
    #[default]
    Millimeter,
    Centimeter,
    Meter,
    Inch,
    Foot,
}

impl LengthUnit {
    pub const ALL: [LengthUnit; 5] =
        [LengthUnit::Millimeter, LengthUnit::Centimeter, LengthUnit::Meter, LengthUnit::Inch, LengthUnit::Foot];

    /// Millimetres in one unit.
    pub fn mm(self) -> f64 {
        match self {
            LengthUnit::Millimeter => 1.0,
            LengthUnit::Centimeter => 10.0,
            LengthUnit::Meter => 1000.0,
            LengthUnit::Inch => 25.4,
            LengthUnit::Foot => 304.8,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            LengthUnit::Millimeter => "mm",
            LengthUnit::Centimeter => "cm",
            LengthUnit::Meter => "m",
            LengthUnit::Inch => "in",
            LengthUnit::Foot => "ft",
        }
    }

    pub fn name(self) -> String {
        match self {
            LengthUnit::Millimeter => t!("unit.millimeter"),
            LengthUnit::Centimeter => t!("unit.centimeter"),
            LengthUnit::Meter => t!("unit.meter"),
            LengthUnit::Inch => t!("unit.inch"),
            LengthUnit::Foot => t!("unit.foot"),
        }
    }
}

impl Units {
    /// A length in mm, for display in these units, as the locale writes it: `12,5 cm`
    /// (`12,5` when the symbol is not shown).
    pub fn format(&self, mm: f64) -> String {
        format!("{}{}", self.number(mm), self.suffix())
    }

    /// What follows a number shown in these units: ` cm`, or nothing when the symbol is not
    /// shown.
    pub fn suffix(&self) -> String {
        if self.symbol { format!(" {}", self.length.symbol()) } else { String::new() }
    }

    /// The number alone, in these units, without needless decimals: `12,5`.
    pub fn number(&self, mm: f64) -> String {
        let p = 10f64.powi(self.decimals.min(9) as i32);
        let r = (mm / self.length.mm() * p).round() / p;
        let r = if r == 0.0 { 0.0 } else { r }; // no "-0"
        solidsketch_i18n::number(&r.to_string())
    }
}

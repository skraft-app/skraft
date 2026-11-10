//! The colour tokens SolidSketch draws with (docs/ARCHITECTURE.md §7 quater): named colours,
//! in linear RGBA, in a light and a dark theme. No UI toolkit: `render` gets them through its
//! frame, `tools` uses the axis colours, the app turns the rest into egui's visuals.
//!
//! The viewport's background is not a token: it is a setting of the document (its style).

use serde::{Deserialize, Serialize};

/// A colour, linear RGB, `a` its opacity (or how much of it is mixed in, for a tint).
pub type Rgba = [f32; 4];

/// The theme a user picks (`System` follows the operating system).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Choice {
    #[default]
    System,
    Light,
    Dark,
}

/// A theme as drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

/// How a highlighted part is drawn again: its faces and its edges tinted (`a`: how much).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkColors {
    pub face: Rgba,
    pub edge: Rgba,
}

/// Every token.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub mode: Mode,
    /// The front of a face with no material, and the back of every face (SketchUp's
    /// convention: warm white, blue-grey).
    pub face_front: Rgba,
    pub face_back: Rgba,
    /// B-rep edges and silhouettes.
    pub edge: Rgba,
    /// How much of the background covers what is outside the context being edited.
    pub fade: f32,
    /// How much darker than its part a section cap is (0: the same colour).
    pub cap_shade: f32,
    pub selected: MarkColors,
    pub hover: MarkColors,
    /// Picked by a tool (the boolean's target).
    pub picked: MarkColors,
    /// The selection where the move tool would put it.
    pub ghost: MarkColors,
    /// The other occurrences a change reaches.
    pub related: MarkColors,
    pub marquee_fill: Rgba,
    pub marquee_stroke: Rgba,
    /// Inference markers (§6 ter).
    pub endpoint: Rgba,
    pub midpoint: Rgba,
    pub center: Rgba,
    pub origin: Rgba,
    pub on_edge: Rgba,
    pub on_face: Rgba,
    pub tracked: Rgba,
    pub marker_outline: Rgba,
    pub tooltip_fill: Rgba,
    pub tooltip_text: Rgba,
    pub tooltip_border: Rgba,
}

/// The axes' colours, the same in every theme (red, green, blue: x, y, z).
pub const AXIS_X: Rgba = [0.85, 0.05, 0.05, 1.0];
pub const AXIS_Y: Rgba = [0.05, 0.6, 0.05, 1.0];
pub const AXIS_Z: Rgba = [0.05, 0.1, 0.85, 1.0];
/// A line along no axis (parallel, perpendicular, tangent…).
pub const FREE_LINE: Rgba = [0.8, 0.05, 0.75, 1.0];
/// Tools' preview lines.
pub const GUIDE_INK: Rgba = [0.05, 0.05, 0.08, 1.0];
/// What a tool picked (orange).
pub const PICK: Rgba = [0.85, 0.35, 0.0, 1.0];

impl Theme {
    pub fn of(mode: Mode) -> Self {
        match mode {
            Mode::Light => Self::light(),
            Mode::Dark => Self::dark(),
        }
    }

    pub fn light() -> Self {
        Theme {
            mode: Mode::Light,
            face_front: [0.92, 0.91, 0.87, 1.0],
            face_back: [0.55, 0.60, 0.72, 1.0],
            edge: [0.02, 0.02, 0.03, 1.0],
            fade: 0.7,
            cap_shade: 0.25,
            selected: MarkColors { face: [0.15, 0.35, 1.0, 0.35], edge: [0.05, 0.2, 1.0, 1.0] },
            hover: MarkColors { face: [0.2, 0.45, 1.0, 0.12], edge: [0.1, 0.35, 1.0, 0.7] },
            picked: MarkColors { face: [1.0, 0.45, 0.05, 0.4], edge: [0.9, 0.3, 0.0, 1.0] },
            ghost: MarkColors { face: [0.3, 0.5, 1.0, 0.3], edge: [0.05, 0.2, 1.0, 1.0] },
            related: MarkColors { face: [1.0, 0.55, 0.15, 0.2], edge: [0.95, 0.45, 0.1, 0.6] },
            marquee_fill: srgba(60, 110, 230, 25),
            marquee_stroke: srgb(40, 80, 200),
            endpoint: srgb(40, 190, 60),
            midpoint: srgb(40, 200, 220),
            center: srgb(170, 70, 220),
            origin: srgb(240, 180, 30),
            on_edge: srgb(220, 40, 40),
            on_face: srgb(60, 90, 230),
            tracked: srgb(150, 40, 160),
            marker_outline: srgb(30, 30, 30),
            tooltip_fill: srgb(255, 250, 215),
            tooltip_text: srgb(20, 20, 20),
            tooltip_border: srgb(120, 120, 120),
        }
    }

    /// The dark theme: the interface and what is drawn over the model. The model keeps the
    /// document's style (its background) and its faces' colours.
    pub fn dark() -> Self {
        Theme {
            mode: Mode::Dark,
            selected: MarkColors { face: [0.2, 0.45, 1.0, 0.4], edge: [0.25, 0.5, 1.0, 1.0] },
            marquee_fill: srgba(90, 140, 255, 35),
            marquee_stroke: srgb(120, 160, 255),
            marker_outline: srgb(235, 235, 235),
            tooltip_fill: srgb(45, 46, 52),
            tooltip_text: srgb(235, 235, 235),
            tooltip_border: srgb(110, 110, 120),
            ..Self::light()
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

/// An sRGB byte, to linear.
pub fn linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// A linear value, to an sRGB byte.
pub fn srgb_byte(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// An opaque sRGB colour, as a token.
pub fn srgb(r: u8, g: u8, b: u8) -> Rgba {
    [linear(r), linear(g), linear(b), 1.0]
}

/// An sRGB colour and its opacity (0–255), as a token.
pub fn srgba(r: u8, g: u8, b: u8, a: u8) -> Rgba {
    [linear(r), linear(g), linear(b), a as f32 / 255.0]
}

/// A token, as sRGB bytes and its opacity as a byte.
pub fn to_srgba8(c: Rgba) -> [u8; 4] {
    [srgb_byte(c[0]), srgb_byte(c[1]), srgb_byte(c[2]), (c[3].clamp(0.0, 1.0) * 255.0).round() as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_round_trips() {
        for c in [0u8, 1, 10, 60, 128, 200, 255] {
            assert_eq!(srgb_byte(linear(c)), c);
        }
        assert_eq!(to_srgba8(srgba(60, 110, 230, 25)), [60, 110, 230, 25]);
    }

    #[test]
    fn the_dark_theme_keeps_the_model_colours() {
        let (l, d) = (Theme::light(), Theme::dark());
        assert_eq!((l.face_front, l.face_back, l.edge), (d.face_front, d.face_back, d.edge));
        assert_ne!(l.tooltip_fill, d.tooltip_fill);
        assert_eq!(Theme::of(Mode::Dark).mode, Mode::Dark);
    }
}

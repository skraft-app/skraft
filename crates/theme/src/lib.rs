//! The colour tokens SolidSketch draws with (docs/ARCHITECTURE.md §7 quater): named colours,
//! in linear RGBA, in a light and a dark theme. No UI toolkit: `render` gets them through its
//! frame, `tools` uses the axis colours, the app turns the rest — the interface's tokens
//! ([`UiTokens`]) among them — into egui's visuals.
//!
//! The 3D view's background is a token of the palette ([`UiTokens::view`]), not a setting of
//! the document: a file never sets how the app looks.

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

/// The interface's colours (§7 quater), each in a light and a dark variant: the theme
/// [`Choice`] picks the variant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Palette {
    /// Neutral greys, a blue accent (SolidCraft's).
    Neutral,
    /// Warm greys, an amber accent.
    #[default]
    Warm,
    /// Cool, deeper greys, a teal accent.
    Graphite,
}

impl Palette {
    pub const ALL: [Palette; 3] = [Palette::Neutral, Palette::Warm, Palette::Graphite];
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

/// The interface's tokens: the colours and sizes of panels and widgets, of one [`Palette`] in
/// one [`Mode`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTokens {
    /// Panels and windows.
    pub panel: Rgba,
    /// Buttons at rest, headers, a floating window's title bar.
    pub panel_header: Rgba,
    /// Text fields and other sunken areas.
    pub field: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    /// Secondary text (hints, disabled labels).
    pub text_dim: Rgba,
    /// A hovered widget's fill.
    pub hover: Rgba,
    /// The accent: selected text's outline, focused widgets.
    pub accent: Rgba,
    /// The accent as a fill: selected items, a pressed button.
    pub accent_soft: Rgba,
    /// Corner radii, in points: widgets, windows and menus.
    pub radius_sm: u8,
    pub radius_md: u8,
    /// Space between widgets (x, y), and inside a button, in points.
    pub item_spacing: [f32; 2],
    pub button_padding: [f32; 2],
    /// The title bar: dark in both modes (as SolidCraft's application bar), its text light.
    pub title_bar: TitleBarTokens,
    /// The 3D view's background, and the text drawn on it (the shown file tab, which opens onto
    /// the view). Light in both modes, dimmed in the dark one: the model's colours, the
    /// tools' inks and the axes are drawn for a light ground.
    pub view: Rgba,
    pub view_text: Rgba,
}

/// The title bar's colours: a dark band whatever the mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleBarTokens {
    pub ground: Rgba,
    pub text: Rgba,
    pub text_dim: Rgba,
    /// Its separators.
    pub border: Rgba,
    /// A lit button or the shown tab: a surface lifted from the bar.
    pub lit: Rgba,
}

impl UiTokens {
    pub fn of(palette: Palette, mode: Mode) -> Self {
        match (palette, mode) {
            (Palette::Neutral, Mode::Light) => Self::light(),
            (Palette::Neutral, Mode::Dark) => Self::dark(),
            (Palette::Warm, Mode::Light) => Self::warm_light(),
            (Palette::Warm, Mode::Dark) => Self::warm_dark(),
            (Palette::Graphite, Mode::Light) => Self::graphite_light(),
            (Palette::Graphite, Mode::Dark) => Self::graphite_dark(),
        }
    }

    /// The neutral palette, light: SolidCraft's (`crates/theme/src/palettes.rs` in
    /// tangency-labs/solidcraft, Apache-2.0).
    pub fn light() -> Self {
        UiTokens {
            panel: srgb(250, 251, 252),
            panel_header: srgb(236, 238, 242),
            field: srgb(255, 255, 255),
            border: srgb(208, 212, 220),
            text: srgb(38, 42, 50),
            text_dim: srgb(110, 116, 128),
            hover: srgb(226, 234, 246),
            accent: srgb(38, 120, 218),
            accent_soft: srgb(214, 230, 250),
            radius_sm: 4,
            radius_md: 6,
            item_spacing: [6.0, 4.0],
            button_padding: [6.0, 3.0],
            // SolidCraft's application bar (`app_bar`, `app_bar_text`).
            title_bar: TitleBarTokens {
                ground: srgb(52, 56, 64),
                text: srgb(230, 234, 242),
                text_dim: srgb(160, 168, 182),
                border: srgb(84, 90, 102),
                lit: srgb(68, 76, 92),
            },
            view: srgb(246, 247, 249),
            view_text: srgb(38, 42, 50),
        }
    }

    /// The neutral palette, dark (SolidCraft's).
    pub fn dark() -> Self {
        UiTokens {
            panel: srgb(39, 43, 52),
            panel_header: srgb(47, 52, 62),
            field: srgb(30, 34, 41),
            border: srgb(63, 69, 82),
            text: srgb(222, 226, 232),
            text_dim: srgb(148, 155, 168),
            hover: srgb(54, 61, 74),
            accent: srgb(58, 136, 238),
            accent_soft: srgb(42, 70, 110),
            title_bar: TitleBarTokens {
                ground: srgb(28, 31, 38),
                border: srgb(56, 62, 74),
                lit: srgb(54, 61, 74),
                ..Self::light().title_bar
            },
            view: srgb(198, 203, 211),
            ..Self::light()
        }
    }

    pub fn warm_light() -> Self {
        UiTokens {
            panel: srgb(251, 250, 247),
            panel_header: srgb(240, 236, 228),
            field: srgb(255, 255, 255),
            border: srgb(221, 213, 200),
            text: srgb(47, 42, 36),
            text_dim: srgb(118, 108, 95),
            hover: srgb(243, 232, 214),
            accent: srgb(184, 106, 30),
            accent_soft: srgb(246, 227, 203),
            title_bar: TitleBarTokens {
                ground: srgb(58, 52, 46),
                text: srgb(240, 233, 222),
                text_dim: srgb(176, 164, 148),
                border: srgb(92, 83, 73),
                lit: srgb(82, 73, 63),
            },
            view: srgb(245, 243, 238),
            view_text: srgb(47, 42, 36),
            ..Self::light()
        }
    }

    pub fn warm_dark() -> Self {
        UiTokens {
            panel: srgb(46, 42, 38),
            panel_header: srgb(55, 50, 45),
            field: srgb(36, 33, 30),
            border: srgb(74, 67, 59),
            text: srgb(236, 230, 221),
            text_dim: srgb(168, 157, 143),
            hover: srgb(66, 59, 52),
            accent: srgb(224, 149, 74),
            accent_soft: srgb(90, 65, 40),
            title_bar: TitleBarTokens {
                ground: srgb(32, 29, 26),
                border: srgb(62, 56, 49),
                lit: srgb(66, 59, 52),
                ..Self::warm_light().title_bar
            },
            view: srgb(206, 200, 190),
            view_text: Self::warm_light().view_text,
            ..Self::light()
        }
    }

    pub fn graphite_light() -> Self {
        UiTokens {
            panel: srgb(245, 246, 247),
            panel_header: srgb(228, 231, 234),
            field: srgb(255, 255, 255),
            border: srgb(197, 203, 209),
            text: srgb(31, 36, 40),
            text_dim: srgb(98, 108, 118),
            hover: srgb(220, 237, 236),
            accent: srgb(15, 138, 138),
            accent_soft: srgb(205, 235, 234),
            title_bar: TitleBarTokens {
                ground: srgb(40, 45, 49),
                text: srgb(228, 232, 235),
                text_dim: srgb(152, 162, 170),
                border: srgb(70, 77, 83),
                lit: srgb(62, 71, 78),
            },
            view: srgb(238, 240, 242),
            view_text: srgb(31, 36, 40),
            ..Self::light()
        }
    }

    pub fn graphite_dark() -> Self {
        UiTokens {
            panel: srgb(30, 33, 36),
            panel_header: srgb(39, 43, 47),
            field: srgb(22, 24, 26),
            border: srgb(58, 64, 70),
            text: srgb(227, 231, 234),
            text_dim: srgb(149, 158, 166),
            hover: srgb(47, 54, 59),
            accent: srgb(43, 179, 177),
            accent_soft: srgb(29, 74, 74),
            title_bar: TitleBarTokens {
                ground: srgb(20, 22, 24),
                border: srgb(46, 51, 56),
                lit: srgb(47, 54, 59),
                ..Self::graphite_light().title_bar
            },
            view: srgb(196, 202, 207),
            view_text: Self::graphite_light().view_text,
            ..Self::light()
        }
    }
}

/// Every token.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub mode: Mode,
    /// Panels and widgets.
    pub ui: UiTokens,
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
    /// How opaque an open part's blank is while its machinings show (§2.9).
    pub ghost_opacity: f32,
    /// How opaque the edges of what the eraser would take are (its faces: `ghost_opacity`).
    pub ghost_edge_opacity: f32,
    /// The tools of an open part's machinings (ghosts, a little more opaque than its blanks:
    /// `machining_opacity`), and those that failed to cut (opaque).
    pub machining: Rgba,
    pub machining_opacity: f32,
    pub machining_failed: Rgba,
    /// The selection: its edges only, its faces keeping their colour (face tint alpha 0).
    pub selected: MarkColors,
    pub hover: MarkColors,
    /// Picked by a tool (the boolean's target).
    pub picked: MarkColors,
    /// The selection where the move tool would put it.
    pub ghost: MarkColors,
    /// The other occurrences a change reaches.
    pub related: MarkColors,
    /// The dashed box around the assembly being edited.
    pub context_box: Rgba,
    /// Construction guides (§6 quinquies); those outside the active context are faded like
    /// the parts.
    pub guide: Rgba,
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
/// A deep green: a bright one has the dimmed view's luminance and fades into it.
pub const AXIS_Y: Rgba = [0.007, 0.262, 0.007, 1.0];
pub const AXIS_Z: Rgba = [0.05, 0.1, 0.85, 1.0];
/// A line along no axis (parallel, perpendicular, tangent…).
pub const FREE_LINE: Rgba = [0.8, 0.05, 0.75, 1.0];
/// Tools' preview lines.
pub const GUIDE_INK: Rgba = [0.05, 0.05, 0.08, 1.0];
/// What a tool picked (a deep orange, for the same reason as [`AXIS_Y`]).
pub const PICK: Rgba = [0.68, 0.127, 0.0, 1.0];

impl Theme {
    /// The theme of `mode`, its interface in `palette`.
    pub fn of(palette: Palette, mode: Mode) -> Self {
        let base = match mode {
            Mode::Light => Self::light(),
            Mode::Dark => Self::dark(),
        };
        Theme { ui: UiTokens::of(palette, mode), ..base }
    }

    pub fn light() -> Self {
        Theme {
            mode: Mode::Light,
            ui: UiTokens::light(),
            face_front: [0.92, 0.91, 0.87, 1.0],
            face_back: [0.55, 0.60, 0.72, 1.0],
            edge: [0.02, 0.02, 0.03, 1.0],
            fade: 0.7,
            cap_shade: 0.25,
            ghost_opacity: 0.15,
            ghost_edge_opacity: 0.35,
            machining: srgb(215, 75, 60),
            machining_opacity: 0.3,
            machining_failed: srgb(150, 20, 110),
            selected: MarkColors { face: [0.05, 0.2, 1.0, 0.0], edge: [0.005, 0.035, 1.0, 1.0] },
            hover: MarkColors { face: [0.07, 0.25, 1.0, 0.2], edge: [0.01, 0.1, 1.0, 0.75] },
            picked: MarkColors { face: [1.0, 0.3, 0.0, 0.55], edge: [0.79, 0.073, 0.0, 1.0] },
            ghost: MarkColors { face: [0.1, 0.25, 1.0, 0.4], edge: [0.005, 0.035, 1.0, 1.0] },
            related: MarkColors { face: [1.0, 0.4, 0.02, 0.28], edge: [0.89, 0.17, 0.01, 0.65] },
            context_box: [0.45, 0.45, 0.48, 1.0],
            guide: srgb(70, 70, 90),
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

    /// The dark theme: the interface and what is drawn over the model. The model keeps its
    /// faces' colours, the view a light ground ([`UiTokens::view`], dimmed).
    pub fn dark() -> Self {
        Theme {
            mode: Mode::Dark,
            ui: UiTokens::dark(),
            selected: MarkColors { face: [0.1, 0.3, 1.0, 0.0], edge: [0.05, 0.21, 1.0, 1.0] },
            marquee_fill: srgba(90, 140, 255, 35),
            marquee_stroke: srgb(120, 160, 255),
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

    /// Relative luminance of a linear colour (WCAG).
    fn luminance(c: Rgba) -> f32 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn every_palette_keeps_its_text_readable() {
        for palette in Palette::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                let t = UiTokens::of(palette, mode);
                for ground in [t.panel, t.panel_header, t.field, t.hover, t.accent_soft] {
                    assert!(contrast(t.text, ground) >= 7.0, "{palette:?} {mode:?} text");
                    assert!(contrast(t.text_dim, ground) >= 3.0, "{palette:?} {mode:?} dim text");
                }
                let bar = t.title_bar;
                for ground in [bar.ground, bar.lit] {
                    assert!(contrast(bar.text, ground) >= 7.0, "{palette:?} {mode:?} title bar text");
                    assert!(contrast(bar.text_dim, ground) >= 3.0, "{palette:?} {mode:?} title bar dim text");
                }
                assert!(luminance(bar.ground) < 0.1, "{palette:?} {mode:?} title bar is dark");
                let dark = luminance(t.panel) < 0.2;
                assert_eq!(dark, mode == Mode::Dark, "{palette:?} {mode:?} is the mode it says");
            }
        }
    }

    #[test]
    fn the_view_keeps_the_inks_drawn_on_it_readable() {
        for palette in Palette::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                let (th, t) = (Theme::of(palette, mode), UiTokens::of(palette, mode));
                for (ink, what) in
                    [(GUIDE_INK, "tools' ink"), (th.edge, "edges"), (th.guide, "guides"), (th.marker_outline, "markers' outline")]
                {
                    assert!(contrast(ink, t.view) >= 4.0, "{palette:?} {mode:?} {what}");
                }
                assert!(contrast(t.view_text, t.view) >= 4.5, "{palette:?} {mode:?} the shown tab's text");
                // The axes and the tools' coloured lines: saturated, thin, told apart by their hue
                // too, but never of the view's luminance.
                for (ink, what) in [(AXIS_X, "x axis"), (AXIS_Y, "y axis"), (AXIS_Z, "z axis"), (FREE_LINE, "free line"), (PICK, "pick")] {
                    assert!(contrast(ink, t.view) >= 2.0, "{palette:?} {mode:?} {what}: {:.2}", contrast(ink, t.view));
                }
                // The view is not the panels: the cards floating over it stand out.
                assert!(contrast(t.view, t.panel) >= 1.02, "{palette:?} {mode:?} view against the panels");
            }
            let (light, dark) = (UiTokens::of(palette, Mode::Light), UiTokens::of(palette, Mode::Dark));
            assert!(luminance(dark.view) < luminance(light.view), "{palette:?}: dimmed in the dark mode");
        }
    }

    #[test]
    fn a_palette_changes_the_interface_only() {
        let (n, w) = (Theme::of(Palette::Neutral, Mode::Light), Theme::of(Palette::Warm, Mode::Light));
        assert_ne!(n.ui, w.ui);
        assert_eq!(Theme { ui: n.ui, ..w }, n);
    }

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
        assert_eq!(Theme::of(Palette::Neutral, Mode::Dark), d);
        assert_ne!(l.ui.panel, d.ui.panel);
        assert_eq!((l.ui.radius_sm, l.ui.item_spacing), (d.ui.radius_sm, d.ui.item_spacing));
    }
}

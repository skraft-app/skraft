//! « Capturer » (docs/ui/capture.md, decision 0060): a frame centred on the free part of the
//! view (what zoom extents frames), the user
//! navigating behind it; Enter (or the bar's button) asks the app for an image of what the frame
//! shows, rendered off screen at the size chosen. Its options are settings: the ratio, the
//! size, the background, the format, and what of the editing state the image keeps.

use glam::DVec2;
use skraft_i18n::t;

use super::{Command, Cursor, Cx, Key, LeftOut, OptionChoice, OptionValue, Preview, Tool, ToolInput, ToolKind, ToolOption};

/// The frame's proportions: free, or one of [`RATIOS`].
pub const RATIO: &str = "ratio";
/// The image's longer side, in pixels: one of [`SIZES`].
pub const SIZE: &str = "size";
/// The image's ground: [`THEME`], [`WHITE`] or [`TRANSPARENT`] (PNG only).
pub const BACKGROUND: &str = "background";
/// The file's format: [`PNG`] or [`JPEG`].
pub const FORMAT: &str = "format";
/// The toggles keeping in the image what the view shows of the editing: the selection, the
/// guides, the axes and the grid. Off, never set: an image of the model alone.
pub const KEEP_SELECTION: &str = "keep_selection";
pub const KEEP_GUIDES: &str = "keep_guides";
pub const KEEP_AXES: &str = "keep_axes";

pub const FREE: &str = "free";
/// The fixed ratios, by value, and their width over height.
pub const RATIOS: [(&str, f64); 6] = [
    ("16_9", 16.0 / 9.0),
    ("4_3", 4.0 / 3.0),
    ("3_2", 3.0 / 2.0),
    ("1_1", 1.0),
    ("a4_landscape", 297.0 / 210.0),
    ("a4_portrait", 210.0 / 297.0),
];
/// The longer sides offered (pixels), growing. Past 3840, the targets would weigh more than a
/// GPU readily gives (step 3's tiles, decision 0060).
pub const SIZES: [u32; 4] = [1280, 1920, 2560, 3840];
/// The size before any is chosen.
pub const DEFAULT_SIZE: u32 = 1920;

pub const THEME: &str = "theme";
pub const WHITE: &str = "white";
pub const TRANSPARENT: &str = "transparent";
pub const PNG: &str = "png";
pub const JPEG: &str = "jpeg";

/// The room left between the frame and the view's edges (points).
pub const MARGIN: f64 = 32.0;
/// The smallest half side a free frame is dragged to (points).
const MIN_HALF: f64 = 16.0;
/// How near a corner the pointer takes its handle (points).
pub const HANDLE_PX: f64 = 10.0;

/// The image's ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    /// The view's, as the theme paints it.
    Theme,
    White,
    /// None: each pixel's coverage kept (PNG only).
    Transparent,
}

/// The image file's format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    /// The file's extension.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }

    /// Its name in the save panel's filter.
    pub fn name(self) -> &'static str {
        match self {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
        }
    }
}

/// What the app renders and saves: the frame over the view (points, the view `view` large),
/// rendered `size` pixels large on `background`, written as `format`.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRequest {
    pub min: DVec2,
    pub max: DVec2,
    pub view: DVec2,
    pub size: [u32; 2],
    pub background: Background,
    pub format: ImageFormat,
}

/// The frame the app draws over the view (points), its corners' handles when it is free, the
/// one under the pointer or dragged, and the image's size written by it.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureFrame {
    pub min: DVec2,
    pub max: DVec2,
    pub handles: bool,
    pub hovered: Option<Corner>,
    pub label: String,
}

/// A corner of the frame, by its sides: right or left, below or above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corner {
    pub right: bool,
    pub below: bool,
}

#[derive(Default)]
pub struct CaptureTool {
    /// The free frame's half size (points) while a corner is pulled; else the document
    /// session's (`Cx::capture_free`), the area less its margins before any.
    free: Option<DVec2>,
    hover: Option<Corner>,
    dragging: Option<Corner>,
}

/// The frame's centre: the free area's.
fn center(cx: &Cx<'_>) -> DVec2 {
    (cx.free_area.0 + cx.free_area.1) / 2.0
}

/// The ratio chosen: none when free.
fn ratio_of(cx: &Cx<'_>) -> Option<f64> {
    let value = cx.choice(RATIO).unwrap_or("16_9");
    RATIOS.iter().find(|(v, _)| *v == value).map(|(_, r)| *r)
}

/// The longer side chosen (pixels).
fn size_of(cx: &Cx<'_>) -> u32 {
    cx.choice(SIZE).and_then(|v| v.parse().ok()).filter(|s| SIZES.contains(s)).unwrap_or(DEFAULT_SIZE)
}

fn format_of(cx: &Cx<'_>) -> ImageFormat {
    if cx.choice(FORMAT) == Some(JPEG) { ImageFormat::Jpeg } else { ImageFormat::Png }
}

/// The ground chosen, white for a transparent one in a JPEG (no alpha).
fn background_of(cx: &Cx<'_>) -> Background {
    match (cx.choice(BACKGROUND), format_of(cx)) {
        (Some(WHITE), _) | (Some(TRANSPARENT), ImageFormat::Jpeg) => Background::White,
        (Some(TRANSPARENT), ImageFormat::Png) => Background::Transparent,
        _ => Background::Theme,
    }
}

/// The frame in the area `(lo, hi)` of the view (points): centred on it, the largest of `ratio`
/// that keeps [`MARGIN`] from its edges; free, `free` half sizes (the area less its margins
/// without), kept inside them.
pub fn frame_in((lo, hi): (DVec2, DVec2), ratio: Option<f64>, free: Option<DVec2>) -> (DVec2, DVec2) {
    let room = (hi - lo - DVec2::splat(2.0 * MARGIN)).max(DVec2::splat(2.0 * MIN_HALF));
    let half = match ratio {
        Some(r) if room.x / room.y > r => DVec2::new(room.y * r, room.y) / 2.0,
        Some(r) => DVec2::new(room.x, room.x / r) / 2.0,
        None => free.unwrap_or(room / 2.0).clamp(DVec2::splat(MIN_HALF), room / 2.0),
    };
    let center = (lo + hi) / 2.0;
    (center - half, center + half)
}

/// The image's size for a frame `frame` large of `ratio`, its longer side `long` (pixels).
pub fn image_size(frame: DVec2, ratio: Option<f64>, long: u32) -> [u32; 2] {
    let r = ratio.unwrap_or(frame.x / frame.y.max(1e-9));
    let long = f64::from(long);
    let (w, h) = if r >= 1.0 { (long, long / r) } else { (long * r, long) };
    [(w.round() as u32).max(1), (h.round() as u32).max(1)]
}

impl CaptureTool {
    fn frame(&self, cx: &Cx<'_>) -> (DVec2, DVec2) {
        frame_in(cx.free_area, ratio_of(cx), self.free.or(cx.capture_free))
    }

    fn image_size(&self, cx: &Cx<'_>) -> [u32; 2] {
        let (min, max) = self.frame(cx);
        image_size(max - min, ratio_of(cx), size_of(cx))
    }

    /// The handle under `px`, when the frame is free.
    fn corner_at(&self, px: DVec2, cx: &Cx<'_>) -> Option<Corner> {
        ratio_of(cx).is_none().then_some(())?;
        let (min, max) = self.frame(cx);
        [false, true].into_iter().flat_map(|right| [false, true].map(|below| Corner { right, below })).find(|c| {
            let at = DVec2::new(if c.right { max.x } else { min.x }, if c.below { max.y } else { min.y });
            (px - at).abs().max_element() <= HANDLE_PX
        })
    }

    fn request(&self, cx: &Cx<'_>) -> CaptureRequest {
        let (min, max) = self.frame(cx);
        CaptureRequest { min, max, view: cx.size, size: self.image_size(cx), background: background_of(cx), format: format_of(cx) }
    }
}

impl Tool for CaptureTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Capture
    }

    fn cursor(&self, _cx: &Cx<'_>) -> Option<Cursor> {
        None
    }

    fn picking(&self) -> bool {
        true
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => self.hover = self.corner_at(*px, cx),
            ToolInput::DragStart { px, .. } => {
                self.dragging = self.corner_at(*px, cx);
                self.hover = self.dragging;
            }
            // A corner pulled: the frame grows or shrinks about its centre.
            ToolInput::Drag { px } if self.dragging.is_some() => self.free = Some((*px - center(cx)).abs()),
            ToolInput::DragEnd { .. } => {
                if self.dragging.take().is_some() {
                    let half = (self.frame(cx).1 - center(cx)).abs();
                    self.free = None;
                    return vec![Command::RememberCaptureFrame(half)];
                }
            }
            ToolInput::Key { key: Key::Enter, .. } => return vec![Command::Capture(self.request(cx))],
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let (min, max) = self.frame(cx);
        let [w, h] = self.image_size(cx);
        let left_out = LeftOut { selection: !cx.toggle(KEEP_SELECTION), guides: !cx.toggle(KEEP_GUIDES), axes: !cx.toggle(KEEP_AXES) };
        Preview {
            hide_selection: left_out.selection,
            left_out,
            capture: Some(CaptureFrame {
                min,
                max,
                handles: ratio_of(cx).is_none(),
                hovered: self.dragging.or(self.hover),
                label: t!("tool.capture.size", width = w, height = h),
            }),
            ..Preview::default()
        }
    }

    fn status(&self) -> String {
        t!("tool.capture.hint")
    }

    fn busy(&self) -> bool {
        self.dragging.is_some()
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let choice = |value, label: String| OptionChoice { value, tip: String::new(), label, icon: None };
        let setting = |key, label, tip, value: &str, choices| ToolOption {
            key,
            label,
            tip,
            value: OptionValue::Choice(value.to_owned()),
            choices,
            saved: true,
            icon: None,
            swatch: None,
        };
        let toggle = |key, label, tip, icon| ToolOption {
            key,
            label,
            tip,
            value: OptionValue::Toggle(cx.toggle(key)),
            choices: Vec::new(),
            saved: true,
            icon: Some(icon),
            swatch: None,
        };
        let ratio = cx.choice(RATIO).filter(|v| *v == FREE || RATIOS.iter().any(|(r, _)| r == v)).unwrap_or("16_9");
        let mut ratios = vec![choice(FREE, t!("tool.capture.ratio.free"))];
        ratios.extend(RATIOS.iter().map(|(v, _)| choice(v, ratio_label(v))));
        // Each size as the image it gives with the frame as it is.
        let (min, max) = self.frame(cx);
        let sizes = SIZES
            .iter()
            .map(|s| {
                let [w, h] = image_size(max - min, ratio_of(cx), *s);
                choice(size_value(*s), t!("tool.capture.size", width = w, height = h))
            })
            .collect();
        let format = format_of(cx);
        let mut grounds = vec![choice(THEME, t!("tool.capture.background.theme")), choice(WHITE, t!("tool.capture.background.white"))];
        if format == ImageFormat::Png {
            grounds.push(choice(TRANSPARENT, t!("tool.capture.background.transparent")));
        }
        let ground = match background_of(cx) {
            Background::Theme => THEME,
            Background::White => WHITE,
            Background::Transparent => TRANSPARENT,
        };
        vec![
            setting(RATIO, t!("tool.capture.option.ratio"), t!("tool.capture.option.ratio_tip"), ratio, ratios),
            setting(SIZE, t!("tool.capture.option.size"), t!("tool.capture.option.size_tip"), size_value(size_of(cx)), sizes),
            setting(BACKGROUND, t!("tool.capture.option.background"), t!("tool.capture.option.background_tip"), ground, grounds),
            setting(
                FORMAT,
                t!("tool.capture.option.format"),
                t!("tool.capture.option.format_tip"),
                if format == ImageFormat::Jpeg { JPEG } else { PNG },
                vec![choice(PNG, "PNG".to_owned()), choice(JPEG, "JPEG".to_owned())],
            ),
            toggle(KEEP_SELECTION, t!("tool.capture.option.selection"), t!("tool.capture.option.selection_tip"), "select"),
            toggle(KEEP_GUIDES, t!("tool.capture.option.guides"), t!("tool.capture.option.guides_tip"), "guides"),
            toggle(KEEP_AXES, t!("tool.capture.option.axes"), t!("tool.capture.option.axes_tip"), "axes"),
        ]
    }

    fn lists_choices(&self) -> bool {
        true
    }

    /// Always: ✓ captures (Enter), ✗ gives the tool up (Esc).
    fn awaits_validation(&self) -> bool {
        true
    }

    fn leave(&mut self) {
        self.hover = None;
    }

    fn back(&mut self) {
        self.reset();
    }

    fn reset(&mut self) {
        self.dragging = None;
        self.hover = None;
    }
}

/// A fixed ratio's name in the bar.
fn ratio_label(value: &str) -> String {
    match value {
        "16_9" => "16:9".to_owned(),
        "4_3" => "4:3".to_owned(),
        "3_2" => "3:2".to_owned(),
        "1_1" => "1:1".to_owned(),
        "a4_landscape" => t!("tool.capture.ratio.a4_landscape"),
        _ => t!("tool.capture.ratio.a4_portrait"),
    }
}

/// A size's value among the choices: its number of pixels.
fn size_value(size: u32) -> &'static str {
    match size {
        1280 => "1280",
        2560 => "2560",
        3840 => "3840",
        _ => "1920",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_frame_is_the_largest_of_its_ratio_centred() {
        let area = (DVec2::new(100.0, 50.0), DVec2::new(1164.0, 714.0));
        let (min, max) = frame_in(area, Some(1.0), None);
        assert_eq!(max - min, DVec2::splat(600.0));
        assert_eq!((min + max) / 2.0, (area.0 + area.1) / 2.0);
        let (min, max) = frame_in(area, Some(4.0), None);
        assert_eq!(max - min, DVec2::new(1000.0, 250.0));
    }

    #[test]
    fn a_free_frame_keeps_inside_the_margins() {
        let view = DVec2::new(1064.0, 664.0);
        assert_eq!(frame_in((DVec2::ZERO, view), None, None), (DVec2::splat(MARGIN), view - DVec2::splat(MARGIN)));
        let (min, max) = frame_in((DVec2::ZERO, view), None, Some(DVec2::new(2000.0, 100.0)));
        assert_eq!((min.x, max.y - min.y), (MARGIN, 200.0));
    }

    #[test]
    fn the_image_has_the_frames_ratio_and_the_longer_side_chosen() {
        assert_eq!(image_size(DVec2::new(800.0, 450.0), Some(16.0 / 9.0), 1920), [1920, 1080]);
        assert_eq!(image_size(DVec2::new(210.0, 297.0), Some(210.0 / 297.0), 3840), [2715, 3840]);
        assert_eq!(image_size(DVec2::new(300.0, 100.0), None, 1280), [1280, 427]);
    }
}

//! The app's own mouse cursors (docs/ui/cursors.md): OpenCutList's SVG cursors (32 × 32 points,
//! its hotspots), the tool's at its step ([`Cursor`], `Tool::cursor`).
//!
//! - Native: rasterised once per scale and handed to the OS through egui's `cursor_image`.
//!   macOS takes the bitmap at @2x whatever the screen (Skraft's winit fork, which also
//!   premultiplies it); the other systems in physical pixels, at the window's scale.
//! - Web: eframe drops `cursor_image` and sets the canvas' `cursor` style each frame; a style
//!   sheet's `!important` rule per cursor wins over it, chosen by the canvas' `data-cursor`.

use skraft_tools::Cursor;

/// Size of a cursor, in points (OCL's `CURSOR_SIZE`).
const SIZE: u32 = 32;

/// The SVG source, its name and the hotspot in points of the 32 × 32 image (OCL's
/// `SmartCursorManager`, `create_cursor`).
fn source(cursor: Cursor) -> (&'static [u8], &'static str, [u32; 2]) {
    macro_rules! svg {
        ($name:literal, $x:literal, $y:literal) => {
            (include_bytes!(concat!("../assets/cursors/", $name, ".svg")), $name, [$x, $y])
        };
    }
    match cursor {
        Cursor::Select => svg!("select", 0, 0),
        Cursor::SelectPart => svg!("select-part", 0, 0),
        Cursor::SelectPlusMinus => svg!("select-plus-minus", 0, 0),
        Cursor::SelectRect => svg!("select-rect", 0, 0),
        Cursor::SelectCopyLine => svg!("select-copy-line", 0, 0),
        Cursor::SelectCopyGrid => svg!("select-copy-grid", 0, 0),
        Cursor::SelectDistribute => svg!("select-distribute", 0, 0),
        Cursor::SelectStretch => svg!("select-stretch", 0, 0),
        Cursor::SelectUniteA => svg!("select-unite-a", 0, 0),
        Cursor::SelectUniteB => svg!("select-unite-b", 0, 0),
        Cursor::SelectSubtractA => svg!("select-subtract-a", 0, 0),
        Cursor::SelectSubtractB => svg!("select-subtract-b", 0, 0),
        Cursor::SelectIntersectA => svg!("select-intersect-a", 0, 0),
        Cursor::SelectIntersectB => svg!("select-intersect-b", 0, 0),
        Cursor::Move => svg!("move", 16, 16),
        Cursor::MoveCopy => svg!("move-copy", 16, 16),
        Cursor::PencilRectangle => svg!("pencil-rectangle", 0, 31),
        Cursor::PencilCircle => svg!("pencil-circle", 0, 31),
        Cursor::PencilPolygon => svg!("pencil-polygon", 0, 31),
        Cursor::Pull => svg!("pull", 16, 3),
        Cursor::PaintPart => svg!("paint-part", 2, 15),
        Cursor::PaintFace => svg!("paint-face-1", 2, 15),
        Cursor::Picker => svg!("picker", 0, 0),
    }
}

/// The view's cursor for this frame (the tool's), shown by [`apply`] at the frame's end.
pub fn ask(ctx: &egui::Context, cursor: Cursor) {
    ctx.data_mut(|d| d.insert_temp(asked_id(), Some(cursor)));
}

/// At the frame's end, once every widget has said its `CursorIcon`: the cursor the view asked
/// for, else `select` where egui leaves the system's arrow (`CursorIcon::Default`), else the
/// system's icon (text, resize, hand…). **Why:** one pointer over the whole window, the view's
/// and the interface's alike.
pub fn apply(ctx: &egui::Context) {
    let asked = ctx.data_mut(|d| d.remove_temp::<Option<Cursor>>(asked_id())).flatten();
    let arrow = ctx.output(|o| o.cursor_icon == egui::CursorIcon::Default);
    show(ctx, asked.or(arrow.then_some(Cursor::Select)));
}

/// Where `cursor`'s drawing lies about its hotspot (points): the bounds of its visible pixels,
/// worked out once. What is written by the pointer (a tooltip) keeps out of it.
pub fn footprint(cursor: Cursor) -> egui::Rect {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<Cursor, egui::Rect>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    *cache.entry(cursor).or_insert_with(|| {
        const SCALE: u32 = 2;
        let (rgba, side) = rasterise(cursor, SCALE);
        let (mut lo, mut hi) = ([side, side], [0, 0]);
        for (i, p) in rgba.chunks_exact(4).enumerate() {
            if p[3] > 0 {
                let (x, y) = (i as u32 % side, i as u32 / side);
                lo = [lo[0].min(x), lo[1].min(y)];
                hi = [hi[0].max(x + 1), hi[1].max(y + 1)];
            }
        }
        let [hx, hy] = source(cursor).2.map(|v| v as f32);
        let at = |[x, y]: [u32; 2]| egui::pos2(x as f32 / SCALE as f32 - hx, y as f32 / SCALE as f32 - hy);
        egui::Rect::from_min_max(at(lo), at(hi))
    })
}

fn asked_id() -> egui::Id {
    egui::Id::unique("cursor_asked")
}

/// Shows `cursor` over the pointer from this frame on, or the system's `CursorIcon` (none).
/// Both keep from frame to frame: [`apply`] sets one or the other every frame.
fn show(ctx: &egui::Context, cursor: Option<Cursor>) {
    #[cfg(not(target_arch = "wasm32"))]
    ctx.set_cursor_image(cursor.map(|c| native::image(c, native::scale(ctx))));
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        web::show(cursor);
    }
}

/// `cursor` rasterised at `scale` pixels per point: straight RGBA, its side in pixels.
fn rasterise(cursor: Cursor, scale: u32) -> (Vec<u8>, u32) {
    use resvg::{tiny_skia, usvg};
    let (svg, name, _) = source(cursor);
    let side = SIZE * scale;
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).unwrap_or_else(|e| panic!("cursor {name}: {e}"));
    let mut pixmap = tiny_skia::Pixmap::new(side, side).expect("cursor size");
    let k = side as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(k, k), &mut pixmap.as_mut());
    // tiny-skia is premultiplied; winit and PNG take straight alpha.
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    (rgba, side)
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    use super::{Cursor, rasterise, source};

    /// Pixels per point of the bitmap handed to the OS.
    pub fn scale(ctx: &egui::Context) -> u32 {
        if cfg!(target_os = "macos") { 2 } else { ctx.pixels_per_point().round().max(1.0) as u32 }
    }

    /// `cursor` at `scale`, cached: the same `Arc` every frame, which egui-winit dedupes by
    /// pointer (no upload to the OS each frame).
    pub fn image(cursor: Cursor, scale: u32) -> egui::CustomCursorImage {
        static CACHE: OnceLock<Mutex<HashMap<(Cursor, u32), egui::CustomCursorImage>>> = OnceLock::new();
        let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
        cache
            .entry((cursor, scale))
            .or_insert_with(|| {
                let (rgba, side) = rasterise(cursor, scale);
                let hot = source(cursor).2.map(|v| (v * scale).min(side - 1) as u16);
                egui::CustomCursorImage { rgba: Arc::from(rgba), size: [side as u16; 2], hotspot: hot }
            })
            .clone()
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::{Cell, RefCell};

    use super::{Cursor, rasterise, source};

    thread_local! {
        static SHEET: Cell<bool> = const { Cell::new(false) };
        static SHOWN: RefCell<Option<Cursor>> = const { RefCell::new(None) };
    }

    /// The canvas' `data-cursor` set to `cursor`'s name (removed for none), when it changes;
    /// the style sheet added the first time.
    pub fn show(cursor: Option<Cursor>) {
        if SHOWN.with(|s| *s.borrow() == cursor) {
            return;
        }
        SHOWN.with(|s| *s.borrow_mut() = cursor);
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        if cursor.is_some() && !SHEET.with(|s| s.replace(true)) {
            add_sheet(&document);
        }
        let Some(canvas) = document.get_element_by_id("skraft") else { return };
        let _ = match cursor {
            Some(c) => canvas.set_attribute("data-cursor", source(c).1),
            None => canvas.remove_attribute("data-cursor"),
        };
    }

    /// One rule per cursor, `!important` over eframe's inline style: PNGs at 1x and 2x
    /// (`image-set`, crisp on HiDPI screens), the hotspot in CSS pixels.
    fn add_sheet(document: &web_sys::Document) {
        let mut css = String::new();
        for cursor in Cursor::ALL {
            let (_, name, [x, y]) = source(cursor);
            let one = png_url(cursor, 1);
            let two = png_url(cursor, 2);
            css += &format!(
                "#skraft[data-cursor=\"{name}\"] {{ cursor: url({one}) {x} {y}, default !important; \
                 cursor: -webkit-image-set(url({one}) 1x, url({two}) 2x) {x} {y}, default !important; \
                 cursor: image-set(url({one}) 1x, url({two}) 2x) {x} {y}, default !important; }}\n"
            );
        }
        let Ok(style) = document.create_element("style") else { return };
        style.set_text_content(Some(&css));
        if let Some(head) = document.head() {
            let _ = head.append_child(&style);
        }
    }

    /// `cursor` at `scale` as a PNG `data:` URL.
    fn png_url(cursor: Cursor, scale: u32) -> String {
        let (rgba, side) = rasterise(cursor, scale);
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, side, side);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("cursor PNG");
            writer.write_image_data(&rgba).expect("cursor PNG");
        }
        format!("\"data:image/png;base64,{}\"", base64(&bytes))
    }

    fn base64(bytes: &[u8]) -> String {
        const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cursor's SVG parses and draws something, its hotspot inside it.
    #[test]
    fn every_cursor_rasterises() {
        for cursor in Cursor::ALL {
            let (rgba, side) = rasterise(cursor, 2);
            assert_eq!(side, 2 * SIZE);
            assert!(rgba.chunks_exact(4).any(|p| p[3] > 0), "{cursor:?} is empty");
            assert!(source(cursor).2.iter().all(|&v| v < SIZE), "{cursor:?}'s hotspot");
        }
    }

    /// A cursor's footprint is its drawing about its hotspot: the pipette's, whose tip is the
    /// hotspot, lies right of it and below.
    #[test]
    fn a_footprint_lies_about_the_hotspot() {
        let picker = footprint(Cursor::Picker);
        assert!(picker.min.x >= -0.5 && picker.min.y >= -0.5, "{picker:?}");
        assert!(picker.max.x > 20.0 && picker.max.y > 20.0, "{picker:?}");
        // The pencils' tip at the bottom left: drawn above it.
        let pencil = footprint(Cursor::PencilRectangle);
        assert!(pencil.max.y < 2.0 && pencil.min.y < -20.0, "{pencil:?}");
    }
}

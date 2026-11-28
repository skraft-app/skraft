//! « Capturer »'s image (docs/ui/capture.md, decision 0060): the view's frame rendered again off
//! screen, its projection narrowed to the capture frame, at the size asked; read back from the
//! GPU without waiting for it (the web cannot), then encoded.

use std::sync::{Arc, Mutex};

use glam::{DVec2, Mat4, Vec4};
use skraft_render as render;
use skraft_theme::Theme;
use skraft_tools::Editor;
use skraft_tools::tools::capture::{Background, CaptureRequest, ImageFormat};

use super::Viewport;

/// A texture copied into a buffer, being mapped for reading: `Some(ok)` once it is (or failed).
struct Readback {
    buffer: wgpu::Buffer,
    /// Bytes per row in the buffer (a multiple of 256).
    row: u32,
    size: [u32; 2],
    mapped: Arc<Mutex<Option<bool>>>,
}

impl Readback {
    fn start(state: &egui_wgpu::RenderState, texture: &wgpu::Texture, ctx: &egui::Context) -> Self {
        let size = [texture.width(), texture.height()];
        let row = (size[0] * 4).div_ceil(256) * 256;
        let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture"),
            size: u64::from(row) * u64::from(size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = state.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
        state.queue.submit([enc.finish()]);
        let mapped = Arc::new(Mutex::new(None));
        let (slot, ctx) = (mapped.clone(), ctx.clone());
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            if let Ok(mut m) = slot.lock() {
                *m = Some(result.is_ok());
            }
            ctx.request_repaint();
        });
        Readback { buffer, row, size, mapped }
    }

    /// Its pixels (RGBA rows, top first) once mapped; `Some(None)` when the mapping failed.
    fn take(&self) -> Option<Option<Vec<u8>>> {
        let ok = (*self.mapped.lock().ok()?)?;
        if !ok {
            return Some(None);
        }
        let Ok(data) = self.buffer.slice(..).get_mapped_range() else { return Some(None) };
        let [w, h] = self.size;
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let s = (y * self.row) as usize;
            px.extend_from_slice(&data[s..s + (w * 4) as usize]);
        }
        Some(Some(px))
    }
}

/// An image under way: rendered, being read back — twice for a transparent ground, on black
/// then on white.
pub struct Shot {
    reads: Vec<Readback>,
    pub format: ImageFormat,
}

/// An image read back: its size and pixels (RGBA rows, sRGB, top first).
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Shot {
    /// The image once read back (an error when the GPU failed it); None meanwhile.
    pub fn poll(&self, state: &egui_wgpu::RenderState) -> Option<Result<Image, String>> {
        // The desktop's buffers map when the device is polled; the web's on their own.
        #[cfg(not(target_arch = "wasm32"))]
        let _ = state.device.poll(wgpu::PollType::Poll);
        #[cfg(target_arch = "wasm32")]
        let _ = state;
        let mut pixels = Vec::new();
        for r in &self.reads {
            match r.take()? {
                Some(px) => pixels.push(px),
                None => return Some(Err("GPU read back failed".to_owned())),
            }
        }
        let [width, height] = self.reads[0].size;
        let rgba = match pixels.as_slice() {
            [black, white] => super::unblend(black, white),
            _ => pixels.swap_remove(0),
        };
        Some(Ok(Image { width, height, rgba }))
    }
}

/// The clip-space transform that makes the part of the view between `min` and `max` (points of
/// a view `view` large) the whole picture.
fn crop(min: DVec2, max: DVec2, view: DVec2) -> Mat4 {
    let scale = view / (max - min);
    let center = DVec2::new((min.x + max.x) / view.x - 1.0, 1.0 - (min.y + max.y) / view.y);
    let (sx, sy) = (scale.x as f32, scale.y as f32);
    Mat4::from_cols(
        Vec4::new(sx, 0.0, 0.0, 0.0),
        Vec4::new(0.0, sy, 0.0, 0.0),
        Vec4::Z,
        Vec4::new(-sx * center.x as f32, -sy * center.y as f32, 0.0, 1.0),
    )
}

/// `size` scaled down, ratio kept, to fit `limit` pixels a side.
fn fit(size: [u32; 2], limit: u32) -> [u32; 2] {
    let long = size[0].max(size[1]);
    if long <= limit {
        return size;
    }
    let k = f64::from(limit) / f64::from(long);
    size.map(|s| ((f64::from(s) * k).round() as u32).max(1))
}

impl Viewport {
    /// Renders the image `r` asks for (« Capturer », docs/ui/capture.md): what the view shows,
    /// its projection narrowed to the frame, its lines as wide as in the frame, on the ground
    /// asked; the read back started. None before the view was drawn.
    pub fn start_shot(
        &mut self,
        state: &egui_wgpu::RenderState,
        ctx: &egui::Context,
        editor: &Editor,
        theme: &Theme,
        r: &CaptureRequest,
    ) -> Option<Shot> {
        let shown = self.size;
        if shown[0] == 0 || r.view.x <= 0.0 || r.max.x <= r.min.x || r.max.y <= r.min.y {
            return None;
        }
        let ppp = shown[0] as f32 / r.view.x as f32;
        let [w, h] = fit(r.size, state.device.limits().max_texture_dimension_2d);
        let picture = self.renderer.as_ref()?.new_picture(&state.device, [w, h]);
        let crop = crop(r.min, r.max, r.view);
        // The frame enlarged: as many pixels per point of it as the image has.
        let pixels_per_point = w as f32 / (r.max.x - r.min.x) as f32;
        let grounds: &[Option<[f32; 3]>] = match r.background {
            Background::Theme => &[None],
            Background::White => &[Some([1.0; 3])],
            Background::Transparent => &[Some([0.0; 3]), Some([1.0; 3])],
        };
        let reads = self.with_frame(state, shown, ppp, editor, theme, |renderer, frame| {
            let reads = grounds
                .iter()
                .map(|ground| {
                    let frame = render::Frame {
                        proj: crop * frame.proj,
                        background: ground.unwrap_or(frame.background),
                        pixels_per_point,
                        ..*frame
                    };
                    renderer.render_picture(&state.device, &state.queue, &frame, &picture);
                    Readback::start(state, picture.texture(), ctx)
                })
                .collect();
            // Its multisampled targets are large: freed now, not kept for the next picture.
            renderer.release_picture_targets();
            reads
        })?;
        Some(Shot { reads, format: r.format })
    }
}

/// The image as a file of `format`: PNG with its alpha, JPEG without (its ground opaque).
pub fn encode(image: &Image, format: ImageFormat) -> Result<Vec<u8>, String> {
    match format {
        ImageFormat::Png => crate::system::driving::png(image.width, image.height, &image.rgba),
        ImageFormat::Jpeg => {
            let rgb: Vec<u8> = image.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
                .encode(&rgb, image.width, image.height, image::ExtendedColorType::Rgb8)
                .map_err(|e| e.to_string())?;
            Ok(out)
        }
    }
}

/// A JPEG's quality (decision 0060: no option).
const JPEG_QUALITY: u8 = 92;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crop_maps_the_frame_on_the_whole_picture() {
        let view = DVec2::new(1000.0, 500.0);
        let m = crop(DVec2::new(100.0, 50.0), DVec2::new(500.0, 250.0), view);
        // The frame's corners, in clip space before: (−0.8, 0.8) and (0, 0).
        let at = |x: f32, y: f32| m * Vec4::new(x, y, 0.5, 1.0);
        assert!((at(-0.8, 0.8) - Vec4::new(-1.0, 1.0, 0.5, 1.0)).length() < 1e-6);
        assert!((at(0.0, 0.0) - Vec4::new(1.0, -1.0, 0.5, 1.0)).length() < 1e-6);
    }

    #[test]
    fn a_size_past_the_limit_is_scaled_down() {
        assert_eq!(fit([3840, 2160], 8192), [3840, 2160]);
        assert_eq!(fit([3840, 2160], 2048), [2048, 1152]);
    }
}

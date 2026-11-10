//! Off-screen rendering: needs a GPU adapter (Metal on macOS); skipped when there is none.

use glam::{DMat4, DVec3, Mat4};
use solidsketch_kernel::Solid;

use super::*;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
    pollster::block_on(adapter.request_device(&Default::default())).ok()
}

/// Renders `frame` and reads the target back as RGBA8 rows.
fn draw(r: &mut Renderer, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2], frame: &Frame<'_>) -> Vec<u8> {
    let mut enc = device.create_command_encoder(&Default::default());
    r.render(device, queue, &mut enc, size, frame);
    let row = (size[0] * 4).div_ceil(256) * 256;
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * size[1]) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        r.target_texture().unwrap().as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &out,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    out.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = out.slice(..).get_mapped_range().unwrap();
    let mut px = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for y in 0..size[1] {
        let s = (y * row) as usize;
        px.extend_from_slice(&data[s..s + (size[0] * 4) as usize]);
    }
    px
}

fn pixel(px: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

fn save(px: &[u8], size: [u32; 2], name: &str) {
    let Ok(dir) = std::env::var("SOLIDSKETCH_SNAPSHOTS") else { return };
    let f = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), size[0], size[1]);
    e.set_color(png::ColorType::Rgba);
    e.write_header().unwrap().write_image_data(px).unwrap();
}

/// Three placements of one definition (a cube with a cylinder on top), seen from a camera, all
/// shifted by `offset` (the camera too): the picture must not depend on it.
fn scene(
    r: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    offset: DVec3,
    highlight: bool,
    decor: bool,
) -> Vec<u8> {
    let cube = Solid::new_box(offset, DVec3::splat(10.0)).unwrap().mesh().unwrap();
    let cyl = Solid::new_cylinder(offset + DVec3::new(5.0, 5.0, 10.0), 3.0, 8.0).unwrap().mesh().unwrap();
    // The revision changes with the offset: the definition's mesh is re-uploaded.
    r.set_mesh(device, 7, offset.x.to_bits(), &[Part { id: 1, mesh: &cube }, Part { id: 2, mesh: &cyl }]);
    let at = |i: i32| DMat4::from_translation(DVec3::new(i as f64 * 20.0 - 25.0, 0.0, 0.0));
    let placements = (0..3).map(|i| Placement { transform: at(i), tint: NO_TINT }).collect();
    let batches = [Batch { key: 7, placements }];
    let size = SIZE;
    let eye = offset + DVec3::new(-20.0, -70.0, 50.0);
    let view = DMat4::look_at_rh(eye, offset + DVec3::new(0.0, 5.0, 5.0), DVec3::Z);
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, size[0] as f32 / size[1] as f32, 1.0);
    let hl = [Highlight {
        key: 7,
        part: 1,
        transform: at(1),
        face_tint: [0.1, 0.3, 0.9, 0.5],
        edge_tint: [0.1, 0.3, 0.9, 1.0],
    }];
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        batches: &batches,
        highlights: if highlight { &hl } else { &[] },
        lines: &[],
        overlay: &[],
        grid: decor.then_some(Grid { spacing: 10.0, count: 10 }),
        axes: decor,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
    };
    draw(r, device, queue, size, &frame)
}

const SIZE: [u32; 2] = [320, 200];

#[test]
fn draws_instanced_definition_and_highlight() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let plain = scene(&mut r, &device, &queue, DVec3::ZERO, false, true);
    save(&plain, SIZE, "plain");
    // The middle cube covers the centre of the picture.
    let c = pixel(&plain, SIZE[0], 160, 115);
    assert_ne!(c, [255, 255, 255, 255], "centre is background");

    // Highlight the middle placement's cube: the centre turns blue.
    let lit = scene(&mut r, &device, &queue, DVec3::ZERO, true, true);
    save(&lit, SIZE, "highlight");
    let h = pixel(&lit, SIZE[0], 160, 115);
    assert!(h[2] > h[0] + 30, "highlighted pixel {h:?} is not blue (was {c:?})");
}

#[test]
fn far_from_the_origin_looks_the_same() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let near = scene(&mut r, &device, &queue, DVec3::ZERO, true, false);
    // 100 km away: a naive f32 world transform has a 8 mm step there.
    let far = scene(&mut r, &device, &queue, DVec3::new(1e8, -3e7, 0.0), true, false);
    save(&far, SIZE, "far");
    // The same picture (no grid or axes: those stay at the world origin).
    let differing = near
        .chunks(4)
        .zip(far.chunks(4))
        .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8))
        .count();
    assert!(differing < 40, "{differing} pixels differ 100 km away");
}

/// Wide edges keep their full width where neighbouring faces rise towards the eye: the
/// concave corners of a pocket, seen from above at an angle.
#[test]
fn concave_edges_are_not_buried() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    use solidsketch_kernel::BooleanOp;
    let block = Solid::new_box(DVec3::ZERO, DVec3::new(40.0, 40.0, 20.0)).unwrap();
    let pocket = Solid::new_box(DVec3::new(10.0, 10.0, 5.0), DVec3::new(20.0, 20.0, 20.0)).unwrap();
    let mesh = block.boolean(&pocket, BooleanOp::Difference).unwrap().mesh().unwrap();
    let mut r = Renderer::new(&device);
    r.set_mesh(&device, 1, 0, &[Part { id: 1, mesh: &mesh }]);
    let batches = [Batch { key: 1, placements: vec![Placement { transform: DMat4::IDENTITY, tint: NO_TINT }] }];
    let size = [640, 400];
    let view = DMat4::look_at_rh(DVec3::new(-30.0, -60.0, 90.0), DVec3::new(20.0, 20.0, 5.0), DVec3::Z);
    let proj = Mat4::perspective_infinite_reverse_rh(0.6, size[0] as f32 / size[1] as f32, 1.0);
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        batches: &batches,
        highlights: &[],
        lines: &[],
        overlay: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 2.0,
    };
    let px = draw(&mut r, &device, &queue, size, &frame);
    save(&px, size, "pocket");
    // Along the pocket's far vertical corner (x = 30, y = 30, z from 5 to 20), the ink is
    // there: a dark pixel within a pixel of the projected line.
    let to_px = |p: DVec3| {
        let c = proj.as_dmat4() * view * p.extend(1.0);
        let n = c.truncate() / c.w;
        ((n.x + 1.0) * 0.5 * size[0] as f64, (1.0 - n.y) * 0.5 * size[1] as f64)
    };
    let mut buried = 0;
    for k in 1..10 {
        let (x, y) = to_px(DVec3::new(30.0, 30.0, 5.0 + 15.0 * k as f64 / 10.0));
        let dark = (-1..=1).any(|d| {
            let p = pixel(&px, size[0], (x.round() as i32 + d) as u32, y.round() as u32);
            p[0] < 80
        });
        if !dark {
            buried += 1;
        }
    }
    assert_eq!(buried, 0, "{buried} of 9 samples of the concave edge are not drawn");
}

#[test]
fn a_mirrored_placement_shows_its_outside() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    // A cube centred on the origin: mirrored about X it covers the same pixels, its triangles
    // wound the other way. Its faces must stay front faces (not the blue-grey of back faces).
    let cube = Solid::new_box(DVec3::splat(-5.0), DVec3::splat(10.0)).unwrap().mesh().unwrap();
    r.set_mesh(&device, 3, 1, &[Part { id: 1, mesh: &cube }]);
    let view = DMat4::look_at_rh(DVec3::new(-20.0, -30.0, 25.0), DVec3::ZERO, DVec3::Z);
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, SIZE[0] as f32 / SIZE[1] as f32, 1.0);
    let mut shot = |transform: DMat4| {
        let batches = [Batch { key: 3, placements: vec![Placement { transform, tint: NO_TINT }] }];
        let frame = Frame {
            view,
            proj,
            background: [1.0, 1.0, 1.0],
            batches: &batches,
            highlights: &[],
            lines: &[],
            overlay: &[],
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
        };
        draw(&mut r, &device, &queue, SIZE, &frame)
    };
    let plain = shot(DMat4::IDENTITY);
    let mirrored = shot(DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
    save(&mirrored, SIZE, "mirrored");
    let (a, b) = (pixel(&plain, SIZE[0], 160, 100), pixel(&mirrored, SIZE[0], 160, 100));
    assert_ne!(a, [255, 255, 255, 255], "the cube covers the centre");
    let diff = a.iter().zip(&b).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
    assert!(diff <= 2, "mirrored {b:?} vs plain {a:?}");
}

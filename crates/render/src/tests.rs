//! Off-screen rendering: needs a GPU adapter (Metal on macOS); skipped when there is none.

use glam::{DMat4, DVec3, Mat4};
use solidsketch_kernel::Solid;

use super::*;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
    let required_features = adapter.features() & OPTIONAL_FEATURES;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features, ..Default::default() })).ok()
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
fn scene(r: &mut Renderer, device: &wgpu::Device, queue: &wgpu::Queue, offset: DVec3, highlight: bool, decor: bool) -> Vec<u8> {
    let cube = Solid::new_box(offset, DVec3::splat(10.0)).unwrap().mesh().unwrap();
    let cyl = Solid::new_cylinder(offset + DVec3::new(5.0, 5.0, 10.0), 3.0, 8.0).unwrap().mesh().unwrap();
    // The revision changes with the offset: the definition's mesh is re-uploaded.
    r.set_mesh(device, 7, offset.x.to_bits(), &[Part { id: 1, mesh: &cube, face_paint: &[] }, Part { id: 2, mesh: &cyl, face_paint: &[] }]);
    let at = |i: i32| DMat4::from_translation(DVec3::new(i as f64 * 20.0 - 25.0, 0.0, 0.0));
    let placements = (0..3).map(|i| Placement::new(at(i))).collect();
    let batches = [Batch::new(7, placements)];
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
        material: NO_MATERIAL,
        ghost: false,
    }];
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        style: Style::default(),
        batches: &batches,
        highlights: if highlight { &hl } else { &[] },
        lines: &[],
        boxes: &[],
        overlay: &[],
        xray: &[],
        fills: &[],
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
    let differing = near.chunks(4).zip(far.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
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
    r.set_mesh(&device, 1, 0, &[Part { id: 1, mesh: &mesh, face_paint: &[] }]);
    let batches = [Batch::new(1, vec![Placement::new(DMat4::IDENTITY)])];
    let size = [640, 400];
    let view = DMat4::look_at_rh(DVec3::new(-30.0, -60.0, 90.0), DVec3::new(20.0, 20.0, 5.0), DVec3::Z);
    let proj = Mat4::perspective_infinite_reverse_rh(0.6, size[0] as f32 / size[1] as f32, 1.0);
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        style: Style::default(),
        batches: &batches,
        highlights: &[],
        lines: &[],
        boxes: &[],
        overlay: &[],
        xray: &[],
        fills: &[],
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
    r.set_mesh(&device, 3, 1, &[Part { id: 1, mesh: &cube, face_paint: &[] }]);
    let view = DMat4::look_at_rh(DVec3::new(-20.0, -30.0, 25.0), DVec3::ZERO, DVec3::Z);
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, SIZE[0] as f32 / SIZE[1] as f32, 1.0);
    let mut shot = |transform: DMat4| {
        let batches = [Batch::new(3, vec![Placement::new(transform)])];
        let frame = Frame {
            view,
            proj,
            background: [1.0, 1.0, 1.0],
            style: Style::default(),
            batches: &batches,
            highlights: &[],
            lines: &[],
            boxes: &[],
            overlay: &[],
            xray: &[],
            fills: &[],
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

fn shot(
    r: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: DMat4,
    placement: Placement,
    style: Style<'_>,
) -> (Vec<u8>, bool) {
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, SIZE[0] as f32 / SIZE[1] as f32, 1.0);
    let batches = [Batch::new(1, vec![placement])];
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        style,
        batches: &batches,
        highlights: &[],
        lines: &[],
        boxes: &[],
        overlay: &[],
        xray: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
    };
    let mut enc = device.create_command_encoder(&Default::default());
    let drew = r.render(device, queue, &mut enc, SIZE, &frame);
    queue.submit([enc.finish()]);
    (draw(r, device, queue, SIZE, &frame), drew)
}

fn front_view() -> DMat4 {
    DMat4::look_at_rh(DVec3::new(-20.0, -30.0, 25.0), DVec3::ZERO, DVec3::Z)
}

#[test]
fn a_still_frame_is_not_drawn_again() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let cube = Solid::new_box(DVec3::splat(-5.0), DVec3::splat(10.0)).unwrap().mesh().unwrap();
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &cube, face_paint: &[] }]);
    let p = Placement::new(DMat4::IDENTITY);
    let (first, drew) = shot(&mut r, &device, &queue, front_view(), p, Style::default());
    assert!(drew, "the first frame is drawn");
    // `shot` renders the same frame twice: the second time, nothing is drawn.
    let mut enc = device.create_command_encoder(&Default::default());
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, SIZE[0] as f32 / SIZE[1] as f32, 1.0);
    let batches = [Batch::new(1, vec![p])];
    let frame = Frame {
        view: front_view(),
        proj,
        background: [1.0, 1.0, 1.0],
        style: Style::default(),
        batches: &batches,
        highlights: &[],
        lines: &[],
        boxes: &[],
        overlay: &[],
        xray: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
    };
    assert!(!r.render(&device, &queue, &mut enc, SIZE, &frame), "the same frame again");
    // The target still holds the picture.
    let again = draw(&mut r, &device, &queue, SIZE, &frame);
    assert_eq!(first, again);
    // Anything that changes the picture draws it: the camera, a colour, a new mesh.
    let moved = DMat4::look_at_rh(DVec3::new(-21.0, -30.0, 25.0), DVec3::ZERO, DVec3::Z);
    assert!(shot(&mut r, &device, &queue, moved, p, Style::default()).1);
    let red = [[0.8, 0.05, 0.05, 1.0]];
    let style = Style { palette: &red, ..Style::default() };
    assert!(shot(&mut r, &device, &queue, moved, p, style).1, "a palette change");
    r.set_mesh(&device, 1, 2, &[Part { id: 1, mesh: &cube, face_paint: &[] }]);
    assert!(shot(&mut r, &device, &queue, moved, p, style).1, "a new mesh");
}

#[test]
fn faces_take_their_colour_from_the_palette() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let cube = Solid::new_box(DVec3::splat(-5.0), DVec3::splat(10.0)).unwrap().mesh().unwrap();
    let palette = [[0.8, 0.05, 0.05, 1.0], [0.05, 0.05, 0.8, 1.0]];
    let style = Style { palette: &palette, ..Style::default() };
    let at_centre = |px: &[u8]| pixel(px, SIZE[0], 160, 100);

    // The placement's colour: red.
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &cube, face_paint: &[] }]);
    let p = Placement { material: 0, ..Placement::new(DMat4::IDENTITY) };
    let (px, _) = shot(&mut r, &device, &queue, front_view(), p, style);
    save(&px, SIZE, "palette_placement");
    let c = at_centre(&px);
    assert!(c[0] > c[1] + 60 && c[0] > c[2] + 60, "{c:?} is not red");

    // Every face painted blue wins over the placement's red.
    let faces: Vec<u32> = vec![1; 6];
    r.set_mesh(&device, 1, 2, &[Part { id: 1, mesh: &cube, face_paint: &faces }]);
    let (px, _) = shot(&mut r, &device, &queue, front_view(), p, style);
    save(&px, SIZE, "palette_faces");
    let c = at_centre(&px);
    assert!(c[2] > c[0] + 60 && c[2] > c[1] + 60, "{c:?} is not blue");

    // No colour anywhere: the default front.
    r.set_mesh(&device, 1, 3, &[Part { id: 1, mesh: &cube, face_paint: &[] }]);
    let (px, _) = shot(&mut r, &device, &queue, front_view(), Placement::new(DMat4::IDENTITY), Style::default());
    let c = at_centre(&px);
    assert!(c[0].abs_diff(c[2]) < 60, "{c:?} is not the warm white");
}

#[test]
fn a_cylinder_shows_its_outline() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // A tall cylinder seen from the side: its B-rep edges are the two circles; its sides
    // are only silhouettes.
    let cyl = Solid::new_cylinder(DVec3::new(0.0, 0.0, -15.0), 6.0, 30.0).unwrap().mesh().unwrap();
    let view = DMat4::look_at_rh(DVec3::new(0.0, -60.0, 0.0), DVec3::ZERO, DVec3::Z);
    let mut r = Renderer::new(&device);
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &cyl, face_paint: &[] }]);
    let p = Placement::new(DMat4::IDENTITY);
    let (with, _) = shot(&mut r, &device, &queue, view, p, Style::default());
    let (without, _) = shot(&mut r, &device, &queue, view, p, Style { silhouettes: false, ..Style::default() });
    save(&with, SIZE, "silhouette");
    // Along the middle row, the darkest pixel near each side.
    let dark = |px: &[u8]| (0..SIZE[0]).filter(|&x| pixel(px, SIZE[0], x, 100)[0] < 60).count();
    assert!(dark(&with) >= 2, "no outline drawn on the sides");
    assert_eq!(dark(&without), 0, "dark pixels without silhouettes");
}

#[test]
fn a_section_cuts_and_caps() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // A cube cut by the plane x = 0, the +x half removed; seen from +x, the cap faces the eye.
    let cube = Solid::new_box(DVec3::splat(-5.0), DVec3::splat(10.0)).unwrap().mesh().unwrap();
    let view = DMat4::look_at_rh(DVec3::new(30.0, -10.0, 8.0), DVec3::ZERO, DVec3::Z);
    let mut r = Renderer::new(&device);
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &cube, face_paint: &[] }]);
    let palette = [[0.05, 0.6, 0.05, 1.0]];
    let p = Placement { material: 0, ..Placement::new(DMat4::IDENTITY) };
    let section = Some(Section { origin: DVec3::ZERO, normal: DVec3::X });
    let style = Style { palette: &palette, section, cap_shade: 0.0, ..Style::default() };
    let (plain, _) = shot(&mut r, &device, &queue, view, p, Style { section: None, ..style });
    let (cut, _) = shot(&mut r, &device, &queue, view, p, style);
    save(&cut, SIZE, "section");
    let c = pixel(&cut, SIZE[0], 160, 100);
    // The centre shows the cap: flat, the part's green at full strength (no shading).
    assert!(c[1] > c[0] + 60 && c[1] > c[2] + 60, "cap {c:?} is not green");
    // The cut part is smaller on screen: fewer non-background pixels.
    let covered = |px: &[u8]| px.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
    assert!(covered(&cut) < covered(&plain), "nothing was cut away");
}

#[test]
fn a_section_caps_only_where_it_cuts() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // The plane lies between the eye and the cube: the cube is wholly kept, so it must look
    // as without a section — shaded, with its edges — not painted over as a cap.
    let cube = Solid::new_box(DVec3::splat(-5.0), DVec3::splat(10.0)).unwrap().mesh().unwrap();
    let view = DMat4::look_at_rh(DVec3::new(30.0, -10.0, 8.0), DVec3::ZERO, DVec3::Z);
    let mut r = Renderer::new(&device);
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &cube, face_paint: &[] }]);
    let palette = [[0.05, 0.6, 0.05, 1.0]];
    let p = Placement { material: 0, ..Placement::new(DMat4::IDENTITY) };
    let section = Some(Section { origin: DVec3::new(15.0, 0.0, 0.0), normal: DVec3::X });
    let style = Style { palette: &palette, section, ..Style::default() };
    let (plain, _) = shot(&mut r, &device, &queue, view, p, Style { section: None, ..style });
    let (cut, _) = shot(&mut r, &device, &queue, view, p, style);
    save(&cut, SIZE, "section_kept");
    save(&plain, SIZE, "section_kept_plain");
    // Its edges all show (dark ink)…
    let ink = |px: &[u8]| px.chunks(4).filter(|p| p[..3].iter().all(|c| *c < 60)).count();
    assert!(ink(&cut) * 100 >= ink(&plain) * 95, "edges lost: {} ink pixels, {} uncut", ink(&cut), ink(&plain));
    // …and its faces are not painted over: only a sliver along the outline may differ, where
    // a back face already shows through between the edge's samples.
    let covered = plain.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
    let differ = plain.chunks(4).zip(cut.chunks(4)).filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 2)).count();
    assert!(differ * 100 < covered * 2, "{differ} of {covered} pixels differ from the uncut picture");
}

#[test]
fn a_ghost_shows_what_it_holds() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    // A large blank around a small red tool.
    let blank = Solid::new_box(DVec3::splat(-8.0), DVec3::splat(16.0)).unwrap().mesh().unwrap();
    let tool = Solid::new_box(DVec3::splat(-2.0), DVec3::splat(4.0)).unwrap().mesh().unwrap();
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &blank, face_paint: &[] }]);
    r.set_mesh(&device, 2, 1, &[Part { id: 2, mesh: &tool, face_paint: &[] }]);
    let red = [[0.8, 0.02, 0.02, 1.0]];
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, SIZE[0] as f32 / SIZE[1] as f32, 1.0);
    let shoot = |r: &mut Renderer, ghost: bool| {
        let batches = [
            Batch { ghost, ..Batch::new(1, vec![Placement::new(DMat4::IDENTITY)]) },
            Batch::new(2, vec![Placement { material: 0, ..Placement::new(DMat4::IDENTITY) }]),
        ];
        let frame = Frame {
            view: front_view(),
            proj,
            background: [1.0, 1.0, 1.0],
            style: Style { palette: &red, ..Style::default() },
            batches: &batches,
            highlights: &[],
            lines: &[],
            boxes: &[],
            overlay: &[],
            xray: &[],
            fills: &[],
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
        };
        draw(r, &device, &queue, SIZE, &frame)
    };
    let centre = |px: &[u8]| pixel(px, SIZE[0], SIZE[0] / 2, SIZE[1] / 2);
    let solid = shoot(&mut r, false);
    let ghost = shoot(&mut r, true);
    save(&ghost, SIZE, "ghost");
    // Opaque, the blank hides the tool; a ghost, it shows it, red.
    let [sr, sg, _, _] = centre(&solid);
    assert!(sr.abs_diff(sg) < 30, "{:?}", centre(&solid));
    let [gr, gg, gb, _] = centre(&ghost);
    assert!(gr > gg + 60 && gr > gb + 60, "{:?}", centre(&ghost));
}

#[test]
fn faint_edges_are_see_through() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let mesh = Solid::new_box(DVec3::splat(-8.0), DVec3::splat(16.0)).unwrap().mesh().unwrap();
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &mesh, face_paint: &[] }]);
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, SIZE[0] as f32 / SIZE[1] as f32, 1.0);
    let shoot = |r: &mut Renderer, faint_edges: bool| {
        let batches = [Batch { ghost: true, faint_edges, ..Batch::new(1, vec![Placement::new(DMat4::IDENTITY)]) }];
        let frame = Frame {
            view: front_view(),
            proj,
            background: [1.0, 1.0, 1.0],
            style: Style::default(),
            batches: &batches,
            highlights: &[],
            lines: &[],
            boxes: &[],
            overlay: &[],
            xray: &[],
            fills: &[],
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
        };
        draw(r, &device, &queue, SIZE, &frame)
    };
    // The darkest pixel: an edge's.
    let darkest = |px: &[u8]| px.chunks(4).map(|p| p[..3].iter().copied().max().unwrap()).min().unwrap();
    let solid = darkest(&shoot(&mut r, false));
    let faint = shoot(&mut r, true);
    save(&faint, SIZE, "faint_edges");
    let faint = darkest(&faint);
    // Black edges, opaque; at a third of their opacity over white, much lighter (still drawn).
    assert!(solid < 80, "{solid}");
    assert!(faint > solid + 70 && faint < 235, "{solid} → {faint}");
}

/// A dashed line: dashes and gaps as long as asked, the pattern where it is said to be at the
/// line's start. Where the device clamps depth, past the parallel projection's depth range too.
#[test]
fn dashed_lines_are_dashed_on_screen() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    // Looking down -Z, a pixel a unit, the plane z = 0 at depth 10 in a 5–15 range.
    let size = [200, 40];
    let view = DMat4::look_at_rh(DVec3::new(0.0, 0.0, 10.0), DVec3::ZERO, DVec3::Y);
    let proj = Mat4::orthographic_rh(-100.0, 100.0, -20.0, 20.0, 15.0, 5.0);
    let clamped = device.features().contains(wgpu::Features::DEPTH_CLIP_CONTROL);
    let z = if clamped { 1000.0 } else { 2.0 };
    let line = Line {
        a: DVec3::new(-150.0, 0.0, -z),
        b: DVec3::new(150.0, 0.0, z),
        color: [0.0, 0.0, 0.0, 1.0],
        dash: Some(Dash { length: 5.0, phase: 2.0 }),
        width: LINE_WIDTH,
    };
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        style: Style::default(),
        batches: &[],
        highlights: &[],
        lines: &[line],
        boxes: &[],
        overlay: &[],
        xray: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
    };
    let px = draw(&mut r, &device, &queue, size, &frame);
    save(&px, size, "dashes");
    // The line starts 50 px left of the picture: at pixel x, the pattern is at 2 + 50 + x. It
    // runs between two rows of pixels: each takes half its ink.
    let ink = |x: u32| (19..=20).map(|y| pixel(&px, size[0], x, y)[0]).min().unwrap();
    let (mut dashes, mut gaps) = (0, 0);
    for x in 5..195 {
        let at = (2.0 + 50.0 + x as f64 + 0.5).rem_euclid(10.0);
        if (1.0..4.0).contains(&at) {
            assert!(ink(x) < 200, "no dash at {x} ({at}): {}", ink(x));
            dashes += 1;
        } else if (6.0..9.0).contains(&at) {
            assert!(ink(x) > 230, "no gap at {x} ({at}): {}", ink(x));
            gaps += 1;
        }
    }
    assert!(dashes > 40 && gaps > 40, "{dashes} {gaps}");
}

/// A box outline: its edges dashed from every corner (the corners inked), its inside left
/// alone. One instance per box.
#[test]
fn box_outlines_are_dashed_from_their_corners() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    // Looking down -Z, a pixel a unit; the box 160 × 30 px, centred, flat at z = 0..1.
    let size = [200, 40];
    let view = DMat4::look_at_rh(DVec3::new(0.0, 0.0, 10.0), DVec3::ZERO, DVec3::Y);
    let proj = Mat4::orthographic_rh(-100.0, 100.0, -20.0, 20.0, 15.0, 5.0);
    let outline = BoxOutline {
        transform: DMat4::from_translation(DVec3::new(-80.0, -15.0, 0.0)) * DMat4::from_scale(DVec3::new(160.0, 30.0, 1.0)),
        color: [0.0, 0.0, 0.0, 1.0],
        width: LINE_WIDTH,
        dash: Some(5.0),
    };
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        style: Style::default(),
        batches: &[],
        highlights: &[],
        lines: &[],
        boxes: &[outline],
        overlay: &[],
        xray: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
    };
    let px = draw(&mut r, &device, &queue, size, &frame);
    save(&px, size, "box_outline");
    // Picture rows run down: the box spans x 20..180, y 5..35.
    let ink = |x: u32, y: u32| pixel(&px, size[0], x, y)[0];
    for (x, y) in [(21u32, 5u32), (178, 5), (21, 34), (178, 34)] {
        let near = (x.saturating_sub(1)..=x + 1).flat_map(|x| (y.saturating_sub(1)..=y + 1).map(move |y| ink(x, y))).min().unwrap();
        assert!(near < 150, "corner ({x}, {y}) not inked: {near}");
    }
    assert!(ink(100, 20) > 240, "inside inked: {}", ink(100, 20));
    // Along the top edge, dashes and gaps.
    let row: Vec<u8> = (25..95).map(|x| (4..=6).map(|y| ink(x, y)).min().unwrap()).collect();
    assert!(row.iter().any(|v| *v < 150) && row.iter().any(|v| *v > 230), "{row:?}");
}

/// A fill is drawn in the scene: hidden behind a part, blended over the background.
#[test]
fn fills_hide_behind_faces() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let cube = Solid::new_box(DVec3::splat(-5.0), DVec3::splat(10.0)).unwrap().mesh().unwrap();
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &cube, face_paint: &[] }]);
    let batches = [Batch::new(1, vec![Placement::new(DMat4::IDENTITY)])];
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, SIZE[0] as f32 / SIZE[1] as f32, 1.0);
    // A large red plane under the cube.
    let c = |x: f64, y: f64| DVec3::new(x, y, -20.0);
    let plane =
        [Fill { corners: vec![c(-500.0, -500.0), c(500.0, -500.0), c(500.0, 500.0), c(-500.0, 500.0)], color: [1.0, 0.0, 0.0, 0.5] }];
    let shoot = |r: &mut Renderer, fills: &[Fill]| {
        let frame = Frame {
            view: front_view(),
            proj,
            background: [1.0, 1.0, 1.0],
            style: Style::default(),
            batches: &batches,
            highlights: &[],
            lines: &[],
            boxes: &[],
            overlay: &[],
            xray: &[],
            fills,
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
        };
        draw(r, &device, &queue, SIZE, &frame)
    };
    let plain = shoot(&mut r, &[]);
    let filled = shoot(&mut r, &plane);
    save(&filled, SIZE, "fill");
    let at = |px: &[u8], x, y| pixel(px, SIZE[0], x, y);
    // The cube hides the plane; below it, the plane tints the background.
    assert_eq!(at(&plain, 160, 100), at(&filled, 160, 100));
    let [r0, g0, b0, _] = at(&filled, 160, SIZE[1] - 5);
    assert!(r0 > 200 && g0 < 220 && b0 < 220 && r0 > g0 + 30, "{:?}", [r0, g0, b0]);
}

/// An x-ray line (a move's): plain where it is seen, dashed where a face hides it.
#[test]
fn x_ray_lines_are_dashed_behind_faces() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    // Looking down -Z, a pixel a unit; a slab 40 px wide over the middle of a red line.
    let size = [200, 40];
    let view = DMat4::look_at_rh(DVec3::new(0.0, 0.0, 10.0), DVec3::ZERO, DVec3::Y);
    let proj = Mat4::orthographic_rh(-100.0, 100.0, -20.0, 20.0, 15.0, 5.0);
    let slab = Solid::new_box(DVec3::new(-20.0, -15.0, -2.0), DVec3::new(40.0, 30.0, 4.0)).unwrap().mesh().unwrap();
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &slab, face_paint: &[] }]);
    let batches = [Batch::new(1, vec![Placement::new(DMat4::IDENTITY)])];
    let line = Line {
        a: DVec3::new(-90.0, 0.0, -4.0),
        b: DVec3::new(90.0, 0.0, -4.0),
        color: [1.0, 0.0, 0.0, 1.0],
        dash: Some(Dash { length: 4.0, phase: 0.0 }),
        width: BOLD_WIDTH,
    };
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        style: Style::default(),
        batches: &batches,
        highlights: &[],
        lines: &[],
        boxes: &[],
        overlay: &[],
        xray: &[line],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
    };
    let px = draw(&mut r, &device, &queue, size, &frame);
    save(&px, size, "x_ray");
    let red = |x: u32| {
        (19..=20).any(|y| {
            let [r, g, b, _] = pixel(&px, size[0], x, y);
            r > 180 && g < 90 && b < 90
        })
    };
    // Seen: plain, every pixel.
    assert!((15..75).chain(125..185).all(red), "a gap where the line is seen");
    // Behind the slab: dashes and gaps.
    let hidden: Vec<bool> = (84..116).map(red).collect();
    assert!(hidden.iter().any(|r| *r) && hidden.iter().any(|r| !*r), "{hidden:?}");
}

//! Off-screen rendering: needs a GPU adapter (Metal on macOS); skipped when there is none.

use glam::{DMat4, DVec3, Mat4};
use skraft_kernel::Solid;

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
    let Ok(dir) = std::env::var("SKRAFT_SNAPSHOTS") else { return };
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
        faint_lines: &[],
        boxes: &[],
        overlay: &[],
        hidden_lines: &[],
        fills: &[],
        grid: decor.then_some(Grid { spacing: 10.0, count: 10 }),
        axes: decor,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
        model_per_point: 1.0,
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
    use skraft_kernel::BooleanOp;
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
        faint_lines: &[],
        boxes: &[],
        overlay: &[],
        hidden_lines: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 2.0,
        model_per_point: 1.0,
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
            faint_lines: &[],
            boxes: &[],
            overlay: &[],
            hidden_lines: &[],
            fills: &[],
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
            model_per_point: 1.0,
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
        faint_lines: &[],
        boxes: &[],
        overlay: &[],
        hidden_lines: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
        model_per_point: 1.0,
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
        faint_lines: &[],
        boxes: &[],
        overlay: &[],
        hidden_lines: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
        model_per_point: 1.0,
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

    // The cap is hatched: along a row and a column across it, darker lines on the green, a few
    // points apart (whatever their direction); without hatches, flat.
    let row = |px: &[u8]| (140..180).map(|x| pixel(px, SIZE[0], x, 100)[1] as i32).collect::<Vec<_>>();
    let column = |px: &[u8]| (80..120).map(|y| pixel(px, SIZE[0], 160, y)[1] as i32).collect::<Vec<_>>();
    let spread = |r: Vec<i32>| r.iter().max().unwrap() - r.iter().min().unwrap();
    let lines = |r: &[i32]| r.windows(2).filter(|w| w[1] < w[0] - 20).count();
    let (hatched, down) = (row(&cut), column(&cut));
    let crossed = lines(&hatched) + lines(&down);
    assert!((3..=12).contains(&crossed), "{crossed} hatches across 40 px both ways: {hatched:?} {down:?}");
    let (flat, _) = shot(&mut r, &device, &queue, view, p, Style { hatch_opacity: 0.0, ..style });
    assert!(spread(row(&flat)) <= 2 && spread(column(&flat)) <= 2, "{:?} {:?}", row(&flat), column(&flat));
}

#[test]
fn a_cap_wins_over_a_part_lying_on_it() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // A green block standing on a red one (in contact at z = 0), cut at z = 5: looking down
    // through the cut, the green block's cap, not the red block's top under it — nor when the
    // red top rises a little into the green block (two curved meshes crossing by their chords).
    let green = Solid::new_box(DVec3::new(-5.0, -5.0, 0.0), DVec3::new(10.0, 10.0, 10.0)).unwrap().mesh().unwrap();
    let mut r = Renderer::new(&device);
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &green, face_paint: &[] }]);
    let palette = [[0.05, 0.6, 0.05, 1.0], [0.7, 0.05, 0.05, 1.0]];
    let placed = |material: u32| vec![Placement { material, ..Placement::new(DMat4::IDENTITY) }];
    // The red block drawn last: ties of the depth test go its way.
    let batches = [Batch::new(1, placed(0)), Batch::new(2, placed(1))];
    let section = Some(Section { origin: DVec3::new(0.0, 0.0, 5.0), normal: DVec3::Z });
    let style = Style { palette: &palette, section, hatch_opacity: 0.0, ..Style::default() };
    let cases = [(0.0, DVec3::new(3.0, -8.0, 40.0), "section_contact"), (0.0, DVec3::new(30.0, -45.0, 25.0), "section_contact_side")];
    for (rise, eye, name) in cases.into_iter().chain([(0.05, DVec3::new(3.0, -8.0, 40.0), "section_overlap")]) {
        let red = Solid::new_box(DVec3::new(-10.0, -10.0, -10.0), DVec3::new(20.0, 20.0, 10.0 + rise)).unwrap().mesh().unwrap();
        r.set_mesh(&device, 2, rise.to_bits(), &[Part { id: 2, mesh: &red, face_paint: &[] }]);
        let frame = Frame {
            view: DMat4::look_at_rh(eye, DVec3::new(0.0, 0.0, 5.0), DVec3::Z),
            proj: Mat4::perspective_infinite_reverse_rh(0.8, SIZE[0] as f32 / SIZE[1] as f32, 1.0),
            background: [1.0, 1.0, 1.0],
            style,
            batches: &batches,
            highlights: &[],
            lines: &[],
            faint_lines: &[],
            boxes: &[],
            overlay: &[],
            hidden_lines: &[],
            fills: &[],
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
            model_per_point: 1.0,
        };
        let px = draw(&mut r, &device, &queue, SIZE, &frame);
        save(&px, SIZE, name);
        // Across the green cap, every pixel green.
        let reds = (0..SIZE[0])
            .flat_map(|x| (0..SIZE[1]).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                let c = pixel(&px, SIZE[0], x, y);
                c[0] as i32 > c[1] as i32 + 60 && x.abs_diff(SIZE[0] / 2) < 6 && y.abs_diff(SIZE[1] / 2) < 4
            })
            .count();
        assert_eq!(reds, 0, "{name}: {reds} red pixels in the green cap");
    }
}

#[test]
fn hatches_run_across_a_cap_seen_at_a_slant() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // A wide slab running away from the eye, cut flat at z = 0, seen low: one pitch for its
    // whole cap, so its hatches run from the near end to the far one — as many crossing a row
    // near the eye as one far off (a pitch per pixel ended half of them on the way).
    let slab = Solid::new_box(DVec3::new(-100.0, 0.0, -5.0), DVec3::new(200.0, 300.0, 10.0)).unwrap().mesh().unwrap();
    let mut r = Renderer::new(&device);
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &slab, face_paint: &[] }]);
    let palette = [[0.05, 0.6, 0.05, 1.0]];
    let p = Placement { material: 0, ..Placement::new(DMat4::IDENTITY) };
    let section = Some(Section { origin: DVec3::ZERO, normal: DVec3::Z });
    let style = Style { palette: &palette, section, cap_shade: 0.0, ..Style::default() };
    let view = DMat4::look_at_rh(DVec3::new(0.0, -60.0, 40.0), DVec3::new(0.0, 150.0, 0.0), DVec3::Z);
    let (px, _) = shot(&mut r, &device, &queue, view, p, style);
    save(&px, SIZE, "section_slant");
    // The rows the cap crosses (green), and the hatches crossing each, its middle half.
    let green = |c: [u8; 4]| c[1] as i32 > c[0] as i32 + 40;
    let rows: Vec<u32> = (0..SIZE[1]).filter(|&y| (0..SIZE[0]).filter(|&x| green(pixel(&px, SIZE[0], x, y))).count() > 20).collect();
    assert!(rows.len() > 40, "the cap spans {} rows", rows.len());
    let lines = |y: u32| {
        let xs: Vec<u32> = (0..SIZE[0]).filter(|&x| green(pixel(&px, SIZE[0], x, y))).collect();
        let (lo, hi) = (xs[0], xs[xs.len() - 1]);
        let (lo, hi) = (lo + (hi - lo) / 4, hi - (hi - lo) / 4);
        let top = (lo..=hi).map(|x| pixel(&px, SIZE[0], x, y)[1]).max().unwrap();
        let dark: Vec<bool> = (lo..=hi).map(|x| pixel(&px, SIZE[0], x, y)[1] + 20 < top).collect();
        dark.windows(2).filter(|w| !w[0] && w[1]).count()
    };
    let (far, near) = (lines(rows[rows.len() / 5]), lines(rows[rows.len() * 4 / 5]));
    assert!(near >= 1 && far >= 1 && near.abs_diff(far) <= 1, "{near} hatches near the eye, {far} far off");
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
            faint_lines: &[],
            boxes: &[],
            overlay: &[],
            hidden_lines: &[],
            fills: &[],
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
            model_per_point: 1.0,
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
            faint_lines: &[],
            boxes: &[],
            overlay: &[],
            hidden_lines: &[],
            fills: &[],
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
            model_per_point: 1.0,
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

/// A dashed line on screen (a guide's): dashes and gaps as long as asked, the pattern where it
/// is said to be at the line's start. Where the device clamps depth, past the parallel
/// projection's depth range too.
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
        dash: Some(Dash::even(5.0, Lay::Screen(2.0))),
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
        faint_lines: &[],
        boxes: &[],
        overlay: &[],
        hidden_lines: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
        model_per_point: 1.0,
    };
    let px = draw(&mut r, &device, &queue, size, &frame);
    save(&px, size, "dashes_on_screen");
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

/// A dashed line: its dashes laid on it in model units, from the phase asked at its start, the
/// period the power of two nearest to the pattern's length at the view's focus. Where the
/// device clamps depth, past the parallel projection's depth range too (the line then running
/// steeply away: many model units a pixel).
#[test]
fn dashed_lines_are_laid_in_model_units() {
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
        dash: Some(Dash::even(5.0, Lay::Model(2.0))),
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
        faint_lines: &[],
        boxes: &[],
        overlay: &[],
        hidden_lines: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
        // A point at the focus: the line's model units a pixel.
        model_per_point: line.a.distance(line.b) / 300.0,
    };
    let px = draw(&mut r, &device, &queue, size, &frame);
    save(&px, size, "dashes");
    // The line starts 50 px left of the picture, `k` model units a pixel (as at the focus): at
    // pixel x, the pattern is at 2 + (50 + x)·k, its period (model units) the power of two
    // nearest to 10 k.
    // It runs between two rows of pixels: each takes half its ink.
    let k = line.a.distance(line.b) / 300.0;
    let period = 2f64.powf((10.0 * k).log2().round());
    let on_screen = period / k;
    assert!(on_screen > 10.0 / 2f64.sqrt() && on_screen < 10.0 * 2f64.sqrt(), "{on_screen}");
    let ink = |x: u32| (19..=20).map(|y| pixel(&px, size[0], x, y)[0]).min().unwrap();
    let (mut dashes, mut gaps) = (0, 0);
    for x in 5..195 {
        // Where the pixel is in its period, on screen.
        let at = (2.0 + (50.0 + x as f64 + 0.5) * k).rem_euclid(period) / k;
        let half = on_screen / 2.0;
        if (1.0..half - 1.0).contains(&at) {
            assert!(ink(x) < 200, "no dash at {x} ({at}): {}", ink(x));
            dashes += 1;
        } else if (half + 1.0..on_screen - 1.0).contains(&at) {
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
        dash: Some(Dash::even(5.0, Lay::Model(0.0))),
        ..BoxOutline::new(
            DMat4::from_translation(DVec3::new(-80.0, -15.0, 0.0)) * DMat4::from_scale(DVec3::new(160.0, 30.0, 1.0)),
            [0.0, 0.0, 0.0, 1.0],
            LINE_WIDTH,
        )
    };
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        style: Style::default(),
        batches: &[],
        highlights: &[],
        lines: &[],
        faint_lines: &[],
        boxes: &[outline],
        overlay: &[],
        hidden_lines: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
        model_per_point: 1.0,
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
    let plane = [Fill::polygon(vec![c(-500.0, -500.0), c(500.0, -500.0), c(500.0, 500.0), c(-500.0, 500.0)], [1.0, 0.0, 0.0, 0.5])];
    let shoot = |r: &mut Renderer, fills: &[Fill]| {
        let frame = Frame {
            view: front_view(),
            proj,
            background: [1.0, 1.0, 1.0],
            style: Style::default(),
            batches: &batches,
            highlights: &[],
            lines: &[],
            faint_lines: &[],
            boxes: &[],
            overlay: &[],
            hidden_lines: &[],
            fills,
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
            model_per_point: 1.0,
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

/// An x-ray line (a move's): plain where it is seen (in the depth-tested lines), dashed where
/// a face hides it (in the hidden lines).
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
        dash: None,
        width: BOLD_WIDTH,
    };
    let hidden = Line { dash: Some(Dash::even(4.0, Lay::Model(0.0))), ..line };
    let frame = Frame {
        view,
        proj,
        background: [1.0, 1.0, 1.0],
        style: Style::default(),
        batches: &batches,
        highlights: &[],
        lines: &[line],
        faint_lines: &[],
        boxes: &[],
        overlay: &[],
        hidden_lines: &[hidden],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
        model_per_point: 1.0,
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

/// A faint line (a guide outside the context): blended over the face it lies on, so it takes
/// its colour, rather than mixed with the background; hidden behind faces.
#[test]
fn faint_lines_blend_over_faces() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    // Looking down -Z, a pixel a unit, on a black background; a slab 40 px wide.
    let size = [200, 40];
    let view = DMat4::look_at_rh(DVec3::new(0.0, 0.0, 10.0), DVec3::ZERO, DVec3::Y);
    let proj = Mat4::orthographic_rh(-100.0, 100.0, -20.0, 20.0, 15.0, 5.0);
    let slab = Solid::new_box(DVec3::new(-20.0, -15.0, -2.0), DVec3::new(40.0, 30.0, 4.0)).unwrap().mesh().unwrap();
    r.set_mesh(&device, 1, 1, &[Part { id: 1, mesh: &slab, face_paint: &[] }]);
    let batches = [Batch::new(1, vec![Placement::new(DMat4::IDENTITY)])];
    let line = |y: f64, z: f64| Line {
        a: DVec3::new(-90.0, y, z),
        b: DVec3::new(90.0, y, z),
        color: [1.0, 0.0, 0.0, 0.5],
        dash: None,
        width: BOLD_WIDTH,
    };
    // On the slab's top face, and under the slab.
    let faint = [line(0.0, 2.0), line(8.0, -4.0)];
    let shoot = |r: &mut Renderer, faint_lines: &[Line]| {
        let frame = Frame {
            view,
            proj,
            background: [0.0, 0.0, 0.0],
            style: Style::default(),
            batches: &batches,
            highlights: &[],
            lines: &[],
            faint_lines,
            boxes: &[],
            overlay: &[],
            hidden_lines: &[],
            fills: &[],
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: 1.0,
            model_per_point: 1.0,
        };
        draw(r, &device, &queue, size, &frame)
    };
    let plain = shoot(&mut r, &[]);
    let px = shoot(&mut r, &faint);
    save(&px, size, "faint_lines");
    let at = |px: &[u8], x, y| pixel(px, size[0], x, y);
    // Over the background: half red on black.
    let [r0, g0, b0, _] = at(&px, 40, 20);
    assert!(r0 > 120 && g0 < 30 && b0 < 30, "{:?}", [r0, g0, b0]);
    // On the face: half red on the face's light colour.
    let [r1, g1, b1, _] = at(&px, 100, 20);
    assert!(r1 > g1 + 30 && g1 > 120 && b1 > 120, "{:?}", [r1, g1, b1]);
    // Under the slab: hidden.
    assert_eq!(at(&plain, 100, 12), at(&px, 100, 12));
}

/// A frame looking down -Z from z = 10, a pixel a unit, 200 × 40 px, its depth range z = 9
/// to -30: `lines`, `boxes`, `fills` and the slab `batches` drawn on white.
fn flat_frame<'a>(batches: &'a [Batch], lines: &'a [Line], boxes: &'a [BoxOutline], fills: &'a [Fill]) -> Frame<'a> {
    Frame {
        view: DMat4::look_at_rh(DVec3::new(0.0, 0.0, 10.0), DVec3::ZERO, DVec3::Y),
        proj: Mat4::orthographic_rh(-100.0, 100.0, -20.0, 20.0, 40.0, 1.0),
        background: [1.0, 1.0, 1.0],
        style: Style::default(),
        batches,
        highlights: &[],
        lines,
        faint_lines: &[],
        boxes,
        overlay: &[],
        hidden_lines: &[],
        fills,
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: 1.0,
        model_per_point: 1.0,
    }
}

/// A slab over the middle of the view: x and y in ±20 × ±15, z in ±2.
fn slab(r: &mut Renderer, device: &wgpu::Device) -> [Batch; 1] {
    let slab = Solid::new_box(DVec3::new(-20.0, -15.0, -2.0), DVec3::new(40.0, 30.0, 4.0)).unwrap().mesh().unwrap();
    r.set_mesh(device, 1, 1, &[Part { id: 1, mesh: &slab, face_paint: &[] }]);
    [Batch::new(1, vec![Placement::new(DMat4::IDENTITY)])]
}

/// A line's ink at column `x` of the rows `rows` (the darkest red channel).
fn ink_at(px: &[u8], x: u32, rows: std::ops::RangeInclusive<u32>) -> u8 {
    rows.map(|y| pixel(px, 200, x, y)[0]).min().unwrap()
}

/// A pattern of more than a dash: dash, gap, dot, gap (Kuix's dash-dot-dash), each where it
/// should be.
#[test]
fn dash_dot_lines_follow_their_pattern() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let line = Line {
        a: DVec3::new(-150.0, 0.0, 0.0),
        b: DVec3::new(150.0, 0.0, 0.0),
        color: [0.0, 0.0, 0.0, 1.0],
        dash: Some(Dash { pattern: [8.0, 3.0, 2.0, 3.0], lay: Lay::Model(0.0) }),
        width: LINE_WIDTH,
    };
    let px = draw(&mut r, &device, &queue, [200, 40], &flat_frame(&[], &[line], &[], &[]));
    save(&px, [200, 40], "dash_dot");
    let (mut seen, mut dots) = (0, 0);
    for x in 5..195 {
        // The line starts 50 px left of the picture.
        let at = (50.0 + x as f64 + 0.5).rem_euclid(16.0);
        let ink = ink_at(&px, x, 19..=20);
        if (1.0..7.0).contains(&at) {
            assert!(ink < 200, "no dash at {x} ({at}): {ink}");
            seen += 1;
        } else if (11.4..12.6).contains(&at) {
            assert!(ink < 200, "no dot at {x} ({at}): {ink}");
            dots += 1;
        } else if (8.6..10.4).contains(&at) || (13.6..15.4).contains(&at) {
            assert!(ink > 230, "no gap at {x} ({at}): {ink}");
        }
    }
    assert!(seen > 40 && dots > 8, "{seen} {dots}");
}

/// An anchored dashed line starts and ends with a dash, whatever its length; its pattern
/// barely stretched.
#[test]
fn anchored_dashes_start_and_end_the_line() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    for end in [70.3, 74.0, 77.1, 80.6] {
        let line = Line {
            a: DVec3::new(-83.3, 0.0, 0.0),
            b: DVec3::new(end, 0.0, 0.0),
            color: [0.0, 0.0, 0.0, 1.0],
            dash: Some(Dash::even(5.0, Lay::Anchored)),
            width: LINE_WIDTH,
        };
        let px = draw(&mut r, &device, &queue, [200, 40], &flat_frame(&[], &[line], &[], &[]));
        let first = (100.0 - 83.3 + 1.0) as u32;
        let last = (100.0 + end - 1.0) as u32;
        assert!(ink_at(&px, first, 19..=20) < 200, "{end}: no dash at the start");
        assert!(ink_at(&px, last, 19..=20) < 200, "{end}: no dash at the end");
        // Dashes and gaps between.
        let gaps = (first..=last).filter(|x| ink_at(&px, *x, 19..=20) > 230).count();
        assert!((60..=90).contains(&gaps), "{end}: {gaps} gap pixels");
    }
}

/// A box under a slab, tilted so that its four edges along X fall apart on screen: in the
/// scene, the slab hides them; with `shell_on_top`, the edges of its faces turned to the eye
/// are drawn over the slab, the back one (of no such face) stays hidden.
#[test]
fn box_fronts_are_drawn_over_the_scene() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let batches = slab(&mut r, &device);
    // The unit cube 120 × 10 × 6, turned 30° about X, below the slab.
    let transform = DMat4::from_translation(DVec3::new(-60.0, 0.0, -20.0))
        * DMat4::from_rotation_x(30f64.to_radians())
        * DMat4::from_scale(DVec3::new(120.0, 10.0, 6.0));
    let plain = BoxOutline::new(transform, [0.0, 0.0, 0.0, 1.0], LINE_WIDTH);
    // The edges along X by their corner (y, z): screen rows from the world y.
    let row = |y: f64, z: f64| {
        let p = transform.transform_point3(DVec3::new(0.0, y, z));
        (20.0 - p.y).floor() as u32
    };
    let rows = |y, z| row(y, z).saturating_sub(1)..=row(y, z) + 1;
    let exposed = [(1.0, 1.0), (0.0, 1.0), (1.0, 0.0)];
    let back = (0.0, 0.0);
    for shell in [false, true] {
        let b = BoxOutline { shell_on_top: shell, ..plain };
        let px = draw(&mut r, &device, &queue, [200, 40], &flat_frame(&batches, &[], &[b], &[]));
        save(&px, [200, 40], if shell { "box_front" } else { "box_hidden" });
        // Beside the slab, every edge is seen.
        for (y, z) in exposed.iter().chain([&back]) {
            assert!(ink_at(&px, 50, rows(*y, *z)) < 170, "shell {shell}: edge {y} {z} not seen beside the slab");
        }
        // Under it, only the exposed faces' edges, and only drawn over the scene.
        for (y, z) in exposed {
            let ink = ink_at(&px, 100, rows(y, z));
            assert_eq!(ink < 170, shell, "shell {shell}: edge {y} {z} under the slab: {ink}");
            assert!(shell || ink > 200, "shell {shell}: edge {y} {z} under the slab: {ink}");
        }
        assert!(ink_at(&px, 100, rows(back.0, back.1)) > 200, "shell {shell}: the back edge is drawn over the slab");
    }
}

/// A box drawn by its corners: each edge only so far from its ends.
#[test]
fn box_corners_stop_short_of_the_middle() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let outline = BoxOutline {
        corners: Some(10.0),
        ..BoxOutline::new(
            DMat4::from_translation(DVec3::new(-80.0, -15.0, 0.0)) * DMat4::from_scale(DVec3::new(160.0, 30.0, 1.0)),
            [0.0, 0.0, 0.0, 1.0],
            LINE_WIDTH,
        )
    };
    let px = draw(&mut r, &device, &queue, [200, 40], &flat_frame(&[], &[], &[outline], &[]));
    save(&px, [200, 40], "box_corners");
    // The top edge (row 5): inked near its ends, not past 10 px from them.
    assert!(ink_at(&px, 25, 4..=5) < 200 && ink_at(&px, 175, 4..=5) < 200);
    assert!((32..168).all(|x| ink_at(&px, x, 4..=5) > 230));
}

/// Fills over the scene show over a slab; with `cull_back`, a triangle whose back is turned to
/// the eye is left out, one whose front is drawn.
#[test]
fn fills_over_the_scene_and_their_backs() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let batches = slab(&mut r, &device);
    let c = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
    let red = [1.0, 0.0, 0.0, 0.5];
    let under = c(0.0, 0.0, -10.0);
    let fills = [
        // Under the slab, over the scene.
        Fill {
            over: true,
            ..Fill::polygon(
                vec![under + c(-5.0, -5.0, 0.0), under + c(5.0, -5.0, 0.0), under + c(5.0, 5.0, 0.0), under + c(-5.0, 5.0, 0.0)],
                red,
            )
        },
        // Counter-clockwise seen from the eye (its front), then clockwise (its back).
        Fill {
            triangles: true,
            cull_back: true,
            ..Fill::polygon(vec![c(-80.0, -10.0, 0.0), c(-60.0, -10.0, 0.0), c(-70.0, 10.0, 0.0)], red)
        },
        Fill { triangles: true, cull_back: true, ..Fill::polygon(vec![c(60.0, -10.0, 0.0), c(70.0, 10.0, 0.0), c(80.0, -10.0, 0.0)], red) },
    ];
    let px = draw(&mut r, &device, &queue, [200, 40], &flat_frame(&batches, &[], &[], &fills));
    save(&px, [200, 40], "fills_over");
    let reddish = |x, y| {
        let [r, g, b, _] = pixel(&px, 200, x, y);
        r as i32 > g as i32 + 40 && r as i32 > b as i32 + 40
    };
    assert!(reddish(100, 20), "the fill over the scene is hidden by the slab");
    assert!(reddish(30, 22), "the front of the triangle is not drawn");
    assert!(!reddish(170, 22), "the back of the triangle is drawn");
}

#[test]
fn hatch_directions_spread_over_the_keys() {
    // Ids go by fours in a document (part, body definition, body, occurrence): every direction
    // still comes, about as often.
    let mut count = [0usize; HATCH_DIRECTIONS as usize];
    for key in (4..4 * 600).step_by(4) {
        count[hatch_of(key) as usize] += 1;
    }
    assert!(count.iter().all(|&n| (70..=130).contains(&n)), "{count:?}");
}

/// Where the world point `p` falls in a picture of `size` drawn with `proj` from camera space
/// (pixels, rows running down).
fn on_picture(proj: Mat4, size: [u32; 2], p: DVec3) -> [f64; 2] {
    let c = proj.as_dmat4() * p.extend(1.0);
    [(c.x / c.w + 1.0) / 2.0 * size[0] as f64, (1.0 - c.y / c.w) / 2.0 * size[1] as f64]
}

/// The segment `a`..`b` (camera space) dashed `dash` / `gap` pixels, anchored with `tail` of a
/// dash at `b` (a whole one for a line, half for a box's half edge): its dashes and gaps laid
/// evenly on the segment itself — in perspective, shorter on screen as it runs away — their
/// count set by its length on screen.
fn assert_dashes_on_segment(px: &[u8], size: [u32; 2], proj: Mat4, a: DVec3, b: DVec3, dash: f64, gap: f64, tail: f64) {
    let dist = |p: [f64; 2], q: [f64; 2]| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
    let len = dist(on_picture(proj, size, a), on_picture(proj, size, b));
    let period = dash + gap;
    let span = ((len - tail) / period).round().max(0.0) * period + tail;
    let ink = |[x, y]: [f64; 2]| {
        let (x, y) = (x as i64, y as i64);
        (x - 1..=x + 1).flat_map(|x| (y - 1..=y + 1).map(move |y| pixel(px, size[0], x as u32, y as u32)[0])).min().unwrap()
    };
    let (mut u, mut on, mut checked) = (0.0, true, [0, 0]);
    while u < span - 1e-9 {
        let end = (u + if on { dash } else { gap }).min(span);
        let at = |u: f64| on_picture(proj, size, a.lerp(b, u / span));
        // Long enough on screen to look into its middle.
        if dist(at(u), at(end)) >= 8.0 {
            let mid = at((u + end) / 2.0);
            let v = ink(mid);
            if on {
                assert!(v < 200, "no dash at {u:.1}..{end:.1} of {span:.1} ({mid:?}): {v}");
            } else {
                assert!(v > 230, "no gap at {u:.1}..{end:.1} of {span:.1} ({mid:?}): {v}");
            }
            checked[on as usize] += 1;
        }
        (u, on) = (end, !on);
    }
    assert!(checked[0] >= 2 && checked[1] >= 2, "{checked:?} gaps and dashes checked, {len:.0} px");
}

/// Seen in perspective, a line running away and a box's edge running away keep their dashes on
/// themselves (not evenly on screen): they hold still on them as the view orbits.
#[test]
fn anchored_dashes_lie_on_the_line_in_perspective() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let size = [400, 400];
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, 1.0, 0.1);
    // The camera at the origin looking down -Z.
    fn frame<'a>(proj: Mat4, lines: &'a [Line], boxes: &'a [BoxOutline]) -> Frame<'a> {
        Frame { view: DMat4::IDENTITY, proj, ..flat_frame(&[], lines, boxes, &[]) }
    }
    let line = Line {
        a: DVec3::new(-1.0, -1.0, -4.0),
        b: DVec3::new(2.0, -1.0, -20.0),
        color: [0.0, 0.0, 0.0, 1.0],
        dash: Some(Dash::even(12.0, Lay::Anchored)),
        width: LINE_WIDTH,
    };
    let px = draw(&mut r, &device, &queue, size, &frame(proj, &[line], &[]));
    save(&px, size, "anchored_perspective");
    assert_dashes_on_segment(&px, size, proj, line.a, line.b, 12.0, 12.0, 12.0);

    // A box 2 × 2 × 32 running away: its near half edge from (-1, -1, -4).
    let outline = BoxOutline {
        dash: Some(Dash::even(12.0, Lay::Model(0.0))),
        ..BoxOutline::new(
            DMat4::from_translation(DVec3::new(-1.0, -1.0, -36.0)) * DMat4::from_scale(DVec3::new(2.0, 2.0, 32.0)),
            [0.0, 0.0, 0.0, 1.0],
            LINE_WIDTH,
        )
    };
    let px = draw(&mut r, &device, &queue, size, &frame(proj, &[], &[outline]));
    save(&px, size, "box_perspective");
    assert_dashes_on_segment(&px, size, proj, DVec3::new(-1.0, -1.0, -4.0), DVec3::new(-1.0, -1.0, -20.0), 12.0, 12.0, 6.0);
}

/// A line dashed in model units, running away in perspective, seen from two views (an orbit):
/// one period all along it (the power of two nearest to the pattern's length at the focus), at
/// every point of it dash or gap as its phase there says — the same points of the line inked in
/// both views; where the dashes are a few pixels long, a lighter line.
#[test]
fn model_dashes_hold_still_on_a_line_as_the_view_orbits() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::new(&device);
    let size = [400, 400];
    let proj = Mat4::perspective_infinite_reverse_rh(0.8, 1.0, 0.1);
    let (a, b, phase) = (DVec3::new(-1.0, -1.0, -4.0), DVec3::new(2.0, -1.0, -20.0), 3.7);
    let line = Line { a, b, color: [0.0, 0.0, 0.0, 1.0], dash: Some(Dash::even(5.0, Lay::Model(phase))), width: LINE_WIDTH };
    // 10 points at the focus: 0.5 model units, a period of 0.5.
    const MPP: f64 = 0.05;
    let period = 0.5;
    let mut checked = 0;
    for (i, view) in [DMat4::IDENTITY, DMat4::from_rotation_y(0.08) * DMat4::from_rotation_x(-0.05)].into_iter().enumerate() {
        let frame = Frame { view, proj, model_per_point: MPP, ..flat_frame(&[], std::slice::from_ref(&line), &[], &[]) };
        let px = draw(&mut r, &device, &queue, size, &frame);
        save(&px, size, &format!("model_dashes_{i}"));
        let mut faded = false;
        let at = |t: f64| on_picture(proj, size, view.transform_point3(a.lerp(b, t)));
        let len = a.distance(b);
        for k in 1..2000 {
            let t = k as f64 / 2000.0;
            let [x, y] = at(t);
            // Model units a pixel along the line there.
            let [x1, y1] = at(t + 1e-4);
            let per_px = len * 1e-4 / ((x1 - x).powi(2) + (y1 - y).powi(2)).sqrt();
            let (xi, yi) = (x as i64, y as i64);
            let ink = (xi - 1..=xi + 1)
                .flat_map(|x| (yi - 1..=yi + 1).map(move |y| (x, y)))
                .map(|(x, y)| pixel(&px, size[0], x as u32, y as u32)[0])
                .min()
                .unwrap();
            if period / per_px < 3.0 {
                // Faded: a lighter line, neither ink nor gap.
                assert!(ink > 40 && ink < 230, "view {i}: not faded at {t}: {ink}");
                faded = true;
                continue;
            }
            if period / per_px < 6.0 {
                continue; // fading
            }
            let u = (phase + t * len).rem_euclid(period);
            // Away from the dash's ends by more than the pixels looked at.
            let edge = u.min((u - period / 2.0).abs()).min(period - u) / per_px;
            if edge < 2.5 || !(2.0..398.0).contains(&x) || !(2.0..398.0).contains(&y) {
                continue;
            }
            if u < period / 2.0 {
                assert!(ink < 200, "view {i}: no dash at {t} ({x:.1}, {y:.1}): {ink} u {u:.4} edge {edge:.2}");
            } else {
                assert!(ink > 230, "view {i}: no gap at {t} ({x:.1}, {y:.1}): {ink} u {u:.4} edge {edge:.2}");
            }
            checked += 1;
        }
        assert!(faded, "view {i}: never faded");
    }
    assert!(checked > 100, "{checked} points checked");
}

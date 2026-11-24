//! The views' pictures: what is asked of the renderer, and printed.

use super::*;

/// What a view's picture and lines are kept by: the view and, for a projected one, the projection
/// of the presentation it is drawn in. A composition's projected view may so look two ways on the
/// board at once, and a view whose direction changes keeps its last picture until the next.
pub type ViewSlot = (Id, Option<Projection>);

/// The slot of view `id` (`view`) drawn with `projection`.
pub(super) fn slot_of(id: Id, view: &View, projection: Projection) -> ViewSlot {
    (id, matches!(view.kind, ViewKind::Projected { .. }).then_some(projection))
}

/// Of pictures (key, bytes, the frame they were last asked for), those to free for the rest to
/// fit in `budget`, the least recently asked first; none asked at frame `now`.
pub fn over_budget<K: Copy>(pictures: impl Iterator<Item = (K, u64, u64)>, now: u64, budget: u64) -> Vec<K> {
    let mut all: Vec<(K, u64, u64)> = pictures.collect();
    let mut total: u64 = all.iter().map(|p| p.1).sum();
    all.sort_by_key(|p| p.2);
    let mut out = Vec::new();
    for (key, bytes, seen) in all {
        if total <= budget || seen >= now {
            break;
        }
        total -= bytes;
        out.push(key);
    }
    out
}

/// A view's picture the 3D renderer is asked for: its subject's parts seen in its frame (the
/// *Rendered* and *Hybrid* styles) and its lines over them (*Lines*, *Hybrid*), framed exactly
/// on its box (what its frame on the sheet shows), at `size` pixels; drawn again when `stamp`
/// changes.
pub struct Wanted {
    pub slot: ViewSlot,
    pub size: [u32; 2],
    /// Subject axes to eye, and the projection (reversed depth).
    pub camera: (DMat4, glam::Mat4),
    /// Per part (its definition): its placements in the subject's axes and their materials.
    pub parts: Vec<(Id, Vec<Placed>)>,
    pub lines: Option<PictureLines>,
    /// The texture images it shows: it is drawn again when one more of them is ready.
    pub images: Arc<[String]>,
    pub stamp: u64,
}

/// A view's lines in its picture: in its frame (`right`, `up`, model units), laid at `depth`
/// along `forward`; the widths and dashes in points.
pub struct PictureLines {
    pub lines: Arc<skraft_tools::hidden_lines::ViewLines>,
    pub axes: [DVec3; 3],
    pub depth: f64,
    /// In perspective, the eye: the lines are tangents off its axis, laid `depth` from it.
    pub eye: Option<DVec3>,
    /// The hidden ones drawn too (*Lines*), dashed.
    pub hidden: bool,
    pub width: f32,
    pub hidden_width: f32,
    pub dash: [f32; 2],
}

/// Line widths on paper (millimetres): visible, hidden; the hidden lines' dash and gap.
const LINE_MM: f64 = 0.35;

const HIDDEN_MM: f64 = 0.18;

const DASH_MM: [f64; 2] = [2.0, 1.0];

/// The largest side of a view's picture, in pixels.
const MAX_PICTURE: f64 = 4096.0;

/// A perspective view's near plane, short of its subject's box (`r`'s frame: x, y, depth).
fn lens_near(r: &Resolved<'_>, (lo, hi): (DVec3, DVec3)) -> f64 {
    let from_eye = r.perspective.map_or(0.0, |p| lo.z - p.eye.dot(r.frame.forward));
    (from_eye * 0.9).max((hi.z - lo.z) * 1e-3).max(1e-3)
}

/// The camera seeing the box `(lo, hi)` (in `r`'s frame: x, y, depth) exactly across, parallel.
fn picture_camera(r: &Resolved<'_>, (lo, hi): (DVec3, DVec3)) -> (DMat4, glam::Mat4) {
    let f = &r.frame;
    if let Some(p) = r.perspective {
        // Its eye and opening, across its own frame; the near plane short of the subject.
        let view = glam::dcamera::rh::view::look_to_mat4(p.eye, f.forward, f.up);
        let near = lens_near(r, (lo, hi));
        let aspect = p.size.x / p.size.y;
        let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(p.fov_y as f32, aspect as f32, near as f32);
        return (view, cropped_proj(r.crop, proj, p.size / 2.0));
    }
    let c = (lo + hi) / 2.0;
    let half = ((hi - lo) / 2.0).max(DVec3::splat(1e-3));
    let centre = f.right * c.x + f.up * c.y + f.forward * c.z;
    let back = half.z + half.x.max(half.y) + 1.0;
    let eye = centre - f.forward * back;
    let view = glam::dcamera::rh::view::look_to_mat4(eye, f.forward, f.up);
    let (near, far) = ((back - half.z * 1.05 - 1.0).max(1e-3), back + half.z * 1.05 + 1.0);
    // Reversed depth: near → 1, far → 0.
    let proj = glam::camera::rh::proj::directx::orthographic(
        -half.x as f32,
        half.x as f32,
        -half.y as f32,
        half.y as f32,
        far as f32,
        near as f32,
    );
    (view, cropped_proj(r.crop, proj, half.truncate() * r.scale))
}

/// What takes clip space's window `lo`–`hi` (x, y from -1 to 1) to the whole of it.
pub fn window_matrix(lo: DVec2, hi: DVec2) -> glam::Mat4 {
    let size = hi - lo;
    let (scale, shift) = (DVec2::splat(2.0) / size, -(hi + lo) / size);
    glam::Mat4::from_cols(
        glam::Vec4::new(scale.x as f32, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, scale.y as f32, 0.0, 0.0),
        glam::Vec4::Z,
        glam::Vec4::new(shift.x as f32, shift.y as f32, 0.0, 1.0),
    )
}

/// `proj`, whose picture is `half` wide and high about its centre on the paper, seeing `crop` alone.
fn cropped_proj(crop: Option<skraft_model::Crop>, proj: glam::Mat4, half: DVec2) -> glam::Mat4 {
    crop.map_or(proj, |c| window_matrix(c.lo / half, c.hi / half) * proj)
}

/// A side of a picture in pixels, in steps of a fifth (a zoom redraws it only past a step).
fn picture_side(px: f64) -> u32 {
    let step = 1.2f64;
    let q = step.powf((px.max(8.0).ln() / step.ln()).round());
    q.min(MAX_PICTURE).round() as u32
}

/// The picture of view slot `slot` (`style`, showing `data`) asks for, its frame `frame` points
/// wide and high on screen (turned or not), its lines (`lines`, as last computed) over its parts.
pub(super) fn wanted_picture(
    slot: ViewSlot,
    style: ViewStyle,
    r: &Resolved<'_>,
    data: &ViewData,
    lines: Option<Arc<skraft_tools::hidden_lines::ViewLines>>,
    frame: egui::Vec2,
    ppp: f32,
) -> Option<Wanted> {
    let b = data.bx3?;
    let size = [picture_side((frame.x * ppp) as f64), picture_side((frame.y * ppp) as f64)];
    // Points per paper millimetre in the picture.
    let on_paper = frame_of(r, Some((b.0.truncate(), b.1.truncate())));
    let frame_mm = (on_paper.1.x - on_paper.0.x).max(1e-9);
    let ppm = size[0] as f64 / ppp as f64 / frame_mm;
    let f = &r.frame;
    let lines = lines.filter(|_| style != ViewStyle::Rendered).map(|l| PictureLines {
        lines: l,
        axes: [f.right, f.up, f.forward],
        // In front of every part; in perspective, in the middle of them, drawn over all the same.
        depth: r.perspective.map_or(b.0.z - 1.0, |p| ((b.0.z + b.1.z) / 2.0 - p.eye.dot(f.forward)).max(lens_near(r, b) * 1.5)),
        eye: r.perspective.map(|p| p.eye),
        hidden: style == ViewStyle::Lines,
        width: (LINE_MM * ppm).max(1.0) as f32,
        hidden_width: (HIDDEN_MM * ppm).max(0.6) as f32,
        dash: [(DASH_MM[0] * ppm).max(3.0) as f32, (DASH_MM[1] * ppm).max(2.0) as f32],
    });
    let (parts, images) = if style == ViewStyle::Lines { (Vec::new(), Arc::default()) } else { (data.parts.clone(), data.images.clone()) };
    let camera = picture_camera(r, b);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (e_key(r), size, slot.0.0, style as u8, lines.as_ref().map(|l| Arc::as_ptr(&l.lines) as usize)).hash(&mut h);
    // Its box moves the camera; a picture of lines alone does not show the parts' look.
    (camera.0.to_cols_array().map(f64::to_bits), camera.1.to_cols_array().map(f32::to_bits)).hash(&mut h);
    if style != ViewStyle::Lines {
        data.looks.hash(&mut h);
    }
    data.parts.len().hash(&mut h);
    for (p, places) in &data.parts {
        p.0.hash(&mut h);
        for (t, m) in places {
            t.to_cols_array().map(f64::to_bits).hash(&mut h);
            m.map(|m| m.0).hash(&mut h);
        }
    }
    Some(Wanted { slot, size, camera, parts, lines, images, stamp: h.finish() })
}

/// What a view puts on paper (the PDF's, docs/ui/drawing.md#pdf): its frame (sheet
/// millimetres), its picture asked for (its parts, no lines: *Rendered*, *Hybrid*), its lines
/// computed now (*Lines*, *Hybrid*) in its frame, the centre of its box there.
pub struct Printed {
    pub frame: (DVec2, DVec2),
    pub picture: Option<Wanted>,
    pub lines: Option<skraft_tools::hidden_lines::ViewLines>,
    pub centre: DVec2,
    /// Paper millimetres per unit of its lines, from `centre`.
    pub to_paper: f64,
}

/// What view `id` (`r`, drawn in `style`) puts on paper at `dpi`; none while its box is not
/// known.
pub fn printed(e: &Editor, id: Id, style: ViewStyle, r: &Resolved<'_>, dpi: f64) -> Option<Printed> {
    let placed = e.document().subject_parts(r.subject);
    let b = box_of(e, r, &placed)?;
    let frame = frame_of(r, Some((b.0.truncate(), b.1.truncate())));
    let picture = (style != ViewStyle::Lines).then(|| {
        let px = |mm: f64| (mm * dpi / 25.4).round().clamp(8.0, 8192.0) as u32;
        let mut by_part: HashMap<Id, Vec<Placed>> = HashMap::new();
        for p in &placed {
            by_part.entry(p.part).or_default().push((p.transform, p.material));
        }
        let mut parts: Vec<_> = by_part.into_iter().collect();
        parts.sort_by_key(|(p, _)| *p);
        let size = [px(frame.1.x - frame.0.x), px(frame.1.y - frame.0.y)];
        Wanted { slot: (id, None), size, camera: picture_camera(r, b), parts, lines: None, images: Arc::default(), stamp: 0 }
    });
    let lines =
        if style == ViewStyle::Rendered { None } else { crate::drawing::view_lines::LinesJob::of(e, r, &placed).map(|job| job.run()) };
    // A perspective's lines are tangents about its axis, on the frame's height.
    let (centre, to_paper) = match r.perspective {
        Some(p) => (DVec2::ZERO, p.size.y / (2.0 * (p.fov_y / 2.0).tan())),
        None => (((b.0 + b.1) / 2.0).truncate(), r.scale),
    };
    // A parallel view's lines are the whole subject's: on paper, what its frame holds.
    let lines = match (r.crop, r.perspective) {
        (Some(c), None) => lines.map(|l| l.clipped(centre + c.lo / r.scale, centre + c.hi / r.scale)),
        _ => lines,
    };
    Some(Printed { frame, picture, lines, centre, to_paper })
}

/// What of a view's look changes its picture and lines: its direction, and a perspective's eye,
/// opening and frame.
pub(super) fn e_key(r: &Resolved<'_>) -> [u64; 19] {
    let f = &r.frame;
    let mut k = [0u64; 19];
    for (i, v) in [f.right, f.up, f.forward].iter().enumerate() {
        k[3 * i..3 * i + 3].copy_from_slice(&v.to_array().map(f64::to_bits));
    }
    if let Some(p) = r.perspective {
        k[9..15].copy_from_slice(&[p.eye.x, p.eye.y, p.eye.z, p.fov_y, p.size.x, p.size.y].map(f64::to_bits));
    }
    if let Some(c) = r.crop {
        k[15..19].copy_from_slice(&[c.lo.x, c.lo.y, c.hi.x, c.hi.y].map(f64::to_bits));
    }
    k
}

/// A picture on the four corners `quad` (its top left first, clockwise on screen).
pub(super) fn draw_picture(painter: &egui::Painter, texture: egui::TextureId, quad: [egui::Pos2; 4]) {
    let mut mesh = egui::Mesh::with_texture(texture);
    let uvs = [egui::pos2(0.0, 0.0), egui::pos2(1.0, 0.0), egui::pos2(1.0, 1.0), egui::pos2(0.0, 1.0)];
    for (pos, uv) in quad.into_iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: egui::Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

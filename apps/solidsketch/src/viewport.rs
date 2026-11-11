//! The 3D viewport: input to the editor (SketchUp navigation, tool clicks), the editor's view
//! to the renderer, the renderer's off-screen target shown as an egui image.

use std::collections::HashMap;

use glam::{DMat4, DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::Id;
use solidsketch_render as render;
use solidsketch_theme::{Rgba, Theme};
use solidsketch_tools::{Editor, InferKind, Look, Mark, Modifiers, ToolInput, ToolKind};

use crate::prefs::Navigation;

/// A token as an egui colour (sRGB, unmultiplied alpha).
pub fn color(c: Rgba) -> egui::Color32 {
    let [r, g, b, a] = solidsketch_theme::to_srgba8(c);
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// How long without trackpad scrolling ends a trackpad gesture (seconds): the next one picks
/// a new orbit pivot.
const GESTURE_GAP: f64 = 0.3;

#[derive(Default)]
pub struct Viewport {
    renderer: Option<render::Renderer>,
    texture: Option<(egui::TextureId, u64)>,
    last_hover: Option<egui::Pos2>,
    last_trackpad: f64,
    /// Each material's palette entry, for the session: an entry is never given to another
    /// material, so meshes with painted faces stay valid when materials come and go.
    slots: HashMap<Id, u32>,
    /// The size of the last frame drawn (pixels).
    size: [u32; 2],
}

/// `c` mixed with the background by `fade` (what is outside the context being edited).
fn faded(c: Rgba, background: [f32; 3], fade: f32) -> Rgba {
    let mix = |k: usize| c[k] + (background[k] - c[k]) * fade;
    [mix(0), mix(1), mix(2), c[3]]
}

/// What the copy modifier is on this platform (SketchUp: Option on macOS, Ctrl elsewhere).
pub fn modifiers(ctx: &egui::Context, m: egui::Modifiers) -> Modifiers {
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    Modifiers { toggle: m.shift, copy: if mac { m.alt } else { m.ctrl } }
}

impl Viewport {
    pub fn new() -> Self {
        Viewport::default()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, editor: &mut Editor, nav: &Navigation, theme: &Theme) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        editor.set_viewport_size(DVec2::new(rect.width() as f64, rect.height() as f64));
        self.input(ui, &response, rect, editor, nav);

        let cursor = match editor.tool_kind() {
            _ if editor.busy() => egui::CursorIcon::Progress,     // the kernel is at work
            _ if editor.tool_copying() => egui::CursorIcon::Copy, // SketchUp's « + »
            // Primitives keep the arrow: the crosshair hides the snap point.
            ToolKind::Select | ToolKind::Box | ToolKind::Cylinder => egui::CursorIcon::Default,
            ToolKind::Move => egui::CursorIcon::Move,
            ToolKind::Paint => egui::CursorIcon::PointingHand,
            _ => egui::CursorIcon::Crosshair,
        };
        if response.hovered() {
            ui.ctx().set_cursor_icon(if response.dragged_by(egui::PointerButton::Middle) { egui::CursorIcon::Grabbing } else { cursor });
        }

        let ppp = ui.ctx().pixels_per_point();
        let size = [(rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32];
        let Some(state) = frame.wgpu_render_state() else {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, t!("status.no_wgpu"), Default::default(), egui::Color32::RED);
            return;
        };
        self.render(state, size, ppp, editor, theme);
        if let Some((id, _)) = self.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(id, rect, uv, egui::Color32::WHITE);
        }
        paint_marks(ui.painter(), rect, editor);
        let mut clear = Vec::new();
        if let Some(cursor) = response.hover_pos() {
            clear.push(egui::Rect::from_center_size(cursor, egui::vec2(CURSOR_CLEAR, CURSOR_CLEAR)));
            clear.extend(paint_inference(ui.painter(), rect, cursor, editor, theme));
        }
        if let Some(m) = editor.marquee() {
            paint_marquee(ui.painter(), rect, &m, theme);
        }
        // Last: the lengths stay above everything (markers, tooltip, marquee), placed clear of
        // the cursor, the markers, the tooltip and the crosses.
        paint_labels(ui.painter(), rect, editor, clear);
    }

    fn input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: egui::Rect, editor: &mut Editor, nav: &Navigation) {
        let ctx = ui.ctx();
        let local = |p: egui::Pos2| DVec2::new((p.x - rect.min.x) as f64, (p.y - rect.min.y) as f64);
        // The preferences: orbit direction and speed per device, zoom speed and target.
        let sign = |invert: bool| if invert { -1.0 } else { 1.0 };
        let mouse_orbit = DVec2::new(sign(nav.mouse_invert_x), sign(nav.mouse_invert_y)) * nav.orbit_speed;
        let trackpad_orbit = DVec2::new(sign(nav.trackpad_invert_x), sign(nav.trackpad_invert_y)) * nav.orbit_speed;
        let zoom_at =
            |p: egui::Pos2| if nav.zoom_to_cursor { local(p) } else { DVec2::new(rect.width() as f64, rect.height() as f64) / 2.0 };
        let zoom_steps = nav.zoom_speed * sign(nav.wheel_invert);
        let (mods, events, time, zoom) = ui.input(|i| (i.modifiers, i.events.clone(), i.time, i.zoom_delta()));
        // Shift held locks the line inferred (§6 ter); a point hovered a while is tracked: the
        // editor needs the time, every frame while it waits.
        editor.set_shift(mods.shift);
        editor.set_copy(modifiers(ctx, mods).copy);
        if editor.tick(time) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let hover = response.hover_pos();

        // Hover: tools follow the cursor; off the view or over a panel, they hide what follows it.
        match hover {
            Some(p) if self.last_hover != Some(p) => {
                self.last_hover = Some(p);
                editor.input(ToolInput::Hover { px: local(p) });
            }
            None if self.last_hover.take().is_some() => editor.leave(),
            _ => {}
        }

        // Middle button: orbit, Shift = pan; double-click = recentre.
        use egui::PointerButton::{Middle, Primary};
        if response.drag_started_by(Middle)
            && let Some(p) = response.interact_pointer_pos()
        {
            editor.nav_begin(local(p));
        }
        if response.dragged_by(Middle) {
            let d = response.drag_delta();
            let d = DVec2::new(d.x as f64, d.y as f64);
            if mods.shift { editor.pan(d) } else { editor.orbit(d * mouse_orbit) }
        }
        if response.double_clicked_by(Middle)
            && let Some(p) = response.interact_pointer_pos()
        {
            editor.recenter(local(p));
        }

        // Primary button: the tool (clicks; drags from where the button went down).
        if let Some(p) = response.interact_pointer_pos() {
            let m = modifiers(ctx, mods);
            if response.double_clicked_by(Primary) {
                editor.input(ToolInput::DoubleClick { px: local(p), mods: m });
            } else if response.clicked_by(Primary) {
                editor.input(ToolInput::Click { px: local(p), mods: m });
            } else if response.drag_started_by(Primary) {
                let from = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
                editor.input(ToolInput::DragStart { px: local(from), mods: m });
                editor.input(ToolInput::Drag { px: local(p) });
            } else if response.dragged_by(Primary) && response.drag_delta() != egui::Vec2::ZERO {
                editor.input(ToolInput::Drag { px: local(p) });
            }
            // Started and released in the same frame: ended too.
            if response.drag_stopped_by(Primary) {
                editor.input(ToolInput::DragEnd { px: local(p), mods: m });
            }
        }

        // Wheel = zoom; trackpad scroll = orbit (Shift: pan); pinch = zoom.
        let Some(p) = hover else { return };
        for e in &events {
            let egui::Event::MouseWheel { unit, delta, modifiers, .. } = e else { continue };
            if modifiers.command || modifiers.ctrl {
                continue; // egui turns these into zoom_delta
            }
            match unit {
                egui::MouseWheelUnit::Point => {
                    if time - self.last_trackpad > GESTURE_GAP {
                        editor.nav_begin(local(p));
                    }
                    self.last_trackpad = time;
                    let d = DVec2::new(-delta.x as f64, -delta.y as f64);
                    if modifiers.shift { editor.pan(-d) } else { editor.orbit(d * trackpad_orbit) }
                }
                egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                    editor.zoom(zoom_at(p), delta.y as f64 * zoom_steps);
                }
            }
        }
        if (zoom - 1.0).abs() > 1e-4 {
            editor.zoom(zoom_at(p), (zoom as f64).ln() / (1.0f64 / 0.85).ln() * nav.zoom_speed);
        }
    }

    /// The palette entry of `material`, given on first use (§7 quater).
    fn slot(&mut self, material: Option<Id>) -> u32 {
        let Some(m) = material else { return render::NO_MATERIAL };
        let next = self.slots.len() as u32;
        *self.slots.entry(m).or_insert(next)
    }

    fn render(&mut self, state: &egui_wgpu::RenderState, size: [u32; 2], pixels_per_point: f32, editor: &Editor, theme: &Theme) {
        let device = &state.device;
        let doc = editor.document();
        // The palette: every material at its entry (entries of deleted ones stay unused).
        let materials: Vec<(Id, [u8; 3])> = doc.materials().map(|(id, m)| (id, m.color)).collect();
        for (id, _) in &materials {
            self.slot(Some(*id));
        }
        let mut palette = vec![[0.0f32; 4]; self.slots.len()];
        for (id, [r, g, b]) in &materials {
            palette[self.slots[id] as usize] = solidsketch_theme::srgb(*r, *g, *b);
        }
        // Then the colours of an open part's tools (§2.9).
        let tool_slot = palette.len() as u32;
        palette.push(theme.machining);
        palette.push(theme.machining_failed);

        // Meshes: one per definition, re-uploaded when its revision (faces' paint included)
        // changes.
        let scene = editor.scene();
        let mut uploads = Vec::new();
        for dm in scene.definition_meshes() {
            if self.renderer.as_ref().is_some_and(|r| r.has_mesh(dm.definition.0, dm.revision)) {
                continue;
            }
            let len = dm.faces.keys().next_back().map_or(0, |f| *f as usize + 1);
            let mut paint = vec![render::NO_MATERIAL; len];
            for (f, m) in &dm.faces {
                paint[*f as usize] = self.slot(Some(*m));
            }
            uploads.push((dm, paint));
        }
        let renderer = self.renderer.get_or_insert_with(|| render::Renderer::new(device));
        for (dm, paint) in &uploads {
            let parts: Vec<render::Part<'_>> =
                dm.parts.iter().map(|p| render::Part { id: p.solid.0, mesh: &p.mesh, face_paint: paint }).collect();
            renderer.set_mesh(device, dm.definition.0, dm.revision, &parts);
        }
        renderer.retain_meshes(|k| scene.definition_mesh(solidsketch_model::Id(k)).is_some());

        let display = &doc.settings().display;
        let [r, g, b] = display.background;
        let bg = egui::Rgba::from(egui::Color32::from_rgb(r, g, b)); // linear
        let background = [bg.r(), bg.g(), bg.b()];
        let view = editor.view();
        let fade: render::Tint = [background[0], background[1], background[2], theme.fade];
        let slots = &self.slots;
        let slot = |m: Option<Id>| m.and_then(|m| slots.get(&m).copied()).unwrap_or(render::NO_MATERIAL);
        let batches: Vec<render::Batch> = view
            .batches
            .iter()
            .map(|(def, places)| render::Batch {
                key: def.0,
                placements: places
                    .iter()
                    .map(|p| render::Placement {
                        transform: DMat4::from(p.transform),
                        tint: if p.faded { fade } else { render::NO_TINT },
                        material: match p.look {
                            Look::Tool => tool_slot,
                            Look::FailedTool => tool_slot + 1,
                            Look::Shape | Look::Blank => slot(p.material),
                        },
                    })
                    .collect(),
                ghost: places.iter().any(|p| p.look == Look::Blank),
            })
            .collect();
        let highlights: Vec<render::Highlight> = view
            .highlights
            .iter()
            .map(|h| {
                let colors = match h.mark {
                    Mark::Selected => theme.selected,
                    Mark::Hover => theme.hover,
                    Mark::Picked => theme.picked,
                    Mark::Ghost => theme.ghost,
                    // The other copies a change reaches: the picked target's orange, lighter.
                    Mark::Related => theme.related,
                };
                render::Highlight {
                    key: h.definition.0,
                    part: h.solid.0,
                    transform: DMat4::from(h.transform),
                    face_tint: colors.face,
                    edge_tint: colors.edge,
                    material: slot(h.material),
                }
            })
            .collect();
        let plain = |a, b, color| render::Line { a, b, color, dash: None };
        // A guide's stroke, dashed by the renderer.
        let dashed = |s: &solidsketch_tools::guides::Stroke, color| render::Line {
            a: s.a,
            b: s.b,
            color,
            dash: Some(render::Dash { length: solidsketch_tools::guides::DASH_PX as f32, phase: s.phase as f32 }),
        };
        let overlay: Vec<render::Line> =
            (view.overlay.iter().map(|l| plain(l.a, l.b, l.color))).chain(view.overlay_guides.iter().map(|(s, c)| dashed(s, *c))).collect();
        // An open part always shows its axes (ARCHITECTURE.md §2.9).
        let axes = display.axes || editor.open_part().is_some();
        let axis_lines = view.axes.iter().zip([render::AXIS_X, render::AXIS_Y, render::AXIS_Z]).flat_map(|(halves, c)| {
            let negative = [c[0], c[1], c[2], render::AXIS_NEGATIVE_ALPHA];
            halves.iter().zip([c, negative]).filter_map(|(h, c)| h.map(|(a, b)| plain(a, b, c)))
        });
        let lines: Vec<render::Line> = (view.selection_boxes.iter().map(|&(a, b)| plain(a, b, theme.selected.edge)))
            .chain(axis_lines.filter(|_| axes))
            .chain(view.context_box.iter().map(|&(a, b)| plain(a, b, theme.context_box)))
            .chain(view.guides.iter().map(|s| dashed(s, theme.guide)))
            .chain(view.faded_guides.iter().map(|s| dashed(s, faded(theme.guide, background, theme.fade))))
            .chain(view.hovered_guides.iter().map(|s| dashed(s, theme.hover.edge)))
            .chain(view.selected_guides.iter().map(|s| dashed(s, theme.selected.edge)))
            .collect();

        let aspect = size[0] as f64 / size[1] as f64;
        let cam = &editor.camera;
        let rgb = |c: Rgba| [c[0], c[1], c[2]];
        let style = render::Style {
            face_front: rgb(theme.face_front),
            face_back: rgb(theme.face_back),
            edge: rgb(theme.edge),
            palette: &palette,
            silhouettes: display.silhouettes,
            section: view.section.map(|s| render::Section { origin: s.origin, normal: s.normal }),
            cap_shade: theme.cap_shade,
            ghost_opacity: theme.ghost_opacity,
        };
        let frame = render::Frame {
            view: cam.view(),
            proj: cam.proj(aspect, scene.bounds()),
            background,
            style,
            batches: &batches,
            highlights: &highlights,
            lines: &lines,
            overlay: &overlay,
            grid: display.grid.then(|| grid_for(cam.focus)),
            axes,
            axes_frame: DMat4::from(editor.axes_transform()),
            pixels_per_point,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport") });
        // A still view draws nothing: the target keeps its picture (§7 quater).
        if renderer.render(device, &state.queue, &mut encoder, size, &frame) {
            state.queue.submit([encoder.finish()]);
        }
        self.size = size;

        // Show the target through egui: (re-)register it when it was recreated.
        if let Some((target, generation)) = renderer.target() {
            let mut egui_renderer = state.renderer.write();
            match self.texture {
                Some((id, g)) if g == generation => {
                    let _ = id;
                }
                Some((id, _)) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(device, target, wgpu::FilterMode::Linear, id);
                    self.texture = Some((id, generation));
                }
                None => {
                    let id = egui_renderer.register_native_texture(device, target, wgpu::FilterMode::Linear);
                    self.texture = Some((id, generation));
                }
            }
        }
    }
}

/// The selection rectangle: solid when it takes what it encloses (drawn left to right),
/// dashed when it takes what it touches (right to left), as in SketchUp.
fn paint_marquee(painter: &egui::Painter, rect: egui::Rect, m: &solidsketch_tools::Marquee, theme: &Theme) {
    use egui::Stroke;
    let at = |p: DVec2| rect.min + egui::vec2(p.x as f32, p.y as f32);
    let r = egui::Rect::from_two_pos(at(m.from), at(m.to));
    painter.rect_filled(r, 0.0, color(theme.marquee_fill));
    let stroke = Stroke::new(1.0, color(theme.marquee_stroke));
    if m.crossing {
        let path = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(egui::Shape::dashed_line(&path, stroke, 5.0, 4.0));
    } else {
        painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
    }
}

/// Half the arm of a cross marking a point (points).
const MARK_ARM: f32 = 6.0;

/// The points a tool marks (a centre, a start): a `+` on the screen.
fn paint_marks(painter: &egui::Painter, rect: egui::Rect, editor: &Editor) {
    let stroke = egui::Stroke::new(1.5, color(solidsketch_theme::GUIDE_INK));
    for p in editor.marks() {
        let Some(at) = editor.camera.project(p, editor.viewport_size()) else { continue };
        let at = rect.min + egui::vec2(at.x as f32, at.y as f32);
        painter.line_segment([at - egui::vec2(MARK_ARM, 0.0), at + egui::vec2(MARK_ARM, 0.0)], stroke);
        painter.line_segment([at - egui::vec2(0.0, MARK_ARM), at + egui::vec2(0.0, MARK_ARM)], stroke);
    }
}

/// The square around the cursor (points) the lengths keep clear of.
const CURSOR_CLEAR: f32 = 24.0;
/// The square around a cross or a marker (points) the lengths keep clear of.
const MARK_CLEAR: f32 = 16.0;
/// The gap (points) between a length moved off its side and the side.
const LABEL_GAP: f32 = 4.0;

/// The lengths a tool writes by what it draws, framed in their axis's colour, on white with
/// square corners: at the middle of their side, else slid along it or off it to keep clear of
/// `clear` (the cursor, the markers, the tooltip), the crosses and the lengths placed before.
fn paint_labels(painter: &egui::Painter, rect: egui::Rect, editor: &Editor, mut clear: Vec<egui::Rect>) {
    use egui::{Stroke, vec2};
    let size = editor.viewport_size();
    let screen = |p: DVec3| editor.camera.project(p, size).map(|at| rect.min + vec2(at.x as f32, at.y as f32));
    clear.extend(editor.marks().into_iter().filter_map(screen).map(|at| egui::Rect::from_center_size(at, vec2(MARK_CLEAR, MARK_CLEAR))));
    for l in editor.labels() {
        // A side half behind the eye: its middle only.
        let Some((a, b)) = screen(l.a).zip(screen(l.b)).or_else(|| screen((l.a + l.b) / 2.0).map(|m| (m, m))) else { continue };
        let galley = painter.layout_no_wrap(l.text, egui::FontId::proportional(12.0), color(l.color));
        let bg = place(galley.size() + vec2(10.0, 4.0), a, b, &clear);
        painter.rect(bg, 0.0, egui::Color32::WHITE, Stroke::new(1.0, color(l.color)), egui::StrokeKind::Inside);
        painter.galley(bg.center() - galley.size() / 2.0, galley, color(l.color));
        clear.push(bg);
    }
}

/// Where a length of `size` (its frame) goes for the side `a`–`b` (screen): the first spot,
/// in order, that overlaps nothing of `clear` — the middle, then along the side towards either
/// end, then off it on either side; with none free, the middle.
fn place(size: egui::Vec2, a: egui::Pos2, b: egui::Pos2, clear: &[egui::Rect]) -> egui::Rect {
    let at = |p: egui::Pos2| egui::Rect::from_center_size(p, size);
    let d = b - a;
    // Off the side: along its normal, far enough for the frame to leave it.
    let n = if d.length() > 1e-3 { egui::vec2(-d.y, d.x).normalized() } else { egui::vec2(0.0, -1.0) };
    let off = (n.x.abs() * size.x + n.y.abs() * size.y) / 2.0 + LABEL_GAP;
    let along = [0.5, 0.3, 0.7, 0.15, 0.85];
    along
        .iter()
        .map(|&t| a + d * t)
        .chain(along.iter().flat_map(|&t| [a + d * t + n * off, a + d * t - n * off]))
        .map(at)
        .find(|r| !clear.iter().any(|c| c.intersects(*r)))
        .unwrap_or_else(|| at(a + d * 0.5))
}

/// The inferred point's marker, coloured by kind, and its tooltip by the cursor (§6 ter).
/// What it draws (screen), for the lengths to keep clear of.
fn paint_inference(painter: &egui::Painter, rect: egui::Rect, cursor: egui::Pos2, editor: &Editor, theme: &Theme) -> Vec<egui::Rect> {
    use egui::{Color32, Stroke, pos2, vec2};
    let mut drawn = Vec::new();
    let mut mark = |at: egui::Pos2| drawn.push(egui::Rect::from_center_size(at, vec2(MARK_CLEAR, MARK_CLEAR)));
    // Tracked points: a small ring each.
    for p in editor.tracked() {
        if let Some(at) = editor.camera.project(*p, editor.viewport_size()) {
            let at = rect.min + vec2(at.x as f32, at.y as f32);
            painter.circle_stroke(at, 4.0, Stroke::new(1.5, color(theme.tracked)));
            mark(at);
        }
    }
    let Some((point, kind)) = editor.inference() else { return drawn };
    let Some(at) = editor.camera.project(point, editor.viewport_size()) else { return drawn };
    let at = rect.min + vec2(at.x as f32, at.y as f32);
    if kind != InferKind::OnPlane {
        mark(at);
    }
    let outline = Stroke::new(1.0, color(theme.marker_outline));
    match kind {
        InferKind::Endpoint => {
            painter.circle(at, 5.0, color(theme.endpoint), outline);
        }
        InferKind::Midpoint => {
            painter.circle(at, 5.0, color(theme.midpoint), outline);
        }
        InferKind::Center => {
            painter.circle(at, 5.0, color(theme.center), outline);
        }
        InferKind::Origin => {
            painter.circle(at, 5.0, color(theme.origin), outline);
        }
        InferKind::OnTwoLines(_, second) => {
            // The first line's colour, ringed with the second's.
            let rgb = |k: InferKind| {
                let [r, g, b, _] = solidsketch_tools::tools::guide_color(k).map(|c| (c * 255.0) as u8);
                Color32::from_rgb(r, g, b)
            };
            painter.circle(at, 6.0, rgb(InferKind::OnLine(second)), outline);
            painter.circle(at, 3.0, rgb(kind), Stroke::NONE);
        }
        InferKind::OnEdgeAndLine(_) | InferKind::OnGuideAndLine(_) | InferKind::OnGuideAndEdge | InferKind::OnTwoGuides => {
            // The edge's square, in the line's colour.
            let [r, g, b, _] = solidsketch_tools::tools::guide_color(kind).map(|c| (c * 255.0) as u8);
            let square = egui::Rect::from_center_size(at, vec2(9.0, 9.0));
            painter.rect(square, 0.0, Color32::from_rgb(r, g, b), outline, egui::StrokeKind::Middle);
        }
        InferKind::OnEdge => {
            let square = egui::Rect::from_center_size(at, vec2(8.0, 8.0));
            painter.rect(square, 0.0, color(theme.on_edge), outline, egui::StrokeKind::Middle);
        }
        InferKind::OnFace => {
            let d = 5.0;
            let pts = vec![at + vec2(0.0, -d), at + vec2(d, 0.0), at + vec2(0.0, d), at + vec2(-d, 0.0)];
            painter.add(egui::Shape::convex_polygon(pts, color(theme.on_face), outline));
        }
        InferKind::OnLine(_) | InferKind::OnGuide => {
            let [r, g, b, _] = solidsketch_tools::tools::guide_color(kind).map(|c| (c * 255.0) as u8);
            painter.circle(at, 4.0, Color32::from_rgb(r, g, b), outline);
        }
        InferKind::OnPlane => {}
    }
    let label = kind.label();
    if label.is_empty() {
        return drawn;
    }
    let text = color(theme.tooltip_text);
    let galley = painter.layout_no_wrap(label, egui::FontId::proportional(12.0), text);
    let pos = pos2(cursor.x + 14.0, cursor.y + 16.0);
    let bg = egui::Rect::from_min_size(pos, galley.size()).expand2(vec2(5.0, 2.0));
    painter.rect(bg, 3.0, color(theme.tooltip_fill), Stroke::new(1.0, color(theme.tooltip_border)), egui::StrokeKind::Inside);
    painter.galley(pos, galley, text);
    drawn.push(bg);
    drawn
}

/// A grid whose spacing follows the zoom (powers of ten, 100 lines).
fn grid_for(focus: f64) -> render::Grid {
    let spacing = 10f64.powf((focus / 20.0).max(1.0).log10().floor());
    render::Grid { spacing, count: 100 }
}

impl Viewport {
    /// The picture last drawn, as RGBA rows (sRGB): the control channel's screenshot. Waits
    /// for the GPU.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn capture(&self, state: &egui_wgpu::RenderState) -> Option<(u32, u32, Vec<u8>)> {
        let texture = self.renderer.as_ref()?.target_texture()?;
        let [w, h] = [texture.width(), texture.height()];
        let (device, queue) = (&state.device, &state.queue);
        let row = (w * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = buffer.slice(..).get_mapped_range().ok()?;
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let s = (y * row) as usize;
            px.extend_from_slice(&data[s..s + (w * 4) as usize]);
        }
        Some((w, h, px))
    }
}

#[cfg(test)]
mod tests {
    use super::place;
    use egui::{Rect, pos2, vec2};

    #[test]
    fn a_length_keeps_clear() {
        let size = vec2(40.0, 16.0);
        let (a, b) = (pos2(0.0, 100.0), pos2(200.0, 100.0));
        // Nothing in the way: the middle.
        assert_eq!(place(size, a, b, &[]).center(), pos2(100.0, 100.0));
        // The middle taken: along the side.
        let mid = Rect::from_center_size(pos2(100.0, 100.0), vec2(16.0, 16.0));
        assert_eq!(place(size, a, b, &[mid]).center(), pos2(60.0, 100.0));
        // The whole side taken: off it, the frame clear of the side.
        let side = Rect::from_min_max(pos2(-50.0, 99.0), pos2(250.0, 101.0));
        let r = place(size, a, b, &[side]);
        assert!(!r.intersects(side) && r.center().x == 100.0, "{r:?}");
        // Nowhere free: the middle.
        let all = Rect::from_min_max(pos2(-500.0, -500.0), pos2(500.0, 500.0));
        assert_eq!(place(size, a, b, &[all]).center(), pos2(100.0, 100.0));
    }
}

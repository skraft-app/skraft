//! The 3D viewport: input to the editor (SketchUp navigation, tool clicks), the editor's view
//! to the renderer, the renderer's off-screen target shown as an egui image.

use glam::{DMat4, DVec2};
use solidsketch_i18n::t;
use solidsketch_render as render;
use solidsketch_tools::{Editor, InferKind, Mark, Modifiers, ToolInput, ToolKind};

use crate::prefs::Navigation;

/// How long without trackpad scrolling ends a trackpad gesture (seconds): the next one picks
/// a new orbit pivot.
const GESTURE_GAP: f64 = 0.3;

#[derive(Default)]
pub struct Viewport {
    renderer: Option<render::Renderer>,
    texture: Option<(egui::TextureId, u64)>,
    last_hover: Option<egui::Pos2>,
    last_trackpad: f64,
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

    pub fn ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, editor: &mut Editor, nav: &Navigation) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        editor.set_viewport_size(DVec2::new(rect.width() as f64, rect.height() as f64));
        self.input(ui, &response, rect, editor, nav);

        let cursor = match editor.tool_kind() {
            _ if editor.busy() => egui::CursorIcon::Progress, // the kernel is at work
            _ if editor.tool_copying() => egui::CursorIcon::Copy, // SketchUp's « + »
            ToolKind::Select => egui::CursorIcon::Default,
            ToolKind::Move => egui::CursorIcon::Move,
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
        self.render(state, size, ppp, editor);
        if let Some((id, _)) = self.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(id, rect, uv, egui::Color32::WHITE);
        }
        if let Some(cursor) = response.hover_pos() {
            paint_inference(ui.painter(), rect, cursor, editor);
        }
        if let Some(m) = editor.marquee() {
            paint_marquee(ui.painter(), rect, &m);
        }
    }

    fn input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: egui::Rect, editor: &mut Editor, nav: &Navigation) {
        let ctx = ui.ctx();
        let local = |p: egui::Pos2| DVec2::new((p.x - rect.min.x) as f64, (p.y - rect.min.y) as f64);
        // The preferences: orbit direction and speed per device, zoom speed and target.
        let sign = |invert: bool| if invert { -1.0 } else { 1.0 };
        let mouse_orbit = DVec2::new(sign(nav.mouse_invert_x), sign(nav.mouse_invert_y)) * nav.orbit_speed;
        let trackpad_orbit = DVec2::new(sign(nav.trackpad_invert_x), sign(nav.trackpad_invert_y)) * nav.orbit_speed;
        let zoom_at = |p: egui::Pos2| if nav.zoom_to_cursor { local(p) } else { DVec2::new(rect.width() as f64, rect.height() as f64) / 2.0 };
        let zoom_steps = nav.zoom_speed * sign(nav.wheel_invert);
        let (mods, events, time, zoom) = ui.input(|i| (i.modifiers, i.events.clone(), i.time, i.zoom_delta()));
        // Shift held locks the line inferred (§6 ter); a point hovered a while is tracked: the
        // editor needs the time, every frame while it waits.
        editor.set_shift(mods.shift);
        if editor.tick(time) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let hover = response.hover_pos();

        // Hover: tools follow the cursor.
        if let Some(p) = hover
            && self.last_hover != Some(p)
        {
            self.last_hover = Some(p);
            editor.input(ToolInput::Hover { px: local(p) });
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

    fn render(&mut self, state: &egui_wgpu::RenderState, size: [u32; 2], pixels_per_point: f32, editor: &Editor) {
        let device = &state.device;
        let renderer = self.renderer.get_or_insert_with(|| render::Renderer::new(device));

        // Meshes: one per definition, re-uploaded when its revision changes.
        let scene = editor.scene();
        for dm in scene.definition_meshes() {
            if !renderer.has_mesh(dm.definition.0, dm.revision) {
                let parts: Vec<render::Part<'_>> = dm.parts.iter().map(|p| render::Part { id: p.solid.0, mesh: &p.mesh }).collect();
                renderer.set_mesh(device, dm.definition.0, dm.revision, &parts);
            }
        }
        renderer.retain_meshes(|k| scene.definition_mesh(solidsketch_model::Id(k)).is_some());

        let display = &editor.document().settings().display;
        let [r, g, b] = display.background;
        let bg = egui::Rgba::from(egui::Color32::from_rgb(r, g, b)); // linear
        let background = [bg.r(), bg.g(), bg.b()];
        let view = editor.view();
        let fade: render::Tint = [background[0], background[1], background[2], 0.7];
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
                    })
                    .collect(),
            })
            .collect();
        let highlights: Vec<render::Highlight> = view
            .highlights
            .iter()
            .map(|h| {
                let (face_tint, edge_tint) = match h.mark {
                    Mark::Selected => ([0.15, 0.35, 1.0, 0.35], [0.05, 0.2, 1.0, 1.0]),
                    Mark::Hover => ([0.2, 0.45, 1.0, 0.12], [0.1, 0.35, 1.0, 0.7]),
                    Mark::Picked => ([1.0, 0.45, 0.05, 0.4], [0.9, 0.3, 0.0, 1.0]),
                    Mark::Ghost => ([0.3, 0.5, 1.0, 0.3], [0.05, 0.2, 1.0, 1.0]),
                    // The other copies a change reaches: the picked target's orange, lighter.
                    Mark::Related => ([1.0, 0.55, 0.15, 0.2], [0.95, 0.45, 0.1, 0.6]),
                };
                render::Highlight { key: h.definition.0, part: h.solid.0, transform: DMat4::from(h.transform), face_tint, edge_tint }
            })
            .collect();
        let overlay: Vec<render::Line> =
            view.overlay.iter().map(|l| render::Line { a: l.a, b: l.b, color: l.color }).collect();

        let aspect = size[0] as f64 / size[1] as f64;
        let cam = &editor.camera;
        let frame = render::Frame {
            view: cam.view(),
            proj: cam.proj(aspect, scene.bounds()),
            background,
            batches: &batches,
            highlights: &highlights,
            lines: &[],
            overlay: &overlay,
            grid: display.grid.then(|| grid_for(cam.focus)),
            axes: display.axes,
            axes_frame: DMat4::from(editor.context_transform()),
            pixels_per_point,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport") });
        renderer.render(device, &state.queue, &mut encoder, size, &frame);
        state.queue.submit([encoder.finish()]);

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
fn paint_marquee(painter: &egui::Painter, rect: egui::Rect, m: &solidsketch_tools::Marquee) {
    use egui::{Color32, Stroke};
    let at = |p: DVec2| rect.min + egui::vec2(p.x as f32, p.y as f32);
    let r = egui::Rect::from_two_pos(at(m.from), at(m.to));
    painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(60, 110, 230, 25));
    let stroke = Stroke::new(1.0, Color32::from_rgb(40, 80, 200));
    if m.crossing {
        let path = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(egui::Shape::dashed_line(&path, stroke, 5.0, 4.0));
    } else {
        painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
    }
}

/// The inferred point's marker, coloured by kind, and its tooltip by the cursor (§6 ter).
fn paint_inference(painter: &egui::Painter, rect: egui::Rect, cursor: egui::Pos2, editor: &Editor) {
    use egui::{Color32, Stroke, pos2, vec2};
    // Tracked points: a small ring each.
    for p in editor.tracked() {
        if let Some(at) = editor.camera.project(*p, editor.viewport_size()) {
            let at = rect.min + vec2(at.x as f32, at.y as f32);
            painter.circle_stroke(at, 4.0, Stroke::new(1.5, Color32::from_rgb(150, 40, 160)));
        }
    }
    let Some((point, kind)) = editor.inference() else { return };
    let Some(at) = editor.camera.project(point, editor.viewport_size()) else { return };
    let at = rect.min + vec2(at.x as f32, at.y as f32);
    let outline = Stroke::new(1.0, Color32::from_gray(30));
    match kind {
        InferKind::Endpoint => {
            painter.circle(at, 5.0, Color32::from_rgb(40, 190, 60), outline);
        }
        InferKind::Midpoint => {
            painter.circle(at, 5.0, Color32::from_rgb(40, 200, 220), outline);
        }
        InferKind::Center => {
            painter.circle(at, 5.0, Color32::from_rgb(170, 70, 220), outline);
        }
        InferKind::Origin => {
            painter.circle(at, 5.0, Color32::from_rgb(240, 180, 30), outline);
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
        InferKind::OnEdgeAndLine(_) => {
            // The edge's square, in the line's colour.
            let [r, g, b, _] = solidsketch_tools::tools::guide_color(kind).map(|c| (c * 255.0) as u8);
            let square = egui::Rect::from_center_size(at, vec2(9.0, 9.0));
            painter.rect(square, 0.0, Color32::from_rgb(r, g, b), outline, egui::StrokeKind::Middle);
        }
        InferKind::OnEdge => {
            let square = egui::Rect::from_center_size(at, vec2(8.0, 8.0));
            painter.rect(square, 0.0, Color32::from_rgb(220, 40, 40), outline, egui::StrokeKind::Middle);
        }
        InferKind::OnFace => {
            let d = 5.0;
            let pts = vec![at + vec2(0.0, -d), at + vec2(d, 0.0), at + vec2(0.0, d), at + vec2(-d, 0.0)];
            painter.add(egui::Shape::convex_polygon(pts, Color32::from_rgb(60, 90, 230), outline));
        }
        InferKind::OnLine(_) => {
            let [r, g, b, _] = solidsketch_tools::tools::guide_color(kind).map(|c| (c * 255.0) as u8);
            painter.circle(at, 4.0, Color32::from_rgb(r, g, b), outline);
        }
        InferKind::OnPlane => {}
    }
    let label = kind.label();
    if label.is_empty() {
        return;
    }
    let galley = painter.layout_no_wrap(label, egui::FontId::proportional(12.0), Color32::from_gray(20));
    let pos = pos2(cursor.x + 14.0, cursor.y + 16.0);
    let bg = egui::Rect::from_min_size(pos, galley.size()).expand2(vec2(5.0, 2.0));
    painter.rect(bg, 3.0, Color32::from_rgb(255, 250, 215), Stroke::new(1.0, Color32::from_gray(120)), egui::StrokeKind::Inside);
    painter.galley(pos, galley, Color32::from_gray(20));
}

/// A grid whose spacing follows the zoom (powers of ten, 100 lines).
fn grid_for(focus: f64) -> render::Grid {
    let spacing = 10f64.powf((focus / 20.0).max(1.0).log10().floor());
    render::Grid { spacing, count: 100 }
}

//! The 3D viewport: input to the editor (SketchUp navigation, tool clicks), the editor's view
//! to the renderer, the renderer's off-screen target shown as an egui image.

use std::collections::HashMap;

use glam::{DMat4, DVec2, DVec3};
use skraft_i18n::t;
use skraft_model::Id;
use skraft_render as render;
use skraft_theme::{Rgba, Theme};
use skraft_tools::draw;
use skraft_tools::navigation::{Button, Mods, NavAction, Trigger};
use skraft_tools::{Editor, InferKind, Look, Mark, Modifiers, PointShape, ToolInput};

use crate::prefs::Navigation;

/// A token as an egui colour (sRGB, unmultiplied alpha).
pub fn color(c: Rgba) -> egui::Color32 {
    let [r, g, b, a] = skraft_theme::to_srgba8(c);
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// How long without trackpad scrolling ends a trackpad gesture (seconds): the next one picks
/// a new orbit pivot.
const GESTURE_GAP: f64 = 0.3;
/// How long without wheel or pinch zooming ends a burst of them: the next one is another step
/// of the views gone through (seconds).
const VIEW_BURST_GAP: f64 = 0.5;
/// How far a zoom drag moves for one wheel notch (points).
const ZOOM_DRAG_PX: f64 = 20.0;

/// The buttons a navigation style may use, and egui's.
const BUTTONS: [(Button, egui::PointerButton); 4] = [
    (Button::Middle, egui::PointerButton::Middle),
    (Button::Right, egui::PointerButton::Secondary),
    (Button::Back, egui::PointerButton::Extra1),
    (Button::Forward, egui::PointerButton::Extra2),
];

/// A navigation gesture under way (docs/ui/navigation-styles.md).
struct Gesture {
    button: Button,
    egui: egui::PointerButton,
    /// Where the button went down, and the pointer now.
    from: egui::Pos2,
    at: egui::Pos2,
    /// What it does with the modifiers held now (none: they match no binding).
    action: Option<NavAction>,
    /// The modifiers the tool had when it began: it keeps them till it ends.
    tool_mods: egui::Modifiers,
}

#[derive(Default)]
pub struct Viewport {
    renderer: Option<render::Renderer>,
    texture: Option<(egui::TextureId, u64)>,
    last_hover: Option<egui::Pos2>,
    last_trackpad: f64,
    /// The last wheel or pinch zoom (egui's clock).
    last_zoom: f64,
    gesture: Option<Gesture>,
    /// The modifiers the tool saw last, and those hidden from it till released (Shift, Ctrl,
    /// Alt, Command), pressed during a gesture.
    tool_mods: egui::Modifiers,
    hidden: [bool; 4],
    /// Each material's palette entry, for the session: an entry is never given to another
    /// material, so meshes with painted faces stay valid when materials come and go.
    /// They are the shown document's: ids are a document's own.
    slots: HashMap<Id, u32>,
    /// The document shown ([`crate::docs::DocId`]), and the others' palette entries, kept
    /// while they are not shown.
    document: u64,
    parked_slots: HashMap<u64, HashMap<Id, u32>>,
    /// Each definition's grain — the axis of its mesh along its part's length, a texture's
    /// (docs/render/look.md#textures) — by document and definition, with the mesh's revision.
    grains: HashMap<(u64, u64), (u64, u8)>,
    /// The textures' images, prepared off the interface's thread.
    images: crate::images::TextureImages,
    /// The size of the last frame drawn (pixels).
    size: [u32; 2],
    /// What the render statistics overlay shows.
    timing: Timing,
    /// A component dragged from the « Composants » card is carried (docs/ui/components-library.md#placing):
    /// the card's row holds the drag, the view follows the pointer and puts it down where the
    /// button is released.
    pub dragged_in: bool,
    /// The components' thumbnails (docs/ui/components-library.md#thumbnails), drawn by its renderer.
    pub thumbnails: crate::thumbnails::Thumbnails,
    /// The palette of the last frame and its entries' textures, the thumbnails' too.
    palette: Vec<[f32; 4]>,
    textures: Vec<[f32; 4]>,
    /// The pictures of the drawing's views (docs/ui/drawing.md#rendered-views), by document
    /// and view slot, and the textures of those dropped, freed at the next frame.
    sheet: HashMap<(u64, crate::sheet::ViewSlot), SheetPicture>,
    sheet_freed: Vec<egui::TextureId>,
    /// Counts the frames a sheet asked for pictures: how recently each was seen.
    sheet_frame: u64,
}

/// A view's picture on a sheet, drawn for `stamp`, last asked for at frame `seen`.
struct SheetPicture {
    picture: render::Picture,
    texture: egui::TextureId,
    stamp: u64,
    seen: u64,
}

/// Measured on every frame, for the render statistics overlay.
#[derive(Default)]
struct Timing {
    /// When the last frames were painted (egui's clock, seconds), over the last second.
    frames: std::collections::VecDeque<f64>,
    /// Between the last two frames.
    interval: f64,
    /// Building the editor's view, then preparing and encoding the picture (milliseconds).
    view_ms: f64,
    render_ms: f64,
    /// The picture was drawn again (else the target kept it).
    redrawn: bool,
}

/// One tint `a` then another `b`, as one: mixing `rgb` by `a` (what a tint does) twice is one
/// mix, by `1 - (1 - a.a)(1 - b.a)` towards their weighted `rgb`.
fn then_fade(a: render::Tint, b: render::Tint) -> render::Tint {
    let k = 1.0 - (1.0 - a[3]) * (1.0 - b[3]);
    if k <= 0.0 {
        return render::NO_TINT;
    }
    let mix = |i: usize| (a[i] * a[3] * (1.0 - b[3]) + b[i] * b[3]) / k;
    [mix(0), mix(1), mix(2), k]
}

/// The axis of a definition's mesh along its part's length: its longest extent (x first on a
/// tie).
fn grain_of(dm: &skraft_tools::DefinitionMesh) -> u8 {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &dm.parts {
        let o = p.mesh.origin.as_vec3().to_array();
        for v in &p.mesh.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k] + o[k]);
                hi[k] = hi[k].max(v[k] + o[k]);
            }
        }
    }
    let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    (0..3).fold(0u8, |best, k| if size[k] > size[best as usize] { k as u8 } else { best })
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

    /// Shows document `doc` (docs/ui/file-tabs.md): its palette entries and its
    /// meshes, the other documents' kept for when they are shown again.
    fn show_document(&mut self, doc: u64) {
        if doc != self.document {
            let shown = std::mem::replace(&mut self.slots, self.parked_slots.remove(&doc).unwrap_or_default());
            if !shown.is_empty() {
                self.parked_slots.insert(self.document, shown);
            }
            self.document = doc;
        }
        if let Some(r) = &mut self.renderer {
            r.set_document(doc);
        }
    }

    /// Forgets document `doc` (closed): its palette entries and its meshes.
    pub fn drop_document(&mut self, doc: u64) {
        if doc == self.document {
            self.slots.clear();
        } else {
            self.parked_slots.remove(&doc);
        }
        if let Some(r) = &mut self.renderer {
            r.drop_document(doc);
        }
        self.thumbnails.drop_document(doc);
        let gone: Vec<_> = self.sheet.keys().filter(|(d, _)| *d == doc).copied().collect();
        for k in gone {
            if let Some(p) = self.sheet.remove(&k) {
                self.sheet_freed.push(p.texture);
            }
        }
    }

    /// The views' pictures of document `doc` drawn so far, by view slot.
    pub fn sheet_textures(&self, doc: u64) -> HashMap<crate::sheet::ViewSlot, egui::TextureId> {
        self.sheet.iter().filter(|((d, _), _)| *d == doc).map(|((_, v), p)| (*v, p.texture)).collect()
    }

    /// Draws the views' pictures a sheet of document `doc` asks for (docs/ui/drawing.md#rendered-views),
    /// those whose stamp changed, within [`SHEET_BUDGET_MS`] (the others the next frame), in the
    /// order asked; those no longer asked for kept within [`SHEET_KEPT_BYTES`], so that panning
    /// back redraws nothing. The paper's white behind them.
    pub fn sheet_pictures(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        doc: u64,
        editor: &Editor,
        theme: &Theme,
        wanted: &[crate::sheet::Wanted],
    ) {
        let Some(state) = frame.wgpu_render_state() else { return };
        self.show_document(doc);
        self.sheet_frame += 1;
        let now = self.sheet_frame;
        for w in wanted {
            if let Some(p) = self.sheet.get_mut(&(doc, w.slot)) {
                p.seen = now;
            }
        }
        let sizes = self.sheet.iter().map(|(k, p)| {
            let [w, h] = p.picture.size();
            (*k, w as u64 * h as u64 * 4, p.seen)
        });
        for k in crate::sheet::over_budget(sizes, now, SHEET_KEPT_BYTES) {
            if let Some(p) = self.sheet.remove(&k) {
                self.sheet_freed.push(p.texture);
            }
        }
        if !self.sheet_freed.is_empty() {
            let mut egui_renderer = state.renderer.write();
            for t in self.sheet_freed.drain(..) {
                egui_renderer.free_texture(&t);
            }
        }
        // The thumbnails of the dialog choosing a view's content: no 3D view draws them here.
        if self.thumbnails.pending() {
            let _ = self.sync(state, editor, theme);
            self.render_thumbnails(ctx, state, ctx.pixels_per_point(), editor, theme);
        }
        let todo: Vec<&crate::sheet::Wanted> =
            wanted.iter().filter(|w| self.sheet.get(&(doc, w.slot)).is_none_or(|p| p.stamp != w.stamp)).collect();
        if todo.is_empty() {
            return;
        }
        let (palette, textures, _) = self.sync(state, editor, theme);
        let document = self.document;
        let Some(renderer) = self.renderer.as_mut() else { return };
        let scene = editor.scene();
        let start = web_time::Instant::now();
        let ppp = ctx.pixels_per_point();
        let mut more = false;
        for w in todo {
            if start.elapsed().as_secs_f64() * 1e3 > SHEET_BUDGET_MS {
                more = true;
                break;
            }
            let device = &state.device;
            let key = (doc, w.slot);
            let look = SheetLook { theme, palette: &palette, textures: &textures, slots: &self.slots, grains: &self.grains, document, ppp };
            match self.sheet.get_mut(&key) {
                Some(p) if p.picture.size() == w.size => {
                    draw_sheet_picture(renderer, state, scene, &look, w, &p.picture);
                    p.stamp = w.stamp;
                }
                _ => {
                    let picture = renderer.new_picture(device, w.size);
                    draw_sheet_picture(renderer, state, scene, &look, w, &picture);
                    let texture = state.renderer.write().register_native_texture(device, picture.display_view(), wgpu::FilterMode::Linear);
                    if let Some(old) = self.sheet.insert(key, SheetPicture { picture, texture, stamp: w.stamp, seen: now }) {
                        self.sheet_freed.push(old.texture);
                    }
                }
            }
        }
        // Drawn now, shown at the next frame.
        ctx.request_repaint();
        let _ = more;
    }

    /// A sheet view's picture `w` drawn now and read back (RGBA, rows top first): the PDF's
    /// (docs/ui/drawing.md#pdf). `ppp`: the picture's pixels per point (its lines' widths).
    pub fn picture_rgba(
        &mut self,
        frame: &eframe::Frame,
        doc: u64,
        editor: &Editor,
        theme: &Theme,
        w: &crate::sheet::Wanted,
        ppp: f32,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let state = frame.wgpu_render_state()?;
        self.show_document(doc);
        let (palette, textures, _) = self.sync(state, editor, theme);
        let document = self.document;
        let renderer = self.renderer.as_mut()?;
        let look = SheetLook { theme, palette: &palette, textures: &textures, slots: &self.slots, grains: &self.grains, document, ppp };
        let picture = renderer.new_picture(&state.device, w.size);
        draw_sheet_picture(renderer, state, editor.scene(), &look, w, &picture);
        read_texture(state, picture.texture())
    }

    /// Draws document `doc`, edited by `editor`, and feeds it the view's input; `typed` is the
    /// text typed in the measurements box (shown in the lengths it is for).
    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        doc: u64,
        editor: &mut Editor,
        typed: &str,
        nav: &Navigation,
        theme: &Theme,
        stats: bool,
    ) {
        self.show_document(doc);
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        crate::handles::publish(|| "viewport".into(), rect);
        editor.set_viewport_size(DVec2::new(rect.width() as f64, rect.height() as f64));
        self.input(ui, &response, rect, editor, nav);

        // The tool's own cursor at its step (docs/ui/cursors.md), else the system's icon.
        let own = if editor.busy() { None } else { editor.tool_cursor() };
        let cursor = if editor.busy() {
            egui::CursorIcon::Progress // the kernel is at work
        } else if editor.tool_picking() {
            egui::CursorIcon::Default
        } else if editor.tool_copying() {
            egui::CursorIcon::Copy // SketchUp's « + »
        } else {
            egui::CursorIcon::Crosshair
        };
        // The cursor shown over the view: the tool's own, else the system's.
        let mut shown = None;
        if response.hovered() {
            let navigating = match self.gesture.as_ref().map(|g| g.action) {
                Some(Some(NavAction::Orbit | NavAction::Pan)) => Some(egui::CursorIcon::Grabbing),
                Some(Some(NavAction::ZoomDrag | NavAction::ZoomWindow)) => Some(egui::CursorIcon::ZoomIn),
                _ => None,
            };
            // Navigating: the system's hand or magnifier.
            shown = own.filter(|_| navigating.is_none());
            if let Some(own) = shown {
                crate::cursors::ask(ui.ctx(), own);
            }
            ui.ctx().set_cursor_icon(navigating.unwrap_or(cursor));
        }
        // What it draws about the pointer: the system's, a square about its hotspot.
        let footprint =
            shown.map_or(egui::Rect::from_center_size(egui::Pos2::ZERO, egui::Vec2::splat(CURSOR_CLEAR)), crate::cursors::footprint);

        let ppp = ui.ctx().pixels_per_point();
        let size = [(rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32];
        let Some(state) = frame.wgpu_render_state() else {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, t!("status.no_wgpu"), Default::default(), egui::Color32::RED);
            return;
        };
        let start = web_time::Instant::now();
        self.render(state, size, ppp, editor, theme);
        self.render_thumbnails(ui.ctx(), state, ppp, editor, theme);
        self.timing.render_ms = start.elapsed().as_secs_f64() * 1e3 - self.timing.view_ms;
        let now = ui.input(|i| i.time);
        self.timing.interval = self.timing.frames.back().map_or(0.0, |t| now - t);
        self.timing.frames.push_back(now);
        while self.timing.frames.front().is_some_and(|t| now - t > 1.0) {
            self.timing.frames.pop_front();
        }
        if let Some((id, _)) = self.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(id, rect, uv, egui::Color32::WHITE);
        }
        paint_overlay_points(ui.painter(), rect, editor, theme);
        paint_marks(ui.painter(), rect, editor, theme);
        let mut clear = Vec::new();
        if let Some(cursor) = response.hover_pos() {
            clear.push(footprint.translate(cursor.to_vec2()));
            clear.extend(paint_inference(ui.painter(), rect, cursor, footprint, editor, theme));
        }
        if let Some(m) = editor.marquee() {
            paint_marquee(ui.painter(), rect, &m, theme);
        }
        // The zoom window's rectangle, dashed.
        if let Some(g) = self.gesture.as_ref().filter(|g| g.action == Some(NavAction::ZoomWindow)) {
            let r = egui::Rect::from_two_pos(g.from, g.at);
            let path = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            ui.painter().extend(egui::Shape::dashed_line(&path, egui::Stroke::new(1.0, color(theme.marquee_stroke)), 5.0, 4.0));
        }
        // Last: the lengths stay above everything (markers, tooltip, marquee), placed clear of
        // the cursor, the markers, the tooltip and the crosses.
        paint_labels(ui.painter(), rect, editor, typed, clear);
        if stats {
            self.paint_stats(ui, rect, editor);
        }
    }

    /// The render statistics, in the top right corner of what the floating cards leave free.
    fn paint_stats(&self, ui: &egui::Ui, rect: egui::Rect, editor: &Editor) {
        let Some(renderer) = &self.renderer else { return };
        let r = renderer.stats();
        let (meshes, mesh_bytes) = renderer.mesh_memory();
        let tm = &self.timing;
        let occurrences: usize = editor.scene().placements().map(|(_, p)| p.len()).sum();
        let rows = [
            (t!("render_stats.fps"), format!("{}", tm.frames.len())),
            (t!("render_stats.interval"), format!("{:.1} ms", tm.interval * 1e3)),
            (t!("render_stats.view_cpu"), format!("{:.2} ms", tm.view_ms)),
            (t!("render_stats.render_cpu"), format!("{:.2} ms", tm.render_ms)),
            (t!("render_stats.picture"), if tm.redrawn { t!("render_stats.redrawn") } else { t!("render_stats.kept") }),
            (t!("render_stats.draw_calls"), count(r.draw_calls as u64)),
            (t!("render_stats.triangles"), count(r.triangles)),
            (t!("render_stats.instances"), count(r.instances as u64)),
            (t!("render_stats.lines"), count(r.lines as u64)),
            (t!("render_stats.boxes"), count(r.boxes as u64)),
            (t!("render_stats.upload"), bytes(r.upload_bytes)),
            (t!("render_stats.meshes"), format!("{} · {}", count(meshes as u64), bytes(mesh_bytes))),
            (t!("render_stats.occurrences"), count(occurrences as u64)),
            (t!("render_stats.selection"), count(editor.selection().len() as u64)),
            (t!("render_stats.size"), format!("{} × {}", self.size[0], self.size[1])),
        ];
        let label_width = rows.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
        let value_width = rows.iter().map(|(_, v)| v.chars().count()).max().unwrap_or(0);
        let text: Vec<String> = rows.iter().map(|(l, v)| format!("{l:<label_width$}  {v:>value_width$}")).collect();
        let visuals = ui.visuals();
        let font = egui::FontId::monospace(11.0);
        let galley = ui.painter().layout_no_wrap(text.join("\n"), font, visuals.text_color());
        let margin = 8.0;
        let pad = egui::vec2(8.0, 6.0);
        let size = galley.size() + 2.0 * pad;
        // Under the ribbon, beside the docked cards (as the last frame left them).
        let corner = match editor.free_area() {
            Some((min, max)) => rect.min + egui::vec2(max.x as f32, min.y as f32) + egui::vec2(0.0, -margin),
            None => rect.right_top(),
        };
        let card = egui::Rect::from_min_size(corner + egui::vec2(-size.x - margin, margin), size);
        let painter = ui.painter();
        painter.rect_filled(card, 6.0, visuals.window_fill.gamma_multiply(0.88));
        painter.rect_stroke(card, 6.0, visuals.window_stroke, egui::StrokeKind::Inside);
        painter.galley(card.min + pad, galley, visuals.text_color());
    }

    fn input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: egui::Rect, editor: &mut Editor, nav: &Navigation) {
        let ctx = ui.ctx();
        let mac = ctx.os() == egui::os::OperatingSystem::Mac;
        let local = |p: egui::Pos2| DVec2::new((p.x - rect.min.x) as f64, (p.y - rect.min.y) as f64);
        let style = &nav.style;
        // The preferences: orbit direction and speed per device, zoom speed and target.
        let sign = |invert: bool| if invert { -1.0 } else { 1.0 };
        let mouse_orbit = DVec2::new(sign(nav.mouse_invert_x), sign(nav.mouse_invert_y)) * nav.orbit_rate();
        let trackpad_orbit = DVec2::new(sign(nav.trackpad_invert_x), sign(nav.trackpad_invert_y)) * nav.orbit_rate();
        let zoom_at =
            |p: egui::Pos2| if nav.zoom_to_cursor { local(p) } else { DVec2::new(rect.width() as f64, rect.height() as f64) / 2.0 };
        let zoom_steps = nav.zoom_rate() * sign(style.wheel_reversed);
        let (mods, events, time, zoom) = ui.input(|i| (i.modifiers, i.events.clone(), i.time, i.zoom_delta()));
        let nav_mods = Mods { shift: mods.shift, ctrl: mods.ctrl, alt: mods.alt, cmd: mac && mods.mac_cmd };

        // The style's gestures (docs/ui/navigation-styles.md). A drag starts past egui's threshold: a right
        // button released before it is a click, kept for a context menu.
        if self.gesture.is_none() {
            for (b, eb) in BUTTONS {
                if response.drag_started_by(eb) && style.drags_with(b) {
                    let from = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos()).unwrap_or_default();
                    editor.nav_begin_about(local(from), style.pivot);
                    self.gesture = Some(Gesture { button: b, egui: eb, from, at: from, action: None, tool_mods: self.tool_mods });
                    break;
                }
            }
        }
        if let Some(g) = &mut self.gesture {
            // Live modifiers: the action is resolved again at every frame, the pivot kept.
            let action = style.resolve(Trigger::Drag(g.button), nav_mods);
            if response.dragged_by(g.egui) {
                let d = response.drag_delta();
                let d = DVec2::new(d.x as f64, d.y as f64);
                match action {
                    Some(NavAction::Orbit) => editor.orbit(d * mouse_orbit),
                    Some(NavAction::Pan) => editor.pan(d),
                    Some(NavAction::ZoomDrag) => editor.zoom(zoom_at(g.from), -d.y / ZOOM_DRAG_PX * zoom_steps),
                    _ => {}
                }
                if let Some(p) = response.interact_pointer_pos() {
                    g.at = p;
                }
            }
            g.action = action;
            if response.drag_stopped_by(g.egui) || !ui.input(|i| i.pointer.button_down(g.egui)) {
                if g.action == Some(NavAction::ZoomWindow) {
                    let r = egui::Rect::from_two_pos(g.from, g.at);
                    if r.width().max(r.height()) > ctx.options(|o| o.input_options.max_click_dist) {
                        editor.zoom_window(local(g.from), local(g.at));
                    }
                }
                self.gesture = None;
            }
        }
        if self.gesture.is_none()
            && let Some(p) = response.interact_pointer_pos()
        {
            for (b, eb) in BUTTONS {
                // A double-click the style has nothing for is a click (back, back, quickly).
                let double = response.double_clicked_by(eb).then(|| style.resolve(Trigger::DoubleClick(b), nav_mods)).flatten();
                let action = double.or_else(|| response.clicked_by(eb).then(|| style.resolve(Trigger::Click(b), nav_mods)).flatten());
                match action {
                    Some(NavAction::Recentre) => editor.recenter(local(p)),
                    Some(NavAction::ZoomExtents) => editor.zoom_extents(),
                    Some(NavAction::PreviousView) => {
                        editor.previous_view();
                    }
                    Some(NavAction::NextView) => {
                        editor.next_view();
                    }
                    _ => {}
                }
            }
        }

        // Priority to the button: during a gesture the tool keeps the modifiers it had; one
        // pressed then stays hidden from it until released.
        let tool_mods = self.tool_modifiers(mods, mac);
        // Shift held locks the line inferred (docs/tools/inferences.md); a point hovered a while is tracked: the
        // editor needs the time, every frame while it waits.
        editor.set_shift(tool_mods.shift);
        editor.set_copy(modifiers(ctx, tool_mods).copy);
        editor.set_alt(if mac { tool_mods.mac_cmd } else { tool_mods.alt });
        if editor.tick(time) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if self.dragged_in {
            return self.dragged_in_input(ctx, rect, editor, modifiers(ctx, tool_mods));
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

        // Primary button: the tool (clicks; drags from where the button went down).
        use egui::PointerButton::Primary;
        if let Some(p) = response.interact_pointer_pos() {
            let m = modifiers(ctx, tool_mods);
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

        // Wheel = zoom; trackpad scroll = the style's (orbit or pan, Shift: the other); pinch =
        // zoom. A burst of them is one step of the views gone through.
        let Some(p) = hover else { return };
        let burst = |editor: &mut Editor, last: &mut f64| {
            if time - *last > VIEW_BURST_GAP {
                editor.view_changing();
            }
            *last = time;
        };
        for e in &events {
            let egui::Event::MouseWheel { unit, delta, modifiers, .. } = e else { continue };
            if modifiers.command || modifiers.ctrl {
                continue; // egui turns these into zoom_delta
            }
            match unit {
                egui::MouseWheelUnit::Point => {
                    if time - self.last_trackpad > GESTURE_GAP {
                        editor.nav_begin_about(local(p), style.pivot);
                    }
                    self.last_trackpad = time;
                    let d = DVec2::new(-delta.x as f64, -delta.y as f64);
                    let action = if modifiers.shift { style.trackpad.shift_scroll } else { style.trackpad.scroll };
                    match action {
                        NavAction::Pan => editor.pan(-d),
                        _ => editor.orbit(d * trackpad_orbit),
                    }
                }
                egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                    burst(editor, &mut self.last_zoom);
                    editor.zoom(zoom_at(p), delta.y as f64 * zoom_steps);
                }
            }
        }
        if (zoom - 1.0).abs() > 1e-4 {
            burst(editor, &mut self.last_zoom);
            editor.zoom(zoom_at(p), (zoom as f64).ln() / (1.0f64 / 0.85).ln() * nav.zoom_rate());
        }
    }

    /// A component dragged in from the « Composants » card (docs/ui/components-library.md#placing): no
    /// widget of the view is hovered while the card's row holds the drag, so the pointer is
    /// followed here over the view (not over a card floating on it); released there, it is put
    /// down — a click —, elsewhere given up (Esc).
    fn dragged_in_input(&mut self, ctx: &egui::Context, rect: egui::Rect, editor: &mut Editor, mods: Modifiers) {
        let local = |p: egui::Pos2| DVec2::new((p.x - rect.min.x) as f64, (p.y - rect.min.y) as f64);
        let (pos, down) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down()));
        // Not over a card floating on the view; a tooltip following the pointer does not count.
        let over = pos.filter(|p| {
            rect.contains(*p) && ctx.layer_id_at(*p).is_none_or(|l| matches!(l.order, egui::Order::Background | egui::Order::Tooltip))
        });
        if !editor.carrying() {
            self.dragged_in = false;
            return;
        }
        match over {
            Some(p) if self.last_hover != Some(p) => {
                self.last_hover = Some(p);
                editor.input(ToolInput::Hover { px: local(p) });
            }
            None if self.last_hover.take().is_some() => editor.leave(),
            _ => {}
        }
        if down {
            return;
        }
        self.dragged_in = false;
        match over {
            Some(p) => editor.input(ToolInput::Click { px: local(p), mods }),
            None => editor.input(ToolInput::Key { key: skraft_tools::Key::Escape, mods: Modifiers::default() }),
        }
    }

    /// The modifiers the tool sees (docs/ui/navigation-styles.md, priority to the button): the ones it had
    /// when a navigation gesture began, as long as it lasts; after it, a modifier pressed
    /// during it stays hidden until released.
    fn tool_modifiers(&mut self, mods: egui::Modifiers, mac: bool) -> egui::Modifiers {
        let keys = |m: &egui::Modifiers| [m.shift, m.ctrl, m.alt, m.mac_cmd];
        let now = keys(&mods);
        let seen = if let Some(g) = &self.gesture {
            let before = keys(&g.tool_mods);
            for ((hidden, now), before) in self.hidden.iter_mut().zip(now).zip(before) {
                *hidden |= now && !before;
            }
            g.tool_mods
        } else {
            for (hidden, now) in self.hidden.iter_mut().zip(now) {
                *hidden &= now;
            }
            let mut m = mods;
            m.shift &= !self.hidden[0];
            m.ctrl &= !self.hidden[1];
            m.alt &= !self.hidden[2];
            m.mac_cmd &= !self.hidden[3];
            m.command = if mac { m.mac_cmd } else { m.ctrl };
            m
        };
        self.tool_mods = seen;
        seen
    }

    /// Is a navigation gesture under way? (A modifier tapped meanwhile is not a tap.)
    pub fn navigating(&self) -> bool {
        self.gesture.is_some()
    }

    /// Goes on preparing the textures of `doc`, whatever draws it or not, and says how many of
    /// its images are done, out of all (docs/ui/loading.md).
    pub fn textures_progress(&mut self, ctx: &egui::Context, doc: &skraft_model::Document) -> (usize, usize) {
        let mut ids: Vec<String> = doc.materials().filter_map(|(_, m)| m.texture.as_ref().map(|t| t.image.clone())).collect();
        ids.sort();
        ids.dedup();
        self.images.update(ctx, doc, ids.iter());
        (ids.iter().filter(|id| self.images.settled(id)).count(), ids.len())
    }

    /// The palette entry of `material`, given on first use (docs/render/look.md).
    fn slot(&mut self, material: Option<Id>) -> u32 {
        let Some(m) = material else { return render::NO_MATERIAL };
        let next = self.slots.len() as u32;
        *self.slots.entry(m).or_insert(next)
    }

    /// What every picture of the shown document needs, kept up to date: the palette (each
    /// material at its entry, then an open part's tools' colours, from the returned slot), the
    /// textures' layers, the meshes uploaded, the definitions' grains.
    fn sync(&mut self, state: &egui_wgpu::RenderState, editor: &Editor, theme: &Theme) -> (Vec<[f32; 4]>, Vec<[f32; 4]>, u32) {
        let device = &state.device;
        let doc = editor.document();
        // The palette: every material at its entry (entries of deleted ones stay unused).
        let materials: Vec<(Id, [u8; 3])> = doc.materials().map(|(id, m)| (id, m.color)).collect();
        for (id, _) in &materials {
            self.slot(Some(*id));
        }
        let mut palette = vec![[0.0f32; 4]; self.slots.len()];
        for (id, [r, g, b]) in &materials {
            palette[self.slots[id] as usize] = skraft_theme::srgb(*r, *g, *b);
        }
        // The textures (docs/render/look.md#textures): one layer per image used, its entry beside
        // its material's colour.
        let mut layers: Vec<String> = doc.materials().filter_map(|(_, m)| m.texture.as_ref().map(|t| t.image.clone())).collect();
        layers.sort();
        layers.dedup();
        // Those not ready yet ([`Viewport::textures_progress`], every frame): their material's
        // colour meanwhile.
        layers.retain(|id| self.images.get(id).is_some());
        let mut textures = vec![render::NO_TEXTURE; palette.len()];
        for (id, m) in doc.materials() {
            if let Some(t) = &m.texture
                && let Some(layer) = layers.iter().position(|l| *l == t.image)
            {
                textures[self.slots[&id] as usize] = render::texture_entry(layer as u32, t.size, t.rotation);
            }
        }
        // Then the colours of an open part's tools (docs/model/opening-a-part.md).
        let tool_slot = palette.len() as u32;
        palette.push(theme.machining);
        palette.push(theme.machining_failed);
        palette.push(theme.intersect);

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
        let document = self.document;
        let renderer = self.renderer.get_or_insert_with(|| render::Renderer::new(device));
        renderer.set_document(document);
        for (dm, paint) in &uploads {
            let parts: Vec<render::Part<'_>> =
                dm.parts.iter().map(|p| render::Part { id: p.solid.0, mesh: &p.mesh, face_paint: paint }).collect();
            renderer.set_mesh(device, dm.definition.0, dm.revision, &parts);
        }
        let layer_images: Vec<(u64, &[Vec<u8>])> =
            layers.iter().filter_map(|id| self.images.get(id).map(|l| (u64::from_str_radix(&id[..16], 16).unwrap_or(0), l))).collect();
        renderer.set_textures(device, &state.queue, &layer_images);
        // The grain of each definition: its mesh's longest axis, measured again when it changes.
        for dm in scene.definition_meshes() {
            let key = (document, dm.definition.0);
            if self.grains.get(&key).is_none_or(|(revision, _)| *revision != dm.revision) {
                self.grains.insert(key, (dm.revision, grain_of(dm)));
            }
        }
        renderer.retain_meshes(|k| scene.definition_mesh(skraft_model::Id(k)).is_some());
        (palette, textures, tool_slot)
    }

    fn render(&mut self, state: &egui_wgpu::RenderState, size: [u32; 2], pixels_per_point: f32, editor: &Editor, theme: &Theme) {
        let device = &state.device;
        let doc = editor.document();
        let (palette, textures, tool_slot) = self.sync(state, editor, theme);
        let scene = editor.scene();
        let document = self.document;
        let Some(renderer) = self.renderer.as_mut() else { return };
        let grains = &self.grains;
        let grain = |def: u64| grains.get(&(document, def)).map_or(0, |(_, g)| *g);

        let display = &doc.settings().display;
        // The palette's, not the document's (docs/render/look.md).
        let [r, g, b, _] = theme.ui.view;
        let background = [r, g, b];
        let start = web_time::Instant::now();
        let view = editor.view();
        self.timing.view_ms = start.elapsed().as_secs_f64() * 1e3;
        let fade: render::Tint = [background[0], background[1], background[2], theme.fade];
        let slots = &self.slots;
        let slot = |m: Option<Id>| m.and_then(|m| slots.get(&m).copied()).unwrap_or(render::NO_MATERIAL);
        // A machining's ghost, a little more opaque than a blank's.
        let machining = (theme.machining_opacity - theme.ghost_opacity).max(0.0);
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
                        // Not what the eraser would take: that one as faint as any.
                        opacity: if matches!(p.look, Look::Tool | Look::Intersect) && p.ghost && !p.faint_edges { machining } else { 0.0 },
                        material: match p.look {
                            Look::Tool => tool_slot,
                            Look::FailedTool => tool_slot + 1,
                            Look::Intersect => tool_slot + 2,
                            Look::Shape | Look::Blank => slot(p.material),
                        },
                    })
                    .collect(),
                ghost: places.iter().any(|p| p.ghost),
                faint_edges: places.iter().any(|p| p.faint_edges),
                grain: grain(def.0),
            })
            .collect();
        let highlights: Vec<render::Highlight> = view
            .highlights
            .iter()
            .map(|h| {
                let colors = mark_colors(theme, h.mark, fade);
                render::Highlight {
                    key: h.definition.0,
                    part: h.solid.0,
                    transform: DMat4::from(h.transform),
                    face_tint: colors.face,
                    edge_tint: colors.edge,
                    material: slot(h.material),
                    ghost: h.ghost,
                    face: h.face,
                    grain: grain(h.definition.0),
                }
            })
            .collect();
        // An open part always shows its axes (docs/model/opening-a-part.md): the grid leaves out its
        // lines along them.
        let axes = display.axes || editor.open_part().is_some();
        let (mut lines, mut axis_lines, mut faint_lines, mut overlay, mut hidden_lines, mut boxes, mut fills) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        // Everything drawn over the model, through the drawer (docs/render/drawer.md).
        let out = Translated {
            lines: &mut lines,
            axis_lines: &mut axis_lines,
            faint_lines: &mut faint_lines,
            overlay: &mut overlay,
            hidden_lines: &mut hidden_lines,
            boxes: &mut boxes,
            fills: &mut fills,
        };
        out.push(&view.drawing, |c| theme_color(theme, c));
        // The section's cut lines, bold in the edges' ink (docs/render/look.md): a highlighted part's in
        // its mark's edge colour, a faded part's faded.
        let [er, eg, eb, _] = theme.edge;
        lines.extend(view.section_cuts.iter().map(|c| render::Line {
            a: c.a,
            b: c.b,
            color: match c.mark {
                // Over the ink, as a highlight's edges.
                Some(m) => {
                    let [r, g, b, a] = mark_colors(theme, m, fade).edge;
                    [er + (r - er) * a, eg + (g - eg) * a, eb + (b - eb) * a, 1.0]
                }
                None if c.faded => [er, eg, eb, 1.0 - theme.fade],
                None => [er, eg, eb, 1.0],
            },
            dash: None,
            width: render::BOLD_WIDTH,
        }));

        let aspect = size[0] as f64 / size[1] as f64;
        let cam = &editor.camera;
        let rgb = |c: Rgba| [c[0], c[1], c[2]];
        let style = render::Style {
            face_front: rgb(theme.face_front),
            face_back: rgb(theme.face_back),
            edge: rgb(theme.edge),
            grid: rgb(theme.grid),
            palette: &palette,
            textures: &textures,
            silhouettes: display.silhouettes,
            section: view.section.map(|s| render::Section { origin: s.origin, normal: s.normal }),
            cap_shade: theme.cap_shade,
            hatch_opacity: theme.hatch_opacity,
            ghost_opacity: theme.ghost_opacity,
            ghost_edge_opacity: theme.ghost_edge_opacity,
        };
        self.palette.clone_from(&palette);
        self.textures.clone_from(&textures);
        let frame = render::Frame {
            view: cam.view(),
            proj: cam.proj(aspect, scene.bounds()),
            background,
            style,
            batches: &batches,
            highlights: &highlights,
            lines: &lines,
            axis_lines: &axis_lines,
            faint_lines: &faint_lines,
            boxes: &boxes,
            overlay: &overlay,
            hidden_lines: &hidden_lines,
            fills: &fills,
            grid: display.grid.then(|| grid_for(cam.focus)),
            axes,
            axes_frame: DMat4::from(editor.axes_transform()),
            pixels_per_point,
            model_per_point: {
                let points = DVec2::new(size[0] as f64, size[1] as f64) / pixels_per_point as f64;
                cam.world_per_pixel(points, cam.eye + cam.forward() * cam.focus)
            },
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport") });
        // A still view draws nothing: the target keeps its picture (docs/render/look.md).
        self.timing.redrawn = renderer.render(device, &state.queue, &mut encoder, size, &frame);
        if self.timing.redrawn {
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

impl Viewport {
    /// Draws the thumbnails the « Composants » card asked for (docs/ui/components-library.md#thumbnails),
    /// the shown document's, within [`THUMBNAIL_BUDGET_MS`]: the others the next frame. Each a
    /// parallel iso view of the component's meshes framed on their box, in the view's colours
    /// and with its palette, registered as an egui texture; those evicted freed.
    fn render_thumbnails(&mut self, ctx: &egui::Context, state: &egui_wgpu::RenderState, ppp: f32, editor: &Editor, theme: &Theme) {
        let freed = self.thumbnails.evict();
        if !freed.is_empty() {
            let mut egui_renderer = state.renderer.write();
            for t in freed {
                egui_renderer.free_texture(&t);
            }
        }
        if !self.thumbnails.pending() {
            return;
        }
        let Some(renderer) = self.renderer.as_mut() else { return };
        let side = ((crate::thumbnails::TILE * ppp).round() as u32).clamp(16, 512);
        let size = [side, side];
        let scene = editor.scene();
        let start = web_time::Instant::now();
        let mut rest = Vec::new();
        for w in self.thumbnails.take_wanted() {
            if w.doc != self.document {
                continue;
            }
            if start.elapsed().as_secs_f64() * 1e3 > THUMBNAIL_BUDGET_MS {
                rest.push(w);
                continue;
            }
            // Its box, in its axes: its meshes' boxes at their placements.
            let mut bounds: Option<(DVec3, DVec3)> = None;
            for (p, places) in &w.content.parts {
                let Some(dm) = scene.definition_mesh(*p) else { continue };
                for pm in &dm.parts {
                    let Some((lo, hi)) = pm.bounds else { continue };
                    for (t, _) in places {
                        for i in 0..8 {
                            let c = DVec3::new(
                                if i & 1 == 0 { lo.x } else { hi.x },
                                if i & 2 == 0 { lo.y } else { hi.y },
                                if i & 4 == 0 { lo.z } else { hi.z },
                            );
                            let c = t.transform_point3(c);
                            bounds = Some(bounds.map_or((c, c), |(a, b)| (a.min(c), b.max(c))));
                        }
                    }
                }
            }
            let Some(bounds) = bounds else { continue };
            let slots = &self.slots;
            let slot = |m: Option<Id>| m.and_then(|m| slots.get(&m).copied()).unwrap_or(render::NO_MATERIAL);
            let batches: Vec<render::Batch> = w
                .content
                .parts
                .iter()
                .filter(|(p, _)| renderer.has_mesh(p.0, scene.definition_mesh(*p).map_or(u64::MAX, |d| d.revision)))
                .map(|(p, places)| {
                    let placements = places
                        .iter()
                        .map(|(t, m)| render::Placement { material: slot(*m), ..render::Placement::new(DMat4::from(*t)) })
                        .collect();
                    let grain = self.grains.get(&(self.document, p.0)).map_or(0, |(_, g)| *g);
                    render::Batch { grain, ..render::Batch::new(p.0, placements) }
                })
                .collect();
            if batches.is_empty() {
                continue;
            }
            let (view, proj) = thumbnail_camera(bounds);
            let rgb = |c: Rgba| [c[0], c[1], c[2]];
            let [r, g, b, _] = theme.ui.view;
            let frame = render::Frame {
                view,
                proj,
                background: [r, g, b],
                style: render::Style {
                    face_front: rgb(theme.face_front),
                    face_back: rgb(theme.face_back),
                    edge: rgb(theme.edge),
                    grid: rgb(theme.grid),
                    palette: &self.palette,
                    textures: &self.textures,
                    silhouettes: true,
                    section: None,
                    ..render::Style::default()
                },
                batches: &batches,
                highlights: &[],
                lines: &[],
                axis_lines: &[],
                faint_lines: &[],
                boxes: &[],
                overlay: &[],
                hidden_lines: &[],
                fills: &[],
                grid: None,
                axes: false,
                axes_frame: DMat4::IDENTITY,
                // Edges and silhouettes finer than the view's, at the thumbnail's small scale.
                pixels_per_point: ppp * THUMBNAIL_EDGE_SCALE,
                model_per_point: 1.0,
            };
            let device = &state.device;
            if let Some(picture) = self.thumbnails.picture_for(&w, size) {
                renderer.render_picture(device, &state.queue, &frame, picture);
                self.thumbnails.redrawn(&w);
            } else {
                let picture = renderer.new_picture(device, size);
                renderer.render_picture(device, &state.queue, &frame, &picture);
                let mut egui_renderer = state.renderer.write();
                let texture = egui_renderer.register_native_texture(device, picture.display_view(), wgpu::FilterMode::Linear);
                if let Some(old) = self.thumbnails.drawn(&w, picture, texture) {
                    egui_renderer.free_texture(&old);
                }
            }
        }
        let more = !rest.is_empty();
        self.thumbnails.defer(rest);
        if more {
            ctx.request_repaint();
        }
    }
}

/// How much finer a thumbnail's edges are than the view's (`render::EDGE_WIDTH` times this).
const THUMBNAIL_EDGE_SCALE: f32 = 0.5;

/// What a sheet view's picture is drawn with: the theme, the palette and textures, the
/// materials' entries, the definitions' grains, the document shown, its points' pixels.
struct SheetLook<'a> {
    theme: &'a Theme,
    palette: &'a [[f32; 4]],
    textures: &'a [[f32; 4]],
    slots: &'a HashMap<Id, u32>,
    grains: &'a HashMap<(u64, u64), (u64, u8)>,
    document: u64,
    ppp: f32,
}

/// A sheet view's picture `w` drawn into `picture` (docs/ui/drawing.md#rendered-views): its
/// parts, its lines over them, on the paper's white.
fn draw_sheet_picture(
    renderer: &mut render::Renderer,
    state: &egui_wgpu::RenderState,
    scene: &skraft_tools::DisplayScene,
    look: &SheetLook<'_>,
    w: &crate::sheet::Wanted,
    picture: &render::Picture,
) {
    let (theme, palette, textures, document, ppp) = (look.theme, look.palette, look.textures, look.document, look.ppp);
    let slot = |m: Option<Id>| m.and_then(|m| look.slots.get(&m).copied()).unwrap_or(render::NO_MATERIAL);
    let rgb = |c: Rgba| [c[0], c[1], c[2]];
    let batches: Vec<render::Batch> = w
        .parts
        .iter()
        .filter(|(p, _)| renderer.has_mesh(p.0, scene.definition_mesh(*p).map_or(u64::MAX, |d| d.revision)))
        .map(|(p, places)| {
            let placements =
                places.iter().map(|(t, m)| render::Placement { material: slot(*m), ..render::Placement::new(DMat4::from(*t)) }).collect();
            let grain = look.grains.get(&(document, p.0)).map_or(0, |(_, g)| *g);
            render::Batch { grain, ..render::Batch::new(p.0, placements) }
        })
        .collect();
    // Its lines over everything: the visible ones dark, the hidden ones grey and dashed.
    let mut overlay = Vec::new();
    if let Some(pl) = &w.lines {
        let [right, up, forward] = pl.axes;
        let at = |p: glam::DVec2| match pl.eye {
            Some(eye) => eye + (right * p.x + up * p.y + forward) * pl.depth,
            None => right * p.x + up * p.y + forward * pl.depth,
        };
        if pl.hidden {
            let dash = Some(render::Dash { pattern: [pl.dash[0], pl.dash[1], 0.0, 0.0], lay: render::Lay::Screen(0.0) });
            for poly in &pl.lines.hidden {
                for s in poly.windows(2) {
                    overlay.push(render::Line { a: at(s[0]), b: at(s[1]), color: [0.25, 0.25, 0.25, 1.0], dash, width: pl.hidden_width });
                }
            }
        }
        for poly in &pl.lines.visible {
            for s in poly.windows(2) {
                overlay.push(render::Line { a: at(s[0]), b: at(s[1]), color: [0.01, 0.01, 0.01, 1.0], dash: None, width: pl.width });
            }
        }
    }
    let frame = render::Frame {
        view: w.camera.0,
        proj: w.camera.1,
        // The paper's.
        background: [1.0, 1.0, 1.0],
        style: render::Style {
            face_front: rgb(theme.face_front),
            face_back: rgb(theme.face_back),
            edge: rgb(theme.edge),
            grid: rgb(theme.grid),
            palette,
            textures,
            silhouettes: true,
            section: None,
            ..render::Style::default()
        },
        batches: &batches,
        highlights: &[],
        lines: &[],
        axis_lines: &[],
        faint_lines: &[],
        boxes: &[],
        overlay: &overlay,
        hidden_lines: &[],
        fills: &[],
        grid: None,
        axes: false,
        axes_frame: DMat4::IDENTITY,
        pixels_per_point: ppp,
        model_per_point: 1.0,
    };
    renderer.render_picture(&state.device, &state.queue, &frame, picture);
}

/// The pixels of `texture` (RGBA, rows top first), read back from the GPU.
fn read_texture(state: &egui_wgpu::RenderState, texture: &wgpu::Texture) -> Option<(u32, u32, Vec<u8>)> {
    let [w, h] = [texture.width(), texture.height()];
    let (device, queue) = (&state.device, &state.queue);
    let row = (w * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("read back"),
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

/// The time the views' pictures of a sheet may take per frame (milliseconds): the others wait
/// for the next.
const SHEET_BUDGET_MS: f64 = 40.0;

/// What the views' pictures may keep in memory, those on screen always kept (bytes).
const SHEET_KEPT_BYTES: u64 = if cfg!(target_arch = "wasm32") { 64 << 20 } else { 256 << 20 };

/// The time the thumbnails may take per frame (milliseconds): the others wait for the next.
const THUMBNAIL_BUDGET_MS: f64 = 6.0;

/// A thumbnail's camera: a parallel iso view from the front, right and above, framed tight on
/// `bounds` (their corners' extent across the view, a margin around), square.
fn thumbnail_camera((lo, hi): (DVec3, DVec3)) -> (DMat4, glam::Mat4) {
    let forward = DVec3::new(-1.0, 1.0, -1.0).normalize();
    let right = forward.cross(DVec3::Z).normalize();
    let up = right.cross(forward);
    let center = (lo + hi) * 0.5;
    let (mut w, mut h, mut d) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..8 {
        let c =
            DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z })
                - center;
        w = w.max(c.dot(right).abs());
        h = h.max(c.dot(up).abs());
        d = d.max(c.dot(forward).abs());
    }
    let half = (w.max(h) * 1.12).max(1e-3);
    let eye = center - forward * (2.0 * d + half + 1.0);
    let view = glam::dcamera::rh::view::look_to_mat4(eye, forward, up);
    let dist = (center - eye).dot(forward);
    let (near, far) = ((dist - d * 1.1 - 1.0).max(1e-3), dist + d * 1.1 + 1.0);
    // Reversed depth: near → 1, far → 0.
    let proj = glam::camera::rh::proj::directx::orthographic(-half as f32, half as f32, -half as f32, half as f32, far as f32, near as f32);
    (view, proj)
}

/// How a highlight's mark colours its faces and edges (`fade`: the tint of what lies outside
/// the context being edited).
fn mark_colors(theme: &Theme, mark: Mark, fade: render::Tint) -> skraft_theme::MarkColors {
    match mark {
        Mark::Selected => theme.selected,
        Mark::Hover => theme.hover,
        Mark::HoverFaded => skraft_theme::MarkColors { face: then_fade(theme.hover.face, fade), edge: then_fade(theme.hover.edge, fade) },
        Mark::Picked => theme.picked,
        Mark::PickedTool => theme.picked_tool,
        Mark::Ghost => theme.ghost,
        // The other copies a change reaches: the picked target's orange, lighter.
        Mark::Related => theme.related,
        // In the colour the paint would give: no tint over it.
        Mark::Painted => skraft_theme::MarkColors { face: [0.0; 4], edge: theme.hover.edge },
    }
}

/// The unit cube to the box `b`: scaled to its bounds, placed by its frame.
fn box_transform(b: &skraft_tools::OrientedBox) -> DMat4 {
    DMat4::from(b.frame) * DMat4::from_translation(b.lo) * DMat4::from_scale(b.hi - b.lo)
}

/// A colour of a drawing in the theme shown (linear RGBA).
fn theme_color(theme: &Theme, c: draw::Color) -> Rgba {
    match c {
        // A tool's ink, the mode's (docs/render/look.md#view-background).
        draw::Color::Rgba(c) => theme.ink(c),
        draw::Color::Theme(t) => match t {
            draw::ThemeColor::Edge => theme.edge,
            draw::ThemeColor::Guide => theme.guide,
            draw::ThemeColor::FadedGuide => [theme.guide[0], theme.guide[1], theme.guide[2], theme.guide[3] * (1.0 - theme.fade)],
            draw::ThemeColor::HalfFadedGuide => [theme.guide[0], theme.guide[1], theme.guide[2], theme.guide[3] * (1.0 - theme.fade * 0.5)],
            draw::ThemeColor::Hover => theme.hover.edge,
            draw::ThemeColor::Selected => theme.selected.edge,
            draw::ThemeColor::ContextBox => theme.context_box,
        },
    }
}

/// The renderer's lists a [`draw::Drawing`] goes into.
struct Translated<'a> {
    lines: &'a mut Vec<render::Line>,
    axis_lines: &'a mut Vec<render::Line>,
    faint_lines: &'a mut Vec<render::Line>,
    overlay: &'a mut Vec<render::Line>,
    hidden_lines: &'a mut Vec<render::Line>,
    boxes: &'a mut Vec<render::BoxOutline>,
    fills: &'a mut Vec<render::Fill>,
}

impl Translated<'_> {
    /// `drawing`'s lines, boxes and fills, each to the list of its layer; `color` resolves the
    /// theme's colours. (Its grips, marks and lengths are drawn by the app over the view.)
    fn push(self, drawing: &draw::Drawing, color: impl Fn(draw::Color) -> Rgba) {
        let dash = |stipple: draw::Stipple, lay: draw::Lay, phase: f64| {
            let lay = match lay {
                draw::Lay::Model => render::Lay::Model(phase),
                draw::Lay::Anchored => render::Lay::Anchored,
                draw::Lay::Screen => render::Lay::Screen(phase as f32),
            };
            stipple.pattern().map(|pattern| render::Dash { pattern, lay })
        };
        for l in &drawing.lines {
            let p = l.pen;
            let line = render::Line { a: l.a, b: l.b, color: color(p.color), dash: dash(p.stipple, p.lay, l.phase), width: p.width };
            match (p.layer, p.blended) {
                (draw::Layer::Over, _) => self.overlay.push(line),
                (draw::Layer::Axis, _) => self.axis_lines.push(line),
                (draw::Layer::Scene, true) => self.faint_lines.push(line),
                (draw::Layer::Scene, false) => self.lines.push(line),
            }
            if let (draw::Layer::Scene, Some(h)) = (p.layer, p.hidden) {
                let [r, g, b, a] = h.color.map_or(line.color, &color);
                // Its dashes those of the part seen when they are the same (anchored on the
                // whole segment: they run on through what hides them).
                let lay = match p.lay {
                    draw::Lay::Anchored if h.stipple == p.stipple => draw::Lay::Anchored,
                    draw::Lay::Screen => draw::Lay::Screen,
                    _ => draw::Lay::Model,
                };
                self.hidden_lines.push(render::Line {
                    color: [r, g, b, a * h.alpha],
                    width: h.width.unwrap_or(line.width),
                    dash: dash(h.stipple, lay, l.phase),
                    ..line
                });
            }
        }
        for b in &drawing.boxes {
            self.boxes.push(render::BoxOutline {
                transform: box_transform(&b.bounds),
                color: color(b.pen.color),
                width: b.pen.width,
                dash: dash(b.pen.stipple, draw::Lay::Anchored, 0.0),
                corners: b.corners,
                shell_on_top: b.shell_on_top,
                over: b.pen.layer == draw::Layer::Over,
            });
        }
        for f in &drawing.fills {
            self.fills.push(render::Fill {
                corners: f.corners.clone(),
                color: color(f.brush.color),
                triangles: f.triangles,
                over: f.brush.layer == draw::Layer::Over,
                cull_back: f.brush.cull_back,
            });
        }
    }
}

/// `n` with its thousands apart ("40 000").
fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202F}');
        }
        out.push(c);
    }
    out
}

/// A size in bytes, in the largest unit that keeps it above 1.
pub(crate) fn bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 20 => t!("render_stats.mebibytes", n = format!("{:.1}", n as f64 / (1u64 << 20) as f64)),
        n if n >= 1 << 10 => t!("render_stats.kibibytes", n = format!("{:.1}", n as f64 / (1u64 << 10) as f64)),
        n => t!("render_stats.bytes", n = n),
    }
}

/// The selection rectangle: solid when it takes what it encloses (drawn left to right),
/// dashed when it takes what it touches (right to left), as in SketchUp.
fn paint_marquee(painter: &egui::Painter, rect: egui::Rect, m: &skraft_tools::Marquee, theme: &Theme) {
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

/// How much longer than wide a bar point is, each way (points).
const BAR_LENGTHEN: f32 = 4.0;

/// The points a tool draws over everything (grips).
fn paint_overlay_points(painter: &egui::Painter, rect: egui::Rect, editor: &Editor, theme: &Theme) {
    let color = |c: Rgba| color(theme.ink(c));
    let size = editor.viewport_size();
    let screen = |p: DVec3| editor.camera.project(p, size).map(|at| rect.min + egui::vec2(at.x as f32, at.y as f32));
    for p in editor.overlay_points() {
        let Some(at) = screen(p.at) else { continue };
        let fill = p.fill.map_or(egui::Color32::TRANSPARENT, color);
        let stroke = p.stroke.map_or(egui::Stroke::NONE, |c| egui::Stroke::new(1.5, color(c)));
        let r = p.radius;
        match p.shape {
            PointShape::Diamond => {
                let corners = vec![at + egui::vec2(0.0, -r), at + egui::vec2(r, 0.0), at + egui::vec2(0.0, r), at + egui::vec2(-r, 0.0)];
                painter.add(egui::Shape::convex_polygon(corners, fill, stroke));
            }
            PointShape::Round => {
                painter.circle(at, r, fill, stroke);
            }
            PointShape::Square => {
                painter.rect(egui::Rect::from_center_size(at, egui::vec2(2.0 * r, 2.0 * r)), 0.0, fill, stroke, egui::StrokeKind::Middle);
            }
            PointShape::Bar { along } => {
                // The direction on the screen: from the point to a little further along.
                let toward = screen(p.at + along).map(|b| b - at).and_then(|d| (d.length() > 1e-3).then(|| d.normalized()));
                let (u, v) = match toward {
                    Some(u) => (u, u.rot90()),
                    None => (egui::vec2(1.0, 0.0), egui::vec2(0.0, 1.0)),
                };
                let long = if toward.is_some() { r + BAR_LENGTHEN } else { r };
                let corners = vec![at - u * long - v * r, at + u * long - v * r, at + u * long + v * r, at - u * long + v * r];
                painter.add(egui::Shape::convex_polygon(corners, fill, stroke));
            }
            PointShape::Padlock => paint_padlock(painter, at, r, fill, stroke),
            PointShape::Mirror => paint_mirror(painter, at, r, fill, stroke.color),
        }
    }
}

/// A closed padlock centred on `at`, `r` its half height: its body and its shackle alike,
/// filled and ringed (the shackle a band of the fill outlined by the ring).
fn paint_padlock(painter: &egui::Painter, at: egui::Pos2, r: f32, fill: egui::Color32, stroke: egui::Stroke) {
    let body = egui::Rect::from_center_size(at + egui::vec2(0.0, r * 0.35), egui::vec2(r * 1.6, r * 1.2));
    // The shackle: an arch over the body, its legs into it — narrow enough that, outline
    // included, it stands on the body's straight top, clear of its rounded corners.
    let (radius, top) = (r * 0.28, body.top());
    let centre = egui::pos2(at.x, top - r * 0.35);
    let mut arch = vec![egui::pos2(at.x - radius, top)];
    arch.extend((0..=12).map(|i| {
        let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
        centre + egui::vec2(a.cos(), a.sin()) * radius
    }));
    arch.push(egui::pos2(at.x + radius, top));
    let ink = if stroke.color == egui::Color32::TRANSPARENT { fill } else { stroke.color };
    painter.add(egui::Shape::line(arch.clone(), egui::Stroke::new(r * 0.24 + 2.0 * stroke.width, ink)));
    painter.add(egui::Shape::line(arch, egui::Stroke::new(r * 0.24, fill)));
    painter.rect(body, r * 0.2, fill, stroke, egui::StrokeKind::Middle);
    // The keyhole.
    painter.circle_filled(body.center() - egui::vec2(0.0, r * 0.1), r * 0.15, ink);
}

/// The mirror's badge (OpenCutList's motif): a square of half side `r` in `fill`, inside it two
/// triangles facing a dashed axis, in `ink`.
fn paint_mirror(painter: &egui::Painter, at: egui::Pos2, r: f32, fill: egui::Color32, ink: egui::Color32) {
    painter.rect_filled(egui::Rect::from_center_size(at, egui::vec2(2.0 * r, 2.0 * r)), r * 0.25, fill);
    // The motif in a unit square, padded by a fifth of the badge on each side.
    let inner = r * 0.6;
    let p = |x: f32, y: f32| at + egui::vec2((x - 0.5) * 2.0 * inner, (y - 0.5) * 2.0 * inner);
    let stroke = egui::Stroke::new(1.5, ink);
    for (a, b) in [(0.0, 0.2), (0.4, 0.6), (0.8, 1.0)] {
        painter.line_segment([p(0.5, a), p(0.5, b)], stroke);
    }
    for (side, tip) in [(0.0, 0.3), (1.0, 0.7)] {
        painter.add(egui::Shape::closed_line(vec![p(side, 0.2), p(tip, 0.5), p(side, 0.8)], stroke));
    }
}

/// Half the arm of a cross marking a point (points).
const MARK_ARM: f32 = 6.0;

/// The points a tool marks (a centre, a start): a `+` on the screen.
fn paint_marks(painter: &egui::Painter, rect: egui::Rect, editor: &Editor, theme: &Theme) {
    let stroke = egui::Stroke::new(1.5, color(theme.inks.tool));
    for p in editor.marks() {
        let Some(at) = editor.camera.project(p, editor.viewport_size()) else { continue };
        let at = rect.min + egui::vec2(at.x as f32, at.y as f32);
        painter.line_segment([at - egui::vec2(MARK_ARM, 0.0), at + egui::vec2(MARK_ARM, 0.0)], stroke);
        painter.line_segment([at - egui::vec2(0.0, MARK_ARM), at + egui::vec2(0.0, MARK_ARM)], stroke);
    }
}

/// The square around the system's cursor (points) the lengths and the tooltip keep clear of
/// (the app's own say where they draw, `cursors::footprint`).
const CURSOR_CLEAR: f32 = 24.0;
/// Between the cursor's drawing and its tooltip (points).
const TIP_GAP: f32 = 4.0;
/// The square around a cross or a marker (points) the lengths keep clear of.
const MARK_CLEAR: f32 = 16.0;
/// The room (points) a length being typed into leaves for its caret after the text.
const CARET_ROOM: f32 = 4.0;
/// What a text typed in a length reads as, written after it.
const RESULT_INK: egui::Color32 = egui::Color32::from_gray(140);
/// The gap (points) between a length moved off its side and the side.
const LABEL_GAP: f32 = 4.0;

/// The lengths a tool writes by what it draws, framed in their axis's colour, on white with
/// square corners: at the middle of their side, else slid along it or off it to keep clear of
/// `clear` (the cursor, the markers, the tooltip), the crosses and the lengths placed before.
/// A length whose value is being typed (in `typed`) shows it, framed bolder, with a caret.
fn paint_labels(painter: &egui::Painter, rect: egui::Rect, editor: &Editor, typed: &str, mut clear: Vec<egui::Rect>) {
    use egui::{Stroke, vec2};
    let size = editor.viewport_size();
    let screen = |p: DVec3| editor.camera.project(p, size).map(|at| rect.min + vec2(at.x as f32, at.y as f32));
    clear.extend(editor.marks().into_iter().filter_map(screen).map(|at| egui::Rect::from_center_size(at, vec2(MARK_CLEAR, MARK_CLEAR))));
    for l in editor.labels(typed) {
        // A side half behind the eye: its middle only.
        let Some((a, b)) = screen(l.a).zip(screen(l.b)).or_else(|| screen((l.a + l.b) / 2.0).map(|m| (m, m))) else { continue };
        let ink = color(l.color);
        let font = egui::FontId::proportional(12.0);
        let typed = painter.layout_no_wrap(l.text, font.clone(), ink);
        // What the text typed reads as, greyed after it (and after the caret).
        let result = l.result.map(|r| {
            let lead = if typed.size().x > 0.0 { " " } else { "" };
            painter.layout_no_wrap(format!("{lead}= {r}"), font, RESULT_INK)
        });
        let caret = if l.active { CARET_ROOM } else { 0.0 };
        let height = typed.size().y;
        let size = vec2(typed.size().x + caret + result.as_ref().map_or(0.0, |r| r.size().x), height);
        let bg = place(size + vec2(10.0, 4.0), a, b, &clear);
        let width = if l.active { 2.0 } else { 1.0 };
        painter.rect(bg, 0.0, egui::Color32::WHITE, Stroke::new(width, ink), egui::StrokeKind::Inside);
        let at = bg.center() - size / 2.0;
        let end = at.x + typed.size().x;
        painter.galley(at, typed, ink);
        if l.active {
            let x = end + CARET_ROOM / 2.0;
            painter.line_segment([egui::pos2(x, at.y + 1.0), egui::pos2(x, at.y + height - 1.0)], Stroke::new(1.0, ink));
        }
        if let Some(r) = result {
            painter.galley(egui::pos2(end + caret, at.y), r, RESULT_INK);
        }
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

/// The inferred point's marker, coloured by kind, and its tooltip by the cursor (docs/tools/inferences.md).
/// What it draws (screen), for the lengths to keep clear of.
fn paint_inference(
    painter: &egui::Painter,
    rect: egui::Rect,
    cursor: egui::Pos2,
    footprint: egui::Rect,
    editor: &Editor,
    theme: &Theme,
) -> Vec<egui::Rect> {
    use egui::{Color32, Stroke, vec2};
    let mut drawn = Vec::new();
    let mut mark = |at: egui::Pos2| drawn.push(egui::Rect::from_center_size(at, vec2(MARK_CLEAR, MARK_CLEAR)));
    let to_screen = |p| editor.camera.project(p, editor.viewport_size()).map(|s| rect.min + vec2(s.x as f32, s.y as f32));
    // Violet rings are the temporary points, each on a light edge.
    let ring = |at: egui::Pos2, radius: f32| {
        painter.circle_stroke(at, radius, Stroke::new(3.0, color(theme.tracked_halo)));
        painter.circle_stroke(at, radius, Stroke::new(1.5, color(theme.tracked)));
    };
    let inference = editor.inference();
    // The middle of the tracked points: a small ring; aimed at, the line it stands on.
    if let [a, b] = editor.tracked() {
        if matches!(inference, Some((_, InferKind::TrackedMiddle))) {
            if let (Some(m), Some(a), Some(b)) = (to_screen((*a + *b) * 0.5), to_screen(*a), to_screen(*b)) {
                // Stopped at the rings, the marker's and the two points'.
                let stroke = Stroke::new(1.0, color(theme.tracked));
                for end in [a, b] {
                    if m.distance(end) > 14.0 {
                        let u = (end - m).normalized();
                        painter.extend(egui::Shape::dashed_line(&[m + u * 7.0, end - u * 7.0], stroke, 4.0, 3.0));
                    }
                }
            }
        } else if let Some(at) = to_screen((*a + *b) * 0.5) {
            ring(at, 3.0);
        }
    }
    // Tracked points: a ring wide enough to surround the marker of the point hovered there.
    for p in editor.tracked() {
        if let Some(at) = to_screen(*p) {
            ring(at, 7.0);
            mark(at);
        }
    }
    let Some((point, kind)) = inference else {
        // A tool's tooltip of its own (the pipette's material).
        if let Some(tip) = editor.inference_tip() {
            drawn.push(cursor_tip(painter, rect, cursor, footprint, tip, theme));
        }
        return drawn;
    };
    let Some(at) = editor.camera.project(point, editor.viewport_size()) else { return drawn };
    let at = rect.min + vec2(at.x as f32, at.y as f32);
    if kind != InferKind::OnPlane {
        mark(at);
    }
    let outline = Stroke::new(1.0, color(theme.marker_outline));
    let cross = |at: egui::Pos2, r: f32| {
        painter.line_segment([at - vec2(r, 0.0), at + vec2(r, 0.0)], outline);
        painter.line_segment([at - vec2(0.0, r), at + vec2(0.0, r)], outline);
    };
    match kind {
        InferKind::Endpoint => {
            painter.circle(at, 5.0, color(theme.endpoint), outline);
        }
        InferKind::Midpoint => {
            painter.circle(at, 5.0, color(theme.midpoint), outline);
        }
        InferKind::Third => {
            let pts = vec![at + vec2(0.0, -6.0), at + vec2(5.5, 4.0), at + vec2(-5.5, 4.0)];
            painter.add(egui::Shape::convex_polygon(pts, color(theme.third), outline));
        }
        InferKind::TrackedMiddle => {
            ring(at, 7.0);
            painter.circle(at, 2.5, color(theme.tracked), Stroke::new(1.0, color(theme.tracked_halo)));
        }
        InferKind::BoxCorner => {
            let square = egui::Rect::from_center_size(at, vec2(9.0, 9.0));
            painter.rect(square, 0.0, color(theme.endpoint), outline, egui::StrokeKind::Middle);
        }
        InferKind::Center => {
            painter.circle(at, 5.0, color(theme.center), outline);
            cross(at, 3.0);
        }
        InferKind::Origin => {
            painter.circle(at, 4.0, color(theme.origin), outline);
            cross(at, 8.0);
        }
        InferKind::OnTwoLines(_, second) => {
            // The first line's colour, ringed with the second's.
            let rgb = |k: InferKind| {
                let [r, g, b, _] = skraft_tools::tools::guide_color(k).map(|c| (c * 255.0) as u8);
                Color32::from_rgb(r, g, b)
            };
            painter.circle(at, 6.0, rgb(InferKind::OnLine(second)), outline);
            painter.circle(at, 3.0, rgb(kind), Stroke::NONE);
        }
        InferKind::OnEdgeAndLine(_) | InferKind::OnGuideAndLine(_) | InferKind::OnGuideAndEdge | InferKind::OnTwoGuides => {
            // The edge's square, in the line's colour.
            let [r, g, b, _] = skraft_tools::tools::guide_color(kind).map(|c| (c * 255.0) as u8);
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
            let [r, g, b, _] = skraft_tools::tools::guide_color(kind).map(|c| (c * 255.0) as u8);
            painter.circle(at, 4.0, Color32::from_rgb(r, g, b), outline);
        }
        InferKind::OnPlane => {}
    }
    let label = match (kind.label(), editor.inference_tip()) {
        (label, None) => label,
        (label, Some(tip)) if label.is_empty() => tip,
        (label, Some(tip)) => format!("{label}\n{tip}"),
    };
    if label.is_empty() {
        return drawn;
    }
    drawn.push(cursor_tip(painter, rect, cursor, footprint, label, theme));
    drawn
}

/// The tooltip `label` by the cursor, off its drawing (`footprint`, about the pointer); its
/// frame (screen).
fn cursor_tip(
    painter: &egui::Painter,
    view: egui::Rect,
    cursor: egui::Pos2,
    footprint: egui::Rect,
    label: String,
    theme: &Theme,
) -> egui::Rect {
    use egui::{Stroke, vec2};
    const PAD: egui::Vec2 = vec2(5.0, 2.0);
    let text = color(theme.tooltip_text);
    let galley = painter.layout_no_wrap(label, egui::FontId::proportional(12.0), text);
    let bg = tip_place(view, cursor, footprint, galley.size() + 2.0 * PAD);
    painter.rect(bg, 3.0, color(theme.tooltip_fill), Stroke::new(1.0, color(theme.tooltip_border)), egui::StrokeKind::Inside);
    painter.galley(bg.min + PAD, galley, text);
    bg
}

/// Where a tooltip of `size` goes by the pointer at `cursor`: under the cursor's drawing
/// (`footprint`, about the pointer), from a little right of the pointer — above it when the
/// view's bottom is too near —, moved left to stay in the view.
fn tip_place(view: egui::Rect, cursor: egui::Pos2, footprint: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let drawn = footprint.translate(cursor.to_vec2());
    let below = drawn.bottom() + TIP_GAP;
    let y = if below + size.y > view.bottom() { drawn.top() - TIP_GAP - size.y } else { below };
    let x = (cursor.x + 9.0).min(view.right() - size.x).max(view.left());
    egui::Rect::from_min_size(egui::pos2(x, y), size)
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

    /// A tooltip goes under the cursor's drawing, above it at the view's bottom, and stays in
    /// the view.
    #[test]
    fn a_tooltip_keeps_off_the_cursor() {
        let view = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        // The pipette's drawing: right of the pointer and 26 points below it.
        let footprint = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(26.0, 26.0));
        let size = egui::vec2(80.0, 20.0);
        let at = egui::pos2(100.0, 100.0);
        let tip = tip_place(view, at, footprint, size);
        assert!(tip.top() >= 126.0 && !tip.intersects(footprint.translate(at.to_vec2())), "{tip:?}");
        // Near the bottom: above the drawing.
        let low = egui::pos2(100.0, 590.0);
        let tip = tip_place(view, low, footprint, size);
        assert!(tip.bottom() <= 590.0, "{tip:?}");
        // Near the right edge: moved left, inside.
        let tip = tip_place(view, egui::pos2(790.0, 100.0), footprint, size);
        assert!(tip.right() <= 800.0, "{tip:?}");
    }
    use super::{place, tip_place};
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

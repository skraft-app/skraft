//! The window's layout: the columns, the panels' cards, the stage, the status bar.

use super::*;

/// A side panel dragged by its header to another place.
#[derive(Clone, Copy)]
pub(super) struct PanelDrag {
    pub(super) id: PanelId,
    /// The pointer from the card's top left corner.
    pub(super) offset: egui::Vec2,
    pub(super) size: egui::Vec2,
}

/// A split stage's xs ([`App::split`]): its table's left edge, where the 3D view starts, the
/// right end of the room they share.
#[derive(Clone, Copy, Debug)]
pub(super) struct Split {
    table: f32,
    pub(super) view: f32,
    pub(super) right: f32,
}

/// The least and most of the room a split stage's table takes.
const TABLE_SHARE: (f32, f32) = (0.2, 0.8);

impl SkraftApp {
    /// The paint tool or « Prélever » just started (whatever started it: the ribbon, its key,
    /// the palette, a material picked, a client): the Matières panel shown and unfolded, to see
    /// what it paints with (docs/render/look.md); left as it is when the tool ends. Says whether
    /// it did.
    pub(super) fn reveal_materials(&mut self) -> bool {
        let painting = self.docs.active().is_some_and(|d| matches!(d.editor.tool_kind(), ToolKind::Paint | ToolKind::Sample));
        let started = painting && !self.painting;
        self.painting = painting;
        started && self.prefs.layout_mut(self.workspace().id).reveal(PanelId::Materials)
    }

    /// The docked cards' columns: each one's left edge and width (left, right), the view's span
    /// the shown ones leave free (x) — a dragged card still holding its room in its column —,
    /// and a column's widest.
    fn columns(&self, view: egui::Rect, margin: f32) -> ([(f32, f32); 2], egui::Rangef, f32) {
        let max_width = (view.width() * 0.4).max(dock::COLUMN_MIN_WIDTH);
        let column = |side: Dock| {
            let width = self.prefs.layout(self.workspace().id).column(side).width.clamp(dock::COLUMN_MIN_WIDTH, max_width);
            match side {
                Dock::Left => (view.left() + margin, width),
                Dock::Right => (view.right() - margin - width, width),
            }
        };
        let columns = [column(Dock::Left), column(Dock::Right)];
        let left = if self.prefs.layout(self.workspace().id).occupied(Dock::Left, None) {
            columns[0].0 + columns[0].1 + margin
        } else {
            view.left()
        };
        let right = if self.prefs.layout(self.workspace().id).occupied(Dock::Right, None) { columns[1].0 - margin } else { view.right() };
        (columns, egui::Rangef::new(left, right), max_width)
    }

    /// A split stage's xs in `area` (docs/ui/workspaces.md#stage): its table from the left
    /// column (or the area's edge) to the view, which goes on to the area's right edge, under
    /// the right column; the room between the columns shared by the preference `table_share`.
    pub(super) fn split(&self, area: egui::Rect, margin: f32) -> Split {
        let (_, free, _) = self.columns(area, margin);
        let table = if free.min > area.left() { free.min } else { area.left() + margin };
        let right = if free.max < area.right() { free.max } else { area.right() - margin };
        let share = self.prefs.table_share.clamp(TABLE_SHARE.0, TABLE_SHARE.1);
        Split { table, view: table + share * (right - table).max(0.0) + margin, right }
    }

    /// A split stage's table in `rect` (the parts list), and the separation right of it,
    /// `margin` wide, dragged to share the room between the table and the view.
    fn stage_table(&mut self, ctx: &egui::Context, rect: egui::Rect, area: egui::Rect, margin: f32, actions: &mut Vec<Action>) {
        let Some(d) = self.docs.active_mut() else { return };
        egui::Area::new(egui::Id::unique("stage_table")).fixed_pos(rect.min).constrain_to(area).show(ctx, |ui| {
            let frame = card_frame(ui.style());
            let inner = (rect.size() - frame.total_margin().sum()).max(egui::Vec2::ZERO);
            frame.show(ui, |ui| {
                ui.set_min_size(inner);
                ui.set_max_size(inner);
                crate::kit::handles::publish(|| "parts_list".into(), rect);
                // A title bar as the side panels' (neither folded nor moved: no chevron).
                let header = ui.spacing().interact_size.y;
                let (head_rect, _) = ui.allocate_exact_size(egui::vec2(inner.x, header), egui::Sense::hover());
                title_bar(ui, rect, head_rect, &t!("parts_list.title"), false, true);
                ui.add_space(2.0 * HEAD_PAD);
                crate::cards::parts_table::ui(ui, &d.editor, &mut d.parts_list, actions);
            });
        });
        let grip = egui::Rect::from_x_y_ranges(rect.right()..=rect.right() + margin, rect.y_range());
        let split = self.split(area, margin);
        egui::Area::new(egui::Id::unique("stage_split")).fixed_pos(grip.min).show(ctx, |ui| {
            let (_, edge) = ui.allocate_exact_size(grip.size(), egui::Sense::click_and_drag());
            crate::kit::handles::publish(|| "stage_split".into(), grip);
            if edge.hovered() || edge.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if edge.dragged()
                && let Some(p) = edge.interact_pointer_pos()
            {
                let room = (split.right - split.table).max(1.0);
                self.prefs.table_share = ((p.x - margin / 2.0 - split.table) / room).clamp(TABLE_SHARE.0, TABLE_SHARE.1);
            }
            if edge.double_clicked() {
                self.prefs.table_share = Preferences::default().table_share;
            }
        });
    }

    /// The side panels, as cards in their columns ([`dock`]), the one dragged by its header
    /// following the pointer. Gives the view's span left free between the columns (x).
    /// The docked cards, in columns from `top` (the view's top, or under the ribbon when it is
    /// above them) to the bottom of the view.
    fn side_panels(&mut self, ctx: &egui::Context, view: egui::Rect, top: f32, margin: f32, actions: &mut Vec<Action>) -> egui::Rangef {
        let style = ctx.global_style();
        let frame = card_frame(&style).total_margin().sum();
        let header = style.spacing.interact_size.y;
        let metrics = dock::Metrics { header: header + frame.y, min_height: header + frame.y + 2.0 * HEAD_PAD + 60.0, gap: margin };
        let (xs, free, max_width) = self.columns(view, margin);
        let pointer = ctx.pointer_latest_pos();
        let dragged = self.panel_drag.map(|d| d.id);
        let room = view.bottom() - margin - top;
        let mut event = None;
        let mut outliner_shown = false;
        let mut columns = Vec::new();
        for (side, (x, width)) in [Dock::Left, Dock::Right].into_iter().zip(xs) {
            let placed = dock::place_with(self.prefs.layout(self.workspace().id).column(side), top, room, metrics, dragged, PanelId::fills);
            for p in &placed {
                let rect = egui::Rect::from_min_size(egui::pos2(x, p.top), egui::vec2(width, p.height));
                // Every card but the one filling the column has a bottom edge to drag.
                let edges = Edges { side: Some(side), bottom: !p.fills, max_width, min_height: metrics.min_height };
                if let Some(e) = self.panel_card(ctx, p.id, rect, header, edges, &mut outliner_shown, actions) {
                    event = Some(e);
                }
            }
            columns.push((side, x, width, placed));
        }
        if let (Some(d), Some(p)) = (self.panel_drag, pointer) {
            let rect = egui::Rect::from_min_size(p - d.offset, d.size);
            let edges = Edges { side: None, bottom: false, max_width, min_height: metrics.min_height };
            if let Some(e) = self.panel_card(ctx, d.id, rect, header, edges, &mut outliner_shown, actions) {
                event = Some(e);
            }
        }
        if !outliner_shown {
            actions.push(Action::HoverOutliner(None));
            actions.push(Action::HoverMachining(None));
        }
        // Where a dragged card goes: the column of the nearer edge, above the first card whose
        // middle is below the pointer.
        let target = |p: egui::Pos2| {
            let side = if p.x < view.center().x { Dock::Left } else { Dock::Right };
            let (_, x, width, placed) = columns.iter().find(|c| c.0 == side).expect("both columns");
            let before = placed.iter().find(|q| q.top + q.height / 2.0 > p.y);
            (side, *x, *width, placed, before)
        };
        match event {
            Some(CardEvent::Grabbed(drag)) => self.panel_drag = Some(drag),
            Some(CardEvent::Dropped) => {
                if let (Some(d), Some(p)) = (self.panel_drag.take(), pointer) {
                    let (side, .., before) = target(p);
                    self.prefs.layout_mut(self.workspace().id).move_card(d.id, side, before.map(|q| q.id));
                }
            }
            None => {
                if let (Some(_), Some(p)) = (self.panel_drag, pointer) {
                    // Shown under the dragged card: the whole column if empty, else a bar
                    // where the card will be inserted.
                    let (_, x, width, placed, before) = target(p);
                    let accent = style.visuals.selection.bg_fill;
                    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("panel_drop")));
                    let x_range = egui::Rangef::new(x, x + width);
                    match (before, placed.last()) {
                        (_, None) => {
                            let r = egui::Rect::from_x_y_ranges(x_range, top..=view.bottom() - margin);
                            painter.rect(r, 6.0, accent.gamma_multiply(0.25), egui::Stroke::new(1.5, accent), egui::StrokeKind::Inside);
                        }
                        (b, Some(last)) => {
                            let y = b.map_or(last.top + last.height + margin / 2.0, |q| q.top - margin / 2.0);
                            let r = egui::Rect::from_x_y_ranges(x_range, y - 2.0..=y + 2.0);
                            painter.rect_filled(r, 2.0, accent);
                        }
                    }
                }
            }
        }
        free
    }

    /// One side panel in `rect`: its header (click: fold, drag: move it) and its content, and
    /// the edges `edges` gives to drag (width of its column, its height).
    #[allow(clippy::too_many_arguments)]
    fn panel_card(
        &mut self,
        ctx: &egui::Context,
        id: PanelId,
        rect: egui::Rect,
        header: f32,
        edges: Edges,
        outliner_shown: &mut bool,
        actions: &mut Vec<Action>,
    ) -> Option<CardEvent> {
        let workspace = self.workspace().id;
        let folded = self.prefs.layout(workspace).card(id).is_some_and(|c| c.folded);
        let floating = edges.side.is_none();
        let mut event = None;
        // Drawn only with a document shown.
        let i = self.docs.active_index()?;
        let docs = &mut self.docs;
        egui::Area::new(egui::Id::unique(("panel", id)))
            .fixed_pos(rect.min)
            .order(if floating { egui::Order::Foreground } else { egui::Order::Middle })
            .show(ctx, |ui| {
                let frame = card_frame(ui.style());
                let inner = rect.size() - frame.total_margin().sum();
                frame.show(ui, |ui| {
                    ui.set_min_size(inner);
                    ui.set_max_size(inner);
                    let (head_rect, _) = ui.allocate_exact_size(egui::vec2(inner.x, header), egui::Sense::hover());
                    let head = ui.interact(head_rect, egui::Id::unique(("panel_head", id)), egui::Sense::click_and_drag());
                    // No line above the « Composants » card's tabs, whose band of the same colour
                    // goes on from the title's.
                    let band = title_bar(ui, rect, head_rect, &panel_title(id), folded, id != PanelId::Components);
                    let stroke = ui.visuals().window_stroke;
                    let color = ui.visuals().strong_text_color();
                    // The fold chevron at the right end: down when open, right when folded.
                    let angle = if folded { -std::f32::consts::FRAC_PI_2 } else { 0.0 };
                    crate::kit::icons::chevron(ui, egui::pos2(head_rect.right() - 8.0, head_rect.center().y), 14.0, color, angle);
                    if head.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    } else if head.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    if head.clicked()
                        && let Some(c) = self.prefs.layout_mut(workspace).card_mut(id)
                    {
                        c.folded = !c.folded;
                    }
                    if head.drag_started()
                        && let Some(p) = head.interact_pointer_pos()
                    {
                        event = Some(CardEvent::Grabbed(PanelDrag { id, offset: p - rect.min, size: rect.size() }));
                    }
                    if head.drag_stopped() {
                        event = Some(CardEvent::Dropped);
                    }
                    if folded {
                        return;
                    }
                    let Some(doc) = docs.iter_mut().nth(i) else { return };
                    // The navigator's rows close under the title bar (their backgrounds have
                    // their own room, `panels::outliner`); the other cards' content further down.
                    let pad = if id == PanelId::Outliner { HEAD_PAD / 2.0 } else { 2.0 * HEAD_PAD };
                    ui.add_space(pad);
                    let body = (inner.y - header - pad - 2.0 * ui.spacing().item_spacing.y).max(0.0);
                    // The bottom of the line under the title bar.
                    let title_line = band.bottom() + stroke.width / 2.0;
                    // The floating scroll bar out in the card's right margin, a pixel short of its
                    // border, and no wider than that margin once hovered: over the content it hid
                    // the rows' right end (the navigator's eyes, signs).
                    let scroll = &mut ui.spacing_mut().scroll;
                    scroll.bar_outer_margin = -(CARD_MARGIN - 1.0);
                    scroll.bar_width = CARD_MARGIN;
                    let area = egui::ScrollArea::vertical().auto_shrink(false).max_height(body).min_scrolled_height(body);
                    match id {
                        PanelId::EntityInfo => {
                            // Salted by the document: a field focused in one is not another's.
                            let doc_id = doc.id;
                            let name = doc.name();
                            // In « Mise en plan », the view picked or the sheet (docs/ui/drawing.md).
                            let sheet = doc.workspace.stage == Stage::Sheet;
                            area.id_salt("entity_info").show(ui, |ui| {
                                ui.push_id(doc_id, |ui| {
                                    if sheet {
                                        panels::drawing_info(ui, &doc.editor, &mut doc.sheet, &name, actions)
                                    } else {
                                        panels::info(ui, &doc.editor, &name, actions)
                                    }
                                })
                            });
                        }
                        PanelId::Materials => {
                            let (doc_id, list) = (doc.id, self.prefs.materials_list);
                            let (editor, state) = (&doc.editor, &mut doc.materials);
                            ui.push_id(doc_id, |ui| {
                                crate::cards::materials::card(ui, editor, state, list, body, actions, area.id_salt("materials"))
                            });
                        }
                        PanelId::Components => {
                            let doc_id = doc.id;
                            let (editor, state) = (&doc.editor, &mut doc.library);
                            let look = crate::cards::library::Look {
                                doc: doc_id,
                                thumbnails: &mut self.viewport.thumbnails,
                                list: self.prefs.components_list,
                                height: body,
                                band_top: head_rect.bottom() + HEAD_PAD,
                                margin: CARD_MARGIN,
                            };
                            ui.push_id(doc_id, |ui| {
                                crate::cards::library::components(ui, editor, state, look, actions, area.id_salt("components"))
                            });
                        }
                        PanelId::Compositions => {
                            let (doc_id, list) = (doc.id, self.prefs.compositions_list);
                            let (editor, st) = (&doc.editor, &mut doc.sheet);
                            ui.push_id(doc_id, |ui| {
                                crate::drawing::sheet_library::compositions(
                                    ui,
                                    editor,
                                    st,
                                    list,
                                    body,
                                    actions,
                                    area.id_salt("compositions"),
                                )
                            });
                        }
                        // On a sheet, the drawing's tree (docs/ui/drawing.md#navigator).
                        PanelId::Outliner if doc.workspace.stage == Stage::Sheet => {
                            let (doc_id, name) = (doc.id, doc.name());
                            let (editor, st) = (&doc.editor, &mut doc.sheet);
                            ui.push_id(doc_id, |ui| {
                                crate::drawing::sheet_tree::navigator(ui, editor, st, &name, actions, area.id_salt("sheet_tree"))
                            });
                        }
                        PanelId::Outliner => {
                            *outliner_shown = true;
                            // Salted by the document: its rows' clicks apart from another's.
                            let (doc_id, name) = (doc.id, doc.name());
                            let (editor, state) = (&doc.editor, &mut doc.outliner);
                            ui.push_id(doc_id, |ui| {
                                panels::outliner(ui, editor, state, &name, actions, area.id_salt("outliner"), (body, title_line))
                            });
                        }
                    }
                });
                let Some(side) = edges.side else { return };
                // The edge facing the view widens or narrows the column (double click: its
                // default width).
                let (x, sign) = match side {
                    Dock::Left => (rect.right(), 1.0),
                    Dock::Right => (rect.left(), -1.0),
                };
                let grip = egui::Rect::from_x_y_ranges(x - 3.0..=x + 3.0, rect.y_range());
                let edge = ui.interact(grip, egui::Id::unique(("panel_width", id)), egui::Sense::click_and_drag());
                if edge.hovered() || edge.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                let column = self.prefs.layout_mut(workspace).column_mut(side);
                if edge.dragged() {
                    column.width = (rect.width() + sign * edge.drag_delta().x).clamp(dock::COLUMN_MIN_WIDTH, edges.max_width);
                }
                if edge.double_clicked() {
                    column.width = dock::COLUMN_WIDTH;
                }
                // The bottom edge sets its height (the card filling the column takes the rest).
                if !edges.bottom || folded {
                    return;
                }
                let grip = egui::Rect::from_x_y_ranges(rect.x_range(), rect.bottom() - 3.0..=rect.bottom() + 3.0);
                let edge = ui.interact(grip, egui::Id::unique(("panel_height", id)), egui::Sense::drag());
                if edge.hovered() || edge.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                if edge.dragged()
                    && let Some(c) = self.prefs.layout_mut(workspace).card_mut(id)
                {
                    c.height = (rect.height() + edge.drag_delta().y).max(edges.min_height);
                }
            });
        event
    }

    /// The status bar: the tool's message, then the measurements box (VCB) at the right end.
    /// The box is laid out first: a long message is cut short (…) before it, whole in its tip.
    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let Some(d) = self.docs.active_mut() else { return };
            // On a sheet, its tool's hint and its own measurements box.
            if d.workspace.stage == Stage::Sheet {
                let status = d.sheet.status();
                self.driving.message(&status);
                let measure = d.sheet.vcb();
                let takes = measure.is_some();
                let (label, value) = measure.unwrap_or_else(|| (t!("status.measurements"), String::new()));
                // What was typed, while the box still takes it.
                if !takes {
                    d.vcb.clear();
                }
                let mut text = if d.vcb.is_empty() { value } else { d.vcb.clone() };
                let r = ui.add_enabled(takes, egui::TextEdit::singleline(&mut text).desired_width(160.0));
                crate::kit::handles::publish_enabled(|| "vcb".into(), r.rect, takes);
                if r.changed() {
                    d.vcb = text.clone();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !d.vcb.is_empty() {
                    d.sheet.typed = Some(std::mem::take(&mut d.vcb));
                }
                ui.label(label);
                ui.add_space(ui.spacing().item_spacing.x * 2.0);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(status).truncate());
                });
                return;
            }
            let status = d.editor.status();
            self.driving.message(&status);
            // The box first (right to left): its field, then its label at its left.
            let (label, value) = d.editor.vcb().unwrap_or_else(|| (t!("status.measurements"), String::new()));
            let mut text = if d.vcb.is_empty() { value } else { d.vcb.clone() };
            let r = ui.add_enabled(d.editor.takes_typing(), egui::TextEdit::singleline(&mut text).desired_width(160.0));
            crate::kit::handles::publish_enabled(|| "vcb".into(), r.rect, d.editor.takes_typing());
            if r.changed() {
                d.vcb = text.clone();
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !d.vcb.is_empty() {
                let t = std::mem::take(&mut d.vcb);
                d.editor.input(ToolInput::Vcb(t));
            }
            ui.label(label);
            ui.add_space(ui.spacing().item_spacing.x * 2.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(status).truncate());
            });
        });
    }

    /// The start page (docs/ui/file-tabs.md): no document open.
    pub(super) fn start_page(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(24.0));
            // The app's mark. Its tile fills 824 of the icon's 1024 units: drawn larger so the
            // tile is MARK wide (as in the title bar).
            const MARK: f32 = 72.0;
            const TILE: f32 = 824.0 / 1024.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(MARK, MARK), egui::Sense::hover());
            egui::Image::new(crate::kit::icons::APP)
                .paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(MARK / TILE)));
            ui.add_space(16.0);
            ui.heading(egui::RichText::new(t!("start.title")).size(28.0).strong());
            ui.add_space(20.0);
            let width = 260.0;
            for (id, label) in [("file.new", t!("start.new")), ("file.open", t!("start.open")), ("file.import", t!("start.import"))] {
                if ui.add_sized([width, 32.0], egui::Button::new(label)).clicked() {
                    actions.push(Action::Command(id));
                }
            }
            if cfg!(target_arch = "wasm32") {
                return;
            }
            ui.add_space(24.0);
            ui.strong(t!("start.recent"));
            ui.add_space(4.0);
            if self.prefs.recent_files.is_empty() {
                ui.weak(t!("start.no_recent"));
            }
            for (k, path) in self.prefs.recent_files.iter().enumerate().take(commands::RECENT_FILES) {
                let id = commands::COMMANDS.iter().find(|c| commands::recent_slot(c.id) == Some(k)).map(|c| c.id);
                let r = ui.add_sized([width, 24.0], egui::Button::new(file_name(path)).frame_when_inactive(false)).tip(path);
                if let (true, Some(id)) = (r.clicked(), id) {
                    actions.push(Action::Command(id));
                }
            }
        });
    }
}

/// A veil over the window while files are dragged over it, saying what letting go does.
pub(super) fn drop_hint(ctx: &egui::Context) {
    if ctx.input(|i| i.raw.hovered_files.is_empty()) {
        return;
    }
    let rect = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("file_drop")));
    painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(120));
    // On a plate of its own: what is under the veil would cross it.
    let text = painter.layout_no_wrap(t!("toast.drop_hint"), egui::FontId::proportional(20.0), egui::Color32::WHITE);
    let plate = egui::Rect::from_center_size(rect.center(), text.size() + egui::vec2(48.0, 28.0));
    painter.rect_filled(plate, 8.0, egui::Color32::from_black_alpha(220));
    painter.galley(rect.center() - text.size() / 2.0, text, egui::Color32::WHITE);
}

impl SkraftApp {
    /// The shown document: the view, the ribbon, the docked cards and the tool's options.
    pub(super) fn document_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        views: &[CommandView],
        actions: &mut Vec<Action>,
    ) {
        // The central area, between the title bar and the status bar, is the workspace's stage
        // (docs/ui/workspaces.md#stage): the 3D view over all of it, or a table on the left and
        // the view on the right. The ribbon, the tools' cards and the side panels float over
        // it, so opening or resizing them never resizes the view.
        let Some(i) = self.docs.active_index() else { return };
        let margin = crate::kit::style::FLOAT_GAP;
        let area = ui.available_rect_before_wrap();
        let stage = self.workspace().stage;
        let split = match stage {
            Stage::View | Stage::Sheet => None,
            Stage::Parts => Some(self.split(area, margin)),
        };
        // The 3D view's rect: what overlays on the view (quick card, drop-up, steps bar, view
        // bar) and what zoom extents frames are placed in.
        let view = split.map_or(area, |s| egui::Rect::from_min_max(egui::pos2(s.view, area.top()), area.max));
        self.stage_ui(ui, frame, i, area, view, margin);
        let (above, below_ribbon) = self.ribbon_ui(ctx, area, split, margin, views, actions);
        let columns_top = if above { below_ribbon } else { area.top() + margin };
        let free = self.side_panels(ctx, area, columns_top, margin, actions);
        // A split stage's table, from the left column to the view: the area's whole height, as
        // the columns, when the ribbon fits beside it, else under the ribbon.
        if let Some(s) = split {
            let table = egui::Rect::from_min_max(egui::pos2(s.table, columns_top), egui::pos2(s.view - margin, area.bottom() - margin));
            self.stage_table(ctx, table, area, margin, actions);
        }
        // Beside a split stage's table, what is left free is the view's.
        let (free_left, free_right) = (free.min.max(view.left()), free.max);
        // What follows belongs to the 3D view and its tools, which a sheet's stage does not show.
        if stage == Stage::Sheet {
            return;
        }
        let room = Room { view, margin, below_ribbon, free: (free_left, free_right) };
        let free_top = self.option_cards(ctx, room, actions);
        self.view_overlays(ctx, room, free_top, views, actions);
    }

    /// Over the view under the ribbon, centered together between the columns: the context card
    /// (what a pick takes, which copies a change reaches), then the active tool's card
    /// (docs/ui/interface.md#context-card), each translucent but under the pointer or typed
    /// in; no option, no card. The row's widths are known before drawing it (as last measured:
    /// a pass whose widths changed is discarded, so the cards are never shown misplaced while
    /// they settle). Gives where what is under them starts.
    fn option_cards(&mut self, ctx: &egui::Context, room: Room, actions: &mut Vec<Action>) -> f32 {
        let Room { view, margin, below_ribbon, free: (free_left, free_right) } = room;
        let options_id = egui::Id::unique("tool_options");
        let widths_id = options_id.with("widths");
        let mut free_top = below_ribbon;
        let Some(doc) = self.docs.active_mut() else { return free_top };
        let (has_context, has_tool) = (panels::has_context_options(&doc.editor), panels::has_tool_options(&doc.editor));
        if has_context || has_tool {
            let widths = ctx.data(|d| d.get_temp::<[f32; 2]>(widths_id)).unwrap_or_default();
            let context_width = if has_context { widths[0] } else { 0.0 };
            let tool_width = if has_tool { widths[1] } else { 0.0 };
            let gap = if has_context && has_tool { margin } else { 0.0 };
            let row_width = context_width + gap + tool_width;
            let options_left = (view.center().x - row_width / 2.0).min(free_right - row_width).max(free_left);
            let options = egui::Area::new(options_id)
                .fixed_pos(egui::pos2(options_left, below_ribbon))
                .constrain_to(view)
                .show(ctx, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = margin;
                        let mut drawn = widths;
                        if has_context {
                            let shown =
                                crate::kit::style::hover_card(ui, option_card(ui.style()), options_id.with("context_hovered"), |ui| {
                                    panels::context_options(ui, &doc.editor, actions)
                                });
                            drawn[0] = shown.response.rect.width();
                        }
                        if has_tool {
                            let shown = crate::kit::style::hover_card(ui, option_card(ui.style()), options_id.with("tool_hovered"), |ui| {
                                panels::tool_options(ui, &doc.editor, actions)
                            });
                            drawn[1] = shown.response.rect.width();
                        }
                        if drawn != widths {
                            ui.data_mut(|d| d.insert_temp(widths_id, drawn));
                            // Past egui's passes for this frame, the discard is refused: the next frame then.
                            ui.ctx().request_discard("tool option cards resized");
                            ui.ctx().request_repaint();
                        }
                    });
                })
                .response
                .rect;
            free_top = options.bottom() + margin;
        }
        free_top
    }

    /// What floats over the 3D view: the view bar, the tree drop-up, the steps bar, the quick
    /// tools' card; then what zoom extents frames, from `free_top` down.
    fn view_overlays(&mut self, ctx: &egui::Context, room: Room, free_top: f32, views: &[CommandView], actions: &mut Vec<Action>) {
        let Room { view, margin, below_ribbon, free: (free_left, free_right) } = room;
        let Some(doc) = self.docs.active_mut() else { return };
        // The view bar (docs/ui/view-bar.md): over the left or right edge, beside that side's
        // column, centered on the view's height but never over the ribbon; what zoom extents
        // frames stops at it.
        let bar_width = crate::window::view_bar::width(&ctx.global_style());
        let left_x = if free_left > view.left() { free_left } else { view.left() + margin };
        let right_x = if free_right < view.right() { free_right } else { view.right() - margin } - bar_width;
        let side = self.prefs.view_bar_side;
        let heights = egui::Rangef::new(below_ribbon, view.bottom() - margin);
        let place = crate::window::view_bar::Place { side, xs: [left_x, right_x], heights, center: view.center().y };
        crate::window::view_bar::bar(ctx, view, &place, &self.prefs.view_bar, views, actions);
        let (free_left, free_right) = match side {
            crate::window::dock::Dock::Left => (left_x + bar_width + margin, free_right),
            crate::window::dock::Dock::Right => (free_left, right_x - margin),
        };
        // OCL's tree drop-up, at the cursor where it was opened (docs/tools/selection.md).
        if let Some(tree) = doc.editor.tree() {
            let at = view.min + egui::vec2(tree.at.x as f32, tree.at.y as f32);
            crate::window::drop_up::show(ctx, view, at, tree, actions);
        }
        // The steps bar at the bottom centre of the view, the validation at the top under the
        // tool's options (docs/ui/interface.md#steps-bar).
        let steps = doc.editor.tool_steps();
        self.steps_top = crate::window::steps::bar(ctx, view.center().x, (free_left, free_right), view.bottom(), steps.as_ref());
        if doc.editor.tool_awaits_validation() {
            crate::window::steps::validation(ctx, view.center().x, free_top, actions);
        }
        // The quick tools' card, at the centre of what was just made (docs/tools/quick-tools.md).
        if let Some(at) = doc.editor.quick_card() {
            let at = view.min + egui::vec2(at.x as f32, at.y as f32);
            crate::window::quick::card(ctx, view, at, &self.prefs.quick_tools, views, actions);
        }
        // What zoom extents frames: below the ribbon (and the options), beside the side panels,
        // in viewport pixels.
        let free_min = egui::vec2(free_left - view.left(), free_top - view.top());
        let free_max = egui::vec2(free_right - view.left(), view.height());
        doc.editor
            .set_free_area(glam::DVec2::new(free_min.x as f64, free_min.y as f64), glam::DVec2::new(free_max.x as f64, free_max.y as f64));
    }

    /// The stage: the shown document's 3D view or sheet in `view`, the view's ground beside it
    /// over the rest of `area`.
    fn stage_ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, i: usize, area: egui::Rect, view: egui::Rect, margin: f32) {
        let theme = self.theme;
        let stage = self.workspace().stage;
        // A sheet is framed in what the ribbon (as last drawn) and the side columns leave.
        let sheet_room = self.ribbon_last_bottom.map(|b| {
            let (_, free, _) = self.columns(view, margin);
            egui::Rect::from_x_y_ranges(free, (b + margin).clamp(area.top(), area.bottom())..=area.bottom())
        });
        let (viewport, docs, prefs) = (&mut self.viewport, &mut self.docs, &self.prefs);
        egui::CentralPanel::no_frame().show(ui, |ui| {
            // Beside the view, the stage's ground is the view's (the table's card over it).
            if view != area {
                ui.painter().rect_filled(area, 0.0, viewport::color(theme.ui.view));
            }
            if let Some(d) = docs.iter_mut().nth(i) {
                ui.scope_builder(egui::UiBuilder::new().max_rect(view), |ui| {
                    if stage == Stage::Sheet {
                        // A sheet of the drawing instead of the 3D view (docs/ui/drawing.md).
                        let ready = viewport.sheet_textures(d.id);
                        let title = d.name();
                        let wanted = crate::drawing::sheet::ui(
                            ui,
                            view,
                            sheet_room,
                            &mut d.editor,
                            &mut d.sheet,
                            &mut d.lines,
                            &theme,
                            &ready,
                            &title,
                        );
                        viewport.sheet_pictures(ui.ctx(), frame, d.id, &d.editor, &theme, &wanted);
                    } else {
                        viewport.ui(ui, frame, d.id, &mut d.editor, &d.vcb, &prefs.navigation, &theme, prefs.show_render_stats);
                    }
                });
            }
        });
    }

    /// The ribbon at the top (docs/ui/ribbon.md#layout): between the docked cards' columns,
    /// which then take the view's whole height, when it fits there whole; else above them,
    /// over the view's width, the columns under it. Gives whether it is above them, and where
    /// what is under it starts.
    fn ribbon_ui(
        &mut self,
        ctx: &egui::Context,
        area: egui::Rect,
        split: Option<Split>,
        margin: f32,
        views: &[CommandView],
        actions: &mut Vec<Action>,
    ) -> (bool, f32) {
        let card = |style: &egui::Style| card_frame(style);
        let ribbon_view = ribbon::View { workspace: self.workspace(), prefs: &self.prefs.ribbon, views };
        let frame_x = card(&ctx.global_style()).total_margin().sum().x;
        let natural = ribbon::natural_width(ctx, &ribbon_view);
        let (_, free, _) = self.columns(area, margin);
        let inside = |x: f32| x > area.left() && x < area.right();
        // A split stage's table counts as a column: the ribbon between it and the right one.
        let between = egui::Rangef::new(
            match split {
                Some(s) => s.view,
                None if inside(free.min) => free.min,
                None => area.left() + margin,
            },
            if inside(free.max) { free.max } else { area.right() - margin },
        );
        let above = natural + frame_x > between.span();
        let span = if above { egui::Rangef::new(area.left() + margin, area.right() - margin) } else { between };
        // As wide as its content, centered within its span (on the area, or between a split
        // stage's table and the right column): a window narrower than that narrows it
        // ([`ribbon::arrange`] then gives way).
        let center = if split.is_some() && !above { span.center() } else { area.center().x };
        let ribbon_width = natural.min((span.span() - frame_x).max(0.0));
        let ribbon_left = (center - (ribbon_width + frame_x) / 2.0).min(span.max - ribbon_width - frame_x).max(span.min);
        let ribbon_area = egui::Area::new(egui::Id::unique("ribbon"))
            .fixed_pos(egui::pos2(ribbon_left, area.top() + margin))
            .constrain_to(area)
            .sense(egui::Sense::click())
            .show(ctx, |ui| {
                let card = card(ui.style());
                let inset = card.inner_margin;
                card.show(ui, |ui| ribbon::ribbon(ui, &ribbon_view, ribbon_width, inset, actions))
            });
        egui::Popup::context_menu(&ribbon_area.response)
            .style(crate::kit::style::menu)
            .show(|ui| ribbon::ribbon_menu(ui, &self.prefs.ribbon, actions));
        let ribbon_rect = ribbon_area.response.rect;
        self.ribbon_bottom = Some(ribbon_rect.bottom());
        self.ribbon_last_bottom = self.ribbon_bottom;
        (above, ribbon_rect.bottom() + margin)
    }
}

/// Where the 3D view lies among what floats over it.
#[derive(Clone, Copy)]
struct Room {
    view: egui::Rect,
    /// Between the floating cards, and around them.
    margin: f32,
    /// Where what is under the ribbon starts.
    below_ribbon: f32,
    /// The xs the side columns leave free over the view.
    free: (f32, f32),
}

/// The edges of a side panel that can be dragged.
#[derive(Clone, Copy)]
pub(super) struct Edges {
    /// Its column (`None`: dragged, out of any column).
    pub(super) side: Option<Dock>,
    /// Its bottom edge sets its height.
    pub(super) bottom: bool,
    max_width: f32,
    min_height: f32,
}

/// Room under a side panel's title before its bar's bottom line, and as much again under the
/// line before the content.
const HEAD_PAD: f32 = 4.0;

/// A card's title bar (docs/ui/interface.md#layout): a band of its own colour across the top of
/// the card `rect`, around `head` (the title's row), `line` under it parting it from the content
/// (the whole card once `folded`), the title semibold at its left. Gives the band.
pub(super) fn title_bar(ui: &egui::Ui, rect: egui::Rect, head: egui::Rect, title: &str, folded: bool, line: bool) -> egui::Rect {
    let painter = ui.painter();
    let stroke = ui.visuals().window_stroke;
    let card = rect.shrink(stroke.width);
    let bottom = if folded { card.bottom() } else { head.bottom() + HEAD_PAD };
    let band = egui::Rect::from_x_y_ranges(card.x_range(), card.top()..=bottom);
    let r = ui.visuals().window_corner_radius;
    let corners = if folded { r } else { egui::CornerRadius { sw: 0, se: 0, ..r } };
    painter.rect_filled(band, corners, ui.visuals().faint_bg_color);
    if !folded && line {
        painter.hline(band.x_range(), band.bottom(), stroke);
    }
    let font = crate::kit::style::semibold(egui::TextStyle::Body.resolve(ui.style()).size + 1.0);
    painter.text(egui::pos2(head.left() + 4.0, head.center().y), egui::Align2::LEFT_CENTER, title, font, ui.visuals().strong_text_color());
    band
}

/// What a side panel's header did.
pub(super) enum CardEvent {
    /// Its drag started.
    Grabbed(PanelDrag),
    /// Released.
    Dropped,
}

pub(super) fn panel_title(id: PanelId) -> String {
    match id {
        PanelId::EntityInfo => t!("panel.entity_info.title"),
        PanelId::Materials => t!("panel.materials.title"),
        PanelId::Outliner => t!("panel.outliner.title"),
        PanelId::Components => t!("panel.components.title"),
        PanelId::Compositions => t!("panel.compositions.title"),
    }
}

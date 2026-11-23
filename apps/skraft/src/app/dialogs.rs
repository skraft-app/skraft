//! The modal dialogs and the command palette.

use super::*;

impl SkraftApp {
    /// The dialog choosing what a view shows (decision 0051), on the shown document.
    pub(super) fn view_picker_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = self.docs.active_mut() else { return };
        let Some(mut picker) = d.sheet.picker.take() else { return };
        let doc = d.id;
        match picker.show(ctx, &d.editor, &mut self.viewport.thumbnails, doc) {
            crate::view_picker::Answer::Open => d.sheet.picker = Some(picker),
            crate::view_picker::Answer::Cancel => {}
            crate::view_picker::Answer::Done(subject) => {
                if let Err(e) = crate::sheet::picked(&mut d.editor, &mut d.sheet, picker.purpose, subject) {
                    self.toasts.push(Note::error(t!("notice.op_failed", op = t!("operation.set_element")), e));
                }
            }
        }
    }

    /// The dialog naming what was just made (docs/tools/quick-tools.md): opened by an editor (its tool's
    /// « Nommage des pièces ») or by a toast's « Renommer »; gone with its document or what it names.
    pub(super) fn naming_dialog(&mut self, ctx: &egui::Context) {
        for d in self.docs.iter_mut() {
            if let Some(n) = d.editor.take_naming()
                && self.naming.is_none()
            {
                self.naming = Some(crate::naming::Dialog::new(d.id, n));
            }
        }
        let Some(dialog) = &mut self.naming else { return };
        let Some(d) = self.docs.by_id_mut(dialog.doc).filter(|d| d.editor.naming_of(dialog.naming.target).is_some()) else {
            self.naming = None;
            return;
        };
        let editor = &mut d.editor;
        let naming = dialog.naming.clone();
        match dialog.show(ctx, |name| editor.name_refusal(&naming, name)) {
            crate::naming::Answer::Open => {}
            crate::naming::Answer::Cancel => self.naming = None,
            crate::naming::Answer::Name(name) => {
                if let Err(e) = editor.give_name(&naming, &name) {
                    self.toasts.push(Note::error(t!("notice.op_failed", op = t!("operation.rename")), e));
                }
                self.naming = None;
            }
        }
    }

    /// « Enregistrer les modifications de « X » ? », while a modified document is closing.
    pub(super) fn save_prompt(&mut self, ctx: &egui::Context) {
        let Some(id) = self.closing.as_ref().and_then(|c| c.asking) else { return };
        let Some(name) = self.docs.index_of(id).and_then(|i| self.docs.get(i)).map(|d| d.name()) else {
            self.closing = None;
            return;
        };
        let mut chosen = None;
        let modal = egui::Modal::new(egui::Id::unique("save_prompt")).frame(modal_frame(ctx)).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.spacing_mut().button_padding = MODAL_BUTTON_PADDING;
            ui.heading(t!("save_prompt.title"));
            ui.add_space(6.0);
            ui.label(t!("save_prompt.question", name = name));
            ui.weak(t!("save_prompt.note"));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(t!("save_prompt.dont_save")).clicked() {
                    chosen = Some(Answer::DontSave);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = ui.add(egui::Button::new(t!("save_prompt.save")).fill(ui.visuals().selection.bg_fill));
                    if save.clicked() {
                        chosen = Some(Answer::Save);
                    }
                    if ui.button(t!("save_prompt.cancel")).clicked() {
                        chosen = Some(Answer::Cancel);
                    }
                });
            });
        });
        if ui_key(ctx, egui::Key::Enter) {
            chosen = chosen.or(Some(Answer::Save));
        }
        if modal.should_close() {
            chosen = chosen.or(Some(Answer::Cancel));
        }
        if let Some(a) = chosen {
            self.answer(a);
        }
    }

    /// The window over a document being loaded (docs/ui/loading.md): the file
    /// read, its preview meshes, its final meshes, each with how far it is, until the last final
    /// mesh; « Annuler » (or Esc) stops it all. Shown once a load has lasted [`LOAD_WINDOW_AFTER`]
    /// (a small file would only flash it).
    pub(super) fn loading_window(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let loading: Vec<(DocId, LoadKind)> = self.docs.iter().filter_map(|d| d.editor.loading().map(|l| (d.id, l.kind.clone()))).collect();
        // The shown document's textures, prepared whatever draws it: a load waits for them too.
        let shown = self.docs.active().map(|d| d.id);
        let textures = match self.docs.active() {
            Some(d) => self.viewport.textures_progress(ctx, d.editor.document()),
            None => (0, 0),
        };
        let waiting = textures.0 < textures.1;
        self.loads_seen.retain(|id, _| loading.iter().any(|(l, _)| l == id) || (waiting && shown == Some(*id)));
        for (id, kind) in loading {
            self.loads_seen.entry(id).or_insert((now, kind));
        }
        let Some(d) = self.docs.active() else { return };
        let Some((since, kind)) = self.loads_seen.get(&d.id).cloned() else { return };
        // None once the meshes are all there, the textures still coming.
        let load = d.editor.loading().cloned();
        if now - since < LOAD_WINDOW_AFTER {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(LOAD_WINDOW_AFTER - (now - since)));
            return;
        }
        let title = match &kind {
            LoadKind::Open => t!("loading.open", name = d.name()),
            LoadKind::Import { name } => t!("loading.import", name = name),
        };
        let row = |ui: &mut egui::Ui, label: String, done: usize, total: usize, current: bool| {
            ui.label(label);
            let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
            let bar = egui::ProgressBar::new(fraction).text(format!("{done} / {total}")).animate(current && done < total);
            ui.add(bar);
            ui.end_row();
        };
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("loading")).frame(modal_frame(ctx)).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.spacing_mut().button_padding = MODAL_BUTTON_PADDING;
            ui.heading(title);
            ui.add_space(8.0);
            egui::Grid::new("loading_stages").num_columns(2).spacing([12.0, 8.0]).min_col_width(90.0).show(ui, |ui| {
                if let Some(load) = &load {
                    match load.reading {
                        Some(Progress::Bodies { done, total }) => row(ui, t!("loading.bodies"), done as usize, total as usize, true),
                        Some(Progress::Reading) => {
                            ui.label(t!("loading.reading"));
                            ui.add(egui::Spinner::new());
                            ui.end_row();
                        }
                        None => {}
                    }
                    let (pd, pt) = load.previews;
                    let (fd, ft) = load.finals;
                    if load.reading.is_none() || pt > 0 {
                        row(ui, t!("loading.previews"), pd, pt, pd < pt);
                        row(ui, t!("loading.finals"), fd, ft, pd >= pt);
                    }
                }
                if textures.1 > 0 {
                    row(ui, t!("loading.textures"), textures.0, textures.1, waiting);
                }
            });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Only textures left: the model is there, usable without them.
                let label = if load.is_some() { t!("loading.cancel") } else { t!("loading.skip") };
                if ui.button(label).clicked() {
                    cancel = true;
                }
            });
        });
        if modal.should_close() || ui_key(ctx, egui::Key::Escape) {
            cancel = true;
        }
        if cancel && let Some(d) = self.docs.active_mut() {
            d.editor.cancel_loading();
            let id = d.id;
            self.loads_seen.remove(&id);
        }
    }

    /// The command palette (Cmd/Ctrl+K): type to filter, ↑↓ to pick, Entrée to run.
    pub(super) fn palette_window(&mut self, ctx: &egui::Context, views: &[CommandView], actions: &mut Vec<Action>) {
        let Some(p) = &mut self.palette else { return };
        let found: Vec<&CommandView> = views
            .iter()
            .filter(|v| v.enabled && (v.menu != Menu::Debug || self.debug_menu))
            .filter(|v| commands::find(v.id).is_some_and(|c| commands::matches(&p.query, c)))
            .collect();
        // ↑↓ taken before the query field sees them: they pick a row, never move its cursor.
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if down {
            p.picked = (p.picked + 1).min(found.len().saturating_sub(1));
        }
        if up {
            p.picked = p.picked.saturating_sub(1);
        }
        p.picked = p.picked.min(found.len().saturating_sub(1));
        let mut run = None;
        let mut open = !escape;
        let top = self.ribbon_bottom.map_or(80.0, |b| (b - ctx.content_rect().top() + 8.0).max(80.0));
        // A fixed id: the title changes with the language.
        egui::Window::new(t!("palette.title"))
            .id(egui::Id::unique("command_palette"))
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .fixed_size([600.0, 0.0])
            // Below the ribbon when a document is shown, never over it.
            .anchor(egui::Align2::CENTER_TOP, [0.0, top])
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut p.query)
                        .hint_text(ellipsis(t!("palette.hint")))
                        .margin(egui::vec2(8.0, 6.0))
                        .desired_width(f32::INFINITY),
                );
                // Only when lost: asked every frame, it would reset the field's key filter (the
                // arrows would then move the focus to a row for a frame) and break dead keys.
                if !r.has_focus() {
                    r.request_focus();
                }
                if r.changed() {
                    p.picked = 0;
                    p.at_top = false;
                }
                ui.separator();
                let mut list = egui::ScrollArea::vertical().max_height(480.0).min_scrolled_height(480.0);
                if !std::mem::replace(&mut p.at_top, true) {
                    list = list.vertical_scroll_offset(0.0);
                }
                list.show(ui, |ui| {
                    // In menu order, each menu's commands under its name.
                    let mut header = None;
                    for (k, v) in found.iter().enumerate() {
                        if k == 0 || found[k - 1].menu != v.menu {
                            if k > 0 {
                                ui.add_space(6.0);
                            }
                            header = Some(palette_header(ui, v.menu));
                        }
                        let row = palette_row(ui, v, k == p.picked).tip(v.id);
                        if k == p.picked && (up || down) {
                            // A section's first row brings its name along.
                            let first = k == 0 || found[k - 1].menu != v.menu;
                            let rect = match header {
                                Some(h) if first => h.union(row.rect),
                                _ => row.rect,
                            };
                            ui.scroll_to_rect(rect, None);
                        }
                        if row.clicked() {
                            run = Some(v.id);
                        }
                    }
                    if found.is_empty() {
                        ui.weak(t!("palette.none"));
                    }
                });
            });
        if enter {
            run = run.or_else(|| found.get(p.picked).map(|v| v.id));
        }
        if let Some(id) = run {
            open = false;
            actions.push(Action::Command(id));
        }
        if !open {
            self.palette = None;
        }
    }
}

/// The frame of the app's modal boxes: egui's popup frame, with room around the content.
pub(crate) fn modal_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(MODAL_MARGIN))
}

/// The room between a modal box's border and its content (points).
const MODAL_MARGIN: i8 = 20;

/// The padding of a modal box's buttons (points): roomier than the app's compact buttons.
pub(crate) const MODAL_BUTTON_PADDING: egui::Vec2 = egui::vec2(14.0, 5.0);

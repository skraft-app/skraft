//! Opening, closing and saving documents, and exporting them.

use super::*;

impl SkraftApp {
    /// Opens again the documents open when the app last quit (desktop, if the preference
    /// says so); none left: the start page.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn reopen_documents(&mut self) {
        if !self.prefs.reopen_documents {
            return;
        }
        let (files, shown) = (self.prefs.open_files.clone(), self.prefs.shown_file);
        let mut tabs = Vec::new();
        for path in &files {
            let read = std::fs::read(path).map_err(|e| e.to_string());
            match read.and_then(|b| Document::from_file(&b).map(|d| (d, Document::file_view(&b))).map_err(|e| e.to_string())) {
                Ok((doc, view)) => {
                    self.open_document(doc, Some(path.clone()), &view);
                    if let Ok(bytes) = std::fs::read(path) {
                        self.keep_file_picture(&bytes);
                    }
                    tabs.push(self.docs.active_index());
                }
                Err(e) => {
                    tabs.push(None);
                    self.toasts.push(Note::error(t!("toast.open_failed", file = file_name(path)), e));
                }
            }
        }
        // None shown: the app quit on the start page.
        match shown {
            Some(k) => {
                if let Some(i) = tabs.get(k).copied().flatten() {
                    self.docs.select(i);
                }
            }
            None => self.docs.home(),
        }
    }

    /// The page asks before it is left while a document is modified (web).
    #[cfg(target_arch = "wasm32")]
    pub(super) fn ask_before_leaving(&self) {
        use wasm_bindgen::JsCast;
        let unsaved = self.unsaved.clone();
        let handler = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(move |e: web_sys::BeforeUnloadEvent| {
            if unsaved.get() {
                e.prevent_default();
                e.set_return_value("");
            }
        });
        if let Some(w) = web_sys::window() {
            w.set_onbeforeunload(Some(handler.as_ref().unchecked_ref()));
        }
        // For as long as the page lives.
        handler.forget();
    }

    /// Opens `doc` (read from `path`, seen as `view` says) and shows it: its tab if that file
    /// is open already, the blank document's place, or a new tab.
    pub(super) fn open_document(&mut self, doc: Document, path: Option<String>, view: &FileView) {
        if let Some(i) = path.as_deref().and_then(|p| self.docs.find_path(p)) {
            self.docs.select(i);
            return;
        }
        match self.docs.open(doc, path, Self::setup(&self.prefs)) {
            Ok((_, replaced)) => {
                if let Some(replaced) = replaced {
                    self.viewport.drop_document(replaced);
                }
                if let Some(d) = self.docs.active_mut() {
                    d.workspace = workspaces::find(view.workspace.as_deref());
                    if let Some(camera) = view.camera.as_ref().and_then(skraft_tools::Camera::from_seen) {
                        d.editor.camera = camera;
                    }
                    if let Some(board) = &view.board {
                        d.sheet.restore(d.editor.document().drawing(), board);
                    }
                }
            }
            Err(_) => self.toasts.push(Note::info(t!("toast.too_many_documents", max = crate::docs::MAX_OPEN))),
        }
    }

    /// The shown document's workspace; the first one on the start page.
    pub(super) fn workspace(&self) -> &'static Workspace {
        self.docs.active().map_or_else(|| workspaces::find(None), |d| d.workspace)
    }

    /// Shows the shown document in workspace `id` (decision 0047): the running tool cancelled,
    /// the selection and the level open kept. Not a change of the document: nothing modified,
    /// nothing to undo; its file keeps it at the next save.
    pub(super) fn set_workspace(&mut self, id: &str) -> Result<(), String> {
        let w = workspaces::listed().find(|w| w.id == id).ok_or_else(|| format!("no workspace {id}"))?;
        let Some(d) = self.docs.active_mut() else { return Err("no document".into()) };
        if d.workspace.id != w.id {
            d.vcb.clear();
            d.editor.set_tool(ToolKind::Select);
            d.workspace = w;
        }
        Ok(())
    }

    /// Closes tab `i` now, without asking.
    pub(super) fn close_now(&mut self, i: usize) {
        if let Some(doc) = self.docs.close(i) {
            self.viewport.drop_document(doc.id);
        }
    }

    /// Closes the documents `ids`, asking about the modified ones (docs/ui/file-tabs.md).
    /// Nothing while tabs are being closed already.
    pub(super) fn close_documents(&mut self, ids: Vec<DocId>) {
        if self.closing.is_none() {
            self.closing = Some(Closing { queue: ids, quit: false, asking: None, saving: None });
            self.advance_closing();
        }
    }

    /// Quits: asks about each modified document first (none closes meanwhile: Cancel leaves
    /// them as they were). The documents open are kept to be opened again at launch.
    pub(super) fn request_quit(&mut self) {
        if self.closing.as_ref().is_some_and(|c| c.quit) {
            return;
        }
        let queue = self.docs.iter().filter(|d| d.modified()).map(|d| d.id).collect();
        self.closing = Some(Closing { queue, quit: true, asking: None, saving: None });
        self.advance_closing();
    }

    /// Goes on with the documents being closed: an unmodified one closes, a modified one is
    /// shown and asked about; when none is left, the app quits if it was quitting.
    fn advance_closing(&mut self) {
        loop {
            let Some(c) = &mut self.closing else { return };
            if c.asking.is_some() || c.saving.is_some() {
                return;
            }
            let Some(&id) = c.queue.first() else {
                let quit = c.quit;
                self.closing = None;
                if quit {
                    self.quit_ok = true;
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                return;
            };
            let quit = c.quit;
            match self.docs.index_of(id) {
                Some(i) if self.docs.get(i).is_some_and(|d| d.modified()) => {
                    c.asking = Some(id);
                    self.docs.select(i);
                    return;
                }
                found => {
                    c.queue.remove(0);
                    if let (Some(i), false) = (found, quit) {
                        self.close_now(i);
                    }
                }
            }
        }
    }

    /// The answer to the question about the document being closed.
    pub(super) fn answer(&mut self, answer: Answer) {
        let Some(c) = &mut self.closing else { return };
        let Some(id) = c.asking.take() else { return };
        match answer {
            Answer::Cancel => self.closing = None,
            Answer::Save => {
                c.saving = Some(id);
                let ask = self.docs.index_of(id).and_then(|i| self.docs.get(i)).is_some_and(|d| d.path.is_none());
                self.save_document(id, ask);
            }
            Answer::DontSave => {
                c.queue.retain(|q| *q != id);
                if !c.quit
                    && let Some(i) = self.docs.index_of(id)
                {
                    self.close_now(i);
                }
            }
        }
        self.advance_closing();
    }

    /// Document `doc` was saved (`ok`) or not: one being closed then closes, if it is not
    /// modified again; else the closing stops.
    pub(super) fn closing_saved(&mut self, doc: DocId, ok: bool) {
        let Some(c) = &mut self.closing else { return };
        if c.saving != Some(doc) {
            return;
        }
        c.saving = None;
        if !ok {
            self.closing = None;
            return;
        }
        // Modified since the save started: asked about again.
        if self.docs.index_of(doc).and_then(|i| self.docs.get(i)).is_some_and(|d| d.modified()) {
            self.advance_closing();
            return;
        }
        c.queue.retain(|q| *q != doc);
        if !c.quit
            && let Some(i) = self.docs.index_of(doc)
        {
            self.close_now(i);
        }
        self.advance_closing();
    }

    /// The tabs' actions (docs/ui/file-tabs.md).
    pub(super) fn tab_action(&mut self, a: TabAction) {
        let id = |docs: &Docs, i: usize| docs.get(i).map(|d| d.id);
        match a {
            TabAction::Select(i) => self.docs.select(i),
            TabAction::New => self.new_document(),
            TabAction::Reorder { from, to } => self.docs.move_tab(from, to),
            TabAction::Close(i) | TabAction::Menu(i, MenuRow::Close) => {
                if let Some(d) = id(&self.docs, i) {
                    self.close_documents(vec![d]);
                }
            }
            TabAction::Menu(i, MenuRow::CloseOthers) => {
                let keep = id(&self.docs, i);
                let others = self.docs.iter().map(|d| d.id).filter(|d| Some(*d) != keep).collect();
                self.close_documents(others);
            }
            TabAction::Menu(_, MenuRow::CloseAll) => {
                let all = self.docs.iter().map(|d| d.id).collect();
                self.close_documents(all);
            }
            TabAction::Menu(i, MenuRow::Reveal) => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(p) = self.docs.get(i).and_then(|d| d.path.clone()) {
                    reveal(&p);
                }
                #[cfg(target_arch = "wasm32")]
                let _ = i;
            }
        }
    }

    /// The tabs as the title bar draws them.
    pub(super) fn tabs(&self) -> Vec<Tab> {
        self.docs.iter().map(|d| Tab { id: d.id, name: d.name(), path: d.path.clone(), modified: d.modified() }).collect()
    }

    /// The PDF asked for, written once its document's textures are ready, or they would print
    /// as their materials' colours: meanwhile a window shows how far they are, « Annuler » (or
    /// Esc) giving up, as does leaving the document.
    pub(super) fn export_pdf_when_ready(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some((doc, since)) = self.pdf_asked else { return };
        let Some(d) = self.docs.active().filter(|d| d.id == doc) else {
            self.pdf_asked = None;
            return;
        };
        let (ready, total) = self.viewport.textures_progress(ctx, d.editor.document());
        if ready >= total {
            self.pdf_asked = None;
            self.export_pdf(frame);
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        // The loading window, while there, already shows them.
        if self.loads_seen.contains_key(&doc) || ctx.input(|i| i.time) - since < LOAD_WINDOW_AFTER {
            return;
        }
        let mut cancel = false;
        let modal = modal(ctx, egui::Id::unique("pdf_waiting"), 380.0, t!("loading.pdf"), |ui| {
            ui.horizontal(|ui| {
                ui.label(t!("loading.textures"));
                let fraction = ready as f32 / total as f32;
                ui.add(egui::ProgressBar::new(fraction).text(format!("{ready} / {total}")).animate(true));
            });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = ui.button(t!("loading.cancel")).clicked();
            });
        });
        if cancel || modal.should_close() || ui_key(ctx, egui::Key::Escape) {
            self.pdf_asked = None;
        }
    }

    /// The presentation shown of the shown document written as a PDF (docs/ui/drawing.md#pdf),
    /// then saved where the user says (a download on the web): named after the document, and
    /// after the presentation when it has several.
    pub(super) fn export_pdf(&mut self, frame: &eframe::Frame) {
        let theme = self.theme;
        let (viewport, docs) = (&mut self.viewport, &self.docs);
        let Some(d) = docs.active() else { return };
        let title = d.name();
        // The pictures' lines and edges about as wide as on screen at their resolution.
        let ppp = (crate::workspaces::drawing::pdf::DPI / 150.0) as f32;
        let drawing = d.editor.document().drawing();
        let Some(presentation) = d.sheet.presentation(drawing) else { return };
        let bytes = crate::workspaces::drawing::pdf::drawing_pdf(&d.editor, &title, presentation.id, |w| {
            viewport.picture_rgba(frame, d.id, &d.editor, &theme, w, ppp)
        });
        match bytes {
            Ok(bytes) => {
                let stem = std::path::Path::new(&title).file_stem().map_or_else(|| title.clone(), |s| s.to_string_lossy().into_owned());
                let name = match drawing.presentations.len() {
                    1 => format!("{stem}.pdf"),
                    _ => format!("{stem} - {}.pdf", presentation.name.replace(['/', '\\', ':'], "-")),
                };
                file::export_file(bytes, &name, "PDF", "pdf", &self.inbox, &self.ctx);
                // What would go out as `{client}` is said (docs/ui/drawing.md#pdf).
                let missing = crate::workspaces::drawing::pdf::missing_fields(&d.editor, presentation.id);
                if !missing.is_empty() {
                    let fields = missing.iter().map(|f| format!("{{{f}}}")).collect::<Vec<_>>().join(", ");
                    self.toasts.push(Note::new(crate::window::toasts::Level::Warning, t!("toast.pdf_missing_fields"), Some(fields)));
                }
            }
            Err(e) => self.toasts.push(Note::error(t!("toast.pdf_failed"), e)),
        }
    }

    /// A new document, in a tab of its own (or the blank one's place).
    pub(super) fn new_document(&mut self) {
        self.open_document(Document::with_settings(self.prefs.new_model.clone()), None, &FileView::default());
    }

    /// Opens the recent file `k`; one that cannot be read leaves the list.
    pub(super) fn open_recent(&mut self, k: usize) -> Result<(), Note> {
        let Some(path) = self.prefs.recent_files.get(k).cloned() else {
            return Err(Note::info(t!("toast.command_unavailable", id = format!("file.recent.{}", k + 1))));
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(e) = file::open_path(&path, &self.inbox) {
            self.prefs.recent_files.retain(|p| *p != path);
            return Err(Note::error(t!("toast.open_failed", file = file_name(&path)), e));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = path;
        Ok(())
    }

    /// Remembers `path` in the recent files (desktop: the browser gives only a name).
    pub(super) fn add_recent_file(&mut self, path: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        self.prefs.add_recent_file(path);
        #[cfg(target_arch = "wasm32")]
        let _ = path;
    }

    /// Saves the shown document (`ask`: where, whatever its file).
    pub(super) fn save(&mut self, ask: bool) {
        if let Some(id) = self.docs.active().map(|d| d.id) {
            self.save_document(id, ask);
        }
    }

    /// Saves document `id`: to its file, else (or if `ask`) where the user says.
    fn save_document(&mut self, id: DocId, ask: bool) {
        self.draw_file_picture(id);
        let Some(d) = self.docs.iter().find(|d| d.id == id) else { return };
        let path = if ask { None } else { d.path.clone() };
        // The name offered: its file's, else the default one.
        let name = d.path.as_deref().map(file_name).map(|n| n.trim_end_matches(&format!(".{}", file::EXTENSION)).to_owned());
        let name = name.unwrap_or_else(|| t!("default_name.file"));
        let view = FileView { workspace: Some(d.workspace.id.to_owned()), camera: Some(d.editor.camera.seen()), board: d.sheet.seen() };
        let saved = file::Saved { document: d.editor.document(), view: &view, thumbnail: d.thumbnail.as_deref() };
        file::save(id, saved, d.editor.history().revision(), path.as_deref(), &name, &self.inbox, &self.ctx);
    }

    /// Draws document `id`'s picture of its model for its file (docs/io/file-format.md), in the
    /// light look whatever the theme: a file reads the same on every machine. Kept from its
    /// file when it is not shown or has nothing drawn yet (desktop: the web cannot wait for
    /// the GPU).
    fn draw_file_picture(&mut self, id: DocId) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(state) = self.render_state.as_ref() else { return };
            let Some(d) = self.docs.iter_mut().find(|d| d.id == id) else { return };
            if let Some(png) = self.viewport.file_picture(state, id, &d.editor, &Theme::light()) {
                d.thumbnail = Some(png.into());
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = id;
    }

    /// The picture of its model `bytes` (a file just opened) carries, kept by the document shown.
    pub(super) fn keep_file_picture(&mut self, bytes: &[u8]) {
        if let Some(d) = self.docs.active_mut() {
            d.thumbnail = Document::file_thumbnail(std::io::Cursor::new(bytes)).map(Into::into);
        }
    }
}

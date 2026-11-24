//! What each frame does around the documents' own drawing: what came in (the kernel, files,
//! dialogs), the window's title and title bar, the windows over it, the toasts, the preferences.

use super::*;

impl SkraftApp {
    pub(super) fn poll_documents(&mut self) {
        // What the kernel finished since the last frame (it wakes the UI when it does), for
        // every document: one in the background goes on computing.
        for d in self.docs.iter_mut() {
            d.editor.poll();
        }
        // Their undo histories kept within the memory allowed, the meshes just come weighed.
        self.docs.cap_histories(u64::from(self.prefs.history_memory) << 20);
    }

    pub(super) fn end_cancelled_loads(&mut self) {
        // A file whose opening was cancelled is closed (an import cancelled is undone already).
        let cancelled: Vec<DocId> = self
            .docs
            .iter_mut()
            .filter_map(|d| matches!(d.editor.take_load_end(), Some(LoadEnd::Cancelled(LoadKind::Open))).then_some(d.id))
            .collect();
        for id in cancelled {
            self.loads_seen.remove(&id);
            if let Some(i) = self.docs.index_of(id) {
                self.close_now(i);
            }
        }
    }

    pub(super) fn take_dropped_files(&mut self, ctx: &egui::Context) {
        // The files the system asked to open (a double click, the command line): read now, opened
        // just below with the dialogs'.
        #[cfg(not(target_arch = "wasm32"))]
        for path in crate::opened::take() {
            self.user_acted();
            if let Err(e) = file::open_path(&path, &self.inbox) {
                self.toasts.push(Note::error(t!("toast.open_failed", file = file_name(&path)), e));
            }
        }
        // The files dropped on the window: read in the background, they come back below.
        for file in ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files)) {
            #[cfg(not(target_arch = "wasm32"))]
            self.user_acted();
            if let Err(name) = file::drop_file(file, &self.inbox, ctx) {
                self.toasts.push(Note::error(t!("toast.drop_unknown", file = name), t!("toast.drop_supported")));
            }
        }
    }

    pub(super) fn take_file_outcomes(&mut self) {
        // What the file dialogs ended with (they never block the event loop).
        for outcome in self.inbox.take() {
            match outcome {
                file::Outcome::Opened { name, bytes } => self.file_opened(name, bytes),
                file::Outcome::TextureImage { doc, material, bytes } => self.texture_read(doc, material, bytes),
                file::Outcome::Written { path } => self.toasts.push(Note::success(t!("toast.written", file = file_name(&path)))),
                file::Outcome::Exported { path } => self
                    .toasts
                    .push(Note::success(t!("toast.written", file = file_name(&path))).with_action(t!("toast.open"), Act::Open(path))),
                file::Outcome::WriteFailed { error } => self.toasts.push(Note::error(t!("toast.export_failed"), error)),
                file::Outcome::Saved { doc, path, revision } => {
                    self.add_recent_file(&path);
                    if let Some(d) = self.docs.by_id_mut(doc) {
                        d.path = Some(path);
                        d.saved_at = revision;
                        self.toasts.push(Note::success(t!("toast.saved", file = d.name())));
                    }
                    self.closing_saved(doc, true);
                }
                file::Outcome::SaveFailed { doc, error } => {
                    let name = self.docs.by_id_mut(doc).map(|d| d.name()).unwrap_or_default();
                    self.toasts.push(Note::error(t!("toast.save_failed", file = name), error));
                    self.closing_saved(doc, false);
                }
                file::Outcome::SaveCancelled { doc } => self.closing_saved(doc, false),
                file::Outcome::Imported { name, bytes } => {
                    // Into the shown document; from the start page, a new one.
                    if self.docs.active().is_none() {
                        self.new_document();
                    }
                    // Several dropped at once: the editor reads one at a time.
                    if self.docs.active().is_some_and(|d| d.editor.is_importing()) {
                        self.toasts.push(Note::error(t!("toast.open_failed", file = name), t!("toast.import_busy")));
                    } else if let Some(d) = self.docs.active_mut() {
                        d.editor.import(name, bytes);
                    }
                }
                file::Outcome::ReadFailed { name, error } => {
                    self.toasts.push(Note::error(t!("toast.open_failed", file = name), error));
                }
            }
        }
    }

    /// A file read: a model, materials or a template.
    fn file_opened(&mut self, name: String, bytes: Vec<u8>) {
        match Document::read_file(&bytes) {
            Ok((doc, skraft_model::Content::Model)) => {
                self.add_recent_file(&name);
                self.open_document(doc, Some(name), &Document::file_view(&bytes));
            }
            // Materials alone: into the shown document; from the start page, a new one.
            Ok((materials, skraft_model::Content::Materials)) => {
                if self.docs.active().is_none() {
                    self.new_document();
                }
                let count = materials.materials().count();
                if let Some(d) = self.docs.active_mut() {
                    let op = skraft_model::Operation::ImportMaterials { fragment: std::sync::Arc::new(materials) };
                    if d.editor.apply(op).is_ok() {
                        self.toasts.push(Note::success(tp!("toast.materials_imported", count, file = file_name(&name))));
                    }
                }
            }
            // A template: put in the library, and in the shown document a sheet made from
            // it, or a presentation when it holds several (docs/ui/drawing.md#templates).
            Ok((template, skraft_model::Content::Template)) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let stem = std::path::Path::new(&name).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                    let _ = crate::drawing::templates::install(&stem, &bytes);
                }
                if self.docs.active().is_none() {
                    self.new_document();
                }
                if let Some(d) = self.docs.active_mut() {
                    let several = template.drawing().sheets().count() > 1;
                    let template = std::sync::Arc::new(template);
                    let added = if several {
                        crate::drawing::sheet::presentation_from(&mut d.editor, &mut d.sheet, template)
                    } else {
                        crate::drawing::sheet::sheet_from(&mut d.editor, &mut d.sheet, template, None, None)
                    };
                    if added {
                        let key = if several {
                            t!("toast.template_presentation_added", file = file_name(&name))
                        } else {
                            t!("toast.template_added", file = file_name(&name))
                        };
                        self.toasts.push(Note::success(key));
                    }
                }
            }
            Err(e) => self.toasts.push(Note::error(t!("toast.open_failed", file = file_name(&name)), e)),
        }
    }

    /// An image picked for a material's texture.
    fn texture_read(&mut self, doc: DocId, material: skraft_model::Id, bytes: Vec<u8>) {
        let Some(d) = self.docs.by_id_mut(doc) else { return };
        let Some(image) = skraft_model::Asset::new(bytes) else {
            self.toasts.push(Note::error(t!("toast.texture_failed"), t!("toast.texture_not_image")));
            return;
        };
        // Its size kept when it had one; else 600 mm wide, its height as the image's.
        let current = d.editor.document().material(material).ok().and_then(|m| m.texture.clone());
        let size = match current {
            Some(t) => t.size,
            None => {
                let (w, h) = image_size(&image.bytes).unwrap_or((1, 1));
                [600.0, 600.0 * h as f64 / w.max(1) as f64]
            }
        };
        let rotation = d.editor.document().material(material).ok().and_then(|m| m.texture.as_ref()).map_or(0.0, |t| t.rotation);
        let texture = skraft_model::Texture { image: image.id(), size, rotation };
        let op = skraft_model::Operation::SetTexture { material, texture: Some(texture), image: Some(image) };
        let _ = d.editor.apply(op);
    }

    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    pub(super) fn show_window_title(&self, ctx: &egui::Context) {
        // Still the window's title where the OS shows it (Window menu, Mission Control).
        // A dev build says which (several may run, one per worktree).
        let app = crate::identity::build().map_or_else(|| "Skraft".to_owned(), |b| format!("Skraft [{}]", b.label()));
        let title = match self.docs.active() {
            Some(d) => format!("{app} — {}{}", d.name(), if d.modified() { " •" } else { "" }),
            None => app,
        };
        #[cfg(not(target_arch = "wasm32"))]
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        // eframe's web backend ignores that command, with a console warning at each frame.
        #[cfg(target_arch = "wasm32")]
        if let Some(page) = web_sys::window().and_then(|w| w.document()) {
            if page.title() != title {
                page.set_title(&title);
            }
        }
    }

    pub(super) fn show_title_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, views: &[CommandView], actions: &mut Vec<Action>) {
        let tabs = self.tabs();
        let bar = titlebar::Bar {
            tabs: &tabs,
            shown: self.docs.active_index(),
            reveal: !cfg!(target_arch = "wasm32"),
            integrated: crate::INTEGRATED_TITLEBAR,
            ground: self.docs.active().map(|_| (viewport::color(self.theme.ui.view), viewport::color(self.theme.ui.view_text))),
            remote: self.remote_mark(ctx),
            #[cfg(not(target_arch = "wasm32"))]
            update: self.prefs.updates.announce().map(str::to_owned),
            #[cfg(target_arch = "wasm32")]
            update: None,
            #[cfg(not(target_arch = "wasm32"))]
            instance: self.registration.as_ref().map(|r| r.info()).filter(|i| i.build.is_some()),
            #[cfg(target_arch = "wasm32")]
            instance: None,
            #[cfg(not(target_arch = "wasm32"))]
            session: self.session.as_ref().and_then(crate::identity::Session::name),
            #[cfg(target_arch = "wasm32")]
            session: None,
        };
        titlebar::show(ui, &bar, views, actions);
    }

    /// The windows over the document: preferences, about, updates, journal, palette, prompts.
    pub(super) fn secondary_windows(&mut self, ctx: &egui::Context, views: &[CommandView], actions: &mut Vec<Action>) {
        self.preferences_window(ctx);
        let check_updates = crate::dialogs::about::show(ctx, &mut self.about_open);
        #[cfg(not(target_arch = "wasm32"))]
        {
            if check_updates {
                self.updates.open(&self.prefs.updates);
            }
            self.updates.show(ctx, &mut self.prefs.updates);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = check_updates;
        let journal = self.docs.active().map(|d| (d.name(), d.editor.journal()));
        crate::dialogs::journal_window::show(ctx, &mut self.journal_window, journal);
        self.palette_window(ctx, views, actions);
        self.save_prompt(ctx);
        self.loading_window(ctx);
    }

    pub(super) fn show_toasts(&mut self, ctx: &egui::Context, status_top: f32) {
        // A toast whose part is no more goes (undone, erased, its document closed).
        let docs = &self.docs;
        self.toasts.retain_live(|act| match act {
            Act::Rename { doc, target } => {
                docs.index_of(*doc).and_then(|i| docs.get(i)).is_some_and(|d| d.editor.naming_of(*target).is_some())
            }
            Act::Link { doc, act } => docs.index_of(*doc).and_then(|i| docs.get(i)).is_some_and(|d| d.editor.link_act_live(*act)),
            Act::Reveal(_) | Act::Open(_) | Act::ShowUpdate => true,
        });
        for act in self.toasts.show(ctx, self.steps_top.unwrap_or(status_top)) {
            match act {
                #[cfg(not(target_arch = "wasm32"))]
                Act::Reveal(path) => reveal(&path),
                #[cfg(not(target_arch = "wasm32"))]
                Act::Open(path) => open_file(&path),
                #[cfg(target_arch = "wasm32")]
                Act::Reveal(_) | Act::Open(_) | Act::ShowUpdate => {}
                #[cfg(not(target_arch = "wasm32"))]
                Act::ShowUpdate => self.updates.open(&self.prefs.updates),
                Act::Link { doc, act } => {
                    if let Some(d) = self.docs.by_id_mut(doc) {
                        d.editor.run_link_act(act);
                    }
                }
                Act::Rename { doc, target } => {
                    if self.naming.is_none()
                        && let Some(n) = self.docs.by_id_mut(doc).and_then(|d| d.editor.naming_of(target))
                    {
                        self.naming = Some(crate::dialogs::naming::Dialog::new(doc, n));
                    }
                }
            }
        }
    }

    pub(super) fn store_preferences(&mut self, ctx: &egui::Context) {
        // Not while dragging (a panel's edge…): written once released.
        if self.prefs != self.stored_prefs && !ctx.input(|i| i.pointer.any_down()) {
            if self.prefs.shortcuts != self.stored_prefs.shortcuts {
                self.keymap = Keymap::new(&self.prefs.shortcuts);
            }
            if let Err(e) = self.prefs.save() {
                self.toasts.push(Note::error(t!("toast.prefs_not_saved"), e));
            }
            self.stored_prefs = self.prefs.clone();
        }
    }
}

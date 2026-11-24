//! The commands: their state in the menus, and running them (docs/ui/commands.md).

use super::*;

impl SkraftApp {
    /// The menus are the native ones (macOS desktop).
    fn native_menus(&self) -> bool {
        #[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
        return self.native_menu.is_some();
        #[cfg(not(all(target_os = "macos", not(target_arch = "wasm32"))))]
        false
    }

    /// A command chosen in the native menu bar. AppKit takes its ⌘ shortcut before egui sees
    /// the key, so it goes back to egui as that key while a shortcut is being captured.
    #[cfg_attr(not(all(target_os = "macos", not(target_arch = "wasm32"))), allow(dead_code))]
    pub(super) fn native_command(&mut self, id: &'static str, actions: &mut Vec<Action>) {
        if let Some(s) = self.keymap.get(id)
            && self.capturing.is_some()
        {
            let modifiers = egui::Modifiers { alt: s.alt, shift: s.shift, mac_cmd: s.command, command: s.command, ctrl: false };
            for pressed in [true, false] {
                self.pending_keys.push(egui::Event::Key { key: s.key, physical_key: None, pressed, repeat: false, modifiers });
            }
            self.ctx.request_repaint();
        } else {
            actions.push(Action::Command(id));
        }
    }

    /// Changes the shown document's settings (an undoable step).
    pub(super) fn set_settings(&mut self, settings: Settings) {
        if let Some(d) = self.docs.active_mut()
            && &settings != d.editor.document().settings()
        {
            let _ = d.editor.apply(Operation::SetSettings(settings));
        }
    }

    /// Can `id` run now?
    pub(super) fn enabled(&self, id: &str) -> bool {
        // Without a document (the start page): only what opens one, and the app's own.
        let Some(d) = self.docs.active() else {
            return match id {
                "file.new"
                | "file.open"
                | "file.import"
                | "app.command_palette"
                | "app.preferences"
                | "app.about"
                | "view.render_stats" => true,
                "app.journal" => true,
                "view.next_tab" | "view.previous_tab" => self.docs.len() > 0,
                "app.quit" | "app.remote_control" | "app.updates" => !cfg!(target_arch = "wasm32"),
                _ if let Some(k) = commands::recent_slot(id) => k < self.prefs.recent_files.len(),
                _ => PanelId::from_command(id).is_some(),
            };
        };
        let e = &d.editor;
        // While a view is framed, nothing else changes the document or the sheet.
        if d.workspace.stage == Stage::Sheet && d.sheet.framer.is_some() && ["drawing.", "edit.", "tool."].iter().any(|p| id.starts_with(p))
        {
            return false;
        }
        let section = e.document().settings().display.section.is_some();
        match id {
            "drawing.frame_view" => crate::sheet::selected_view(e, &d.sheet).is_some(),
            "edit.undo" => e.history().can_undo(),
            "edit.redo" => e.history().can_redo(),
            // « Pièces » too (docs/tools/selection.md): its selection at any depth.
            // On a sheet, the view picked (docs/ui/drawing.md).
            "edit.erase" if d.workspace.stage == Stage::Sheet => !crate::sheet::selection(e, &d.sheet).is_empty(),
            id if crate::sheet::SheetTool::of(id).is_some() => d.sheet.shown(e.document().drawing()).is_some(),
            "edit.erase" | "edit.make_assembly" => e.anything_selected(),
            "edit.explode" => e.can_explode(),
            "edit.make_unique" => e.can_make_unique(),
            "edit.group_bodies" => e.can_group_bodies(),
            "edit.move_out" => e.can_move_out(),
            "edit.imprint" => e.can_imprint(),
            // On a sheet, what is picked there: the 3D selection is not what is seen.
            "edit.cut" | "edit.copy" if d.workspace.stage == Stage::Sheet => !crate::sheet::selection(e, &d.sheet).is_empty(),
            "edit.paste" | "edit.paste_in_place" if d.workspace.stage == Stage::Sheet => {
                d.sheet.shown(e.document().drawing()).is_some()
                    && self.clipboard.as_ref().is_some_and(|c| matches!(c.content, Copied::Sheet { .. }))
            }
            "edit.select_all" if d.workspace.stage == Stage::Sheet => d.sheet.shown(e.document().drawing()).is_some(),
            "edit.cut" | "edit.copy" => e.can_copy(),
            "edit.paste" | "edit.paste_in_place" => {
                self.clipboard.as_ref().is_some_and(|c| matches!(&c.content, Copied::Model(clip) if e.can_paste(clip)))
            }
            "view.section" | "view.section_clear" => section,
            "view.show_all" => e.has_hidden(),
            "edit.delete_guides" => e.document().component(e.guide_component()).is_ok_and(|c| !c.guides.is_empty()),
            "components.purge" => !e.library_disabled() && !e.unused_components().is_empty(),
            "drawing.add_view" | "drawing.export_pdf" => d.sheet.shown(e.document().drawing()).is_some(),
            "drawing.project_view" => crate::sheet::can_project(e, &d.sheet),
            "drawing.make_composition" => !crate::sheet::selection(e, &d.sheet).is_empty(),
            "drawing.explode" | "drawing.make_unique" | "drawing.edit_composition" => {
                !crate::sheet::selected_instances(e, &d.sheet).is_empty()
            }
            id if crate::sheet::style_of(id).is_some() => !crate::sheet::selected_views(e, &d.sheet).is_empty(),
            "debug.crash_kernel" => self.debug_menu,
            "view.next_tab" | "view.previous_tab" => self.docs.len() > 1,
            "view.home" => true,
            "app.quit" | "app.remote_control" | "app.updates" => !cfg!(target_arch = "wasm32"),
            _ if let Some(k) = commands::recent_slot(id) => k < self.prefs.recent_files.len(),
            _ => commands::find(id).is_some(),
        }
    }

    /// Is `id` a tool waiting under the active one (the tool stack, docs/tools/interaction.md#tool-stack)?
    pub(super) fn waiting(&self, id: &str) -> bool {
        let Some(kind) = tool_of(id) else { return false };
        self.docs.active().is_some_and(|d| d.editor.suspended_tools().contains(&kind))
    }

    /// A toggle's state, or which tool is on.
    pub(super) fn checked(&self, id: &str) -> Option<bool> {
        match id {
            id if let Some(p) = PanelId::from_command(id) => return Some(self.prefs.layout(self.workspace().id).is_shown(p)),
            "view.render_stats" => return Some(self.prefs.show_render_stats),
            id if let Some(style) = crate::sheet::style_of(id) => {
                let d = self.docs.active()?;
                let views = crate::sheet::selected_views(&d.editor, &d.sheet);
                return Some(!views.is_empty() && views.iter().all(|(_, v)| v.style == style));
            }
            // On the sheet, Select is the sheet's tool.
            "tool.select" if self.docs.active().is_some_and(|d| d.workspace.stage == Stage::Sheet) => {
                return Some(self.docs.active()?.sheet.tool == crate::sheet::SheetTool::Select);
            }
            "drawing.project_view" => {
                let d = self.docs.active()?;
                return Some(matches!(d.sheet.tool, crate::sheet::SheetTool::Project { .. }));
            }
            id if let Some(tool) = crate::sheet::SheetTool::of(id) => return Some(self.docs.active()?.sheet.tool == tool),
            "app.journal" => return Some(self.journal_window.is_some()),
            #[cfg(not(target_arch = "wasm32"))]
            "app.remote_control" => return Some(self.remote.is_on()),
            _ => {}
        }
        let editor = &self.docs.active()?.editor;
        let d = &editor.document().settings().display;
        match id {
            "view.parallel" => Some(editor.camera.is_parallel()),
            "view.grid" => Some(d.grid),
            "view.axes" => Some(d.axes),
            "view.guides" => Some(d.guides),
            "view.silhouettes" => Some(d.silhouettes),
            "view.xray" => Some(editor.xray()),
            "view.hide_rest_of_model" => Some(d.hide_rest_of_model),
            "view.show_hidden" => Some(d.show_hidden),
            "view.section" => Some(d.section_active),
            _ => tool_of(id).map(|k| editor.tool_kind() == k),
        }
    }

    /// Its label now (« Annuler Déplacer »).
    pub(super) fn label(&self, c: &commands::Command) -> String {
        let h = self.docs.active().map(|d| d.editor.history());
        let h = h.as_ref();
        match c.id {
            "edit.undo" => h.and_then(|h| h.undo_op()).map_or_else(|| (c.label)(), |o| t!("menu.edit.undo_op", op = o.label())),
            "edit.redo" => h.and_then(|h| h.redo_op()).map_or_else(|| (c.label)(), |o| t!("menu.edit.redo_op", op = o.label())),
            id if let Some(f) = commands::recent_slot(id).and_then(|k| self.prefs.recent_files.get(k)) => file_name(f).to_owned(),
            // In the macOS app menu: « Quitter Skraft ».
            "app.quit" if self.native_menus() => t!("menu.app.quit", app = "Skraft"),
            _ => (c.label)(),
        }
    }

    pub(super) fn mac(&self) -> bool {
        self.ctx.os() == egui::os::OperatingSystem::Mac
    }

    /// Every command as the menus, the tool bar and the palette show it now: those available
    /// in the workspace shown.
    pub(super) fn command_views(&self) -> Vec<CommandView> {
        let mac = self.mac();
        let w = self.workspace();
        COMMANDS
            .iter()
            .filter(|c| w.has(c.id))
            // An empty recent file slot is not shown; nor Quit on the web (the page is closed).
            .filter(|c| commands::recent_slot(c.id).is_none_or(|k| k < self.prefs.recent_files.len()))
            .filter(|c| c.id != "app.quit" || !cfg!(target_arch = "wasm32"))
            .map(|c| CommandView {
                id: c.id,
                label: self.label(c),
                menu: c.menu,
                group: c.group,
                enabled: self.enabled(c.id),
                checked: self.checked(c.id),
                waiting: self.waiting(c.id),
                shortcut: self.keymap.get(c.id).map(|s| s.display(mac)),
            })
            .collect()
    }

    /// Runs the command `id`.
    pub(super) fn run(&mut self, id: &str) -> Result<(), Note> {
        if commands::find(id).is_none() {
            return Err(Note::info(t!("toast.command_unknown", id = id)));
        }
        if !self.workspace().has(id) || !self.enabled(id) {
            return Err(Note::info(t!("toast.command_unavailable", id = id)));
        }
        if let Some(k) = commands::recent_slot(id) {
            return self.open_recent(k);
        }
        if self.app_command(id) {
            return Ok(());
        }
        self.document_command(id)
    }

    /// Runs the command `id` if it needs no document: done?
    fn app_command(&mut self, id: &str) -> bool {
        match id {
            "file.new" => {
                self.new_document();
                true
            }
            "file.open" => {
                file::open(&self.inbox, &self.ctx);
                true
            }
            "file.import" => {
                file::import(&self.inbox, &self.ctx);
                true
            }
            "file.save" | "file.save_as" => {
                self.save(id == "file.save_as");
                true
            }
            "file.close" => {
                let shown = self.docs.active().map(|d| d.id);
                self.close_documents(shown.into_iter().collect());
                true
            }
            "app.quit" => {
                self.request_quit();
                true
            }
            "view.next_tab" | "view.previous_tab" => {
                self.docs.cycle(id == "view.next_tab");
                true
            }
            "view.home" => {
                self.docs.home();
                true
            }
            "app.command_palette" => {
                self.palette = Some(Palette::default());
                true
            }
            "app.journal" => {
                self.journal_window = match self.journal_window {
                    Some(_) => None,
                    None => Some(Default::default()),
                };
                true
            }
            "app.about" => {
                self.about_open = true;
                true
            }
            #[cfg(not(target_arch = "wasm32"))]
            "app.updates" => {
                self.updates.open(&self.prefs.updates);
                true
            }
            "app.preferences" => {
                let first = if self.docs.active().is_some() { PrefsTab::Model } else { PrefsTab::General };
                self.prefs_window = Some(self.prefs_window.unwrap_or(first));
                true
            }
            id if let Some(p) = PanelId::from_command(id) => {
                self.prefs.layout_mut(self.workspace().id).toggle(p);
                true
            }
            "view.render_stats" => {
                self.prefs.show_render_stats = !self.prefs.show_render_stats;
                true
            }
            #[cfg(not(target_arch = "wasm32"))]
            "app.remote_control" => {
                self.set_remote(!self.remote.is_on());
                true
            }
            "edit.cut" | "edit.copy" => {
                self.copy(id == "edit.cut");
                true
            }
            "edit.paste" | "edit.paste_in_place" => {
                self.paste(id == "edit.paste_in_place");
                true
            }
            "edit.imprint" => {
                self.ask_imprint();
                true
            }
            _ => false,
        }
    }

    /// Runs the command `id` on the shown document: its tools and views, then the drawing's
    /// commands, then the model's.
    fn document_command(&mut self, id: &str) -> Result<(), Note> {
        let Some(doc) = self.docs.active_mut() else {
            return Err(Note::info(t!("toast.command_unavailable", id = id)));
        };
        // On a sheet, Select (Space) is the sheet's tool (docs/ui/drawing.md#placing-views).
        if id == "tool.select" && doc.workspace.stage == Stage::Sheet {
            doc.sheet.set_tool(crate::sheet::SheetTool::Select);
            return Ok(());
        }
        if let Some(k) = tool_of(id) {
            doc.vcb.clear();
            doc.editor.set_tool(k);
            return Ok(());
        }
        if let Some(v) = view_of(id) {
            doc.editor.standard_view(v);
            return Ok(());
        }
        // The drawing as a PDF, at the next frame (the GPU draws the views' pictures).
        if id == "drawing.export_pdf" {
            if doc.sheet.shown(doc.editor.document().drawing()).is_none() {
                return Err(Note::info(t!("drawing.no_sheet")));
            }
            self.pdf_asked = Some((doc.id, self.ctx.input(|i| i.time)));
            return Ok(());
        }
        self.drawing_command(id)
    }

    /// Runs the command `id` if it is the drawing's (docs/ui/drawing.md), else the model's.
    fn drawing_command(&mut self, id: &str) -> Result<(), Note> {
        let Some(doc) = self.docs.active_mut() else {
            return Err(Note::info(t!("toast.command_unavailable", id = id)));
        };
        // The presentation shown as a template in the library (docs/ui/drawing.md#templates).
        if id == "drawing.save_template" {
            let d = doc.editor.document();
            let Some(p) = doc.sheet.presentation(d.drawing()).filter(|p| !p.sheets.is_empty()) else {
                return Err(Note::info(t!("drawing.no_sheet")));
            };
            let name = p.name.clone();
            let bytes = d.template_file(p.id).map_err(|e| Note::error(t!("toast.template_failed"), e))?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                let path = crate::templates::install(&name, &bytes).map_err(|e| Note::error(t!("toast.template_failed"), e))?;
                self.toasts.push(Note::success(t!("toast.template_saved", file = path.display().to_string())));
                return Ok(());
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = (name, bytes);
                return Err(Note::info(t!("toast.template_failed")));
            }
        }
        // The drawing's (docs/ui/drawing.md); zoom extents on a sheet frames the whole board.
        match id {
            "drawing.new_sheet" => {
                crate::sheet::new_sheet(&mut doc.editor, &mut doc.sheet);
                return Ok(());
            }
            "edit.select_all" if doc.workspace.stage == Stage::Sheet => {
                crate::sheet::select_all(&doc.editor, &mut doc.sheet);
                return Ok(());
            }
            "drawing.new_presentation" => {
                crate::sheet::new_presentation(&mut doc.editor, &mut doc.sheet);
                return Ok(());
            }
            "drawing.add_view" => return crate::sheet::add_view(&mut doc.editor, &mut doc.sheet).map_err(Note::info),
            "drawing.project_view" => return crate::sheet::project_view(&doc.editor, &mut doc.sheet).map_err(Note::info),
            "drawing.frame_view" => return crate::sheet::frame_view(&doc.editor, &mut doc.sheet).map_err(Note::info),
            "drawing.make_composition" => return crate::sheet::compose_selected(&mut doc.editor, &mut doc.sheet).map_err(Note::info),
            "drawing.explode" => return crate::sheet::explode_selected(&mut doc.editor, &mut doc.sheet).map_err(Note::info),
            "drawing.make_unique" => return crate::sheet::unique_selected(&mut doc.editor, &doc.sheet).map_err(Note::info),
            "drawing.edit_composition" => return crate::sheet::edit_selected(&doc.editor, &mut doc.sheet).map_err(Note::info),
            "edit.erase" if doc.workspace.stage == Stage::Sheet => {
                return crate::sheet::delete_selected(&mut doc.editor, &mut doc.sheet).map_err(Note::info);
            }
            id if let Some(tool) = crate::sheet::SheetTool::of(id) => {
                doc.sheet.set_tool(tool);
                return Ok(());
            }
            id if let Some(style) = crate::sheet::style_of(id) => {
                return crate::sheet::set_style(&mut doc.editor, &doc.sheet, style).map_err(Note::info);
            }
            "view.zoom_extents" if doc.workspace.stage == Stage::Sheet => {
                doc.sheet.refit();
                return Ok(());
            }
            _ => {}
        }
        self.model_command(id)
    }

    /// Runs the command `id` on the model.
    fn model_command(&mut self, id: &str) -> Result<(), Note> {
        let Some(doc) = self.docs.active_mut() else {
            return Err(Note::info(t!("toast.command_unavailable", id = id)));
        };
        let mut s = doc.editor.document().settings().clone();
        let e = &mut doc.editor;
        match id {
            "edit.undo" => e.undo(),
            "edit.redo" => e.redo(),
            "edit.select_all" => e.select_all(),
            "edit.erase" => e.delete_selection(),
            "edit.make_assembly" => e.make_assembly(),
            "edit.explode" => e.explode(),
            "edit.make_unique" => e.make_unique(),
            "edit.group_bodies" => e.group_bodies(),
            "edit.move_out" => e.move_out(),
            "edit.delete_guides" => e.delete_guides(),
            "components.purge" => e.purge_unused(),
            "view.parallel" => {
                let p = !e.camera.is_parallel();
                e.set_parallel(p);
            }
            "view.xray" => e.set_xray(!e.xray()),
            "view.grid"
            | "view.axes"
            | "view.guides"
            | "view.silhouettes"
            | "view.hide_rest_of_model"
            | "view.show_hidden"
            | "view.section"
            | "view.section_clear" => {
                let d = &mut s.display;
                match id {
                    "view.grid" => d.grid = !d.grid,
                    "view.guides" => d.guides = !d.guides,
                    "view.axes" => d.axes = !d.axes,
                    "view.silhouettes" => d.silhouettes = !d.silhouettes,
                    "view.hide_rest_of_model" => d.hide_rest_of_model = !d.hide_rest_of_model,
                    "view.show_hidden" => d.show_hidden = !d.show_hidden,
                    "view.section" => d.section_active = !d.section_active,
                    _ => {
                        d.section = None;
                        d.section_active = false;
                    }
                }
                self.set_settings(s);
            }
            "view.zoom_extents" => e.zoom_extents(),
            "view.show_all" => e.show_all(),
            "debug.crash_kernel" => e.crash_kernel(),
            _ => return Err(Note::info(t!("toast.command_unknown", id = id))),
        }
        Ok(())
    }

    /// Copies the shown document's selection (and erases it, for `cut`) into the app's
    /// clipboard; the system clipboard gets its token.
    pub(super) fn copy(&mut self, cut: bool) {
        let Some(d) = self.docs.active_mut() else { return };
        // The state copied from: the one before the erase, for a cut.
        let revision = d.editor.history().revision();
        let content = if d.workspace.stage == Stage::Sheet {
            let Some((fragment, bounds)) = crate::sheet::copy_selected(&d.editor, &d.sheet) else { return };
            if cut {
                let _ = crate::sheet::delete_selected(&mut d.editor, &mut d.sheet);
            }
            Copied::Sheet { fragment, bounds }
        } else {
            let Some(clip) = (if cut { d.editor.cut() } else { d.editor.copy() }) else { return };
            Copied::Model(clip)
        };
        self.copies += 1;
        let c = Clipboard::new(content, d.id, revision, self.copies);
        self.ctx.copy_text(c.token.clone());
        self.clipboard = Some(c);
    }

    /// Pastes the app's clipboard into the shown document: at the cursor, or `in_place`
    /// (where it was copied from).
    pub(super) fn paste(&mut self, in_place: bool) {
        let (Some(c), Some(d)) = (&self.clipboard, self.docs.active_mut()) else { return };
        let reuse = c.reuse(d.id, |r| d.editor.history().holds(r));
        d.vcb.clear();
        match &c.content {
            // At the sheet's next frame, where the pointer is known in the level open.
            Copied::Sheet { fragment, bounds } if d.workspace.stage == Stage::Sheet => {
                d.sheet.asked =
                    Some(crate::sheet::SheetAsk::Paste { fragment: fragment.clone(), bounds: *bounds, in_place, linked: reuse });
            }
            Copied::Model(clip) if d.workspace.stage != Stage::Sheet => {
                if in_place {
                    d.editor.paste_in_place(clip, reuse);
                } else {
                    d.editor.paste(clip, reuse);
                }
            }
            _ => {}
        }
    }

    pub(super) fn act(&mut self, a: Action) {
        match a {
            Action::Command(id) => {
                if let Err(m) = self.run(id) {
                    self.toasts.push(m);
                }
                return;
            }
            Action::Tab(t) => return self.tab_action(t),
            #[cfg(not(target_arch = "wasm32"))]
            Action::RemoteRetry => return self.set_remote(true),
            #[cfg(target_arch = "wasm32")]
            Action::RemoteRetry => return,
            Action::Ribbon(change) => {
                let w = self.workspace();
                return self.prefs.ribbon.apply(change, w);
            }
            Action::Workspace(id) => {
                if let Err(e) = self.set_workspace(id) {
                    log::warn!("{e}");
                }
                return;
            }
            Action::QuickPin(id) => return crate::window::quick::toggle(&mut self.prefs.quick_tools, id),
            Action::ViewPin(id) => return crate::window::view_bar::toggle(&mut self.prefs.view_bar, id),
            Action::ViewBarSide(side) => return self.prefs.view_bar_side = side,
            Action::CardList(card, list) => {
                let pref = match card {
                    crate::catalog::Card::Components => &mut self.prefs.components_list,
                    crate::catalog::Card::Compositions => &mut self.prefs.compositions_list,
                    crate::catalog::Card::Materials => &mut self.prefs.materials_list,
                };
                return *pref = list;
            }
            Action::ViewMove(id, before) => return crate::window::view_bar::move_to(&mut self.prefs.view_bar, id, before),
            Action::QuickMove(id, before) => return crate::window::quick::move_to(&mut self.prefs.quick_tools, id, before),
            Action::QuickTool(id) => {
                if let (Some(k), Some(d)) = (tool_of(id), self.docs.active_mut()) {
                    d.vcb.clear();
                    d.editor.start_quick_tool(k);
                }
                return;
            }
            _ => {}
        }
        let Some(doc) = self.docs.active_mut() else { return };
        let e = &mut doc.editor;
        match a {
            Action::Command(_)
            | Action::Tab(_)
            | Action::Ribbon(_)
            | Action::Workspace(_)
            | Action::QuickPin(_)
            | Action::ViewPin(_)
            | Action::ViewMove(..)
            | Action::ViewBarSide(_)
            | Action::CardList(..)
            | Action::QuickMove(..)
            | Action::QuickTool(_)
            | Action::RemoteRetry => {}
            // A body chosen in the outliner: its part opened.
            Action::SelectBody(path, body, add) => e.toggle_body(&path, body, add),
            Action::EditBlend(node) => e.edit_blend(node),
            Action::SelectPath(p, add) => e.toggle_path(&p, add),
            Action::SelectParts(paths) => e.select_occurrences(paths),
            Action::SelectModel => e.select_model(),
            Action::ChooseComponent(c) => e.choose_component(c),
            Action::ChooseMaterial(m) => e.choose_material(m),
            Action::PlaceComponent(c) => {
                doc.vcb.clear();
                if e.place_component(c) {
                    self.viewport.dragged_in = true;
                }
            }
            Action::SelectOccurrences(c) => e.select_occurrences_of(c),
            Action::ReplaceSelection(c) => e.replace_selection(c),
            Action::DeleteComponent(c) => e.delete_component(c),
            Action::EditProfile(p, body) => e.edit_body_profile(&p, body),
            Action::SetHidden(p, hidden) => e.set_hidden(&p, hidden),
            Action::Apply(op) => {
                let _ = e.apply(op);
            }
            Action::ToolKey(key) => e.input(skraft_tools::ToolInput::Key { key, mods: Default::default() }),
            Action::ToolOption(key, value) => e.set_tool_option(key, value),
            Action::ToolLength(key, text) => e.type_tool_length(key, &text),
            Action::PartScope(s) => e.set_part_scope(s),
            Action::TreeHover(row) => e.hover_tree_row(row),
            Action::TreePick(row) => e.pick_tree_row(row),
            Action::BodySign(s) => e.set_body_sign(s),
            Action::HoverMachining(m) => e.set_listed_hover(m),
            // The view is drawn before the panels: again, for the hover to show this frame.
            Action::HoverOutliner(p) => {
                if e.set_outliner_hover(p) {
                    self.ctx.request_repaint();
                }
            }
            Action::PickTexture(m) => file::pick_texture(doc.id, m, &self.inbox, &self.ctx),
            Action::ImportMaterials => file::import_materials(&self.inbox, &self.ctx),
            Action::ExportMaterials(one) => {
                let document = doc.editor.document();
                // One: its name; all: the model's.
                let name = match one.and_then(|id| document.material(id).ok()) {
                    Some(m) => m.name.clone(),
                    None => {
                        let name = doc.name();
                        let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        t!("panel.materials.export_name", model = stem)
                    }
                };
                let ids: Vec<skraft_model::Id> = one.into_iter().collect();
                match document.materials_file(&ids) {
                    Ok(bytes) => file::export_materials(bytes, &name, &self.inbox, &self.ctx),
                    Err(err) => self.toasts.push(Note::error(t!("toast.export_failed"), err)),
                }
            }
            Action::PaintWith(m) => {
                e.choose_material(m);
                if e.tool_kind() != ToolKind::Paint {
                    doc.vcb.clear();
                    e.set_tool(ToolKind::Paint);
                }
            }
        }
    }
}

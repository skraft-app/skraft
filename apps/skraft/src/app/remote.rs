//! Serving the control channel (docs/control-protocol.md).

use super::*;

impl SkraftApp {
    /// Switches the control channel on or off, and remembers it (the preference): from the
    /// command, the preferences, the title bar's mark.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn set_remote(&mut self, on: bool) {
        self.prefs.remote_control = on;
        if let Err(e) = self.remote.set(on) {
            self.toasts.push(Note::error(t!("toast.control_failed"), e));
        }
    }

    /// Answers the control channel's requests (desktop, when it is on): the editor's methods
    /// through `skraft_control::editor`, the window's here; an `app.wait` not met yet waits
    /// for a later frame.
    pub(super) fn serve_control(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        // Desktop: the loopback server's requests.
        #[cfg(not(target_arch = "wasm32"))]
        let askers: Vec<crate::driving::Asker> = {
            crate::handles::set_enabled(self.remote.is_on());
            match self.remote.control() {
                Some(c) => c
                    .take()
                    .into_iter()
                    .map(|p| {
                        let call = p.call.clone();
                        crate::driving::Asker::new(call, move |o| p.answer(o))
                    })
                    .collect(),
                None => Vec::new(),
            }
        };
        // Web: the page's calls (`window.skraft`).
        #[cfg(target_arch = "wasm32")]
        let askers = {
            crate::handles::set_enabled(crate::web::used());
            crate::web::take()
        };
        // What a call does is its client's (the journal says so), whatever this frame's input.
        let origin = self.driving.origin();
        for asker in askers {
            self.set_origin(&skraft_tools::journal::Origin::control(asker.call.client.as_deref()));
            self.serve(asker, ctx, frame);
        }
        self.set_origin(&origin);
        self.answer_waits();
        // Frames go on while something waits: an idle window draws none.
        if self.driving.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    /// Who causes what the documents go through from now on (their journals).
    pub(super) fn set_origin(&mut self, origin: &skraft_tools::journal::Origin) {
        for d in self.docs.iter_mut() {
            d.editor.set_origin(origin.clone());
        }
    }

    /// The user acted other than through the window's events (the native menu bar, a file
    /// the system opens): the journal notes what follows as theirs. Native only: the web has
    /// neither.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn user_acted(&mut self) {
        self.driving.user_acted();
        self.set_origin(&skraft_tools::journal::Origin::User);
    }

    /// Takes one request: answered now, or once a later frame has what it waits for.
    fn serve(&mut self, asker: crate::driving::Asker, ctx: &egui::Context, frame: &eframe::Frame) {
        use skraft_control::Error;
        use skraft_control::params::{UiDragParams, UiKeyParams, UiPointerParams, UiScrollParams, UiTextParams, WaitParams};
        let call = &asker.call;
        let frames = match call.method.as_str() {
            "app.wait" => match call.params::<WaitParams>() {
                Ok(params) => {
                    let deadline = web_time::Instant::now() + params.timeout();
                    self.driving.waiting.push(crate::driving::Waiting { asker, params, deadline });
                    return;
                }
                Err(e) => Err(e),
            },
            "ui.screenshot" => {
                self.driving.screenshot(ctx, asker);
                return;
            }
            // The web draws the viewport in the window's canvas: the window's image.
            #[cfg(target_arch = "wasm32")]
            "app.screenshot" => {
                self.driving.screenshot(ctx, asker);
                return;
            }
            "ui.click" | "ui.move" => call.params::<UiPointerParams>().and_then(|p| {
                let pos = self.ui_where(&p.at)?;
                let mods = crate::driving::modifiers(&p.mods);
                Ok(match call.method.as_str() {
                    "ui.click" => crate::driving::click(pos, p.button, p.double, mods),
                    _ => vec![(vec![egui::Event::PointerMoved(pos)], mods)],
                })
            }),
            "ui.drag" => call.params::<UiDragParams>().and_then(|p| {
                let (from, to) = (self.ui_where(&p.from)?, self.ui_where(&p.to)?);
                Ok(crate::driving::drag(from, to, p.steps.unwrap_or(8).clamp(1, 1000), p.button, crate::driving::modifiers(&p.mods)))
            }),
            "ui.key" => call.params::<UiKeyParams>().and_then(|p| {
                let key = egui::Key::from_name(&p.key)
                    .ok_or_else(|| Error::BadParams(format!("no key {:?} (egui's names: Escape, Enter, ArrowUp, A…)", p.key)))?;
                let mods = crate::driving::modifiers(&p.mods);
                let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: mods };
                Ok(match p.pressed {
                    Some(pressed) => vec![(vec![event(pressed)], mods)],
                    None => vec![(vec![event(true)], mods), (vec![event(false)], mods)],
                })
            }),
            "ui.text" => call.params::<UiTextParams>().map(|p| vec![(vec![egui::Event::Text(p.text)], egui::Modifiers::NONE)]),
            "ui.scroll" => call.params::<UiScrollParams>().and_then(|p| {
                let pos = self.ui_where(&p.at)?;
                let mods = crate::driving::modifiers(&p.mods);
                let wheel = egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(p.delta[0], p.delta[1]),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: mods,
                };
                Ok(vec![(vec![egui::Event::PointerMoved(pos)], mods), (vec![wheel], mods)])
            }),
            _ => {
                let outcome = self.control_call(call, ctx, frame);
                asker.answer(outcome);
                return;
            }
        };
        match frames {
            Ok(frames) => self.driving.take_in(asker, frames),
            Err(e) => asker.answer(Err(e)),
        }
    }

    /// Where a real pointer event lands (window points): a point, a handle's centre, a world
    /// point's pixel in the viewport.
    fn ui_where(&self, w: &skraft_control::params::UiWhere) -> Result<egui::Pos2, skraft_control::Error> {
        use skraft_control::Error;
        match (w.pos, &w.handle, w.at) {
            (Some([x, y]), None, None) => Ok(egui::pos2(x, y)),
            (None, Some(name), None) => crate::handles::find(name)
                .map(|h| h.rect.center())
                .ok_or_else(|| Error::Failed(format!("no handle {name:?} drawn (ui.handles lists them)"))),
            (None, None, Some(at)) => {
                let view = crate::handles::find("viewport").ok_or_else(|| Error::Failed("no viewport drawn".into()))?;
                let d = self.docs.active().ok_or_else(|| Error::Failed("no document open".into()))?;
                let px = d
                    .editor
                    .camera
                    .project(glam::DVec3::from_array(at), d.editor.viewport_size())
                    .ok_or_else(|| Error::Failed(format!("{at:?} is behind the camera")))?;
                Ok(view.rect.min + egui::vec2(px.x as f32, px.y as f32))
            }
            _ => Err(Error::BadParams("`where` is `pos`, `handle` or `at`, one of them".into())),
        }
    }

    /// Answers what waited for this frame: real inputs taken in, waits met or out of time, the
    /// window's image.
    fn answer_waits(&mut self) {
        use serde_json::json;
        use skraft_control::Answer;
        let stamp = self.control_stamp();
        for asker in self.driving.taken() {
            asker.answer(Ok(Answer::Json(stamp.clone())));
        }
        let (revision, idle) = match self.docs.active() {
            Some(d) => (d.editor.history().revision(), skraft_control::editor::idle(&d.editor)),
            None => (0, true),
        };
        let now = web_time::Instant::now();
        let mut i = 0;
        while i < self.driving.waiting.len() {
            let w = &self.driving.waiting[i];
            let met = w.params.met(revision, idle);
            if met || now >= w.deadline {
                let w = self.driving.waiting.swap_remove(i);
                let mut v = stamp.clone();
                v["timed_out"] = json!(!met);
                w.asker.answer(Ok(Answer::Json(v)));
            } else {
                i += 1;
            }
        }
        if let Some((image, askers)) = self.driving.screenshot_taken() {
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            let png = crate::driving::png(image.size[0] as u32, image.size[1] as u32, &rgba);
            for a in askers {
                a.answer(png.clone().map(Answer::Png).map_err(skraft_control::Error::Failed));
            }
        }
    }

    /// The active document's identity and state, as every wait answers it.
    fn control_stamp(&self) -> serde_json::Value {
        use serde_json::json;
        match self.docs.active() {
            Some(d) => json!({
                "document": d.id,
                "revision": d.editor.history().revision(),
                "busy": d.editor.busy(),
                "idle": skraft_control::editor::idle(&d.editor),
            }),
            None => json!({ "document": null, "revision": 0, "busy": false, "idle": true }),
        }
    }

    /// Answers one call of the control channel (but `app.wait`).
    fn control_call(&mut self, call: &skraft_control::Call, ctx: &egui::Context, frame: &eframe::Frame) -> skraft_control::Outcome {
        // The viewport's own capture is the desktop's (the web's is the window's).
        #[cfg(target_arch = "wasm32")]
        let _ = frame;
        use serde_json::json;
        use skraft_control::params::{DocsAction, DocsParams, NoParams, RunParams};
        use skraft_control::{Answer, Error};
        if !skraft_control::is_method(&call.method) {
            return Err(Error::UnknownMethod(call.method.clone()));
        }
        if let Some(d) = self.docs.active_mut()
            && let Some(mut outcome) = skraft_control::editor::call(&mut d.editor, call)
        {
            // A screenshot's pixels are the viewport's points times this.
            if call.method == "view.camera"
                && let Ok(Answer::Json(v)) = &mut outcome
            {
                v["pixels_per_point"] = json!(ctx.pixels_per_point());
            }
            return outcome;
        }
        let ok = |v: serde_json::Value| Ok(Answer::Json(v));
        match call.method.as_str() {
            "app.ping" => ok(json!({})),
            "app.methods" => ok(skraft_control::methods_json()),
            "app.commands" => {
                call.params::<NoParams>()?;
                let list: Vec<_> = self
                    .command_views()
                    .into_iter()
                    .map(|v| json!({ "id": v.id, "label": v.label, "enabled": v.enabled, "checked": v.checked, "shortcut": v.shortcut }))
                    .collect();
                ok(json!(list))
            }
            "app.run" => {
                let p: RunParams = call.params()?;
                self.run(&p.id).map_err(|n| Error::Failed(n.to_string()))?;
                let mut v = self.control_stamp();
                v["status"] =
                    json!(self.docs.active().map(|d| if d.workspace.stage == Stage::Sheet { d.sheet.status() } else { d.editor.status() }));
                ok(v)
            }
            "app.docs" => {
                let p: DocsParams = call.params()?;
                let tab = || p.index.filter(|i| *i < self.docs.len()).ok_or_else(|| Error::Failed(format!("no tab {:?}", p.index)));
                match p.action {
                    DocsAction::List => {}
                    DocsAction::New => self.new_document(),
                    DocsAction::Switch => {
                        let i = tab()?;
                        self.docs.select(i);
                    }
                    DocsAction::Close => {
                        let i = tab()?;
                        if p.force {
                            self.close_now(i);
                        } else {
                            self.tab_action(TabAction::Close(i));
                        }
                    }
                }
                let list: Vec<_> =
                    self.docs.iter().map(|d| json!({ "id": d.id, "name": d.name(), "path": d.path, "modified": d.modified() })).collect();
                let asking = self.closing.as_ref().and_then(|c| c.asking);
                ok(json!({ "docs": list, "shown": self.docs.active_index(), "asking": asking }))
            }
            "app.workspace" => {
                let p: skraft_control::params::WorkspaceParams = call.params()?;
                if let Some(id) = &p.id {
                    self.set_workspace(id).map_err(Error::Failed)?;
                }
                let list: Vec<_> = workspace::listed()
                    .map(|w| json!({ "id": w.id, "label": (w.label)(), "under_way": w.under_way, "stage": w.stage.id() }))
                    .collect();
                ok(json!({ "workspaces": list, "shown": self.docs.active().map(|d| d.workspace.id) }))
            }
            "app.state" => {
                call.params::<NoParams>()?;
                let naming = self.naming.as_ref().map(|n| {
                    let target = match n.naming.target {
                        skraft_tools::NameTarget::Component(c) => json!({ "component": c.0 }),
                        skraft_tools::NameTarget::BodyDef { part, def } => json!({ "part": part.0, "def": def.0 }),
                    };
                    json!({ "document": n.doc, "target": target, "title": n.naming.title, "name": n.text() })
                });
                // Which app answers: several may run.
                #[cfg(not(target_arch = "wasm32"))]
                let instance = self.registration.as_ref().map(|r| json!(r.info()));
                #[cfg(target_arch = "wasm32")]
                let instance: Option<serde_json::Value> = None;
                let Some(d) = self.docs.active() else {
                    return ok(json!({ "document": null, "toasts": self.toasts.json(), "naming": naming, "instance": instance }));
                };
                let mut state = skraft_control::editor::state(&d.editor);
                state["modified"] = json!(d.modified());
                state["path"] = json!(d.path);
                state["document"] = json!(d.id);
                state["workspace"] = json!(d.workspace.id);
                // On a sheet, the status bar says the sheet's tool's.
                if d.workspace.stage == Stage::Sheet {
                    state["status"] = json!(d.sheet.status());
                }
                state["toasts"] = self.toasts.json();
                state["naming"] = json!(naming);
                state["instance"] = json!(instance);
                ok(state)
            }
            "app.instances" => {
                call.params::<NoParams>()?;
                #[cfg(not(target_arch = "wasm32"))]
                let all = skraft_control::server::default_dir().map(|d| skraft_control::instances::list_json(&d));
                #[cfg(target_arch = "wasm32")]
                let all: Option<serde_json::Value> = None;
                ok(all.unwrap_or_else(|| json!([])))
            }
            #[cfg(not(target_arch = "wasm32"))]
            "app.screenshot" => {
                call.params::<NoParams>()?;
                let shot = frame.wgpu_render_state().and_then(|s| self.viewport.capture(s));
                match shot.map(|(w, h, px)| crate::driving::png(w, h, &px)) {
                    Some(Ok(png)) => Ok(Answer::Png(png)),
                    Some(Err(e)) => Err(Error::Failed(e)),
                    None => Err(Error::Unavailable("nothing drawn yet".into())),
                }
            }
            "ui.handles" => {
                let p: skraft_control::params::UiHandlesParams = call.params()?;
                let list: Vec<_> = crate::handles::last(&p.prefix)
                    .into_iter()
                    .map(|h| {
                        json!({
                            "name": h.name,
                            "enabled": h.enabled,
                            "min": [h.rect.min.x, h.rect.min.y],
                            "max": [h.rect.max.x, h.rect.max.y],
                            "center": [h.rect.center().x, h.rect.center().y],
                        })
                    })
                    .collect();
                ok(json!({ "handles": list, "pixels_per_point": ctx.pixels_per_point() }))
            }
            "ui.messages" => {
                let p: skraft_control::params::UiMessagesParams = call.params()?;
                let list: Vec<_> = self.driving.messages.iter().map(|m| json!({ "text": m.text, "ms": m.ms })).collect();
                if p.clear {
                    self.driving.messages.clear();
                }
                ok(json!({ "messages": list }))
            }
            m if ["model.", "tool.", "view.", "journal."].iter().any(|p| m.starts_with(p)) => Err(Error::Failed("no document open".into())),
            m => Err(Error::UnknownMethod(m.into())),
        }
    }
}

impl SkraftApp {
    /// The title bar's mark of the control channel, while it is on; frames follow while it
    /// flashes.
    pub(super) fn remote_mark(&mut self, ctx: &egui::Context) -> Option<titlebar::RemoteMark> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = ctx;
            None
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::control::Status;
            let status = self.remote.status();
            if let Some(r) = &mut self.registration
                && let Err(e) = r.set_port(match &status {
                    Status::On { port, .. } => Some(*port),
                    _ => None,
                })
            {
                log::warn!("the registry of running apps: {e}");
            }
            match status {
                Status::Off => None,
                Status::On { port, served, flash, token_file } => {
                    if flash > 0.0 {
                        ctx.request_repaint();
                    }
                    Some(titlebar::RemoteMark { port, served, flash, error: None, token_file })
                }
                Status::Failed { port, error } => {
                    Some(titlebar::RemoteMark { port, served: 0, flash: 0.0, error: Some(error), token_file: None })
                }
            }
        }
    }
}

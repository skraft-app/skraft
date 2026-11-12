//! The document side of the editor: operations started, run by the kernel, finished or
//! abandoned; undo, redo, opening a file; the scene kept in step (docs/ARCHITECTURE.md §7 ter).

use glam::{DAffine3, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::kernel::{Job, JobId, Output, Task};
use solidsketch_model::{Applied, Document, EntityPath, History, Operation, Outcome, Prepared, Primitive};

use super::Editor;

/// What follows an operation applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum After {
    Nothing,
    /// Select what it created, if anything.
    Select,
    /// Gather what it created; the whole gathered is selected once the queue is empty.
    Collect,
}

/// A job of [`Editor::crash_kernel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Probe {
    /// Panics on purpose.
    Crash,
    /// Sent right behind it: must come through anyway.
    Next,
}

/// The operation the kernel is working on.
pub(super) struct Pending {
    pub(super) job: JobId,
    pub(super) op: Operation,
    pub(super) after: After,
    /// The last step was undone to make way for this one (`Command::Replace`): redo it if
    /// this one fails or is abandoned.
    pub(super) restore: bool,
}

impl Editor {
    // ---- document ------------------------------------------------------------------------------

    /// Applies `op`, or starts it if it needs the kernel: `Ok(None)`, the result comes with a
    /// later [`Editor::poll`]. On failure the status bar says why and nothing changes.
    pub fn apply(&mut self, op: Operation) -> Result<Option<Outcome>, String> {
        self.start(op, After::Nothing, false)
    }

    pub(super) fn start(&mut self, op: Operation, after: After, restore: bool) -> Result<Option<Outcome>, String> {
        if self.pending.is_some() {
            let msg = t!("status.busy");
            self.message = Some(msg.clone());
            return Err(msg);
        }
        match self.document().prepare(&op) {
            Err(e) => Err(self.fail(&op, e.to_string(), restore)),
            Ok(Prepared::Done(applied)) => Ok(Some(self.finish(op, *applied, after))),
            Ok(Prepared::Kernel(job)) => {
                let id = self.job_id();
                self.pending = Some(Pending { job: id, op, after, restore });
                self.last = None;
                self.kernel.submit(id, job);
                // A kernel run inline has answered already.
                self.poll();
                match &self.pending {
                    Some(p) if p.job == id => Ok(None),
                    _ => self.last.take().map_or(Ok(None), |r| r.map(Some)),
                }
            }
        }
    }

    pub(super) fn job_id(&mut self) -> JobId {
        self.next_job += 1;
        self.next_job
    }

    /// Records `applied` (`op` done) in the history and follows up.
    pub(super) fn finish(&mut self, op: Operation, applied: Applied, after: After) -> Outcome {
        let Applied { document, outcome, displays } = applied;
        self.history.push(document, op);
        for (solid, display) in displays {
            if let Err(e) = &display {
                self.message = Some(t!("status.display_failed", error = e));
            }
            self.scene.insert(&solid, display);
        }
        match after {
            After::Select if !outcome.created.is_empty() => self.selection = outcome.created.clone(),
            // Machinings made in an open part: selected while they are shown.
            After::Select if !outcome.machinings.is_empty() => {
                self.selection = if self.showing_machinings() { outcome.machinings.clone() } else { Vec::new() };
            }
            After::Collect => self.collected.extend(outcome.created.iter().copied()),
            _ => {}
        }
        self.after_change();
        self.follow_cursor();
        outcome
    }

    /// Once nothing is under way any more, the tool follows the cursor where it is now (a
    /// shape tool back at its first step shows its plane there).
    fn follow_cursor(&mut self) {
        if !self.busy() {
            self.rehover();
        }
    }

    /// Says why `op` failed; puts back the step it replaced. Returns the message.
    pub(super) fn fail(&mut self, op: &Operation, why: String, restore: bool) -> String {
        let msg = t!("status.op_failed", op = op.label(), error = why);
        if restore && self.history.redo() {
            self.after_change();
        }
        self.message = Some(msg.clone());
        self.follow_cursor();
        msg
    }

    /// Takes in what the kernel finished: the pending operation, meshes. Call every frame;
    /// returns whether anything came.
    pub fn poll(&mut self) -> bool {
        let results = self.kernel.poll();
        if results.is_empty() {
            return false;
        }
        let mut meshed = false;
        for (id, result) in results {
            if let Some(name) = self.importing.remove(&id) {
                self.imported(&name, result);
            } else if let Some(probe) = self.probes.remove(&id) {
                self.probe_report.push(match (probe, &result) {
                    (Probe::Crash, Err(e)) => t!("status.debug.crashed", error = e),
                    (Probe::Crash, Ok(_)) => t!("status.debug.not_crashed"),
                    (Probe::Next, Ok(_)) => t!("status.debug.next_done"),
                    (Probe::Next, Err(e)) => t!("status.debug.next_failed", error = e),
                });
                self.message = Some(self.probe_report.join(" — "));
                if self.probes.is_empty() {
                    self.probe_report.clear();
                }
            } else if let Some(solid) = self.meshing.remove(&id) {
                let display = match result {
                    Ok(mut outputs) => match outputs.pop() {
                        Some(Output::Display(d)) => d,
                        _ => Err(t!("status.unexpected_reply")),
                    },
                    Err(e) => Err(e.to_string()),
                };
                if let Err(e) = &display {
                    self.message = Some(t!("status.display_failed", error = e));
                }
                self.scene.insert(&solid, display);
                meshed = true;
            } else if self.pending.as_ref().is_some_and(|p| p.job == id) {
                let Some(p) = self.pending.take() else { continue };
                let applied = result.map_err(Into::into).and_then(|outputs| self.document().commit(&p.op, outputs));
                self.last = Some(match applied {
                    Ok(a) => Ok(self.finish(p.op, a, p.after)),
                    Err(e) => Err(self.fail(&p.op, e.to_string(), p.restore)),
                });
                self.pump();
                // A stretch held while computed is done: the part is drawn as it now is.
                self.held_stretch = None;
                self.update_preview_mesh();
            }
            // Anything else was abandoned: dropped.
        }
        if meshed {
            // The document the same: only what draws the meshes changes.
            self.scene.refresh_meshes();
            self.request_meshes();
        }
        true
    }

    /// Starts the queued operations, one at a time; once all are done, selects what they
    /// gathered.
    pub(super) fn pump(&mut self) {
        while self.pending.is_none() {
            let Some((op, after)) = self.queue.pop_front() else { break };
            let _ = self.start(op, after, false);
        }
        if !self.busy() && !self.collected.is_empty() {
            self.selection = std::mem::take(&mut self.collected);
        }
    }

    /// Esc while the kernel works: its result will be dropped, the queue too.
    pub(super) fn abandon(&mut self) {
        let Some(p) = self.pending.take() else { return };
        self.held_stretch = None;
        self.update_preview_mesh();
        self.queue.clear();
        self.collected.clear();
        if p.restore && self.history.redo() {
            self.after_change();
        }
        self.message = Some(t!("status.abandoned", op = p.op.label()));
    }

    /// The debug menu: makes the kernel panic on purpose, a job queued right behind, to check that a
    /// crash is recovered from (docs/ARCHITECTURE.md §7 ter): natively the panic is caught; in
    /// the browser the worker is replaced and the job behind sent to the new one. The status
    /// bar says how both went.
    pub fn crash_kernel(&mut self) {
        let crash = self.job_id();
        self.probes.insert(crash, Probe::Crash);
        self.kernel.submit(crash, Job { inputs: Vec::new(), tasks: vec![Task::Panic { message: t!("status.debug.crash_message") }] });
        let next = self.job_id();
        self.probes.insert(next, Probe::Next);
        let sphere = Primitive::Sphere { center: DVec3::ZERO, radius: 10.0 };
        self.kernel
            .submit(next, Job { inputs: Vec::new(), tasks: vec![Task::Primitive { primitive: sphere, placement: DAffine3::IDENTITY }] });
        self.message = Some(solidsketch_i18n::ellipsis(t!("status.debug.crash_asked")));
        // A kernel run inline has answered already.
        self.poll();
    }

    /// Imports a STEP file (`name`, its `bytes`) into the active context (ARCHITECTURE.md §8
    /// ter): the kernel reads it, then its content is inserted as one occurrence, selected —
    /// one step to undo. The rest of the editor stays usable meanwhile.
    pub fn import_step(&mut self, name: String, bytes: Vec<u8>) {
        let id = self.job_id();
        self.message = Some(solidsketch_i18n::ellipsis(t!("status.importing", file = name)));
        self.importing.insert(id, name);
        self.kernel.submit(id, Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes }] });
        // A kernel run inline has answered already.
        self.poll();
    }

    /// Is a file being read?
    pub fn is_importing(&self) -> bool {
        !self.importing.is_empty()
    }

    /// The kernel read the file `name`: its content inserted, or why not.
    fn imported(&mut self, name: &str, result: solidsketch_model::kernel::JobResult) {
        let step = match result {
            Ok(mut outputs) => match outputs.pop() {
                Some(Output::Step(s)) => s,
                _ => {
                    self.message = Some(t!("status.import_failed", file = name, error = t!("status.unexpected_reply")));
                    return;
                }
            },
            Err(e) => {
                self.message = Some(t!("status.import_failed", file = name, error = e));
                return;
            }
        };
        let (fragment, report) = Document::from_step(&step, name);
        if fragment.root_assembly().occurrences().is_empty() {
            self.message = Some(solidsketch_i18n::tp!("status.import_empty", report.excluded, file = name));
            return;
        }
        let context = self.document().context_of(&self.context).unwrap_or(self.document().root());
        let op = Operation::Insert { context, fragment: std::sync::Arc::new(fragment), transform: DAffine3::IDENTITY };
        self.queue.push_back((op, After::Select));
        self.pump();
        if !self.busy() {
            self.zoom_extents();
        }
        let mut message = solidsketch_i18n::tp!("status.imported", report.parts, file = name);
        if report.excluded > 0 {
            message = format!("{message} — {}", solidsketch_i18n::tp!("status.import_excluded", report.excluded));
        }
        self.message = Some(message);
    }

    /// Asks the kernel for the meshes the scene lacks, one job per solid.
    pub(super) fn request_meshes(&mut self) {
        let missing = self.scene.take_missing();
        if missing.is_empty() {
            return;
        }
        for solid in missing {
            let id = self.job_id();
            self.meshing.insert(id, solid.clone());
            self.kernel.submit(id, Job { inputs: vec![solid], tasks: vec![Task::Display { solid: 0 }] });
        }
        // A kernel run inline has answered already.
        self.poll();
    }

    pub fn undo(&mut self) {
        if self.busy() {
            return;
        }
        self.tool.reset();
        self.clear_locks();
        if self.history.undo() {
            self.after_change();
        }
    }

    pub fn redo(&mut self) {
        if self.busy() {
            return;
        }
        self.tool.reset();
        self.clear_locks();
        if self.history.redo() {
            self.after_change();
        }
    }

    /// Replaces the document (a file opened).
    pub fn open(&mut self, doc: Document) {
        self.history = History::new(doc);
        self.has_hidden.set(None);
        // What the kernel was doing for the previous document is dropped when it comes.
        self.pending = None;
        self.importing.clear();
        self.queue.clear();
        self.collected.clear();
        self.context.clear();
        self.open_part = None;
        self.selection.clear();
        self.tool.reset();
        self.after_change();
        self.zoom_extents();
    }

    /// Keeps the context, the open part and the selection valid, and the scene in step.
    pub(super) fn after_change(&mut self) {
        let doc = self.history.document();
        while !self.context.is_empty() && doc.context_of(&self.context).is_err() {
            self.context.pop();
            self.open_part = None;
        }
        let ctx = doc.context_of(&self.context).unwrap_or(doc.root());
        let open = self.open_part.and_then(|id| doc.resolve_part(&EntityPath::new(self.context.clone(), id)).ok());
        if open.is_none() {
            self.open_part = None;
        }
        // The guides of the active context stay selected while they are shown.
        let guide_component = match &open {
            Some((part, _, _)) => *part,
            None => ctx,
        };
        let guides_shown = doc.settings().display.guides;
        let guide = |id: &solidsketch_model::Id| guides_shown && doc.component(guide_component).is_ok_and(|c| c.guide(*id).is_some());
        match open {
            // An open part: its machinings while shown, and its guides.
            Some((_, p, _)) => {
                let shown = self.machinings_shown;
                self.selection.retain(|id| (shown && p.machining(*id).is_some()) || guide(id));
            }
            None => {
                if let Ok(a) = doc.assembly(ctx) {
                    // A set: the selection and the assembly can hold tens of thousands.
                    let present: std::collections::HashSet<_> = a.occurrences().iter().map(|o| o.id).collect();
                    self.selection.retain(|id| present.contains(id) || guide(id));
                }
            }
        }
        self.sync_scene();
    }
}

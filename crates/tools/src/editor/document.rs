//! The document side of the editor: operations started, run by the kernel, finished or
//! abandoned; undo, redo, opening a file; the scene kept in step (docs/ARCHITECTURE.md §7 ter).

use glam::{DAffine3, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::kernel::{Error as KernelError, Job, JobId, Output, Quality, Task};
use web_time::Instant;

use super::journal::{Event, JobKind, Step};
use solidsketch_model::{Applied, Document, EntityPath, History, Operation, Outcome, Prepared, Primitive};

use super::{Editor, NoticeKind};

/// What follows an operation applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum After {
    Nothing,
    /// Select what it created, if anything.
    Select,
    /// Gather what it created; the whole gathered is selected once the queue is empty.
    Collect,
    /// A paste done in place: select what it made, then carry it from this point (world) at
    /// the cursor (§6 decies).
    Carry(DVec3),
    /// A drawing tool's: nothing selected, the quick tools' card offered on what it made
    /// (§6 terdecies); a new part's name asked (`Some(true)`) or offered by a toast
    /// (`Some(false)`, §6 quattuordecies).
    Card {
        ask_name: Option<bool>,
    },
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
    /// The job sent (kept for the journal: a failure's repro) and when.
    pub(super) sent: Job,
    pub(super) at: Instant,
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
                self.pending = Some(Pending { job: id, op, after, restore, sent: job.clone(), at: Instant::now() });
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
        let carried = matches!(after, After::Carry(_)).then(|| op.clone());
        let label = op.label();
        self.history.push(document, op);
        self.journal.log(Event::Applied { op: label, revision: self.history.revision() });
        for w in &outcome.welds {
            self.journal.log(Event::Welded { part: w.part.0, body: w.body.0, reference: w.reference.0, distance: w.distance });
        }
        for (solid, display) in displays {
            if let Err(e) = &display {
                self.notify(NoticeKind::Error, t!("notice.display_failed"), Some(e.clone()));
                let job = Job { inputs: vec![solid.clone()], tasks: vec![Task::Display { solid: 0, quality: Quality::Final }] };
                self.journal.fail(None, job, e.clone());
            }
            self.scene.insert(&solid, display);
        }
        // A paste carried at the cursor says it once, put down (§6 decies).
        if let Some(w) = outcome.warning.filter(|_| !matches!(after, After::Carry(_))) {
            self.notify(NoticeKind::Warning, w.to_string(), None);
        }
        match after {
            After::Select | After::Carry(_) if !outcome.created.is_empty() => self.selection = outcome.created.clone(),
            // Machinings made in an open part: selected while they are shown.
            After::Select | After::Carry(_) if !outcome.bodies.is_empty() => {
                self.selection = if self.showing_machinings() { outcome.bodies.clone() } else { Vec::new() };
            }
            After::Collect => self.collected.extend(outcome.created.iter().copied()),
            _ => {}
        }
        self.after_change();
        if let After::Card { ask_name } = after {
            self.offer_card(&outcome);
            if let Some(ask) = ask_name {
                self.offer_naming(&outcome, ask);
            }
        }
        if let (After::Carry(from), Some(op)) = (after, carried) {
            self.carry(from, op);
        }
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
        self.journal.log(Event::Failed { op: op.label(), error: why.clone() });
        let msg = t!("status.op_failed", op = op.label(), error = why);
        if restore && self.history.redo() {
            self.after_change();
        }
        self.notify(NoticeKind::Error, t!("notice.op_failed", op = op.label()), Some(why));
        self.follow_cursor();
        msg
    }

    /// Takes in what the kernel finished: the pending operation, meshes. Call every frame;
    /// returns whether anything came.
    pub fn poll(&mut self) -> bool {
        // How far the file being read is (the load's window shows it).
        let mut moved = false;
        for (id, p) in self.kernel.progress() {
            if let Some(load) = &mut self.load
                && load.reading == Some(id)
            {
                load.view.reading = Some(p);
                moved = true;
            }
        }
        let results = self.kernel.poll();
        if results.is_empty() {
            return moved;
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
            } else if let Some((solid, quality)) = self.meshing.remove(&id) {
                if let Some(load) = &mut self.load
                    && load.meshes.remove(&id)
                {
                    match quality {
                        Quality::Preview => load.view.previews.0 += 1,
                        Quality::Final => load.view.finals.0 += 1,
                    }
                }
                if matches!(result, Err(KernelError::Cancelled)) {
                    continue;
                }
                let display = match result {
                    Ok(mut outputs) => match outputs.pop() {
                        Some(Output::Display(d)) => d,
                        _ => Err(t!("status.unexpected_reply")),
                    },
                    Err(e) => Err(e.to_string()),
                };
                if let Err(e) = &display {
                    let job = Job { inputs: vec![solid.clone()], tasks: vec![Task::Display { solid: 0, quality }] };
                    self.journal.log(Event::Job { what: JobKind::Mesh, ms: 0.0, error: Some(e.clone()) });
                    self.journal.fail(None, job, e.clone());
                }
                if quality == Quality::Final
                    && let Err(e) = &display
                {
                    self.notify(NoticeKind::Error, t!("notice.display_failed"), Some(e.clone()));
                }
                self.scene.insert_at(&solid, display, quality);
                meshed = true;
            } else if self.pending.as_ref().is_some_and(|p| p.job == id) {
                let Some(p) = self.pending.take() else { continue };
                self.journal_job(&p, &result);
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
        self.settle_load();
        true
    }

    /// Notes an operation's job back in the journal: its time, its error; a failure (an error,
    /// or bodies of a recipe skipped) with the job, to run again.
    fn journal_job(&mut self, p: &Pending, result: &solidsketch_model::kernel::JobResult) {
        let ms = p.at.elapsed().as_secs_f64() * 1000.0;
        let error = match result {
            Err(KernelError::Cancelled) => None,
            Err(e) => Some(e.to_string()),
            Ok(outputs) => {
                let failed: Vec<u32> = outputs
                    .iter()
                    .flat_map(|o| match o {
                        Output::Recipe { failed, .. } => failed.clone(),
                        _ => Vec::new(),
                    })
                    .collect();
                (!failed.is_empty()).then(|| format!("recipe: the bodies {failed:?} failed (skipped)"))
            }
        };
        self.journal.log(Event::Job { what: JobKind::Operation, ms, error: error.clone() });
        if let Some(e) = error {
            self.journal.fail(Some(p.op.clone()), p.sent.clone(), e);
        }
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
        // Stopped: no need to compute what nobody waits for.
        self.kernel.cancel(p.job);
        self.held_stretch = None;
        self.update_preview_mesh();
        self.queue.clear();
        self.collected.clear();
        if p.restore && self.history.redo() {
            self.after_change();
        }
        self.notify(NoticeKind::Info, t!("notice.abandoned", op = p.op.label()), None);
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
        // A load of its own (none under way: the app's window keeps another from starting).
        if self.load.is_none() {
            self.begin_load(super::LoadKind::Import { name: name.clone() });
        }
        if let Some(load) = &mut self.load {
            load.reading = Some(id);
            load.view.reading = Some(solidsketch_model::kernel::Progress::Reading);
        }
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
        // « Lecture de … » done: what came of it is a toast.
        self.message = None;
        if let Some(load) = &mut self.load {
            load.reading = None;
            load.view.reading = None;
        }
        let step = match result {
            Ok(mut outputs) => match outputs.pop() {
                Some(Output::Step(s)) => s,
                _ => {
                    self.notify(NoticeKind::Error, t!("notice.import_failed", file = name), Some(t!("status.unexpected_reply")));
                    return;
                }
            },
            Err(e) => {
                self.notify(NoticeKind::Error, t!("notice.import_failed", file = name), Some(e.to_string()));
                return;
            }
        };
        let (fragment, report) = Document::from_step(&step, name);
        if fragment.root_assembly().occurrences().is_empty() {
            let detail = (report.excluded > 0).then(|| solidsketch_i18n::tp!("notice.import_excluded", report.excluded));
            self.notify(NoticeKind::Error, t!("notice.import_empty", file = name), detail);
            return;
        }
        let context = self.document().context_of(&self.context).unwrap_or(self.document().root());
        let op = Operation::Insert { context, fragment: std::sync::Arc::new(fragment), transform: DAffine3::IDENTITY };
        self.queue.push_back((op, After::Select));
        let before = self.history.revision();
        self.pump();
        // Done at once (no kernel call): a cancelled load undoes it.
        if let Some(load) = &mut self.load
            && self.history.revision() != before
        {
            load.inserted = true;
        }
        if !self.busy() {
            self.zoom_extents();
        }
        let detail = (report.excluded > 0).then(|| solidsketch_i18n::tp!("notice.import_excluded", report.excluded));
        self.notify(NoticeKind::Success, solidsketch_i18n::tp!("notice.imported", report.parts, file = name), detail);
    }

    /// Are meshes asked of the kernel still on their way (or meshes missing, not asked yet)?
    pub fn meshing(&self) -> bool {
        !self.meshing.is_empty() || self.scene.has_missing()
    }

    /// Asks the kernel for the meshes the scene lacks, one job per solid. During a load, a
    /// preview of each first (all the previews asked before the final meshes: the kernel takes
    /// them in that order), counted in the load.
    pub(super) fn request_meshes(&mut self) {
        let missing = self.scene.take_missing();
        if missing.is_empty() {
            return;
        }
        let qualities: &[Quality] = if self.load.is_some() { &[Quality::Preview, Quality::Final] } else { &[Quality::Final] };
        for &quality in qualities {
            for solid in &missing {
                let id = self.job_id();
                self.meshing.insert(id, (solid.clone(), quality));
                if let Some(load) = &mut self.load {
                    load.meshes.insert(id);
                    match quality {
                        Quality::Preview => load.view.previews.1 += 1,
                        Quality::Final => load.view.finals.1 += 1,
                    }
                }
                self.kernel.submit(id, Job { inputs: vec![solid.clone()], tasks: vec![Task::Display { solid: 0, quality }] });
            }
        }
        // A kernel run inline has answered already.
        self.poll();
    }

    pub fn undo(&mut self) {
        if self.busy() {
            return;
        }
        self.note(Event::Undo, Some(Step::Undo));
        // A paste carried: undoing gives it up.
        if self.carrying() {
            self.settle_carry(true);
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
        self.note(Event::Redo, Some(Step::Redo));
        if self.carrying() {
            self.settle_carry(true);
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
        self.open_group.clear();
        self.selection.clear();
        self.tool.reset();
        self.after_change();
        self.zoom_extents();
        self.views.clear();
    }

    /// Keeps the context, the open part and the selection valid, and the scene in step.
    pub(super) fn after_change(&mut self) {
        let doc = self.history.document();
        while !self.context.is_empty() && doc.context_of(&self.context).is_err() {
            self.context.pop();
            self.open_part = None;
            self.open_group.clear();
        }
        let ctx = doc.context_of(&self.context).unwrap_or(doc.root());
        let open = self.open_part.and_then(|id| doc.resolve_part(&EntityPath::new(self.context.clone(), id)).ok());
        if open.is_none() {
            self.open_part = None;
        }
        // A group open no longer there (undone, ungrouped): closed, up to what still is.
        while !self.open_group.is_empty() && self.open_part_path().is_none_or(|p| doc.resolve_container(&p).is_err()) {
            self.open_group.pop();
        }
        if self.open_part.is_none() {
            self.open_group.clear();
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
                self.selection.retain(|id| (shown && p.body(*id).is_some()) || guide(id));
            }
            None => {
                if let Ok(a) = doc.assembly(ctx)
                    && !self.selection.is_empty()
                {
                    // By the assembly's index: the selection and the assembly can hold tens
                    // of thousands.
                    self.selection.retain(|id| a.occurrence(*id).is_some() || guide(id));
                }
            }
        }
        self.sync_scene();
    }
}

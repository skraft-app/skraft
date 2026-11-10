//! The document side of the editor: operations started, run by the kernel, finished or
//! abandoned; undo, redo, opening a file; the scene kept in step (docs/ARCHITECTURE.md §7 ter).

use glam::{DAffine3, DVec3};
use solidsketch_model::kernel::{Job, JobId, Output, Task};
use solidsketch_model::{Applied, Document, History, Operation, Outcome, Prepared, Primitive};

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
            let msg = "Un calcul est déjà en cours".to_owned();
            self.message = Some(msg.clone());
            return Err(msg);
        }
        match self.document().prepare(&op) {
            Err(e) => Err(self.fail(&op, e.to_string(), restore)),
            Ok(Prepared::Done(applied)) => Ok(Some(self.finish(op, applied, after))),
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
                self.message = Some(format!("Affichage impossible : {e}"));
            }
            self.scene.insert(&solid, display);
        }
        match after {
            After::Select if !outcome.created.is_empty() => self.selection = outcome.created.clone(),
            After::Collect => self.collected.extend(outcome.created.iter().copied()),
            _ => {}
        }
        self.after_change();
        outcome
    }

    /// Says why `op` failed; puts back the step it replaced. Returns the message.
    pub(super) fn fail(&mut self, op: &Operation, why: String, restore: bool) -> String {
        let msg = format!("{} : {why}", op.label());
        if restore && self.history.redo() {
            self.after_change();
        }
        self.message = Some(msg.clone());
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
            if let Some(probe) = self.probes.remove(&id) {
                self.probe_report.push(match (probe, &result) {
                    (Probe::Crash, Err(e)) => format!("Plantage provoqué : {e}"),
                    (Probe::Crash, Ok(_)) => "Plantage provoqué : le noyau n'a pas planté".into(),
                    (Probe::Next, Ok(_)) => "la tâche suivante a abouti".into(),
                    (Probe::Next, Err(e)) => format!("la tâche suivante a échoué : {e}"),
                });
                self.message = Some(self.probe_report.join(" — "));
                if self.probes.is_empty() {
                    self.probe_report.clear();
                }
            } else if let Some(solid) = self.meshing.remove(&id) {
                let display = match result {
                    Ok(mut outputs) => match outputs.pop() {
                        Some(Output::Display(d)) => d,
                        _ => Err("réponse inattendue du noyau".into()),
                    },
                    Err(e) => Err(e.to_string()),
                };
                if let Err(e) = &display {
                    self.message = Some(format!("Affichage impossible : {e}"));
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
            }
            // Anything else was abandoned: dropped.
        }
        if meshed {
            self.scene.sync(self.history.document());
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
        self.queue.clear();
        self.collected.clear();
        if p.restore && self.history.redo() {
            self.after_change();
        }
        self.message = Some(format!("{} : abandonné", p.op.label()));
    }

    /// Débogage: makes the kernel panic on purpose, a job queued right behind, to check that a
    /// crash is recovered from (docs/ARCHITECTURE.md §7 ter): natively the panic is caught; in
    /// the browser the worker is replaced and the job behind sent to the new one. The status
    /// bar says how both went.
    pub fn crash_kernel(&mut self) {
        let crash = self.job_id();
        self.probes.insert(crash, Probe::Crash);
        self.kernel.submit(crash, Job { inputs: Vec::new(), tasks: vec![Task::Panic { message: "plantage voulu (menu Débogage)".into() }] });
        let next = self.job_id();
        self.probes.insert(next, Probe::Next);
        let sphere = Primitive::Sphere { center: DVec3::ZERO, radius: 10.0 };
        self.kernel.submit(next, Job { inputs: Vec::new(), tasks: vec![Task::Primitive { primitive: sphere, placement: DAffine3::IDENTITY }] });
        self.message = Some("Plantage du noyau demandé…".into());
        // A kernel run inline has answered already.
        self.poll();
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
        // What the kernel was doing for the previous document is dropped when it comes.
        self.pending = None;
        self.queue.clear();
        self.collected.clear();
        self.context.clear();
        self.selection.clear();
        self.tool.reset();
        self.after_change();
        self.zoom_extents();
    }

    /// Keeps the context and the selection valid, and the scene in step.
    pub(super) fn after_change(&mut self) {
        let doc = self.history.document();
        while !self.context.is_empty() && doc.context_of(&self.context).is_err() {
            self.context.pop();
        }
        let ctx = doc.context_of(&self.context).unwrap_or(doc.root());
        if let Ok(a) = doc.assembly(ctx) {
            self.selection.retain(|id| a.occurrence(*id).is_some());
        }
        self.scene.sync(doc);
        self.request_meshes();
    }
}

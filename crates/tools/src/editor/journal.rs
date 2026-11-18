//! What the editor went through, for whoever debugs it (docs/ARCHITECTURE.md §7 septies): a
//! journal of its inputs, tools, operations and kernel jobs; the kernel's failures, each with
//! the job that failed (its input solids and tasks: run again by `kernel::run`, it makes a
//! headless test); and a recording of the inputs from a document and a camera, which a test
//! plays again.
//!
//! Always kept, bounded: a ring of the last [`JOURNAL_LEN`] entries (hovers in a row are one
//! entry), the last [`FAILURES_LEN`] failures.

use std::collections::VecDeque;
use std::sync::Arc;

use glam::DVec2;
use solidsketch_model::Operation;
use solidsketch_model::kernel::Job;
use web_time::Instant;

use crate::camera::Camera;
use crate::tools::{OptionValue, ToolInput, ToolKind};

/// The journal's entries kept.
pub const JOURNAL_LEN: usize = 1000;
/// The failures kept.
pub const FAILURES_LEN: usize = 20;

/// Something the editor went through.
#[derive(Clone, Debug)]
pub enum Event {
    /// An input for the active tool. Hovers in a row are one entry, the last one, `count` of
    /// them.
    Input {
        input: ToolInput,
        count: u32,
    },
    Tool(ToolKind),
    Option {
        key: String,
        value: OptionValue,
    },
    /// An operation done: the document at `revision`.
    Applied {
        op: String,
        revision: u64,
    },
    /// A face put onto another after the operation before (docs/ARCHITECTURE.md §2.11's
    /// welding): a face of the body `body` of the part `part` moved by `distance` (mm) onto one
    /// of the body `reference`.
    Welded {
        part: u64,
        body: u64,
        reference: u64,
        distance: f64,
    },
    /// An operation refused, or failed in the kernel.
    Failed {
        op: String,
        error: String,
    },
    /// A kernel job back (an operation's, a mesh's, a file's): how long it took, its error.
    Job {
        what: JobKind,
        ms: f64,
        error: Option<String>,
    },
    Undo,
    Redo,
    /// The oldest states of the undo history (or the last ones to redo) forgotten to keep the
    /// memory the histories hold under the budget (docs/ARCHITECTURE.md §5 « Memory cap »):
    /// how many, what they held (bytes, estimated).
    Forgotten {
        steps: u32,
        bytes: u64,
    },
}

/// What a kernel job was for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Operation,
    Mesh,
}

/// Who caused an entry: the person at the window, or a client of the control channel (an
/// agent, a script: docs/ARCHITECTURE.md §7 septies), named by itself if it said (the
/// `X-SolidSketch-Client` header).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    #[default]
    User,
    Control {
        client: Option<Arc<str>>,
    },
}

impl Origin {
    pub fn control(client: Option<&str>) -> Self {
        Origin::Control { client: client.map(Arc::from) }
    }

    pub fn is_control(&self) -> bool {
        matches!(self, Origin::Control { .. })
    }

    /// The client's name, if it gave one.
    pub fn client(&self) -> Option<&str> {
        match self {
            Origin::Control { client } => client.as_deref(),
            Origin::User => None,
        }
    }
}

/// One entry: its number (from 1, never reused), its time since the editor started (ms), who
/// caused it (an operation's outcome, computed later: who asked for it).
#[derive(Clone, Debug)]
pub struct Entry {
    pub seq: u64,
    pub ms: f64,
    pub event: Event,
    pub origin: Origin,
}

/// A kernel job that failed: what it was for, the job, the error.
#[derive(Clone, Debug)]
pub struct Failure {
    pub seq: u64,
    pub ms: f64,
    /// The operation it computed (none for a mesh).
    pub op: Option<Operation>,
    pub job: Job,
    pub error: String,
}

/// A step of a recording.
#[derive(Clone, Debug)]
pub enum Step {
    /// The camera or the viewport changed since the last step (navigation): set before the
    /// inputs that follow, whose pixels are its.
    View {
        camera: Camera,
        size: DVec2,
    },
    Input(ToolInput),
    Tool(ToolKind),
    Option {
        key: String,
        value: OptionValue,
    },
    Shift(bool),
    Copy(bool),
    Alt(bool),
    Undo,
    Redo,
}

/// The inputs from a document and a view on.
#[derive(Clone, Debug)]
pub struct Recording {
    /// The document as it was, in its file's JSON.
    pub document: String,
    /// The tool active then.
    pub tool: ToolKind,
    pub steps: Vec<Step>,
}

pub struct Journal {
    start: Instant,
    next: u64,
    entries: VecDeque<Entry>,
    failures: VecDeque<Failure>,
    recording: Option<Recording>,
    /// The view the recording's inputs were given in last.
    recorded_view: Option<(Camera, DVec2)>,
    /// Who causes the entries logged now.
    origin: Origin,
}

impl Default for Journal {
    fn default() -> Self {
        Journal {
            start: Instant::now(),
            next: 1,
            entries: VecDeque::new(),
            failures: VecDeque::new(),
            recording: None,
            recorded_view: None,
            origin: Origin::User,
        }
    }
}

impl Journal {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    /// Who causes the entries logged from now on.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    pub(super) fn set_origin(&mut self, origin: Origin) {
        self.origin = origin;
    }

    pub(super) fn log(&mut self, event: Event) {
        // A hover after a hover, from the same origin: the same entry, moved.
        if let Event::Input { input: input @ ToolInput::Hover { .. }, .. } = &event
            && let Some(Entry { event: Event::Input { input: last @ ToolInput::Hover { .. }, count }, ms, origin, .. }) =
                self.entries.back_mut()
            && *origin == self.origin
        {
            *last = input.clone();
            *count += 1;
            *ms = self.start.elapsed().as_secs_f64() * 1000.0;
            return;
        }
        let entry = Entry { seq: self.next, ms: self.now(), event, origin: self.origin.clone() };
        self.next += 1;
        if self.entries.len() == JOURNAL_LEN {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub(super) fn fail(&mut self, op: Option<Operation>, job: Job, error: String) {
        let failure = Failure { seq: self.next, ms: self.now(), op, job, error };
        if self.failures.len() == FAILURES_LEN {
            self.failures.pop_front();
        }
        self.failures.push_back(failure);
    }

    /// The entries numbered after `since`, oldest first.
    pub fn since(&self, since: u64) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(move |e| e.seq > since)
    }

    /// The number the next entry takes.
    pub fn next_seq(&self) -> u64 {
        self.next
    }

    pub fn failures(&self) -> impl Iterator<Item = &Failure> {
        self.failures.iter()
    }

    pub fn recording(&self) -> Option<&Recording> {
        self.recording.as_ref()
    }

    pub(super) fn start_recording(&mut self, document: String, tool: ToolKind) {
        self.recording = Some(Recording { document, tool, steps: Vec::new() });
        self.recorded_view = None;
    }

    pub(super) fn stop_recording(&mut self) -> Option<Recording> {
        self.recorded_view = None;
        self.recording.take()
    }

    /// Records `step`, given in the view `camera`, `size` (set first when it changed).
    pub(super) fn record(&mut self, step: Step, camera: &Camera, size: DVec2) {
        let Some(r) = &mut self.recording else { return };
        if matches!(step, Step::Input(_)) && self.recorded_view.as_ref() != Some(&(camera.clone(), size)) {
            r.steps.push(Step::View { camera: camera.clone(), size });
            self.recorded_view = Some((camera.clone(), size));
        }
        // A hover right after a hover (the view the same): only the last one matters.
        if let (Step::Input(ToolInput::Hover { .. }), Some(Step::Input(ToolInput::Hover { .. }))) = (&step, r.steps.last()) {
            r.steps.pop();
        }
        r.steps.push(step);
    }

    pub(super) fn is_recording(&self) -> bool {
        self.recording.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hovers_in_a_row_are_one_entry_and_the_ring_is_bounded() {
        let mut j = Journal::default();
        for k in 0..5 {
            j.log(Event::Input { input: ToolInput::Hover { px: DVec2::splat(k as f64) }, count: 1 });
        }
        j.log(Event::Undo);
        j.log(Event::Input { input: ToolInput::Hover { px: DVec2::ZERO }, count: 1 });
        let all: Vec<_> = j.since(0).collect();
        assert_eq!(all.len(), 3);
        assert!(matches!(&all[0].event, Event::Input { input: ToolInput::Hover { px }, count: 5 } if px.x == 4.0));
        assert_eq!(j.since(all[1].seq).count(), 1);
        for _ in 0..JOURNAL_LEN + 10 {
            j.log(Event::Redo);
        }
        assert_eq!(j.since(0).count(), JOURNAL_LEN);
    }

    #[test]
    fn entries_say_who_caused_them_and_hovers_merge_only_from_one_origin() {
        let mut j = Journal::default();
        let hover = || Event::Input { input: ToolInput::Hover { px: DVec2::ZERO }, count: 1 };
        j.log(hover());
        j.set_origin(Origin::control(Some("mcp")));
        j.log(hover());
        j.log(hover());
        j.set_origin(Origin::User);
        j.log(Event::Undo);
        let all: Vec<_> = j.since(0).collect();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].origin, Origin::User);
        assert_eq!(all[1].origin.client(), Some("mcp"));
        assert!(matches!(all[1].event, Event::Input { count: 2, .. }));
        assert_eq!(all[2].origin, Origin::User);
    }
}

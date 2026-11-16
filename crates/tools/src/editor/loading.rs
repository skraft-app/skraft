//! Loading a model (docs/ARCHITECTURE.md §7 sexies): a file opened, or a STEP file
//! imported, is read, then shown at once by coarse preview meshes, then by the final ones. The
//! app shows a window over it all, with how far each stage is and a button that stops it.

use std::collections::HashSet;

use solidsketch_i18n::t;
use solidsketch_model::kernel::{JobId, Progress};

use super::Editor;

/// What is being loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadKind {
    /// The document this editor opened.
    Open,
    /// A STEP file (its name) read into the document.
    Import { name: String },
}

/// How far a load is, for its window.
#[derive(Clone, Debug, PartialEq)]
pub struct Loading {
    pub kind: LoadKind,
    /// The file being read (an import), and how far, until the kernel has read it.
    pub reading: Option<Progress>,
    /// Preview meshes: done, asked.
    pub previews: (usize, usize),
    /// Final meshes: done, asked.
    pub finals: (usize, usize),
}

/// How a load ended (taken by the app: [`Editor::take_load_end`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadEnd {
    Done,
    /// Stopped by the user. An import is undone; an opened file is the app's to close.
    Cancelled(LoadKind),
}

/// A load under way: what the window shows, and the jobs it waits for.
pub(super) struct Load {
    pub(super) view: Loading,
    /// The kernel reading the file (an import).
    pub(super) reading: Option<JobId>,
    /// Its meshes still out.
    pub(super) meshes: HashSet<JobId>,
    /// The import's insertion is in the history (undone if the load is cancelled).
    pub(super) inserted: bool,
}

impl Load {
    pub(super) fn new(kind: LoadKind) -> Load {
        Load {
            view: Loading { kind, reading: None, previews: (0, 0), finals: (0, 0) },
            reading: None,
            meshes: HashSet::new(),
            inserted: false,
        }
    }
}

impl Editor {
    /// The load under way, if any.
    pub fn loading(&self) -> Option<&Loading> {
        self.load.as_ref().map(|l| &l.view)
    }

    /// How the last load ended, once: the app closes an opened file whose load was cancelled.
    pub fn take_load_end(&mut self) -> Option<LoadEnd> {
        self.load_end.take()
    }

    /// Stops the load under way: the kernel's jobs for it are cancelled (the file's reading,
    /// its meshes), an import undone. An opened file stays as it is, drawn by what it has: the
    /// app closes it ([`LoadEnd::Cancelled`] of [`LoadKind::Open`]).
    pub fn cancel_loading(&mut self) {
        let Some(load) = self.load.take() else { return };
        if let Some(id) = load.reading {
            self.importing.remove(&id);
            self.kernel.cancel(id);
        }
        for id in load.meshes {
            self.meshing.remove(&id);
            self.kernel.cancel(id);
        }
        if let LoadKind::Import { name } = &load.view.kind {
            if load.inserted && self.history.undo() {
                self.after_change();
            }
            self.message = None;
            let title = t!("notice.import_cancelled", file = name);
            self.notify(super::NoticeKind::Info, title, None);
        }
        self.load_end = Some(LoadEnd::Cancelled(load.view.kind));
    }

    /// Starts a load (none was under way).
    pub(super) fn begin_load(&mut self, kind: LoadKind) {
        self.load = Some(Load::new(kind));
    }

    /// Ends the load once nothing is left to wait for.
    pub(super) fn settle_load(&mut self) {
        // An import's insertion still computing: its meshes are not asked yet.
        if !self.busy() && self.load.as_ref().is_some_and(|l| l.reading.is_none() && l.meshes.is_empty()) {
            self.load = None;
            self.load_end = Some(LoadEnd::Done);
        }
    }
}

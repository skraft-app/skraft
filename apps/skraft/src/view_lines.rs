//! The lines of a drawing's views (docs/ui/drawing.md#lines), computed off the UI thread and
//! kept per view: SolidCore's exact hidden lines for a view of one part (native), else, or when
//! it refuses, the lines found on the display meshes (`skraft_tools::hidden_lines`). A view's
//! lines are asked for by what they depend on (its frame, its parts' meshes or shapes and
//! places): moving the view or changing its scale computes nothing.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::DAffine3;
use skraft_model::kernel::{Mesh, Solid};
use skraft_model::{Id, Resolved};
use skraft_tools::Editor;
use skraft_tools::hidden_lines::{Look, ViewLines};

/// What a view's lines are computed from.
#[derive(Clone)]
pub struct LinesJob {
    look: Look,
    /// Every part's meshes, part to the view's axes.
    meshes: Vec<(Arc<Mesh>, DAffine3)>,
    /// The part alone, when the view shows one: drawn exactly (natively).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    part: Option<(Arc<Solid>, DAffine3)>,
    stamp: u64,
}

impl LinesJob {
    /// The job of the view `r` showing `placed`; none while a part's mesh is not there yet.
    pub fn of(e: &Editor, r: &Resolved<'_>, placed: &[skraft_model::PlacedPart]) -> Option<LinesJob> {
        let mut meshes = Vec::new();
        for p in placed {
            let dm = e.scene().definition_mesh(p.part)?;
            meshes.extend(dm.parts.iter().map(|pm| (pm.mesh.clone(), p.transform)));
        }
        let part = match placed {
            [one] => Some((one.shape.clone(), one.transform)),
            _ => None,
        };
        let f = &r.frame;
        let look = Look { right: f.right, up: f.up, forward: f.forward };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for v in [f.right, f.up, f.forward] {
            v.to_array().map(f64::to_bits).hash(&mut h);
        }
        for (m, t) in &meshes {
            (Arc::as_ptr(m) as usize).hash(&mut h);
            t.to_cols_array().map(f64::to_bits).hash(&mut h);
        }
        if let Some((s, _)) = &part {
            (Arc::as_ptr(s) as usize).hash(&mut h);
        }
        Some(LinesJob { look, meshes, part, stamp: h.finish() })
    }

    /// The lines: exact for one part when SolidCore draws it, else from the meshes.
    pub fn run(self) -> ViewLines {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((solid, place)) = &self.part {
            let l = &self.look;
            let exact = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                skraft_kernel::drawing::part_lines(solid, place, l.right, l.up, l.forward)
            }));
            if let Ok(Ok((visible, hidden))) = exact {
                return ViewLines { visible, hidden };
            }
        }
        skraft_tools::hidden_lines::hidden_lines(&self.meshes, &self.look)
    }
}

/// A document's views' lines: those done, by view (with the stamp they were done for), and
/// those being computed.
#[derive(Default)]
pub struct LinesCache {
    done: HashMap<Id, (u64, Arc<ViewLines>)>,
    #[cfg(not(target_arch = "wasm32"))]
    running: HashMap<Id, (u64, std::sync::mpsc::Receiver<ViewLines>)>,
}

impl std::fmt::Debug for LinesCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinesCache").field("done", &self.done.len()).finish()
    }
}

impl LinesCache {
    /// The lines of `view` as last computed (perhaps for an older stamp, while new ones are on
    /// their way); `job` computed when its stamp is new. Is anything still being computed?
    pub fn get(&mut self, view: Id, job: Option<&LinesJob>) -> (Option<Arc<ViewLines>>, bool) {
        self.collect();
        let Some(job) = job else { return (self.done.get(&view).map(|d| d.1.clone()), false) };
        let stamp = job.stamp;
        if self.done.get(&view).is_some_and(|d| d.0 == stamp) {
            return (self.done.get(&view).map(|d| d.1.clone()), false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.running.get(&view).is_none_or(|r| r.0 != stamp) {
                let job = job.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(job.run());
                });
                self.running.insert(view, (stamp, rx));
            }
            (self.done.get(&view).map(|d| d.1.clone()), true)
        }
        #[cfg(target_arch = "wasm32")]
        {
            // No threads: on the UI thread.
            let lines = Arc::new(job.clone().run());
            self.done.insert(view, (stamp, lines.clone()));
            (Some(lines), false)
        }
    }

    /// Takes in what the threads finished.
    fn collect(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.running.retain(|view, (stamp, rx)| match rx.try_recv() {
            Ok(lines) => {
                self.done.insert(*view, (*stamp, Arc::new(lines)));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
    }

    /// Forgets the views not in `kept`.
    pub fn retain(&mut self, kept: &[Id]) {
        self.done.retain(|v, _| kept.contains(v));
        #[cfg(not(target_arch = "wasm32"))]
        self.running.retain(|v, _| kept.contains(v));
    }
}

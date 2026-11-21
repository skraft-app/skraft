//! The lines of a drawing's views (docs/ui/drawing.md#lines), computed off the UI thread through
//! a queue, the active sheet's first, and kept per view slot: SolidCore's exact hidden lines for
//! a view of one part (native), else, or when it refuses, the lines found on the display meshes
//! (`skraft_tools::hidden_lines`). A view's lines are asked for by what they depend on (its
//! frame, its parts' meshes or shapes and places): moving the view or changing its scale
//! computes nothing.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicUsize, Ordering};

use glam::DAffine3;
use skraft_model::kernel::{Mesh, Solid};
use skraft_model::{Id, Resolved};
use skraft_tools::Editor;
use skraft_tools::hidden_lines::{Look, ViewLines};

use crate::sheet::ViewSlot;

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
    pub fn run(&self) -> ViewLines {
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

/// A view's lines asked for this frame; `first`: on the active sheet.
struct Asked {
    slot: ViewSlot,
    job: Arc<LinesJob>,
    first: bool,
}

/// A document's views' lines: those done, by view slot (with the stamp they were done for), those
/// asked for this frame, and those being computed.
#[derive(Default)]
pub struct LinesCache {
    done: HashMap<ViewSlot, (u64, Arc<ViewLines>)>,
    asked: Vec<Asked>,
    #[cfg(not(target_arch = "wasm32"))]
    running: HashMap<ViewSlot, (u64, std::sync::mpsc::Receiver<ViewLines>)>,
    /// The threads alive, those whose result is no longer wanted included.
    #[cfg(not(target_arch = "wasm32"))]
    live: Arc<AtomicUsize>,
}

/// How many views' lines are computed at once: half the machine, the rest for the UI and the
/// kernel.
#[cfg(not(target_arch = "wasm32"))]
fn max_jobs() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get() / 2).clamp(1, 6)
}

impl std::fmt::Debug for LinesCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinesCache").field("done", &self.done.len()).finish()
    }
}

impl LinesCache {
    /// The lines of `slot` as last computed (perhaps for an older stamp, while new ones are on
    /// their way); `job` asked for when its stamp is new, `first` before the others. Are its
    /// lines still to come?
    pub fn get(&mut self, slot: ViewSlot, job: Option<&Arc<LinesJob>>, first: bool) -> (Option<Arc<ViewLines>>, bool) {
        let last = self.done.get(&slot).map(|d| d.1.clone());
        let Some(job) = job else { return (last, false) };
        if self.done.get(&slot).is_some_and(|d| d.0 == job.stamp) {
            return (last, false);
        }
        if !self.is_running(slot, job.stamp) {
            self.asked.push(Asked { slot, job: job.clone(), first });
        }
        (last, true)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn is_running(&self, slot: ViewSlot, stamp: u64) -> bool {
        self.running.get(&slot).is_some_and(|r| r.0 == stamp)
    }

    #[cfg(target_arch = "wasm32")]
    fn is_running(&self, _: ViewSlot, _: u64) -> bool {
        false
    }

    /// Takes in what finished and starts what this frame asked for, the active sheet's first, as
    /// many as may run; the rest is asked for again by the next frame, if still on screen.
    pub fn start(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.start_up_to(max_jobs());
        #[cfg(target_arch = "wasm32")]
        {
            // No threads: one a frame, on the UI thread.
            if let Some(a) = self.next_asked().into_iter().next() {
                let lines = Arc::new(a.job.run());
                self.done.insert(a.slot, (a.job.stamp, lines));
            }
        }
    }

    /// What this frame asked for, once each, the active sheet's first.
    fn next_asked(&mut self) -> Vec<Asked> {
        let mut asked = std::mem::take(&mut self.asked);
        asked.sort_by_key(|a| !a.first);
        let mut seen = HashSet::new();
        asked.retain(|a| seen.insert(a.slot));
        asked
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_up_to(&mut self, limit: usize) {
        self.collect();
        for a in self.next_asked() {
            if self.live.load(Ordering::Relaxed) >= limit {
                break;
            }
            /// Counts its thread out however it ends.
            struct Alive(Arc<AtomicUsize>);
            impl Drop for Alive {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::Relaxed);
                }
            }
            self.live.fetch_add(1, Ordering::Relaxed);
            let alive = Alive(self.live.clone());
            let (tx, rx) = std::sync::mpsc::channel();
            let (stamp, job) = (a.job.stamp, a.job);
            std::thread::spawn(move || {
                let _alive = alive;
                let _ = tx.send(job.run());
            });
            self.running.insert(a.slot, (stamp, rx));
        }
    }

    /// Takes in what the threads finished; a job that died counts as done, without lines, or it
    /// would be started again at every frame.
    #[cfg(not(target_arch = "wasm32"))]
    fn collect(&mut self) {
        self.running.retain(|slot, (stamp, rx)| match rx.try_recv() {
            Ok(lines) => {
                self.done.insert(*slot, (*stamp, Arc::new(lines)));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.done.insert(*slot, (*stamp, Arc::default()));
                false
            }
        });
    }

    /// Forgets the views not in `kept`.
    pub fn retain(&mut self, kept: &HashSet<Id>) {
        self.done.retain(|(v, _), _| kept.contains(v));
        #[cfg(not(target_arch = "wasm32"))]
        self.running.retain(|(v, _), _| kept.contains(v));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use glam::DVec3;

    use super::*;

    fn job(stamp: u64) -> Arc<LinesJob> {
        let look = Look { right: DVec3::X, up: DVec3::Y, forward: DVec3::Z };
        Arc::new(LinesJob { look, meshes: Vec::new(), part: None, stamp })
    }

    fn wait_for(c: &mut LinesCache, slot: ViewSlot) {
        for _ in 0..2000 {
            c.collect();
            if c.done.contains_key(&slot) && c.live.load(Ordering::Relaxed) == 0 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("its lines never came");
    }

    #[test]
    fn the_active_sheets_views_start_first_and_no_more_than_allowed() {
        let mut c = LinesCache::default();
        let (other, active) = ((Id(1), None), (Id(2), None));
        assert!(c.get(other, Some(&job(1)), false).1);
        assert!(c.get(active, Some(&job(2)), true).1);
        c.start_up_to(1);
        assert!(c.running.contains_key(&active) || c.done.contains_key(&active), "the active sheet's first");
        assert!(!c.running.contains_key(&other), "one at a time");
        assert!(c.asked.is_empty(), "the next frame asks again");
        wait_for(&mut c, active);
        assert!(!c.get(active, Some(&job(2)), true).1, "done");
        // The other one's turn.
        assert!(c.get(other, Some(&job(1)), false).1);
        c.start_up_to(1);
        wait_for(&mut c, other);
        assert!(!c.get(other, Some(&job(1)), false).1);
    }

    #[test]
    fn two_projections_of_a_view_keep_their_own_lines() {
        use skraft_model::Projection;
        let mut c = LinesCache::default();
        let (a, b) = ((Id(1), Some(Projection::FirstAngle)), (Id(1), Some(Projection::ThirdAngle)));
        c.get(a, Some(&job(1)), true);
        c.start_up_to(4);
        wait_for(&mut c, a);
        // The other's stamp neither replaces nor is answered by it.
        assert!(c.get(b, Some(&job(2)), true).0.is_none());
        c.start_up_to(4);
        wait_for(&mut c, b);
        assert!(!c.get(a, Some(&job(1)), true).1, "still done");
        assert!(!c.get(b, Some(&job(2)), true).1, "still done");
    }

    #[test]
    fn a_view_asked_twice_in_a_frame_starts_once_as_the_active_one() {
        let mut c = LinesCache::default();
        let slot = (Id(3), None);
        c.get(slot, Some(&job(1)), false);
        c.get(slot, Some(&job(1)), true);
        let asked = c.next_asked();
        assert_eq!(asked.len(), 1);
        assert!(asked[0].first);
    }
}

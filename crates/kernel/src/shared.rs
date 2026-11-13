//! One kernel service for several documents (docs/ARCHITECTURE.md §6 octies): each document's
//! editor gets a [`KernelHandle`], itself a [`KernelService`], over one [`SharedKernel`] — one
//! thread on the desktop, one web worker (one SolidCore wasm) in the browser.
//!
//! A handle's jobs are given ids of the shared service's own, and their results come back to
//! the handle that submitted them, with its ids: each editor sees a service of its own. The
//! service's results are collected by whichever handle polls first and wait in the others'
//! mailboxes. A handle dropped (its document closed) gets nothing more: results still on their
//! way are dropped when they come.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::job::{Job, JobId, JobResult, KernelService};

/// The shared service; [`SharedKernel::handle`] gives each document its own.
#[derive(Clone)]
pub struct SharedKernel {
    mux: Rc<RefCell<Mux>>,
}

/// A document's view of the [`SharedKernel`].
pub struct KernelHandle {
    mux: Rc<RefCell<Mux>>,
    channel: u64,
}

struct Mux {
    service: Box<dyn KernelService>,
    /// The next id given to the service.
    next_id: JobId,
    next_channel: u64,
    /// A job in flight: its handle and the id the handle gave it.
    routes: HashMap<JobId, (u64, JobId)>,
    /// The results waiting for each live handle.
    mailboxes: HashMap<u64, Vec<(JobId, JobResult)>>,
}

impl SharedKernel {
    pub fn new(service: Box<dyn KernelService>) -> Self {
        SharedKernel {
            mux: Rc::new(RefCell::new(Mux { service, next_id: 1, next_channel: 0, routes: HashMap::new(), mailboxes: HashMap::new() })),
        }
    }

    /// A new handle, with an empty mailbox.
    pub fn handle(&self) -> KernelHandle {
        let mut mux = self.mux.borrow_mut();
        let channel = mux.next_channel;
        mux.next_channel += 1;
        mux.mailboxes.insert(channel, Vec::new());
        KernelHandle { mux: self.mux.clone(), channel }
    }

    /// Jobs in flight, all handles together.
    pub fn in_flight(&self) -> usize {
        self.mux.borrow().routes.len()
    }
}

impl Mux {
    /// Takes the service's results to their handles' mailboxes.
    fn pump(&mut self) {
        for (id, result) in self.service.poll() {
            let Some((channel, theirs)) = self.routes.remove(&id) else { continue };
            if let Some(mailbox) = self.mailboxes.get_mut(&channel) {
                mailbox.push((theirs, result));
            }
        }
    }
}

impl KernelService for KernelHandle {
    fn submit(&mut self, id: JobId, job: Job) {
        let mut mux = self.mux.borrow_mut();
        let ours = mux.next_id;
        mux.next_id += 1;
        mux.routes.insert(ours, (self.channel, id));
        mux.service.submit(ours, job);
    }

    fn poll(&mut self) -> Vec<(JobId, JobResult)> {
        let mut mux = self.mux.borrow_mut();
        mux.pump();
        mux.mailboxes.get_mut(&self.channel).map(std::mem::take).unwrap_or_default()
    }
}

impl Drop for KernelHandle {
    fn drop(&mut self) {
        // Its jobs still in flight are forgotten: their results are dropped when they come.
        let mut mux = self.mux.borrow_mut();
        let channel = self.channel;
        mux.mailboxes.remove(&channel);
        mux.routes.retain(|_, (c, _)| *c != channel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers each job with no output at the next poll, in reverse
    /// order (results may come in any order).
    #[derive(Default)]
    struct Echo {
        ready: Vec<JobId>,
    }

    impl KernelService for Echo {
        fn submit(&mut self, id: JobId, _job: Job) {
            self.ready.push(id);
        }

        fn poll(&mut self) -> Vec<(JobId, JobResult)> {
            std::mem::take(&mut self.ready).into_iter().rev().map(|id| (id, Ok(Vec::new()))).collect()
        }
    }

    fn ids(mut v: Vec<(JobId, JobResult)>) -> Vec<JobId> {
        v.sort_by_key(|(id, _)| *id);
        v.into_iter().map(|(id, _)| id).collect()
    }

    #[test]
    fn each_handle_gets_its_own_results_with_its_own_ids() {
        let shared = SharedKernel::new(Box::new(Echo::default()));
        let (mut a, mut b) = (shared.handle(), shared.handle());
        // Both number their jobs from 1, as two editors do.
        a.submit(1, Job::default());
        b.submit(1, Job::default());
        a.submit(2, Job::default());
        assert_eq!(shared.in_flight(), 3);
        // `b` polls first: `a`'s results wait for it.
        assert_eq!(ids(b.poll()), [1]);
        assert_eq!(ids(a.poll()), [1, 2]);
        assert!(a.poll().is_empty() && b.poll().is_empty());
        assert_eq!(shared.in_flight(), 0);
    }

    #[test]
    fn a_dropped_handle_s_results_are_dropped() {
        let shared = SharedKernel::new(Box::new(Echo::default()));
        let (mut a, mut b) = (shared.handle(), shared.handle());
        a.submit(1, Job::default());
        b.submit(1, Job::default());
        drop(a);
        assert_eq!(shared.in_flight(), 1);
        assert_eq!(ids(b.poll()), [1]);
        assert_eq!(shared.in_flight(), 0);
    }
}

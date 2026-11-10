//! The kernel in a web worker (docs/ARCHITECTURE.md §7 ter): the browser's [`KernelService`].
//!
//! Jobs go to `kernel-worker.js` (which runs the `solidsketch-kernel-worker` wasm) as
//! MessagePack bytes in a transferred `ArrayBuffer`; results come back the same way and wait
//! in [`WebKernel::poll`]. The worker runs them one at a time, in order.
//!
//! A kernel panic aborts the worker's wasm: its `{ panic }` message, then `{ id, crashed }`,
//! come back. That job fails, the worker is replaced and the jobs it still had are sent to the
//! new one. If the kernel cannot load at all (`{ fatal }`), every job fails from then on.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use solidsketch_i18n::t;
use solidsketch_model::kernel::{Error, Job, JobId, JobResult, KernelService, decode, encode};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// The worker's script, next to `index.html`.
const SCRIPT: &str = "./kernel-worker.js";

pub struct WebKernel {
    inner: Rc<RefCell<Inner>>,
}

type Listeners = (Option<Closure<dyn FnMut(web_sys::MessageEvent)>>, Option<Closure<dyn FnMut(web_sys::ErrorEvent)>>);

struct Inner {
    worker: Option<web_sys::Worker>,
    /// Kept alive as long as the worker they listen to.
    on_message: Option<Closure<dyn FnMut(web_sys::MessageEvent)>>,
    on_error: Option<Closure<dyn FnMut(web_sys::ErrorEvent)>>,
    /// The previous worker's listeners: a crash is handled inside its `on_message`, which must
    /// not be dropped while it runs. Dropped at the next replacement.
    retired: Vec<Listeners>,
    /// Jobs sent and not answered, as sent (to send again to a new worker).
    in_flight: BTreeMap<JobId, Vec<u8>>,
    ready: Vec<(JobId, JobResult)>,
    /// What the last panic said (it comes just before the crash it explains).
    panic: Option<String>,
    /// Why the kernel is unavailable for good.
    dead: Option<String>,
    wake: Rc<dyn Fn()>,
}

impl WebKernel {
    /// `wake` is called when a result is ready (e.g. to repaint).
    pub fn new(wake: Rc<dyn Fn()>) -> Self {
        let inner = Rc::new(RefCell::new(Inner {
            worker: None,
            on_message: None,
            on_error: None,
            retired: Vec::new(),
            in_flight: BTreeMap::new(),
            ready: Vec::new(),
            panic: None,
            dead: None,
            wake,
        }));
        spawn(&inner);
        WebKernel { inner }
    }
}

impl KernelService for WebKernel {
    fn submit(&mut self, id: JobId, job: Job) {
        let mut inner = self.inner.borrow_mut();
        if let Some(why) = &inner.dead {
            let why = why.clone();
            inner.ready.push((id, Err(Error::Crashed(why))));
            return;
        }
        match encode(&job) {
            Ok(bytes) => {
                inner.post(id, &bytes);
                inner.in_flight.insert(id, bytes);
            }
            Err(e) => inner.ready.push((id, Err(Error::Crashed(t!("error.worker.send", error = e))))),
        }
    }

    fn poll(&mut self) -> Vec<(JobId, JobResult)> {
        std::mem::take(&mut self.inner.borrow_mut().ready)
    }
}

impl Drop for WebKernel {
    fn drop(&mut self) {
        if let Some(w) = self.inner.borrow_mut().worker.take() {
            w.terminate();
        }
    }
}

impl Inner {
    /// Sends a job: a copy of its bytes, whose buffer is transferred.
    fn post(&self, id: JobId, bytes: &[u8]) {
        let Some(worker) = &self.worker else { return };
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        let msg = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&msg, &"id".into(), &JsValue::from_f64(id as f64));
        let _ = js_sys::Reflect::set(&msg, &"bytes".into(), &buffer);
        if let Err(e) = worker.post_message_with_transfer(&msg, &js_sys::Array::of1(&buffer)) {
            log::error!("kernel worker: post failed: {e:?}");
        }
    }

    /// Every job in flight fails with `why`.
    fn fail_all(&mut self, why: &str) {
        for id in std::mem::take(&mut self.in_flight).into_keys() {
            self.ready.push((id, Err(Error::Crashed(why.to_owned()))));
        }
    }
}

/// Starts a worker (again), listening to it.
fn spawn(inner: &Rc<RefCell<Inner>>) {
    let opts = web_sys::WorkerOptions::new();
    opts.set_type(web_sys::WorkerType::Module);
    let worker = match web_sys::Worker::new_with_options(SCRIPT, &opts) {
        Ok(w) => w,
        Err(e) => {
            let mut i = inner.borrow_mut();
            let why = t!("error.worker.start", error = format!("{e:?}"));
            i.fail_all(&why);
            i.dead = Some(why);
            return;
        }
    };
    let weak = Rc::downgrade(inner);
    let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
        on_message(&weak, e.data());
    });
    let weak = Rc::downgrade(inner);
    let on_error = Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |e: web_sys::ErrorEvent| {
        // The script itself failed (not found, not a module…): no kernel.
        let Some(inner) = weak.upgrade() else { return };
        let why = t!("error.worker.failed", error = e.message());
        let wake = {
            let mut i = inner.borrow_mut();
            i.fail_all(&why);
            i.dead = Some(why);
            i.wake.clone()
        };
        wake();
    });
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    let mut i = inner.borrow_mut();
    let old = (i.on_message.replace(on_message), i.on_error.replace(on_error));
    i.retired.clear();
    i.retired.push(old);
    i.worker = Some(worker);
}

fn on_message(weak: &Weak<RefCell<Inner>>, data: JsValue) {
    let Some(inner) = weak.upgrade() else { return };
    let get = |k: &str| js_sys::Reflect::get(&data, &k.into()).ok().filter(|v| !v.is_undefined());
    let mut respawn = false;
    {
        let mut i = inner.borrow_mut();
        if let Some(p) = get("panic") {
            i.panic = p.as_string();
            return;
        }
        if let Some(f) = get("fatal") {
            let why = t!("error.worker.load", error = f.as_string().unwrap_or_default());
            log::error!("{why}");
            i.fail_all(&why);
            i.dead = Some(why);
        } else if let Some(id) = get("id").and_then(|v| v.as_f64()).map(|v| v as JobId) {
            if i.in_flight.remove(&id).is_none() {
                return;
            }
            let result = if let Some(bytes) = get("bytes") {
                let bytes = js_sys::Uint8Array::new(&bytes).to_vec();
                decode::<JobResult>(&bytes).unwrap_or_else(|e| Err(Error::Crashed(t!("error.worker.unreadable_reply", error = e))))
            } else {
                let crashed = get("crashed").and_then(|v| v.as_string()).unwrap_or_default();
                let why = i.panic.take().unwrap_or(crashed);
                log::error!("kernel worker crashed on job {id}: {why}");
                respawn = true;
                Err(Error::Crashed(why))
            };
            i.ready.push((id, result));
        } else {
            return;
        }
    }
    if respawn {
        // The trapped wasm may be in any state: a new worker takes over what was left.
        if let Some(w) = inner.borrow_mut().worker.take() {
            w.terminate();
        }
        spawn(&inner);
        let i = inner.borrow();
        for (id, bytes) in &i.in_flight {
            i.post(*id, bytes);
        }
    }
    let wake = inner.borrow().wake.clone();
    wake();
}

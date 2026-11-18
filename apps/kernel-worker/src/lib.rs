//! The kernel's web worker (docs/ARCHITECTURE.md §7 ter): `web/kernel-worker.js` loads this
//! module and hands it each job the UI posts, as MessagePack bytes; the result goes back the
//! same way. Only the kernel is here: no UI, no GPU.
//!
//! A kernel panic cannot be caught in wasm (`panic = abort`): the panic hook first tells the UI
//! why, then the call traps; the UI drops this worker and starts another.

#[cfg(target_arch = "wasm32")]
mod web {
    use skraft_kernel::{Job, decode, encode};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(start)]
    pub fn start() {
        std::panic::set_hook(Box::new(|info| {
            let message = info.to_string();
            web_sys::console::error_1(&message.clone().into());
            // `{ panic }` reaches the UI before the trap's `{ id, crashed }`.
            if let Ok(scope) = js_sys::global().dyn_into::<web_sys::DedicatedWorkerGlobalScope>() {
                let o = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&o, &"panic".into(), &message.into());
                let _ = scope.post_message(&o);
            }
        }));
    }

    /// Runs job `id`: MessagePack `Job` in, MessagePack `JobResult` out. Its progress goes to
    /// the UI as it comes, `{ id, progress }` (a MessagePack `Progress`): the worker's messages
    /// leave while the job runs.
    #[wasm_bindgen]
    pub fn run(id: f64, bytes: &[u8]) -> Result<Vec<u8>, JsError> {
        let job: Job = decode(bytes).map_err(|e| JsError::new(&format!("job: {e}")))?;
        // Only the id is held (JS values are not `Sync`): the worker's scope is looked up at
        // each report.
        let report = move |p: skraft_kernel::Progress| {
            let Ok(scope) = js_sys::global().dyn_into::<web_sys::DedicatedWorkerGlobalScope>() else { return };
            let Ok(bytes) = encode(&p) else { return };
            let o = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&o, &"id".into(), &id.into());
            let _ = js_sys::Reflect::set(&o, &"progress".into(), &js_sys::Uint8Array::from(bytes.as_slice()));
            let _ = scope.post_message(&o);
        };
        let cx = skraft_kernel::Context { report: &report, cancel: None };
        encode(&skraft_kernel::run_with(&job, &cx)).map_err(|e| JsError::new(&format!("result: {e}")))
    }
}

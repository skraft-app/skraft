//! The page's command API (docs/control-protocol.md): the control protocol's methods
//! reached from JavaScript in the tab, `window.skraft.call(method, params)` — a promise of
//! the method's result (an `Uint8Array` of PNG for a screenshot), rejected with an `Error`.
//!
//! No listener and no token: a script of the page already has the app's power, and reaches
//! nothing outside the tab. Each call is handed to the UI thread, which answers it at its next
//! frame, as the desktop's server does.

use std::cell::{Cell, RefCell};

use skraft_control::{Answer, Call};
use wasm_bindgen::prelude::*;

use crate::system::driving::Asker;

/// The calls waiting for a frame at most; beyond, refused.
const MAX_PENDING: usize = 256;

thread_local! {
    static INBOX: RefCell<Vec<Asker>> = const { RefCell::new(Vec::new()) };
    static CONTEXT: RefCell<Option<egui::Context>> = const { RefCell::new(None) };
    static USED: Cell<bool> = const { Cell::new(false) };
}

/// The app's context, to ask for a frame when a call comes.
pub fn set_context(ctx: egui::Context) {
    CONTEXT.with(|c| *c.borrow_mut() = Some(ctx));
}

/// The calls that came since the last frame.
pub fn take() -> Vec<Asker> {
    INBOX.with(|i| std::mem::take(&mut *i.borrow_mut()))
}

/// Has the page called the app yet (the named handles kept from then on)?
pub fn used() -> bool {
    USED.with(Cell::get)
}

/// `window.skraft.call(method, params)`: the method's result, at the app's next frame.
#[wasm_bindgen(js_name = skraftCall)]
pub fn call(method: String, params: JsValue) -> js_sys::Promise {
    let params = if params.is_null() || params.is_undefined() {
        serde_json::Value::Null
    } else {
        match js_sys::JSON::stringify(&params).ok().and_then(|s| s.as_string()).map(|s| serde_json::from_str(&s)) {
            Some(Ok(v)) => v,
            _ => return js_sys::Promise::reject(&js_sys::Error::new("the parameters are not JSON").into()),
        }
    };
    js_sys::Promise::new(&mut |resolve, reject| {
        if INBOX.with(|i| i.borrow().len()) >= MAX_PENDING {
            let _ = reject.call1(&JsValue::NULL, &js_sys::Error::new("too many calls waiting for the app").into());
            return;
        }
        let asker = Asker::new(Call::new(method.clone(), params.clone()), move |outcome| {
            let _ = match outcome {
                Ok(Answer::Json(v)) => resolve.call1(&JsValue::NULL, &js_sys::JSON::parse(&v.to_string()).unwrap_or(JsValue::NULL)),
                Ok(Answer::Png(bytes)) => resolve.call1(&JsValue::NULL, &js_sys::Uint8Array::from(&bytes[..]).into()),
                Err(e) => reject.call1(&JsValue::NULL, &js_sys::Error::new(&e.message()).into()),
            };
        });
        INBOX.with(|i| i.borrow_mut().push(asker));
        USED.with(|u| u.set(true));
        CONTEXT.with(|c| {
            if let Some(ctx) = &*c.borrow() {
                ctx.request_repaint();
            }
        });
    })
}

/// The page's colours around the app (`web/index.html`): the title bar's above it — the status bar
/// of a phone's web app too, its theme colour —, the panels' below and beside it, under the home
/// indicator (docs/ui/touch.md#safe-areas).
pub fn set_page_colors(top: egui::Color32, bottom: egui::Color32) {
    let hex = |c: egui::Color32| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    // The opaque status bar of a phone's web app takes the theme colour.
    if let Ok(Some(meta)) = document.query_selector("meta[name=theme-color]") {
        let _ = meta.set_attribute("content", &hex(top));
    }
    let Some(root) = document.document_element() else { return };
    let Ok(root) = root.dyn_into::<web_sys::HtmlElement>() else { return };
    let style = root.style();
    let _ = style.set_property("--skraft-top", &hex(top));
    let _ = style.set_property("--skraft-bottom", &hex(bottom));
}

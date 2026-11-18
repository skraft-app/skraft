//! Opening and saving `.skraft` files (versioned JSON), importing STEP files: file
//! dialogs on the desktop, a file picker and a download in the browser.

use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use skraft_i18n::t;
use skraft_model::Document;

use crate::docs::DocId;

pub const EXTENSION: &str = "skraft";

/// The extensions of the files the app imports (docs/io/step-import.md).
pub const IMPORT_EXTENSIONS: &[&str] = &["step", "stp", "STEP", "STP"];

/// What a file dialog ended with. Dialogs never block: a modal one (rfd's `FileDialog`) runs a
/// nested event loop inside winit's event handler, which panics on macOS ("tried to handle event
/// while another event is currently being handled") as soon as another event is queued — the
/// kernel's thread waking the UI, say. So they run asynchronously (a sheet on macOS) and their
/// outcome lands here, collected by the app on its next frame.
pub enum Outcome {
    /// A file read: its name and text.
    Opened { name: String, text: String },
    /// A file to import read: its name (without folders) and bytes.
    Imported { name: String, bytes: Vec<u8> },
    /// Document `doc` written to `path`, as it was at history `revision`.
    Saved { doc: DocId, path: String, revision: u64 },
    /// Writing document `doc` failed (the message is ready to show).
    SaveFailed { doc: DocId, error: String },
    /// The save dialog for document `doc` was dismissed (desktop: the browser downloads).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    SaveCancelled { doc: DocId },
}

/// The outcomes of the file dialogs, collected by the app.
#[derive(Clone, Default)]
pub struct Inbox(Arc<Mutex<Vec<Outcome>>>);

impl Inbox {
    /// The outcomes since the last call.
    pub fn take(&self) -> Vec<Outcome> {
        self.0.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }

    fn put(&self, outcome: Outcome) {
        if let Ok(mut v) = self.0.lock() {
            v.push(outcome);
        }
    }
}

/// Runs `fut` (a dialog) off the UI thread on the desktop…
#[cfg(not(target_arch = "wasm32"))]
fn spawn(fut: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || pollster::block_on(fut));
}

/// …in the page's executor on the web.
#[cfg(target_arch = "wasm32")]
fn spawn(fut: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(fut);
}

/// Asks for a file to open; its text arrives in `inbox`.
pub fn open(inbox: &Inbox, ctx: &egui::Context) {
    let inbox = inbox.clone();
    let ctx = ctx.clone();
    spawn(async move {
        let picked = rfd::AsyncFileDialog::new().add_filter("Skraft", &[EXTENSION, "json"]).pick_file().await;
        if let Some(f) = picked {
            #[cfg(not(target_arch = "wasm32"))]
            let name = f.path().display().to_string();
            #[cfg(target_arch = "wasm32")]
            let name = f.file_name();
            let bytes = f.read().await;
            inbox.put(Outcome::Opened { name, text: String::from_utf8_lossy(&bytes).into_owned() });
            ctx.request_repaint();
        }
    });
}

/// Reads the file at `path` (a recent file, desktop); its text arrives in `inbox`.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_path(path: &str, inbox: &Inbox) -> std::io::Result<()> {
    let text = std::fs::read(path)?;
    inbox.put(Outcome::Opened { name: path.to_owned(), text: String::from_utf8_lossy(&text).into_owned() });
    Ok(())
}

/// Asks for a file to import (STEP); its name and bytes arrive in `inbox`.
pub fn import(inbox: &Inbox, ctx: &egui::Context) {
    let inbox = inbox.clone();
    let ctx = ctx.clone();
    spawn(async move {
        let picked = rfd::AsyncFileDialog::new().add_filter("STEP", IMPORT_EXTENSIONS).pick_file().await;
        if let Some(f) = picked {
            let name = f.file_name();
            let bytes = f.read().await;
            inbox.put(Outcome::Imported { name, bytes });
            ctx.request_repaint();
        }
    });
}

/// Saves `document` (document `doc`, at history `revision`): to `path` if given (desktop),
/// else asking where (`name`: the name offered). The outcome arrives in `inbox`, with `doc`:
/// another tab may be shown by then.
pub fn save(doc: DocId, document: &Document, revision: u64, path: Option<&str>, name: &str, inbox: &Inbox, ctx: &egui::Context) {
    let text = match document.to_json() {
        Ok(t) => t,
        Err(e) => return inbox.put(Outcome::SaveFailed { doc, error: e.to_string() }),
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let write = move |path: std::path::PathBuf| match std::fs::write(&path, text) {
            Ok(()) => Outcome::Saved { doc, path: path.display().to_string(), revision },
            Err(e) => Outcome::SaveFailed { doc, error: t!("error.io", path = path.display(), error = e) },
        };
        match path {
            Some(p) => inbox.put(write(p.into())),
            None => {
                let inbox = inbox.clone();
                let ctx = ctx.clone();
                let name = format!("{name}.{EXTENSION}");
                spawn(async move {
                    let picked = rfd::AsyncFileDialog::new().add_filter("Skraft", &[EXTENSION]).set_file_name(name).save_file().await;
                    inbox.put(match picked {
                        Some(f) => write(f.path().to_path_buf()),
                        None => Outcome::SaveCancelled { doc },
                    });
                    ctx.request_repaint();
                });
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        let fallback = format!("{name}.{EXTENSION}");
        let name = path.and_then(|p| p.rsplit('/').next()).unwrap_or(&fallback).to_owned();
        inbox.put(match download(&name, &text) {
            Ok(()) => Outcome::Saved { doc, path: name, revision },
            Err(e) => Outcome::SaveFailed { doc, error: format!("{e:?}") },
        });
    }
}

/// Offers `text` as a file download.
#[cfg(target_arch = "wasm32")]
fn download(name: &str, text: &str) -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(text));
    let props = web_sys::BlobPropertyBag::new();
    props.set_type("application/json");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &props)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let a: web_sys::HtmlAnchorElement = doc.create_element("a")?.dyn_into()?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    web_sys::Url::revoke_object_url(&url)?;
    Ok(())
}

//! Opening and saving `.solidsketch` files (versioned JSON): file dialogs on the desktop, a
//! file picker and a download in the browser.

use std::sync::{Arc, Mutex};

use solidsketch_i18n::t;
use solidsketch_model::Document;

pub const EXTENSION: &str = "solidsketch";

/// A file read in the background (the browser's picker is asynchronous), collected by the app.
#[derive(Clone, Default)]
pub struct Inbox(Arc<Mutex<Option<(String, String)>>>);

impl Inbox {
    /// The (name, text) of a file read since the last call.
    pub fn take(&self) -> Option<(String, String)> {
        self.0.lock().ok()?.take()
    }

    #[allow(dead_code)]
    fn put(&self, name: String, text: String) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some((name, text));
        }
    }
}

/// Asks for a file to open; its text arrives in `inbox`.
pub fn open(inbox: &Inbox, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = ctx;
        let picked = rfd::FileDialog::new().add_filter("SolidSketch", &[EXTENSION, "json"]).pick_file();
        if let Some(path) = picked {
            match std::fs::read_to_string(&path) {
                Ok(text) => inbox.put(path.display().to_string(), text),
                Err(e) => log::error!("{}: {e}", path.display()),
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let inbox = inbox.clone();
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let picked = rfd::AsyncFileDialog::new().add_filter("SolidSketch", &[EXTENSION, "json"]).pick_file().await;
            if let Some(f) = picked {
                let bytes = f.read().await;
                inbox.put(f.file_name(), String::from_utf8_lossy(&bytes).into_owned());
                ctx.request_repaint();
            }
        });
    }
}

/// Saves `doc`: to `path` if given (desktop), else asking where. Returns the path written.
pub fn save(doc: &Document, path: Option<&str>) -> Result<Option<String>, String> {
    let text = doc.to_json().map_err(|e| e.to_string())?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = match path {
            Some(p) => std::path::PathBuf::from(p),
            None => match rfd::FileDialog::new()
                .add_filter("SolidSketch", &[EXTENSION])
                .set_file_name(format!("{}.{EXTENSION}", t!("default_name.file")))
                .save_file()
            {
                Some(p) => p,
                None => return Ok(None),
            },
        };
        std::fs::write(&path, text).map_err(|e| t!("error.io", path = path.display(), error = e))?;
        Ok(Some(path.display().to_string()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let fallback = format!("{}.{EXTENSION}", t!("default_name.file"));
        let name = path.and_then(|p| p.rsplit('/').next()).unwrap_or(&fallback).to_owned();
        download(&name, &text).map_err(|e| format!("{e:?}"))?;
        Ok(Some(name))
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

//! Opening and saving `.skraft` files (docs/io/file-format.md), importing STEP and SketchUp
//! files: file dialogs on the desktop, a file picker and a download in the browser.

use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use skraft_i18n::t;
use skraft_model::Document;

use crate::docs::DocId;

pub const EXTENSION: &str = "skraft";
/// Materials alone (docs/io/file-format.md): exported, imported into a model.
pub const MATERIALS_EXTENSION: &str = "skraftmat";
/// The images a texture takes (docs/render/look.md#textures).
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"];

/// The extensions of the files the app imports (docs/io/step-import.md,
/// docs/io/sketchup-import.md).
pub const IMPORT_EXTENSIONS: &[&str] = &["step", "stp", "skp", "STEP", "STP", "SKP"];

/// What a file dialog ended with. Dialogs never block: a modal one (rfd's `FileDialog`) runs a
/// nested event loop inside winit's event handler, which panics on macOS ("tried to handle event
/// while another event is currently being handled") as soon as another event is queued — the
/// kernel's thread waking the UI, say. So they run asynchronously (a sheet on macOS) and their
/// outcome lands here, collected by the app on its next frame.
pub enum Outcome {
    /// A file read: its name and bytes.
    Opened { name: String, bytes: Vec<u8> },
    /// A file to import read: its name (without folders) and bytes.
    Imported { name: String, bytes: Vec<u8> },
    /// An image picked for the texture of `material`, in document `doc`.
    TextureImage { doc: DocId, material: skraft_model::Id, bytes: Vec<u8> },
    /// A file written that is not a document's (materials exported): where.
    Written { path: String },
    /// A file exported (a drawing's PDF) written: where — to be opened from its toast.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Exported { path: String },
    /// Writing it failed (the message is ready to show).
    WriteFailed { error: String },
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

/// Asks for a file to open; its bytes arrive in `inbox`.
pub fn open(inbox: &Inbox, ctx: &egui::Context) {
    let inbox = inbox.clone();
    let ctx = ctx.clone();
    spawn(async move {
        let picked = rfd::AsyncFileDialog::new().add_filter("Skraft", &[EXTENSION]).pick_file().await;
        if let Some(f) = picked {
            #[cfg(not(target_arch = "wasm32"))]
            let name = f.path().display().to_string();
            #[cfg(target_arch = "wasm32")]
            let name = f.file_name();
            let bytes = f.read().await;
            inbox.put(Outcome::Opened { name, bytes });
            ctx.request_repaint();
        }
    });
}

/// Reads the file at `path` (a recent file, desktop); its bytes arrive in `inbox`.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_path(path: &str, inbox: &Inbox) -> std::io::Result<()> {
    let bytes = std::fs::read(path)?;
    inbox.put(Outcome::Opened { name: path.to_owned(), bytes });
    Ok(())
}

/// Asks for a file to import (STEP, SketchUp); its name and bytes arrive in `inbox`.
pub fn import(inbox: &Inbox, ctx: &egui::Context) {
    let inbox = inbox.clone();
    let ctx = ctx.clone();
    spawn(async move {
        let picked = rfd::AsyncFileDialog::new().add_filter("STEP, SKP", IMPORT_EXTENSIONS).pick_file().await;
        if let Some(f) = picked {
            let name = f.file_name();
            let bytes = f.read().await;
            inbox.put(Outcome::Imported { name, bytes });
            ctx.request_repaint();
        }
    });
}

/// Asks for an image for the texture of `material` (document `doc`); its bytes arrive in `inbox`.
pub fn pick_texture(doc: DocId, material: skraft_model::Id, inbox: &Inbox, ctx: &egui::Context) {
    let inbox = inbox.clone();
    let ctx = ctx.clone();
    spawn(async move {
        let picked = rfd::AsyncFileDialog::new().add_filter("Image", IMAGE_EXTENSIONS).pick_file().await;
        if let Some(f) = picked {
            let bytes = f.read().await;
            inbox.put(Outcome::TextureImage { doc, material, bytes });
            ctx.request_repaint();
        }
    });
}

/// Asks for a `.skraftmat` to import into the shown document: it arrives as a file opened.
pub fn import_materials(inbox: &Inbox, ctx: &egui::Context) {
    let inbox = inbox.clone();
    let ctx = ctx.clone();
    spawn(async move {
        let picked = rfd::AsyncFileDialog::new().add_filter("Skraft", &[MATERIALS_EXTENSION]).pick_file().await;
        if let Some(f) = picked {
            #[cfg(not(target_arch = "wasm32"))]
            let name = f.path().display().to_string();
            #[cfg(target_arch = "wasm32")]
            let name = f.file_name();
            let bytes = f.read().await;
            inbox.put(Outcome::Opened { name, bytes });
            ctx.request_repaint();
        }
    });
}

/// Writes `bytes` — materials exported (`.skraftmat`) — where the user says, `name` offered;
/// in the browser, a download.
pub fn export_materials(bytes: Vec<u8>, name: &str, inbox: &Inbox, ctx: &egui::Context) {
    let name = format!("{name}.{MATERIALS_EXTENSION}");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let inbox = inbox.clone();
        let ctx = ctx.clone();
        spawn(async move {
            let picked = rfd::AsyncFileDialog::new().add_filter("Skraft", &[MATERIALS_EXTENSION]).set_file_name(name).save_file().await;
            if let Some(f) = picked {
                let path = f.path().to_path_buf();
                inbox.put(match std::fs::write(&path, &bytes) {
                    Ok(()) => Outcome::Written { path: path.display().to_string() },
                    Err(e) => Outcome::WriteFailed { error: t!("error.io", path = path.display(), error = e) },
                });
                ctx.request_repaint();
            }
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        inbox.put(match download(&name, &bytes) {
            Ok(()) => Outcome::Written { path: name },
            Err(e) => Outcome::WriteFailed { error: format!("{e:?}") },
        });
    }
}

/// Writes `bytes` to a file the user picks (`name` offered, `ext` its extension, `kind` the
/// filter's name); a download on the web. The outcome arrives in `inbox`.
pub fn export_file(bytes: Vec<u8>, name: &str, kind: &str, ext: &str, inbox: &Inbox, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (inbox, ctx, name, kind, ext) = (inbox.clone(), ctx.clone(), name.to_owned(), kind.to_owned(), ext.to_owned());
        spawn(async move {
            let picked = rfd::AsyncFileDialog::new().add_filter(kind, &[ext.as_str()]).set_file_name(name).save_file().await;
            if let Some(f) = picked {
                let path = f.path().to_path_buf();
                inbox.put(match std::fs::write(&path, &bytes) {
                    Ok(()) => Outcome::Exported { path: path.display().to_string() },
                    Err(e) => Outcome::WriteFailed { error: t!("error.io", path = path.display(), error = e) },
                });
                ctx.request_repaint();
            }
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ctx, kind, ext);
        inbox.put(match download(name, &bytes) {
            Ok(()) => Outcome::Written { path: name.to_owned() },
            Err(e) => Outcome::WriteFailed { error: format!("{e:?}") },
        });
    }
}

/// Saves `document` (document `doc`, at history `revision`, as `view` says it was seen): to
/// `path` if given (desktop), else asking where (`name`: the name offered). The outcome arrives
/// in `inbox`, with `doc`: another tab may be shown by then.
#[allow(clippy::too_many_arguments)]
pub fn save(
    doc: DocId,
    document: &Document,
    view: &skraft_model::FileView,
    revision: u64,
    path: Option<&str>,
    name: &str,
    inbox: &Inbox,
    ctx: &egui::Context,
) {
    let bytes = match document.to_file_seen(view) {
        Ok(t) => t,
        Err(e) => return inbox.put(Outcome::SaveFailed { doc, error: e.to_string() }),
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let write = move |path: std::path::PathBuf| match std::fs::write(&path, &bytes) {
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
        inbox.put(match download(&name, &bytes) {
            Ok(()) => Outcome::Saved { doc, path: name, revision },
            Err(e) => Outcome::SaveFailed { doc, error: format!("{e:?}") },
        });
    }
}

/// Offers `bytes` as a file download.
#[cfg(target_arch = "wasm32")]
fn download(name: &str, bytes: &[u8]) -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let props = web_sys::BlobPropertyBag::new();
    props.set_type("application/x-skraft");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &props)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let a: web_sys::HtmlAnchorElement = doc.create_element("a")?.dyn_into()?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    web_sys::Url::revoke_object_url(&url)?;
    Ok(())
}

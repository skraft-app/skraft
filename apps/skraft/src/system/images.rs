//! The textures' images prepared off the interface's thread (docs/render/look.md#textures): a
//! [`Decoder`] reads them on threads (desktop) or in a web worker (`web/image-worker.js`), and
//! [`TextureImages`] keeps the renderer's layers made from them, one more per frame.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use skraft_model::Document;

/// The size an image is decoded at.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Size {
    /// A square of this side, whatever its proportions.
    Exact(u32),
    /// Its proportions kept, within a square of this side.
    Fit(u32),
}

/// An image decoded: sRGB RGBA.
pub(crate) struct Decoded {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

fn decode(bytes: &[u8], size: Size) -> Option<Decoded> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = match size {
        Size::Exact(side) => image.resize_exact(side, side, image::imageops::FilterType::Triangle),
        Size::Fit(side) => image.thumbnail(side, side),
    }
    .to_rgba8();
    Some(Decoded { size: [image.width(), image.height()], rgba: image.into_raw() })
}

/// What a decoder gives back: an image's key, and the image unless it does not read.
type Result = (String, Option<Decoded>);

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use threads::Decoder;
#[cfg(target_arch = "wasm32")]
pub(crate) use worker::Decoder;

#[cfg(not(target_arch = "wasm32"))]
mod threads {
    use std::sync::mpsc;

    use super::*;

    /// How many images are decoded at once.
    const THREADS: usize = 2;

    pub(crate) struct Decoder {
        queue: VecDeque<(String, Arc<[u8]>, Size)>,
        running: usize,
        sender: mpsc::Sender<Result>,
        receiver: mpsc::Receiver<Result>,
    }

    impl Default for Decoder {
        fn default() -> Self {
            let (sender, receiver) = mpsc::channel();
            Decoder { queue: VecDeque::new(), running: 0, sender, receiver }
        }
    }

    impl Decoder {
        /// Asks for the image `key`; a repaint comes with it.
        pub fn ask(&mut self, ctx: &egui::Context, key: String, bytes: Arc<[u8]>, size: Size) {
            self.queue.push_back((key, bytes, size));
            self.start(ctx);
        }

        fn start(&mut self, ctx: &egui::Context) {
            while self.running < THREADS {
                let Some((key, bytes, size)) = self.queue.pop_front() else { break };
                self.running += 1;
                let (sender, ctx) = (self.sender.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let image = decode(&bytes, size);
                    let _ = sender.send((key, image));
                    ctx.request_repaint();
                });
            }
        }

        /// The images decoded since the last call.
        pub fn take(&mut self, ctx: &egui::Context) -> Vec<Result> {
            let done: Vec<Result> = self.receiver.try_iter().collect();
            self.running -= done.len();
            self.start(ctx);
            done
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod worker {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    use super::*;

    /// The worker's script, next to `index.html`.
    const SCRIPT: &str = "./image-worker.js";

    /// What the worker's messages leave for the next [`Decoder::take`].
    #[derive(Default)]
    struct Inbox {
        done: Vec<(String, Decoded)>,
        /// Those the browser could not decode.
        failed: Vec<String>,
        /// The worker itself failed: nothing more comes from it.
        broken: bool,
    }

    type Listener<T> = Closure<dyn FnMut(T)>;

    /// The worker, started at the first image, and what it was given. An image it cannot give
    /// is decoded here after all, one per call: slower, but the texture is there.
    #[derive(Default)]
    pub(crate) struct Decoder {
        worker: Option<(web_sys::Worker, Listener<web_sys::MessageEvent>, Listener<web_sys::ErrorEvent>)>,
        inbox: Rc<RefCell<Inbox>>,
        sent: HashMap<String, (Arc<[u8]>, Size)>,
        here: VecDeque<(String, Arc<[u8]>, Size)>,
    }

    fn field(data: &JsValue, name: &str) -> Option<JsValue> {
        js_sys::Reflect::get(data, &name.into()).ok().filter(|v| !v.is_undefined())
    }

    impl Decoder {
        fn start(&mut self, ctx: &egui::Context) {
            if self.worker.is_some() || self.inbox.borrow().broken {
                return;
            }
            let Ok(worker) = web_sys::Worker::new(SCRIPT) else {
                self.inbox.borrow_mut().broken = true;
                return;
            };
            let (inbox, wake) = (self.inbox.clone(), ctx.clone());
            let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
                let data = e.data();
                let Some(key) = field(&data, "key").and_then(|k| k.as_string()) else { return };
                let image = (|| {
                    let width = field(&data, "width")?.as_f64()? as u32;
                    let height = field(&data, "height")?.as_f64()? as u32;
                    let rgba = js_sys::Uint8Array::new(&field(&data, "rgba")?).to_vec();
                    (rgba.len() == width as usize * height as usize * 4).then_some(Decoded { size: [width, height], rgba })
                })();
                match image {
                    Some(image) => inbox.borrow_mut().done.push((key, image)),
                    None => inbox.borrow_mut().failed.push(key),
                }
                wake.request_repaint();
            });
            let (inbox, wake) = (self.inbox.clone(), ctx.clone());
            let on_error = Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |e: web_sys::ErrorEvent| {
                log::error!("image worker: {}", e.message());
                inbox.borrow_mut().broken = true;
                wake.request_repaint();
            });
            worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
            self.worker = Some((worker, on_message, on_error));
        }

        /// Asks for the image `key`; a repaint comes with it.
        pub fn ask(&mut self, ctx: &egui::Context, key: String, bytes: Arc<[u8]>, size: Size) {
            self.start(ctx);
            let posted = self.worker.as_ref().is_some_and(|(worker, ..)| {
                let buffer = js_sys::Uint8Array::from(&bytes[..]).buffer();
                let msg = js_sys::Object::new();
                let (exact, fit) = match size {
                    Size::Exact(side) => (Some(side), None),
                    Size::Fit(side) => (None, Some(side)),
                };
                let _ = js_sys::Reflect::set(&msg, &"key".into(), &key.as_str().into());
                let _ = js_sys::Reflect::set(&msg, &"bytes".into(), &buffer);
                if let Some(side) = exact {
                    let _ = js_sys::Reflect::set(&msg, &"exact".into(), &side.into());
                }
                if let Some(side) = fit {
                    let _ = js_sys::Reflect::set(&msg, &"fit".into(), &side.into());
                }
                worker.post_message_with_transfer(&msg, &js_sys::Array::of1(&buffer)).is_ok()
            });
            if posted {
                self.sent.insert(key, (bytes, size));
            } else {
                self.here.push_back((key, bytes, size));
                ctx.request_repaint();
            }
        }

        /// The images decoded since the last call.
        pub fn take(&mut self, ctx: &egui::Context) -> Vec<Result> {
            let (done, failed, broken) = {
                let mut inbox = self.inbox.borrow_mut();
                (std::mem::take(&mut inbox.done), std::mem::take(&mut inbox.failed), inbox.broken)
            };
            let mut out = Vec::with_capacity(done.len() + 1);
            for (key, image) in done {
                if self.sent.remove(&key).is_some() {
                    out.push((key, Some(image)));
                }
            }
            for key in failed {
                if let Some((bytes, size)) = self.sent.remove(&key) {
                    self.here.push_back((key, bytes, size));
                }
            }
            if broken {
                self.worker = None;
                self.here.extend(self.sent.drain().map(|(key, (bytes, size))| (key, bytes, size)));
            }
            if let Some((key, bytes, size)) = self.here.pop_front() {
                out.push((key, decode(&bytes, size)));
            }
            if !self.here.is_empty() {
                ctx.request_repaint();
            }
            out
        }
    }
}

/// The renderer's layers of the textures' images, by image id (none: unreadable); an image's
/// id is its content's hash, the same in every document. Only the images asked for are made,
/// and at most one set of mip levels per frame.
#[derive(Default)]
pub(crate) struct TextureImages {
    levels: HashMap<String, Option<Vec<Vec<u8>>>>,
    asked: HashSet<String>,
    arrived: VecDeque<Result>,
    decoder: Decoder,
    pass: u64,
}

impl TextureImages {
    /// Asks for the images `ids` of `doc` not asked yet, and takes in what has come.
    pub fn update<'a>(&mut self, ctx: &egui::Context, doc: &Document, ids: impl Iterator<Item = &'a String>) {
        for id in ids {
            if self.levels.contains_key(id) || self.asked.contains(id) {
                continue;
            }
            match doc.asset(id) {
                Some(asset) => {
                    self.asked.insert(id.clone());
                    self.decoder.ask(ctx, id.clone(), asset.bytes.clone(), Size::Exact(render::TEXTURE_SIZE));
                }
                None => {
                    self.levels.insert(id.clone(), None);
                }
            }
        }
        self.arrived.extend(self.decoder.take(ctx));
        let pass = ctx.cumulative_pass_nr();
        if pass != self.pass
            && let Some((id, image)) = self.arrived.pop_front()
        {
            self.pass = pass;
            self.asked.remove(&id);
            self.levels.insert(id, image.map(|i| render::texture_levels(i.rgba)));
            // Taken in after this frame's pictures: the next one draws them with it.
            ctx.request_repaint();
        }
        if !self.arrived.is_empty() {
            ctx.request_repaint();
        }
    }

    /// The mip levels of image `id`, once made.
    pub fn get(&self, id: &str) -> Option<&[Vec<u8>]> {
        self.levels.get(id)?.as_deref()
    }

    /// Has image `id` been dealt with, read or not?
    pub fn settled(&self, id: &str) -> bool {
        self.levels.contains_key(id)
    }
}

use skraft_render as render;

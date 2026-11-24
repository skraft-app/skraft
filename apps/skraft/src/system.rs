//! What the app needs from its platform and process: files and their dialogs, the web page
//! and its workers, crashes, the build's identity, the documents the system opens, the
//! language and separators, dates, images off the interface's thread, thumbnails, the
//! clipboard, the control channel and its driving.

pub(crate) mod clipboard;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod control;
#[cfg(not(target_arch = "wasm32"))]
pub mod crash;
pub(crate) mod dates;
pub(crate) mod driving;
pub(crate) mod file;
pub(crate) mod identity;
pub(crate) mod images;
pub(crate) mod language;
#[cfg(not(target_arch = "wasm32"))]
pub mod opened;
pub(crate) mod separators;
pub(crate) mod thumbnails;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(target_arch = "wasm32")]
pub(crate) mod worker;

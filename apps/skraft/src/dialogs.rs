//! The app's own windows and boxes: About, updates, the journal, the navigation preferences,
//! the imprint's confirmation.

pub(crate) mod about;
pub(crate) mod imprint;
pub(crate) mod journal_window;
pub(crate) mod nav_prefs;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod update;

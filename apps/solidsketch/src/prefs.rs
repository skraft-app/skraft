//! The user's preferences (docs/ARCHITECTURE.md §6 bis): navigation, and the settings new
//! models start with. A versioned JSON file of our own in the OS's configuration folder
//! (`~/Library/Application Support/SolidSketch/preferences.json` on macOS); in the browser,
//! `localStorage`.
//!
//! Every field has a default, so an older or partial file still loads; a file that cannot be
//! read is left alone (the defaults are used, and it is only replaced once a preference
//! changes).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidsketch_i18n::t;
use solidsketch_model::Settings;
use solidsketch_tools::OptionValues;

/// The version this build writes.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub navigation: Navigation,
    /// The display style and units of new models.
    pub new_model: Settings,
    /// The tools' settings from their option bar (the booleans' scope…), by tool id.
    pub tool_options: BTreeMap<String, OptionValues>,
    /// The language of the app, a locale of /locales (`fr`).
    pub locale: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            version: VERSION,
            navigation: Navigation::default(),
            new_model: Settings::default(),
            tool_options: BTreeMap::new(),
            locale: solidsketch_i18n::DEFAULT_LOCALE.to_owned(),
        }
    }
}

/// How the mouse and the trackpad move the camera.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Navigation {
    /// Orbit speed factor (1: SketchUp's).
    pub orbit_speed: f64,
    pub mouse_invert_x: bool,
    pub mouse_invert_y: bool,
    pub trackpad_invert_x: bool,
    pub trackpad_invert_y: bool,
    /// Zoom speed factor (1: a wheel notch zooms by 15 %).
    pub zoom_speed: f64,
    pub wheel_invert: bool,
    /// Zoom towards the cursor (else towards the centre of the view).
    pub zoom_to_cursor: bool,
}

impl Default for Navigation {
    fn default() -> Self {
        Navigation {
            orbit_speed: 1.0,
            mouse_invert_x: false,
            mouse_invert_y: false,
            trackpad_invert_x: false,
            trackpad_invert_y: false,
            zoom_speed: 1.0,
            wheel_invert: false,
            zoom_to_cursor: true,
        }
    }
}

impl Preferences {
    /// Reads the stored preferences: the defaults when there are none, and an error message
    /// when they cannot be read.
    pub fn load() -> (Preferences, Option<String>) {
        match store::read() {
            Ok(None) => (Preferences::default(), None),
            Ok(Some(text)) => match serde_json::from_str::<Preferences>(&text) {
                Ok(p) if p.version <= VERSION => (Preferences { version: VERSION, ..p }, None),
                Ok(p) => (Preferences::default(), Some(t!("error.preferences.newer", version = p.version))),
                Err(e) => (Preferences::default(), Some(t!("error.preferences.unreadable", error = e))),
            },
            Err(e) => (Preferences::default(), Some(t!("error.preferences.unreadable", error = e))),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        store::write(&text)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod store {
    use std::path::PathBuf;

    use solidsketch_i18n::t;

    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("SolidSketch").join("preferences.json"))
    }

    pub fn read() -> Result<Option<String>, String> {
        let Some(p) = path() else { return Ok(None) };
        match std::fs::read_to_string(&p) {
            Ok(t) => Ok(Some(t)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(t!("error.io", path = p.display(), error = e)),
        }
    }

    /// Writes through a temporary file, so a crash never leaves half a file.
    pub fn write(text: &str) -> Result<(), String> {
        let p = path().ok_or_else(|| t!("error.preferences.no_config_dir"))?;
        let err = |e: std::io::Error| t!("error.io", path = p.display(), error = e);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(err)?;
        std::fs::rename(&tmp, &p).map_err(err)
    }
}

#[cfg(target_arch = "wasm32")]
mod store {
    use solidsketch_i18n::t;

    const KEY: &str = "solidsketch.preferences";

    fn storage() -> Result<web_sys::Storage, String> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .ok_or_else(|| t!("error.preferences.no_browser_storage"))
    }

    pub fn read() -> Result<Option<String>, String> {
        storage()?.get_item(KEY).map_err(|e| format!("{e:?}"))
    }

    pub fn write(text: &str) -> Result<(), String> {
        storage()?.set_item(KEY, text).map_err(|e| format!("{e:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_files_load_with_defaults() {
        let p: Preferences = serde_json::from_str(r#"{"navigation":{"mouse_invert_y":true}}"#).unwrap();
        assert!(p.navigation.mouse_invert_y);
        assert_eq!(p.navigation.orbit_speed, 1.0);
        assert_eq!(p.new_model, Settings::default());
        let back: Preferences = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }
}

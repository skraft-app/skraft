//! The user's preferences (docs/ui/interface.md): navigation, and the settings new
//! models start with. A versioned JSON file of our own in the OS's configuration folder
//! (`~/Library/Application Support/Skraft/preferences.json` on macOS); in the browser,
//! `localStorage`.
//!
//! Every field has a default, so an older or partial file still loads; a file that cannot be
//! read is left alone (the defaults are used, and it is only replaced once a preference
//! changes).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dock::Layout;
use skraft_i18n::t;
use skraft_model::Settings;
use skraft_theme::{Choice, Palette};
use skraft_tools::OptionValues;
use skraft_tools::navigation::NavStyle;

use crate::commands::RECENT_FILES;

/// The control channel's port by default.
pub const DEFAULT_REMOTE_PORT: u16 = 7858;

/// The memory the undo histories of the documents open may hold together by default, in MiB
/// (docs/model/undo.md#memory-cap): less in the browser, whose tab has less.
pub const DEFAULT_HISTORY_MEMORY: u32 = if cfg!(target_arch = "wasm32") { 512 } else { 1024 };

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
    /// The language of the app, a locale of /locales (`fr`); none: the system's
    /// (src/language.rs).
    pub locale: Option<String>,
    /// The interface's theme (docs/render/look.md): the system's, light or dark.
    pub theme: Choice,
    /// The interface's colours, in the variant the theme picks.
    pub palette: Palette,
    /// Shortcuts changed by the user, by command id (`Cmd+Shift+Z`; empty: none).
    pub shortcuts: BTreeMap<String, String>,
    /// The render statistics overlay, in a corner of the view.
    pub show_render_stats: bool,
    /// The last files opened or saved, the latest first (desktop: the browser gives no path to
    /// open again).
    pub recent_files: Vec<String>,
    /// Open again at launch the saved documents open when the app quit (desktop,
    /// docs/ui/file-tabs.md)…
    pub reopen_documents: bool,
    /// …which are these, in tab order, and the one shown among them.
    pub open_files: Vec<String>,
    pub shown_file: Option<usize>,
    /// The side panels: their columns, order, sizes, folded or closed.
    pub panels: Layout,
    /// The workspace and tab shown, the ribbon's pins and labels (docs/ui/ribbon.md).
    pub ribbon: crate::ribbon::Prefs,
    /// The commands the quick tools' card shows (docs/tools/quick-tools.md); absent: the
    /// defaults.
    pub quick_tools: Option<Vec<String>>,
    /// The commands the view bar shows (docs/ui/view-bar.md); absent: the defaults.
    pub view_bar: Option<Vec<String>>,
    /// The side of the view the view bar is docked to.
    pub view_bar_side: crate::dock::Dock,
    /// The control channel (docs/ui/commands.md) is on at launch (desktop).
    pub remote_control: bool,
    /// Its port, on 127.0.0.1.
    pub remote_port: u16,
    /// The memory the undo histories of the documents open may hold together, in MiB (their
    /// states' B-reps and meshes, besides the current documents): beyond, their oldest steps
    /// are forgotten (docs/model/undo.md#memory-cap).
    pub history_memory: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            version: VERSION,
            navigation: Navigation::default(),
            new_model: Settings::default(),
            tool_options: BTreeMap::new(),
            locale: None,
            theme: Choice::System,
            palette: Palette::Warm,
            shortcuts: BTreeMap::new(),
            show_render_stats: false,
            recent_files: Vec::new(),
            reopen_documents: true,
            open_files: Vec::new(),
            shown_file: None,
            panels: Layout::default(),
            ribbon: crate::ribbon::Prefs::default(),
            quick_tools: None,
            view_bar: None,
            view_bar_side: crate::dock::Dock::Left,
            remote_control: false,
            remote_port: DEFAULT_REMOTE_PORT,
            history_memory: DEFAULT_HISTORY_MEMORY,
        }
    }
}

/// How the mouse and the trackpad move the camera.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Navigation {
    /// Orbit speed factor (1: `ORBIT_BASE` times SketchUp's).
    pub orbit_speed: f64,
    pub mouse_invert_x: bool,
    pub mouse_invert_y: bool,
    pub trackpad_invert_x: bool,
    pub trackpad_invert_y: bool,
    /// Zoom speed factor (1: `ZOOM_BASE` times a 15 % zoom per wheel notch).
    pub zoom_speed: f64,
    /// Zoom towards the cursor (else towards the centre of the view).
    pub zoom_to_cursor: bool,
    /// Which buttons and gestures do what (docs/ui/navigation-styles.md): a preset, maybe edited. The wheel's
    /// direction is the style's.
    pub style: NavStyle,
}

impl Navigation {
    /// The orbit rate at speed 1, relative to SketchUp's.
    const ORBIT_BASE: f64 = 0.7;
    /// The zoom rate at speed 1, in 15 % steps per wheel notch.
    const ZOOM_BASE: f64 = 0.5;

    /// The orbit rate the speed preference gives, relative to SketchUp's.
    pub fn orbit_rate(&self) -> f64 {
        self.orbit_speed * Self::ORBIT_BASE
    }

    /// The zoom rate the speed preference gives, in 15 % steps per wheel notch.
    pub fn zoom_rate(&self) -> f64 {
        self.zoom_speed * Self::ZOOM_BASE
    }
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
            zoom_to_cursor: true,
            style: NavStyle::default(),
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
                Ok(mut p) if p.version <= VERSION => {
                    p.panels.normalize();
                    (Preferences { version: VERSION, ..p }, None)
                }
                Ok(p) => (Preferences::default(), Some(t!("error.preferences.newer", version = p.version))),
                Err(e) => (Preferences::default(), Some(t!("error.preferences.unreadable", error = e))),
            },
            Err(e) => (Preferences::default(), Some(t!("error.preferences.unreadable", error = e))),
        }
    }

    /// Puts `path` first in the recent files, keeping the last [`RECENT_FILES`].
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn add_recent_file(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_owned());
        self.recent_files.truncate(RECENT_FILES);
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        store::write(&text)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod store {
    use std::path::PathBuf;

    use skraft_i18n::t;

    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("Skraft").join("preferences.json"))
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
    use skraft_i18n::t;

    const KEY: &str = "skraft.preferences";

    fn storage() -> Result<web_sys::Storage, String> {
        web_sys::window().and_then(|w| w.local_storage().ok().flatten()).ok_or_else(|| t!("error.preferences.no_browser_storage"))
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

    #[test]
    fn recent_files_keep_the_last_ones_without_duplicates() {
        let mut p = Preferences::default();
        for f in ["a", "b", "c", "d", "e", "c"] {
            p.add_recent_file(f);
        }
        assert_eq!(p.recent_files, ["c", "e", "d", "b"]);
    }
}

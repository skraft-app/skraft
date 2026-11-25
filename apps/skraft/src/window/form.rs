//! The window's form (docs/ui/touch.md#the-compact-arrangement): the desktop arrangement, or the
//! compact one of a phone — chosen by the screen, not a preference
//! (docs/decisions/0056-touch-screens-phone-views-tablet-models.md).

/// How the window is arranged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Form {
    /// The title bar with the file tabs, the ribbon, the docked columns, the status bar.
    #[default]
    Desktop,
    /// A phone's: bars at the top and the bottom, the view between, a card rising from the
    /// bottom.
    Compact,
}

/// Under a finger, the whole interface is drawn this much larger (egui's zoom): buttons, rows and
/// text, together — the text near a phone's usual 16 pt.
pub const TOUCH_ZOOM: f32 = 1.2;

/// Under a finger, a press held this long (s) is a long press — the right click, or with the select
/// tool the start of a selection rectangle — and no more a click (egui's 0.8 s; iOS's 0.5).
pub const LONG_PRESS: f64 = 0.5;

/// A touch screen whose short side is below this (points) is a phone's: compact either way up.
/// A tablet's is wider (an iPad mini's 744).
pub const COMPACT_SIDE: f32 = 600.0;

/// What the window's form is read from: the pointer (a finger or not), and a form forced for
/// testing — `SKRAFT_FORM=compact` (desktop) or `?form=compact` (web) in the page's address.
#[derive(Clone, Copy, Debug, Default)]
pub struct Probe {
    /// The primary pointer is a finger (CSS `pointer: coarse`): targets are made larger.
    pub coarse: bool,
    forced: Option<Form>,
}

impl Probe {
    /// Reads the platform once, at launch.
    pub fn read() -> Probe {
        let forced = forced();
        Probe { coarse: coarse() || forced == Some(Form::Compact), forced }
    }

    /// egui's zoom for this pointer.
    pub fn zoom(&self) -> f32 {
        if self.coarse { TOUCH_ZOOM } else { 1.0 }
    }

    /// The form for a window of `size` (points).
    pub fn form(&self, size: egui::Vec2) -> Form {
        self.forced.unwrap_or(if self.coarse && size.min_elem() < COMPACT_SIDE { Form::Compact } else { Form::Desktop })
    }

    /// Forced compact: the desktop window may be as small as a phone.
    pub fn forced_compact(&self) -> bool {
        self.forced == Some(Form::Compact)
    }
}

fn parse(value: &str) -> Option<Form> {
    match value {
        "compact" => Some(Form::Compact),
        "desktop" => Some(Form::Desktop),
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn forced() -> Option<Form> {
    std::env::var("SKRAFT_FORM").ok().as_deref().and_then(parse)
}

#[cfg(target_arch = "wasm32")]
fn forced() -> Option<Form> {
    let search = web_sys::window()?.location().search().ok()?;
    search.trim_start_matches('?').split('&').find_map(|kv| kv.strip_prefix("form=")).and_then(parse)
}

/// The desktop's pointer is a mouse or a trackpad.
#[cfg(not(target_arch = "wasm32"))]
fn coarse() -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
fn coarse() -> bool {
    web_sys::window().and_then(|w| w.match_media("(pointer: coarse)").ok().flatten()).is_some_and(|m| m.matches())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_touch_screen_is_compact_when_its_short_side_is_a_phones() {
        let touch = Probe { coarse: true, forced: None };
        assert_eq!(touch.form(egui::vec2(390.0, 844.0)), Form::Compact);
        assert_eq!(touch.form(egui::vec2(844.0, 390.0)), Form::Compact);
        assert_eq!(touch.form(egui::vec2(1024.0, 768.0)), Form::Desktop);
        // A small window under a mouse stays the desktop's.
        let mouse = Probe { coarse: false, forced: None };
        assert_eq!(mouse.form(egui::vec2(640.0, 400.0)), Form::Desktop);
        assert_eq!(Probe { coarse: false, forced: Some(Form::Compact) }.form(egui::vec2(1280.0, 800.0)), Form::Compact);
    }
}

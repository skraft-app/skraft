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

/// Under a finger, a press held this long (s) is a long press — the right click, or with the select
/// tool the start of a selection rectangle — and no more a click (egui's 0.8 s; iOS's 0.5).
pub const LONG_PRESS: f64 = 0.5;

/// Under a finger, a tap may move this much (points) and still be one (egui's 6, a mouse's), and
/// two taps this far apart (s) make a double tap (egui's 0.3): a finger is less still and slower.
pub const TAP_SLOP: f32 = 12.0;
pub const DOUBLE_TAP: f64 = 0.4;

/// A touch screen whose short side is below this (points) is a phone's: compact either way up.
/// A tablet's is wider (an iPad mini's 744).
pub const COMPACT_SIDE: f32 = crate::kit::style::PHONE_SIDE;

/// What the window's form is read from: the pointer (a finger or not), and a form forced for
/// testing — `SKRAFT_FORM=compact` (desktop) or `?form=compact` (web) in the page's address.
#[derive(Clone, Copy, Debug, Default)]
pub struct Probe {
    /// The primary pointer is a finger (CSS `pointer: coarse`): targets are made larger.
    pub coarse: bool,
    forced: Option<Form>,
    /// A key was typed on a keyboard (a tablet's, plugged in): the shortcuts are shown again.
    keyboard: bool,
}

impl Probe {
    /// Reads the platform once, at launch.
    pub fn read() -> Probe {
        let forced = forced();
        Probe { coarse: coarse() || forced == Some(Form::Compact), forced, keyboard: false }
    }

    /// The form for a window of `size` (points).
    pub fn form(&self, size: egui::Vec2) -> Form {
        self.forced.unwrap_or(if self.coarse && size.min_elem() < COMPACT_SIDE { Form::Compact } else { Form::Desktop })
    }

    /// Whether the keyboard shortcuts are shown (menus, tool tips, the palette): not under a
    /// finger, until a key is typed on a keyboard (docs/ui/touch.md#keyboard-shortcuts).
    pub fn shows_shortcuts(&self) -> bool {
        !self.coarse || self.keyboard
    }

    /// Reads this frame's events for a key typed on a keyboard. The system's on-screen
    /// keyboard only types into a text field, which then wants the keys: not counted.
    pub fn watch_keyboard(&mut self, ctx: &egui::Context) {
        if !self.shows_shortcuts() {
            // Asked before `input`, which holds the context's lock.
            let typing = ctx.egui_wants_keyboard_input();
            ctx.input(|i| self.read_keys(&i.events, typing));
        }
    }

    /// A frame's `events`; `typing`: a text field takes them.
    fn read_keys(&mut self, events: &[egui::Event], typing: bool) {
        self.keyboard |= !typing && events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. }));
    }

    /// Forced compact: the desktop window may be as small as a phone.
    #[cfg(not(target_arch = "wasm32"))]
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
        let touch = Probe { coarse: true, forced: None, keyboard: false };
        assert_eq!(touch.form(egui::vec2(390.0, 844.0)), Form::Compact);
        assert_eq!(touch.form(egui::vec2(844.0, 390.0)), Form::Compact);
        assert_eq!(touch.form(egui::vec2(1024.0, 768.0)), Form::Desktop);
        // A small window under a mouse stays the desktop's.
        let mouse = Probe { coarse: false, forced: None, keyboard: false };
        assert_eq!(mouse.form(egui::vec2(640.0, 400.0)), Form::Desktop);
        assert_eq!(Probe { coarse: false, forced: Some(Form::Compact), keyboard: false }.form(egui::vec2(1280.0, 800.0)), Form::Compact);
    }

    #[test]
    fn a_touch_screen_shows_the_shortcuts_once_a_key_is_typed() {
        assert!(Probe { coarse: false, forced: None, keyboard: false }.shows_shortcuts());
        let mut touch = Probe { coarse: true, forced: None, keyboard: false };
        touch.read_keys(&[egui::Event::Text("a".into())], false);
        assert!(!touch.shows_shortcuts(), "text alone is not a keyboard's key");
        let key = egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
        touch.read_keys(std::slice::from_ref(&key), true);
        assert!(!touch.shows_shortcuts(), "a key typed into a text field may be the on-screen keyboard's");
        touch.read_keys(&[key], false);
        assert!(touch.shows_shortcuts());
        touch.read_keys(&[], false);
        assert!(touch.shows_shortcuts(), "for the rest of the session");
    }
}

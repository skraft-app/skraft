//! The macOS menu bar (docs/ui/title-bar.md), built with `muda` from the command
//! registry: the app menu (About, Preferences, Services, Hide, Quit), the File menu, then the
//! system's Window menu — as the title bar, which has only File: the other commands are in the
//! ribbon, the command search and the shortcuts (the app's keymap, ⌘ ones included). A chosen
//! item is the id of its command, run as the title bar's are.
//!
//! Only the shortcuts with ⌘ become the items' key equivalents: AppKit takes a key equivalent
//! before the window sees the key, so a single letter (R, M, Espace…) would be lost to the
//! measurements box and the text fields. Those stay with the app's keymap. For the same reason
//! a ⌘ shortcut chosen while a shortcut is being captured goes back to egui as its key
//! ([`crate::app`]).
//!
//! The items follow the app every frame (enabled, checked, « Annuler Déplacer »); the bar is
//! built again when the language or the shortcuts change.

use std::sync::mpsc::{Receiver, sync_channel};

use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, CheckMenuItem, Menu as Bar, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use skraft_i18n::t;

use crate::commands::{self, COMMANDS, Keymap, Menu, Shortcut};
use crate::panels::CommandView;

const APP: &str = "Skraft";

enum Item {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

/// The app's menu bar while it lives.
pub struct NativeMenu {
    _bar: Bar,
    items: Vec<(&'static str, Item)>,
    events: Receiver<MenuEvent>,
    /// What the bar was built for: the language, the shortcuts, the recent files.
    built_for: String,
}

/// What a bar is built for: it is built again when this changes.
fn signature(views: &[CommandView], keymap: &Keymap) -> String {
    let mut s = skraft_i18n::locale().to_string();
    // The recent files come and go with their slots.
    for v in views.iter().filter(|v| commands::recent_slot(v.id).is_some()) {
        s.push_str(&format!("|{}={}", v.id, v.label));
    }
    for c in COMMANDS {
        if let Some(k) = keymap.get(c.id) {
            s.push_str(&format!("|{}={}", c.id, k.text()));
        }
    }
    s
}

/// The key equivalent of a ⌘ shortcut (none for the others, see the module's doc).
fn accelerator(s: Option<Shortcut>) -> Option<Accelerator> {
    let s = s.filter(|s| s.command)?;
    use egui::Key as K;
    let code = match s.key {
        K::A => Code::KeyA,
        K::B => Code::KeyB,
        K::C => Code::KeyC,
        K::D => Code::KeyD,
        K::E => Code::KeyE,
        K::F => Code::KeyF,
        K::G => Code::KeyG,
        K::H => Code::KeyH,
        K::I => Code::KeyI,
        K::J => Code::KeyJ,
        K::K => Code::KeyK,
        K::L => Code::KeyL,
        K::M => Code::KeyM,
        K::N => Code::KeyN,
        K::O => Code::KeyO,
        K::P => Code::KeyP,
        K::Q => Code::KeyQ,
        K::R => Code::KeyR,
        K::S => Code::KeyS,
        K::T => Code::KeyT,
        K::U => Code::KeyU,
        K::V => Code::KeyV,
        K::W => Code::KeyW,
        K::X => Code::KeyX,
        K::Y => Code::KeyY,
        K::Z => Code::KeyZ,
        K::Num0 => Code::Digit0,
        K::Num1 => Code::Digit1,
        K::Num2 => Code::Digit2,
        K::Num3 => Code::Digit3,
        K::Num4 => Code::Digit4,
        K::Num5 => Code::Digit5,
        K::Num6 => Code::Digit6,
        K::Num7 => Code::Digit7,
        K::Num8 => Code::Digit8,
        K::Num9 => Code::Digit9,
        K::Comma => Code::Comma,
        K::Period => Code::Period,
        K::Minus => Code::Minus,
        K::Equals => Code::Equal,
        K::Slash => Code::Slash,
        K::Semicolon => Code::Semicolon,
        K::Backspace => Code::Backspace,
        K::Delete => Code::Delete,
        K::Enter => Code::Enter,
        K::Tab => Code::Tab,
        K::Space => Code::Space,
        K::ArrowLeft => Code::ArrowLeft,
        K::ArrowRight => Code::ArrowRight,
        K::ArrowUp => Code::ArrowUp,
        K::ArrowDown => Code::ArrowDown,
        _ => return None,
    };
    let mut mods = Modifiers::META;
    if s.shift {
        mods |= Modifiers::SHIFT;
    }
    if s.alt {
        mods |= Modifiers::ALT;
    }
    Some(Accelerator::new(mods, code))
}

impl NativeMenu {
    /// Builds the bar and installs it. A chosen item repaints `ctx`. On the main thread only, as
    /// every AppKit menu.
    pub fn new(ctx: &egui::Context, views: &[CommandView], keymap: &Keymap) -> Result<Self, String> {
        let (sender, events) = sync_channel(64);
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e| {
            let _ = sender.try_send(e);
            ctx.request_repaint();
        }));
        let (bar, items) = build(views, keymap).map_err(|e| e.to_string())?;
        Ok(NativeMenu { _bar: bar, items, events, built_for: signature(views, keymap) })
    }

    /// The commands chosen since the last call; the bar follows `views` (and is built again
    /// for new texts or shortcuts).
    pub fn update(&mut self, views: &[CommandView], keymap: &Keymap) -> Vec<&'static str> {
        let sig = signature(views, keymap);
        if sig != self.built_for {
            match build(views, keymap) {
                // The new bar is installed before the old one goes.
                Ok((bar, items)) => {
                    self._bar = bar;
                    self.items = items;
                    self.built_for = sig;
                }
                Err(e) => log::warn!("menu bar not rebuilt: {e}"),
            }
        }
        for (id, item) in &self.items {
            let Some(v) = views.iter().find(|v| v.id == *id) else { continue };
            match item {
                Item::Plain(i) => {
                    if i.is_enabled() != v.enabled {
                        i.set_enabled(v.enabled);
                    }
                    if i.text() != v.label {
                        i.set_text(&v.label);
                    }
                }
                Item::Check(i) => {
                    if i.is_enabled() != v.enabled {
                        i.set_enabled(v.enabled);
                    }
                    // A click toggles the item itself: set back to the app's state.
                    let on = v.checked == Some(true);
                    if i.is_checked() != on {
                        i.set_checked(on);
                    }
                }
            }
        }
        self.events.try_iter().filter_map(|e| commands::find(e.id.as_ref()).map(|c| c.id)).collect()
    }
}

fn build(views: &[CommandView], keymap: &Keymap) -> muda::Result<(Bar, Vec<(&'static str, Item)>)> {
    let bar = Bar::new();
    let mut items = Vec::new();
    let mut item = |v: &CommandView| -> Item {
        let accel = accelerator(keymap.get(v.id));
        let (kept, shown) = match v.checked {
            Some(on) => {
                let m = CheckMenuItem::with_id(v.id, &v.label, v.enabled, on, accel);
                (Item::Check(m.clone()), Item::Check(m))
            }
            None => {
                let m = MenuItem::with_id(v.id, &v.label, v.enabled, accel);
                (Item::Plain(m.clone()), Item::Plain(m))
            }
        };
        items.push((v.id, kept));
        shown
    };
    let append = |menu: &Submenu, i: &Item| match i {
        Item::Plain(m) => menu.append(m),
        Item::Check(m) => menu.append(m),
    };
    let sep = PredefinedMenuItem::separator;

    // The app menu: macOS titles it with the app's name whatever its text.
    let app = Submenu::new(APP, true);
    let about = AboutMetadata { name: Some(APP.to_owned()), version: Some(env!("CARGO_PKG_VERSION").to_owned()), ..Default::default() };
    app.append(&PredefinedMenuItem::about(Some(&t!("menu.app.about", app = APP)), Some(about)))?;
    app.append(&sep())?;
    if let Some(v) = views.iter().find(|v| v.id == "app.preferences") {
        append(&app, &item(v))?;
        app.append(&sep())?;
    }
    app.append(&PredefinedMenuItem::services(Some(&t!("menu.app.services"))))?;
    app.append(&sep())?;
    app.append(&PredefinedMenuItem::hide(Some(&t!("menu.app.hide", app = APP))))?;
    app.append(&PredefinedMenuItem::hide_others(Some(&t!("menu.app.hide_others"))))?;
    app.append(&PredefinedMenuItem::show_all(Some(&t!("menu.app.show_all"))))?;
    app.append(&sep())?;
    // Not AppKit's Quit, which ends the app at once: the app's, which asks about the modified
    // documents first (docs/ui/file-tabs.md).
    if let Some(v) = views.iter().find(|v| v.id == "app.quit") {
        append(&app, &item(v))?;
    }
    bar.append(&app)?;

    let file = Submenu::new(Menu::File.label(), true);
    let mut first = true;
    // Quit is in the app menu.
    for v in views.iter().filter(|v| v.menu == Menu::File && v.id != "app.quit") {
        if v.group && !first {
            file.append(&sep())?;
        }
        first = false;
        append(&file, &item(v))?;
    }
    bar.append(&file)?;

    let window = Submenu::new(t!("menu.window.title"), true);
    window.append(&PredefinedMenuItem::minimize(Some(&t!("menu.window.minimize"))))?;
    window.append(&PredefinedMenuItem::maximize(Some(&t!("menu.window.zoom"))))?;
    window.append(&sep())?;
    window.append(&PredefinedMenuItem::fullscreen(Some(&t!("menu.window.fullscreen"))))?;
    window.append(&sep())?;
    window.append(&PredefinedMenuItem::bring_all_to_front(Some(&t!("menu.window.bring_all_to_front"))))?;
    bar.append(&window)?;

    bar.init_for_nsapp();
    window.set_as_windows_menu_for_nsapp();
    Ok((bar, items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_command_shortcuts_become_key_equivalents() {
        assert!(accelerator(Shortcut::parse("Cmd+Shift+Z")).is_some());
        assert!(accelerator(Shortcut::parse("Cmd+Comma")).is_some());
        assert!(accelerator(Shortcut::parse("R")).is_none());
        assert!(accelerator(Shortcut::parse("Shift+Z")).is_none());
        assert!(accelerator(None).is_none());
    }
}

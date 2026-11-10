//! The command registry (docs/ARCHITECTURE.md §7 quater): every action of the menus, the tool
//! bar and the keyboard, by a stable id (`edit.undo`, `tool.move`, `view.top`…), with its
//! label, its menu and its default shortcut. The app says whether a command is enabled (or
//! checked) and runs it ([`crate::app`]); the command palette, the shortcut preferences and
//! the control channel ([`crate::control`]) go through the same ids.
//!
//! The keys of the active tool (Esc, arrows, Tab, Delete, typed values) are not commands: the
//! editor reads them.

use std::collections::BTreeMap;

use solidsketch_i18n::{ellipsis, t};

/// Where a command shows in the menu bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    File,
    Edit,
    View,
    Tools,
    Solids,
    /// Only with the debug menu (`?debug`, `SOLIDSKETCH_DEBUG`).
    Debug,
}

impl Menu {
    pub const BAR: [Menu; 6] = [Menu::File, Menu::Edit, Menu::View, Menu::Tools, Menu::Solids, Menu::Debug];

    pub fn label(self) -> String {
        match self {
            Menu::File => t!("menu.file.title"),
            Menu::Edit => t!("menu.edit.title"),
            Menu::View => t!("menu.view.title"),
            Menu::Tools => t!("menu.tools.title"),
            Menu::Solids => t!("menu.solids.title"),
            Menu::Debug => t!("menu.debug.title"),
        }
    }
}

/// A command.
#[derive(Clone, Copy, Debug)]
pub struct Command {
    pub id: &'static str,
    /// Its label, in the current language.
    pub label: fn() -> String,
    pub menu: Menu,
    /// A separator goes before it in its menu.
    pub group: bool,
    /// Its default shortcut (`Cmd+Shift+Z`; Cmd is Ctrl off macOS).
    pub shortcut: Option<&'static str>,
}

const fn c(id: &'static str, label: fn() -> String, menu: Menu, shortcut: Option<&'static str>) -> Command {
    Command { id, label, menu, group: false, shortcut }
}

/// The same, with a separator before it.
const fn g(id: &'static str, label: fn() -> String, menu: Menu, shortcut: Option<&'static str>) -> Command {
    Command { id, label, menu, group: true, shortcut }
}

use Menu as M;

/// Every command, in menu order.
pub const COMMANDS: &[Command] = &[
    c("file.new", || t!("menu.file.new"), M::File, Some("Cmd+N")),
    c("file.open", || ellipsis(t!("menu.file.open")), M::File, Some("Cmd+O")),
    c("file.save", || t!("menu.file.save"), M::File, Some("Cmd+S")),
    c("file.save_as", || ellipsis(t!("menu.file.save_as")), M::File, Some("Cmd+Shift+S")),
    c("edit.undo", || t!("menu.edit.undo"), M::Edit, Some("Cmd+Z")),
    c("edit.redo", || t!("menu.edit.redo"), M::Edit, Some("Cmd+Shift+Z")),
    g("edit.select_all", || t!("menu.edit.select_all"), M::Edit, Some("Cmd+A")),
    c("edit.erase", || t!("menu.edit.erase"), M::Edit, None),
    g("edit.make_assembly", || t!("menu.edit.make_assembly"), M::Edit, Some("G")),
    c("edit.explode", || t!("menu.edit.explode"), M::Edit, None),
    c("edit.make_unique", || t!("menu.edit.make_unique"), M::Edit, None),
    g("app.command_palette", || ellipsis(t!("menu.edit.command_palette")), M::Edit, Some("Cmd+K")),
    c("app.preferences", || ellipsis(t!("menu.edit.preferences")), M::Edit, Some("Cmd+Comma")),
    c("view.parallel", || t!("menu.view.parallel"), M::View, None),
    c("view.grid", || t!("menu.view.grid"), M::View, None),
    c("view.axes", || t!("menu.view.axes"), M::View, None),
    c("view.silhouettes", || t!("menu.view.silhouettes"), M::View, None),
    g("view.section", || t!("menu.view.section"), M::View, None),
    c("view.section_clear", || t!("menu.view.section_clear"), M::View, None),
    g("view.materials", || t!("menu.view.materials"), M::View, None),
    g("view.top", || t!("menu.view.top"), M::View, Some("Cmd+1")),
    c("view.front", || t!("menu.view.front"), M::View, Some("Cmd+2")),
    c("view.right", || t!("menu.view.right"), M::View, Some("Cmd+3")),
    c("view.back", || t!("menu.view.back"), M::View, Some("Cmd+4")),
    c("view.left", || t!("menu.view.left"), M::View, Some("Cmd+5")),
    c("view.iso", || t!("menu.view.iso"), M::View, Some("Cmd+6")),
    g("view.zoom_extents", || t!("menu.view.zoom_extents"), M::View, Some("Shift+Z")),
    c("tool.select", || t!("tool.select.name"), M::Tools, Some("Space")),
    c("tool.box", || t!("tool.box.name"), M::Tools, Some("R")),
    c("tool.cylinder", || t!("tool.cylinder.name"), M::Tools, Some("C")),
    c("tool.move", || t!("tool.move.name"), M::Tools, Some("M")),
    c("tool.rotate", || t!("tool.rotate.name"), M::Tools, Some("Q")),
    g("tool.paint", || t!("tool.paint.name"), M::Tools, Some("B")),
    c("tool.section", || t!("tool.section.name"), M::Tools, None),
    c("tool.union", || t!("tool.boolean.union.name"), M::Solids, None),
    c("tool.difference", || t!("tool.boolean.difference.name"), M::Solids, None),
    c("tool.intersection", || t!("tool.boolean.intersection.name"), M::Solids, None),
    c("debug.crash_kernel", || t!("menu.debug.crash_kernel"), M::Debug, None),
];

pub fn find(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// A key and its modifiers. `command` is Cmd on macOS, Ctrl elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub key: egui::Key,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    /// Reads `Cmd+Shift+Z`, `Shift+Z`, `Space`, `Comma`… (egui's key names).
    pub fn parse(text: &str) -> Option<Shortcut> {
        let mut s = Shortcut { key: egui::Key::Space, command: false, shift: false, alt: false };
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "cmd" | "ctrl" | "command" => s.command = true,
                "shift" => s.shift = true,
                "alt" | "option" => s.alt = true,
                _ => key = Some(egui::Key::from_name(part)?),
            }
        }
        s.key = key?;
        Some(s)
    }

    /// As stored: `Cmd+Shift+Z`.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (on, name) in [(self.command, "Cmd+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                out.push_str(name);
            }
        }
        out.push_str(self.key.name());
        out
    }

    /// As shown on this platform: `⇧⌘Z` on macOS, `Ctrl+Maj+Z` elsewhere.
    pub fn display(&self, mac: bool) -> String {
        let key = match self.key {
            egui::Key::Comma => ",".to_owned(),
            egui::Key::Space => t!("key.space"),
            egui::Key::Delete => t!("key.delete"),
            k => k.symbol_or_name().to_owned(),
        };
        if mac {
            let mut out = String::new();
            for (on, sym) in [(self.alt, "⌥"), (self.shift, "⇧"), (self.command, "⌘")] {
                if on {
                    out.push_str(sym);
                }
            }
            out + &key
        } else {
            let mut parts = Vec::new();
            for (on, name) in [(self.command, t!("key.ctrl")), (self.alt, t!("key.alt")), (self.shift, t!("key.shift"))] {
                if on {
                    parts.push(name);
                }
            }
            parts.push(key);
            parts.join("+")
        }
    }

    /// Does a key press with these modifiers make this shortcut? (`command` as egui gives it:
    /// Cmd on macOS, Ctrl elsewhere.)
    pub fn matches(&self, key: egui::Key, m: egui::Modifiers) -> bool {
        self.key == key && self.command == m.command && self.shift == m.shift && self.alt == m.alt
    }
}

/// The shortcuts in use: the defaults, with the user's changes (`id → shortcut`, an empty
/// one for none) over them.
#[derive(Clone, Debug)]
pub struct Keymap {
    map: Vec<(&'static str, Shortcut)>,
}

impl Keymap {
    pub fn new(overrides: &BTreeMap<String, String>) -> Self {
        let map = COMMANDS
            .iter()
            .filter_map(|c| {
                let text = overrides.get(c.id).map(String::as_str).or(c.shortcut)?;
                Some((c.id, Shortcut::parse(text)?))
            })
            .collect();
        Keymap { map }
    }

    pub fn get(&self, id: &str) -> Option<Shortcut> {
        self.map.iter().find(|(c, _)| *c == id).map(|(_, s)| *s)
    }

    /// The command a key press runs.
    pub fn command_for(&self, key: egui::Key, m: egui::Modifiers) -> Option<&'static str> {
        self.map.iter().find(|(_, s)| s.matches(key, m)).map(|(c, _)| *c)
    }

    /// The other commands bound to `s`.
    pub fn conflicts(&self, id: &str, s: Shortcut) -> Vec<&'static str> {
        self.map.iter().filter(|(c, t)| *c != id && *t == s).map(|(c, _)| *c).collect()
    }
}

/// Does `query` find the command (every word in its label or id, case and accents ignored)?
pub fn matches(query: &str, c: &Command) -> bool {
    let fold = |s: &str| {
        s.to_lowercase()
            .chars()
            .map(|ch| match ch {
                'à' | 'â' | 'ä' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'î' | 'ï' => 'i',
                'ô' | 'ö' => 'o',
                'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                ch => ch,
            })
            .collect::<String>()
    };
    let hay = fold(&format!("{} {}", (c.label)(), c.id));
    fold(query).split_whitespace().all(|w| hay.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_defaults_parse_without_conflict() {
        let mut ids: Vec<_> = COMMANDS.iter().map(|c| c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), COMMANDS.len());
        for c in COMMANDS {
            if let Some(s) = c.shortcut {
                let parsed = Shortcut::parse(s).unwrap_or_else(|| panic!("{}: {s}", c.id));
                assert_eq!(parsed.text(), s, "{} round trip", c.id);
            }
        }
        let keymap = Keymap::new(&BTreeMap::new());
        for c in COMMANDS {
            if let Some(s) = keymap.get(c.id) {
                assert!(keymap.conflicts(c.id, s).is_empty(), "{} shares {}", c.id, s.text());
            }
        }
    }

    #[test]
    fn shortcuts_read_show_and_match() {
        let s = Shortcut::parse("Cmd+Shift+Z").unwrap();
        assert_eq!(s.display(true), "⇧⌘Z");
        assert_eq!(s.display(false), "Ctrl+Maj+Z");
        let m = egui::Modifiers { command: true, shift: true, ..Default::default() };
        assert!(s.matches(egui::Key::Z, m));
        assert!(!s.matches(egui::Key::Z, egui::Modifiers { command: true, ..Default::default() }));
        assert_eq!(Shortcut::parse("Cmd+Comma").unwrap().display(true), "⌘,");
        assert!(Shortcut::parse("Cmd+Nope").is_none());
    }

    #[test]
    fn user_shortcuts_replace_or_remove_defaults() {
        let overrides = BTreeMap::from([("tool.box".to_owned(), "Shift+B".to_owned()), ("tool.move".to_owned(), String::new())]);
        let k = Keymap::new(&overrides);
        assert_eq!(k.command_for(egui::Key::B, egui::Modifiers::SHIFT), Some("tool.box"));
        assert_eq!(k.command_for(egui::Key::R, egui::Modifiers::NONE), None);
        assert_eq!(k.get("tool.move"), None, "an empty shortcut removes it");
        assert_eq!(k.command_for(egui::Key::B, egui::Modifiers::NONE), Some("tool.paint"));
    }

    #[test]
    fn the_palette_finds_by_words_without_accents() {
        let find = |q: &str| COMMANDS.iter().filter(|c| matches(q, c)).map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(find("etendu"), ["view.zoom_extents"]);
        assert!(find("coupe").contains(&"tool.section"));
        assert_eq!(find("tool box"), ["tool.box"]);
    }
}

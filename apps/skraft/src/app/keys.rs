//! The keyboard: shortcuts, what goes to the tool, the modifiers tapped alone.

use super::*;

impl SkraftApp {
    /// A modifier tapped alone goes to the tool: the copy modifier (Option on macOS, Ctrl
    /// elsewhere) toggles copying (SketchUp; the box: its base centred, or its height), Shift
    /// (with the copy modifier held, a shape tool takes the whole face) and the other modifier
    /// (Command on macOS, Alt elsewhere: a shape tool measures from a corner). Told on release,
    /// and only if no key, click or scroll, nor another modifier pressed, came in between: Ctrl+Z
    /// and the like are left alone, and so is the copy modifier held while Shift is tapped.
    fn modifier_taps(&mut self, ctx: &egui::Context) {
        // The Tabs taken from egui for the tool interrupt a tap too (Shift+Tab is no Shift tap).
        let (mods, events) = ctx.input(|i| (i.modifiers, i.events.iter().chain(&self.tabs).any(interrupts_tap)));
        let other_key = if ctx.os() == egui::os::OperatingSystem::Mac { mods.mac_cmd } else { mods.alt };
        let down = [viewport::modifiers(ctx, mods).copy, mods.shift, other_key];
        let pressed: Vec<bool> = down.iter().zip(&self.taps).map(|(d, t)| *d && !t.down).collect();
        for (k, key) in [Key::Copy, Key::Shift, Key::Alt].into_iter().enumerate() {
            // A modifier pressed during a navigation gesture is the gesture's (docs/ui/navigation-styles.md).
            let other = events || self.viewport.navigating() || pressed.iter().enumerate().any(|(j, p)| j != k && *p);
            if self.taps[k].feed(down[k], other)
                && !ctx.egui_wants_keyboard_input()
                && let Some(d) = self.docs.active_mut()
            {
                d.editor.input(ToolInput::Key { key, mods: Default::default() });
            }
        }
    }

    /// Keyboard: the tool's keys, the measurements box fed from anywhere (SketchUp), then the
    /// commands' shortcuts.
    /// Do the keys go to the shown document's tool and the shortcuts ([`Self::keyboard`]), no
    /// modal dialog (closing, naming a part) taking them?
    pub(super) fn keys_to_tools(&self) -> bool {
        self.closing.as_ref().is_none_or(|c| c.asking.is_none())
            && self.naming.is_none()
            && self
                .docs
                .active()
                .is_none_or(|d| d.sheet.picker.is_none() && (d.workspace.stage != Stage::Sheet || d.sheet.framer.is_none()))
    }

    pub(super) fn keyboard(&mut self, ctx: &egui::Context) {
        self.modifier_taps(ctx);
        let tabs = std::mem::take(&mut self.tabs);
        if ctx.egui_wants_keyboard_input() || self.capturing.is_some() || self.palette.is_some() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in tabs.into_iter().chain(events) {
            if let Some(id) = self.key_event(ctx, ev) {
                self.act(Action::Command(id));
            }
        }
    }

    /// One keyboard event: to the shown document's tool or measurements box, or the command its
    /// shortcut runs (given back).
    fn key_event(&mut self, ctx: &egui::Context, ev: egui::Event) -> Option<&'static str> {
        // The clipboard's keys come as these (desktop and web alike), not as shortcuts; done
        // only when they can be, without a word otherwise. ⌘V pastes only while the system
        // clipboard still holds the app's token (docs/tools/copy-paste.md).
        let clipboard = match &ev {
            egui::Event::Cut => Some("edit.cut"),
            egui::Event::Copy => Some("edit.copy"),
            egui::Event::Paste(text) if self.clipboard.as_ref().is_some_and(|c| c.is_on_system_clipboard(text)) => Some("edit.paste"),
            _ => None,
        };
        if let Some(id) = clipboard {
            return self.enabled(id).then_some(id);
        }
        let egui::Event::Key { key, pressed: true, modifiers, .. } = ev else {
            if let egui::Event::Text(t) = ev
                && let Some(d) = self.docs.active_mut()
            {
                // Shift with + / − (the numeric keypad's too; − shifted types `_`): the copies'
                // count, when the tool takes it and nothing is typed (OCL's SmartHandle).
                let shift = ctx.input(|i| i.modifiers.shift);
                let step = match t.as_str() {
                    "+" => Some(Key::More),
                    "-" | "_" | "−" => Some(Key::Fewer),
                    _ => None,
                };
                if let Some(key) = step
                    && shift
                    && d.vcb.is_empty()
                    && d.editor.takes_counts()
                {
                    d.editor.input(ToolInput::Key { key, mods: Default::default() });
                    return None;
                }
                // `*5` and `/5` (arrays), `@` (the current value), `(`, and points `[x;y;z]`,
                // `<dx;dy;dz>` start a value too.
                let starts_value = t.starts_with(|c: char| {
                    c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ',' | '*' | '/' | '@' | '(' | '[' | '<' | ';')
                });
                let takes = if d.workspace.stage == Stage::Sheet { d.sheet.vcb().is_some() } else { d.editor.takes_typing() };
                if takes && (!d.vcb.is_empty() || starts_value) {
                    d.vcb.push_str(&t);
                }
            }
            return None;
        };
        use egui::Key as K;
        // The tabs' fixed keys, whatever the shortcuts (docs/ui/file-tabs.md): Ctrl+Tab
        // (Ctrl on macOS too) and ⌘T / Ctrl+T beside ⌘N.
        if key == K::Tab && modifiers.ctrl {
            return Some(if modifiers.shift { "view.previous_tab" } else { "view.next_tab" });
        }
        if key == K::T && modifiers.command && !modifiers.shift && !modifiers.alt {
            return Some("file.new");
        }
        // The web sends these as keys too, before its clipboard events: done by those.
        if modifiers.command_only() && matches!(key, K::X | K::C | K::V) {
            return None;
        }
        // The shortcuts of the commands available in the workspace shown (decision 0047).
        let w = self.workspace();
        let command = || self.keymap.command_for(key, modifiers, |id| w.has(id));
        let Some(d) = self.docs.active_mut() else { return command() };
        let typing = !d.vcb.is_empty();
        // On a sheet, its own measurements box (docs/ui/drawing.md#measurements); Delete or
        // Backspace erases what is picked; the 3D tool hears nothing.
        if d.workspace.stage == Stage::Sheet {
            if typing {
                match key {
                    K::Enter => d.sheet.typed = Some(std::mem::take(&mut d.vcb)),
                    K::Backspace => {
                        d.vcb.pop();
                    }
                    K::Escape => {
                        d.vcb.clear();
                        d.sheet.esc_eaten = true;
                    }
                    _ => {}
                }
                return None;
            }
            if !(modifiers.command || modifiers.alt || modifiers.ctrl) && matches!(key, K::Delete | K::Backspace) {
                return Some("edit.erase");
            }
            return self.keymap.command_for(key, modifiers, |id| w.has(id));
        }
        let m = viewport::modifiers(ctx, modifiers);
        let tool_key = |k: Key| ToolInput::Key { key: k, mods: m };
        let plain = !(modifiers.command || modifiers.alt || modifiers.ctrl);
        let e = &mut d.editor;
        match key {
            K::Enter if typing => {
                let text = std::mem::take(&mut d.vcb);
                e.input(ToolInput::Vcb(text));
            }
            K::Backspace if typing => {
                d.vcb.pop();
            }
            K::Escape if typing => d.vcb.clear(),
            _ if typing => {}
            K::Escape => e.input(tool_key(Key::Escape)),
            K::Delete | K::Backspace if plain => e.input(tool_key(Key::Delete)),
            K::ArrowLeft => e.input(tool_key(Key::Left)),
            K::ArrowRight => e.input(tool_key(Key::Right)),
            K::ArrowUp => e.input(tool_key(Key::Up)),
            K::ArrowDown => e.input(tool_key(Key::Down)),
            K::Tab => e.input(tool_key(Key::Tab)),
            // Enter with nothing typed: the tool's (the prism tool closes its outline).
            K::Enter => e.input(tool_key(Key::Enter)),
            _ => return self.keymap.command_for(key, modifiers, |id| w.has(id)),
        }
        None
    }
}

/// Does `e` make a held copy modifier something else than a tap (a shortcut, a click, a
/// scroll)? The modifier keys themselves come as key events too: they do not.
/// A modifier followed to tell a tap of it alone: down, then up, with nothing else in between.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Tap {
    pub(super) down: bool,
    /// Down, and nothing else happened since it went down.
    pub(super) tap: bool,
}

impl Tap {
    /// This frame: the modifier down or not, and whether something `other` happened; true on
    /// the release that ends a tap.
    pub(super) fn feed(&mut self, down: bool, other: bool) -> bool {
        if down && (!self.down || other) {
            self.tap = !other;
        }
        let tapped = !down && self.down && self.tap;
        if !down {
            self.tap = false;
        }
        self.down = down;
        tapped
    }
}

pub(super) fn interrupts_tap(e: &egui::Event) -> bool {
    use egui::Key as K;
    let modifier = |k: &K| {
        matches!(
            k,
            K::ShiftLeft | K::ShiftRight | K::ControlLeft | K::ControlRight | K::AltLeft | K::AltRight | K::SuperLeft | K::SuperRight
        )
    };
    match e {
        egui::Event::Key { key, physical_key, .. } => !(modifier(key) || physical_key.as_ref().is_some_and(modifier)),
        egui::Event::PointerButton { .. } | egui::Event::MouseWheel { .. } => true,
        _ => false,
    }
}

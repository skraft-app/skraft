//! Named handles (docs/control-protocol.md, after SolidCraft's `ui-registry`): widgets
//! publish where they were drawn, under a stable name (`ribbon:tool.box`, `tab:0`,
//! `viewport`…), so a script clicks a name, not a pixel (`ui.handles`, `ui.click` with
//! `handle`).
//!
//! Per thread (the UI's), double-buffered: what the frame draws, and what the last whole frame
//! drew, which is what is answered. Off — nothing kept, nothing costs — unless the control
//! channel is on ([`set_enabled`]).

use std::cell::{Cell, RefCell};

/// A widget where it was drawn (points, window coordinates), and whether it takes a click.
#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
    pub name: String,
    pub rect: egui::Rect,
    pub enabled: bool,
}

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static FRAME: RefCell<Vec<Handle>> = const { RefCell::new(Vec::new()) };
    static LAST: RefCell<Vec<Handle>> = const { RefCell::new(Vec::new()) };
}

/// Keeps handles (the control channel on) or not.
pub fn set_enabled(on: bool) {
    ENABLED.with(|e| e.set(on));
    if !on {
        FRAME.with(|f| f.borrow_mut().clear());
        LAST.with(|f| f.borrow_mut().clear());
    }
}

pub fn enabled() -> bool {
    ENABLED.with(Cell::get)
}

/// A frame starts: the one before is the last whole one.
pub fn frame_start() {
    if enabled() {
        FRAME.with(|f| LAST.with(|l| *l.borrow_mut() = std::mem::take(&mut *f.borrow_mut())));
    }
}

/// `name` drawn at `rect` this frame, clickable.
pub fn publish(name: impl FnOnce() -> String, rect: egui::Rect) {
    publish_enabled(name, rect, true);
}

/// `name` drawn at `rect` this frame, clickable or not. The name is only made when kept.
pub fn publish_enabled(name: impl FnOnce() -> String, rect: egui::Rect, enabled: bool) {
    if self::enabled() {
        FRAME.with(|f| f.borrow_mut().push(Handle { name: name(), rect, enabled }));
    }
}

/// The handles the last whole frame drew, those starting with `prefix`. A name drawn twice:
/// the last one.
pub fn last(prefix: &str) -> Vec<Handle> {
    LAST.with(|l| {
        let l = l.borrow();
        let mut out: Vec<Handle> = Vec::new();
        for h in l.iter().filter(|h| h.name.starts_with(prefix)) {
            match out.iter_mut().find(|o| o.name == h.name) {
                Some(o) => *o = h.clone(),
                None => out.push(h.clone()),
            }
        }
        out
    })
}

/// The handle named `name` the last whole frame drew.
pub fn find(name: &str) -> Option<Handle> {
    LAST.with(|l| l.borrow().iter().rev().find(|h| h.name == name).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_kept_only_when_on_and_answered_from_the_last_frame() {
        let r = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(30.0, 40.0));
        publish(|| "ribbon:tool.box".into(), r);
        frame_start();
        assert!(last("").is_empty());
        set_enabled(true);
        publish(|| "ribbon:tool.box".into(), r);
        publish_enabled(|| "ribbon:tool.move".into(), r, false);
        // Not answered before its frame is whole.
        assert!(find("ribbon:tool.box").is_none());
        frame_start();
        assert_eq!(find("ribbon:tool.box").map(|h| h.rect), Some(r));
        assert_eq!(last("ribbon:").len(), 2);
        assert!(!find("ribbon:tool.move").unwrap().enabled);
        set_enabled(false);
        assert!(find("ribbon:tool.box").is_none());
    }
}

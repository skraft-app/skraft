//! The rows of the menus (docs/ui/interface.md#widgets).

use crate::panels::CommandView;

/// Whether a command is shown in a menu as a check box (a display toggle).
pub fn is_toggle(v: &CommandView) -> bool {
    v.checked.is_some() && v.id.starts_with("view.")
}

/// A menu item: a check box for a toggle, else a button with its shortcut; no hover text.
/// `column` when the menu has check boxes: the label after their column. True when clicked.
pub fn command_item(ui: &mut egui::Ui, v: &CommandView, column: bool) -> bool {
    let mut button = if is_toggle(v) {
        crate::style::menu_item(&v.label, v.checked, column)
    } else {
        crate::style::menu_item(&v.label, None, column).selected(v.checked == Some(true))
    };
    if let Some(s) = &v.shortcut {
        button = button.shortcut_text(s);
    }
    let r = ui.add_enabled(v.enabled, button);
    crate::handles::publish_enabled(|| format!("menu:{}", v.id), r.rect, v.enabled);
    r.clicked()
}

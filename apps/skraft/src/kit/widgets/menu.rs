//! The rows of the menus (docs/ui/interface.md#widgets).

use crate::commands::CommandView;
use crate::kit::icons;
use crate::kit::style::Tip;

/// Whether a command is shown in a menu as a check box (a display toggle).
pub fn is_toggle(v: &CommandView) -> bool {
    v.checked.is_some() && v.id.starts_with("view.")
}

/// A menu item: a check box for a toggle, else a button with its shortcut; no hover text.
/// `column` when the menu has check boxes: the label after their column. True when clicked.
pub fn command_item(ui: &mut egui::Ui, v: &CommandView, column: bool) -> bool {
    let mut button = if is_toggle(v) {
        crate::kit::style::menu_item(&v.label, v.checked, column)
    } else {
        crate::kit::style::menu_item(&v.label, None, column).selected(v.checked == Some(true))
    };
    if let Some(s) = &v.shortcut {
        button = button.shortcut_text(s);
    }
    let r = ui.add_enabled(v.enabled, button);
    crate::kit::handles::publish_enabled(|| format!("menu:{}", v.id), r.rect, v.enabled);
    r.clicked()
}

/// An entry of a menu that is not a command's (a right click's on a row, a card's item): its
/// label, as [`command_item`] draws a command's — the menus' look in one place. Enabled or not,
/// published under a handle when it has one, its tip when it needs one; clicked?
pub(crate) struct MenuEntry {
    label: String,
    icon: Option<egui::ImageSource<'static>>,
    shortcut: Option<String>,
    enabled: bool,
    handle: Option<String>,
    tip: Option<String>,
}

impl MenuEntry {
    pub(crate) fn new(label: impl Into<String>) -> Self {
        MenuEntry { label: label.into(), icon: None, shortcut: None, enabled: true, handle: None, tip: None }
    }

    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// An icon before its label (the view bar's views, as the ▾ menu's rows).
    pub(crate) fn icon(mut self, icon: egui::ImageSource<'static>) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Its shortcut, at the row's right end.
    pub(crate) fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Its handle for the control protocol (`navigator:menu:rename`).
    pub(crate) fn handle(mut self, handle: impl Into<String>) -> Self {
        self.handle = Some(handle.into());
        self
    }

    pub(crate) fn tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    /// Laid out in the menu `ui`: clicked?
    pub(crate) fn show(self, ui: &mut egui::Ui) -> bool {
        let mut button = match self.icon {
            Some(icon) => {
                let image = egui::Image::new(icon).fit_to_exact_size(egui::Vec2::splat(16.0)).tint(ui.visuals().text_color());
                egui::Button::new((image, self.label))
            }
            None => crate::kit::style::menu_item(self.label, None, false),
        };
        if let Some(s) = self.shortcut {
            button = button.shortcut_text(s);
        }
        let r = ui.add_enabled(self.enabled, button);
        if let Some(handle) = self.handle {
            crate::kit::handles::publish_enabled(|| handle, r.rect, self.enabled);
        }
        match self.tip {
            Some(tip) => r.tip(tip).clicked(),
            None => r.clicked(),
        }
    }
}

/// A row of a menu listing commands to pin (the ribbon's groups, the view bar's and the quick
/// tools' ▾).
pub(crate) struct PinItem<'a> {
    pub(crate) id: &'static str,
    pub(crate) icon: Option<egui::ImageSource<'static>>,
    pub(crate) label: &'a str,
    pub(crate) shortcut: Option<&'a str>,
    pub(crate) enabled: bool,
    /// A toggle's state, or whether a tool is the active one.
    pub(crate) checked: Option<bool>,
    pub(crate) pinned: bool,
    /// A click on the row runs it (not a row whose menu is its button's).
    pub(crate) runs: bool,
}

impl<'a> PinItem<'a> {
    /// A command's row, as the command shows now.
    pub(crate) fn command(c: &'a CommandView, pinned: bool) -> PinItem<'a> {
        PinItem {
            id: c.id,
            icon: icons::command(c.id),
            label: &c.label,
            shortcut: c.shortcut.as_deref(),
            enabled: c.enabled,
            checked: c.checked,
            pinned,
            runs: true,
        }
    }
}

/// A pin menu's texts: the pin's tip (its row pinned or not) and the note under the rows.
pub(crate) struct PinTexts {
    pub(crate) tip: fn(bool) -> String,
    pub(crate) note: fn() -> String,
}

/// How a pin menu opens and shows its rows.
pub(crate) struct PinMenu {
    pub(crate) id: egui::Id,
    /// Where it opens from its button (egui's default: below).
    pub(crate) align: Option<egui::RectAlign>,
    /// A column of check boxes every row keeps, each toggle's state in it (the View menu's), in
    /// place of lighting the checked rows.
    pub(crate) checks: bool,
    pub(crate) texts: PinTexts,
}

/// What was done in a pin menu: a row run (the menu then closes), a pin clicked (it stays open).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PinEvent {
    Run(&'static str),
    Pin(&'static str),
}

/// A pin menu opened by `response`: every row with its pin, then the note. Its rows are built
/// only while it is open.
pub(crate) fn pin_menu<'a>(response: &egui::Response, menu: &PinMenu, items: impl FnOnce() -> Vec<PinItem<'a>>) -> Option<PinEvent> {
    let mut popup = egui::Popup::menu(response)
        .id(menu.id)
        .style(crate::kit::style::menu)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    if let Some(align) = menu.align {
        popup = popup.align(align);
    }
    popup.show(|ui| pin_list(ui, &items(), menu.checks, &menu.texts)).and_then(|r| r.inner)
}

/// A pin menu's content, in a menu opened otherwise (a submenu).
pub(crate) fn pin_list(ui: &mut egui::Ui, items: &[PinItem], checks: bool, texts: &PinTexts) -> Option<PinEvent> {
    let mut event = None;
    for item in items {
        let (run, pin) = pin_row(ui, item, checks, texts);
        if run.clicked() && item.runs {
            event = Some(PinEvent::Run(item.id));
            ui.close();
        }
        if pin {
            event = Some(PinEvent::Pin(item.id));
        }
    }
    // The note wraps within the rows' width: the rows set the menu's, never the note.
    let width = ui.min_rect().width();
    ui.separator();
    ui.set_max_width(width);
    ui.add(egui::Label::new(crate::kit::style::caption((texts.note)())).wrap());
    event
}

/// Right click on a pinned button: its menu, « Retirer de… » (the pin's tip when pinned), then
/// `more`, the bar's own items, after a separator. True when it was chosen.
pub(crate) fn unpin_menu(response: &egui::Response, texts: &PinTexts, more: Option<&mut dyn FnMut(&mut egui::Ui)>) -> bool {
    let mut unpin = false;
    egui::Popup::context_menu(response).style(crate::kit::style::menu).show(|ui| {
        // Its label in the check boxes' column the bar's items may have.
        let column = more.is_some();
        unpin = ui.add(crate::kit::style::menu_item((texts.tip)(true), None, column)).clicked();
        if let Some(more) = more {
            ui.separator();
            more(ui);
        }
    });
    unpin
}

/// The least width of a pin row's button, the same in every pin menu: the pins line up in a
/// column the longest label still fits before.
const PIN_ROW_WIDTH: f32 = 240.0;

/// A row and its pin, side by side: the row's response (no tip: it would say its label again),
/// and whether the pin was clicked.
fn pin_row(ui: &mut egui::Ui, row: &PinItem, checks: bool, texts: &PinTexts) -> (egui::Response, bool) {
    ui.horizontal(|ui| {
        let ink = ui.visuals().text_color();
        let image = row.icon.clone().map(|icon| egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0)).tint(ink));
        let check = crate::kit::style::menu_check(row.checked.filter(|_| checks));
        let button = match (checks, image) {
            (true, Some(image)) => egui::Button::new((check, image, row.label)),
            (true, None) => egui::Button::new((check, row.label)),
            (false, Some(image)) => egui::Button::image_and_text(image, row.label),
            (false, None) => egui::Button::new(row.label),
        };
        // Lit when checked, but a check box tells a toggle's state.
        let selected = !checks && row.checked == Some(true);
        // Always a shortcut, if empty: it pushes the label to the left (else centred).
        let button = button.selected(selected).min_size(egui::vec2(PIN_ROW_WIDTH, 22.0)).shortcut_text(row.shortcut.unwrap_or(""));
        let run = ui.add_enabled(row.enabled, button);
        crate::kit::handles::publish_enabled(|| format!("pin_menu:{}", row.id), run.rect, row.enabled);
        // Pinned: a filled pin in the accent; unpinned: a faint outline.
        let (icon, tint) = if row.pinned {
            (icons::PIN_FILLED, ui.visuals().selection.stroke.color)
        } else {
            (icons::PIN, ui.visuals().weak_text_color().gamma_multiply(0.55))
        };
        let pin = egui::Button::image(egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint)).frame(false);
        let pin = ui.add(pin).tip((texts.tip)(row.pinned)).clicked();
        (run, pin)
    })
    .inner
}

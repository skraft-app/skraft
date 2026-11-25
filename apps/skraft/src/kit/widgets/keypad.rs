//! The numeric keypad of the measurements box under a finger (docs/ui/touch.md#the-keypad): the
//! app's own, not the system's keyboard, which would cover half the screen and the model.

use egui::{Sense, vec2};

/// A key's side (points): a finger's.
const KEY: f32 = 48.0;
const GAP: f32 = 6.0;

/// What a key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeypadAct {
    /// `text` changed.
    Typed,
    /// Enter: the value typed goes to the tool.
    Enter,
}

/// The keys, row by row: digits, the decimal and list separators (the system's), the minus,
/// erasing and Enter.
fn keys() -> [[char; 4]; 4] {
    let decimal = skraft_i18n::decimal_separator();
    let list = skraft_i18n::list_separator();
    [['7', '8', '9', '⌫'], ['4', '5', '6', list], ['1', '2', '3', '-'], ['0', decimal, '⏎', '⏎']]
}

/// Draws the keypad, editing `text`; Enter spans the last two keys of its row.
pub fn keypad(ui: &mut egui::Ui, text: &mut String) -> Option<KeypadAct> {
    let (rect, _) = ui.allocate_exact_size(vec2(4.0 * KEY + 3.0 * GAP, 4.0 * KEY + 3.0 * GAP), Sense::hover());
    let mut act = None;
    for (r, row) in keys().iter().enumerate() {
        let mut c = 0;
        while c < 4 {
            let key = row[c];
            let span = if key == '⏎' { 2 } else { 1 };
            let min = rect.min + vec2(c as f32 * (KEY + GAP), r as f32 * (KEY + GAP));
            let size = vec2(span as f32 * KEY + (span - 1) as f32 * GAP, KEY);
            let cell = egui::Rect::from_min_size(min, size);
            let id = ui.scope_id().with(("keypad", r, c));
            let response = ui.interact(cell, id, Sense::click());
            crate::kit::handles::publish(|| format!("keypad:{}", key_name(key)), cell);
            let visuals = ui.style().interact(&response);
            let enter = key == '⏎';
            let fill = if enter { ui.visuals().selection.bg_fill } else { visuals.weak_bg_fill };
            ui.painter().rect(cell, 6.0, fill, visuals.bg_stroke, egui::StrokeKind::Inside);
            let label = match key {
                '⏎' => skraft_i18n::t!("keypad.enter"),
                k => k.to_string(),
            };
            let ink = if enter { ui.visuals().selection.stroke.color } else { visuals.text_color() };
            ui.painter().text(cell.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(20.0), ink);
            if response.clicked() {
                act = Some(match key {
                    '⏎' => KeypadAct::Enter,
                    '⌫' => {
                        text.pop();
                        KeypadAct::Typed
                    }
                    k => {
                        text.push(k);
                        KeypadAct::Typed
                    }
                });
            }
            c += span;
        }
    }
    act
}

/// A key's name in its handle (`keypad:<name>`).
fn key_name(key: char) -> String {
    match key {
        '⌫' => "erase".into(),
        '⏎' => "enter".into(),
        '-' => "minus".into(),
        k if k.is_ascii_digit() => k.to_string(),
        k if k == skraft_i18n::decimal_separator() => "decimal".into(),
        _ => "list".into(),
    }
}

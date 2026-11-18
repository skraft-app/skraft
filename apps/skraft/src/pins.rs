//! The pinned commands of the quick tools' card (§6 terdecies) and of the view bar
//! (§6 sexdecies): which ones and in what order, as the user left them; and moving one of
//! their buttons by a drag, a line showing where it will go.

use egui::{ImageSource, Rect, Response, vec2};

/// The pinned commands among `all`, in the user's order: `pins` as they left them (unknown
/// ids and repeats ignored), else `default`.
pub fn pinned(all: &[&'static str], default: &[&'static str], pins: &Option<Vec<String>>) -> Vec<&'static str> {
    let Some(ids) = pins else { return default.to_vec() };
    let mut out: Vec<&'static str> = Vec::new();
    for i in ids {
        if let Some(c) = all.iter().copied().find(|c| c == i)
            && !out.contains(&c)
        {
            out.push(c);
        }
    }
    out
}

/// Pins `id` (at the end), or unpins it; back to `default`, nothing kept.
pub fn toggle(all: &[&'static str], default: &[&'static str], pins: &mut Option<Vec<String>>, id: &'static str) {
    let mut now = pinned(all, default, pins);
    if now.contains(&id) {
        now.retain(|c| *c != id);
    } else {
        now.push(id);
    }
    keep(default, pins, now);
}

/// Moves the pinned `id` before `before` (none: at the end); back to `default`, nothing kept.
pub fn move_to(
    all: &[&'static str],
    default: &[&'static str],
    pins: &mut Option<Vec<String>>,
    id: &'static str,
    before: Option<&'static str>,
) {
    let mut now = pinned(all, default, pins);
    if before == Some(id) || !now.contains(&id) {
        return;
    }
    now.retain(|c| *c != id);
    let at = before.and_then(|b| now.iter().position(|c| *c == b)).unwrap_or(now.len());
    now.insert(at, id);
    keep(default, pins, now);
}

fn keep(default: &[&'static str], pins: &mut Option<Vec<String>>, now: Vec<&'static str>) {
    *pins = (now != default).then(|| now.into_iter().map(str::to_owned).collect());
}

/// A pinned command's button, as drawn this frame (its response senses drags).
pub struct Item {
    pub id: &'static str,
    pub rect: Rect,
    pub response: Response,
    pub icon: ImageSource<'static>,
}

/// Moving a button of `items` (a row, or a column when `vertical`) by a drag: past egui's
/// drag threshold (a click stays a click) the button follows the pointer, faded, a line shows
/// where it will be put, and once let go gives the move: the button, and the one it goes
/// before (none: at the end). `key` keeps which one is dragged between frames.
pub fn reorder(ui: &egui::Ui, key: egui::Id, vertical: bool, items: &[Item]) -> Option<(&'static str, Option<&'static str>)> {
    if let Some(it) = items.iter().find(|it| it.response.drag_started()) {
        ui.ctx().data_mut(|d| d.insert_temp(key, it.id));
    }
    let dragged: &'static str = ui.ctx().data(|d| d.get_temp(key))?;
    let Some(from) = items.iter().position(|it| it.id == dragged) else {
        ui.ctx().data_mut(|d| d.remove::<&'static str>(key));
        return None;
    };
    let pointer = ui.ctx().pointer_latest_pos()?;
    let along = |p: egui::Pos2| if vertical { p.y } else { p.x };
    let at = items.iter().position(|it| along(it.rect.center()) > along(pointer)).unwrap_or(items.len());
    let before = items.get(at).map(|it| it.id);
    let moves = at != from && at != from + 1;
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, key.with("drop")));
    let accent = ui.visuals().selection.stroke.color;
    if moves {
        // Between two buttons, or past the last one.
        let gap = ui.spacing().item_spacing;
        let line = match items.get(at) {
            Some(b) if vertical => {
                Rect::from_x_y_ranges(b.rect.x_range(), (b.rect.top() - gap.y / 2.0 - 1.0)..=(b.rect.top() - gap.y / 2.0 + 1.0))
            }
            Some(b) => Rect::from_x_y_ranges((b.rect.left() - gap.x / 2.0 - 1.0)..=(b.rect.left() - gap.x / 2.0 + 1.0), b.rect.y_range()),
            None => {
                let last = items[items.len() - 1].rect;
                if vertical {
                    Rect::from_x_y_ranges(last.x_range(), (last.bottom() + gap.y / 2.0 - 1.0)..=(last.bottom() + gap.y / 2.0 + 1.0))
                } else {
                    Rect::from_x_y_ranges((last.right() + gap.x / 2.0 - 1.0)..=(last.right() + gap.x / 2.0 + 1.0), last.y_range())
                }
            }
        };
        painter.rect_filled(line, 1.0, accent);
    }
    // The button under the pointer, faded.
    let it = &items[from];
    let ghost = Rect::from_center_size(pointer, it.rect.size());
    painter.rect_filled(ghost, 4.0, ui.visuals().selection.bg_fill.gamma_multiply(0.6));
    let icon = Rect::from_center_size(pointer, vec2(18.0, 18.0));
    egui::Image::new(it.icon.clone()).tint(ui.visuals().text_color().gamma_multiply(0.8)).paint_at(ui, icon);
    ui.ctx().request_repaint();
    if it.response.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
        ui.ctx().data_mut(|d| d.remove::<&'static str>(key));
        return moves.then_some((dragged, before));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[&str] = &["a", "b", "c", "d"];
    const DEFAULT: &[&str] = &["a", "b"];

    #[test]
    fn a_pin_goes_at_the_end_and_a_move_keeps_the_order_back_to_the_defaults() {
        let mut pins = None;
        toggle(ALL, DEFAULT, &mut pins, "d");
        toggle(ALL, DEFAULT, &mut pins, "c");
        assert_eq!(pinned(ALL, DEFAULT, &pins), ["a", "b", "d", "c"]);
        move_to(ALL, DEFAULT, &mut pins, "c", Some("a"));
        assert_eq!(pinned(ALL, DEFAULT, &pins), ["c", "a", "b", "d"]);
        move_to(ALL, DEFAULT, &mut pins, "c", None);
        assert_eq!(pinned(ALL, DEFAULT, &pins), ["a", "b", "d", "c"]);
        // Before itself: nothing moves.
        move_to(ALL, DEFAULT, &mut pins, "d", Some("d"));
        assert_eq!(pinned(ALL, DEFAULT, &pins), ["a", "b", "d", "c"]);
        toggle(ALL, DEFAULT, &mut pins, "c");
        toggle(ALL, DEFAULT, &mut pins, "d");
        assert_eq!(pins, None, "the defaults again: nothing kept");
        // Unknown ids and repeats ignored; the defaults reordered are kept.
        let mut pins = Some(vec!["b".to_owned(), "x".to_owned(), "b".to_owned(), "a".to_owned()]);
        assert_eq!(pinned(ALL, DEFAULT, &pins), ["b", "a"]);
        move_to(ALL, DEFAULT, &mut pins, "a", Some("b"));
        assert_eq!(pins, None);
    }
}

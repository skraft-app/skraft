//! The file tabs in the title bar (docs/ARCHITECTURE.md §6 octies): one per open document,
//! the shown one lit, a bullet after a modified one's name. Click selects, × or a middle click
//! closes, a drag reorders, right click opens the tab's menu; `+` opens a new document and »
//! lists every tab when they do not all fit ([`layout`], a pure function of the width).
//!
//! On the macOS desktop the tabs are part of the window's title bar: dragging the only tab, or
//! a tab mostly up or down, moves the window ([`drag_moves_window`], told once as the drag
//! starts).

use solidsketch_i18n::t;

use crate::icons;

/// A tab as the app describes it this frame.
pub struct Tab {
    /// Its document's id: what follows the tab while tabs move.
    pub id: u64,
    pub name: String,
    /// Its file (none: never saved).
    pub path: Option<String>,
    pub modified: bool,
}

impl Tab {
    /// Its hover text: its full name when cut short, where it is saved, whether it is modified.
    fn tip(&self, cut: bool) -> String {
        let mut lines = Vec::new();
        if cut {
            lines.push(self.name.clone());
        }
        lines.push(self.path.clone().unwrap_or_else(|| t!("tabs.unsaved")));
        if self.modified && self.path.is_some() {
            lines.push(t!("titlebar.modified"));
        }
        lines.join("\n")
    }
}

/// A row of a tab's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuRow {
    Close,
    CloseOthers,
    CloseAll,
    /// Shows its file in the Finder / the file manager (desktop).
    Reveal,
}

/// What the user did on the tabs this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAction {
    Select(usize),
    Close(usize),
    Menu(usize, MenuRow),
    Reorder { from: usize, to: usize },
    New,
}

/// A tab's narrowest and widest, the `+` and » buttons' width.
const MIN_TAB: f32 = 90.0;
const MAX_TAB: f32 = 200.0;
const BUTTON: f32 = 26.0;
const HEIGHT: f32 = 26.0;

/// Which tabs fit in `room` points with `+` after them: (first, how many, tab width). Tabs
/// shrink to [`MIN_TAB`]; past that a run of tabs around `shown` is drawn, the others in the »
/// list (its button then takes room too).
pub fn layout(n: usize, shown: usize, room: f32) -> (usize, usize, f32) {
    if n == 0 {
        return (0, 0, MIN_TAB);
    }
    let avail = room - BUTTON;
    if avail >= n as f32 * MIN_TAB {
        return (0, n, (avail / n as f32).min(MAX_TAB));
    }
    let fit = (((avail - BUTTON) / MIN_TAB).floor().max(1.0) as usize).min(n);
    let first = shown.min(n - 1).saturating_sub(fit - 1).min(n - fit);
    (first, fit, MIN_TAB)
}

/// The place among the drawn tabs (`first..first + count`, `width` wide from `x0`) where a tab
/// dropped with the pointer at `x` goes.
pub fn drop_place(x: f32, x0: f32, width: f32, first: usize, count: usize) -> usize {
    let k = ((x - x0) / width).floor().max(0.0) as usize;
    first + k.min(count.saturating_sub(1))
}

/// Does a tab drag that started by moving `moved` move the window rather than the tab? Only
/// when the tabs are the window's title bar (`window_moves`): the only tab, or one dragged
/// mostly up or down.
pub fn drag_moves_window(n: usize, window_moves: bool, moved: egui::Vec2) -> bool {
    window_moves && (n <= 1 || moved.y.abs() > moved.x.abs())
}

/// Where each drawn tab goes (`first..first + count`, by place along the strip) while tab
/// `from` is dragged over place `to`: the others close up around the gap it leaves and open one
/// where it would land.
pub fn places(first: usize, count: usize, from: usize, to: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (first..first + count).filter(|&i| i != from).collect();
    order.insert((to - first).min(order.len()), from);
    let mut place = vec![0; count];
    for (k, i) in order.into_iter().enumerate() {
        place[i - first] = k;
    }
    place
}

/// The tab dragged along the strip, and where the pointer holds it (from its left edge).
#[derive(Clone, Copy)]
struct Drag {
    from: usize,
    grab: f32,
}

/// Draws `tabs` in `rect` (`shown`: the lit one, none on the start page). `window_moves`: the
/// tabs are in the window's title bar (macOS desktop).
pub fn show(ui: &mut egui::Ui, rect: egui::Rect, tabs: &[Tab], shown: Option<usize>, window_moves: bool, reveal: bool) -> Vec<TabAction> {
    let mut out = Vec::new();
    let id = egui::Id::unique("file_tabs");
    let n = tabs.len();
    let (first, count, width) = layout(n, shown.unwrap_or(0), rect.width());
    let y = rect.center().y - HEIGHT / 2.0;
    let mut drag: Option<Drag> = ui.data(|d| d.get_temp(id.with("drag")));
    let pointer = ui.ctx().pointer_latest_pos();
    let visuals = ui.visuals().clone();
    let font = egui::TextStyle::Body.resolve(ui.style());

    // The tab dragged, if it is drawn: its left edge, under the pointer, kept on the strip, and
    // the place it would land on.
    let last_left = rect.left() + count.saturating_sub(1) as f32 * width;
    let held = match (drag, pointer) {
        (Some(d), Some(p)) if (first..first + count).contains(&d.from) => {
            let left = (p.x - d.grab).clamp(rect.left(), last_left.max(rect.left()));
            Some((d.from, left, drop_place(left + width / 2.0, rect.left(), width, first, count)))
        }
        _ => None,
    };
    let place = match held {
        Some((from, _, to)) => places(first, count, from, to),
        None => (0..count).collect(),
    };
    // The dragged tab is drawn last, over the others.
    let mut order: Vec<usize> = (first..first + count).filter(|&i| held.is_none_or(|(from, ..)| from != i)).collect();
    order.extend(held.map(|(from, ..)| from));
    let dt = ui.input(|i| i.stable_dt).min(0.1);

    for i in order {
        let tab = &tabs[i];
        let dragged = held.is_some_and(|(from, ..)| from == i);
        // Each tab slides to its place (a tab let go slides in from where it was dropped).
        let target = match held {
            Some((from, left, _)) if from == i => left,
            _ => rect.left() + place[i - first] as f32 * width,
        };
        let key = id.with(("x", tab.id));
        let x = match ui.data(|d| d.get_temp::<f32>(key)) {
            Some(prev) if !dragged && (prev - target).abs() > 0.5 => {
                ui.ctx().request_repaint();
                prev + (target - prev) * (1.0 - (-dt * 18.0).exp())
            }
            _ => target,
        };
        ui.data_mut(|d| d.insert_temp(key, x));
        let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width - 2.0, HEIGHT));
        let resp = ui.interact(r, id.with(("tab", i)), egui::Sense::click_and_drag());
        let active = shown == Some(i);
        let hovered = resp.hovered();
        if dragged {
            // Lifted: a shadow, the panels' fill and the accent's outline, unlike any tab at rest
            // (the shown one is filled with the soft accent).
            let shadow = egui::epaint::Shadow { offset: [0, 3], blur: 12, spread: 0, color: egui::Color32::from_black_alpha(90) };
            ui.painter().add(shadow.as_shape(r, 6.0));
            ui.painter().rect(
                r,
                6.0,
                visuals.window_fill,
                egui::Stroke::new(1.5, visuals.selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        } else {
            let fill = if active {
                visuals.widgets.active.weak_bg_fill
            } else if hovered && held.is_none() {
                visuals.widgets.hovered.weak_bg_fill
            } else {
                egui::Color32::TRANSPARENT
            };
            ui.painter().rect_filled(r, 6.0, fill);
        }

        // The × on the lit tab and the hovered one.
        let close = egui::Rect::from_center_size(egui::pos2(r.right() - 12.0, r.center().y), egui::vec2(16.0, 16.0));
        let show_close = active || hovered || ui.rect_contains_pointer(close);
        let text_right = if show_close { close.left() - 4.0 } else { r.right() - 8.0 };
        let label = format!("{}{}", tab.name, if tab.modified { " •" } else { "" });
        let color = if active || dragged { visuals.strong_text_color() } else { visuals.text_color() };
        let galley = egui::WidgetText::from(label).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (text_right - r.left() - 10.0).max(0.0),
            font.clone(),
        );
        let cut = galley.elided;
        ui.painter().galley(egui::pos2(r.left() + 10.0, r.center().y - galley.size().y / 2.0), galley, color);
        if show_close && held.is_none() {
            let x = small_button(ui, close, id.with(("close", i)), Glyph::Icon(icons::CLOSE), 12.0);
            if x.on_hover_text(t!("tabs.close_tip")).clicked() {
                out.push(TabAction::Close(i));
            }
        }

        let resp = if held.is_none() { resp.on_hover_text(tab.tip(cut)) } else { resp };
        if resp.clicked() {
            out.push(TabAction::Select(i));
        }
        if resp.middle_clicked() {
            out.push(TabAction::Close(i));
        }
        egui::Popup::context_menu(&resp).show(|ui| {
            let mut row = |ui: &mut egui::Ui, row: MenuRow, label: String, enabled: bool| {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    out.push(TabAction::Menu(i, row));
                }
            };
            row(ui, MenuRow::Close, t!("tabs.close"), true);
            row(ui, MenuRow::CloseOthers, t!("tabs.close_others"), n > 1);
            row(ui, MenuRow::CloseAll, t!("tabs.close_all"), true);
            if reveal {
                ui.separator();
                let label = if ui.ctx().os() == egui::os::OperatingSystem::Mac { t!("tabs.reveal_finder") } else { t!("tabs.reveal") };
                row(ui, MenuRow::Reveal, label, tab.path.is_some());
            }
        });

        // Dragging, told once as it starts (after SolidCraft): along the strip, the tab moves;
        // in the title bar, the only tab or a tab dragged mostly up or down moves the window.
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin());
            let moved = match (origin, pointer) {
                (Some(o), Some(p)) => p - o,
                _ => egui::Vec2::ZERO,
            };
            if drag_moves_window(n, window_moves, moved) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            } else if n > 1 {
                drag = Some(Drag { from: i, grab: origin.map_or(width / 2.0, |o| o.x - r.left()) });
            }
        }
        if dragged {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if resp.drag_stopped() {
                drag = None;
                if let Some((from, _, to)) = held
                    && to != from
                {
                    out.push(TabAction::Reorder { from, to });
                }
            }
        }
    }
    if drag.is_some_and(|d| d.from >= n) || !ui.input(|i| i.pointer.any_down()) && drag.is_some() {
        drag = None;
    }
    ui.data_mut(|d| match drag {
        Some(v) => {
            d.insert_temp(id.with("drag"), v);
        }
        None => {
            d.remove::<Drag>(id.with("drag"));
        }
    });

    // `+`, then » when tabs are left out.
    let mut x = rect.left() + count as f32 * width;
    let button = |x: f32| egui::Rect::from_min_size(egui::pos2(x, rect.center().y - BUTTON / 2.0), egui::vec2(BUTTON, BUTTON));
    if count < n {
        let r = button(x);
        let mut chosen = None;
        let more = small_button(ui, r, id.with("more"), Glyph::Text("»"), 16.0).on_hover_text(t!("tabs.more_tip"));
        egui::Popup::menu(&more).show(|ui| {
            for (i, tab) in tabs.iter().enumerate() {
                let label = format!("{}{}", tab.name, if tab.modified { " •" } else { "" });
                if ui.selectable_label(shown == Some(i), label).clicked() {
                    chosen = Some(i);
                }
            }
        });
        if let Some(i) = chosen {
            out.push(TabAction::Select(i));
        }
        x += BUTTON;
    }
    if small_button(ui, button(x).shrink(1.0), id.with("new"), Glyph::Icon(icons::ADD), 16.0).on_hover_text(t!("tabs.new_tip")).clicked() {
        out.push(TabAction::New);
    }
    out
}

/// What a [`small_button`] shows.
enum Glyph {
    Icon(egui::ImageSource<'static>),
    Text(&'static str),
}

/// A frameless button's corner radius.
pub const VEIL_RADIUS: f32 = 5.0;

/// A frameless button's background: a veil of the text's colour when hovered, darker when
/// pressed — seen on any ground, in either theme. None at rest.
pub fn veil(ui: &egui::Ui, resp: &egui::Response) -> Option<egui::Color32> {
    let alpha = if resp.is_pointer_button_down_on() {
        0.22
    } else if resp.hovered() {
        0.12
    } else {
        return None;
    };
    Some(ui.visuals().text_color().gamma_multiply(alpha))
}

/// A frameless button of the strip (×, +, »): its [`veil`]. `size`: its glyph's.
fn small_button(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id, glyph: Glyph, size: f32) -> egui::Response {
    let resp = ui.interact(rect, id, egui::Sense::click());
    let ink = ui.visuals().text_color();
    if let Some(fill) = veil(ui, &resp) {
        ui.painter().rect_filled(rect, VEIL_RADIUS, fill);
    }
    match glyph {
        Glyph::Icon(icon) => {
            egui::Image::new(icon).tint(ink).paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::vec2(size, size)))
        }
        Glyph::Text(text) => {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(size), ink);
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_shrink_then_a_run_around_the_shown_one_is_drawn() {
        // Room for all at their widest.
        assert_eq!(layout(2, 0, 1000.0), (0, 2, MAX_TAB));
        // Shrunk.
        let (first, count, w) = layout(5, 0, 5.0 * 100.0 + BUTTON);
        assert_eq!((first, count, w), (0, 5, 100.0));
        // Too many: as many as fit at their narrowest, the shown one among them.
        let room = 4.0 * MIN_TAB + 2.0 * BUTTON;
        assert_eq!(layout(10, 0, room), (0, 4, MIN_TAB));
        assert_eq!(layout(10, 6, room), (3, 4, MIN_TAB));
        assert_eq!(layout(10, 9, room), (6, 4, MIN_TAB));
        // Never none, however narrow.
        assert_eq!(layout(3, 2, 10.0), (2, 1, MIN_TAB));
        assert_eq!(layout(0, 0, 500.0).1, 0);
    }

    #[test]
    fn a_tab_dragged_along_the_strip_moves_the_tab_not_the_window() {
        let along = egui::vec2(6.0, 1.0);
        let up = egui::vec2(1.0, -6.0);
        assert!(!drag_moves_window(3, true, along));
        assert!(drag_moves_window(3, true, up));
        assert!(drag_moves_window(1, true, along), "the only tab: the window");
        // Not in the window's title bar (web, Windows, Linux): never the window.
        assert!(!drag_moves_window(1, false, up));
    }

    #[test]
    fn the_other_tabs_make_room_for_the_one_dragged() {
        // Tab 1 of 0..4 dragged over place 3: 0 stays, 2 and 3 close up, 1 lands last.
        assert_eq!(places(0, 4, 1, 3), [0, 3, 1, 2]);
        // Back to its own place: nothing moves.
        assert_eq!(places(0, 4, 1, 1), [0, 1, 2, 3]);
        // Dragged to the front, with the strip scrolled (tabs 5..8 drawn).
        assert_eq!(places(5, 3, 7, 5), [1, 2, 0]);
    }

    #[test]
    fn a_tab_drops_on_the_place_under_the_pointer() {
        assert_eq!(drop_place(5.0, 0.0, 100.0, 0, 3), 0);
        assert_eq!(drop_place(150.0, 0.0, 100.0, 0, 3), 1);
        assert_eq!(drop_place(950.0, 0.0, 100.0, 0, 3), 2, "past the last: the last place");
        assert_eq!(drop_place(-20.0, 0.0, 100.0, 4, 3), 4);
    }
}

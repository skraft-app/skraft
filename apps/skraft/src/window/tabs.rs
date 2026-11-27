//! The file tabs in the title bar (docs/ui/file-tabs.md): one per open document,
//! the shown one lit, a bullet after a modified one's name. Click selects, × or a middle click
//! closes, a drag reorders, right click opens the tab's menu; `+` opens a new document and »
//! lists every tab when they do not all fit ([`layout`], a pure function of the width).
//!
//! On the macOS desktop the strip sits in the window's own title bar, so a drag can mean two
//! things: sideways it reorders, while a lone tab or a mainly vertical movement carries the
//! window instead. [`drag_moves_window`] settles this once, when the drag begins.

use skraft_i18n::t;

use crate::kit::icons;
use crate::kit::style::Tip;
use crate::kit::widgets::IconButton;

/// What the app tells the strip about one open document, anew each frame.
pub struct Tab {
    /// Its document's id: what follows the tab while tabs move.
    pub id: u64,
    pub name: String,
    /// Its file (none: never saved).
    pub path: Option<String>,
    pub modified: bool,
}

impl Tab {
    /// Its name, with a bullet when it is modified.
    fn title(&self) -> String {
        format!("{}{}", self.name, if self.modified { " •" } else { "" })
    }

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

/// An entry of the context menu opened on a tab.
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

/// Which tabs fit in `room`: `(first drawn, how many, their width)`. All of them share the room
/// while they can stay at [`MIN_TAB`]; past that, » joins `+` and the widest run holding tab
/// `shown` is drawn, as far left as it can start.
pub fn layout(n: usize, shown: usize, room: f32) -> (usize, usize, f32) {
    if n == 0 {
        return (0, 0, MIN_TAB);
    }
    let strip = room - BUTTON;
    if n as f32 * MIN_TAB <= strip {
        return (0, n, (strip / n as f32).min(MAX_TAB));
    }
    let count = (((strip - BUTTON) / MIN_TAB).floor() as usize).clamp(1, n);
    let end = shown.min(n - 1) + 1;
    (end.saturating_sub(count), count, MIN_TAB)
}

/// The tab index of the drawn slot under `x`. Slots are `width` wide from `x0` and numbered from
/// `first`; a position outside the run counts as its nearest end.
pub fn drop_place(x: f32, x0: f32, width: f32, first: usize, count: usize) -> usize {
    let slot = ((x - x0) / width).floor().max(0.0) as usize;
    first + slot.min(count.saturating_sub(1))
}

/// Whether a drag beginning with the movement `moved` carries the window rather than a tab: only
/// from a title bar (`window_moves`), for a lone tab or a movement more vertical than horizontal.
pub fn drag_moves_window(n: usize, window_moves: bool, moved: egui::Vec2) -> bool {
    window_moves && (n <= 1 || moved.y.abs() > moved.x.abs())
}

/// The slot (0-based along the strip) of each drawn tab `first..first + count`, in tab order,
/// while tab `from` is carried to the slot of tab `to`. The others fill the remaining slots in
/// their own order.
pub fn places(first: usize, count: usize, from: usize, to: usize) -> Vec<usize> {
    if !(first..first + count).contains(&from) {
        return (0..count).collect();
    }
    let taken = to.saturating_sub(first);
    let mut free = (0..count).filter(|&slot| slot != taken);
    (first..first + count).map(|i| if i == from { taken } else { free.next().unwrap_or(taken) }).collect()
}

/// The tab following the pointer, kept from one frame to the next.
#[derive(Clone, Copy)]
struct CarriedTab {
    index: usize,
    /// The pointer's distance from the tab's left edge.
    grip: f32,
}

/// What the shown tab opens onto: it runs down to the bar's `bottom` edge in the 3D view's
/// background (`fill`), its text in `ink`, so that it reads as the view's tab.
#[derive(Clone, Copy)]
pub struct Ground {
    pub bottom: f32,
    pub fill: egui::Color32,
    pub ink: egui::Color32,
}

/// The shown tab's top corners, and the radius of the curves joining it to the bar's edge.
pub const TAB_RADIUS: f32 = 6.0;

/// Draws `tabs` in `rect` (`shown`: the lit one, none on the start page; `ground`: what it
/// opens onto, else it is only lit). `window_moves`: the tabs are in the window's title bar
/// (macOS desktop).
pub fn show(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    tabs: &[Tab],
    shown: Option<usize>,
    ground: Option<Ground>,
    window_moves: bool,
    reveal: bool,
) -> Vec<TabAction> {
    let mut out = Vec::new();
    let id = egui::Id::unique("file_tabs");
    let n = tabs.len();
    let (first, count, width) = layout(n, shown.unwrap_or(0), rect.width());
    let y = rect.center().y - HEIGHT / 2.0;
    let carried_key = id.with("carried");
    // A carried tab whose document has gone is dropped here.
    let mut carried = ui.data(|d| d.get_temp::<CarriedTab>(carried_key)).filter(|c| c.index < n);
    let pointer = ui.ctx().pointer_latest_pos();
    let visuals = ui.visuals().clone();
    let font = egui::TextStyle::Body.resolve(ui.style());

    // This frame's picture of the drag; a drag starting below shows from the next frame.
    let carrying = carried.is_some();
    let drawn = first..first + count;
    let lifted = carried.map(|c| c.index).filter(|i| drawn.contains(i));
    // The lifted tab's left edge and the index it would land on.
    let flight = carried.filter(|c| drawn.contains(&c.index)).zip(pointer).map(|(c, p)| {
        let last = (rect.left() + (count - 1) as f32 * width).max(rect.left());
        let left = (p.x - c.grip).clamp(rect.left(), last);
        (left, drop_place(left + width / 2.0, rect.left(), width, first, count))
    });
    let slots = lifted.zip(flight).map(|(i, (_, to))| places(first, count, i, to));
    // Painted last, so that it stays above its neighbours.
    let on_top = lifted.or(shown.filter(|i| drawn.contains(i)));
    let order: Vec<usize> = drawn.clone().filter(|&i| Some(i) != on_top).chain(on_top).collect();
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let strip = Strip { id, tabs, shown, ground, reveal, visuals: &visuals, font, carrying };

    for i in order {
        let tab = &tabs[i];
        let dragged = lifted == Some(i);
        let slot = slots.as_ref().map_or(i - first, |s| s[i - first]);
        let target = match flight {
            Some((left, _)) if dragged => left,
            _ => rect.left() + slot as f32 * width,
        };
        let x = slide(ui, id.with(("x", tab.id)), target, dragged, dt);
        let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width - 2.0, HEIGHT));
        let resp = draw_tab(ui, &strip, i, r, dragged, &mut out);
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|input| input.pointer.press_origin());
            let moved = origin.zip(pointer).map_or(egui::Vec2::ZERO, |(o, p)| p - o);
            if drag_moves_window(n, window_moves, moved) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            } else if n >= 2 {
                let grip = origin.map_or(width / 2.0, |o| o.x - r.left());
                carried = Some(CarriedTab { index: i, grip });
            }
        }
        if resp.drag_stopped() && carried.is_some_and(|c| c.index == i) {
            carried = None;
            if let Some((_, to)) = flight.filter(|&(_, to)| dragged && to != i) {
                out.push(TabAction::Reorder { from: i, to });
            }
        }
    }
    // Every button up with the drag still open: its tab was not there to see the release.
    if !ui.input(|input| input.pointer.any_down()) {
        carried = None;
    }
    match carried {
        Some(c) => {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            ui.data_mut(|d| d.insert_temp(carried_key, c));
        }
        None => ui.data_mut(|d| d.remove::<CarriedTab>(carried_key)),
    }
    end_buttons(ui, &strip, rect.left() + count as f32 * width, rect.center().y, count < n, &mut out);
    out
}

/// After the tabs, from `x` on the line `y`: » listing them all when some are left out
/// (`more`), then `+`.
fn end_buttons(ui: &mut egui::Ui, s: &Strip<'_>, mut x: f32, y: f32, more: bool, out: &mut Vec<TabAction>) {
    let Strip { id, tabs, shown, visuals, .. } = *s;
    let button = |x: f32| egui::Rect::from_min_size(egui::pos2(x, y - BUTTON / 2.0), egui::vec2(BUTTON, BUTTON));
    if more {
        let r = button(x);
        let mut chosen = None;
        let more = IconButton::new(icons::MORE, r.width(), 14.0)
            .ink(visuals.text_color())
            .show_at(ui, r, id.with("more"))
            .tip(t!("tabs.more_tip"));
        egui::Popup::menu(&more).style(crate::kit::style::menu).show(|ui| {
            for (i, tab) in tabs.iter().enumerate() {
                if crate::kit::widgets::choice_item(ui, shown == Some(i), tab.title()).clicked() {
                    chosen = Some(i);
                }
            }
        });
        if let Some(i) = chosen {
            out.push(TabAction::Select(i));
        }
        x += BUTTON;
    }
    crate::kit::handles::publish(|| "tab_new".into(), button(x).shrink(1.0));
    let new = button(x).shrink(1.0);
    if IconButton::new(icons::ADD, new.width(), 16.0)
        .ink(visuals.text_color())
        .show_at(ui, new, id.with("new"))
        .tip(t!("tabs.new_tip"))
        .clicked()
    {
        out.push(TabAction::New);
    }
}

/// What the tabs drawn in one frame share.
struct Strip<'a> {
    id: egui::Id,
    tabs: &'a [Tab],
    shown: Option<usize>,
    ground: Option<Ground>,
    reveal: bool,
    visuals: &'a egui::Visuals,
    font: egui::FontId,
    /// A tab is carried: the others neither light up nor show their × or tip.
    carrying: bool,
}

/// A tab's left edge on its way to `target`, kept under `key` from one frame to the next;
/// the dragged one follows the pointer at once.
fn slide(ui: &egui::Ui, key: egui::Id, target: f32, dragged: bool, dt: f32) -> f32 {
    let x = match ui.data(|d| d.get_temp::<f32>(key)) {
        Some(prev) if !dragged && (prev - target).abs() > 0.5 => {
            ui.ctx().request_repaint();
            prev + (target - prev) * (1.0 - (-dt * 18.0).exp())
        }
        _ => target,
    };
    ui.data_mut(|d| d.insert_temp(key, x));
    x
}

/// Draws the tab `i` in `r`: its ground, its name, its ×, its tip and menu; what is clicked
/// goes to `out`.
fn draw_tab(ui: &mut egui::Ui, s: &Strip<'_>, i: usize, r: egui::Rect, dragged: bool, out: &mut Vec<TabAction>) -> egui::Response {
    let Strip { id, tabs, shown, ground, carrying, visuals, .. } = *s;
    let tab = &tabs[i];
    let resp = ui.interact(r, id.with(("tab", i)), egui::Sense::click_and_drag());
    crate::kit::handles::publish(|| format!("tab:{i}"), r);
    let active = shown == Some(i);
    let hovered = resp.hovered();
    let ink = match ground {
        Some(g) if active && !dragged => g.ink,
        _ => visuals.text_color(),
    };
    tab_ground(ui, s, r, active, hovered, dragged);

    // The × on the lit tab and the hovered one.
    let close = egui::Rect::from_center_size(egui::pos2(r.right() - 12.0, r.center().y), egui::vec2(16.0, 16.0));
    let show_close = active || hovered || ui.rect_contains_pointer(close);
    let text_right = if show_close { close.left() - 4.0 } else { r.right() - 8.0 };
    let color = if active && ground.is_some() && !dragged {
        ink
    } else if active || dragged {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    // Its colour given to the text: the bar's style overrides the text's colour.
    let galley = egui::WidgetText::from(egui::RichText::new(tab.title()).color(color)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (text_right - r.left() - 10.0).max(0.0),
        s.font.clone(),
    );
    let cut = galley.elided;
    ui.painter().galley(egui::pos2(r.left() + 10.0, r.center().y - galley.size().y / 2.0), galley, color);
    if show_close && !carrying {
        let x = IconButton::new(icons::CLOSE, close.width(), 12.0).ink(ink).show_at(ui, close, id.with(("close", i)));
        crate::kit::handles::publish(|| format!("tab_close:{i}"), close);
        if x.tip(t!("tabs.close_tip")).clicked() {
            out.push(TabAction::Close(i));
        }
    }

    let resp = if carrying { resp } else { resp.tip(tab.tip(cut)) };
    if resp.clicked() {
        out.push(TabAction::Select(i));
    }
    if resp.middle_clicked() {
        out.push(TabAction::Close(i));
    }
    tab_menu(&resp, s, i, out);
    resp
}

/// A tab's ground: lifted when dragged, opening onto the view when shown, else lit while
/// hovered.
fn tab_ground(ui: &egui::Ui, s: &Strip<'_>, r: egui::Rect, active: bool, hovered: bool, dragged: bool) {
    let visuals = s.visuals;
    if dragged {
        // Lifted: a shadow, the panels' fill and the accent's outline, unlike any tab at rest
        // (the shown one opens onto the view).
        let shadow = egui::epaint::Shadow { offset: [0, 3], blur: 12, spread: 0, color: egui::Color32::from_black_alpha(90) };
        ui.painter().add(shadow.as_shape(r, 6.0));
        ui.painter().rect(r, 6.0, visuals.window_fill, egui::Stroke::new(1.5, visuals.selection.stroke.color), egui::StrokeKind::Inside);
    } else if let Some(g) = s.ground.filter(|_| active) {
        open_tab(ui.painter(), egui::Rect::from_x_y_ranges(r.x_range(), r.top()..=g.bottom), g.fill);
    } else {
        let fill = if active {
            visuals.widgets.active.weak_bg_fill
        } else if hovered && !s.carrying {
            visuals.widgets.hovered.weak_bg_fill
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(r, 6.0, fill);
    }
}

/// The tab `i`'s right-click menu.
fn tab_menu(resp: &egui::Response, s: &Strip<'_>, i: usize, out: &mut Vec<TabAction>) {
    let tab = &s.tabs[i];
    egui::Popup::context_menu(resp).style(crate::kit::style::menu).show(|ui| {
        let mut row = |ui: &mut egui::Ui, row: MenuRow, label: String, enabled: bool| {
            if crate::kit::widgets::MenuEntry::new(label).enabled(enabled).show(ui) {
                out.push(TabAction::Menu(i, row));
            }
        };
        row(ui, MenuRow::Close, t!("tabs.close"), true);
        row(ui, MenuRow::CloseOthers, t!("tabs.close_others"), s.tabs.len() > 1);
        row(ui, MenuRow::CloseAll, t!("tabs.close_all"), true);
        if s.reveal {
            ui.separator();
            let label = if ui.ctx().os() == egui::os::OperatingSystem::Mac { t!("tabs.reveal_finder") } else { t!("tabs.reveal") };
            row(ui, MenuRow::Reveal, label, tab.path.is_some());
        }
    });
}

/// The shown tab, `rect` down to the bar's edge, filled with `fill`: rounded at the top, and
/// at the bottom curving out into the edge, as a tab of the view under it.
pub fn open_tab(p: &egui::Painter, rect: egui::Rect, fill: egui::Color32) {
    let r = TAB_RADIUS;
    let round = r as u8;
    p.rect_filled(rect, egui::CornerRadius { nw: round, ne: round, sw: 0, se: 0 }, fill);
    for (corner, out) in [(rect.left_bottom(), -1.0), (rect.right_bottom(), 1.0)] {
        // The room between the tab's side, the bar's edge and a quarter circle centred `r`
        // out and `r` up: a fan from the corner, its arc stroked for a smooth edge.
        let centre = corner + egui::vec2(out * r, -r);
        let arc: Vec<egui::Pos2> = (0..=8)
            .map(|k| {
                let a = k as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
                centre + egui::vec2(-out * r * a.cos(), r * a.sin())
            })
            .collect();
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(corner, fill);
        for &q in &arc {
            mesh.colored_vertex(q, fill);
        }
        for k in 1..arc.len() as u32 {
            mesh.add_triangle(0, k, k + 1);
        }
        p.add(mesh);
        p.add(egui::Shape::line(arc, egui::Stroke::new(1.0, fill)));
    }
}

/// The edge under a band from `left` to `right`, going round the shown tab `rect` (as
/// [`open_tab`] draws it, down to the edge at its bottom): along the edge, up the curve, over
/// the tab's rounded top and back down. Stroked, it outlines the tab as part of the ground under
/// the band. `inset`: the stroke's half width, which keeps it inside the tab.
pub fn open_tab_outline(rect: egui::Rect, left: f32, right: f32, inset: f32) -> Vec<egui::Pos2> {
    let r = TAB_RADIUS;
    let y = rect.bottom() - inset;
    let quarter = |centre: egui::Pos2, from: f32, radius: f32| -> Vec<egui::Pos2> {
        (0..=8)
            .map(|k| {
                let a = from + k as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
                centre + radius * egui::vec2(a.cos(), a.sin())
            })
            .collect()
    };
    use std::f32::consts::{FRAC_PI_2, PI};
    let (l, t, rr) = (rect.left() + inset, rect.top() + inset, rect.right() - inset);
    // Up the left curve (centred out and up of the tab's corner), the top's corners, down the
    // right curve.
    let mut out = vec![egui::pos2(left, y)];
    out.extend(quarter(egui::pos2(l - r, y - r), 0.0, r).into_iter().rev());
    out.extend(quarter(egui::pos2(l + r, t + r), PI, r));
    out.extend(quarter(egui::pos2(rr - r, t + r), PI + FRAC_PI_2, r));
    out.extend(quarter(egui::pos2(rr + r, y - r), FRAC_PI_2, r).into_iter().rev());
    out.push(egui::pos2(right, y));
    out
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

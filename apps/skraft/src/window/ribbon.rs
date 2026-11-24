//! The ribbon, whose first cell switches the workspace (docs/ui/ribbon.md).
//!
//! The ribbon is data: [`crate::workspace::WORKSPACES`] → tabs → groups → command ids of the registry
//! (docs/ui/commands.md), each group with its default pins. A group shows its pinned commands as buttons;
//! its label (▾) opens its menu, which lists every command of the group with a pin. The user's
//! pins, the tab shown per workspace and the labels' switch are a preference ([`Prefs`]); the
//! workspace shown is the document's ([`crate::workspace`]).
//! When the ribbon is too narrow, [`arrange`] decides what gives way. Drawing only reads: what the
//! user chose comes back as actions ([`Change`] for the ribbon's own state).

use std::collections::BTreeMap;

use egui::{Align2, Color32, FontId, Popup, PopupCloseBehavior, Rect, Sense, StrokeKind, pos2, vec2};
use serde::{Deserialize, Serialize};
use skraft_i18n::t;

use crate::kit::widgets::{PinRow, pin_row};

use crate::commands::{Action, CommandView};
use crate::kit::icons;
use crate::kit::style::Tip;
use crate::kit::style::menu_item;
use crate::workspace::{self, Workspace};

// ---- the description -------------------------------------------------------------------------

pub struct Tab {
    pub id: &'static str,
    pub label: fn() -> String,
    pub groups: &'static [Group],
}

/// Commands of the registry under one label. A command may be in several groups.
pub struct Group {
    pub id: &'static str,
    pub label: fn() -> String,
    /// In the menu's order, which is also the buttons' order.
    pub commands: &'static [&'static str],
    /// The buttons shown until the user changes them.
    pub pinned: &'static [&'static str],
}

pub(crate) const fn group(
    id: &'static str,
    label: fn() -> String,
    commands: &'static [&'static str],
    pinned: &'static [&'static str],
) -> Group {
    Group { id, label, commands, pinned }
}

/// A group's key in the pins: `"<workspace>/<tab>/<group>"`, `"<workspace>/<group>"` for the
/// fixed group.
pub fn key(workspace: &Workspace, tab: Option<&Tab>, group: &Group) -> String {
    match tab {
        Some(t) => format!("{}/{}/{}", workspace.id, t.id, group.id),
        None => format!("{}/{}", workspace.id, group.id),
    }
}

// ---- the user's state ------------------------------------------------------------------------

/// The ribbon's preferences.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// The tab shown, by workspace.
    pub tabs: BTreeMap<String, String>,
    /// The pinned commands of the groups the user changed, by [`key`] (absent: the defaults).
    pub pins: BTreeMap<String, Vec<String>>,
    /// Buttons show their label under their icon (off by default: compact, the names in the
    /// tips).
    pub labels: bool,
}

impl Prefs {
    /// The tab shown in workspace `w`.
    pub fn tab(&self, w: &Workspace) -> &'static Tab {
        w.tab(self.tabs.get(w.id).map(String::as_str))
    }

    /// The group's pinned commands, in the group's order (unknown ids are ignored).
    pub fn pinned(&self, key: &str, group: &Group) -> Vec<&'static str> {
        match self.pins.get(key) {
            Some(ids) => group.commands.iter().copied().filter(|c| ids.iter().any(|i| i == c)).collect(),
            None => group.pinned.to_vec(),
        }
    }

    /// Applies `change`, made on the ribbon of workspace `w`.
    pub fn apply(&mut self, change: Change, w: &Workspace) {
        match change {
            Change::Tab(id) => {
                self.tabs.insert(w.id.to_owned(), id.to_owned());
            }
            Change::TogglePin { key, group, id } => {
                let mut pinned = self.pinned(&key, group);
                if pinned.contains(&id) {
                    pinned.retain(|c| *c != id);
                } else {
                    pinned = group.commands.iter().copied().filter(|c| *c == id || pinned.contains(c)).collect();
                }
                // Back to the defaults: nothing to keep.
                if pinned == group.pinned {
                    self.pins.remove(&key);
                } else {
                    self.pins.insert(key, pinned.into_iter().map(str::to_owned).collect());
                }
            }
            Change::ResetTab => {
                let tab = self.tab(w);
                self.pins.remove(&key(w, None, &w.fixed));
                for g in tab.groups {
                    self.pins.remove(&key(w, Some(tab), g));
                }
            }
            Change::Labels(on) => self.labels = on,
        }
    }
}

/// What the user changed on the ribbon (applied by the app after drawing, [`Prefs::apply`]).
#[derive(Clone)]
pub enum Change {
    Tab(&'static str),
    /// Pin command `id` of the group under `key`, or unpin it.
    TogglePin {
        key: String,
        group: &'static Group,
        id: &'static str,
    },
    /// The tab's groups (and the fixed group) back to their default pins.
    ResetTab,
    Labels(bool),
}

// ---- fitting ---------------------------------------------------------------------------------

/// A group's sizes, the input of the ribbon's fitting.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupSize {
    /// Each pinned button's preferred and least width.
    pub buttons: Vec<(f32, f32)>,
    /// Its label's width: the group is never narrower.
    pub label: f32,
}

/// Room between two buttons, and between two groups (their separator).
pub const BUTTON_GAP: f32 = 2.0;
pub const GROUP_GAP: f32 = 13.0;
/// Width set aside at the right edge for the » button, when it has groups to list.
pub const MORE_ROOM: f32 = 30.0;

/// What became of a group once the ribbon is arranged in its room.
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    /// On the ribbon: its first `widths.len()` buttons, at these widths.
    Row(Vec<f32>),
    /// Folded into one button that opens its menu.
    Folded,
    /// Listed in the » menu: it takes no room at all.
    Overflow,
}

impl Placement {
    /// `group` with nothing given up.
    fn unreduced(group: &GroupSize) -> Self {
        if group.buttons.is_empty() { Placement::Folded } else { Placement::Row(group.buttons.iter().map(|b| b.0).collect()) }
    }

    /// The width it takes on the ribbon. `folded`: the width of a folded group's button.
    fn width(&self, group: &GroupSize, folded: f32) -> f32 {
        match self {
            Placement::Row(widths) => row_width(group.label, widths.iter().copied()),
            Placement::Folded => folded.max(group.label),
            Placement::Overflow => 0.0,
        }
    }
}

/// Buttons side by side, the group's label as a lower bound.
fn row_width(label: f32, buttons: impl ExactSizeIterator<Item = f32>) -> f32 {
    let gaps = buttons.len().saturating_sub(1) as f32 * BUTTON_GAP;
    (buttons.sum::<f32>() + gaps).max(label)
}

/// Group widths laid end to end with their separators.
fn joined(widths: impl Iterator<Item = f32>) -> f32 {
    let (sum, count) = widths.fold((0.0, 0usize), |(sum, count), w| (sum + w, count + 1));
    sum + count.saturating_sub(1) as f32 * GROUP_GAP
}

/// The width the placed groups take together, the » button aside.
fn span(placements: &[Placement], groups: &[GroupSize], folded: f32) -> f32 {
    joined(placements.iter().zip(groups).filter(|(p, _)| **p != Placement::Overflow).map(|(p, g)| p.width(g, folded)))
}

/// A button's width once every button gave up `drop` points: never under its least width.
fn narrowed((preferred, least): (f32, f32), drop: f32) -> f32 {
    (preferred - drop).max(least.min(preferred))
}

/// The smallest whole drop for which `enough` holds, known to hold once all buttons are at their
/// least. Bisected: the total width never grows with the drop.
fn smallest_drop(groups: &[GroupSize], enough: impl Fn(f32) -> bool) -> u32 {
    if enough(0.0) {
        return 0;
    }
    let slack = groups.iter().flat_map(|g| &g.buttons).map(|&(p, l)| p - l.min(p)).fold(0.0, f32::max);
    // One past the largest slack, so that rounding cannot leave a button above its least.
    let (mut short, mut sufficient) = (0u32, (slack.ceil() as u32).saturating_add(1));
    while sufficient - short > 1 {
        let middle = short + (sufficient - short) / 2;
        if enough(middle as f32) {
            sufficient = middle;
        } else {
            short = middle;
        }
    }
    sufficient
}

/// With every button at its least width, takes buttons away until the groups fit in `room`: the
/// group showing the most goes first, the rightmost among equals. None if all folded is too wide.
fn drop_buttons(groups: &[GroupSize], room: f32, folded: f32) -> Option<Vec<Placement>> {
    // That order is known ahead: each button that can go, by how many its group shows just
    // before, then by group. A group never rests on one button unless it only has one.
    let mut turns: Vec<(usize, usize)> = groups
        .iter()
        .enumerate()
        .flat_map(|(i, g)| {
            let n = g.buttons.len();
            (if n == 1 { 1 } else { 2 }..=n).map(move |showing| (showing, i))
        })
        .collect();
    turns.sort_unstable_by(|a, b| b.cmp(a));

    let least = |g: &'_ GroupSize, k: usize| g.buttons[..k].iter().map(|&b| narrowed(b, f32::INFINITY)).collect::<Vec<f32>>();
    let width = |g: &GroupSize, k: usize| match k {
        0 => folded.max(g.label),
        k => row_width(g.label, g.buttons[..k].iter().map(|&b| narrowed(b, f32::INFINITY))),
    };
    let mut showing: Vec<usize> = groups.iter().map(|g| g.buttons.len()).collect();
    let mut widths: Vec<f32> = groups.iter().zip(&showing).map(|(g, &k)| width(g, k)).collect();
    for (before, i) in turns {
        showing[i] = if before > 2 { before - 1 } else { 0 };
        widths[i] = width(&groups[i], showing[i]);
        if joined(widths.iter().copied()) <= room {
            let place = |(g, &k): (&GroupSize, &usize)| if k == 0 { Placement::Folded } else { Placement::Row(least(g, k)) };
            return Some(groups.iter().zip(&showing).map(place).collect());
        }
    }
    None
}

/// `groups`, all of them on the ribbon or folded, in `room`; None if they cannot be.
fn settle(groups: &[GroupSize], room: f32, folded: f32) -> Option<Vec<Placement>> {
    let width_at = |g: &GroupSize, drop: f32| {
        if g.buttons.is_empty() { folded.max(g.label) } else { row_width(g.label, g.buttons.iter().map(|&b| narrowed(b, drop))) }
    };
    let fits_at = |drop: f32| joined(groups.iter().map(|g| width_at(g, drop))) <= room;
    if !fits_at(f32::INFINITY) {
        return drop_buttons(groups, room, folded);
    }
    let drop = smallest_drop(groups, fits_at) as f32;
    let place = |g: &GroupSize| {
        if g.buttons.is_empty() { Placement::Folded } else { Placement::Row(g.buttons.iter().map(|&b| narrowed(b, drop)).collect()) }
    };
    Some(groups.iter().map(place).collect())
}

/// What each group becomes in `room` (a non-finite one counts as 0). `folded`: the width of a
/// folded group's button. Depends on its arguments only: buttons narrow together, then go one by
/// one, then the last groups move to the » menu (docs/ui/ribbon.md).
pub fn arrange(groups: &[GroupSize], room: f32, folded: f32) -> Vec<Placement> {
    let room = if room.is_finite() { room } else { 0.0 };
    if let Some(all) = settle(groups, room, folded) {
        return all;
    }
    let beside_more = room - MORE_ROOM;
    (1..groups.len())
        .rev()
        .find_map(|kept| {
            let mut head = settle(&groups[..kept], beside_more, folded)?;
            head.resize(groups.len(), Placement::Overflow);
            Some(head)
        })
        .unwrap_or_else(|| vec![Placement::Overflow; groups.len()])
}

// ---- drawing ---------------------------------------------------------------------------------

/// A button's height with and without its label, and the group label's row under the buttons.
const BUTTON_HEIGHT: f32 = 62.0;
const COMPACT_HEIGHT: f32 = 34.0;
const GROUP_LABEL_HEIGHT: f32 = 16.0;
/// Between the buttons and their group's label: a button's highlight never touches it.
const LABEL_GAP: f32 = 4.0;
/// A labelled button's widths: from its label's, within these.
const BUTTON_MIN_WIDTH: f32 = 50.0;
const BUTTON_MAX_WIDTH: f32 = 74.0;
const COMPACT_WIDTH: f32 = 34.0;
const ICON: f32 = 24.0;
/// How wide the cell that picks the workspace is; the ribbon starts with it.
const WORKSPACE_WIDTH: f32 = 92.0;
/// The workspace's icon in its cell, and the gap between it and its name.
const WORKSPACE_ICON: f32 = 28.0;
const WORKSPACE_ICON_GAP: f32 = 4.0;

fn label_font() -> FontId {
    FontId::proportional(11.5)
}

/// What the ribbon shows this frame.
pub struct View<'a> {
    /// The shown document's workspace.
    pub workspace: &'static Workspace,
    pub prefs: &'a Prefs,
    pub views: &'a [CommandView],
}

impl View<'_> {
    fn command(&self, id: &str) -> Option<&CommandView> {
        self.views.iter().find(|v| v.id == id)
    }
}

/// The cell that opens the ribbon and picks the workspace. Its icon sits above its name, the
/// pair centred over the line of group labels; its chevron lies on that line, level with the
/// groups' chevrons (docs/ui/ribbon.md#layout). All of it is clickable, margins included: `inset`
/// extends it at the top, left and bottom, `sep` bounds it on the right. Clicking lists the
/// workspaces.
fn workspace_cell(ui: &mut egui::Ui, v: &View, rect: Rect, inset: egui::Margin, sep: f32, actions: &mut Vec<Action>) {
    let current = v.workspace;
    let full = Rect::from_min_max(pos2(rect.left() - inset.leftf(), rect.top() - inset.topf()), pos2(sep, rect.bottom() + inset.bottomf()));
    let response = ui.interact(full, egui::Id::unique("ribbon_workspace"), Sense::click());
    crate::kit::handles::publish(|| "ribbon_workspace".into(), full);
    let hovered = response.hovered() || response.is_pointer_button_down_on();
    if hovered {
        // The card's corners on its left, square along the separator.
        let r = ui.visuals().window_corner_radius;
        let corners = egui::CornerRadius { nw: r.nw, sw: r.sw, ne: 0, se: 0 };
        ui.painter().rect_filled(full, corners, ui.style().interact(&response).weak_bg_fill);
    }
    let ink = ui.visuals().text_color();
    let label = ui.painter().layout_no_wrap((current.label)(), FontId::proportional(12.5), ink);
    let line = rect.bottom() - GROUP_LABEL_HEIGHT;
    let block = WORKSPACE_ICON + WORKSPACE_ICON_GAP + label.size().y;
    let top = (full.top() + line - block) / 2.0;
    let icon = Rect::from_min_size(pos2(full.center().x - WORKSPACE_ICON / 2.0, top), vec2(WORKSPACE_ICON, WORKSPACE_ICON));
    egui::Image::new(current.icon.clone()).tint(ink).paint_at(ui, icon);
    let at = pos2(full.center().x - label.size().x / 2.0, icon.bottom() + WORKSPACE_ICON_GAP);
    ui.painter().galley(at, label, ink);
    // As a group label's chevron: weak, the text's colour when hovered.
    let chevron_ink = if hovered { ink } else { ui.visuals().weak_text_color() };
    chevron(ui, pos2(full.center().x, line + GROUP_LABEL_HEIGHT / 2.0), chevron_ink);
    let response = response.tip(t!("ribbon.workspace.tip"));
    Popup::menu(&response).style(crate::kit::style::menu).show(|ui| {
        let rows: Vec<_> = workspace::listed().map(|w| WorkspaceRow::new(ui, w)).collect();
        let width = rows.iter().map(WorkspaceRow::width).fold(WORKSPACE_ROW_MIN_WIDTH, f32::max);
        for row in rows {
            let w = row.workspace;
            if row.show(ui, width, w.id == current.id).clicked() {
                if w.id != current.id {
                    actions.push(Action::Workspace(w.id));
                }
                ui.close();
            }
        }
    });
}

/// A row of the workspaces' menu: the icon, then the name over what it is for, smaller and weak.
const WORKSPACE_ROW_ICON: f32 = 22.0;
const WORKSPACE_ROW_HEIGHT: f32 = 40.0;
const WORKSPACE_ROW_PAD: f32 = 8.0;
const WORKSPACE_ROW_MIN_WIDTH: f32 = 200.0;

struct WorkspaceRow {
    workspace: &'static Workspace,
    name: std::sync::Arc<egui::Galley>,
    about: std::sync::Arc<egui::Galley>,
}

impl WorkspaceRow {
    fn new(ui: &egui::Ui, w: &'static Workspace) -> Self {
        let name = if w.under_way { t!("ribbon.workspace.under_way", name = (w.label)()) } else { (w.label)() };
        let name = ui.painter().layout_no_wrap(name, FontId::proportional(13.0), ui.visuals().text_color());
        let about = ui.painter().layout_no_wrap((w.about)(), FontId::proportional(11.0), ui.visuals().weak_text_color());
        WorkspaceRow { workspace: w, name, about }
    }

    /// The row's width for its texts.
    fn width(&self) -> f32 {
        3.0 * WORKSPACE_ROW_PAD + WORKSPACE_ROW_ICON + self.name.size().x.max(self.about.size().x)
    }

    /// Drawn `width` wide, filled when `selected` (the workspace shown) or hovered.
    fn show(self, ui: &mut egui::Ui, width: f32, selected: bool) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(vec2(width, WORKSPACE_ROW_HEIGHT), Sense::click());
        crate::kit::handles::publish(|| format!("ribbon_workspace:{}", self.workspace.id), rect);
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() {
            ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        }
        let ink = ui.visuals().text_color();
        let icon = Rect::from_min_size(
            pos2(rect.left() + WORKSPACE_ROW_PAD, rect.center().y - WORKSPACE_ROW_ICON / 2.0),
            vec2(WORKSPACE_ROW_ICON, WORKSPACE_ROW_ICON),
        );
        egui::Image::new(self.workspace.icon.clone()).tint(ink).paint_at(ui, icon);
        let x = icon.right() + WORKSPACE_ROW_PAD;
        let gap = 2.0;
        let top = rect.center().y - (self.name.size().y + gap + self.about.size().y) / 2.0;
        let about_at = pos2(x, top + self.name.size().y + gap);
        ui.painter().galley(pos2(x, top), self.name, ink);
        ui.painter().galley(about_at, self.about, ink);
        response
    }
}

/// The tab row, as the file tabs of the title bar (docs/ui/title-bar.md): a band across the card's top
/// in the headers' fill, the tabs centered on it, the shown one in the card's fill opening onto
/// the groups under it.
/// The row's height; the tabs rise above it into the card's margin, by [`TAB_RISE`].
const TAB_ROW_HEIGHT: f32 = 20.0;
const TAB_RISE: f32 = 3.0;
/// Between the tab row and the groups.
const TAB_GAP_BELOW: f32 = 4.0;
/// A tab's room around its label, and between two tabs.
const TAB_PAD: f32 = 12.0;
const TAB_GAP: f32 = 2.0;

fn tab_font() -> FontId {
    FontId::proportional(12.0)
}

/// `inset`: the card's inner margin, which the band spans to reach the card's edges.
fn tab_row(ui: &mut egui::Ui, w: &Workspace, tab: &Tab, width: f32, inset: egui::Margin, actions: &mut Vec<Action>) {
    let labels: Vec<String> = w.tabs.iter().map(|t| (t.label)()).collect();
    let shown = w.tabs.iter().position(|t| t.id == tab.id);
    let ids: Vec<_> = w.tabs.iter().map(|t| egui::Id::unique(("ribbon_tab", t.id))).collect();
    if let Some(k) = tab_strip(ui, &labels, &ids, shown, width, inset, Corners::RoundRight, &[]) {
        actions.push(Action::Ribbon(Change::Tab(w.tabs[k].id)));
    }
}

/// Which top corners of a [`tab_strip`]'s band follow its card's: none (the band under a title
/// bar), both, or only the right one (the ribbon's band, which starts at the workspace cell).
#[derive(Clone, Copy, PartialEq)]
pub enum Corners {
    Square,
    Round,
    RoundRight,
}

/// A row of tabs as the ribbon's (also the journal window's): a faint band, the shown tab
/// open onto the content below, the others greyed until hovered. `ids`: each tab's; `inset`:
/// the margin of the card (or window) the band spans to reach its edges; `corners`: the
/// band's top corners that follow the card's; `handles`: each tab's name for the control
/// protocol, if given. The tab clicked, if any (not the shown one).
#[allow(clippy::too_many_arguments)]
pub fn tab_strip(
    ui: &mut egui::Ui,
    labels: &[String],
    ids: &[egui::Id],
    shown: Option<usize>,
    width: f32,
    inset: egui::Margin,
    corners: Corners,
    handles: &[&'static str],
) -> Option<usize> {
    let (row, _) = ui.allocate_exact_size(vec2(width, TAB_ROW_HEIGHT), Sense::hover());
    let visuals = ui.visuals().clone();
    let band =
        Rect::from_min_max(pos2(row.left() - inset.leftf(), row.top() - inset.topf()), pos2(row.right() + inset.rightf(), row.bottom()));
    let r = visuals.window_corner_radius;
    let (nw, ne) = match corners {
        Corners::Square => (0, 0),
        Corners::Round => (r.nw, r.ne),
        Corners::RoundRight => (0, r.ne),
    };
    ui.painter().rect_filled(band, egui::CornerRadius { nw, ne, sw: 0, se: 0 }, visuals.faint_bg_color);

    // Centered on the card; from its left edge when narrower than the tabs.
    let mut x = (row.center().x - tab_labels_width(ui.painter(), labels) / 2.0).max(row.left() + crate::window::tabs::TAB_RADIUS);
    let font = tab_font();
    let mut clicked = None;
    // The shown tab drawn last: its curves over its neighbours' hover.
    let mut open = None;
    for (k, label) in labels.iter().enumerate() {
        let galley = ui.painter().layout_no_wrap(label.clone(), font.clone(), Color32::WHITE);
        let rect = Rect::from_min_size(pos2(x, row.top() - TAB_RISE), vec2(galley.size().x + 2.0 * TAB_PAD, TAB_ROW_HEIGHT + TAB_RISE));
        x = rect.right() + TAB_GAP;
        if let Some(&name) = handles.get(k) {
            crate::kit::handles::publish(|| name.into(), rect);
        }
        if shown == Some(k) {
            open = Some((label, rect));
            continue;
        }
        let response = ui.interact(rect, ids[k], Sense::click());
        if let Some(fill) = crate::kit::widgets::veil(ui, &response) {
            ui.painter().rect_filled(rect.shrink2(vec2(0.0, 2.0)), crate::kit::widgets::VEIL_RADIUS, fill);
        }
        let ink = if response.hovered() { visuals.text_color() } else { visuals.weak_text_color() };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font.clone(), ink);
        if response.clicked() {
            clicked = Some(k);
        }
    }
    // The band's edge, going round the shown tab.
    let line = visuals.widgets.noninteractive.bg_stroke;
    match open {
        Some((label, rect)) => {
            crate::window::tabs::open_tab(ui.painter(), rect, visuals.window_fill);
            let outline = crate::window::tabs::open_tab_outline(rect, band.left(), band.right(), line.width / 2.0);
            ui.painter().add(egui::Shape::line(outline, line));
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font.clone(), visuals.strong_text_color());
        }
        None => {
            ui.painter().hline(band.x_range(), band.bottom() - line.width / 2.0, line);
        }
    }
    clicked
}

/// One group laid out: where it is, and how [`arrange`] placed it.
struct Laid {
    key: String,
    group: &'static Group,
    pinned: Vec<&'static str>,
    place: Placement,
}

/// The tabs' width, with the gaps between them.
fn tab_labels_width(painter: &egui::Painter, labels: &[String]) -> f32 {
    let width: f32 = labels.iter().map(|l| painter.layout_no_wrap(l.clone(), tab_font(), Color32::WHITE).size().x + 2.0 * TAB_PAD).sum();
    width + labels.len().saturating_sub(1) as f32 * TAB_GAP
}

/// The width a [`tab_strip`] of `labels` needs: its tabs, and the shown one's curves on both
/// sides (a window sized to its content gives it at least that).
pub fn tab_strip_width(painter: &egui::Painter, labels: &[String]) -> f32 {
    tab_labels_width(painter, labels) + 2.0 * crate::window::tabs::TAB_RADIUS
}

/// The tab row's width: its tabs, and the shown one's curves on both sides.
fn tab_row_width(painter: &egui::Painter, w: &Workspace) -> f32 {
    let labels: Vec<String> = w.tabs.iter().map(|t| (t.label)()).collect();
    tab_labels_width(painter, &labels) + 2.0 * crate::window::tabs::TAB_RADIUS
}

/// The groups shown (the fixed one first) and their sizes, as [`arrange`] takes them.
fn measure(painter: &egui::Painter, v: &View) -> (Vec<Laid>, Vec<GroupSize>) {
    let w = v.workspace;
    let tab = v.prefs.tab(v.workspace);
    let labels = v.prefs.labels;
    let mut laid: Vec<Laid> = Vec::new();
    for (tab_of, g) in std::iter::once((None, &w.fixed)).chain(tab.groups.iter().map(|g| (Some(tab), g))) {
        let key = key(w, tab_of, g);
        let pinned = v.prefs.pinned(&key, g);
        laid.push(Laid { key, group: g, pinned, place: Placement::Overflow });
    }
    let measure = |text: &str, font: FontId| painter.layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x;
    let sizes: Vec<GroupSize> = laid
        .iter()
        .map(|l| GroupSize {
            buttons: l
                .pinned
                .iter()
                .map(|id| {
                    if !labels {
                        return (COMPACT_WIDTH, COMPACT_WIDTH);
                    }
                    let label = v.command(id).map(|c| c.label.as_str()).unwrap_or("");
                    // Never narrower than its longest word: labels wrap between words only.
                    let word = label.split_whitespace().map(|w| measure(w, label_font())).fold(0.0, f32::max);
                    let least = (word + 6.0).clamp(BUTTON_MIN_WIDTH, BUTTON_MAX_WIDTH);
                    ((measure(label, label_font()) + 10.0).clamp(least, BUTTON_MAX_WIDTH), least)
                })
                .collect(),
            label: measure(&(l.group.label)(), label_font()) + 14.0,
        })
        .collect();
    (laid, sizes)
}

/// The ribbon card's content width with nothing given up: the workspace cell, every pinned
/// button at its preferred width, and the tab row. The card is never wider (it is narrower
/// when the window is).
pub fn natural_width(ctx: &egui::Context, v: &View) -> f32 {
    let painter = egui::Painter::new(ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
    let (_, sizes) = measure(&painter, v);
    let collapsed = if v.prefs.labels { BUTTON_MIN_WIDTH } else { COMPACT_WIDTH };
    let unreduced: Vec<Placement> = sizes.iter().map(Placement::unreduced).collect();
    let groups = span(&unreduced, &sizes, collapsed);
    let w = v.workspace;
    let tabs = tab_row_width(&painter, w);
    (WORKSPACE_WIDTH + GROUP_GAP + groups.max(tabs)).ceil()
}

/// The ribbon card's content, `width` wide: the workspace cell at the left, over the whole
/// height, then the tab row over the groups. `inset`: the card's inner margin.
pub fn ribbon(ui: &mut egui::Ui, v: &View, width: f32, inset: egui::Margin, actions: &mut Vec<Action>) {
    let labels = v.prefs.labels;
    let button_h = if labels { BUTTON_HEIGHT } else { COMPACT_HEIGHT };
    let spacing = ui.spacing().item_spacing.y;
    // The tab row always, a single tab too: the ribbon keeps its height whatever the workspace.
    let height = TAB_ROW_HEIGHT + spacing + TAB_GAP_BELOW + button_h + LABEL_GAP + GROUP_LABEL_HEIGHT;
    ui.set_width(width);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let (cell, _) = ui.allocate_exact_size(vec2(WORKSPACE_WIDTH, height), Sense::hover());
        // The tab band from the separator, which is drawn over it, across the whole card.
        let band = egui::Margin { left: (GROUP_GAP / 2.0) as i8, ..inset };
        let sx = cell.right() + GROUP_GAP - band.leftf();
        workspace_cell(ui, v, cell, inset, sx, actions);
        ui.add_space(GROUP_GAP);
        let content = (width - WORKSPACE_WIDTH - GROUP_GAP).max(0.0);
        ui.vertical(|ui| groups(ui, v, content, band, actions));
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        ui.painter().vline(sx, cell.top() - inset.topf()..=cell.bottom() + inset.bottomf(), stroke);
    });
}

/// The tab row (`inset`: the card's inner margin, beside the workspace cell) over the groups,
/// `width` wide.
fn groups(ui: &mut egui::Ui, v: &View, width: f32, inset: egui::Margin, actions: &mut Vec<Action>) {
    let w = v.workspace;
    let tab = v.prefs.tab(v.workspace);
    ui.set_width(width);
    tab_row(ui, w, tab, width, inset, actions);
    ui.add_space(TAB_GAP_BELOW);

    let labels = v.prefs.labels;
    let (button_h, collapsed) = if labels { (BUTTON_HEIGHT, BUTTON_MIN_WIDTH) } else { (COMPACT_HEIGHT, COMPACT_WIDTH) };
    let (mut laid, sizes) = measure(ui.painter(), v);
    for (l, p) in laid.iter_mut().zip(arrange(&sizes, width, collapsed)) {
        l.place = p;
    }

    let (row, _) = ui.allocate_exact_size(vec2(width, button_h + LABEL_GAP + GROUP_LABEL_HEIGHT), Sense::hover());
    let running = v.views.iter().find(|c| c.id.starts_with("tool.") && c.checked == Some(true)).map(|c| c.id);
    let mut x = row.left();
    let mut first = true;
    for (l, size) in laid.iter().zip(&sizes) {
        if l.place == Placement::Overflow {
            continue;
        }
        if !first {
            let sx = x + GROUP_GAP / 2.0;
            let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            ui.painter().vline(sx, row.top() + 4.0..=row.bottom() - 4.0, stroke);
            x += GROUP_GAP;
        }
        first = false;
        let gw = l.place.width(size, collapsed);
        let mut bx = x;
        let mut shown = Vec::new();
        let mut collapsed_response = None;
        match &l.place {
            Placement::Row(widths) => {
                // Centered in the group when its label is the wider.
                let buttons = widths.iter().sum::<f32>() + widths.len().saturating_sub(1) as f32 * BUTTON_GAP;
                bx += (gw - buttons) / 2.0;
                for (id, bw) in l.pinned.iter().zip(widths) {
                    let rect = Rect::from_min_size(pos2(bx, row.top()), vec2(*bw, button_h));
                    command_button(ui, v, &l.key, l.group, id, rect, labels, actions);
                    shown.push(*id);
                    bx += bw + BUTTON_GAP;
                }
            }
            Placement::Folded => {
                let rect = Rect::from_min_size(pos2(x + (gw - collapsed) / 2.0, row.top()), vec2(collapsed, button_h));
                collapsed_response = Some(collapsed_button(ui, l, rect, labels));
            }
            Placement::Overflow => {}
        }
        // The group's label opens its menu; lit when the running tool is in the menu only.
        let label_rect = Rect::from_min_size(pos2(x, row.top() + button_h + LABEL_GAP), vec2(gw, GROUP_LABEL_HEIGHT));
        let lit = running.is_some_and(|r| l.group.commands.contains(&r) && !shown.contains(&r));
        let label = group_label(ui, l, label_rect, lit);
        let response = match collapsed_response {
            Some(c) => label.union(c),
            None => label,
        };
        Popup::menu(&response)
            .style(crate::kit::style::menu)
            .id(egui::Id::unique(("ribbon_menu", &l.key)))
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| group_menu(ui, v, &l.key, l.group, actions));
        x += gw;
    }

    // Groups left without a place are reached through the » button, at the right edge.
    if laid.iter().any(|l| l.place == Placement::Overflow) {
        let rect = Rect::from_min_size(pos2(row.right() - MORE_ROOM + 4.0, row.top()), vec2(MORE_ROOM - 4.0, button_h));
        let response = ui.interact(rect, egui::Id::unique("ribbon_more"), Sense::click());
        let visuals = ui.style().interact(&response);
        if response.hovered() {
            ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        }
        egui::Image::new(icons::MORE).tint(visuals.text_color()).paint_at(ui, Rect::from_center_size(rect.center(), vec2(16.0, 16.0)));
        let response = response.tip(t!("ribbon.more"));
        Popup::menu(&response).style(crate::kit::style::menu).show(|ui| {
            for l in laid.iter().filter(|l| l.place == Placement::Overflow) {
                ui.menu_button((l.group.label)(), |ui| group_menu(ui, v, &l.key, l.group, actions));
            }
        });
    }
}

/// The ribbon's own menu (right click on its empty space or on a button).
/// Its items after the check boxes' column (see [`crate::kit::style::menu_item`]), as the items
/// put before them.
pub fn ribbon_menu(ui: &mut egui::Ui, prefs: &Prefs, actions: &mut Vec<Action>) {
    if ui.add(menu_item(t!("ribbon.reset_tab"), None, true)).clicked() {
        actions.push(Action::Ribbon(Change::ResetTab));
    }
    if ui.add(menu_item(t!("ribbon.labels"), Some(prefs.labels), true)).clicked() {
        actions.push(Action::Ribbon(Change::Labels(!prefs.labels)));
    }
}

/// A command's tool tip on the ribbon: its label and shortcut, no more (a tool's options and
/// steps are in its option bar).
fn tip(c: &CommandView) -> String {
    match c.id {
        // Erase is not a command shortcut: the tool reads the delete key.
        "edit.erase" => t!("toolbar.with_shortcut", label = c.label, shortcut = crate::commands::delete_key()),
        _ => c.tip(),
    }
}

/// A pinned command's button: its icon over its label (or its icon alone).
#[allow(clippy::too_many_arguments)]
fn command_button(
    ui: &mut egui::Ui,
    v: &View,
    key: &str,
    group: &'static Group,
    id: &'static str,
    rect: Rect,
    labels: bool,
    actions: &mut Vec<Action>,
) {
    let Some(c) = v.command(id) else { return };
    let selected = c.checked == Some(true);
    let response = ui.interact(rect, egui::Id::unique(("ribbon_button", key, id)), if c.enabled { Sense::click() } else { Sense::hover() });
    crate::kit::handles::publish_enabled(|| format!("ribbon:{id}"), rect, c.enabled);
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || (c.enabled && response.hovered()) {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    } else if c.waiting {
        // Waiting under the active tool, which hands back to it: its frame alone, lighter.
        let waiting = ui.style().interact_selectable(&response, true);
        let stroke = egui::Stroke::new(1.0, ui.visuals().selection.stroke.color.gamma_multiply(0.6));
        ui.painter().rect_stroke(rect, waiting.corner_radius, stroke, StrokeKind::Inside);
    }
    let mut ink = if selected { visuals.text_color() } else { ui.visuals().text_color() };
    if !c.enabled {
        ink = ink.gamma_multiply(0.35);
    }
    let icon_center = if labels { pos2(rect.center().x, rect.top() + 4.0 + ICON / 2.0) } else { rect.center() };
    if let Some(icon) = icons::command(id) {
        egui::Image::new(icon).tint(ink).paint_at(ui, Rect::from_center_size(icon_center, vec2(ICON, ICON)));
    }
    if labels {
        text_lines(ui, &c.label, pos2(rect.center().x, rect.top() + ICON + 7.0), rect.width() - 4.0, 2, ink);
    }
    let response = response.tip(tip(c));
    if response.clicked() {
        actions.push(Action::Command(id));
    }
    Popup::context_menu(&response).style(crate::kit::style::menu).show(|ui| {
        if ui.add(menu_item(t!("ribbon.unpin"), None, true)).clicked() {
            actions.push(Action::Ribbon(Change::TogglePin { key: key.to_owned(), group, id }));
        }
        ui.separator();
        ribbon_menu(ui, v.prefs, actions);
    });
}

/// A collapsed group's button: the icon of its first command; a click opens its menu.
fn collapsed_button(ui: &mut egui::Ui, l: &Laid, rect: Rect, labels: bool) -> egui::Response {
    let response = ui.interact(rect, egui::Id::unique(("ribbon_collapsed", &l.key)), Sense::click());
    crate::kit::handles::publish(|| format!("ribbon_group:{}", l.key), rect);
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    }
    let ink = ui.visuals().text_color();
    let center = if labels { pos2(rect.center().x, rect.top() + 4.0 + ICON / 2.0) } else { rect.center() };
    if let Some(icon) = l.group.commands.first().and_then(|id| icons::command(id)) {
        egui::Image::new(icon).tint(ink).paint_at(ui, Rect::from_center_size(center, vec2(ICON, ICON)));
    }
    chevron(ui, pos2(rect.right() - 7.0, center.y + ICON / 2.0 - 2.0), ink);
    response.tip((l.group.label)())
}

/// A group's label under its buttons, with the chevron of its menu.
fn group_label(ui: &mut egui::Ui, l: &Laid, rect: Rect, lit: bool) -> egui::Response {
    let response = ui.interact(rect, egui::Id::unique(("ribbon_label", &l.key)), Sense::click());
    crate::kit::handles::publish(|| format!("ribbon_label:{}", l.key), rect);
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    let ink = if lit {
        ui.visuals().selection.stroke.color
    } else if response.hovered() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let galley = ui.painter().layout_no_wrap((l.group.label)(), label_font(), ink);
    let at = pos2(rect.center().x - (galley.size().x + CHEVRON_AFTER) / 2.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley.clone(), ink);
    chevron(ui, pos2(at.x + galley.size().x + CHEVRON_AFTER - 6.0, rect.center().y), ink);
    response
}

/// A group's menu: every command, each with its pin, and how pinning works.
fn group_menu(ui: &mut egui::Ui, v: &View, key: &str, group: &'static Group, actions: &mut Vec<Action>) {
    let pinned = v.prefs.pinned(key, group);
    for &id in group.commands {
        let Some(c) = v.command(id) else { continue };
        let on = pinned.contains(&id);
        let (run, pin) = pin_row(
            ui,
            PinRow {
                icon: icons::command(id),
                check: None,
                label: &c.label,
                shortcut: c.shortcut.as_deref(),
                enabled: c.enabled,
                selected: c.checked == Some(true),
                width: 210.0,
                pinned: on,
                pin_tip: if on { t!("ribbon.unpin") } else { t!("ribbon.pin") },
            },
        );
        if run.tip(tip(c)).clicked() {
            actions.push(Action::Command(id));
            ui.close();
        }
        if pin {
            actions.push(Action::Ribbon(Change::TogglePin { key: key.to_owned(), group, id }));
        }
    }
    ui.separator();
    ui.add(egui::Label::new(egui::RichText::new(t!("ribbon.menu_note")).small().weak()).wrap());
}

/// `text` on at most `rows` lines `width` wide, centered on `top_center`, the last line cut
/// with an ellipsis.
fn text_lines(ui: &egui::Ui, text: &str, top_center: egui::Pos2, width: f32, rows: usize, ink: Color32) {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), label_font(), ink, width);
    job.wrap.max_rows = rows;
    job.wrap.overflow_character = Some('…');
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    let at = pos2(top_center.x - galley.rect.center().x, top_center.y);
    ui.painter().galley(at, galley, ink);
}

/// The room a menu's chevron takes after its label (the gap, then the chevron 12 wide, its
/// stroke's 6 in the middle), the pair centred.
const CHEVRON_AFTER: f32 = 15.0;

/// The chevron of a menu, centered on `at`.
pub(crate) fn chevron(ui: &egui::Ui, at: egui::Pos2, ink: Color32) {
    crate::kit::icons::chevron(ui, at, 12.0, ink, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands;
    use crate::workspace::WORKSPACES;

    fn every_group() -> Vec<(String, &'static Group)> {
        let mut out = Vec::new();
        for w in WORKSPACES {
            out.push((key(w, None, &w.fixed), &w.fixed));
            for t in w.tabs {
                for g in t.groups {
                    out.push((key(w, Some(t), g), g));
                }
            }
        }
        out
    }

    #[test]
    fn the_ribbon_names_commands_of_the_registry_with_an_icon() {
        let mut keys = std::collections::BTreeSet::new();
        for (key, g) in every_group() {
            assert!(keys.insert(key.clone()), "{key} twice");
            assert!(!g.commands.is_empty(), "{key} is empty");
            for id in g.commands {
                assert!(commands::find(id).is_some(), "{key}: {id} is not a command");
                assert!(icons::command(id).is_some(), "{key}: {id} has no icon");
            }
            for id in g.pinned {
                assert!(g.commands.contains(id), "{key}: {id} pinned but not in the group");
            }
        }
    }

    #[test]
    fn pins_toggle_in_the_groups_order_and_reset() {
        let mut p = Prefs::default();
        let w = workspace::find(None);
        let tab = p.tab(w);
        let create = &tab.groups[0];
        let k = key(w, Some(tab), create);
        assert_eq!(p.pinned(&k, create), ["tool.box", "tool.cylinder", "tool.prism", "tool.turn"]);
        p.apply(Change::TogglePin { key: k.clone(), group: create, id: "tool.box" }, w);
        assert_eq!(p.pinned(&k, create), ["tool.cylinder", "tool.prism", "tool.turn"]);
        p.apply(Change::TogglePin { key: k.clone(), group: create, id: "tool.box" }, w);
        assert_eq!(p.pinned(&k, create), ["tool.box", "tool.cylinder", "tool.prism", "tool.turn"]);
        assert!(p.pins.is_empty(), "back to the defaults, nothing is kept");
        let view = &w.tabs[2].groups[2];
        let kv = key(w, Some(&w.tabs[2]), view);
        p.apply(Change::Tab("inspect"), w);
        p.apply(Change::TogglePin { key: kv.clone(), group: view, id: "view.iso" }, w);
        p.apply(Change::TogglePin { key: kv.clone(), group: view, id: "view.top" }, w);
        assert_eq!(p.pinned(&kv, view), ["view.zoom_extents", "view.top", "view.iso"]);
        p.apply(Change::ResetTab, w);
        assert!(p.pins.is_empty());
    }

    #[test]
    fn unknown_pins_and_tabs_are_ignored() {
        let p: Prefs = serde_json::from_str(
            r#"{"workspace":"gone","tabs":{"modelling":"gone"},"pins":{"modelling/selection":["tool.select","nope"]}}"#,
        )
        .unwrap();
        let w = workspace::find(None);
        assert_eq!(p.tab(w).id, "build");
        assert_eq!(p.pinned(&key(w, None, &w.fixed), &w.fixed), ["tool.select"]);
        assert!(!p.labels);
    }

    fn groups() -> Vec<GroupSize> {
        vec![
            GroupSize { buttons: vec![(60.0, 50.0); 2], label: 40.0 },
            GroupSize { buttons: vec![(70.0, 50.0); 3], label: 40.0 },
            GroupSize { buttons: vec![(60.0, 50.0); 3], label: 40.0 },
            GroupSize { buttons: vec![], label: 40.0 },
        ]
    }

    fn total(placed: &[Placement], g: &[GroupSize]) -> f32 {
        let more = if placed.contains(&Placement::Overflow) { MORE_ROOM } else { 0.0 };
        span(placed, g, 50.0) + more
    }

    /// The buttons a group shows on the ribbon.
    fn showing(p: &Placement) -> usize {
        match p {
            Placement::Row(widths) => widths.len(),
            _ => 0,
        }
    }

    fn sized(buttons: &[(f32, f32)], label: f32) -> GroupSize {
        GroupSize { buttons: buttons.to_vec(), label }
    }

    #[test]
    fn everything_shows_when_there_is_room() {
        let g = groups();
        let placed = arrange(&g, 2000.0, 50.0);
        assert!(placed[..3].iter().all(|p| matches!(p, Placement::Row(_))));
        assert_eq!(placed[1], Placement::Row(vec![70.0; 3]));
        // A group without pins is one button opening its menu.
        assert_eq!(placed[3], Placement::Folded);
    }

    #[test]
    fn buttons_narrow_before_any_is_hidden() {
        let g = groups();
        let full = total(&arrange(&g, 2000.0, 50.0), &g);
        let placed = arrange(&g, full - 20.0, 50.0);
        assert!(placed.iter().zip(&g).all(|(p, g)| showing(p) == g.buttons.len()));
        let Placement::Row(widths) = &placed[1] else { panic!("the group stays on the ribbon") };
        assert!(widths[0] < 70.0);
        assert!(total(&placed, &g) <= full - 20.0);
    }

    #[test]
    fn groups_give_way_one_button_at_a_time_then_go_into_the_more_menu() {
        let g = groups();
        // (groups in the » menu, buttons shown) as the room shrinks.
        let mut last = (0, usize::MAX);
        let mut room = 2000.0;
        while room > 0.0 {
            let placed = arrange(&g, room, 50.0);
            let more = placed.iter().filter(|p| **p == Placement::Overflow).count();
            let on: usize = placed.iter().map(showing).sum();
            assert!(more >= last.0, "less room never brings a group back from the » menu");
            // A group going into the » menu frees room the others may take back.
            assert!(more > last.0 || on <= last.1, "less room never shows more buttons");
            last = (more, on);
            if total(&placed, &g) > room {
                // Only when even the » menu alone does not fit.
                assert!(placed.iter().all(|p| *p == Placement::Overflow));
            }
            assert_eq!(placed, arrange(&g, room, 50.0), "the same room, the same result");
            room -= 7.0;
        }
        let placed = arrange(&g, 300.0, 50.0);
        assert!(placed.contains(&Placement::Folded));
        let placed = arrange(&g, 120.0, 50.0);
        assert_eq!(*placed.last().unwrap(), Placement::Overflow, "the last groups go first");
        assert_ne!(placed[0], Placement::Overflow);
    }

    #[test]
    fn the_narrowing_is_the_smallest_whole_number_of_points() {
        let g = [sized(&[(60.5, 50.0), (55.0, 54.0)], 10.0)];
        // 117.5 wide at rest; a drop of 3 gives 57.5 + 54 + 2 = 113.5.
        assert_eq!(arrange(&g, 114.0, 50.0), [Placement::Row(vec![57.5, 54.0])]);
        assert_eq!(arrange(&g, 113.5, 50.0), [Placement::Row(vec![57.5, 54.0])]);
        assert_eq!(arrange(&g, 113.0, 50.0), [Placement::Row(vec![56.5, 54.0])]);
    }

    #[test]
    fn buttons_go_from_the_fullest_group_the_rightmost_first() {
        let g = [sized(&[(30.0, 30.0); 3], 0.0), sized(&[(30.0, 30.0); 3], 0.0)];
        // 94 + 13 + 94 = 201 at the least widths.
        let row = |n: usize| Placement::Row(vec![30.0; n]);
        assert_eq!(arrange(&g, 201.0, 20.0), [row(3), row(3)]);
        assert_eq!(arrange(&g, 200.0, 20.0), [row(3), row(2)]);
        assert_eq!(arrange(&g, 169.0, 20.0), [row(3), row(2)]);
        assert_eq!(arrange(&g, 168.0, 20.0), [row(2), row(2)]);
        // From two buttons, a group folds; the rightmost before the other.
        assert_eq!(arrange(&g, 136.0, 20.0), [row(2), Placement::Folded]);
        assert_eq!(arrange(&g, 94.0, 20.0), [Placement::Folded, Placement::Folded]);
    }

    #[test]
    fn a_single_button_group_folds_last() {
        let g = [sized(&[(40.0, 40.0)], 0.0), sized(&[(40.0, 40.0); 2], 0.0)];
        assert_eq!(arrange(&g, 135.0, 20.0), [Placement::Row(vec![40.0]), Placement::Row(vec![40.0; 2])]);
        assert_eq!(arrange(&g, 73.0, 20.0), [Placement::Row(vec![40.0]), Placement::Folded]);
        assert_eq!(arrange(&g, 72.0, 20.0), [Placement::Folded, Placement::Folded]);
    }

    #[test]
    fn the_more_menu_takes_the_fewest_trailing_groups() {
        let g = [sized(&[(40.0, 40.0); 2], 0.0), sized(&[(40.0, 40.0); 2], 0.0), sized(&[(40.0, 40.0); 2], 0.0)];
        // Three folded groups need 116; two, with the » button, 103; one, 60.
        let folded = Placement::Folded;
        assert_eq!(arrange(&g, 116.0, 30.0), [folded.clone(), folded.clone(), folded.clone()]);
        assert_eq!(arrange(&g, 115.0, 30.0), [folded.clone(), folded.clone(), Placement::Overflow]);
        assert_eq!(arrange(&g, 103.0, 30.0), [folded.clone(), folded.clone(), Placement::Overflow]);
        assert_eq!(arrange(&g, 102.0, 30.0), [folded.clone(), Placement::Overflow, Placement::Overflow]);
        assert_eq!(arrange(&g, 60.0, 30.0), [folded, Placement::Overflow, Placement::Overflow]);
        // Nothing fits beside the » button: every group is in it.
        assert_eq!(arrange(&g, 59.0, 30.0), vec![Placement::Overflow; 3]);
    }

    #[test]
    fn a_room_that_is_not_finite_counts_as_none() {
        let g = [sized(&[(40.0, 40.0); 2], 0.0)];
        for room in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(arrange(&g, room, 30.0), arrange(&g, 0.0, 30.0));
        }
    }
}

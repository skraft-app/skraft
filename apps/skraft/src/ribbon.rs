//! The workspace switcher and the ribbon (docs/ui/ribbon.md).
//!
//! The ribbon is data: [`WORKSPACES`] → tabs → groups → command ids of the registry
//! (docs/ui/commands.md), each group with its default pins. A group shows its pinned commands as buttons;
//! its label (▾) opens its menu, which lists every command of the group with a pin. The user's
//! pins, the tab shown per workspace and the labels' switch are a preference ([`Prefs`]).
//! When the ribbon is too narrow, [`fit`] decides what gives way. Drawing only reads: what the
//! user chose comes back as actions ([`Change`] for the ribbon's own state).

use std::collections::BTreeMap;

use egui::{Align2, Color32, FontId, ImageSource, Popup, PopupCloseBehavior, Rect, Sense, StrokeKind, pos2, vec2};
use serde::{Deserialize, Serialize};
use skraft_i18n::t;

use crate::icons;
use crate::panels::{Action, CommandView};
use crate::style::menu_item;

// ---- the description -------------------------------------------------------------------------

/// A workspace: a tab row, and a group shown first whatever the tab.
pub struct Workspace {
    pub id: &'static str,
    pub label: fn() -> String,
    pub icon: ImageSource<'static>,
    /// Before the tab's own groups, on every tab (Selection).
    pub fixed: Group,
    pub tabs: &'static [Tab],
}

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

const fn group(id: &'static str, label: fn() -> String, commands: &'static [&'static str], pinned: &'static [&'static str]) -> Group {
    Group { id, label, commands, pinned }
}

const STANDARD_VIEWS: &[&str] = &["view.zoom_extents", "view.top", "view.front", "view.right", "view.back", "view.left", "view.iso"];

/// Every workspace. Cut list and Drawing are planned (docs/ui/ribbon.md): not listed until they exist.
pub const WORKSPACES: &[Workspace] = &[Workspace {
    id: "modelling",
    label: || t!("ribbon.workspace.modelling"),
    icon: icons::MODELLING,
    fixed: group("selection", || t!("ribbon.group.selection"), &["tool.select", "tool.erase"], &["tool.select", "tool.erase"]),
    tabs: &[
        Tab {
            id: "build",
            label: || t!("ribbon.tab.build"),
            groups: &[
                group(
                    "draw",
                    || t!("ribbon.group.draw"),
                    &["tool.box", "tool.cylinder", "tool.prism", "tool.turn"],
                    &["tool.box", "tool.cylinder", "tool.prism", "tool.turn"],
                ),
                group(
                    "handle",
                    || t!("ribbon.group.handle"),
                    &["tool.move", "tool.rotate", "tool.copy_line", "tool.copy_grid", "tool.distribute"],
                    &["tool.move", "tool.rotate", "tool.copy_line", "tool.copy_grid", "tool.distribute"],
                ),
                group(
                    "reshape",
                    || t!("ribbon.group.reshape"),
                    &["tool.stretch", "tool.union", "tool.difference", "tool.intersection"],
                    &["tool.stretch", "tool.union", "tool.difference", "tool.intersection"],
                ),
                group("appearance", || t!("ribbon.group.appearance"), &["tool.paint", "tool.sample"], &["tool.paint", "tool.sample"]),
            ],
        },
        Tab {
            id: "assemble",
            label: || t!("ribbon.tab.assemble"),
            groups: &[
                group(
                    "components",
                    || t!("ribbon.group.components"),
                    &["edit.make_assembly", "edit.explode", "edit.make_unique", "edit.group_bodies", "edit.ungroup_bodies"],
                    &["edit.make_assembly", "edit.explode", "edit.make_unique", "edit.group_bodies", "edit.ungroup_bodies"],
                ),
            ],
        },
        Tab {
            id: "inspect",
            label: || t!("ribbon.tab.inspect"),
            groups: &[
                group("measure", || t!("ribbon.group.measure"), &["tool.measure", "edit.delete_guides"], &["tool.measure"]),
                group("section", || t!("ribbon.group.section"), &["tool.section", "view.section_clear"], &["tool.section"]),
                group("view", || t!("ribbon.group.view"), STANDARD_VIEWS, &["view.zoom_extents"]),
            ],
        },
    ],
}];

/// The workspace `id`, or the first one.
pub fn workspace(id: &str) -> &'static Workspace {
    WORKSPACES.iter().find(|w| w.id == id).unwrap_or(&WORKSPACES[0])
}

impl Workspace {
    /// The tab `id`, or the first one.
    pub fn tab(&self, id: Option<&str>) -> &'static Tab {
        let tabs: &'static [Tab] = self.tabs;
        tabs.iter().find(|t| Some(t.id) == id).unwrap_or(&tabs[0])
    }
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// The workspace shown.
    pub workspace: String,
    /// The tab shown, by workspace.
    pub tabs: BTreeMap<String, String>,
    /// The pinned commands of the groups the user changed, by [`key`] (absent: the defaults).
    pub pins: BTreeMap<String, Vec<String>>,
    /// Buttons show their label under their icon.
    pub labels: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { workspace: WORKSPACES[0].id.to_owned(), tabs: BTreeMap::new(), pins: BTreeMap::new(), labels: true }
    }
}

impl Prefs {
    pub fn workspace(&self) -> &'static Workspace {
        workspace(&self.workspace)
    }

    pub fn tab(&self) -> &'static Tab {
        let w = self.workspace();
        w.tab(self.tabs.get(w.id).map(String::as_str))
    }

    /// The group's pinned commands, in the group's order (unknown ids are ignored).
    pub fn pinned(&self, key: &str, group: &Group) -> Vec<&'static str> {
        match self.pins.get(key) {
            Some(ids) => group.commands.iter().copied().filter(|c| ids.iter().any(|i| i == c)).collect(),
            None => group.pinned.to_vec(),
        }
    }

    pub fn apply(&mut self, change: Change) {
        match change {
            Change::Workspace(id) => self.workspace = id.to_owned(),
            Change::Tab(id) => {
                self.tabs.insert(self.workspace().id.to_owned(), id.to_owned());
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
                let (w, tab) = (self.workspace(), self.tab());
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
    Workspace(&'static str),
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

/// How a group is drawn on a ribbon that may be too narrow for every button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Its first `shown` buttons; its menu lists every command anyway.
    Buttons,
    /// One button that opens its menu.
    Collapsed,
    /// In the » menu at the end of the ribbon.
    More,
}

/// A group as [`fit`] sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupSize {
    /// Each pinned button's preferred and least width.
    pub buttons: Vec<(f32, f32)>,
    /// Its label's width: the group is never narrower.
    pub label: f32,
}

/// Where [`fit`] put a group: its first `shown` buttons, at these widths.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub fit: Fit,
    pub shown: usize,
    pub widths: Vec<f32>,
}

/// Room between two buttons, and between two groups (their separator).
pub const BUTTON_GAP: f32 = 2.0;
pub const GROUP_GAP: f32 = 13.0;
/// The » button's room, kept only while some group is in it.
pub const MORE_ROOM: f32 = 30.0;

impl Plan {
    fn full(g: &GroupSize) -> Self {
        let fit = if g.buttons.is_empty() { Fit::Collapsed } else { Fit::Buttons };
        Plan { fit, shown: g.buttons.len(), widths: g.buttons.iter().map(|b| b.0).collect() }
    }

    /// The group's width on the ribbon (0 in the » menu); `collapsed`: a collapsed group's button.
    pub fn width(&self, g: &GroupSize, collapsed: f32) -> f32 {
        match self.fit {
            Fit::More => 0.0,
            Fit::Collapsed => collapsed.max(g.label),
            Fit::Buttons => {
                let shown = &self.widths[..self.shown];
                (shown.iter().sum::<f32>() + shown.len().saturating_sub(1) as f32 * BUTTON_GAP).max(g.label)
            }
        }
    }
}

fn used(plans: &[Plan], groups: &[GroupSize], collapsed: f32) -> f32 {
    let on: Vec<f32> = plans.iter().zip(groups).filter(|(p, _)| p.fit != Fit::More).map(|(p, g)| p.width(g, collapsed)).collect();
    on.iter().sum::<f32>() + on.len().saturating_sub(1) as f32 * GROUP_GAP
}

/// Fits the groups into `room`, giving up as little as it takes, one step at a time:
/// 1. buttons narrow together, from their preferred width to their least;
/// 2. groups hand their last buttons to their menu one at a time — the group showing the most
///    first, the rightmost of equals — down to one collapsed button;
/// 3. the last groups go into the » menu (which takes [`MORE_ROOM`]).
///
/// It depends on `room` only (no state from earlier frames), so it never flickers; nothing is
/// ever unreachable. `collapsed` is a collapsed group's button width.
pub fn fit(groups: &[GroupSize], room: f32, collapsed: f32) -> Vec<Plan> {
    let room = if room.is_finite() { room } else { 0.0 };
    if let Some(plans) = shrink(groups, room, collapsed) {
        return plans;
    }
    for kept in (0..groups.len()).rev() {
        if let Some(mut plans) = shrink(&groups[..kept], room - MORE_ROOM, collapsed) {
            plans.extend(groups[kept..].iter().map(|_| Plan { fit: Fit::More, shown: 0, widths: Vec::new() }));
            return plans;
        }
    }
    // Narrower than the » button alone: everything in it.
    groups.iter().map(|_| Plan { fit: Fit::More, shown: 0, widths: Vec::new() }).collect()
}

/// Steps 1 and 2 of [`fit`]: every group on the ribbon, or None if even collapsed they don't fit.
fn shrink(groups: &[GroupSize], room: f32, collapsed: f32) -> Option<Vec<Plan>> {
    let mut plans: Vec<Plan> = groups.iter().map(Plan::full).collect();
    if used(&plans, groups, collapsed) <= room {
        return Some(plans);
    }
    // 1. Narrow together in one-point steps, each button down to its least width.
    let most = groups.iter().flat_map(|g| g.buttons.iter().map(|b| b.0 - b.1)).fold(0.0f32, f32::max).ceil() as usize;
    for step in 1..=most {
        for (plan, g) in plans.iter_mut().zip(groups) {
            for (w, b) in plan.widths.iter_mut().zip(&g.buttons) {
                *w = (b.0 - step as f32).max(b.1);
            }
        }
        if used(&plans, groups, collapsed) <= room {
            return Some(plans);
        }
    }
    // 2. One button at a time into the menus.
    while used(&plans, groups, collapsed) > room {
        let (i, _) = plans
            .iter()
            .enumerate()
            .filter(|(_, p)| p.fit == Fit::Buttons)
            .max_by(|(i, a), (j, b)| a.shown.cmp(&b.shown).then(i.cmp(j)))?;
        let p = &mut plans[i];
        p.shown -= 1;
        if p.shown <= 1 {
            p.fit = Fit::Collapsed;
        }
    }
    Some(plans)
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
/// The workspace card's width.
pub const WORKSPACE_WIDTH: f32 = 92.0;

fn label_font() -> FontId {
    FontId::proportional(11.5)
}

/// What the ribbon shows this frame.
pub struct View<'a> {
    pub prefs: &'a Prefs,
    pub views: &'a [CommandView],
}

impl View<'_> {
    fn command(&self, id: &str) -> Option<&CommandView> {
        self.views.iter().find(|v| v.id == id)
    }
}

/// The workspace card: the workspace's icon and name; a click lists the workspaces.
pub fn workspace_card(ui: &mut egui::Ui, v: &View, height: f32, actions: &mut Vec<Action>) {
    let current = v.prefs.workspace();
    let (rect, response) = ui.allocate_exact_size(vec2(WORKSPACE_WIDTH, height), Sense::click());
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    let ink = ui.visuals().text_color();
    let icon = Rect::from_center_size(rect.center() - vec2(0.0, 10.0), vec2(28.0, 28.0));
    egui::Image::new(current.icon.clone()).tint(ink).paint_at(ui, icon);
    let label = ui.painter().layout_no_wrap((current.label)(), FontId::proportional(12.5), ink);
    let at = pos2(rect.center().x - (label.size().x + CHEVRON_AFTER) / 2.0, icon.bottom() + 6.0);
    ui.painter().galley(at, label.clone(), ink);
    chevron(ui, pos2(at.x + label.size().x + CHEVRON_AFTER - 6.0, at.y + label.size().y / 2.0), ink);
    let response = response.on_hover_text(t!("ribbon.workspace.tip"));
    Popup::menu(&response).style(crate::style::menu).show(|ui| {
        for w in WORKSPACES {
            if ui.selectable_label(w.id == current.id, (w.label)()).clicked() && w.id != current.id {
                actions.push(Action::Ribbon(Change::Workspace(w.id)));
            }
        }
    });
}

/// The tab row, as the file tabs of the title bar (docs/ui/title-bar.md): a band across the card's top
/// in the headers' fill, the tabs centered on it, the shown one in the card's fill opening onto
/// the groups under it.
/// The row's height; the tabs rise above it into the card's margin, by [`TAB_RISE`].
const TAB_ROW_HEIGHT: f32 = 20.0;
const TAB_RISE: f32 = 3.0;
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
    if let Some(k) = tab_strip(ui, &labels, &ids, shown, width, inset, true, &[]) {
        actions.push(Action::Ribbon(Change::Tab(w.tabs[k].id)));
    }
}

/// A row of tabs as the ribbon's (also the journal window's): a faint band, the shown tab
/// open onto the content below, the others greyed until hovered. `ids`: each tab's; `inset`:
/// the margin of the card (or window) the band spans to reach its edges; `round_top`: the
/// band's top corners follow the card's; `handles`: each tab's name for the control protocol,
/// if given. The tab clicked, if any (not the shown one).
#[allow(clippy::too_many_arguments)]
pub fn tab_strip(
    ui: &mut egui::Ui,
    labels: &[String],
    ids: &[egui::Id],
    shown: Option<usize>,
    width: f32,
    inset: egui::Margin,
    round_top: bool,
    handles: &[&'static str],
) -> Option<usize> {
    let (row, _) = ui.allocate_exact_size(vec2(width, TAB_ROW_HEIGHT), Sense::hover());
    let visuals = ui.visuals().clone();
    let band =
        Rect::from_min_max(pos2(row.left() - inset.leftf(), row.top() - inset.topf()), pos2(row.right() + inset.rightf(), row.bottom()));
    let r = if round_top { visuals.window_corner_radius } else { egui::CornerRadius::ZERO };
    ui.painter().rect_filled(band, egui::CornerRadius { nw: r.nw, ne: r.ne, sw: 0, se: 0 }, visuals.faint_bg_color);

    // Centered on the card; from its left edge when narrower than the tabs.
    let mut x = (row.center().x - tab_labels_width(ui.painter(), labels) / 2.0).max(row.left() + crate::tabs::TAB_RADIUS);
    let font = tab_font();
    let mut clicked = None;
    // The shown tab drawn last: its curves over its neighbours' hover.
    let mut open = None;
    for (k, label) in labels.iter().enumerate() {
        let galley = ui.painter().layout_no_wrap(label.clone(), font.clone(), Color32::WHITE);
        let rect = Rect::from_min_size(pos2(x, row.top() - TAB_RISE), vec2(galley.size().x + 2.0 * TAB_PAD, TAB_ROW_HEIGHT + TAB_RISE));
        x = rect.right() + TAB_GAP;
        if let Some(&name) = handles.get(k) {
            crate::handles::publish(|| name.into(), rect);
        }
        if shown == Some(k) {
            open = Some((label, rect));
            continue;
        }
        let response = ui.interact(rect, ids[k], Sense::click());
        if let Some(fill) = crate::tabs::veil(ui, &response) {
            ui.painter().rect_filled(rect.shrink2(vec2(0.0, 2.0)), crate::tabs::VEIL_RADIUS, fill);
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
            crate::tabs::open_tab(ui.painter(), rect, visuals.window_fill);
            let outline = crate::tabs::open_tab_outline(rect, band.left(), band.right(), line.width / 2.0);
            ui.painter().add(egui::Shape::line(outline, line));
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font.clone(), visuals.strong_text_color());
        }
        None => {
            ui.painter().hline(band.x_range(), band.bottom() - line.width / 2.0, line);
        }
    }
    clicked
}

/// One group laid out: where it is, and how [`fit`] placed it.
struct Laid {
    key: String,
    group: &'static Group,
    pinned: Vec<&'static str>,
    plan: Plan,
}

/// The tabs' width, with the gaps between them.
fn tab_labels_width(painter: &egui::Painter, labels: &[String]) -> f32 {
    let width: f32 = labels.iter().map(|l| painter.layout_no_wrap(l.clone(), tab_font(), Color32::WHITE).size().x + 2.0 * TAB_PAD).sum();
    width + labels.len().saturating_sub(1) as f32 * TAB_GAP
}

/// The width a [`tab_strip`] of `labels` needs: its tabs, and the shown one's curves on both
/// sides (a window sized to its content gives it at least that).
pub fn tab_strip_width(painter: &egui::Painter, labels: &[String]) -> f32 {
    tab_labels_width(painter, labels) + 2.0 * crate::tabs::TAB_RADIUS
}

/// The tab row's width: its tabs, and the shown one's curves on both sides.
fn tab_row_width(painter: &egui::Painter, w: &Workspace) -> f32 {
    let labels: Vec<String> = w.tabs.iter().map(|t| (t.label)()).collect();
    tab_labels_width(painter, &labels) + 2.0 * crate::tabs::TAB_RADIUS
}

/// The groups shown (the fixed one first) and their sizes, as [`fit`] sees them.
fn measure(painter: &egui::Painter, v: &View) -> (Vec<Laid>, Vec<GroupSize>) {
    let w = v.prefs.workspace();
    let tab = v.prefs.tab();
    let labels = v.prefs.labels;
    let mut laid: Vec<Laid> = Vec::new();
    for (tab_of, g) in std::iter::once((None, &w.fixed)).chain(tab.groups.iter().map(|g| (Some(tab), g))) {
        let key = key(w, tab_of, g);
        let pinned = v.prefs.pinned(&key, g);
        laid.push(Laid { key, group: g, pinned, plan: Plan { fit: Fit::More, shown: 0, widths: Vec::new() } });
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

/// The ribbon card's content width with nothing given up: every pinned button at its preferred
/// width, and the tab row. The card is never wider (it is narrower when the window is).
pub fn natural_width(ctx: &egui::Context, v: &View) -> f32 {
    let painter = egui::Painter::new(ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
    let (_, sizes) = measure(&painter, v);
    let collapsed = if v.prefs.labels { BUTTON_MIN_WIDTH } else { COMPACT_WIDTH };
    let plans: Vec<Plan> = sizes.iter().map(Plan::full).collect();
    let groups = used(&plans, &sizes, collapsed);
    let w = v.prefs.workspace();
    let tabs = if w.tabs.len() > 1 { tab_row_width(&painter, w) } else { 0.0 };
    groups.max(tabs).ceil()
}

/// The ribbon card's content, `width` wide: the tab row over the groups.
/// `inset`: the card's inner margin.
pub fn ribbon(ui: &mut egui::Ui, v: &View, width: f32, inset: egui::Margin, actions: &mut Vec<Action>) {
    let w = v.prefs.workspace();
    let tab = v.prefs.tab();
    ui.set_width(width);
    if w.tabs.len() > 1 {
        tab_row(ui, w, tab, width, inset, actions);
        ui.add_space(4.0);
    }

    let labels = v.prefs.labels;
    let (button_h, collapsed) = if labels { (BUTTON_HEIGHT, BUTTON_MIN_WIDTH) } else { (COMPACT_HEIGHT, COMPACT_WIDTH) };
    let (mut laid, sizes) = measure(ui.painter(), v);
    let plans = fit(&sizes, width, collapsed);
    for (l, p) in laid.iter_mut().zip(plans) {
        l.plan = p;
    }

    let (row, _) = ui.allocate_exact_size(vec2(width, button_h + LABEL_GAP + GROUP_LABEL_HEIGHT), Sense::hover());
    let running = v.views.iter().find(|c| c.id.starts_with("tool.") && c.checked == Some(true)).map(|c| c.id);
    let mut x = row.left();
    let mut first = true;
    for (l, size) in laid.iter().zip(&sizes) {
        if l.plan.fit == Fit::More {
            continue;
        }
        if !first {
            let sx = x + GROUP_GAP / 2.0;
            let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            ui.painter().vline(sx, row.top() + 4.0..=row.bottom() - 4.0, stroke);
            x += GROUP_GAP;
        }
        first = false;
        let gw = l.plan.width(size, collapsed);
        let mut bx = x;
        let mut shown = Vec::new();
        let mut collapsed_response = None;
        match l.plan.fit {
            Fit::Buttons => {
                // Centered in the group when its label is the wider.
                let buttons: f32 = l.plan.widths[..l.plan.shown].iter().sum::<f32>() + (l.plan.shown.saturating_sub(1)) as f32 * BUTTON_GAP;
                bx += (gw - buttons) / 2.0;
                for (id, bw) in l.pinned.iter().zip(&l.plan.widths).take(l.plan.shown) {
                    let rect = Rect::from_min_size(pos2(bx, row.top()), vec2(*bw, button_h));
                    command_button(ui, v, &l.key, l.group, id, rect, labels, actions);
                    shown.push(*id);
                    bx += bw + BUTTON_GAP;
                }
            }
            Fit::Collapsed => {
                let rect = Rect::from_min_size(pos2(x + (gw - collapsed) / 2.0, row.top()), vec2(collapsed, button_h));
                collapsed_response = Some(collapsed_button(ui, l, rect, labels));
            }
            Fit::More => {}
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
            .style(crate::style::menu)
            .id(egui::Id::unique(("ribbon_menu", &l.key)))
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| group_menu(ui, v, &l.key, l.group, actions));
        x += gw;
    }

    // The groups that did not fit: in the » menu.
    if laid.iter().any(|l| l.plan.fit == Fit::More) {
        let rect = Rect::from_min_size(pos2(row.right() - MORE_ROOM + 4.0, row.top()), vec2(MORE_ROOM - 4.0, button_h));
        let response = ui.interact(rect, egui::Id::unique("ribbon_more"), Sense::click());
        let visuals = ui.style().interact(&response);
        if response.hovered() {
            ui.painter().rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        }
        egui::Image::new(icons::MORE).tint(visuals.text_color()).paint_at(ui, Rect::from_center_size(rect.center(), vec2(16.0, 16.0)));
        let response = response.on_hover_text(t!("ribbon.more"));
        Popup::menu(&response).style(crate::style::menu).show(|ui| {
            for l in laid.iter().filter(|l| l.plan.fit == Fit::More) {
                ui.menu_button((l.group.label)(), |ui| group_menu(ui, v, &l.key, l.group, actions));
            }
        });
    }
}

/// The ribbon's own menu (right click on its empty space or on a button).
/// Its items after the check boxes' column (see [`crate::style::menu_item`]), as the items
/// put before them.
pub fn ribbon_menu(ui: &mut egui::Ui, prefs: &Prefs, actions: &mut Vec<Action>) {
    if ui.add(menu_item(t!("ribbon.reset_tab"), None, true)).clicked() {
        actions.push(Action::Ribbon(Change::ResetTab));
    }
    if ui.add(menu_item(t!("ribbon.labels"), Some(prefs.labels), true)).clicked() {
        actions.push(Action::Ribbon(Change::Labels(!prefs.labels)));
    }
}

/// A command's tool tip on the ribbon: its label and shortcut, and what its tip adds.
fn tip(c: &CommandView) -> String {
    match c.id {
        "tool.move" | "tool.rotate" => {
            t!("toolbar.with_hint", tip = c.tip(), hint = t!("toolbar.copy_hint", copy_key = skraft_tools::tools::copy_key()))
        }
        "tool.union" | "tool.difference" | "tool.intersection" => t!("toolbar.with_hint", tip = c.tip(), hint = t!("toolbar.boolean_hint")),
        // Erase is not a command shortcut: the tool reads the delete key.
        "edit.erase" => t!("toolbar.with_shortcut", label = c.label, shortcut = crate::panels::delete_key()),
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
    crate::handles::publish_enabled(|| format!("ribbon:{id}"), rect, c.enabled);
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
    let response = response.on_hover_text(tip(c));
    if response.clicked() {
        actions.push(Action::Command(id));
    }
    Popup::context_menu(&response).style(crate::style::menu).show(|ui| {
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
    crate::handles::publish(|| format!("ribbon_group:{}", l.key), rect);
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
    response.on_hover_text((l.group.label)())
}

/// A group's label under its buttons, with the chevron of its menu.
fn group_label(ui: &mut egui::Ui, l: &Laid, rect: Rect, lit: bool) -> egui::Response {
    let response = ui.interact(rect, egui::Id::unique(("ribbon_label", &l.key)), Sense::click());
    crate::handles::publish(|| format!("ribbon_label:{}", l.key), rect);
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
    let ink = ui.visuals().text_color();
    for &id in group.commands {
        let Some(c) = v.command(id) else { continue };
        ui.horizontal(|ui| {
            let mut button = match icons::command(id) {
                Some(icon) => egui::Button::image_and_text(egui::Image::new(icon).fit_to_exact_size(vec2(16.0, 16.0)).tint(ink), &c.label),
                None => egui::Button::new(&c.label),
            };
            button = button.selected(c.checked == Some(true)).min_size(vec2(210.0, 22.0));
            // Always a shortcut, if empty: it pushes the label to the left (else centered).
            button = button.shortcut_text(c.shortcut.as_deref().unwrap_or(""));
            if ui.add_enabled(c.enabled, button).on_hover_text(tip(c)).clicked() {
                actions.push(Action::Command(id));
                ui.close();
            }
            let on = pinned.contains(&id);
            // Pinned: a filled pin in the accent; unpinned: a faint outline.
            let (icon, tint) = if on {
                (icons::PIN_FILLED, ui.visuals().selection.stroke.color)
            } else {
                (icons::PIN, ui.visuals().weak_text_color().gamma_multiply(0.55))
            };
            let pin = egui::Button::image(egui::Image::new(icon).fit_to_exact_size(vec2(14.0, 14.0)).tint(tint)).frame(false);
            let tip = if on { t!("ribbon.unpin") } else { t!("ribbon.pin") };
            if ui.add(pin).on_hover_text(tip).clicked() {
                actions.push(Action::Ribbon(Change::TogglePin { key: key.to_owned(), group, id }));
            }
        });
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
    crate::icons::chevron(ui, at, 12.0, ink, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands;

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
        let w = p.workspace();
        let tab = p.tab();
        let create = &tab.groups[0];
        let k = key(w, Some(tab), create);
        assert_eq!(p.pinned(&k, create), ["tool.box", "tool.cylinder", "tool.prism", "tool.turn"]);
        p.apply(Change::TogglePin { key: k.clone(), group: create, id: "tool.box" });
        assert_eq!(p.pinned(&k, create), ["tool.cylinder", "tool.prism", "tool.turn"]);
        p.apply(Change::TogglePin { key: k.clone(), group: create, id: "tool.box" });
        assert_eq!(p.pinned(&k, create), ["tool.box", "tool.cylinder", "tool.prism", "tool.turn"]);
        assert!(p.pins.is_empty(), "back to the defaults, nothing is kept");
        let view = &w.tabs[2].groups[2];
        let kv = key(w, Some(&w.tabs[2]), view);
        p.apply(Change::Tab("inspect"));
        p.apply(Change::TogglePin { key: kv.clone(), group: view, id: "view.iso" });
        p.apply(Change::TogglePin { key: kv.clone(), group: view, id: "view.top" });
        assert_eq!(p.pinned(&kv, view), ["view.zoom_extents", "view.top", "view.iso"]);
        p.apply(Change::ResetTab);
        assert!(p.pins.is_empty());
    }

    #[test]
    fn unknown_pins_and_tabs_are_ignored() {
        let p: Prefs = serde_json::from_str(
            r#"{"workspace":"gone","tabs":{"modelling":"gone"},"pins":{"modelling/selection":["tool.select","nope"]}}"#,
        )
        .unwrap();
        assert_eq!(p.workspace().id, "modelling");
        assert_eq!(p.tab().id, "build");
        let w = p.workspace();
        assert_eq!(p.pinned(&key(w, None, &w.fixed), &w.fixed), ["tool.select"]);
        assert!(p.labels);
    }

    fn groups() -> Vec<GroupSize> {
        vec![
            GroupSize { buttons: vec![(60.0, 50.0); 2], label: 40.0 },
            GroupSize { buttons: vec![(70.0, 50.0); 3], label: 40.0 },
            GroupSize { buttons: vec![(60.0, 50.0); 3], label: 40.0 },
            GroupSize { buttons: vec![], label: 40.0 },
        ]
    }

    fn total(plans: &[Plan], g: &[GroupSize]) -> f32 {
        let more = if plans.iter().any(|p| p.fit == Fit::More) { MORE_ROOM } else { 0.0 };
        used(plans, g, 50.0) + more
    }

    #[test]
    fn everything_shows_when_there_is_room() {
        let g = groups();
        let plans = fit(&g, 2000.0, 50.0);
        assert!(plans[..3].iter().all(|p| p.fit == Fit::Buttons));
        assert_eq!(plans[1].widths, [70.0; 3]);
        // A group without pins is one button opening its menu.
        assert_eq!(plans[3].fit, Fit::Collapsed);
    }

    #[test]
    fn buttons_narrow_before_any_is_hidden() {
        let g = groups();
        let full = total(&fit(&g, 2000.0, 50.0), &g);
        let plans = fit(&g, full - 20.0, 50.0);
        assert!(plans.iter().zip(&g).all(|(p, g)| p.shown == g.buttons.len()));
        assert!(plans[1].widths[0] < 70.0);
        assert!(total(&plans, &g) <= full - 20.0);
    }

    #[test]
    fn groups_give_way_one_button_at_a_time_then_go_into_the_more_menu() {
        let g = groups();
        // (groups in the » menu, buttons shown) as the room shrinks.
        let mut last = (0, usize::MAX);
        let mut room = 2000.0;
        while room > 0.0 {
            let plans = fit(&g, room, 50.0);
            let more = plans.iter().filter(|p| p.fit == Fit::More).count();
            let on: usize = plans.iter().map(|p| if p.fit == Fit::Buttons { p.shown } else { 0 }).sum();
            assert!(more >= last.0, "less room never brings a group back from the » menu");
            // A group going into the » menu frees room the others may take back.
            assert!(more > last.0 || on <= last.1, "less room never shows more buttons");
            last = (more, on);
            if total(&plans, &g) > room {
                // Only when even the » menu alone does not fit.
                assert!(plans.iter().all(|p| p.fit == Fit::More));
            }
            assert_eq!(plans, fit(&g, room, 50.0), "the same room, the same plans");
            room -= 7.0;
        }
        let plans = fit(&g, 300.0, 50.0);
        assert!(plans.iter().any(|p| p.fit == Fit::Collapsed));
        let plans = fit(&g, 120.0, 50.0);
        assert_eq!(plans.last().unwrap().fit, Fit::More, "the last groups go first");
        assert_ne!(plans[0].fit, Fit::More);
    }
}

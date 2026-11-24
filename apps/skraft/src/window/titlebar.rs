//! The title bar (docs/ui/title-bar.md): one band at the top of the window with
//! the app's mark and name, the File menu, undo and redo, the file tabs (docs/ui/file-tabs.md,
//! [`crate::window::tabs`]), the command search and the preferences, every button a command of the
//! registry. The other menus are not in the bar: their commands are in the ribbon, the view bar
//! ([`crate::window::view_bar`]), the command search and the shortcuts.
//!
//! On the macOS desktop this band stands in for the system's title bar, which
//! [`native_options`](crate::native_options) hides while letting the content reach the window's
//! top. The band keeps clear of the three window buttons; dragging a free part of it carries the
//! window and double-clicking there toggles its maximized size. The system menu bar
//! ([`crate::window::native_menu`]) holds the File menu only. The web, Windows and Linux keep the OS frame.

use skraft_i18n::{ellipsis, t};

/// The app's name, as the bar shows it next to its mark. Not translated.
const APP_NAME: &str = "Skraft";
/// The app's version (the workspace's, Cargo.toml), shown after its name.
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

use crate::commands::Menu;
use crate::kit::icons;
use crate::kit::style::{BAR_BUTTON, BAR_ICON, Tip};
use crate::kit::widgets::{IconButton, Look, VEIL_RADIUS, backed, command_item, is_toggle, unframed, veil};
use crate::panels::{Action, CommandView};
use crate::window::tabs::{self, Tab};

/// The bar's height, in points.
pub const HEIGHT: f32 = 34.0;
/// Width left clear for the three macOS window buttons ahead of the app's mark, margin included.
const TRAFFIC_LIGHTS: f32 = 84.0;
/// The command search's widest.
const SEARCH_WIDTH: f32 = 160.0;
/// Below this width, the search is only its icon.
const SEARCH_MIN: f32 = 120.0;

/// What the bar shows besides the commands.
pub struct Bar<'a> {
    /// The file tabs, and the one shown (none: the start page).
    pub tabs: &'a [Tab],
    pub shown: Option<usize>,
    /// A tab's menu offers to show its file (desktop).
    pub reveal: bool,
    /// Set on the macOS desktop, where the bar replaces the title bar: it clears the window
    /// buttons and its free areas drag the window.
    pub integrated: bool,
    /// The 3D view's background under the bar and the text drawn on it (none: the start
    /// page): the shown tab opens onto it.
    pub ground: Option<(egui::Color32, egui::Color32)>,
    /// The control channel, while it is on (desktop).
    pub remote: Option<RemoteMark>,
    /// The newer version to announce (docs/ui/updates.md; desktop).
    pub update: Option<String>,
    /// This app, when a dev build (desktop): its badge after the name says which build it is,
    /// several running at once.
    pub instance: Option<&'a skraft_control::instances::Instance>,
    /// The name of the Claude Code session that launched it, if it has one: named after a
    /// colour, it colours the badge.
    pub session: Option<String>,
}

/// The control channel's mark (docs/ui/commands.md): green while it runs,
/// flashing at each request; in red when it could not start. Its menu: the address, copy a
/// test command, stop (or try again).
pub struct RemoteMark {
    pub port: u16,
    /// The requests served since it started.
    pub served: u64,
    /// 1 just after a request, down to 0.
    pub flash: f32,
    /// Why it is not running, though asked for.
    pub error: Option<String>,
    /// Its token file (docs/control-protocol.md), which the test command reads.
    pub token_file: Option<std::path::PathBuf>,
}

/// The running channel's mark: a green readable on the bar's dark ground, whatever the palette.
pub const ACTIVE: egui::Color32 = egui::Color32::from_rgb(88, 204, 112);
/// The badge of a newer version: magenta, its text white.
const UPDATE: egui::Color32 = egui::Color32::from_rgb(0xe8, 0x4f, 0xa9);
const UPDATE_HOVERED: egui::Color32 = egui::Color32::from_rgb(0xf0, 0x6e, 0xbb);
/// The failed channel's mark: a red readable on the bar's dark ground.
const ALERT: egui::Color32 = egui::Color32::from_rgb(240, 96, 88);

/// The bar's panel, at the top of the window.
pub fn show(ui: &mut egui::Ui, bar: &Bar, views: &[CommandView], actions: &mut Vec<Action>) {
    let mut style = ui.style().as_ref().clone();
    crate::kit::style::title_bar(ui.ctx(), &mut style);
    let frame = egui::Frame::side_top_panel(&style).inner_margin(egui::Margin::symmetric(8, 0));
    // The line along the bar's bottom edge is drawn by the bar, under the shown tab, which
    // opens through it onto the view.
    let panel = egui::Panel::top("title_bar").exact_size(HEIGHT).resizable(false).show_separator_line(false).frame(frame);
    panel.show(ui, |ui| {
        ui.set_style(style);
        if bar.integrated {
            move_window(ui);
        }
        let rect = ui.max_rect().expand2(egui::vec2(8.0, 0.0));
        let line = ui.visuals().widgets.noninteractive.bg_stroke;
        ui.painter().hline(rect.x_range(), rect.bottom() - line.width / 2.0, line);
        let ground = bar.ground.map(|(fill, ink)| tabs::Ground { bottom: rect.bottom(), fill, ink });
        ui.horizontal_centered(|ui| contents(ui, bar, ground, views, actions));
    });
}

/// Gives the bar's free areas their window behaviour: a drag carries the window, a double-click
/// toggles maximized. Set up ahead of the bar's widgets, so that those keep the pointer wherever
/// they sit.
fn move_window(ui: &egui::Ui) {
    let r = ui.interact(ui.max_rect(), egui::Id::unique("title_bar_drag"), egui::Sense::click_and_drag());
    if r.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if r.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

/// Room kept for the tabs before the search field narrows.
const TABS_MIN: f32 = 160.0;

fn contents(ui: &mut egui::Ui, bar: &Bar, ground: Option<tabs::Ground>, views: &[CommandView], actions: &mut Vec<Action>) {
    let find = |id: &str| views.iter().find(|v| v.id == id);
    // In full screen the traffic lights are gone.
    if bar.integrated && !ui.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        ui.add_space(TRAFFIC_LIGHTS - 8.0);
    }
    brand(ui, find("view.home"), actions);
    if let Some(i) = bar.instance {
        instance_badge(ui, i, bar.session.as_deref());
    }
    if let Some(version) = &bar.update {
        update_badge(ui, version, actions);
    }
    ui.separator();
    file_menu(ui, views, actions);
    for id in ["edit.undo", "edit.redo"] {
        if let Some(v) = find(id) {
            command_button(ui, id_icon(id), v, v.checked == Some(true), &v.tip(), actions);
        }
    }
    ui.separator();
    // The right end first (right to left): the preferences, the search; the tabs take what is
    // left between.
    let row = ui.available_rect_before_wrap();
    let right = ui
        .scope_builder(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            if let Some(v) = find("app.preferences") {
                command_button(ui, icons::SETTINGS, v, false, &v.tip(), actions);
            }
            if let Some(mark) = &bar.remote {
                remote_mark(ui, mark, actions);
            }
            if let Some(v) = find("app.command_palette") {
                ui.separator();
                let room = ui.available_width() - TABS_MIN;
                if room >= SEARCH_MIN {
                    if search(ui, v, room.min(SEARCH_WIDTH)) {
                        actions.push(Action::Command(v.id));
                    }
                } else {
                    command_button(ui, icons::SEARCH, v, false, &v.tip(), actions);
                }
            }
            ui.separator();
            ui.min_rect()
        })
        .inner;
    let room = egui::Rect::from_x_y_ranges(row.left()..=(right.left() - 4.0).max(row.left()), row.y_range());
    let window_moves = bar.integrated;
    for a in tabs::show(ui, room, bar.tabs, bar.shown, ground, window_moves, bar.reveal) {
        actions.push(Action::Tab(a));
    }
}

/// The badge of a newer version, after the app's own: opens its window.
fn update_badge(ui: &mut egui::Ui, version: &str, actions: &mut Vec<Action>) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(t!("update.mark", version = version), font, egui::Color32::WHITE);
    let pad = egui::vec2(6.0, 2.0);
    let (rect, r) = ui.allocate_exact_size(galley.size() + 2.0 * pad, egui::Sense::click());
    let fill = if r.hovered() { UPDATE_HOVERED } else { UPDATE };
    ui.painter().rect_filled(rect, 4.0, fill);
    ui.painter().galley(rect.min + pad, galley, egui::Color32::PLACEHOLDER);
    crate::kit::handles::publish(|| "titlebar:app.updates".to_owned(), r.rect);
    if r.tip(t!("update.toast", version = version)).clicked() {
        actions.push(Action::Command("app.updates"));
    }
}

/// The icon of a bar button.
fn id_icon(id: &str) -> egui::ImageSource<'static> {
    match id {
        "edit.undo" => icons::UNDO,
        _ => icons::REDO,
    }
}

/// A bar button: smaller than the tool bar's.
fn bar_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, lit: bool, enabled: bool, tip: &str) -> egui::Response {
    IconButton::new(icon, BAR_BUTTON, BAR_ICON).look(Look::Veil).lit(lit).enabled(enabled).show(ui).tip(tip)
}

/// A menu's button: lit while its menu is open.
fn menu_button(ui: &mut egui::Ui, handle: &str, button: egui::Button, tip: Option<String>, content: impl FnOnce(&mut egui::Ui)) {
    let r = backed(ui, menu_open, |ui| {
        egui::containers::menu::MenuButton::from_button(unframed(button))
            .config(egui::containers::menu::MenuConfig::new().style(crate::kit::style::menu))
            .ui(ui, |ui| content(ui))
            .0
    });
    crate::kit::handles::publish(|| handle.to_owned(), r.rect);
    if let Some(tip) = tip {
        r.tip(tip);
    }
}

/// The menu of the button `r` is open.
fn menu_open(r: &egui::Response) -> bool {
    egui::Popup::is_id_open(&r.ctx, egui::Popup::default_response_id(r))
}

fn command_button(ui: &mut egui::Ui, icon: egui::ImageSource<'static>, v: &CommandView, lit: bool, tip: &str, actions: &mut Vec<Action>) {
    let r = bar_button(ui, icon, lit, v.enabled, tip);
    crate::kit::handles::publish_enabled(|| format!("titlebar:{}", v.id), r.rect, v.enabled);
    if r.clicked() {
        actions.push(Action::Command(v.id));
    }
}

/// The app's mark, name and version: a button to the start page while a document is shown
/// (`view.home`); on the start page not a control, its place moving the window like the bar's
/// empty space.
fn brand(ui: &mut egui::Ui, home: Option<&CommandView>, actions: &mut Vec<Action>) {
    // The icon's tile fills 824 of its 1024 units: drawn larger so the tile is MARK wide.
    const MARK: f32 = 18.0;
    const TILE: f32 = 824.0 / 1024.0;
    // Between the name and the version (the mark and the name: the items' spacing).
    const GAP: f32 = 6.0;
    // Around them, the veil's margin.
    const PAD: egui::Vec2 = egui::vec2(4.0, 4.0);
    // The name in the headings' semibold at the text's size, then the version, smaller and
    // weak, on the name's baseline.
    let mut font = egui::TextStyle::Heading.resolve(ui.style());
    font.size = egui::TextStyle::Body.resolve(ui.style()).size;
    let small = egui::TextStyle::Small.resolve(ui.style());
    let name = ui.painter().layout_no_wrap(APP_NAME.to_owned(), font, ui.visuals().strong_text_color());
    let version = ui.painter().layout_no_wrap(APP_VERSION.to_owned(), small, ui.visuals().weak_text_color());
    let text = egui::vec2(name.size().x + GAP + version.size().x, name.size().y);
    let spacing = ui.spacing().item_spacing.x;
    let size = egui::vec2(MARK + spacing + text.x, MARK.max(text.y)) + 2.0 * PAD;
    let home = home.filter(|v| v.enabled);
    let sense = if home.is_some() { egui::Sense::click() } else { egui::Sense::hover() };
    // Drawn back by PAD: the mark stays where it was without the veil.
    ui.add_space(-PAD.x);
    let behind = ui.painter().add(egui::Shape::Noop);
    let (rect, r) = ui.allocate_exact_size(size, sense);
    ui.add_space(-PAD.x);
    let inner = rect.shrink2(PAD);
    let mark = egui::Rect::from_min_size(egui::pos2(inner.left(), inner.center().y - MARK / 2.0), egui::Vec2::splat(MARK));
    egui::Image::new(icons::APP).paint_at(ui, egui::Rect::from_center_size(mark.center(), egui::Vec2::splat(MARK / TILE)));
    let top = inner.center().y - text.y / 2.0;
    let lift = baseline(&version) - baseline(&name);
    let right = mark.right() + spacing + text.x;
    ui.painter().galley(egui::pos2(mark.right() + spacing, top), name, egui::Color32::PLACEHOLDER);
    ui.painter().galley(egui::pos2(right - version.size().x, top - lift), version, egui::Color32::PLACEHOLDER);
    let Some(v) = home else { return };
    if let Some(fill) = veil(ui, &r) {
        ui.painter().set(behind, egui::Shape::rect_filled(rect, VEIL_RADIUS, fill));
    }
    crate::kit::handles::publish(|| format!("titlebar:{}", v.id), rect);
    if r.tip(v.tip()).clicked() {
        actions.push(Action::Command(v.id));
    }
}

/// A dev build's badge: its worktree (its branch for the main checkout) and commit, `*` when
/// changes not committed went in, on a colour of the worktree's own — or the one its Claude
/// Code session is named after; the hover says all, who launched it among it.
fn instance_badge(ui: &mut egui::Ui, i: &skraft_control::instances::Instance, session: Option<&str>) {
    let Some(b) = &i.build else { return };
    let label = b.label();
    let text = format!("{label} · {}{}", b.commit, if b.dirty { "*" } else { "" });
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text, font, egui::Color32::from_gray(24));
    let pad = egui::vec2(6.0, 2.0);
    let (rect, r) = ui.allocate_exact_size(galley.size() + 2.0 * pad, egui::Sense::hover());
    ui.painter().rect_filled(rect, 4.0, crate::identity::color(&label, session));
    ui.painter().galley(rect.min + pad, galley, egui::Color32::PLACEHOLDER);
    r.tip_ui(|ui| {
        let now = skraft_control::instances::now();
        ui.strong(t!("titlebar.instance.title", label = label));
        ui.label(t!("titlebar.instance.worktree", path = b.worktree));
        ui.label(t!("titlebar.instance.commit", branch = b.branch, commit = b.commit));
        if b.dirty {
            ui.label(t!("titlebar.instance.dirty"));
        }
        ui.label(t!("titlebar.instance.built", ago = ago(now.saturating_sub(b.built))));
        ui.label(t!("titlebar.instance.started", ago = ago(now.saturating_sub(i.started)), pid = i.pid));
        match (&i.claude_session, &i.launcher) {
            (_, Some(who)) => ui.label(t!("titlebar.instance.launcher", who = who)),
            (Some(s), None) => {
                let id = s.chars().take(8).collect::<String>();
                match session {
                    Some(name) => ui.label(t!("titlebar.instance.session_named", name = name, id = id)),
                    None => ui.label(t!("titlebar.instance.session", id = id)),
                }
            }
            (None, None) => ui.label(t!("titlebar.instance.by_hand")),
        };
        if let Some(port) = i.port {
            ui.label(t!("titlebar.instance.port", port = port));
        }
    });
}

/// A duration, `seconds` long, as the hover says it: in the largest unit that fits.
fn ago(seconds: u64) -> String {
    match seconds {
        s if s < 60 => t!("titlebar.instance.seconds", n = s),
        s if s < 3600 => t!("titlebar.instance.minutes", n = s / 60),
        s if s < 86_400 => t!("titlebar.instance.hours", n = s / 3600),
        s => t!("titlebar.instance.days", n = s / 86_400),
    }
}

/// The baseline of a galley's first row, from its top.
fn baseline(galley: &egui::Galley) -> f32 {
    galley.rows.first().and_then(|r| r.glyphs.first().map(|g| r.pos.y + g.pos.y)).unwrap_or(galley.size().y)
}

/// The File menu: a text button opening the registry's File commands.
fn file_menu(ui: &mut egui::Ui, views: &[CommandView], actions: &mut Vec<Action>) {
    /// The button's side margins, wider than the buttons' padding.
    const PADDING_X: f32 = 10.0;
    let button = egui::Button::new(Menu::File.label()).min_size(egui::vec2(0.0, 26.0));
    ui.scope(|ui| {
        ui.spacing_mut().button_padding.x = PADDING_X;
        // The menu opens in the window's style: its items keep their padding.
        menu_button(ui, "titlebar:file", button, None, |ui| menu_items(ui, views.iter().filter(|v| v.menu == Menu::File), actions));
    });
}

/// A menu's items, a separator before each group.
fn menu_items<'a>(ui: &mut egui::Ui, items: impl Iterator<Item = &'a CommandView>, actions: &mut Vec<Action>) {
    let items: Vec<&CommandView> = items.collect();
    // A check box among them: every label after their column.
    let column = items.iter().any(|v| is_toggle(v));
    let mut first = true;
    for v in items {
        if v.group && !first {
            ui.separator();
        }
        first = false;
        if command_item(ui, v, column) {
            actions.push(Action::Command(v.id));
        }
    }
}

/// The command search: its icon, a hint and its shortcut, opens the command palette. Drawn as
/// the bar's buttons: no background at rest, the [`veil`] when hovered. True when
/// clicked.
fn search(ui: &mut egui::Ui, v: &CommandView, width: f32) -> bool {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click());
    crate::kit::handles::publish(|| format!("titlebar:{}", v.id), rect);
    if let Some(fill) = veil(ui, &r) {
        ui.painter().rect_filled(rect, VEIL_RADIUS, fill);
    }
    let visuals = ui.visuals();
    let p = ui.painter();
    let weak = visuals.weak_text_color();
    let icon = egui::Rect::from_center_size(rect.left_center() + egui::vec2(14.0, 0.0), egui::vec2(14.0, 14.0));
    egui::Image::new(icons::SEARCH).tint(weak).paint_at(ui, icon);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut right = rect.right() - 8.0;
    if let Some(s) = &v.shortcut {
        let g = p.layout_no_wrap(s.clone(), font.clone(), weak);
        right -= g.size().x;
        p.galley(egui::pos2(right, rect.center().y - g.size().y / 2.0), g, weak);
        right -= 8.0;
    }
    let left = icon.right() + 6.0;
    let hint = egui::WidgetText::from(ellipsis(t!("titlebar.search"))).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (right - left).max(0.0),
        font,
    );
    ui.painter().galley(egui::pos2(left, rect.center().y - hint.size().y / 2.0), hint, weak);
    r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The control channel's mark and its menu.
fn remote_mark(ui: &mut egui::Ui, mark: &RemoteMark, actions: &mut Vec<Action>) {
    let ink = if mark.error.is_some() { ALERT } else { ACTIVE };
    let img = egui::Image::new(icons::REMOTE).fit_to_exact_size(egui::vec2(18.0, 18.0)).tint(ink);
    let address = format!("127.0.0.1:{}", mark.port);
    let tip = match &mark.error {
        Some(e) => t!("titlebar.remote.failed", error = e),
        None => t!("titlebar.remote.on", address = address, n = mark.served),
    };
    // The flash: its green behind the mark, fading.
    let behind = ui.painter().add(egui::Shape::Noop);
    let button = egui::Button::image(img).min_size(egui::vec2(26.0, 26.0));
    let r = backed(ui, menu_open, |ui| {
        egui::containers::menu::MenuButton::from_button(unframed(button))
            .config(egui::containers::menu::MenuConfig::new().style(crate::kit::style::menu))
            .ui(ui, |ui| {
                ui.weak(&tip);
                ui.separator();
                if mark.error.is_some() {
                    if ui.button(t!("titlebar.remote.retry")).clicked() {
                        actions.push(Action::RemoteRetry);
                    }
                } else if ui.button(t!("titlebar.remote.copy_ping")).clicked() {
                    let token = mark
                        .token_file
                        .as_ref()
                        .map_or_else(|| "$SKRAFT_CONTROL_TOKEN".to_owned(), |f| format!("$(cat '{}')", f.display()));
                    ui.ctx().copy_text(format!(
                        "curl -s -H 'X-Skraft: 1' -H \"Authorization: Bearer {token}\" -d '{{\"method\": \"app.ping\"}}' http://{address}/call"
                    ));
                }
                if ui.button(t!("titlebar.remote.journal")).clicked() {
                    actions.push(Action::Command("app.journal"));
                }
                if ui.button(t!("titlebar.remote.stop")).clicked() {
                    actions.push(Action::Command("app.remote_control"));
                }
            })
            .0
    });
    if mark.flash > 0.0 {
        let fill = ACTIVE.gamma_multiply(0.45 * mark.flash);
        ui.painter().set(behind, egui::Shape::rect_filled(r.rect, VEIL_RADIUS, fill));
    }
    if !menu_open(&r) {
        r.tip(tip);
    }
}

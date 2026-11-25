//! The start page (docs/ui/file-tabs.md#start-page), shown when no document is: the app's
//! mark, its name and what it is, the buttons starting a document, the recent files, over a
//! drafting grid. Its colours are the theme's.

use skraft_i18n::t;

use crate::commands::{self, Action};
use crate::docs::file_name;
use crate::kit::icons;
use crate::kit::style::Tip;
use crate::kit::widgets::LabelButton;

/// The app's mark, its tile as wide (as in the title bar): the icon's tile fills 824 of its
/// 1024 units, the icon is drawn larger.
const MARK: f32 = 72.0;
const MARK_TILE: f32 = 824.0 / 1024.0;
const TITLE_SIZE: f32 = 30.0;
const TAGLINE_SIZE: f32 = 15.0;
const BUTTON_HEIGHT: f32 = 40.0;
const BUTTON_GAP: f32 = 10.0;
/// The room kept on each side when the buttons do not fit on one line and stack (a narrow
/// window, a phone), and the most a stacked button spans.
const STACK_MARGIN: f32 = 16.0;
const STACK_WIDTH: f32 = 320.0;
const RECENT_WIDTH: f32 = 260.0;

/// The drafting grid: a line every `GRID_STEP` points, every `GRID_MAJOR`th stronger, in the
/// accent's colour; it fades out from a little above the page's middle towards its edges
/// (`GRID_FADE`: the fade's start and end, in the ellipse's radii, these `GRID_RADII` of the
/// page).
const GRID_STEP: f32 = 24.0;
const GRID_MAJOR: i32 = 5;
const GRID_ALPHA: (f32, f32) = (0.06, 0.12);
const GRID_FADE: (f32, f32) = (0.3, 1.0);
const GRID_RADII: egui::Vec2 = egui::vec2(0.6, 0.7);
const GRID_MIDDLE: f32 = 0.42;

/// The page in `ui`, `recent` the recent files (the web keeps none).
pub(crate) fn show(ui: &mut egui::Ui, recent: &[String], actions: &mut Vec<Action>) {
    grid(ui, ui.max_rect());
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.18).max(24.0));
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(MARK), egui::Sense::hover());
        egui::Image::new(icons::APP).paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(MARK / MARK_TILE)));
        ui.add_space(14.0);
        ui.label(egui::RichText::new(t!("start.title")).font(crate::kit::style::semibold(TITLE_SIZE)));
        ui.add_space(2.0);
        ui.label(egui::RichText::new(t!("start.tagline")).size(TAGLINE_SIZE).weak());
        ui.add_space(28.0);
        buttons(ui, actions);
        if cfg!(target_arch = "wasm32") {
            return;
        }
        ui.add_space(40.0);
        ui.strong(t!("start.recent"));
        ui.add_space(4.0);
        if recent.is_empty() {
            ui.weak(t!("start.no_recent"));
        }
        for (k, path) in recent.iter().enumerate().take(commands::RECENT_FILES) {
            let id = commands::COMMANDS.iter().find(|c| commands::recent_slot(c.id) == Some(k)).map(|c| c.id);
            let r = ui.add_sized([RECENT_WIDTH, 24.0], egui::Button::new(file_name(path)).frame_when_inactive(false)).tip(path);
            if let (true, Some(id)) = (r.clicked(), id) {
                actions.push(Action::Command(id));
            }
        }
    });
}

/// New, Open and Import, the first standing out: on one line, or stacked when the line does
/// not fit.
fn buttons(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    let buttons = [
        ("file.new", LabelButton::new(ui, icons::ADD, t!("start.new"), true)),
        ("file.open", LabelButton::new(ui, icons::OPEN, t!("start.open"), false)),
        ("file.import", LabelButton::new(ui, icons::IMPORT, t!("start.import"), false)),
    ];
    let gaps = BUTTON_GAP * (buttons.len() - 1) as f32;
    let line = buttons.iter().map(|(_, b)| b.width()).sum::<f32>() + gaps;
    let room = ui.available_width() - 2.0 * STACK_MARGIN;
    let stacked = line > room;
    let size = if stacked {
        egui::vec2(room.min(STACK_WIDTH), BUTTON_HEIGHT * buttons.len() as f32 + gaps)
    } else {
        egui::vec2(line, BUTTON_HEIGHT)
    };
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let mut at = area.min;
    for (id, button) in buttons {
        let width = if stacked { area.width() } else { button.width() };
        let rect = egui::Rect::from_min_size(at, egui::vec2(width, BUTTON_HEIGHT));
        if button.show_at(ui, rect, &format!("start:{id}")).clicked() {
            actions.push(Action::Command(id));
        }
        if stacked {
            at.y += BUTTON_HEIGHT + BUTTON_GAP;
        } else {
            at.x += width + BUTTON_GAP;
        }
    }
}

/// The drafting grid over `rect`, as one mesh: each line cut at the grid's steps, its pieces'
/// ends taking the fade's alpha.
fn grid(ui: &egui::Ui, rect: egui::Rect) {
    let accent = ui.visuals().selection.stroke.color;
    let middle = egui::pos2(rect.center().x, rect.top() + rect.height() * GRID_MIDDLE);
    let radii = rect.size() * GRID_RADII;
    let fade = |p: egui::Pos2| {
        let d = ((p - middle) / radii).length();
        1.0 - ((d - GRID_FADE.0) / (GRID_FADE.1 - GRID_FADE.0)).clamp(0.0, 1.0)
    };
    // The lines through the page's centre, every step either side.
    let origin = rect.center().round();
    let span = |from: f32, to: f32, at: f32| ((from - at) / GRID_STEP).ceil() as i32..=((to - at) / GRID_STEP).floor() as i32;
    let mut mesh = egui::Mesh::default();
    for vertical in [true, false] {
        let (lines, along) = if vertical {
            (span(rect.left(), rect.right(), origin.x), rect.y_range())
        } else {
            (span(rect.top(), rect.bottom(), origin.y), rect.x_range())
        };
        for i in lines {
            let alpha = if i % GRID_MAJOR == 0 { GRID_ALPHA.1 } else { GRID_ALPHA.0 };
            let across = if vertical { origin.x } else { origin.y } + i as f32 * GRID_STEP;
            let point = |t: f32| if vertical { egui::pos2(across, t) } else { egui::pos2(t, across) };
            let mut t = along.min;
            while t < along.max {
                let next = (t + GRID_STEP).min(along.max);
                let (a, b) = (point(t), point(next));
                let (fa, fb) = (fade(a), fade(b));
                if fa > 0.0 || fb > 0.0 {
                    let thick = if vertical { egui::vec2(1.0, 0.0) } else { egui::vec2(0.0, 1.0) };
                    let base = mesh.vertices.len() as u32;
                    for (p, f) in [(a, fa), (b, fb), (b + thick, fb), (a + thick, fa)] {
                        mesh.colored_vertex(p, accent.gamma_multiply(alpha * f));
                    }
                    mesh.add_triangle(base, base + 1, base + 2);
                    mesh.add_triangle(base, base + 2, base + 3);
                }
                t = next;
            }
        }
    }
    ui.painter().add(mesh);
}

//! The start page (docs/ui/file-tabs.md#start-page), shown when no document is: the app's
//! mark, its name and what it is, the buttons starting a document, the recent files, over a
//! drafting grid. Its colours are the theme's.

use std::collections::HashMap;

use skraft_i18n::t;
use skraft_theme::Theme;

use crate::commands::{self, Action};
use crate::docs::file_name;
use crate::kit::icons;
use crate::kit::style::Tip;
use crate::kit::widgets::LabelButton;
#[cfg(not(target_arch = "wasm32"))]
use crate::system::images::{Decoder, Size};

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

/// The recent files' cards: at most this wide all together, four on a line (two when that
/// would make them narrower than `CARD_MIN`), their picture's height this share of their
/// width (the pictures' proportions), their text under it this high.
const CARDS_WIDTH: f32 = 920.0;
const CARD_MIN: f32 = 150.0;
const CARD_GAP: f32 = 16.0;
const CARD_PICTURE: f32 = 124.0 / 220.0;
const CARD_TEXT: f32 = 52.0;
const CARD_RADIUS: u8 = 8;
/// The pictures are decoded this large (the files' are 480 × 270).
#[cfg(not(target_arch = "wasm32"))]
const PICTURE_SIDE: u32 = 480;

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

/// The pictures of the recent files' models (docs/ui/file-tabs.md#start-page), read from their
/// files (docs/io/file-format.md) and decoded off the interface's thread; read again when a
/// file changes.
#[derive(Default)]
pub(crate) struct Pictures {
    seen: HashMap<String, Seen>,
    #[cfg(not(target_arch = "wasm32"))]
    decoder: Decoder,
}

/// A recent file as last looked at: when it changed (none: it could not be read), its
/// picture once decoded.
struct Seen {
    modified: Option<std::time::SystemTime>,
    texture: Option<egui::TextureHandle>,
}

impl Pictures {
    /// The files `recent` looked at again: a picture asked for each one changed since.
    #[cfg(not(target_arch = "wasm32"))]
    fn refresh(&mut self, ctx: &egui::Context, recent: &[String]) {
        for (key, image) in self.decoder.take(ctx) {
            if let (Some(seen), Some(image)) = (self.seen.get_mut(&key), image) {
                let color = egui::ColorImage::from_rgba_unmultiplied([image.size[0] as usize, image.size[1] as usize], &image.rgba);
                seen.texture = Some(ctx.load_texture(format!("start:{key}"), color, egui::TextureOptions::LINEAR));
            }
        }
        self.seen.retain(|path, _| recent.contains(path));
        for path in recent {
            let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            if self.seen.get(path).is_some_and(|s| s.modified == modified) {
                continue;
            }
            let picture = std::fs::File::open(path).ok().and_then(|f| skraft_model::Document::file_thumbnail(std::io::BufReader::new(f)));
            let texture = self.seen.remove(path).and_then(|s| s.texture).filter(|_| picture.is_some());
            if let Some(png) = picture {
                self.decoder.ask(ctx, path.clone(), png.into(), Size::Fit(PICTURE_SIDE));
            }
            self.seen.insert(path.clone(), Seen { modified, texture });
        }
    }
}

/// The page in `ui`, `recent` the recent files (the web keeps none), their models' pictures in
/// `pictures`.
pub(crate) fn show(ui: &mut egui::Ui, theme: &Theme, recent: &[String], pictures: &mut Pictures, actions: &mut Vec<Action>) {
    grid(ui, ui.max_rect());
    let recent = &recent[..recent.len().min(commands::RECENT_FILES)];
    #[cfg(not(target_arch = "wasm32"))]
    pictures.refresh(ui.ctx(), recent);
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
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
            ui.add_space(48.0);
            cards(ui, theme, recent, pictures, actions);
            ui.add_space(24.0);
        })
    });
}

/// The recent files as cards: the picture of each one's model on the view's ground (the app's
/// mark, faint, for a file without one), its name and when it last changed; a click opens it.
fn cards(ui: &mut egui::Ui, theme: &Theme, recent: &[String], pictures: &Pictures, actions: &mut Vec<Action>) {
    let width = (ui.available_width() - 2.0 * STACK_MARGIN).min(CARDS_WIDTH);
    let mut columns = 4;
    if (width - CARD_GAP * 3.0) / 4.0 < CARD_MIN {
        columns = 2;
    }
    let card_w = (width - CARD_GAP * (columns - 1) as f32) / columns as f32;
    let picture_h = (card_w * CARD_PICTURE).round();
    let card_h = picture_h + CARD_TEXT;
    let rows = recent.len().div_ceil(columns).max(1);
    let heading = 26.0;
    let height = heading + rows as f32 * card_h + (rows - 1) as f32 * CARD_GAP;
    let (area, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let v = ui.visuals().clone();
    let weak = v.weak_text_color();
    let painter = ui.painter();
    let title = painter.layout_no_wrap(t!("start.recent").to_uppercase(), crate::kit::style::semibold(12.0), weak);
    painter.galley(area.min, title, weak);
    if recent.is_empty() {
        let none = painter.layout_no_wrap(t!("start.no_recent"), egui::FontId::proportional(13.0), weak);
        painter.galley(area.min + egui::vec2(0.0, heading), none, weak);
        return;
    }
    let [r, g, b, _] = theme.ui.view;
    let view = egui::Color32::from(egui::Rgba::from_rgb(r, g, b));
    let today = days(std::time::SystemTime::now());
    for (k, path) in recent.iter().enumerate() {
        let (row, column) = (k / columns, k % columns);
        let min = area.min + egui::vec2(column as f32 * (card_w + CARD_GAP), heading + row as f32 * (card_h + CARD_GAP));
        let rect = egui::Rect::from_min_size(min, egui::vec2(card_w, card_h));
        let handle = format!("start:recent:{k}");
        let r = ui.interact(rect, ui.scope_id().with(&handle), egui::Sense::click());
        crate::kit::handles::publish(|| handle.clone(), rect);
        let name = file_name(path);
        let name = name.strip_suffix(&format!(".{}", crate::system::file::EXTENSION)).unwrap_or(name);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        let lit = r.hovered() || r.has_focus() || r.is_pointer_button_down_on();
        let painter = ui.painter();
        if lit {
            painter.rect_stroke(rect.expand(1.0), CARD_RADIUS + 1, egui::Stroke::new(2.0, v.selection.bg_fill), egui::StrokeKind::Outside);
        }
        painter.rect_filled(rect, CARD_RADIUS, v.extreme_bg_color);
        let picture = egui::Rect::from_min_size(rect.min, egui::vec2(card_w, picture_h));
        let top = egui::CornerRadius { nw: CARD_RADIUS, ne: CARD_RADIUS, sw: 0, se: 0 };
        painter.rect_filled(picture, top, view);
        let border = v.widgets.noninteractive.bg_stroke;
        painter.hline(picture.x_range(), picture.bottom(), border);
        let seen = pictures.seen.get(path);
        match seen.and_then(|s| s.texture.as_ref()) {
            Some(texture) => {
                let size = texture.size_vec2();
                let fit = (picture.width() / size.x).min(picture.height() / size.y);
                let shown = egui::Rect::from_center_size(picture.center(), size * fit);
                let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
                painter.image(texture.id(), shown, uv, egui::Color32::WHITE);
            }
            None => {
                let mark = egui::Rect::from_center_size(picture.center(), egui::Vec2::splat(36.0 / MARK_TILE));
                egui::Image::new(icons::APP).tint(egui::Color32::from_white_alpha(70)).paint_at(ui, mark);
            }
        }
        let stroke = if lit { egui::Stroke::new(1.0, v.selection.stroke.color) } else { border };
        let painter = ui.painter();
        painter.rect_stroke(rect, CARD_RADIUS, stroke, egui::StrokeKind::Inside);
        let text_w = card_w - 24.0;
        let mut job = egui::text::LayoutJob::single_section(
            name.to_owned(),
            egui::TextFormat::simple(crate::kit::style::semibold(13.0), v.text_color()),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
        let name = painter.layout_job(job);
        let text_top = picture.bottom() + 9.0;
        painter.galley(egui::pos2(rect.left() + 12.0, text_top), name, v.text_color());
        let when = match seen.and_then(|s| s.modified) {
            Some(t) => match today - days(t) {
                0 => t!("start.today"),
                1 => t!("start.yesterday"),
                _ => {
                    let (y, m, d) = crate::system::dates::civil_from_days(days(t));
                    format!("{d:02}/{m:02}/{y}")
                }
            },
            None => t!("start.missing"),
        };
        let when = painter.layout_no_wrap(when, egui::FontId::proportional(12.0), weak);
        painter.galley(egui::pos2(rect.left() + 12.0, text_top + 20.0), when, weak);
        let id = commands::COMMANDS.iter().find(|c| commands::recent_slot(c.id) == Some(k)).map(|c| c.id);
        if let (true, Some(id)) = (r.tip(path).clicked(), id) {
            actions.push(Action::Command(id));
        }
    }
}

/// The days from 1970-01-01 to `t` (UTC).
fn days(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| (d.as_secs() / 86_400) as i64)
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

//! The colour box and its panel (docs/ui/interface.md#colours): a colour shown in a box, edited
//! in a panel of our own — a saturation and value square, a hue bar, a hex field, the user's
//! favourite colours —, copied and pasted as `#RRGGBB` text.

use egui::{Color32, Id, Rect, Sense, Stroke, Vec2, pos2, vec2};
use skraft_i18n::t;

use crate::commands::Shortcut;
use crate::kit::icons;
use crate::kit::style::Tip;
use crate::kit::widgets::{Field, IconButton, Look, OPTION_HEIGHT, framed_field};

/// A colour, sRGB.
pub type Rgb = [u8; 3];

/// The `#RRGGBB` text of `c`.
pub fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// A colour read from text: `#C49862`, `C49862`, `196 152 98`, `196, 152, 98`,
/// `rgb(196, 152, 98)`.
pub fn parse(text: &str) -> Option<Rgb> {
    let t = text.trim();
    let h = t.strip_prefix('#').unwrap_or(t);
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        return Some([v(0)?, v(2)?, v(4)?]);
    }
    let lower = t.to_ascii_lowercase();
    let inner = match lower.strip_prefix("rgb") {
        Some(rest) => rest.trim().strip_prefix('(')?.strip_suffix(')')?,
        None => lower.as_str(),
    };
    let parts: Vec<&str> = inner.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).collect();
    match parts[..] {
        [r, g, b] => Some([r.parse().ok()?, g.parse().ok()?, b.parse().ok()?]),
        _ => None,
    }
}

/// `c` as hue (degrees), saturation, value (0 to 1), in sRGB as colour pickers show them.
fn to_hsv(c: Rgb) -> [f32; 3] {
    let [r, g, b] = c.map(|x| x as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max]
}

/// The colour of hue `h` (degrees), saturation `s`, value `v`.
fn from_hsv([h, s, v]: [f32; 3]) -> Rgb {
    let f = |n: f32| {
        let k = (n + h / 60.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)].map(|x| (x * 255.0).round() as u8)
}

/// `c` as egui's colour.
pub fn color32([r, g, b]: Rgb) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// What the panel keeps while open: the hue, saturation, value picked (the hue kept on a grey,
/// whose own is none).
#[derive(Clone)]
struct Panel {
    hsv: [f32; 3],
}

impl Panel {
    fn of(c: Rgb) -> Self {
        Panel { hsv: to_hsv(c) }
    }

    fn rgb(&self) -> Rgb {
        from_hsv(self.hsv)
    }

    /// Follows `c` when it is no longer the colour picked (changed elsewhere, pasted), its hue
    /// kept on a grey.
    fn follow(&mut self, c: Rgb) {
        if self.rgb() != c {
            let hue = self.hsv[0];
            *self = Panel::of(c);
            if self.hsv[1] == 0.0 {
                self.hsv[0] = hue;
            }
        }
    }
}

/// The colour copied in the app, for « Coller » (egui reads the system clipboard only when ⌘V
/// is pressed: ⌘V over a box or in the hex field pastes another app's colour).
fn clip_id() -> Id {
    Id::unique("kit.color.clip")
}

/// The pass a colour box or panel was last under the pointer: the clipboard's keys are then
/// theirs, not the model's ([`takes_clipboard_keys`]).
fn hover_id() -> Id {
    Id::unique("kit.color.hover")
}

/// The user's favourite colours, as the preferences hold them ([`sync_favorites`]).
fn favorites_id() -> Id {
    Id::unique("kit.color.favorites")
}

#[derive(Clone, Default)]
struct Favorites {
    colors: Vec<Rgb>,
    /// Changed in a panel, for the preferences to take.
    changed: bool,
}

/// At a frame's start: the favourite colours changed in a panel go to the preferences' `colors`,
/// else the panels show `colors`.
pub fn sync_favorites(ctx: &egui::Context, colors: &mut Vec<Rgb>) {
    ctx.data_mut(|d| {
        let f = d.get_temp_mut_or_default::<Favorites>(favorites_id());
        if f.changed {
            *colors = f.colors.clone();
            f.changed = false;
        } else if f.colors != *colors {
            f.colors = colors.clone();
        }
    });
}

fn favorites(ctx: &egui::Context) -> Vec<Rgb> {
    ctx.data(|d| d.get_temp::<Favorites>(favorites_id()).map(|f| f.colors).unwrap_or_default())
}

fn set_favorites(ctx: &egui::Context, colors: Vec<Rgb>) {
    ctx.data_mut(|d| d.insert_temp(favorites_id(), Favorites { colors, changed: true }));
}

/// Were the pointer over a colour box or its panel last frame? ⌘C and ⌘V are then the colour's
/// (read by the app before the interface is laid out, hence the last frame's).
pub fn takes_clipboard_keys(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(hover_id())).is_some_and(|p| p + 1 >= pass)
}

fn mark_hover(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(hover_id(), pass));
}

fn copy(ctx: &egui::Context, c: Rgb) {
    ctx.copy_text(hex(c));
    ctx.data_mut(|d| d.insert_temp(clip_id(), c));
}

fn copied(ctx: &egui::Context) -> Option<Rgb> {
    ctx.data(|d| d.get_temp::<Rgb>(clip_id()))
}

/// This frame's ⌘C and ⌘V, for a colour under the pointer: copies `c`, gives the colour pasted.
fn clipboard_keys(ui: &egui::Ui, c: Rgb, pastes: bool) -> Option<Rgb> {
    mark_hover(ui.ctx());
    let (copying, pasted) = ui.input(|i| {
        let copying = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
        let pasted = i.events.iter().find_map(|e| match e {
            egui::Event::Paste(text) => parse(text),
            _ => None,
        });
        (copying, pasted)
    });
    if copying {
        copy(ui.ctx(), c);
    }
    pasted.filter(|_| pastes)
}

/// A shortcut as this platform shows it.
fn shortcut(ctx: &egui::Context, text: &str) -> String {
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    Shortcut::parse(text).map(|s| s.display(mac)).unwrap_or_default()
}

/// The corner radius of a colour box, a swatch, a material's texture box (points).
pub const BOX_RADIUS: f32 = 3.0;

/// A swatch of `color` filling `rect`, edged by `edge`; none: empty, crossed.
pub fn paint_swatch(ui: &egui::Ui, rect: Rect, color: Option<Rgb>, edge: Stroke) {
    let p = ui.painter();
    match color {
        Some(c) => drop(p.rect(rect, BOX_RADIUS, color32(c), edge, egui::StrokeKind::Inside)),
        None => {
            p.rect_stroke(rect, BOX_RADIUS, edge, egui::StrokeKind::Inside);
            p.line_segment([rect.left_bottom(), rect.right_top()], edge);
        }
    }
}

/// A swatch of `color`, `size` points square, laid out (a material in a list); none: empty,
/// crossed.
pub fn swatch(ui: &mut egui::Ui, color: Option<Rgb>, size: f32) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_swatch(ui, rect, color, Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color));
    r
}

/// A colour box of `size`: `color` shown, its panel opened by a click, copied and pasted by its
/// menu (right click) or ⌘C / ⌘V under the pointer. `enabled` false: shown and copied only
/// (a colour following something else, as a texture's mean). Its response, and the colour
/// picked when changed.
pub fn color_box(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    color: Rgb,
    size: Vec2,
    enabled: bool,
) -> (egui::Response, Option<Rgb>) {
    let id = ui.make_persistent_id(id_salt);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    let edge = if enabled { visuals.text_color() } else { visuals.widgets.noninteractive.bg_stroke.color };
    paint_swatch(ui, rect, Some(color), Stroke::new(1.0, edge));
    let mut picked = None;
    if response.hovered() {
        picked = clipboard_keys(ui, color, enabled);
    }
    let ctx = ui.ctx().clone();
    egui::Popup::context_menu(&response).style(crate::kit::style::menu).show(|ui| {
        let item = crate::kit::style::menu_item(t!("color.copy"), None, false).shortcut_text(shortcut(&ctx, "Cmd+C"));
        if ui.add(item).clicked() {
            copy(&ctx, color);
        }
        let clip = copied(&ctx).filter(|_| enabled);
        let item = crate::kit::style::menu_item(t!("color.paste"), None, false).shortcut_text(shortcut(&ctx, "Cmd+V"));
        if ui.add_enabled(clip.is_some(), item).clicked() {
            picked = clip;
        }
    });
    if enabled {
        let panel_id = id.with("panel");
        let shown = egui::Popup::menu(&response)
            .style(crate::kit::style::menu)
            .id(panel_id)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| panel(ui, panel_id, picked.unwrap_or(color)));
        if let Some(inner) = shown {
            picked = picked.or(inner.inner);
        } else {
            ui.data_mut(|d| d.remove::<Panel>(panel_id));
        }
    }
    let response = if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response };
    (response, picked)
}

/// The side of the saturation and value square (points), the width of the hue bar.
const SQUARE: f32 = 200.0;
const HUE_BAR: f32 = 16.0;
/// A favourite colour's swatch (points).
const SWATCH: f32 = 20.0;
/// The hex field's width (points).
const HEX_FIELD: f32 = 84.0;

/// The panel of a colour box: the colour picked, when changed.
fn panel(ui: &mut egui::Ui, id: Id, color: Rgb) -> Option<Rgb> {
    let mut state = ui.data(|d| d.get_temp::<Panel>(id)).unwrap_or_else(|| Panel::of(color));
    state.follow(color);
    let mut picked = None;
    ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
    ui.set_width(SQUARE + 6.0 + HUE_BAR);
    let inner = ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if square(ui, &mut state.hsv) | hue_bar(ui, &mut state.hsv) {
                picked = Some(state.rgb());
            }
        });
        ui.horizontal(|ui| {
            let now = state.rgb();
            let (r, _) = ui.allocate_exact_size(vec2(28.0, OPTION_HEIGHT), Sense::hover());
            paint_swatch(ui, r, Some(now), Stroke::new(1.0, ui.visuals().text_color()));
            // The entity info's field: applied by Enter or a click away, Esc putting it back.
            let tip = t!("color.hex_tip");
            let field = Field {
                hint: None,
                tip: Some(&tip),
                width: HEX_FIELD,
                align: egui::Align2::LEFT_CENTER,
                lit: false,
                addon: None,
                blur_applies: true,
                empty_applies: false,
                handle: None,
            };
            let shown = hex(now);
            if let Some(c) = framed_field(ui, id.with("hex"), &shown, &shown, field).and_then(|t| parse(&t)) {
                picked = Some(c);
            }
        });
        ui.label(crate::kit::style::caption(t!("color.favorites")));
        if let Some(c) = favorites_row(ui, state.rgb()) {
            picked = Some(c);
        }
    });
    if ui.rect_contains_pointer(inner.response.rect)
        && !ui.ctx().egui_wants_keyboard_input()
        && let Some(c) = clipboard_keys(ui, state.rgb(), true)
    {
        picked = Some(c);
    }
    if let Some(c) = picked {
        state.follow(c);
    }
    ui.data_mut(|d| d.insert_temp(id, state));
    picked
}

/// The saturation (across) and value (up) square at the hue picked: whether it was moved.
fn square(ui: &mut egui::Ui, hsv: &mut [f32; 3]) -> bool {
    let (rect, r) = ui.allocate_exact_size(vec2(SQUARE, SQUARE * 0.75), Sense::click_and_drag());
    let mut moved = false;
    if let Some(p) = r.interact_pointer_pos() {
        hsv[1] = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        hsv[2] = 1.0 - ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        moved = true;
    }
    let h = hsv[0];
    gradient(ui, rect, 16, 16, |x, y| from_hsv([h, x, 1.0 - y]));
    let at = pos2(rect.left() + hsv[1] * rect.width(), rect.top() + (1.0 - hsv[2]) * rect.height());
    marker(ui, at);
    moved
}

/// The hue bar, red at the top: whether it was moved.
fn hue_bar(ui: &mut egui::Ui, hsv: &mut [f32; 3]) -> bool {
    let (rect, r) = ui.allocate_exact_size(vec2(HUE_BAR, SQUARE * 0.75), Sense::click_and_drag());
    let mut moved = false;
    if let Some(p) = r.interact_pointer_pos() {
        hsv[0] = (((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0) * 360.0).min(359.9);
        moved = true;
    }
    gradient(ui, rect, 1, 36, |_, y| from_hsv([y * 360.0, 1.0, 1.0]));
    let y = rect.top() + hsv[0] / 360.0 * rect.height();
    let bar = Rect::from_center_size(pos2(rect.center().x, y), vec2(rect.width() + 4.0, 4.0));
    ui.painter().rect(bar, 2.0, Color32::WHITE, Stroke::new(1.0, Color32::from_gray(40)), egui::StrokeKind::Outside);
    moved
}

/// `rect` filled with `color_at(x, y)` (0 to 1, from the top left), on a grid of `nx` by `ny`
/// cells, edged.
fn gradient(ui: &egui::Ui, rect: Rect, nx: usize, ny: usize, color_at: impl Fn(f32, f32) -> Rgb) {
    let mut mesh = egui::Mesh::default();
    for yi in 0..=ny {
        for xi in 0..=nx {
            let (x, y) = (xi as f32 / nx as f32, yi as f32 / ny as f32);
            mesh.colored_vertex(pos2(rect.left() + x * rect.width(), rect.top() + y * rect.height()), color32(color_at(x, y)));
        }
    }
    let row = nx as u32 + 1;
    for yi in 0..ny as u32 {
        for xi in 0..nx as u32 {
            let i = yi * row + xi;
            mesh.add_triangle(i, i + 1, i + row);
            mesh.add_triangle(i + 1, i + row + 1, i + row);
        }
    }
    let painter = ui.painter();
    painter.add(mesh);
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Outside);
}

/// The point picked in the square: a white ring, edged dark, readable on any colour.
fn marker(ui: &egui::Ui, at: egui::Pos2) {
    let p = ui.painter();
    p.circle_stroke(at, 6.0, Stroke::new(3.0, Color32::from_gray(40)));
    p.circle_stroke(at, 6.0, Stroke::new(2.0, Color32::WHITE));
}

/// The favourite colours, a click picking one, its menu removing it; « + » adds `current`, « − »
/// takes it out when it is one. The colour picked.
fn favorites_row(ui: &mut egui::Ui, current: Rgb) -> Option<Rgb> {
    let mut favs = favorites(ui.ctx());
    let mut picked = None;
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        let mut remove = None;
        for (i, &c) in favs.iter().enumerate() {
            let (rect, r) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
            let lit = c == current;
            let edge = if lit { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
            paint_swatch(ui, rect, Some(c), Stroke::new(if lit { 2.0 } else { 1.0 }, edge));
            let r = r.on_hover_cursor(egui::CursorIcon::PointingHand).tip(hex(c));
            if r.clicked() {
                picked = Some(c);
            }
            egui::Popup::context_menu(&r).style(crate::kit::style::menu).show(|ui| {
                if ui.add(crate::kit::style::menu_item(t!("color.favorite_remove"), None, false)).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            favs.remove(i);
            changed = true;
        }
        // « + » adds the colour shown, « − » takes it out when it is one already.
        let known = favs.iter().position(|&c| c == current);
        let (icon, tip) = match known {
            Some(_) => (icons::MINUS, t!("color.favorite_remove")),
            None => (icons::ADD, t!("color.favorite_add_tip")),
        };
        if IconButton::new(icon, SWATCH, 12.0).look(Look::Veil).show(ui).tip(tip).clicked() {
            match known {
                Some(i) => drop(favs.remove(i)),
                None => favs.push(current),
            }
            changed = true;
        }
    });
    if changed {
        set_favorites(ui.ctx(), favs);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_read_as_hex_or_numbers() {
        let oak = Some([196, 152, 98]);
        for text in ["#C49862", "c49862", " #c49862 ", "196 152 98", "196, 152, 98", "rgb(196, 152, 98)", "RGB(196,152,98)"] {
            assert_eq!(parse(text), oak, "{text}");
        }
        for text in ["", "#C4986", "#C498622", "196 152", "256 0 0", "rgb 1 2 3", "oak"] {
            assert_eq!(parse(text), None, "{text}");
        }
        assert_eq!(hex([196, 152, 98]), "#C49862");
    }

    #[test]
    fn hsv_round_trips() {
        for c in [[196, 152, 98], [0, 0, 0], [255, 255, 255], [90, 92, 96], [255, 0, 0], [12, 200, 77]] {
            assert_eq!(from_hsv(to_hsv(c)), c);
        }
    }

    #[test]
    fn a_grey_keeps_the_hue_picked() {
        let mut p = Panel::of([200, 40, 40]);
        let hue = p.hsv[0];
        p.follow([128, 128, 128]);
        assert_eq!(p.hsv[0], hue);
        assert_eq!(p.rgb(), [128, 128, 128]);
    }
}

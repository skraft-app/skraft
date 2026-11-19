//! The Preferences' Navigation tab (docs/ui/navigation-styles.md): the navigation style
//! (a preset, maybe edited), a drawing of the mouse whose buttons lead to their gestures, the
//! gestures of the button picked, the trackpad, the speeds. One column of labels, the same
//! width in every section.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use skraft_i18n::t;
use skraft_tools::navigation::{Binding, BindingError, Button, Mods, NavAction, NavStyle, Pivot, Preset, Trigger};

use crate::prefs::Navigation;
use crate::style::{settings_grid as grid, settings_section as section};

/// The mouse drawn: its size, and its buttons' line (points from its top).
const MOUSE: Vec2 = vec2(84.0, 136.0);
const BUTTONS_LINE: f32 = 56.0;
/// The radius of the mouse's top corners.
const BODY_TOP_RADIUS: f32 = 40.0;
/// The room on each side of the mouse, for the lines to the gestures.
const LEAD: f32 = 34.0;
/// The horizontal end of a leader line, under its text's start (points).
const SHELF: f32 = 10.0;
const LINE_HEIGHT: f32 = 18.0;
const BLOCK_GAP: f32 = 14.0;

pub fn navigation_settings(ui: &mut egui::Ui, nav: &mut Navigation) {
    // Taller than most screens: it scrolls down to the screen's bottom, not past it.
    let max_height = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 32.0).max(200.0);
    egui::ScrollArea::vertical()
        .max_height(max_height)
        .min_scrolled_height(max_height)
        .auto_shrink([false, true])
        .show(ui, |ui| settings(ui, nav));
}

fn settings(ui: &mut egui::Ui, nav: &mut Navigation) {
    let before = nav.style.clone();
    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;

    section(ui, &t!("prefs.navigation.section.style"));
    grid(ui, "nav_style", |ui| {
        ui.label(t!("prefs.navigation.style.label"));
        ui.horizontal(|ui| {
            let style = &mut nav.style;
            let shown =
                if style.custom { t!("prefs.navigation.style.custom", name = style.preset.name()) } else { style.preset.name().to_owned() };
            egui::ComboBox::from_id_salt("nav_preset").icon(crate::icons::combo_chevron).width(200.0).selected_text(shown).show_ui(
                ui,
                |ui| {
                    for p in Preset::ALL {
                        if ui.selectable_label(!style.custom && style.preset == p, p.name()).clicked() {
                            *style = p.style();
                        }
                    }
                },
            );
            if style.custom && ui.button(t!("prefs.navigation.style.restore", name = style.preset.name())).clicked() {
                *style = style.preset.style();
            }
        });
        ui.end_row();
        ui.label(t!("prefs.navigation.style.pivot"));
        egui::ComboBox::from_id_salt("nav_pivot")
            .icon(crate::icons::combo_chevron)
            .width(200.0)
            .selected_text(pivot_name(nav.style.pivot))
            .show_ui(ui, |ui| {
                for p in Pivot::ALL {
                    ui.selectable_value(&mut nav.style.pivot, p, pivot_name(p));
                }
            });
        ui.end_row();
    });

    section(ui, &t!("prefs.navigation.section.mouse"));
    // The whole mouse's settings first; then its buttons drawn, and the picked one's gestures,
    // framed, ending the section.
    grid(ui, "nav_mouse", |ui| {
        ui.label(t!("prefs.navigation.mouse.wheel"));
        ui.checkbox(&mut nav.style.wheel_reversed, t!("prefs.navigation.mouse.wheel_reversed"));
        ui.end_row();
        ui.label(t!("prefs.navigation.mouse.orbit"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut nav.mouse_invert_x, t!("prefs.navigation.invert_x"));
            ui.checkbox(&mut nav.mouse_invert_y, t!("prefs.navigation.invert_y"));
        });
        ui.end_row();
    });
    ui.add_space(10.0);
    let picked_id = egui::Id::unique("nav_picked_button");
    let mut picked = ui.data(|d| d.get_temp::<Button>(picked_id)).unwrap_or(Button::Middle);
    mouse_drawing(ui, &nav.style, &mut picked, mac);
    ui.add_space(6.0);
    button_gestures(ui, &mut nav.style, &mut picked, mac);
    ui.data_mut(|d| d.insert_temp(picked_id, picked));

    section(ui, &t!("prefs.navigation.section.trackpad"));
    grid(ui, "nav_trackpad", |ui| {
        ui.label(t!("prefs.navigation.trackpad.scroll"));
        trackpad_combo(ui, "nav_scroll", &mut nav.style.trackpad.scroll);
        ui.end_row();
        ui.label(t!("prefs.navigation.trackpad.shift_scroll", shift = if mac { "⇧".to_owned() } else { t!("key.shift") }));
        trackpad_combo(ui, "nav_shift_scroll", &mut nav.style.trackpad.shift_scroll);
        ui.end_row();
        ui.label(t!("prefs.navigation.trackpad.pinch"));
        ui.label(egui::RichText::new(action_name(NavAction::ZoomDrag)).weak());
        ui.end_row();
        ui.label(t!("prefs.navigation.trackpad.orbit"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut nav.trackpad_invert_x, t!("prefs.navigation.invert_x"));
            ui.checkbox(&mut nav.trackpad_invert_y, t!("prefs.navigation.invert_y"));
        });
        ui.end_row();
    });

    section(ui, &t!("prefs.navigation.section.speeds"));
    grid(ui, "nav_speeds", |ui| {
        ui.label(t!("prefs.navigation.speeds.orbit"));
        ui.add(egui::Slider::new(&mut nav.orbit_speed, 0.25..=4.0).logarithmic(true).fixed_decimals(2));
        ui.end_row();
        ui.label(t!("prefs.navigation.speeds.zoom"));
        ui.add(egui::Slider::new(&mut nav.zoom_speed, 0.25..=4.0).logarithmic(true).fixed_decimals(2));
        ui.end_row();
        ui.label(t!("prefs.navigation.speeds.zoom_target"));
        ui.checkbox(&mut nav.zoom_to_cursor, t!("prefs.navigation.speeds.to_cursor"));
        ui.end_row();
    });

    // Any change to the style but choosing a preset: it is the user's own from now on.
    if nav.style != before && nav.style.preset == before.preset && !before.custom && !nav.style.custom {
        nav.style.edited();
    }
    ui.add_space(10.0);
    ui.separator();
    if ui.add_enabled(*nav != Navigation::default(), egui::Button::new(t!("prefs.navigation.defaults"))).clicked() {
        *nav = Navigation::default();
    }
}

/// One button's gestures, as drawn beside the mouse: its title, then a line per binding.
struct Block {
    button: Button,
    title: String,
    lines: Vec<(String, bool)>,
}

impl Block {
    fn new(style: &NavStyle, button: Button, mac: bool) -> Self {
        let lines: Vec<(String, bool)> = style
            .bindings
            .iter()
            .enumerate()
            .filter(|(_, b)| b.trigger.button() == button)
            .map(|(i, b)| {
                (format!("{}{}  →  {}", mods_prefix(b.mods, mac), trigger_name(b.trigger), action_name(b.action)), style.error(i).is_some())
            })
            .collect();
        Block { button, title: button_name(button), lines }
    }

    fn height(&self) -> f32 {
        LINE_HEIGHT * (1 + self.lines.len().max(1)) as f32
    }
}

/// The mouse drawn: its buttons and wheel, the side buttons on its left; each button led by a
/// line to its gestures (the side buttons' on the left, the others' on the right). A click on a
/// button or on its gestures picks it; the picked one is filled.
fn mouse_drawing(ui: &mut egui::Ui, style: &NavStyle, picked: &mut Button, mac: bool) {
    let visuals = ui.visuals().clone();
    let font = FontId::proportional(13.0);
    let right_blocks = [Block::new(style, Button::Middle, mac), Block::new(style, Button::Right, mac)];
    let left_blocks = [Block::new(style, Button::Forward, mac), Block::new(style, Button::Back, mac)];
    let column = |blocks: &[Block]| blocks.iter().map(Block::height).sum::<f32>() + BLOCK_GAP * (blocks.len() - 1) as f32;
    let height = (MOUSE.y + 26.0).max(column(&right_blocks)).max(column(&left_blocks) + 50.0) + 4.0;
    // The columns as wide as their texts, the whole centred.
    let text_width =
        |text: &str, size: f32| ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::WHITE).size().x;
    // A title as wide as it is in semibold (the picked one's).
    let title_width = |text: &str| ui.painter().layout_no_wrap(text.to_owned(), crate::style::semibold(13.5), Color32::WHITE).size().x;
    let column_width = |blocks: &[Block]| {
        blocks
            .iter()
            .flat_map(|b| std::iter::once(title_width(&b.title)).chain(b.lines.iter().map(|(l, _)| text_width(l, 13.0))))
            .fold(0.0f32, f32::max)
    };
    let (left_width, right_width) = (column_width(&left_blocks), column_width(&right_blocks));
    let whole = left_width + 2.0 * LEAD + MOUSE.x + right_width;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(whole), height), Sense::hover());
    let painter = ui.painter_at(rect.expand(2.0));
    let x0 = rect.left() + ((rect.width() - whole) / 2.0).max(0.0);

    // The mouse and its parts.
    let body = Rect::from_min_size(pos2(x0 + left_width + LEAD, rect.top() + 2.0), MOUSE);
    let cx = body.center().x;
    let split = body.top() + BUTTONS_LINE;
    let left = Rect::from_min_max(body.min, pos2(cx, split));
    let right = Rect::from_min_max(pos2(cx, body.top()), pos2(body.right(), split));
    let wheel = Rect::from_center_size(pos2(cx, body.top() + 28.0), vec2(12.0, 26.0));
    let forward = Rect::from_min_size(pos2(body.left() - 5.0, body.top() + 68.0), vec2(9.0, 18.0));
    let back = Rect::from_min_size(pos2(body.left() - 5.0, body.top() + 92.0), vec2(9.0, 18.0));
    let part = |b: Button| match b {
        Button::Middle => wheel,
        Button::Right => right,
        Button::Forward => forward,
        Button::Back => back,
    };

    let ink = visuals.text_color();
    let line = Stroke::new(1.0, visuals.weak_text_color());
    let accent = visuals.selection.bg_fill;
    let accent_ink = visuals.selection.stroke.color;
    let hover_fill = visuals.widgets.hovered.weak_bg_fill;
    let round = |nw: f32, ne: f32, sw: f32, se: f32| CornerRadius { nw: nw as u8, ne: ne as u8, sw: sw as u8, se: se as u8 };
    let body_radius = round(BODY_TOP_RADIUS, BODY_TOP_RADIUS, 36.0, 36.0);

    // Hover and clicks: on the parts, and on the gestures' blocks (laid out below).
    let mut hovered = None;
    for b in Button::ALL {
        let r = ui.interact(part(b).expand(2.0), ui.scope_id().with(("nav_part", b)), Sense::click());
        if r.hovered() {
            hovered = Some(b);
        }
        if r.clicked() {
            *picked = b;
        }
    }

    // Blocks: placed first, so their clicks count too.
    // Each line its own start, so that none runs along another: the wheel's and the side
    // buttons' from their centre, the right button's from low in it, below the wheel's.
    let anchor = |b: Button| match b {
        Button::Middle => wheel.center(),
        Button::Right => pos2(right.center().x + 6.0, split - 12.0),
        Button::Forward | Button::Back => part(b).center(),
    };
    let mut placed: Vec<(Rect, &Block, Pos2, bool)> = Vec::new();
    let mut y = rect.top();
    for block in &right_blocks {
        let r = Rect::from_min_size(pos2(body.right() + LEAD, y), vec2(right_width, block.height()));
        placed.push((r, block, anchor(block.button), true));
        y += block.height() + BLOCK_GAP;
    }
    let mut y = rect.top() + 50.0;
    for block in &left_blocks {
        let r = Rect::from_min_size(pos2(x0, y), vec2(left_width, block.height()));
        placed.push((r, block, anchor(block.button), false));
        y += block.height() + BLOCK_GAP;
    }
    for (r, block, _, _) in &placed {
        let resp = ui.interact(*r, ui.scope_id().with(("nav_block", block.button)), Sense::click());
        if resp.hovered() {
            hovered = Some(block.button);
        }
        if resp.clicked() {
            *picked = block.button;
        }
    }
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let fill = |b: Button| {
        if *picked == b {
            Some(accent)
        } else if hovered == Some(b) {
            Some(hover_fill)
        } else {
            None
        }
    };

    // The left button, the tools': dotted, not to be picked.
    let corner = pos2(left.left() + BODY_TOP_RADIUS, left.top() + BODY_TOP_RADIUS);
    let mut p = left.min + vec2(9.0, 9.0);
    while p.y < left.bottom() - 4.0 {
        while p.x < left.right() - 4.0 {
            let outside_corner = p.x < corner.x && p.y < corner.y && p.distance(corner) > BODY_TOP_RADIUS - 6.0;
            if !outside_corner {
                painter.circle_filled(p, 0.9, visuals.weak_text_color());
            }
            p.x += 7.0;
        }
        p = pos2(left.left() + 9.0 + if ((p.y - left.top()) / 7.0) as i32 % 2 == 0 { 3.5 } else { 0.0 }, p.y + 7.0);
    }
    if let Some(c) = fill(Button::Right) {
        // Following the body's top right curve, inside its outline (1.5 wide). Not a rounded
        // rectangle: egui keeps a corner's radius within half the rectangle's width (21 here,
        // not the body's 40).
        let r = BODY_TOP_RADIUS - 1.5;
        let centre = pos2(body.right() - BODY_TOP_RADIUS, body.top() + BODY_TOP_RADIUS);
        let mut points = vec![pos2(right.left(), split), pos2(right.left(), body.top() + 1.5)];
        points.extend((0..=16).map(|k| {
            let a = std::f32::consts::FRAC_PI_2 * (1.0 - k as f32 / 16.0);
            pos2(centre.x + r * a.cos(), centre.y - r * a.sin())
        }));
        points.push(pos2(body.right() - 1.5, split));
        painter.add(egui::Shape::convex_polygon(points, c, Stroke::NONE));
    }
    painter.rect_stroke(body, body_radius, Stroke::new(1.5, ink), StrokeKind::Inside);
    painter.line_segment([pos2(body.left(), split), pos2(body.right(), split)], Stroke::new(1.0, ink));
    painter.line_segment([pos2(cx, body.top()), pos2(cx, split)], Stroke::new(1.0, ink));
    for b in [Button::Middle, Button::Forward, Button::Back] {
        let r = part(b);
        let radius = if b == Button::Middle { 6.0 } else { 3.0 };
        painter.rect_filled(r, CornerRadius::same(radius as u8), fill(b).unwrap_or(visuals.extreme_bg_color));
        let edge = if *picked == b { accent_ink } else { ink };
        painter.rect_stroke(r, CornerRadius::same(radius as u8), Stroke::new(1.2, edge), StrokeKind::Inside);
    }
    painter.text(
        pos2(cx, body.bottom() + 6.0),
        Align2::CENTER_TOP,
        t!("prefs.navigation.mouse.left_button"),
        FontId::proportional(11.0),
        visuals.weak_text_color(),
    );

    // The gestures, each block led to its button.
    for (r, block, anchor, on_right) in &placed {
        let title_y = r.top() + LINE_HEIGHT / 2.0;
        // One bend: a slant from the button, then a short shelf to the text (a drawing's leader).
        let end_x = if *on_right { r.left() - 4.0 } else { r.right() + 4.0 };
        let shelf_x = if *on_right { end_x - SHELF } else { end_x + SHELF };
        let points = vec![*anchor, pos2(shelf_x, title_y), pos2(end_x, title_y)];
        let stroke = if *picked == block.button { Stroke::new(1.5, accent_ink) } else { line };
        painter.add(egui::Shape::line(points, stroke));
        painter.circle_filled(*anchor, 2.0, stroke.color);
        let (align, x) = if *on_right { (Align2::LEFT_CENTER, r.left()) } else { (Align2::RIGHT_CENTER, r.right()) };
        // The picked button's title in semibold, in its line's colour.
        let (title_font, title_color) =
            if *picked == block.button { (crate::style::semibold(13.5), accent_ink) } else { (FontId::proportional(13.5), ink) };
        painter.text(pos2(x, title_y), align, &block.title, title_font, title_color);
        if block.lines.is_empty() {
            painter.text(pos2(x, title_y + LINE_HEIGHT), align, "—", font.clone(), visuals.weak_text_color());
        }
        for (k, (text, refused)) in block.lines.iter().enumerate() {
            let color = if *refused { visuals.error_fg_color } else { visuals.weak_text_color() };
            painter.text(pos2(x, title_y + LINE_HEIGHT * (k + 1) as f32), align, text, font.clone(), color);
        }
    }
}

/// The gestures of the button picked in the drawing: a row each (the trigger, the modifiers as
/// toggles, the action, delete), why a row is not applied under it, then « Ajouter un geste ».
fn button_gestures(ui: &mut egui::Ui, style: &mut NavStyle, picked: &mut Button, mac: bool) {
    // The window's corners: the tabs' band follows them (`round_top`).
    let margin = egui::Margin::same(10);
    let frame = egui::Frame::group(ui.style()).inner_margin(margin).corner_radius(ui.visuals().window_corner_radius);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        // A tab per button, as the ribbon's; one picked here is picked in the drawing too.
        let tabs = [Button::Middle, Button::Right, Button::Back, Button::Forward];
        // The buttons' names, as in the drawing; the frame (and the window) as wide as the tabs.
        let labels: Vec<String> = tabs.iter().map(|b| button_name(*b)).collect();
        ui.set_min_width(crate::ribbon::tab_strip_width(ui.painter(), &labels));
        let ids: Vec<egui::Id> = tabs.iter().map(|b| egui::Id::unique(("nav_button_tab", *b))).collect();
        let shown = tabs.iter().position(|b| b == picked);
        if let Some(k) = crate::ribbon::tab_strip(ui, &labels, &ids, shown, ui.available_width(), margin, true, &[]) {
            *picked = tabs[k];
        }
        ui.add_space(6.0);
        let button = *picked;
        let rows: Vec<usize> = (0..style.bindings.len()).filter(|&i| style.bindings[i].trigger.button() == button).collect();
        let mut delete = None;
        if rows.is_empty() {
            ui.label(egui::RichText::new(t!("prefs.navigation.gestures.none")).weak());
        }
        // One row each, across the frame: the modifiers, the trigger, → the action (taking the
        // rest), delete at the right edge; all as high as a combo box.
        let h = ui.spacing().interact_size.y;
        for &i in &rows {
            let error = style.error(i);
            let b = &mut style.bindings[i];
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (on, name) in mod_keys(&mut b.mods, mac) {
                    ui.add(egui::Button::new(name).selected(*on).min_size(vec2(26.0, h))).clicked().then(|| *on = !*on);
                }
                ui.add_space(10.0);
                egui::ComboBox::from_id_salt(("nav_trigger", i))
                    .icon(crate::icons::combo_chevron)
                    .width(120.0)
                    .selected_text(trigger_name(b.trigger))
                    .show_ui(ui, |ui| {
                        for k in [Trigger::Drag(button), Trigger::Click(button), Trigger::DoubleClick(button)] {
                            if ui.selectable_label(b.trigger == k, trigger_name(k)).clicked() && b.trigger != k {
                                b.trigger = k;
                                // An action of the trigger's kind.
                                if b.action.is_drag() != k.is_drag() {
                                    b.action = if k.is_drag() { NavAction::Orbit } else { NavAction::ZoomExtents };
                                }
                            }
                        }
                    });
                ui.add_space(4.0);
                ui.label("→");
                ui.add_space(4.0);
                let actions = if b.trigger.is_drag() { NavAction::DRAG } else { NavAction::CLICK };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let x = egui::Image::new(crate::icons::CLOSE).fit_to_exact_size(vec2(12.0, 12.0)).tint(ui.visuals().text_color());
                    if ui
                        .add(egui::Button::image(x).min_size(vec2(h, h)))
                        .on_hover_text(t!("prefs.navigation.gestures.delete_tip"))
                        .clicked()
                    {
                        delete = Some(i);
                    }
                    ui.add_space(6.0);
                    let width = ui.available_width();
                    egui::ComboBox::from_id_salt(("nav_action", i))
                        .icon(crate::icons::combo_chevron)
                        .width(width)
                        .selected_text(action_name(b.action))
                        .show_ui(ui, |ui| {
                            for a in actions {
                                ui.selectable_value(&mut b.action, a, action_name(a));
                            }
                        });
                });
            });
            if let Some(e) = error {
                ui.label(egui::RichText::new(error_text(e)).small().color(ui.visuals().error_fg_color));
            }
        }
        ui.add_space(2.0);
        if let Some(i) = delete {
            style.bindings.remove(i);
        }
        let add = egui::Image::new(crate::icons::ADD).fit_to_exact_size(vec2(14.0, 14.0)).tint(ui.visuals().text_color());
        if ui.add(egui::Button::image_and_text(add, t!("prefs.navigation.gestures.add"))).clicked() {
            style.bindings.push(new_binding(style, button));
        }
    });
}

/// A gesture to add on `button`: a drag (a click for the side buttons), with the first
/// modifiers it does not have yet, so that it is applied at once.
fn new_binding(style: &NavStyle, button: Button) -> Binding {
    let side = matches!(button, Button::Back | Button::Forward);
    let trigger = if side { Trigger::Click(button) } else { Trigger::Drag(button) };
    let action = if side { NavAction::ZoomExtents } else { NavAction::Orbit };
    let candidates = [
        Mods::NONE,
        Mods { shift: true, ..Mods::NONE },
        Mods { ctrl: true, ..Mods::NONE },
        Mods { alt: true, ..Mods::NONE },
        Mods { shift: true, ctrl: true, ..Mods::NONE },
    ];
    let mods = candidates.into_iter().find(|m| !style.bindings.iter().any(|b| b.trigger == trigger && b.mods == *m)).unwrap_or(Mods::NONE);
    Binding { trigger, mods, action }
}

/// The modifier keys as toggles: ⇧ ⌃ ⌥ ⌘ on macOS, Maj Ctrl Alt elsewhere.
fn mod_keys(m: &mut Mods, mac: bool) -> Vec<(&mut bool, String)> {
    if mac {
        vec![(&mut m.shift, "⇧".into()), (&mut m.ctrl, "⌃".into()), (&mut m.alt, "⌥".into()), (&mut m.cmd, "⌘".into())]
    } else {
        vec![(&mut m.shift, t!("key.shift")), (&mut m.ctrl, t!("key.ctrl")), (&mut m.alt, t!("key.alt"))]
    }
}

/// The modifiers before a gesture's name: « ⇧⌃ » on macOS, « Maj+Ctrl+ » elsewhere.
fn mods_prefix(m: Mods, mac: bool) -> String {
    if mac {
        let s: String = [(m.ctrl, "⌃"), (m.alt, "⌥"), (m.shift, "⇧"), (m.cmd, "⌘")].iter().filter(|(on, _)| *on).map(|(_, s)| *s).collect();
        if s.is_empty() { s } else { s + " " }
    } else {
        [(m.ctrl, t!("key.ctrl")), (m.alt, t!("key.alt")), (m.shift, t!("key.shift"))]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, s)| s + "+")
            .collect()
    }
}

fn error_text(e: BindingError) -> String {
    match e {
        BindingError::Duplicate => t!("prefs.navigation.gestures.duplicate"),
        BindingError::RightClick => t!("prefs.navigation.gestures.right_click"),
        BindingError::ClickAndDoubleClick => t!("prefs.navigation.gestures.click_and_double"),
        BindingError::WrongKind => t!("prefs.navigation.gestures.wrong_kind"),
    }
}

fn trackpad_combo(ui: &mut egui::Ui, id: &str, action: &mut NavAction) {
    egui::ComboBox::from_id_salt(id).icon(crate::icons::combo_chevron).width(200.0).selected_text(action_name(*action)).show_ui(ui, |ui| {
        for a in [NavAction::Orbit, NavAction::Pan] {
            ui.selectable_value(action, a, action_name(a));
        }
    });
}

fn trigger_name(t: Trigger) -> String {
    match t {
        Trigger::Drag(_) => t!("prefs.navigation.trigger.drag"),
        Trigger::Click(_) => t!("prefs.navigation.trigger.click"),
        Trigger::DoubleClick(_) => t!("prefs.navigation.trigger.double_click"),
    }
}

fn button_name(b: Button) -> String {
    match b {
        Button::Middle => t!("prefs.navigation.button.middle"),
        Button::Right => t!("prefs.navigation.button.right"),
        Button::Back => t!("prefs.navigation.button.back"),
        Button::Forward => t!("prefs.navigation.button.forward"),
    }
}

fn action_name(a: NavAction) -> String {
    match a {
        NavAction::Orbit => t!("prefs.navigation.action.orbit"),
        NavAction::Pan => t!("prefs.navigation.action.pan"),
        NavAction::ZoomDrag => t!("prefs.navigation.action.zoom_drag"),
        NavAction::ZoomWindow => t!("prefs.navigation.action.zoom_window"),
        NavAction::Recentre => t!("prefs.navigation.action.recentre"),
        NavAction::ZoomExtents => t!("prefs.navigation.action.zoom_extents"),
        NavAction::PreviousView => t!("prefs.navigation.action.previous_view"),
        NavAction::NextView => t!("prefs.navigation.action.next_view"),
    }
}

fn pivot_name(p: Pivot) -> String {
    match p {
        Pivot::Aimed => t!("prefs.navigation.pivot.aimed"),
        Pivot::SelectionOrModel => t!("prefs.navigation.pivot.selection_or_model"),
        Pivot::ScreenCentre => t!("prefs.navigation.pivot.screen_centre"),
    }
}

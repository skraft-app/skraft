//! The interface's look (docs/ARCHITECTURE.md §7 quater): the theme's interface tokens turned
//! into egui's visuals and spacing, for the light and the dark style, and the bundled fonts.
//! Both styles of a palette are installed together, when it is chosen; egui switches between
//! them when its theme changes.

use std::sync::Arc;

use egui::{CornerRadius, FontData, FontDefinitions, FontFamily, Shadow, Stroke, TextStyle};
use solidsketch_theme::{Mode, Palette, TitleBarTokens, UiTokens};

use crate::viewport::color;

/// IBM Plex Sans (variable, SIL Open Font License: `assets/fonts/ibm-plex-sans/OFL.txt`).
const PLEX_SANS: &[u8] = include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf");
/// IBM Plex Mono regular (SIL Open Font License: `assets/fonts/ibm-plex-mono/OFL.txt`).
const PLEX_MONO: &[u8] = include_bytes!("../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf");
/// The arrows and the technical symbols (U+2190–21FF, U+2300–23FF) of DejaVu Sans
/// (Bitstream Vera licence: `assets/fonts/dejavu-sans/LICENSE`): the macOS key symbols
/// (⌘ ⌥ ⌃ ⌫ ⌦ ⏎ ⇥) that Plex and egui's fonts lack.
const KEY_SYMBOLS: &[u8] = include_bytes!("../assets/fonts/dejavu-sans/DejaVuSans-KeySymbols.ttf");
/// The family headings are written in.
const SEMIBOLD: &str = "plex-sans-semibold";

/// Plex Sans semibold at `size`: text painted bold (a heading's family).
pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(SEMIBOLD.into()))
}

/// The width of a settings window's labels' column (points), the same in every section.
const LABEL_WIDTH: f32 = 170.0;

/// A settings window's section: some room above, its title (semibold), a thin line under it.
pub fn settings_section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    section_title(ui, title);
    ui.separator();
}

/// A settings window's two-column grid, its labels' column as wide in every section.
pub fn settings_grid(ui: &mut egui::Ui, id: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_WIDTH).show(ui, body);
}

/// A section's title in a settings window: semibold, at the body's size, in the strong text
/// colour.
pub fn section_title(ui: &mut egui::Ui, title: impl Into<String>) -> egui::Response {
    let size = egui::TextStyle::Body.resolve(ui.style()).size;
    ui.label(egui::RichText::new(title.into()).font(semibold(size)).color(ui.visuals().strong_text_color()))
}

/// Installs the fonts (once, at start-up).
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
}

/// Installs the light and the dark style of `palette`.
pub fn install_palette(ctx: &egui::Context, palette: Palette) {
    for (theme, mode) in [(egui::Theme::Light, Mode::Light), (egui::Theme::Dark, Mode::Dark)] {
        let t = UiTokens::of(palette, mode);
        ctx.style_mut_of(theme, |s| apply(s, &t));
        ctx.data_mut(|d| d.insert_temp(title_bar_id(theme), t.title_bar));
    }
}

fn title_bar_id(theme: egui::Theme) -> egui::Id {
    egui::Id::unique(("title_bar_tokens", theme == egui::Theme::Dark))
}

/// Turns `style` into the title bar's: its dark ground, light text, separators and lit fill
/// (docs/ARCHITECTURE.md §7 quinquies). The popups it opens (menus, tips) keep the window's
/// style.
pub fn title_bar(ctx: &egui::Context, style: &mut egui::Style) {
    let Some(t) = ctx.data(|d| d.get_temp::<TitleBarTokens>(title_bar_id(ctx.theme()))) else { return };
    let v = &mut style.visuals;
    let text = color(t.text);
    v.panel_fill = color(t.ground);
    v.override_text_color = Some(text);
    v.weak_text_color = Some(color(t.text_dim));
    // The lit buttons (and the open menus' buttons), the shown tab, a tab being dragged.
    v.selection.bg_fill = color(t.lit);
    v.selection.stroke.color = text;
    v.window_fill = color(t.lit);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke.color = color(t.border);
    w.active.weak_bg_fill = color(t.lit);
    w.hovered.weak_bg_fill = text.gamma_multiply(0.12);
    for state in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active] {
        state.fg_stroke.color = text;
    }
    // The bar's buttons are frameless (their background is painted behind them). egui keeps a
    // button's size by taking its state's stroke and expansion out of its margins: with a
    // stroke or an expansion on hover, a frameless button would narrow when hovered.
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.bg_stroke = egui::Stroke::NONE;
        state.expansion = 0.0;
    }
}

/// The side margins of a menu's items: egui's menus keep only 2 pt.
const MENU_PADDING_X: f32 = 8.0;

/// The style of every menu (drop-down, context menu, and the submenus they open): egui's
/// menus', their items wider.
pub fn menu(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding.x = MENU_PADDING_X;
}

/// The check box column of a menu that has check boxes: its side, the box's.
const MENU_CHECK: f32 = 13.0;

/// A menu item: `check` its check box (a toggle's state), in a column every item of the menu
/// keeps when one of them has one (`column`), so that their labels start at the same place.
pub fn menu_item<'a>(label: impl Into<String>, check: Option<bool>, column: bool) -> egui::Button<'a> {
    let label = label.into();
    if !column && check.is_none() {
        return egui::Button::new(label);
    }
    egui::Button::new((menu_check(check), label))
}

/// A menu item's check box (none: its empty column), as [`menu_item`] draws it.
pub fn menu_check<'a>(check: Option<bool>) -> egui::Atom<'a> {
    egui::Atom::paint(egui::Vec2::splat(MENU_CHECK), move |ui: &egui::Ui, args: egui::AtomPaintArgs| {
        let Some(on) = check else { return };
        let ink = args.fallback_text_color;
        let p = ui.painter();
        let r = egui::Rect::from_center_size(args.rect.center(), egui::Vec2::splat(MENU_CHECK - 1.0));
        p.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.0, ink.gamma_multiply(0.7)), egui::StrokeKind::Inside);
        if on {
            let (c, s) = (r.center(), r.width() / 2.0);
            let points = vec![c + egui::vec2(-0.55 * s, 0.0), c + egui::vec2(-0.15 * s, 0.4 * s), c + egui::vec2(0.55 * s, -0.4 * s)];
            p.add(egui::Shape::line(points, Stroke::new(1.6, ink)));
        }
    })
}

/// A palette's card in the preferences: a small window drawn in its colours (`mode`'s
/// variant), its name below; outlined in the current accent when `selected`.
pub fn palette_card(ui: &mut egui::Ui, palette: Palette, mode: Mode, name: &str, selected: bool) -> egui::Response {
    let t = UiTokens::of(palette, mode);
    let size = egui::vec2(96.0, 60.0);
    let (rect, response) = ui.allocate_exact_size(size + egui::vec2(0.0, 18.0), egui::Sense::click());
    let card = egui::Rect::from_min_size(rect.min, size);
    let p = ui.painter();
    let r = CornerRadius::same(t.radius_md);
    p.rect_filled(card, r, color(t.panel));
    // A header, two lines of text, a button and a selected row.
    let header = egui::Rect::from_min_size(card.min, egui::vec2(size.x, 14.0));
    p.rect_filled(header, CornerRadius { nw: t.radius_md, ne: t.radius_md, sw: 0, se: 0 }, color(t.panel_header));
    let line =
        |y: f32, w: f32, c| p.rect_filled(egui::Rect::from_min_size(card.min + egui::vec2(8.0, y), egui::vec2(w, 3.0)), 1.0, color(c));
    line(22.0, 44.0, t.text);
    line(30.0, 32.0, t.text_dim);
    let row = egui::Rect::from_min_size(card.min + egui::vec2(6.0, 38.0), egui::vec2(52.0, 9.0));
    p.rect_filled(row, 2.0, color(t.accent_soft));
    let button = egui::Rect::from_min_size(card.min + egui::vec2(66.0, 42.0), egui::vec2(22.0, 10.0));
    p.rect_filled(button, 2.0, color(t.accent));
    let outline = if selected {
        Stroke::new(2.0, ui.visuals().selection.stroke.color)
    } else if response.hovered() {
        Stroke::new(1.0, ui.visuals().widgets.hovered.fg_stroke.color)
    } else {
        Stroke::new(1.0, color(t.border))
    };
    p.rect_stroke(card, r, outline, egui::StrokeKind::Outside);
    let text = if selected { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
    p.text(egui::pos2(card.center().x, card.max.y + 4.0), egui::Align2::CENTER_TOP, name, egui::FontId::proportional(12.0), text);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Plex Sans regular for the text, semibold for headings, Plex Mono for monospace, each
/// followed by the key symbols, then egui's own fonts (symbols, emoji).
fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, weight) in [("plex-sans", 400.0), (SEMIBOLD, 600.0)] {
        let mut data = FontData::from_static(PLEX_SANS);
        data.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", weight)]);
        fonts.font_data.insert(name.to_owned(), Arc::new(data));
    }
    fonts.font_data.insert("plex-mono".to_owned(), Arc::new(FontData::from_static(PLEX_MONO)));
    fonts.font_data.insert("key-symbols".to_owned(), Arc::new(FontData::from_static(KEY_SYMBOLS)));
    let family = |first: &str, rest: &[String]| [first, "key-symbols"].into_iter().map(str::to_owned).chain(rest.iter().cloned()).collect();
    let mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    fonts.families.insert(FontFamily::Proportional, family("plex-sans", &fallbacks));
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), family(SEMIBOLD, &fallbacks));
    fonts.families.insert(FontFamily::Monospace, family("plex-mono", &mono));
    fonts
}

/// One style (light or dark) from its tokens.
fn apply(s: &mut egui::Style, t: &UiTokens) {
    if let Some(heading) = s.text_styles.get_mut(&TextStyle::Heading) {
        heading.family = FontFamily::Name(SEMIBOLD.into());
    }
    s.spacing.item_spacing = t.item_spacing.into();
    s.spacing.button_padding = t.button_padding.into();
    // Tips come quickly (egui waits 0.5 s), and once one is shown the next ones follow at once
    // while the pointer goes from widget to widget.
    s.interaction.tooltip_delay = 0.15;
    s.interaction.tooltip_grace_time = 0.5;

    let v = &mut s.visuals;
    let border = Stroke::new(1.0, color(t.border));
    let text = Stroke::new(1.0, color(t.text));
    v.panel_fill = color(t.panel);
    v.window_fill = color(t.panel);
    v.window_stroke = border;
    v.extreme_bg_color = color(t.field);
    v.faint_bg_color = color(t.panel_header);
    v.override_text_color = Some(color(t.text));
    v.weak_text_color = Some(color(t.text_dim));
    v.selection.bg_fill = color(t.accent_soft);
    v.selection.stroke = Stroke::new(1.0, color(t.accent));
    v.window_corner_radius = CornerRadius::same(t.radius_md);
    v.menu_corner_radius = CornerRadius::same(t.radius_md);
    // One short, soft shadow for the floating cards, the windows and the popups (egui's: offset
    // 10×20 blur 15 for windows, 6×10 blur 8 for popups): they sit on the view, not high above it.
    for shadow in [&mut v.window_shadow, &mut v.popup_shadow] {
        *shadow = Shadow { offset: [0, 2], blur: 6, spread: 0, color: shadow.color };
    }

    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = border;
    w.noninteractive.fg_stroke = text;
    w.inactive.weak_bg_fill = color(t.panel_header);
    w.inactive.bg_fill = color(t.panel_header);
    w.inactive.fg_stroke = text;
    w.hovered.weak_bg_fill = color(t.hover);
    w.hovered.bg_fill = color(t.hover);
    w.active.weak_bg_fill = color(t.accent_soft);
    // egui fills a movable window's active title bar with the open state's.
    w.open.weak_bg_fill = color(t.panel_header);
    // Every state, so that hovering or pressing does not bring back other corners.
    for state in [&mut w.noninteractive, &mut w.inactive, &mut w.active, &mut w.open] {
        state.corner_radius = CornerRadius::same(t.radius_sm / 2);
    }
    w.hovered.corner_radius = CornerRadius::same(t.radius_sm.saturating_sub(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_styles_follow_the_palette() {
        let ctx = egui::Context::default();
        for palette in [Palette::Warm, Palette::Graphite, Palette::Neutral] {
            install_palette(&ctx, palette);
            for (theme, mode) in [(egui::Theme::Light, Mode::Light), (egui::Theme::Dark, Mode::Dark)] {
                let (v, t) = (&ctx.style_of(theme).visuals, UiTokens::of(palette, mode));
                assert_eq!(v.panel_fill, color(t.panel));
                assert_eq!(v.text_color(), color(t.text));
                assert_eq!(v.weak_text_color(), color(t.text_dim));
                assert_eq!(v.selection.stroke.color, color(t.accent));
            }
        }
    }

    #[test]
    fn headings_are_semibold_plex() {
        let fonts = fonts();
        assert_eq!(fonts.families[&FontFamily::Proportional][0], "plex-sans");
        assert_eq!(fonts.families[&FontFamily::Monospace][0], "plex-mono");
        assert!(fonts.families[&FontFamily::Monospace].len() > 1, "keeps egui's fallbacks");
        assert!(fonts.families[&FontFamily::Name(SEMIBOLD.into())].len() > 1, "keeps egui's fallbacks");
        let mut style = egui::Style::default();
        apply(&mut style, &UiTokens::light());
        assert_eq!(style.text_styles[&TextStyle::Heading].family, FontFamily::Name(SEMIBOLD.into()));
    }

    #[test]
    fn every_family_has_the_key_symbols() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        let families = [FontFamily::Proportional, FontFamily::Monospace, FontFamily::Name(SEMIBOLD.into())];
        for family in families {
            let font = egui::FontId::new(13.0, family.clone());
            let missing: String = ctx.fonts_mut(|f| "⌘⌥⇧⌃⌫⌦⏎↩⇥←↑→↓".chars().filter(|c| !f.has_glyph(&font, *c)).collect());
            assert!(missing.is_empty(), "{family:?} lacks {missing}");
        }
    }
}

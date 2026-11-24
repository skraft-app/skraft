//! The rows of the trees and lists (docs/ui/interface.md#widgets): their line, their fold,
//! their name edited in place, the copy that follows a row dragged.

use skraft_model::FastSet;

use crate::icons;
use crate::style::CARD_MARGIN;

/// A row's fold: its key in its document's [`OutlinerState`](crate::panels::OutlinerState), open or not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fold {
    pub(crate) key: egui::Id,
    pub(crate) open: bool,
}

/// An outliner row's fold toggle, one indent wide: the panels' chevron (as in their title bars),
/// turning from right (folded) to down (open) as it opens; a click folds or unfolds the row (in
/// `folds`, its document's). `salt` (the document's outliner) keeps its click and turn apart
/// from another document's row of the same ids.
pub(crate) fn fold_toggle(ui: &mut egui::Ui, salt: egui::Id, fold: Fold, folds: &mut FastSet<egui::Id>) {
    let (_, rect) = ui.allocate_space(egui::vec2(ui.spacing().indent, ui.spacing().icon_width));
    let id = salt.with(fold.key);
    let response = ui.interact(rect, id, egui::Sense::click());
    let mut open = fold.open;
    if response.clicked() {
        open = !open;
        if open {
            folds.insert(fold.key);
        } else {
            folds.remove(&fold.key);
        }
        ui.ctx().request_repaint();
    }
    let openness = ui.ctx().animate_bool_responsive(id, open);
    let color = ui.style().interact(&response).fg_stroke.color;
    let center = egui::pos2(rect.left() + ui.spacing().indent / 2.0, rect.center().y);
    icons::chevron(ui, center, 14.0, color, -(1.0 - openness) * std::f32::consts::FRAC_PI_2);
}

/// The background of a part's bodies in the outliner: white in the light theme, in the dark one
/// the card's a shade towards the text.
pub(crate) fn band_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode { v.window_fill.lerp_to_gamma(v.text_color(), 0.07) } else { egui::Color32::WHITE }
}

/// The text and icons of a hidden row (itself or by an assembly above): the weak text halfway
/// to the row's background (`band`: a part's bodies'), well behind the rows shown.
pub(crate) fn hidden_color(v: &egui::Visuals, band: bool) -> egui::Color32 {
    let back = if band { band_color(v) } else { v.window_fill };
    v.weak_text_color().lerp_to_gamma(back, 0.5)
}

/// One line of the outliner across the whole list: its background — `band` that of a part's
/// bodies, then the selection, or the hover (`lit`: hovered in the view; its context menu open:
/// the row it acts on stays marked) — painted under the widgets `add` lays in it from left to
/// right. The response is the line's click; the buttons in it (a fold, an eye, a sign), added
/// after, take theirs first.
pub(crate) fn tree_row<R>(
    ui: &mut egui::Ui,
    height: f32,
    selected: bool,
    lit: bool,
    band: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> (egui::Response, R) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    // Edge to edge of the list, its indent included, and half the spacing above and below: the
    // band of a part's bodies unbroken, the hover covering the line's whole height.
    let gap = ui.spacing().item_spacing.y / 2.0;
    // Out to the card's border, across its margin: the list's width (a vertical scroll area
    // does not clip across), widened by the margin, painted past the list's clip.
    let x = ui.max_rect().x_range();
    let clip = egui::Rect::from_x_y_ranges(x, ui.clip_rect().y_range()).expand2(egui::vec2(CARD_MARGIN, 0.0));
    let back = egui::Rect::from_x_y_ranges(clip.x_range(), rect.y_range()).expand2(egui::vec2(0.0, gap));
    let v = ui.visuals();
    let fill = if selected {
        Some(v.selection.bg_fill)
    } else if lit || ui.rect_contains_pointer(back) || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response)) {
        Some(v.widgets.hovered.weak_bg_fill)
    } else if band {
        Some(band_color(v))
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().with_clip_rect(clip).rect_filled(back, 0.0, fill);
    }
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    let inner = add(&mut row);
    (response, inner)
}

/// What a tree's line shows before its own widgets: its [`tree_row`]'s height and marks, its
/// depth, its fold, its icon.
pub(crate) struct TreeLine<'a> {
    pub(crate) height: f32,
    pub(crate) selected: bool,
    pub(crate) lit: bool,
    pub(crate) band: bool,
    /// The indents before its fold.
    pub(crate) depth: usize,
    /// Its fold toggle; none: the room of one, so that the icons of a level line up.
    pub(crate) fold: Option<Fold>,
    pub(crate) icon: egui::Image<'a>,
}

/// A tree's line in its [`tree_row`]: its indents, its fold toggle (under `salt` in `folds`), its
/// icon, then what `trailing` lays at the right end, right to left (given whether the pointer is
/// over the line), and `label` in the room left, left to right. The row's response, and what
/// both gave.
pub(crate) fn tree_line<T, L>(
    ui: &mut egui::Ui,
    line: TreeLine<'_>,
    (salt, folds): (egui::Id, &mut FastSet<egui::Id>),
    trailing: impl FnOnce(&mut egui::Ui, bool) -> T,
    label: impl FnOnce(&mut egui::Ui) -> L,
) -> (egui::Response, (T, L)) {
    let TreeLine { height, selected, lit, band, depth, fold, icon } = line;
    tree_row(ui, height, selected, lit, band, |ui| {
        let spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(ui.spacing().indent * depth as f32);
        match fold {
            Some(fold) => fold_toggle(ui, salt, fold, folds),
            None => ui.add_space(ui.spacing().indent),
        }
        ui.spacing_mut().item_spacing.x = spacing;
        ui.add(icon);
        let over = ui.rect_contains_pointer(ui.max_rect());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let trailing = trailing(ui, over);
            let label = ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), label).inner;
            (trailing, label)
        })
        .inner
    })
}

/// What follows the pointer while a navigator's row is dragged (docs/ui/interface.md#navigator):
/// its icon and name, how many go with it.
#[derive(Clone)]
struct RowGhost {
    icon: egui::ImageSource<'static>,
    name: String,
    count: usize,
}

/// The key of the row being dragged, in the context's memory: the row itself may scroll out of
/// the list, which then no longer draws it.
const ROW_GHOST: &str = "navigator_row_ghost";

/// A row was picked up: its translucent copy, `count` elements going with it, follows the pointer.
pub(crate) fn set_drag_ghost(ctx: &egui::Context, icon: egui::ImageSource<'static>, name: String, count: usize) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(ROW_GHOST), RowGhost { icon, name, count }));
}

/// The copy of the row being dragged under the pointer, over everything and never in the way of
/// what is under it; forgotten once nothing is dragged.
pub(crate) fn paint_drag_ghost(ctx: &egui::Context) {
    let key = egui::Id::unique(ROW_GHOST);
    if !egui::DragAndDrop::has_any_payload(ctx) {
        ctx.data_mut(|d| d.remove::<RowGhost>(key));
        return;
    }
    let (Some(ghost), Some(at)) = (ctx.data(|d| d.get_temp::<RowGhost>(key)), ctx.input(|i| i.pointer.latest_pos())) else { return };
    egui::Area::new(key).order(egui::Order::Tooltip).interactable(false).fixed_pos(at + egui::vec2(12.0, 10.0)).show(ctx, |ui| {
        ui.multiply_opacity(0.75);
        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
            ui.set_max_width(240.0);
            ui.horizontal(|ui| {
                let ink = ui.visuals().text_color();
                ui.add(egui::Image::new(ghost.icon.clone()).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(ink));
                ui.add(egui::Label::new(egui::RichText::new(&ghost.name).color(ink)).selectable(false).truncate());
                if ghost.count > 1 {
                    ui.label(egui::RichText::new(format!("+{}", ghost.count - 1)).color(ui.visuals().weak_text_color()));
                }
            });
        });
    });
}

/// The temporary key of the body row whose name is edited in place, if any.
pub(crate) const RENAMING: &str = "outliner_renaming";

/// A navigator row's name edited in place, under the key `rename` (`renaming`: it is): a
/// double-click on the row (`response`), or F2 while it is the one thing selected (`alone`),
/// opens its field; `renamed`, the field's outcome, closes it and is given back.
pub(crate) fn start_rename(
    ui: &mut egui::Ui,
    response: &egui::Response,
    renaming: bool,
    alone: bool,
    rename: egui::Id,
    renamed: Option<Option<String>>,
) -> Option<Option<String>> {
    if renamed.is_some() {
        ui.data_mut(|d| d.remove::<egui::Id>(egui::Id::unique(RENAMING)));
    } else if !renaming {
        let f2 = alone && ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(egui::Key::F2));
        if response.double_clicked() || f2 {
            ui.data_mut(|d| d.insert_temp(egui::Id::unique(RENAMING), rename));
        }
    }
    renamed
}

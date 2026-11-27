//! The choices among values listed (docs/ui/interface.md#widgets): a drop-down list, and the
//! row of a value in it, in a menu or in a list drawn by hand. Side by side, a few values are
//! a [`super::segmented`] control instead.

use std::hash::Hash;

/// A value of a list to choose from, `on` when it is the one chosen: the row lit. Its response
/// (clicked: chosen, or chosen again).
pub(crate) fn choice_item(ui: &mut egui::Ui, on: bool, label: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::selectable(on, label))
}

/// A drop-down list: the value shown in its field, the kit's chevron at its right end, its
/// values listed under it when clicked.
pub(crate) struct ChoiceList<S> {
    salt: S,
    shown: egui::WidgetText,
    width: Option<f32>,
    keep_open: bool,
}

impl<S: Hash + std::fmt::Debug> ChoiceList<S> {
    /// Under the key `salt`, showing `shown` (the value chosen, or what stands for it).
    pub(crate) fn new(salt: S, shown: impl Into<egui::WidgetText>) -> Self {
        ChoiceList { salt, shown: shown.into(), width: None, keep_open: false }
    }

    /// Its field's width; egui's default else.
    pub(crate) fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Open until a click outside it, not closed by a value clicked: a list of check boxes,
    /// several ticked in a row.
    pub(crate) fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    /// Laid out in `ui`, its values added by `items` when it is open: its field's response.
    pub(crate) fn show(self, ui: &mut egui::Ui, items: impl FnOnce(&mut Choices<'_>)) -> egui::Response {
        let mut combo = egui::ComboBox::from_id_salt(self.salt).icon(crate::kit::icons::combo_chevron).selected_text(self.shown);
        if let Some(w) = self.width {
            combo = combo.width(w);
        }
        if self.keep_open {
            combo = combo.close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        }
        combo.show_ui(ui, |ui| items(&mut Choices { ui })).response
    }
}

/// The values of an open [`ChoiceList`].
pub(crate) struct Choices<'u> {
    ui: &'u mut egui::Ui,
}

impl Choices<'_> {
    /// A value, `on` when it is the one chosen: clicked?
    pub(crate) fn item(&mut self, on: bool, label: impl Into<egui::WidgetText>) -> bool {
        choice_item(self.ui, on, label).clicked()
    }

    /// The value `value` of `current`, chosen into it when clicked: changed?
    pub(crate) fn value<T: PartialEq>(&mut self, current: &mut T, value: T, label: impl Into<egui::WidgetText>) -> bool {
        let clicked = self.item(*current == value, label);
        let changed = clicked && *current != value;
        if clicked {
            *current = value;
        }
        changed
    }

    /// A check box, `under` the one above it (indented, enabled only while that one is on: a
    /// kind among another's): changed?
    pub(crate) fn check(&mut self, on: &mut bool, label: impl Into<egui::WidgetText>, under: Option<bool>) -> bool {
        match under {
            None => self.ui.checkbox(on, label).changed(),
            Some(enabled) => {
                self.ui
                    .horizontal(|ui| {
                        ui.add_space(ui.spacing().indent);
                        ui.add_enabled(enabled, egui::Checkbox::new(on, label)).changed()
                    })
                    .inner
            }
        }
    }

    /// A line between two groups of values.
    pub(crate) fn separator(&mut self) {
        self.ui.separator();
    }
}

/// A choice among `options` (none: « — »), framed as the fields, `width` wide: the one picked,
/// when it changed.
pub(crate) fn choice_field(ui: &mut egui::Ui, id: egui::Id, current: Option<&str>, options: &[String], width: f32) -> Option<String> {
    ui.vertical(|ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, super::OPTION_HEIGHT), egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let visuals = ui.visuals();
        let stroke = super::field_stroke(visuals, open, false, response.hovered());
        let p = ui.painter();
        p.rect(rect, visuals.widgets.inactive.corner_radius, visuals.extreme_bg_color, stroke, egui::StrokeKind::Inside);
        let ink = visuals.text_color();
        let inner = rect.shrink2(egui::vec2(6.0, 0.0));
        let icon = egui::Rect::from_center_size(egui::pos2(inner.right() - 5.0, inner.center().y), egui::vec2(14.0, 14.0));
        crate::kit::icons::chevron(ui, icon.center(), 14.0, ink, 0.0);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let text = ui.fonts_mut(|f| f.layout_no_wrap(current.unwrap_or("—").to_owned(), font, ink));
        let clip = inner.with_max_x(icon.left() - 4.0);
        ui.painter_at(clip).galley(egui::pos2(inner.left(), inner.center().y - text.size().y / 2.0), text, ink);
        let mut picked = None;
        egui::Popup::menu(&response).id(id).width(width).style(crate::kit::style::menu).show(|ui| {
            ui.set_min_width(ui.available_width());
            for o in options {
                if choice_item(ui, current == Some(o.as_str()), o).clicked() && current != Some(o.as_str()) {
                    picked = Some(o.clone());
                }
            }
        });
        picked
    })
    .inner
}

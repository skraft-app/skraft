//! The choices among values listed (docs/ui/interface.md#widgets): a drop-down list, and the
//! row of a value in it, in a menu or in a list drawn by hand. Side by side, a few values are
//! a [`super::segmented`] control instead.

use std::hash::Hash;

/// A value of a list to choose from, `on` when it is the one chosen: the row lit. Its response
/// (clicked: chosen, or chosen again).
pub(crate) fn choice_item(ui: &mut egui::Ui, on: bool, label: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::selectable(on, label))
}

/// A mark drawn in its square before a value (a material's swatch).
type Lead<'a> = Box<dyn Fn(&egui::Ui, egui::Rect) + 'a>;

/// The one drop-down list: framed as the fields (their ground, their outline telling hovered
/// and open, their height), the value shown in it (« — » for none), the kit's chevron at its
/// right end, a mark before it when it has one; its values listed under it in a menu of the
/// app's style, at least as wide.
pub(crate) struct ChoiceList<'a, S> {
    salt: S,
    shown: String,
    width: Option<f32>,
    keep_open: bool,
    lead: Option<Lead<'a>>,
}

impl<'a, S: Hash + std::fmt::Debug> ChoiceList<'a, S> {
    /// Under the key `salt`, showing `shown` (the value chosen, or what stands for it).
    pub(crate) fn new(salt: S, shown: impl Into<String>) -> Self {
        ChoiceList { salt, shown: shown.into(), width: None, keep_open: false, lead: None }
    }

    /// A mark before the value shown, drawn by `paint` in its square (a material's swatch), as
    /// its row's [`Choices::item_lead`] in the list.
    pub(crate) fn lead(mut self, paint: impl Fn(&egui::Ui, egui::Rect) + 'a) -> Self {
        self.lead = Some(Box::new(paint));
        self
    }

    /// Its width; as wide as the value shown else (egui's lists' width at least).
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
        const PAD: f32 = 6.0;
        const CHEVRON: f32 = 14.0;
        let id = ui.make_persistent_id(self.salt);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let shown = if self.shown.is_empty() { "—".to_owned() } else { self.shown };
        let ink = ui.visuals().text_color();
        let text = ui.fonts_mut(|f| f.layout_no_wrap(shown, font, ink));
        let natural = (PAD + text.size().x + 4.0 + CHEVRON + PAD).max(ui.spacing().combo_width);
        let width = self.width.unwrap_or(natural);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, super::OPTION_HEIGHT), egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let visuals = ui.visuals();
        let stroke = super::field_stroke(visuals, open, false, response.hovered());
        ui.painter().rect(rect, visuals.widgets.inactive.corner_radius, visuals.extreme_bg_color, stroke, egui::StrokeKind::Inside);
        let inner = rect.shrink2(egui::vec2(PAD, 0.0));
        let chevron = egui::pos2(inner.right() - CHEVRON / 2.0 + 2.0, inner.center().y);
        crate::kit::icons::chevron(ui, chevron, CHEVRON, ink, 0.0);
        let mut left = inner.left();
        if let Some(paint) = &self.lead {
            let side = crate::kit::style::MENU_MARK;
            paint(ui, egui::Rect::from_min_size(egui::pos2(left, inner.center().y - side / 2.0), egui::Vec2::splat(side)));
            left += side + ui.spacing().icon_spacing;
        }
        let clip = inner.with_max_x(chevron.x - CHEVRON / 2.0 - 4.0);
        ui.painter_at(clip).galley(egui::pos2(left, inner.center().y - text.size().y / 2.0), text, ink);
        let close = if self.keep_open { egui::PopupCloseBehavior::CloseOnClickOutside } else { egui::PopupCloseBehavior::CloseOnClick };
        egui::Popup::menu(&response).id(id).close_behavior(close).style(crate::kit::style::menu).show(|ui| {
            ui.set_min_width(width);
            let mut list = Choices { ui, hovered: None, chosen: None };
            items(&mut list);
            // The note under the values, as the ribbon's menus have (`pin_list`): it wraps
            // within their width, never widening the list.
            if let Some(help) = list.hovered.or(list.chosen) {
                let width = ui.min_rect().width();
                ui.separator();
                ui.set_max_width(width);
                ui.add(egui::Label::new(crate::kit::style::caption(help)).wrap());
            }
        });
        response
    }
}

/// The values of an open [`ChoiceList`].
pub(crate) struct Choices<'u> {
    ui: &'u mut egui::Ui,
    /// The help of the value under the pointer, and of the value chosen ([`Choices::option`]).
    hovered: Option<String>,
    chosen: Option<String>,
}

impl Choices<'_> {
    /// A value, `on` when it is the one chosen: clicked?
    pub(crate) fn item(&mut self, on: bool, label: impl Into<egui::WidgetText>) -> bool {
        choice_item(self.ui, on, label).clicked()
    }

    /// A value, `on` when it is the one chosen, greyed unless `enabled`; what it does (or why it
    /// is greyed), `help`, in the note under the values while the pointer is on it — the chosen
    /// one's else —, as the ribbon's menus have theirs: no tip over the next values. Clicked?
    pub(crate) fn option(&mut self, on: bool, enabled: bool, label: &str, help: &str) -> bool {
        let r = self.ui.add_enabled(enabled, egui::Button::selectable(on, label));
        if !help.is_empty() {
            if r.contains_pointer() {
                self.hovered = Some(help.to_owned());
            }
            if on {
                self.chosen = Some(help.to_owned());
            }
        }
        r.clicked()
    }

    /// A value with a mark before its label (`style::menu_mark`: a material's swatch), where the
    /// menus' check boxes go, the labels lined up; `on` when it is the one chosen: clicked?
    pub(crate) fn item_lead(&mut self, on: bool, lead: egui::Atom<'_>, label: impl Into<String>) -> bool {
        self.ui.add(crate::kit::style::menu_item_lead(lead, label).selected(on)).clicked()
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

/// A [`ChoiceList`] among the texts `options` (none: « — »), `width` wide: the one picked, when
/// it changed.
pub(crate) fn choice_field(ui: &mut egui::Ui, id: egui::Id, current: Option<&str>, options: &[String], width: f32) -> Option<String> {
    let mut picked = None;
    ChoiceList::new(id, current.unwrap_or_default()).width(width).show(ui, |list| {
        for o in options {
            if list.item(current == Some(o.as_str()), o) && current != Some(o.as_str()) {
                picked = Some(o.clone());
            }
        }
    });
    picked
}

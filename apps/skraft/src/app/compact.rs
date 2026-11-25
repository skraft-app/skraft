//! The compact arrangement of a phone (docs/ui/touch.md#the-compact-arrangement): at the bottom
//! of the window the ribbon in one strip scrolled sideways and the cards' tabs; a card risen
//! from the bottom over the view, one at a time.

use super::layout::{CardEvent, Edges};
use super::*;

/// What the compact arrangement keeps for the session: the card risen and its share of the
/// stage's height (none: half), where the bars and the card begin (for the toasts above them).
#[derive(Default)]
pub(super) struct Compact {
    pub(super) card: Option<PanelId>,
    pub(super) share: Option<f32>,
    pub(super) top: Option<f32>,
}

/// A card's share of the stage's height: risen, at its full height; dragged under the least, it
/// goes down when let go.
const HALF: f32 = 0.5;
const LEAST: f32 = 0.2;

/// The cards' tabs' height: a finger's.
const TABS_HEIGHT: f32 = 44.0;
/// A card's tab's narrowest, in the title bar.
const TAB_MIN_WIDTH: f32 = 96.0;
/// The cards, in their tabs' order.
const CARDS: [PanelId; 4] = [PanelId::Navigator, PanelId::EntityInfo, PanelId::Materials, PanelId::Library];

impl SkraftApp {
    /// The bars laid out before the stage, which takes the room between them: the ribbon's strip
    /// under the title bar, the cards' tabs at the bottom (the measurements box above them while it
    /// takes a value). Gives the top of those at the bottom.
    /// Held sideways, the cards' tabs go to the room `bar_room` the title bar leaves, when they
    /// fit there: the view keeps its height.
    pub(super) fn compact_bars(
        &mut self,
        ui: &mut egui::Ui,
        bar_room: Option<egui::Rect>,
        views: &[CommandView],
        actions: &mut Vec<Action>,
    ) -> f32 {
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        // The ribbon under the title bar, where it is on the desktop: from edge to edge, at least
        // the window's width, its card's margins inside what scrolls (its tab band reaches into
        // them, as on the desktop).
        let style = ctx.global_style();
        let card = crate::kit::style::card_frame(&style).corner_radius(0).stroke(egui::Stroke::NONE).shadow(egui::Shadow::NONE);
        let inset = card.inner_margin;
        let panel = egui::Frame::side_top_panel(&style).inner_margin(0);
        egui::Panel::top("compact_ribbon").resizable(false).frame(panel).show(ui, |ui| {
            let seen = ui.available_width();
            let view = ribbon::View { workspace: self.workspace(), prefs: &self.prefs.ribbon, views, seen: Some(seen - inset.sum().x) };
            let width = ribbon::natural_width(ui.ctx(), &view).max(seen - inset.sum().x);
            // Dragged from the first touch on (egui's default waits to have seen a touch screen).
            egui::ScrollArea::horizontal()
                .id_salt("compact_ribbon")
                .scroll_source(egui::scroll_area::ScrollSource::ALL)
                .show(ui, |ui| card.show(ui, |ui| ribbon::ribbon(ui, &view, width, inset, actions)));
        });
        // The cards the workspace shows (docs/ui/workspaces.md#panels): none, no tabs.
        let cards = self.compact_cards();
        if self.compact.card.is_some_and(|c| !cards.contains(&c)) {
            self.compact.card = None;
        }
        let in_bar = bar_room.filter(|r| screen.width() > screen.height() && r.width() >= cards.len() as f32 * TAB_MIN_WIDTH);
        // The top of what is at the bottom, for the toasts above it.
        let mut bottom = screen.bottom();
        match in_bar {
            _ if cards.is_empty() => {}
            Some(room) => {
                egui::Area::new(egui::Id::unique("compact_cards_in_bar")).fixed_pos(room.min).order(egui::Order::Foreground).show(
                    &ctx,
                    |ui| {
                        let mut style = ui.style().as_ref().clone();
                        crate::kit::style::title_bar(&ctx, &mut style);
                        ui.set_style(style);
                        ui.allocate_space(room.size());
                        self.card_tabs(ui, &cards, room);
                    },
                );
            }
            None => {
                let panel = egui::Panel::bottom("compact_cards").exact_size(TABS_HEIGHT).resizable(false).show(ui, |ui| {
                    let row = ui.max_rect();
                    self.card_tabs(ui, &cards, row)
                });
                bottom = panel.response.rect.top();
            }
        }
        // The measurements box while the tool takes a typed value; the tool's hint is the
        // status bar's, which is not shown.
        if self.vcb_takes() {
            let panel = egui::Panel::bottom("compact_vcb").exact_size(TABS_HEIGHT).resizable(false).show(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(status) = self.vcb_box(ui) {
                        self.driving.message(&status);
                    }
                })
            });
            bottom = panel.response.rect.top();
        }
        bottom
    }

    /// The room the title bar keeps for the cards' tabs (held sideways, a document shown), none
    /// otherwise.
    pub(super) fn compact_tabs_reserve(&self, ctx: &egui::Context) -> f32 {
        let screen = ctx.content_rect();
        let sideways = self.form == crate::window::form::Form::Compact && screen.width() > screen.height();
        if sideways && self.docs.active().is_some() { self.compact_cards().len() as f32 * TAB_MIN_WIDTH } else { 0.0 }
    }

    /// The cards of the workspace shown, in their tabs' order.
    fn compact_cards(&self) -> Vec<PanelId> {
        let shown = self.workspace().panels;
        CARDS.into_iter().filter(|p| shown.is_none_or(|s| s.contains(p))).collect()
    }

    /// The cards' tabs in `row`, as wide as each other: a tap raises its card, again lowers it.
    fn card_tabs(&mut self, ui: &mut egui::Ui, cards: &[PanelId], row: egui::Rect) {
        let width = row.width() / cards.len() as f32;
        for (k, &id) in cards.iter().enumerate() {
            let rect = egui::Rect::from_min_size(egui::pos2(row.left() + k as f32 * width, row.top()), egui::vec2(width, row.height()));
            let lit = self.compact.card == Some(id);
            let r = ui.interact(rect, egui::Id::unique(("compact_tab", id)), egui::Sense::click());
            crate::kit::handles::publish(|| format!("compact_tab:{}", id.command()), rect);
            let visuals = ui.visuals();
            if lit {
                ui.painter().rect_filled(rect.shrink(4.0), 6.0, visuals.selection.bg_fill);
            }
            let ink = if lit { visuals.selection.stroke.color } else { visuals.text_color() };
            // On one line: smaller when the tab is too narrow for it.
            let mut font = egui::TextStyle::Body.resolve(ui.style());
            let room = rect.width() - 8.0;
            let natural = ui.painter().layout_no_wrap(tab_label(id), font.clone(), ink).size().x;
            if natural > room {
                font.size *= room / natural;
            }
            let text = ui.painter().layout_no_wrap(tab_label(id), font, ink);
            ui.painter().galley(rect.center() - text.size() / 2.0, text, ink);
            if r.clicked() {
                self.compact.card = if lit { None } else { Some(id) };
                // Raised again at half the stage.
                if !lit && self.compact.share.is_some_and(|s| s < LEAST) {
                    self.compact.share = None;
                }
            }
        }
    }

    /// The card risen from the bottom of `area`, over the view: half its height, or all of it.
    /// Gives its top.
    pub(super) fn compact_card(&mut self, ctx: &egui::Context, area: egui::Rect, margin: f32, actions: &mut Vec<Action>) -> Option<f32> {
        let id = self.compact.card?;
        let room = area.height() - 2.0 * margin;
        let share = self.compact.share.unwrap_or(HALF).clamp(0.0, 1.0);
        let height = (share * room).max(TABS_HEIGHT * 2.0).min(room);
        let full = share > 0.75;
        let rect = egui::Rect::from_min_max(
            egui::pos2(area.left() + margin, area.bottom() - margin - height),
            egui::pos2(area.right() - margin, area.bottom() - margin),
        );
        let header = ctx.global_style().spacing.interact_size.y;
        let edges = Edges { side: None, bottom: false, sheet: Some(full), max_width: rect.width(), min_height: 0.0 };
        let mut navigator_shown = false;
        match self.panel_card(ctx, id, rect, header, edges, &mut navigator_shown, actions) {
            // Tapped: to its full height, or back to half.
            Some(CardEvent::Tapped) => self.compact.share = Some(if full { HALF } else { 1.0 }),
            // Its header follows the finger; let go low, it goes down.
            Some(CardEvent::Pulled(dy)) => self.compact.share = Some(share - dy / room.max(1.0)),
            Some(CardEvent::Let) if share < LEAST => {
                self.compact.card = None;
                self.compact.share = None;
            }
            _ => {}
        }
        if !navigator_shown {
            actions.push(Action::HoverNavigator(None));
            actions.push(Action::HoverMachining(None));
        }
        Some(rect.top())
    }
}

/// A card's tab: its title, short.
fn tab_label(id: PanelId) -> String {
    match id {
        PanelId::EntityInfo => t!("compact.entity_info"),
        _ => layout::panel_title(id),
    }
}

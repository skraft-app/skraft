//! The interface's zoom, shown when it changes (docs/ui/interface.md#interface-zoom): egui
//! zooms the whole interface on ⌘+, ⌘− and ⌘0 (Ctrl elsewhere); its new value is shown at the
//! center of the window for a moment, then fades.

use skraft_i18n::t;

use crate::kit::style;

/// How long the value stays fully shown, then how long it takes to fade (seconds).
const HOLD: f64 = 0.8;
const FADE: f64 = 0.3;
const RADIUS: u8 = 8;

#[derive(Default)]
pub struct ZoomLabel {
    /// The zoom of the last frame; none before the first (the zoom restored at launch is not
    /// shown).
    last: Option<f32>,
    /// When the zoom last changed (egui's time).
    changed: Option<f64>,
}

impl ZoomLabel {
    pub fn show(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        let now = ctx.input(|i| i.time);
        if self.last.is_some_and(|last| last != zoom) {
            self.changed = Some(now);
        }
        self.last = Some(zoom);
        let Some(changed) = self.changed else { return };
        let age = now - changed;
        if age >= HOLD + FADE {
            self.changed = None;
            return;
        }
        let alpha = if age <= HOLD { 1.0 } else { (1.0 - (age - HOLD) / FADE) as f32 };
        egui::Area::new(egui::Id::unique("zoom_label"))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                egui::Frame::popup(ui.style()).corner_radius(RADIUS).inner_margin(egui::Margin::symmetric(18, 10)).show(ui, |ui| {
                    let percent = (zoom * 100.0).round() as i32;
                    // One line whatever the zoom: the area's width is last frame's.
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(t!("window.zoom", percent = percent))
                                .font(style::semibold(24.0))
                                .color(ui.visuals().strong_text_color()),
                        )
                        .extend(),
                    );
                });
            });
        ctx.request_repaint();
    }
}

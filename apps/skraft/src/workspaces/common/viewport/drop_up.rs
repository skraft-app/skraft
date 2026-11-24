//! OpenCutList's tree drop-up (docs/tools/selection.md): the path of the part under
//! the cursor, from the model down to the part, listed while the copy modifier is held — its
//! bottom right corner on the cursor; a row hovered is drawn hovered in the view, a row
//! clicked selected. The editor keeps the list ([`skraft_tools::Tree`]); this draws it.

use egui::{Rect, vec2};
use skraft_tools::{Tree, TreeRow};

use crate::commands::Action;
use crate::workspaces::common::instance_name::{self, NameInk};

/// The indent per level of the path (points).
const INDENT: f32 = 6.0;
/// A row's height, and its text's side margin (the ribbon's menus').
const ROW_HEIGHT: f32 = 22.0;
const ROW_PAD: f32 = 8.0;

/// The list, its bottom right corner on `at` (the cursor, screen points), kept inside `view`.
/// Every row as wide as the list (the ribbon's menus), its name as [`instance_name`] writes it,
/// in the strong ink — greyed for the levels that cannot be chosen.
pub fn show(ctx: &egui::Context, view: Rect, at: egui::Pos2, tree: &Tree, actions: &mut Vec<Action>) {
    let mut hovered = None;
    egui::Area::new(egui::Id::unique("tree_drop_up"))
        .fixed_pos(at)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .constrain_to(view)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let size = egui::TextStyle::Body.resolve(ui.style()).size;
                let (strong, weak) = (ui.visuals().strong_text_color(), ui.visuals().weak_text_color());
                let ink = |r: &TreeRow| if r.enabled { strong } else { weak };
                let galleys: Vec<_> = tree
                    .rows
                    .iter()
                    .map(|r| {
                        ui.painter().layout_job(instance_name::job(&r.name, &r.component, r.auto, NameInk { size, ink: ink(r), weak }))
                    })
                    .collect();
                let width =
                    tree.rows.iter().zip(&galleys).map(|(r, g)| r.depth as f32 * INDENT + g.size().x).fold(0.0, f32::max) + 2.0 * ROW_PAD;
                for (k, (row, galley)) in tree.rows.iter().zip(galleys).enumerate() {
                    let sense = if row.enabled { egui::Sense::click() } else { egui::Sense::hover() };
                    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), sense);
                    crate::kit::handles::publish_enabled(|| format!("drop_up:{k}"), rect, row.enabled);
                    let lit = row.enabled && (response.hovered() || tree.hovered == Some(k));
                    if lit {
                        let w = &ui.visuals().widgets.hovered;
                        ui.painter().rect_filled(rect, w.corner_radius, w.weak_bg_fill);
                    }
                    let x = rect.left() + ROW_PAD + row.depth as f32 * INDENT;
                    ui.painter().galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, ink(row));
                    if row.enabled && response.hovered() {
                        hovered = Some(k);
                    }
                    if row.enabled && response.clicked() {
                        actions.push(Action::TreePick(k));
                    }
                }
            });
        });
    if hovered != tree.hovered {
        actions.push(Action::TreeHover(hovered));
    }
}

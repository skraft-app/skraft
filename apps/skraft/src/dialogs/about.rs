//! The « À propos » window (docs/ui/interface.md#about): the version and the notices the GPL
//! asks an interactive program to show, with where to read the licenses and the source.

use skraft_i18n::t;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY: &str = "https://github.com/skraft-app/skraft";

/// The source of this very version: its release tag.
fn source_url() -> String {
    format!("{REPOSITORY}/tree/v{VERSION}")
}

fn license_url() -> String {
    format!("{REPOSITORY}/blob/v{VERSION}/LICENSE")
}

/// The third-party notices of this version among its release's files: where a desktop build
/// without them beside it reads them.
#[cfg(not(target_arch = "wasm32"))]
fn notices_url() -> String {
    format!("{REPOSITORY}/releases/download/v{VERSION}/THIRD-PARTY-LICENSES.txt")
}

/// The third-party notices an installer put beside the app (`scripts/package-*.sh`). None in a
/// build run from the sources.
#[cfg(not(target_arch = "wasm32"))]
fn notices_path() -> Option<std::path::PathBuf> {
    const NAME: &str = "THIRD-PARTY-LICENSES.txt";
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("../Resources").join(NAME), dir.join(NAME), dir.join("../share/doc/skraft").join(NAME)].into_iter().find(|p| p.is_file())
}

/// A link opening `url` in the browser.
fn link(ui: &mut egui::Ui, label: String, url: String) {
    if ui.link(label).clicked() {
        crate::app::open_url(ui.ctx(), url);
    }
}

/// The projects Skraft stands on, each with its project's page, what it does here and its
/// license. The full list is in the third-party notices.
fn credits() -> [(&'static str, &'static str, String, &'static str); 7] {
    [
        ("SolidCore", "https://github.com/tangency-labs/solidcore", t!("about.role.kernel"), "Apache-2.0"),
        ("skpwright", "https://github.com/hew3d/skpwright", t!("about.role.sketchup"), "GPL-3.0"),
        ("SolidCraft", "https://github.com/tangency-labs/solidcraft", t!("about.role.adapted"), "MIT"),
        ("egui", "https://github.com/emilk/egui", t!("about.role.interface"), "MIT / Apache-2.0"),
        ("wgpu", "https://github.com/gfx-rs/wgpu", t!("about.role.display"), "MIT / Apache-2.0"),
        ("IBM Plex", "https://github.com/IBM/plex", t!("about.role.font"), "OFL-1.1"),
        ("DejaVu", "https://dejavu-fonts.github.io", t!("about.role.symbols"), "Bitstream Vera"),
    ]
}

/// The credits' table, its three columns of fixed widths.
fn credits_table(ui: &mut egui::Ui) {
    const COLUMNS: [f32; 3] = [96.0, 136.0, 124.0];
    const GAP: f32 = 12.0;
    let width = COLUMNS.iter().sum::<f32>() + 2.0 * GAP;
    ui.allocate_ui_with_layout(egui::vec2(width, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
        egui::Grid::new("about_credits").num_columns(3).spacing([GAP, 3.0]).show(ui, |ui| {
            for (name, url, role, license) in credits() {
                ui.scope(|ui| {
                    ui.set_min_width(COLUMNS[0]);
                    link(ui, name.to_owned(), url.to_owned());
                });
                ui.scope(|ui| {
                    ui.set_min_width(COLUMNS[1]);
                    ui.label(role);
                });
                ui.scope(|ui| {
                    ui.set_min_width(COLUMNS[2]);
                    ui.weak(license);
                });
                ui.end_row();
            }
        });
    });
}

/// The license, the third-party licenses and the source, on one line.
fn links_row(ui: &mut egui::Ui) {
    let (license, notices, source) = (t!("about.license"), t!("about.third_party"), t!("about.source"));
    let font = egui::TextStyle::Body.resolve(ui.style());
    let measure = |text: &str| ui.painter().layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::PLACEHOLDER).size();
    let dot = measure(" · ");
    let width = measure(&license).x + measure(&notices).x + measure(&source).x + 2.0 * dot.x;
    ui.allocate_ui_with_layout(egui::vec2(width, dot.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        link(ui, license, license_url());
        ui.weak(" · ");
        #[cfg(target_arch = "wasm32")]
        link(ui, notices, "THIRD-PARTY-LICENSES.txt".to_owned());
        #[cfg(not(target_arch = "wasm32"))]
        if ui.link(notices).clicked() {
            match notices_path() {
                Some(path) => crate::app::open_file(&path.display().to_string()),
                None => crate::app::open_url(ui.ctx(), notices_url()),
            }
        }
        ui.weak(" · ");
        link(ui, source, source_url());
    });
}

/// The window, while `open`. Whether the user asks to look for a new version.
pub fn show(ctx: &egui::Context, open: &mut bool) -> bool {
    let mut check_updates = false;
    if !*open {
        return check_updates;
    }
    egui::Window::new(t!("menu.app.about", app = t!("start.title")))
        .id(egui::Id::unique("about"))
        .title_frame(crate::kit::style::window_title_frame(ctx))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_width(420.0);
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                // The app's mark: its tile fills 824 of the icon's 1024 units.
                const MARK: f32 = 60.0;
                const TILE: f32 = 824.0 / 1024.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(MARK, MARK), egui::Sense::hover());
                egui::Image::new(crate::kit::icons::APP)
                    .paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(MARK / TILE)));
                ui.add_space(10.0);
                ui.heading(egui::RichText::new(t!("start.title")).size(22.0).strong());
                ui.weak(t!("about.version", version = VERSION));
                // The check for a new version (docs/ui/updates.md; desktop): it opens its window.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.add_space(6.0);
                    let r = ui.button(skraft_i18n::ellipsis(t!("about.check_updates")));
                    crate::kit::handles::publish(|| "about:check_updates".to_owned(), r.rect);
                    if r.clicked() {
                        check_updates = true;
                    }
                }
                ui.add_space(8.0);
                ui.label(t!("about.tagline"));
                ui.add_space(12.0);
                ui.label(t!("about.copyright"));
                ui.add_space(4.0);
                ui.small(t!("about.notice"));
                ui.add_space(12.0);
                ui.strong(t!("about.built_with"));
                ui.add_space(4.0);
                credits_table(ui);
                ui.add_space(6.0);
                link(ui, t!("about.inspired"), "https://github.com/lairdubois/lairdubois-opencutlist-sketchup-extension".to_owned());
                ui.add_space(12.0);
                links_row(ui);
                ui.add_space(6.0);
            });
        });
    check_updates
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_links_name_this_version_tag() {
        let tag = format!("/v{}", super::VERSION);
        assert!(super::source_url().ends_with(&format!("/tree{tag}")));
        assert!(super::license_url().ends_with(&format!("/blob{tag}/LICENSE")));
        #[cfg(not(target_arch = "wasm32"))]
        assert!(super::notices_url().ends_with(&format!("/releases/download{tag}/THIRD-PARTY-LICENSES.txt")));
    }
}

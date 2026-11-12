//! SolidSketch, the application: egui panels around a viewport drawn by `solidsketch-render`,
//! driven by the `solidsketch-tools` editor. Native and web (docs/ARCHITECTURE.md §6 bis).

mod app;
mod commands;
#[cfg(not(target_arch = "wasm32"))]
mod control;
mod dock;
mod file;
mod icons;
#[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
mod native_menu;
mod panels;
mod prefs;
mod style;
mod titlebar;
mod viewport;
#[cfg(target_arch = "wasm32")]
mod worker;

pub use app::SolidSketchApp;

/// The bar is the window's title bar: on the macOS desktop (docs/ARCHITECTURE.md §7 quinquies).
pub(crate) const INTEGRATED_TITLEBAR: bool = cfg!(all(target_os = "macos", not(target_arch = "wasm32")));

/// The desktop window. On macOS the OS title bar is hidden and the content runs under it, the
/// traffic lights kept: the app's bar ([`titlebar`]) takes its place.
#[cfg(not(target_arch = "wasm32"))]
pub fn native_options() -> eframe::NativeOptions {
    let mut viewport =
        egui::ViewportBuilder::default().with_title("SolidSketch").with_inner_size([1280.0, 800.0]).with_min_inner_size([640.0, 400.0]);
    // The app icon (window, taskbar, macOS Dock), rendered from assets/app-icon.svg.
    match eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png")) {
        Ok(icon) => viewport = viewport.with_icon(std::sync::Arc::new(icon)),
        Err(error) => log::warn!("app icon: {error}"),
    }
    if INTEGRATED_TITLEBAR {
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, wgpu_options: wgpu_options(), ..Default::default() }
}

/// egui's wgpu setup, the device also asked for the optional features the viewport uses
/// where the adapter has them ([`solidsketch_render::OPTIONAL_FEATURES`]).
fn wgpu_options() -> egui_wgpu::WgpuConfiguration {
    let mut options = egui_wgpu::WgpuConfiguration::default();
    if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut descriptor = base(adapter);
            descriptor.required_features |= adapter.features() & solidsketch_render::OPTIONAL_FEATURES;
            descriptor
        });
    }
    options
}

/// Web entry point: starts the app on the `<canvas id="solidsketch">`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    use wasm_bindgen::JsCast;
    console_error_panic_hook::set_once();
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("solidsketch"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("no <canvas id=\"solidsketch\">");
        let options = eframe::WebOptions { wgpu_options: wgpu_options(), ..Default::default() };
        let result = eframe::WebRunner::new().start(canvas, options, Box::new(|cc| Ok(Box::new(SolidSketchApp::new(cc))))).await;
        if let Err(e) = result {
            log::error!("SolidSketch failed to start: {e:?}");
        }
    });
}

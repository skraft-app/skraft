//! SolidSketch, the application: egui panels around a viewport drawn by `solidsketch-render`,
//! driven by the `solidsketch-tools` editor. Native and web (docs/ARCHITECTURE.md §6 bis).

mod app;
mod commands;
#[cfg(not(target_arch = "wasm32"))]
mod control;
mod file;
mod icons;
mod panels;
mod prefs;
mod viewport;
#[cfg(target_arch = "wasm32")]
mod worker;

pub use app::SolidSketchApp;

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
        let options = eframe::WebOptions::default();
        let result = eframe::WebRunner::new()
            .start(canvas, options, Box::new(|cc| Ok(Box::new(SolidSketchApp::new(cc)))))
            .await;
        if let Err(e) = result {
            log::error!("SolidSketch failed to start: {e:?}");
        }
    });
}

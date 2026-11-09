//! SolidSketch on the desktop.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SolidSketch")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("SolidSketch", options, Box::new(|cc| Ok(Box::new(solidsketch::SolidSketchApp::new(cc)))))
}

#[cfg(target_arch = "wasm32")]
fn main() {}

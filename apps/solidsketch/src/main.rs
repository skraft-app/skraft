//! SolidSketch on the desktop.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    eframe::run_native("SolidSketch", solidsketch::native_options(), Box::new(|cc| Ok(Box::new(solidsketch::SolidSketchApp::new(cc)))))
}

#[cfg(target_arch = "wasm32")]
fn main() {}

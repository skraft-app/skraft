//! SolidSketch on the desktop.

// A release build on Windows is a GUI app: no console window opens with it (a debug build keeps
// one for the logs).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    solidsketch::opened::install();
    eframe::run_native("SolidSketch", solidsketch::native_options(), Box::new(|cc| Ok(Box::new(solidsketch::SolidSketchApp::new(cc)))))
}

#[cfg(target_arch = "wasm32")]
fn main() {}

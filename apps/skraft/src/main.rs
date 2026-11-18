//! Skraft on the desktop.

// A release build on Windows is a GUI app: no console window opens with it (a debug build keeps
// one for the logs).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    skraft::crash::install();
    env_logger::init();
    skraft::opened::install();
    let run = eframe::run_native("Skraft", skraft::native_options(), Box::new(|cc| Ok(Box::new(skraft::SkraftApp::new(cc)))));
    // Without a console nobody would see why it ends.
    if let Err(error) = &run {
        skraft::crash::fatal(error);
    }
    run
}

#[cfg(target_arch = "wasm32")]
fn main() {}

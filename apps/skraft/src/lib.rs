//! Skraft, the application: egui panels around a viewport drawn by `skraft-render`,
//! driven by the `skraft-tools` editor. Native and web (docs/ui/interface.md).

mod app;
mod clipboard;
mod commands;
mod cursors;
#[cfg(not(target_arch = "wasm32"))]
mod control;
#[cfg(not(target_arch = "wasm32"))]
pub mod crash;
mod dock;
mod docs;
mod driving;
mod file;
mod handles;
mod icons;
mod identity;
mod instance_name;
mod journal_window;
mod language;
mod naming;
#[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
mod native_menu;
mod nav_prefs;
#[cfg(not(target_arch = "wasm32"))]
pub mod opened;
mod panels;
mod pins;
mod prefs;
mod quick;
mod ribbon;
mod separators;
mod steps;
mod style;
mod tabs;
mod titlebar;
mod toasts;
mod tree;
mod view_bar;
mod viewport;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
mod worker;

pub use app::SkraftApp;

/// The bar is the window's title bar: on the macOS desktop (docs/ui/title-bar.md).
pub(crate) const INTEGRATED_TITLEBAR: bool = cfg!(all(target_os = "macos", not(target_arch = "wasm32")));

/// Only the app moves the window (macOS desktop): AppKit would move it from any drag in the top
/// of the window, where its hidden title bar still is — over the file tabs, which are dragged
/// along the bar to reorder them (docs/ui/file-tabs.md). The bar's empty space, and a
/// tab dragged up or down, still move it: `ViewportCommand::StartDrag` (AppKit's
/// `performWindowDragWithEvent:`) works on a window that is not movable by the user.
#[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
pub(crate) fn app_drags_window(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    let Ok(handle) = cc.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    // SAFETY: the handle's `ns_view` is the window's live content view, used on the main thread
    // (the app is being created there) while the handle is borrowed.
    let view: &objc2_app_kit::NSView = unsafe { h.ns_view.cast().as_ref() };
    if let Some(window) = view.window() {
        window.setMovable(false);
    }
}

/// The desktop window. On macOS the OS title bar is hidden and the content runs under it, the
/// traffic lights kept: the app's bar ([`titlebar`]) takes its place.
#[cfg(not(target_arch = "wasm32"))]
pub fn native_options() -> eframe::NativeOptions {
    let mut viewport =
        egui::ViewportBuilder::default().with_title("Skraft").with_inner_size([1280.0, 800.0]).with_min_inner_size([640.0, 400.0]);
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
/// where the adapter has them ([`skraft_render::OPTIONAL_FEATURES`]).
fn wgpu_options() -> egui_wgpu::WgpuConfiguration {
    let mut options = egui_wgpu::WgpuConfiguration::default();
    if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        // On Windows, Direct3D 12 only: wgpu would take Vulkan where the driver has it, and
        // Intel's Vulkan driver (igvk64.dll) crashes the app at launch. `WGPU_BACKEND` (vulkan,
        // dx12, gl…) still chooses another.
        if cfg!(windows) && wgpu::Backends::from_env().is_none() {
            setup.instance_descriptor.backends = wgpu::Backends::DX12;
        }
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut descriptor = base(adapter);
            descriptor.required_features |= adapter.features() & skraft_render::OPTIONAL_FEATURES;
            descriptor
        });
    }
    options
}

/// Web entry point: starts the app on the `<canvas id="skraft">`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    use wasm_bindgen::JsCast;
    console_error_panic_hook::set_once();
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("skraft"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("no <canvas id=\"skraft\">");
        let options = eframe::WebOptions { wgpu_options: wgpu_options(), ..Default::default() };
        let result = eframe::WebRunner::new().start(canvas, options, Box::new(|cc| Ok(Box::new(SkraftApp::new(cc))))).await;
        if let Err(e) = result {
            log::error!("Skraft failed to start: {e:?}");
        }
    });
}

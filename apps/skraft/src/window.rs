//! The window's frame around the stage (docs/ui/interface.md): the title bar and the native
//! menus, the file tabs, the ribbon, the toasts, the interface's zoom, the docked columns, and the start page shown
//! when no document is.

pub(crate) mod dock;
#[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
pub(crate) mod native_menu;
pub(crate) mod ribbon;
pub(crate) mod start;
pub(crate) mod tabs;
pub(crate) mod titlebar;
pub(crate) mod toasts;
pub(crate) mod zoom_label;

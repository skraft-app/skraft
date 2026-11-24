//! The window's frame around the stage (docs/ui/interface.md): the title bar and the native
//! menus, the file tabs, the ribbon, the quick tools' card, the toasts, the docked columns.

pub(crate) mod dock;
#[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
pub(crate) mod native_menu;
pub(crate) mod quick;
pub(crate) mod ribbon;
pub(crate) mod tabs;
pub(crate) mod titlebar;
pub(crate) mod toasts;

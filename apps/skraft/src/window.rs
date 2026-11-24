//! The window's frame around the stage (docs/ui/interface.md): the title bar and the native
//! menus, the file tabs, the ribbon, the bars over the view (view bar, quick tools, steps
//! bar, tree drop-up) and their pinned commands, the toasts, the docked columns.

pub(crate) mod dock;
pub(crate) mod drop_up;
#[cfg(all(target_os = "macos", not(target_arch = "wasm32")))]
pub(crate) mod native_menu;
pub(crate) mod pins;
pub(crate) mod quick;
pub(crate) mod ribbon;
pub(crate) mod steps;
pub(crate) mod tabs;
pub(crate) mod titlebar;
pub(crate) mod toasts;
pub(crate) mod view_bar;

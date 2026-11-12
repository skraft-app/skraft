//! The tool icons: original white SVGs (assets/icons), tinted by the theme at display.

use egui::ImageSource;

macro_rules! icon {
    ($name:ident, $file:literal) => {
        pub const $name: ImageSource<'static> = egui::include_image!(concat!("../assets/icons/", $file, ".svg"));
    };
}

icon!(SELECT, "select");
icon!(BOX, "box");
icon!(CYLINDER, "cylinder");
icon!(MOVE, "move");
icon!(ROTATE, "rotate");
icon!(UNION, "union");
icon!(SUBTRACT, "subtract");
icon!(INTERSECT, "intersect");
icon!(UNDO, "undo");
icon!(REDO, "redo");
icon!(COMPONENT, "component");
icon!(EXPLODE, "explode");
icon!(UNIQUE, "unique");
icon!(EXTENTS, "extents");
icon!(PERSPECTIVE, "perspective");
icon!(ERASE, "erase");
icon!(PAINT, "paint");
icon!(SECTION, "section");
icon!(MEASURE, "measure");
icon!(STRETCH, "stretch");
icon!(MENU, "menu");
icon!(GRID, "grid");
icon!(GUIDES, "guides");
icon!(AXES, "axes");
icon!(SILHOUETTES, "silhouettes");
icon!(VIEWS, "views");
icon!(SEARCH, "search");
icon!(SETTINGS, "settings");
icon!(MATERIALS, "materials");
icon!(EYE, "eye");
icon!(EYE_OFF, "eye_off");

/// A tool option's icon (assets/icons/options), by the name the tool gives it
/// ([`solidsketch_tools::OptionChoice::icon`]); none for an unknown name (its label is shown).
pub fn option(name: &str) -> Option<ImageSource<'static>> {
    macro_rules! options {
        ($($file:literal),* $(,)?) => {
            match name {
                $($file => Some(egui::include_image!(concat!("../assets/icons/options/", $file, ".svg"))),)*
                _ => None,
            }
        };
    }
    options!(
        "measure_overall",
        "measure_offset",
        "axes_active",
        "axes_context",
        "axes_entity",
        "centered",
        "scope_all",
        "scope_this",
        "rectangle_centered",
        "measure_radius",
        "measure_diameter",
        "from_corner",
    )
}

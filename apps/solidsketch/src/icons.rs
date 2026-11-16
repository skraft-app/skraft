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
icon!(PRISM, "prism");
icon!(TURN, "turn");
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
icon!(COPY_LINE, "copy_line");
icon!(COPY_GRID, "copy_grid");
icon!(DISTRIBUTE, "distribute");
icon!(GRID, "grid");
icon!(GUIDES, "guides");
icon!(AXES, "axes");
icon!(SILHOUETTES, "silhouettes");
icon!(XRAY, "xray");
icon!(VIEWS, "views");
icon!(SEARCH, "search");
icon!(SETTINGS, "settings");
icon!(REMOTE, "remote");
icon!(MATERIALS, "materials");
icon!(EYE, "eye");
icon!(EYE_OFF, "eye_off");
icon!(MODELLING, "modelling");
icon!(PIN, "pin");
icon!(PIN_FILLED, "pin_filled");
icon!(SECTION_CLEAR, "section_clear");
icon!(GUIDES_DELETE, "guides_delete");
icon!(CLOSE, "close");
icon!(ADD, "add");

/// A command's icon on the ribbon (docs/ARCHITECTURE.md §6 septies); none for a command the
/// ribbon does not show.
pub fn command(id: &str) -> Option<ImageSource<'static>> {
    Some(match id {
        "tool.select" => SELECT,
        "tool.erase" => ERASE,
        "tool.box" => BOX,
        "tool.cylinder" => CYLINDER,
        "tool.prism" => PRISM,
        "tool.turn" => TURN,
        "tool.move" => MOVE,
        "tool.rotate" => ROTATE,
        "tool.stretch" => STRETCH,
        "tool.copy_line" => COPY_LINE,
        "tool.copy_grid" => COPY_GRID,
        "tool.distribute" => DISTRIBUTE,
        "tool.paint" => PAINT,
        "tool.section" => SECTION,
        "tool.measure" => MEASURE,
        "tool.union" => UNION,
        "tool.difference" => SUBTRACT,
        "tool.intersection" => INTERSECT,
        "edit.erase" => ERASE,
        "edit.make_assembly" => COMPONENT,
        "edit.explode" => EXPLODE,
        "edit.make_unique" => UNIQUE,
        "edit.delete_guides" => GUIDES_DELETE,
        "view.section_clear" => SECTION_CLEAR,
        "view.zoom_extents" => EXTENTS,
        "view.top" | "view.front" | "view.right" | "view.back" | "view.left" | "view.iso" => VIEWS,
        _ => return None,
    })
}

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
        "measure_reversed",
        "trace_line",
        "trace_tangent",
        "trace_arc",
        "copy_measure_outside",
        "copy_measure_centered",
        "copy_measure_inside",
        "mirror",
        "ask_name",
    )
}

/// The app's icon, in its colours (not tinted): the title bar's mark.
pub const APP: ImageSource<'static> = egui::include_image!("../assets/app-icon.svg");

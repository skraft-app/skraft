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
icon!(EXTRUSION, "extrusion");
icon!(REVOLUTION, "revolution");
icon!(SOLID, "solid");
icon!(PROFILE, "profile");
icon!(MOVE, "move");
icon!(ROTATE, "rotate");
icon!(UNION, "union");
icon!(SUBTRACT, "subtract");
icon!(INTERSECT, "intersect");
icon!(UNDO, "undo");
icon!(REDO, "redo");
icon!(COMPONENT, "component");
icon!(MODEL, "model");
icon!(GROUP, "group");
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
icon!(MORE, "more");
icon!(WARNING, "warning");
icon!(CHEVRON_DOWN, "chevron_down");
icon!(HIDE_REST, "hide_rest");

/// The chevron (pointing down, turned by `angle` clockwise) `side` wide centered on `at`: the
/// one fold and drop-down mark of the app (panels' title bars, outliner rows, menus, combo boxes).
pub fn chevron(ui: &egui::Ui, at: egui::Pos2, side: f32, ink: egui::Color32, angle: f32) {
    let rect = egui::Rect::from_center_size(at, egui::Vec2::splat(side));
    egui::Image::new(CHEVRON_DOWN).tint(ink).rotate(angle, egui::Vec2::splat(0.5)).paint_at(ui, rect);
}

/// A combo box's drop-down mark (its `icon`): the chevron in place of egui's triangle.
pub fn combo_chevron(ui: &egui::Ui, rect: egui::Rect, visuals: &egui::style::WidgetVisuals, _open: bool) {
    chevron(ui, rect.center(), rect.height().min(14.0), visuals.fg_stroke.color, 0.0);
}

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
        "edit.group_bodies" => GROUP,
        "edit.ungroup_bodies" => EXPLODE,
        "edit.delete_guides" => GUIDES_DELETE,
        "view.section_clear" => SECTION_CLEAR,
        "view.parallel" => PERSPECTIVE,
        "view.grid" => GRID,
        "view.axes" => AXES,
        "view.guides" => GUIDES,
        "view.silhouettes" => SILHOUETTES,
        "view.xray" => XRAY,
        "view.hide_rest_of_model" => HIDE_REST,
        "view.show_hidden" => EYE,
        "view.materials" => MATERIALS,
        "view.section" => SECTION,
        "view.zoom_extents" => EXTENTS,
        "view.top" | "view.front" | "view.right" | "view.back" | "view.left" | "view.iso" => VIEWS,
        _ => return None,
    })
}

/// A tool option's icon (assets/icons/options), by the name the tool gives it
/// ([`solidsketch_tools::OptionChoice::icon`]); none for an unknown name (its label is shown).
pub fn option(name: &str) -> Option<ImageSource<'static>> {
    // The booleans' icons, shared with the open part's « Solide » (§2.11).
    match name {
        "union" => return Some(UNION),
        "subtract" => return Some(SUBTRACT),
        _ => {}
    }
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

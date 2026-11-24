//! The tool icons: original white SVGs (assets/icons), tinted by the theme at display.

use egui::ImageSource;

macro_rules! icon {
    ($name:ident, $file:literal) => {
        pub const $name: ImageSource<'static> = egui::include_image!(concat!("../../assets/icons/", $file, ".svg"));
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
icon!(IMPRINT, "imprint");
icon!(LINKED, "linked");
icon!(UNION, "union");
icon!(SUBTRACT, "subtract");
icon!(INTERSECT, "intersect");
icon!(UNDO, "undo");
icon!(REDO, "redo");
icon!(COMPONENT, "component");
icon!(MODEL, "model");
icon!(GROUP, "group");
icon!(COMPOSITION, "composition");
icon!(MOVE_OUT, "move_out");
icon!(EXPLODE, "explode");
icon!(UNIQUE, "unique");
icon!(EXTENTS, "extents");
icon!(PERSPECTIVE, "perspective");
icon!(ERASE, "erase");
icon!(PAINT, "paint");
icon!(SAMPLE, "sample");
icon!(SECTION, "section");
icon!(MEASURE, "measure");
icon!(STRETCH, "stretch");
icon!(REORIENT, "reorient");
icon!(ROUND, "round");
icon!(CHAMFER, "chamfer");
icon!(COPY_LINE, "copy_line");
icon!(COPY_GRID, "copy_grid");
icon!(DISTRIBUTE, "distribute");
icon!(GRID, "grid");
icon!(GUIDES, "guides");
icon!(AXES, "axes");
icon!(SILHOUETTES, "silhouettes");
icon!(XRAY, "xray");
icon!(VIEWS, "views");
icon!(VIEW_TOP, "view_top");
icon!(VIEW_FRONT, "view_front");
icon!(VIEW_RIGHT, "view_right");
icon!(VIEW_BACK, "view_back");
icon!(VIEW_LEFT, "view_left");
icon!(VIEW_ISO, "view_iso");
icon!(SEARCH, "search");
icon!(SETTINGS, "settings");
icon!(REMOTE, "remote");
icon!(MATERIALS, "materials");
icon!(EYE, "eye");
icon!(EYE_OFF, "eye_off");
icon!(LOCK, "lock");
icon!(UNLOCK, "unlock");
icon!(ARROW_UP, "arrow_up");
icon!(ARROW_DOWN, "arrow_down");
icon!(MODELLING, "modelling");
icon!(PARTS_LIST, "parts_list");
icon!(DRAWING, "drawing");
icon!(SHEET, "sheet");
icon!(SHEET_ADD, "sheet_add");
icon!(PRESENTATION_ADD, "presentation_add");
icon!(VIEW_ADD, "view_add");
icon!(VIEW_PROJECT, "view_project");
icon!(SHEET_TEXT, "sheet_text");
icon!(SHEET_RECT, "sheet_rect");
icon!(SHEET_CIRCLE, "sheet_circle");
icon!(SHEET_LINE, "sheet_line");
icon!(DIM_LINEAR, "dim_linear");
icon!(DIM_RADIUS, "dim_radius");
icon!(DIM_DIAMETER, "dim_diameter");
icon!(DIM_ANGLE, "dim_angle");
icon!(STYLE_LINES, "style_lines");
icon!(STYLE_RENDERED, "style_rendered");
icon!(STYLE_HYBRID, "style_hybrid");
icon!(PIN, "pin");
icon!(PIN_FILLED, "pin_filled");
icon!(SECTION_CLEAR, "section_clear");
icon!(GUIDES_DELETE, "guides_delete");
icon!(CLOSE, "close");
icon!(ADD, "add");
icon!(TEXTURE, "texture");
icon!(EXPORT, "export");
icon!(IMPORT, "import");
icon!(MORE, "more");
icon!(WARNING, "warning");
icon!(CHEVRON_DOWN, "chevron_down");
icon!(TILES, "tiles");
icon!(LIST, "list");
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

/// A command's icon on the ribbon (docs/ui/ribbon.md); none for a command the
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
        "tool.reorient" => REORIENT,
        "tool.round" => ROUND,
        "tool.chamfer" => CHAMFER,
        "tool.copy_line" => COPY_LINE,
        "tool.copy_grid" => COPY_GRID,
        "tool.distribute" => DISTRIBUTE,
        "tool.paint" => PAINT,
        "tool.sample" => SAMPLE,
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
        "edit.imprint" => IMPRINT,
        "edit.move_out" => MOVE_OUT,
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
        "drawing.new_presentation" => PRESENTATION_ADD,
        "drawing.new_sheet" => SHEET_ADD,
        "drawing.make_composition" => COMPOSITION,
        "drawing.explode" => EXPLODE,
        "drawing.make_unique" => UNIQUE,
        "drawing.add_view" => VIEW_ADD,
        "drawing.project_view" => VIEW_PROJECT,
        "drawing.frame_view" => VIEW_ISO,
        "drawing.export_pdf" => EXPORT,
        "drawing.tool_text" => SHEET_TEXT,
        "drawing.tool_rect" => SHEET_RECT,
        "drawing.tool_circle" => SHEET_CIRCLE,
        "drawing.tool_line" => SHEET_LINE,
        "drawing.dim_linear" => DIM_LINEAR,
        "drawing.dim_radius" => DIM_RADIUS,
        "drawing.dim_diameter" => DIM_DIAMETER,
        "drawing.dim_angle" => DIM_ANGLE,
        "drawing.style_lines" => STYLE_LINES,
        "drawing.style_rendered" => STYLE_RENDERED,
        "drawing.style_hybrid" => STYLE_HYBRID,
        "view.top" => VIEW_TOP,
        "view.front" => VIEW_FRONT,
        "view.right" => VIEW_RIGHT,
        "view.back" => VIEW_BACK,
        "view.left" => VIEW_LEFT,
        "view.iso" => VIEW_ISO,
        _ => return None,
    })
}

/// A tool option's icon (assets/icons/options), by the name the tool gives it
/// ([`skraft_tools::OptionChoice::icon`]); none for an unknown name (its label is shown).
pub fn option(name: &str) -> Option<ImageSource<'static>> {
    // The booleans' icons, shared with the open part's « Solide » (docs/model/bodies.md).
    match name {
        "union" => return Some(UNION),
        "subtract" => return Some(SUBTRACT),
        "intersect" => return Some(INTERSECT),
        _ => {}
    }
    macro_rules! options {
        ($($file:literal),* $(,)?) => {
            match name {
                $($file => Some(egui::include_image!(concat!("../../assets/icons/options/", $file, ".svg"))),)*
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
        "select_context",
        "select_parts",
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
        "draw_in",
        "pick_material",
        "drop_unused",
    )
}

/// The app's icon, in its colours (not tinted): the title bar's mark.
pub const APP: ImageSource<'static> = egui::include_image!("../../assets/app-icon.svg");

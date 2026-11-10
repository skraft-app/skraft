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

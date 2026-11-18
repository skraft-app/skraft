//! A recording of the editor's inputs written as a test (docs/ARCHITECTURE.md §7 septies): a
//! file of `crates/tools/tests` that opens the document recorded, sets the view the inputs were
//! given in, plays them again on an editor with the kernel inline, then checks the revision and
//! the parts reached. What it then shows is the test's to check: the file says where.

use std::fmt::Write;

use glam::DVec2;
use skraft_tools::journal::{Recording, Step};
use skraft_tools::{Camera, Key, OptionValue, Projection, ToolInput};

/// What the test reached when recorded.
pub struct Reached {
    pub revision: u64,
    pub parts: usize,
}

/// The test file (Rust) for `r`, named `name`; it reads the document from `<name>.skraft`
/// beside it.
pub fn test_file(r: &Recording, name: &str, reached: &Reached) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "//! Recorded by the control protocol (`journal.record`, docs/ARCHITECTURE.md §7 septies): {} steps from\n\
         //! `{name}.skraft`, played again with the kernel inline.\n",
        r.steps.len()
    );
    out.push_str("use glam::{DMat3, DVec2, DVec3};\n");
    out.push_str("use skraft_model::Document;\n");
    out.push_str("#[allow(unused_imports)]\n");
    out.push_str("use skraft_tools::{Camera, Editor, Key, Modifiers, OptionValue, Projection, ToolInput, ToolKind};\n\n");
    out.push_str("/// The view the inputs that follow were given in (their pixels are its).\n");
    out.push_str("fn view(e: &mut Editor, camera: Camera, size: DVec2) {\n    e.set_viewport_size(size);\n    e.camera = camera;\n}\n\n");
    let _ = writeln!(out, "#[test]\nfn {name}() {{");
    let _ = writeln!(out, "    let doc = Document::from_json(include_str!(\"{name}.skraft\")).unwrap();");
    out.push_str("    let mut e = Editor::new(doc, Box::new(skraft_kernel::Inline::default()));\n");
    let _ = writeln!(out, "    e.set_tool(ToolKind::from_id({:?}).unwrap());", r.tool.id());
    for step in &r.steps {
        let line = match step {
            Step::View { camera, size } => format!("view(&mut e, {}, {});", camera_code(camera), v2(*size)),
            Step::Input(input) => match input_code(input) {
                // As the window does, a frame between inputs: what the kernel finished comes in.
                Some(code) => format!("e.poll();\n    e.input({code});"),
                None => format!("// Not played again: {input:?}"),
            },
            Step::Tool(kind) => format!("e.set_tool(ToolKind::from_id({:?}).unwrap());", kind.id()),
            Step::Option { key, value } => format!("e.set_tool_option({key:?}, {});", option_code(value)),
            Step::Shift(down) => format!("e.set_shift({down});"),
            Step::Copy(down) => format!("e.set_copy({down});"),
            Step::Alt(down) => format!("e.set_alt({down});"),
            Step::Undo => "e.undo();".into(),
            Step::Redo => "e.redo();".into(),
        };
        let _ = writeln!(out, "    {line}");
    }
    out.push_str("    e.poll();\n");
    let _ = writeln!(out, "    // Where the recording ended. What to check of it comes here (e.g. `e.document().placed_parts()`).");
    let _ = writeln!(out, "    assert_eq!(e.history().revision(), {}, \"{{}}\", e.status());", reached.revision);
    let _ = writeln!(out, "    assert_eq!(e.document().placed_parts().len(), {});", reached.parts);
    out.push_str("}\n");
    out
}

/// Is `name` a test's name: a Rust identifier, lower case?
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn f(x: f64) -> String {
    // Debug: the shortest text that reads back as the same f64.
    format!("{x:?}")
}

fn v2(v: DVec2) -> String {
    format!("DVec2::new({}, {})", f(v.x), f(v.y))
}

fn camera_code(c: &Camera) -> String {
    let p = match c.projection {
        Projection::Perspective { fov_y } => format!("Projection::Perspective {{ fov_y: {} }}", f(fov_y)),
        Projection::Parallel { height } => format!("Projection::Parallel {{ height: {} }}", f(height)),
    };
    let m = c.frame.to_cols_array().map(f).join(", ");
    format!(
        "Camera {{ eye: DVec3::new({}, {}, {}), yaw: {}, pitch: {}, projection: {p}, focus: {}, frame: DMat3::from_cols_array(&[{m}]) }}",
        f(c.eye.x),
        f(c.eye.y),
        f(c.eye.z),
        f(c.yaw),
        f(c.pitch),
        f(c.focus)
    )
}

fn mods(m: &skraft_tools::Modifiers) -> String {
    format!("Modifiers {{ toggle: {}, copy: {} }}", m.toggle, m.copy)
}

fn key(k: Key) -> String {
    format!("Key::{k:?}")
}

fn input_code(input: &ToolInput) -> Option<String> {
    Some(match input {
        ToolInput::Hover { px } => format!("ToolInput::Hover {{ px: {} }}", v2(*px)),
        ToolInput::Click { px, mods: m } => format!("ToolInput::Click {{ px: {}, mods: {} }}", v2(*px), mods(m)),
        ToolInput::DoubleClick { px, mods: m } => format!("ToolInput::DoubleClick {{ px: {}, mods: {} }}", v2(*px), mods(m)),
        ToolInput::DragStart { px, mods: m } => format!("ToolInput::DragStart {{ px: {}, mods: {} }}", v2(*px), mods(m)),
        ToolInput::Drag { px } => format!("ToolInput::Drag {{ px: {} }}", v2(*px)),
        ToolInput::DragEnd { px, mods: m } => format!("ToolInput::DragEnd {{ px: {}, mods: {} }}", v2(*px), mods(m)),
        ToolInput::Key { key: k, mods: m } => format!("ToolInput::Key {{ key: {}, mods: {} }}", key(*k), mods(m)),
        ToolInput::Vcb(text) => format!("ToolInput::Vcb({text:?}.into())"),
        // Given by the editor itself, from a typed text.
        ToolInput::Value(_) => return None,
    })
}

fn option_code(v: &OptionValue) -> String {
    match v {
        OptionValue::Choice(c) => format!("OptionValue::Choice({c:?}.into())"),
        OptionValue::Toggle(b) => format!("OptionValue::Toggle({b})"),
        OptionValue::Length(l) => format!("OptionValue::Length({})", f(*l)),
    }
}

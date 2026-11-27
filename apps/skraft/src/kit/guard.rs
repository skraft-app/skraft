//! The guard of « built once » (docs/ui/interface.md#widgets): outside `kit/`, no button nor
//! drop-down list is built from egui's own — a screen takes the kit's (`TextButton`,
//! `IconButton`, `MenuEntry`, `ChoiceList`, `segmented`…), and a variant missing (a size, a lit
//! state, a look) is added to the kit's widget.

/// What builds a button from egui's own.
const RAW: &[&str] = &[
    "egui::Button::",
    "egui::ComboBox",
    "egui::ImageButton",
    "egui::SelectableLabel",
    ".button(",
    ".small_button(",
    "selectable_label(",
    "selectable_value(",
    "toggle_value(",
];

/// The raw buttons of `src` (comment lines left out). A call with no argument is not egui's
/// (`binding.trigger.button()`, a mouse button).
fn count(src: &str) -> usize {
    let raw = |l: &str, p: &str| l.match_indices(p).filter(|(i, _)| !l[i + p.len()..].starts_with(')')).count();
    src.lines().filter(|l| !l.trim_start().starts_with("//")).map(|l| RAW.iter().map(|p| raw(l, p)).sum::<usize>()).sum()
}

/// Every `.rs` file under `dir`, its path from `root` with `/`, but those under `kit/`.
fn files(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            if rel != "kit" {
                files(root, &path, out);
            }
        } else if rel.ends_with(".rs") {
            out.push((rel, std::fs::read_to_string(&path).unwrap()));
        }
    }
}

#[test]
fn no_button_is_built_outside_the_kit() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut all = Vec::new();
    files(&root, &root, &mut all);
    let wrong: Vec<String> =
        all.iter().map(|(f, s)| (f, count(s))).filter(|(_, n)| *n > 0).map(|(f, n)| format!("{f}: {n} egui button(s)")).collect();
    assert!(
        wrong.is_empty(),
        "Buttons and drop-down lists are the kit's (docs/ui/interface.md#widgets): take its widget, or add \
         the variant missing to it.\n{}",
        wrong.join("\n")
    );
}

/// What `kit/widgets.rs` gives that is no widget of its own, so not in the gallery: a widget's
/// parts and settings, what it answers, its constants, the helpers that paint for it.
const NOT_IN_GALLERY: &[&str] = &[
    "Confirm",
    "KeypadAct",
    "PinEvent",
    "OPTION_HEIGHT",
    "VEIL_RADIUS",
    "BADGE",
    "RENAMING",
    "veil",
    "menu_open",
    "field_stroke",
    "is_toggle",
    "badge_rect",
    "picture_ground",
    "band_color",
    "set_drag_ghost",
    "paint_drag_ghost",
];

/// The names `kit/widgets.rs` gives: its re-exports and its own public items.
fn widget_names(src: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut uses = String::new();
    let mut in_use = false;
    for line in src.lines().map(str::trim) {
        if line.starts_with("pub(crate) use ") {
            in_use = true;
        }
        if in_use {
            uses.push_str(line);
            in_use = !line.ends_with(';');
            continue;
        }
        for prefix in ["pub fn ", "pub const ", "pub enum ", "pub struct "] {
            if let Some(rest) = line.strip_prefix(prefix) {
                names.push(rest.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap().to_owned());
            }
        }
    }
    for group in uses.split("pub(crate) use ").filter(|g| !g.is_empty()) {
        let inner = group.split_once('{').map_or(group, |(_, i)| i);
        names.extend(inner.trim_end_matches(';').trim_end_matches('}').split(',').map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()));
    }
    names
}

#[test]
fn every_widget_is_in_the_gallery() {
    let src = |p: &str| std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(p)).unwrap();
    let gallery = src("dialogs/kit_gallery.rs");
    let uses = |name: &str| {
        gallery.match_indices(name).any(|(i, _)| {
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            !word(gallery[..i].chars().next_back()) && !word(gallery[i + name.len()..].chars().next())
        })
    };
    let mut names = widget_names(&src("kit/widgets.rs"));
    names.extend(["color_box", "paint_swatch"].map(str::to_owned));
    let missing: Vec<&String> = names.iter().filter(|n| !NOT_IN_GALLERY.contains(&n.as_str()) && !uses(n)).collect();
    assert!(names.len() > 40, "the names of kit/widgets.rs were not read: {names:?}");
    assert!(
        missing.is_empty(),
        "Every widget of the kit is shown in its gallery (dialogs/kit_gallery.rs, docs/ui/interface.md#the-kits-gallery): \
         add its specimens there, or list it in NOT_IN_GALLERY when it is no widget of its own.\n{missing:?}"
    );
}

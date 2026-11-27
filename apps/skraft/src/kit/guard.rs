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

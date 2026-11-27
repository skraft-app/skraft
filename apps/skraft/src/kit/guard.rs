//! The guard of « built once » (docs/ui/interface.md#widgets): outside `kit/`, no button nor
//! drop-down list is built from egui's own — a screen takes the kit's (`IconButton`, `segmented`,
//! `option_button`, `LabelButton`, `confirm_buttons`…), and a variant missing (a size, a lit
//! state, a look) is added to the kit's widget. The files still holding egui's buttons from
//! before are listed with their count, a ceiling that only goes down: a file moved to the kit
//! lowers its line, a new raw button fails here.

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

/// The files under `apps/skraft/src` still building egui's buttons, and how many: to move to
/// the kit, never to raise.
const CEILINGS: &[(&str, usize)] = &[
    ("app/dialogs.rs", 2),
    ("app/documents.rs", 1),
    ("app/layout.rs", 1),
    ("app/settings.rs", 2),
    ("dialogs/about.rs", 1),
    ("dialogs/journal_window.rs", 2),
    ("dialogs/nav_prefs.rs", 5),
    ("dialogs/update.rs", 3),
    ("window/tabs.rs", 1),
    ("window/titlebar.rs", 2),
    ("workspaces/common/cards/catalog.rs", 1),
    ("workspaces/common/cards/info.rs", 1),
    ("workspaces/common/cards/materials.rs", 1),
    ("workspaces/common/cards/navigator.rs", 2),
    ("workspaces/common/cards/props.rs", 2),
    ("workspaces/common/viewport/options.rs", 1),
    ("workspaces/common/viewport/view_bar.rs", 2),
    ("workspaces/drawing/cards/info.rs", 2),
    ("workspaces/drawing/dialogs/view_framer.rs", 2),
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
    let found: Vec<(String, usize)> = all.iter().map(|(f, s)| (f.clone(), count(s))).filter(|(_, n)| *n > 0).collect();
    let mut wrong = Vec::new();
    for (file, n) in &found {
        match CEILINGS.iter().find(|(f, _)| f == file) {
            None => wrong.push(format!("{file}: {n} egui button(s) built outside the kit")),
            Some((_, max)) if n > max => wrong.push(format!("{file}: {n} egui buttons, {max} at most")),
            Some((_, max)) if n < max => wrong.push(format!("{file}: {n} egui buttons now, lower its ceiling from {max}")),
            Some(_) => {}
        }
    }
    for (file, _) in CEILINGS.iter().filter(|(f, _)| !found.iter().any(|(g, _)| g == f)) {
        wrong.push(format!("{file}: no egui button left, drop its line"));
    }
    let table: String = found.iter().map(|(f, n)| format!("    (\"{f}\", {n}),\n")).collect();
    assert!(
        wrong.is_empty(),
        "Buttons are the kit's (docs/ui/interface.md#widgets, apps/skraft/src/kit/guard.rs): take its \
         widget, or add the variant missing to it.\n{}\n\nThe counts now:\n{table}",
        wrong.join("\n")
    );
}

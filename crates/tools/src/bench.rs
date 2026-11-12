//! Timings of an editor with many occurrences (ignored; run with
//! `cargo test -p solidsketch-tools --release bench -- --ignored --nocapture`).

use std::time::Instant;

use glam::{DVec2, DVec3};
use solidsketch_model::Document;

use crate::*;

fn time<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    eprintln!("{what:<40} {:>10.2} ms", t.elapsed().as_secs_f64() * 1e3);
    r
}

#[test]
#[ignore]
fn bench_array() {
    let n: usize = std::env::var("BENCH_N").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
    let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Inline::default()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.set_parallel(true);
    let span = 100.0 * n as f64;
    e.camera.set_standard_view(StandardView::Top, Some((DVec3::splat(-100.0), DVec3::splat(span))), crate::camera::ViewArea::full(e.viewport_size()));
    let px = |e: &Editor, p: DVec3| e.camera.project(p, e.viewport_size()).unwrap();
    let click = |e: &mut Editor, p: DVec3| {
        let at = px(e, p);
        e.input(ToolInput::Hover { px: at });
        e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    };
    e.set_tool(ToolKind::Cylinder);
    click(&mut e, DVec3::ZERO);
    e.input(ToolInput::Vcb("30".into()));
    e.input(ToolInput::Vcb("50".into()));
    e.set_tool(ToolKind::Select);
    e.select_all();
    // A row along X, then the row copied along Y.
    for (axis, key) in [(DVec3::X, Key::Right), (DVec3::Y, Key::Left)] {
        e.set_tool(ToolKind::Move);
        e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
        click(&mut e, DVec3::new(0.0, 0.0, 50.0));
        e.input(ToolInput::Key { key, mods: Modifiers::default() });
        e.input(ToolInput::Hover { px: px(&e, DVec3::new(0.0, 0.0, 50.0) + axis * 100.0) });
        time(&format!("copy along {axis}"), || e.input(ToolInput::Vcb("100".into())));
        time(&format!("array {}x", n - 1), || e.input(ToolInput::Vcb(format!("{}x", n - 1))));
        e.set_tool(ToolKind::Select);
        time("select_all", || e.select_all());
    }
    let count = e.document().root_assembly().occurrences.len();
    eprintln!("occurrences: {count}, selected: {}", e.selection().len());
    for _ in 0..2 {
        time("view (all selected)", || e.view());
    }
    time("hover (select tool, all selected)", || e.input(ToolInput::Hover { px: DVec2::new(500.0, 500.0) }));
    e.select(vec![]);
    for _ in 0..2 {
        time("view (none selected)", || e.view());
    }
    time("hover (select tool)", || e.input(ToolInput::Hover { px: DVec2::new(501.0, 500.0) }));
    e.set_tool(ToolKind::Move);
    time("hover (move tool)", || e.input(ToolInput::Hover { px: DVec2::new(502.0, 500.0) }));
    e.set_tool(ToolKind::Box);
    time("hover (box tool)", || e.input(ToolInput::Hover { px: DVec2::new(503.0, 500.0) }));
    e.set_tool(ToolKind::Select);
    time("undo", || e.undo());
    time("redo", || e.redo());
}

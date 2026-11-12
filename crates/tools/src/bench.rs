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
    // BENCH_LOOP set: undoes and redoes for a few seconds, to profile (`sample <pid>`).
    if std::env::var("BENCH_LOOP").is_ok() {
        let t = Instant::now();
        while t.elapsed().as_secs() < 6 {
            e.undo();
            e.redo();
        }
    }
}

/// A Z stretch of one cube among `BENCH_N`² (100 mm, every 200 mm), scope "this": what each
/// mouse move costs.
#[test]
#[ignore]
fn bench_stretch_one_among_many() {
    use glam::DAffine3;
    use solidsketch_model::{Operation, Primitive};
    let n: u32 = std::env::var("BENCH_N").ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Inline::default()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.set_parallel(true);
    let root = e.document().root();
    let cube = Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) };
    let a = e.apply(Operation::AddPrimitive { context: root, primitive: cube, placement: DAffine3::IDENTITY }).unwrap().unwrap().created[0];
    let row = e
        .apply(Operation::Array { context: root, entities: vec![a], step: DAffine3::from_translation(DVec3::X * 200.0), count: n - 1 })
        .unwrap()
        .unwrap()
        .created;
    let row: Vec<_> = std::iter::once(a).chain(row).collect();
    time("array rows", || {
        e.apply(Operation::Array { context: root, entities: row, step: DAffine3::from_translation(DVec3::Y * 200.0), count: n - 1 }).unwrap()
    });
    eprintln!("occurrences: {}", e.document().root_assembly().occurrences.len());
    // Looking at the first cube from the front, close.
    e.camera.set_standard_view(StandardView::Front, Some((DVec3::splat(-50.0), DVec3::splat(150.0))), crate::camera::ViewArea::full(e.viewport_size()));
    let px = |e: &Editor, p: DVec3| e.camera.project(p, e.viewport_size()).unwrap();
    e.select(vec![a]);
    e.set_tool(ToolKind::Stretch);
    e.set_tool_option("scope", OptionValue::Choice("this".into()));
    let top = DVec3::new(50.0, 50.0, 100.0);
    e.input(ToolInput::Hover { px: px(&e, top) });
    time("click grip", || e.input(ToolInput::Click { px: px(&e, top), mods: Modifiers::default() }));
    eprintln!("{}", e.status());
    for i in 1..=4 {
        let at = px(&e, top + DVec3::Z * (10.0 * i as f64));
        time(&format!("hover {i} (input)"), || e.input(ToolInput::Hover { px: at }));
        time(&format!("hover {i} (view)"), || e.view());
    }
    eprintln!("placements drawn: {}", e.view().batches.iter().map(|(_, ps)| ps.len()).sum::<usize>());
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    // Seen from above, all of it: a hover, a rectangle dragged over a quarter.
    e.set_tool(ToolKind::Select);
    let span = 200.0 * n as f64;
    e.camera.set_standard_view(StandardView::Top, Some((DVec3::ZERO, DVec3::new(span, span, 100.0))), crate::camera::ViewArea::full(e.viewport_size()));
    time("hover (select tool, top)", || e.input(ToolInput::Hover { px: DVec2::new(500.0, 500.0) }));
    let none = Modifiers::default();
    e.input(ToolInput::DragStart { px: DVec2::new(250.0, 250.0), mods: none });
    for k in 1..=2 {
        let at = DVec2::new(500.0 + k as f64, 500.0);
        time(&format!("marquee drag {k} (window)"), || e.input(ToolInput::Drag { px: at }));
    }
    e.input(ToolInput::DragEnd { px: DVec2::new(502.0, 500.0), mods: none });
    eprintln!("selected: {}", e.selection().len());
    time("undo (scene synced)", || e.undo());
    // BENCH_LOOP set: hovers for a few seconds, to profile (`sample <pid>`).
    if std::env::var("BENCH_LOOP").is_ok() {
        let t = std::time::Instant::now();
        let mut i = 0;
        while t.elapsed().as_secs() < 6 {
            i += 1;
            let at = px(&e, top + DVec3::Z * (10.0 + (i % 50) as f64));
            e.input(ToolInput::Hover { px: at });
        }
        eprintln!("loops: {i}");
    }
}

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
    e.camera.set_standard_view(
        StandardView::Top,
        glam::DMat3::IDENTITY,
        Some((DVec3::splat(-100.0), DVec3::splat(span))),
        crate::camera::ViewArea::full(e.viewport_size()),
    );
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
    let count = e.document().root_assembly().occurrences().len();
    eprintln!("occurrences: {count}, selected: {}", e.selection().len());
    for _ in 0..2 {
        time("view (all selected)", || e.view());
    }
    // BENCH_VIEW set: draws for a few seconds, all selected, to profile (`sample <pid>`).
    if std::env::var("BENCH_VIEW").is_ok() {
        let t = Instant::now();
        let first = std::env::var("BENCH_VIEW").is_ok_and(|v| v == "first");
        while t.elapsed().as_secs() < 6 {
            if first {
                e.select(vec![]);
                std::hint::black_box(e.view());
                e.select_all();
            }
            std::hint::black_box(e.view());
        }
    }
    // The selection changed: its drawing worked out again, once.
    e.select(vec![]);
    e.view();
    e.select_all();
    time("view (all selected again: selection redrawn)", || e.view());
    time("view (all selected again: kept)", || e.view());
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
    let a = e
        .apply(Operation::AddPrimitive { context: root, primitive: cube, placement: DAffine3::IDENTITY, material: None })
        .unwrap()
        .unwrap()
        .created[0];
    let row = e
        .apply(Operation::Array { context: root, entities: vec![a], step: DAffine3::from_translation(DVec3::X * 200.0), count: n - 1 })
        .unwrap()
        .unwrap()
        .created;
    let row: Vec<_> = std::iter::once(a).chain(row).collect();
    time("array rows", || {
        e.apply(Operation::Array { context: root, entities: row, step: DAffine3::from_translation(DVec3::Y * 200.0), count: n - 1 })
            .unwrap()
    });
    eprintln!("occurrences: {}", e.document().root_assembly().occurrences().len());
    // Looking at the first cube from the front, close.
    e.camera.set_standard_view(
        StandardView::Front,
        glam::DMat3::IDENTITY,
        Some((DVec3::splat(-50.0), DVec3::splat(150.0))),
        crate::camera::ViewArea::full(e.viewport_size()),
    );
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
    // BENCH_LOOP set: pulls the one cube for a few seconds, to profile (`sample <pid>`).
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
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    // Seen from above, all of it: a hover, a rectangle dragged over a quarter.
    e.set_tool(ToolKind::Select);
    let span = 200.0 * n as f64;
    e.camera.set_standard_view(
        StandardView::Top,
        glam::DMat3::IDENTITY,
        Some((DVec3::ZERO, DVec3::new(span, span, 100.0))),
        crate::camera::ViewArea::full(e.viewport_size()),
    );
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
    e.redo();
    // Everything selected, the stretch tool on it: its grips follow the cursor.
    e.select_all();
    e.set_tool(ToolKind::Stretch);
    eprintln!("selected: {}", e.selection().len());
    for k in 0..3 {
        time(&format!("hover {k} (stretch, all selected)"), || e.input(ToolInput::Hover { px: DVec2::new(600.0 + k as f64, 400.0) }));
    }
}

/// The index where it is weakest: 10 000 small cubes under 20 plates as wide as the whole
/// grid (each plate's box spans every node it falls in). Hovers with an inferring tool.
#[test]
#[ignore]
fn bench_mixed_sizes() {
    use glam::DAffine3;
    use solidsketch_model::{Operation, Primitive};
    let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Inline::default()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    let root = e.document().root();
    let cube = Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(20.0) };
    let a = e
        .apply(Operation::AddPrimitive { context: root, primitive: cube, placement: DAffine3::IDENTITY, material: None })
        .unwrap()
        .unwrap()
        .created[0];
    let row = e
        .apply(Operation::Array { context: root, entities: vec![a], step: DAffine3::from_translation(DVec3::X * 100.0), count: 99 })
        .unwrap()
        .unwrap()
        .created;
    let row: Vec<_> = std::iter::once(a).chain(row).collect();
    e.apply(Operation::Array { context: root, entities: row, step: DAffine3::from_translation(DVec3::Y * 100.0), count: 99 }).unwrap();
    for k in 0..20 {
        let plate = Primitive::Box { origin: DVec3::new(-50.0, -50.0, 40.0 + k as f64 * 30.0), size: DVec3::new(10_000.0, 10_000.0, 5.0) };
        e.apply(Operation::AddPrimitive { context: root, primitive: plate, placement: DAffine3::IDENTITY, material: None }).unwrap();
    }
    eprintln!("occurrences: {}", e.document().root_assembly().occurrences().len());
    e.set_tool(ToolKind::Move);
    for (view, parallel) in [(StandardView::Iso, false), (StandardView::Top, true)] {
        e.set_parallel(parallel);
        e.camera.set_standard_view(
            view,
            glam::DMat3::IDENTITY,
            Some((DVec3::ZERO, DVec3::new(10_000.0, 10_000.0, 700.0))),
            crate::camera::ViewArea::full(e.viewport_size()),
        );
        for k in 0..3 {
            let at = DVec2::new(480.0 + 7.0 * k as f64, 520.0);
            time(&format!("hover {view:?} {k} (move tool)"), || e.input(ToolInput::Hover { px: at }));
        }
    }
}

/// Opening a model of many distinct parts (5 000 definitions of different sizes, 4 copies
/// each), their meshes coming in a few at a time as the kernel thread finishes them: what
/// each frame that takes some in costs.
#[test]
#[ignore]
fn bench_meshes_coming_in() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use solidsketch_model::kernel::{Job, JobId, JobResult, KernelService, Solid};

    /// Holds the jobs; hands out `n` results when asked.
    #[derive(Clone, Default)]
    struct Trickle {
        jobs: Rc<RefCell<Vec<(JobId, Job)>>>,
        ready: Rc<RefCell<Vec<(JobId, JobResult)>>>,
    }
    impl Trickle {
        fn finish(&self, n: usize) -> usize {
            let mut jobs = self.jobs.borrow_mut();
            let take = n.min(jobs.len());
            let done: Vec<_> = jobs.drain(..take).map(|(id, job)| (id, solidsketch_kernel::run(&job))).collect();
            self.ready.borrow_mut().extend(done);
            take
        }
    }
    impl KernelService for Trickle {
        fn submit(&mut self, id: JobId, job: Job) {
            self.jobs.borrow_mut().push((id, job));
        }
        fn poll(&mut self) -> Vec<(JobId, JobResult)> {
            std::mem::take(&mut *self.ready.borrow_mut())
        }
    }

    let defs: usize = std::env::var("BENCH_DEFS").ok().and_then(|s| s.parse().ok()).unwrap_or(5000);
    let per_frame: usize = 20;
    // The export: box `k` is (100 + k mm) × 50 × 20, placed 4 times along a grid.
    let faces = r#"[{"normal":[0,0,-1],"loops":[[0,2,3,1]]},{"normal":[0,0,1],"loops":[[4,5,7,6]]},{"normal":[0,-1,0],"loops":[[0,1,5,4]]},{"normal":[0,1,0],"loops":[[2,6,7,3]]},{"normal":[-1,0,0],"loops":[[0,4,6,2]]},{"normal":[1,0,0],"loops":[[1,3,7,5]]}]"#;
    let mut definitions = Vec::with_capacity(defs);
    let mut instances = Vec::with_capacity(defs * 4);
    for k in 0..defs {
        let (x, y, z) = (100.0 + k as f64, 50.0, 20.0);
        let v: Vec<String> = (0..8)
            .map(|i| {
                format!("[{},{},{}]", if i & 1 == 0 { 0.0 } else { x }, if i & 2 == 0 { 0.0 } else { y }, if i & 4 == 0 { 0.0 } else { z })
            })
            .collect();
        definitions.push(format!(r#"{{"name":"P{k}","vertices":[{}],"faces":{faces}}}"#, v.join(",")));
        for c in 0..4 {
            let (tx, ty) = ((k % 100) as f64 * 300.0, ((k / 100) * 4 + c) as f64 * 80.0);
            instances.push(format!(r#"{{"definition":{k},"transform":[1,0,0,0,0,1,0,0,0,0,1,0,{tx},{ty},0,1]}}"#));
        }
    }
    let text = format!(
        r#"{{"format":"{}","version":1,"definitions":[{}],"root":{{"instances":[{}]}}}}"#,
        solidsketch_model::SKETCHUP_FORMAT,
        definitions.join(","),
        instances.join(",")
    );
    let (doc, _) = time("import", || Document::from_sketchup(&text, &Solid::from_polyhedron).unwrap());
    let kernel = Trickle::default();
    let mut e = time("open (stand-ins)", || Editor::new(doc, Box::new(kernel.clone())));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    eprintln!("parts: {defs}, placements: {}, meshes asked: {}", defs * 4, kernel.jobs.borrow().len());
    let (mut frames, mut total, mut worst) = (0, std::time::Duration::ZERO, std::time::Duration::ZERO);
    while kernel.finish(per_frame) > 0 {
        let t = Instant::now();
        e.poll();
        let spent = t.elapsed();
        frames += 1;
        total += spent;
        worst = worst.max(spent);
    }
    eprintln!(
        "{frames} frames taking meshes in ({per_frame} each): {:.2} ms each on average, {:.2} at worst, {:.0} ms in all",
        total.as_secs_f64() * 1e3 / frames as f64,
        worst.as_secs_f64() * 1e3,
        total.as_secs_f64() * 1e3
    );
}

/// One small edit (one occurrence moved) in a model of 40 000 placements, either of one
/// cube (`BENCH_DISTINCT` unset) or of 10 000 distinct parts placed 4 times: the document's
/// share, and the editor's in all (the scene synced). `BENCH_LOOP` set: moves it for a few
/// seconds, to profile (`sample <pid>`).
#[test]
#[ignore]
fn bench_small_edit() {
    use glam::DAffine3;
    use solidsketch_model::kernel::Solid;
    use solidsketch_model::{Operation, Primitive};
    let distinct = std::env::var("BENCH_DISTINCT").is_ok();
    let mut e = if distinct {
        let faces = r#"[{"normal":[0,0,-1],"loops":[[0,2,3,1]]},{"normal":[0,0,1],"loops":[[4,5,7,6]]},{"normal":[0,-1,0],"loops":[[0,1,5,4]]},{"normal":[0,1,0],"loops":[[2,6,7,3]]},{"normal":[-1,0,0],"loops":[[0,4,6,2]]},{"normal":[1,0,0],"loops":[[1,3,7,5]]}]"#;
        let (mut definitions, mut instances) = (Vec::new(), Vec::new());
        for k in 0..10_000 {
            let (x, y, z) = (100.0 + k as f64 * 0.01, 50.0, 20.0);
            let v: Vec<String> = (0..8)
                .map(|i| {
                    format!(
                        "[{},{},{}]",
                        if i & 1 == 0 { 0.0 } else { x },
                        if i & 2 == 0 { 0.0 } else { y },
                        if i & 4 == 0 { 0.0 } else { z }
                    )
                })
                .collect();
            definitions.push(format!(r#"{{"name":"P{k}","vertices":[{}],"faces":{faces}}}"#, v.join(",")));
            for c in 0..4 {
                let (tx, ty) = ((k % 100) as f64 * 300.0, ((k / 100) * 4 + c) as f64 * 80.0);
                instances.push(format!(r#"{{"definition":{k},"transform":[1,0,0,0,0,1,0,0,0,0,1,0,{tx},{ty},0,1]}}"#));
            }
        }
        let text = format!(
            r#"{{"format":"{}","version":1,"definitions":[{}],"root":{{"instances":[{}]}}}}"#,
            solidsketch_model::SKETCHUP_FORMAT,
            definitions.join(","),
            instances.join(",")
        );
        let (doc, _) = Document::from_sketchup(&text, &Solid::from_polyhedron).unwrap();
        Editor::new(doc, Box::new(solidsketch_kernel::Inline::default()))
    } else {
        let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Inline::default()));
        let root = e.document().root();
        let cube = Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(20.0) };
        let a = e
            .apply(Operation::AddPrimitive { context: root, primitive: cube, placement: DAffine3::IDENTITY, material: None })
            .unwrap()
            .unwrap()
            .created[0];
        let step = DAffine3::from_translation(DVec3::X * 100.0);
        let row = e.apply(Operation::Array { context: root, entities: vec![a], step, count: 199 }).unwrap().unwrap().created;
        let row: Vec<_> = std::iter::once(a).chain(row).collect();
        let step = DAffine3::from_translation(DVec3::Y * 100.0);
        e.apply(Operation::Array { context: root, entities: row, step, count: 199 }).unwrap();
        e
    };
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.poll();
    let root = e.document().root();
    let id = e.document().root_assembly().occurrences()[123].id;
    eprintln!("placements: {}", e.document().placed_parts().len());
    let op = |k: usize| Operation::Transform {
        context: root,
        entities: vec![id],
        transform: DAffine3::from_translation(DVec3::Z * if k.is_multiple_of(2) { 10.0 } else { -10.0 }),
    };
    for k in 0..4 {
        let doc = e.document().clone();
        time(&format!("move one: document only {k}"), || doc.apply(&op(k)).unwrap());
        time(&format!("move one: editor (scene synced) {k}"), || e.apply(op(k)).unwrap());
    }
    if std::env::var("BENCH_LOOP").is_ok() {
        let t = Instant::now();
        let mut k = 0;
        while t.elapsed().as_secs() < 8 {
            e.apply(op(k)).unwrap();
            k += 1;
        }
        eprintln!("loops: {k}");
    }
}

/// The life of a real model (`BENCH_STEP`: a STEP file's path): read, made a document, opened
/// with stand-ins, meshed (each solid timed; all of them on every core, as the worker does),
/// the meshes taken in, then drawn, hovered, all selected, one occurrence moved and undone.
#[test]
#[ignore]
fn bench_step_model() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use glam::DAffine3;
    use solidsketch_model::Operation;
    use solidsketch_model::kernel::{Job, JobId, JobResult, KernelService, Output, Task};

    /// Holds the jobs until the bench runs them.
    #[derive(Clone, Default)]
    struct Hold {
        jobs: Rc<RefCell<Vec<(JobId, Job)>>>,
        ready: Rc<RefCell<Vec<(JobId, JobResult)>>>,
    }
    impl KernelService for Hold {
        fn submit(&mut self, id: JobId, job: Job) {
            self.jobs.borrow_mut().push((id, job));
        }
        fn poll(&mut self) -> Vec<(JobId, JobResult)> {
            std::mem::take(&mut *self.ready.borrow_mut())
        }
    }

    let Ok(path) = std::env::var("BENCH_STEP") else {
        eprintln!("BENCH_STEP unset: nothing to do");
        return;
    };
    let bytes = time("read the file", || std::fs::read(&path).unwrap());
    eprintln!("{:.1} MB", bytes.len() as f64 / 1e6);
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes }] };
    let step = time("kernel: STEP read (B-reps)", || match solidsketch_kernel::run(&job).unwrap().pop() {
        Some(Output::Step(s)) => *s,
        _ => panic!("no STEP output"),
    });
    let (doc, report) = time("model: Document::from_step", || Document::from_step(&step, "model"));
    drop(step);
    eprintln!(
        "parts {}, assemblies {}, excluded {}, placements {}",
        report.parts,
        report.assemblies,
        report.excluded,
        doc.placed_parts().len()
    );
    let kernel = Hold::default();
    let mut e = time("open (stand-ins)", || Editor::new(doc, Box::new(kernel.clone())));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    // Each job alone (a CPU's share), then all at once on every core.
    let jobs: Vec<(JobId, Job)> = std::mem::take(&mut *kernel.jobs.borrow_mut());
    let mut each: Vec<Duration> = jobs
        .iter()
        .map(|(_, job)| {
            let t = Instant::now();
            std::hint::black_box(solidsketch_kernel::run(job).ok());
            t.elapsed()
        })
        .collect();
    let cpu: Duration = each.iter().sum();
    each.sort_unstable();
    eprintln!(
        "meshing: {} jobs, {:.0} ms of CPU, slowest {:.0} ms, median {:.1} ms",
        jobs.len(),
        cpu.as_secs_f64() * 1e3,
        each.last().map_or(0.0, |d| d.as_secs_f64() * 1e3),
        each.get(each.len() / 2).map_or(0.0, |d| d.as_secs_f64() * 1e3)
    );
    let results =
        time("meshing on every core", || solidsketch_kernel::parallel_map(&jobs, |(id, job)| (*id, solidsketch_kernel::run(job))));
    kernel.ready.borrow_mut().extend(results);
    time("meshes taken in (poll)", || e.poll());
    let (mut unique, mut drawn) = (0usize, 0usize);
    for (def, places) in e.scene().placements() {
        let tris: usize = e.scene().definition_mesh(def).map_or(0, |d| d.parts.iter().map(|p| p.mesh.indices.len() / 3).sum());
        unique += tris;
        drawn += tris * places.len();
    }
    eprintln!("triangles: {:.2} M meshed, {:.2} M drawn", unique as f64 / 1e6, drawn as f64 / 1e6);
    // The heaviest meshes: a few parts, or all of them?
    let mut heavy: Vec<(usize, usize, f64, String)> = e
        .scene()
        .definition_meshes()
        .filter_map(|d| {
            let part = d.parts.first()?;
            let size = part.bounds.map_or(0.0, |(lo, hi)| (hi - lo).length());
            let faces = part.mesh.tri_face.iter().max().map_or(0, |f| *f as usize + 1);
            let name = e.document().component(d.definition).map(|c| c.name.clone()).unwrap_or_default();
            Some((d.parts.iter().map(|p| p.mesh.indices.len() / 3).sum::<usize>(), faces, size, name))
        })
        .collect();
    heavy.sort_unstable_by_key(|h| std::cmp::Reverse(h.0));
    let total: usize = heavy.iter().map(|h| h.0).sum();
    for (k, (tris, faces, size, name)) in heavy.iter().take(8).enumerate() {
        eprintln!("  #{k} {tris:>9} triangles, {faces:>5} faces, {size:>8.1} mm across: {name}");
    }
    let top10: usize = heavy.iter().take(heavy.len() / 10).map(|h| h.0).sum();
    eprintln!("  the heaviest tenth of the parts holds {:.0} % of the triangles", top10 as f64 * 100.0 / total.max(1) as f64);
    let bounds = e.scene().bounds().unwrap();
    eprintln!("model size: {:.1} mm", bounds.1 - bounds.0);
    e.camera.set_standard_view(StandardView::Iso, glam::DMat3::IDENTITY, Some(bounds), crate::camera::ViewArea::full(e.viewport_size()));
    for k in 0..2 {
        time(&format!("view {k}"), || std::hint::black_box(e.view()));
    }
    e.set_tool(ToolKind::Move);
    for k in 0..3 {
        time(&format!("hover {k} (move tool)"), || e.input(ToolInput::Hover { px: DVec2::new(500.0 + 3.0 * k as f64, 500.0) }));
    }
    e.set_tool(ToolKind::Select);
    time("select_all", || e.select_all());
    time("view (all selected)", || std::hint::black_box(e.view()));
    e.select(vec![]);
    let root = e.document().root();
    let id = e.document().root_assembly().occurrences()[0].id;
    let op = Operation::Transform { context: root, entities: vec![id], transform: DAffine3::from_translation(DVec3::Z * 10.0) };
    time("move one occurrence", || e.apply(op).unwrap());
    time("undo", || e.undo());
}

/// A real model loaded as the desktop app loads it (`BENCH_STEP`: a STEP file's path): imported
/// through the editor, the kernel a [`solidsketch_kernel::Pool`] on every core — when the file
/// is read, when every preview is drawn, when the last final mesh is (§7 sexies). Then the
/// same load cancelled once the previews are drawn: how long until an operation gets through.
#[test]
#[ignore]
fn bench_step_loading() {
    use std::time::Duration;

    use glam::DAffine3;
    use solidsketch_model::{Operation, Primitive};

    let Ok(path) = std::env::var("BENCH_STEP") else {
        eprintln!("BENCH_STEP unset: nothing to do");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let load = |e: &mut Editor, stop_after_previews: bool| -> (Duration, Duration, Duration) {
        let t = Instant::now();
        e.import_step("model".into(), bytes.clone());
        let (mut read, mut previewed) = (None, None);
        while e.loading().is_some() && t.elapsed() < Duration::from_secs(600) {
            e.poll();
            let Some(l) = e.loading() else { break };
            if read.is_none() && l.reading.is_none() {
                read = Some(t.elapsed());
            }
            if previewed.is_none() && l.previews.1 > 0 && l.previews.0 == l.previews.1 {
                previewed = Some(t.elapsed());
                if stop_after_previews {
                    e.cancel_loading();
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        (read.unwrap_or_default(), previewed.unwrap_or_default(), t.elapsed())
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Pool::new(None)));
    let (read, previewed, done) = load(&mut e, false);
    let l = e.document().placed_parts().len();
    eprintln!(
        "{threads} threads: read {:.2} s, previews drawn {:.2} s, final meshes {:.2} s ({l} placements)",
        read.as_secs_f64(),
        previewed.as_secs_f64(),
        done.as_secs_f64()
    );

    // Cancelled once the previews are drawn: an operation right after it gets the kernel when
    // the jobs running stop.
    let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Pool::new(None)));
    let (_, previewed, cancelled) = load(&mut e, true);
    let root = e.document().root();
    let t = Instant::now();
    let op = Operation::AddPrimitive {
        context: root,
        primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::ONE },
        placement: DAffine3::IDENTITY,
        material: None,
    };
    let _ = e.apply(op);
    while e.busy() && t.elapsed() < Duration::from_secs(600) {
        e.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    eprintln!(
        "cancelled at {:.2} s (previews drawn at {:.2} s): an operation done {:.0} ms later; the import undone: {}",
        cancelled.as_secs_f64(),
        previewed.as_secs_f64(),
        t.elapsed().as_secs_f64() * 1e3,
        e.document().root_assembly().occurrences().len() == 1
    );
}

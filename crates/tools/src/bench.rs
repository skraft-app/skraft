//! Timings of an editor with many occurrences (ignored; run with
//! `cargo test -p skraft-tools --release bench -- --ignored --nocapture`).

use std::time::Instant;

use glam::{DVec2, DVec3};
use skraft_model::Document;

use crate::*;

fn time<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    eprintln!("{what:<40} {:>10.2} ms", t.elapsed().as_secs_f64() * 1e3);
    r
}

/// A SketchUp model read (what the kernel's `Task::ImportSketchUp` gives) of `defs` boxes, box
/// `k` `length(k)` × 50 × 20 mm, each placed 4 times along a grid; their solids built here.
fn boxes(defs: usize, length: impl Fn(usize) -> f64) -> skraft_kernel::SketchUpImport {
    use skraft_kernel::{PolyFace, Polyhedron, SketchUpChild, SketchUpDefinition, SketchUpImport, SketchUpNode, SketchUpPart};
    let faces: [([f64; 3], [u32; 4]); 6] = [
        ([0.0, 0.0, -1.0], [0, 2, 3, 1]),
        ([0.0, 0.0, 1.0], [4, 5, 7, 6]),
        ([0.0, -1.0, 0.0], [0, 1, 5, 4]),
        ([0.0, 1.0, 0.0], [2, 6, 7, 3]),
        ([-1.0, 0.0, 0.0], [0, 4, 6, 2]),
        ([1.0, 0.0, 0.0], [1, 3, 7, 5]),
    ];
    let mut skp = SketchUpImport::default();
    let mut top = Vec::with_capacity(defs * 4);
    for k in 0..defs {
        let size = DVec3::new(length(k), 50.0, 20.0);
        let poly = Polyhedron {
            vertices: (0..8).map(|i| DVec3::new((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64) * size).collect(),
            faces: faces.iter().map(|(n, l)| PolyFace { normal: DVec3::from_array(*n), loops: vec![l.to_vec()] }).collect(),
        };
        let solid = skraft_kernel::Solid::from_polyhedron(&poly).map_err(|e| e.to_string());
        skp.definitions.push(SketchUpDefinition { name: format!("P{k}"), ..Default::default() });
        skp.parts.push(SketchUpPart { definition: Some(k), loose: false, faces: 6, solid, painted: Vec::new(), time: Default::default() });
        skp.nodes.push(SketchUpNode { definition: Some(k), part: Some(k), children: Vec::new() });
        for c in 0..4 {
            let t = DVec3::new((k % 100) as f64 * 300.0, ((k / 100) * 4 + c) as f64 * 80.0, 0.0);
            top.push(SketchUpChild { node: k, transform: glam::DAffine3::from_translation(t), name: String::new(), material: None });
        }
    }
    skp.nodes.push(SketchUpNode { definition: None, part: None, children: top });
    skp.root = defs;
    skp
}

#[test]
#[ignore]
fn bench_array() {
    let n: usize = std::env::var("BENCH_N").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
    let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
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
    use skraft_model::{Operation, Primitive};
    let n: u32 = std::env::var("BENCH_N").ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
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
    use skraft_model::{Operation, Primitive};
    let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
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

    use skraft_model::kernel::{Job, JobId, JobResult, KernelService};

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
            let done: Vec<_> = jobs.drain(..take).map(|(id, job)| (id, skraft_kernel::run(&job))).collect();
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
    // Box `k` is (100 + k mm) × 50 × 20, placed 4 times along a grid.
    let skp = time("solids", || boxes(defs, |k| 100.0 + k as f64));
    let (doc, _) = time("import", || Document::from_sketchup(&skp, "bench.skp").unwrap());
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
    use skraft_model::{Operation, Primitive};
    let distinct = std::env::var("BENCH_DISTINCT").is_ok();
    let mut e = if distinct {
        let skp = boxes(10_000, |k| 100.0 + k as f64 * 0.01);
        let (doc, _) = Document::from_sketchup(&skp, "bench.skp").unwrap();
        Editor::new(doc, Box::new(skraft_kernel::Inline::default()))
    } else {
        let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
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
        // What the app does every frame after it (docs/model/undo.md#memory-cap).
        time(&format!("move one: history weighed {k}"), || e.history_bytes());
    }
    time("history weighed, nothing new", || e.history_bytes());
    time("history: forget one state", || e.forget_history(1));
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
    use skraft_model::Operation;
    use skraft_model::kernel::{Job, JobId, JobResult, KernelService, Output, Task};

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
    let step = time("kernel: STEP read (B-reps)", || match skraft_kernel::run(&job).unwrap().pop() {
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
            std::hint::black_box(skraft_kernel::run(job).ok());
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
    let results = time("meshing on every core", || skraft_kernel::parallel_map(&jobs, |(id, job)| (*id, skraft_kernel::run(job))));
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
/// through the editor, the kernel a [`skraft_kernel::Pool`] on every core — when the file
/// is read, when every preview is drawn, when the last final mesh is (docs/ui/loading.md). Then the
/// same load cancelled once the previews are drawn: how long until an operation gets through.
#[test]
#[ignore]
fn bench_step_loading() {
    use std::time::Duration;

    use glam::DAffine3;
    use skraft_model::{Operation, Primitive};

    let Ok(path) = std::env::var("BENCH_STEP") else {
        eprintln!("BENCH_STEP unset: nothing to do");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let load = |e: &mut Editor, stop_after_previews: bool| -> (Duration, Duration, Duration) {
        let t = Instant::now();
        e.import("model".into(), bytes.clone());
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
    let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Pool::new(None)));
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
    let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Pool::new(None)));
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

/// The viewport's rendering on a real model (`BENCH_STEP`: a STEP file's path), off-screen on
/// the GPU: its meshes and instances as the app gives them to the renderer, then frames drawn
/// while the view orbits, each waited for (`BENCH_FRAMES`, default 60), at the size of a
/// high-density laptop screen; and the meshes' GPU memory:
/// `BENCH_STEP=… cargo test --workspace bench_render_step -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_render_step() {
    use glam::DMat4;
    use skraft_model::kernel::{Job, JobId, JobResult, KernelService, Output, Task};
    use std::cell::RefCell;
    use std::rc::Rc;

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
    let frames: usize = std::env::var("BENCH_FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let required_features = adapter.features() & skraft_render::OPTIONAL_FEATURES;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features, ..Default::default() })).unwrap();
    eprintln!("adapter: {}", adapter.get_info().name);

    // The model, meshed on every core.
    let bytes = std::fs::read(&path).unwrap();
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes }] };
    let step = time("STEP read", || match skraft_kernel::run(&job).unwrap().pop() {
        Some(Output::Step(s)) => *s,
        _ => panic!("no STEP output"),
    });
    let (doc, _) = Document::from_step(&step, "model");
    drop(step);
    let kernel = Hold::default();
    let mut e = Editor::new(doc, Box::new(kernel.clone()));
    let size = [3024u32, 1964];
    let ppp = 2.0f32;
    e.set_viewport_size(DVec2::new(size[0] as f64, size[1] as f64) / ppp as f64);
    let jobs: Vec<(JobId, Job)> = std::mem::take(&mut *kernel.jobs.borrow_mut());
    let results = time("meshing on every core", || skraft_kernel::parallel_map(&jobs, |(id, job)| (*id, skraft_kernel::run(job))));
    kernel.ready.borrow_mut().extend(results);
    e.poll();

    // Uploaded as the viewport does.
    let mut r = skraft_render::Renderer::new(&device);
    time("meshes uploaded", || {
        for dm in e.scene().definition_meshes() {
            let parts: Vec<skraft_render::Part<'_>> =
                dm.parts.iter().map(|p| skraft_render::Part { id: p.solid.0, mesh: &p.mesh, face_paint: &[] }).collect();
            r.set_mesh(&device, dm.definition.0, dm.revision, &parts);
        }
    });
    let (meshes, bytes) = r.mesh_memory();
    let (tris, segments): (usize, usize) = e
        .scene()
        .definition_meshes()
        .flat_map(|d| &d.parts)
        .map(|p| (p.mesh.indices.len() / 3, p.mesh.edges.iter().map(|e| e.len().saturating_sub(1)).sum::<usize>()))
        .fold((0, 0), |(t, s), (a, b)| (t + a, s + b));
    eprintln!(
        "{meshes} meshes: {:.2} M triangles, {:.2} M edge segments, {:.1} MB on the GPU",
        tris as f64 / 1e6,
        segments as f64 / 1e6,
        bytes as f64 / 1e6
    );

    // Frames while orbiting about the model, the whole of it in view.
    let bounds = e.scene().bounds().unwrap();
    e.camera.set_standard_view(StandardView::Iso, glam::DMat3::IDENTITY, Some(bounds), crate::camera::ViewArea::full(e.viewport_size()));
    let view = e.view();
    let batches: Vec<skraft_render::Batch> = view
        .batches
        .iter()
        .map(|(def, places)| {
            skraft_render::Batch::new(def.0, places.iter().map(|p| skraft_render::Placement::new(DMat4::from(p.transform))).collect())
        })
        .collect();
    let instances: usize = batches.iter().map(|b| b.placements.len()).sum();
    eprintln!("{} batches, {instances} placements", batches.len());
    let pivot = (bounds.0 + bounds.1) * 0.5;
    let mut times = Vec::with_capacity(frames);
    let mut stats = None;
    for k in 0..frames + 3 {
        e.camera.orbit(DVec2::new(6.0, 0.0), pivot);
        let aspect = size[0] as f64 / size[1] as f64;
        let frame = skraft_render::Frame {
            view: e.camera.view(),
            proj: e.camera.proj(aspect, Some(bounds)),
            background: [1.0, 1.0, 1.0],
            // BENCH_NO_SILHOUETTES: without the outlines of curved surfaces.
            style: skraft_render::Style { silhouettes: std::env::var("BENCH_NO_SILHOUETTES").is_err(), ..Default::default() },
            batches: &batches,
            highlights: &[],
            lines: &[],
            axis_lines: &[],
            faint_lines: &[],
            boxes: &[],
            overlay: &[],
            hidden_lines: &[],
            fills: &[],
            grid: None,
            axes: false,
            axes_frame: DMat4::IDENTITY,
            pixels_per_point: ppp,
            model_per_point: 1.0,
        };
        let t = Instant::now();
        let mut enc = device.create_command_encoder(&Default::default());
        r.render(&device, &queue, &mut enc, size, &frame);
        queue.submit([enc.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        // The first frames make the target and the pipelines.
        if k >= 3 {
            times.push(t.elapsed().as_secs_f64() * 1e3);
        }
        stats = Some(r.stats());
    }
    times.sort_by(f64::total_cmp);
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    eprintln!(
        "{frames} frames at {}×{}: median {:.2} ms, mean {:.2} ms, fastest {:.2} ms, slowest {:.2} ms",
        size[0],
        size[1],
        times[times.len() / 2],
        mean,
        times[0],
        times[times.len() - 1]
    );
    if let Some(s) = stats {
        eprintln!("per frame: {} draw calls, {:.2} M triangles, {} instances", s.draw_calls, s.triangles as f64 / 1e6, s.instances);
    }
}

/// The drawing's hidden lines found on the meshes (docs/ui/drawing.md#lines) of a STEP model
/// (`BENCH_STEP`), the whole model seen from the front, top, right and iso:
/// `BENCH_STEP=… cargo test --workspace bench_hidden_lines -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_hidden_lines() {
    use skraft_model::kernel::{Job, JobId, JobResult, KernelService, Output, Task};
    use std::cell::RefCell;
    use std::rc::Rc;

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
    let bytes = std::fs::read(&path).unwrap();
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes }] };
    let step = time("STEP read", || match skraft_kernel::run(&job).unwrap().pop() {
        Some(Output::Step(s)) => *s,
        _ => panic!("no STEP output"),
    });
    let (doc, _) = Document::from_step(&step, "model");
    let kernel = Hold::default();
    let mut e = Editor::new(doc, Box::new(kernel.clone()));
    let jobs: Vec<(JobId, Job)> = std::mem::take(&mut *kernel.jobs.borrow_mut());
    let results = time("meshing on every core", || skraft_kernel::parallel_map(&jobs, |(id, job)| (*id, skraft_kernel::run(job))));
    kernel.ready.borrow_mut().extend(results);
    e.poll();
    let parts: Vec<_> = e.scene().parts().filter(|(p, _)| !p.hidden).map(|(p, m)| (m.mesh.clone(), p.transform)).collect();
    let tris: usize = parts.iter().map(|(m, _)| m.indices.len() / 3).sum();
    eprintln!("{} placed parts, {:.2} M triangles", parts.len(), tris as f64 / 1e6);
    let look = |forward: DVec3, up: DVec3| {
        let forward = forward.normalize();
        let right = forward.cross(up).normalize();
        crate::hidden_lines::Look { right, up: right.cross(forward), forward }
    };
    for (name, l) in [
        ("front", look(DVec3::Y, DVec3::Z)),
        ("top", look(-DVec3::Z, DVec3::Y)),
        ("right", look(-DVec3::X, DVec3::Z)),
        ("iso", look(DVec3::new(-1.0, 1.0, -1.0), DVec3::Z)),
    ] {
        let lines = time(&format!("hidden lines, {name}"), || crate::hidden_lines::hidden_lines(&parts, &l));
        eprintln!("  {} visible polylines, {} hidden, {} segments", lines.visible.len(), lines.hidden.len(), lines.segments());
    }
}

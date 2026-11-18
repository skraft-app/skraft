//! How many triangles a real model's display costs, and where they come from (ignored; run
//! with `BENCH_STEP=<file.step> cargo test -p skraft-kernel --release --test mesh_budget
//! -- --ignored --nocapture`).

use std::collections::BTreeMap;

use skraft_kernel::{Job, Output, Solid, StepImport, Task, run};
use solidcore_tess::MeshTolerance;

fn read(path: &str) -> StepImport {
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes: std::fs::read(path).unwrap() }] };
    match run(&job).unwrap().pop() {
        Some(Output::Step(s)) => *s,
        _ => panic!("no STEP output"),
    }
}

fn size(s: &Solid) -> f64 {
    s.bounds().map(|(lo, hi)| (hi - lo).length()).unwrap_or(100.0)
}

/// Triangles per face of `s` at `tol`.
fn per_face(s: &Solid, tol: &MeshTolerance) -> Vec<usize> {
    let (plan, _) = solidcore_tess::plan(s.record(), tol, size(s)).unwrap();
    let (m, _) = plan.evaluate();
    let mut n = vec![0; s.record().faces.len()];
    for f in &m.tri_face {
        n[*f as usize] += 1;
    }
    n
}

fn kind(s: &Solid, face: usize) -> String {
    let f = &s.record().faces[face];
    let k = format!("{:?}", f.surface);
    let k: String = k.chars().take_while(|c| c.is_alphanumeric()).collect();
    if f.nurbs.is_some() { format!("{k} (as NURBS)") } else { k }
}

#[test]
#[ignore]
fn mesh_budget() {
    let Ok(path) = std::env::var("BENCH_STEP") else { return };
    let step = read(&path);
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for x in &step.excluded {
        *reasons.entry(x.reason.clone()).or_default() += 1;
    }
    eprintln!("excluded bodies, by reason:");
    for (r, n) in &reasons {
        eprintln!("  {n:>4} × {r}");
    }
    let bodies: Vec<(&str, &Solid)> =
        step.products.iter().flat_map(|p| p.bodies.iter().map(move |b| (p.name.as_str(), &b.solid))).collect();
    // The heaviest body, face by face, at a few tolerances.
    let current = |s: &Solid| MeshTolerance::display_of(size(s));
    let (name, heavy) = *bodies.iter().max_by_key(|(_, s)| per_face(s, &current(s)).iter().sum::<usize>()).unwrap();
    let sz = size(heavy);
    eprintln!("\nheaviest body: {name}, {:.3} mm across, {} faces", sz, heavy.record().faces.len());
    let tols = [
        ("display 1/1000 (now)", current(heavy)),
        ("chord 1/1000, no angle", MeshTolerance::chord(sz * 1e-3)),
        ("display 1/300", MeshTolerance::display(sz / 300.0)),
        ("display 1/100", MeshTolerance::display(sz / 100.0)),
    ];
    let counts: Vec<Vec<usize>> = tols.iter().map(|(_, t)| per_face(heavy, t)).collect();
    eprintln!("{:>4} {:<28} {}", "face", "surface", tols.iter().map(|(n, _)| format!("{n:>24}")).collect::<String>());
    for f in 0..heavy.record().faces.len() {
        eprintln!("{f:>4} {:<28} {}", kind(heavy, f), counts.iter().map(|c| format!("{:>24}", c[f])).collect::<String>());
    }
    eprintln!("{:>4} {:<28} {}", "", "total", counts.iter().map(|c| format!("{:>24}", c.iter().sum::<usize>())).collect::<String>());
    let (_, stats) = solidcore_tess::plan(heavy.record(), &current(heavy), sz).unwrap();
    eprintln!(
        "plan: planar {}, structured {}, delaunay {}, lenient {}, unmeshed {}, over tolerance {}, edges capped {}",
        stats.planar,
        stats.structured,
        stats.delaunay,
        stats.lenient.len(),
        stats.unmeshed.len(),
        stats.over_tolerance.len(),
        stats.edges_capped.len()
    );
    for o in stats.over_tolerance.iter().take(5) {
        eprintln!("  over tolerance: {o:?}");
    }
    for c in stats.edges_capped.iter().take(5) {
        eprintln!("  edge capped: {c:?}");
    }
    // The heavy faces' splines: their nets, and how far they stand off flat.
    for (f, face) in heavy.record().faces.iter().enumerate() {
        if counts[0][f] < 1000 {
            continue;
        }
        if let solidcore_record::brep::Surface::Spline { u_knots, v_knots, points } = &face.surface {
            let all: Vec<glam::DVec3> = points.iter().flatten().map(|p| glam::DVec3::from_array(*p)).collect();
            let (lo, hi) = all.iter().fold((glam::DVec3::INFINITY, glam::DVec3::NEG_INFINITY), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            let distinct = |k: &[f64]| k.windows(2).filter(|w| w[1] > w[0]).count();
            eprintln!(
                "face {f}: net {}×{}, knots {}×{} ({}×{} spans), domain {:?}, net box {:.5?} mm",
                points.len(),
                points.first().map_or(0, |r| r.len()),
                u_knots.len(),
                v_knots.len(),
                distinct(u_knots),
                distinct(v_knots),
                face.domain,
                hi - lo
            );
        }
    }
}

/// Over the whole model: the triangles now, those spent on faces the planner refined to its
/// bound without reaching the tolerance, and the triangles once each such body is meshed again
/// at the sag it did reach.
#[test]
#[ignore]
fn mesh_budget_retry() {
    let Ok(path) = std::env::var("BENCH_STEP") else { return };
    let step = read(&path);
    let bodies: Vec<&Solid> = step.products.iter().flat_map(|p| p.bodies.iter().map(|b| &b.solid)).collect();
    let t = std::time::Instant::now();
    let rows = skraft_kernel::parallel_map(&bodies, |s| {
        let sz = size(s);
        let tol = MeshTolerance::display_of(sz);
        let Ok((plan, stats)) = solidcore_tess::plan(s.record(), &tol, sz) else { return (0, 0, 0, false) };
        let (m, _) = plan.evaluate();
        let now = m.triangles.len();
        let refined: Vec<&solidcore_tess::OverTolerance> =
            stats.over_tolerance.iter().filter(|o| o.caps.iter().any(|c| matches!(c, solidcore_tess::Cap::Refine))).collect();
        if refined.is_empty() {
            return (now, 0, now, false);
        }
        let wasted: usize = m.tri_face.iter().filter(|f| refined.iter().any(|o| o.face == **f as usize)).count();
        let reached = refined.iter().map(|o| o.sag).fold(0.0, f64::max);
        let retry = MeshTolerance::display(reached * 1.25);
        let after = solidcore_tess::plan(s.record(), &retry, sz).map_or(now, |(p, _)| p.evaluate().0.triangles.len());
        (now, wasted, after, true)
    });
    let now: usize = rows.iter().map(|r| r.0).sum();
    let wasted: usize = rows.iter().map(|r| r.1).sum();
    let after: usize = rows.iter().map(|r| r.2).sum();
    let hit = rows.iter().filter(|r| r.3).count();
    eprintln!(
        "{} bodies: {:.2} M triangles now; {hit} bodies refined to the bound, {:.2} M triangles on those faces; {:.2} M once those are meshed at the sag reached ({:.1} s)",
        bodies.len(),
        now as f64 / 1e6,
        wasted as f64 / 1e6,
        after as f64 / 1e6,
        t.elapsed().as_secs_f64()
    );
}

/// Where the meshing time goes (one thread, every body): planning, evaluating, and the plan
/// again at the sag reached for the bodies refined to the bound.
#[test]
#[ignore]
fn mesh_budget_time() {
    let Ok(path) = std::env::var("BENCH_STEP") else { return };
    let step = read(&path);
    let bodies: Vec<&Solid> = step.products.iter().flat_map(|p| p.bodies.iter().map(|b| &b.solid)).collect();
    let (mut plan_hit, mut eval_hit, mut plan_rest, mut eval_rest, mut retry) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for s in bodies {
        let sz = size(s);
        let t = std::time::Instant::now();
        let Ok((plan, stats)) = solidcore_tess::plan(s.record(), &MeshTolerance::display_of(sz), sz) else { continue };
        let p = t.elapsed().as_secs_f64();
        let t = std::time::Instant::now();
        std::hint::black_box(plan.evaluate());
        let e = t.elapsed().as_secs_f64();
        let reached = stats
            .over_tolerance
            .iter()
            .filter(|o| o.caps.iter().any(|c| matches!(c, solidcore_tess::Cap::Refine)))
            .map(|o| o.sag)
            .fold(0.0, f64::max);
        if reached > 0.0 {
            plan_hit += p;
            eval_hit += e;
            let t = std::time::Instant::now();
            if let Ok((plan, _)) = solidcore_tess::plan(s.record(), &MeshTolerance::display(reached * 1.25), sz) {
                std::hint::black_box(plan.evaluate());
            }
            retry += t.elapsed().as_secs_f64();
        } else {
            plan_rest += p;
            eval_rest += e;
        }
    }
    eprintln!(
        "refined to the bound: plan {plan_hit:.1} s + evaluate {eval_hit:.1} s, again at the sag reached {retry:.1} s; the rest: plan {plan_rest:.1} s + evaluate {eval_rest:.1} s"
    );
}

/// How many bodies are the same geometry, the STEP file holding them as separate products.
#[test]
#[ignore]
fn mesh_budget_duplicates() {
    use std::collections::HashMap;
    let Ok(path) = std::env::var("BENCH_STEP") else { return };
    let step = read(&path);
    let bodies: Vec<&Solid> = step.products.iter().flat_map(|p| p.bodies.iter().map(|b| &b.solid)).collect();
    let mut groups: HashMap<Vec<u8>, usize> = HashMap::new();
    for s in &bodies {
        *groups.entry(rmp_serde::to_vec(s.record()).unwrap()).or_default() += 1;
    }
    let mut sizes: Vec<usize> = groups.values().copied().collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    eprintln!("{} bodies, {} distinct records; largest groups {:?}", bodies.len(), groups.len(), &sizes[..sizes.len().min(10)]);
}

/// Writes the heaviest body's record, and the same with only its heaviest face, as JSON to
/// `BENCH_DUMP` (a directory): a reproduction to hand on.
#[test]
#[ignore]
fn mesh_budget_dump() {
    let (Ok(path), Ok(dir)) = (std::env::var("BENCH_STEP"), std::env::var("BENCH_DUMP")) else { return };
    let step = read(&path);
    let current = |s: &Solid| MeshTolerance::display_of(size(s));
    let bodies: Vec<&Solid> = step.products.iter().flat_map(|p| p.bodies.iter().map(|b| &b.solid)).collect();
    let heavy = *bodies.iter().max_by_key(|s| per_face(s, &current(s)).iter().sum::<usize>()).unwrap();
    let counts = per_face(heavy, &current(heavy));
    let face = (0..counts.len()).max_by_key(|f| counts[*f]).unwrap();
    std::fs::write(format!("{dir}/body.json"), serde_json::to_string(heavy.record()).unwrap()).unwrap();
    let mut one = heavy.record().clone();
    one.faces = vec![one.faces[face].clone()];
    std::fs::write(format!("{dir}/face.json"), serde_json::to_string(&one).unwrap()).unwrap();
    let sz = size(heavy);
    let alone = solidcore_tess::plan(&one, &MeshTolerance::display_of(sz), sz)
        .map(|(p, s)| (p.evaluate().0.triangles.len(), s.over_tolerance.len()));
    eprintln!(
        "body: {} triangles; face {face} alone: {alone:?} (triangles, faces over tolerance), size {sz}",
        counts.iter().sum::<usize>()
    );
}

/// What a coarse first mesh would cost (one thread, every body): planned and evaluated at the
/// preview tolerance of `solidcore_tess::preview` (a hundredth of the body), and its triangles.
#[test]
#[ignore]
fn mesh_budget_preview() {
    let Ok(path) = std::env::var("BENCH_STEP") else { return };
    let step = read(&path);
    let bodies: Vec<&Solid> = step.products.iter().flat_map(|p| p.bodies.iter().map(|b| &b.solid)).collect();
    let (mut time, mut tris, mut slowest) = (0.0, 0, 0.0f64);
    for s in bodies {
        let sz = size(s);
        let t = std::time::Instant::now();
        if let Ok((plan, _)) = solidcore_tess::plan(s.record(), &solidcore_tess::preview::tolerance(sz, 1), sz) {
            tris += plan.evaluate().0.triangles.len();
        }
        let e = t.elapsed().as_secs_f64();
        time += e;
        slowest = slowest.max(e);
    }
    eprintln!("preview meshes: {time:.2} s on one thread (slowest {:.0} ms), {:.2} M triangles", slowest * 1e3, tris as f64 / 1e6);
}

/// What of a real model is not drawn: the bodies left out at reading (by product, with why), and
/// those read whose display the mesher refuses or leaves empty (preview and final), the largest
/// first. A big part missing in the app is in one list or the other.
#[test]
#[ignore]
fn mesh_budget_missing() {
    let Ok(path) = std::env::var("BENCH_STEP") else { return };
    let step = read(&path);
    eprintln!("left out at reading: {}", step.excluded.len());
    for x in &step.excluded {
        let reason: String = x.reason.chars().take(400).collect();
        eprintln!("  {:?} / {:?}: {reason}", x.product, x.body);
    }
    let bodies: Vec<(&str, &str, &Solid)> =
        step.products.iter().flat_map(|p| p.bodies.iter().map(move |b| (p.name.as_str(), b.name.as_str(), &b.solid))).collect();
    let mut missing: Vec<(f64, String)> = Vec::new();
    let mut biggest: Vec<(f64, String)> = Vec::new();
    for (product, body, s) in bodies {
        let sz = size(s);
        let faces = s.record().faces.len();
        biggest.push((sz, format!("{product:?} / {body:?}: {faces} faces")));
        for q in [skraft_kernel::Quality::Preview, skraft_kernel::Quality::Final] {
            match s.display_at(q) {
                Ok(d) if d.mesh.indices.is_empty() => missing.push((sz, format!("{q:?} empty: {product:?} / {body:?} ({faces} faces)"))),
                Ok(_) => {}
                Err(e) => {
                    let e: String = e.to_string().chars().take(160).collect();
                    missing.push((sz, format!("{q:?} refused: {product:?} / {body:?} ({faces} faces): {e}")))
                }
            }
        }
    }
    missing.sort_by(|a, b| b.0.total_cmp(&a.0));
    eprintln!("\nread but not drawn: {}", missing.len());
    for (sz, m) in &missing {
        eprintln!("  {sz:8.2} mm  {m}");
    }
    biggest.sort_by(|a, b| b.0.total_cmp(&a.0));
    eprintln!("\nthe largest bodies read:");
    for (sz, m) in biggest.iter().take(12) {
        eprintln!("  {sz:8.2} mm  {m}");
    }
}

/// The bodies a real model leaves out at reading, and the largest read (no meshing: quick).
#[test]
#[ignore]
fn mesh_budget_excluded() {
    let Ok(path) = std::env::var("BENCH_STEP") else { return };
    let step = read(&path);
    eprintln!("left out at reading: {}", step.excluded.len());
    for x in &step.excluded {
        let reason: String = x.reason.chars().take(200).collect();
        eprintln!("  {:?}: {reason}", x.product);
    }
    let mut sizes: Vec<(f64, String, usize)> = step
        .products
        .iter()
        .flat_map(|p| p.bodies.iter().map(move |b| (size(&b.solid), p.name.clone(), b.solid.record().faces.len())))
        .collect();
    sizes.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (sz, name, faces) in sizes.iter().take(5) {
        eprintln!("  {sz:8.2} mm  {name:?}: {faces} faces");
    }
}

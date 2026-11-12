//! Converts a SketchUp export (the JSON `scripts/sketchup/export.rb` writes) to a
//! `.solidsketch` file, and says what it did:
//!
//! ```sh
//! cargo run --release -p solidsketch-import -- model.json [model.solidsketch]
//! ```
//!
//! A developer tool (its output in English): the app will import from its own menu later.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use solidsketch_kernel::Solid;
use solidsketch_model::Document;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(input) = args.first().map(PathBuf::from) else {
        eprintln!("usage: solidsketch-import <export.json> [<out.solidsketch>]");
        return ExitCode::FAILURE;
    };
    let output = args.get(1).map(PathBuf::from).unwrap_or_else(|| input.with_extension("solidsketch"));
    let is_step =
        input.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"));
    if is_step {
        return step(&input, &output);
    }

    let t = Instant::now();
    let text = match std::fs::read_to_string(&input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    let read = t.elapsed();
    let (doc, report) = match Document::from_sketchup(&text, &Solid::from_polyhedron) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    let t = Instant::now();
    let json = match doc.to_json() {
        Ok(j) => j,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::write(&output, &json) {
        eprintln!("{}: {e}", output.display());
        return ExitCode::FAILURE;
    }
    let write = t.elapsed();
    // What opening the file costs before any display: reading the document back.
    let t = Instant::now();
    let reopened = Document::from_json(&json).map(|d| d.placed_parts().len());
    let reopen = t.elapsed();

    let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
    println!("{} → {}", input.display(), output.display());
    println!("  parts {:>8}   assemblies {:>6}   occurrences {:>8}", report.parts, report.assemblies, report.occurrences);
    println!("  placed parts {:>8}   faces {:>10}", report.placed_parts, report.faces);
    println!("  read {:>9.1} ms   parse {:>9.1} ms   build {:>9.1} ms", ms(read), ms(report.parse), ms(report.build));
    println!("  assemble {:>5.1} ms   write {:>9.1} ms   ({:.1} MB)", ms(report.assemble), ms(write), json.len() as f64 / 1e6);
    match reopened {
        Ok(n) => println!("  reopen {:>7.1} ms   ({n} placed parts)", ms(reopen)),
        Err(e) => println!("  reopen failed: {e}"),
    }
    println!("  slowest builds:");
    for (t, name, faces, ok) in &report.slowest {
        println!("    {:>9.1} ms  {:>6} faces  {}{name}", ms(*t), faces, if *ok { "" } else { "(excluded) " });
    }
    if report.dropped_instances > 0 {
        println!("  instances dropped (under excluded definitions): {}", report.dropped_instances);
    }
    if !report.excluded.is_empty() {
        println!("  excluded (not a closed solid): {}", report.excluded.len());
        for (name, why) in &report.excluded {
            println!("    {name}: {why}");
        }
    }
    ExitCode::SUCCESS
}

/// A STEP file read as the app reads it (the kernel's `Task::ImportStep`, then
/// `Document::from_step` and `Operation::Insert` into an empty model), saved, and reported.
fn step(input: &std::path::Path, output: &std::path::Path) -> ExitCode {
    use solidsketch_kernel::{Job, Output, Task};
    use solidsketch_model::{Operation, Prepared};

    let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
    let bytes = match std::fs::read(input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    let t = Instant::now();
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes }] };
    let step = match solidsketch_kernel::run(&job).map(|mut o| o.pop()) {
        Ok(Some(Output::Step(s))) => s,
        Ok(_) => {
            eprintln!("unexpected kernel output");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("{}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    let read = t.elapsed();
    let name = input.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let t = Instant::now();
    let (fragment, report) = Document::from_step(&step, &name);
    let doc = Document::new();
    let op = Operation::Insert { context: doc.root(), fragment: std::sync::Arc::new(fragment), transform: glam::DAffine3::IDENTITY };
    let doc = match doc.prepare(&op) {
        Ok(Prepared::Done(a)) => a.document,
        Ok(Prepared::Kernel(_)) => unreachable!("an insert needs no kernel"),
        Err(e) => {
            eprintln!("insert: {e}");
            return ExitCode::FAILURE;
        }
    };
    let insert = t.elapsed();
    let json = match doc.to_json() {
        Ok(j) => j,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::write(output, &json) {
        eprintln!("{}: {e}", output.display());
        return ExitCode::FAILURE;
    }

    println!("{} → {}", input.display(), output.display());
    println!(
        "  products {:>5}   parts {:>5}   assemblies {:>5}   placed parts {:>6}",
        step.products.len(),
        report.parts,
        report.assemblies,
        doc.placed_parts().len()
    );
    let faces: usize = step.products.iter().flat_map(|p| &p.bodies).map(|b| b.solid.topology_counts().0).sum();
    println!(
        "  bodies {:>7}   faces {:>7}   materials {:>3}",
        step.products.iter().map(|p| p.bodies.len()).sum::<usize>(),
        faces,
        doc.materials().count()
    );
    println!("  read + build {:>8.1} ms   insert {:>6.1} ms   ({:.1} MB)", ms(read), ms(insert), json.len() as f64 / 1e6);
    println!("  tree:");
    fn tree(step: &solidsketch_kernel::StepImport, product: usize, name: &str, depth: usize, budget: &mut usize) {
        if *budget == 0 || depth > 12 {
            return;
        }
        *budget -= 1;
        let p = &step.products[product];
        let colors: Vec<String> =
            p.bodies.iter().filter_map(|b| b.color).map(|c| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])).collect();
        println!(
            "    {}{}{} — {} bodies{}",
            "  ".repeat(depth),
            p.name,
            if name.is_empty() { String::new() } else { format!(" «{name}»") },
            p.bodies.len(),
            if colors.is_empty() { String::new() } else { format!(" {}", colors.join(" ")) }
        );
        for c in &p.children {
            tree(step, c.product, &c.name, depth + 1, budget);
        }
    }
    let mut budget = 40;
    for r in &step.roots {
        tree(&step, r.product, &r.name, 0, &mut budget);
    }
    // Where the parts land: their world boxes (an assembly not applied piles them at 0).
    println!("  placed parts (world box, mm):");
    for p in doc.placed_parts().iter().take(20) {
        let Ok((lo, hi)) = p.shape.bounds() else { continue };
        let corners = (0..8).map(|i| {
            p.transform.transform_point3(glam::DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            ))
        });
        let (wlo, whi) = corners.fold((glam::DVec3::INFINITY, glam::DVec3::NEG_INFINITY), |(a, b), c| (a.min(c), b.max(c)));
        let name = doc.component(p.part).map_or("?", |c| c.name.as_str());
        println!("    {name:<32} {:>8.1} {:>8.1} {:>8.1}  →  {:>8.1} {:>8.1} {:>8.1}", wlo.x, wlo.y, wlo.z, whi.x, whi.y, whi.z);
    }
    if !step.excluded.is_empty() {
        println!("  excluded: {}", step.excluded.len());
        for x in step.excluded.iter().take(20) {
            println!("    {} / {}: {}", x.product, x.body, x.reason);
        }
    }
    if !step.diagnostics.is_empty() {
        println!("  diagnostics:");
        for d in step.diagnostics.iter().take(20) {
            println!("    {d}");
        }
    }
    ExitCode::SUCCESS
}

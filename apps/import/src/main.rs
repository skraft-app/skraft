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

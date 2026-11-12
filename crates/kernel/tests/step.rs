//! Reading a STEP file through the kernel (`Task::ImportStep`): bodies, structure, colours.

use glam::DVec3;
use solidsketch_kernel::{Job, Output, StepImport, Task, run};

fn import(bytes: &[u8]) -> StepImport {
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes: bytes.to_vec() }] };
    match run(&job).unwrap().pop() {
        Some(Output::Step(s)) => *s,
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_assembly_keeps_its_structure() {
    let s = import(include_bytes!("fixtures/assembly.step"));
    assert_eq!(s.excluded.len(), 1, "{:?}", s.excluded);
    assert_eq!(s.excluded[0].body, "Sheet");

    let board = s.products.iter().position(|p| p.name == "Planche é").unwrap();
    let p = &s.products[board];
    assert_eq!(p.bodies.len(), 1);
    assert_eq!(p.bodies[0].name, "Board");
    assert_eq!(p.bodies[0].color, Some([204, 153, 102]));
    // In mm: the file is in cm.
    let (lo, hi) = p.bodies[0].solid.bounds().unwrap();
    assert!(((hi - lo) - DVec3::new(600.0, 300.0, 18.0)).length() < 1e-6, "{lo} {hi}");

    assert_eq!(s.roots.len(), 1);
    let cabinet = &s.products[s.roots[0].product];
    assert_eq!(cabinet.name, "Caisson");
    assert!(cabinet.bodies.is_empty());
    assert_eq!(cabinet.children.len(), 2);
    let high = cabinet.children.iter().find(|c| c.name == "Haut").unwrap();
    assert_eq!(high.product, board);
    // A quarter turn about Z, 500 mm up: the board's far corner (600, 0, 0) lands on (0, 600, 500).
    let p = high.transform.transform_point3(DVec3::new(600.0, 0.0, 0.0));
    assert!((p - DVec3::new(0.0, 600.0, 500.0)).length() < 1e-9, "{p}");
}

#[test]
fn garbage_is_an_error() {
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes: b"\x00\xff not a step file".to_vec() }] };
    assert!(run(&job).is_err());
}

#[test]
fn the_task_crosses_to_the_worker_as_bytes() {
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes: vec![1, 2, 3, 250] }] };
    let encoded = solidsketch_kernel::encode(&job).unwrap();
    assert!(encoded.len() < 64, "{} bytes", encoded.len());
    let back: Job = solidsketch_kernel::decode(&encoded).unwrap();
    assert_eq!(back.tasks, job.tasks);
}

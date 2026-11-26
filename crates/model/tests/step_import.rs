//! A STEP file read by the kernel, turned into a fragment and inserted (`Operation::Insert`).

use std::sync::Arc;

use glam::{DAffine3, DVec3};
use skraft_model::kernel::{Job, Output, StepImport, Task, run};
use skraft_model::{Document, MaterialKind, Operation, Outcome, Prepared, Source};

/// Applies an operation that needs no kernel.
fn apply(doc: &Document, op: &Operation) -> (Document, Outcome) {
    match doc.prepare(op).unwrap() {
        Prepared::Done(a) => (a.document, a.outcome),
        Prepared::Kernel(_) => panic!("an insert needs no kernel"),
    }
}

fn read(bytes: &[u8]) -> StepImport {
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportStep { bytes: bytes.to_vec() }] };
    match run(&job).unwrap().pop() {
        Some(Output::Step(s)) => *s,
        other => panic!("{other:?}"),
    }
}

const ASSEMBLY: &[u8] = include_bytes!("../../kernel/tests/fixtures/assembly.step");

#[test]
fn a_step_file_becomes_parts_and_assemblies() {
    let (frag, report) = Document::from_step(&read(ASSEMBLY), "assembly.step");
    assert_eq!((report.parts, report.assemblies, report.excluded), (1, 1, 1));
    let board = frag.components().find(|c| c.name == "Planche é").unwrap();
    assert!(board.is_part() && board.named);
    assert_eq!(frag.occurrence_count(board.id), 2);
    // The body's colour on its part, which both occurrences show.
    let m = frag.material(board.as_part().unwrap().coloration.unwrap()).unwrap();
    assert_eq!(m.color, [204, 153, 102]);
    assert_eq!(frag.materials().count(), 1);
    assert_eq!(frag.root_assembly().occurrences().len(), 1);
}

#[test]
fn a_product_bought_and_its_material_designation_are_read() {
    // The board bought (AP214's source on its formation), its material designated through its
    // shape; the cabinet made.
    let text = String::from_utf8(ASSEMBLY.to_vec()).unwrap();
    let text = text
        .replace(
            "#111=PRODUCT_DEFINITION_FORMATION('','',#110);",
            "#111=PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#110,.BOUGHT.);",
        )
        .replace(
            "#121=PRODUCT_DEFINITION_FORMATION('','',#120);",
            "#121=PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#120,.MADE.);",
        )
        .replace("ENDSEC;\nEND-ISO", "#170=MATERIAL_DESIGNATION('Ch\\X\\EAne',(#113));\nENDSEC;\nEND-ISO");
    assert!(text.contains("#170="), "the designation added");
    let (frag, _) = Document::from_step(&read(text.as_bytes()), "assembly.step");
    let board = frag.components().find(|c| c.name == "Planche é").unwrap();
    assert_eq!(board.source, Source::Bought);
    let oak = frag.material(board.as_part().unwrap().material.unwrap()).unwrap();
    assert_eq!((oak.name.as_str(), oak.kind), ("Chêne", Some(MaterialKind::Generic)));
    assert!(board.as_part().unwrap().coloration.is_some(), "its colour still a coloration over it");
    let cabinet = frag.components().find(|c| c.name == "Caisson").unwrap();
    assert_eq!(cabinet.source, Source::Made);
    // Without them: made, no material.
    let (frag, _) = Document::from_step(&read(ASSEMBLY), "assembly.step");
    let board = frag.components().find(|c| c.name == "Planche é").unwrap();
    assert_eq!((board.source, board.as_part().unwrap().material), (Source::Made, None));
}

#[test]
fn inserting_places_the_file_in_the_context_once() {
    let (frag, _) = Document::from_step(&read(ASSEMBLY), "assembly.step");
    let frag = Arc::new(frag);
    let doc = Document::new();
    let at = DAffine3::from_translation(DVec3::new(1000.0, 0.0, 0.0));
    let op = Operation::Insert { context: doc.root(), fragment: frag.clone(), transform: at };
    let (doc, outcome) = apply(&doc, &op);
    assert_eq!(outcome.created.len(), 1);
    // The file's single top product is placed itself, not wrapped.
    let placed = &doc.root_assembly().occurrences()[0];
    assert_eq!(doc.component(placed.component).unwrap().name, "Caisson");
    assert_eq!(placed.transform, at);
    assert_eq!(doc.placed_parts().len(), 2);

    // Twice: names made unique, the material shared, ids fresh.
    let (doc, _) = apply(&doc, &Operation::Insert { context: doc.root(), fragment: frag, transform: DAffine3::IDENTITY });
    assert_eq!(doc.placed_parts().len(), 4);
    assert!(doc.components().any(|c| c.name == "Caisson#2"));
    assert_eq!(doc.materials().count(), 1);
    // It saves and reads back (the file's checks pass: ids, names, materials).
    let back = Document::from_json(&doc.to_json().unwrap()).unwrap();
    assert_eq!(back.placed_parts().len(), 4);
    // The operation itself is plain data.
    let json = serde_json::to_string(&op).unwrap();
    let again: Operation = serde_json::from_str(&json).unwrap();
    assert_eq!(again, op);
}

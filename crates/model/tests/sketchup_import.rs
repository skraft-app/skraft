//! Importing a SketchUp export (`Document::from_sketchup`).

use glam::{DAffine3, DMat3, DVec3};
use serde_json::{Value, json};
use skraft_model::kernel::Solid;
use skraft_model::{Document, ImportReport};

/// The vertices and faces of a box `0..size`, as the exporter writes them.
fn cuboid(size: [f64; 3]) -> Value {
    let v: Vec<[f64; 3]> = (0..8)
        .map(|i| [if i & 1 == 0 { 0.0 } else { size[0] }, if i & 2 == 0 { 0.0 } else { size[1] }, if i & 4 == 0 { 0.0 } else { size[2] }])
        .collect();
    let faces = json!([
        { "normal": [0, 0, -1], "loops": [[0, 2, 3, 1]] },
        { "normal": [0, 0, 1], "loops": [[4, 5, 7, 6]] },
        { "normal": [0, -1, 0], "loops": [[0, 1, 5, 4]] },
        { "normal": [0, 1, 0], "loops": [[2, 6, 7, 3]] },
        { "normal": [-1, 0, 0], "loops": [[0, 4, 6, 2]] },
        { "normal": [1, 0, 0], "loops": [[1, 3, 7, 5]] },
    ]);
    json!({ "vertices": v, "faces": faces })
}

fn transform(t: DAffine3) -> Vec<f64> {
    glam::DMat4::from(t).to_cols_array().to_vec()
}

fn instance(definition: usize, t: DAffine3, name: &str, material: Option<usize>) -> Value {
    json!({ "definition": definition, "name": name, "transform": transform(t), "material": material })
}

fn import(export: Value) -> (Document, ImportReport) {
    Document::from_sketchup(&export.to_string(), &Solid::from_polyhedron).unwrap()
}

fn with(mut base: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

/// A board (0) placed twice in a cabinet (1), once mirrored; the cabinet placed twice in the
/// model; a board stretched ×2 along X; an open box (2), excluded.
fn cabinet() -> Value {
    let board = with(cuboid([600.0, 300.0, 18.0]), json!({ "name": "Planche", "no_scale_mask": 4 }));
    let mut open = cuboid([10.0, 10.0, 10.0]);
    open["faces"].as_array_mut().unwrap().remove(1);
    let open = with(open, json!({ "name": "Groupe#1", "group": true }));
    let mirror = DAffine3::from_translation(DVec3::new(0.0, 0.0, 500.0)) * DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0));
    let cab = json!({
        "name": "Caisson",
        "instances": [
            instance(0, DAffine3::IDENTITY, "Bas", None),
            instance(0, mirror, "Haut", Some(0)),
            instance(2, DAffine3::IDENTITY, "", None),
        ],
    });
    json!({
        "format": "skraft-sketchup-export",
        "version": 1,
        "materials": [{ "name": "Chêne", "color": [180, 140, 90] }],
        "definitions": [board, cab, open],
        "root": {
            "instances": [
                instance(1, DAffine3::IDENTITY, "", None),
                instance(1, DAffine3::from_translation(DVec3::X * 1000.0), "", None),
                instance(0, DAffine3::from_translation(DVec3::Y * 1000.0) * DAffine3::from_scale(DVec3::new(2.0, 1.0, 1.0)), "Longue", None),
            ],
        },
    })
}

#[test]
fn definitions_become_shared_parts_and_assemblies() {
    let (doc, report) = import(cabinet());
    assert_eq!(report.excluded.len(), 1, "{:?}", report.excluded);
    assert_eq!(report.excluded[0].0, "Groupe#1");
    // Planche, Planche at ×2, Caisson.
    assert_eq!((report.parts, report.assemblies), (2, 1));
    assert_eq!(report.dropped_instances, 1);
    assert_eq!(report.placed_parts, 5);
    let names: Vec<&str> = doc.components().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Planche") && names.contains(&"Planche#2") && names.contains(&"Caisson"), "{names:?}");

    let board = doc.components().find(|c| c.name == "Planche").unwrap();
    assert!(board.named);
    assert_eq!(board.behavior.no_stretch.0, 4);
    assert_eq!(doc.occurrence_count(board.id), 2, "the two boards of the cabinet share one part");

    // The mirror stays in T, the occurrence keeps its material.
    let cab = doc.components().find(|c| c.name == "Caisson").unwrap().as_assembly().unwrap();
    let high = cab.occurrences().iter().find(|o| o.name == "Haut").unwrap();
    assert!(high.transform.matrix3.determinant() < 0.0);
    assert_eq!(doc.material(high.material.unwrap()).unwrap().name, "Chêne");

    // The scale is in the geometry: a part 1200 long, placed by a translation only.
    let long = doc.components().find(|c| c.name == "Planche#2").unwrap();
    let (lo, hi) = long.as_part().unwrap().shape().bounds().unwrap();
    assert!(((hi - lo) - DVec3::new(1200.0, 300.0, 18.0)).length() < 1e-6);
    let top = doc.root_assembly().occurrences().iter().find(|o| o.name == "Longue").unwrap();
    assert_eq!(top.transform.matrix3, DMat3::IDENTITY);

    // It saves and reads back as any document.
    let back = Document::from_json(&doc.to_json().unwrap()).unwrap();
    assert_eq!(back.placed_parts().len(), 5);
}

#[test]
fn faces_beside_instances_make_a_part_in_the_assembly() {
    let mixed = with(
        cuboid([100.0, 100.0, 100.0]),
        json!({ "name": "Socle", "instances": [instance(1, DAffine3::from_translation(DVec3::Z * 100.0), "", None)] }),
    );
    let leg = with(cuboid([10.0, 10.0, 50.0]), json!({ "name": "Pied" }));
    let export = json!({
        "format": "skraft-sketchup-export",
        "version": 1,
        "definitions": [mixed, leg],
        "root": with(cuboid([5.0, 5.0, 5.0]), json!({ "instances": [instance(0, DAffine3::IDENTITY, "", None)] })),
    });
    let (doc, report) = import(export);
    assert!(report.excluded.is_empty(), "{:?}", report.excluded);
    assert_eq!((report.parts, report.assemblies), (3, 1));
    let socle = doc.components().find(|c| c.name == "Socle").unwrap();
    assert!(!socle.is_part());
    assert!(doc.components().any(|c| c.name == "Socle (faces)" && c.is_part()));
    assert_eq!(socle.as_assembly().unwrap().occurrences().len(), 2);
    // The model's loose faces: one part straight in the root.
    assert_eq!(doc.root_assembly().occurrences().len(), 2);
}

#[test]
fn a_wrong_format_is_refused() {
    let r = Document::from_sketchup(r#"{"format":"x","version":1,"root":{}}"#, &Solid::from_polyhedron);
    assert!(r.is_err());
}

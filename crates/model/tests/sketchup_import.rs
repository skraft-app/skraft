//! Importing a SketchUp file: the kernel's `Task::ImportSketchUp`, then
//! `Document::from_sketchup` (docs/io/sketchup-import.md).
//!
//! `cabinet.skp` was drawn in SketchUp 2026 (through its Ruby API), `cabinet-2017.skp` is the
//! same model written by skpwright as release 2017 (the older container). It holds:
//! - « Planche » (600 × 300 × 18, `no_scale_mask` 4), placed twice in « Caisson »: « Bas », and
//!   « Haut » mirrored (X → −X) and raised 500, painted « Chêne »; with them in « Caisson » a group
//!   « Groupe2 », an open box (its top face erased);
//! - « Caisson » placed twice in the model, at 0 and at X 1000;
//! - « Planche » scaled ×2 along X at Y 1000, named « Longue »;
//! - « Socle » (a 100 mm cube) holding « Pied » (10 × 10 × 50) at Z 100, placed at X 3000;
//! - a 5 mm cube drawn in the model itself, at Y −500;
//! - left out: a « Planche » hidden, a « Pied » on the hidden tag « Caché ».

use glam::{DMat3, DVec3};
use skraft_model::kernel::{Job, Output, Task};
use skraft_model::{Document, ImportReport};

const CABINET: &[u8] = include_bytes!("../../kernel/tests/fixtures/cabinet.skp");
const CABINET_2017: &[u8] = include_bytes!("../../kernel/tests/fixtures/cabinet-2017.skp");

fn import(bytes: &[u8]) -> (Document, ImportReport) {
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportSketchUp { bytes: bytes.to_vec() }] };
    let Ok(Some(Output::SketchUp(skp))) = skraft_model::kernel::run(&job).map(|mut o| o.pop()) else { panic!("no SketchUp read") };
    Document::from_sketchup(&skp, "cabinet.skp").unwrap()
}

#[test]
fn definitions_become_shared_parts_and_assemblies() {
    for bytes in [CABINET, CABINET_2017] {
        let (doc, report) = import(bytes);
        // Planche (twice: « Haut » in oak, « Bas » in none), Planche at ×2, Socle's faces,
        // Pied, the model's cube; Caisson, Socle.
        assert_eq!((report.parts, report.assemblies), (6, 2), "{:?}", report.excluded);
        assert_eq!(report.excluded.len(), 1, "{:?}", report.excluded);
        assert_eq!(report.excluded[0].0, "Groupe2");
        assert_eq!(report.dropped_instances, 1, "the open group's instance");
        // 2 × 2 boards, the long one, Socle and its leg, the cube: nothing hidden.
        assert_eq!(report.placed_parts, 8);
        assert_eq!(doc.root_assembly().occurrences().len(), 5);
        assert_eq!(doc.component(doc.root()).unwrap().name, "cabinet.skp");

        let board = doc.components().find(|c| c.name == "Planche").unwrap();
        assert!(board.named);
        assert_eq!(board.behavior.no_stretch.0, 4);
        assert_eq!(doc.occurrence_count(board.id), 1, "« Haut » painted, « Bas » not: two parts");
        let (lo, hi) = board.as_part().unwrap().shape().bounds().unwrap();
        assert!(((hi - lo) - DVec3::new(600.0, 300.0, 18.0)).length() < 1e-9);

        // The mirror stays in T; the instance's material is its part's.
        let cab = doc.components().find(|c| c.name == "Caisson").unwrap().as_assembly().unwrap();
        let high = cab.occurrences().iter().find(|o| o.name == "Haut").unwrap();
        assert!(high.transform.matrix3.determinant() < 0.0);
        assert!((high.transform.translation - DVec3::new(0.0, 0.0, 500.0)).length() < 1e-9);
        let oak = doc.material(doc.part(high.component).unwrap().coloration.unwrap()).unwrap();
        assert_eq!((oak.name.as_str(), oak.color), ("Chêne", [180, 140, 90]));
        let low = cab.occurrences().iter().find(|o| o.name == "Bas").unwrap();
        assert_ne!(low.component, high.component);
        assert_eq!(doc.part(low.component).unwrap().coloration, None);

        // The scale is in the geometry: a part 1200 long, placed by a translation only.
        let top = doc.root_assembly().occurrences().iter().find(|o| o.name == "Longue").unwrap();
        assert_eq!(top.transform.matrix3, DMat3::IDENTITY);
        assert!((top.transform.translation - DVec3::new(0.0, 1000.0, 0.0)).length() < 1e-9);
        let long = doc.component(top.component).unwrap();
        assert_ne!(long.id, board.id);
        let (lo, hi) = long.as_part().unwrap().shape().bounds().unwrap();
        assert!(((hi - lo) - DVec3::new(1200.0, 300.0, 18.0)).length() < 1e-9);

        // It saves and reads back as any document.
        let back = Document::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(back.placed_parts().len(), 8);
    }
}

#[test]
fn faces_beside_instances_make_a_part_in_the_assembly() {
    let (doc, _) = import(CABINET);
    let socle = doc.components().find(|c| c.name == "Socle").unwrap();
    assert!(!socle.is_part());
    let occurrences = socle.as_assembly().unwrap().occurrences();
    assert_eq!(occurrences.len(), 2);
    let faces = doc.component(occurrences[0].component).unwrap();
    assert!(faces.is_part() && faces.name == "Socle (faces)", "{}", faces.name);
    let leg = occurrences.iter().find(|o| doc.component(o.component).unwrap().name == "Pied").unwrap();
    assert!((leg.transform.translation - DVec3::new(0.0, 0.0, 100.0)).length() < 1e-9);
    // The model's own faces: one part straight in the root.
    let cube = doc.root_assembly().occurrences().iter().map(|o| doc.component(o.component).unwrap()).find(|c| c.is_part() && !c.named);
    let (lo, hi) = cube.unwrap().as_part().unwrap().shape().bounds().unwrap();
    assert!((lo - DVec3::new(0.0, -500.0, 0.0)).length() < 1e-9 && ((hi - lo) - DVec3::splat(5.0)).length() < 1e-9);
}

#[test]
fn a_file_that_is_no_sketchup_model_is_refused() {
    let job = Job { inputs: Vec::new(), tasks: vec![Task::ImportSketchUp { bytes: b"not a SketchUp file".to_vec() }] };
    assert!(skraft_model::kernel::run(&job).is_err());
}

/// `painted.skp` (SketchUp 2026): a 100 mm cube « Cube » whose top is painted « Rouge »; its
/// face at X 100 reversed (its front inside), its back painted « Bleu »; its face at X 0
/// reversed, its front painted « Vert » (inside: not seen); its bottom's back painted « Rouge »
/// (inside). Its instance painted « Bois », a texture of 100 × 50 mm.
#[test]
fn faces_take_the_material_of_their_side_facing_out_and_textures_come_along() {
    let (doc, report) = import(include_bytes!("../../kernel/tests/fixtures/painted.skp"));
    assert_eq!(report.painted, 2);
    let cube = doc.components().find(|c| c.name == "Cube").unwrap();
    let part = cube.as_part().unwrap();
    let planes: Vec<_> = part.shape().planes().collect();
    let painted: Vec<(String, glam::DVec3, glam::DVec3)> = part
        .faces
        .iter()
        .map(|(&f, m)| {
            let p = &planes[f as usize];
            (doc.material(*m).unwrap().name.clone(), p.point, p.normal.abs())
        })
        .collect();
    assert_eq!(painted.len(), 2, "{painted:?}");
    let on = |name: &str| painted.iter().find(|(n, _, _)| n == name).unwrap();
    let (_, point, normal) = on("Rouge");
    assert!((normal - DVec3::Z).length() < 1e-9 && (point.z - 100.0).abs() < 1e-9, "the top");
    let (_, point, normal) = on("Bleu");
    assert!((normal - DVec3::X).length() < 1e-9 && (point.x - 100.0).abs() < 1e-9, "the face at X 100");

    // The instance's material, on its part, its texture with its image.
    let wood = doc.material(cube.as_part().unwrap().coloration.unwrap()).unwrap();
    assert!(wood.name.starts_with("Bois"), "{}", wood.name);
    let texture = wood.texture.as_ref().unwrap();
    assert_eq!(texture.size, [100.0, 50.0]);
    assert!(doc.asset(&texture.image).is_some());
}

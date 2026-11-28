//! Exporting a model as a STEP file (AP242): the products the model gathers, each body's record
//! written exactly by SolidCore's writer (`solidcore_step::write::product`), off the UI thread
//! (docs/io/step-export.md, docs/decisions/0059-exporters-step-exact-skp-meshed.md).

use glam::DAffine3;
use serde::{Deserialize, Serialize};

/// What to write: products, the top one, and the header's names.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StepExport {
    /// Every product written, children before or after their parents.
    pub products: Vec<StepExportProduct>,
    /// Index of the top product in [`StepExport::products`].
    pub root: usize,
    /// The file's name, for its header.
    pub file_name: String,
    /// The program that writes it (its header's preprocessor and originating system).
    pub program: String,
}

/// A part or an assembly: its own bodies and the products it places.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StepExportProduct {
    pub name: String,
    pub bodies: Vec<StepExportBody>,
    /// The products it places: an index into [`StepExport::products`], the rigid placement in
    /// this product's axes, the occurrence's name.
    pub children: Vec<(usize, DAffine3, String)>,
}

/// A body of a product, in its axes (mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepExportBody {
    pub name: String,
    /// The job's input holding its record.
    pub solid: u32,
    /// Written mirrored across its YZ plane (x → −x): a mirrored occurrence's twin.
    pub mirrored: bool,
    /// sRGB, the whole body's colour.
    pub color: Option<[u8; 3]>,
    /// Faces with colours of their own: index in the record's faces, sRGB.
    pub faces: Vec<(u32, [u8; 3])>,
}

/// The mirror a twin's records are written through, in the component's axes.
pub const MIRROR: DAffine3 =
    DAffine3 { matrix3: glam::DMat3::from_cols(glam::DVec3::NEG_X, glam::DVec3::Y, glam::DVec3::Z), translation: glam::DVec3::ZERO };

#[cfg(feature = "engine")]
pub(crate) mod write {
    use std::borrow::Cow;
    use std::collections::HashMap;
    use std::collections::hash_map::Entry;
    use std::sync::Arc;

    use solidcore_record::brep::{Brep, Surface};

    use solidcore_step::write::product::{Colour, ExportBody, ExportProduct, Header, products};

    use super::{MIRROR, StepExport};
    use crate::engine::row_major;
    use crate::{Error, Result, Solid};

    fn err(message: impl Into<String>) -> Error {
        Error::Kernel { call: "step export".into(), message: message.into() }
    }

    /// `record` as STEP can state it: a cone of no angle (a round's, SolidCore keeps its NURBS
    /// form beside it) is the cylinder it is — the same (u, v) —, where the writer would write a
    /// `CONICAL_SURFACE` of semi-angle 0, which STEP forbids and readers refuse.
    pub(crate) fn step_ready(record: &Brep) -> Cow<'_, Brep> {
        let flat = |s: &Surface| matches!(s, Surface::Cone { half_angle, .. } if half_angle.abs() < 1e-12);
        if !record.faces.iter().any(|f| flat(&f.surface)) {
            return Cow::Borrowed(record);
        }
        let mut r = record.clone();
        for f in &mut r.faces {
            if let Surface::Cone { frame, radius_at_origin, .. } = &f.surface
                && flat(&f.surface)
            {
                f.surface = Surface::Cylinder { frame: frame.clone(), radius: *radius_at_origin };
            }
        }
        Cow::Owned(r)
    }

    fn colour(c: [u8; 3]) -> Colour {
        Colour { rgb: c.map(|v| f32::from(v) / 255.0), opacity: 1.0, name: String::new() }
    }

    /// The STEP text of `export`, its bodies' records found by `input`; the mirrored ones
    /// mirrored first, each once.
    pub(crate) fn export<'a>(export: &StepExport, input: &dyn Fn(u32) -> Result<&'a Solid>) -> Result<String> {
        let mut mirrored: HashMap<u32, Arc<Solid>> = HashMap::new();
        for b in export.products.iter().flat_map(|p| &p.bodies).filter(|b| b.mirrored) {
            if let Entry::Vacant(slot) = mirrored.entry(b.solid) {
                slot.insert(Arc::new(input(b.solid)?.transformed(&MIRROR)?));
            }
        }
        // Each record as STEP can state it, once.
        let mut ready: HashMap<(u32, bool), Cow<'_, Brep>> = HashMap::new();
        for b in export.products.iter().flat_map(|p| &p.bodies) {
            if let Entry::Vacant(slot) = ready.entry((b.solid, b.mirrored)) {
                let solid: &Solid = if b.mirrored { &mirrored[&b.solid] } else { input(b.solid)? };
                slot.insert(step_ready(solid.record()));
            }
        }
        let mut list = Vec::with_capacity(export.products.len());
        for p in &export.products {
            let mut bodies = Vec::with_capacity(p.bodies.len());
            for b in &p.bodies {
                let record: &Brep = &ready[&(b.solid, b.mirrored)];
                bodies.push(ExportBody {
                    name: b.name.clone(),
                    record,
                    color: b.color.map(colour),
                    // A face index the record lacks is dropped: a boolean renumbered the faces
                    // since it was painted (docs/render/look.md).
                    faces: b
                        .faces
                        .iter()
                        .filter(|(f, _)| (*f as usize) < record.faces.len())
                        .map(|(f, c)| (*f as usize, colour(*c)))
                        .collect(),
                });
            }
            let children = p.children.iter().map(|(c, t, name)| (*c, row_major(t), name.clone())).collect();
            list.push(ExportProduct { name: p.name.clone(), bodies, children });
        }
        let header = Header {
            file_name: export.file_name.clone(),
            preprocessor_version: export.program.clone(),
            originating_system: export.program.clone(),
            ..Header::default()
        };
        crate::engine::guarded("step export", || products(&list, export.root, &header).map_err(|e| err(e.to_string())))
    }
}

#[cfg(all(test, feature = "engine"))]
mod tests {
    use std::sync::Arc;

    use glam::{DAffine3, DVec3};

    use super::*;
    use crate::{Job, Output, Solid, Task};

    fn written(export: StepExport, inputs: Vec<Arc<Solid>>) -> String {
        let job = Job { inputs, tasks: vec![Task::ExportStep { export: Box::new(export) }] };
        match crate::run(&job).unwrap().pop() {
            Some(Output::StepFile(text)) => text,
            o => panic!("{o:?}"),
        }
    }

    fn body(solid: u32, mirrored: bool) -> StepExportBody {
        StepExportBody { name: "Planche".into(), solid, mirrored, color: Some([200, 100, 50]), faces: vec![(0, [10, 20, 30])] }
    }

    /// A box placed twice, once mirrored: two products under the top one, each read back by the
    /// importer with the box's volume where it was placed.
    #[test]
    fn a_mirrored_occurrence_places_a_mirrored_twin_read_back_in_place() {
        let board = Arc::new(Solid::new_box(DVec3::ZERO, DVec3::new(100.0, 20.0, 10.0)).unwrap());
        let right = DAffine3::from_translation(DVec3::new(500.0, 0.0, 0.0));
        let left = DAffine3::from_translation(DVec3::new(-50.0, 0.0, 0.0)) * MIRROR;
        let export = StepExport {
            products: vec![
                StepExportProduct {
                    name: "Modèle".into(),
                    bodies: vec![],
                    children: vec![(1, right, "Droite".into()), (2, left * MIRROR, "Gauche".into())],
                },
                StepExportProduct { name: "Planche".into(), bodies: vec![body(0, false)], children: vec![] },
                StepExportProduct { name: "Planche".into(), bodies: vec![body(0, true)], children: vec![] },
            ],
            root: 0,
            file_name: "test.step".into(),
            program: "Skraft".into(),
        };
        let text = written(export, vec![board]);
        assert!(text.contains("AP242") || text.contains("ap242"), "{}", &text[..400.min(text.len())]);
        assert!(text.contains("Mod\\X2\\00E8\\X0\\le"), "the top product's name");
        assert!(text.contains("COLOUR_RGB"));

        let read = crate::run(&Job { inputs: vec![], tasks: vec![Task::ImportStep { bytes: text.into_bytes() }] }).unwrap();
        let Some(Output::Step(step)) = read.into_iter().next() else { panic!() };
        assert!(step.excluded.is_empty(), "{:?}", step.excluded);
        // Every board's world box: one from 500 to 600 along x, the mirrored one from −150 to −50.
        let mut boxes = Vec::new();
        fn walk(step: &crate::StepImport, p: usize, t: DAffine3, out: &mut Vec<(f64, f64)>) {
            for b in &step.products[p].bodies {
                let (lo, hi) = b.solid.vertex_bounds().unwrap();
                let (a, c) = (t.transform_point3(lo).x, t.transform_point3(hi).x);
                out.push((a.min(c), a.max(c)));
                assert_eq!(b.color, Some([200, 100, 50]));
            }
            for c in &step.products[p].children {
                walk(step, c.product, t * c.transform, out);
            }
        }
        for r in &step.roots {
            walk(&step, r.product, r.transform, &mut boxes);
        }
        boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        assert!((boxes[0].0 + 150.0).abs() < 1e-6 && (boxes[0].1 + 50.0).abs() < 1e-6, "{boxes:?}");
        assert!((boxes[1].0 - 500.0).abs() < 1e-6 && (boxes[1].1 - 600.0).abs() < 1e-6, "{boxes:?}");
    }

    /// A curved body goes out exactly: a cylinder's face read back as a cylinder, not facets.
    #[test]
    fn a_cylinder_goes_out_exact() {
        let cyl = crate::Primitive::Cylinder { origin: DVec3::ZERO, radius: 10.0, height: 30.0 }.build().unwrap();
        let faces = cyl.record().faces.len();
        let export = StepExport {
            products: vec![StepExportProduct { name: "Tourillon".into(), bodies: vec![body(0, false)], children: vec![] }],
            root: 0,
            file_name: "t.step".into(),
            program: "Skraft".into(),
        };
        let text = written(export, vec![Arc::new(cyl)]);
        assert!(text.contains("CYLINDRICAL_SURFACE"));
        let read = crate::run(&Job { inputs: vec![], tasks: vec![Task::ImportStep { bytes: text.into_bytes() }] }).unwrap();
        let Some(Output::Step(step)) = read.into_iter().next() else { panic!() };
        assert_eq!(step.products[0].bodies[0].solid.record().faces.len(), faces);
    }

    /// A cone of no angle (a round's) goes out as the cylinder it is: STEP forbids a
    /// `CONICAL_SURFACE` of semi-angle 0, and the reader refuses it.
    #[test]
    fn a_cone_of_no_angle_goes_out_as_a_cylinder() {
        use solidcore_record::brep::Surface;
        let cyl = crate::Primitive::Cylinder { origin: DVec3::ZERO, radius: 10.0, height: 30.0 }.build().unwrap();
        let mut record = cyl.record().clone();
        for f in &mut record.faces {
            if let Surface::Cylinder { frame, radius } = &f.surface {
                f.surface = Surface::Cone { frame: frame.clone(), radius_at_origin: *radius, half_angle: 0.0 };
            }
        }
        let export = StepExport {
            products: vec![StepExportProduct { name: "Tourillon".into(), bodies: vec![body(0, false)], children: vec![] }],
            root: 0,
            file_name: "t.step".into(),
            program: "Skraft".into(),
        };
        let text = written(export, vec![Arc::new(Solid::from_record(record))]);
        assert!(!text.contains("CONICAL_SURFACE") && text.contains("CYLINDRICAL_SURFACE"));
        let read = crate::run(&Job { inputs: vec![], tasks: vec![Task::ImportStep { bytes: text.into_bytes() }] }).unwrap();
        let Some(Output::Step(step)) = read.into_iter().next() else { panic!() };
        assert!(step.excluded.is_empty(), "{:?}", step.excluded);
    }
}

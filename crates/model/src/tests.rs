use std::sync::Arc;

use glam::{DAffine3, DVec3};

use crate::*;

fn cube(at: DVec3, size: f64) -> Primitive {
    Primitive::Box { origin: at, size: DVec3::splat(size) }
}

/// Adds a primitive at the root, returns its occurrence.
fn add(h: &mut History, p: Primitive) -> Id {
    let root = h.document().root();
    h.apply(Operation::AddPrimitive { context: root, primitive: p, placement: DAffine3::IDENTITY, material: None }).unwrap().created[0]
}

fn occ(doc: &Document, path: &EntityPath) -> Occurrence {
    doc.resolve(path).unwrap().clone()
}

/// The part's world bounds (its occurrence's transform applied to its shape's corners).
fn world_bounds(doc: &Document, path: &EntityPath) -> (DVec3, DVec3) {
    let (_, part, t) = doc.resolve_part(path).unwrap();
    let (lo, hi) = part.shape().vertex_bounds().unwrap();
    let mut out = (DVec3::INFINITY, DVec3::NEG_INFINITY);
    for i in 0..8 {
        let c =
            DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
        let p = t.transform_point3(c);
        out = (out.0.min(p), out.1.max(p));
    }
    out
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-9
}

#[test]
fn a_primitive_is_a_part_in_its_own_axes() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, Primitive::Box { origin: DVec3::new(10.0, 20.0, 30.0), size: DVec3::new(-5.0, 6.0, 7.0) });
    let doc = h.document();
    let o = occ(doc, &EntityPath::root(a));
    let part = doc.part(o.component).unwrap();
    // The blank starts at the part's origin; the occurrence puts it back where it was drawn.
    let (lo, hi) = part.blank().vertex_bounds().unwrap();
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(5.0, 6.0, 7.0)), "{lo} {hi}");
    assert!(close(o.transform.translation, DVec3::new(5.0, 20.0, 30.0)));
    assert_eq!(doc.component(o.component).unwrap().name, "Pièce#1");
    assert!(!doc.component(o.component).unwrap().named);

    assert!(h.undo());
    assert!(h.document().root_assembly().occurrences().is_empty());
    assert_eq!(h.document().components().count(), 0, "the part went with its occurrence");
    assert!(h.redo());
    assert_eq!(h.document().root_assembly().occurrences()[0].id, a);
}

#[test]
fn moving_copying_and_arraying_need_no_kernel() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let doc = h.document().clone();
    let root = doc.root();
    let part = occ(&doc, &EntityPath::root(a)).component;
    let t = DAffine3::from_translation(DVec3::new(20.0, 0.0, 0.0));
    let rot = DAffine3::from_rotation_z(0.3) * t;
    for op in [
        Operation::Transform { context: root, entities: vec![a], transform: rot },
        Operation::Duplicate { context: root, entities: vec![a], transform: t },
        Operation::Array { context: root, entities: vec![a], step: t, count: 3 },
    ] {
        assert!(matches!(doc.prepare(&op).unwrap(), Prepared::Done(_)), "{op:?}");
    }

    // Moving changes the occurrence only: the part's B-rep is the same.
    let blank = doc.part(part).unwrap().blank().clone();
    h.apply(Operation::Transform { context: root, entities: vec![a], transform: rot }).unwrap();
    assert!(Arc::ptr_eq(&blank, &h.document().part(part).unwrap().blank()));
    assert!(close(occ(h.document(), &EntityPath::root(a)).transform.translation, rot.translation));

    // Copies are more occurrences of the same part.
    let out = h.apply(Operation::Array { context: root, entities: vec![a], step: t, count: 3 }).unwrap();
    assert_eq!(out.created.len(), 3);
    assert_eq!(h.document().occurrence_count(part), 4);
    assert_eq!(h.document().components().count(), 1);
    let xs: Vec<f64> = out.created.iter().map(|id| occ(h.document(), &EntityPath::root(*id)).transform.translation.x).collect();
    let x0 = rot.translation.x;
    assert!(xs.iter().zip([x0 + 20.0, x0 + 40.0, x0 + 60.0]).all(|(x, e)| (x - e).abs() < 1e-9), "{xs:?}");
    assert!(h.apply(Operation::Array { context: root, entities: vec![a], step: t, count: 0 }).is_err());
}

#[test]
fn mirrors_place_but_scales_are_refused() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let root = h.document().root();
    let mirror = DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0));
    h.apply(Operation::Duplicate { context: root, entities: vec![a], transform: mirror }).unwrap();
    assert_eq!(h.document().components().count(), 1, "left and right are the same part");
    for t in [DAffine3::from_scale(DVec3::splat(2.0)), DAffine3::from_scale(DVec3::new(1.0, 2.0, 1.0))] {
        assert!(h.apply(Operation::Transform { context: root, entities: vec![a], transform: t }).is_err());
        assert!(h.apply(Operation::Duplicate { context: root, entities: vec![a], transform: t }).is_err());
    }
}

#[test]
fn unused_parts_stay_until_purged() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::splat(20.0), 5.0));
    let (pa, pb) = (occ(h.document(), &EntityPath::root(a)).component, occ(h.document(), &EntityPath::root(b)).component);
    assert_eq!(h.document().component(pb).unwrap().name, "Pièce#2");
    h.apply(Operation::RenameComponent { component: pb, name: "Pied".into() }).unwrap();
    assert!(h.apply(Operation::RenameComponent { component: pa, name: "Pied".into() }).is_err(), "names are unique");
    h.apply(Operation::Delete { context: root, entities: vec![a, b] }).unwrap();
    // Named or not, a part placed nowhere stays, unused (docs/ui/components-library.md)…
    let counts = h.document().counts_in_model();
    assert!(h.document().component(pa).is_ok() && h.document().component(pb).is_ok());
    assert!(!counts.contains_key(&pa) && !counts.contains_key(&pb));
    // …can be placed again…
    let out = h.apply(Operation::PlaceOccurrence { context: root, component: pb, transform: DAffine3::IDENTITY }).unwrap();
    assert_eq!(occ(h.document(), &EntityPath::root(out.created[0])).component, pb);
    // …until purged.
    h.apply(Operation::PurgeUnused).unwrap();
    assert!(h.document().component(pa).is_err() && h.document().component(pb).is_ok());
}

#[test]
fn assemblies_make_explode_and_share() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let g = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().created[0];
    let doc = h.document();
    assert_eq!(doc.root_assembly().occurrences().len(), 1);
    let asm = occ(doc, &EntityPath::root(g)).component;
    assert_eq!(doc.component(asm).unwrap().name, "Assemblage#1");
    assert_eq!(doc.context_of(&[g]).unwrap(), asm);
    assert!(doc.is_assembly_occurrence(root, g));

    // Moving the assembly moves its occurrence only.
    let t = DAffine3::from_translation(DVec3::new(100.0, 0.0, 0.0));
    h.apply(Operation::Transform { context: root, entities: vec![g], transform: t }).unwrap();
    let placed = h.document().placed_parts();
    assert_eq!(placed.len(), 2);
    assert!(placed.iter().all(|p| p.path.occurrences == vec![g]));
    assert!(close(world_bounds(h.document(), &EntityPath::new(vec![g], a)).0, DVec3::new(100.0, 0.0, 0.0)));

    // A copy of the assembly shares it: two paths to each part.
    h.apply(Operation::Duplicate { context: root, entities: vec![g], transform: t }).unwrap();
    assert_eq!(h.document().placed_parts().len(), 4);
    assert_eq!(h.document().occurrence_count(asm), 2);

    // Making one copy unique gives it an assembly of its own, the parts still shared.
    h.apply(Operation::MakeUnique { context: root, occurrence: g }).unwrap();
    let own = occ(h.document(), &EntityPath::root(g)).component;
    assert_ne!(own, asm);
    assert_eq!(h.document().component(own).unwrap().name, "Assemblage#2");
    assert_eq!(h.document().components().filter(|c| c.is_part()).count(), 2);

    // Exploding puts its occurrences in place, the assembly's transform composed; the
    // assembly stays, unused.
    let out = h.apply(Operation::Explode { context: root, occurrence: g }).unwrap();
    assert_eq!(out.created.len(), 2);
    assert!(h.document().component(own).is_ok() && !h.document().counts_in_model().contains_key(&own));
    let xs: Vec<f64> = out.created.iter().map(|id| world_bounds(h.document(), &EntityPath::root(*id)).0.x).collect();
    assert!(xs.iter().zip([100.0, 120.0]).all(|(x, e)| (x - e).abs() < 1e-9), "{xs:?}");
    // A part's occurrence is not exploded.
    assert!(h.apply(Operation::Explode { context: root, occurrence: out.created[0] }).is_err());
    // No assembly inside itself.
    assert!(matches!(
        h.apply(Operation::PlaceOccurrence { context: asm, component: asm, transform: DAffine3::IDENTITY }),
        Err(Error::Cycle)
    ));
}

#[test]
fn a_difference_machines_the_part_for_all_its_occurrences() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::new(1000.0, 0.0, 0.0), size: DVec3::new(600.0, 300.0, 18.0) });
    let copy = h
        .apply(Operation::Duplicate { context: root, entities: vec![board], transform: DAffine3::from_translation(DVec3::Y * 500.0) })
        .unwrap()
        .created[0];
    // A drill through the first board's top, in an assembly moved elsewhere.
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(0.0, 0.0, 6.0), radius: 4.0, height: 20.0 });
    let g = h.apply(Operation::MakeAssembly { context: root, entities: vec![drill], name: None }).unwrap().created[0];
    h.apply(Operation::Transform {
        context: root,
        entities: vec![g],
        transform: DAffine3::from_translation(DVec3::new(1100.0, 50.0, 0.0)),
    })
    .unwrap();
    let part = occ(h.document(), &EntityPath::root(board)).component;
    let blank = h.document().part(part).unwrap().blank().clone();

    let target = EntityPath::root(board);
    let tool = EntityPath::new(vec![g], drill);
    h.apply(Operation::boolean(target.clone(), tool, Boolean::Difference, Scope::All)).unwrap();
    let doc = h.document();
    let p = doc.part(part).unwrap();
    assert!(Arc::ptr_eq(&p.blank(), &blank), "the blank stays");
    assert_eq!(p.machinings().len(), 1);
    // The drill, in the board's axes: 100, 50 from its corner, its base 6 up.
    assert!(close(p.machinings()[0].placement.translation, DVec3::new(100.0, 50.0, 6.0)), "{}", p.machinings()[0].placement.translation);
    assert!(p.shape().topology_counts().0 > blank.topology_counts().0);
    // Both occurrences show the hole; the drill left the model (its assembly emptied: the
    // editor takes it away), its part unused.
    assert_eq!(doc.resolve_part(&EntityPath::root(copy)).unwrap().0, part);
    assert!(doc.resolve(&EntityPath::root(g)).is_err() || doc.assembly(doc.context_of(&[g]).unwrap()).unwrap().occurrences().is_empty());
    let counts = doc.counts_in_model();
    assert_eq!(doc.components().filter(|c| c.is_part() && counts.contains_key(&c.id)).count(), 1);

    // A union gives the target the tool's bodies (docs/model/bodies.md): its blank and its hole
    // stay, grouped so the hole keeps off the tool.
    let other = add(&mut h, Primitive::Box { origin: DVec3::new(1590.0, 0.0, 0.0), size: DVec3::new(20.0, 300.0, 100.0) });
    h.apply(Operation::boolean(target, EntityPath::root(other), Boolean::Union, Scope::All)).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!((p.positives().count(), p.negatives().count()), (2, 0));
    let own = p.def_of(&p.bodies[0]).children();
    assert_eq!(own.iter().map(|c| c.sign).collect::<Vec<_>>(), [Sign::Positive, Sign::Negative]);
    assert!(Arc::ptr_eq(p.solid_of(&own[0]), &blank));
    assert!(own.iter().chain(&p.bodies[1..]).all(|b| p.profile_of(b).is_some()), "the profiles kept");
    let (_, hi) = p.shape().vertex_bounds().unwrap();
    assert!((hi.x - 610.0).abs() < 1e-9 && (hi.z - 100.0).abs() < 1e-9, "{hi}");
    assert_eq!(p.pieces(), 1);
}

#[test]
fn errors_leave_the_document_unchanged() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let before = h.document().clone();
    let r = h.apply(Operation::AddPrimitive {
        context: root,
        primitive: cube(DVec3::ZERO, 0.0),
        placement: DAffine3::IDENTITY,
        material: None,
    });
    assert!(matches!(r, Err(Error::Kernel(_))), "{r:?}");
    assert_eq!(*h.document(), before);
    assert!(!h.can_undo());
    assert!(h.apply(Operation::Delete { context: root, entities: vec![Id(999)] }).is_err());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let same = EntityPath::root(a);
    assert!(h.apply(Operation::boolean(same.clone(), same, Boolean::Union, Scope::All)).is_err());
}

#[test]
fn file_round_trip() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 50.0, 18.0) });
    let b = add(&mut h, Primitive::Cylinder { origin: DVec3::new(50.0, 25.0, 10.0), radius: 4.0, height: 20.0 });
    h.apply(Operation::boolean(EntityPath::root(a), EntityPath::root(b), Boolean::Difference, Scope::All)).unwrap();
    h.apply(Operation::MakeAssembly { context: root, entities: vec![a], name: Some("Tiroir".into()) }).unwrap();
    let json = h.document().to_json().unwrap();
    assert!(json.contains("\"version\":1"));
    let back = Document::from_json(&json).unwrap();
    assert_eq!(back, *h.document());
    assert!(Document::from_json(r#"{"format":"other","version":1}"#).is_err());
    assert!(Document::from_json(&json.replacen("\"version\":1", "\"version\":99", 1)).is_err());
}

/// A file's entries, by path.
fn unzip(file: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(file)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut e = zip.by_index(i).unwrap();
            let mut out = Vec::new();
            e.read_to_end(&mut out).unwrap();
            (e.name().unwrap().into_owned(), out)
        })
        .collect()
}

fn rezip(entries: &std::collections::BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn a_file_is_a_zip_of_the_structure_and_its_b_reps() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 50.0, 18.0) });
    let b = add(&mut h, Primitive::Cylinder { origin: DVec3::new(50.0, 25.0, 10.0), radius: 4.0, height: 20.0 });
    let c = add(&mut h, Primitive::Sphere { center: DVec3::new(300.0, 0.0, 0.0), radius: 10.0 });
    h.apply(Operation::boolean(EntityPath::root(a), EntityPath::root(b), Boolean::Difference, Scope::All)).unwrap();
    h.apply(Operation::Duplicate { context: root, entities: vec![a, c], transform: DAffine3::from_translation(DVec3::Y * 500.0) }).unwrap();
    let doc = h.document();
    let file = doc.to_file().unwrap();
    assert_eq!(Document::from_file(&file).unwrap(), *doc);
    // The same document, the same bytes.
    assert_eq!(doc.to_file().unwrap(), file);

    let entries = unzip(&file);
    let (manifest, document) = (&entries["manifest.json"], std::str::from_utf8(&entries["document.json"]).unwrap());
    let manifest: serde_json::Value = serde_json::from_slice(manifest).unwrap();
    assert_eq!(
        (manifest["format"].as_str(), manifest["version"].as_u64(), manifest["content"].as_str()),
        (Some("skraft"), Some(1), Some("model"))
    );
    // The structure only, readable: no record inline.
    assert!(!document.contains("\"faces\""), "{document}");
    let names: Vec<_> = entries.keys().collect();
    let in_dir = |dir: &str| entries.keys().filter(|k| k.starts_with(dir)).count();
    // Each solid once (the copies share theirs), named in the document: the board and the
    // cylinder (profiled), the drilled board's shape, the sphere.
    assert_eq!(in_dir("breps/") + in_dir("cache/"), 4, "{names:?}");
    assert!(in_dir("cache/") >= 3, "{names:?}");
    for name in entries.keys().filter_map(|k| k.strip_prefix("breps/").or(k.strip_prefix("cache/"))) {
        assert!(document.contains(&format!("\"{}\"", name.strip_suffix(".json").unwrap())), "{name}");
    }

    // Refused: not a ZIP (an older file, JSON), another format, a newer version, an entry missing.
    assert!(matches!(Document::from_file(doc.to_json().unwrap().as_bytes()), Err(Error::File(FileError::NotZip))));
    let with = |name: &str, bytes: Option<Vec<u8>>| {
        let mut e = entries.clone();
        match bytes {
            Some(b) => e.insert(name.to_owned(), b),
            None => e.remove(name),
        };
        Document::from_file(&rezip(&e))
    };
    let manifest_with = |field: &str, value: serde_json::Value| {
        let mut m = manifest.clone();
        m[field] = value;
        with("manifest.json", Some(serde_json::to_vec(&m).unwrap()))
    };
    assert!(matches!(manifest_with("format", "other".into()), Err(Error::File(FileError::NotSkraft(f))) if f == "other"));
    assert!(matches!(manifest_with("version", 99.into()), Err(Error::File(FileError::TooNew(99)))));
    assert!(matches!(with("document.json", None), Err(Error::File(FileError::MissingEntry(e))) if e == "document.json"));
    let some_cache = entries.keys().find(|k| k.starts_with("cache/")).unwrap().clone();
    let missing = with(&some_cache, None);
    assert!(matches!(&missing, Err(Error::File(FileError::Unreadable(e))) if e.contains("missing")), "{missing:?}");
    // Zipped again as it was, it reads.
    assert_eq!(Document::from_file(&rezip(&entries)).unwrap(), *doc);
}

/// A ZIP of `entries`, deflated.
fn deflated(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write as _;
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

const SMALL_MANIFEST: &[u8] = br#"{"format":"skraft","version":1,"content":"model"}"#;

#[test]
fn an_entry_is_read_within_what_the_file_may_expand_to() {
    let read = |bytes: &[u8], declared, left: &mut u64| crate::file::read_entry(bytes, declared, left).unwrap();
    let mut left = 10;
    assert_eq!(read(b"abcd", 4, &mut left).as_deref(), Some(&b"abcd"[..]));
    assert_eq!(left, 6);
    // Exactly what is left still reads; declaring less does not let more through.
    assert_eq!(read(b"abcdef", 1, &mut left).as_deref(), Some(&b"abcdef"[..]));
    assert_eq!(left, 0);
    let mut left = 3;
    assert_eq!(read(b"abcd", 1, &mut left), None);
    assert_eq!(read(b"a", 4, &mut left), None, "declared past it: not even read");
    assert_eq!(left, 3);
}

#[test]
fn a_file_lying_about_a_size_is_refused_without_reserving_it() {
    let mut file = deflated(&[("manifest.json", SMALL_MANIFEST)]);
    // Its central directory now claims 2^62 bytes, through a Zip64 field.
    let at = file.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    let u16_at = |f: &[u8], i: usize| u16::from_le_bytes([f[i], f[i + 1]]) as usize;
    let (name, extra) = (u16_at(&file, at + 28), u16_at(&file, at + 30));
    file[at + 24..at + 28].copy_from_slice(&u32::MAX.to_le_bytes());
    file[at + 30..at + 32].copy_from_slice(&((extra + 12) as u16).to_le_bytes());
    let mut zip64 = vec![1, 0, 8, 0];
    zip64.extend_from_slice(&(1u64 << 62).to_le_bytes());
    file.splice(at + 46 + name..at + 46 + name, zip64);
    let end = file.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    let size = u32::from_le_bytes(file[end + 12..end + 16].try_into().unwrap());
    file[end + 12..end + 16].copy_from_slice(&(size + 12).to_le_bytes());
    assert!(matches!(Document::from_file(&file), Err(Error::File(FileError::TooLarge))));
}

#[test]
fn a_file_expanding_past_its_limit_is_refused() {
    let blanks = vec![b' '; crate::file::MIN_EXPANDED as usize + 1];
    let file = deflated(&[("manifest.json", SMALL_MANIFEST), ("document.json", &blanks)]);
    assert!(file.len() < 1 << 20, "a small file: {}", file.len());
    assert!(matches!(Document::from_file(&file), Err(Error::File(FileError::TooLarge))));
}

#[test]
fn an_oversized_view_is_ignored() {
    let doc = Document::new();
    let mut entries = unzip(&doc.to_file().unwrap());
    let mut view = br#"{"workspace":"cut_list"}"#.to_vec();
    view.resize((1 << 20) + 1, b' ');
    entries.insert("view.json".into(), view);
    assert_eq!(Document::file_view(&rezip(&entries)), FileView::default());
}

#[test]
fn a_file_keeps_how_it_was_seen_beside_the_document() {
    let doc = Document::new();
    // Nothing to say: no entry, the same bytes as without it.
    assert_eq!(doc.to_file_seen(&FileView::default()).unwrap(), doc.to_file().unwrap());
    assert!(!unzip(&doc.to_file().unwrap()).contains_key("view.json"));
    assert_eq!(Document::file_view(&doc.to_file().unwrap()), FileView::default());

    let camera = SeenCamera {
        eye: DVec3::new(1.5, -2.25, 3.0),
        yaw: 0.5,
        pitch: -0.25,
        projection: SeenProjection::Parallel { height: 420.0 },
        focus: 12.0,
        frame: glam::DMat3::IDENTITY,
    };
    let board = SeenBoard { sheet: Some(Id(7)), presentation: Some(Id(3)), pan: glam::DVec2::new(10.0, -20.0), zoom: 1.5 };
    let seen = FileView { workspace: Some("cut_list".into()), camera: Some(camera), board: Some(board) };
    let file = doc.to_file_seen(&seen).unwrap();
    assert_eq!(Document::file_view(&file), seen);
    // The document reads as without it.
    assert_eq!(Document::from_file(&file).unwrap(), doc);
    // Unreadable or not a ZIP: the default, never an error.
    let mut entries = unzip(&file);
    entries.insert("view.json".into(), b"{ nope".to_vec());
    assert_eq!(Document::file_view(&rezip(&entries)), FileView::default());
    assert_eq!(Document::file_view(b"not a zip"), FileView::default());
}

#[test]
fn a_file_carries_a_picture_of_its_model_read_alone() {
    let doc = Document::new();
    let plain = doc.to_file().unwrap();
    assert_eq!(Document::file_thumbnail(std::io::Cursor::new(&plain)), None);
    let png = b"\x89PNG not really".to_vec();
    let file = doc.to_file_pictured(&FileView::default(), Some(&png)).unwrap();
    assert_eq!(Document::file_thumbnail(std::io::Cursor::new(&file)), Some(png));
    // The document reads as without it.
    assert_eq!(Document::from_file(&file).unwrap(), doc);
    assert_eq!(Document::file_thumbnail(std::io::Cursor::new(b"not a zip")), None);
}

#[test]
fn guides_belong_to_their_component() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::new(100.0, 0.0, 0.0), 10.0));
    let part = occ(h.document(), &EntityPath::root(a)).component;
    let line = GuideShape::Line { origin: DVec3::new(0.0, 5.0, 0.0), dir: DVec3::X };
    let g = h.apply(Operation::AddGuide { component: part, shape: line }).unwrap().guide.unwrap();
    assert_eq!(h.document().component(part).unwrap().guide(g).unwrap().shape, line);
    // Placed by its occurrence, as the part.
    let placed = h.document().placed_guides();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].path, vec![a]);
    assert!(close(placed[0].transform.translation, DVec3::new(100.0, 0.0, 0.0)));

    // Refused: no direction, no radius; unknown component.
    let flat = GuideShape::Line { origin: DVec3::ZERO, dir: DVec3::ZERO };
    assert!(h.apply(Operation::AddGuide { component: root, shape: flat }).is_err());
    let dot = GuideShape::Circle { center: DVec3::ZERO, normal: DVec3::Z, radius: 0.0 };
    assert!(h.apply(Operation::AddGuide { component: root, shape: dot }).is_err());
    assert!(h.apply(Operation::AddGuide { component: Id(999), shape: line }).is_err());

    // Saved and read back; undone.
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.component(part).unwrap().guides.len(), 1);
    assert!(h.undo());
    assert!(h.document().component(part).unwrap().guides.is_empty());
    assert!(h.redo());

    // A separate part keeps its guides, ids and all.
    let b = h.apply(Operation::Duplicate { context: root, entities: vec![a], transform: DAffine3::IDENTITY }).unwrap().created[0];
    h.apply(Operation::MakeUnique { context: root, occurrence: b }).unwrap();
    let copy = occ(h.document(), &EntityPath::root(b)).component;
    assert_ne!(copy, part);
    assert_eq!(h.document().component(copy).unwrap().guide(g).unwrap().shape, line);

    // An exploded assembly hands its guides to the context, where they stood.
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a], name: None }).unwrap().created[0];
    // Its axes have their origin at the low corner of its content.
    assert!(close(occ(h.document(), &EntityPath::root(asm)).transform.translation, DVec3::new(100.0, 0.0, 0.0)));
    let asm_def = occ(h.document(), &EntityPath::root(asm)).component;
    h.apply(Operation::AddGuide { component: asm_def, shape: line }).unwrap();
    let shift = DAffine3::from_translation(DVec3::new(0.0, 0.0, 50.0));
    h.apply(Operation::Transform { context: root, entities: vec![asm], transform: shift }).unwrap();
    h.apply(Operation::Explode { context: root, occurrence: asm }).unwrap();
    let guides = &h.document().component(root).unwrap().guides;
    assert_eq!(guides.len(), 1);
    assert_eq!(guides[0].shape, GuideShape::Line { origin: DVec3::new(100.0, 5.0, 50.0), dir: DVec3::X });
}

#[test]
fn a_file_with_a_bad_guide_is_refused() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    h.apply(Operation::AddGuide { component: root, shape: GuideShape::Line { origin: DVec3::ZERO, dir: DVec3::Y } }).unwrap();
    let json = h.document().to_json().unwrap();
    // A guide whose id is past the counter.
    let past = json.replace("\"guides\":[{\"id\":2", "\"guides\":[{\"id\":99");
    assert_ne!(past, json);
    assert!(matches!(Document::from_json(&past), Err(Error::File(FileError::BadGuide(Id(99))))));
}

#[test]
fn settings_are_saved_undone_and_merged() {
    let mut h = History::new(Document::new());
    let mut s = h.document().settings().clone();
    s.display.grid = true;
    h.apply(Operation::SetSettings(s.clone())).unwrap();
    s.units.length = LengthUnit::Centimeter;
    h.apply(Operation::SetSettings(s.clone())).unwrap();
    // Two changes in a row: one step.
    assert_eq!(h.journal().count(), 1);
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.settings(), &s);
    assert!(h.undo());
    assert_eq!(h.document().settings(), &Settings::default());
}

#[test]
fn units_format() {
    let mut u = Units { decimals: 2, symbol: true, ..Default::default() };
    assert_eq!(u.format(12.5), "12,5 mm");
    assert_eq!(u.format(300.0), "300 mm");
    u.length = LengthUnit::Centimeter;
    u.decimals = 1;
    assert_eq!(u.format(1234.0), "123,4 cm");
    assert_eq!(u.format(-0.01), "~ 0 cm");
    u.symbol = false;
    assert_eq!(u.format(1234.0), "123,4");
}

#[test]
fn units_mark_rounded_values() {
    let mut u = Units::default();
    assert_eq!(u.format(600.0), "600");
    assert_eq!(u.format(600.4), "~ 600");
    assert_eq!(u.format(-12.6), "~ -13");
    // Float noise is not a rounding.
    assert_eq!(u.format(0.1 + 0.2 + 299.7), "300");
    u.length = LengthUnit::Inch;
    assert_eq!(u.format(25.4 * 3.0), "3");
    assert_eq!(u.format(25.4 * 3.0 + 0.01), "~ 3");
    // The exact number, for editing: to the model's precision, never marked.
    u.length = LengthUnit::Millimeter;
    assert_eq!(u.exact_number(12.3456), "12,346");
    assert_eq!(u.exact_number(199.999998), "200");
    assert_eq!(u.format(199.999998), "200");
    u.length = LengthUnit::Meter;
    assert_eq!(u.exact_number(1234.5678), "1,234568");
}

#[test]
fn units_exact_number_reads_back_to_the_value() {
    // What a focused length field shows, typed back: within half the precision (a micron) of
    // the value, shown the same again, and never marked rounded when the value shows exactly.
    let values = [0.0, 1.5875, 19.0, 199.999998, 360.555128, 1234.5678, 2438.4, -12.34567, 25.4 * 3.0 + 0.01];
    for length in LengthUnit::ALL {
        for decimals in 0..=6 {
            let u = Units { length, decimals, symbol: false };
            let back = |mm: f64| {
                let text = u.exact_number(mm);
                let typed = measure::coordinate(&text, &u, 0.0).unwrap_or_else(|e| panic!("{text}: {e}"));
                (text, typed)
            };
            for v in values {
                let (text, typed) = back(v);
                assert!((typed - v).abs() <= 0.5e-3 + 1e-9, "{length:?} {decimals}: {v} → {text} → {typed}");
                assert_eq!(u.exact_number(typed), text, "{length:?} {decimals}: {v}");
            }
            // Values shown exactly at these decimals: 1, 37 and 1234 steps of the display.
            let step = length.mm() / 10f64.powi(decimals as i32);
            for n in [1.0, 37.0, 1234.0] {
                let (text, typed) = back(n * step);
                assert!(!u.is_rounded(typed), "{length:?} {decimals}: {} → {text} → {}", u.format(n * step), u.format(typed));
            }
        }
    }
}

#[test]
fn revision_tells_the_saved_state() {
    let mut h = History::new(Document::new());
    assert_eq!(h.revision(), 0);
    add(&mut h, cube(DVec3::ZERO, 10.0));
    add(&mut h, cube(DVec3::splat(20.0), 5.0));
    let saved = h.revision();
    // Undo then another step: same journal length, but not the saved document.
    assert!(h.undo());
    assert_ne!(h.revision(), saved);
    add(&mut h, cube(DVec3::splat(40.0), 5.0));
    assert_eq!(h.journal().count(), 2);
    assert_ne!(h.revision(), saved);
    // Back to the saved state by undo / redo: unmodified again.
    assert!(h.undo());
    let before = h.revision();
    add(&mut h, cube(DVec3::splat(20.0), 5.0));
    assert_ne!(h.revision(), saved, "an equal step redone by hand is still a new state");
    assert!(h.undo());
    assert_eq!(h.revision(), before);
    assert!(h.redo());
    assert_ne!(h.revision(), before);
}

#[test]
fn merged_settings_make_a_new_revision() {
    let mut h = History::new(Document::new());
    let mut s = Settings::default();
    s.display.grid = !s.display.grid;
    h.apply(Operation::SetSettings(s.clone())).unwrap();
    let saved = h.revision();
    s.display.axes = !s.display.axes;
    h.apply(Operation::SetSettings(s)).unwrap();
    assert_eq!(h.journal().count(), 1, "the two settings steps merge");
    assert_ne!(h.revision(), saved);
    // Undoing the merged step goes back before both, redoing returns to the merged state.
    let merged = h.revision();
    assert!(h.undo());
    assert_eq!(h.revision(), 0);
    assert!(h.redo());
    assert_eq!(h.revision(), merged);
}

#[test]
fn prepare_gathers_kernel_calls_and_commit_replays_them() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::splat(5.0), 10.0));
    let doc = h.document().clone();

    // A boolean: both shapes cross, one task.
    let op = Operation::boolean(EntityPath::root(a), EntityPath::root(b), Boolean::Union, Scope::All);
    let Prepared::Kernel(job) = doc.prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert_eq!(job.inputs.len(), 2);
    assert_eq!(job.tasks.len(), 1);
    let outputs = kernel::run(&job).unwrap();

    // Out of step outputs are refused; the right ones give what `apply` gives.
    assert!(doc.commit(&op, Vec::new()).is_err());
    let applied = doc.commit(&op, outputs).unwrap();
    assert_eq!(applied.displays.len(), 1);
    assert!(applied.displays.iter().all(|(_, d)| d.is_ok()));
    let (direct, outcome) = doc.apply(&op).unwrap();
    assert_eq!(applied.document, direct);
    assert_eq!(applied.outcome, outcome);

    // Validation happens before any kernel call.
    let bad = Operation::boolean(EntityPath::root(a), EntityPath::root(Id(999)), Boolean::Union, Scope::All);
    assert!(doc.prepare(&bad).is_err());
}

/// Opens a file given by `SKRAFT_OPEN` (a check of a real file, by hand):
/// `SKRAFT_OPEN=path cargo test --workspace open_given_file -- --nocapture`.
#[test]
fn open_given_file() {
    let Some(path) = std::env::var_os("SKRAFT_OPEN") else { return };
    let doc = Document::from_file(&std::fs::read(path).unwrap()).unwrap();
    for c in doc.components() {
        println!("{} {:?} named={} occurrences={}", c.id, c.name, c.named, doc.occurrence_count(c.id));
    }
    println!("{} parts placed", doc.placed_parts().len());
    // And it saves again.
    assert_eq!(Document::from_file(&doc.to_file().unwrap()).unwrap(), doc);
}

#[test]
fn only_this_one_makes_the_way_to_it_unique() {
    // A shelf part, in a cabinet assembly copied twice; plus a loose copy of the shelf.
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let shelf = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(500.0, 300.0, 18.0) });
    let loose = h
        .apply(Operation::Duplicate { context: root, entities: vec![shelf], transform: DAffine3::from_translation(DVec3::Y * 1000.0) })
        .unwrap()
        .created[0];
    let cab = h.apply(Operation::MakeAssembly { context: root, entities: vec![shelf], name: Some("Caisson".into()) }).unwrap().created[0];
    let cab2 = h
        .apply(Operation::Duplicate { context: root, entities: vec![cab], transform: DAffine3::from_translation(DVec3::X * 1000.0) })
        .unwrap()
        .created[0];
    let part = occ(h.document(), &EntityPath::new(vec![cab], shelf)).component;
    let cabinet = occ(h.document(), &EntityPath::root(cab)).component;
    assert_eq!(h.document().placed_parts().iter().filter(|p| p.part == part).count(), 3);

    // A hole in the shelf of the second cabinet only.
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(1250.0, 150.0, 10.0), radius: 4.0, height: 20.0 });
    let target = EntityPath::new(vec![cab2], shelf);
    h.apply(Operation::boolean(target.clone(), EntityPath::root(drill), Boolean::Difference, Scope::This)).unwrap();
    let doc = h.document();
    // The path still names it; its cabinet and its shelf are their own now.
    let (own_part, p, _) = doc.resolve_part(&target).unwrap();
    assert_ne!(own_part, part);
    assert_eq!(p.machinings().len(), 1);
    let own_cabinet = occ(doc, &EntityPath::root(cab2)).component;
    assert_ne!(own_cabinet, cabinet);
    assert_eq!(doc.component(own_cabinet).unwrap().name, "Caisson#2");
    // The others are untouched: the first cabinet's shelf and the loose one share the old part.
    assert_eq!(doc.resolve_part(&EntityPath::new(vec![cab], shelf)).unwrap().0, part);
    assert_eq!(doc.resolve_part(&EntityPath::root(loose)).unwrap().0, part);
    assert!(doc.part(part).unwrap().machinings().is_empty());
    // Occurrence ids are unique within an assembly only: the file says so, and opens.
    let back = Document::from_json(&doc.to_json().unwrap()).unwrap();
    assert_eq!(back, *doc);

    // Nothing shared on the way: nothing copied.
    let n = doc.components().count();
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(1300.0, 150.0, 10.0), radius: 4.0, height: 20.0 });
    h.apply(Operation::boolean(target.clone(), EntityPath::root(drill), Boolean::Difference, Scope::This)).unwrap();
    assert_eq!(h.document().components().count(), n + 1, "the drill's part unused, nothing else copied");
    assert_eq!(h.document().resolve_part(&target).unwrap().1.machinings().len(), 2);

    // All occurrences: the first cabinet's shelf and the loose one both get the hole.
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(250.0, 150.0, 10.0), radius: 4.0, height: 20.0 });
    h.apply(Operation::boolean(EntityPath::new(vec![cab], shelf), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    assert_eq!(h.document().part(part).unwrap().machinings().len(), 1);
    assert_eq!(h.document().resolve_part(&EntityPath::root(loose)).unwrap().0, part);
}

#[test]
fn materials_paint_parts_assemblies_and_faces() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let oak = h
        .apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90], kind: Default::default() })
        .unwrap()
        .material
        .unwrap();
    let walnut =
        h.apply(Operation::AddMaterial { name: "Noyer".into(), color: [90, 60, 40], kind: Default::default() }).unwrap().material.unwrap();
    assert_eq!(h.document().materials().count(), 2);
    let paint = |targets: Vec<EntityPath>, material, scope| Operation::PaintParts { targets, layer: Family::Coloration, material, scope };

    // A part painted: on its definition, the other part not.
    h.apply(paint(vec![EntityPath::root(a)], Some(oak), Scope::All)).unwrap();
    let placed = |h: &History, o: Id| h.document().placed_parts().into_iter().find(|p| p.path.entity == o).unwrap().shown().top();
    assert_eq!(placed(&h, a), Some(oak));
    assert_eq!(placed(&h, b), None);
    let part_a = h.document().root_assembly().occurrence(a).unwrap().component;
    assert_eq!(h.document().part(part_a).unwrap().coloration, Some(oak));

    // An assembly painted paints every part below it, one painted too; it has none itself.
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().created[0];
    h.apply(paint(vec![EntityPath::root(asm)], Some(walnut), Scope::All)).unwrap();
    assert_eq!((placed(&h, a), placed(&h, b)), (Some(walnut), Some(walnut)));

    // A face of a part: kept by the part (every occurrence), refused past the last face.
    let part = h.document().placed_parts().into_iter().find(|p| p.path.entity == b).unwrap().part;
    h.apply(Operation::PaintFaces { part, faces: vec![0, 2], layer: Family::Coloration, material: Some(oak) }).unwrap();
    assert_eq!(h.document().part(part).unwrap().faces.len(), 2);
    assert!(h.apply(Operation::PaintFaces { part, faces: vec![6], layer: Family::Coloration, material: Some(oak) }).is_err());
    h.apply(Operation::PaintFaces { part, faces: vec![2], layer: Family::Coloration, material: None }).unwrap();
    assert_eq!(h.document().part(part).unwrap().faces.keys().copied().collect::<Vec<_>>(), [0]);

    // Recolouring in a row is one step; the file keeps it all.
    let before = h.document().clone();
    h.apply(Operation::EditMaterial { material: oak, name: "Chêne".into(), color: [190, 150, 100] }).unwrap();
    h.apply(Operation::EditMaterial { material: oak, name: "Chêne".into(), color: [200, 160, 110] }).unwrap();
    assert_eq!(h.document().material(oak).unwrap().color, [200, 160, 110]);
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(&back, h.document());
    assert!(h.undo());
    assert_eq!(h.document(), &before, "both recolourings undo together");

    // Deleting a material unpaints what used it, parts and faces.
    h.apply(paint(vec![EntityPath::new(vec![asm], a)], Some(oak), Scope::All)).unwrap();
    h.apply(Operation::DeleteMaterial { material: oak }).unwrap();
    assert_eq!((placed(&h, a), placed(&h, b)), (None, Some(walnut)));
    assert!(h.document().part(part).unwrap().faces.is_empty());
    assert!(h.apply(paint(vec![EntityPath::root(asm)], Some(oak), Scope::All)).is_err());
}

#[test]
fn a_part_is_painted_on_its_definition_placed_or_not() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::splat(20.0), 5.0));
    let (pa, pb) = (occ(h.document(), &EntityPath::root(a)).component, occ(h.document(), &EntityPath::root(b)).component);
    let item = |h: &mut History, name: &str, kind| {
        h.apply(Operation::AddMaterial { name: name.into(), color: [1, 2, 3], kind }).unwrap().material.unwrap()
    };
    let oak = item(&mut h, "Chêne massif", Some(MaterialKind::Solid));
    let paint = item(&mut h, "Peinture", None);
    // B placed nowhere: still painted, as the library's entity info does.
    h.apply(Operation::Delete { context: root, entities: vec![b] }).unwrap();
    let part = |h: &History, id| h.document().part(id).unwrap().clone();
    let count = h.document().components().count();
    h.apply(Operation::PaintPart { part: pb, layer: Family::Material, material: Some(oak) }).unwrap();
    assert_eq!(part(&h, pb).material, Some(oak));
    // Placed: every occurrence shows it, no copy made.
    h.apply(Operation::PlaceOccurrence { context: root, component: pa, transform: DAffine3::from_translation(DVec3::X * 50.0) }).unwrap();
    h.apply(Operation::PaintPart { part: pa, layer: Family::Material, material: Some(oak) }).unwrap();
    assert_eq!((part(&h, pa).material, h.document().components().count()), (Some(oak), count));
    h.apply(Operation::PaintPart { part: pa, layer: Family::Material, material: None }).unwrap();
    assert_eq!(part(&h, pa).material, None);
    // Each family to its layer; only a part.
    let refused = |r: Result<Outcome>| matches!(r, Err(Error::Invalid(Refusal::MaterialLayer)));
    assert!(refused(h.apply(Operation::PaintPart { part: pa, layer: Family::Material, material: Some(paint) })));
    assert!(refused(h.apply(Operation::PaintPart { part: pa, layer: Family::Covering, material: None })));
    assert!(h.apply(Operation::PaintPart { part: root, layer: Family::Material, material: None }).is_err());
}

#[test]
fn each_family_goes_to_its_layer_colorations_over_the_others() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let item = |h: &mut History, name: &str, kind| {
        h.apply(Operation::AddMaterial { name: name.into(), color: [1, 2, 3], kind }).unwrap().material.unwrap()
    };
    let oak = item(&mut h, "Chêne massif", Some(MaterialKind::Solid));
    let veneer = item(&mut h, "Placage chêne", Some(MaterialKind::Veneer));
    let paint = item(&mut h, "Peinture", None);
    let families = |h: &History| [oak, veneer, paint].map(|m| h.document().material(m).unwrap().family());
    assert_eq!(families(&h), [Family::Material, Family::Covering, Family::Coloration]);
    let part = h.document().root_assembly().occurrence(a).unwrap().component;
    let on_part = |layer, material| Operation::PaintParts { targets: vec![EntityPath::root(a)], layer, material, scope: Scope::All };
    let on_face = |layer, material| Operation::PaintFaces { part, faces: vec![0], layer, material };
    let refused = |r: Result<Outcome>| matches!(r, Err(Error::Invalid(Refusal::MaterialLayer)));

    // Each family to its layer; anywhere else, refused.
    h.apply(on_part(Family::Material, Some(oak))).unwrap();
    h.apply(on_face(Family::Covering, Some(veneer))).unwrap();
    assert!(refused(h.apply(on_part(Family::Coloration, Some(oak)))));
    assert!(refused(h.apply(on_part(Family::Material, Some(veneer)))));
    assert!(refused(h.apply(on_face(Family::Covering, Some(oak)))));
    assert!(refused(h.apply(on_face(Family::Coloration, Some(veneer)))));
    assert!(refused(h.apply(on_part(Family::Covering, None))), "no covering on a part");
    assert!(refused(h.apply(on_face(Family::Material, None))), "no material on a face");

    // Seen: the coloration over the material, a face's coloration over its covering.
    let p = |h: &History| h.document().part(part).unwrap().clone();
    assert_eq!((p(&h).shown().top(), p(&h).faces_shown().get(&0).and_then(|s| s.top())), (Some(oak), Some(veneer)));
    h.apply(on_part(Family::Coloration, Some(paint))).unwrap();
    h.apply(on_face(Family::Coloration, Some(paint))).unwrap();
    assert_eq!(
        (p(&h).material, p(&h).shown().top(), p(&h).faces_shown().get(&0).and_then(|s| s.top())),
        (Some(oak), Some(paint), Some(paint))
    );
    h.apply(on_part(Family::Coloration, None)).unwrap();
    assert_eq!(p(&h).shown().top(), Some(oak), "the coloration taken off: the material again");

    // A type changes within its family only.
    h.apply(Operation::SetMaterialKind { material: oak, kind: MaterialKind::Sheet }).unwrap();
    assert_eq!(h.document().material(oak).unwrap().kind, Some(MaterialKind::Sheet));
    assert!(refused(h.apply(Operation::SetMaterialKind { material: oak, kind: MaterialKind::Edge })));
    assert!(refused(h.apply(Operation::SetMaterialKind { material: paint, kind: MaterialKind::Solid })));

    // A table per family in the file; an item without a type of its table's family takes its
    // first one.
    let json = h.document().to_json().unwrap();
    for key in ["\"materials\":", "\"coverings\":", "\"colorations\":"] {
        assert!(json.contains(key), "{key}");
    }
    assert_eq!(&Document::from_json(&json).unwrap(), h.document());
    let json = json.replace("\"kind\":\"sheet\",", "");
    assert_eq!(Document::from_json(&json).unwrap().material(oak).unwrap().kind, Some(MaterialKind::Generic));
}

#[test]
fn a_source_is_kept_by_the_file_and_the_copies() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = h
        .apply(Operation::AddPrimitive { context: root, primitive: cube(DVec3::ZERO, 10.0), placement: DAffine3::IDENTITY, material: None })
        .unwrap()
        .created[0];
    h.apply(Operation::Duplicate { context: root, entities: vec![a], transform: DAffine3::from_translation(DVec3::X * 20.0) }).unwrap();
    let occurrences = h.document().assembly(root).unwrap().occurrences().iter().map(|o| (o.id, o.component)).collect::<Vec<_>>();
    let part = occurrences[0].1;
    h.apply(Operation::SetSource { component: part, source: Source::Bought }).unwrap();
    let doc = Document::from_file(&h.document().to_file().unwrap()).unwrap();
    assert_eq!(doc.component(part).unwrap().source, Source::Bought, "saved");
    h.apply(Operation::MakeUnique { context: root, occurrence: occurrences[1].0 }).unwrap();
    let copy = h.document().assembly(root).unwrap().occurrence(occurrences[1].0).unwrap().component;
    assert_ne!(copy, part);
    assert_eq!(h.document().component(copy).unwrap().source, Source::Bought, "a copy comes from where its part does");
}

#[test]
fn schemes_lay_coats_of_the_catalog_which_leave_them_when_deleted() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let item = |h: &mut History, name: &str, kind| {
        h.apply(Operation::AddMaterial { name: name.into(), color: [9, 9, 9], kind }).unwrap().material.unwrap()
    };
    let stain = item(&mut h, "Teinte noyer", None);
    let varnish = item(&mut h, "Vernis ambré", Some(MaterialKind::Coat));
    let scheme = item(&mut h, "Noyer verni", Some(MaterialKind::Scheme));
    let oak = item(&mut h, "Chêne", Some(MaterialKind::Solid));
    let refused = |r: Result<Outcome>| matches!(r, Err(Error::Invalid(Refusal::MaterialLayer)));
    assert_eq!(h.document().material(stain).unwrap().kind, Some(MaterialKind::Coat), "a coat by default");

    // A coat's blend and intensity; dragging it merges into one step.
    h.apply(Operation::SetCoat { material: stain, blend: BlendMode::Tint, intensity: 0.4 }).unwrap();
    h.apply(Operation::SetCoat { material: stain, blend: BlendMode::Tint, intensity: 0.6 }).unwrap();
    assert_eq!(h.document().material(stain).unwrap().intensity, 0.6);
    assert!(refused(h.apply(Operation::SetCoat { material: stain, blend: BlendMode::Cover, intensity: 1.5 })));
    assert!(refused(h.apply(Operation::SetCoat { material: scheme, blend: BlendMode::Cover, intensity: 1.0 })));

    // A scheme lays coats, twice the same if need be; never a scheme nor a material.
    h.apply(Operation::SetCoats { material: scheme, coats: vec![stain, varnish, varnish] }).unwrap();
    assert!(refused(h.apply(Operation::SetCoats { material: scheme, coats: vec![scheme] })));
    assert!(refused(h.apply(Operation::SetCoats { material: scheme, coats: vec![oak] })));
    assert!(refused(h.apply(Operation::SetCoats { material: stain, coats: vec![varnish] })), "a coat lays none");
    let ids = |h: &History| h.document().coats_of(scheme).into_iter().map(|(id, _)| id).collect::<Vec<_>>();
    assert_eq!(ids(&h), [stain, varnish, varnish]);
    assert_eq!(h.document().schemes_with(varnish), [scheme]);

    // Laid on a part: the scheme and its coats count as used, kept by the purge.
    let paint =
        Operation::PaintParts { targets: vec![EntityPath::root(a)], layer: Family::Coloration, material: Some(scheme), scope: Scope::All };
    h.apply(paint).unwrap();
    h.apply(Operation::PurgeMaterials).unwrap();
    for m in [stain, varnish, scheme] {
        assert!(h.document().material(m).is_ok());
    }
    assert!(h.document().material(oak).is_err(), "unused: purged");

    // A file keeps it all; a material file brings a scheme's coats along.
    assert_eq!(&Document::from_json(&h.document().to_json().unwrap()).unwrap(), h.document());
    let alone = h.document().materials_fragment(&[scheme]).unwrap();
    assert_eq!(alone.materials().count(), 3);
    let (_, s) = alone.materials().find(|(_, m)| m.is_scheme()).unwrap();
    assert_eq!(s.coats.len(), 3);
    assert!(s.coats.iter().all(|c| alone.material(*c).is_ok()));

    // Deleted, a coat leaves the schemes laying it.
    h.apply(Operation::DeleteMaterial { material: varnish }).unwrap();
    assert_eq!(ids(&h), [stain]);
}

#[test]
fn painting_only_these_copies_makes_their_parts_unique() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().created[0];
    let t = DAffine3::from_translation(DVec3::Y * 50.0);
    let copy = h.apply(Operation::Duplicate { context: root, entities: vec![asm], transform: t }).unwrap().created[0];
    let oak = h
        .apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90], kind: Default::default() })
        .unwrap()
        .material
        .unwrap();
    let parts = |h: &History| h.document().components().filter(|c| c.is_part()).count();
    let material = |h: &History, p: EntityPath| h.document().resolve_part(&p).unwrap().1.coloration;
    assert_eq!(parts(&h), 2);

    // « Only this one » on one copy of the assembly: it and its parts made its own.
    let paint = |targets, scope| Operation::PaintParts { targets, layer: Family::Coloration, material: Some(oak), scope };
    h.apply(paint(vec![EntityPath::root(copy)], Scope::This)).unwrap();
    assert_eq!(parts(&h), 4);
    assert_eq!((material(&h, EntityPath::new(vec![copy], a)), material(&h, EntityPath::new(vec![copy], b))), (Some(oak), Some(oak)));
    assert_eq!(material(&h, EntityPath::new(vec![asm], a)), None, "the other copy left as it was");
    assert!(h.undo());

    // Every placement of a part painted: the part itself, no copy.
    h.apply(paint(vec![EntityPath::new(vec![asm], a), EntityPath::new(vec![copy], a)], Scope::This)).unwrap();
    assert_eq!(parts(&h), 2);
    assert_eq!(material(&h, EntityPath::new(vec![asm], a)), Some(oak));
    assert!(h.undo());

    // One part of one copy: that copy of the assembly made its own, a copy of the part alone.
    h.apply(paint(vec![EntityPath::new(vec![copy], a)], Scope::This)).unwrap();
    assert_eq!(parts(&h), 3);
    assert_eq!(material(&h, EntityPath::new(vec![copy], b)), None);
    let doc = h.document();
    let shared = |o: Id| {
        (doc.resolve(&EntityPath::new(vec![asm], o)).unwrap().component, doc.resolve(&EntityPath::new(vec![copy], o)).unwrap().component)
    };
    assert_eq!(shared(b).0, shared(b).1, "b still one part");
}

#[test]
fn occurrences_hide_and_show() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let c = add(&mut h, cube(DVec3::new(40.0, 0.0, 0.0), 10.0));
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().created[0];
    let hidden_at = |h: &History, o: Id| h.document().placed_parts().into_iter().find(|p| p.path.entity == o).unwrap().hidden_at;
    assert_eq!(hidden_at(&h, a), None);

    // A part hidden: its own level; an assembly hidden: its level, for every part under it.
    h.apply(Operation::SetHidden { context: asm_def(&h, asm), entities: vec![b], hidden: true }).unwrap();
    assert_eq!(h.undo_op().map(|o| o.label()), Some("Masquer".to_owned()));
    assert_eq!((hidden_at(&h, a), hidden_at(&h, b), hidden_at(&h, c)), (None, Some(1), None));
    h.apply(Operation::SetHidden { context: root, entities: vec![asm], hidden: true }).unwrap();
    assert_eq!((hidden_at(&h, a), hidden_at(&h, b)), (Some(0), Some(1)), "the deepest one");

    // Saved with the file; a file without the flag shows everything.
    let json = h.document().to_json().unwrap();
    assert_eq!(&Document::from_json(&json).unwrap(), h.document());
    assert_eq!(json.matches("\"hidden\":true").count(), 2);

    // Exploded, a hidden assembly leaves its content hidden.
    let mut x = h.clone();
    let out = x.apply(Operation::Explode { context: root, occurrence: asm }).unwrap();
    assert!(out.created.iter().all(|id| x.document().root_assembly().occurrence(*id).unwrap().hidden));

    // Shown again one by one, or all at once; undone in one step.
    h.apply(Operation::SetHidden { context: root, entities: vec![asm], hidden: false }).unwrap();
    assert_eq!(h.undo_op().map(|o| o.label()), Some("Afficher".to_owned()));
    assert_eq!(hidden_at(&h, a), None);
    h.apply(Operation::SetHidden { context: root, entities: vec![asm, c], hidden: true }).unwrap();
    h.apply(Operation::ShowAll).unwrap();
    assert!(h.document().placed_parts().iter().all(|p| p.hidden_at.is_none()));
    assert!(h.undo());
    assert_eq!((hidden_at(&h, b), hidden_at(&h, c)), (Some(1), Some(0)));

    // An occurrence of another assembly is refused.
    assert!(h.apply(Operation::SetHidden { context: root, entities: vec![a], hidden: true }).is_err());
}

/// The assembly the occurrence `o` of the root places.
fn asm_def(h: &History, o: Id) -> Id {
    h.document().root_assembly().occurrence(o).unwrap().component
}

#[test]
fn a_file_with_an_unknown_material_is_refused() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let m = h
        .apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90], kind: Default::default() })
        .unwrap()
        .material
        .unwrap();
    h.apply(Operation::PaintParts { targets: vec![EntityPath::root(a)], layer: Family::Coloration, material: Some(m), scope: Scope::All })
        .unwrap();
    let json = h.document().to_json().unwrap();
    let broken = json.replace(&format!("\"coloration\":{}", m.0), "\"coloration\":999");
    assert_ne!(broken, json);
    assert!(Document::from_json(&broken).is_err());
}

#[test]
fn the_section_is_a_setting() {
    let mut h = History::new(Document::new());
    let mut s = h.document().settings().clone();
    assert!(s.display.silhouettes && s.display.active_section().is_none());
    s.display.section = Some(Section { origin: DVec3::new(0.0, 0.0, 5.0), normal: DVec3::Z });
    s.display.section_active = true;
    h.apply(Operation::SetSettings(s)).unwrap();
    let sec = h.document().settings().display.active_section().unwrap();
    assert!(sec.cuts(DVec3::new(0.0, 0.0, 6.0)) && !sec.cuts(DVec3::new(0.0, 0.0, 4.0)));
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.settings(), h.document().settings());
}

/// A board drilled once at (20, 25), its occurrence and part.
fn drilled_board(h: &mut History) -> (EntityPath, Id, Id) {
    let board = add(h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let drill = add(h, Primitive::Cylinder { origin: DVec3::new(20.0, 25.0, -5.0), radius: 4.0, height: 30.0 });
    let target = EntityPath::root(board);
    h.apply(Operation::boolean(target.clone(), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let (part, p, _) = h.document().resolve_part(&target).unwrap();
    (target, part, p.machinings()[0].id)
}

fn faces(doc: &Document, part: Id) -> usize {
    doc.part(part).unwrap().shape().topology_counts().0
}

#[test]
fn machinings_are_copied_arrayed_moved_and_deleted() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    let one = faces(h.document(), part);

    // A copy is cut from the shape as it is: one tool in the recipe.
    let dx = |x: f64| DAffine3::from_translation(DVec3::X * x);
    let op = Operation::DuplicateBodies { target: target.clone(), bodies: vec![hole], transform: dx(40.0), scope: Scope::All };
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Recipe { negatives: tools, .. }] if tools.len() == 1), "{:?}", job.tasks);
    let copy = h.apply(op).unwrap().machinings()[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.machinings().len(), 2);
    assert!(close(p.machining(copy).unwrap().placement.translation, DVec3::new(60.0, 25.0, -5.0)));
    let two = faces(h.document(), part);
    assert!(two > one, "{two} > {one}");

    // An array: three more, 40 apart.
    let created = h
        .apply(Operation::ArrayBodies { target: target.clone(), bodies: vec![copy], step: dx(40.0), count: 3, scope: Scope::All })
        .unwrap()
        .machinings();
    assert_eq!(created.len(), 3);
    let p = h.document().part(part).unwrap();
    assert!(close(p.machining(created[2]).unwrap().placement.translation, DVec3::new(180.0, 25.0, -5.0)));
    assert!(p.failed.is_empty());

    // A move computes the part again from the blank: every tool in the recipe.
    let op = Operation::TransformBodies { target: target.clone(), bodies: vec![hole], transform: dx(-10.0), scope: Scope::All };
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Recipe { negatives: tools, .. }] if tools.len() == 5));
    h.apply(op).unwrap();
    assert!(close(h.document().part(part).unwrap().machining(hole).unwrap().placement.translation, DVec3::new(10.0, 25.0, -5.0)));
    assert!(
        h.apply(Operation::TransformBodies {
            target: target.clone(),
            bodies: vec![hole],
            transform: DAffine3::from_scale(DVec3::splat(2.0)),
            scope: Scope::All
        })
        .is_err()
    );

    // Deleting all but one, then the last: the blank is the shape again, without the kernel.
    let all: Vec<Id> = h.document().part(part).unwrap().machinings().iter().map(|m| m.id).collect();
    h.apply(Operation::DeleteBodies { target: target.clone(), bodies: all[1..].to_vec(), scope: Scope::All }).unwrap();
    assert_eq!(faces(h.document(), part), one);
    let op = Operation::DeleteBodies { target: target.clone(), bodies: vec![hole], scope: Scope::All };
    assert!(matches!(h.document().prepare(&op).unwrap(), Prepared::Done(_)));
    h.apply(op).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.machinings().is_empty() && p.shape.is_none());

    // Refused: no machining named, one the part does not have.
    assert!(h.apply(Operation::DeleteBodies { target: target.clone(), bodies: vec![], scope: Scope::All }).is_err());
    assert!(h.apply(Operation::DeleteBodies { target, bodies: vec![hole], scope: Scope::All }).is_err());
    // Undo brings them back.
    h.undo();
    assert_eq!(h.document().part(part).unwrap().machinings().len(), 1);
}

#[test]
fn a_machining_swallowing_the_part_leaves_it_empty() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    // A tool swallowing the board, as a moved machining could.
    let mut doc = h.document().clone();
    let huge = doc.fresh_id();
    let tool = Arc::new(kernel::Solid::new_box(DVec3::splat(-100.0), DVec3::splat(1000.0)).unwrap());
    let def = doc.fresh_id();
    let p = doc.part_mut(part).unwrap();
    p.defs.push(BodyDef::new(def, tool));
    p.bodies.push(Body::new(huge, def, Sign::Negative, DAffine3::IDENTITY));
    let mut h = History::new(doc);

    // Moving the hole computes the part from the blank: the huge tool leaves nothing, no
    // failure (docs/model/bodies.md#empty-parts).
    h.apply(Operation::TransformBodies {
        target: target.clone(),
        bodies: vec![hole],
        transform: DAffine3::from_translation(DVec3::X * 50.0),
        scope: Scope::All,
    })
    .unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.shape().is_empty() && p.pieces() == 0 && p.failed.is_empty());
    // Saved with the file.
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert!(back.part(part).unwrap().shape().is_empty());
    // A machining added to the empty part: still empty.
    let drill = Primitive::Cylinder { origin: DVec3::new(20.0, 25.0, -5.0), radius: 3.0, height: 30.0 };
    let add = Operation::AddBody {
        target: target.clone(),
        primitive: drill,
        placement: DAffine3::IDENTITY,
        sign: Sign::Negative,
        scope: Scope::All,
    };
    h.apply(add).unwrap();
    assert!(h.document().part(part).unwrap().shape().is_empty());
    // The tool gone: the board back, drilled.
    h.apply(Operation::DeleteBodies { target, bodies: vec![huge], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.failed.is_empty() && p.pieces() == 1);
    assert!(faces(h.document(), part) > p.blank().topology_counts().0);
}

#[test]
fn only_this_one_separates_a_machined_part_keeping_machining_ids() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let (target, part, hole) = drilled_board(&mut h);
    let other = h
        .apply(Operation::Duplicate {
            context: root,
            entities: vec![target.entity],
            transform: DAffine3::from_translation(DVec3::Y * 100.0),
        })
        .unwrap()
        .created[0];

    h.apply(Operation::DeleteBodies { target: target.clone(), bodies: vec![hole], scope: Scope::This }).unwrap();
    let doc = h.document();
    let (own, p, _) = doc.resolve_part(&target).unwrap();
    assert_ne!(own, part);
    assert!(p.machinings().is_empty());
    // The other occurrence keeps the hole, under the same id.
    let (shared, q, _) = doc.resolve_part(&EntityPath::root(other)).unwrap();
    assert_eq!(shared, part);
    assert!(q.machining(hole).is_some());

    // Two parts with the same machining id: the file opens again (ids are unique per part).
    let mut h = History::new(Document::new());
    let (target, _, hole) = drilled_board(&mut h);
    let root = h.document().root();
    h.apply(Operation::Duplicate { context: root, entities: vec![target.entity], transform: DAffine3::from_translation(DVec3::Y * 100.0) })
        .unwrap();
    h.apply(Operation::TransformBodies {
        target: target.clone(),
        bodies: vec![hole],
        transform: DAffine3::from_translation(DVec3::X * 30.0),
        scope: Scope::This,
    })
    .unwrap();
    let doc = h.document();
    assert_eq!(doc.components().filter(|c| c.as_part().is_some_and(|p| p.machining(hole).is_some())).count(), 2);
    assert_eq!(Document::from_json(&doc.to_json().unwrap()).unwrap(), *doc);
}

#[test]
fn a_primitive_drawn_in_a_part_is_a_machining() {
    let mut h = History::new(Document::new());
    let (target, part, _) = drilled_board(&mut h);
    let before = faces(h.document(), part);
    // A groove along the board's top, flush with its ends, over the hole, in its axes.
    let groove = Primitive::Box { origin: DVec3::new(0.0, 20.0, 12.0), size: DVec3::new(200.0, 10.0, 10.0) };
    let out = h
        .apply(Operation::AddBody {
            target: target.clone(),
            primitive: groove,
            placement: DAffine3::IDENTITY,
            sign: Sign::Negative,
            scope: Scope::All,
        })
        .unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.machinings().len(), 2);
    let m = p.machining(out.machinings()[0]).unwrap();
    // The tool at its own origin, placed where it was drawn.
    assert!(close(m.placement.translation, DVec3::new(0.0, 20.0, 12.0)));
    assert!(close(p.solid_of(m).vertex_bounds().unwrap().0, DVec3::ZERO));
    assert!(p.failed.is_empty(), "{:?}", p.failed);
    assert!(faces(h.document(), part) > before, "{} {before}", faces(h.document(), part));
    assert!(
        h.apply(Operation::AddBody {
            target,
            primitive: cube(DVec3::ZERO, 5.0),
            placement: DAffine3::from_scale(DVec3::splat(2.0)),
            sign: Sign::Negative,
            scope: Scope::All
        })
        .is_err()
    );
}

#[test]
fn guides_are_erased_moved_copied_and_grouped_like_occurrences() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let line = GuideShape::Line { origin: DVec3::new(0.0, 5.0, 0.0), dir: DVec3::X };
    let g = h.apply(Operation::AddGuide { component: root, shape: line }).unwrap().guide.unwrap();
    let guides = |h: &History| h.document().component(root).unwrap().guides.clone();
    let up = DAffine3::from_translation(DVec3::new(0.0, 0.0, 20.0));

    // Moved with an occurrence.
    h.apply(Operation::Transform { context: root, entities: vec![a, g], transform: up }).unwrap();
    assert_eq!(guides(&h)[0].shape, GuideShape::Line { origin: DVec3::new(0.0, 5.0, 20.0), dir: DVec3::X });
    assert!(close(occ(h.document(), &EntityPath::root(a)).transform.translation, DVec3::new(0.0, 0.0, 20.0)));
    // Copied, alone (no kernel): the copy among what was created.
    let out = h.apply(Operation::Array { context: root, entities: vec![g], step: up, count: 2 }).unwrap();
    assert_eq!(out.created.len(), 2);
    assert_eq!(guides(&h).len(), 3);
    assert_eq!(guides(&h)[2].shape, GuideShape::Line { origin: DVec3::new(0.0, 5.0, 60.0), dir: DVec3::X });
    // Erased.
    h.apply(Operation::Delete { context: root, entities: vec![out.created[0]] }).unwrap();
    assert_eq!(guides(&h).len(), 2);
    // Grouped: the guide goes into the assembly, where it stood — in its axes, whose origin is
    // the low corner of its content.
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, g], name: None }).unwrap().created[0];
    assert_eq!(guides(&h).len(), 1);
    assert!(close(occ(h.document(), &EntityPath::root(asm)).transform.translation, DVec3::new(0.0, 0.0, 20.0)));
    let def = occ(h.document(), &EntityPath::root(asm)).component;
    let moved = GuideShape::Line { origin: DVec3::new(0.0, 5.0, 0.0), dir: DVec3::X };
    assert_eq!(h.document().component(def).unwrap().guide(g).unwrap().shape, moved);
    // Only guides: no assembly.
    let rest = guides(&h)[0].id;
    assert!(h.apply(Operation::MakeAssembly { context: root, entities: vec![rest], name: None }).is_err());
}

#[test]
fn a_part_s_guides_go_with_its_machinings() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 100.0));
    let b = add(&mut h, Primitive::Cylinder { origin: DVec3::new(50.0, 50.0, 80.0), radius: 5.0, height: 30.0 });
    h.apply(Operation::boolean(EntityPath::root(a), EntityPath::root(b), Boolean::Difference, Scope::All)).unwrap();
    let target = EntityPath::root(a);
    let part = occ(h.document(), &target).component;
    let circle = GuideShape::Circle { center: DVec3::new(50.0, 50.0, 100.0), normal: DVec3::Z, radius: 20.0 };
    let g = h.apply(Operation::AddGuide { component: part, shape: circle }).unwrap().guide.unwrap();
    let m = h.document().part(part).unwrap().machinings()[0].id;
    let shift = DAffine3::from_translation(DVec3::new(10.0, 0.0, 0.0));

    // Guides alone: no kernel, no separate part even "only this one".
    let doc = h.document().clone();
    let op = Operation::TransformBodies { target: target.clone(), bodies: vec![g], transform: shift, scope: Scope::This };
    assert!(matches!(doc.prepare(&op).unwrap(), Prepared::Done(_)));
    h.apply(op).unwrap();
    assert_eq!(occ(h.document(), &target).component, part);
    let GuideShape::Circle { center, .. } = h.document().component(part).unwrap().guide(g).unwrap().shape else { panic!() };
    assert!(close(center, DVec3::new(60.0, 50.0, 100.0)));
    // Copied with a machining: both copies created.
    let out = h.apply(Operation::DuplicateBodies { target: target.clone(), bodies: vec![m, g], transform: shift, scope: Scope::All });
    assert_eq!(out.unwrap().machinings().len(), 2);
    assert_eq!(h.document().component(part).unwrap().guides.len(), 2);
    // Erased alone.
    h.apply(Operation::DeleteBodies { target, bodies: vec![g], scope: Scope::All }).unwrap();
    assert_eq!(h.document().component(part).unwrap().guides.len(), 1);
    assert_eq!(h.document().part(part).unwrap().machinings().len(), 2);
}

/// A stretch of the occurrence `target`, in its component's axes.
fn stretch(doc: &Document, target: &EntityPath, axis: DVec3, cuts: Vec<f64>, moves: Vec<f64>, scope: Scope) -> Operation {
    let frame = doc.resolve(target).unwrap().transform;
    Operation::Stretch {
        context: target.occurrences.clone(),
        occurrences: vec![target.entity],
        frame,
        axis,
        cuts,
        moves,
        scope,
        cutters: None,
    }
}

/// A stretch along the part's X: the cutter at `cut`, the far side moved by `d`.
fn stretch_x(doc: &Document, target: &EntityPath, cut: f64, d: f64, scope: Scope) -> Operation {
    stretch(doc, target, DVec3::X, vec![cut], vec![0.0, d], scope)
}

#[test]
fn a_stretch_stretches_the_blank_and_moves_or_stretches_the_machinings() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    // A second hole past the cutter, and a groove along the whole board.
    let far = h
        .apply(Operation::DuplicateBodies {
            target: target.clone(),
            bodies: vec![hole],
            transform: DAffine3::from_translation(DVec3::X * 160.0),
            scope: Scope::All,
        })
        .unwrap()
        .machinings()[0];
    let groove = Primitive::Box { origin: DVec3::new(0.0, 20.0, 12.0), size: DVec3::new(200.0, 10.0, 10.0) };
    let groove = h
        .apply(Operation::AddBody {
            target: target.clone(),
            primitive: groove,
            placement: DAffine3::IDENTITY,
            sign: Sign::Negative,
            scope: Scope::All,
        })
        .unwrap()
        .machinings()[0];

    // The blank and the groove's tool are stretched, the holes only moved; then the recipe.
    let op = stretch_x(h.document(), &target, 100.0, 50.0, Scope::All);
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(
        matches!(&job.tasks[..], [kernel::Task::Stretch { .. }, kernel::Task::Stretch { .. }, kernel::Task::Recipe { negatives: tools, .. }] if tools.len() == 3),
        "{:?}",
        job.tasks
    );
    h.apply(op).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(close(p.blank().vertex_bounds().unwrap().1, DVec3::new(250.0, 50.0, 18.0)));
    assert!(close(p.machining(hole).unwrap().placement.translation, DVec3::new(20.0, 25.0, -5.0)));
    assert!(close(p.machining(far).unwrap().placement.translation, DVec3::new(230.0, 25.0, -5.0)));
    assert!(close(p.solid_of(p.machining(groove).unwrap()).vertex_bounds().unwrap().1, DVec3::new(250.0, 10.0, 10.0)));
    assert!(p.failed.is_empty(), "{:?}", p.failed);
    assert!(close(p.shape().bounds().unwrap().1, DVec3::new(250.0, 50.0, 18.0)));

    // Compressed back, then undone.
    h.apply(stretch_x(h.document(), &target, 100.0, -50.0, Scope::All)).unwrap();
    assert!(close(h.document().part(part).unwrap().blank().vertex_bounds().unwrap().1, DVec3::new(200.0, 50.0, 18.0)));
    h.undo();
    h.undo();
    assert!(close(h.document().part(part).unwrap().blank().vertex_bounds().unwrap().1, DVec3::new(200.0, 50.0, 18.0)));
    assert!(close(h.document().part(part).unwrap().machining(far).unwrap().placement.translation, DVec3::new(180.0, 25.0, -5.0)));
}

#[test]
fn a_bare_part_is_stretched_without_a_recipe() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let target = EntityPath::root(board);
    let op = stretch(h.document(), &target, DVec3::Y, vec![25.0], vec![-10.0, 10.0], Scope::All);
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Stretch { .. }]), "{:?}", job.tasks);
    h.apply(op).unwrap();
    let (_, p, t) = h.document().resolve_part(&target).unwrap();
    assert!(p.shape.is_none());
    // The origin, in the section moving by -10, follows it (OCL's anchor).
    assert_eq!(p.blank().vertex_bounds().unwrap(), (DVec3::ZERO, DVec3::new(200.0, 70.0, 18.0)));
    assert!(close(t.translation, DVec3::new(0.0, -10.0, 0.0)));
}

#[test]
fn the_origin_follows_its_section_and_the_other_occurrences_show_the_same_change() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let target = EntityPath::root(board);
    let other = h
        .apply(Operation::Duplicate { context: root, entities: vec![board], transform: DAffine3::from_translation(DVec3::Y * 100.0) })
        .unwrap()
        .created[0];
    let other = EntityPath::root(other);
    // The origin's side pulled: the origin moves with it, the far end stays.
    h.apply(stretch(h.document(), &target, DVec3::X, vec![100.0], vec![-30.0, 0.0], Scope::All)).unwrap();
    let doc = h.document();
    let (part, p, t) = doc.resolve_part(&target).unwrap();
    assert_eq!(p.blank().vertex_bounds().unwrap(), (DVec3::ZERO, DVec3::new(230.0, 50.0, 18.0)));
    assert!(close(t.translation, DVec3::new(-30.0, 0.0, 0.0)));
    assert!(close(world_bounds(doc, &target).0, DVec3::new(-30.0, 0.0, 0.0)));
    // The other occurrence, not stretched, shows the same change in its own axes.
    assert_eq!(doc.resolve_part(&other).unwrap().0, part);
    let (lo, hi) = world_bounds(doc, &other);
    assert!(close(lo, DVec3::new(-30.0, 100.0, 0.0)) && close(hi, DVec3::new(200.0, 150.0, 18.0)), "{lo} {hi}");
}

#[test]
fn the_origin_stays_when_its_section_does_not_move() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let target = EntityPath::root(board);
    h.apply(stretch_x(h.document(), &target, 100.0, 30.0, Scope::All)).unwrap();
    let (_, p, t) = h.document().resolve_part(&target).unwrap();
    assert_eq!(p.blank().vertex_bounds().unwrap(), (DVec3::ZERO, DVec3::new(230.0, 50.0, 18.0)));
    assert!(close(t.translation, DVec3::ZERO));
}

#[test]
fn a_stretch_of_only_this_one_makes_it_a_separate_part() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let (target, part, _) = drilled_board(&mut h);
    let other = h
        .apply(Operation::Duplicate {
            context: root,
            entities: vec![target.entity],
            transform: DAffine3::from_translation(DVec3::Y * 100.0),
        })
        .unwrap()
        .created[0];
    h.apply(stretch_x(h.document(), &target, 100.0, 50.0, Scope::This)).unwrap();
    let doc = h.document();
    let (mine, p, _) = doc.resolve_part(&target).unwrap();
    assert_ne!(mine, part);
    assert!(close(p.blank().vertex_bounds().unwrap().1, DVec3::new(250.0, 50.0, 18.0)));
    let (theirs, q, _) = doc.resolve_part(&EntityPath::root(other)).unwrap();
    assert_eq!(theirs, part);
    assert!(close(q.blank().vertex_bounds().unwrap().1, DVec3::new(200.0, 50.0, 18.0)));
}

#[test]
fn guides_follow_their_section() {
    let mut h = History::new(Document::new());
    let (target, part, _) = drilled_board(&mut h);
    let guide = |h: &mut History, shape| h.apply(Operation::AddGuide { component: part, shape }).unwrap().guide.unwrap();
    let across = guide(&mut h, GuideShape::Line { origin: DVec3::new(150.0, 0.0, 0.0), dir: DVec3::Y });
    let along = guide(&mut h, GuideShape::Line { origin: DVec3::new(0.0, 25.0, 0.0), dir: DVec3::X });
    let ring = guide(&mut h, GuideShape::Circle { center: DVec3::new(150.0, 25.0, 9.0), normal: DVec3::X, radius: 5.0 });
    let before = guide(&mut h, GuideShape::Line { origin: DVec3::new(50.0, 0.0, 0.0), dir: DVec3::Z });
    h.apply(stretch_x(h.document(), &target, 100.0, 50.0, Scope::All)).unwrap();
    let c = h.document().component(part).unwrap();
    let shape = |id| c.guide(id).unwrap().shape;
    assert_eq!(shape(across), GuideShape::Line { origin: DVec3::new(200.0, 0.0, 0.0), dir: DVec3::Y });
    assert_eq!(shape(along), GuideShape::Line { origin: DVec3::new(0.0, 25.0, 0.0), dir: DVec3::X });
    assert_eq!(shape(ring), GuideShape::Circle { center: DVec3::new(200.0, 25.0, 9.0), normal: DVec3::X, radius: 5.0 });
    assert_eq!(shape(before), GuideShape::Line { origin: DVec3::new(50.0, 0.0, 0.0), dir: DVec3::Z });
}

#[test]
fn a_stretch_that_cannot_apply_is_refused_when_prepared() {
    let mut h = History::new(Document::new());
    // A standing cylinder cut across its axis, compressed past its width: refused before any
    // kernel call (scaled, it would turn inside out).
    let rod = add(&mut h, Primitive::Cylinder { origin: DVec3::ZERO, radius: 10.0, height: 40.0 });
    let target = EntityPath::root(rod);
    let refused = |op: &Operation| {
        let e = h.document().prepare(op).unwrap_err();
        assert!(matches!(e, Error::Kernel(kernel::Error::Stretch(_))), "{e:?}");
    };
    refused(&stretch_x(h.document(), &target, 0.0, -25.0, Scope::All));
    // Malformed: a move per section missing; compressed past nothing.
    refused(&stretch(h.document(), &target, DVec3::Z, vec![20.0], vec![0.0], Scope::All));
    refused(&stretch(h.document(), &target, DVec3::Z, vec![20.0], vec![0.0, -45.0], Scope::All));
    assert!(h.apply(stretch_x(h.document(), &target, 0.0, -25.0, Scope::All)).is_err());
    // Unchanged.
    assert!((h.document().resolve_part(&target).unwrap().1.blank().bounds().unwrap().1.z - 40.0).abs() < 1e-9);
    // Along its axis it stretches.
    h.apply(stretch(h.document(), &target, DVec3::Z, vec![20.0], vec![0.0, 10.0], Scope::All)).unwrap();
    let (_, p, _) = h.document().resolve_part(&target).unwrap();
    assert!((p.blank().bounds().unwrap().1.z - 50.0).abs() < 1e-9);
}

/// A frame along X at the root, made an assembly: two rails 0–600 (one part, the second a copy
/// of the first 300 further along Y) and two legs 0–50 and 550–600 (one part). Returns the
/// history, the assembly's occurrence, then the rails' and the legs' occurrences in it.
fn frame(rail_shift: f64) -> (History, Id, [Id; 2], [Id; 2]) {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let rail = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 20.0, 40.0) });
    let leg = add(&mut h, Primitive::Box { origin: DVec3::new(0.0, 0.0, -400.0), size: DVec3::new(50.0, 50.0, 400.0) });
    let copy = |h: &mut History, id: Id, by: DVec3| {
        h.apply(Operation::Duplicate { context: root, entities: vec![id], transform: DAffine3::from_translation(by) }).unwrap().created[0]
    };
    let rail2 = copy(&mut h, rail, DVec3::new(rail_shift, 300.0, 0.0));
    let leg2 = copy(&mut h, leg, DVec3::X * 550.0);
    let a = h.apply(Operation::MakeAssembly { context: root, entities: vec![rail, rail2, leg, leg2], name: None }).unwrap().created[0];
    (h, a, [rail, rail2], [leg, leg2])
}

/// The X extent of an occurrence of the assembly `a` (at the root), and its part.
fn x_of(doc: &Document, a: Id, o: Id) -> ((f64, f64), Id) {
    let path = EntityPath::new(vec![a], o);
    let (lo, hi) = world_bounds(doc, &path);
    ((lo.x, hi.x), doc.resolve(&path).unwrap().component)
}

#[test]
fn parts_cut_at_different_places_between_the_same_pieces_keep_one_part() {
    // A plain rail, its copy 100 further along X: cut at 300, one is cut 300 into its length,
    // the other 200 — between the same pieces (its ends), the same stretch: one part.
    let (mut h, a, rails, _) = frame(100.0);
    let rail_part = x_of(h.document(), a, rails[0]).1;
    h.apply(stretch_x(h.document(), &EntityPath::root(a), 300.0, 100.0, Scope::All)).unwrap();
    let doc = h.document();
    assert_eq!((x_of(doc, a, rails[0]), x_of(doc, a, rails[1])), (((0.0, 700.0), rail_part), ((100.0, 800.0), rail_part)));
}

#[test]
fn an_assembly_stretched_moves_what_is_in_one_section_and_stretches_what_crosses() {
    let (mut h, a, rails, legs) = frame(0.0);
    let (rail_part, leg_part) = (x_of(h.document(), a, rails[0]).1, x_of(h.document(), a, legs[0]).1);
    let target = EntityPath::root(a);
    // The spans a compression keeps apart: the matter at each end.
    let spans = h.document().stretch_spans(&[], &[a], &DAffine3::IDENTITY, DVec3::X, &[300.0]).unwrap();
    assert_eq!(spans, vec![Some((0.0, 50.0)), Some((550.0, 600.0))]);
    let (lo, hi) = h.document().stretch_bounds(&[], &[a], &DAffine3::IDENTITY).unwrap().unwrap();
    assert!(close(lo, DVec3::new(0.0, 0.0, -400.0)) && close(hi, DVec3::new(600.0, 320.0, 40.0)), "{lo} {hi}");

    let out = h.apply(stretch_x(h.document(), &target, 300.0, 100.0, Scope::All)).unwrap();
    let doc = h.document();
    // The rails, one part still, stretched once; the far leg moved, its part untouched.
    assert_eq!(x_of(doc, a, rails[0]), ((0.0, 700.0), rail_part));
    assert_eq!(x_of(doc, a, rails[1]), ((0.0, 700.0), rail_part));
    assert_eq!(x_of(doc, a, legs[0]), ((0.0, 50.0), leg_part));
    assert_eq!(x_of(doc, a, legs[1]), ((650.0, 700.0), leg_part));
    assert_eq!(out.stretched.iter().map(|s| (s.part, s.from)).collect::<Vec<_>>(), vec![(rail_part, rail_part)]);
    assert!((doc.part(leg_part).unwrap().blank().bounds().unwrap().1.x - 50.0).abs() < 1e-9);
}

#[test]
fn occurrences_of_one_part_stretched_differently_become_separate_parts() {
    // The second rail 100 further along X, the rail drilled 250 into its length: cut at 300,
    // the first rail has its hole before the cutter, the second after it.
    let (mut h, a, rails, _) = frame(100.0);
    let rail_part = x_of(h.document(), a, rails[0]).1;
    let hole = Primitive::Cylinder { origin: DVec3::new(250.0, 10.0, -10.0), radius: 5.0, height: 60.0 };
    let target = EntityPath::new(vec![a], rails[0]);
    h.apply(Operation::AddBody { target, primitive: hole, placement: DAffine3::IDENTITY, sign: Sign::Negative, scope: Scope::All })
        .unwrap();
    h.apply(stretch_x(h.document(), &EntityPath::root(a), 300.0, 100.0, Scope::All)).unwrap();
    let doc = h.document();
    let ((r0, p0), (r1, p1)) = (x_of(doc, a, rails[0]), x_of(doc, a, rails[1]));
    assert_eq!((r0, r1), ((0.0, 700.0), (100.0, 800.0)));
    // The first met keeps the part; the other is a copy, its ids kept.
    assert_eq!(p0, rail_part);
    assert_ne!(p1, rail_part);
    assert_eq!(doc.part(p1).unwrap().blank().bounds().unwrap().1.x, 700.0);
}

#[test]
fn a_part_only_moved_in_the_stretched_tree_keeps_its_shape() {
    // Cut along Y at 310: only the second rail crosses the cutter, the first one and the legs
    // are in one section. Even "all occurrences", the first rail must not take the change of
    // the second (OCL counts moved instances among the stretched ones: the split is made unique).
    let (mut h, a, rails, legs) = frame(0.0);
    let rail_part = x_of(h.document(), a, rails[0]).1;
    let leg_part = x_of(h.document(), a, legs[0]).1;
    // A third rail over the second, cut too: two stretched alike, one moved — still kept apart.
    let ac = h.document().resolve(&EntityPath::root(a)).unwrap().component;
    let rail3 = h
        .apply(Operation::Duplicate { context: ac, entities: vec![rails[1]], transform: DAffine3::from_translation(DVec3::Z * 100.0) })
        .unwrap()
        .created[0];
    h.apply(stretch(h.document(), &EntityPath::root(a), DVec3::Y, vec![310.0], vec![0.0, 100.0], Scope::All)).unwrap();
    let doc = h.document();
    let y_of = |o: Id| {
        let path = EntityPath::new(vec![a], o);
        let (lo, hi) = world_bounds(doc, &path);
        ((lo.y, hi.y), doc.resolve(&path).unwrap().component)
    };
    let ((y0, p0), (y1, p1), (y3, p3)) = (y_of(rails[0]), y_of(rails[1]), y_of(rail3));
    assert_eq!((y0, y1, y3), ((0.0, 20.0), (300.0, 420.0), (300.0, 420.0)));
    assert_eq!(p0, rail_part);
    assert_ne!(p1, rail_part);
    assert_eq!(p1, p3);
    assert_eq!(doc.part(rail_part).unwrap().blank().bounds().unwrap().1.y, 20.0);
    assert_eq!(y_of(legs[0]), ((0.0, 50.0), leg_part));
}

#[test]
fn an_assembly_shown_twice_is_stretched_everywhere_or_only_there() {
    for scope in [Scope::All, Scope::This] {
        let (mut h, a, rails, legs) = frame(0.0);
        let root = h.document().root();
        let b = h
            .apply(Operation::Duplicate { context: root, entities: vec![a], transform: DAffine3::from_translation(DVec3::Y * 1000.0) })
            .unwrap()
            .created[0];
        // The rail's part shown outside the assembly too.
        let rail_part = x_of(h.document(), a, rails[0]).1;
        let alone =
            h.apply(Operation::PlaceOccurrence { context: root, component: rail_part, transform: DAffine3::IDENTITY }).unwrap().created[0];
        h.apply(stretch_x(h.document(), &EntityPath::root(a), 300.0, 100.0, scope)).unwrap();
        let doc = h.document();
        let alone_x = world_bounds(doc, &EntityPath::root(alone)).1.x;
        let (b_rail, b_leg) = (x_of(doc, b, rails[0]).0, x_of(doc, b, legs[1]).0);
        assert_eq!(x_of(doc, a, rails[1]).0, (0.0, 700.0), "{scope:?}");
        assert_eq!(x_of(doc, a, legs[1]).0, (650.0, 700.0), "{scope:?}");
        match scope {
            // Everywhere: the other assembly, the rail alone.
            Scope::All => assert_eq!((b_rail, b_leg, alone_x), ((0.0, 700.0), (650.0, 700.0), 700.0)),
            // Only there: the assembly and its rail are its own; the rest as it was.
            Scope::This => {
                assert_eq!((b_rail, b_leg, alone_x), ((0.0, 600.0), (550.0, 600.0), 600.0));
                assert_ne!(doc.resolve(&EntityPath::root(a)).unwrap().component, doc.resolve(&EntityPath::root(b)).unwrap().component);
                assert_ne!(x_of(doc, a, rails[0]).1, rail_part);
                assert_eq!(x_of(doc, a, rails[0]).1, x_of(doc, a, rails[1]).1);
            }
        }
    }
}

#[test]
fn a_part_of_an_assembly_that_cannot_stretch_is_named() {
    let (mut h, a, _, _) = frame(0.0);
    // A rod in the assembly across the cutter, square to the axis, compressed past its width: it
    // cannot be (scaled, it would turn inside out).
    let asm = h.document().resolve(&EntityPath::root(a)).unwrap().component;
    let cyl = Primitive::Cylinder { origin: DVec3::new(300.0, 150.0, 0.0), radius: 10.0, height: 40.0 };
    h.apply(Operation::AddPrimitive { context: asm, primitive: cyl, placement: DAffine3::IDENTITY, material: None }).unwrap();
    let e = h.document().prepare(&stretch_x(h.document(), &EntityPath::root(a), 300.0, -25.0, Scope::All)).unwrap_err();
    assert!(matches!(&e, Error::InPart { error, .. } if matches!(**error, Error::Kernel(kernel::Error::Stretch(_)))), "{e:?}");
}

#[test]
fn a_copy_turned_half_round_stretched_alike_keeps_its_part() {
    for (turn, mirror) in
        [(DAffine3::from_rotation_z(std::f64::consts::PI), false), (DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)), true)]
    {
        let mut h = History::new(Document::new());
        let root = h.document().root();
        let rail = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 20.0, 40.0) });
        // The copy turned (or mirrored) about its far end: it spans 0–600 along X too, its own
        // X running the other way.
        let t = DAffine3::from_translation(DVec3::new(600.0, if mirror { 300.0 } else { 320.0 }, 0.0)) * turn;
        let rail2 = h.apply(Operation::Duplicate { context: root, entities: vec![rail], transform: t }).unwrap().created[0];
        let a = h.apply(Operation::MakeAssembly { context: root, entities: vec![rail, rail2], name: None }).unwrap().created[0];
        let part = x_of(h.document(), a, rail).1;
        assert_eq!(x_of(h.document(), a, rail2).0, (0.0, 600.0));
        // Cut off-centre, its far end pulled 100: both stretched alike, one part still, the
        // turned one moved along its own axis to show it.
        h.apply(stretch_x(h.document(), &EntityPath::root(a), 200.0, 100.0, Scope::All)).unwrap();
        let doc = h.document();
        let ((r0, p0), (r1, p1)) = (x_of(doc, a, rail), x_of(doc, a, rail2));
        assert_eq!((r0, r1), ((0.0, 700.0), (0.0, 700.0)), "mirror {mirror}");
        assert_eq!((p0, p1), (part, part), "mirror {mirror}");
    }
}

/// Three boards of one part at the root, 0–200 along X, 100 apart along Y.
fn three_boards() -> (History, Id, [Id; 3]) {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let copy = |h: &mut History, by: f64| {
        h.apply(Operation::Duplicate { context: root, entities: vec![board], transform: DAffine3::from_translation(DVec3::Y * by) })
            .unwrap()
            .created[0]
    };
    let (b2, b3) = (copy(&mut h, 100.0), copy(&mut h, 300.0));
    let part = h.document().resolve(&EntityPath::root(board)).unwrap().component;
    (h, part, [board, b2, b3])
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

/// The world X extent of the root occurrence `o`, and its part.
fn root_x(doc: &Document, o: Id) -> ((f64, f64), Id) {
    let path = EntityPath::root(o);
    let (lo, hi) = world_bounds(doc, &path);
    ((lo.x, hi.x), doc.resolve(&path).unwrap().component)
}

#[test]
fn several_occurrences_stretched_together_share_their_change() {
    let (mut h, part, [b1, b2, b3]) = three_boards();
    // Their common box, in the root's axes.
    let (lo, hi) = h.document().stretch_bounds(&[], &[b1, b2], &DAffine3::IDENTITY).unwrap().unwrap();
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(200.0, 150.0, 18.0)), "{lo} {hi}");
    let op = |scope| Operation::Stretch {
        context: vec![],
        occurrences: vec![b1, b2],
        frame: DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![100.0],
        moves: vec![0.0, 50.0],
        scope,
        cutters: None,
    };

    // Only these: one copy for both, the third as it was.
    let out = h.apply(op(Scope::This)).unwrap();
    let doc = h.document();
    let ((x1, p1), (x2, p2), (x3, p3)) = (root_x(doc, b1), root_x(doc, b2), root_x(doc, b3));
    assert_eq!((x1, x2, x3), ((0.0, 250.0), (0.0, 250.0), (0.0, 200.0)));
    assert_eq!((p1 == p2, p1 != part, p3), (true, true, part));
    assert_eq!(out.stretched.iter().map(|s| (s.part, s.from)).collect::<Vec<_>>(), vec![(p1, part)]);

    // All: the part itself, the third showing it.
    h.undo();
    h.apply(op(Scope::All)).unwrap();
    let doc = h.document();
    assert_eq!((root_x(doc, b1), root_x(doc, b2), root_x(doc, b3)), (((0.0, 250.0), part), ((0.0, 250.0), part), ((0.0, 250.0), part)));
}

#[test]
fn several_occurrences_in_the_context_axes_turned_ones_take_their_own_change() {
    let (mut h, part, [b1, b2, _]) = three_boards();
    let root = h.document().root();
    // The second turned square, standing at 100–150 along X, 0–200 along Y.
    let turn = DAffine3::from_translation(DVec3::new(250.0, 0.0, 0.0)) * DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    h.apply(Operation::Transform { context: root, entities: vec![b2], transform: turn }).unwrap();
    let (lo, hi) = world_bounds(h.document(), &EntityPath::root(b2));
    assert!(close(lo, DVec3::new(100.0, 0.0, 0.0)) && close(hi, DVec3::new(150.0, 200.0, 18.0)), "{lo} {hi}");
    let op = |cut: f64| Operation::Stretch {
        context: vec![],
        occurrences: vec![b1, b2],
        frame: DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![cut],
        moves: vec![0.0, 50.0],
        scope: Scope::All,
        cutters: None,
    };

    // Cut at 50: the first board stretched along its length, the turned one only moved — it
    // keeps the part and its shape, the stretched one goes to a copy.
    h.apply(op(50.0)).unwrap();
    let doc = h.document();
    let ((x1, p1), (x2, p2)) = (root_x(doc, b1), root_x(doc, b2));
    assert!(near(x1, (0.0, 250.0)) && near(x2, (150.0, 200.0)), "{x1:?} {x2:?}");
    assert_eq!((p1 != part, p2), (true, part));
    let (lo, hi) = world_bounds(doc, &EntityPath::root(b2));
    assert!(close(lo, DVec3::new(150.0, 0.0, 0.0)) && close(hi, DVec3::new(200.0, 200.0, 18.0)), "{lo} {hi}");
    h.undo();

    // Cut at 125: both cross it, the turned one across its width — a change of its own.
    h.apply(op(125.0)).unwrap();
    let doc = h.document();
    let ((x1, p1), (x2, p2)) = (root_x(doc, b1), root_x(doc, b2));
    assert!(near(x1, (0.0, 250.0)) && near(x2, (100.0, 200.0)), "{x1:?} {x2:?}");
    assert_eq!(p1, part);
    assert_ne!(p2, part);
    let (lo, hi) = world_bounds(doc, &EntityPath::root(b2));
    assert!(close(lo, DVec3::new(100.0, 0.0, 0.0)) && close(hi, DVec3::new(200.0, 200.0, 18.0)), "{lo} {hi}");
}

#[test]
fn a_stretch_in_the_model_axes_reaches_into_a_turned_context() {
    // A board in an assembly turned square and moved: stretched along the model's X, from
    // inside the assembly, it grows across its width.
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let a = h.apply(Operation::MakeAssembly { context: root, entities: vec![board], name: None }).unwrap().created[0];
    let turn = DAffine3::from_translation(DVec3::new(1000.0, 0.0, 0.0)) * DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    h.apply(Operation::Transform { context: root, entities: vec![a], transform: turn }).unwrap();
    let path = EntityPath::new(vec![a], board);
    let (lo, hi) = world_bounds(h.document(), &path);
    assert!(close(lo, DVec3::new(950.0, 0.0, 0.0)) && close(hi, DVec3::new(1000.0, 200.0, 18.0)), "{lo} {hi}");

    // The model's axes in the context's.
    let frame = h.document().transform_of(&[a]).unwrap().inverse();
    let (blo, bhi) = h.document().stretch_bounds(&[a], &[board], &frame).unwrap().unwrap();
    assert!(close(blo, lo) && close(bhi, hi), "{blo} {bhi}");
    let op = Operation::Stretch {
        context: vec![a],
        occurrences: vec![board],
        frame,
        axis: DVec3::X,
        cuts: vec![975.0],
        moves: vec![0.0, 30.0],
        scope: Scope::All,
        cutters: None,
    };
    h.apply(op).unwrap();
    let (lo, hi) = world_bounds(h.document(), &path);
    assert!(close(lo, DVec3::new(950.0, 0.0, 0.0)) && close(hi, DVec3::new(1030.0, 200.0, 18.0)), "{lo} {hi}");
}

#[test]
fn a_stretch_without_occurrences_is_refused() {
    let (h, _, _) = three_boards();
    let op = Operation::Stretch {
        context: vec![],
        occurrences: vec![],
        frame: DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![100.0],
        moves: vec![0.0, 50.0],
        scope: Scope::All,
        cutters: None,
    };
    assert!(matches!(h.document().prepare(&op), Err(Error::Invalid(Refusal::NothingStretched))));
}

#[test]
fn a_behavior_is_set_merged_saved_copied_and_checked() {
    let (mut h, part, [b1, ..]) = three_boards();
    let root = h.document().root();
    let behavior = |mask: u8, x: Vec<f64>, y: Vec<f64>| Behavior { no_stretch: StretchMask(mask), cutters: Some([x, y, vec![0.5]]) };
    let set = |b: Behavior| Operation::SetBehavior { component: part, behavior: b };
    // Two changes in a row to one component: one undo step.
    h.apply(set(behavior(0, vec![0.5], vec![0.5]))).unwrap();
    // Y locked: its cutters dropped, the mask the one truth.
    h.apply(set(behavior(2, vec![0.25, 0.75], vec![0.3]))).unwrap();
    let saved = &h.document().component(part).unwrap().behavior;
    assert_eq!(*saved, behavior(2, vec![0.25, 0.75], vec![]));
    assert!(saved.no_stretch.locks(1) && !saved.no_stretch.locks(0));
    assert!(saved.no_stretch.locks_along(DVec3::new(0.6, 0.8, 0.0)) && !saved.no_stretch.locks_along(DVec3::X));
    assert!(h.undo());
    assert!(h.document().component(part).unwrap().behavior.is_default());
    assert!(h.redo());

    // Saved and read back.
    let json = h.document().to_json().unwrap();
    assert_eq!(Document::from_json(&json).unwrap(), *h.document());
    // A file with cutters out of ]0, 1[ is refused.
    let bad = json.replacen("[0.25,0.75]", "[0.25,1.75]", 1);
    assert_ne!(bad, json);
    assert!(matches!(Document::from_json(&bad), Err(Error::File(FileError::BadBehavior(id))) if id == part));
    // So is such an operation.
    assert!(matches!(h.document().prepare(&set(behavior(0, vec![0.6, 0.4], vec![0.5]))), Err(Error::Invalid(Refusal::BadBehavior))));
    assert!(matches!(h.document().prepare(&set(behavior(128, vec![0.5], vec![0.5]))), Err(Error::Invalid(Refusal::BadBehavior))));

    // A separate part keeps it.
    h.apply(Operation::MakeUnique { context: root, occurrence: b1 }).unwrap();
    let copy = occ(h.document(), &EntityPath::root(b1)).component;
    assert_ne!(copy, part);
    assert_eq!(h.document().component(copy).unwrap().behavior, behavior(2, vec![0.25, 0.75], vec![]));
}

#[test]
fn a_stretch_sets_its_targets_cutters() {
    let (mut h, part, [b1, ..]) = three_boards();
    h.apply(Operation::SetBehavior { component: part, behavior: Behavior { no_stretch: StretchMask(4), cutters: None } }).unwrap();
    let cutters = [vec![0.4], vec![0.5], vec![0.5]];
    let op = match stretch_x(h.document(), &EntityPath::root(b1), 100.0, 50.0, Scope::This) {
        Operation::Stretch { context, occurrences, frame, axis, cuts, moves, scope, .. } => {
            Operation::Stretch { context, occurrences, frame, axis, cuts, moves, scope, cutters: Some(Box::new(cutters.clone())) }
        }
        _ => unreachable!(),
    };
    h.apply(op).unwrap();
    // On the copy the stretch made, its lock kept (and its cutters dropped); the others' part
    // as it was.
    let copy = occ(h.document(), &EntityPath::root(b1)).component;
    assert_ne!(copy, part);
    let b = &h.document().component(copy).unwrap().behavior;
    assert_eq!(*b, Behavior { no_stretch: StretchMask(4), cutters: Some([vec![0.4], vec![0.5], vec![]]) });
    assert_eq!(h.document().component(part).unwrap().behavior.cutters, None);
}

#[test]
fn a_locked_part_is_a_block_moved_with_its_origin() {
    // A board 0–200 along X and a hinge across the cutter at 100, in an assembly.
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let hinge = add(&mut h, cube(DVec3::new(90.0, 0.0, 18.0), 20.0));
    let a = h.apply(Operation::MakeAssembly { context: root, entities: vec![board, hinge], name: None }).unwrap().created[0];
    let path = EntityPath::new(vec![a], hinge);
    let hinge_part = occ(h.document(), &path).component;
    let origin = occ(h.document(), &path).transform.translation.x;
    assert!(origin < 100.0, "the hinge's origin at {origin}");
    let frame = h.document().resolve(&EntityPath::root(a)).unwrap().transform;
    let op = || Operation::Stretch {
        context: vec![],
        occurrences: vec![a],
        frame,
        axis: DVec3::X,
        cuts: vec![100.0],
        moves: vec![0.0, 50.0],
        scope: Scope::All,
        cutters: None,
    };
    // Free, the hinge is stretched.
    let out = h.apply(op()).unwrap();
    assert!(out.stretched.iter().any(|s| s.from == hinge_part));
    let (lo, hi) = world_bounds(h.document(), &path);
    assert!((lo.x - 90.0).abs() < 1e-9 && (hi.x - 160.0).abs() < 1e-9, "{lo} {hi}");
    h.undo();

    // Locked along X (all axes, as OCL's hardware): moved with its origin's section — here
    // not at all — and not stretched; the board is.
    let all = Behavior { no_stretch: StretchMask(StretchMask::ALL), cutters: None };
    h.apply(Operation::SetBehavior { component: hinge_part, behavior: all }).unwrap();
    // The compression limit counts it whole in its origin's section.
    let spans = h.document().stretch_spans(&[], &[a], &frame, DVec3::X, &[100.0]).unwrap();
    assert!(spans[0].is_some_and(|(_, hi)| (hi - 110.0).abs() < 1e-9), "{spans:?}");
    let out = h.apply(op()).unwrap();
    assert!(!out.stretched.iter().any(|s| s.from == hinge_part));
    assert_eq!(occ(h.document(), &path).component, hinge_part);
    let (lo, hi) = world_bounds(h.document(), &path);
    assert!((lo.x - 90.0).abs() < 1e-9 && (hi.x - 110.0).abs() < 1e-9, "{lo} {hi}");
    let (lo, hi) = world_bounds(h.document(), &EntityPath::new(vec![a], board));
    assert!((lo.x).abs() < 1e-9 && (hi.x - 250.0).abs() < 1e-9, "{lo} {hi}");
    h.undo();

    // Locked along Y only: stretched along X as before.
    let y = Behavior { no_stretch: StretchMask(2), cutters: None };
    h.apply(Operation::SetBehavior { component: hinge_part, behavior: y }).unwrap();
    let out = h.apply(op()).unwrap();
    assert!(out.stretched.iter().any(|s| s.from == hinge_part));
}

/// What `placed_changes` should find, the long way: every placement compared by path.
fn placed_changes_by_path(after: &Document, before: &Document, touched: &[Id]) -> Vec<(EntityPath, Id, Id)> {
    let was: std::collections::HashMap<EntityPath, PlacedPart> = before.placed_parts().into_iter().map(|p| (p.path.clone(), p)).collect();
    let mut out: Vec<(EntityPath, Id, Id)> = after
        .placed_parts()
        .into_iter()
        .filter_map(|p| {
            let b = was.get(&p.path)?;
            let changed = p.part != b.part
                || !Arc::ptr_eq(&p.shape, &b.shape)
                || !p.transform.abs_diff_eq(b.transform, 1e-9)
                || touched.contains(&p.part);
            changed.then_some((p.path, p.part, b.part))
        })
        .collect();
    out.sort_by_key(|(p, _, _)| format!("{p:?}"));
    out
}

fn placed_changes_found(after: &Document, before: &Document, touched: &[Id]) -> Vec<(EntityPath, Id, Id)> {
    let mut out: Vec<(EntityPath, Id, Id)> =
        after.placed_changes(before, touched).into_iter().map(|(p, was, _)| (p.path, p.part, was)).collect();
    out.sort_by_key(|(p, _, _)| format!("{p:?}"));
    out
}

#[test]
fn placed_changes_skip_only_what_is_shared() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 100.0));
    let b = add(&mut h, cube(DVec3::X * 300.0, 100.0));
    let root = h.document().root();
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a], name: None }).unwrap().created[0];
    h.apply(Operation::Duplicate { context: root, entities: vec![asm], transform: DAffine3::from_translation(DVec3::Y * 300.0) }).unwrap();
    let before = h.document().clone();
    let component = before.resolve(&EntityPath::root(asm)).unwrap().component;
    let inner = before.assembly(component).unwrap().occurrences()[0].id;
    let part_b = before.resolve(&EntityPath::root(b)).unwrap().component;

    // Nothing changed: nothing found; a part named touched: each of its placements.
    assert!(before.placed_changes(&before, &[]).is_empty());
    assert_eq!(placed_changes_found(&before, &before, &[part_b]), placed_changes_by_path(&before, &before, &[part_b]));
    assert_eq!(placed_changes_found(&before, &before, &[part_b]).len(), 1);

    // The part inside the assembly stretched for all: both copies of the assembly — the
    // assembly itself the same, holding a changed part.
    let stretch = |scope| Operation::Stretch {
        context: vec![asm],
        occurrences: vec![inner],
        frame: DAffine3::IDENTITY,
        axis: DVec3::Z,
        cuts: vec![50.0],
        moves: vec![0.0, 40.0],
        scope,
        cutters: None,
    };
    for scope in [Scope::All, Scope::This] {
        h.apply(stretch(scope)).unwrap();
        let after = h.document();
        let found = placed_changes_found(after, &before, &[]);
        assert_eq!(found, placed_changes_by_path(after, &before, &[]), "{scope:?}");
        assert_eq!(found.len(), if scope == Scope::All { 2 } else { 1 }, "{scope:?}");
        h.undo();
    }

    // B moved: only it.
    h.apply(Operation::Transform { context: root, entities: vec![b], transform: DAffine3::from_translation(DVec3::Z) }).unwrap();
    let found = placed_changes_found(h.document(), &before, &[]);
    assert_eq!(found, placed_changes_by_path(h.document(), &before, &[]));
    assert_eq!(found.len(), 1);
}

/// An occurrence of `component`, for the assembly's own tests.
fn bare_occurrence(id: u64, component: Id) -> Occurrence {
    Occurrence { id: Id(id), name: String::new(), component, transform: DAffine3::IDENTITY, hidden: false, props: Props::default() }
}

#[test]
fn an_assembly_finds_its_occurrences_as_a_scan_does() {
    let scan = |a: &Assembly, id: Id| a.occurrences().iter().position(|o| o.id == id);
    let check = |a: &Assembly, what: &str| {
        // Every id there, some that are not; each asked often enough to build the index.
        for _ in 0..2 {
            for id in (0..400).map(Id) {
                assert_eq!(a.position(id), scan(a, id), "{what}: {id:?}");
                assert_eq!(a.occurrence(id).map(|o| o.id), scan(a, id).map(|_| id), "{what}");
            }
        }
    };
    let part = Id(10_000);
    let mut a = Assembly::new((0..200).map(|i| bare_occurrence(i * 2, part)).collect());
    check(&a, "built");
    a.occurrences_mut().retain(|o| o.id.0 % 3 != 0);
    check(&a, "some removed");
    a.occurrences_mut().insert(5, bare_occurrence(301, part));
    a.occurrences_mut().push(bare_occurrence(303, part));
    a.occurrences_mut().reverse();
    check(&a, "added, reordered");
    // A clone keeps the index; changing it leaves the original as it was.
    let mut b = a.clone();
    check(&b, "clone");
    b.occurrences_mut().truncate(10);
    check(&b, "clone cut");
    check(&a, "original");
    // A few: scanned, never indexed.
    let small = Assembly::new((0..5).map(|i| bare_occurrence(i, part)).collect());
    check(&small, "small");
}

#[test]
fn placements_are_counted_over_the_distinct_assemblies() {
    // The counts every path gives, one by one.
    fn by_path(doc: &Document, component: Id, placed: &mut std::collections::HashMap<Id, usize>) {
        let Ok(a) = doc.assembly(component) else { return };
        for o in a.occurrences() {
            *placed.entry(o.component).or_default() += 1;
            by_path(doc, o.component, placed);
        }
    }
    // Parts in runs and apart, an assembly copied three times holding another twice.
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::X * 20.0, 10.0));
    let step = DAffine3::from_translation(DVec3::Y * 20.0);
    h.apply(Operation::Array { context: root, entities: vec![a], step, count: 4 }).unwrap();
    let c = add(&mut h, cube(DVec3::X * 40.0, 10.0));
    h.apply(Operation::Array { context: root, entities: vec![b], step, count: 2 }).unwrap();
    let inner = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, c], name: None }).unwrap().created[0];
    h.apply(Operation::Duplicate { context: root, entities: vec![inner], transform: step }).unwrap();
    let outer = h.apply(Operation::MakeAssembly { context: root, entities: vec![inner, b], name: None }).unwrap().created[0];
    h.apply(Operation::Array { context: root, entities: vec![outer], step: DAffine3::from_translation(DVec3::Z * 50.0), count: 2 })
        .unwrap();
    let doc = h.document();
    let (mut fast, mut slow) = (crate::FastMap::default(), std::collections::HashMap::new());
    doc.count_placed(doc.root(), &mut fast);
    by_path(doc, doc.root(), &mut slow);
    assert_eq!(fast.into_iter().collect::<std::collections::HashMap<_, _>>(), slow);
    assert!(slow.values().any(|n| *n >= 6), "{slow:?}");
}

// ---- Blank profiles (docs/tools/prism.md, step 3) --------------------------------------

fn profile_of(doc: &Document, path: &EntityPath) -> Option<BodyProfile> {
    let p = doc.resolve_part(path).unwrap().1;
    p.profiled().and_then(|b| p.profile_of(b).cloned())
}

#[test]
fn a_turned_body_keeps_its_half_profile() {
    use skraft_kernel::profile::polygon;
    let v = glam::DVec2::new;
    let mut h = History::new(Document::new());
    let root = h.document().root();
    // A leg Ø 40 × 700 lying along X: turned about the body's Z, placed with its Z along X.
    let half = polygon(&[v(0.0, 10.0), v(20.0, 10.0), v(20.0, 710.0), v(0.0, 710.0)]);
    let lying = DAffine3::from_translation(DVec3::new(0.0, 100.0, 50.0)) * DAffine3::from_rotation_y(std::f64::consts::FRAC_PI_2);
    let leg = h
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Revolution { origin: DVec3::ZERO, profile: half },
            placement: lying,
            material: None,
        })
        .unwrap()
        .created[0];
    let path = EntityPath::root(leg);
    // Its profile stands on its lowest height, turned.
    let b = profile_of(h.document(), &path).unwrap();
    assert!(b.is_turned() && b.extrusion().is_none());
    assert_eq!(skraft_kernel::profile::bounds(&b.profile), Some((v(0.0, 0.0), v(20.0, 700.0))));
    let world = |h: &History| {
        let (_, part, t) = h.document().resolve_part(&path).unwrap();
        let (lo, hi) = part.shape().bounds().unwrap();
        let (a, b) = (t.transform_point3(lo), t.transform_point3(hi));
        (a.min(b), a.max(b))
    };
    let (lo, hi) = world(&h);
    assert!((lo - DVec3::new(10.0, 80.0, 30.0)).length() < 1e-6 && (hi - DVec3::new(710.0, 120.0, 70.0)).length() < 1e-6, "{lo} {hi}");

    // Its profile set: thinner; across its axis, refused.
    let body = h.document().resolve_part(&path).unwrap().1.profiled().unwrap().id;
    let set = |profile| Operation::SetProfile { target: path.clone(), body, profile: BodyProfile::turned(profile), scope: Scope::All };
    h.apply(set(polygon(&[v(0.0, 0.0), v(15.0, 0.0), v(15.0, 700.0), v(0.0, 700.0)]))).unwrap();
    let (lo, hi) = world(&h);
    assert!((lo.y - 85.0).abs() < 1e-6 && (hi.y - 115.0).abs() < 1e-6, "{lo} {hi}");
    assert!(h.apply(set(polygon(&[v(-5.0, 0.0), v(15.0, 0.0), v(15.0, 700.0), v(-5.0, 700.0)]))).is_err());

    // Stretched along its axis (the part's Z, the world's X): the half profile follows, still
    // turned.
    h.apply(stretch(h.document(), &path, DVec3::Z, vec![300.0], vec![0.0, 100.0], Scope::All)).unwrap();
    let b = profile_of(h.document(), &path).unwrap();
    assert!(b.is_turned());
    assert_eq!(skraft_kernel::profile::bounds(&b.profile).unwrap().1, v(15.0, 800.0));
    assert!((world(&h).1.x - 810.0).abs() < 1e-6, "{:?}", world(&h));
    // Across its axis (the part's X), cut through: refused.
    let e = h.apply(stretch(h.document(), &path, DVec3::X, vec![0.0], vec![0.0, 10.0], Scope::All)).unwrap_err();
    assert!(matches!(e, Error::Invalid(Refusal::TurnedAcross)), "{e:?}");

    // Saved and read back.
    let json = h.document().to_json().unwrap();
    assert!(json.contains("\"revolution\":\"whole\""));
    assert_eq!(Document::from_json(&json).unwrap(), *h.document());
}

#[test]
fn a_profile_is_kept_set_and_stretched() {
    use skraft_kernel::profile::{Corner, polygon};
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::new(100.0, 0.0, 0.0), size: DVec3::new(600.0, 300.0, 18.0) });
    let path = EntityPath::root(board);
    // The box's rectangle, at the part's origin (its lowest corner).
    let b = profile_of(h.document(), &path).unwrap();
    assert_eq!(skraft_kernel::profile::bounds(&b.profile), Some((glam::DVec2::ZERO, glam::DVec2::new(600.0, 300.0))));
    assert_eq!(b.extrusion(), Some((0.0, 18.0)));

    // A copy of it, then the profile set for all: an L, thicker.
    h.apply(Operation::Duplicate { context: root, entities: vec![board], transform: DAffine3::from_translation(DVec3::Y * 500.0) })
        .unwrap();
    let copy = h.document().assembly(root).unwrap().occurrences().last().unwrap().id;
    let l = polygon(
        &[(0.0, 0.0), (600.0, 0.0), (600.0, 100.0), (100.0, 100.0), (100.0, 300.0), (0.0, 300.0)].map(|(x, y)| glam::DVec2::new(x, y)),
    );
    let body = h.document().resolve_part(&path).unwrap().1.profiled().unwrap().id;
    let set = |profile: Vec<Corner>, height: f64, scope| Operation::SetProfile {
        target: path.clone(),
        body,
        profile: BodyProfile::extruded(profile, 0.0, height),
        scope,
    };
    h.apply(set(l.clone(), 19.0, Scope::All)).unwrap();
    let (_, part, _) = h.document().resolve_part(&path).unwrap();
    assert_eq!(part.shape().topology_counts(), (8, 18, 12));
    assert!(close(world_bounds(h.document(), &EntityPath::root(copy)).1, DVec3::new(700.0, 800.0, 19.0)), "the copy follows");

    // Only this one: a separate part; the copy keeps the L.
    let square = polygon(&[(0.0, 0.0), (50.0, 0.0), (50.0, 50.0), (0.0, 50.0)].map(|(x, y)| glam::DVec2::new(x, y)));
    h.apply(set(square, 10.0, Scope::This)).unwrap();
    assert_ne!(occ(h.document(), &path).component, occ(h.document(), &EntityPath::root(copy)).component);
    assert_eq!(profile_of(h.document(), &EntityPath::root(copy)).unwrap().profile.len(), 6);
    assert!(close(world_bounds(h.document(), &path).1, DVec3::new(150.0, 50.0, 10.0)));

    // Refused: crossing itself.
    let bow = polygon(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)].map(|(x, y)| glam::DVec2::new(x, y)));
    assert!(h.apply(set(bow, 10.0, Scope::All)).is_err());

    // Stretched along X across the L: the corners beyond move, the profile kept.
    let copy_path = EntityPath::root(copy);
    h.apply(stretch(h.document(), &copy_path, DVec3::X, vec![300.0], vec![0.0, 100.0], Scope::All)).unwrap();
    let b = profile_of(h.document(), &copy_path).unwrap();
    assert_eq!(skraft_kernel::profile::bounds(&b.profile).unwrap().1, glam::DVec2::new(700.0, 300.0));

    // A free part (a sphere) has no profile to set.
    let a = add(&mut h, Primitive::Sphere { center: DVec3::new(-500.0, 0.0, 0.0), radius: 20.0 });
    let common = EntityPath::root(a);
    assert!(profile_of(h.document(), &common).is_none());
    let any = profile_of(h.document(), &copy_path).unwrap();
    let only = h.document().resolve_part(&common).unwrap().1.bodies[0].id;
    assert!(h.apply(Operation::SetProfile { target: common, body: only, profile: any, scope: Scope::All }).is_err());
}

// ---- copy and paste (docs/tools/copy-paste.md) ---------------------------------------------------------

/// A paste needing no kernel, applied.
fn paste(h: &mut History, context: Id, fragment: &Arc<Document>, transform: DAffine3, reuse: bool) -> Outcome {
    let op = Operation::Paste { context, fragment: fragment.clone(), transform, reuse };
    assert!(matches!(h.document().prepare(&op).unwrap(), Prepared::Done(_)), "a paste needs no kernel");
    h.apply(op).unwrap()
}

#[test]
fn pasting_in_the_same_document_gives_occurrences_of_the_same_parts() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().created[0];
    let line = GuideShape::Line { origin: DVec3::new(0.0, 5.0, 0.0), dir: DVec3::X };
    let g = h.apply(Operation::AddGuide { component: root, shape: line }).unwrap().guide.unwrap();
    let components = h.document().components().count();

    let fragment = Arc::new(h.document().entities_fragment(&[], &[asm, g]).unwrap());
    // Only what the copied occurrences use: the assembly and its two parts.
    assert_eq!(fragment.components().count(), 3);
    let up = DAffine3::from_translation(DVec3::Z * 50.0);
    let out = paste(&mut h, root, &fragment, up, true);
    assert_eq!(out.created.len(), 2, "the occurrence and the guide");
    let doc = h.document();
    assert_eq!(doc.components().count(), components, "no new part nor assembly");
    let def = occ(doc, &EntityPath::root(asm)).component;
    assert_eq!(doc.occurrence_count(def), 2);
    let pasted = occ(doc, &EntityPath::root(out.created[0]));
    assert_eq!(pasted.component, def);
    assert!(close(pasted.transform.translation, occ(doc, &EntityPath::root(asm)).transform.translation + DVec3::Z * 50.0));
    let guide = doc.component(root).unwrap().guide(out.created[1]).unwrap();
    assert_eq!(guide.shape, GuideShape::Line { origin: DVec3::new(0.0, 5.0, 50.0), dir: DVec3::X });

    // Pasted into the assembly it copied: it would contain itself.
    assert!(matches!(
        h.apply(Operation::Paste { context: def, fragment: fragment.clone(), transform: DAffine3::IDENTITY, reuse: true }),
        Err(Error::Cycle)
    ));
    // Not rigid, nothing in it: refused.
    let scale = DAffine3::from_scale(DVec3::splat(2.0));
    assert!(h.apply(Operation::Paste { context: root, fragment: fragment.clone(), transform: scale, reuse: true }).is_err());
    let empty = Arc::new(h.document().entities_fragment(&[], &[]).unwrap());
    assert!(matches!(
        h.apply(Operation::Paste { context: root, fragment: empty, transform: DAffine3::IDENTITY, reuse: true }),
        Err(Error::Invalid(Refusal::NothingPasted))
    ));
}

#[test]
fn the_fragment_is_in_world_axes_and_pastes_in_place_in_any_context() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::new(10.0, 0.0, 0.0), 10.0));
    let b = add(&mut h, cube(DVec3::new(40.0, 0.0, 0.0), 10.0));
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().created[0];
    h.apply(Operation::Transform { context: root, entities: vec![asm], transform: DAffine3::from_translation(DVec3::Y * 100.0) }).unwrap();
    let before = world_bounds(h.document(), &EntityPath::new(vec![asm], a));

    // Copied inside the assembly, pasted in place at the root: where it was in the world.
    let fragment = Arc::new(h.document().entities_fragment(&[asm], &[a]).unwrap());
    let out = paste(&mut h, root, &fragment, DAffine3::IDENTITY, true);
    let after = world_bounds(h.document(), &EntityPath::root(out.created[0]));
    assert!(close(before.0, after.0) && close(before.1, after.1), "{before:?} {after:?}");

    // And back in the assembly, brought into its axes.
    let def = h.document().context_of(&[asm]).unwrap();
    let to_axes = h.document().transform_of(&[asm]).unwrap().inverse();
    let out = paste(&mut h, def, &fragment, to_axes, true);
    let again = world_bounds(h.document(), &EntityPath::new(vec![asm], out.created[0]));
    assert!(close(before.0, again.0), "{before:?} {again:?}");
}

#[test]
fn pasting_in_another_document_grafts_and_keeps_occurrences_loose() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let oak = h
        .apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90], kind: Default::default() })
        .unwrap()
        .material
        .unwrap();
    h.apply(Operation::AddMaterial { name: "Noyer".into(), color: [90, 60, 40], kind: Default::default() }).unwrap();
    h.apply(Operation::PaintParts {
        targets: vec![EntityPath::root(a)],
        layer: Family::Coloration,
        material: Some(oak),
        scope: Scope::All,
    })
    .unwrap();
    // Two occurrences of one part: still one part once pasted.
    h.apply(Operation::Duplicate { context: root, entities: vec![b], transform: DAffine3::from_translation(DVec3::X * 20.0) }).unwrap();
    let all: Vec<Id> = h.document().root_assembly().occurrences().iter().map(|o| o.id).collect();
    let fragment = Arc::new(h.document().entities_fragment(&[], &all).unwrap());
    assert_eq!(fragment.materials().count(), 1, "only the material used");

    let mut other = History::new(Document::new());
    let oak2 = other
        .apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90], kind: Default::default() })
        .unwrap()
        .material
        .unwrap();
    let c = add(&mut other, cube(DVec3::new(0.0, 50.0, 0.0), 5.0));
    let parts = other.document().components().count();
    let other_root = other.document().root();
    let out = paste(&mut other, other_root, &fragment, DAffine3::IDENTITY, false);
    let doc = other.document();
    assert_eq!(out.created.len(), 3);
    assert_eq!(doc.root_assembly().occurrences().len(), 4, "loose, not gathered in an assembly");
    assert_eq!(doc.components().count(), parts + 2);
    assert_eq!(doc.materials().count(), 1, "the same oak");
    assert_eq!(doc.part(occ(doc, &EntityPath::root(out.created[0])).component).unwrap().coloration, Some(oak2));
    let (p1, p2) = (occ(doc, &EntityPath::root(out.created[1])).component, occ(doc, &EntityPath::root(out.created[2])).component);
    assert_eq!(p1, p2);
    assert_ne!(occ(doc, &EntityPath::root(out.created[0])).component, occ(doc, &EntityPath::root(c)).component);
}

#[test]
fn a_part_gone_since_the_copy_is_grafted_back() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let part = occ(h.document(), &EntityPath::root(a)).component;
    let fragment = Arc::new(h.document().entities_fragment(&[], &[a]).unwrap());

    // Cut, then purged: the part is gone.
    h.apply(Operation::Delete { context: root, entities: vec![a] }).unwrap();
    h.apply(Operation::PurgeUnused).unwrap();
    assert!(h.document().component(part).is_err());
    let out = paste(&mut h, root, &fragment, DAffine3::IDENTITY, true);
    let back = occ(h.document(), &EntityPath::root(out.created[0])).component;
    assert!(h.document().part(back).is_ok());
    assert_eq!(h.document().components().count(), 1);

    // Copied, then undone: the ids came back and another part took them; the caller passes
    // `reuse: false` there, and the copy is a part of its own.
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let fragment = Arc::new(h.document().entities_fragment(&[], &[a]).unwrap());
    h.undo();
    let c = add(&mut h, Primitive::Cylinder { origin: DVec3::ZERO, radius: 5.0, height: 10.0 });
    let taken = occ(h.document(), &EntityPath::root(c)).component;
    let out = paste(&mut h, root, &fragment, DAffine3::IDENTITY, false);
    assert_ne!(occ(h.document(), &EntityPath::root(out.created[0])).component, taken);
    assert_eq!(h.document().components().count(), 2);
}

#[test]
fn machinings_paste_into_another_part() {
    let mut h = History::new(Document::new());
    let (source, part, hole) = drilled_board(&mut h);
    let line = GuideShape::Line { origin: DVec3::new(0.0, 25.0, 18.0), dir: DVec3::X };
    let g = h.apply(Operation::AddGuide { component: part, shape: line }).unwrap().guide.unwrap();
    let fragment = Arc::new(h.document().bodies_fragment(&source, &[hole, g]).unwrap());
    assert_eq!((fragment.bodies.len(), fragment.guides.len()), (1, 1));

    // Its twin, elsewhere and turned: in place, the hole lands at the same spot in its axes.
    let twin = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let root = h.document().root();
    h.apply(Operation::Transform {
        context: root,
        entities: vec![twin],
        transform: DAffine3::from_rotation_z(1.0) * DAffine3::from_translation(DVec3::Y * 300.0),
    })
    .unwrap();
    let target = EntityPath::root(twin);
    let before = faces(h.document(), occ(h.document(), &target).component);
    let op =
        Operation::PasteBodies { target: target.clone(), fragment: fragment.clone(), transform: DAffine3::IDENTITY, scope: Scope::All };
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Recipe { negatives: tools, .. }] if tools.len() == 1));
    let out = h.apply(op).unwrap();
    assert_eq!(out.machinings().len(), 2, "the machining and the guide");
    let twin_part = occ(h.document(), &target).component;
    let p = h.document().part(twin_part).unwrap();
    assert_eq!(p.machinings().len(), 1);
    assert!(close(
        p.machinings()[0].placement.translation,
        h.document().part(part).unwrap().machining(hole).unwrap().placement.translation
    ));
    assert!(faces(h.document(), twin_part) > before);
    assert_eq!(h.document().component(twin_part).unwrap().guide(out.machinings()[1]).unwrap().shape, line);

    // Into a part with no occurrence path: refused; nothing to paste: refused.
    assert!(
        h.apply(Operation::PasteBodies {
            target: EntityPath::root(Id(999)),
            fragment: fragment.clone(),
            transform: DAffine3::IDENTITY,
            scope: Scope::All
        })
        .is_err()
    );
    let empty = Arc::new(BodyFragment::default());
    assert!(h.apply(Operation::PasteBodies { target, fragment: empty, transform: DAffine3::IDENTITY, scope: Scope::All }).is_err());
}

#[test]
fn parts_paste_into_a_part_as_groups_unless_plain() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    // A drilled board (a group of its bodies) and a plain dowel (kept whole, its profile with
    // it), into a board elsewhere.
    let (drilled, drilled_part, _) = drilled_board(&mut h);
    let dowel = add(&mut h, Primitive::Cylinder { origin: DVec3::new(0.0, 100.0, 0.0), radius: 4.0, height: 30.0 });
    let target = add(&mut h, Primitive::Box { origin: DVec3::new(0.0, 0.0, 100.0), size: DVec3::new(300.0, 200.0, 18.0) });
    let target = EntityPath::root(target);
    let fragment = Arc::new(h.document().entities_fragment(&[], &[drilled.entity, dowel]).unwrap());
    let parts = fragment.parts_as_bodies().unwrap();
    assert_eq!(parts.bodies.len(), 2);
    assert!(parts.bodies.iter().all(|b| b.sign == Sign::Positive));

    let (part, _, t) = h.document().resolve_part(&target).unwrap();
    let op = Operation::PasteParts { target: target.clone(), fragment: fragment.clone(), transform: t.inverse(), scope: Scope::All };
    let out = h.apply(op).unwrap();
    assert_eq!(out.machinings().len(), 2);
    let p = h.document().part(part).unwrap();
    assert_eq!(p.positives().count(), 3);
    // The drilled board, a group of its blank and its hole, its shape the final one, at its
    // world place.
    let group = p.body(out.machinings()[0]).unwrap();
    let def = p.def_of(group);
    assert!(def.is_group() && p.profile_of(group).is_none());
    assert_eq!(def.children().iter().map(|c| c.sign).collect::<Vec<_>>(), vec![Sign::Positive, Sign::Negative]);
    assert!(Arc::ptr_eq(p.solid_of(group), h.document().part(drilled_part).unwrap().shape()));
    assert!(def.children().iter().all(|c| p.def(c.def).is_some()));
    let (_, _, t_drilled) = h.document().resolve_part(&drilled).unwrap();
    assert!(close((t * group.placement).translation, t_drilled.translation));
    // The dowel, its profile kept.
    let kept = p.body(out.machinings()[1]).unwrap();
    assert!(p.profile_of(kept).is_some());
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap(), p);

    // An assembly: one body per part under it, the hidden ones left out.
    let a = add(&mut h, cube(DVec3::new(500.0, 0.0, 0.0), 10.0));
    let b = add(&mut h, cube(DVec3::new(520.0, 0.0, 0.0), 10.0));
    let c = add(&mut h, cube(DVec3::new(540.0, 0.0, 0.0), 10.0));
    let asm = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b, c], name: None }).unwrap().created[0];
    h.apply(Operation::SetHidden { context: asm_def(&h, asm), entities: vec![c], hidden: true }).unwrap();
    let fragment = Arc::new(h.document().entities_fragment(&[], &[asm]).unwrap());
    assert_eq!(fragment.parts_as_bodies().unwrap().bodies.len(), 2);
}

#[test]
fn bodies_paste_out_of_a_part_as_a_new_part_in_its_axes() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let (source, part, hole) = drilled_board(&mut h);
    let turn = DAffine3::from_translation(DVec3::Y * 300.0) * DAffine3::from_rotation_z(1.0);
    h.apply(Operation::Transform { context: root, entities: vec![source.entity], transform: turn }).unwrap();
    let blank = h.document().part(part).unwrap().positives().next().unwrap().id;

    // The blank and its hole: a part drilled as the source, where it is.
    let fragment = Arc::new(h.document().bodies_fragment(&source, &[blank, hole]).unwrap());
    let op = Operation::PasteAsPart { context: root, fragment, transform: DAffine3::IDENTITY };
    let out = h.apply(op).unwrap();
    let path = EntityPath::root(out.created[0]);
    let (new, p, t) = h.document().resolve_part(&path).unwrap();
    assert_ne!(new, part);
    assert_eq!((p.positives().count(), p.negatives().count()), (1, 1));
    assert!(close(t.translation, h.document().resolve_part(&source).unwrap().2.translation));
    assert_eq!(faces(h.document(), new), faces(h.document(), part));

    // The hole alone: its shape becomes a part (a dowel), positive.
    let fragment = Arc::new(h.document().bodies_fragment(&source, &[hole]).unwrap());
    let out = h.apply(Operation::PasteAsPart { context: root, fragment, transform: DAffine3::IDENTITY }).unwrap();
    let (_, p, _) = h.document().resolve_part(&EntityPath::root(out.created[0])).unwrap();
    assert_eq!((p.positives().count(), p.negatives().count()), (1, 0));

    // Guides only: refused.
    let line = GuideShape::Line { origin: DVec3::ZERO, dir: DVec3::X };
    let g = h.apply(Operation::AddGuide { component: part, shape: line }).unwrap().guide.unwrap();
    let fragment = Arc::new(h.document().bodies_fragment(&source, &[g]).unwrap());
    assert!(h.apply(Operation::PasteAsPart { context: root, fragment, transform: DAffine3::IDENTITY }).is_err());
}

#[test]
fn the_history_tells_which_states_it_holds() {
    let mut h = History::new(Document::new());
    add(&mut h, cube(DVec3::ZERO, 10.0));
    let copied = h.revision();
    add(&mut h, cube(DVec3::X * 20.0, 10.0));
    assert!(h.holds(copied) && h.holds(0));
    h.undo();
    h.undo();
    assert!(h.holds(copied), "undone, still there to redo");
    // Another way taken: the state copied is gone.
    add(&mut h, cube(DVec3::Y * 20.0, 10.0));
    assert!(!h.holds(copied));
}

// ---- the history's memory (docs/model/undo.md#memory-cap) ------------------------------

fn record_bytes(s: &Arc<kernel::Solid>) -> u64 {
    s.approx_bytes()
}

#[test]
fn the_history_weighs_what_its_states_hold_alone() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    h.weigh(&record_bytes);
    let cube_bytes = h.document().components().find_map(|c| c.as_part()).unwrap().shape().approx_bytes();
    assert!(cube_bytes > 1000, "a cube's record: {cube_bytes} bytes");
    assert!(h.held_bytes() < cube_bytes, "the cube is in the current state: not held by the history");
    let cube = h.document().resolve(&EntityPath::root(a)).unwrap().component;
    h.apply(Operation::DeleteComponent { component: cube }).unwrap();
    h.weigh(&record_bytes);
    assert!(h.held_bytes() > cube_bytes, "deleted, the state before holds it alone");
    // Two settings changes in a row merge: the pair weighed again.
    let mut s = h.document().settings().clone();
    s.display.grid = !s.display.grid;
    h.apply(Operation::SetSettings(s.clone())).unwrap();
    s.display.axes = !s.display.axes;
    h.apply(Operation::SetSettings(s)).unwrap();
    h.weigh(&record_bytes);
    assert_eq!(h.held_bytes(), h.reweighed(&record_bytes));
    // The weights follow undo and redo.
    for undo in [true, true, true, false, true, false, false] {
        assert!(if undo { h.undo() } else { h.redo() });
        assert_eq!(h.held_bytes(), h.reweighed(&record_bytes), "after {}", if undo { "undo" } else { "redo" });
    }
}

#[test]
fn forgetting_drops_the_farthest_state_and_keeps_one_step_to_undo() {
    let mut h = History::new(Document::new());
    for i in 0..4 {
        add(&mut h, cube(DVec3::X * 20.0 * i as f64, 10.0));
    }
    h.weigh(&record_bytes);
    h.undo();
    // States 0 to 2 before, 4 to redo: the oldest is the farthest.
    let held = h.held_bytes();
    let freed = h.forget().unwrap();
    assert!(!h.holds(0) && h.holds(1));
    assert_eq!(h.held_bytes(), held - freed);
    assert_eq!(h.held_bytes(), h.reweighed(&record_bytes));
    assert!(h.forget().is_some() && !h.holds(1), "two before, one to redo: the one before");
    assert!(h.forget().is_some() && !h.can_redo(), "one each way: the one to redo");
    assert!(h.can_undo() && !h.can_forget() && h.forget().is_none(), "the last step to undo stays");
    assert_eq!((h.forgotten(), h.journal().count()), (3, 1));
    assert!(h.undo());
    assert_eq!(h.document().components().count(), 2);
}

// ---- bodies: positive and negative solids (docs/model/bodies.md) ---------------------------

/// The bounds of the part `target` places, in its own axes.
fn part_bounds(doc: &Document, target: &EntityPath) -> (DVec3, DVec3) {
    doc.resolve_part(target).unwrap().1.shape().bounds().unwrap()
}

#[test]
fn a_positive_body_joins_the_part_and_every_negative_cuts_it() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    assert!(!h.document().part(part).unwrap().is_plain());
    // A tenon at the board's end, drawn in the part: one recipe, from the part as it is.
    let tenon = Primitive::Box { origin: DVec3::new(200.0, 15.0, 4.0), size: DVec3::new(30.0, 20.0, 10.0) };
    let op = Operation::AddBody {
        target: target.clone(),
        primitive: tenon,
        placement: DAffine3::IDENTITY,
        sign: Sign::Positive,
        scope: Scope::All,
    };
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!() };
    assert!(
        matches!(&job.tasks[..], [kernel::Task::Primitive { .. }, kernel::Task::Recipe { base: Some(_), positives, negatives, .. }] if positives.len() == 1 && negatives.len() == 1),
        "{:?}",
        job.tasks
    );
    let tenon = h.apply(op).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    assert_eq!((p.positives().count(), p.negatives().count()), (2, 1));
    assert_eq!(p.body(tenon).unwrap().sign, Sign::Positive);
    assert!(p.profile_of(p.body(tenon).unwrap()).is_some(), "drawn, it keeps its profile");
    assert_eq!(p.pieces(), 1);
    // The part's size is its final shape's (docs/model/bodies.md): the tenon counts.
    let (lo, hi) = part_bounds(h.document(), &target);
    assert!((hi.x - lo.x - 230.0).abs() < 1e-6, "{lo} {hi}");

    // The hole moved onto the tenon cuts it too, though it was made before.
    let before = faces(h.document(), part);
    h.apply(Operation::TransformBodies {
        target: target.clone(),
        bodies: vec![hole],
        transform: DAffine3::from_translation(DVec3::X * 195.0),
        scope: Scope::All,
    })
    .unwrap();
    assert!(faces(h.document(), part) >= before, "the hole goes through the tenon");
    assert!(h.document().part(part).unwrap().failed.is_empty());

    // Deleting the blank keeps the tenon; deleting the last positive leaves the part empty
    // (docs/model/bodies.md#empty-parts): no piece, its machinings kept.
    let blank = h.document().part(part).unwrap().positives().next().unwrap().id;
    h.apply(Operation::DeleteBodies { target: target.clone(), bodies: vec![blank], scope: Scope::All }).unwrap();
    h.apply(Operation::DeleteBodies { target: target.clone(), bodies: vec![tenon], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.shape().is_empty() && p.pieces() == 0 && p.positives().next().is_none() && p.negatives().count() > 0);

    // Saved and read back with its bodies.
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap().bodies, h.document().part(part).unwrap().bodies);
}

#[test]
fn a_body_s_sign_is_toggled() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    let blank = h.document().part(part).unwrap().positives().next().unwrap().id;
    let full = faces(h.document(), part);
    let set = |bodies: Vec<Id>, sign| Operation::SetSign { target: target.clone(), bodies, sign, scope: Scope::All };
    // The hole made a blank: a pin through the board, out of it on both sides.
    h.apply(set(vec![hole], Sign::Positive)).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!((p.positives().count(), p.negatives().count()), (2, 0));
    let (lo, hi) = part_bounds(h.document(), &target);
    assert!((lo.z + 5.0).abs() < 1e-6 && (hi.z - 25.0).abs() < 1e-6, "{lo} {hi}");
    // Back to a machining: the part as it was, the body keeping its id and number.
    h.apply(set(vec![hole], Sign::Negative)).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), vec![blank, hole]);
    assert_eq!(faces(h.document(), part), full);
    // A sign already held changes nothing; the last blank made a machining leaves the part
    // empty (docs/model/bodies.md#empty-parts), saved and read back as it is; made a blank again, the part
    // as it was.
    h.apply(set(vec![hole], Sign::Negative)).unwrap();
    h.apply(set(vec![blank, hole], Sign::Negative)).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.shape().is_empty() && p.pieces() == 0 && p.failed.is_empty());
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap(), p);
    h.apply(set(vec![blank], Sign::Positive)).unwrap();
    assert_eq!(faces(h.document(), part), full);
    // Undone with the history.
    h.apply(set(vec![hole], Sign::Positive)).unwrap();
    h.undo();
    assert_eq!(h.document().part(part).unwrap().body(hole).unwrap().sign, Sign::Negative);
}

#[test]
fn a_part_in_several_pieces_is_counted() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let target = EntityPath::root(board);
    let apart = Primitive::Box { origin: DVec3::new(300.0, 0.0, 0.0), size: DVec3::new(20.0, 20.0, 18.0) };
    h.apply(Operation::AddBody {
        target: target.clone(),
        primitive: apart,
        placement: DAffine3::IDENTITY,
        sign: Sign::Positive,
        scope: Scope::All,
    })
    .unwrap();
    assert_eq!(h.document().resolve_part(&target).unwrap().1.pieces(), 2);
    // A groove right through splits a part too.
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let target = EntityPath::root(board);
    let groove = Primitive::Box { origin: DVec3::new(90.0, -10.0, -10.0), size: DVec3::new(20.0, 70.0, 40.0) };
    h.apply(Operation::AddBody {
        target: target.clone(),
        primitive: groove,
        placement: DAffine3::IDENTITY,
        sign: Sign::Negative,
        scope: Scope::All,
    })
    .unwrap();
    assert_eq!(h.document().resolve_part(&target).unwrap().1.pieces(), 2);
}

#[test]
fn a_placed_body_keeps_its_profile_in_its_own_axes() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 300.0, 18.0) });
    let target = EntityPath::root(board);
    let (part, p, _) = h.document().resolve_part(&target).unwrap();
    let blank = p.bodies[0].id;
    // Stood up in the part: a quarter turn about X, its profile untouched.
    let up = DAffine3::from_rotation_x(std::f64::consts::FRAC_PI_2);
    h.apply(Operation::TransformBodies { target: target.clone(), bodies: vec![blank], transform: up, scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(!p.is_plain() && p.shape.is_some());
    let b = p.body(blank).unwrap();
    assert_eq!(kernel::profile::bounds(&p.profile_of(b).unwrap().profile).unwrap().1, glam::DVec2::new(600.0, 300.0));
    let (lo, hi) = part_bounds(h.document(), &target);
    assert!(close(hi - lo, DVec3::new(600.0, 18.0, 300.0)), "{lo} {hi}");

    // Stretched along the part's Z: along the body's Y, in its plane — the profile follows.
    h.apply(stretch(h.document(), &target, DVec3::Z, vec![150.0], vec![0.0, 100.0], Scope::All)).unwrap();
    let p = h.document().part(part).unwrap();
    let b = p.body(blank).unwrap();
    assert_eq!(kernel::profile::bounds(&p.profile_of(b).unwrap().profile).unwrap().1, glam::DVec2::new(600.0, 400.0));
    let (lo, hi) = part_bounds(h.document(), &target);
    assert!((hi.z - lo.z - 400.0).abs() < 1e-6, "{lo} {hi}");

    // Its profile set again, in its own axes: the part stays stood up.
    let square = kernel::profile::polygon(&[(0.0, 0.0), (50.0, 0.0), (50.0, 50.0), (0.0, 50.0)].map(|(x, y)| glam::DVec2::new(x, y)));
    let profile = BodyProfile::extruded(square, 0.0, 10.0);
    h.apply(Operation::SetProfile { target: target.clone(), body: blank, profile, scope: Scope::All }).unwrap();
    let (lo, hi) = part_bounds(h.document(), &target);
    assert!(close(hi - lo, DVec3::new(50.0, 10.0, 50.0)), "{lo} {hi}");
}

#[test]
fn a_union_brings_the_tool_as_a_group() {
    let mut h = History::new(Document::new());
    let (target, part, _) = drilled_board(&mut h);
    // A second drilled board, crossing the first's end, turned a quarter.
    let other = add(&mut h, Primitive::Box { origin: DVec3::new(180.0, -100.0, 0.0), size: DVec3::new(20.0, 250.0, 18.0) });
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(190.0, -80.0, -5.0), radius: 3.0, height: 30.0 });
    h.apply(Operation::boolean(EntityPath::root(other), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    h.apply(Operation::boolean(target.clone(), EntityPath::root(other), Boolean::Union, Scope::All)).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!((p.positives().count(), p.negatives().count()), (2, 0));
    assert!(p.failed.is_empty(), "{:?}", p.failed);
    assert_eq!(p.pieces(), 1);
    // The target's bodies grouped first, its hole with them; the tool, a group of its blank and
    // its hole, in the target's axes.
    assert_eq!(p.def_of(&p.bodies[0]).children().len(), 2);
    let group = p.positives().nth(1).unwrap();
    let children = p.def_of(group).children();
    assert_eq!(children.len(), 2);
    let hole = children.iter().find(|c| c.sign == Sign::Negative).unwrap();
    assert!(close((group.placement * hole.placement).translation, DVec3::new(190.0, -80.0, -5.0)));
    let (lo, hi) = part_bounds(h.document(), &target);
    assert!(close(lo, DVec3::new(0.0, -100.0, 0.0)) && close(hi, DVec3::new(200.0, 150.0, 18.0)), "{lo} {hi}");
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap(), p);
}

#[test]
fn a_union_keeps_the_target_s_machinings_off_the_tools() {
    let mut h = History::new(Document::new());
    // The board's hole runs from 5 under it: into a block set under the board.
    let (target, part, hole) = drilled_board(&mut h);
    let block = add(&mut h, Primitive::Box { origin: DVec3::new(10.0, 15.0, -30.0), size: DVec3::new(20.0, 20.0, 30.0) });
    h.apply(Operation::boolean(target.clone(), EntityPath::root(block), Boolean::Union, Scope::All)).unwrap();
    let p = h.document().part(part).unwrap();
    let whole = 200.0 * 50.0 * 18.0 - std::f64::consts::PI * 16.0 * 18.0 + 20.0 * 20.0 * 30.0;
    assert!((volume(h.document(), part) - whole).abs() < 1e-3, "{}", volume(h.document(), part));
    // The board's bodies a group of their own, the first body; the block beside it.
    let group = &p.bodies[0];
    assert!(p.def_of(group).is_group() && group.sign == Sign::Positive);
    assert!(p.def_of(group).children().iter().any(|c| c.id == hole));
    assert!(!p.def_of(&p.bodies[1]).is_group());
    let named = p.def_of(group).name.clone();
    assert!(named.contains('#'), "{named}");
    // A second union: no machining at the top level now, nothing grouped again.
    let other = add(&mut h, Primitive::Box { origin: DVec3::new(100.0, 0.0, 18.0), size: DVec3::new(20.0, 20.0, 20.0) });
    h.apply(Operation::boolean(target, EntityPath::root(other), Boolean::Union, Scope::All)).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.bodies.len(), 3);
    assert_eq!(p.defs.iter().filter(|d| d.is_group()).count(), 1);
    // Undone: the board as it was, its hole at the top level.
    assert!(h.undo() && h.undo() && h.undo());
    let p = h.document().part(part).unwrap();
    assert!(p.body(hole).is_some() && p.defs.iter().all(|d| !d.is_group()));
}

/// A board with a block on it drilled through by a drill longer than it, down into the board,
/// the block joined to the board: its part and the occurrence.
fn board_and_drilled_block() -> (History, Id, EntityPath) {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let block = add(&mut h, Primitive::Box { origin: DVec3::new(50.0, 10.0, 18.0), size: DVec3::new(30.0, 30.0, 20.0) });
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(65.0, 25.0, 5.0), radius: 4.0, height: 40.0 });
    h.apply(Operation::boolean(EntityPath::root(block), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let target = EntityPath::root(board);
    h.apply(Operation::boolean(target.clone(), EntityPath::root(block), Boolean::Union, Scope::All)).unwrap();
    let part = h.document().resolve_part(&target).unwrap().0;
    (h, part, target)
}

fn volume(doc: &Document, part: Id) -> f64 {
    doc.part(part).unwrap().shape().volume().unwrap()
}

#[test]
fn a_union_s_tool_machinings_cut_the_tool_only_until_ungrouped() {
    let (mut h, part, target) = board_and_drilled_block();
    let hole = |length: f64| std::f64::consts::PI * 16.0 * length;
    // The drill goes through the block (20 long in it), not into the board (13 more).
    let whole = 200.0 * 50.0 * 18.0 + 30.0 * 30.0 * 20.0;
    assert!((volume(h.document(), part) - (whole - hole(20.0))).abs() < 1e-3, "{}", volume(h.document(), part));
    // Flattened, its reach is the part's: it drills the board too.
    let group = h.document().part(part).unwrap().positives().nth(1).unwrap().id;
    let out = h.apply(Operation::UngroupBodies { target: target.clone(), bodies: vec![group], scope: Scope::All }).unwrap();
    assert_eq!(out.bodies.len(), 2);
    let p = h.document().part(part).unwrap();
    assert!(p.defs.iter().all(|d| !d.is_group()), "the group's definition dropped");
    assert_eq!((p.positives().count(), p.negatives().count()), (2, 1));
    assert!((volume(h.document(), part) - (whole - hole(33.0))).abs() < 1e-3, "{}", volume(h.document(), part));
    // Grouped again: the drill's reach is the group's once more.
    let ids: Vec<Id> = out.bodies.clone();
    let out = h.apply(Operation::GroupBodies { target: target.clone(), bodies: ids, scope: Scope::All }).unwrap();
    assert_eq!(out.bodies.len(), 1);
    assert!((volume(h.document(), part) - (whole - hole(20.0))).abs() < 1e-3, "{}", volume(h.document(), part));
    assert!(h.undo() && h.undo());
    assert!(h.document().part(part).unwrap().def_of(h.document().part(part).unwrap().positives().nth(1).unwrap()).is_group());
}

#[test]
fn negatives_grouped_make_a_compound_machining() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    // A counterbore: a wider, shallow cylinder over the hole.
    let bore = Primitive::Cylinder { origin: DVec3::new(20.0, 25.0, 12.0), radius: 8.0, height: 10.0 };
    let bore = h
        .apply(Operation::AddBody {
            target: target.clone(),
            primitive: bore,
            placement: DAffine3::IDENTITY,
            sign: Sign::Negative,
            scope: Scope::All,
        })
        .unwrap()
        .bodies[0];
    let before = volume(h.document(), part);
    let out = h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![hole, bore], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    let group = p.body(out.bodies[0]).unwrap().clone();
    assert_eq!(group.sign, Sign::Negative);
    assert!(p.def_of(&group).children().iter().all(|c| c.sign == Sign::Positive));
    assert_eq!((p.positives().count(), p.negatives().count()), (1, 1));
    assert!((volume(h.document(), part) - before).abs() < 1e-6);
    // Copied along the board: one definition, computed once.
    let step = DAffine3::from_translation(DVec3::X * 50.0);
    h.apply(Operation::ArrayBodies { target: target.clone(), bodies: vec![group.id], step, count: 2, scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.negatives().count(), 3);
    assert_eq!(p.defs.iter().filter(|d| d.is_group()).count(), 1);
    assert_eq!(p.copies_of(group.def), 3);
    // Made unique: a definition of its own, its children too.
    let first = p.negatives().next().unwrap().id;
    h.apply(Operation::MakeBodiesUnique { target: target.clone(), bodies: vec![first], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    let groups: Vec<&BodyDef> = p.defs.iter().filter(|d| d.is_group()).collect();
    assert_eq!(groups.len(), 2);
    let ids = |d: &BodyDef| d.children().iter().map(|c| c.id).collect::<Vec<_>>();
    assert!(ids(groups[0]).iter().all(|id| !ids(groups[1]).contains(id)));
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap(), p);
    // Flattened: two machinings again.
    let out = h.apply(Operation::UngroupBodies { target: target.clone(), bodies: vec![first], scope: Scope::All }).unwrap();
    assert_eq!(out.bodies.len(), 2);
    assert_eq!(h.document().part(part).unwrap().negatives().count(), 4);
}

#[test]
fn a_negative_group_sparing_material_is_not_flattened() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    // A tool that spares material: a pocket with an island (a drilled block, subtracted).
    let pocket = add(&mut h, Primitive::Box { origin: DVec3::new(50.0, 10.0, 10.0), size: DVec3::new(40.0, 30.0, 20.0) });
    let island = add(&mut h, Primitive::Box { origin: DVec3::new(65.0, 20.0, 0.0), size: DVec3::new(10.0, 10.0, 40.0) });
    h.apply(Operation::boolean(EntityPath::root(pocket), EntityPath::root(island), Boolean::Difference, Scope::All)).unwrap();
    let target = EntityPath::root(board);
    h.apply(Operation::boolean(target.clone(), EntityPath::root(pocket), Boolean::Difference, Scope::All)).unwrap();
    let part = h.document().resolve_part(&target).unwrap().0;
    let p = h.document().part(part).unwrap();
    // The tool, a negative group of its bodies: still editable.
    let group = p.negatives().next().unwrap().clone();
    assert!(p.def_of(&group).is_group());
    assert_eq!(p.def_of(&group).children().len(), 2);
    let blank = p.positives().next().unwrap().id;
    // The pocket, 8 deep, around a 10 × 10 island left standing.
    let expected = 200.0 * 50.0 * 18.0 - (40.0 * 30.0 - 100.0) * 8.0;
    assert!((volume(h.document(), part) - expected).abs() < 1e-3, "{}", volume(h.document(), part));
    let op = Operation::UngroupBodies { target: target.clone(), bodies: vec![group.id], scope: Scope::All };
    assert!(matches!(h.apply(op), Err(Error::Invalid(Refusal::UngroupNegative))));
    // Computed again from its bodies (a body added and deleted): the same.
    let other = h
        .apply(Operation::AddBody {
            target: target.clone(),
            primitive: cube(DVec3::new(150.0, 0.0, 18.0), 10.0),
            placement: DAffine3::IDENTITY,
            sign: Sign::Positive,
            scope: Scope::All,
        })
        .unwrap()
        .bodies[0];
    h.apply(Operation::DeleteBodies { target: target.clone(), bodies: vec![other], scope: Scope::All }).unwrap();
    assert!((volume(h.document(), part) - expected).abs() < 1e-3);
    assert!(h.document().part(part).unwrap().body(blank).is_some());
}

#[test]
fn a_stretch_goes_through_groups() {
    let (mut h, part, target) = board_and_drilled_block();
    // Across the block (x 50..80) at x = 70: the block 30 longer, its hole (at 65) left where it
    // is; the board too, 230 long.
    let op = stretch_x(h.document(), &target, 70.0, 30.0, Scope::All);
    h.apply(op).unwrap();
    let p = h.document().part(part).unwrap();
    let group = p.positives().find(|b| p.def_of(b).is_group()).unwrap();
    let def = p.def_of(group);
    let block = def.children().iter().find(|c| c.sign == Sign::Positive).unwrap();
    let (lo, hi) = p.solid_of(block).vertex_bounds().unwrap();
    let (lo, hi) = (block.placement.transform_point3(lo), block.placement.transform_point3(hi));
    let (lo, hi) = (group.placement.transform_point3(lo), group.placement.transform_point3(hi));
    assert!((hi.x - lo.x - 60.0).abs() < 1e-6, "{lo} {hi}");
    let hole = |length: f64| std::f64::consts::PI * 16.0 * length;
    let whole = 230.0 * 50.0 * 18.0 + 60.0 * 30.0 * 20.0;
    assert!((volume(h.document(), part) - (whole - hole(20.0))).abs() < 1e-3, "{}", volume(h.document(), part));
    let (lo, hi) = part_bounds(h.document(), &target);
    assert!((hi.x - lo.x - 230.0).abs() < 1e-6);
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap(), p);
}

#[test]
fn a_file_with_a_group_holding_itself_is_refused() {
    let (h, part, _) = board_and_drilled_block();
    let p = h.document().part(part).unwrap();
    let group = p.defs.iter().find(|d| d.is_group()).unwrap();
    let json = h.document().to_json().unwrap();
    // A child of the group made an occurrence of the group itself.
    let child = group.children()[0].def;
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    fn repoint(v: &mut serde_json::Value, group: u64, from: u64) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("id").and_then(|i| i.as_u64()) == Some(group)
                    && let Some(children) = m.get_mut("group").and_then(|g| g.get_mut("children")).and_then(|c| c.as_array_mut())
                {
                    for c in children {
                        if c.get("def").and_then(|d| d.as_u64()) == Some(from) {
                            c["def"] = group.into();
                        }
                    }
                }
                m.values_mut().for_each(|x| repoint(x, group, from));
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(|x| repoint(x, group, from)),
            _ => {}
        }
    }
    repoint(&mut v, group.id.0, child.0);
    assert!(Document::from_json(&v.to_string()).is_err());
    assert!(Document::from_json(&json).is_ok());
}

#[test]
fn a_difference_keeps_a_drawn_tool_s_profile() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    let p = h.document().part(part).unwrap();
    let (b, profile) = (p.body(hole).unwrap().clone(), p.profile_of(p.body(hole).unwrap()).cloned());
    assert!(profile.is_some(), "the drill's disc");
    assert!(close(b.placement.translation, DVec3::new(20.0, 25.0, -5.0)), "{}", b.placement.translation);
    // Its profile set: the hole made larger.
    let mut disc = profile.unwrap();
    disc.profile = kernel::profile::with_radius(&disc.profile, 6.0).unwrap();
    h.apply(Operation::SetProfile { target: target.clone(), body: hole, profile: disc, scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    let (lo, hi) = p.solid_of(p.body(hole).unwrap()).bounds().unwrap();
    assert!(((hi - lo).x - 12.0).abs() < 1e-6, "{lo} {hi}");
    // A machined tool is taken as its shape, no profile.
    let tool = add(&mut h, Primitive::Box { origin: DVec3::new(100.0, 10.0, -5.0), size: DVec3::new(20.0, 20.0, 30.0) });
    let small = add(&mut h, Primitive::Cylinder { origin: DVec3::new(110.0, 20.0, -10.0), radius: 2.0, height: 40.0 });
    h.apply(Operation::boolean(EntityPath::root(tool), EntityPath::root(small), Boolean::Difference, Scope::All)).unwrap();
    let out = h.apply(Operation::boolean(target, EntityPath::root(tool), Boolean::Difference, Scope::All)).unwrap();
    let _ = out;
    let p = h.document().part(part).unwrap();
    assert!(p.profile_of(p.negatives().last().unwrap()).is_none());
}

#[test]
fn copies_of_a_body_share_its_definition() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    let dx = DAffine3::from_translation(DVec3::X * 100.0);
    let copy = h
        .apply(Operation::DuplicateBodies { target: target.clone(), bodies: vec![hole], transform: dx, scope: Scope::All })
        .unwrap()
        .bodies[0];
    let p = h.document().part(part).unwrap();
    let def = p.body(hole).unwrap().def;
    assert_eq!(p.body(copy).unwrap().def, def, "a copy shares the definition");
    assert_eq!((p.defs.len(), p.copies_of(def)), (2, 2));
    // Saved once.
    let json = h.document().to_json().unwrap();
    let back = Document::from_json(&json).unwrap();
    assert_eq!(back.part(part).unwrap().defs.len(), 2);

    // The copy's profile set: both holes made larger.
    let width = |h: &History, id: Id| {
        let p = h.document().part(part).unwrap();
        let (lo, hi) = p.solid_of(p.body(id).unwrap()).bounds().unwrap();
        (hi - lo).x
    };
    let mut disc = p.profile_of(p.body(copy).unwrap()).cloned().unwrap();
    disc.profile = kernel::profile::with_radius(&disc.profile, 6.0).unwrap();
    h.apply(Operation::SetProfile { target: target.clone(), body: copy, profile: disc.clone(), scope: Scope::All }).unwrap();
    assert!((width(&h, hole) - 12.0).abs() < 1e-6 && (width(&h, copy) - 12.0).abs() < 1e-6);

    // Made unique, the copy changes alone.
    h.apply(Operation::MakeBodiesUnique { target: target.clone(), bodies: vec![copy], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert_ne!(p.body(copy).unwrap().def, p.body(hole).unwrap().def);
    disc.profile = kernel::profile::with_radius(&disc.profile, 3.0).unwrap();
    h.apply(Operation::SetProfile { target: target.clone(), body: copy, profile: disc, scope: Scope::All }).unwrap();
    assert!((width(&h, hole) - 12.0).abs() < 1e-6 && (width(&h, copy) - 6.0).abs() < 1e-6);
    // Erased, its definition goes with it.
    h.apply(Operation::DeleteBodies { target: target.clone(), bodies: vec![copy], scope: Scope::All }).unwrap();
    assert_eq!(h.document().part(part).unwrap().defs.len(), 2);
}

#[test]
fn bodies_pasted_share_their_definition_in_their_part_only() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    let fragment = Arc::new(h.document().bodies_fragment(&target, &[hole]).unwrap());
    let paste = |target: &EntityPath| Operation::PasteBodies {
        target: target.clone(),
        fragment: fragment.clone(),
        transform: DAffine3::from_translation(DVec3::X * 50.0),
        scope: Scope::All,
    };
    let pasted = h.apply(paste(&target)).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.body(pasted).unwrap().def, p.body(hole).unwrap().def, "pasted in its part: a copy");
    let other = add(&mut h, Primitive::Box { origin: DVec3::new(0.0, 100.0, 0.0), size: DVec3::new(200.0, 50.0, 18.0) });
    let other = EntityPath::root(other);
    let there = h.apply(paste(&other)).unwrap().bodies[0];
    let (_, q, _) = h.document().resolve_part(&other).unwrap();
    let def = q.body(there).unwrap().def;
    assert!(q.def(def).is_some() && h.document().part(part).unwrap().def(def).is_none(), "elsewhere: a definition of its own");
}

#[test]
fn copies_of_a_body_stretched_alike_or_not() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    let dx = DAffine3::from_translation(DVec3::X * 100.0);
    let copy = h
        .apply(Operation::DuplicateBodies { target: target.clone(), bodies: vec![hole], transform: dx, scope: Scope::All })
        .unwrap()
        .bodies[0];
    let height = |h: &History, id: Id| {
        let p = h.document().part(part).unwrap();
        let (lo, hi) = p.solid_of(p.body(id).unwrap()).bounds().unwrap();
        (hi - lo).z
    };
    let stretch = |bodies: Vec<Id>, axis: DVec3, cut: f64| Operation::StretchBodies {
        target: target.clone(),
        bodies,
        frame: DAffine3::IDENTITY,
        axis,
        cuts: vec![cut],
        moves: vec![0.0, 5.0],
        scope: Scope::All,
    };
    // One of them stretched along Z: the other, its copy, shows it too.
    h.apply(stretch(vec![hole], DVec3::Z, 10.0)).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.body(hole).unwrap().def, p.body(copy).unwrap().def);
    assert!((height(&h, hole) - 35.0).abs() < 1e-6 && (height(&h, copy) - 35.0).abs() < 1e-6);
    // Both, alike: still one definition.
    h.apply(stretch(vec![hole, copy], DVec3::Z, 10.0)).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!((p.defs.len(), p.body(hole).unwrap().def), (2, p.body(copy).unwrap().def));
    assert!((height(&h, copy) - 40.0).abs() < 1e-6);
    // The copy raised, a cutter across it only: the hole keeps the shape (moved), the copy goes
    // to a definition of its own.
    let up = DAffine3::from_translation(DVec3::Z * 100.0);
    h.apply(Operation::TransformBodies { target: target.clone(), bodies: vec![copy], transform: up, scope: Scope::All }).unwrap();
    h.apply(stretch(vec![hole, copy], DVec3::Z, 110.0)).unwrap();
    let p = h.document().part(part).unwrap();
    assert_ne!(p.body(hole).unwrap().def, p.body(copy).unwrap().def);
    assert_eq!(p.defs.len(), 3);
    assert!((height(&h, hole) - 40.0).abs() < 1e-6 && (height(&h, copy) - 45.0).abs() < 1e-6);
}

#[test]
fn a_union_s_bodies_share_their_copied_definitions() {
    let mut h = History::new(Document::new());
    let (_, _, hole) = drilled_board(&mut h);
    let _ = hole;
    let (tool_path, tool_part, tool_hole) = drilled_board(&mut h);
    let dx = DAffine3::from_translation(DVec3::X * 100.0);
    h.apply(Operation::DuplicateBodies { target: tool_path.clone(), bodies: vec![tool_hole], transform: dx, scope: Scope::All }).unwrap();
    let target = add(&mut h, Primitive::Box { origin: DVec3::new(0.0, 100.0, 0.0), size: DVec3::new(50.0, 50.0, 18.0) });
    let target = EntityPath::root(target);
    h.apply(Operation::boolean(target.clone(), tool_path, Boolean::Union, Scope::All)).unwrap();
    let (_, p, _) = h.document().resolve_part(&target).unwrap();
    // In the tool's group.
    let group = p.positives().find(|b| p.def_of(b).is_group()).unwrap();
    let holes: Vec<&Body> = p.def_of(group).children().iter().filter(|c| c.sign == Sign::Negative).collect();
    assert_eq!(holes.len(), 2);
    assert_eq!(holes[0].def, holes[1].def, "still copies of one definition");
    let source = h.document().part(tool_part).ok();
    assert!(source.is_none_or(|s| s.def(holes[0].def).is_none()), "a definition of the target's own");
}

#[test]
fn bodies_are_stretched_alone_in_their_axes() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(400.0, 200.0, 18.0) });
    let target = EntityPath::root(board);
    // A block on it, turned a quarter about Z: its own X along the part's Y.
    let turn = DAffine3::from_translation(DVec3::new(300.0, 50.0, 18.0)) * DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    let block = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 40.0, 30.0) };
    let out = h
        .apply(Operation::AddBody { target: target.clone(), primitive: block, placement: turn, sign: Sign::Positive, scope: Scope::All })
        .unwrap();
    let block = out.bodies[0];
    let (part, p, _) = h.document().resolve_part(&target).unwrap();
    let board_body = p.positives().next().unwrap().clone();
    let placement = p.body(block).unwrap().placement;
    // Along its own X (frame: its placement), a cutter at 50 moving what lies beyond by 60.
    let op = Operation::StretchBodies {
        target: target.clone(),
        bodies: vec![block],
        frame: placement,
        axis: DVec3::X,
        cuts: vec![50.0],
        moves: vec![0.0, 60.0],
        scope: Scope::All,
    };
    h.apply(op).unwrap();
    let p = h.document().part(part).unwrap();
    let b = p.body(block).unwrap();
    assert_eq!(kernel::profile::bounds(&p.profile_of(b).unwrap().profile).unwrap().1, glam::DVec2::new(160.0, 40.0));
    assert_eq!(p.positives().next().unwrap(), &board_body, "the board untouched");
    let (lo, hi) = part_bounds(h.document(), &target);
    assert!((hi.y - lo.y - 210.0).abs() < 1e-6 && (hi.z - 48.0).abs() < 1e-6, "{lo} {hi}");
    // A body wholly beyond the cutter is only moved.
    let p = h.document().part(part).unwrap();
    let at = p.body(block).unwrap().placement;
    h.apply(Operation::StretchBodies {
        target,
        bodies: vec![block],
        frame: DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![10.0],
        moves: vec![0.0, 5.0],
        scope: Scope::All,
    })
    .unwrap();
    let b = h.document().part(part).unwrap().body(block).unwrap().clone();
    assert!(close(b.placement.translation, at.translation + DVec3::X * 5.0), "{}", b.placement.translation);
}

// ---- test helpers over bodies ------------------------------------------------------------

#[test]
fn copies_put_down_each_placement_and_move_the_sources() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let root = h.document().root();
    let part = occ(h.document(), &EntityPath::root(a)).component;
    let line = GuideShape::Line { origin: DVec3::new(0.0, 5.0, 0.0), dir: DVec3::X };
    let g = h.apply(Operation::AddGuide { component: root, shape: line }).unwrap().guide.unwrap();
    let at = |x: f64, y: f64| DAffine3::from_translation(DVec3::new(x, y, 0.0));

    // A grid of 2 × 2 less the source, and a mirrored copy: no kernel, the same part.
    let mirror = DAffine3::from_translation(DVec3::X * 100.0) * DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0));
    let placements = vec![at(20.0, 0.0), at(0.0, 30.0), at(20.0, 30.0), mirror];
    let op = Operation::Copies { context: root, entities: vec![a, g], moved: DAffine3::IDENTITY, placements: placements.clone() };
    assert!(matches!(h.document().prepare(&op).unwrap(), Prepared::Done(_)));
    assert_eq!(op.label(), t!("operation.copies"));
    let out = h.apply(op).unwrap();
    assert_eq!(out.created.len(), 8, "4 occurrences, 4 guides");
    assert_eq!(h.document().occurrence_count(part), 5);
    assert_eq!(h.document().components().count(), 1);
    for (id, t) in out.created[..4].iter().zip(&placements) {
        assert!(occ(h.document(), &EntityPath::root(*id)).transform.abs_diff_eq(*t, 1e-12));
    }
    assert!(occ(h.document(), &EntityPath::root(out.created[3])).transform.matrix3.determinant() < 0.0, "mirrored");
    let guides = h.document().component(root).unwrap().guides.clone();
    assert_eq!(guides.len(), 5);
    assert_eq!(guides[3].shape, GuideShape::Line { origin: DVec3::new(20.0, 35.0, 0.0), dir: DVec3::X });
    assert!(occ(h.document(), &EntityPath::root(a)).transform.abs_diff_eq(DAffine3::IDENTITY, 1e-12), "the source stays");

    // Distribute: the source moved to the first slot, the copies placed from where it was.
    let op = Operation::Copies { context: root, entities: vec![a, g], moved: at(0.0, 100.0), placements: vec![at(0.0, 200.0)] };
    assert_eq!(op.label(), t!("operation.distribute"));
    let out = h.apply(op).unwrap();
    assert!(close(occ(h.document(), &EntityPath::root(a)).transform.translation, DVec3::new(0.0, 100.0, 0.0)));
    assert!(close(occ(h.document(), &EntityPath::root(out.created[0])).transform.translation, DVec3::new(0.0, 200.0, 0.0)));
    let guides = h.document().component(root).unwrap().guides.clone();
    assert_eq!(guides.iter().find(|x| x.id == g).unwrap().shape, GuideShape::Line { origin: DVec3::new(0.0, 105.0, 0.0), dir: DVec3::X });
    assert_eq!(guides.last().unwrap().shape, GuideShape::Line { origin: DVec3::new(0.0, 205.0, 0.0), dir: DVec3::X });
    // A move alone (one piece distributed) is fine.
    h.apply(Operation::Copies { context: root, entities: vec![a], moved: at(5.0, 0.0), placements: vec![] }).unwrap();

    // Refused: nothing to do, too many copies, a scale.
    let copies = |moved: DAffine3, placements: Vec<DAffine3>| Operation::Copies { context: root, entities: vec![a], moved, placements };
    assert!(h.apply(copies(DAffine3::IDENTITY, vec![])).is_err());
    assert!(h.apply(copies(DAffine3::IDENTITY, vec![at(1.0, 0.0); MAX_ARRAY as usize + 1])).is_err());
    assert!(h.apply(copies(DAffine3::IDENTITY, vec![DAffine3::from_scale(DVec3::splat(2.0))])).is_err());
    assert!(h.apply(copies(DAffine3::from_scale(DVec3::splat(2.0)), vec![at(1.0, 0.0)])).is_err());
}

#[test]
fn copies_keep_an_identity_move_out_of_the_file() {
    let op = Operation::Copies { context: Id(1), entities: vec![Id(2)], moved: DAffine3::IDENTITY, placements: vec![DAffine3::IDENTITY] };
    let json = serde_json::to_string(&op).unwrap();
    assert!(!json.contains("moved"), "{json}");
    assert_eq!(serde_json::from_str::<Operation>(&json).unwrap(), op);
}

#[test]
fn bodies_are_copied_along_placements_sharing_their_definition() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    let dx = |x: f64| DAffine3::from_translation(DVec3::X * x);

    // Copies only: cut from the shape as it is.
    let op = Operation::CopyBodies {
        target: target.clone(),
        bodies: vec![hole],
        moved: DAffine3::IDENTITY,
        placements: vec![dx(40.0), dx(80.0)],
        scope: Scope::All,
    };
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Recipe { negatives: tools, .. }] if tools.len() == 2), "{:?}", job.tasks);
    let created = h.apply(op).unwrap().machinings();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.machinings().len(), 3);
    assert!(close(p.machining(created[1]).unwrap().placement.translation, DVec3::new(100.0, 25.0, -5.0)));
    assert!(created.iter().all(|c| p.body(*c).unwrap().def == p.body(hole).unwrap().def), "one definition");

    // Distributed: the source moved too, the part computed again from its bodies.
    let op = Operation::CopyBodies {
        target: target.clone(),
        bodies: vec![hole],
        moved: dx(10.0),
        placements: vec![dx(150.0)],
        scope: Scope::All,
    };
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("needs the kernel") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Recipe { negatives: tools, .. }] if tools.len() == 4), "{:?}", job.tasks);
    let created = h.apply(op).unwrap().machinings();
    let p = h.document().part(part).unwrap();
    assert!(close(p.machining(hole).unwrap().placement.translation, DVec3::new(30.0, 25.0, -5.0)));
    assert!(close(p.machining(created[0]).unwrap().placement.translation, DVec3::new(170.0, 25.0, -5.0)));
    assert!(p.failed.is_empty());
}

impl Part {
    /// The first positive body's solid: a part's blank before docs/model/bodies.md.
    fn blank(&self) -> Arc<kernel::Solid> {
        self.solid_of(self.positives().next().unwrap()).clone()
    }

    /// The negative bodies: the machinings.
    fn machinings(&self) -> Vec<&Body> {
        self.negatives().collect()
    }

    fn machining(&self, id: Id) -> Option<&Body> {
        self.body(id).filter(|b| b.sign == Sign::Negative)
    }
}

impl Outcome {
    fn machinings(&self) -> Vec<Id> {
        self.bodies.clone()
    }
}

// ---- plane welding (docs/model/bodies.md) -------------------------------------------------------------------

/// A board 600 × 300 × 18 at the origin, as a part; its occurrence.
fn weld_board(h: &mut History) -> Id {
    add(h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 300.0, 18.0) })
}

fn add_body(h: &mut History, board: Id, primitive: Primitive, sign: Sign) -> Outcome {
    let target = EntityPath::root(board);
    h.apply(Operation::AddBody { target, primitive, placement: DAffine3::IDENTITY, sign, scope: Scope::All }).unwrap()
}

/// The bounds, in the part's axes, of its body `id`.
fn body_bounds(h: &History, board: Id, id: Id) -> (DVec3, DVec3) {
    let (_, p, _) = h.document().resolve_part(&EntityPath::root(board)).unwrap();
    let b = p.body(id).unwrap();
    let (lo, hi) = p.solid_of(b).vertex_bounds().unwrap();
    let (a, c) = (b.placement.transform_point3(lo), b.placement.transform_point3(hi));
    (a.min(c), a.max(c))
}

fn faces_of(h: &History, board: Id) -> usize {
    let (_, p, _) = h.document().resolve_part(&EntityPath::root(board)).unwrap();
    p.shape().record().faces.len()
}

#[test]
fn a_hole_a_hair_short_of_a_face_goes_through() {
    let mut h = History::new(Document::new());
    let board = weld_board(&mut h);
    // A hole from 1.4e-5 above the bottom to the top: welded onto the bottom, through.
    let hole = Primitive::Cylinder { origin: DVec3::new(100.0, 100.0, 1.4e-5), radius: 10.0, height: 18.0 - 1.4e-5 };
    let out = add_body(&mut h, board, hole, Sign::Negative);
    let id = out.bodies[0];
    assert_eq!(out.welds.len(), 1, "{:?}", out.welds);
    let w = out.welds[0];
    assert_eq!(w.body, id);
    assert!((w.distance + 1.4e-5).abs() < 1e-12, "{w:?}");
    let (lo, hi) = body_bounds(&h, board, id);
    assert!(lo.z.abs() < 1e-12 && (hi.z - 18.0).abs() < 1e-12, "{lo} {hi}");
    // Through: the board's six faces and the hole's side, no skin.
    assert_eq!(faces_of(&h, board), 7);
    // One undo step.
    h.undo();
    assert_eq!(h.document().resolve_part(&EntityPath::root(board)).unwrap().1.bodies.len(), 1);
}

#[test]
fn faces_further_than_the_weld_stay_where_they_are() {
    let mut h = History::new(Document::new());
    let board = weld_board(&mut h);
    let hole = Primitive::Cylinder { origin: DVec3::new(100.0, 100.0, 2.0 * WELD), radius: 10.0, height: 18.0 - 2.0 * WELD };
    let out = add_body(&mut h, board, hole, Sign::Negative);
    assert!(out.welds.is_empty(), "{:?}", out.welds);
    assert!((body_bounds(&h, board, out.bodies[0]).0.z - 2.0 * WELD).abs() < 1e-12);
}

#[test]
fn a_side_goes_onto_a_side_and_the_references_stay() {
    let mut h = History::new(Document::new());
    let board = weld_board(&mut h);
    // A groove along X, its far side 3e-5 short of the board's edge (y = 300), its bottom 6 deep.
    let groove = Primitive::Box { origin: DVec3::new(-10.0, 280.0, 12.0), size: DVec3::new(620.0, 20.0 - 3e-5, 10.0) };
    let out = add_body(&mut h, board, groove, Sign::Negative);
    assert_eq!(out.welds.len(), 1, "{:?}", out.welds);
    let (lo, hi) = body_bounds(&h, board, out.bodies[0]);
    assert!((hi.y - 300.0).abs() < 1e-12 && (lo.y - 280.0).abs() < 1e-12, "{lo} {hi}");
    // The board, left alone, did not move.
    let (_, p, _) = h.document().resolve_part(&EntityPath::root(board)).unwrap();
    let blank = p.bodies[0].id;
    assert_eq!(body_bounds(&h, board, blank), (DVec3::ZERO, DVec3::new(600.0, 300.0, 18.0)));
}

/// Bodies dropped one by one a little off the board's planes and off each other's: each face
/// moves once, onto a face that was there, never more than the weld away from where it was
/// asked; then two parallel faces of two bodies are one plane or the weld apart at least.
#[test]
fn welding_never_drifts() {
    let mut h = History::new(Document::new());
    let board = weld_board(&mut h);
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    // Places on a grid of 5 mm, each a jitter of up to twice the weld away.
    let mut asked: Vec<(Id, DVec3, DVec3)> = Vec::new();
    for _ in 0..12 {
        let mut at = || (next() * 20.0).floor() * 5.0 + (next() - 0.5) * 4.0 * WELD;
        let (x, y, z) = (at() + 50.0, at() + 50.0, at() * 0.1);
        let size = DVec3::new(at().abs() + 20.0, at().abs() + 20.0, at().abs() * 0.1 + 5.0);
        let lo = DVec3::new(x, y, z);
        let out = add_body(&mut h, board, Primitive::Box { origin: lo, size }, Sign::Negative);
        asked.push((out.bodies[0], lo, lo + size));
        // The bodies welded are this one only.
        assert!(out.welds.iter().all(|w| w.body == out.bodies[0]), "{:?}", out.welds);
    }
    let (_, p, _) = h.document().resolve_part(&EntityPath::root(board)).unwrap();
    let mut planes: Vec<(Id, usize, f64)> = Vec::new();
    for (id, lo, hi) in &asked {
        let (a, b) = body_bounds(&h, board, *id);
        for k in 0..3 {
            assert!((a[k] - lo[k]).abs() < WELD && (b[k] - hi[k]).abs() < WELD, "{id:?}: {a} {b}, asked {lo} {hi}");
            planes.push((*id, k, a[k]));
            planes.push((*id, k, b[k]));
        }
    }
    planes.extend([(p.bodies[0].id, 2, 0.0), (p.bodies[0].id, 2, 18.0)]);
    for (i, (ia, ka, a)) in planes.iter().enumerate() {
        for (ib, kb, b) in &planes[i + 1..] {
            if ia != ib && ka == kb {
                let d = (a - b).abs();
                assert!(!(1e-9..WELD - 1e-12).contains(&d), "{ia:?} and {ib:?} along {ka}: {a} {b}");
            }
        }
    }
}

#[test]
fn an_amended_step_undoes_as_one() {
    let mut h = History::new(Document::new());
    assert!(!h.amend(Document::new()), "no step to join");
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let part = occ(h.document(), &EntityPath::root(a)).component;
    let made = h.revision();
    let (named, _) = h.document().apply(&Operation::RenameComponent { component: part, name: "Pied".into() }).unwrap();
    assert!(h.amend(named));
    assert_ne!(h.revision(), made, "another state");
    assert_eq!(h.document().component(part).unwrap().name, "Pied");
    assert_eq!(h.journal().count(), 1);
    assert!(matches!(h.undo_op(), Some(Operation::AddPrimitive { .. })), "the step keeps its operation");
    assert!(h.undo());
    assert!(h.document().component(part).is_err(), "one undo takes the part away");
    assert!(h.redo());
    assert_eq!(h.document().component(part).unwrap().name, "Pied", "redone named");
    assert!(h.undo());
    assert!(!h.amend(Document::new()), "not with steps undone");
}

// ---- editing inside groups (docs/model/bodies.md#body-groups) -----------------------------------------------

#[test]
fn bodies_are_edited_inside_a_group_for_every_copy() {
    let (mut h, part, target) = board_and_drilled_block();
    let hole = |length: f64| std::f64::consts::PI * 16.0 * length;
    let whole = 200.0 * 50.0 * 18.0 + 30.0 * 30.0 * 20.0;
    let p = h.document().part(part).unwrap();
    let group = p.positives().find(|b| p.def_of(b).is_group()).unwrap().clone();
    let inside = target.clone().in_group(vec![group.id]);
    // Seen as a part: its children, in the group's axes, placed in the world.
    let (_, g, t) = h.document().resolve_container(&inside).unwrap();
    assert_eq!(g.bodies.len(), 2);
    assert!(close(t.translation, group.placement.translation));
    let drill = g.negatives().next().unwrap().id;

    // The drill moved down 13 inside the group (5..45 → -8..32): still the group's, it cuts the
    // block over 14 only and never the board, the group's shape and the part computed again.
    let down = DAffine3::from_translation(DVec3::Z * -13.0);
    h.apply(Operation::TransformBodies { target: inside.clone(), bodies: vec![drill], transform: down, scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.bodies.len(), 2, "nothing came out of the group");
    let (_, g, _) = h.document().resolve_container(&inside).unwrap();
    assert!(
        close(g.body(drill).unwrap().placement.translation.z * DVec3::Z, DVec3::Z * (5.0 - 13.0 - 18.0)),
        "{}",
        g.body(drill).unwrap().placement.translation
    );
    assert!((volume(h.document(), part) - (whole - hole(14.0))).abs() < 1e-3, "{}", volume(h.document(), part));

    // A body drawn in the group: a second hole, through the block only.
    let second = Primitive::Cylinder { origin: DVec3::new(5.0, 5.0, -10.0), radius: 2.0, height: 40.0 };
    let out = h
        .apply(Operation::AddBody {
            target: inside.clone(),
            primitive: second,
            placement: DAffine3::IDENTITY,
            sign: Sign::Negative,
            scope: Scope::All,
        })
        .unwrap();
    let (_, g, _) = h.document().resolve_container(&inside).unwrap();
    assert!(g.body(out.bodies[0]).is_some());
    assert!((volume(h.document(), part) - (whole - hole(14.0) - std::f64::consts::PI * 4.0 * 20.0)).abs() < 1e-3);

    // Copied along the board: the copies show what is done inside the group.
    let dx = DAffine3::from_translation(DVec3::X * 100.0);
    h.apply(Operation::DuplicateBodies { target: target.clone(), bodies: vec![group.id], transform: dx, scope: Scope::All }).unwrap();
    h.apply(Operation::DeleteBodies { target: inside.clone(), bodies: vec![out.bodies[0]], scope: Scope::All }).unwrap();
    let both = 200.0 * 50.0 * 18.0 + 2.0 * (30.0 * 30.0 * 20.0 - hole(14.0));
    assert!((volume(h.document(), part) - both).abs() < 1e-3, "{}", volume(h.document(), part));
    // Its last positive deleted: the group is empty, and failed in the part — every copy of it
    // —, the board alone left (docs/model/bodies.md#empty-parts).
    let (_, g, _) = h.document().resolve_container(&inside).unwrap();
    let block = g.positives().next().unwrap().id;
    h.apply(Operation::DeleteBodies { target: inside.clone(), bodies: vec![block], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.solid_of(&group).is_empty());
    assert!(p.bodies.iter().filter(|b| b.def == group.def).all(|b| p.is_failed(b.id)));
    assert!((volume(h.document(), part) - 200.0 * 50.0 * 18.0).abs() < 1e-3, "{}", volume(h.document(), part));
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap(), h.document().part(part).unwrap());
    // Undone in one step each.
    assert!(h.undo());
    assert!((volume(h.document(), part) - both).abs() < 1e-3);
    assert!(h.undo());
    assert!((volume(h.document(), part) - (both - 2.0 * std::f64::consts::PI * 4.0 * 20.0)).abs() < 1e-3);
}

#[test]
fn groups_nest_and_never_hold_themselves() {
    let (mut h, part, target) = board_and_drilled_block();
    let p = h.document().part(part).unwrap();
    let group = p.positives().find(|b| p.def_of(b).is_group()).unwrap().id;
    let inside = target.clone().in_group(vec![group]);
    let (_, g, _) = h.document().resolve_container(&inside).unwrap();
    let drill = g.negatives().next().unwrap().id;
    // The drill grouped inside the group: a compound machining two levels down.
    let out = h.apply(Operation::GroupBodies { target: inside.clone(), bodies: vec![drill], scope: Scope::All }).unwrap();
    let deeper = target.clone().in_group(vec![group, out.bodies[0]]);
    let (_, g2, _) = h.document().resolve_container(&deeper).unwrap();
    assert_eq!(g2.bodies.len(), 1);
    let hole = |length: f64| std::f64::consts::PI * 16.0 * length;
    let whole = 200.0 * 50.0 * 18.0 + 30.0 * 30.0 * 20.0;
    assert!((volume(h.document(), part) - (whole - hole(20.0))).abs() < 1e-3);
    // Edited two levels down: wider, every level computed again.
    let (_, g2, _) = h.document().resolve_container(&deeper).unwrap();
    let inner = g2.bodies[0].id;
    let mut disc = g2.profile_of(&g2.bodies[0]).cloned().unwrap();
    disc.profile = kernel::profile::with_radius(&disc.profile, 6.0).unwrap();
    h.apply(Operation::SetProfile { target: deeper.clone(), body: inner, profile: disc, scope: Scope::All }).unwrap();
    assert!((volume(h.document(), part) - (whole - std::f64::consts::PI * 36.0 * 20.0)).abs() < 1e-3, "{}", volume(h.document(), part));

    // The group copied, then pasted into itself: refused.
    let fragment = Arc::new(h.document().bodies_fragment(&target, &[group]).unwrap());
    let op = Operation::PasteBodies { target: inside.clone(), fragment, transform: DAffine3::IDENTITY, scope: Scope::All };
    assert!(matches!(h.apply(op), Err(Error::Cycle)));
    // A path through a leaf, or to nothing: refused.
    let blank = h.document().part(part).unwrap().bodies[0].id;
    let op = Operation::DeleteBodies { target: target.clone().in_group(vec![blank]), bodies: vec![drill], scope: Scope::All };
    assert!(h.apply(op).is_err());
}

#[test]
fn only_this_one_separates_the_part_before_editing_inside_its_group() {
    let (mut h, part, target) = board_and_drilled_block();
    let root = h.document().root();
    let twin = h
        .apply(Operation::Duplicate {
            context: root,
            entities: vec![target.entity],
            transform: DAffine3::from_translation(DVec3::Y * 200.0),
        })
        .unwrap()
        .created[0];
    let p = h.document().part(part).unwrap();
    let group = p.positives().find(|b| p.def_of(b).is_group()).unwrap().id;
    let inside = target.clone().in_group(vec![group]);
    let (_, g, _) = h.document().resolve_container(&inside).unwrap();
    let drill = g.negatives().next().unwrap().id;
    h.apply(Operation::DeleteBodies { target: inside.clone(), bodies: vec![drill], scope: Scope::This }).unwrap();
    let mine = h.document().resolve_part(&target).unwrap().0;
    assert_ne!(mine, part, "the part made its own");
    assert_eq!(h.document().resolve_part(&EntityPath::root(twin)).unwrap().0, part);
    let whole = 200.0 * 50.0 * 18.0 + 30.0 * 30.0 * 20.0;
    assert!((volume(h.document(), mine) - whole).abs() < 1e-3);
    assert!(volume(h.document(), part) < whole - 1.0);
}

#[test]
fn guides_move_with_bodies_inside_a_placed_group_in_its_axes() {
    let (mut h, part, target) = board_and_drilled_block();
    let p = h.document().part(part).unwrap();
    // The block's group, placed at the block's corner (50, 10, 18) in the part.
    let group = p.positives().find(|b| p.def_of(b).is_group()).unwrap().clone();
    assert!(close(group.placement.translation, DVec3::new(50.0, 10.0, 18.0)));
    let line = |origin: DVec3| GuideShape::Line { origin, dir: DVec3::X };
    let mut ids = Vec::new();
    for at in [DVec3::new(60.0, 20.0, 38.0), DVec3::new(0.0, 0.0, 18.0)] {
        ids.push(h.apply(Operation::AddGuide { component: part, shape: line(at) }).unwrap().guide.unwrap());
    }
    let inside = target.clone().in_group(vec![group.id]);
    let (_, g, _) = h.document().resolve_container(&inside).unwrap();
    let drill = g.negatives().next().unwrap().id;
    // A quarter turn about the group's Z, through its origin, the drill and the first guide.
    let turn = DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    let op = Operation::TransformBodies { target: inside.clone(), bodies: vec![drill, ids[0]], transform: turn, scope: Scope::All };
    h.apply(op).unwrap();
    let c = h.document().component(part).unwrap();
    // In the group's axes (10, 10, 20) → (-10, 10, 20): (40, 20, 38) in the part's, along Y.
    let expected = GuideShape::Line { origin: DVec3::new(40.0, 20.0, 38.0), dir: DVec3::Y };
    match (c.guide(ids[0]).unwrap().shape, expected) {
        (GuideShape::Line { origin, dir }, GuideShape::Line { origin: o, dir: d }) => {
            assert!(close(origin, o) && close(dir, d), "{origin} {dir}");
        }
        (s, _) => panic!("{s:?}"),
    }
    // The other one left exactly as it was.
    assert_eq!(c.guide(ids[1]).unwrap().shape, line(DVec3::new(0.0, 0.0, 18.0)));

    // Copied from inside the group: in its axes; pasted at the part's level, back where the
    // group puts it (the fragment's world, the group's).
    let fragment = h.document().bodies_fragment(&inside, &[ids[0]]).unwrap();
    match fragment.guides.iter().map(|g| g.shape).collect::<Vec<_>>()[..] {
        [GuideShape::Line { origin, .. }] => assert!(close(origin, DVec3::new(-10.0, 10.0, 20.0)), "{origin}"),
        _ => panic!("{:?}", fragment.guides),
    }
}

#[test]
fn bodies_inside_groups_weld_onto_bodies_anywhere_in_the_part() {
    let mut h = History::new(Document::new());
    let board = weld_board(&mut h);
    let target = EntityPath::root(board);
    // Two holes through, grouped: a compound machining (its children positive).
    let mut holes = Vec::new();
    for x in [100.0, 200.0] {
        let hole = Primitive::Cylinder { origin: DVec3::new(x, 100.0, -5.0), radius: 5.0, height: 30.0 };
        holes.push(add_body(&mut h, board, hole, Sign::Negative).bodies[0]);
    }
    let group = h.apply(Operation::GroupBodies { target: target.clone(), bodies: holes, scope: Scope::All }).unwrap().bodies[0];
    let inside = target.clone().in_group(vec![group]);
    // A third one drawn in the group, 1.4e-5 above the board's bottom (a body outside the group):
    // welded onto it, through.
    let hole = Primitive::Cylinder { origin: DVec3::new(300.0, 100.0, 1.4e-5), radius: 5.0, height: 18.0 - 1.4e-5 };
    let op = Operation::AddBody {
        target: inside.clone(),
        primitive: hole,
        placement: DAffine3::IDENTITY,
        sign: Sign::Positive,
        scope: Scope::All,
    };
    let out = h.apply(op).unwrap();
    let id = out.bodies[0];
    assert_eq!(out.welds.len(), 1, "{:?}", out.welds);
    assert_eq!(out.welds[0].body, id);
    let (_, g, _) = h.document().resolve_container(&inside).unwrap();
    let b = g.body(id).unwrap();
    let (lo, _) = g.solid_of(b).vertex_bounds().unwrap();
    assert!(b.placement.transform_point3(lo).z.abs() < 1e-12);
    // Through: the board's six faces and three holes' sides, no skin.
    assert_eq!(faces_of(&h, board), 9);

    // The group copied: a hair-short body drawn in it is never moved (it would be elsewhere in
    // each copy).
    let dy = DAffine3::from_translation(DVec3::Y * 100.0);
    h.apply(Operation::DuplicateBodies { target: target.clone(), bodies: vec![group], transform: dy, scope: Scope::All }).unwrap();
    let hole = Primitive::Cylinder { origin: DVec3::new(400.0, 100.0, 1.4e-5), radius: 5.0, height: 18.0 - 1.4e-5 };
    let op = Operation::AddBody {
        target: inside.clone(),
        primitive: hole,
        placement: DAffine3::IDENTITY,
        sign: Sign::Positive,
        scope: Scope::All,
    };
    assert!(h.apply(op).unwrap().welds.is_empty());
}

#[test]
fn body_definitions_take_a_name_their_copies_share() {
    let mut h = History::new(Document::new());
    let (target, part, hole) = drilled_board(&mut h);
    let def = h.document().part(part).unwrap().body(hole).unwrap().def;
    // The name given, None while automatic.
    let name = |h: &History, body: Id| {
        let p = h.document().part(part).unwrap();
        let d = p.def_of(p.body(body).unwrap());
        d.named.then(|| d.name.clone())
    };
    let auto = |h: &History, body: Id| {
        let p = h.document().part(part).unwrap();
        p.def_of(p.body(body).unwrap()).name.clone()
    };
    assert_eq!(name(&h, hole), None);
    assert_eq!(auto(&h, hole), "Extrusion#2");

    // Named, trimmed; the shape left alone: no kernel.
    let op = Operation::RenameBodyDef { part, def, name: " Trou de tourillon ".into() };
    assert!(matches!(h.document().prepare(&op).unwrap(), Prepared::Done(_)));
    h.apply(op).unwrap();
    assert_eq!(name(&h, hole).as_deref(), Some("Trou de tourillon"));

    // A copy shares it; made unique, keeps it; a name twice is fine.
    let dx = DAffine3::from_translation(DVec3::X * 40.0);
    let op = Operation::DuplicateBodies { target: target.clone(), bodies: vec![hole], transform: dx, scope: Scope::All };
    let copy = h.apply(op).unwrap().machinings()[0];
    assert_eq!(name(&h, copy).as_deref(), Some("Trou de tourillon"));
    h.apply(Operation::MakeBodiesUnique { target: target.clone(), bodies: vec![copy], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert_ne!(p.body(copy).unwrap().def, def);
    assert_eq!(name(&h, copy).as_deref(), Some("Trou de tourillon"));

    // Saved and read back.
    let back = Document::from_json(&h.document().to_json().unwrap()).unwrap();
    assert_eq!(back.part(part).unwrap(), h.document().part(part).unwrap());

    // Empty clears it: automatic again; an unknown definition or part is refused.
    h.apply(Operation::RenameBodyDef { part, def, name: "  ".into() }).unwrap();
    assert_eq!(name(&h, hole), None);
    assert_eq!(auto(&h, hole), "Extrusion#2");
    assert!(h.apply(Operation::RenameBodyDef { part, def: Id(999), name: "x".into() }).is_err());
    assert!(h.apply(Operation::RenameBodyDef { part: Id(999), def, name: "x".into() }).is_err());
}

#[test]
fn named_parts_name_the_bodies_they_become_and_back() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    let dowel = add(&mut h, Primitive::Cylinder { origin: DVec3::new(20.0, 25.0, 10.0), radius: 4.0, height: 30.0 });
    let target = EntityPath::root(board);
    let (part, _, _) = h.document().resolve_part(&target).unwrap();
    let (dowel_part, _, _) = h.document().resolve_part(&EntityPath::root(dowel)).unwrap();
    h.apply(Operation::RenameComponent { component: dowel_part, name: "Tourillon".into() }).unwrap();
    // The names given, None where automatic.
    let names = |h: &History| -> Vec<Option<String>> {
        let p = h.document().part(part).unwrap();
        p.bodies.iter().map(|b| p.def_of(b)).map(|d| d.named.then(|| d.name.clone())).collect()
    };

    // A part pasted into a part: the body it becomes takes its name, an unnamed one none.
    let other = add(&mut h, cube(DVec3::new(0.0, 0.0, 100.0), 10.0));
    let fragment = Arc::new(h.document().entities_fragment(&[], &[dowel, other]).unwrap());
    let (_, _, t) = h.document().resolve_part(&target).unwrap();
    let pasted = Operation::PasteParts { target: target.clone(), fragment, transform: t.inverse(), scope: Scope::All };
    h.apply(pasted).unwrap();
    assert_eq!(names(&h), vec![None, Some("Tourillon".into()), None]);

    // The union's tool too.
    h.apply(Operation::boolean(target.clone(), EntityPath::root(dowel), Boolean::Union, Scope::All)).unwrap();
    assert_eq!(names(&h)[3].as_deref(), Some("Tourillon"));

    // A named body pasted out of its part: a new part of its name (made unique).
    let body = h.document().part(part).unwrap().bodies[1].id;
    let fragment = Arc::new(h.document().bodies_fragment(&target, &[body]).unwrap());
    let out = h.apply(Operation::PasteAsPart { context: root, fragment, transform: DAffine3::IDENTITY }).unwrap();
    let (new, _, _) = h.document().resolve_part(&EntityPath::root(out.created[0])).unwrap();
    let new = h.document().component(new).unwrap();
    assert!(new.name.starts_with("Tourillon") && new.name != "Tourillon", "{}", new.name);
}

#[test]
fn bodies_take_names_of_their_own_as_occurrences_do() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let (target, part, hole) = drilled_board(&mut h);
    let name = |h: &History, body: Id| h.document().part(part).unwrap().body(body).unwrap().name.clone();
    assert_eq!(name(&h, hole), "");

    // Named, trimmed, no kernel; its copy keeps it, the definition's name untouched.
    let op = Operation::RenameBody { part, body: hole, name: " Haut ".into() };
    assert!(matches!(h.document().prepare(&op).unwrap(), Prepared::Done(_)));
    h.apply(op).unwrap();
    assert_eq!(name(&h, hole), "Haut");
    let dx = DAffine3::from_translation(DVec3::X * 40.0);
    let op = Operation::DuplicateBodies { target: target.clone(), bodies: vec![hole], transform: dx, scope: Scope::All };
    let copy = h.apply(op).unwrap().machinings()[0];
    assert_eq!(name(&h, copy), "Haut");
    let p = h.document().part(part).unwrap();
    assert!(!p.def_of(p.body(hole).unwrap()).named);
    h.apply(Operation::RenameBody { part, body: copy, name: "".into() }).unwrap();
    assert_eq!((name(&h, hole), name(&h, copy)), ("Haut".into(), "".into()));
    assert!(h.apply(Operation::RenameBody { part, body: Id(999), name: "x".into() }).is_err());

    // A single body pasted out as a part: its name the occurrence's.
    let fragment = Arc::new(h.document().bodies_fragment(&target, &[hole]).unwrap());
    let out = h.apply(Operation::PasteAsPart { context: root, fragment, transform: DAffine3::IDENTITY }).unwrap();
    let o = h.document().root_assembly().occurrence(out.created[0]).unwrap();
    assert_eq!(o.name, "Haut");
    let (_, p, _) = h.document().resolve_part(&EntityPath::root(o.id)).unwrap();
    assert_eq!(p.bodies[0].name, "");

    // And back: an occurrence's name, the body's it becomes (pasted into a part).
    let tool = o.id;
    let fragment = Arc::new(h.document().entities_fragment(&[], &[tool]).unwrap());
    let (_, _, t) = h.document().resolve_part(&target).unwrap();
    let pasted = Operation::PasteParts { target: target.clone(), fragment, transform: t.inverse(), scope: Scope::All };
    let bodies = h.apply(pasted).unwrap().bodies;
    assert_eq!(name(&h, bodies[0]), "Haut");
}

#[test]
fn automatic_names_are_numbered_by_nature_and_follow_it() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) });
    add(&mut h, Primitive::Box { origin: DVec3::Z * 100.0, size: DVec3::splat(10.0) });
    let target = EntityPath::root(board);
    let (part, _, _) = h.document().resolve_part(&target).unwrap();
    // Components from #1, as SketchUp's.
    let names: Vec<String> = h.document().components().filter(|c| c.id != h.document().root()).map(|c| c.name.clone()).collect();
    assert!(names.contains(&"Pièce#1".to_owned()) && names.contains(&"Pièce#2".to_owned()), "{names:?}");
    let defs = |h: &History| -> Vec<String> {
        let p = h.document().part(part).unwrap();
        p.bodies.iter().map(|b| p.def_of(b).name.clone()).collect()
    };
    assert_eq!(defs(&h), ["Extrusion#1"]);

    // A copy shares its definition's name; made unique, numbered anew.
    let b0 = h.document().part(part).unwrap().bodies[0].id;
    let dx = DAffine3::from_translation(DVec3::Y * 60.0);
    let op = Operation::DuplicateBodies { target: target.clone(), bodies: vec![b0], transform: dx, scope: Scope::All };
    let copy = h.apply(op).unwrap().machinings()[0];
    assert_eq!(defs(&h), ["Extrusion#1", "Extrusion#1"]);
    h.apply(Operation::MakeBodiesUnique { target: target.clone(), bodies: vec![copy], scope: Scope::All }).unwrap();
    assert_eq!(defs(&h), ["Extrusion#1", "Extrusion#2"]);

    // Its nature changed (no profile any more): renamed after it, a given name kept.
    let mut p = h.document().part(part).unwrap().clone();
    p.defs[1].profile = None;
    p.defs[0].set_name(Some("Plateau".into()));
    assert!(p.settle_names());
    assert_eq!(p.bodies.iter().map(|b| p.def_of(b).name.as_str()).collect::<Vec<_>>(), ["Plateau", "Solide#1"]);
    assert!(!p.settle_names());
    // An automatic name a given one took: renumbered.
    p.defs[0].set_name(Some("Solide#1".into()));
    p.settle_names();
    assert_eq!(p.def_of(&p.bodies[1]).name, "Solide#2");

    // A named component's copy: the next number of its base.
    assert_eq!(h.document().unique_name("Caisson#2", true), "Caisson#2");
    assert_eq!(h.document().unique_name("Pièce#1", true), "Pièce#3");
    assert_eq!(h.document().unique_name("Pièce#2", false), "Pièce#3");
}

#[test]
fn operations_at_a_path_reach_one_copy_when_made_unique_and_batch_in_one_step() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(20.0, 0.0, 0.0), 10.0));
    let g = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().created[0];
    // A second copy of the assembly, 100 further.
    let t = DAffine3::from_translation(DVec3::new(100.0, 0.0, 0.0));
    let g2 = h.apply(Operation::Duplicate { context: root, entities: vec![g], transform: t }).unwrap().created[0];
    let x = |h: &History, path: EntityPath| world_bounds(h.document(), &path).0.x;
    let up = DAffine3::from_translation(DVec3::new(0.0, 0.0, 50.0));

    // At the path, without making unique: the assembly's definition changes, both copies.
    let moved = |entities| Box::new(Operation::Transform { context: Id(0), entities, transform: up });
    h.apply(Operation::At { parent: vec![g], unique: false, op: moved(vec![a]) }).unwrap();
    let z = |h: &History, path: EntityPath| world_bounds(h.document(), &path).0.z;
    assert!((z(&h, EntityPath::new(vec![g], a)) - 50.0).abs() < 1e-9);
    assert!((z(&h, EntityPath::new(vec![g2], a)) - 50.0).abs() < 1e-9);
    h.undo();

    // Made unique: only the copy reached moves; one undo step, the batch too.
    let steps = h.revision();
    let batch = Operation::Batch(vec![
        Operation::At { parent: vec![g2], unique: true, op: moved(vec![a]) },
        Operation::At { parent: vec![g2], unique: true, op: moved(vec![b]) },
    ]);
    h.apply(batch).unwrap();
    assert!(z(&h, EntityPath::new(vec![g], a)).abs() < 1e-9);
    assert!((z(&h, EntityPath::new(vec![g2], a)) - 50.0).abs() < 1e-9);
    assert!((z(&h, EntityPath::new(vec![g2], b)) - 50.0).abs() < 1e-9);
    assert!((x(&h, EntityPath::new(vec![g2], b)) - 120.0).abs() < 1e-9);
    assert_ne!(occ(h.document(), &EntityPath::root(g)).component, occ(h.document(), &EntityPath::root(g2)).component);
    h.undo();
    assert_eq!(h.revision(), steps);
    assert!(z(&h, EntityPath::new(vec![g2], a)).abs() < 1e-9);

    // Only operations on occurrences of a context go at a path.
    let renamed = Box::new(Operation::RenameComponent { component: root, name: "x".into() });
    assert!(h.apply(Operation::At { parent: vec![g], unique: false, op: renamed }).is_err());
}

/// A boolean of several targets and tools (docs/tools/solid-tools.md).
fn booleans(targets: &[Id], tools: &[Id], op: Boolean, scope: Scope, keep: (bool, bool, bool)) -> Operation {
    Operation::Boolean {
        targets: targets.iter().map(|o| EntityPath::root(*o)).collect(),
        tools: tools.iter().map(|o| EntityPath::root(*o)).collect(),
        op,
        scope,
        keep_targets: keep.0,
        keep_tools: keep.1,
        drop_unused: keep.2,
    }
}

#[test]
fn a_difference_cuts_each_target_with_the_tools_reaching_it() {
    let mut h = History::new(Document::new());
    let left = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 100.0, 18.0) });
    let right = add(&mut h, Primitive::Box { origin: DVec3::new(200.0, 0.0, 0.0), size: DVec3::new(100.0, 100.0, 18.0) });
    let hole = |x: f64| Primitive::Cylinder { origin: DVec3::new(x, 50.0, -5.0), radius: 4.0, height: 30.0 };
    let tools = [add(&mut h, hole(20.0)), add(&mut h, hole(80.0)), add(&mut h, hole(250.0)), add(&mut h, hole(500.0))];
    h.apply(booleans(&[left, right], &tools, Boolean::Difference, Scope::All, (false, false, false))).unwrap();
    let doc = h.document();
    let machinings = |o: Id| doc.resolve_part(&EntityPath::root(o)).unwrap().1.machinings().len();
    assert_eq!((machinings(left), machinings(right)), (2, 1));
    // The tools that cut left; the one reaching nothing stays.
    let root: Vec<Id> = doc.root_assembly().occurrences().iter().map(|o| o.id).collect();
    assert_eq!(root, vec![left, right, tools[3]]);
    assert!(h.undo());
    assert_eq!(h.document().root_assembly().occurrences().len(), 6, "one step");
}

#[test]
fn a_union_takes_one_target_and_nothing_reached_is_refused() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let b = add(&mut h, cube(DVec3::new(5.0, 0.0, 0.0), 10.0));
    let c = add(&mut h, cube(DVec3::new(100.0, 0.0, 0.0), 10.0));
    let r = h.apply(booleans(&[a, b], &[c], Boolean::Union, Scope::All, (false, false, false)));
    assert!(matches!(r, Err(Error::Invalid(Refusal::UnionTargets))), "{r:?}");
    let r = h.apply(booleans(&[a], &[c], Boolean::Difference, Scope::All, (false, false, false)));
    assert!(matches!(r, Err(Error::Invalid(Refusal::NothingReached))), "{r:?}");
    let r = h.apply(booleans(&[a, b], &[b], Boolean::Difference, Scope::All, (false, false, false)));
    assert!(matches!(r, Err(Error::Invalid(Refusal::SelfBoolean))), "{r:?}");
    // A union joins every tool, reaching or not.
    h.apply(booleans(&[a], &[b, c], Boolean::Union, Scope::All, (false, false, false))).unwrap();
    let p = h.document().resolve_part(&EntityPath::root(a)).unwrap().1;
    assert_eq!(p.positives().count(), 3);
    assert_eq!(h.document().root_assembly().occurrences().len(), 1);
}

#[test]
fn kept_targets_and_tools_stay_as_they_were() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 100.0, 18.0) });
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(50.0, 50.0, -5.0), radius: 4.0, height: 30.0 });
    let part = occ(h.document(), &EntityPath::root(board)).component;
    let out = h.apply(booleans(&[board], &[drill], Boolean::Difference, Scope::All, (true, true, false))).unwrap();
    let doc = h.document();
    assert!(doc.part(part).unwrap().machinings().is_empty(), "the target left as it was");
    let result = out.created[0];
    assert_ne!(result, board);
    let (copy, p, _) = doc.resolve_part(&EntityPath::root(result)).unwrap();
    assert_ne!(copy, part);
    assert_eq!(p.machinings().len(), 1);
    assert_eq!(occ(doc, &EntityPath::root(result)).transform, occ(doc, &EntityPath::root(board)).transform);
    assert!(doc.resolve(&EntityPath::root(drill)).is_ok(), "the tool kept");
}

#[test]
fn unused_tool_parts_go_even_named() {
    let mut h = History::new(Document::new());
    let board = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 100.0, 18.0) });
    let drill = add(&mut h, Primitive::Cylinder { origin: DVec3::new(50.0, 50.0, -5.0), radius: 4.0, height: 30.0 });
    let tool = occ(h.document(), &EntityPath::root(drill)).component;
    h.apply(Operation::RenameComponent { component: tool, name: "Mèche".into() }).unwrap();
    h.apply(booleans(&[board], &[drill], Boolean::Difference, Scope::All, (false, false, false))).unwrap();
    assert!(h.document().component(tool).is_ok(), "named: kept in the model");
    assert!(h.undo());
    h.apply(booleans(&[board], &[drill], Boolean::Difference, Scope::All, (false, false, true))).unwrap();
    assert!(h.document().component(tool).is_err(), "dropped");
}

#[test]
fn targets_of_one_part_cut_alike_share_it_and_the_others_get_their_own() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 100.0, 18.0) });
    let step = DAffine3::from_translation(DVec3::X * 200.0);
    let copies = h.apply(Operation::Array { context: root, entities: vec![a], step, count: 3 }).unwrap().created;
    let (b, c, d) = (copies[0], copies[1], copies[2]);
    let part = occ(h.document(), &EntityPath::root(a)).component;
    // One hole at the same place in a and b, one elsewhere in c; d not targeted.
    let hole = |x: f64, y: f64| Primitive::Cylinder { origin: DVec3::new(x, y, -5.0), radius: 4.0, height: 30.0 };
    let tools = [add(&mut h, hole(50.0, 50.0)), add(&mut h, hole(250.0, 50.0)), add(&mut h, hole(420.0, 20.0))];
    h.apply(booleans(&[a, b, c], &tools, Boolean::Difference, Scope::All, (false, false, false))).unwrap();
    let doc = h.document();
    let of = |o: Id| doc.resolve_part(&EntityPath::root(o)).unwrap().0;
    assert_eq!((of(a), of(b), of(d)), (part, part, part), "the change most take, in place");
    assert_ne!(of(c), part);
    assert_eq!(doc.part(part).unwrap().machinings().len(), 1);

    // Only these: every placement of a part targeted alike changes it in place.
    assert!(h.undo());
    let all = [a, b, c, d];
    let holes: Vec<Id> = all.iter().map(|_| 0).enumerate().map(|(i, _)| add(&mut h, hole(50.0 + 200.0 * i as f64, 50.0))).collect();
    h.apply(booleans(&all, &holes, Boolean::Difference, Scope::This, (false, false, false))).unwrap();
    let doc = h.document();
    let parts: Vec<Id> = all.iter().map(|o| doc.resolve_part(&EntityPath::root(*o)).unwrap().0).collect();
    assert!(parts.iter().all(|p| *p == part), "{parts:?} {part:?}");
    assert_eq!(doc.part(part).unwrap().machinings().len(), 1);
}

#[test]
fn an_intersection_keeps_what_each_target_shares_with_the_tools() {
    let mut h = History::new(Document::new());
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let t1 = add(&mut h, cube(DVec3::new(-5.0, 0.0, 0.0), 10.0));
    let t2 = add(&mut h, cube(DVec3::new(8.0, 0.0, 0.0), 10.0));
    h.apply(booleans(&[a], &[t1, t2], Boolean::Intersection, Scope::All, (false, false, false))).unwrap();
    let (lo, hi) = world_bounds(h.document(), &EntityPath::root(a));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::splat(10.0)), "{lo} {hi}");
    // A ∩ (B1 ∪ B2): [0, 5] and [8, 10] along x, not A ∩ B1 ∩ B2 (nothing).
    let p = h.document().resolve_part(&EntityPath::root(a)).unwrap().1;
    assert_eq!(p.pieces(), 2);
}

/// A document with two parts A and B, an assembly holding A twice and B once, placed twice at
/// the root, and A once more at the root: (root occurrence of A, the assembly's two
/// occurrences, the assembly, part A, part B).
fn library() -> (History, Id, Vec<Id>, Id, Id, Id) {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let a2 = h
        .apply(Operation::Duplicate { context: root, entities: vec![a], transform: DAffine3::from_translation(DVec3::X * 20.0) })
        .unwrap()
        .created[0];
    let b = add(&mut h, cube(DVec3::new(0.0, 40.0, 0.0), 5.0));
    let g = h.apply(Operation::MakeAssembly { context: root, entities: vec![a, a2, b], name: None }).unwrap().created[0];
    let g2 = h
        .apply(Operation::Duplicate { context: root, entities: vec![g], transform: DAffine3::from_translation(DVec3::Y * 100.0) })
        .unwrap()
        .created[0];
    let doc = h.document();
    let asm = occ(doc, &EntityPath::root(g)).component;
    let (pa, pb) = (occ(doc, &EntityPath::new(vec![g], a)).component, occ(doc, &EntityPath::new(vec![g], b)).component);
    let a3 = h.apply(Operation::PlaceOccurrence { context: root, component: pa, transform: DAffine3::IDENTITY }).unwrap().created[0];
    (h, a3, vec![g, g2], asm, pa, pb)
}

#[test]
fn counts_in_the_model_multiply_down_the_assemblies() {
    let (mut h, _, _, asm, pa, pb) = library();
    let counts = h.document().counts_in_model();
    assert_eq!((counts[&asm], counts[&pa], counts[&pb]), (2, 5, 2), "A: 2 × 2 in the assemblies + 1 at the root");
    assert_eq!(counts[&h.document().root()], 1);

    // A part placed nowhere is not in the model, nor what an unused assembly holds.
    let root = h.document().root();
    h.apply(Operation::RenameComponent { component: asm, name: "Caisson".into() }).unwrap();
    let placed: Vec<Id> = h.document().root_assembly().occurrences().iter().filter(|o| o.component == asm).map(|o| o.id).collect();
    h.apply(Operation::Delete { context: root, entities: placed }).unwrap();
    let counts = h.document().counts_in_model();
    assert!(h.document().component(asm).is_ok(), "kept while unused");
    assert!(!counts.contains_key(&asm) && !counts.contains_key(&pb));
    assert_eq!(counts[&pa], 1, "the root's occurrence of A");
}

#[test]
fn purging_drops_what_the_model_does_not_place_the_named_too() {
    let (mut h, a3, _, asm, pa, pb) = library();
    let root = h.document().root();
    h.apply(Operation::RenameComponent { component: asm, name: "Caisson".into() }).unwrap();
    h.apply(Operation::RenameComponent { component: pb, name: "Fond".into() }).unwrap();
    let placed: Vec<Id> = h.document().root_assembly().occurrences().iter().filter(|o| o.component == asm).map(|o| o.id).collect();
    h.apply(Operation::Delete { context: root, entities: placed }).unwrap();
    assert!(h.document().component(asm).is_ok() && h.document().component(pb).is_ok());

    h.apply(Operation::PurgeUnused).unwrap();
    let doc = h.document();
    assert!(doc.component(asm).is_err() && doc.component(pb).is_err(), "the named assembly and the named part in it");
    assert!(doc.component(pa).is_ok(), "A is still placed at the root");
    assert_eq!(occ(doc, &EntityPath::root(a3)).component, pa);
    assert!(h.undo());
    assert!(h.document().component(asm).is_ok() && h.document().component(pb).is_ok());
}

#[test]
fn deleting_a_component_takes_all_its_occurrences() {
    let (mut h, a3, groups, asm, pa, pb) = library();
    let root = h.document().root();
    assert!(matches!(h.apply(Operation::DeleteComponent { component: root }), Err(Error::Invalid(Refusal::RootDeleted))));

    h.apply(Operation::DeleteComponent { component: pa }).unwrap();
    let doc = h.document();
    assert!(doc.component(pa).is_err());
    assert!(doc.resolve(&EntityPath::root(a3)).is_err(), "the root's occurrence too");
    assert_eq!(doc.assembly(asm).unwrap().occurrences().len(), 1, "B left in the assembly");
    assert_eq!(doc.counts_in_model()[&pb], 2);
    assert!(h.undo());
    assert_eq!(h.document().counts_in_model()[&pa], 5);

    // An assembly: its occurrences go, its parts stay (B unused now).
    h.apply(Operation::DeleteComponent { component: asm }).unwrap();
    let doc = h.document();
    assert!(groups.iter().all(|g| doc.resolve(&EntityPath::root(*g)).is_err()));
    assert!(doc.component(asm).is_err() && doc.component(pb).is_ok());
    assert!(!doc.counts_in_model().contains_key(&pb));
    assert_eq!(doc.counts_in_model()[&pa], 1);
}

#[test]
fn replacing_occurrences_keeps_their_placement_name_and_visibility() {
    let (mut h, a3, groups, asm, pa, pb) = library();
    let root = h.document().root();
    h.apply(Operation::RenameOccurrence { context: root, occurrence: a3, name: "Gauche".into() }).unwrap();
    h.apply(Operation::SetHidden { context: root, entities: vec![a3], hidden: true }).unwrap();
    let before = occ(h.document(), &EntityPath::root(a3));

    h.apply(Operation::ReplaceOccurrences { context: root, entities: vec![a3], component: pb }).unwrap();
    let after = occ(h.document(), &EntityPath::root(a3));
    assert_eq!(after.component, pb);
    assert_eq!(Occurrence { component: before.component, ..after }, before, "all but its component kept");
    assert!(h.document().component(pa).is_ok(), "A still in the assemblies");

    // No assembly into itself; the model is not placed.
    let b_in_g = h.document().assembly(asm).unwrap().occurrences().iter().find(|o| o.component == pb).unwrap().id;
    assert!(matches!(h.apply(Operation::ReplaceOccurrences { context: asm, entities: vec![b_in_g], component: asm }), Err(Error::Cycle)));
    assert!(h.apply(Operation::ReplaceOccurrences { context: root, entities: vec![a3], component: root }).is_err());

    // At depth, through the assembly placed twice: both copies change (`unique` off), and the
    // part A left unused stays.
    let in_g: Vec<Id> = h.document().assembly(asm).unwrap().occurrences().iter().filter(|o| o.component == pa).map(|o| o.id).collect();
    let op = Operation::ReplaceOccurrences { context: root, entities: in_g, component: pb };
    h.apply(Operation::At { parent: vec![groups[0]], unique: false, op: Box::new(op) }).unwrap();
    let doc = h.document();
    assert!(doc.component(pa).is_ok() && !doc.counts_in_model().contains_key(&pa), "A unused, kept");
    assert_eq!(doc.counts_in_model()[&pb], 7, "3 in each assembly + the root's");
}

// ---- properties (docs/model/properties.md) -------------------------------------------------------------

fn set_props(target: PropTarget, props: &[(&str, Option<Value>)]) -> Operation {
    Operation::SetProps { target, props: props.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

#[test]
fn properties_are_typed_kept_by_copies_and_saved() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let part = occ(h.document(), &EntityPath::root(a)).component;
    let oak = h
        .apply(Operation::AddMaterial { name: "Chêne".into(), color: [200, 160, 100], kind: Default::default() })
        .unwrap()
        .material
        .unwrap();
    let guide = h
        .apply(Operation::AddGuide { component: root, shape: GuideShape::Line { origin: DVec3::ONE, dir: DVec3::X } })
        .unwrap()
        .guide
        .unwrap();
    let (def, body) = {
        let p = h.document().part(part).unwrap();
        (p.defs[0].id, p.bodies[0].id)
    };
    let price = Value::Money { amount: 12.5, currency: "EUR".into() };
    let targets = [
        (PropTarget::Component(root), EntityKind::Model),
        (PropTarget::Component(part), EntityKind::Part),
        (PropTarget::Occurrence { context: root, occurrence: a }, EntityKind::Occurrence),
        (PropTarget::BodyDef { part, def }, EntityKind::BodyDef),
        (PropTarget::Body { part, body }, EntityKind::Body),
        (PropTarget::Material(oak), EntityKind::Material),
        (PropTarget::Guide { component: root, guide }, EntityKind::Guide),
    ];
    for (target, kind) in &targets {
        h.apply(set_props(target.clone(), &[("user.prix", Some(price.clone())), ("user.note", Some(Value::Text("à poncer".into())))]))
            .unwrap();
        assert_eq!(h.document().prop_kind(target).unwrap(), *kind);
        assert_eq!(h.document().props(target).unwrap().get("user.prix"), Some(&price));
    }
    // Removed, kept apart.
    let occurrence = PropTarget::Occurrence { context: root, occurrence: a };
    h.apply(set_props(occurrence.clone(), &[("user.note", None)])).unwrap();
    assert_eq!(h.document().props(&occurrence).unwrap().len(), 1);
    // Changes in a row to the same keys undo in one go.
    let thickness = |t: f64| set_props(PropTarget::Component(part), &[("user.epaisseur", Some(Value::Length(t)))]);
    h.apply(thickness(18.0)).unwrap();
    h.apply(thickness(19.0)).unwrap();
    h.undo();
    assert!(h.document().props(&PropTarget::Component(part)).unwrap().get("user.epaisseur").is_none());

    // A copy keeps its occurrence's and its part's; made unique, the part's go along.
    let copy = h
        .apply(Operation::Duplicate { context: root, entities: vec![a], transform: DAffine3::from_translation(DVec3::X * 50.0) })
        .unwrap()
        .created[0];
    let copy_target = PropTarget::Occurrence { context: root, occurrence: copy };
    assert_eq!(h.document().props(&copy_target).unwrap(), h.document().props(&occurrence).unwrap());
    h.apply(Operation::MakeUnique { context: root, occurrence: copy }).unwrap();
    let unique = occ(h.document(), &EntityPath::root(copy)).component;
    assert_ne!(unique, part);
    assert_eq!(h.document().props(&PropTarget::Component(unique)).unwrap().get("user.prix"), Some(&price));

    // Saved and read again, each value with its kind.
    let doc = h.document();
    let file = doc.to_file().unwrap();
    assert_eq!(Document::from_file(&file).unwrap(), *doc);
    let document = String::from_utf8(unzip(&file)["document.json"].clone()).unwrap();
    assert!(document.contains(r#""user.prix": {"#) && document.contains(r#""currency": "EUR""#), "{document}");

    // Refused: a key without its namespace, a number not finite, an unknown entity.
    let bad = |op| matches!(h.document().prepare(&op), Err(Error::Invalid(Refusal::BadPropKey | Refusal::BadPropValue)));
    assert!(bad(set_props(PropTarget::Component(part), &[("prix", Some(Value::Int(1)))])));
    assert!(bad(set_props(PropTarget::Component(part), &[("User.prix", Some(Value::Int(1)))])));
    assert!(bad(set_props(PropTarget::Component(part), &[("user.long", Some(Value::Length(f64::INFINITY)))])));
    assert!(bad(set_props(PropTarget::Component(part), &[("user.l", Some(Value::List(vec![Value::Int(1), Value::Text("a".into())])))])));
    assert!(h.document().prepare(&set_props(PropTarget::Material(Id(999)), &[("user.x", Some(Value::Int(1)))])).is_err());
}

#[test]
fn property_definitions_check_their_values() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let a = add(&mut h, cube(DVec3::ZERO, 10.0));
    let part = PropTarget::Component(occ(h.document(), &EntityPath::root(a)).component);
    let occurrence = PropTarget::Occurrence { context: root, occurrence: a };
    let species = PropDef {
        label: "Essence".into(),
        choices: vec!["Chêne".into(), "Hêtre".into()],
        applies_to: [EntityKind::Part, EntityKind::Material].into(),
        ..PropDef::new(ValueKind::Choice)
    };
    let define = |def: Option<PropDef>| Operation::SetPropDef { key: "user.essence".into(), def };
    h.apply(define(Some(species.clone()))).unwrap();
    let choice = |c: &str| Some(Value::Choice(c.into()));
    h.apply(set_props(part.clone(), &[("user.essence", choice("Chêne"))])).unwrap();
    let refused = |h: &History, op: Operation, r: Refusal| matches!(h.document().prepare(&op), Err(Error::Invalid(x)) if x == r);
    // Not one of its choices, another kind, an entity it does not apply to.
    assert!(refused(&h, set_props(part.clone(), &[("user.essence", choice("Pin"))]), Refusal::BadPropValue));
    assert!(refused(&h, set_props(part.clone(), &[("user.essence", Some(Value::Text("Chêne".into())))]), Refusal::BadPropValue));
    assert!(refused(&h, set_props(occurrence.clone(), &[("user.essence", choice("Chêne"))]), Refusal::BadPropValue));
    // A definition its values do not fit, or not well formed.
    assert!(refused(&h, define(Some(PropDef::new(ValueKind::Length))), Refusal::PropDefContradicted));
    let no_choices = PropDef { choices: Vec::new(), ..species.clone() };
    assert!(refused(&h, define(Some(no_choices)), Refusal::BadPropDef));
    let bad_default = PropDef { default: choice("Pin"), ..species.clone() };
    assert!(refused(&h, define(Some(bad_default)), Refusal::BadPropDef));
    assert!(matches!(
        h.document().prepare(&Operation::SetPropDef { key: "essence".into(), def: Some(species.clone()) }),
        Err(Error::Invalid(Refusal::BadPropKey))
    ));
    // A list of its choices.
    let several = PropDef { kind: ValueKind::List(Box::new(ValueKind::Choice)), applies_to: Default::default(), ..species.clone() };
    h.apply(Operation::SetPropDef { key: "user.essences".into(), def: Some(several) }).unwrap();
    let list = |l: &[&str]| Some(Value::List(l.iter().map(|c| Value::Choice(c.to_string())).collect()));
    h.apply(set_props(occurrence.clone(), &[("user.essences", list(&["Chêne", "Hêtre"]))])).unwrap();
    assert!(refused(&h, set_props(occurrence.clone(), &[("user.essences", list(&["Chêne", "Pin"]))]), Refusal::BadPropValue));

    // A file whose value contradicts its definition is refused.
    let json = h.document().to_json().unwrap();
    assert_eq!(Document::from_json(&json).unwrap(), *h.document());
    let tampered = json.replacen(r#"{"choice":"Chêne"}"#, r#"{"choice":"Pin"}"#, 1);
    assert_ne!(tampered, json);
    assert!(matches!(Document::from_json(&tampered), Err(Error::File(FileError::BadProp(k))) if k.starts_with("user.essence")));

    // Its definition removed, its values stay, any kind welcome again.
    h.apply(define(None)).unwrap();
    assert_eq!(h.document().props(&part).unwrap().get("user.essence"), choice("Chêne").as_ref());
    h.apply(set_props(part, &[("user.essence", Some(Value::Text("Pin".into())))])).unwrap();
}

#[test]
fn pasting_renames_properties_defined_otherwise() {
    // The source: « user.x » a length, on a part and its occurrence.
    let mut src = History::new(Document::new());
    let root = src.document().root();
    let a = add(&mut src, cube(DVec3::ZERO, 10.0));
    let part = occ(src.document(), &EntityPath::root(a)).component;
    src.apply(Operation::SetPropDef { key: "user.x".into(), def: Some(PropDef::new(ValueKind::Length)) }).unwrap();
    src.apply(set_props(PropTarget::Component(part), &[("user.x", Some(Value::Length(18.0))), ("user.raw", Some(Value::Int(3)))])).unwrap();
    let fragment = Arc::new(src.document().entities_fragment(&[], &[a]).unwrap());

    // Pasted where it was copied from: nothing renamed.
    let outcome = paste(&mut src, root, &fragment, DAffine3::IDENTITY, true);
    assert_eq!(outcome.warning, None);

    // Elsewhere, « user.x » a text and « user.raw » a length: both renamed, their definitions with them.
    let mut dst = History::new(Document::new());
    let root = dst.document().root();
    for (key, kind) in [("user.x", ValueKind::Text), ("user.raw", ValueKind::Length)] {
        dst.apply(Operation::SetPropDef { key: key.into(), def: Some(PropDef::new(kind)) }).unwrap();
    }
    let outcome = paste(&mut dst, root, &fragment, DAffine3::IDENTITY, false);
    assert_eq!(
        outcome.warning,
        Some(Warning::PropsRenamed(vec![("user.raw".into(), "user.raw_2".into()), ("user.x".into(), "user.x_2".into())]))
    );
    let pasted = occ(dst.document(), &EntityPath::root(outcome.created[0])).component;
    let props = dst.document().props(&PropTarget::Component(pasted)).unwrap();
    assert_eq!(props.get("user.x_2"), Some(&Value::Length(18.0)));
    assert_eq!(props.get("user.raw_2"), Some(&Value::Int(3)));
    assert_eq!(dst.document().prop_defs()["user.x_2"].kind, ValueKind::Length);
    assert_eq!(dst.document().prop_defs()["user.x"].kind, ValueKind::Text);
    assert!(!dst.document().prop_defs().contains_key("user.raw_2"));
    // What it reads checks.
    let doc = dst.document();
    assert_eq!(Document::from_file(&doc.to_file().unwrap()).unwrap(), *doc);
}

// ---- textures and materials files (docs/render/look.md#textures, docs/io/file-format.md) -------------

/// Bytes a PNG starts with, then `seed`: an image as far as the model goes (it does not decode).
fn png(seed: u8) -> Asset {
    Asset::new([&b"\x89PNG\r\n\x1a\n"[..], &[seed; 64]].concat()).unwrap()
}

fn texture(image: &Asset) -> Texture {
    Texture { image: image.id(), size: [600.0, 300.0], rotation: 0.0 }
}

#[test]
fn a_textured_material_takes_its_image_mean_colour_and_keeps_it() {
    let mut h = History::new(Document::new());
    let oak = h
        .apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90], kind: Some(MaterialKind::Solid) })
        .unwrap()
        .material
        .unwrap();
    let image = png(1);
    let set =
        Operation::SetTexture { material: oak, texture: Some(texture(&image)), image: Some(image.clone()), color: Some([10, 20, 30]) };
    h.apply(set).unwrap();
    assert_eq!(h.document().material(oak).unwrap().color, [10, 20, 30], "the image's mean");
    // Renamed: its colour not edited while it has a texture.
    h.apply(Operation::EditMaterial { material: oak, name: "Chêne clair".into(), color: [255, 0, 0] }).unwrap();
    let m = h.document().material(oak).unwrap();
    assert_eq!((m.name.as_str(), m.color), ("Chêne clair", [10, 20, 30]));
    // Its texture taken off: the mean stays, edited again.
    h.apply(Operation::SetTexture { material: oak, texture: None, image: None, color: Some([1, 1, 1]) }).unwrap();
    assert_eq!(h.document().material(oak).unwrap().color, [10, 20, 30]);
    h.apply(Operation::EditMaterial { material: oak, name: "Chêne clair".into(), color: [255, 0, 0] }).unwrap();
    assert_eq!(h.document().material(oak).unwrap().color, [255, 0, 0]);
}

#[test]
fn textures_are_stored_once_and_saved() {
    let mut h = History::new(Document::new());
    let oak = h
        .apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90], kind: Some(MaterialKind::Solid) })
        .unwrap()
        .material
        .unwrap();
    let walnut = h
        .apply(Operation::AddMaterial { name: "Noyer".into(), color: [90, 60, 40], kind: Some(MaterialKind::Solid) })
        .unwrap()
        .material
        .unwrap();
    let image = png(1);
    h.apply(Operation::SetTexture { material: oak, texture: Some(texture(&image)), image: Some(image.clone()), color: None }).unwrap();
    // The same image for another: no bytes again.
    h.apply(Operation::SetTexture { material: walnut, texture: Some(texture(&image)), image: None, color: None }).unwrap();
    assert_eq!(h.document().asset(&image.id()), Some(&image));

    let doc = h.document();
    let file = doc.to_file().unwrap();
    assert_eq!(Document::from_file(&file).unwrap(), *doc);
    let entries = unzip(&file);
    let images: Vec<_> = entries.keys().filter(|k| k.starts_with("textures/")).collect();
    assert_eq!(images, [&format!("textures/{}.png", image.id())]);
    assert_eq!(&entries[images[0]][..], &image.bytes[..]);
    let document = String::from_utf8(entries["document.json"].clone()).unwrap();
    assert!(document.contains(&format!(r#""{}": "png""#, image.id())), "{document}");
    // Inline in the one JSON.
    assert_eq!(Document::from_json(&doc.to_json().unwrap()).unwrap(), *doc);
    // Its image missing, refused.
    let mut without = entries.clone();
    without.remove(images[0]);
    assert!(Document::from_file(&rezip(&without)).is_err());

    // Refused: an image the document does not have, a size not positive.
    let other = png(2);
    let refused = |op| matches!(h.document().prepare(&op), Err(Error::Invalid(Refusal::BadTexture)));
    assert!(refused(Operation::SetTexture { material: oak, texture: Some(texture(&other)), image: None, color: None }));
    let flat = Texture { size: [0.0, 300.0], ..texture(&image) };
    assert!(refused(Operation::SetTexture { material: oak, texture: Some(flat), image: None, color: None }));
    assert!(Asset::new(b"GIF89a....".to_vec()).is_none());
    assert_eq!(Asset::new(b"\xFF\xD8\xFF\xE0rest".to_vec()).unwrap().kind, ImageKind::Jpeg);

    // Used by none, the image goes.
    h.apply(Operation::SetTexture { material: oak, texture: None, image: None, color: None }).unwrap();
    assert!(h.document().asset(&image.id()).is_some());
    h.apply(Operation::DeleteMaterial { material: walnut }).unwrap();
    assert!(h.document().asset(&image.id()).is_none());
    assert!(!unzip(&h.document().to_file().unwrap()).keys().any(|k| k.starts_with("textures/")));
}

#[test]
fn a_coloration_has_no_texture() {
    let mut h = History::new(Document::new());
    let add = |h: &mut History, kind| {
        h.apply(Operation::AddMaterial { name: "Teinte".into(), color: [120, 80, 40], kind }).unwrap().material.unwrap()
    };
    let coat = add(&mut h, Some(MaterialKind::Coat));
    let image = png(1);
    let set = |material| Operation::SetTexture { material, texture: Some(texture(&image)), image: Some(image.clone()), color: None };
    assert!(matches!(h.document().prepare(&set(coat)), Err(Error::Invalid(Refusal::ColorationTexture))));
    // Taking none off is no change, and allowed.
    h.apply(Operation::SetTexture { material: coat, texture: None, image: None, color: None }).unwrap();

    // A file's coloration with a texture is read without it, its image dropped.
    let mut h = History::new(Document::new());
    let oak = add(&mut h, Some(MaterialKind::Solid));
    h.apply(set(oak)).unwrap();
    let json = h.document().to_json().unwrap();
    let key = r#""materials":{"#;
    assert_eq!(json.matches(key).count(), 1, "{json}");
    let read = Document::from_json(&json.replace(key, r#""colorations":{"#)).unwrap();
    let m = read.material(oak).unwrap();
    assert_eq!((m.family(), m.texture.as_ref(), m.color), (Family::Coloration, None, [120, 80, 40]));
    assert!(read.asset(&image.id()).is_none());
}

#[test]
fn materials_files_export_and_import() {
    let mut src = History::new(Document::new());
    let oak = src
        .apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90], kind: Some(MaterialKind::Solid) })
        .unwrap()
        .material
        .unwrap();
    src.apply(Operation::AddMaterial { name: "Noyer".into(), color: [90, 60, 40], kind: Some(MaterialKind::Solid) }).unwrap();
    let image = png(7);
    src.apply(Operation::SetTexture { material: oak, texture: Some(texture(&image)), image: Some(image.clone()), color: None }).unwrap();
    src.apply(Operation::SetPropDef { key: "user.densite".into(), def: Some(PropDef::new(ValueKind::Real)) }).unwrap();
    src.apply(set_props(PropTarget::Material(oak), &[("user.densite", Some(Value::Real(0.7)))])).unwrap();
    let a = add(&mut src, cube(DVec3::ZERO, 10.0));
    src.apply(Operation::PaintParts {
        targets: vec![EntityPath::root(a)],
        layer: Family::Material,
        material: Some(oak),
        scope: Scope::All,
    })
    .unwrap();

    // Oak alone: a document without geometry, its image and the definitions with it.
    let file = src.document().materials_file(&[oak]).unwrap();
    let (mats, content) = Document::read_file(&file).unwrap();
    assert_eq!(content, Content::Materials);
    assert_eq!(mats.materials().map(|(_, m)| m.name.as_str()).collect::<Vec<_>>(), ["Chêne"]);
    assert_eq!(mats.asset(&image.id()), Some(&image));
    assert!(mats.prop_defs().contains_key("user.densite"));
    assert_eq!(mats.components().count(), 0, "no component but its root");
    assert!(matches!(Document::from_file(&file), Err(Error::File(FileError::MaterialsOnly))));
    assert!(src.document().materials_file(&[Id(999)]).is_err());

    // Imported elsewhere: the material, its texture, its image, its properties; once.
    let mut dst = History::new(Document::new());
    let fragment = Arc::new(mats);
    dst.apply(Operation::ImportMaterials { fragment: fragment.clone() }).unwrap();
    dst.apply(Operation::ImportMaterials { fragment: fragment.clone() }).unwrap();
    let imported: Vec<_> = dst.document().materials().map(|(_, m)| m.clone()).collect();
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].texture, Some(texture(&image)));
    assert_eq!(imported[0].props.get("user.densite"), Some(&Value::Real(0.7)));
    assert_eq!(dst.document().asset(&image.id()), Some(&image));
    let doc = dst.document();
    assert_eq!(Document::from_file(&doc.to_file().unwrap()).unwrap(), *doc);
    assert!(dst.document().prepare(&Operation::ImportMaterials { fragment: Arc::new(Document::new()) }).is_err());

    // A part painted with it, pasted into another document: the image goes along.
    let copied = Arc::new(src.document().entities_fragment(&[], &[a]).unwrap());
    assert!(copied.asset(&image.id()).is_some());
    let mut other = History::new(Document::new());
    let root = other.document().root();
    paste(&mut other, root, &copied, DAffine3::IDENTITY, false);
    assert_eq!(other.document().asset(&image.id()), Some(&image));
    let doc = other.document();
    assert_eq!(Document::from_file(&doc.to_file().unwrap()).unwrap(), *doc);
}

#[test]
fn materials_are_counted_per_placed_part_and_purged_when_nothing_refers_to_them() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let add = |h: &mut History, name: &str| {
        h.apply(Operation::AddMaterial { name: name.into(), color: [1, 2, 3], kind: Default::default() }).unwrap();
        h.document().materials().find(|(_, m)| m.name == name).unwrap().0
    };
    let (oak, ash, free) = (add(&mut h, "oak"), add(&mut h, "ash"), add(&mut h, "free"));
    let primitive = Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(10.0) };
    let part =
        h.apply(Operation::AddPrimitive { context: root, primitive, placement: DAffine3::IDENTITY, material: Some(oak) }).unwrap().created
            [0];
    let assembly = h.apply(Operation::MakeAssembly { context: root, entities: vec![part], name: Some("A".into()) }).unwrap().created[0];
    let definition = h.document().root_assembly().occurrence(assembly).unwrap().component;
    h.apply(Operation::PlaceOccurrence { context: root, component: definition, transform: DAffine3::IDENTITY }).unwrap();
    let counts = h.document().material_counts();
    assert_eq!(counts.get(&oak), Some(&2), "one part, through two assemblies");
    assert!(!counts.contains_key(&ash) && !counts.contains_key(&free));
    h.apply(Operation::PurgeMaterials).unwrap();
    assert!(h.document().material(oak).is_ok(), "referred to: kept");
    assert!(h.document().material(ash).is_err() && h.document().material(free).is_err());
}

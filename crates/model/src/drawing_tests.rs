//! The drawing's sheets, layers and views (docs/ui/drawing.md).

use glam::{DAffine3, DVec2, DVec3};

use crate::*;

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-12
}

fn sheet(h: &mut History) -> Id {
    h.apply(Operation::AddSheet { name: None, paper: Paper::A3 }).unwrap().sheet.unwrap()
}

fn base(look: Look) -> ElementKind {
    ElementKind::View(View {
        name: "Face".into(),
        kind: ViewKind::Base { subject: Subject::Model, look, scale: 0.1, at: DVec2::new(100.0, 150.0) },
        style: ViewStyle::Lines,
    })
}

fn projected(base: Id, side: Side) -> ElementKind {
    ElementKind::View(View { name: "P".into(), kind: ViewKind::Projected { base, side, distance: 80.0 }, style: ViewStyle::Lines })
}

fn add(h: &mut History, sheet: Id, element: ElementKind) -> Result<Id> {
    h.apply(Operation::AddElement { at: Placing { sheet, layer: None }, element }).map(|o| o.element.unwrap())
}

#[test]
fn a_first_sheet_comes_with_its_layers_and_a_name() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let d = h.document().drawing();
    assert_eq!(d.sheets.len(), 1);
    assert_eq!(d.sheets[0].id, s);
    assert_eq!(d.sheets[0].paper, Paper::A3);
    assert!(!d.sheets[0].name.is_empty());
    // The drawing's own, then the title block's, shared.
    assert_eq!(d.layers.len(), 2);
    assert!(!d.layers[0].shared && d.layers[1].shared);
    let s2 = sheet(&mut h);
    assert_eq!(h.document().drawing().layers.len(), 2, "layers made once");
    assert_ne!(h.document().drawing().sheets[0].name, h.document().drawing().sheets[1].name);
    h.apply(Operation::MoveSheet { sheet: s2, index: 0 }).unwrap();
    assert_eq!(h.document().drawing().sheets[0].id, s2);
    assert!(h.apply(Operation::SetSheet { sheet: s, name: "Plan".into(), paper: Paper { width: 0.0, height: 10.0 }, grid: 0.0 }).is_err());
    assert!(h.apply(Operation::SetSheet { sheet: s, name: "Plan".into(), paper: Paper::A4, grid: -1.0 }).is_err());
    h.apply(Operation::SetSheet { sheet: s, name: "Plan".into(), paper: Paper::A4, grid: 5.0 }).unwrap();
    assert_eq!(h.document().drawing().sheet(s).unwrap().grid, 5.0);
    assert_eq!(h.document().drawing().sheet(s).unwrap().name, "Plan");
    // Undone as the rest of the document.
    assert!(h.undo() && h.undo() && h.undo());
    assert_eq!(h.document().drawing().sheets.len(), 1);
    assert!(h.undo());
    assert!(h.document().drawing().is_empty());
}

#[test]
fn projected_views_turn_and_follow_their_base() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let front = add(&mut h, s, base(Look::Standard(Direction::Front))).unwrap();
    let right = add(&mut h, s, projected(front, Side::Right)).unwrap();
    let above = add(&mut h, s, projected(front, Side::Above)).unwrap();
    let d = h.document().drawing();
    let f = d.resolve(front).unwrap();
    assert!(close(f.frame.forward, DVec3::Y) && close(f.frame.up, DVec3::Z) && close(f.frame.right, DVec3::X));
    // First-angle (European): the view placed right is seen from the left, the one above
    // from below; the front face beside the front view.
    let r = d.resolve(right).unwrap();
    assert!(close(r.frame.forward, DVec3::X) && close(r.frame.up, DVec3::Z), "{:?}", r.frame);
    assert!(close(r.frame.right, -DVec3::Y), "the front face on its left, by the front view");
    assert_eq!(r.at, DVec2::new(180.0, 150.0));
    assert_eq!(r.scale, 0.1);
    let a = d.resolve(above).unwrap();
    assert!(close(a.frame.forward, DVec3::Z) && close(a.frame.up, DVec3::Y), "{:?}", a.frame);
    assert_eq!(a.at, DVec2::new(100.0, 230.0));
    // Third-angle: the view placed right is seen from the right.
    h.apply(Operation::SetProjection(Projection::ThirdAngle)).unwrap();
    let d = h.document().drawing();
    let r = d.resolve(right).unwrap();
    assert!(close(r.frame.forward, -DVec3::X) && close(r.frame.right, DVec3::Y), "{:?}", r.frame);
    let a = d.resolve(above).unwrap();
    assert!(close(a.frame.forward, -DVec3::Z) && close(a.frame.up, DVec3::Y), "the top view, the front face down");
    // A projection of a projection: the right view's own right.
    let back = add(&mut h, s, projected(right, Side::Right)).unwrap();
    let b = h.document().drawing().resolve(back).unwrap();
    assert!(close(b.frame.forward, -DVec3::Y), "the back view: {:?}", b.frame);
    assert_eq!(b.at, DVec2::new(260.0, 150.0));
    // Moving the base moves its projections.
    let mut moved = base(Look::Standard(Direction::Front));
    if let ElementKind::View(View { kind: ViewKind::Base { at, .. }, .. }) = &mut moved {
        *at = DVec2::new(50.0, 50.0);
    }
    h.apply(Operation::SetElement { element: front, kind: moved }).unwrap();
    assert_eq!(h.document().drawing().resolve(back).unwrap().at, DVec2::new(210.0, 50.0));
}

#[test]
fn views_refuse_loops_perspective_bases_and_bad_values() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let front = add(&mut h, s, base(Look::Standard(Direction::Front))).unwrap();
    let right = add(&mut h, s, projected(front, Side::Right)).unwrap();
    // The base made a projection of its own projection, a view projected from itself.
    let err = h.apply(Operation::SetElement { element: front, kind: projected(right, Side::Left) }).unwrap_err();
    assert!(matches!(err, Error::Invalid(Refusal::ProjectionLoop)), "projected from its own projection");
    let err = h.apply(Operation::SetElement { element: right, kind: projected(right, Side::Left) }).unwrap_err();
    assert!(matches!(err, Error::Invalid(Refusal::ProjectionLoop)));
    let persp = Look::Camera {
        forward: DVec3::new(1.0, 1.0, -0.5),
        up: DVec3::Z,
        perspective: Some(Perspective { eye: DVec3::new(-1000.0, -1000.0, 800.0), fov_y: 0.6 }),
    };
    let p = add(&mut h, s, base(persp)).unwrap();
    assert!(matches!(add(&mut h, s, projected(p, Side::Right)).unwrap_err(), Error::Invalid(Refusal::ProjectedFromPerspective)));
    assert!(matches!(add(&mut h, s, projected(Id(9999), Side::Right)).unwrap_err(), Error::NotFound(Id(9999))));
    let mut zero = base(Look::Standard(Direction::Top));
    if let ElementKind::View(View { kind: ViewKind::Base { scale, .. }, .. }) = &mut zero {
        *scale = 0.0;
    }
    assert!(matches!(add(&mut h, s, zero).unwrap_err(), Error::Invalid(Refusal::BadElement)));
    let gone = ElementKind::View(View {
        name: "x".into(),
        kind: ViewKind::Base { subject: Subject::Component(Id(4242)), look: Look::Standard(Direction::Top), scale: 1.0, at: DVec2::ZERO },
        style: ViewStyle::Rendered,
    });
    assert!(matches!(add(&mut h, s, gone).unwrap_err(), Error::Invalid(Refusal::BadElement)));
}

#[test]
fn deleting_a_view_or_a_sheet_takes_its_projections() {
    let mut h = History::new(Document::new());
    let s1 = sheet(&mut h);
    let s2 = sheet(&mut h);
    let front = add(&mut h, s1, base(Look::Standard(Direction::Front))).unwrap();
    let right = add(&mut h, s1, projected(front, Side::Right)).unwrap();
    // On another sheet, projected from the first's.
    let other = add(&mut h, s2, projected(right, Side::Below)).unwrap();
    let text = add(&mut h, s2, ElementKind::Text { at: DVec2::new(10.0, 10.0), text: "{title}".into(), size: 3.5 }).unwrap();
    h.apply(Operation::DeleteElements { elements: vec![right] }).unwrap();
    let d = h.document().drawing();
    assert!(d.element(right).is_none() && d.element(other).is_none());
    assert!(d.element(front).is_some() && d.element(text).is_some());
    h.undo();
    h.apply(Operation::DeleteSheet { sheet: s1 }).unwrap();
    let d = h.document().drawing();
    assert_eq!(d.sheets.len(), 1);
    assert!(d.element(other).is_none(), "projected from a view of the sheet deleted");
    assert!(d.element(text).is_some());
}

#[test]
fn a_shared_layer_shows_on_every_sheet() {
    let mut h = History::new(Document::new());
    let s1 = sheet(&mut h);
    let s2 = sheet(&mut h);
    let title = h.document().drawing().layers.iter().find(|l| l.shared).unwrap().id;
    let block = h
        .apply(Operation::AddElement {
            at: Placing { sheet: s1, layer: Some(title) },
            element: ElementKind::Rect { min: DVec2::new(240.0, 10.0), max: DVec2::new(410.0, 50.0) },
        })
        .unwrap()
        .element
        .unwrap();
    let own = add(&mut h, s1, ElementKind::Line { from: DVec2::ZERO, to: DVec2::ONE }).unwrap();
    let d = h.document().drawing();
    let on = |s: Id| d.elements_of(d.sheet(s).unwrap()).map(|e| e.id).collect::<Vec<_>>();
    // Bottom first: the drawing's layer, then the title block's.
    assert_eq!(on(s1), vec![own, block]);
    assert_eq!(on(s2), vec![block]);
}

#[test]
fn a_drawing_goes_through_the_file() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let part = h
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 400.0, 18.0) },
            placement: DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .created[0];
    let component = h.document().root_assembly().occurrence(part).unwrap().component;
    let s = sheet(&mut h);
    let front = add(
        &mut h,
        s,
        ElementKind::View(View {
            name: "Plateau".into(),
            kind: ViewKind::Base {
                subject: Subject::Component(component),
                look: Look::Standard(Direction::Top),
                scale: 0.2,
                at: DVec2::ONE,
            },
            style: ViewStyle::Hybrid,
        }),
    )
    .unwrap();
    add(&mut h, s, projected(front, Side::Below)).unwrap();
    h.apply(Operation::SetProjection(Projection::ThirdAngle)).unwrap();
    let bytes = h.document().to_file().unwrap();
    let back = Document::from_file(&bytes).unwrap();
    assert_eq!(back.drawing(), h.document().drawing());
    // A model without a drawing writes none.
    let json = Document::new().to_json().unwrap();
    assert!(!json.contains("drawing"), "{json}");
}

#[test]
fn layers_are_added_shared_unshared_and_deleted() {
    let mut h = History::new(Document::new());
    let s1 = sheet(&mut h);
    let s2 = sheet(&mut h);
    let plan = h.document().drawing().layers[0].id;
    let notes = h.apply(Operation::AddLayer { name: None }).unwrap().layer.unwrap();
    assert_eq!(h.document().drawing().layers.last().unwrap().id, notes, "on top");
    let on = |layer: Id| Placing { sheet: s1, layer: Some(layer) };
    let text = h
        .apply(Operation::AddElement { at: on(notes), element: ElementKind::Text { at: DVec2::ONE, text: "Note".into(), size: 3.5 } })
        .unwrap()
        .element
        .unwrap();
    let other = h
        .apply(Operation::AddElement {
            at: Placing { sheet: s2, layer: Some(notes) },
            element: ElementKind::Line { from: DVec2::ZERO, to: DVec2::ONE },
        })
        .unwrap()
        .element
        .unwrap();
    // Shared from the first sheet: its text on every sheet, the second's line gone.
    h.apply(Operation::ShareLayer { layer: notes, shared: true, sheet: s1 }).unwrap();
    let d = h.document().drawing();
    assert!(d.layer(notes).unwrap().shared);
    assert!(d.element(other).is_none());
    let on_sheet = |d: &Drawing, s: Id| d.elements_of(d.sheet(s).unwrap()).filter(|e| e.layer == notes).count();
    assert_eq!((on_sheet(d, s1), on_sheet(d, s2)), (1, 1));
    // Not shared again: a copy on each sheet, the first keeping the text's id.
    h.apply(Operation::ShareLayer { layer: notes, shared: false, sheet: s1 }).unwrap();
    let d = h.document().drawing();
    assert!(d.sheet(s1).unwrap().elements.iter().any(|e| e.id == text));
    assert_eq!((on_sheet(d, s1), on_sheet(d, s2)), (1, 1));
    // Hidden and locked, renamed.
    h.apply(Operation::SetLayer { layer: notes, name: "Notes".into(), hidden: true, locked: true }).unwrap();
    let l = h.document().drawing().layer(notes).unwrap().clone();
    assert!(l.hidden && l.locked && l.name == "Notes");
    // Moved onto the plan's layer.
    h.apply(Operation::MoveToLayer { elements: vec![text], layer: plan, sheet: s1 }).unwrap();
    assert_eq!(h.document().drawing().element(text).unwrap().layer, plan);
    // Deleted with what is on it; the last layer not shared stays.
    h.apply(Operation::DeleteLayer { layer: notes }).unwrap();
    assert!(h.document().drawing().layer(notes).is_none());
    assert_eq!(on_sheet(h.document().drawing(), s2), 0);
    assert!(matches!(h.apply(Operation::DeleteLayer { layer: plan }).unwrap_err(), Error::Invalid(Refusal::LastLayer)));
    // The title block's layer moved under the plan's.
    let title = h.document().drawing().layers.iter().find(|l| l.shared).unwrap().id;
    h.apply(Operation::MoveLayer { layer: title, index: 0 }).unwrap();
    assert_eq!(h.document().drawing().layers[0].id, title);
    let bytes = h.document().to_file().unwrap();
    assert_eq!(Document::from_file(&bytes).unwrap().drawing(), h.document().drawing());
}

#[test]
fn sheets_are_made_from_templates_their_title_block_once() {
    // The template Skraft comes with goes through its file.
    let default = Document::default_template();
    let sheet = default.drawing().sheets[0].id;
    let bytes = default.template_file(sheet).unwrap();
    let (template, content) = Document::read_file(&bytes).unwrap();
    assert_eq!(content, Content::Template);
    assert!(Document::from_file(&bytes).is_err(), "a template is not opened as a model");
    let block = template.drawing().layers.iter().find(|l| l.shared).unwrap().elements.len();
    assert!(block >= 5, "a frame, a title block and its texts");
    let template = std::sync::Arc::new(template);
    let mut h = History::new(Document::new());
    let s1 = h.apply(Operation::AddSheetFrom { template: template.clone(), name: None }).unwrap().sheet.unwrap();
    let d = h.document().drawing();
    assert_eq!(d.sheet(s1).unwrap().paper, Paper::A3);
    assert_eq!(d.layers.len(), 2, "its layers, nothing more: {:?}", d.layers.iter().map(|l| &l.name).collect::<Vec<_>>());
    assert_eq!(d.elements_of(d.sheet(s1).unwrap()).count(), block);
    // A second sheet from it: the title block is there already, not doubled.
    let s2 = h.apply(Operation::AddSheetFrom { template: template.clone(), name: Some("Détails".into()) }).unwrap().sheet.unwrap();
    let d = h.document().drawing();
    assert_eq!(d.layers.len(), 2);
    assert_eq!(d.elements_of(d.sheet(s2).unwrap()).count(), block);
    assert_eq!(d.sheet(s2).unwrap().name, "Détails");
    // A sheet with a view made a template: the view left out.
    add(&mut h, s1, base(Look::Standard(Direction::Front))).unwrap();
    let t = h.document().template_of(s1).unwrap();
    assert!(t.drawing().layers.iter().flat_map(|l| &l.elements).all(|e| !matches!(e.kind, ElementKind::View(_))));
    assert_eq!(Document::read_file(&h.document().template_file(s1).unwrap()).unwrap().1, Content::Template);
}

#[test]
fn a_dimension_anchor_follows_its_vertex_by_name_through_a_stretch() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let b = h
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 50.0, 20.0) },
            placement: DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .created[0];
    let doc = h.document();
    let placed = doc.subject_parts(&Subject::Model).into_iter().find(|p| p.path.entity == b).unwrap();
    let corner = DVec3::new(100.0, 50.0, 20.0);
    let shape = doc.part(placed.part).unwrap().shape().clone();
    let v = shape.vertex_points().iter().position(|p| (*p - corner).length() < 1e-9).unwrap() as u32;
    let anchor = doc.anchor(&placed, Entity::Vertex, v, corner).unwrap();
    assert_eq!(anchor.names.len(), 3, "a box's corner: three faces named");
    assert_eq!(doc.find(&Subject::Model, &anchor), Some(Found::Point(corner)));
    // Stretched 50 along X past its middle: the corner by its faces' names, 50 further.
    h.apply(Operation::Stretch {
        context: vec![],
        occurrences: vec![b],
        frame: DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![50.0],
        moves: vec![0.0, 50.0],
        scope: Scope::All,
        cutters: None,
    })
    .unwrap();
    let found = h.document().find(&Subject::Model, &anchor).unwrap().point();
    assert!((found - DVec3::new(150.0, 50.0, 20.0)).length() < 1e-6, "{found}");
    // Without its names, the point picked is too far now: an orphan.
    let bare = Anchor { names: Vec::new(), ..anchor.clone() };
    assert_eq!(h.document().find(&Subject::Model, &bare), None);
    // Moved: where it stands now.
    h.apply(Operation::Transform { context: root, entities: vec![b], transform: DAffine3::from_translation(DVec3::Z * 100.0) }).unwrap();
    let found = h.document().find(&Subject::Model, &anchor).unwrap().point();
    assert!((found - DVec3::new(150.0, 50.0, 120.0)).length() < 1e-6, "{found}");
    // The part gone: an orphan.
    h.apply(Operation::Delete { context: root, entities: vec![b] }).unwrap();
    assert_eq!(h.document().find(&Subject::Model, &anchor), None);
}

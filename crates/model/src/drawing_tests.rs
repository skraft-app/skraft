//! The drawing's tree: presentations, sheets, compositions and occurrences, views (docs/ui/drawing.md,
//! decision 0049).

use std::sync::Arc;

use glam::{DAffine3, DVec2, DVec3};

use crate::*;

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-12
}

fn near(a: DVec2, b: DVec2) -> bool {
    (a - b).length() < 1e-9
}

fn sheet_of(h: &mut History, presentation: Option<Id>, paper: Paper) -> Id {
    h.apply(Operation::AddSheet { presentation, name: None, paper }).unwrap().sheet.unwrap()
}

fn sheet(h: &mut History) -> Id {
    sheet_of(h, None, Paper::A3)
}

fn base(look: Look) -> ElementKind {
    ElementKind::View(View {
        name: "Face".into(),
        kind: ViewKind::Base { subject: Subject::Model, look, scale: 0.1, at: DVec2::new(100.0, 150.0) },
        style: ViewStyle::Lines,
        crop: None,
    })
}

fn front() -> ElementKind {
    base(Look::Standard(Direction::Front))
}

fn projected(base: Id, side: Side) -> ElementKind {
    ElementKind::View(View {
        name: "P".into(),
        kind: ViewKind::Projected { base, side, distance: 80.0 },
        style: ViewStyle::Lines,
        crop: None,
    })
}

fn rect() -> ElementKind {
    ElementKind::Rect { min: DVec2::ZERO, max: DVec2::new(10.0, 5.0) }
}

fn line() -> ElementKind {
    ElementKind::Line { from: DVec2::ZERO, to: DVec2::X }
}

fn text(at: DVec2) -> ElementKind {
    ElementKind::Text { at, text: "{client}".into(), size: 3.5 }
}

fn occurrence(composition: Id, place: Place) -> ElementKind {
    ElementKind::Occurrence(drawing::Occurrence { composition, place, props: Props::default() })
}

fn add_to(h: &mut History, to: Container, element: ElementKind) -> Result<Id> {
    h.apply(Operation::AddElement { to, element }).map(|o| o.element.unwrap())
}

fn add(h: &mut History, sheet: Id, element: ElementKind) -> Result<Id> {
    add_to(h, Container::Sheet(sheet), element)
}

fn compose(h: &mut History, elements: Vec<Id>, name: Option<&str>) -> Id {
    h.apply(Operation::MakeComposition { elements, name: name.map(str::to_owned) }).unwrap().element.unwrap()
}

fn composition_of(h: &History, occurrence: Id) -> Id {
    h.document().drawing().element(occurrence).unwrap().kind.occurrence().unwrap().composition
}

/// A named composition holding `element`, its one occurrence on `sheet`.
fn composition_with(h: &mut History, sheet: Id, element: ElementKind, name: &str) -> (Id, Id) {
    let e = add(h, sheet, element).unwrap();
    let i = compose(h, vec![e], Some(name));
    (composition_of(h, i), i)
}

#[test]
fn a_first_sheet_comes_in_a_presentation() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let d = h.document().drawing();
    assert_eq!(d.presentations.len(), 1, "made for it");
    assert_eq!(d.presentations[0].sheets[0].id, s);
    assert_eq!(d.sheet(s).unwrap().paper, Paper::A3);
    assert!(!d.sheet(s).unwrap().name.is_empty());
    let s2 = sheet(&mut h);
    let d = h.document().drawing();
    assert_eq!(d.presentations.len(), 1, "the next ones go in it");
    assert_ne!(d.sheet(s).unwrap().name, d.sheet(s2).unwrap().name);
    assert_eq!((d.sheet_number(s), d.sheet_number(s2)), (Some((1, 2)), Some((2, 2))));
    h.apply(Operation::MoveSheet { sheet: s2, presentation: None, index: 0 }).unwrap();
    assert_eq!(h.document().drawing().sheet_number(s2), Some((1, 2)));
    let set = |paper, grid, border| Operation::SetSheet { sheet: s, name: "Plan".into(), paper, grid, border };
    assert!(h.apply(set(Paper { width: 0.0, height: 10.0 }, 0.0, None)).is_err());
    assert!(h.apply(set(Paper::A4, -1.0, None)).is_err());
    assert!(h.apply(set(Paper::A4, 0.0, Some(Border { margin: 105.0, width: 0.25 }))).is_err(), "no paper left inside");
    assert!(h.apply(set(Paper::A4, 0.0, Some(Border { margin: 10.0, width: 0.0 }))).is_err());
    h.apply(set(Paper::A4, 5.0, Some(Border { margin: 10.0, width: 0.25 }))).unwrap();
    let got = h.document().drawing().sheet(s).unwrap();
    assert_eq!((got.grid, got.name.as_str(), got.border.unwrap().margin), (5.0, "Plan", 10.0));
    // Undone as the rest of the document.
    assert!(h.undo() && h.undo() && h.undo());
    assert_eq!(h.document().drawing().sheets().count(), 1);
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
    let p = d.presentations[0].clone();
    h.apply(Operation::SetPresentation { presentation: p.id, name: p.name, projection: Projection::ThirdAngle }).unwrap();
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
        perspective: Some(Perspective { eye: DVec3::new(-1000.0, -1000.0, 800.0), fov_y: 0.6, size: DVec2::new(120.0, 80.0) }),
    };
    let err = h.apply(Operation::SetElement { element: front, kind: base(persp.clone()) }).unwrap_err();
    assert!(matches!(err, Error::Invalid(Refusal::ProjectedFromPerspective)), "a base with projected views stays parallel");
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
        crop: None,
    });
    assert!(matches!(add(&mut h, s, gone).unwrap_err(), Error::Invalid(Refusal::BadElement)));
}

#[test]
fn a_view_and_what_hangs_on_it_stay_in_one_container() {
    let mut h = History::new(Document::new());
    let s1 = sheet(&mut h);
    let s2 = sheet(&mut h);
    let f = add(&mut h, s1, front()).unwrap();
    let right = add(&mut h, s1, projected(f, Side::Right)).unwrap();
    assert!(matches!(add(&mut h, s2, projected(f, Side::Below)).unwrap_err(), Error::Invalid(Refusal::SplitView)));
    let note = add(&mut h, s1, text(DVec2::ONE)).unwrap();
    h.apply(Operation::DeleteElements { elements: vec![f] }).unwrap();
    let d = h.document().drawing();
    assert!(d.element(f).is_none() && d.element(right).is_none(), "its projection with it");
    assert!(d.element(note).is_some());
    h.undo();
    h.apply(Operation::DeleteSheet { sheet: s1 }).unwrap();
    assert_eq!(h.document().drawing().sheets().count(), 1);
    assert!(h.document().drawing().element(right).is_none());
}

#[test]
fn an_anchored_composition_follows_the_corner_of_any_paper() {
    let mut h = History::new(Document::new());
    let a3 = sheet(&mut h);
    let a4 = sheet_of(&mut h, None, Paper::A4);
    let (composition, first) = composition_with(&mut h, a3, rect(), "Cartouche");
    let place = Place::anchored(Corner::BottomRight, DVec2::new(-10.0, 10.0));
    h.apply(Operation::SetElement { element: first, kind: occurrence(composition, place) }).unwrap();
    add(&mut h, a4, occurrence(composition, place)).unwrap();
    let at = |h: &History, s: Id| h.document().drawing().placed(s)[0].transform.translation;
    assert!(near(at(&h, a3), DVec2::new(410.0, 10.0)));
    assert!(near(at(&h, a4), DVec2::new(287.0, 10.0)), "at its own corner");
    // The sheet's format changed: it follows.
    h.apply(Operation::SetSheet { sheet: a3, name: "A".into(), paper: Paper::A2, grid: 0.0, border: None }).unwrap();
    assert!(near(at(&h, a3), DVec2::new(584.0, 10.0)));
    // The definition changed: on both.
    add_to(&mut h, Container::Composition(composition), line()).unwrap();
    let d = h.document().drawing();
    assert_eq!((d.placed(a3).len(), d.placed(a4).len()), (2, 2));
    for (corner, want) in [
        (Corner::BottomLeft, DVec2::ZERO),
        (Corner::BottomRight, DVec2::new(297.0, 0.0)),
        (Corner::TopLeft, DVec2::new(0.0, 210.0)),
        (Corner::TopRight, DVec2::new(297.0, 210.0)),
    ] {
        assert_eq!(corner.on(Paper::A4), want);
    }
}

#[test]
fn occurrences_nest_turn_and_pass_on_hidden_and_locked() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let (inner, inner_i) = composition_with(&mut h, s, line(), "Inner");
    let turned = Place { corner: None, at: DVec2::new(1.0, 0.0), rotation: std::f64::consts::FRAC_PI_2 };
    h.apply(Operation::SetElement { element: inner_i, kind: occurrence(inner, turned) }).unwrap();
    let outer_i = compose(&mut h, vec![inner_i], Some("Outer"));
    let outer = composition_of(&h, outer_i);
    h.apply(Operation::SetElement { element: outer_i, kind: occurrence(outer, Place::free(DVec2::new(10.0, 20.0))) }).unwrap();
    let d = h.document().drawing();
    let p = &d.placed(s)[0];
    assert_eq!(p.path, vec![outer_i, inner_i], "the occurrences down to it");
    assert_eq!(p.top(), outer_i, "what a click on the sheet picks");
    assert_eq!(p.under(&[outer_i]), Some(inner_i), "in the outer composition, the occurrence in it");
    assert_eq!(p.under(&[outer_i, inner_i]), Some(p.element.id), "in the inner one, the line");
    assert_eq!(p.under(&[inner_i]), None, "not in there");
    let (c, t) = d.opened(s, &[outer_i, inner_i]).unwrap();
    assert_eq!(c, Container::Composition(inner));
    assert!(near(t.transform_point2(DVec2::X), DVec2::new(11.0, 21.0)), "what takes the line to the sheet");
    assert_eq!(d.opened(s, &[]).unwrap().0, Container::Sheet(s));
    assert!(d.opened(s, &[inner_i]).is_none(), "not right on the sheet");
    // The line's end (1, 0): turned to (0, 1), moved by (1, 0), then by (10, 20).
    assert!(near(p.transform.transform_point2(DVec2::X), DVec2::new(11.0, 21.0)));
    h.apply(Operation::SetElementState { elements: vec![outer_i], hidden: Some(true), locked: Some(true) }).unwrap();
    let d = h.document().drawing();
    let p = &d.placed(s)[0];
    assert!(p.hidden && p.locked && !p.element.hidden, "through the occurrence above it");
    // The order of a container.
    let a = add(&mut h, s, rect()).unwrap();
    h.apply(Operation::MoveElement { element: a, index: 0 }).unwrap();
    assert_eq!(h.document().drawing().sheet(s).unwrap().elements[0].id, a);
}

#[test]
fn grouping_takes_a_views_family_and_copies_stay_linked() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let f = add(&mut h, s, front()).unwrap();
    let right = add(&mut h, s, projected(f, Side::Right)).unwrap();
    let note = add(&mut h, s, text(DVec2::new(5.0, 5.0))).unwrap();
    let over = add(&mut h, s, line()).unwrap();
    // The projection alone asked for: its base comes along, and the caption.
    let i = compose(&mut h, vec![right, note], None);
    let b = composition_of(&h, i);
    let d = h.document().drawing();
    assert_eq!(d.container_of(f), Some(Container::Composition(b)));
    assert_eq!(d.container_of(right), Some(Container::Composition(b)));
    assert!(!d.composition(b).unwrap().named);
    assert!(d.composition(b).unwrap().name.ends_with("#1"), "{}", d.composition(b).unwrap().name);
    let ids: Vec<Id> = d.sheet(s).unwrap().elements.iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![i, over], "where they were, under what was over them");
    assert!(near(d.resolve(right).unwrap().at, DVec2::new(180.0, 150.0)), "still where it was");
    assert_eq!(d.view_ids().collect::<Vec<_>>(), vec![f, right], "those in compositions too");
    // A copy shares it; made unique, it has its own, its projection on its own view.
    let copy = add(&mut h, s, occurrence(b, Place::free(DVec2::new(20.0, 30.0)))).unwrap();
    assert_eq!(h.document().drawing().occurrences_of(b).len(), 2);
    assert_eq!(h.document().drawing().composition_uses().get(&b), Some(&2));
    let own = h.apply(Operation::MakeOccurrenceUnique { occurrence: copy }).unwrap().composition.unwrap();
    let d = h.document().drawing();
    assert_ne!(own, b);
    assert_eq!(d.occurrences_of(b), vec![i]);
    let elements = &d.composition(own).unwrap().elements;
    let own_base = elements.iter().find(|e| matches!(&e.kind, ElementKind::View(View { kind: ViewKind::Base { .. }, .. }))).unwrap().id;
    assert_ne!(own_base, f);
    assert!(elements.iter().any(|e| e.kind.hung_on() == Some(own_base)));
    assert_eq!(h.apply(Operation::MakeOccurrenceUnique { occurrence: copy }).unwrap().composition, Some(own), "alone: nothing to do");
    // Exploded where it shows them; a group placed nowhere goes.
    let out = h.apply(Operation::ExplodeOccurrence { occurrence: copy }).unwrap().elements;
    assert_eq!(out.len(), 3);
    let d = h.document().drawing();
    assert!(d.composition(own).is_none(), "not named, placed nowhere");
    assert!(out.iter().all(|e| d.container_of(*e) == Some(Container::Sheet(s))));
    let moved = out.iter().find_map(|e| match &d.element(*e).unwrap().kind {
        ElementKind::Text { at, .. } => Some(*at),
        _ => None,
    });
    assert_eq!(moved, Some(DVec2::new(25.0, 35.0)));
    let view = out.iter().find(|e| matches!(d.element(**e).unwrap().kind, ElementKind::View(View { kind: ViewKind::Base { .. }, .. })));
    assert!(near(d.resolve(*view.unwrap()).unwrap().at, DVec2::new(120.0, 180.0)));
}

#[test]
fn compositions_refuse_to_hold_themselves() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let (a, _) = composition_with(&mut h, s, rect(), "A");
    let (b, _) = composition_with(&mut h, s, line(), "B");
    let free = Place::free(DVec2::ZERO);
    assert!(matches!(
        add_to(&mut h, Container::Composition(a), occurrence(a, free)).unwrap_err(),
        Error::Invalid(Refusal::CompositionLoop)
    ));
    add_to(&mut h, Container::Composition(b), occurrence(a, free)).unwrap();
    assert!(matches!(
        add_to(&mut h, Container::Composition(a), occurrence(b, free)).unwrap_err(),
        Error::Invalid(Refusal::CompositionLoop)
    ));
    let corner = Place::anchored(Corner::TopLeft, DVec2::ZERO);
    assert!(
        matches!(add_to(&mut h, Container::Composition(b), occurrence(a, corner)).unwrap_err(), Error::Invalid(Refusal::BadElement)),
        "no paper in a composition"
    );
    assert!(matches!(add(&mut h, s, occurrence(Id(9999), free)).unwrap_err(), Error::NotFound(Id(9999))));
}

#[test]
fn a_turned_occurrence_explodes_only_what_turns() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let turned = Place { corner: None, at: DVec2::new(10.0, 0.0), rotation: std::f64::consts::FRAC_PI_2 };
    let (a, ai) = composition_with(&mut h, s, rect(), "A");
    h.apply(Operation::SetElement { element: ai, kind: occurrence(a, turned) }).unwrap();
    let err = h.apply(Operation::ExplodeOccurrence { occurrence: ai }).unwrap_err();
    assert!(matches!(err, Error::Invalid(Refusal::TurnedOccurrence)));
    let (b, bi) = composition_with(&mut h, s, line(), "B");
    h.apply(Operation::SetElement { element: bi, kind: occurrence(b, turned) }).unwrap();
    let out = h.apply(Operation::ExplodeOccurrence { occurrence: bi }).unwrap().elements;
    match &h.document().drawing().element(out[0]).unwrap().kind {
        ElementKind::Line { from, to } => assert!(near(*from, DVec2::new(10.0, 0.0)) && near(*to, DVec2::new(10.0, 1.0))),
        k => panic!("{k:?}"),
    }
    assert!(h.document().drawing().composition(b).is_some(), "named: it stays");
    h.apply(Operation::DeleteComposition { composition: a }).unwrap();
    let d = h.document().drawing();
    assert!(d.composition(a).is_none() && d.element(ai).is_none(), "with its occurrences");
    h.apply(Operation::RenameComposition { composition: b, name: "Logo".into() }).unwrap();
    assert_eq!(h.document().drawing().composition(b).unwrap().name, "Logo");
    assert!(h.apply(Operation::RenameComposition { composition: b, name: "  ".into() }).is_err());
}

#[test]
fn presentations_number_their_sheets_and_project_their_way() {
    let mut h = History::new(Document::new());
    let atelier = h.apply(Operation::AddPresentation { name: Some("Atelier".into()), paper: None }).unwrap().presentation.unwrap();
    let made = h.apply(Operation::AddPresentation { name: Some("Client".into()), paper: Some(Paper::A4) }).unwrap();
    let client = made.presentation.unwrap();
    let first = made.sheet.unwrap();
    assert_eq!(h.document().drawing().presentation_of(first).unwrap().id, client, "with its first sheet");
    assert_eq!(h.document().drawing().sheet(first).unwrap().paper, Paper::A4);
    h.apply(Operation::DeleteSheet { sheet: first }).unwrap();
    h.apply(Operation::SetPresentation { presentation: client, name: "Client".into(), projection: Projection::ThirdAngle }).unwrap();
    let a1 = sheet_of(&mut h, Some(atelier), Paper::A3);
    let a2 = sheet_of(&mut h, Some(atelier), Paper::A4);
    let c1 = sheet_of(&mut h, Some(client), Paper::A3);
    let d = h.document().drawing();
    assert_eq!((d.sheet_number(a2), d.sheet_number(c1)), (Some((2, 2)), Some((1, 1))));
    // One composition with a projected view, placed in both: each its presentation's projection.
    let f = add(&mut h, a1, front()).unwrap();
    let right = add(&mut h, a1, projected(f, Side::Right)).unwrap();
    let i = compose(&mut h, vec![f], Some("Vues"));
    let b = composition_of(&h, i);
    add(&mut h, c1, occurrence(b, Place::free(DVec2::ZERO))).unwrap();
    let d = h.document().drawing();
    let of = |s: Id| d.placed(s).iter().find(|p| p.element.id == right).unwrap().projection;
    assert_eq!((of(a1), of(c1)), (Projection::FirstAngle, Projection::ThirdAngle));
    let forward = |p| d.resolve_as(right, p).unwrap().frame.forward;
    assert!(close(forward(of(a1)), DVec3::X) && close(forward(of(c1)), -DVec3::X));
    // A sheet moved to the other.
    h.apply(Operation::MoveSheet { sheet: a2, presentation: Some(client), index: 0 }).unwrap();
    let d = h.document().drawing();
    assert_eq!(d.presentation_of(a2).unwrap().id, client);
    assert_eq!((d.sheet_number(a1), d.sheet_number(c1)), (Some((1, 1)), Some((2, 2))));
    // Deleted with its sheets; what they alone placed goes when not named.
    let g = add(&mut h, c1, line()).unwrap();
    let gi = compose(&mut h, vec![g], None);
    let gb = composition_of(&h, gi);
    h.apply(Operation::DeletePresentation { presentation: client }).unwrap();
    let d = h.document().drawing();
    assert!(d.sheet(c1).is_none() && d.composition(gb).is_none());
    assert!(d.composition(b).is_some(), "named");
    assert!(h.apply(Operation::AddSheet { presentation: Some(client), name: None, paper: Paper::A3 }).is_err());
}

#[test]
fn fields_look_up_from_the_occurrence_to_the_document() {
    let text = |s: &str| Value::Text(s.into());
    let mut doc = Document::new();
    let root = doc.root();
    Arc::make_mut(doc.components.get_mut(&root).unwrap()).props.set("user.client", Some(text("Document")));
    let mut h = History::new(doc);
    let s = sheet(&mut h);
    let p = h.document().drawing().presentation_of(s).unwrap().id;
    let (_, i) = composition_with(&mut h, s, rect(), "Cartouche");
    let got = |h: &History| h.document().field(s, &[i], "client").cloned();
    assert_eq!(got(&h), Some(text("Document")));
    let set = |on, v: Option<Value>| Operation::SetField { on, key: "user.client".into(), value: v };
    h.apply(set(FieldOn::Presentation(p), Some(text("Présentation")))).unwrap();
    assert_eq!(got(&h), Some(text("Présentation")));
    h.apply(set(FieldOn::Sheet(s), Some(text("Feuille")))).unwrap();
    assert_eq!(got(&h), Some(text("Feuille")));
    h.apply(set(FieldOn::Occurrence(i), Some(text("Occurrence")))).unwrap();
    assert_eq!(got(&h), Some(text("Occurrence")));
    assert_eq!(h.document().field(s, &[], "user.client"), Some(&text("Feuille")), "not under the occurrence");
    h.apply(set(FieldOn::Occurrence(i), None)).unwrap();
    assert_eq!(got(&h), Some(text("Feuille")));
    assert_eq!(h.document().field(s, &[i], "other"), None);
    assert!(h.apply(Operation::SetField { on: FieldOn::Sheet(s), key: "nokey".into(), value: None }).is_err());
    let line_id = add(&mut h, s, line()).unwrap();
    assert!(h.apply(set(FieldOn::Occurrence(line_id), Some(text("x")))).is_err());
}

#[test]
fn a_template_saved_from_a_document_comes_back_without_doubling_its_compositions() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let (composition, _) = composition_with(&mut h, s, rect(), "Cartouche");
    // A composition of views alone: nothing of it in a template.
    let f = add(&mut h, s, front()).unwrap();
    let views = compose(&mut h, vec![f], Some("Vues"));
    let p = h.document().drawing().presentations[0].id;
    let t = Arc::new(h.document().template_of(p).unwrap());
    assert_eq!(t.drawing().compositions.len(), 1, "the composition of views left out");
    assert!(t.drawing().element(views).is_none(), "and its occurrence");
    // Back in the document it came from: its composition is there, not doubled.
    let again =
        h.apply(Operation::AddSheetFrom { template: t.clone(), sheet: None, presentation: None, name: None }).unwrap().sheet.unwrap();
    let d = h.document().drawing();
    assert_eq!(d.compositions.len(), 2, "no copy of it");
    assert_eq!(d.sheet(again).unwrap().elements[0].kind.occurrence().unwrap().composition, composition);
    // Changed since: what the template holds is another definition.
    add_to(&mut h, Container::Composition(composition), line()).unwrap();
    h.apply(Operation::AddSheetFrom { template: t.clone(), sheet: None, presentation: None, name: None }).unwrap();
    assert_eq!(h.document().drawing().compositions.len(), 3);
    // A presentation from it takes its name, numbered when it is taken.
    let made = h.apply(Operation::AddPresentationFrom { template: t, name: None }).unwrap().presentation.unwrap();
    let d = h.document().drawing();
    assert_ne!(d.presentation(made).unwrap().name, d.presentations[0].name);
    assert!(d.presentation(made).unwrap().name.ends_with("#2"), "{}", d.presentation(made).unwrap().name);
}

#[test]
fn pasting_in_its_document_keeps_families_and_links() {
    let mut h = History::new(Document::new());
    let s1 = sheet(&mut h);
    let s2 = sheet_of(&mut h, None, Paper::A4);
    let f = add(&mut h, s1, front()).unwrap();
    let right = add(&mut h, s1, projected(f, Side::Right)).unwrap();
    let (c, i) = anchored_composition(&mut h, s1);
    // The projection alone asked for: its base comes along.
    let fragment = Arc::new(h.document().elements_fragment(Container::Sheet(s1), &[right, i]).unwrap());
    let paste = |to, linked| Operation::PasteElements { fragment: fragment.clone(), to, offset: DVec2::new(5.0, -5.0), linked };
    let out = h.apply(paste(Container::Sheet(s2), true)).unwrap().elements;
    assert_eq!(out.len(), 3);
    let d = h.document().drawing();
    assert_eq!(d.composition_uses().get(&c), Some(&2), "an occurrence of the same composition");
    let there = &d.sheet(s2).unwrap().elements;
    let view = there.iter().find(|e| matches!(&e.kind, ElementKind::View(View { kind: ViewKind::Base { .. }, .. }))).unwrap();
    assert_ne!(view.id, f);
    assert!(near(d.resolve(view.id).unwrap().at, DVec2::new(105.0, 145.0)), "moved");
    assert!(there.iter().any(|e| e.kind.hung_on() == Some(view.id)), "its projection on its own copy");
    let inst = there.iter().find_map(|e| e.kind.occurrence()).unwrap();
    assert_eq!((inst.place.corner, inst.place.at), (Some(Corner::BottomRight), DVec2::new(-5.0, 5.0)), "from its corner");
    // Into itself: refused.
    let err = h.apply(paste(Container::Composition(c), true)).unwrap_err();
    assert!(matches!(err, Error::Invalid(Refusal::CompositionLoop)), "{err:?}");
    assert_eq!(h.document().drawing().composition(c).unwrap().elements.len(), 1, "nothing left of it");
}

#[test]
fn purging_takes_what_no_sheet_shows_even_placed_in_another_unused() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let line = ElementKind::Line { from: DVec2::ZERO, to: DVec2::X };
    let (a, kept) = (add(&mut h, s, line.clone()).unwrap(), add(&mut h, s, line).unwrap());
    let inner = h.apply(Operation::MakeComposition { elements: vec![a], name: Some("Dedans".into()) }).unwrap().element.unwrap();
    let outer = h.apply(Operation::MakeComposition { elements: vec![inner], name: Some("Dehors".into()) }).unwrap().element.unwrap();
    let k = h.apply(Operation::MakeComposition { elements: vec![kept], name: Some("Gardée".into()) }).unwrap().element.unwrap();
    let k = composition_of(&h, k);
    h.apply(Operation::DeleteElements { elements: vec![outer] }).unwrap();
    assert_eq!(h.document().drawing().compositions.len(), 3, "named: they stay");
    assert_eq!(h.document().drawing().counts_on_sheets().len(), 1, "only one on a sheet");
    h.apply(Operation::PurgeCompositions).unwrap();
    assert_eq!(h.document().drawing().compositions.keys().copied().collect::<Vec<_>>(), vec![k]);
    assert!(h.undo());
    assert_eq!(h.document().drawing().compositions.len(), 3);
}

#[test]
fn cutting_a_compositions_last_occurrence_then_pasting_brings_it_back_whole() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let f = add(&mut h, s, front()).unwrap();
    let right = add(&mut h, s, projected(f, Side::Right)).unwrap();
    let i = compose(&mut h, vec![right], None);
    let c = composition_of(&h, i);
    let fragment = Arc::new(h.document().elements_fragment(Container::Sheet(s), &[i]).unwrap());
    h.apply(Operation::DeleteElements { elements: vec![i] }).unwrap();
    assert!(h.document().drawing().composition(c).is_none(), "gone with its last occurrence");
    let paste = || Operation::PasteElements { fragment: fragment.clone(), to: Container::Sheet(s), offset: DVec2::ZERO, linked: true };
    h.apply(paste()).unwrap();
    h.apply(paste()).unwrap();
    let d = h.document().drawing();
    assert_eq!(d.compositions.len(), 1, "one composition, however many pastes");
    assert_eq!(d.composition_uses().get(&c), Some(&2));
    assert_eq!(d.composition(c).unwrap().elements.len(), 2, "its view and projection kept");
    assert!(d.resolve(right).is_some());
    let bytes = h.document().to_file().unwrap();
    assert_eq!(Document::from_file(&bytes).unwrap().drawing(), h.document().drawing());
}

#[test]
fn pasting_into_another_document_leaves_the_model_behind() {
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let f = add(&mut h, s, front()).unwrap();
    let note = add(&mut h, s, text(DVec2::ONE)).unwrap();
    let (c, i) = anchored_composition(&mut h, s);
    let both = Arc::new(h.document().elements_fragment(Container::Sheet(s), &[f, note, i]).unwrap());
    let views = Arc::new(h.document().elements_fragment(Container::Sheet(s), &[f]).unwrap());
    let _ = c;
    let mut other = History::new(Document::new());
    let t = sheet(&mut other);
    let paste = |fragment| Operation::PasteElements { fragment, to: Container::Sheet(t), offset: DVec2::ZERO, linked: false };
    let out = other.apply(paste(both.clone())).unwrap().elements;
    assert_eq!(out.len(), 2, "the text and the occurrence, no view");
    assert_eq!(other.document().drawing().compositions.len(), 1);
    other.apply(paste(both)).unwrap();
    assert_eq!(other.document().drawing().compositions.len(), 1, "its composition brought once");
    assert!(matches!(other.apply(paste(views)).unwrap_err(), Error::Invalid(Refusal::NothingToPaste)));
}

#[test]
fn elements_move_between_sheets_with_what_hangs_on_them() {
    let mut h = History::new(Document::new());
    let a3 = sheet(&mut h);
    let a4 = sheet_of(&mut h, None, Paper::A4);
    let f = add(&mut h, a3, front()).unwrap();
    let right = add(&mut h, a3, projected(f, Side::Right)).unwrap();
    let (_, i) = anchored_composition(&mut h, a3);
    let moved = |elements: Vec<Id>, copy| Operation::MoveElements { elements, to: a4, offset: DVec2::new(-10.0, 0.0), copy };
    let out = h.apply(moved(vec![right, i], false)).unwrap().elements;
    assert_eq!(out, vec![f, right, i], "ids kept");
    let d = h.document().drawing();
    assert!(d.sheet(a3).unwrap().elements.is_empty());
    assert_eq!(d.container_of(right), Some(Container::Sheet(a4)));
    assert!(near(d.resolve(f).unwrap().at, DVec2::new(90.0, 150.0)), "by the offset");
    let p = d.element(i).unwrap().kind.occurrence().unwrap().place;
    assert_eq!((p.corner, p.at), (Some(Corner::BottomRight), DVec2::new(-10.0, 10.0)), "keeps its corner");
    h.undo();
    assert_eq!(h.document().drawing().sheet(a3).unwrap().elements.len(), 3, "one step");
    // A copy: ids of its own, the source left.
    let out = h.apply(moved(vec![f], true)).unwrap().elements;
    assert_eq!(out.len(), 2);
    assert!(!out.contains(&f));
    assert_eq!(h.document().drawing().sheet(a3).unwrap().elements.len(), 3);
}

#[test]
fn a_sheet_is_copied_with_what_hangs_on_its_views() {
    let mut h = History::new(Document::new());
    let a3 = sheet(&mut h);
    let f = add(&mut h, a3, front()).unwrap();
    add(&mut h, a3, projected(f, Side::Right)).unwrap();
    let (c, _) = anchored_composition(&mut h, a3);
    let client = h.apply(Operation::AddPresentation { name: Some("Client".into()), paper: Some(Paper::A4) }).unwrap();
    let (q, other) = (client.presentation.unwrap(), client.sheet.unwrap());
    let copy = h.apply(Operation::CopySheet { sheet: a3, presentation: q, index: 0 }).unwrap().sheet.unwrap();
    let d = h.document().drawing();
    let list: Vec<Id> = d.presentation(q).unwrap().sheets.iter().map(|s| s.id).collect();
    assert_eq!(list, vec![copy, other], "at its index");
    let (s, from) = (d.sheet(copy).unwrap(), d.sheet(a3).unwrap());
    assert_eq!((s.paper, s.elements.len(), from.elements.len()), (Paper::A3, 3, 3));
    assert!(s.elements.iter().all(|e| from.elements.iter().all(|x| x.id != e.id)), "ids of its own");
    assert_eq!(s.name, skraft_i18n::t!("drawing.sheet_name", n = 2), "a default name taken: the next free one");
    match &s.elements[1].kind {
        ElementKind::View(View { kind: ViewKind::Projected { base, .. }, .. }) => {
            assert_eq!(*base, s.elements[0].id, "on the copy of its base")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(s.elements[2].kind.occurrence().unwrap().composition, c, "the same composition");
    h.undo();
    assert!(h.document().drawing().sheet(copy).is_none(), "one step");
    // A name of one's own, taken: numbered.
    h.apply(Operation::SetSheet { sheet: other, name: "Plan".into(), paper: Paper::A4, grid: 0.0, border: None }).unwrap();
    let copy = h.apply(Operation::CopySheet { sheet: other, presentation: q, index: 1 }).unwrap().sheet.unwrap();
    assert_eq!(h.document().drawing().sheet(copy).unwrap().name, "Plan#2");
}

/// A named composition holding a rectangle, its occurrence anchored 10 mm from the bottom right.
fn anchored_composition(h: &mut History, s: Id) -> (Id, Id) {
    let (c, i) = composition_with(h, s, rect(), "Cartouche");
    let place = Place::anchored(Corner::BottomRight, DVec2::new(-10.0, 10.0));
    h.apply(Operation::SetElement { element: i, kind: occurrence(c, place) }).unwrap();
    (c, i)
}

#[test]
fn a_place_keeps_where_it_is_when_its_corner_changes() {
    let start = Place { corner: Some(Corner::BottomRight), at: DVec2::new(-10.0, 10.0), rotation: 0.5 };
    let a3 = Paper::A3;
    let free = start.with_corner(None, a3);
    assert_eq!((free.corner, free.at, free.rotation), (None, DVec2::new(410.0, 10.0), 0.5));
    let top = free.with_corner(Some(Corner::TopLeft), a3);
    assert_eq!((top.corner, top.at), (Some(Corner::TopLeft), DVec2::new(410.0, -287.0)));
    assert!(near(top.origin(Some(a3)), start.origin(Some(a3))));
}

#[test]
fn field_names_are_read_from_the_texts_through_the_occurrences() {
    assert_eq!(text_fields("a {client} b {sheet.number} {x").collect::<Vec<_>>(), vec!["client", "sheet.number"]);
    assert_eq!((field_key("client"), field_key("skp.ref")), ("user.client".to_owned(), "skp.ref".to_owned()));
    let mut h = History::new(Document::new());
    let s = sheet(&mut h);
    let (b, _) = composition_with(&mut h, s, text(DVec2::ZERO), "Cartouche");
    let (_, outer) = composition_with(&mut h, s, ElementKind::Text { at: DVec2::ONE, text: "{indice}".into(), size: 3.0 }, "Autre");
    let inner = add(&mut h, s, occurrence(b, Place::free(DVec2::ZERO))).unwrap();
    let g = compose(&mut h, vec![outer, inner], None);
    let d = h.document().drawing();
    let names: Vec<String> = d.field_names(&d.sheet(s).unwrap().elements).into_iter().collect();
    assert_eq!(names, vec!["client".to_owned(), "indice".to_owned()], "through {g:?}");
    assert_eq!(d.all_field_names().len(), 2);
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
            crop: None,
        }),
    )
    .unwrap();
    add(&mut h, s, projected(front, Side::Below)).unwrap();
    let (composition, _) = composition_with(&mut h, s, rect(), "Cartouche");
    add(&mut h, s, occurrence(composition, Place::anchored(Corner::TopRight, DVec2::new(-5.0, -5.0)))).unwrap();
    let p = h.document().drawing().presentations[0].clone();
    h.apply(Operation::SetPresentation { presentation: p.id, name: "Atelier".into(), projection: Projection::ThirdAngle }).unwrap();
    let bytes = h.document().to_file().unwrap();
    let back = Document::from_file(&bytes).unwrap();
    assert_eq!(back.drawing(), h.document().drawing());
    // A model without a drawing writes none.
    let json = Document::new().to_json().unwrap();
    assert!(!json.contains("drawing"), "{json}");
    // A composition holding itself is refused.
    let mut bad = h.document().clone();
    let id = bad.fresh_id();
    let looped = Element::new(id, occurrence(composition, Place::free(DVec2::ZERO)));
    Arc::make_mut(&mut bad.drawing).compositions.get_mut(&composition).unwrap().elements.push(looped);
    assert!(matches!(Document::from_file(&bad.to_file().unwrap()), Err(Error::File(FileError::BadDrawing(i))) if i == id));
}

#[test]
fn templates_hold_a_presentation_their_compositions_brought_once() {
    // The template Skraft comes with goes through its file.
    let default = Document::default_template();
    let presentation = default.drawing().presentations[0].id;
    let bytes = default.template_file(presentation).unwrap();
    let (template, content) = Document::read_file(&bytes).unwrap();
    assert_eq!(content, Content::Template);
    assert!(Document::from_file(&bytes).is_err(), "a template is not opened as a model");
    assert_eq!(template.drawing().compositions.len(), 1);
    let held = template.drawing().compositions.values().next().unwrap().elements.len();
    assert!(held >= 5, "a title block and its texts");
    let template = Arc::new(template);
    let mut h = History::new(Document::new());
    let from = |name: Option<&str>| Operation::AddSheetFrom {
        template: template.clone(),
        sheet: None,
        presentation: None,
        name: name.map(str::to_owned),
    };
    let s1 = h.apply(from(None)).unwrap().sheet.unwrap();
    let d = h.document().drawing();
    assert_eq!(d.sheet(s1).unwrap().paper, Paper::A3);
    assert_eq!(d.sheet(s1).unwrap().border, Some(Border { margin: 10.0, width: 0.25 }));
    assert_eq!(d.placed(s1).len(), held);
    assert!(near(d.placed(s1)[0].transform.translation, DVec2::new(410.0, 10.0)), "at the paper's corner");
    // A second sheet from it: the composition is there already, not doubled.
    let s2 = h.apply(from(Some("Détails"))).unwrap().sheet.unwrap();
    let d = h.document().drawing();
    assert_eq!(d.compositions.len(), 1);
    assert_eq!(d.placed(s2).len(), held);
    assert_eq!(d.sheet(s2).unwrap().name, "Détails");
    // A whole presentation from it, its sheets keeping their names, the composition still one.
    let p = h.apply(Operation::AddPresentationFrom { template: template.clone(), name: None }).unwrap().presentation.unwrap();
    let d = h.document().drawing();
    assert_eq!(d.compositions.len(), 1);
    assert_eq!(d.presentation(p).unwrap().sheets[0].name, template.drawing().presentations[0].sheets[0].name);
    // Made a template with a view on a sheet and in a composition: the views left out.
    let f = add(&mut h, s1, front()).unwrap();
    let side = add(&mut h, s1, projected(f, Side::Right)).unwrap();
    let plain = add(&mut h, s1, front()).unwrap();
    compose(&mut h, vec![side], Some("Vues"));
    let first = h.document().drawing().presentation_of(s1).unwrap().id;
    let t = h.document().template_of(first).unwrap();
    assert!(t.drawing().element(plain).is_none());
    assert!(t.drawing().containers().flat_map(|(_, l)| l).all(|e| !matches!(e.kind, ElementKind::View(_) | ElementKind::Dimension(_))));
    assert!(t.drawing().compositions.values().all(|b| b.source.is_some()), "each named for bringing it once");
    assert_eq!(Document::read_file(&h.document().template_file(first).unwrap()).unwrap().1, Content::Template);
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

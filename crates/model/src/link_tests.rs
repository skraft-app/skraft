//! Imprints and links (docs/decisions/0055-linked-bodies-through-imprints.md): an imprint does
//! nothing where it is, a link makes a linked body of it in another part.

use std::sync::Arc;

use glam::{DAffine3, DVec3};
use skraft_kernel::names::Blend;

use crate::*;

const LEG_VOLUME: f64 = 40.0 * 40.0 * 100.0;
const IMPRINT_VOLUME: f64 = 1000.0;

fn at(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn moved(v: DVec3) -> DAffine3 {
    DAffine3::from_translation(v)
}

fn cuboid(origin: DVec3, size: DVec3) -> Primitive {
    Primitive::Box { origin, size }
}

fn add_part(h: &mut History, size: DVec3, placement: DAffine3) -> Id {
    let root = h.document().root();
    h.apply(Operation::AddPrimitive { context: root, primitive: cuboid(DVec3::ZERO, size), placement, material: None }).unwrap().created[0]
}

fn part_of(h: &History, occurrence: Id) -> Id {
    h.document().resolve(&EntityPath::root(occurrence)).unwrap().component
}

fn add_body(h: &mut History, target: Id, origin: DVec3, size: DVec3, sign: Sign) -> (Id, Outcome) {
    let target = EntityPath::root(target);
    let o = h
        .apply(Operation::AddBody { target, primitive: cuboid(origin, size), placement: DAffine3::IDENTITY, sign, scope: Scope::All })
        .unwrap();
    (o.bodies[0], o)
}

fn volume(h: &History, part: Id) -> f64 {
    h.document().part(part).unwrap().shape().volume().unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn refused(r: Result<Outcome>, why: Refusal) {
    match r {
        Err(Error::Invalid(w)) => assert_eq!(w, why),
        other => panic!("expected {why:?}, got {other:?}"),
    }
}

/// A rail 100 × 20 × 20 whose imprint, 10 × 10 × 10, runs past its end into a leg 40 × 40 × 100:
/// `(rail, leg, imprint)`, occurrences and body.
fn rail_and_leg(h: &mut History) -> (Id, Id, Id) {
    let rail = add_part(h, at(100.0, 20.0, 20.0), DAffine3::IDENTITY);
    let leg = add_part(h, at(40.0, 40.0, 100.0), moved(at(100.0, -10.0, -40.0)));
    let (imprint, _) = add_body(h, rail, at(100.0, 5.0, 5.0), DVec3::splat(10.0), Sign::Imprint);
    (rail, leg, imprint)
}

fn link_op(h: &History, source: Id, imprint: Id, receiver: Id, onto: Option<Id>) -> Operation {
    let context = h.document().root();
    Operation::AddLink { context, source: vec![source], imprint, receiver: vec![receiver], sign: Sign::Negative, onto }
}

fn root_links(h: &History) -> Vec<Link> {
    h.document().root_assembly().links().to_vec()
}

fn states(h: &History) -> Vec<LinkState> {
    let root = h.document().root();
    root_links(h).iter().map(|l| h.document().link_state(root, l)).collect()
}

// ---- an imprint does nothing where it is ---------------------------------------------------

#[test]
fn an_imprint_leaves_its_part_as_it_is() {
    let mut h = History::new(Document::new());
    let board = add_part(&mut h, at(100.0, 50.0, 20.0), DAffine3::IDENTITY);
    let part = part_of(&h, board);
    let before = h.document().part(part).unwrap().shape().clone();

    // Across a side: a positive would add to it, a negative cut it.
    let (imprint, _) = add_body(&mut h, board, at(90.0, 10.0, 5.0), at(30.0, 10.0, 10.0), Sign::Imprint);
    let same = |h: &History| Arc::ptr_eq(h.document().part(part).unwrap().shape(), &before);
    assert!(same(&h), "no computation: its faces keep their order");
    assert!(!h.document().part(part).unwrap().is_plain());

    let target = EntityPath::root(board);
    let op = Operation::TransformBodies { target: target.clone(), bodies: vec![imprint], transform: moved(DVec3::X), scope: Scope::All };
    h.apply(op).unwrap();
    assert!(same(&h), "moved");
    let copy = h
        .apply(Operation::DuplicateBodies { target: target.clone(), bodies: vec![imprint], transform: moved(DVec3::Y), scope: Scope::All })
        .unwrap()
        .bodies[0];
    assert!(same(&h), "copied");
    h.apply(Operation::DeleteBodies { target: target.clone(), bodies: vec![copy], scope: Scope::All }).unwrap();
    assert!(same(&h), "one deleted");
    h.apply(Operation::DeleteBodies { target, bodies: vec![imprint], scope: Scope::All }).unwrap();
    assert!(same(&h) && h.document().part(part).unwrap().is_plain(), "its one body again");
}

#[test]
fn a_body_made_an_imprint_stops_acting() {
    let mut h = History::new(Document::new());
    let board = add_part(&mut h, at(100.0, 50.0, 20.0), DAffine3::IDENTITY);
    let part = part_of(&h, board);
    let (extra, _) = add_body(&mut h, board, at(100.0, 0.0, 0.0), at(10.0, 50.0, 20.0), Sign::Positive);
    assert!(close(volume(&h, part), 110.0 * 50.0 * 20.0));
    let set = |sign| Operation::SetSign { target: EntityPath::root(board), bodies: vec![extra], sign, scope: Scope::All };
    h.apply(set(Sign::Imprint)).unwrap();
    assert!(close(volume(&h, part), 100.0 * 50.0 * 20.0));
    h.apply(set(Sign::Positive)).unwrap();
    assert!(close(volume(&h, part), 110.0 * 50.0 * 20.0));
}

#[test]
fn an_imprint_in_a_group_or_a_node_does_nothing() {
    let mut h = History::new(Document::new());
    let board = add_part(&mut h, at(100.0, 50.0, 20.0), moved(at(5.0, 0.0, 0.0)));
    let part = part_of(&h, board);
    let blank = h.document().part(part).unwrap().bodies[0].id;
    let (imprint, _) = add_body(&mut h, board, at(90.0, 10.0, 5.0), at(30.0, 10.0, 10.0), Sign::Imprint);
    let target = EntityPath::root(board);

    let group =
        h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![blank, imprint], scope: Scope::All }).unwrap().bodies[0];
    assert!(close(volume(&h, part), 100.0 * 50.0 * 20.0), "a group");

    h.apply(Operation::Blend {
        target,
        bodies: vec![group],
        blend: Blend::Chamfer,
        size: 2.0,
        picks: vec![at(50.0, 0.0, 20.0)],
        scope: Scope::All,
    })
    .unwrap();
    let chamfered = 100.0 * 50.0 * 20.0 - 0.5 * 2.0 * 2.0 * 100.0;
    assert!(close(volume(&h, part), chamfered), "a node over it: {}", volume(&h, part));
}

#[test]
fn imprints_grouped_are_one_and_explode_as_imprints() {
    let mut h = History::new(Document::new());
    let board = add_part(&mut h, at(100.0, 50.0, 20.0), DAffine3::IDENTITY);
    let part = part_of(&h, board);
    let (a, _) = add_body(&mut h, board, at(100.0, 5.0, 5.0), DVec3::splat(5.0), Sign::Imprint);
    let (b, _) = add_body(&mut h, board, at(100.0, 15.0, 5.0), DVec3::splat(5.0), Sign::Imprint);
    let target = EntityPath::root(board);
    let group = h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![a, b], scope: Scope::All }).unwrap().bodies[0];
    let p = h.document().part(part).unwrap();
    assert_eq!(p.body(group).unwrap().sign, Sign::Imprint);
    assert!(p.def_of(p.body(group).unwrap()).children().iter().all(|c| c.sign == Sign::Positive));
    assert!(close(volume(&h, part), 100.0 * 50.0 * 20.0));

    // A body neither goes into it nor comes out.
    let blank = p.positives().next().unwrap().id;
    let into = Operation::MoveBodies {
        from: target.clone(),
        bodies: vec![blank],
        to: BodiesTo::Level(vec![group]),
        copy: false,
        scope: Scope::All,
    };
    refused(h.apply(into), Refusal::ImprintLevel);

    h.apply(Operation::UngroupBodies { target, bodies: vec![group], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.bodies.iter().filter(|b| b.sign == Sign::Imprint).count(), 2);
    assert!(close(volume(&h, part), 100.0 * 50.0 * 20.0));
}

#[test]
fn an_imprint_stays_one_when_a_machining_group_explodes() {
    let mut h = History::new(Document::new());
    let board = add_part(&mut h, at(100.0, 50.0, 20.0), DAffine3::IDENTITY);
    let part = part_of(&h, board);
    let (drill, _) = add_body(&mut h, board, at(10.0, 10.0, -5.0), at(10.0, 10.0, 30.0), Sign::Positive);
    let (imprint, _) = add_body(&mut h, board, at(50.0, 10.0, -5.0), at(10.0, 10.0, 30.0), Sign::Imprint);
    let target = EntityPath::root(board);
    let group =
        h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![drill, imprint], scope: Scope::All }).unwrap().bodies[0];
    h.apply(Operation::SetSign { target: target.clone(), bodies: vec![group], sign: Sign::Negative, scope: Scope::All }).unwrap();
    let hole = 10.0 * 10.0 * 20.0;
    assert!(close(volume(&h, part), 100.0 * 50.0 * 20.0 - hole), "only the drill cuts");

    h.apply(Operation::UngroupBodies { target, bodies: vec![group], scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!((p.negatives().count(), p.bodies.iter().filter(|b| b.sign == Sign::Imprint).count()), (1, 1));
    assert!(close(volume(&h, part), 100.0 * 50.0 * 20.0 - hole));
}

#[test]
fn welding_ignores_an_imprint_and_never_moves_a_linked_body() {
    let mut h = History::new(Document::new());
    let board = add_part(&mut h, at(100.0, 50.0, 20.0), DAffine3::IDENTITY);
    add_body(&mut h, board, at(120.0, 0.0, 100.0), at(10.0, 10.0, 10.0), Sign::Imprint);
    // Its top a hair under the imprint's bottom plane, far from anything else.
    let (_, o) = add_body(&mut h, board, at(120.0, 0.0, 95.0 - 5e-5), at(10.0, 10.0, 5.0), Sign::Positive);
    assert!(o.welds.is_empty(), "{:?}", o.welds);

    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let body = h.apply(link_op(&h, rail, imprint, leg, None)).unwrap().bodies[0];
    // A linked body with a profile, a hair off the leg's face: it stays where it is.
    let part = part_of(&h, leg);
    let mut doc = h.document().clone();
    let profile = crate::BodyProfile::of(&cuboid(DVec3::ZERO, DVec3::splat(10.0)).at_origin().0);
    let p = doc.part_mut(part).unwrap();
    let def = p.body(body).unwrap().def;
    p.defs.iter_mut().find(|d| d.id == def).unwrap().profile = profile;
    let hair = moved(at(5e-5, 0.0, 0.0));
    let op = Operation::TransformBodies { target: EntityPath::root(leg), bodies: vec![body], transform: hair, scope: Scope::All };
    let (_, outcome) = doc.apply(&op).unwrap();
    assert!(outcome.welds.is_empty(), "{:?}", outcome.welds);
}

#[test]
fn flattening_a_part_keeps_its_imprints() {
    let mut h = History::new(Document::new());
    let board = add_part(&mut h, at(100.0, 50.0, 20.0), DAffine3::IDENTITY);
    let part = part_of(&h, board);
    add_body(&mut h, board, at(10.0, 10.0, -5.0), at(10.0, 10.0, 30.0), Sign::Negative);
    let (imprint, _) = add_body(&mut h, board, at(100.0, 5.0, 5.0), DVec3::splat(5.0), Sign::Imprint);
    let before = volume(&h, part);
    h.apply(Operation::FlattenPart { target: EntityPath::root(board), scope: Scope::All }).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.bodies.len(), 2);
    assert_eq!(p.body(imprint).unwrap().sign, Sign::Imprint);
    assert!(close(volume(&h, part), before));
    Document::from_file(&h.document().to_file().unwrap()).unwrap();
}

#[test]
fn a_union_leaves_the_imprints_at_the_root() {
    let mut h = History::new(Document::new());
    let board = add_part(&mut h, at(100.0, 50.0, 20.0), DAffine3::IDENTITY);
    let part = part_of(&h, board);
    add_body(&mut h, board, at(10.0, 10.0, -5.0), at(10.0, 10.0, 30.0), Sign::Negative);
    let (imprint, _) = add_body(&mut h, board, at(100.0, 5.0, 5.0), DVec3::splat(5.0), Sign::Imprint);
    let tool = add_part(&mut h, at(10.0, 50.0, 20.0), moved(at(100.0, 0.0, 0.0)));
    h.apply(Operation::boolean(EntityPath::root(board), EntityPath::root(tool), Boolean::Union, Scope::All)).unwrap();
    let p = h.document().part(part).unwrap();
    assert_eq!(p.body(imprint).map(|b| b.sign), Some(Sign::Imprint));
    assert!(close(volume(&h, part), 110.0 * 50.0 * 20.0 - 10.0 * 10.0 * 20.0));
    Document::from_file(&h.document().to_file().unwrap()).unwrap();
}

// ---- a link makes a linked body ------------------------------------------------------------

#[test]
fn a_link_cuts_the_imprint_in_the_receiver() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let (rail_part, leg_part) = (part_of(&h, rail), part_of(&h, leg));
    let rail_volume = volume(&h, rail_part);

    let body = h.apply(link_op(&h, rail, imprint, leg, None)).unwrap().bodies[0];
    assert!(close(volume(&h, leg_part), LEG_VOLUME - IMPRINT_VOLUME));
    assert!(close(volume(&h, rail_part), rail_volume), "the source is left as it is");

    let q = h.document().part(leg_part).unwrap();
    let b = q.body(body).unwrap();
    assert_eq!((b.sign, q.def_of(b).source), (Sign::Negative, BodySource::Linked));
    let links = root_links(&h);
    assert_eq!(links.len(), 1);
    assert_eq!((links[0].imprint, links[0].body, links[0].seat), (imprint, body, b.placement));
    assert_eq!(states(&h), vec![LinkState::Seated]);

    // One step of undo takes the body and its link.
    assert!(h.undo());
    assert!(root_links(&h).is_empty());
    assert!(close(volume(&h, leg_part), LEG_VOLUME));
    assert!(h.redo());
    assert_eq!(states(&h), vec![LinkState::Seated]);
}

#[test]
fn two_links_share_a_body_through_a_mirrored_receiver() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let (rail_part, leg_part) = (part_of(&h, rail), part_of(&h, leg));
    let body = h.apply(link_op(&h, rail, imprint, leg, None)).unwrap().bodies[0];

    // The other leg, mirrored; the other rail seen through that mirror, turned a half about the
    // imprint's axis: its pose differs, its box is the same.
    let root = h.document().root();
    let mirror = DAffine3::from_scale(at(-1.0, 1.0, 1.0));
    let leg_t = h.document().resolve(&EntityPath::root(leg)).unwrap().transform;
    let place = |h: &mut History, component, transform| {
        h.apply(Operation::PlaceOccurrence { context: root, component, transform }).unwrap().created[0]
    };
    let leg2 = place(&mut h, leg_part, mirror * leg_t);
    let centre = at(105.0, 10.0, 10.0);
    let half = moved(centre) * DAffine3::from_rotation_x(std::f64::consts::PI) * moved(-centre);
    let rail2 = place(&mut h, rail_part, mirror * half);

    h.apply(link_op(&h, rail2, imprint, leg2, Some(body))).unwrap();
    let links = root_links(&h);
    assert_eq!(links.len(), 2);
    assert_ne!(links[0].seat, links[1].seat, "each link keeps its own pose");
    assert_eq!(links[1].body, body);
    assert_eq!(states(&h), vec![LinkState::Seated, LinkState::Seated]);
    let q = h.document().part(leg_part).unwrap();
    assert_eq!(q.negatives().count(), 1);
    assert!(close(volume(&h, leg_part), LEG_VOLUME - IMPRINT_VOLUME));

    // Onto a body somewhere else: refused.
    let rail3 = place(&mut h, rail_part, moved(DVec3::Z));
    refused(h.apply(link_op(&h, rail3, imprint, leg, Some(body))), Refusal::BadLink);
    assert_eq!(root_links(&h).len(), 2);
}

#[test]
fn a_link_is_refused_without_an_imprint_or_an_acting_sign() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let root = h.document().root();
    let blank = h.document().part(part_of(&h, rail)).unwrap().positives().next().unwrap().id;

    refused(h.apply(link_op(&h, rail, blank, leg, None)), Refusal::BadLink);
    refused(h.apply(link_op(&h, rail, imprint, rail, None)), Refusal::BadLink);
    let op = Operation::AddLink { context: root, source: vec![rail], imprint, receiver: vec![leg], sign: Sign::Imprint, onto: None };
    refused(h.apply(op), Refusal::BadLink);
    let op = Operation::AddLink { context: root, source: vec![], imprint, receiver: vec![leg], sign: Sign::Negative, onto: None };
    refused(h.apply(op), Refusal::BadLink);

    // An imprint inside a group is no source yet.
    let target = EntityPath::root(rail);
    let group = h.apply(Operation::GroupBodies { target, bodies: vec![blank, imprint], scope: Scope::All }).unwrap().bodies[0];
    assert_ne!(group, imprint);
    refused(h.apply(link_op(&h, rail, imprint, leg, None)), Refusal::BadLink);
}

#[test]
fn an_imprint_never_holds_a_linked_body() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let body = h.apply(link_op(&h, rail, imprint, leg, None)).unwrap().bodies[0];
    let target = EntityPath::root(leg);
    let set = |bodies, sign| Operation::SetSign { target: target.clone(), bodies, sign, scope: Scope::All };
    refused(h.apply(set(vec![body], Sign::Imprint)), Refusal::ImprintLinked);

    let blank = h.document().part(part_of(&h, leg)).unwrap().positives().next().unwrap().id;
    let group = h.apply(Operation::GroupBodies { target: target.clone(), bodies: vec![blank, body], scope: Scope::All }).unwrap().bodies[0];
    refused(h.apply(set(vec![group], Sign::Imprint)), Refusal::ImprintLinked);
}

#[test]
fn a_link_is_seated_off_or_broken() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    h.apply(link_op(&h, rail, imprint, leg, None)).unwrap();
    let root = h.document().root();
    let shift = |id, v| Operation::Transform { context: root, entities: vec![id], transform: moved(v) };

    // The receiver moved: off, seated again once the source lands there.
    h.apply(shift(leg, DVec3::X)).unwrap();
    assert_eq!(states(&h), vec![LinkState::Off]);
    h.apply(shift(rail, DVec3::X)).unwrap();
    assert_eq!(states(&h), vec![LinkState::Seated], "both moved alike");

    let deleted = |h: &mut History, op| {
        h.apply(op).unwrap();
        assert_eq!(states(h), vec![LinkState::Broken]);
        assert!(h.undo());
    };
    deleted(&mut h, Operation::DeleteBodies { target: EntityPath::root(rail), bodies: vec![imprint], scope: Scope::All });
    let body = root_links(&h)[0].body;
    deleted(&mut h, Operation::DeleteBodies { target: EntityPath::root(leg), bodies: vec![body], scope: Scope::All });
    deleted(&mut h, Operation::Delete { context: root, entities: vec![rail] });
    deleted(&mut h, Operation::Delete { context: root, entities: vec![leg] });
    assert_eq!(states(&h), vec![LinkState::Seated]);
}

// ---- the file, copies ----------------------------------------------------------------------

#[test]
fn links_and_imprints_are_saved() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let body = h.apply(link_op(&h, rail, imprint, leg, None)).unwrap().bodies[0];
    let read = Document::from_file(&h.document().to_file().unwrap()).unwrap();
    assert_eq!(read.root_assembly().links(), h.document().root_assembly().links());
    assert_eq!(read.link_state(read.root(), &read.root_assembly().links()[0]), LinkState::Seated);
    assert_eq!(read.part(part_of(&h, rail)).unwrap().body(imprint).unwrap().sign, Sign::Imprint);
    let q = read.part(part_of(&h, leg)).unwrap();
    assert_eq!(q.def_of(q.body(body).unwrap()).source, BodySource::Linked);
    assert!(close(q.shape().volume().unwrap(), LEG_VOLUME - IMPRINT_VOLUME));
}

#[test]
fn a_file_with_a_malformed_link_is_refused() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    h.apply(link_op(&h, rail, imprint, leg, None)).unwrap();
    let root = h.document().root();

    let mut doc = h.document().clone();
    doc.assembly_mut(root).unwrap().links_mut()[0].seat = DAffine3::from_scale(DVec3::splat(2.0));
    let id = doc.root_assembly().links()[0].id;
    assert!(matches!(Document::from_file(&doc.to_file().unwrap()), Err(Error::File(FileError::BadLink(i))) if i == id));

    let mut doc = h.document().clone();
    doc.assembly_mut(root).unwrap().links_mut()[0].source.clear();
    assert!(matches!(Document::from_file(&doc.to_file().unwrap()), Err(Error::File(FileError::BadLink(_)))));

    // A path leading nowhere is a broken link, not a bad file.
    let mut doc = h.document().clone();
    doc.assembly_mut(root).unwrap().links_mut()[0].source = vec![Id(0)];
    let read = Document::from_file(&doc.to_file().unwrap()).unwrap();
    assert_eq!(read.link_state(root, &read.root_assembly().links()[0]), LinkState::Broken);
}

#[test]
fn a_copied_assembly_keeps_its_links() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let root = h.document().root();
    let cabinet = h.apply(Operation::MakeAssembly { context: root, entities: vec![rail, leg], name: None }).unwrap().created[0];
    let assembly = h.document().root_assembly().occurrence(cabinet).unwrap().component;
    let op = Operation::AddLink { context: assembly, source: vec![rail], imprint, receiver: vec![leg], sign: Sign::Negative, onto: None };
    h.apply(op).unwrap();

    // Placed again: the same definition, its links with it.
    h.apply(Operation::Duplicate { context: root, entities: vec![cabinet], transform: moved(at(500.0, 0.0, 0.0)) }).unwrap();
    let a = h.document().assembly(assembly).unwrap();
    assert_eq!(h.document().link_state(assembly, &a.links()[0]), LinkState::Seated);

    // Copied into another document: every id drawn afresh, the link following them.
    let mut other = Document::new();
    other.fresh_id();
    other.graft(h.document(), false).unwrap();
    let copy = other.components().find(|c| c.as_assembly().is_some_and(|a| !a.links().is_empty())).unwrap();
    let links = copy.as_assembly().unwrap().links();
    assert_eq!(links.len(), 1);
    assert_ne!(links[0].source, vec![rail]);
    assert_eq!(other.link_state(copy.id, &links[0]), LinkState::Seated);
}

#[test]
fn a_linked_body_copied_to_another_part_is_a_shape_of_its_own() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let body = h.apply(link_op(&h, rail, imprint, leg, None)).unwrap().bodies[0];
    let o = h
        .apply(Operation::MoveBodies {
            from: EntityPath::root(leg),
            bodies: vec![body],
            to: BodiesTo::NewPart(Vec::new()),
            copy: true,
            scope: Scope::All,
        })
        .unwrap();
    let made = o.created[0];
    let p = h.document().part(part_of(&h, made)).unwrap();
    assert!(p.bodies.iter().all(|b| p.def_of(b).source == BodySource::Local));
    assert_eq!(states(&h), vec![LinkState::Seated], "the link stays with the body it was");
}

// ---- following: a table of four legs of one definition, four rails with two imprints ------

const LEG: f64 = 60.0;
const RAIL_W: f64 = 22.0;
const RAIL_H: f64 = 100.0;
const RAIL_Z: f64 = 590.0;
/// How far an imprint enters the leg, how far it runs back into the rail.
const DEPTH: f64 = 24.0;
const BACK: f64 = 30.0;
const SECTION: (f64, f64) = (10.2, 70.2);

fn full_leg() -> f64 {
    LEG * LEG * 700.0 - 2.0 * DEPTH * SECTION.0 * SECTION.1
}

struct Table {
    h: History,
    legs: Vec<Id>,
    rails: Vec<Id>,
    leg_part: Id,
}

/// One link per `(rail, end, leg, slot)`, each slot's first link making its body.
fn table(legs: Vec<DAffine3>, kinds: &[f64], rails: &[(usize, DAffine3)], seats: &[(usize, usize, usize, usize)]) -> Table {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let leg_ids: Vec<Id> = {
        let first = add_part(&mut h, at(LEG, LEG, 700.0), legs[0]);
        let part = part_of(&h, first);
        let mut ids = vec![first];
        for t in &legs[1..] {
            ids.push(h.apply(Operation::PlaceOccurrence { context: root, component: part, transform: *t }).unwrap().created[0]);
        }
        ids
    };
    let leg_part = part_of(&h, leg_ids[0]);
    let mut made: Vec<Option<(Id, [Id; 2])>> = vec![None; kinds.len()];
    let mut rail_ids = Vec::new();
    let mut imprints = Vec::new();
    for &(kind, t) in rails {
        let length = kinds[kind];
        let id = match made[kind] {
            Some((part, ends)) => {
                imprints.push(ends);
                h.apply(Operation::PlaceOccurrence { context: root, component: part, transform: t }).unwrap().created[0]
            }
            None => {
                let occ = add_part(&mut h, at(length, RAIL_W, RAIL_H), t);
                let (y, z) = ((RAIL_W - SECTION.0) / 2.0, (RAIL_H - SECTION.1) / 2.0);
                let size = at(DEPTH + BACK, SECTION.0, SECTION.1);
                let (a, _) = add_body(&mut h, occ, at(-DEPTH, y, z), size, Sign::Imprint);
                let (b, _) = add_body(&mut h, occ, at(length - BACK, y, z), size, Sign::Imprint);
                made[kind] = Some((part_of(&h, occ), [a, b]));
                imprints.push([a, b]);
                occ
            }
        };
        rail_ids.push(id);
    }
    let mut slots: [Option<Id>; 2] = [None; 2];
    for &(r, end, l, slot) in seats {
        let op = link_op(&h, rail_ids[r], imprints[r][end], leg_ids[l], slots[slot]);
        slots[slot] = Some(h.apply(op).unwrap().bodies[0]);
    }
    Table { h, legs: leg_ids, rails: rail_ids, leg_part }
}

/// Four legs, each the last one turned a quarter about the table's centre.
fn turned_table() -> Table {
    let turn = |k: usize| DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2 * k as f64);
    let legs = (0..4).map(|k| turn(k) * moved(at(-360.0, -360.0, 0.0))).collect();
    let rails: Vec<_> = (0..4).map(|k| (0, turn(k) * moved(at(-300.0, -341.0, RAIL_Z)))).collect();
    let seats: Vec<_> = (0..4).flat_map(|k| [(k, 0, k, 0), (k, 1, (k + 1) % 4, 1)]).collect();
    table(legs, &[600.0], &rails, &seats)
}

/// A rectangular table, its legs mirrored across its axes, two long and two short rails.
fn mirrored_table() -> Table {
    let (a, b) = (460.0, 310.0);
    let (mx, my) = (DAffine3::from_scale(at(-1.0, 1.0, 1.0)), DAffine3::from_scale(at(1.0, -1.0, 1.0)));
    let leg = moved(at(-a, -b, 0.0));
    let half = DAffine3::from_rotation_z(std::f64::consts::PI);
    let front = moved(at(-a + LEG, -b + (LEG - RAIL_W) / 2.0, RAIL_Z));
    let left = moved(at(-a + (LEG + RAIL_W) / 2.0, -b + LEG, RAIL_Z)) * DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    let rails = [(0, front), (1, half * left), (0, half * front), (1, left)];
    let seats = [(0, 0, 0, 0), (0, 1, 1, 0), (2, 0, 2, 0), (2, 1, 3, 0), (3, 0, 0, 1), (3, 1, 3, 1), (1, 0, 2, 1), (1, 1, 1, 1)];
    table(vec![leg, mx * leg, mx * my * leg, my * leg], &[800.0, 500.0], &rails, &seats)
}

impl Table {
    fn shift(&mut self, rails: &[usize], v: DVec3) -> Followed {
        let context = self.h.document().root();
        let entities = rails.iter().map(|k| self.rails[*k]).collect();
        self.h.apply(Operation::Transform { context, entities, transform: moved(v) }).unwrap().links
    }

    /// The rails with a link off.
    fn off(&self) -> Vec<usize> {
        let doc = self.h.document();
        let links = doc.root_assembly().links();
        (0..self.rails.len())
            .filter(|k| links.iter().any(|l| l.source == [self.rails[*k]] && doc.link_state(doc.root(), l) != LinkState::Seated))
            .collect()
    }

    fn mortises(&self) -> Vec<DAffine3> {
        self.h.document().part(self.leg_part).unwrap().negatives().map(|b| b.placement).collect()
    }

    fn leg_volume(&self) -> f64 {
        volume(&self.h, self.leg_part)
    }
}

#[test]
fn a_table_of_shared_legs_is_seated() {
    for t in [turned_table(), mirrored_table()] {
        assert!(t.off().is_empty());
        assert_eq!(t.h.document().root_assembly().links().len(), 8);
        assert_eq!(t.mortises().len(), 2, "one body per mortise, in the one definition");
        assert!(close(t.leg_volume(), full_leg()), "{}", t.leg_volume());
    }
}

#[test]
fn a_rail_moved_carries_its_mortises_in_every_leg() {
    let mut t = turned_table();
    let before = t.mortises();
    let steps = t.h.journal().count();
    let followed = t.shift(&[0], DVec3::Z * 10.0);
    assert_eq!((followed.moved.len(), followed.conflicts.len()), (2, 0));
    assert_eq!(t.h.journal().count(), steps + 1, "one step");
    assert_eq!(t.off(), vec![1, 2, 3], "the other rails are told, not moved");
    let after = t.mortises();
    assert!(before.iter().zip(&after).all(|(a, b)| (b.translation - a.translation - DVec3::Z * 10.0).length() < 1e-9));
    assert!(close(t.leg_volume(), full_leg()));
    let parts: std::collections::BTreeSet<Id> = t.legs.iter().map(|l| part_of(&t.h, *l)).collect();
    assert_eq!(parts.len(), 1, "the legs stay one definition");

    // By hand, each lands on its seat; the last one back, all are seated.
    for k in 1..4 {
        assert!(t.shift(&[k], DVec3::Z * 10.0).moved.is_empty(), "a rail off drives nothing");
    }
    assert!(t.off().is_empty());

    // One undo takes one step: the last rail, then back to the move that carried the mortises.
    for _ in 0..4 {
        assert!(t.h.undo());
    }
    assert_eq!(t.mortises(), before);
    assert!(t.off().is_empty());
}

#[test]
fn a_rail_off_placed_roughly_leaves_the_mortise_under_the_others() {
    let mut t = turned_table();
    t.shift(&[0], DVec3::Z * 10.0);
    let mortises = t.mortises();
    assert!(t.shift(&[1], DVec3::Z * 3.0).moved.is_empty());
    assert_eq!(t.mortises(), mortises);
    assert!(!t.off().contains(&0));
    assert!(t.off().contains(&1));
    t.shift(&[1], DVec3::Z * 7.0);
    assert!(!t.off().contains(&1));
}

#[test]
fn every_rail_moved_alike_stays_seated() {
    let mut t = turned_table();
    let followed = t.shift(&[0, 1, 2, 3], DVec3::Z * -25.0);
    assert_eq!((followed.moved.len(), followed.conflicts.len()), (2, 0));
    assert!(t.off().is_empty());
    assert!(close(t.leg_volume(), full_leg()));
}

#[test]
fn one_vector_across_turned_legs_asks_a_mortise_two_moves() {
    let mut t = turned_table();
    let before = t.mortises();
    let followed = t.shift(&[0, 1, 2, 3], DVec3::X * 5.0);
    assert_eq!((followed.moved.len(), followed.conflicts.len()), (0, 2));
    assert_eq!(t.mortises(), before);
    assert_eq!(t.off(), vec![0, 1, 2, 3]);
}

#[test]
fn mirrored_legs_a_rail_raised_moves_its_own_mortise() {
    let mut t = mirrored_table();
    let followed = t.shift(&[0], DVec3::Z * 10.0);
    assert_eq!((followed.moved.len(), followed.conflicts.len()), (1, 0));
    assert_eq!(t.off(), vec![2], "the other long rail; the short ones keep their mortise");
    assert!(close(t.leg_volume(), full_leg()));
}

#[test]
fn mirrored_legs_a_rail_slid_along_itself_is_a_conflict() {
    let mut t = mirrored_table();
    let before = t.mortises();
    let followed = t.shift(&[0], DVec3::X * 5.0);
    assert_eq!((followed.moved.len(), followed.conflicts.len()), (0, 1));
    assert_eq!(t.mortises(), before);
    assert_eq!(t.off(), vec![0]);

    // One leg made a part of its own: each end has its mortise.
    let mut t = mirrored_table();
    let root = t.h.document().root();
    t.h.apply(Operation::MakeUnique { context: root, occurrence: t.legs[1] }).unwrap();
    let followed = t.shift(&[0], DVec3::X * 5.0);
    assert_eq!((followed.moved.len(), followed.conflicts.len()), (2, 0));
    assert!(!t.off().contains(&0));
    assert_ne!(part_of(&t.h, t.legs[0]), part_of(&t.h, t.legs[1]));
}

#[test]
fn a_rail_pulled_out_leaves_the_mortises() {
    let mut t = turned_table();
    let before = t.mortises();
    let followed = t.shift(&[0], DVec3::Y * -200.0);
    assert!(followed.moved.is_empty() && followed.conflicts.is_empty());
    assert_eq!(t.mortises(), before);
    assert_eq!(t.off(), vec![0]);
    t.shift(&[0], DVec3::Y * 200.0);
    assert!(t.off().is_empty());
}

#[test]
fn a_leg_moved_keeps_its_mortises_and_drives_nothing() {
    let mut t = turned_table();
    let before = t.mortises();
    let root = t.h.document().root();
    let op = Operation::Transform { context: root, entities: vec![t.legs[0]], transform: moved(DVec3::Z * 10.0) };
    assert!(matches!(t.h.document().prepare(&op).unwrap(), Prepared::Done(_)), "no kernel");
    let followed = t.h.apply(op).unwrap().links;
    assert!(followed.moved.is_empty());
    assert_eq!(t.mortises(), before);
    assert_eq!(t.off(), vec![0, 3], "the two rails entering it");
}

#[test]
fn an_imprint_moved_in_its_part_carries_the_mortises() {
    let mut t = turned_table();
    let imprint = t.h.document().root_assembly().links()[0].imprint;
    let before = t.mortises();
    let target = EntityPath::root(t.rails[0]);
    let o =
        t.h.apply(Operation::TransformBodies { target, bodies: vec![imprint], transform: moved(DVec3::Z * 4.0), scope: Scope::All })
            .unwrap();
    assert_eq!((o.links.moved.len(), o.links.conflicts.len()), (1, 0), "every rail's, one change");
    assert!(t.off().is_empty());
    assert_ne!(t.mortises(), before);
    assert!(close(t.leg_volume(), full_leg()));
}

#[test]
fn changing_axes_leaves_the_links_seated() {
    for moved_part in [false, true] {
        let mut t = turned_table();
        let component = if moved_part { part_of(&t.h, t.rails[0]) } else { t.leg_part };
        let axes = moved(at(5.0, -3.0, 7.0)) * DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
        let o = t.h.apply(Operation::ChangeAxes { component, axes }).unwrap();
        assert!(o.links.moved.is_empty());
        assert!(t.off().is_empty(), "axes of the {}", if moved_part { "source" } else { "receiver" });
        assert!(close(t.leg_volume(), full_leg()));
    }
}

#[test]
fn a_body_or_an_imprint_changed_is_off() {
    let mut t = turned_table();
    let body = t.h.document().root_assembly().links()[0].body;
    let target = EntityPath::root(t.legs[0]);
    t.h.apply(Operation::TransformBodies { target, bodies: vec![body], transform: moved(DVec3::Z), scope: Scope::All }).unwrap();
    assert_eq!(t.off(), vec![0, 1, 2, 3], "the mortise moved by hand: its rails no longer sit in it");

    let t = turned_table();
    let (doc, imprint) = (t.h.document(), doc_imprint(&t));
    let mut doc = doc.clone();
    let rail = part_of(&t.h, t.rails[0]);
    let other = Arc::new(cuboid(DVec3::ZERO, at(DEPTH + BACK + 1.0, SECTION.0, SECTION.1)).build().unwrap());
    let p = doc.part_mut(rail).unwrap();
    let def = p.body(imprint).unwrap().def;
    p.defs.iter_mut().find(|d| d.id == def).unwrap().solid = other;
    let root = doc.root();
    assert_eq!(doc.link_state(root, &doc.root_assembly().links()[0]), LinkState::Off, "its shape does not follow yet");
}

fn doc_imprint(t: &Table) -> Id {
    t.h.document().root_assembly().links()[0].imprint
}

// ---- links through changes of structure -----------------------------------------------------

#[test]
fn gathering_the_two_ends_lowers_the_link_and_a_copy_follows() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    h.apply(link_op(&h, rail, imprint, leg, None)).unwrap();
    let root = h.document().root();
    let cabinet = h.apply(Operation::MakeAssembly { context: root, entities: vec![rail, leg], name: None }).unwrap().created[0];
    let assembly = h.document().root_assembly().occurrence(cabinet).unwrap().component;
    assert!(h.document().root_assembly().links().is_empty());
    let a = h.document().assembly(assembly).unwrap();
    assert_eq!((a.links().len(), a.links()[0].source.clone(), a.links()[0].receiver.clone()), (1, vec![rail], vec![leg]));
    assert_eq!(h.document().link_state(assembly, &a.links()[0]), LinkState::Seated);

    // Moved inside it, the body follows, in both copies of the assembly.
    h.apply(Operation::Duplicate { context: root, entities: vec![cabinet], transform: moved(at(500.0, 0.0, 0.0)) }).unwrap();
    let o = h.apply(Operation::Transform { context: assembly, entities: vec![rail], transform: moved(DVec3::Z * 3.0) }).unwrap();
    assert_eq!(o.links.moved.len(), 1);
    let a = h.document().assembly(assembly).unwrap();
    assert_eq!(h.document().link_state(assembly, &a.links()[0]), LinkState::Seated);
}

#[test]
fn gathering_one_end_keeps_the_link_where_it_is() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    h.apply(link_op(&h, rail, imprint, leg, None)).unwrap();
    let root = h.document().root();
    let made = h.apply(Operation::MakeAssembly { context: root, entities: vec![rail], name: None }).unwrap().created[0];
    let links = root_links(&h);
    assert_eq!((links[0].source.clone(), links[0].receiver.clone()), (vec![made, rail], vec![leg]));
    assert_eq!(states(&h), vec![LinkState::Seated]);

    // The assembly moved carries the rail, and so the mortise.
    let o = h.apply(Operation::Transform { context: root, entities: vec![made], transform: moved(DVec3::Z * 2.0) }).unwrap();
    assert_eq!(o.links.moved.len(), 1);
    assert_eq!(states(&h), vec![LinkState::Seated]);
}

#[test]
fn exploding_an_assembly_brings_its_links_up() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    h.apply(link_op(&h, rail, imprint, leg, None)).unwrap();
    let root = h.document().root();
    let cabinet = h.apply(Operation::MakeAssembly { context: root, entities: vec![rail, leg], name: None }).unwrap().created[0];
    let made = h.apply(Operation::Explode { context: root, occurrence: cabinet }).unwrap().created;
    let links = root_links(&h);
    assert_eq!(links.len(), 1);
    assert!(made.contains(&links[0].source[0]) && made.contains(&links[0].receiver[0]));
    assert_eq!(states(&h), vec![LinkState::Seated]);

    // Exploding the assembly holding one end only.
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    h.apply(link_op(&h, rail, imprint, leg, None)).unwrap();
    let root = h.document().root();
    let group = h.apply(Operation::MakeAssembly { context: root, entities: vec![leg], name: None }).unwrap().created[0];
    assert_eq!(root_links(&h)[0].receiver, vec![group, leg]);
    let made = h.apply(Operation::Explode { context: root, occurrence: group }).unwrap().created;
    assert_eq!(root_links(&h)[0].receiver, made);
    assert_eq!(states(&h), vec![LinkState::Seated]);
}

#[test]
fn an_occurrence_moved_between_assemblies_keeps_its_links() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let spare = add_part(&mut h, DVec3::splat(10.0), moved(at(500.0, 0.0, 0.0)));
    h.apply(link_op(&h, rail, imprint, leg, None)).unwrap();
    let root = h.document().root();
    let group = h.apply(Operation::MakeAssembly { context: root, entities: vec![spare], name: None }).unwrap().created[0];

    // Into the assembly: the path goes through it.
    h.apply(Operation::MoveOccurrences { instances: vec![], occurrences: vec![rail], to: vec![group], copy: false }).unwrap();
    assert_eq!(root_links(&h)[0].source, vec![group, rail]);
    assert_eq!(states(&h), vec![LinkState::Seated]);

    // The other end too: the link goes down with them.
    h.apply(Operation::MoveOccurrences { instances: vec![], occurrences: vec![leg], to: vec![group], copy: false }).unwrap();
    assert!(root_links(&h).is_empty());
    let assembly = h.document().root_assembly().occurrence(group).unwrap().component;
    let a = h.document().assembly(assembly).unwrap();
    assert_eq!((a.links()[0].source.clone(), a.links()[0].receiver.clone()), (vec![rail], vec![leg]));
    assert_eq!(h.document().link_state(assembly, &a.links()[0]), LinkState::Seated);

    // One back out: the link comes up to the assembly above both.
    h.apply(Operation::MoveOccurrences { instances: vec![group], occurrences: vec![rail], to: vec![], copy: false }).unwrap();
    assert!(h.document().assembly(assembly).unwrap().links().is_empty());
    let links = root_links(&h);
    assert_eq!((links[0].source.clone(), links[0].receiver.clone()), (vec![rail], vec![group, leg]));
    assert_eq!(states(&h), vec![LinkState::Seated]);
}

#[test]
fn the_linked_table_is_saved_after_following() {
    let mut t = turned_table();
    t.shift(&[0], DVec3::Z * 10.0);
    let read = Document::from_file(&t.h.document().to_file().unwrap()).unwrap();
    let root = read.root();
    let states: Vec<LinkState> = read.root_assembly().links().iter().map(|l| read.link_state(root, l)).collect();
    assert_eq!(states.iter().filter(|s| **s == LinkState::Seated).count(), 2);
    assert_eq!(states.iter().filter(|s| **s == LinkState::Off).count(), 6);
}

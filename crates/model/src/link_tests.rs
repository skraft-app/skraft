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

    // The source moved, then the receiver: off, seated again once back.
    h.apply(shift(rail, DVec3::Z)).unwrap();
    assert_eq!(states(&h), vec![LinkState::Off]);
    h.apply(shift(rail, -DVec3::Z)).unwrap();
    assert_eq!(states(&h), vec![LinkState::Seated]);
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

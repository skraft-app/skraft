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

    // Its source gone: broken, and its body told frozen.
    let source_gone = |h: &mut History, op| {
        h.apply(op).unwrap();
        assert_eq!(states(h), vec![LinkState::Broken]);
        assert_eq!(h.document().link_status().broken.len(), 1);
        assert!(h.undo());
    };
    source_gone(&mut h, Operation::DeleteBodies { target: EntityPath::root(rail), bodies: vec![imprint], scope: Scope::All });
    source_gone(&mut h, Operation::Delete { context: root, entities: vec![rail] });

    // Its body or its receiver gone: nothing left to show, the link goes.
    let body = root_links(&h)[0].body;
    let receiver_gone = |h: &mut History, op| {
        h.apply(op).unwrap();
        assert!(root_links(h).is_empty());
        assert!(h.undo());
    };
    receiver_gone(&mut h, Operation::DeleteBodies { target: EntityPath::root(leg), bodies: vec![body], scope: Scope::All });
    receiver_gone(&mut h, Operation::Delete { context: root, entities: vec![leg] });
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

// ---- what is done about links off and broken ------------------------------------------------

impl Table {
    fn source(&self, rail: usize) -> LinkSource {
        LinkSource { assembly: self.h.document().root(), occurrence: self.rails[rail] }
    }

    fn sources(&self, rails: &[usize]) -> Vec<LinkSource> {
        rails.iter().map(|k| self.source(*k)).collect()
    }

    fn legs_parts(&self) -> usize {
        self.legs.iter().map(|l| part_of(&self.h, *l)).collect::<std::collections::BTreeSet<_>>().len()
    }

    fn clear_of_every_leg(&self) -> bool {
        let doc = self.h.document();
        self.rails.iter().all(|r| {
            let (_, rail, tr) = doc.resolve_part(&EntityPath::root(*r)).unwrap();
            self.legs.iter().all(|l| {
                let (_, leg, tl) = doc.resolve_part(&EntityPath::root(*l)).unwrap();
                let placed = rail.shape().transformed(&(tl.inverse() * tr)).unwrap();
                match leg.shape().boolean(&placed, skraft_kernel::BooleanOp::Intersection) {
                    Ok(common) => common.volume().unwrap() < 1e-6,
                    Err(skraft_kernel::Error::Empty(_)) => true,
                    Err(e) => panic!("{e}"),
                }
            })
        })
    }
}

#[test]
fn the_status_tells_the_sources_off_up_the_tree() {
    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    h.apply(link_op(&h, rail, imprint, leg, None)).unwrap();
    let root = h.document().root();
    assert_eq!(h.document().link_status(), LinkStatus::default());
    let group = h.apply(Operation::MakeAssembly { context: root, entities: vec![leg], name: None }).unwrap().created[0];
    let outer = h.apply(Operation::MakeAssembly { context: root, entities: vec![group], name: None }).unwrap().created[0];
    let outer_part = part_of(&h, outer);
    let inner = h.document().assembly(outer_part).unwrap().occurrence(group).unwrap().component;

    // The leg moved in its assembly: the rail, at the root, is off.
    h.apply(Operation::Transform { context: inner, entities: vec![leg], transform: moved(DVec3::Z) }).unwrap();
    let status = h.document().link_status();
    assert!(status.is_off(root, rail));
    assert!(!status.is_off(inner, leg));
    assert!(status.holds(root));
    assert!(!status.holds(inner), "nothing off in the leg's assembly");
}

#[test]
fn a_broken_link_is_told_only_when_no_other_holds_the_body() {
    let mut t = turned_table();
    let root = t.h.document().root();
    t.h.apply(Operation::Delete { context: root, entities: vec![t.rails[0]] }).unwrap();
    assert!(t.h.document().link_status().broken.is_empty(), "the other rails still hold both mortises");

    let mut h = History::new(Document::new());
    let (rail, leg, imprint) = rail_and_leg(&mut h);
    let body = h.apply(link_op(&h, rail, imprint, leg, None)).unwrap().bodies[0];
    let root = h.document().root();
    h.apply(Operation::Delete { context: root, entities: vec![rail] }).unwrap();
    let status = h.document().link_status();
    let leg_part = part_of(&h, leg);
    assert_eq!(status.broken.iter().copied().collect::<Vec<_>>(), vec![(leg_part, body)]);
    assert!(status.holds(leg_part) && status.holds(root));
    assert!(close(volume(&h, leg_part), LEG_VOLUME - IMPRINT_VOLUME), "the machining stays");

    // « Rompre le lien »: a plain body, nothing more to say.
    h.apply(Operation::Unlink { part: leg_part, bodies: vec![body] }).unwrap();
    assert!(root_links(&h).is_empty());
    assert_eq!(h.document().link_status(), LinkStatus::default());
    let q = h.document().part(leg_part).unwrap();
    assert_eq!(q.def_of(q.body(body).unwrap()).source, BodySource::Local);
    assert!(close(volume(&h, leg_part), LEG_VOLUME - IMPRINT_VOLUME));
    assert!(h.undo());
    assert_eq!(h.document().link_status().broken.len(), 1);
}

#[test]
fn replacing_puts_the_other_rails_on_their_seats() {
    let mut t = turned_table();
    t.shift(&[0], DVec3::Z * 10.0);
    assert_eq!(t.off(), vec![1, 2, 3]);
    let sources = t.sources(&[1, 2, 3]);
    let o = t.h.apply(Operation::ReseatSources { sources }).unwrap();
    assert!(o.warning.is_none());
    assert!(t.off().is_empty());
    assert_eq!(t.legs_parts(), 1);
    assert!(t.clear_of_every_leg());
    assert!(close(t.leg_volume(), full_leg()));
    assert!(t.h.undo());
    assert_eq!(t.off(), vec![1, 2, 3]);
}

#[test]
fn replacing_a_rail_whose_ends_disagree_leaves_it_and_says_by_how_much() {
    let mut t = turned_table();
    let root = t.h.document().root();
    t.h.apply(Operation::MakeUnique { context: root, occurrence: t.legs[0] }).unwrap();
    t.shift(&[0], DVec3::Z * 10.0);
    assert_eq!(t.off(), vec![1, 2]);
    let before: Vec<DAffine3> = t.rails.iter().map(|r| t.h.document().resolve(&EntityPath::root(*r)).unwrap().transform).collect();
    let o = t.h.apply(Operation::ReseatSources { sources: t.sources(&[1, 2]) }).unwrap();
    let Some(Warning::NotReseated { names, gap }) = o.warning else { panic!("{:?}", o.warning) };
    assert_eq!(names.len(), 2);
    assert!(gap.starts_with("10"), "{gap}");
    let after: Vec<DAffine3> = t.rails.iter().map(|r| t.h.document().resolve(&EntityPath::root(*r)).unwrap().transform).collect();
    assert_eq!(before, after);
}

#[test]
fn separating_gives_the_moved_rail_its_legs_and_the_others_their_mortises_back() {
    let mut t = turned_table();
    let before = t.mortises();
    t.shift(&[0], DVec3::Z * 10.0);
    assert_eq!(t.off(), vec![1, 2, 3]);
    let steps = t.h.journal().count();
    t.h.apply(Operation::SeparateReceivers { sources: t.sources(&[0]) }).unwrap();
    assert_eq!(t.h.journal().count(), steps + 1);
    assert!(t.off().is_empty(), "every rail seated: {:?}", t.off());
    assert_eq!(t.legs_parts(), 3, "its two legs apart, the other two still one");
    assert_eq!(t.mortises().len(), 2);
    let shared = part_of(&t.h, t.legs[2]);
    assert_eq!(part_of(&t.h, t.legs[3]), shared);
    let left: Vec<DAffine3> = t.h.document().part(shared).unwrap().negatives().map(|b| b.placement).collect();
    assert!(left.iter().zip(&before).all(|(a, b)| a.abs_diff_eq(*b, 1e-9)), "the others keep their mortises as they were");
    assert!(t.clear_of_every_leg());
}

#[test]
fn separating_resolves_a_conflict_between_mirrored_legs() {
    let mut t = mirrored_table();
    assert_eq!(t.shift(&[0], DVec3::X * 5.0).conflicts.len(), 1);
    assert_eq!(t.off(), vec![0]);
    t.h.apply(Operation::SeparateReceivers { sources: t.sources(&[0]) }).unwrap();
    assert!(t.off().is_empty(), "{:?}", t.off());
    assert_eq!(t.legs_parts(), 3);
}

#[test]
fn bringing_the_mortise_to_a_rail_leaves_the_others_off() {
    let mut t = turned_table();
    let before = t.mortises();
    t.shift(&[0], DVec3::Y * -200.0);
    assert_eq!(t.off(), vec![0]);
    t.shift(&[0], at(0.0, 200.0, 7.0));
    assert_eq!(t.off(), vec![0], "back in the leg, not on its seat");
    t.h.apply(Operation::BringLinkedBody { source: t.source(0) }).unwrap();
    assert_eq!(t.off(), vec![1, 2, 3]);
    let after = t.mortises();
    assert!(before.iter().zip(&after).all(|(a, b)| (b.translation - a.translation - DVec3::Z * 7.0).length() < 1e-9));
    assert!(close(t.leg_volume(), full_leg()));
}

#[test]
fn the_followed_outcome_names_the_sources_moved() {
    let mut t = turned_table();
    let followed = t.shift(&[0], DVec3::Y * -200.0);
    assert_eq!(followed.sources, t.sources(&[0]), "pulled out: moved, nothing followed");
    assert!(t.shift(&[1], DVec3::Z).sources.len() == 1);
}

// ---- « Imprimer »: a tenon makes its mortise ------------------------------------------------

const TENON: (f64, f64, f64) = (22.0, 10.0, 70.0);

/// The table turned, its rails carrying real tenons and no imprint yet: `(h, rails, legs)`.
fn tenoned_table() -> (History, Vec<Id>, Vec<Id>) {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let turn = |k: usize| DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2 * k as f64);
    let first_leg = add_part(&mut h, at(LEG, LEG, 700.0), turn(0) * moved(at(-360.0, -360.0, 0.0)));
    let first_rail = add_part(&mut h, at(600.0, RAIL_W, RAIL_H), turn(0) * moved(at(-300.0, -341.0, RAIL_Z)));
    let (y, z) = ((RAIL_W - TENON.1) / 2.0, (RAIL_H - TENON.2) / 2.0);
    for x in [-TENON.0, 600.0] {
        add_body(&mut h, first_rail, at(x, y, z), at(TENON.0, TENON.1, TENON.2), Sign::Positive);
    }
    let (leg_part, rail_part) = (part_of(&h, first_leg), part_of(&h, first_rail));
    let (mut rails, mut legs) = (vec![first_rail], vec![first_leg]);
    for k in 1..4 {
        let place = |h: &mut History, component, transform| {
            h.apply(Operation::PlaceOccurrence { context: root, component, transform }).unwrap().created[0]
        };
        legs.push(place(&mut h, leg_part, turn(k) * moved(at(-360.0, -360.0, 0.0))));
        rails.push(place(&mut h, rail_part, turn(k) * moved(at(-300.0, -341.0, RAIL_Z))));
    }
    (h, rails, legs)
}

fn clashes(h: &History, rails: &[Id], legs: &[Id]) -> f64 {
    let doc = h.document();
    let mut most: f64 = 0.0;
    for r in rails {
        let (_, rail, tr) = doc.resolve_part(&EntityPath::root(*r)).unwrap();
        for l in legs {
            let (_, leg, tl) = doc.resolve_part(&EntityPath::root(*l)).unwrap();
            let placed = rail.shape().transformed(&(tl.inverse() * tr)).unwrap();
            let v = match leg.shape().boolean(&placed, skraft_kernel::BooleanOp::Intersection) {
                Ok(common) => common.volume().unwrap(),
                Err(skraft_kernel::Error::Empty(_)) => 0.0,
                Err(e) => panic!("{e}"),
            };
            most = most.max(v);
        }
    }
    most
}

fn mortise_volume(around: f64, deep: f64) -> f64 {
    (TENON.0 + deep) * (TENON.1 + 2.0 * around) * (TENON.2 + 2.0 * around)
}

fn all_seated(h: &History) -> bool {
    let root = h.document().root();
    root_links(h).iter().all(|l| h.document().link_state(root, l) == LinkState::Seated)
}

#[test]
fn imprinting_one_rail_cuts_every_mortise_of_the_shared_legs() {
    let (mut h, rails, legs) = tenoned_table();
    let root = h.document().root();
    let leg_part = part_of(&h, legs[0]);
    let rail_part = part_of(&h, rails[0]);
    let rail_volume = volume(&h, rail_part);
    assert!(clashes(&h, &rails, &legs) > 1000.0, "the tenons run into the legs");

    let plan = h.document().imprint_plan(root, &[rails[0]]).unwrap();
    assert_eq!((plan.link_count(), plan.machining_count()), (8, 2), "every rail, each end");
    assert_eq!(plan.receivers(), vec![leg_part]);
    assert!(plan.partial.is_empty());

    let steps = h.journal().count();
    let o = h.apply(Operation::Imprint { context: root, sources: vec![rails[0]], around: 0.1, deep: 2.0 }).unwrap();
    assert_eq!(o.imprinted, (2, 8));
    assert_eq!(h.journal().count(), steps + 1);
    assert!(all_seated(&h));
    assert_eq!(h.document().part(leg_part).unwrap().negatives().count(), 2);
    let expected = LEG * LEG * 700.0 - 2.0 * mortise_volume(0.1, 2.0);
    assert!(close(volume(&h, leg_part), expected), "{} against {expected}", volume(&h, leg_part));
    assert!(close(volume(&h, rail_part), rail_volume), "the rail keeps its tenons");
    assert!(clashes(&h, &rails, &legs) < 1e-6, "every tenon in its mortise");
    assert!(legs.iter().all(|l| part_of(&h, *l) == leg_part), "the legs stay one part");

    // Done: nothing more to imprint.
    refused(h.apply(Operation::Imprint { context: root, sources: vec![rails[0]], around: 0.1, deep: 2.0 }), Refusal::NothingToImprint);
    assert!(h.undo());
    assert!(root_links(&h).is_empty());
    assert!(close(volume(&h, leg_part), LEG * LEG * 700.0));
}

#[test]
fn an_imprint_enters_from_its_shoulder_and_runs_back_by_its_length() {
    let (mut h, rails, _) = tenoned_table();
    let root = h.document().root();
    h.apply(Operation::Imprint { context: root, sources: vec![rails[0]], around: 0.0, deep: 2.0 }).unwrap();
    let p = h.document().part(part_of(&h, rails[0])).unwrap();
    let mut axes: Vec<(f64, f64)> = p
        .bodies
        .iter()
        .filter(|b| b.sign == Sign::Imprint)
        .map(|b| {
            let r = p.def_of(b).imprint.as_ref().unwrap();
            (r.clearance.axis.unwrap().x, r.clearance.back)
        })
        .collect();
    axes.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(axes, vec![(-1.0, TENON.0), (1.0, TENON.0)]);
}

#[test]
fn a_rail_moved_carries_its_clearance_cut_mortises() {
    let (mut h, rails, legs) = tenoned_table();
    let root = h.document().root();
    h.apply(Operation::Imprint { context: root, sources: vec![rails[0]], around: 0.1, deep: 2.0 }).unwrap();
    let o = h.apply(Operation::Transform { context: root, entities: vec![rails[0]], transform: moved(DVec3::Z * 10.0) }).unwrap();
    assert_eq!(o.links.moved.len(), 2);
    let off = h.document().link_status().off;
    assert_eq!(off.len(), 3);
    h.apply(Operation::ReseatSources { sources: off.into_iter().collect() }).unwrap();
    assert!(all_seated(&h));
    assert!(clashes(&h, &rails, &legs) < 1e-6);
}

#[test]
fn changing_the_clearance_reshapes_the_mortises() {
    let (mut h, rails, legs) = tenoned_table();
    let root = h.document().root();
    let (rail_part, leg_part) = (part_of(&h, rails[0]), part_of(&h, legs[0]));
    h.apply(Operation::Imprint { context: root, sources: vec![rails[0]], around: 0.1, deep: 2.0 }).unwrap();
    let imprints: Vec<(Id, f64)> = {
        let p = h.document().part(rail_part).unwrap();
        p.bodies.iter().filter(|b| b.sign == Sign::Imprint).map(|b| (b.id, p.def_of(b).imprint.as_ref().unwrap().clearance.back)).collect()
    };
    for (body, back) in imprints {
        h.apply(Operation::SetClearance { part: rail_part, body, around: 0.2, deep: 2.5, back }).unwrap();
    }
    assert!(all_seated(&h), "still in their mortises");
    // Not so deep that the two mortises of a leg meet.
    let expected = LEG * LEG * 700.0 - 2.0 * mortise_volume(0.2, 2.5);
    assert!(close(volume(&h, leg_part), expected), "{} against {expected}", volume(&h, leg_part));
    assert!(clashes(&h, &rails, &legs) < 1e-6);

    let body = h.document().part(rail_part).unwrap().bodies.iter().find(|b| b.sign == Sign::Imprint).unwrap().id;
    refused(h.apply(Operation::SetClearance { part: rail_part, body, around: f64::NAN, deep: 0.0, back: 1.0 }), Refusal::BadClearance);
    refused(h.apply(Operation::SetClearance { part: rail_part, body, around: 0.0, deep: 0.0, back: -1.0 }), Refusal::BadClearance);
}

#[test]
fn a_tenon_moved_in_its_part_takes_its_imprint_and_mortises() {
    let (mut h, rails, legs) = tenoned_table();
    let root = h.document().root();
    let (rail_part, leg_part) = (part_of(&h, rails[0]), part_of(&h, legs[0]));
    h.apply(Operation::Imprint { context: root, sources: vec![rails[0]], around: 0.1, deep: 2.0 }).unwrap();
    let (tenon, imprint) = {
        let p = h.document().part(rail_part).unwrap();
        let i = p.bodies.iter().find(|b| b.sign == Sign::Imprint).unwrap();
        (p.def_of(i).imprint.as_ref().unwrap().of, i.id)
    };
    let before = h.document().part(rail_part).unwrap().body(imprint).unwrap().placement;
    let o = h
        .apply(Operation::TransformBodies {
            target: EntityPath::root(rails[0]),
            bodies: vec![tenon],
            transform: moved(DVec3::Z * 4.0),
            scope: Scope::All,
        })
        .unwrap();
    assert_eq!(o.links.moved.len(), 1, "its mortise, in every leg");
    let after = h.document().part(rail_part).unwrap().body(imprint).unwrap().placement;
    assert!((after.translation - before.translation - DVec3::Z * 4.0).length() < 1e-9);
    assert!(all_seated(&h));
    assert!(close(volume(&h, leg_part), LEG * LEG * 700.0 - 2.0 * mortise_volume(0.1, 2.0)));
    assert!(clashes(&h, &rails, &legs) < 1e-6);
}

#[test]
fn a_tenon_changing_shape_reshapes_its_mortises() {
    let (mut h, rails, legs) = tenoned_table();
    let root = h.document().root();
    let (rail_part, leg_part) = (part_of(&h, rails[0]), part_of(&h, legs[0]));
    h.apply(Operation::Imprint { context: root, sources: vec![rails[0]], around: 0.0, deep: 2.0 }).unwrap();
    // A tenon made 4 mm thicker across, by its definition: every rail shows it.
    let tenon = {
        let p = h.document().part(rail_part).unwrap();
        let i = p.bodies.iter().find(|b| b.sign == Sign::Imprint).unwrap();
        p.def_of(i).imprint.as_ref().unwrap().of
    };
    let middle = (RAIL_W - TENON.1) / 2.0 + TENON.1 / 2.0;
    h.apply(Operation::StretchBodies {
        target: EntityPath::root(rails[0]),
        bodies: vec![tenon],
        frame: DAffine3::IDENTITY,
        axis: DVec3::Y,
        cuts: vec![middle],
        moves: vec![0.0, 4.0],
        scope: Scope::All,
    })
    .unwrap();
    assert!(all_seated(&h), "the mortise took the new shape");
    let one = (TENON.0 + 2.0) * TENON.1 * TENON.2;
    let wider = (TENON.0 + 2.0) * (TENON.1 + 4.0) * TENON.2;
    let expected = LEG * LEG * 700.0 - one - wider;
    assert!(close(volume(&h, leg_part), expected), "{} against {expected}", volume(&h, leg_part));
    assert!(clashes(&h, &rails, &legs) < 1e-6, "the wider tenon fits");
}

#[test]
fn a_tenon_deleted_leaves_its_imprint_frozen() {
    let (mut h, rails, _) = tenoned_table();
    let root = h.document().root();
    let rail_part = part_of(&h, rails[0]);
    h.apply(Operation::Imprint { context: root, sources: vec![rails[0]], around: 0.1, deep: 2.0 }).unwrap();
    let (tenon, imprint) = {
        let p = h.document().part(rail_part).unwrap();
        let i = p.bodies.iter().find(|b| b.sign == Sign::Imprint).unwrap();
        (p.def_of(i).imprint.as_ref().unwrap().of, i.id)
    };
    h.apply(Operation::DeleteBodies { target: EntityPath::root(rails[0]), bodies: vec![tenon], scope: Scope::All }).unwrap();
    let p = h.document().part(rail_part).unwrap();
    assert!(p.def_of(p.body(imprint).unwrap()).imprint.is_none());
    assert!(all_seated(&h), "the imprint keeps its place and shape");
}

#[test]
fn a_part_of_one_body_imprints_itself_without_a_shoulder() {
    let mut h = History::new(Document::new());
    let root = h.document().root();
    let dowel = {
        let primitive = Primitive::Cylinder { origin: DVec3::ZERO, radius: 4.0, height: 30.0 };
        h.apply(Operation::AddPrimitive { context: root, primitive, placement: moved(at(20.0, 20.0, 80.0)), material: None })
            .unwrap()
            .created[0]
    };
    let block = add_part(&mut h, at(40.0, 40.0, 100.0), DAffine3::IDENTITY);
    h.apply(Operation::Imprint { context: root, sources: vec![dowel], around: 0.1, deep: 2.0 }).unwrap();
    let p = h.document().part(part_of(&h, dowel)).unwrap();
    let i = p.bodies.iter().find(|b| b.sign == Sign::Imprint).unwrap();
    let c = p.def_of(i).imprint.as_ref().unwrap().clearance;
    assert_eq!((c.axis, c.deep, c.back), (None, 0.0, 0.0), "around it alone");
    let hole = std::f64::consts::PI * 4.1 * 4.1 * 20.1;
    let v = volume(&h, part_of(&h, block));
    assert!((v - (LEG_VOLUME - hole)).abs() < 1.0, "{v}");
}

#[test]
fn a_machining_reaching_only_some_copies_is_told() {
    let mut h = History::new(Document::new());
    let (rail, leg, _) = rail_and_leg(&mut h);
    let root = h.document().root();
    let leg_part = part_of(&h, leg);
    h.apply(Operation::PlaceOccurrence { context: root, component: leg_part, transform: moved(at(500.0, 0.0, 0.0)) }).unwrap();
    let plan = h.document().imprint_plan(root, &[rail]).unwrap();
    assert_eq!(plan.link_count(), 1);
    assert_eq!(plan.partial, vec![leg_part], "its other copy would show a mortise with no tenon");
}

#[test]
fn an_imprint_is_saved_with_how_it_is_made() {
    let (mut h, rails, legs) = tenoned_table();
    let root = h.document().root();
    h.apply(Operation::Imprint { context: root, sources: vec![rails[0]], around: 0.1, deep: 2.0 }).unwrap();
    let read = Document::from_file(&h.document().to_file().unwrap()).unwrap();
    let p = read.part(part_of(&h, rails[0])).unwrap();
    assert_eq!(p.defs.iter().filter(|d| d.imprint.is_some()).count(), 2);
    assert!(read.root_assembly().links().iter().all(|l| read.link_state(root, l) == LinkState::Seated));
    assert!(close(read.part(part_of(&h, legs[0])).unwrap().shape().volume().unwrap(), LEG * LEG * 700.0 - 2.0 * mortise_volume(0.1, 2.0)));

    // Copied into another document, an imprint follows its body under its new id.
    let mut other = Document::new();
    other.fresh_id();
    other.graft(h.document(), false).unwrap();
    let copy = other.components().find(|c| c.as_part().is_some_and(|p| p.defs.iter().any(|d| d.imprint.is_some()))).unwrap();
    let p = copy.as_part().unwrap();
    assert!(p.defs.iter().filter_map(|d| d.imprint.as_ref()).all(|r| p.body(r.of).is_some()));
}

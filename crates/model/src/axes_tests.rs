//! Changing a component's axes (docs/decisions/0053-changing-a-components-axes.md).

use glam::{DAffine3, DMat3, DQuat, DVec2, DVec3};

use crate::*;

/// New x along the current y, new y along the current -x.
fn quarter() -> DAffine3 {
    DAffine3::from_mat3(DMat3::from_cols(DVec3::Y, -DVec3::X, DVec3::Z))
}

fn oblique() -> DAffine3 {
    DAffine3::from_translation(DVec3::new(10.0, -5.0, 3.0))
        * DAffine3::from_quat(DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 3.0).normalize(), 0.7))
}

fn board(h: &mut History) -> (Id, Id) {
    let root = h.document().root();
    let primitive = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(100.0, 50.0, 20.0) };
    let occ =
        h.apply(Operation::AddPrimitive { context: root, primitive, placement: DAffine3::IDENTITY, material: None }).unwrap().created[0];
    let part = h.document().resolve_part(&EntityPath::root(occ)).unwrap().0;
    (occ, part)
}

fn world(doc: &Document) -> Vec<DVec3> {
    doc.placed_parts()
        .iter()
        .flat_map(|p| p.shape.vertex_points().iter().map(|v| p.transform.transform_point3(*v)).collect::<Vec<_>>())
        .collect()
}

fn same_points(a: &[DVec3], b: &[DVec3]) {
    assert_eq!(a.len(), b.len());
    for p in a {
        assert!(b.iter().any(|q| q.distance(*p) < 1e-6), "{p} is gone");
    }
}

fn size(doc: &Document, part: Id) -> DVec3 {
    let (lo, hi) = doc.part(part).unwrap().shape().vertex_bounds().unwrap();
    hi - lo
}

#[test]
fn nothing_moves_and_the_dimensions_turn_with_the_axes() {
    let mut h = History::new(Document::new());
    let (occ, part) = board(&mut h);
    let root = h.document().root();
    let mirror = DAffine3::from_translation(DVec3::Y * 300.0) * DAffine3::from_mat3(DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)));
    h.apply(Operation::Duplicate { context: root, entities: vec![occ], transform: mirror }).unwrap();
    let before = world(h.document());
    let op = Operation::ChangeAxes { component: part, axes: quarter() };
    let Prepared::Kernel(job) = h.document().prepare(&op).unwrap() else { panic!("the shape is transformed") };
    assert!(matches!(&job.tasks[..], [kernel::Task::Transform { .. }]), "no boolean: {:?}", job.tasks);
    h.apply(op).unwrap();
    same_points(&before, &world(h.document()));
    assert!((size(h.document(), part) - DVec3::new(50.0, 100.0, 20.0)).length() < 1e-9);
    let mirrored = h.document().placed_parts().iter().filter(|p| p.transform.matrix3.determinant() < 0.0).count();
    assert_eq!(mirrored, 1, "a mirrored occurrence stays mirrored");
    assert!(h.undo());
    assert!((size(h.document(), part) - DVec3::new(100.0, 50.0, 20.0)).length() < 1e-9);
}

#[test]
fn a_part_with_machinings_keeps_its_shape_and_comes_back_plain() {
    let mut h = History::new(Document::new());
    let (occ, part) = board(&mut h);
    let volume = |h: &History| h.document().part(part).unwrap().shape().volume().unwrap();
    let primitive = Primitive::Cylinder { origin: DVec3::new(50.0, 25.0, -1.0), radius: 5.0, height: 30.0 };
    let target = EntityPath::root(occ);
    h.apply(Operation::AddBody { target, primitive, placement: DAffine3::IDENTITY, sign: Sign::Negative, scope: Scope::All }).unwrap();
    let (v, before) = (volume(&h), world(h.document()));
    h.apply(Operation::ChangeAxes { component: part, axes: oblique() }).unwrap();
    assert!((volume(&h) - v).abs() < 1e-6);
    same_points(&before, &world(h.document()));

    let mut h = History::new(Document::new());
    let (_, part) = board(&mut h);
    h.apply(Operation::ChangeAxes { component: part, axes: oblique() }).unwrap();
    assert!(!h.document().part(part).unwrap().is_plain());
    h.apply(Operation::ChangeAxes { component: part, axes: oblique().inverse() }).unwrap();
    let p = h.document().part(part).unwrap();
    assert!(p.is_plain() && p.shape.is_none(), "back in its body's axes");
}

#[test]
fn an_assembly_changes_its_own_axes_only() {
    let mut h = History::new(Document::new());
    let (occ, part) = board(&mut h);
    let root = h.document().root();
    let out = h.apply(Operation::MakeAssembly { context: root, entities: vec![occ], name: None }).unwrap();
    let assembly = h.document().component(root).unwrap().as_assembly().unwrap().occurrence(out.created[0]).unwrap().component;
    let before = world(h.document());
    h.apply(Operation::ChangeAxes { component: assembly, axes: quarter() }).unwrap();
    same_points(&before, &world(h.document()));
    assert!((size(h.document(), part) - DVec3::new(100.0, 50.0, 20.0)).length() < 1e-9, "the part keeps its axes");
}

#[test]
fn stretch_settings_are_permuted_or_reset() {
    let mut h = History::new(Document::new());
    let (_, part) = board(&mut h);
    let behavior = Behavior { no_stretch: StretchMask(0b001), cutters: Some([vec![], vec![0.25], vec![0.5]]) };
    h.apply(Operation::SetBehavior { component: part, behavior }).unwrap();
    h.apply(Operation::ChangeAxes { component: part, axes: quarter() }).unwrap();
    let b = &h.document().component(part).unwrap().behavior;
    assert!(!b.no_stretch.locks(0) && b.no_stretch.locks(1), "x locked: the new y");
    assert_eq!(b.cutters, Some([vec![0.25], vec![], vec![0.5]]));
    h.apply(Operation::ChangeAxes { component: part, axes: DAffine3::from_rotation_x(std::f64::consts::PI) }).unwrap();
    let b = &h.document().component(part).unwrap().behavior;
    assert_eq!(b.cutters.as_ref().unwrap()[0], vec![0.25], "x not reversed");
    h.apply(Operation::ChangeAxes { component: part, axes: DAffine3::from_rotation_z(std::f64::consts::PI) }).unwrap();
    assert_eq!(h.document().component(part).unwrap().behavior.cutters.as_ref().unwrap()[0], vec![0.75], "a reversed axis: 1 - r");
    h.apply(Operation::ChangeAxes { component: part, axes: oblique() }).unwrap();
    assert_eq!(h.document().component(part).unwrap().behavior, Behavior::default());
}

#[test]
fn guides_stand_where_they_were() {
    let mut h = History::new(Document::new());
    let (_, part) = board(&mut h);
    let shape = GuideShape::Line { origin: DVec3::new(10.0, 0.0, 0.0), dir: DVec3::Y };
    h.apply(Operation::AddGuide { component: part, shape }).unwrap();
    h.apply(Operation::ChangeAxes { component: part, axes: quarter() }).unwrap();
    let GuideShape::Line { origin, dir } = h.document().component(part).unwrap().guides[0].shape else { panic!() };
    assert!(origin.distance(quarter().inverse().transform_point3(DVec3::new(10.0, 0.0, 0.0))) < 1e-9);
    assert!(dir.distance(DVec3::X) < 1e-9);
}

#[test]
fn a_view_of_the_component_keeps_its_image() {
    let mut h = History::new(Document::new());
    let (_, part) = board(&mut h);
    let sheet = h.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().sheet.unwrap();
    let view = |look| {
        ElementKind::View(View {
            name: "V".into(),
            kind: ViewKind::Base { subject: Subject::Component(part), look, scale: 0.5, at: DVec2::new(100.0, 100.0) },
            style: ViewStyle::Lines,
            crop: Some(Crop { lo: DVec2::splat(-10.0), hi: DVec2::splat(10.0) }),
        })
    };
    let mut add = |look| h.apply(Operation::AddElement { to: Container::Sheet(sheet), element: view(look) }).unwrap().element.unwrap();
    let top = add(Look::Standard(Direction::Top));
    let front = add(Look::Standard(Direction::Front));
    let eye = DVec3::new(300.0, 200.0, 100.0);
    let perspective = Perspective { eye, fov_y: 0.6, size: DVec2::new(80.0, 60.0) };
    let camera = add(Look::Camera { forward: -eye.normalize(), up: DVec3::Z, perspective: Some(perspective) });
    let seen = |doc: &Document, id: Id| {
        let r = doc.drawing().resolve(id).unwrap();
        let points: Vec<DVec3> = doc.part(part).unwrap().shape().vertex_points().iter().map(|p| r.frame.project(*p)).collect();
        let (lo, hi) = points.iter().fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(*p), h.max(*p)));
        let centre = (lo + hi) / 2.0;
        let mut around: Vec<DVec3> = points.iter().map(|p| *p - centre).collect();
        around.sort_by(|a, b| (a.x, a.y, a.z).partial_cmp(&(b.x, b.y, b.z)).unwrap());
        (around, r.at, r.crop)
    };
    let before: Vec<_> = [top, front].map(|v| seen(h.document(), v)).into();
    h.apply(Operation::ChangeAxes { component: part, axes: quarter() }).unwrap();
    for (v, was) in [top, front].into_iter().zip(&before) {
        let now = seen(h.document(), v);
        assert_eq!((now.1, now.2), (was.1, was.2), "its place and crop");
        for (a, b) in now.0.iter().zip(&was.0) {
            assert!(a.distance(*b) < 1e-6, "{a} / {b}");
        }
    }
    let look = |doc: &Document, id: Id| match &doc.drawing().element(id).unwrap().kind {
        ElementKind::View(View { kind: ViewKind::Base { look, .. }, .. }) => look.clone(),
        _ => panic!(),
    };
    assert!(matches!(look(h.document(), top), Look::Camera { perspective: None, .. }), "top turned in its plane: no standard one");
    assert!(matches!(look(h.document(), front), Look::Standard(Direction::Right | Direction::Left)));
    let Look::Camera { perspective: Some(p), .. } = look(h.document(), camera) else { panic!() };
    assert!(p.eye.distance(quarter().inverse().transform_point3(eye)) < 1e-9);
}

#[test]
fn a_dimension_follows_its_vertex_through_new_axes_and_a_stretch() {
    let mut h = History::new(Document::new());
    let (occ, part) = board(&mut h);
    let doc = h.document();
    let placed = doc.subject_parts(&Subject::Model).into_iter().find(|p| p.path.entity == occ).unwrap();
    let corner = DVec3::new(100.0, 50.0, 20.0);
    let v = placed.shape.vertex_points().iter().position(|p| p.distance(corner) < 1e-9).unwrap() as u32;
    let anchor = doc.anchor(&placed, Entity::Vertex, v, corner).unwrap();
    let sheet = h.apply(Operation::AddSheet { presentation: None, name: None, paper: Paper::A3 }).unwrap().sheet.unwrap();
    let base = ElementKind::View(View {
        name: "V".into(),
        kind: ViewKind::Base { subject: Subject::Model, look: Look::Standard(Direction::Front), scale: 0.5, at: DVec2::splat(100.0) },
        style: ViewStyle::Lines,
        crop: None,
    });
    let view = h.apply(Operation::AddElement { to: Container::Sheet(sheet), element: base }).unwrap().element.unwrap();
    let kind = DimKind::Linear { a: anchor.clone(), b: anchor.clone(), orient: Orient::Horizontal, offset: 5.0 };
    let element = ElementKind::Dimension(Dimension { view, kind });
    let dim = h.apply(Operation::AddElement { to: Container::Sheet(sheet), element }).unwrap().element.unwrap();
    h.apply(Operation::ChangeAxes { component: part, axes: quarter() }).unwrap();
    let doc = h.document();
    let ElementKind::Dimension(Dimension { kind: DimKind::Linear { a, .. }, .. }) = &doc.drawing().element(dim).unwrap().kind else {
        panic!()
    };
    assert!(a.at.distance(quarter().inverse().transform_point3(corner)) < 1e-9);
    assert!(doc.find(&Subject::Model, a).unwrap().point().distance(corner) < 1e-6);
    let a = a.clone();
    h.apply(Operation::Stretch {
        context: vec![],
        occurrences: vec![occ],
        frame: DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![50.0],
        moves: vec![0.0, 50.0],
        scope: Scope::All,
        cutters: None,
    })
    .unwrap();
    let found = h.document().find(&Subject::Model, &a).expect("found by its names").point();
    assert!(found.distance(corner + DVec3::X * 50.0) < 1e-6, "{found}");
}

#[test]
fn a_reoriented_part_used_as_a_tool_stays_one_body_with_its_profile() {
    let mut h = History::new(Document::new());
    let (target, _) = board(&mut h);
    let root = h.document().root();
    let primitive = Primitive::Box { origin: DVec3::new(10.0, 10.0, -1.0), size: DVec3::new(20.0, 10.0, 30.0) };
    let tool =
        h.apply(Operation::AddPrimitive { context: root, primitive, placement: DAffine3::IDENTITY, material: None }).unwrap().created[0];
    let tool_part = h.document().resolve_part(&EntityPath::root(tool)).unwrap().0;
    h.apply(Operation::ChangeAxes { component: tool_part, axes: quarter() }).unwrap();
    assert!(part_names(h.document().part(tool_part).unwrap()).is_some(), "its faces keep their names");
    h.apply(Operation::boolean(EntityPath::root(target), EntityPath::root(tool), Boolean::Difference, Scope::All)).unwrap();
    let (_, p, _) = h.document().resolve_part(&EntityPath::root(target)).unwrap();
    let cut = p.bodies.iter().find(|b| b.sign == Sign::Negative).unwrap();
    assert!(!p.def_of(cut).is_group() && p.profile_of(cut).is_some(), "the tool's body, not a group");
    let volume = 100.0 * 50.0 * 20.0 - 20.0 * 10.0 * 20.0;
    assert!((p.shape().volume().unwrap() - volume).abs() < 1e-6);
}

#[test]
fn a_reoriented_part_reads_back_from_its_file() {
    let mut h = History::new(Document::new());
    let (_, part) = board(&mut h);
    h.apply(Operation::ChangeAxes { component: part, axes: oblique() }).unwrap();
    let before = world(h.document());
    let file = h.document().to_file().unwrap();
    let (read, _) = Document::read_file(&file).unwrap();
    same_points(&before, &world(&read));
    assert!(!read.part(part).unwrap().is_plain());
}

#[test]
fn the_root_and_mirrors_are_refused() {
    let mut h = History::new(Document::new());
    let (_, part) = board(&mut h);
    let root = h.document().root();
    let mut refused = |component, axes| match h.apply(Operation::ChangeAxes { component, axes }) {
        Err(Error::Invalid(r)) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(refused(root, quarter()), Refusal::RootAxes);
    let mirror = DAffine3::from_mat3(DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)));
    assert_eq!(refused(part, mirror), Refusal::AxesNotDirect);
    assert_eq!(refused(part, DAffine3::from_scale(DVec3::splat(2.0))), Refusal::AxesNotDirect);
}

fn drawn(h: &mut History, primitive: Primitive) -> Id {
    let root = h.document().root();
    let occ =
        h.apply(Operation::DrawPrimitive { context: root, primitive, placement: DAffine3::IDENTITY, material: None }).unwrap().created[0];
    h.document().resolve_part(&EntityPath::root(occ)).unwrap().0
}

fn exact_size(doc: &Document, part: Id) -> DVec3 {
    let (lo, hi) = doc.part(part).unwrap().shape().bounds().unwrap();
    hi - lo
}

fn world_box(doc: &Document) -> (DVec3, DVec3) {
    let p = &doc.placed_parts()[0];
    let (lo, hi) = p.shape.transformed(&p.transform).unwrap().bounds().unwrap();
    (lo, hi)
}

#[test]
fn a_box_is_born_oriented_and_plain_where_it_was_drawn() {
    for size in
        [DVec3::new(18.0, 600.0, 400.0), DVec3::new(400.0, 18.0, 600.0), DVec3::new(600.0, 400.0, 18.0), DVec3::new(18.0, 400.0, 600.0)]
    {
        let mut h = History::new(Document::new());
        let origin = DVec3::new(10.0, 20.0, 30.0);
        let part = drawn(&mut h, Primitive::Box { origin, size });
        assert!(h.document().part(part).unwrap().is_plain(), "{size}");
        assert!(exact_size(h.document(), part).distance(DVec3::new(600.0, 400.0, 18.0)) < 1e-6, "{size}");
        let (lo, hi) = world_box(h.document());
        assert!(lo.distance(origin) < 1e-6 && hi.distance(origin + size) < 1e-6, "{size}: {lo} {hi}");
        assert!(h.document().placed_parts()[0].transform.matrix3.determinant() > 0.0, "never a mirror");
    }
}

#[test]
fn a_flat_prism_is_turned_in_its_plane_and_stays_plain() {
    let mut h = History::new(Document::new());
    let profile = vec![
        kernel::profile::Corner::new(DVec2::ZERO),
        kernel::profile::Corner::new(DVec2::new(100.0, 0.0)),
        kernel::profile::Corner::arc(DVec2::new(100.0, 300.0), 0.2),
        kernel::profile::Corner::new(DVec2::new(0.0, 300.0)),
    ];
    let primitive = Primitive::Prism { origin: DVec3::new(5.0, 6.0, 7.0), profile, height: 18.0 };
    let (want_lo, want_hi) = {
        let mut raw = History::new(Document::new());
        let root = raw.document().root();
        raw.apply(Operation::AddPrimitive { context: root, primitive: primitive.clone(), placement: DAffine3::IDENTITY, material: None })
            .unwrap();
        world_box(raw.document())
    };
    let part = drawn(&mut h, primitive);
    let p = h.document().part(part).unwrap();
    assert!(p.is_plain() && p.profile_of(&p.bodies[0]).is_some(), "plain, its profile kept");
    let s = exact_size(h.document(), part);
    assert!(s.x > s.y && s.y > s.z && (s.z - 18.0).abs() < 1e-6, "{s}");
    let (lo, hi) = world_box(h.document());
    assert!(lo.distance(want_lo) < 1e-6 && hi.distance(want_hi) < 1e-6, "{lo} {hi}");
}

#[test]
fn a_tall_cylinder_gets_new_axes_and_keeps_its_profile() {
    let mut h = History::new(Document::new());
    let primitive = Primitive::Cylinder { origin: DVec3::new(1.0, 2.0, 3.0), radius: 10.0, height: 200.0 };
    let part = drawn(&mut h, primitive);
    let p = h.document().part(part).unwrap();
    assert!(p.is_one_body() && !p.is_plain());
    assert!(exact_size(h.document(), part).distance(DVec3::new(200.0, 20.0, 20.0)) < 1e-6);
    let (lo, hi) = world_box(h.document());
    assert!(lo.distance(DVec3::new(-9.0, -8.0, 3.0)) < 1e-6 && hi.distance(DVec3::new(11.0, 12.0, 203.0)) < 1e-6, "{lo} {hi}");
    let mut h = History::new(Document::new());
    let flat = drawn(&mut h, Primitive::Cylinder { origin: DVec3::ZERO, radius: 50.0, height: 18.0 });
    assert!(h.document().part(flat).unwrap().is_plain(), "already oriented");
}

#[test]
fn a_slanted_prism_takes_its_longest_edge_as_length_and_stays_plain() {
    let corners = [DVec2::ZERO, DVec2::new(300.0, 100.0), DVec2::new(340.0, 200.0), DVec2::new(40.0, 100.0)];
    let profile: Vec<_> = corners.iter().map(|p| kernel::profile::Corner::new(*p)).collect();
    let primitive = Primitive::Prism { origin: DVec3::new(5.0, 6.0, 7.0), profile, height: 18.0 };
    let mut raw = History::new(Document::new());
    let root = raw.document().root();
    raw.apply(Operation::AddPrimitive { context: root, primitive: primitive.clone(), placement: DAffine3::IDENTITY, material: None })
        .unwrap();
    let mut h = History::new(Document::new());
    let part = drawn(&mut h, primitive);
    let p = h.document().part(part).unwrap();
    assert!(p.is_plain() && p.profile_of(&p.bodies[0]).is_some(), "plain, its profile kept");
    same_points(&world(raw.document()), &world(h.document()));
    let points = p.shape().vertex_points();
    let long = 100000f64.sqrt();
    let along_red = points
        .iter()
        .any(|a| points.iter().any(|b| ((*b - *a).x - long).abs() < 1e-6 && (*b - *a).y.abs() < 1e-6 && (*b - *a).z.abs() < 1e-6));
    assert!(along_red, "its long edge on red");
    assert!((exact_size(h.document(), part).z - 18.0).abs() < 1e-6, "its height on blue");
}

#[test]
fn a_tall_prism_gets_new_axes_and_keeps_its_profile() {
    let profile = kernel::profile::polygon(&[DVec2::ZERO, DVec2::new(40.0, 0.0), DVec2::new(40.0, 40.0), DVec2::new(0.0, 40.0)]);
    let mut h = History::new(Document::new());
    let part = drawn(&mut h, Primitive::Prism { origin: DVec3::ZERO, profile, height: 500.0 });
    let p = h.document().part(part).unwrap();
    assert!(p.is_one_body() && !p.is_plain() && p.profile_of(&p.bodies[0]).is_some());
    assert!(exact_size(h.document(), part).distance(DVec3::new(500.0, 40.0, 40.0)) < 1e-6);
    let (lo, hi) = world_box(h.document());
    assert!(lo.distance(DVec3::ZERO) < 1e-6 && hi.distance(DVec3::new(40.0, 40.0, 500.0)) < 1e-6, "{lo} {hi}");
}

#[test]
fn a_disc_has_no_edge_to_go_by() {
    let profile = vec![kernel::profile::Corner::arc(DVec2::ZERO, 1.0), kernel::profile::Corner::arc(DVec2::new(100.0, 0.0), 1.0)];
    let mut h = History::new(Document::new());
    let part = drawn(&mut h, Primitive::Prism { origin: DVec3::ZERO, profile, height: 18.0 });
    assert!(h.document().part(part).unwrap().is_plain());
    assert!((exact_size(h.document(), part).z - 18.0).abs() < 1e-6);
}

#[test]
fn the_axes_point_towards_the_material_the_thickness_first() {
    // All on the positive side: x and z keep their sign, y follows, never a mirror.
    let points = [DVec3::ZERO, DVec3::new(18.0, 600.0, 400.0)];
    let m = kernel::orient::signed_axes(&points, DVec3::Y, DVec3::Z);
    assert_eq!((m.x_axis, m.z_axis), (DVec3::Y, DVec3::Z));
    assert_eq!(m.y_axis, -DVec3::X);
    assert!(m.determinant() > 0.0);
    // Below the origin: z turned towards it.
    let points = [DVec3::new(0.0, 0.0, -18.0), DVec3::new(600.0, 400.0, 0.0)];
    assert_eq!(kernel::orient::signed_axes(&points, DVec3::X, DVec3::Z).z_axis, -DVec3::Z);
    // Symmetric: the directions given.
    let points = [DVec3::splat(-1.0), DVec3::splat(1.0)];
    assert_eq!(kernel::orient::signed_axes(&points, DVec3::X, DVec3::Z), DMat3::IDENTITY);
}

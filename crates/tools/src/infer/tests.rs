//! Inference tests.

use glam::{DAffine3, DVec2, DVec3};
use solidsketch_model::{Document, EntityPath, Id, Operation, Primitive};

use super::*;
use crate::tests::PartExt;
use crate::{Editor, Modifiers, StandardView, ToolInput, ToolKind, ViewArea};

/// An editor looking straight down (parallel) at ±1 m around `at`.
fn top_editor(at: DVec3) -> Editor {
    view_editor(StandardView::Top, at)
}

fn view_editor(view: StandardView, at: DVec3) -> Editor {
    let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Inline::default()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.set_parallel(true);
    let b = (at - DVec3::splat(1000.0), at + DVec3::splat(1000.0));
    e.camera.set_standard_view(view, glam::DMat3::IDENTITY, Some(b), ViewArea::full(e.viewport_size()));
    e
}

fn add(e: &mut Editor, primitive: Primitive) -> Id {
    let root = e.document().root();
    e.apply(Operation::AddPrimitive { context: root, primitive, placement: glam::DAffine3::IDENTITY, material: None })
        .unwrap()
        .expect("done")
        .created[0]
}

fn add_box(e: &mut Editor, origin: DVec3, size: DVec3) -> Id {
    add(e, Primitive::Box { origin, size })
}

fn px(e: &Editor, p: DVec3) -> DVec2 {
    e.camera.project(p, e.viewport_size()).unwrap()
}

/// What is inferred with the cursor at `px`, with lines through `from`.
fn at_px_from(e: &Editor, px: DVec2, from: DVec3, reference: Option<EdgeRef>, lock: Option<(DVec3, LineKind)>) -> Inference {
    let lock = lock.map(|(dir, line)| Lock { origin: None, dir, line });
    let keep = |_: &Owner<'_>| true;
    let q = Query {
        keep: &keep,
        plane: (from, DVec3::Z),
        origin: Some(DVec3::ZERO),
        model_axes: false,
        from: Some(from),
        tracked: &[],
        in_plane: false,
        axes: [DVec3::X, DVec3::Y, DVec3::Z],
        reference,
        lock,
        tool_line: None,
    };
    infer(e.scene(), &e.camera, e.viewport_size(), px, &q).unwrap()
}

fn at_from(e: &Editor, p: DVec3, from: DVec3, reference: Option<EdgeRef>, lock: Option<(DVec3, LineKind)>) -> Inference {
    at_px_from(e, px(e, p), from, reference, lock)
}

fn near(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-6
}

/// For points the cursor gives (pixel → ray → line rounds to about 1e-6 here); features
/// inferred are exact.
fn about(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-4
}

/// What is inferred with the cursor over the world point `p` (no exclusion, ground z = 0).
fn at(e: &Editor, p: DVec3) -> Inference {
    let keep = |_: &Owner<'_>| true;
    let q = Query {
        keep: &keep,
        plane: (DVec3::ZERO, DVec3::Z),
        origin: Some(DVec3::ZERO),
        model_axes: false,
        from: None,
        tracked: &[],
        in_plane: false,
        axes: [DVec3::X, DVec3::Y, DVec3::Z],
        reference: None,
        lock: None,
        tool_line: None,
    };
    infer(e.scene(), &e.camera, e.viewport_size(), px(e, p), &q).unwrap()
}

#[test]
fn kinds_on_a_box_seen_from_above() {
    let mut e = top_editor(DVec3::ZERO);
    add_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 150.0));
    // 2 to 3 pixels away from the feature: the feature, exactly.
    let i = at(&e, DVec3::new(305.0, -4.0, 150.0));
    assert_eq!((i.kind, i.point), (InferKind::Endpoint, DVec3::new(300.0, 0.0, 150.0)));
    let i = at(&e, DVec3::new(154.0, -6.0, 150.0));
    assert_eq!((i.kind, i.point), (InferKind::Midpoint, DVec3::new(150.0, 0.0, 150.0)));
    let i = at(&e, DVec3::new(60.0, -4.0, 150.0));
    assert_eq!(i.kind, InferKind::OnEdge);
    assert!((i.point - DVec3::new(60.0, 0.0, 150.0)).length() < 1e-6, "{}", i.point);
    let i = at(&e, DVec3::new(100.0, 100.0, 150.0));
    assert_eq!(i.kind, InferKind::OnFace);
    assert_eq!(i.point.z, 150.0, "exactly on the face's plane");
    let i = at(&e, DVec3::new(-500.0, -500.0, 0.0));
    assert_eq!(i.kind, InferKind::OnPlane);
    assert!(i.point.z.abs() < 1e-9);
    let i = at(&e, DVec3::new(-4.0, 600.0, 0.0));
    assert_eq!(i.kind, InferKind::OnPlane, "the origin is far");
    // The origin, where nothing hides it.
    let mut e = top_editor(DVec3::ZERO);
    add_box(&mut e, DVec3::new(500.0, 500.0, 0.0), DVec3::splat(100.0));
    let i = at(&e, DVec3::new(4.0, -4.0, 0.0));
    assert_eq!((i.kind, i.point), (InferKind::Origin, DVec3::ZERO));
}

#[test]
fn centre_of_a_cylinder() {
    let mut e = top_editor(DVec3::ZERO);
    let c = DVec3::new(200.0, 200.0, 50.0);
    add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 600.0, height: 50.0 });
    let wpp = e.camera.world_per_pixel(e.viewport_size(), c);
    assert!(wpp * APERTURE_PX < 100.0, "{wpp}");
    let i = at(&e, c + DVec3::new(4.0, -3.0, 0.0));
    assert_eq!(i.kind, InferKind::Center);
    assert!((i.point - c).length() < 1e-9, "{}", i.point);
    // Away from the centre and the circle: the top face.
    let d = DVec3::new(1.0, 1.0, 0.0).normalize();
    let i = at(&e, c + d * 300.0);
    assert_eq!(i.kind, InferKind::OnFace);
    // Just outside the circle: a point of the exact circle.
    let i = at(&e, c + d * 603.0);
    assert_eq!(i.kind, InferKind::OnEdge);
    let r = (i.point - c).length();
    assert!((r - 600.0).abs() < 1e-9 && (i.point.z - 50.0).abs() < 1e-9, "{}", i.point);
}

#[test]
fn hidden_features_are_not_inferred() {
    let mut e = top_editor(DVec3::ZERO);
    // A small box under a slab: its top corner is right under the cursor, but hidden.
    add_box(&mut e, DVec3::new(100.0, 100.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
    add_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 400.0, 20.0) + DVec3::new(0.0, 0.0, 100.0));
    let i = at(&e, DVec3::new(100.0, 100.0, 120.0));
    assert_eq!(i.kind, InferKind::OnFace, "{i:?}");
    assert_eq!(i.point.z, 120.0);
}

#[test]
fn an_open_part_s_bodies_hide_nothing() {
    use solidsketch_model::{Scope, Sign};
    let mut e = top_editor(DVec3::ZERO);
    // A part: a slab, and under it a small block, two positive bodies.
    let part = add_box(&mut e, DVec3::new(0.0, 0.0, 100.0), DVec3::new(400.0, 400.0, 20.0));
    let target = EntityPath::root(part);
    e.apply(Operation::AddBody {
        target: target.clone(),
        // In the part's axes, at the slab's corner: 100 under it in the world.
        primitive: Primitive::Box { origin: DVec3::new(100.0, 100.0, -100.0), size: DVec3::new(100.0, 100.0, 50.0) },
        placement: DAffine3::IDENTITY,
        sign: Sign::Positive,
        scope: Scope::All,
    })
    .unwrap();
    // Closed, the slab hides the block's corner.
    let i = at(&e, DVec3::new(100.0, 100.0, 120.0));
    assert_eq!(i.kind, InferKind::OnFace, "{} {:?}", i.point, i.path);
    // Open, its bodies shown as ghosts: the corner is seen through the slab.
    e.enter(part);
    let i = at(&e, DVec3::new(100.0, 100.0, 120.0));
    assert_eq!(i.kind, InferKind::Endpoint, "{i:?}");
    assert!(near(i.point, DVec3::new(100.0, 100.0, 50.0)), "{}", i.point);
}

#[test]
fn in_x_ray_nothing_hides() {
    let mut e = top_editor(DVec3::ZERO);
    // A small box under a slab: its top corner, hidden, is seen in X-ray.
    add_box(&mut e, DVec3::new(100.0, 100.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
    add_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 400.0, 20.0) + DVec3::new(0.0, 0.0, 100.0));
    e.set_xray(true);
    let i = at(&e, DVec3::new(100.0, 100.0, 120.0));
    assert_eq!(i.kind, InferKind::Endpoint, "{i:?}");
    assert!(near(i.point, DVec3::new(100.0, 100.0, 50.0)), "{}", i.point);
    // The view draws every part see-through.
    assert!(e.view().batches.iter().all(|(_, ps)| ps.iter().all(|p| p.ghost)));
    e.set_xray(false);
    assert_eq!(at(&e, DVec3::new(100.0, 100.0, 120.0)).kind, InferKind::OnFace);
    // The document's setting (§6 sexdecies): undone as the others.
    e.set_xray(true);
    assert!(e.document().settings().display.xray);
    e.undo();
    assert!(!e.xray());
    assert_eq!(at(&e, DVec3::new(100.0, 100.0, 120.0)).kind, InferKind::OnFace);
}

#[test]
fn exact_in_a_moved_group_and_far_away() {
    for far in [DVec3::ZERO, DVec3::new(1e8, -3e7, 0.0)] {
        let mut e = top_editor(far);
        let a = add_box(&mut e, far, DVec3::new(100.0, 100.0, 37.5));
        e.select(vec![a]);
        e.make_assembly();
        let g = e.selection()[0];
        let root = e.document().root();
        let t = DAffine3::from_translation(DVec3::new(500.0, 250.0, 0.0));
        e.apply(Operation::Transform { context: root, entities: vec![g], transform: t }).unwrap();
        let corner = far + DVec3::new(600.0, 350.0, 37.5);
        let i = at(&e, corner + DVec3::new(4.0, 4.0, 0.0));
        assert_eq!(i.kind, InferKind::Endpoint);
        assert!((i.point - corner).length() < 1e-6, "{} {}", i.point, corner);
        assert_eq!(i.path.unwrap().instances, vec![g]);
    }
}

#[test]
fn move_puts_a_corner_on_a_corner() {
    let mut e = top_editor(DVec3::ZERO);
    let a = add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
    add_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
    e.select(vec![]);
    e.set_tool(ToolKind::Move);
    // Nothing selected: a click picks the box (§6 septendecies); then the cursor just off it
    // takes its corner.
    let p = px(&e, DVec3::new(50.0, 50.0, 50.0));
    e.input(ToolInput::Hover { px: p });
    e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
    assert_eq!(e.selection(), &[a]);
    let p = px(&e, DVec3::new(104.0, -4.0, 50.0));
    e.input(ToolInput::Hover { px: p });
    e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
    let p = px(&e, DVec3::new(303.0, 5.0, 50.0));
    e.input(ToolInput::Hover { px: p });
    assert_eq!(e.view().inference.map(|i| i.1), Some(InferKind::Endpoint));
    e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
    let (lo, _) = bounds(&e, a);
    assert_eq!(lo, DVec3::new(200.0, 0.0, 0.0));
}

#[test]
fn box_takes_a_corner_and_a_height_from_features() {
    let mut e = top_editor(DVec3::ZERO);
    add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
    e.set_tool(ToolKind::Box);
    let click = |e: &mut Editor, p: DVec3| {
        let p = px(e, p);
        e.input(ToolInput::Hover { px: p });
        e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
    };
    click(&mut e, DVec3::new(300.0, 300.0, 0.0));
    // The opposite corner on the other box's top corner, brought down to the ground.
    click(&mut e, DVec3::new(103.0, 97.0, 50.0));
    // The height of that same corner.
    click(&mut e, DVec3::new(97.0, 103.0, 50.0));
    let id = crate::tests::drawn(&e);
    // The corners inferred are exact; the first one, clicked on the ground, is not.
    let (lo, hi) = bounds(&e, id);
    assert_eq!(lo, DVec3::new(100.0, 100.0, 0.0));
    assert!((hi - DVec3::new(300.0, 300.0, 50.0)).length() < 1e-4 && hi.z == 50.0, "{hi}");
}

#[test]
fn lines_through_the_start_point() {
    let mut e = top_editor(DVec3::ZERO);
    add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
    let from = DVec3::new(50.0, 50.0, 50.0);
    let i = at_from(&e, DVec3::new(300.0, 53.0, 50.0), from, None, None);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(0)));
    assert!(about(i.point, DVec3::new(300.0, 50.0, 50.0)) && i.point.y == 50.0, "{}", i.point);
    assert_eq!(i.line, Some((from, DVec3::X)));
    let i = at_from(&e, DVec3::new(53.0, -300.0, 50.0), from, None, None);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(1)));
    // Near the start point, no line means anything.
    let i = at_from(&e, DVec3::new(53.0, 52.0, 50.0), from, None, None);
    assert_eq!(i.kind, InferKind::OnFace);
    // Off every line: no line.
    let i = at_from(&e, DVec3::new(300.0, 300.0, 50.0), from, None, None);
    assert_eq!(i.kind, InferKind::OnPlane);

    // Parallel and perpendicular to a diagonal edge hovered before (in the tool's plane).
    let r = DVec3::new(1.0, 2.0, 0.0).normalize();
    let perp = DVec3::Z.cross(r);
    let side = DVec3::new(0.0, 0.0, 0.0) + perp * 3.0;
    let i = at_from(&e, from + r * 400.0 + side, from, Some(EdgeRef::Straight(r)), None);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Parallel));
    assert!((i.point - from).cross(r).length() < 1e-6);
    let i = at_from(&e, from + perp * 400.0 + r * 3.0, from, Some(EdgeRef::Straight(r)), None);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Perpendicular));
    // A feature still beats a line.
    let i = at_from(&e, DVec3::new(103.0, 53.0, 50.0), from, None, None);
    assert_eq!(i.kind, InferKind::Midpoint);
}

#[test]
fn a_lock_projects_the_feature_hovered() {
    let mut e = top_editor(DVec3::ZERO);
    add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
    add_box(&mut e, DVec3::new(300.0, 200.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
    let from = DVec3::new(50.0, 50.0, 50.0);
    let lock = Some((DVec3::X, LineKind::Axis(0)));
    // The corner at (300, 200, 50), far from the red line: aligned with it on the line.
    let i = at_from(&e, DVec3::new(302.0, 203.0, 50.0), from, None, lock);
    assert_eq!((i.kind, i.point), (InferKind::OnLine(LineKind::Axis(0)), DVec3::new(300.0, 50.0, 50.0)));
    // Nothing hovered: the line's point nearest the cursor.
    let i = at_from(&e, DVec3::new(-500.0, 600.0, 50.0), from, None, lock);
    assert!(about(i.point, DVec3::new(-500.0, 50.0, 50.0)), "{}", i.point);
}

#[test]
fn a_line_pierces_the_face_under_the_cursor() {
    let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
    add_box(&mut e, DVec3::new(300.0, -200.0, 0.0), DVec3::new(20.0, 500.0, 300.0));
    let from = DVec3::new(50.0, 50.0, 50.0);
    // Whichever side of the wall the camera sees, the red line meets it there.
    let fwd = e.camera.forward();
    let x = if fwd.x > 0.0 { 300.0 } else { 320.0 };
    let p = DVec3::new(x, 50.0, 50.0);
    let i = at_px_from(&e, px(&e, p) + DVec2::new(1.5, 1.0), from, None, None);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(0)), "{i:?}");
    assert!(near(i.point, p), "{} {p}", i.point);
}

fn click_at(e: &mut Editor, p: DVec3) {
    let p = px(e, p);
    e.input(ToolInput::Hover { px: p });
    e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
}

/// World bounds of the part the root's occurrence `id` places.
fn bounds(e: &Editor, id: Id) -> (DVec3, DVec3) {
    let (_, part, t) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    crate::scene::transform_bounds(&t, part.shape().vertex_bounds().unwrap())
}

#[test]
fn a_box_on_a_wall() {
    let mut e = view_editor(StandardView::Front, DVec3::ZERO);
    add_box(&mut e, DVec3::ZERO, DVec3::new(1000.0, 100.0, 800.0));
    // The face seen from the front, whichever way the front looks.
    let (y, out) = if e.camera.forward().y > 0.0 { (0.0, -1.0) } else { (100.0, 1.0) };
    e.set_tool(ToolKind::Box);
    // Far from the wall's edges: points of its face.
    click_at(&mut e, DVec3::new(200.0, y, 150.0));
    click_at(&mut e, DVec3::new(500.0, y, 400.0));
    e.input(ToolInput::Vcb("30".into()));
    let id = crate::tests::drawn(&e);
    let (lo, hi) = bounds(&e, id);
    let (y0, y1) = if out < 0.0 { (y - 30.0, y) } else { (y, y + 30.0) };
    assert!(about(lo, DVec3::new(200.0, y0, 150.0)) && about(hi, DVec3::new(500.0, y1, 400.0)), "{lo} {hi}");
    // Exactly on the wall's plane, and exactly 30 deep.
    assert!((lo.y - y0).abs() < 1e-9 && (hi.y - y1).abs() < 1e-9, "{lo} {hi}");
}

#[test]
fn arrow_keys_choose_the_drawing_plane() {
    let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
    e.set_tool(ToolKind::Cylinder);
    // → : the plane across the red axis, through the origin.
    e.input(ToolInput::Key { key: crate::Key::Right, mods: Modifiers::default() });
    click_at(&mut e, DVec3::ZERO);
    e.input(ToolInput::Vcb("40".into()));
    e.input(ToolInput::Vcb("-25".into()));
    let id = crate::tests::drawn(&e);
    // A cylinder's B-rep has a vertex per circle only: both 40 from the red axis, at x 0
    // and -25.
    let (lo, hi) = bounds(&e, id);
    assert!((lo.x + 25.0).abs() < 1e-9 && hi.x.abs() < 1e-9, "{lo} {hi}");
    for p in [lo, hi] {
        assert!((p.y.hypot(p.z) - 40.0).abs() < 1e-9, "{p}");
    }
    // The lock went with the operation: the next one is on the ground.
    e.set_tool(ToolKind::Box);
    assert_eq!(e.axis_lock(), None);
}

#[test]
fn shift_holds_the_line_of_a_move() {
    let mut e = top_editor(DVec3::ZERO);
    let a = add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
    add_box(&mut e, DVec3::new(300.0, 200.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
    e.select(vec![a]);
    e.set_tool(ToolKind::Move);
    click_at(&mut e, DVec3::new(50.0, 50.0, 50.0));
    // Along the red axis, Shift down: held there.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(250.0, 53.0, 50.0)) });
    assert_eq!(e.view().inference.map(|i| i.1), Some(InferKind::OnLine(LineKind::Axis(0))));
    e.set_shift(true);
    // To the other box's corner: the move stops level with it on the red axis.
    click_at(&mut e, DVec3::new(302.0, 203.0, 50.0));
    e.set_shift(false);
    // The start was a point of the face under the cursor; the end, level with the corner.
    let lo = bounds(&e, a).0;
    assert!(about(lo, DVec3::new(250.0, 0.0, 0.0)) && lo.y == 0.0 && lo.z == 0.0, "{lo}");
}

#[test]
fn a_point_on_an_arc_gives_its_tangent() {
    let mut e = top_editor(DVec3::ZERO);
    let c = DVec3::new(200.0, 200.0, 50.0);
    add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 600.0, height: 50.0 });
    let a = 30f64.to_radians();
    let radial = DVec3::new(a.cos(), a.sin(), 0.0);
    let i = at(&e, c + radial * 603.0);
    assert_eq!(i.kind, InferKind::OnEdge);
    let Some(EdgeRef::Arc { tangent, radial: r, center, radius }) = i.edge else { panic!("{:?}", i.edge) };
    assert!((center - c).length() < 1e-6 && (radius - 600.0).abs() < 1e-6, "{center} {radius}");
    // The angle comes from the cursor (rounded); the frame at the point found is exact.
    assert!((r - radial).length() < 1e-6 && (tangent - DVec3::new(-a.sin(), a.cos(), 0.0)).length() < 1e-6, "{r} {tangent}");
    assert!((i.point - c).normalize().dot(r) > 1.0 - 1e-12 && tangent.dot(r).abs() < 1e-12 && tangent.z == 0.0);
    // A straight edge gives its direction.
    let mut e = top_editor(DVec3::ZERO);
    add_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 150.0));
    assert_eq!(at(&e, DVec3::new(60.0, -4.0, 150.0)).edge, Some(EdgeRef::Straight(DVec3::X)));
}

#[test]
fn tangent_and_radial_lines() {
    let e = top_editor(DVec3::ZERO);
    let a = 30f64.to_radians();
    let (tangent, radial) = (DVec3::new(-a.sin(), a.cos(), 0.0), DVec3::new(a.cos(), a.sin(), 0.0));
    let arc = Some(EdgeRef::Arc { tangent, radial, center: DVec3::ZERO, radius: 1.0 });
    let from = DVec3::new(-200.0, -100.0, 0.0);
    let i = at_from(&e, from + tangent * 500.0 + radial * 3.0, from, arc, None);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Tangent));
    assert!((i.point - from).cross(tangent).length() < 1e-6);
    let i = at_from(&e, from + radial * 500.0 + tangent * 3.0, from, arc, None);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Radial));
    assert!((i.point - from).cross(radial).length() < 1e-6);
    // At 0°, the tangent is the green axis: the axis says it.
    let arc = Some(EdgeRef::Arc { tangent: DVec3::Y, radial: DVec3::X, center: DVec3::ZERO, radius: 1.0 });
    let i = at_from(&e, from + DVec3::Y * 500.0, from, arc, None);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(1)));
}

#[test]
fn move_along_the_tangent_of_an_arc_hovered() {
    let mut e = top_editor(DVec3::ZERO);
    let c = DVec3::new(-400.0, -400.0, 50.0);
    add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 300.0, height: 50.0 });
    let b = add_box(&mut e, DVec3::new(300.0, 300.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
    e.select(vec![b]);
    e.set_tool(ToolKind::Move);
    // Hover the circle at 30°: it becomes the reference.
    let a = 30f64.to_radians();
    let (tangent, radial) = (DVec3::new(-a.sin(), a.cos(), 0.0), DVec3::new(a.cos(), a.sin(), 0.0));
    e.input(ToolInput::Hover { px: px(&e, c + radial * 302.0) });
    assert_eq!(e.view().inference.map(|i| i.1), Some(InferKind::OnEdge));
    // Start at the box's corner; the destination follows the tangent.
    let from = DVec3::new(400.0, 400.0, 50.0);
    let p = px(&e, from + DVec3::new(2.0, -2.0, 0.0));
    e.input(ToolInput::Hover { px: p });
    e.input(ToolInput::Click { px: p, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, from + tangent * 300.0 + radial * 3.0) });
    assert_eq!(e.view().inference.map(|i| i.1), Some(InferKind::OnLine(LineKind::Tangent)));
}

#[test]
fn a_line_crossing_an_edge() {
    let mut e = top_editor(DVec3::ZERO);
    // A wall whose top edges run from y = -200 to 400 (midpoints at y = 100, away from it all).
    add_box(&mut e, DVec3::new(300.0, -200.0, 0.0), DVec3::new(20.0, 600.0, 50.0));
    // The red axis from a point at the wall's height crosses its top edge at x = 300.
    let from = DVec3::new(50.0, 50.0, 50.0);
    let i = at_from(&e, DVec3::new(302.0, 55.0, 50.0), from, None, None);
    assert_eq!(i.kind, InferKind::OnEdgeAndLine(LineKind::Axis(0)), "{i:?}");
    assert_eq!(i.point, DVec3::new(300.0, 50.0, 50.0));
    assert_eq!(i.line, Some((from, DVec3::X)));
    assert!(i.path.is_some());
    // From lower down, the axis only passes under the edge (seen from above they overlap).
    let i = at_from(&e, DVec3::new(302.0, 55.0, 50.0), DVec3::new(50.0, 50.0, 10.0), None, None);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Axis(0)), "{i:?}");
    // Locked on the green axis: the crossing with the wall's other top edge… none (parallel);
    // with the box's near edge, at y = -200.
    let i = at_from(&e, DVec3::new(303.0, -197.0, 50.0), DVec3::new(303.0, 50.0, 50.0), None, Some((DVec3::Y, LineKind::Axis(1))));
    assert_eq!(i.kind, InferKind::OnEdgeAndLine(LineKind::Axis(1)), "{i:?}");
    assert_eq!(i.point, DVec3::new(303.0, -200.0, 50.0));
}

#[test]
fn a_line_crossing_a_circle() {
    let mut e = top_editor(DVec3::ZERO);
    let c = DVec3::new(-400.0, -400.0, 50.0);
    add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 300.0, height: 50.0 });
    // In the circle's plane: the red axis through the centre meets it at ±300.
    let from = DVec3::new(-900.0, -400.0, 50.0);
    let i = at_from(&e, DVec3::new(-697.0, -396.0, 50.0), from, None, None);
    assert_eq!(i.kind, InferKind::OnEdgeAndLine(LineKind::Axis(0)), "{i:?}");
    assert!((i.point - DVec3::new(-700.0, -400.0, 50.0)).length() < 1e-9, "{}", i.point);
    // Through it: the blue axis from a point of the top circle, below, meets it there. Seen
    // from above the blue axis is end on: use a side view.
    let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
    add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 300.0, height: 50.0 });
    let on_circle = c + DVec3::new(0.0, -300.0, 0.0);
    let from = on_circle.with_z(-400.0);
    let i = at_px_from(&e, px(&e, on_circle) + DVec2::new(1.0, 1.5), from, None, None);
    assert_eq!(i.kind, InferKind::OnEdgeAndLine(LineKind::Axis(2)), "{i:?}");
    assert!((i.point - on_circle).length() < 1e-9, "{}", i.point);
}

/// What is inferred at the world point `p` with these tracked points (and maybe a start).
fn tracking(e: &Editor, p: DVec3, from: Option<DVec3>, tracked: &[DVec3]) -> Inference {
    let keep = |_: &Owner<'_>| true;
    let q = Query {
        keep: &keep,
        plane: (DVec3::ZERO, DVec3::Z),
        origin: Some(DVec3::ZERO),
        model_axes: false,
        from,
        tracked,
        in_plane: false,
        axes: [DVec3::X, DVec3::Y, DVec3::Z],
        reference: None,
        lock: None,
        tool_line: None,
    };
    infer(e.scene(), &e.camera, e.viewport_size(), px(e, p), &q).unwrap()
}

#[test]
fn tracked_points_give_lines_and_crossings() {
    let e = top_editor(DVec3::ZERO);
    let t = DVec3::new(400.0, 200.0, 0.0);
    let from = DVec3::new(-500.0, -300.0, 0.0);
    // The start's red axis × the tracked point's green one: exactly there.
    let i = tracking(&e, DVec3::new(403.0, -297.0, 0.0), Some(from), &[t]);
    assert_eq!(i.kind, InferKind::OnTwoLines(LineKind::Axis(0), LineKind::TrackedAxis(1)), "{i:?}");
    assert_eq!(i.point, DVec3::new(400.0, -300.0, 0.0));
    assert_eq!((i.line, i.line2), (Some((from, DVec3::X)), Some((t, DVec3::Y))));
    assert_eq!(i.kind.label(), "Axe rouge × axe vert depuis le point");
    // Along the tracked point's green axis alone.
    let i = tracking(&e, DVec3::new(403.0, 600.0, 0.0), Some(from), &[t]);
    assert_eq!(i.kind, InferKind::OnLine(LineKind::TrackedAxis(1)));
    assert_eq!(i.line, Some((t, DVec3::Y)));
    // Two tracked points and no start point (a tool's first click).
    let u = DVec3::new(-300.0, -100.0, 0.0);
    let i = tracking(&e, DVec3::new(403.0, -97.0, 0.0), None, &[t, u]);
    assert_eq!(i.kind, InferKind::OnTwoLines(LineKind::TrackedAxis(1), LineKind::TrackedAxis(0)), "{i:?}");
    assert_eq!(i.point, DVec3::new(400.0, -100.0, 0.0));
    // At other heights the lines only overlap on screen: no crossing.
    let i = tracking(&e, DVec3::new(403.0, -97.0, 0.0), None, &[t, u.with_z(50.0)]);
    assert!(!matches!(i.kind, InferKind::OnTwoLines(..)), "{i:?}");
}

#[test]
fn a_point_hovered_a_while_is_tracked() {
    let mut e = top_editor(DVec3::ZERO);
    let a = add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 50.0));
    add_box(&mut e, DVec3::new(300.0, 300.0, 0.0), DVec3::new(100.0, 100.0, 50.0));
    e.select(vec![a]);
    e.set_tool(ToolKind::Move);
    e.tick(10.0);
    let corner = DVec3::new(300.0, 300.0, 50.0);
    e.input(ToolInput::Hover { px: px(&e, corner + DVec3::new(2.0, -2.0, 0.0)) });
    assert!(e.tick(10.3), "waiting");
    assert!(e.tracked().is_empty());
    assert!(!e.tick(10.6));
    assert_eq!(e.tracked(), &[corner]);
    // The move starts at A's corner; the destination: level with the tracked corner on the
    // red axis, exactly.
    click_at(&mut e, DVec3::new(102.0, 2.0, 50.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(303.0, 3.0, 50.0)) });
    let v = e.view();
    assert_eq!(v.inference.map(|i| i.1), Some(InferKind::OnTwoLines(LineKind::Axis(0), LineKind::TrackedAxis(1))));
    assert!(v.overlay.iter().any(|l| l.a == corner), "a guide from the tracked point");
    e.input(ToolInput::Click { px: px(&e, DVec3::new(303.0, 3.0, 50.0)), mods: Modifiers::default() });
    assert_eq!(bounds(&e, a).0, DVec3::new(200.0, 0.0, 0.0));
    // Gone with the operation.
    assert!(e.tracked().is_empty());
}

#[test]
fn two_points_tracked_at_most() {
    let mut e = top_editor(DVec3::ZERO);
    let a = add_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 400.0, 50.0));
    // Taken: the start point is inferred (§6 septendecies).
    e.select(vec![a]);
    e.set_tool(ToolKind::Move);
    let corners = [DVec3::new(0.0, 0.0, 50.0), DVec3::new(400.0, 0.0, 50.0), DVec3::new(400.0, 400.0, 50.0)];
    let mut t = 0.0;
    for c in corners {
        e.tick(t);
        e.input(ToolInput::Hover { px: px(&e, c) });
        t += 1.0;
        e.tick(t);
        // Moving away (nothing there) ends the dwell.
        e.input(ToolInput::Hover { px: px(&e, DVec3::new(-700.0, 700.0, 0.0)) });
    }
    assert_eq!(e.tracked(), &corners[1..]);
    e.input(ToolInput::Key { key: crate::Key::Escape, mods: Modifiers::default() });
    assert!(e.tracked().is_empty());
}

#[test]
fn a_line_does_not_cross_an_edge_at_its_own_origin() {
    let mut e = top_editor(DVec3::ZERO);
    add_box(&mut e, DVec3::ZERO, DVec3::new(1000.0, 400.0, 50.0));
    // A start point on the top front edge (away from its midpoint): the green axis through it
    // meets that edge right there, which is no crossing.
    let from = DVec3::new(300.0, 0.0, 50.0);
    let i = at_from(&e, DVec3::new(310.0, -15.0, 50.0), from, None, None);
    assert_eq!(i.kind, InferKind::OnEdge, "{i:?}");
    // Nor do two lines meet at the start point: a tracked point level with it on the red axis.
    let i = tracking(&e, DVec3::new(310.0, -15.0, 50.0), Some(from), &[DVec3::new(-400.0, 0.0, 50.0)]);
    assert!(!matches!(i.kind, InferKind::OnTwoLines(..)), "{i:?}");
}

/// Timing, not a check (review finding 7): hovers swept over large scenes, iso perspective.
/// `cargo test --release -p solidsketch-tools infer_timing -- --ignored --nocapture`
#[test]
#[ignore]
fn infer_timing() {
    use solidsketch_model::{Boolean, Scope};
    use std::time::Instant;

    let iso = |e: &mut Editor| {
        e.set_parallel(false);
        let b = e.scene().bounds();
        e.camera.set_standard_view(StandardView::Iso, glam::DMat3::IDENTITY, b, ViewArea::full(e.viewport_size()));
    };
    let array = |e: &mut Editor, ids: Vec<Id>, step: DVec3, count: u32| -> Vec<Id> {
        let root = e.document().root();
        let op = Operation::Array { context: root, entities: ids.clone(), step: DAffine3::from_translation(step), count };
        let mut all = ids;
        all.extend(e.apply(op).unwrap().expect("done").created);
        all
    };

    // Many parts: n × n occurrences of one board (a cabinet maker's sheet of parts).
    let grid = |n: u32| {
        let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
        let a = add_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 60.0, 18.0));
        let row = array(&mut e, vec![a], DVec3::X * 150.0, n - 1);
        array(&mut e, row, DVec3::Y * 100.0, n - 1);
        iso(&mut e);
        e
    };
    // One heavy part: a board drilled with 2^k holes (circles: arcs, a dense mesh).
    let drilled = |k: usize| {
        let mut e = view_editor(StandardView::Iso, DVec3::ZERO);
        let root = e.document().root();
        let board = add_box(&mut e, DVec3::ZERO, DVec3::new(2000.0, 1000.0, 18.0));
        let drill = add(&mut e, Primitive::Cylinder { origin: DVec3::new(50.0, 50.0, -5.0), radius: 4.0, height: 30.0 });
        let target = EntityPath::root(board);
        e.apply(Operation::Boolean { target: target.clone(), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::All })
            .unwrap();
        let _ = root;
        for i in 0..k {
            let holes = e.document().resolve_part(&target).unwrap().1.machinings().iter().map(|m| m.id).collect();
            let step = if i < 4 { DVec3::X * (100.0 * (1 << i) as f64) } else { DVec3::Y * (100.0 * (1 << (i - 4)) as f64) };
            e.apply(Operation::DuplicateBodies {
                target: target.clone(),
                bodies: holes,
                transform: DAffine3::from_translation(step),
                scope: Scope::All,
            })
            .unwrap();
        }
        assert!(!e.busy());
        iso(&mut e);
        e
    };

    let run = |name: &str, e: &Editor| {
        let (parts, edges, tris) =
            e.scene().inferable().fold((0, 0, 0), |(p, ed, t), (_, m, _)| (p + 1, ed + m.snaps.edges.len(), t + m.mesh.indices.len() / 3));
        let keep = |_: &Owner<'_>| true;
        let tracked = [DVec3::new(10.0, 10.0, 18.0), DVec3::new(300.0, 500.0, 18.0)];
        let diagonal = Some(EdgeRef::Straight(DVec3::new(1.0, 1.0, 0.0).normalize()));
        for (case, from, tracked, reference) in
            [("points", None, &[][..], None), ("11 lines", Some(DVec3::new(0.0, 0.0, 18.0)), &tracked[..], diagonal)]
        {
            let q = Query {
                keep: &keep,
                plane: (DVec3::ZERO, DVec3::Z),
                origin: Some(DVec3::ZERO),
                model_axes: false,
                from,
                tracked,
                in_plane: false,
                axes: [DVec3::X, DVec3::Y, DVec3::Z],
                reference,
                lock: None,
                tool_line: None,
            };
            // Only the hovers over the geometry: elsewhere there is next to nothing to do.
            let mut times = Vec::new();
            for i in 0..100 {
                for j in 0..100 {
                    let px = DVec2::new(5.0 + 10.0 * i as f64, 5.0 + 10.0 * j as f64);
                    let t = Instant::now();
                    let r = std::hint::black_box(infer(e.scene(), &e.camera, e.viewport_size(), px, &q));
                    let ms = t.elapsed().as_secs_f64() * 1e3;
                    if r.is_some_and(|r| r.hit.is_some()) {
                        times.push(ms);
                    }
                }
            }
            times.sort_by(f64::total_cmp);
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            let pct = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
            println!(
                "{name:<18} {parts:>5} parts {edges:>6} edges {tris:>7} tris | {case:<8} {:>5} hovers | mean {mean:6.2} ms  p50 {:6.2}  p95 {:6.2}  max {:6.2}",
                times.len(),
                pct(0.5),
                pct(0.95),
                pct(1.0)
            );
        }
    };

    for n in [10, 20, 40] {
        run(&format!("grid {n}x{n}"), &grid(n));
    }
    for k in [5, 7] {
        run(&format!("board, {} holes", 1 << k), &drilled(k));
    }
}

fn add_guide(e: &mut Editor, shape: GuideShape) {
    let root = e.document().root();
    e.apply(Operation::AddGuide { component: root, shape }).unwrap();
}

#[test]
fn guides_are_inferred_on() {
    let mut e = top_editor(DVec3::ZERO);
    let along_x = |y: f64| GuideShape::Line { origin: DVec3::new(0.0, y, 0.0), dir: DVec3::X };
    add_guide(&mut e, along_x(-400.0));
    add_guide(&mut e, GuideShape::Line { origin: DVec3::new(-500.0, 0.0, 0.0), dir: DVec3::Y });
    // On a guide line: the point on it, the guide as the next reference.
    let i = at(&e, DVec3::new(-200.0, -404.0, 0.0));
    assert_eq!(i.kind, InferKind::OnLine(LineKind::Guide), "{i:?}");
    assert!(about(i.point, DVec3::new(-200.0, -400.0, 0.0)), "{}", i.point);
    assert_eq!(i.edge, Some(EdgeRef::Straight(DVec3::X)));
    assert_eq!(i.kind.label(), "Sur le guide");
    // Where two guides cross.
    let i = at(&e, DVec3::new(-503.0, -404.0, 0.0));
    assert_eq!(i.kind, InferKind::OnTwoLines(LineKind::Guide, LineKind::Guide), "{i:?}");
    assert!(about(i.point, DVec3::new(-500.0, -400.0, 0.0)), "{}", i.point);
    assert_eq!(i.kind.label(), "Guide × guide");

    // Where a guide crosses an edge, at the top of a box.
    add_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 150.0));
    add_guide(&mut e, GuideShape::Line { origin: DVec3::new(0.0, 50.0, 150.0), dir: DVec3::X });
    let i = at(&e, DVec3::new(303.0, 53.0, 150.0));
    assert_eq!(i.kind, InferKind::OnEdgeAndLine(LineKind::Guide), "{i:?}");
    assert!(about(i.point, DVec3::new(300.0, 50.0, 150.0)), "{}", i.point);
    // A guide hidden under the box is not inferred.
    add_guide(&mut e, along_x(120.0));
    let i = at(&e, DVec3::new(150.0, 123.0, 150.0));
    assert_eq!(i.kind, InferKind::OnFace, "{i:?}");
}

#[test]
fn guide_circles_are_arcs() {
    let mut e = top_editor(DVec3::ZERO);
    let c = DVec3::new(400.0, 400.0, 0.0);
    add_guide(&mut e, GuideShape::Circle { center: c, normal: DVec3::Z, radius: 200.0 });
    add_guide(&mut e, GuideShape::Line { origin: DVec3::new(0.0, 400.0, 0.0), dir: DVec3::X });
    // On it: the point at its radius, an arc to refer to.
    let a = 60f64.to_radians();
    let radial = DVec3::new(a.cos(), a.sin(), 0.0);
    let i = at(&e, c + radial * 204.0);
    assert_eq!(i.kind, InferKind::OnGuide, "{i:?}");
    assert!(((i.point - c).length() - 200.0).abs() < 1e-6, "{}", i.point);
    let Some(EdgeRef::Arc { center, radius, .. }) = i.edge else { panic!("{:?}", i.edge) };
    assert!(near(center, c) && (radius - 200.0).abs() < 1e-9);
    // Its centre.
    let i = at(&e, c + DVec3::new(3.0, 30.0, 0.0));
    assert_eq!((i.kind, i.point), (InferKind::Center, c));
    // Where the guide line crosses it.
    let i = at(&e, c + DVec3::new(203.0, 4.0, 0.0));
    assert_eq!(i.kind, InferKind::OnGuideAndLine(LineKind::Guide), "{i:?}");
    assert!(about(i.point, c + DVec3::new(200.0, 0.0, 0.0)), "{}", i.point);
    assert_eq!(i.kind.label(), "Guide × guide");
}

#[test]
fn the_axes_as_drawn_are_lines() {
    let e = top_editor(DVec3::ZERO);
    let keep = |_: &Owner<'_>| true;
    let q = |model_axes| Query {
        keep: &keep,
        plane: (DVec3::ZERO, DVec3::Z),
        origin: Some(DVec3::ZERO),
        model_axes,
        from: None,
        tracked: &[],
        in_plane: false,
        axes: [DVec3::X, DVec3::Y, DVec3::Z],
        reference: None,
        lock: None,
        tool_line: None,
    };
    let px = px(&e, DVec3::new(-300.0, 6.0, 0.0));
    let i = infer(e.scene(), &e.camera, e.viewport_size(), px, &q(true)).unwrap();
    assert_eq!(i.kind, InferKind::OnLine(LineKind::ModelAxis(0)), "{i:?}");
    assert!(about(i.point, DVec3::new(-300.0, 0.0, 0.0)), "{}", i.point);
    assert_eq!(i.edge, Some(EdgeRef::Straight(DVec3::X)));
    assert_eq!(i.kind.label(), "Sur l'axe rouge");
    // Hidden axes: not inferred on.
    let i = infer(e.scene(), &e.camera, e.viewport_size(), px, &q(false)).unwrap();
    assert_eq!(i.kind, InferKind::OnPlane);
}

#[test]
fn a_guide_circle_crosses_edges() {
    let mut e = top_editor(DVec3::ZERO);
    // Across the top front edge of a box: 66.33 either side of x = 150.
    add_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 150.0));
    add_guide(&mut e, GuideShape::Circle { center: DVec3::new(150.0, 100.0, 150.0), normal: DVec3::Z, radius: 120.0 });
    let x = 150.0 + (120.0f64 * 120.0 - 100.0 * 100.0).sqrt();
    let i = at(&e, DVec3::new(x + 3.0, 4.0, 150.0));
    assert_eq!(i.kind, InferKind::OnGuideAndEdge, "{i:?}");
    assert!(near(i.point, DVec3::new(x, 0.0, 150.0)), "{}", i.point);
    assert_eq!(i.kind.label(), "Guide × arête");

    // Across a cylinder's top circle, in its plane: at x = 650, y = 600 ± 86.6.
    let c = DVec3::new(600.0, 600.0, 50.0);
    add(&mut e, Primitive::Cylinder { origin: c.with_z(0.0), radius: 100.0, height: 50.0 });
    add_guide(&mut e, GuideShape::Circle { center: c + DVec3::X * 100.0, normal: DVec3::Z, radius: 100.0 });
    let p = c + DVec3::new(50.0, 75f64.sqrt() * 10.0, 0.0);
    let i = at(&e, p + DVec3::new(3.0, 3.0, 0.0));
    assert_eq!(i.kind, InferKind::OnGuideAndEdge, "{i:?}");
    assert!(near(i.point, p), "{}", i.point);
    // And upright, through its centre: where the circle's plane is crossed.
    add_guide(&mut e, GuideShape::Circle { center: c, normal: DVec3::Y, radius: 100.0 });
    let i = at(&e, c + DVec3::new(-103.0, 3.0, 0.0));
    assert_eq!(i.kind, InferKind::OnGuideAndEdge, "{i:?}");
    assert!(near(i.point, c - DVec3::X * 100.0), "{}", i.point);
}

#[test]
fn two_guide_circles_cross() {
    let mut e = top_editor(DVec3::ZERO);
    let c = DVec3::new(-300.0, 200.0, 0.0);
    add_guide(&mut e, GuideShape::Circle { center: c, normal: DVec3::Z, radius: 200.0 });
    add_guide(&mut e, GuideShape::Circle { center: c + DVec3::X * 200.0, normal: DVec3::Z, radius: 200.0 });
    // At x = 100 from the first centre, y = ±173.2.
    let p = c + DVec3::new(100.0, 30000f64.sqrt(), 0.0);
    let i = at(&e, p + DVec3::new(3.0, -4.0, 0.0));
    assert_eq!(i.kind, InferKind::OnTwoGuides, "{i:?}");
    assert!(near(i.point, p), "{}", i.point);
    assert_eq!(i.kind.label(), "Guide × guide");
    // Upright, through the first one's centre: across its plane.
    add_guide(&mut e, GuideShape::Circle { center: c, normal: DVec3::Y, radius: 200.0 });
    let i = at(&e, c + DVec3::new(-203.0, 3.0, 0.0));
    assert_eq!(i.kind, InferKind::OnTwoGuides, "{i:?}");
    assert!(near(i.point, c - DVec3::X * 200.0), "{}", i.point);
}

#[test]
fn the_meshes_near_the_ray_are_those_a_scan_finds() {
    // Boxes of mixed sizes at mixed depths, seen in perspective (the aperture grows with
    // depth): the index's candidates are exactly the scan's, in the same order.
    let mut e = view_editor(StandardView::Iso, DVec3::new(400.0, 400.0, 0.0));
    e.set_parallel(false);
    for i in 0..8 {
        for j in 0..8 {
            let size = DVec3::new(30.0 + (i * j % 4) as f64 * 25.0, 40.0, 20.0 + (j % 3) as f64 * 60.0);
            add_box(&mut e, DVec3::new(i as f64 * 110.0, j as f64 * 110.0, (i % 2) as f64 * 80.0), size);
        }
    }
    add_box(&mut e, DVec3::new(-200.0, -200.0, -30.0), DVec3::new(1300.0, 1300.0, 20.0));
    let (camera, size, scene) = (&e.camera, e.viewport_size(), e.scene());
    let mut met = 0;
    for x in (0..1000).step_by(37) {
        for y in (0..1000).step_by(37) {
            let ray = camera.ray(DVec2::new(x as f64, y as f64), size);
            let near = |b: &(DVec3, DVec3)| near_ray(camera, size, &ray, *b);
            let key = |v: Vec<(&crate::Placement, &crate::PartMesh, crate::Piece)>| -> Vec<(Vec<Id>, crate::Piece)> {
                v.into_iter().map(|(p, _, piece)| (p.path.clone(), piece)).collect()
            };
            let found = key(scene.inferable_where(near));
            let scan = key(scene
                .inferable()
                .filter(|(p, m, _)| m.bounds.is_some_and(|b| near(&crate::scene::transform_bounds(&p.transform, b))))
                .collect());
            assert_eq!(found, scan, "{x} {y}");
            met += found.len();
        }
    }
    assert!(met > 100, "{met}");
}

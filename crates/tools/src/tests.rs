//! Whole scenarios driven like the app does: pixels in, operations out.

use glam::{DVec2, DVec3};
use solidsketch_model::{Boolean, Document, EntityPath, Id, Operation, Primitive};

use crate::*;

/// An editor looking straight down (parallel), framing ±1 m.
fn top_editor(doc: Document) -> Editor {
    let mut e = Editor::new(doc, Box::new(solidsketch_kernel::Inline::default()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.set_parallel(true);
    e.camera.set_standard_view(
        StandardView::Top,
        Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))),
        crate::camera::ViewArea::full(e.viewport_size()),
    );
    e
}

fn px(e: &Editor, p: DVec3) -> DVec2 {
    e.camera.project(p, e.viewport_size()).unwrap()
}

fn click(e: &mut Editor, p: DVec3) {
    let at = px(e, p);
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
}

/// World bounds of the part an occurrence path names.
fn bounds_of(e: &Editor, path: &EntityPath) -> (DVec3, DVec3) {
    let (_, part, t) = e.document().resolve_part(path).unwrap();
    crate::scene::transform_bounds(&t, part.shape().vertex_bounds().unwrap())
}

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-3
}

/// What the last drawing made: the newest occurrence of the context being edited, or the
/// newest machining of the open part.
pub(crate) fn drawn(e: &Editor) -> Id {
    let doc = e.document();
    if let Some(part) = e.open_part() {
        let (_, part, _) = doc.resolve_part(&EntityPath::new(e.context().to_vec(), part)).unwrap();
        return part.machinings.last().unwrap().id;
    }
    doc.assembly(doc.context_of(e.context()).unwrap()).unwrap().occurrences.last().unwrap().id
}

/// Draws a box with the tool: two corners clicked, the height typed. The selection is left
/// as it was; the box is then selected, as the steps that follow expect.
fn draw_box(e: &mut Editor, a: DVec3, b: DVec3, h: &str) -> Id {
    let before = e.selection().to_vec();
    e.set_tool(ToolKind::Box);
    click(e, a);
    click(e, b);
    e.input(ToolInput::Vcb(h.into()));
    e.set_tool(ToolKind::Select);
    assert_eq!(e.selection(), before.as_slice(), "{}", e.status());
    let id = drawn(e);
    e.select(vec![id]);
    id
}

#[test]
fn box_tool_by_clicks_and_values() {
    let mut e = top_editor(Document::new());
    let id = draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "150");
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(300.0, 200.0, 150.0)), "{lo} {hi}");

    // Typed base: "600;400" from the first corner, then the height.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(-500.0, -500.0, 0.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-400.0, -400.0, 0.0)) });
    e.input(ToolInput::Vcb("60cm;0,4m".into()));
    e.input(ToolInput::Vcb("10".into()));
    let id2 = drawn(&e);
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id2));
    assert!(close(lo, DVec3::new(-500.0, -500.0, 0.0)) && close(hi, DVec3::new(100.0, -100.0, 10.0)), "{lo} {hi}");

    // A box drawn on the top face of the first one starts at its height.
    let id3 = draw_box(&mut e, DVec3::new(50.0, 50.0, 150.0), DVec3::new(100.0, 100.0, 150.0), "20");
    let (lo, _) = bounds_of(&e, &EntityPath::root(id3));
    assert!((lo.z - 150.0).abs() < 1e-6, "{lo}");
}

#[test]
fn typed_expressions_lists_and_points() {
    let mut e = top_editor(Document::new());
    // `2*300=`: a square base of 600; a refused height says why; `200/2` the height.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::ZERO);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 50.0, 0.0)) });
    e.input(ToolInput::Vcb("2*300=".into()));
    e.input(ToolInput::Vcb("2m*3m".into()));
    assert_eq!(e.status(), "Hauteur — « 2m*3m » : unités incompatibles");
    e.input(ToolInput::Vcb("200/2".into()));
    let id = drawn(&e);
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(600.0, 600.0, 100.0)), "{lo} {hi}");

    // `;@*2`: the width kept, the length doubled — signed as the drag, here towards -y.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(-500.0, -500.0, 0.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-400.0, -550.0, 0.0)) });
    e.input(ToolInput::Vcb(";@*2".into()));
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(-500.0, -600.0, 0.0)) && close(hi, DVec3::new(-400.0, -500.0, 10.0)), "{lo} {hi}");

    // The height with the base: done at once (`50==`: a cube), and `r;h` for a cylinder.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(700.0, 700.0, 0.0));
    e.input(ToolInput::Vcb("50==".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(700.0, 700.0, 0.0)) && close(hi, DVec3::new(750.0, 750.0, 50.0)), "{lo} {hi}");
    e.set_tool(ToolKind::Cylinder);
    click(&mut e, DVec3::new(-700.0, 700.0, 0.0));
    e.input(ToolInput::Vcb("20;-30".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!((lo.z + 30.0).abs() < 1e-6 && hi.z.abs() < 1e-6, "{lo} {hi}");

    // A typed point: absolute, then relative to the start. The axes hidden: the cursor would
    // take the red one, a few pixels off.
    let mut s = e.document().settings().clone();
    s.display.axes = false;
    e.apply(Operation::SetSettings(s)).unwrap();
    e.set_tool(ToolKind::Select);
    click(&mut e, DVec3::new(300.0, 300.0, 100.0));
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(0.0, 0.0, 100.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(30.0, 0.0, 100.0)) });
    e.input(ToolInput::Vcb("[10;20;100]".into()));
    let (lo, _) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::new(10.0, 20.0, 0.0)), "{lo}");
    click(&mut e, DVec3::new(10.0, 20.0, 100.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(40.0, 20.0, 100.0)) });
    e.input(ToolInput::Vcb("<-10;;@-100>".into()));
    let (lo, _) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::new(0.0, 20.0, -100.0)), "{lo}");
}

#[test]
fn cylinder_tool() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Cylinder);
    click(&mut e, DVec3::new(100.0, 100.0, 0.0));
    e.input(ToolInput::Vcb("50".into()));
    e.input(ToolInput::Vcb("-30".into()));
    let id = drawn(&e);
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id));
    assert!((lo.z + 30.0).abs() < 1e-6 && hi.z.abs() < 1e-6, "{lo} {hi}");
    // (A cylinder's vertices are its seam's ends: the centre plus the radius along red.)
    assert!((lo.x - 150.0).abs() < 1e-3, "radius 50: {lo} {hi}");

    // Measured across (Tab turns the measure over): the typed length is the diameter.
    e.input(ToolInput::Key { key: Key::Tab, mods: Modifiers::default() });
    click(&mut e, DVec3::new(300.0, 100.0, 0.0));
    e.input(ToolInput::Vcb("50;10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!((lo.x - 325.0).abs() < 1e-3, "diameter 50: {lo} {hi}");
}

/// Two boxes on the ground: `a` 300 × 200 × 150 at the origin, `b` 200 × 200 × 50 beside it.
fn two_boxes(e: &mut Editor) {
    draw_box(e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "150");
    draw_box(e, DVec3::new(400.0, 0.0, 0.0), DVec3::new(600.0, 200.0, 0.0), "50");
}

/// Draws a box with the tool: two corners clicked, the height typed; its world bounds.
fn box_bounds(e: &mut Editor, a: DVec3, b: DVec3, h: &str) -> (DVec3, DVec3) {
    e.set_tool(ToolKind::Box);
    click(e, a);
    click(e, b);
    e.input(ToolInput::Vcb(h.into()));
    bounds_of(e, &EntityPath::root(drawn(e)))
}

#[test]
fn box_base_takes_the_plane_the_second_corner_aims_at() {
    let mut e = top_editor(Document::new());
    two_boxes(&mut e);

    // From the ground to the top of `a`, off the ground plane: upright through both corners,
    // the height towards the eye (the top view looks a hair towards +y).
    let (lo, hi) = box_bounds(&mut e, DVec3::new(-300.0, 100.0, 0.0), DVec3::new(100.0, 100.0, 150.0), "20");
    assert!(close(lo, DVec3::new(-300.0, 80.0, 0.0)) && close(hi, DVec3::new(100.0, 100.0, 150.0)), "{lo} {hi}");

    e.undo();
    // To the midpoint of an edge of `a`'s top: the plane through that edge and the first corner.
    let (lo, hi) = box_bounds(&mut e, DVec3::new(-300.0, 50.0, 0.0), DVec3::new(0.0, 100.0, 150.0), "10");
    let n = DVec3::new(-1.0, 0.0, 2.0).normalize() * 10.0;
    assert!(close(lo, DVec3::new(-300.0 + n.x, 50.0, 0.0)), "{lo}");
    assert!(close(hi, DVec3::new(0.0, 100.0, 150.0 + n.z)), "{hi}");
}

#[test]
fn down_locks_the_plane_of_the_face_under_the_cursor() {
    let mut e = top_editor(Document::new());
    two_boxes(&mut e);
    e.set_tool(ToolKind::Box);
    let down = Key::Down;
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 100.0, 150.0)) });
    e.input(ToolInput::Key { key: down, mods: Modifiers::default() });
    assert_eq!(e.face_lock(), Some(DVec3::Z));
    // The same face again: free; and once more, locked.
    e.input(ToolInput::Key { key: down, mods: Modifiers::default() });
    assert_eq!(e.face_lock(), None);
    e.input(ToolInput::Key { key: down, mods: Modifiers::default() });
    // The second corner on `b`'s top stays in the plane of `a`'s, instead of standing up.
    click(&mut e, DVec3::new(100.0, 100.0, 150.0));
    click(&mut e, DVec3::new(500.0, 150.0, 50.0));
    e.input(ToolInput::Vcb("10".into()));
    // (The top view looks a hair askew: the cursor's ray meets the plane 100 higher a bit off.)
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    let near = |a: DVec3, b: DVec3| (a - b).length() < 0.05;
    assert!(near(lo, DVec3::new(100.0, 100.0, 150.0)) && near(hi, DVec3::new(500.0, 150.0, 160.0)), "{lo} {hi}");
    // Gone with the operation.
    assert_eq!(e.face_lock(), None);
}

#[test]
fn a_base_started_on_an_askew_edge_runs_along_it() {
    use glam::{DAffine3, DQuat};
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 400.0, 0.0), "50");
    let turn =
        DAffine3::from_translation(DVec3::new(-200.0, -200.0, 0.0)) * DAffine3::from_quat(DQuat::from_rotation_z(30f64.to_radians()));
    e.apply(Operation::Transform { context: e.document().root(), entities: vec![a], transform: turn }).unwrap();
    let on_a = |p: DVec3| turn.transform_point3(p);
    // From the midpoint of its first edge to a point of its top: a base square to the box.
    let (lo, hi) = box_bounds(&mut e, on_a(DVec3::new(200.0, 0.0, 50.0)), on_a(DVec3::new(300.0, 100.0, 50.0)), "20");
    let corners = [(200.0, 0.0), (300.0, 0.0), (300.0, 100.0), (200.0, 100.0)].map(|(x, y)| on_a(DVec3::new(x, y, 50.0)));
    let want_lo = corners.iter().fold(DVec3::splat(f64::MAX), |m, c| m.min(*c));
    let want_hi = corners.iter().fold(DVec3::splat(f64::MIN), |m, c| m.max(*c)) + DVec3::Z * 20.0;
    assert!(close(lo, want_lo) && close(hi, want_hi), "{lo} {hi} / {want_lo} {want_hi}");
}

#[test]
fn a_base_whose_second_corner_is_on_a_guide_runs_along_it() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let d = DVec3::new(30f64.to_radians().cos(), 30f64.to_radians().sin(), 0.0);
    let origin = DVec3::new(400.0, 0.0, 0.0);
    e.apply(Operation::AddGuide { component: root, shape: GuideShape::Line { origin, dir: d } }).unwrap();
    // From the origin to a point of the guide: the sides along it, the corner on it.
    let q = origin + d * 200.0;
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::ZERO);
    hover(&mut e, q);
    assert_eq!(e.inference().map(|i| i.1), Some(crate::InferKind::OnLine(crate::LineKind::Guide)));
    click(&mut e, q);
    e.input(ToolInput::Vcb("20".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    let v = DVec3::Z.cross(d);
    let corners = [DVec3::ZERO, d * q.dot(d), q, v * q.dot(v)];
    let want_lo = corners.iter().fold(DVec3::splat(f64::MAX), |m, c| m.min(*c));
    let want_hi = corners.iter().fold(DVec3::splat(f64::MIN), |m, c| m.max(*c)) + DVec3::Z * 20.0;
    assert!(close(lo, want_lo) && close(hi, want_hi), "{lo} {hi} / {want_lo} {want_hi}");
}

#[test]
fn a_base_started_on_a_guide_runs_along_it() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let d = DVec3::new(30f64.to_radians().cos(), 30f64.to_radians().sin(), 0.0);
    let origin = DVec3::new(-300.0, -200.0, 0.0);
    e.apply(Operation::AddGuide { component: root, shape: GuideShape::Line { origin, dir: d } }).unwrap();
    // From a point of the guide to one off it: the sides along the guide.
    let a = origin + d * 200.0;
    let b = DVec3::new(300.0, -150.0, 0.0);
    e.set_tool(ToolKind::Box);
    hover(&mut e, a);
    assert_eq!(e.inference().map(|i| i.1), Some(crate::InferKind::OnLine(crate::LineKind::Guide)));
    click(&mut e, a);
    click(&mut e, b);
    e.input(ToolInput::Vcb("20".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    let (v, w) = (DVec3::Z.cross(d), b - a);
    let corners = [a, a + d * w.dot(d), b, a + v * w.dot(v)];
    let want_lo = corners.iter().fold(DVec3::splat(f64::MAX), |m, c| m.min(*c));
    let want_hi = corners.iter().fold(DVec3::splat(f64::MIN), |m, c| m.max(*c)) + DVec3::Z * 20.0;
    assert!(close(lo, want_lo) && close(hi, want_hi), "{lo} {hi} / {want_lo} {want_hi}");
}

#[test]
fn a_base_on_a_guide_circle_runs_along_its_tangent() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    e.apply(Operation::AddGuide { component: root, shape: GuideShape::Circle { center: DVec3::ZERO, normal: DVec3::Z, radius: 300.0 } })
        .unwrap();
    let t = 60f64.to_radians();
    let on = DVec3::new(t.cos(), t.sin(), 0.0) * 300.0;
    let d = DVec3::new(-t.sin(), t.cos(), 0.0);
    // The rectangle from `a` to `b`, its sides along `d`, pulled up 20.
    let want = |a: DVec3, b: DVec3| {
        let (v, w) = (DVec3::Z.cross(d), b - a);
        let corners = [a, a + d * w.dot(d), b, a + v * w.dot(v)];
        let lo = corners.iter().fold(DVec3::splat(f64::MAX), |m, c| m.min(*c));
        (lo, corners.iter().fold(DVec3::splat(f64::MIN), |m, c| m.max(*c)) + DVec3::Z * 20.0)
    };
    for (a, b) in [(DVec3::new(-500.0, -100.0, 0.0), on), (on, DVec3::new(500.0, -200.0, 0.0))] {
        e.set_tool(ToolKind::Box);
        hover(&mut e, a);
        click(&mut e, a);
        hover(&mut e, b);
        click(&mut e, b);
        e.input(ToolInput::Vcb("20".into()));
        let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
        let (want_lo, want_hi) = want(a, b);
        assert!(close(lo, want_lo) && close(hi, want_hi), "{a} {b}: {lo} {hi} / {want_lo} {want_hi}");
        e.undo();
    }
}

#[test]
fn a_cylinder_s_radius_reaches_a_guide_circle() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let g = DVec3::new(-600.0, 300.0, 0.0);
    e.apply(Operation::AddGuide { component: root, shape: GuideShape::Circle { center: g, normal: DVec3::Z, radius: 200.0 } }).unwrap();
    let c = DVec3::new(-100.0, 100.0, 0.0);
    let t = 70f64.to_radians();
    let p = g + DVec3::new(t.cos(), t.sin(), 0.0) * 200.0;
    e.set_tool(ToolKind::Cylinder);
    click(&mut e, c);
    hover(&mut e, p);
    assert_eq!(e.inference().map(|i| i.1), Some(crate::InferKind::OnGuide));
    click(&mut e, p);
    e.input(ToolInput::Vcb("20".into()));
    // (A cylinder's vertices are its seam's ends: the centre plus the radius along red.)
    let (lo, _) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!((lo.x - c.x - c.distance(p)).abs() < 1e-3, "{} / {}", lo.x - c.x, c.distance(p));
}

#[test]
fn once_drawn_the_plane_shows_under_the_cursor_again() {
    let mut e = top_editor(Document::new());
    let (a, b) = (DVec3::new(-200.0, -200.0, 0.0), DVec3::new(100.0, 50.0, 0.0));
    e.set_tool(ToolKind::Box);
    click(&mut e, a);
    click(&mut e, b);
    e.input(ToolInput::Vcb("150".into()));
    // The cursor is still where the second corner was: the small base starts there, on the top
    // of the new box, not where the first corner was.
    let at = |p: DVec3| e.view().overlay.iter().any(|l| (l.a.truncate() - p.truncate()).length() < 1e-3);
    assert!(at(b) && !at(a));
}

#[test]
fn box_base_snaps_to_the_square_and_draws_from_its_centre() {
    let mut e = top_editor(Document::new());
    // A corner a few pixels off the square's: the square.
    let (lo, hi) = box_bounds(&mut e, DVec3::ZERO, DVec3::new(100.0, 95.0, 0.0), "10");
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(100.0, 100.0, 10.0)), "{lo} {hi}");

    // The copy modifier tapped while drawing the base (not before: held there, it takes a
    // face's centroid): centred, kept for the tool.
    let centered = |e: &Editor| e.tool_options().iter().find(|o| o.key == "centered").map(|o| o.value.clone());
    let tap = ToolInput::Key { key: Key::Copy, mods: Modifiers::default() };
    e.set_tool(ToolKind::Box);
    e.input(tap.clone());
    assert_eq!(centered(&e), Some(OptionValue::Toggle(false)));
    click(&mut e, DVec3::new(-500.0, -500.0, 0.0));
    e.input(tap);
    assert_eq!(centered(&e), Some(OptionValue::Toggle(true)));
    click(&mut e, DVec3::new(-400.0, -540.0, 0.0));
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(-600.0, -540.0, 0.0)) && close(hi, DVec3::new(-400.0, -460.0, 10.0)), "{lo} {hi}");
    // Typed, the whole lengths, signed as the drag.
    e.set_tool(ToolKind::Box);
    assert_eq!(centered(&e), Some(OptionValue::Toggle(true)));
    click(&mut e, DVec3::new(500.0, 500.0, 0.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(520.0, 480.0, 0.0)) });
    e.input(ToolInput::Vcb("200;100;10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(400.0, 450.0, 0.0)) && close(hi, DVec3::new(600.0, 550.0, 10.0)), "{lo} {hi}");
}

#[test]
fn the_shape_at_the_cursor_hides_when_it_leaves_the_view() {
    let mut e = top_editor(Document::new());
    for tool in [ToolKind::Box, ToolKind::Cylinder] {
        e.set_tool(tool);
        e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 100.0, 0.0)) });
        assert!(!e.view().overlay.is_empty());
        e.leave();
        assert!(e.view().overlay.is_empty());
        // Back in the view: shown again.
        e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 100.0, 0.0)) });
        assert!(!e.view().overlay.is_empty());
    }
}

#[test]
fn the_height_takes_the_last_one_the_next_face_or_both_sides() {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "150");
    // A block floating above the middle of `a`'s top.
    let block = Primitive::Box { origin: DVec3::new(120.0, 70.0, 350.0), size: DVec3::new(60.0, 60.0, 50.0) };
    e.apply(Operation::AddPrimitive { context: root, primitive: block, placement: glam::DAffine3::IDENTITY }).unwrap();

    // A double-click on the opposite corner: the last height (150).
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(500.0, 500.0, 0.0));
    click(&mut e, DVec3::new(600.0, 600.0, 0.0));
    e.input(ToolInput::DoubleClick { px: px(&e, DVec3::new(600.0, 600.0, 0.0)), mods: Modifiers::default() });
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(500.0, 500.0, 0.0)) && close(hi, DVec3::new(600.0, 600.0, 150.0)), "{lo} {hi}");

    // Shift held: the last height, whatever the cursor.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(-500.0, -500.0, 0.0));
    click(&mut e, DVec3::new(-400.0, -400.0, 0.0));
    e.set_shift(true);
    assert!(e.vcb().is_some_and(|(_, v)| v.contains("150")), "{:?}", e.vcb());
    e.set_shift(false);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });

    // The copy modifier held: up to the first face along the normal (the block's bottom).
    click(&mut e, DVec3::new(50.0, 40.0, 150.0));
    click(&mut e, DVec3::new(250.0, 160.0, 150.0));
    e.set_copy(true);
    click(&mut e, DVec3::new(180.0, 130.0, 400.0));
    e.set_copy(false);
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(50.0, 40.0, 150.0)) && close(hi, DVec3::new(250.0, 160.0, 350.0)), "{lo} {hi}");

    // Centred: the whole height typed, half on each side of the plane.
    e.set_tool_option("pull_centered", OptionValue::Toggle(true));
    let (lo, hi) = box_bounds(&mut e, DVec3::new(-800.0, 500.0, 0.0), DVec3::new(-700.0, 600.0, 0.0), "40");
    assert!(close(lo, DVec3::new(-800.0, 500.0, -20.0)) && close(hi, DVec3::new(-700.0, 600.0, 20.0)), "{lo} {hi}");
}

#[test]
fn the_height_takes_a_face_parallel_to_the_plane() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "150");

    // A base on the ground beside `a`, the height clicked inside `a`'s top: up to that plane.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(400.0, 0.0, 0.0));
    click(&mut e, DVec3::new(500.0, 100.0, 0.0));
    click(&mut e, DVec3::new(120.0, 80.0, 150.0));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(400.0, 0.0, 0.0)) && close(hi, DVec3::new(500.0, 100.0, 150.0)), "{lo} {hi}");

    // The cylinder likewise.
    e.set_tool(ToolKind::Cylinder);
    click(&mut e, DVec3::new(-300.0, -300.0, 0.0));
    click(&mut e, DVec3::new(-250.0, -300.0, 0.0));
    click(&mut e, DVec3::new(120.0, 80.0, 150.0));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(-250.0, -300.0, 0.0)) && close(hi, DVec3::new(-250.0, -300.0, 150.0)), "{lo} {hi}");
}

#[test]
fn the_first_point_from_a_face_centre_a_whole_face_or_a_corner() {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "150");
    let disc = Primitive::Cylinder { origin: DVec3::new(600.0, 600.0, 0.0), radius: 50.0, height: 30.0 };
    e.apply(Operation::AddPrimitive { context: root, primitive: disc, placement: glam::DAffine3::IDENTITY }).unwrap();
    let hover = |e: &mut Editor, p: DVec3| e.input(ToolInput::Hover { px: px(e, p) });

    // The copy modifier held over a face: its centroid.
    e.set_tool(ToolKind::Box);
    e.set_copy(true);
    hover(&mut e, DVec3::new(100.0, 50.0, 150.0));
    let (at, kind) = e.inference().unwrap();
    assert!(close(at, DVec3::new(150.0, 100.0, 150.0)) && kind == InferKind::Center, "{at} {kind:?}");
    // And Shift tapped: the whole face as the base.
    e.input(ToolInput::Key { key: Key::Shift, mods: Modifiers::default() });
    e.set_copy(false);
    // (The cursor on a corner of the face: a typed height goes the way the cursor gives.)
    hover(&mut e, DVec3::new(300.0, 200.0, 150.0));
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(0.0, 0.0, 150.0)) && close(hi, DVec3::new(300.0, 200.0, 160.0)), "{lo} {hi}");
    e.undo();

    // The cylinder over a disc: that circle.
    e.set_tool(ToolKind::Cylinder);
    e.set_copy(true);
    hover(&mut e, DVec3::new(610.0, 590.0, 30.0));
    e.input(ToolInput::Key { key: Key::Shift, mods: Modifiers::default() });
    e.set_copy(false);
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    // (A cylinder's vertices are its seam's ends: the centre plus the radius along red.)
    assert!(close(lo, DVec3::new(650.0, 600.0, 30.0)) && close(hi, DVec3::new(650.0, 600.0, 40.0)), "{lo} {hi}");

    // From a corner (the other modifier tapped): across a face, the distances from its sides.
    e.set_tool(ToolKind::Box);
    e.input(ToolInput::Key { key: Key::Alt, mods: Modifiers::default() });
    hover(&mut e, DVec3::new(80.0, 60.0, 150.0));
    let (_, shown) = e.vcb().unwrap();
    assert!(shown.contains("60") && shown.contains("80"), "{shown}");
    e.input(ToolInput::Vcb("10;10".into()));
    click(&mut e, DVec3::new(100.0, 60.0, 150.0));
    e.input(ToolInput::Vcb("5".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(10.0, 10.0, 150.0)) && close(hi, DVec3::new(100.0, 60.0, 155.0)), "{lo} {hi}");
    // Along an edge, the distance from its nearer end; the mode ends with the first point.
    assert!(e.tool_options().iter().any(|o| o.key == "from_corner" && o.value == OptionValue::Toggle(false)));
    e.input(ToolInput::Key { key: Key::Alt, mods: Modifiers::default() });
    hover(&mut e, DVec3::new(300.0, 150.0, 150.0));
    e.input(ToolInput::Vcb("70".into()));
    click(&mut e, DVec3::new(200.0, 100.0, 150.0));
    e.input(ToolInput::Vcb("5".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(200.0, 100.0, 150.0)) && close(hi, DVec3::new(300.0, 130.0, 155.0)), "{lo} {hi}");
}

#[test]
fn escape_goes_back_one_step() {
    use solidsketch_i18n::t;
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::ZERO);
    click(&mut e, DVec3::new(100.0, 100.0, 0.0));
    let esc = ToolInput::Key { key: Key::Escape, mods: Modifiers::default() };
    e.input(esc.clone());
    let key = crate::tools::copy_key();
    assert_eq!(e.status(), t!("tool.box.hint.base", copy_key = key.clone()));
    e.input(esc.clone());
    assert_eq!(e.status(), t!("tool.box.hint.first", copy_key = key.clone(), alt_key = crate::tools::alt_key()));
    // At the first step, Esc does nothing: the tool stays.
    e.input(esc);
    assert_eq!((e.tool_kind(), e.status()), (ToolKind::Box, t!("tool.box.hint.first", copy_key = key, alt_key = crate::tools::alt_key())));
}

#[test]
fn select_hover_and_escape() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(e.selection(), &[a]);
    e.input(ToolInput::Click { px: px(&e, DVec3::new(350.0, 50.0, 50.0)), mods: Modifiers { toggle: true, copy: false } });
    assert_eq!(e.selection(), &[a, b]);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(350.0, 50.0, 50.0)) });
    let v = e.view();
    assert_eq!(v.highlights.iter().filter(|h| h.mark == Mark::Selected).count(), 2);
    // Clicking the void clears; Esc clears too.
    click(&mut e, DVec3::new(-800.0, -800.0, 0.0));
    assert!(e.selection().is_empty());
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert!(e.selection().is_empty());
}

/// Drags a selection rectangle from `a` to `b` (world points, seen from the top).
fn drag(e: &mut Editor, a: DVec3, b: DVec3, mods: Modifiers) {
    let (pa, pb) = (px(e, a), px(e, b));
    e.input(ToolInput::DragStart { px: pa, mods });
    e.input(ToolInput::Drag { px: pb });
    e.input(ToolInput::DragEnd { px: pb, mods });
}

#[test]
fn rectangle_selection_window_or_crossing_by_direction() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    let none = Modifiers::default();
    // Left to right over all of `a` and half of `b`: only `a` (enclosed).
    drag(&mut e, DVec3::new(-50.0, 150.0, 0.0), DVec3::new(350.0, -50.0, 0.0), none);
    assert_eq!(e.selection(), &[a]);
    // The same rectangle right to left: both (touched).
    drag(&mut e, DVec3::new(350.0, -50.0, 0.0), DVec3::new(-50.0, 150.0, 0.0), none);
    assert_eq!(e.selection().len(), 2);
    // Right to left inside `b`'s top face, touching no edge: still `b`.
    drag(&mut e, DVec3::new(380.0, 80.0, 0.0), DVec3::new(320.0, 20.0, 0.0), none);
    assert_eq!(e.selection(), &[b]);
    // Shift toggles: `a` added, then `b` removed.
    let shift = Modifiers { toggle: true, copy: false };
    drag(&mut e, DVec3::new(-50.0, 150.0, 0.0), DVec3::new(150.0, -50.0, 0.0), shift);
    assert_eq!(e.selection(), &[b, a]);
    drag(&mut e, DVec3::new(380.0, 80.0, 0.0), DVec3::new(320.0, 20.0, 0.0), shift);
    assert_eq!(e.selection(), &[a]);
    // While dragging: the rectangle and what it would take, shown; Esc drops it.
    e.input(ToolInput::DragStart { px: px(&e, DVec3::new(450.0, 150.0, 0.0)), mods: none });
    e.input(ToolInput::Drag { px: px(&e, DVec3::new(250.0, -50.0, 0.0)) });
    let v = e.view();
    assert!(v.marquee.as_ref().is_some_and(|m| m.crossing && m.hits == [b]));
    assert_eq!(v.highlights.iter().filter(|h| h.mark == Mark::Hover).count(), 1);
    e.input(ToolInput::Key { key: Key::Escape, mods: none });
    e.input(ToolInput::DragEnd { px: px(&e, DVec3::new(250.0, -50.0, 0.0)), mods: none });
    assert_eq!(e.selection(), &[a]);
    assert!(e.view().marquee.is_none());
    // A drag of a pixel or two is a click (here in the void: clears).
    let p = px(&e, DVec3::new(-800.0, -800.0, 0.0));
    e.input(ToolInput::DragStart { px: p, mods: none });
    e.input(ToolInput::DragEnd { px: p + DVec2::ONE, mods: none });
    assert!(e.selection().is_empty());
}

#[test]
fn move_with_axis_lock_and_typed_distance_then_copy() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(300.0, 80.0, 50.0)) });
    // Moved, not copied: drawn only where it goes, not twice.
    let shown = |e: &Editor| e.view().batches.iter().map(|(_, ps)| ps.len()).sum::<usize>();
    let selected = |e: &Editor| e.view().highlights.iter().any(|h| h.mark == Mark::Selected);
    assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Ghost));
    assert_eq!(shown(&e), 0);
    assert!(!selected(&e));
    // Copying, the original stays.
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    assert_eq!(shown(&e), 1);
    assert!(selected(&e));
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    e.input(ToolInput::Vcb("250".into()));
    let (lo, _) = bounds_of(&e, &EntityPath::root(a));
    assert!(close(lo, DVec3::new(250.0, 0.0, 0.0)), "{lo}");

    // Copy: a tap of the copy modifier toggles it, before or during the move (twice: off).
    let tap = |e: &mut Editor| e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    tap(&mut e);
    tap(&mut e);
    assert!(!e.tool_copying());
    let at = px(&e, DVec3::new(300.0, 50.0, 50.0));
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    tap(&mut e);
    assert!(e.tool_copying());
    e.input(ToolInput::Key { key: Key::Up, mods: Modifiers::default() });
    e.input(ToolInput::Vcb("100".into()));
    assert!(!e.tool_copying(), "back to moving after a copy");
    assert_eq!(e.document().root_assembly().occurrences.len(), 2);
    let copy = e.selection()[0];
    assert_ne!(copy, a);
    let (lo, _) = bounds_of(&e, &EntityPath::root(copy));
    // Up locks Z; in a top view the cursor's side is undefined, the distance is what counts.
    let dz = lo.z;
    assert!((dz.abs() - 100.0).abs() < 1e-6, "{lo}");

    // `3x`: an array of 3 at the same step, in place of the copy; retyped `/4`: 4 copies
    // spread over the step; both one undo step.
    let steps = e.history().journal().count();
    e.input(ToolInput::Vcb("3x".into()));
    assert_eq!(e.document().root_assembly().occurrences.len(), 4);
    assert_eq!(e.selection().len(), 3);
    let top =
        e.selection().iter().map(|id| bounds_of(&e, &EntityPath::root(*id)).0.z).fold(0.0f64, |m, z| if z.abs() > m.abs() { z } else { m });
    assert!((top - 3.0 * dz).abs() < 1e-6, "{top}");
    e.input(ToolInput::Vcb("/4".into()));
    assert_eq!(e.document().root_assembly().occurrences.len(), 5);
    let mut zs: Vec<f64> = e.selection().iter().map(|id| bounds_of(&e, &EntityPath::root(*id)).0.z.abs()).collect();
    zs.sort_by(f64::total_cmp);
    assert!(zs.iter().zip([25.0, 50.0, 75.0, 100.0]).all(|(z, w)| (z - w).abs() < 1e-6), "{zs:?}");
    assert_eq!(e.history().journal().count(), steps);
    e.undo();
    assert_eq!(e.document().root_assembly().occurrences.len(), 1);
}

#[test]
fn groups_enter_fade_exit() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let _b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    e.select(vec![a]);
    e.make_assembly();
    let g = e.selection()[0];

    // Double-click enters; the rest of the model fades.
    e.input(ToolInput::DoubleClick { px: px(&e, DVec3::new(50.0, 50.0, 50.0)), mods: Modifiers::default() });
    assert_eq!(e.context(), &[g]);
    let v = e.view();
    let faded: Vec<bool> = v.batches.iter().flat_map(|(_, p)| p.iter().map(|p| p.faded)).collect();
    assert_eq!(faded.iter().filter(|f| **f).count(), 1, "{faded:?}");
    // Inside, a click selects the solid itself.
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(e.selection(), &[a]);
    // A box drawn now goes into the group.
    draw_box(&mut e, DVec3::new(0.0, 200.0, 0.0), DVec3::new(50.0, 250.0, 0.0), "5");
    let gdef = e.context_definition();
    assert_eq!(e.document().assembly(gdef).unwrap().occurrences.len(), 2);
    // A click in the void inside its box keeps it, the selection cleared.
    click(&mut e, DVec3::new(80.0, 225.0, 0.0));
    assert_eq!(e.context(), &[g]);
    assert!(e.selection().is_empty());
    // A click off its box leaves, the group selected; a double-click there does nothing more.
    click(&mut e, DVec3::new(-800.0, -800.0, 0.0));
    assert!(e.context().is_empty());
    assert_eq!(e.selection(), &[g]);
    e.input(ToolInput::DoubleClick { px: px(&e, DVec3::new(-800.0, -800.0, 0.0)), mods: Modifiers::default() });
    assert!(e.context().is_empty());
    // Nested, a click off the inner box leaves one level only, even inside the outer one.
    e.select(vec![g, _b]);
    e.make_assembly();
    let outer = e.selection()[0];
    e.enter(outer);
    e.enter(g);
    click(&mut e, DVec3::new(200.0, 50.0, 0.0));
    assert_eq!((e.context(), e.selection()), (&[outer][..], &[g][..]));
    click(&mut e, DVec3::new(200.0, 50.0, 0.0));
    assert_eq!((e.context(), e.selection()), (&[outer][..], &[][..]));
    click(&mut e, DVec3::new(-800.0, -800.0, 0.0));
    assert_eq!((e.context(), e.selection()), (&[][..], &[outer][..]));
    e.undo();
    // Undo back past the group: the context and selection stay valid.
    e.enter(g);
    e.undo();
    e.undo();
    assert!(e.context().is_empty());
}

#[test]
fn a_selection_shows_its_content_box() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 200.0, 0.0), "80");
    // A part alone: its box too.
    e.select(vec![a]);
    let segs = e.view().selection_boxes;
    let (lo, hi) = segs.iter().flat_map(|b| b.corners()).fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(100.0, 100.0, 50.0)), "{lo} {hi}");
    e.select(vec![a, b]);
    e.make_assembly();
    let segs = e.view().selection_boxes;
    assert!(!segs.is_empty());
    // Dashed, the box spans the content's bounds exactly.
    let (lo, hi) = segs.iter().flat_map(|b| b.corners()).fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(400.0, 200.0, 80.0)), "{lo} {hi}");
    // The stretch tool draws its own box: the selection's is not drawn.
    e.set_tool(ToolKind::Stretch);
    assert!(e.view().selection_boxes.is_empty());
    e.set_tool(ToolKind::Select);
    assert!(!e.view().selection_boxes.is_empty());
    e.select(vec![]);
    assert!(e.view().selection_boxes.is_empty());
    assert!(e.view().context_box.is_empty());
    // Inside the assembly: its box stays, a little larger than the content.
    let g = {
        e.select_all();
        e.selection()[0]
    };
    e.enter(g);
    let v = e.view();
    assert!(v.selection_boxes.is_empty());
    let (lo, hi) = v.context_box.iter().flat_map(|b| b.corners()).fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
    let d = lo - DVec3::ZERO;
    assert!(d.x < 0.0 && (d.x - d.y).abs() < 1e-3 && (d.x - d.z).abs() < 1e-3, "{lo}");
    assert!(close(hi - DVec3::new(400.0, 200.0, 80.0), -d), "{lo} {hi}");
}

#[test]
fn component_instances_and_scene_caches() {
    let doc = Document::new();
    let root = doc.root();
    let (d, out) = doc
        .apply(&Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) },
            placement: glam::DAffine3::IDENTITY,
        })
        .unwrap();
    let (d, out) = d.apply(&Operation::MakeAssembly { context: root, entities: out.created, name: None }).unwrap();
    let i1 = out.created[0];
    let asm = d.resolve(&EntityPath::root(i1)).unwrap().component;
    // Meshes are per part: the assembly's one part.
    let def = d.assembly(asm).unwrap().occurrences[0].component;
    let t = glam::DAffine3::from_translation(DVec3::new(500.0, 0.0, 0.0));
    let (d, out) = d.apply(&Operation::PlaceOccurrence { context: root, component: asm, transform: t }).unwrap();
    let i2 = out.created[0];
    let mut e = top_editor(d);

    // One mesh, two placements.
    assert_eq!(e.scene().definition_meshes().count(), 1);
    let (_, places) = e.scene().placements().next().unwrap();
    assert_eq!(places.len(), 2);
    // Picking tells the instances apart.
    let hit = e.scene().pick(&e.camera.ray(px(&e, DVec3::new(550.0, 50.0, 100.0)), e.viewport_size()), |_| true).unwrap();
    assert_eq!(hit.path.instances, vec![i2]);
    assert!((hit.point.z - 100.0).abs() < 1e-6);

    // Moving an instance changes no mesh: same revision, same Arc.
    let mesh = e.scene().definition_mesh(def).unwrap().parts[0].mesh.clone();
    let rev = e.scene().definition_mesh(def).unwrap().revision;
    e.apply(Operation::Transform { context: root, entities: vec![i1], transform: t }).unwrap();
    assert_eq!(e.scene().definition_mesh(def).unwrap().revision, rev);
    assert!(std::sync::Arc::ptr_eq(&mesh, &e.scene().definition_mesh(def).unwrap().parts[0].mesh));

    // Make unique: the copy gets an assembly of its own, its part still shared: one mesh.
    e.select(vec![i2]);
    e.make_unique();
    assert_ne!(e.document().resolve(&EntityPath::root(i2)).unwrap().component, asm);
    assert_eq!(e.scene().definition_meshes().count(), 1);
    // A part made unique shares its shape's Arc: its mesh is not recomputed either.
    e.select(vec![]);
    e.enter(i2);
    e.select_all();
    e.make_unique();
    e.exit();
    assert_eq!(e.scene().definition_meshes().count(), 2, "the part shared by both assemblies, now one each");
    for dm in e.scene().definition_meshes() {
        assert!(std::sync::Arc::ptr_eq(&mesh, &dm.parts[0].mesh));
    }
}

#[test]
fn boolean_tool_across_contexts() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 200.0, 0.0), "100");
    e.select(vec![a]);
    e.make_assembly();
    let g = e.selection()[0];
    // A cylinder at the root, standing through the box (z -20..130).
    let root = e.document().root();
    let cyl = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(100.0, 100.0, -20.0), radius: 30.0, height: 150.0 },
            placement: glam::DAffine3::IDENTITY,
        })
        .unwrap()
        .expect("an inline kernel answers at once")
        .created[0];

    e.set_tool(ToolKind::Boolean(Boolean::Difference));
    click(&mut e, DVec3::new(20.0, 20.0, 100.0)); // the box (target), away from the cylinder
    assert_eq!(e.view().highlights.iter().filter(|h| h.mark == Mark::Picked).count(), 1);
    click(&mut e, DVec3::new(100.0, 100.0, 130.0)); // the cylinder's top (tool)
    let doc = e.document();
    assert!(doc.root_assembly().occurrence(cyl).is_none(), "tool consumed: {}", e.status());
    let (_, part, _) = doc.resolve_part(&EntityPath::new(vec![g], a)).unwrap();
    assert_eq!(part.machinings.len(), 1);
    assert!(part.shape().topology_counts().0 >= 7);
    // The hole is pickable through: the ray at the centre goes down to the ground.
    let hit = e.scene().pick(&e.camera.ray(px(&e, DVec3::new(100.0, 100.0, 100.0)), e.viewport_size()), |_| true);
    assert!(hit.is_none(), "{hit:?}");
}

#[test]
fn rotate_tool_by_clicks_and_typed_angle() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 100.0, 0.0), "50");
    e.set_tool(ToolKind::Rotate);
    // Centre on the box's top face (axis: its normal, Z), reference along +X, then 90° typed.
    click(&mut e, DVec3::new(0.0, 0.0, 50.0));
    assert!(e.view().overlay.len() > 40, "protractor drawn");
    click(&mut e, DVec3::new(150.0, 0.0, 50.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(0.0, 150.0, 50.0)) });
    assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Ghost));
    assert_eq!(e.vcb().map(|v| v.1), Some("90°".into()), "snapped to 90°");
    e.input(ToolInput::Vcb("90".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(a));
    // 200 × 100 turned a quarter about the corner at the origin: x in -100..0, y in 0..200.
    assert!(close(lo, DVec3::new(-100.0, 0.0, 0.0)) && close(hi, DVec3::new(0.0, 200.0, 50.0)), "{lo} {hi}");

    // A copy, turned 45° about the context's Z locked by ↑, by clicks.
    e.set_tool(ToolKind::Rotate);
    e.input(ToolInput::Key { key: Key::Up, mods: Modifiers::default() });
    let at = px(&e, DVec3::new(-50.0, 100.0, 50.0));
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    click(&mut e, DVec3::new(-50.0 + 100.0, 100.0, 50.0));
    click(&mut e, DVec3::new(-50.0 + 100.0, 200.0, 50.0));
    assert_eq!(e.document().root_assembly().occurrences.len(), 2);

    // `/2` right after: two copies spread over the angle (the last one where the copy was).
    let copy = e.selection()[0];
    let before = bounds_of(&e, &EntityPath::root(copy));
    e.input(ToolInput::Vcb("/2".into()));
    assert_eq!(e.document().root_assembly().occurrences.len(), 3);
    let last = *e.selection().last().unwrap();
    let after = bounds_of(&e, &EntityPath::root(last));
    assert!(close(before.0, after.0) && close(before.1, after.1), "{before:?} {after:?}");
}

// ---- the kernel off the UI thread (ARCHITECTURE.md §7 ter) ----------------------------------

mod deferred {
    use std::cell::RefCell;
    use std::rc::Rc;

    use solidsketch_kernel::{Error, Job, JobId, JobResult, KernelService};

    /// A kernel that answers only when the test says so.
    #[derive(Clone, Default)]
    pub struct Deferred {
        jobs: Rc<RefCell<Vec<(JobId, Job)>>>,
        results: Rc<RefCell<Vec<(JobId, JobResult)>>>,
    }

    impl Deferred {
        pub fn queued(&self) -> usize {
            self.jobs.borrow().len()
        }

        pub fn run_all(&self) {
            let jobs = std::mem::take(&mut *self.jobs.borrow_mut());
            self.results.borrow_mut().extend(jobs.into_iter().map(|(id, job)| (id, solidsketch_kernel::run(&job))));
        }

        pub fn crash_all(&self) {
            let jobs = std::mem::take(&mut *self.jobs.borrow_mut());
            self.results.borrow_mut().extend(jobs.into_iter().map(|(id, _)| (id, Err(Error::Crashed("boom".into())))));
        }
    }

    impl KernelService for Deferred {
        fn submit(&mut self, id: JobId, job: Job) {
            self.jobs.borrow_mut().push((id, job));
        }

        fn poll(&mut self) -> Vec<(JobId, JobResult)> {
            std::mem::take(&mut *self.results.borrow_mut())
        }
    }
}

use crate::tools::Command;
use deferred::Deferred;

fn deferred_editor(doc: Document) -> (Editor, Deferred) {
    let k = Deferred::default();
    let mut e = Editor::new(doc, Box::new(k.clone()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.set_parallel(true);
    e.camera.set_standard_view(
        StandardView::Top,
        Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))),
        crate::camera::ViewArea::full(e.viewport_size()),
    );
    (e, k)
}

fn add_cube(e: &mut Editor, k: &Deferred) -> Id {
    let root = e.document().root();
    let op = Operation::AddPrimitive {
        context: root,
        primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) },
        placement: glam::DAffine3::IDENTITY,
    };
    assert_eq!(e.apply(op), Ok(None), "waits for the kernel");
    k.run_all();
    assert!(e.poll());
    e.document().root_assembly().occurrences[0].id
}

#[test]
fn pending_operation_blocks_editing_until_its_result() {
    let (mut e, k) = deferred_editor(Document::new());
    let root = e.document().root();
    let op = Operation::AddPrimitive {
        context: root,
        primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) },
        placement: glam::DAffine3::IDENTITY,
    };
    assert_eq!(e.apply(op.clone()), Ok(None));
    assert!(e.busy());
    assert!(e.status().contains("calcul en cours"), "{}", e.status());
    assert_eq!(k.queued(), 1);
    // Tools, undo and other operations wait.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(300.0, 300.0, 0.0));
    click(&mut e, DVec3::new(400.0, 400.0, 0.0));
    assert!(e.apply(op).is_err());
    e.undo();
    assert_eq!(k.queued(), 1, "nothing else reached the kernel");
    assert!(e.document().root_assembly().occurrences.is_empty());

    k.run_all();
    assert!(e.poll());
    assert!(!e.busy());
    assert_eq!(e.document().root_assembly().occurrences.len(), 1);
    // The result came with its mesh: drawn and pickable, nothing more asked.
    assert_eq!(k.queued(), 0);
    let hit = e.scene().pick(&e.camera.ray(px(&e, DVec3::new(50.0, 50.0, 100.0)), e.viewport_size()), |_| true);
    assert!(hit.is_some());
}

#[test]
fn escape_abandons_and_failures_leave_the_document() {
    let (mut e, k) = deferred_editor(Document::new());
    let root = e.document().root();
    let op = Operation::AddPrimitive {
        context: root,
        primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) },
        placement: glam::DAffine3::IDENTITY,
    };
    assert_eq!(e.apply(op.clone()), Ok(None));
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert!(!e.busy());
    assert!(e.status().contains("abandonné"), "{}", e.status());
    k.run_all();
    e.poll();
    assert!(e.document().root_assembly().occurrences.is_empty(), "the late result is dropped");

    assert_eq!(e.apply(op), Ok(None));
    k.crash_all();
    e.poll();
    assert!(!e.busy());
    assert!(e.document().root_assembly().occurrences.is_empty());
    assert!(e.status().contains("noyau"), "{}", e.status());
    assert!(!e.history().can_undo());
}

#[test]
fn copies_and_arrays_need_no_kernel_and_a_refused_array_gives_the_copy_back() {
    let (mut e, k) = deferred_editor(Document::new());
    let s = add_cube(&mut e, &k);
    let root = e.document().root();
    let t = glam::DAffine3::from_translation(DVec3::new(200.0, 0.0, 0.0));
    let copy = Operation::Duplicate { context: root, entities: vec![s], transform: t };
    assert!(matches!(e.apply(copy.clone()), Ok(Some(_))), "done at once, more occurrences of the part");
    assert_eq!(e.document().root_assembly().occurrences.len(), 2);

    // `5x` typed after the copy: the copy is replaced by the array, at once, in one step.
    e.run(Command::Replace {
        previous: Box::new(copy.clone()),
        op: Box::new(Operation::Array { context: root, entities: vec![s], step: t, count: 5 }),
    });
    assert!(!e.busy());
    assert_eq!(e.document().root_assembly().occurrences.len(), 6);
    assert_eq!(k.queued(), 0, "nothing to mesh: one part");
    e.undo();
    assert_eq!(e.document().root_assembly().occurrences.len(), 1);
    e.redo();

    // A refused array leaves the copy as it was.
    e.undo();
    e.apply(copy.clone()).unwrap();
    e.run(Command::Replace {
        previous: Box::new(copy.clone()),
        op: Box::new(Operation::Array { context: root, entities: vec![s], step: t, count: 0 }),
    });
    assert_eq!(e.history().undo_op(), Some(&copy));
    assert_eq!(e.document().root_assembly().occurrences.len(), 2);
}

#[test]
fn opened_solids_are_outlined_until_meshed_and_undo_does_not_remesh() {
    let doc = Document::new();
    let root = doc.root();
    let (doc, _) = doc
        .apply(&Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) },
            placement: glam::DAffine3::IDENTITY,
        })
        .unwrap();
    let (mut e, k) = deferred_editor(doc);
    // Outlined: twelve edges, no faces; its mesh asked for once.
    let part = &e.scene().definition_meshes().next().unwrap().parts[0];
    assert!(part.mesh.indices.is_empty());
    assert_eq!(part.mesh.edges.len(), 12);
    assert_eq!(k.queued(), 1);
    e.zoom_extents(); // the outline has bounds: framing works
    e.input(ToolInput::Hover { px: DVec2::new(500.0, 500.0) });
    assert_eq!(k.queued(), 1, "asked once");
    k.run_all();
    assert!(e.poll());
    assert!(!e.scene().definition_meshes().next().unwrap().parts[0].mesh.indices.is_empty());

    // Move the part's occurrence (no kernel, the mesh stays), then undo / redo: no meshing.
    let s = e.document().root_assembly().occurrences[0].id;
    let t = glam::DAffine3::from_translation(DVec3::new(10.0, 0.0, 0.0));
    assert!(matches!(e.apply(Operation::Transform { context: root, entities: vec![s], transform: t }), Ok(Some(_))));
    assert_eq!(k.queued(), 0);
    e.undo();
    e.redo();
    e.undo();
    assert_eq!(k.queued(), 0, "meshes kept for the history");
    assert!(!e.scene().definition_meshes().next().unwrap().parts[0].mesh.indices.is_empty());
}

#[test]
fn operations_without_kernel_are_immediate() {
    let (mut e, k) = deferred_editor(Document::new());
    let s = add_cube(&mut e, &k);
    let root = e.document().root();
    e.select(vec![s]);
    e.make_assembly();
    assert!(!e.busy());
    let g = e.selection()[0];
    let t = glam::DAffine3::from_translation(DVec3::new(10.0, 0.0, 0.0));
    let out = e.apply(Operation::Transform { context: root, entities: vec![g], transform: t }).unwrap();
    assert!(out.is_some(), "moving a group needs no kernel");
    assert_eq!(k.queued(), 0);
}

#[test]
fn a_deliberate_kernel_crash_is_reported_and_the_next_job_runs() {
    let mut e = Editor::new(Document::new(), Box::new(solidsketch_kernel::Inline::default()));
    e.crash_kernel();
    let s = e.status();
    assert!(s.contains("Plantage provoqué") && s.contains("plantage voulu"), "{s}");
    assert!(s.contains("la tâche suivante a abouti"), "{s}");
    assert!(!e.busy());
}

// ---- the tool's option bar (ARCHITECTURE.md §6 bis) and the scope of a change (§2.5) --------

#[test]
fn boolean_scope_option_counts_marks_and_cuts_one_or_all() {
    let doc = Document::new();
    let root = doc.root();
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 100.0, 50.0) };
    let (d, out) = doc.apply(&Operation::AddPrimitive { context: root, primitive: board, placement: glam::DAffine3::IDENTITY }).unwrap();
    let a = out.created[0];
    let t = glam::DAffine3::from_translation(DVec3::new(0.0, 300.0, 0.0));
    let (d, out) = d.apply(&Operation::Array { context: root, entities: vec![a], step: t, count: 2 }).unwrap();
    let (b, c) = (out.created[0], out.created[1]);
    let mut e = top_editor(d);
    let drill = |e: &mut Editor, y: f64| {
        let root = e.document().root();
        let op = Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(100.0, y + 50.0, -10.0), radius: 10.0, height: 80.0 },
            placement: glam::DAffine3::IDENTITY,
        };
        e.apply(op).unwrap().unwrap().created[0]
    };
    let hover = |e: &mut Editor, p: DVec3| {
        let at = px(e, p);
        e.input(ToolInput::Hover { px: at });
    };

    e.set_tool(ToolKind::Boolean(Boolean::Difference));
    let scope = |e: &Editor| e.tool_options().into_iter().find(|o| o.key == "scope").unwrap();
    // Nothing aimed at: no count; default "all".
    assert_eq!(scope(&e).value, OptionValue::Choice("all".into()));
    assert_eq!(scope(&e).choices[0].label, "Tous les exemplaires");
    // Aiming at one board: "×3", and the two others marked as changing too.
    hover(&mut e, DVec3::new(20.0, 20.0, 50.0));
    assert_eq!(scope(&e).choices[0].label, "Tous les exemplaires ×3");
    assert_eq!(e.view().highlights.iter().filter(|h| h.mark == Mark::Related).count(), 2);

    // Tab: only this one; nothing else marked.
    e.input(ToolInput::Key { key: Key::Tab, mods: Modifiers::default() });
    assert_eq!(scope(&e).value, OptionValue::Choice("this".into()));
    assert_eq!(e.view().highlights.iter().filter(|h| h.mark == Mark::Related).count(), 0);
    // Cut the middle board only.
    let d1 = drill(&mut e, 300.0);
    e.set_tool(ToolKind::Boolean(Boolean::Difference));
    assert_eq!(scope(&e).value, OptionValue::Choice("this".into()), "kept for the tool");
    click(&mut e, DVec3::new(20.0, 320.0, 50.0));
    click(&mut e, DVec3::new(100.0, 350.0, 50.0));
    let doc = e.document();
    assert!(doc.root_assembly().occurrence(d1).is_none(), "{}", e.status());
    let part = |doc: &Document, id: Id| doc.resolve_part(&EntityPath::root(id)).unwrap().0;
    assert_ne!(part(doc, b), part(doc, a));
    assert_eq!(part(doc, c), part(doc, a));
    assert_eq!(doc.resolve_part(&EntityPath::root(b)).unwrap().1.machinings.len(), 1);
    assert!(doc.resolve_part(&EntityPath::root(a)).unwrap().1.machinings.is_empty());

    // Back to all, by a click in the bar: the other two both get the cut.
    e.set_tool_option("scope", OptionValue::Choice("all".into()));
    let d2 = drill(&mut e, 0.0);
    e.set_tool(ToolKind::Boolean(Boolean::Difference));
    click(&mut e, DVec3::new(20.0, 20.0, 50.0));
    click(&mut e, DVec3::new(100.0, 50.0, 50.0));
    let doc = e.document();
    assert!(doc.root_assembly().occurrence(d2).is_none(), "{}", e.status());
    assert_eq!(part(doc, c), part(doc, a));
    assert_eq!(doc.resolve_part(&EntityPath::root(c)).unwrap().1.machinings.len(), 1);

    // Settings are per tool, and go to and from the preferences as they are.
    assert_eq!(e.tool_settings().keys().collect::<Vec<_>>(), vec!["difference"]);
    let saved = e.tool_settings().clone();
    let mut other = top_editor(Document::new());
    other.set_tool_settings(saved);
    other.set_tool(ToolKind::Boolean(Boolean::Union));
    assert_eq!(scope(&other).value, OptionValue::Choice("all".into()), "the union has its own");
}

#[test]
fn copy_is_a_toggle_in_the_bar() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Move);
    let copy = |e: &Editor| e.tool_options().into_iter().find(|o| o.key == "copy").unwrap();
    assert_eq!(copy(&e).value, OptionValue::Toggle(false));
    assert!(!copy(&e).saved, "the gesture's state, not a setting");
    e.set_tool_option("copy", OptionValue::Toggle(true));
    assert!(e.tool_copying());
    // The copy key and the bar are the same switch.
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    assert_eq!(copy(&e).value, OptionValue::Toggle(false));
    assert!(e.tool_settings().is_empty());
}

/// The occurrence `o` of the root, as drawn: its material.
fn drawn_material(e: &Editor, o: Id) -> Option<Id> {
    e.view()
        .batches
        .iter()
        .flat_map(|(def, ps)| {
            let paths = e.scene().placements().find(|(d, _)| d == def).map(|(_, p)| p.to_vec()).unwrap_or_default();
            paths.into_iter().zip(ps.clone())
        })
        .find(|(p, _)| p.path.last() == Some(&o))
        .and_then(|(_, v)| v.material)
}

#[test]
fn paint_tool_paints_occurrences_the_selection_and_faces() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    let c = draw_box(&mut e, DVec3::new(600.0, 0.0, 0.0), DVec3::new(700.0, 100.0, 0.0), "50");
    e.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap();
    let oak = e.document().materials().next().unwrap().0;
    e.set_paint_material(Some(oak));
    assert_eq!(e.paint_material(), Some(oak));

    // A click on an occurrence paints it.
    e.set_tool(ToolKind::Paint);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(drawn_material(&e, a), Some(oak));
    assert_eq!(drawn_material(&e, b), None);

    // A click on a selected one paints the whole selection.
    let selection = vec![b, c];
    e.set_tool(ToolKind::Select);
    click(&mut e, DVec3::new(350.0, 50.0, 50.0));
    e.input(ToolInput::Click { px: px(&e, DVec3::new(650.0, 50.0, 50.0)), mods: Modifiers { toggle: true, copy: false } });
    assert_eq!(e.selection(), selection.as_slice());
    e.set_tool(ToolKind::Paint);
    e.set_paint_material(None);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(drawn_material(&e, a), None, "no material: the paint is removed");
    e.set_paint_material(Some(oak));
    click(&mut e, DVec3::new(350.0, 50.0, 50.0));
    assert_eq!((drawn_material(&e, b), drawn_material(&e, c)), (Some(oak), Some(oak)));

    // Faces: the top face of the first box, on its part.
    e.set_tool_option("target", OptionValue::Choice("face".into()));
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    let (part, p, _) = e.document().resolve_part(&EntityPath::root(a)).unwrap();
    assert_eq!(p.faces.len(), 1, "one face painted");
    let dm = e.scene().definition_mesh(part).unwrap();
    assert_eq!(dm.faces.values().copied().collect::<Vec<_>>(), [oak]);
}

#[test]
fn section_tool_places_a_plane_and_picking_ignores_what_it_cuts() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "100");
    e.set_tool(ToolKind::Section);
    // On the top face (seen from above, its normal points up), 30 typed: 30 mm down.
    click(&mut e, DVec3::new(50.0, 50.0, 100.0));
    assert!(e.vcb().is_some(), "the depth is typed in the box");
    assert!(e.view().section.is_some(), "the cut shows while placing");
    e.input(ToolInput::Vcb("30".into()));
    let s = e.document().settings().display.active_section().expect("a section");
    assert!(close(s.normal, DVec3::Z) && (s.origin.z - 70.0).abs() < 1e-6, "{s:?}");

    // Looking down, the ray first meets the box at its cut, not at its old top.
    let hit = e.scene().pick(&e.camera.ray(px(&e, DVec3::new(50.0, 50.0, 0.0)), e.viewport_size()), |_| true).unwrap();
    assert_eq!(hit.path, EntityPath::root(a));
    assert!(hit.point.z < 70.0 + 1e-6, "hit at {}", hit.point);

    // Undo removes it.
    e.undo();
    assert!(e.document().settings().display.active_section().is_none());
}

#[test]
fn a_part_opens_with_its_axes_the_rest_faded() {
    use glam::{DAffine3, DQuat};
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 400.0, 0.0), "50");
    let c = draw_box(&mut e, DVec3::new(-700.0, 0.0, 0.0), DVec3::new(-600.0, 100.0, 0.0), "50");
    // A copy of `a`'s part, turned 30° and moved: its axes are not the model's.
    let turn = DAffine3::from_translation(DVec3::new(600.0, -500.0, 0.0)) * DAffine3::from_quat(DQuat::from_rotation_z(30f64.to_radians()));
    let b = e.apply(Operation::Duplicate { context: root, entities: vec![a], transform: turn }).unwrap().unwrap().created[0];
    let tb = e.document().resolve(&EntityPath::root(b)).unwrap().transform;
    let on_b = |p: DVec3| tb.transform_point3(p);

    // Double-click opens the part: its axes, the rest faded, its other occurrence related.
    e.input(ToolInput::DoubleClick { px: px(&e, on_b(DVec3::new(200.0, 200.0, 50.0))), mods: Modifiers::default() });
    assert_eq!(e.open_part(), Some(b));
    assert!(e.context().is_empty() && e.selection().is_empty());
    assert!(e.axes_transform().abs_diff_eq(tb, 1e-9));
    let v = e.view();
    let faded: Vec<bool> = v.batches.iter().flat_map(|(_, p)| p.iter().map(|p| p.faded)).collect();
    assert_eq!((faded.len(), faded.iter().filter(|f| **f).count()), (3, 2), "{faded:?}");
    let related: Vec<_> = v.highlights.iter().filter(|h| h.mark == Mark::Related).collect();
    assert_eq!(related.len(), 1);
    // A dashed box around it, a few pixels off its faces (its top is at z = 50).
    assert!(!v.context_box.is_empty());
    let top = v.context_box.iter().flat_map(|b| b.corners().map(|c| c.z)).fold(f64::MIN, f64::max);
    assert!(top > 50.0 && top < 80.0, "{top}");
    assert!(related[0].transform.abs_diff_eq(e.document().resolve(&EntityPath::root(a)).unwrap().transform, 1e-9));
    // A click on something else, off its box, closes it, the occurrence left selected.
    click(&mut e, DVec3::new(-650.0, 50.0, 50.0));
    assert_eq!((e.open_part(), e.selection()), (None, &[b][..]));
    let _ = c;
    // A click on it keeps it open, so does a double-click.
    e.enter(b);
    click(&mut e, on_b(DVec3::new(200.0, 200.0, 50.0)));
    assert_eq!(e.open_part(), Some(b));
    e.input(ToolInput::DoubleClick { px: px(&e, on_b(DVec3::new(200.0, 200.0, 50.0))), mods: Modifiers::default() });
    assert_eq!(e.open_part(), Some(b));

    // A box drawn on it lies in its axes and is cut from it: a pocket, the part still open.
    e.set_tool(ToolKind::Box);
    click(&mut e, on_b(DVec3::new(100.0, 100.0, 50.0)));
    click(&mut e, on_b(DVec3::new(250.0, 200.0, 50.0)));
    e.input(ToolInput::Vcb("-20".into()));
    e.set_tool(ToolKind::Select);
    assert_eq!(e.open_part(), Some(b), "{}", e.status());
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(b)).unwrap();
    assert_eq!(part.machinings.len(), 1);
    let m = &part.machinings[0];
    assert!(close(m.placement.translation, DVec3::new(100.0, 100.0, 30.0)), "{}", m.placement.translation);
    let (lo, hi) = m.tool.vertex_bounds().unwrap();
    assert!(close(hi - lo, DVec3::new(150.0, 100.0, 20.0)), "{lo} {hi}");
    // Every occurrence shows it (scope "all"): `a` is the same part.
    assert_eq!(e.document().resolve_part(&EntityPath::root(a)).unwrap().1.machinings.len(), 1);

    // Esc closes it, the occurrence left selected; so does a click in the void.
    e.enter(b);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!((e.open_part(), e.selection()), (None, &[b][..]));
    e.enter(b);
    click(&mut e, DVec3::new(-800.0, -800.0, 0.0));
    assert_eq!(e.open_part(), None);
    assert!(e.axes_transform().abs_diff_eq(DAffine3::IDENTITY, 1e-12));
    assert!(e.view().context_box.is_empty());

    // Undo: the pocket goes, the part stays open; then the copy goes, and the part closes.
    e.enter(b);
    e.undo();
    assert_eq!(e.open_part(), Some(b));
    assert!(e.document().resolve_part(&EntityPath::root(b)).unwrap().1.machinings.is_empty());
    e.undo();
    assert_eq!(e.open_part(), None);
}

#[test]
fn the_machinings_switch_shows_the_blank_as_a_ghost_and_the_tools() {
    use glam::DAffine3;
    use solidsketch_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 100.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let target = EntityPath::root(board);
    e.apply(Operation::Boolean { target: target.clone(), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::All })
        .unwrap();
    let (part, p, _) = e.document().resolve_part(&target).unwrap();
    let hole = p.machinings[0].id;
    e.apply(Operation::DuplicateMachinings {
        target: target.clone(),
        machinings: vec![hole],
        transform: DAffine3::from_translation(DVec3::X * 100.0),
        scope: Scope::All,
    })
    .unwrap();
    let looks = |e: &Editor| e.view().batches.iter().flat_map(|(k, ps)| ps.iter().map(move |p| (*k, p.look))).collect::<Vec<_>>();

    // Off, or no part open: shapes only.
    e.set_machinings_switch(true);
    assert!(!e.showing_machinings());
    assert!(looks(&e).iter().all(|(_, l)| *l == Look::Shape));
    e.enter(board);
    assert!(e.showing_machinings());
    let l = looks(&e);
    // The part's shape gives way to its blank, a ghost, and its two tools (one mesh).
    assert!(!l.iter().any(|(k, look)| *k == part && *look == Look::Shape), "{l:?}");
    let blank: Vec<_> = l.iter().filter(|(_, look)| *look == Look::Blank).collect();
    assert_eq!(blank.len(), 1);
    let tools: Vec<_> = l.iter().filter(|(_, look)| *look == Look::Tool).collect();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].0, tools[1].0, "a copied machining shares its tool's mesh");
    // Meshed like shapes (the kernel is inline here).
    for (k, _) in blank.iter().chain(&tools) {
        assert!(e.scene().definition_mesh(*k).is_some_and(|m| m.parts[0].mesh.indices.len() > 36));
    }
    // A tool placed in the world through the occurrence.
    let r = e.scene().recipe().unwrap();
    assert!(close(r.tools[1].placement.translation, DVec3::new(150.0, 100.0, -5.0)));
    // Hovered in the entity info's list: drawn hovered in the view.
    let copy = r.tools[1].machining;
    e.set_listed_hover(Some(copy));
    assert_eq!(e.hovered_machining(), Some(copy));
    let hovered: Vec<_> = e.view().highlights.into_iter().filter(|h| h.mark == Mark::Hover).collect();
    assert_eq!(hovered.len(), 1);
    assert!(close(hovered[0].transform.translation, DVec3::new(150.0, 100.0, -5.0)));
    e.set_listed_hover(None);

    // Switched off, or closed: the shape again.
    e.set_machinings_switch(false);
    assert!(looks(&e).iter().all(|(_, l)| *l == Look::Shape));
    e.set_machinings_switch(true);
    e.exit();
    assert!(looks(&e).iter().all(|(_, l)| *l == Look::Shape));
    assert!(e.scene().recipe().is_none());
}

#[test]
fn machinings_are_picked_moved_copied_arrayed_and_erased_in_the_open_part() {
    use glam::DAffine3;
    use solidsketch_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    let other = e
        .apply(Operation::Duplicate { context: root, entities: vec![board], transform: DAffine3::from_translation(DVec3::Y * 500.0) })
        .unwrap()
        .unwrap()
        .created[0];
    // A drill standing out of the board's top (z 18 → 25).
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 100.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let target = EntityPath::root(board);
    e.apply(Operation::Boolean { target: target.clone(), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::All })
        .unwrap();
    let machinings = |e: &Editor, o: Id| e.document().resolve_part(&EntityPath::root(o)).unwrap().1.machinings.clone();
    let hole = machinings(&e, board)[0].id;
    e.set_machinings_switch(true);
    e.enter(board);

    // A click on the tool selects its machining; the void clears.
    click(&mut e, DVec3::new(50.0, 100.0, 25.0));
    assert_eq!(e.selection(), &[hole]);
    assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Selected));
    // Between the board and its grey box (8 px off its faces), the part stays open.
    let corner = px(&e, DVec3::new(400.0, 200.0, 18.0)) + DVec2::new(4.0, -4.0);
    e.input(ToolInput::Click { px: corner, mods: Modifiers::default() });
    assert!(e.selection().is_empty());
    assert_eq!(e.open_part(), Some(board), "a click inside its box keeps the part open");

    // Move, nothing selected: the tool under the first click; 100 along red. While moving, the
    // tool is a ghost where it goes, not drawn where it was.
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(50.0, 100.0, 25.0));
    assert_eq!(e.selection(), &[hole]);
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(250.0, 100.0, 25.0)) });
    let v = e.view();
    assert!(v.highlights.iter().any(|h| h.mark == Mark::Ghost));
    assert!(!v.batches.iter().flat_map(|(_, p)| p).any(|p| p.look == Look::Tool));
    e.input(ToolInput::Vcb("100".into()));
    assert!(close(machinings(&e, board)[0].placement.translation, DVec3::new(150.0, 100.0, -5.0)));
    assert_eq!(e.selection(), &[hole]);

    // A copy 50 further, then `3x`: an array of three, every occurrence shows them.
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    click(&mut e, DVec3::new(150.0, 100.0, 25.0));
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(300.0, 100.0, 25.0)) });
    e.input(ToolInput::Vcb("50".into()));
    assert_eq!(machinings(&e, board).len(), 2, "{}", e.status());
    e.input(ToolInput::Vcb("3x".into()));
    let ms = machinings(&e, board);
    assert_eq!(ms.len(), 4, "{}", e.status());
    let mut xs: Vec<f64> = ms.iter().map(|m| m.placement.translation.x).collect();
    xs.sort_by(f64::total_cmp);
    assert!(xs.iter().zip([150.0, 200.0, 250.0, 300.0]).all(|(x, w)| (x - w).abs() < 1e-6), "{xs:?}");
    assert_eq!(e.selection().len(), 3);
    assert_eq!(machinings(&e, other).len(), 4);

    // Erased under "only this one": this occurrence gets a part of its own, the other keeps
    // the holes.
    e.set_tool(ToolKind::Select);
    e.select_all();
    assert_eq!(e.selection().len(), 4);
    e.set_part_scope(Scope::This);
    e.input(ToolInput::Key { key: Key::Delete, mods: Modifiers::default() });
    assert!(machinings(&e, board).is_empty(), "{}", e.status());
    assert_eq!(machinings(&e, other).len(), 4);
    assert_ne!(e.document().resolve_part(&target).unwrap().0, e.document().resolve_part(&EntityPath::root(other)).unwrap().0);
    assert_eq!(e.open_part(), Some(board));
    // Reopened, the scope is "all" again.
    e.exit();
    e.enter(board);
    assert_eq!(e.part_scope(), Scope::All);
}

#[test]
fn inferences_see_the_blank_and_the_tools_while_machinings_show() {
    use glam::DAffine3;
    use solidsketch_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    // A drill standing out of the board's top: its top circle at z 25, in no shape.
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 100.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
        })
        .unwrap()
        .unwrap()
        .created[0];
    e.apply(Operation::Boolean {
        target: EntityPath::root(board),
        tool: EntityPath::root(drill),
        op: Boolean::Difference,
        scope: Scope::All,
    })
    .unwrap();
    let hover = |e: &mut Editor, p: DVec3| {
        let at = px(e, p);
        e.input(ToolInput::Hover { px: at });
        e.inference()
    };
    let centre = DVec3::new(50.0, 100.0, 25.0);
    e.set_tool(ToolKind::Move);

    // The part open, its machinings hidden: the shape only (the hole's rim, at z 18).
    e.enter(board);
    let (p, _) = hover(&mut e, centre).unwrap();
    assert!(p.z < 18.0 + 1e-6, "{p}");

    // Shown: the tool's top circle, its centre inferred.
    e.set_machinings_switch(true);
    e.set_tool(ToolKind::Move);
    let (p, kind) = hover(&mut e, centre).unwrap();
    assert_eq!(kind, InferKind::Center);
    assert!(close(p, centre), "{p}");

    // Moving it from there: it is left out, the blank's top is what is under the cursor.
    let at = px(&e, centre);
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    assert_eq!(e.selection().len(), 1);
    let (p, _) = hover(&mut e, DVec3::new(52.0, 100.0, 25.0)).unwrap();
    assert!(p.z < 18.0 + 1e-6, "{p}");
}

#[test]
fn exact_sizes_of_tools_and_blanks() {
    use glam::DAffine3;
    use solidsketch_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let rod = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::ZERO, radius: 20.0, height: 300.0 },
            placement: DAffine3::IDENTITY,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(0.0, 0.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let target = EntityPath::root(rod);
    e.apply(Operation::Boolean { target: target.clone(), tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::All })
        .unwrap();
    let (_, p, _) = e.document().resolve_part(&target).unwrap();
    let (blank, tool) = (p.blank.clone(), p.machinings[0].tool.clone());
    let near = |b: Option<(DVec3, DVec3)>, lo: DVec3, hi: DVec3| b.is_some_and(|(a, b)| close(a, lo) && close(b, hi));
    // A machined part's blank: its exact box, not its vertices' (a cylinder has one per circle).
    assert!(
        near(e.scene().bounds_of(&blank), DVec3::new(-20.0, -20.0, 0.0), DVec3::new(20.0, 20.0, 300.0)),
        "{:?}",
        e.scene().bounds_of(&blank)
    );
    // The open part's tools, their machinings hidden.
    e.enter(rod);
    assert!(!e.showing_machinings());
    assert!(near(e.scene().bounds_of(&tool), DVec3::new(-4.0, -4.0, 0.0), DVec3::new(4.0, 4.0, 30.0)), "{:?}", e.scene().bounds_of(&tool));
}

#[test]
fn the_tape_measures_and_draws_guides() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "100");
    let disc = Primitive::Cylinder { origin: DVec3::new(600.0, 600.0, 0.0), radius: 100.0, height: 50.0 };
    e.apply(Operation::AddPrimitive { context: root, primitive: disc, placement: glam::DAffine3::IDENTITY }).unwrap();
    let hover = |e: &mut Editor, p: DVec3| e.input(ToolInput::Hover { px: px(e, p) });
    let guides = |e: &Editor| e.document().component(root).unwrap().guides.iter().map(|g| g.shape).collect::<Vec<_>>();
    e.set_tool(ToolKind::Measure);
    assert!(e.vcb().is_none());

    // From a corner to the opposite one: the length, and nothing added.
    click(&mut e, DVec3::new(0.0, 0.0, 100.0));
    hover(&mut e, DVec3::new(300.0, 200.0, 100.0));
    assert_eq!(e.vcb(), Some(("Longueur".into(), "360,56 mm".into())));
    assert_eq!(e.labels().len(), 1, "the length written on the segment");
    click(&mut e, DVec3::new(300.0, 200.0, 100.0));
    assert!(e.vcb().is_none(), "the second click ends the measure");
    assert!(guides(&e).is_empty());

    // From an edge: a parallel guide through the point clicked next, the offset shown.
    click(&mut e, DVec3::new(300.0, 50.0, 100.0));
    hover(&mut e, DVec3::new(420.0, 120.0, 100.0));
    assert_eq!(e.vcb(), Some(("Décalage".into(), "120 mm".into())));
    // The edge referred to, end to end, bolder.
    let refs: Vec<_> = e.preview().lines.into_iter().filter(|l| l.color == crate::tools::MAGENTA).collect();
    let (a, b) = (DVec3::new(300.0, 0.0, 100.0), DVec3::new(300.0, 200.0, 100.0));
    let [r] = refs[..] else { panic!("{refs:?}") };
    assert!(r.bold && ((close(r.a, a) && close(r.b, b)) || (close(r.a, b) && close(r.b, a))), "{r:?}");
    // The offset runs from the point clicked, square to the edge, to the guide.
    let l = &e.labels()[0];
    assert!(close(l.a, DVec3::new(300.0, 50.0, 100.0)) && close(l.b, DVec3::new(420.0, 50.0, 100.0)), "{} {}", l.a, l.b);
    // Along the red axis, whatever the cursor snaps to: red.
    assert_eq!(l.color, crate::tools::RED);
    click(&mut e, DVec3::new(420.0, 120.0, 100.0));
    let [GuideShape::Line { origin, dir }] = guides(&e)[..] else { panic!("{:?}", guides(&e)) };
    assert!((origin.x - 420.0).abs() < 1e-3 && (origin.z - 100.0).abs() < 1e-3 && dir.cross(DVec3::Y).length() < 1e-9, "{origin} {dir}");
    assert!(!e.view().guides.is_empty(), "drawn");

    // Typed: the offset towards the cursor's side.
    click(&mut e, DVec3::new(300.0, 50.0, 100.0));
    hover(&mut e, DVec3::new(400.0, 150.0, 100.0));
    e.input(ToolInput::Vcb("50".into()));
    let GuideShape::Line { origin, .. } = guides(&e)[1] else { panic!() };
    assert!(close(origin, DVec3::new(350.0, 50.0, 100.0)), "{origin}");

    // From an arc: a concentric circle, in its plane.
    let a = 45f64.to_radians();
    let radial = DVec3::new(a.cos(), a.sin(), 0.0);
    let c = DVec3::new(600.0, 600.0, 50.0);
    click(&mut e, c + radial * 100.0);
    // The offset runs along the radius through the cursor.
    hover(&mut e, c + DVec3::new(0.0, 250.0, 0.0));
    let l = &e.labels()[0];
    assert_eq!(l.color, crate::tools::GREEN, "along green");
    assert!(close(l.a, c + DVec3::new(0.0, 100.0, 0.0)) && close(l.b, c + DVec3::new(0.0, 250.0, 0.0)), "{} {}", l.a, l.b);
    hover(&mut e, c + radial * 160.0);
    assert_eq!(e.vcb(), Some(("Décalage".into(), "60 mm".into())));
    hover(&mut e, c + radial * 300.0);
    assert_eq!(e.labels()[0].color, crate::tools::INK, "askew: ink");
    e.input(ToolInput::Vcb("20".into()));
    let GuideShape::Circle { center, normal, radius } = guides(&e)[2] else { panic!("{:?}", guides(&e)) };
    assert!(close(center, c) && normal.cross(DVec3::Z).length() < 1e-9 && (radius - 120.0).abs() < 1e-3, "{center} {normal} {radius}");
    // Inwards, past the centre: refused.
    click(&mut e, c + radial * 100.0);
    hover(&mut e, c + radial * 40.0);
    e.input(ToolInput::Vcb("150".into()));
    assert_eq!(e.status(), "Le cercle n'aurait plus de rayon : pas de guide");
    assert_eq!(guides(&e).len(), 3);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });

    // Undone like any change.
    e.undo();
    assert_eq!(guides(&e).len(), 2);

    // The copy modifier turns guides off: an edge is then measured from.
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    click(&mut e, DVec3::new(300.0, 50.0, 100.0));
    assert_eq!(e.vcb().map(|v| v.0), Some("Longueur".into()));
}

#[test]
fn the_tape_draws_the_kind_of_guide_chosen() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "100");
    let disc = Primitive::Cylinder { origin: DVec3::new(600.0, 600.0, 0.0), radius: 100.0, height: 50.0 };
    e.apply(Operation::AddPrimitive { context: root, primitive: disc, placement: glam::DAffine3::IDENTITY }).unwrap();
    let guides = |e: &Editor| e.document().component(root).unwrap().guides.iter().map(|g| g.shape).collect::<Vec<_>>();
    let tab = |e: &mut Editor| e.input(ToolInput::Key { key: Key::Tab, mods: Modifiers::default() });
    e.set_tool(ToolKind::Measure);

    // Auto, a zero offset: the guide along the edge, through the point clicked.
    click(&mut e, DVec3::new(300.0, 50.0, 100.0));
    e.input(ToolInput::Vcb("0".into()));
    let [GuideShape::Line { origin, dir }] = guides(&e)[..] else { panic!("{} {:?}", e.status(), guides(&e)) };
    assert!(close(origin, DVec3::new(300.0, 50.0, 100.0)) && dir.cross(DVec3::Y).length() < 1e-9, "{origin}");

    // Line: tangent to the arc at the point clicked, offset along its radius.
    tab(&mut e);
    let a = 45f64.to_radians();
    let radial = DVec3::new(a.cos(), a.sin(), 0.0);
    let c = DVec3::new(600.0, 600.0, 50.0);
    // The tangent itself: a zero offset.
    click(&mut e, c + radial * 100.0);
    assert_eq!(e.vcb().map(|v| v.0), Some("Décalage".into()), "{}", e.status());
    e.input(ToolInput::Vcb("0".into()));
    let GuideShape::Line { origin, dir } = guides(&e)[1] else { panic!("{:?}", guides(&e)) };
    assert!(close(origin, c + radial * 100.0) && dir.dot(radial).abs() < 1e-9 && dir.z.abs() < 1e-9, "{origin} {dir}");
    // Elsewhere on the arc, offset along its radius.
    let radial = DVec3::new(-a.cos(), a.sin(), 0.0);
    click(&mut e, c + radial * 100.0);
    hover(&mut e, c + radial * 150.0);
    e.input(ToolInput::Vcb("30".into()));
    let GuideShape::Line { origin, dir } = guides(&e)[2] else { panic!("{:?}", guides(&e)) };
    assert!(close(origin, c + radial * 130.0) && dir.dot(radial).abs() < 1e-6, "{origin} {dir}");
    // Line, from a corner: through it and the point clicked next.
    click(&mut e, DVec3::new(0.0, 0.0, 100.0));
    assert_eq!(e.vcb().map(|v| v.0), Some("Longueur".into()));
    click(&mut e, DVec3::new(300.0, 200.0, 100.0));
    let GuideShape::Line { origin, dir } = guides(&e)[3] else { panic!("{:?}", guides(&e)) };
    assert!(close(origin, DVec3::new(0.0, 0.0, 100.0)) && dir.cross(DVec3::new(3.0, 2.0, 0.0).normalize()).length() < 1e-6, "{dir}");

    // Circle: concentric to the arc, a zero offset: the arc's whole circle.
    tab(&mut e);
    click(&mut e, c - DVec3::Y * 100.0);
    e.input(ToolInput::Vcb("0".into()));
    let GuideShape::Circle { center, radius, .. } = guides(&e)[4] else { panic!("{} {:?}", e.status(), guides(&e)) };
    assert!(close(center, c) && (radius - 100.0).abs() < 1e-6, "{radius}");
    // Centred on the corner clicked, level, the radius typed.
    click(&mut e, DVec3::new(300.0, 200.0, 100.0));
    assert_eq!(e.vcb().map(|v| v.0), Some("Rayon".into()));
    hover(&mut e, DVec3::new(380.0, 200.0, 100.0));
    assert_eq!(e.vcb().map(|v| v.1), Some("80 mm".into()));
    e.input(ToolInput::Vcb("25".into()));
    let GuideShape::Circle { center, normal, radius } = guides(&e)[5] else { panic!("{:?}", guides(&e)) };
    assert!(close(center, DVec3::new(300.0, 200.0, 100.0)) && normal.cross(DVec3::Z).length() < 1e-9 && (radius - 25.0).abs() < 1e-9);
    // A circle of no radius: refused.
    click(&mut e, DVec3::new(300.0, 200.0, 100.0));
    e.input(ToolInput::Vcb("0".into()));
    assert_eq!(e.status(), "Le cercle n'aurait plus de rayon : pas de guide");
    assert_eq!(guides(&e).len(), 6);

    // Changed in between: what the first click starts follows (circle, then auto: a measure).
    tab(&mut e);
    assert_eq!(e.vcb().map(|v| v.0), Some("Longueur".into()), "{}", e.status());
    tab(&mut e);
    assert_eq!(e.vcb().map(|v| v.0), Some("Longueur".into()), "line through two points");
}

#[test]
fn at_a_crossing_the_other_modifier_picks_the_guide() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let add = |e: &mut Editor, shape| {
        e.apply(Operation::AddGuide { component: root, shape }).unwrap();
    };
    add(&mut e, GuideShape::Line { origin: DVec3::new(0.0, -100.0, 0.0), dir: DVec3::X });
    let c = DVec3::new(0.0, 100.0, 0.0);
    add(&mut e, GuideShape::Circle { center: c, normal: DVec3::Z, radius: 250.0 });
    add(&mut e, GuideShape::Line { origin: DVec3::new(600.0, 0.0, 0.0), dir: DVec3::Y });
    let guides = |e: &Editor| e.document().component(root).unwrap().guides.iter().map(|g| g.shape).collect::<Vec<_>>();
    let alt = |e: &mut Editor| e.input(ToolInput::Key { key: Key::Alt, mods: Modifiers::default() });
    // They cross at (150, -100): 150² + 200² = 250².
    let x = DVec3::new(150.0, -100.0, 0.0);
    e.set_tool(ToolKind::Measure);
    e.input(ToolInput::Key { key: Key::Tab, mods: Modifiers::default() });

    // Coming from the circle: tangent to it.
    hover(&mut e, c + DVec3::new(-250.0, 0.0, 0.0));
    hover(&mut e, x);
    assert_eq!(e.inference().map(|i| i.1), Some(crate::InferKind::OnGuideAndLine(crate::LineKind::Guide)));
    click(&mut e, x);
    assert!(e.status().starts_with("Guide tangent"), "{}", e.status());
    assert!(e.status().contains("autre guide"), "{}", e.status());
    e.input(ToolInput::Vcb("0".into()));
    let GuideShape::Line { origin, dir } = guides(&e)[3] else { panic!("{:?}", guides(&e)) };
    assert!(close(origin, x) && dir.dot((x - c).normalize()).abs() < 1e-6, "{origin} {dir}");

    // Coming from the line: parallel to it; the other modifier turns it over to the tangent,
    // then to the line through two points, then back.
    hover(&mut e, DVec3::new(-300.0, -100.0, 0.0));
    hover(&mut e, x);
    click(&mut e, x);
    assert!(e.status().starts_with("Guide parallèle"), "{}", e.status());
    alt(&mut e);
    assert!(e.status().starts_with("Guide tangent"), "{}", e.status());
    alt(&mut e);
    assert!(e.status().starts_with("Guide par deux points"), "{}", e.status());
    alt(&mut e);
    assert!(e.status().starts_with("Guide parallèle"), "{}", e.status());
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });

    // Coming from neither: the line through two points.
    hover(&mut e, DVec3::new(600.0, 300.0, 0.0));
    hover(&mut e, x);
    click(&mut e, x);
    assert!(e.status().starts_with("Guide par deux points"), "{}", e.status());
}

#[test]
fn the_tape_draws_a_tangent_from_the_end_of_an_arc_and_marks_its_reference() {
    use solidsketch_model::{GuideShape, Scope};
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let add = |e: &mut Editor, primitive| {
        e.apply(Operation::AddPrimitive { context: root, primitive, placement: glam::DAffine3::IDENTITY }).unwrap().unwrap().created[0]
    };
    // A board with its corner (300, 200) cut round: an arc from (200, 200) to (300, 100) on top.
    let board = add(&mut e, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(300.0, 200.0, 50.0) });
    let drill = add(&mut e, Primitive::Cylinder { origin: DVec3::new(300.0, 200.0, -10.0), radius: 100.0, height: 70.0 });
    let target = EntityPath::root(board);
    e.apply(Operation::Boolean { target, tool: EntityPath::root(drill), op: Boolean::Difference, scope: Scope::All }).unwrap();
    e.select(vec![]);
    let guides = |e: &Editor| e.document().component(root).unwrap().guides.iter().map(|g| g.shape).collect::<Vec<_>>();
    // The arc referred to, as drawn: its own points, bolder, and no guide in its stead.
    let magenta = |e: &Editor| {
        let p = e.preview();
        let on_arc = |q: DVec3| ((q - DVec3::new(300.0, 200.0, 50.0)).length() - 100.0).abs() < 1e-6 && (q.z - 50.0).abs() < 1e-9;
        let arc = p.lines.iter().filter(|l| l.color == crate::tools::MAGENTA && l.bold).collect::<Vec<_>>();
        !arc.is_empty() && arc.iter().all(|l| on_arc(l.a) && on_arc(l.b)) && !p.guides.iter().any(|(_, c)| *c == crate::tools::MAGENTA)
    };
    e.set_tool(ToolKind::Measure);
    e.input(ToolInput::Key { key: Key::Tab, mods: Modifiers::default() });

    // Along the arc to its end: the end gives the arc's tangent there, marked before the click.
    let a = 225f64.to_radians();
    hover(&mut e, DVec3::new(300.0, 200.0, 50.0) + DVec3::new(a.cos(), a.sin(), 0.0) * 100.0);
    let end = DVec3::new(200.0, 200.0, 50.0);
    hover(&mut e, end);
    assert_eq!(e.inference().map(|i| i.1), Some(crate::InferKind::Endpoint));
    assert!(magenta(&e), "the arc it would refer to");
    click(&mut e, end);
    assert!(e.status().starts_with("Guide tangent"), "{}", e.status());
    assert!(magenta(&e), "the arc referred to");
    e.input(ToolInput::Vcb("0".into()));
    let [GuideShape::Line { origin, dir }] = guides(&e)[..] else { panic!("{:?}", guides(&e)) };
    assert!(close(origin, end) && dir.cross(DVec3::Y).length() < 1e-6, "{origin} {dir}");

    // In Auto, the end is measured from: nothing marked; the other modifier gives its guides.
    e.input(ToolInput::Key { key: Key::Tab, mods: Modifiers::default() });
    e.input(ToolInput::Key { key: Key::Tab, mods: Modifiers::default() });
    hover(&mut e, end);
    assert!(!magenta(&e));
    click(&mut e, end);
    assert_eq!(e.vcb().map(|v| v.0), Some("Longueur".into()), "{}", e.status());
    // Turned over to the arc's concentric guide (the upright edge's, seen end on, is not drawn).
    for _ in 0..4 {
        e.input(ToolInput::Key { key: Key::Alt, mods: Modifiers::default() });
        if e.status().starts_with("Guide concentrique") {
            break;
        }
    }
    assert!(e.status().starts_with("Guide concentrique"), "{}", e.status());
    assert!(magenta(&e));
}

#[test]
fn a_guide_drawn_in_an_open_part_is_the_part_s() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::new(100.0, 100.0, 0.0), DVec3::new(400.0, 300.0, 0.0), "50");
    e.input(ToolInput::DoubleClick { px: px(&e, DVec3::new(200.0, 200.0, 50.0)), mods: Modifiers::default() });
    assert_eq!(e.open_part(), Some(a));
    e.set_tool(ToolKind::Measure);
    click(&mut e, DVec3::new(400.0, 150.0, 50.0));
    hover(&mut e, DVec3::new(450.0, 150.0, 50.0));
    e.input(ToolInput::Vcb("25".into()));
    let doc = e.document();
    assert!(doc.component(doc.root()).unwrap().guides.is_empty());
    let part = doc.resolve(&EntityPath::root(a)).unwrap().component;
    // In the part's axes: its blank starts at its origin, the occurrence at (100, 100, 0).
    let [GuideShape::Line { origin, .. }] = doc.component(part).unwrap().guides.iter().map(|g| g.shape).collect::<Vec<_>>()[..] else {
        panic!()
    };
    assert!((origin.x - 325.0).abs() < 1e-3 && (origin.z - 50.0).abs() < 1e-3, "{origin}");
    let v = e.view();
    assert!(!v.guides.is_empty() && v.faded_guides.is_empty());
    // Closed, the part's guide is outside the context: faded.
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    e.set_tool(ToolKind::Select);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!(e.open_part(), None);
    let v = e.view();
    assert!(v.guides.is_empty() && !v.faded_guides.is_empty());
}

fn hover(e: &mut Editor, p: DVec3) {
    let at = px(e, p);
    e.input(ToolInput::Hover { px: at });
}

#[test]
fn the_tape_starts_a_guide_from_a_guide() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let line = GuideShape::Line { origin: DVec3::new(0.0, -300.0, 0.0), dir: DVec3::X };
    e.apply(Operation::AddGuide { component: root, shape: line }).unwrap();
    let circle = GuideShape::Circle { center: DVec3::new(400.0, 400.0, 0.0), normal: DVec3::Z, radius: 150.0 };
    e.apply(Operation::AddGuide { component: root, shape: circle }).unwrap();
    e.set_tool(ToolKind::Measure);
    // The guide referred to, dashed where it is: its very strokes, in magenta.
    let marks_guide = |e: &Editor| {
        let refs: Vec<_> = e.preview().guides.into_iter().filter(|(_, c)| *c == crate::tools::MAGENTA).map(|(s, _)| s).collect();
        let drawn = e.view().guides;
        !refs.is_empty() && refs.iter().all(|s| drawn.contains(s))
    };
    // From the guide line: a parallel one.
    click(&mut e, DVec3::new(-200.0, -300.0, 0.0));
    hover(&mut e, DVec3::new(-200.0, -200.0, 0.0));
    assert!(marks_guide(&e));
    e.input(ToolInput::Vcb("40".into()));
    let guides = |e: &Editor| e.document().component(root).unwrap().guides.iter().map(|g| g.shape).collect::<Vec<_>>();
    let GuideShape::Line { origin, dir } = guides(&e)[2] else { panic!("{:?}", guides(&e)) };
    assert!((origin.y + 260.0).abs() < 1e-3 && dir.cross(DVec3::X).length() < 1e-9, "{origin}");
    // From the guide circle: a concentric one.
    click(&mut e, DVec3::new(400.0, 550.0, 0.0));
    hover(&mut e, DVec3::new(400.0, 700.0, 0.0));
    assert!(marks_guide(&e));
    e.input(ToolInput::Vcb("50".into()));
    let GuideShape::Circle { radius, .. } = guides(&e)[3] else { panic!("{:?}", guides(&e)) };
    assert!((radius - 200.0).abs() < 1e-6, "{radius}");
}

#[test]
fn guides_are_selected_moved_copied_and_erased() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 200.0, 0.0), "50");
    e.select(vec![]);
    let line = GuideShape::Line { origin: DVec3::new(0.0, -300.0, 0.0), dir: DVec3::X };
    let g = e.apply(Operation::AddGuide { component: root, shape: line }).unwrap().unwrap().guide.unwrap();
    let circle = GuideShape::Circle { center: DVec3::new(-500.0, 500.0, 0.0), normal: DVec3::Z, radius: 100.0 };
    let c = e.apply(Operation::AddGuide { component: root, shape: circle }).unwrap().unwrap().guide.unwrap();
    let guides = |e: &Editor| e.document().component(root).unwrap().guides.clone();
    let none = Modifiers::default();
    e.set_tool(ToolKind::Select);

    // A click near a guide takes it; hovered then selected, drawn so.
    hover(&mut e, DVec3::new(100.0, -303.0, 0.0));
    assert!(!e.view().hovered_guides.is_empty());
    click(&mut e, DVec3::new(100.0, -303.0, 0.0));
    assert_eq!(e.selection(), &[g]);
    assert!(!e.view().selected_guides.is_empty());
    // Select all takes the guides too.
    e.select_all();
    assert_eq!(e.selection(), &[a, g, c]);

    // A window around the circle takes it alone; a crossing over the line, it and the box.
    drag(&mut e, DVec3::new(-650.0, 650.0, 0.0), DVec3::new(-350.0, 350.0, 0.0), none);
    assert_eq!(e.selection(), &[c]);
    drag(&mut e, DVec3::new(150.0, -350.0, 0.0), DVec3::new(50.0, 100.0, 0.0), none);
    assert_eq!(e.selection(), &[a, g]);

    // Moved with the box: 100 along green.
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(0.0, 0.0, 50.0));
    hover(&mut e, DVec3::new(0.0, 100.0, 50.0));
    e.input(ToolInput::Vcb("100".into()));
    let GuideShape::Line { origin, .. } = guides(&e)[0].shape else { panic!() };
    assert!((origin.y + 200.0).abs() < 1e-6, "{origin}");

    // Copied alone: the copy is selected after.
    e.select(vec![g]);
    e.set_copy(true);
    e.input(ToolInput::Key { key: Key::Copy, mods: none });
    e.set_copy(false);
    click(&mut e, DVec3::new(0.0, -200.0, 0.0));
    hover(&mut e, DVec3::new(0.0, -100.0, 0.0));
    e.input(ToolInput::Vcb("50".into()));
    assert_eq!(guides(&e).len(), 3, "{:?}", guides(&e));

    // Erased with Delete; then all of them with Edit › Delete guides.
    e.set_tool(ToolKind::Select);
    e.select(vec![g]);
    e.delete_selection();
    assert_eq!(guides(&e).len(), 2);
    e.delete_guides();
    assert!(guides(&e).is_empty());
    e.undo();
    assert_eq!(guides(&e).len(), 2);

    // Hidden: not drawn, picked nor inferred on; dropped from the selection.
    e.select_all();
    let mut s = e.document().settings().clone();
    s.display.guides = false;
    e.apply(Operation::SetSettings(s)).unwrap();
    assert_eq!(e.selection(), &[a]);
    let v = e.view();
    assert!(v.guides.is_empty() && v.selected_guides.is_empty());
    e.set_tool(ToolKind::Box);
    hover(&mut e, DVec3::new(-400.0, 503.0, 0.0));
    assert_ne!(e.inference().map(|i| i.1), Some(crate::InferKind::OnGuide));
}

#[test]
fn an_open_part_s_guides_are_its_own_to_select() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 400.0, 0.0), "50");
    let part = e.document().resolve(&EntityPath::root(a)).unwrap().component;
    let line = GuideShape::Line { origin: DVec3::new(0.0, 100.0, 50.0), dir: DVec3::X };
    let g = e.apply(Operation::AddGuide { component: part, shape: line }).unwrap().unwrap().guide.unwrap();
    // Closed: the part's guide is not the context's.
    e.select(vec![]);
    click(&mut e, DVec3::new(200.0, 103.0, 50.0));
    assert_eq!(e.selection(), &[a], "the box, not its guide");
    // Open: its guide is taken, and erased from the part.
    e.input(ToolInput::DoubleClick { px: px(&e, DVec3::new(200.0, 200.0, 50.0)), mods: Modifiers::default() });
    assert_eq!(e.open_part(), Some(a));
    click(&mut e, DVec3::new(200.0, 103.0, 50.0));
    assert_eq!(e.selection(), &[g]);
    e.delete_selection();
    assert!(e.document().component(part).unwrap().guides.is_empty());
}

#[test]
fn a_guide_from_a_level_edge_is_offset_level_in_the_void() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "100");
    e.select(vec![]);
    // Seen askew: the top front edge is clicked where its front face is.
    let area = crate::camera::ViewArea::full(e.viewport_size());
    e.camera.set_standard_view(StandardView::Iso, Some((DVec3::splat(-500.0), DVec3::splat(500.0))), area);
    e.set_tool(ToolKind::Measure);
    let on_edge = e.camera.project(DVec3::new(90.0, 0.0, 100.0), e.viewport_size()).unwrap() + DVec2::new(0.0, 1.0);
    e.input(ToolInput::Hover { px: on_edge });
    e.input(ToolInput::Click { px: on_edge, mods: Modifiers::default() });
    assert_eq!(e.vcb().map(|v| v.0), Some("Décalage".into()), "{}", e.status());
    // The void in front of the box: the guide stays at the edge's height.
    hover(&mut e, DVec3::new(160.0, -200.0, 100.0));
    e.input(ToolInput::Click { px: px(&e, DVec3::new(160.0, -200.0, 100.0)), mods: Modifiers::default() });
    let [g] = &e.document().component(root).unwrap().guides[..] else { panic!() };
    let GuideShape::Line { origin, dir } = g.shape else { panic!() };
    assert!((origin.z - 100.0).abs() < 1e-3 && (origin.y + 200.0).abs() < 1e-3, "{origin}");
    assert!(dir.cross(DVec3::X).length() < 1e-9);
}

#[test]
fn the_tape_draws_a_guide_from_an_axis() {
    use solidsketch_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    e.set_tool(ToolKind::Measure);
    // On the red axis, in the void: a guide along it, offset level.
    click(&mut e, DVec3::new(-300.0, 4.0, 0.0));
    hover(&mut e, DVec3::new(-300.0, 150.0, 0.0));
    // The axis referred to, across the view.
    let refs: Vec<_> = e.preview().lines.into_iter().filter(|l| l.color == crate::tools::MAGENTA).collect();
    let [r] = refs[..] else { panic!("{refs:?}") };
    assert!(r.a.y.abs() < 1e-6 && r.b.y.abs() < 1e-6 && (r.b - r.a).cross(DVec3::X).length() < 1e-6 * (r.b - r.a).length(), "{r:?}");
    e.input(ToolInput::Vcb("100".into()));
    let [g] = &e.document().component(root).unwrap().guides[..] else { panic!("{}", e.status()) };
    let GuideShape::Line { origin, dir } = g.shape else { panic!() };
    assert!((origin.y - 100.0).abs() < 1e-6 && origin.z.abs() < 1e-6 && dir.cross(DVec3::X).length() < 1e-9, "{origin} {dir}");
}

/// A document with one box per (origin, size), added in that order.
fn boxes_doc(boxes: &[(DVec3, DVec3)]) -> (Document, Vec<Id>) {
    let mut d = Document::new();
    let mut ids = Vec::new();
    for &(origin, size) in boxes {
        let root = d.root();
        let primitive = Primitive::Box { origin, size };
        let (next, out) = d.apply(&Operation::AddPrimitive { context: root, primitive, placement: glam::DAffine3::IDENTITY }).unwrap();
        d = next;
        ids.push(out.created[0]);
    }
    (d, ids)
}

#[test]
fn picking_coincident_faces_prefers_the_smaller_solid_whatever_the_order() {
    let big = (DVec3::ZERO, DVec3::splat(100.0));
    let small = (DVec3::new(0.0, 0.0, 50.0), DVec3::new(50.0, 100.0, 50.0));
    for small_first in [false, true] {
        let (doc, ids) = boxes_doc(&if small_first { [small, big] } else { [big, small] });
        let small_id = ids[if small_first { 0 } else { 1 }];
        let e = top_editor(doc);
        // Both tops are at z = 100: the smaller box wins.
        let hit = e.scene().pick(&e.camera.ray(px(&e, DVec3::new(25.0, 50.0, 100.0)), e.viewport_size()), |_| true).unwrap();
        assert_eq!(hit.path, EntityPath::root(small_id), "small first: {small_first}");
        // Off the small one, the big one.
        let hit = e.scene().pick(&e.camera.ray(px(&e, DVec3::new(75.0, 50.0, 100.0)), e.viewport_size()), |_| true).unwrap();
        assert_ne!(hit.path, EntityPath::root(small_id));
    }
}

#[test]
fn picking_coincident_faces_prefers_the_solid_the_ray_enters() {
    // The lower box, entered at z = 100, is the bigger; the upper one is left there.
    let (doc, ids) = boxes_doc(&[(DVec3::new(0.0, 0.0, 100.0), DVec3::splat(100.0)), (DVec3::ZERO, DVec3::new(200.0, 200.0, 100.0))]);
    let e = top_editor(doc);
    let ray = crate::camera::Ray { origin: DVec3::new(50.0, 50.0, 150.0), dir: -DVec3::Z, t_min: 0.0 };
    let hit = e.scene().pick(&ray, |_| true).unwrap();
    assert_eq!(hit.path, EntityPath::root(ids[1]));
    assert!((hit.point.z - 100.0).abs() < 1e-6);
}

#[test]
fn copying_infers_on_the_original_left_in_place() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    let corner = DVec3::new(400.0, 200.0, 18.0);
    let kind = |e: &Editor| e.inference().map(|i| i.1);

    // Moved, the box goes away from where it was: its corner is not inferred on.
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(0.0, 0.0, 18.0));
    hover(&mut e, corner);
    assert_ne!(kind(&e), Some(crate::InferKind::Endpoint));

    // Copied, the original stays: its corner is.
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    hover(&mut e, corner);
    assert_eq!(kind(&e), Some(crate::InferKind::Endpoint));
    e.input(ToolInput::Click { px: px(&e, corner), mods: Modifiers::default() });
    let (min, max) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(min, DVec3::new(400.0, 200.0, 0.0)) && close(max, DVec3::new(800.0, 400.0, 18.0)), "{min:?} {max:?}");
}

/// A board 600 × 300 × 18 at the origin, drilled through at each `x` (y = 150), selected in a
/// top editor (0.5 px per mm) with the stretch tool.
fn drilled_board_editor(xs: &[f64]) -> (Editor, Id) {
    let doc = Document::new();
    let root = doc.root();
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 300.0, 18.0) };
    let (mut d, out) =
        doc.apply(&Operation::AddPrimitive { context: root, primitive: board, placement: glam::DAffine3::IDENTITY }).unwrap();
    let a = out.created[0];
    for &x in xs {
        let drill = Primitive::Cylinder { origin: DVec3::new(x, 150.0, -10.0), radius: 5.0, height: 40.0 };
        let (next, out) =
            d.apply(&Operation::AddPrimitive { context: root, primitive: drill, placement: glam::DAffine3::IDENTITY }).unwrap();
        let tool = EntityPath::root(out.created[0]);
        d = next
            .apply(&Operation::Boolean { target: EntityPath::root(a), tool, op: Boolean::Difference, scope: solidsketch_model::Scope::All })
            .unwrap()
            .0;
    }
    let mut e = top_editor(d);
    e.select(vec![a]);
    e.set_tool(ToolKind::Stretch);
    (e, a)
}

/// The board's blank extent along X, and its holes' places.
fn stretched(e: &Editor, a: Id) -> ((f64, f64), Vec<f64>) {
    let (_, p, _) = e.document().resolve_part(&EntityPath::root(a)).unwrap();
    let (lo, hi) = p.blank.vertex_bounds().unwrap();
    let mut xs: Vec<f64> = p.machinings.iter().map(|m| (m.placement.translation.x * 1e6).round() / 1e6).collect();
    xs.sort_by(f64::total_cmp);
    ((lo.x, hi.x), xs)
}

#[test]
fn stretch_tool_pulls_a_grip() {
    let (mut e, a) = drilled_board_editor(&[100.0, 250.0, 500.0]);
    // The grip at the centre of the end face, taken near it on screen: the overall length
    // typed; the cutter in the middle: the far hole moves, the others stay.
    click(&mut e, DVec3::new(600.0, 150.0, 18.0));
    assert!(e.status().contains("valider l'étirement"), "{}", e.status());
    e.input(ToolInput::Vcb("800".into()));
    assert_eq!(stretched(&e, a), ((0.0, 800.0), vec![100.0, 250.0, 700.0]), "{}", e.status());

    // Back to the grips on the same part: the offset, −100.
    e.set_tool_option("measure", OptionValue::Choice("offset".into()));
    click(&mut e, DVec3::new(800.0, 150.0, 18.0));
    e.input(ToolInput::Vcb("-100".into()));
    assert_eq!(stretched(&e, a), ((0.0, 700.0), vec![100.0, 250.0, 600.0]));

    // Centred, overall 900: both ends 100 out. Typed too short: said, nothing done.
    e.set_tool_option("measure", OptionValue::Choice("overall".into()));
    e.set_tool_option("centered", OptionValue::Toggle(true));
    click(&mut e, DVec3::new(700.0, 150.0, 18.0));
    e.input(ToolInput::Vcb("100".into()));
    assert!(e.status().contains("longueur minimale"), "{}", e.status());
    e.input(ToolInput::Vcb("900".into()));
    // The origin's end moved by −100: the origin follows it (OCL's anchor), the part's axes
    // with it — in the world, the board spans −100..800, its holes at 0, 150, 700.
    assert_eq!(stretched(&e, a), ((0.0, 900.0), vec![100.0, 250.0, 800.0]));

    // A cutter added (copy modifier held) at 100: the hole at 150 then takes half the move.
    e.set_tool_option("centered", OptionValue::Toggle(false));
    e.set_copy(true);
    click(&mut e, DVec3::new(100.0, 100.0, 18.0));
    e.set_copy(false);
    click(&mut e, DVec3::new(800.0, 150.0, 18.0));
    e.input(ToolInput::Vcb("1000".into()));
    assert_eq!(stretched(&e, a), ((0.0, 1000.0), vec![100.0, 300.0, 900.0]));
    // One undo step each.
    e.undo();
    assert_eq!(stretched(&e, a), ((0.0, 900.0), vec![100.0, 250.0, 800.0]));
}

#[test]
fn stretch_tool_cutters_are_dragged_added_and_removed() {
    let (mut e, a) = drilled_board_editor(&[]);
    // The cutters' planes, by where they cross the board (seen from the top: a line each).
    let cutters = |e: &Editor| {
        let mut xs: Vec<i64> = e.view().fills.iter().map(|f| f.corners[0].x.round() as i64).collect();
        xs.sort();
        xs
    };
    let at = |e: &Editor, x: f64| px(e, DVec3::new(x, 50.0, 18.0));
    // No axis yet: no cutters; a grip hovered makes its axis the active one.
    e.input(ToolInput::Hover { px: at(&e, 150.0) });
    assert_eq!(cutters(&e), Vec::<i64>::new());
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, 150.0, 18.0)) });
    e.input(ToolInput::Hover { px: at(&e, 150.0) });
    assert_eq!(cutters(&e), vec![300]);
    // Dragged from 300 to 450.
    e.input(ToolInput::Hover { px: at(&e, 300.0) });
    e.input(ToolInput::DragStart { px: at(&e, 300.0), mods: Modifiers::default() });
    e.input(ToolInput::Drag { px: at(&e, 450.0) });
    e.input(ToolInput::DragEnd { px: at(&e, 450.0), mods: Modifiers::default() });
    assert_eq!(cutters(&e), vec![450]);
    // One more at 200 (copy modifier held: shown where it would go, then added).
    e.set_copy(true);
    e.input(ToolInput::Hover { px: at(&e, 200.0) });
    assert_eq!(cutters(&e), vec![200, 450]);
    click(&mut e, DVec3::new(200.0, 50.0, 18.0));
    e.set_copy(false);
    assert_eq!(cutters(&e), vec![200, 450]);
    // 200 dragged to the end of the board: removed.
    e.input(ToolInput::DragStart { px: at(&e, 200.0), mods: Modifiers::default() });
    e.input(ToolInput::Drag { px: at(&e, -100.0) });
    e.input(ToolInput::DragEnd { px: at(&e, -100.0), mods: Modifiers::default() });
    assert_eq!(cutters(&e), vec![450]);
    // Kept on the part: one undo step for the whole run of changes.
    let part = e.document().resolve(&EntityPath::root(a)).unwrap().component;
    let saved = |e: &Editor| e.document().component(part).unwrap().behavior.clone();
    assert_eq!(saved(&e).cutters.map(|c| c[0].iter().map(|r| (r * 600.0).round() as i64).collect::<Vec<_>>()), Some(vec![450]));

    // The other modifier held: a click removes; the last one locks the axis — no cutters, no
    // red grips, a padlock at its end.
    e.set_alt(true);
    click(&mut e, DVec3::new(450.0, 50.0, 18.0));
    e.set_alt(false);
    assert_eq!(cutters(&e), Vec::<i64>::new());
    assert!(saved(&e).no_stretch.locks(0));
    let end = DVec3::new(600.0, 150.0, 9.0);
    let padlock =
        |e: &Editor| e.overlay_points().iter().any(|p| p.shape == crate::tools::PointShape::Padlock && (p.at - end).length() < 1e-6);
    assert!(padlock(&e));
    e.input(ToolInput::Hover { px: px(&e, end) });
    assert!(e.status().contains("débloquer"), "{}", e.status());
    // The padlock clicked: unlocked, one cutter in the middle.
    click(&mut e, end);
    assert!(!padlock(&e) && !saved(&e).no_stretch.locks(0));
    e.input(ToolInput::Hover { px: at(&e, 150.0) });
    assert_eq!(cutters(&e), vec![300]);
    // Undone in one step: as it was, read again by the tool.
    e.undo();
    assert!(saved(&e).is_default());
    e.input(ToolInput::Hover { px: at(&e, 150.0) });
    assert_eq!(cutters(&e), vec![300]);
}

#[test]
fn stretch_tool_shows_the_locks_at_once() {
    // The board's length locked: its padlock drawn as soon as the tool takes the board.
    let (mut e, a) = drilled_board_editor(&[]);
    let part = e.document().resolve(&EntityPath::root(a)).unwrap().component;
    let behavior = solidsketch_model::Behavior { no_stretch: solidsketch_model::StretchMask(1), cutters: None };
    e.apply(Operation::SetBehavior { component: part, behavior }).unwrap();
    let padlock = |e: &Editor| e.overlay_points().iter().any(|p| p.shape == crate::tools::PointShape::Padlock);
    // Started on the selection: before any input.
    e.set_tool(ToolKind::Select);
    e.set_tool(ToolKind::Stretch);
    assert!(padlock(&e));
    // Picked by a click: right after it.
    e.select(Vec::new());
    e.set_tool(ToolKind::Select);
    e.set_tool(ToolKind::Stretch);
    assert!(!padlock(&e));
    click(&mut e, DVec3::new(300.0, 150.0, 18.0));
    assert!(padlock(&e));
}

#[test]
fn stretch_tool_moves_an_interior_section() {
    let (mut e, a) = drilled_board_editor(&[100.0, 250.0, 500.0]);
    let on_axis = |x: f64| DVec3::new(x, 150.0, 18.0);
    let cutters = |e: &Editor| {
        let mut xs: Vec<i64> = e.view().fills.iter().map(|f| f.corners[0].x.round() as i64).collect();
        xs.sort();
        xs
    };
    // The red axis active, a second cutter at 200: the section 200–300 (the hole at 250) gets a
    // handle, drawn when hovered (bold, with its arrows).
    e.input(ToolInput::Hover { px: px(&e, on_axis(600.0)) });
    e.set_copy(true);
    click(&mut e, DVec3::new(200.0, 100.0, 18.0));
    e.set_copy(false);
    assert_eq!(cutters(&e), vec![200, 300]);
    let bold = |e: &Editor| e.view().overlay.iter().filter(|l| l.bold).count();
    e.input(ToolInput::Hover { px: px(&e, on_axis(400.0)) });
    assert_eq!(bold(&e), 0);
    e.input(ToolInput::Hover { px: px(&e, on_axis(230.0)) });
    assert_eq!(bold(&e), 5, "the segment and its four arrow strokes");
    assert!(e.status().contains("entre les deux plans de coupe"), "{}", e.status());

    // Taken and moved 50 along the axis (typed): the hole moves, the length stays; the measure
    // is the offset whatever the option; the cutters go back to the middle of the gaps between
    // the matter (the holes at 100, 300 and 500).
    click(&mut e, on_axis(230.0));
    assert_eq!(e.vcb().unwrap().0, "Décalage");
    assert!(e.status().contains("valider le déplacement"), "{}", e.status());
    e.input(ToolInput::Vcb("50".into()));
    assert_eq!(stretched(&e, a), ((0.0, 600.0), vec![100.0, 300.0, 500.0]), "{}", e.status());
    e.input(ToolInput::Hover { px: px(&e, on_axis(420.0)) });
    assert_eq!(cutters(&e), vec![200, 400]);
    // Saved on the part by the stretch itself: undone with it.
    let part = e.document().resolve(&EntityPath::root(a)).unwrap().component;
    let saved = |e: &Editor| {
        let c = e.document().component(part).unwrap().behavior.cutters.clone();
        c.map(|c| c[0].iter().map(|r| (r * 600.0).round() as i64).collect::<Vec<_>>())
    };
    assert_eq!(saved(&e), Some(vec![200, 400]));
    e.undo();
    assert_eq!(saved(&e), Some(vec![200, 300]));
    e.redo();

    // Too far towards the start (away from the grip of the blue axis, at the centre): the gap to the matter before (105–295) keeps 1 mm.
    click(&mut e, on_axis(260.0));
    e.input(ToolInput::Vcb("-200".into()));
    assert!(e.status().contains("compression maximale de 189"), "{}", e.status());
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });

    // Shift held over the handle: its start taken anywhere, here the far hole's centre (a
    // diamond: from the geometry), then moved 20.
    e.input(ToolInput::Hover { px: px(&e, on_axis(260.0)) });
    e.set_shift(true);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 150.0, 18.0)) });
    let points = e.overlay_points();
    let grab = points.iter().find(|p| p.shape == crate::tools::PointShape::Diamond).expect("a diamond");
    assert!((grab.at.x - 500.0).abs() < 1e-6, "{}", grab.at);
    click(&mut e, DVec3::new(500.0, 150.0, 18.0));
    e.set_shift(false);
    e.input(ToolInput::Vcb("20".into()));
    assert_eq!(stretched(&e, a), ((0.0, 600.0), vec![100.0, 320.0, 500.0]), "{}", e.status());
}

#[test]
fn stretch_tool_refuses_what_cannot_stretch_and_takes_the_open_part() {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let rod = Primitive::Cylinder { origin: DVec3::ZERO, radius: 50.0, height: 200.0 };
    let rod =
        e.apply(Operation::AddPrimitive { context: root, primitive: rod, placement: glam::DAffine3::IDENTITY }).unwrap().unwrap().created
            [0];
    e.select(vec![rod]);
    e.set_tool(ToolKind::Stretch);
    // The red axis locked (→): across it, the cylinder cannot stretch.
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    click(&mut e, DVec3::new(50.0, 0.0, 200.0));
    assert!(e.status().contains("traverse une coupe"), "{}", e.status());
    let before = e.document().resolve_part(&EntityPath::root(rod)).unwrap().1.blank.clone();

    // Along its axis it stretches — the top grip, nearer the eye than the bottom one under it;
    // in the open part, the part's own scope (no option of the tool's).
    e.set_tool(ToolKind::Select);
    e.enter(rod);
    e.set_tool(ToolKind::Stretch);
    assert!(e.tool_options().iter().all(|o| o.key != "scope"));
    click(&mut e, DVec3::new(0.0, 0.0, 200.0));
    e.input(ToolInput::Vcb("300".into()));
    let after = e.document().resolve_part(&EntityPath::root(rod)).unwrap().1.blank.clone();
    assert!(!std::sync::Arc::ptr_eq(&before, &after), "{}", e.status());
    let (lo, hi) = after.bounds().unwrap();
    assert!((lo.z - 0.0).abs() < 1e-6 && (hi.z - 300.0).abs() < 1e-6, "{lo} {hi}");
}

#[test]
fn stretch_tool_draws_the_part_stretched_while_pulling() {
    let (mut e, a) = drilled_board_editor(&[500.0]);
    let root = e.document().root();
    // A copy of the board, further up.
    let b = e
        .apply(Operation::Duplicate { context: root, entities: vec![a], transform: glam::DAffine3::from_translation(DVec3::Y * 400.0) })
        .unwrap()
        .unwrap()
        .created[0];
    e.select(vec![a]);
    let key = crate::scene::PREVIEW_KEY;
    let stretched = |e: &Editor| e.view().batches.iter().find(|(k, _)| *k == key).map_or(0, |(_, ps)| ps.len());
    let part = e.document().resolve_part(&EntityPath::root(a)).unwrap().0;
    let drawn_plain = |e: &Editor| e.view().batches.iter().find(|(k, _)| *k == part).map_or(0, |(_, ps)| ps.len());

    // Pulled 200 out: both boards drawn stretched (scope "all"), not as they are; nothing but
    // the measure drawn over them (no box, no cutters).
    click(&mut e, DVec3::new(600.0, 150.0, 18.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(800.0, 150.0, 18.0)) });
    assert_eq!((stretched(&e), drawn_plain(&e)), (2, 0));
    assert!(e.view().fills.is_empty());
    let mesh = &e.scene().definition_mesh(key).unwrap().parts[0].mesh;
    let max_x = mesh.positions.iter().map(|p| mesh.origin.x + p[0] as f64).fold(f64::MIN, f64::max);
    assert!((max_x - 800.0).abs() < 1e-3, "{max_x}");
    // Marked as changing, on the stretched shape: the original is not drawn again. Not marked
    // selected: the tool shows what it took its own way.
    assert!(e.view().highlights.iter().all(|h| h.definition != part));
    assert!(e.view().highlights.iter().all(|h| h.mark != Mark::Selected));
    assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Related && h.definition == key));

    // Only this one: the other drawn as it is.
    e.set_tool_option("scope", OptionValue::Choice("this".into()));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(790.0, 150.0, 18.0)) });
    assert_eq!((stretched(&e), drawn_plain(&e)), (1, 1));

    // Done: drawn from the document again.
    e.input(ToolInput::Click { px: px(&e, DVec3::new(790.0, 150.0, 18.0)), mods: Modifiers::default() });
    assert_eq!(stretched(&e), 0);
    assert_ne!(e.document().resolve_part(&EntityPath::root(a)).unwrap().0, e.document().resolve_part(&EntityPath::root(b)).unwrap().0);
}

#[test]
fn a_compressed_preview_turns_no_triangle_over() {
    // Holes near both ends: the top face's triangles span the cutter.
    let (mut e, a) = drilled_board_editor(&[100.0, 500.0]);
    let part = e.document().resolve_part(&EntityPath::root(a)).unwrap().0;
    click(&mut e, DVec3::new(600.0, 150.0, 18.0));
    // As far as it goes (the holes 1 mm apart).
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 150.0, 18.0)) });
    // Each triangle faces the way of its vertices' normals (the surface's, which a stretch along
    // the axis keeps), as before.
    let turned = |m: &solidsketch_model::kernel::Mesh| {
        let p = |i: u32| DVec3::from_array(m.positions[i as usize].map(f64::from));
        let n = |i: u32| DVec3::from_array(m.normals[i as usize].map(f64::from));
        m.indices
            .chunks(3)
            .filter(|t| {
                let g = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
                g.length() > 1e-6 && g.dot(n(t[0]) + n(t[1]) + n(t[2])) < 0.0
            })
            .count()
    };
    let before = &e.scene().definition_mesh(part).unwrap().parts[0].mesh;
    assert_eq!(turned(before), 0);
    let after = &e.scene().definition_mesh(crate::scene::PREVIEW_KEY).unwrap().parts[0].mesh;
    assert_eq!(turned(after), 0);
}

#[test]
fn a_stretch_stays_drawn_stretched_while_computed() {
    let (mut e, k) = deferred_editor(Document::new());
    let cube = add_cube(&mut e, &k);
    e.select(vec![cube]);
    e.set_tool(ToolKind::Stretch);
    let key = crate::scene::PREVIEW_KEY;
    let part = e.document().resolve_part(&EntityPath::root(cube)).unwrap().0;
    let drawn = |e: &Editor, def: Id| e.view().batches.iter().find(|(k, _)| *k == def).map_or(0, |(_, ps)| ps.len());
    // The grip on +X pulled 300 out, then the click that applies it.
    click(&mut e, DVec3::new(100.0, 50.0, 50.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(400.0, 50.0, 50.0)) });
    assert_eq!((drawn(&e, key), drawn(&e, part)), (1, 0));
    e.input(ToolInput::Click { px: px(&e, DVec3::new(400.0, 50.0, 50.0)), mods: Modifiers::default() });
    // While the kernel works: still drawn stretched, without the tool's grips or cutters.
    assert!(e.busy());
    assert_eq!((drawn(&e, key), drawn(&e, part)), (1, 0));
    assert!(e.view().fills.is_empty() && e.overlay_points().is_empty());
    // Done: drawn from the document, stretched.
    k.run_all();
    assert!(e.poll());
    assert!(!e.busy());
    assert_eq!(drawn(&e, key), 0);
    assert!(e.scene().definition_mesh(key).is_none());
    let (_, hi) = bounds_of(&e, &EntityPath::root(cube));
    assert!((hi.x - 400.0).abs() < 1e-3, "{hi}");
}

#[test]
fn stretch_tool_stretches_an_assembly() {
    // A frame made an assembly: two rails 0–600 along X (one part), two legs at the ends (one
    // part), seen from the top.
    let doc = Document::new();
    let root = doc.root();
    let add = |d: Document, p: Primitive| {
        let (d, out) = d.apply(&Operation::AddPrimitive { context: root, primitive: p, placement: glam::DAffine3::IDENTITY }).unwrap();
        (d, out.created[0])
    };
    let copy = |d: Document, id: Id, by: DVec3| {
        let (d, out) =
            d.apply(&Operation::Duplicate { context: root, entities: vec![id], transform: glam::DAffine3::from_translation(by) }).unwrap();
        (d, out.created[0])
    };
    let (d, rail) = add(doc, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0, 20.0, 40.0) });
    let (d, leg) = add(d, Primitive::Box { origin: DVec3::new(0.0, 0.0, -400.0), size: DVec3::new(50.0, 50.0, 400.0) });
    let (d, rail2) = copy(d, rail, DVec3::Y * 300.0);
    let (d, leg2) = copy(d, leg, DVec3::X * 550.0);
    let (d, out) = d.apply(&Operation::MakeAssembly { context: root, entities: vec![rail, rail2, leg, leg2], name: None }).unwrap();
    let a = out.created[0];
    let mut e = top_editor(d);
    e.set_tool(ToolKind::Stretch);
    let part_of = |e: &Editor, o: Id| e.document().resolve(&EntityPath::new(vec![a], o)).unwrap().component;
    let (rail_part, leg_part) = (part_of(&e, rail), part_of(&e, leg));

    // A click on a rail takes the assembly, as the selection would; its box spans the frame.
    click(&mut e, DVec3::new(100.0, 10.0, 40.0));
    assert!(e.status().contains("Cliquer une poignée"), "{}", e.status());
    // Its grip at the end of the frame pulled 200: the rails drawn stretched (one mesh, both
    // occurrences), the far leg drawn moved, the near one as it is.
    click(&mut e, DVec3::new(600.0, 160.0, 18.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(800.0, 160.0, 18.0)) });
    let key = crate::scene::PREVIEW_KEY;
    let batch = |e: &Editor, def: Id| e.view().batches.iter().filter(|(k, _)| *k == def).flat_map(|(_, ps)| ps.clone()).collect::<Vec<_>>();
    assert_eq!(batch(&e, key).len(), 2);
    assert!(batch(&e, rail_part).is_empty());
    let mut legs: Vec<i64> = batch(&e, leg_part).iter().map(|p| p.transform.translation.x.round() as i64).collect();
    legs.sort();
    assert_eq!(legs, vec![0, 750]);
    // Not inferred where it was: the far leg, moved; the near one, in place, still.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(550.0, 0.0, 0.0)) });
    assert!(e.inference().is_none(), "{:?}", e.inference());
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(50.0, 50.0, 0.0)) });
    assert!(e.inference().is_some());
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(800.0, 160.0, 18.0)) });

    // Typed: 900 overall. The rails stretched, the far leg moved, the parts kept.
    e.input(ToolInput::Vcb("900".into()));
    let x = |e: &Editor, o: Id| bounds_of(e, &EntityPath::new(vec![a], o));
    assert!((x(&e, rail).1.x - 900.0).abs() < 1e-6 && (x(&e, rail2).1.x - 900.0).abs() < 1e-6, "{}", e.status());
    assert!((x(&e, leg2).0.x - 850.0).abs() < 1e-6 && (x(&e, leg).0.x).abs() < 1e-6);
    assert_eq!((part_of(&e, rail), part_of(&e, rail2), part_of(&e, leg2)), (rail_part, rail_part, leg_part));
    assert_eq!(batch(&e, key).len(), 0);
}

/// Three boards of one part at the root, 0–200 along X, 100 apart along Y (seen from the top).
fn three_boards_editor() -> (Editor, [Id; 3]) {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) };
    let b1 =
        e.apply(Operation::AddPrimitive { context: root, primitive: board, placement: glam::DAffine3::IDENTITY }).unwrap().unwrap().created
            [0];
    let mut copy = |by: f64| {
        let t = glam::DAffine3::from_translation(DVec3::Y * by);
        e.apply(Operation::Duplicate { context: root, entities: vec![b1], transform: t }).unwrap().unwrap().created[0]
    };
    let (b2, b3) = (copy(100.0), copy(300.0));
    (e, [b1, b2, b3])
}

#[test]
fn stretch_tool_takes_several_occurrences_by_a_rectangle() {
    let (mut e, [b1, b2, b3]) = three_boards_editor();
    e.set_tool(ToolKind::Stretch);
    let part = e.document().resolve(&EntityPath::root(b1)).unwrap().component;
    // A rectangle around the first two, left to right: they are selected and taken.
    drag(&mut e, DVec3::new(-10.0, -10.0, 0.0), DVec3::new(210.0, 160.0, 0.0), Modifiers::default());
    assert_eq!(e.selection(), [b1, b2]);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 500.0, 0.0)) });
    assert!(e.status().contains("Cliquer une poignée"), "{}", e.status());
    // The grip at the end of their common box, overall 300: both stretched, one part still.
    click(&mut e, DVec3::new(200.0, 75.0, 18.0));
    assert!(e.status().contains("valider l'étirement"), "{}", e.status());
    // Drawn stretched while pulling: one preview mesh, both occurrences; the third shows it
    // too (scope « all »).
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(400.0, 75.0, 18.0)) });
    let previewed = e.view().batches.iter().filter(|(k, _)| *k == crate::scene::PREVIEW_KEY).map(|(_, ps)| ps.len()).sum::<usize>();
    assert_eq!(previewed, 3);
    e.input(ToolInput::Vcb("300".into()));
    let x = |e: &Editor, o: Id| bounds_of(e, &EntityPath::root(o)).1.x;
    assert_eq!((x(&e, b1).round(), x(&e, b2).round(), x(&e, b3).round()), (300.0, 300.0, 300.0), "{}", e.status());
    assert_eq!(e.document().resolve(&EntityPath::root(b2)).unwrap().component, part);
    // One undo step.
    e.undo();
    assert_eq!((x(&e, b1).round(), x(&e, b3).round()), (200.0, 200.0));
}

#[test]
fn stretch_tool_gathers_occurrences_while_shift_is_held() {
    let (mut e, [b1, b2, b3]) = three_boards_editor();
    e.set_tool(ToolKind::Stretch);
    e.set_tool_option("scope", OptionValue::Choice("this".into()));
    let part = e.document().resolve(&EntityPath::root(b1)).unwrap().component;
    let shift = Modifiers { toggle: true, copy: false };
    // Shift held, the first and the third clicked: gathered, not taken yet.
    e.set_shift(true);
    for p in [DVec3::new(100.0, 25.0, 18.0), DVec3::new(100.0, 325.0, 18.0)] {
        e.input(ToolInput::Hover { px: px(&e, p) });
        e.input(ToolInput::Click { px: px(&e, p), mods: shift });
    }
    assert_eq!(e.selection(), [b1, b3]);
    assert!(e.status().contains("Sélection multiple"), "{}", e.status());
    let marked = |e: &Editor| e.view().highlights.iter().any(|h| h.mark == Mark::Selected);
    assert!(marked(&e));
    // Shift released: taken, their box 0–375 along Y; the selection kept but no longer drawn
    // (the grips and cutters show what is taken).
    e.set_shift(false);
    assert!(e.status().contains("Cliquer une poignée"), "{}", e.status());
    assert_eq!(e.selection(), [b1, b3]);
    assert!(!marked(&e));
    click(&mut e, DVec3::new(200.0, 175.0, 18.0));
    e.input(ToolInput::Vcb("250".into()));
    // Only these: one new part for both, the second as it was.
    let doc = e.document();
    let of = |o: Id| doc.resolve(&EntityPath::root(o)).unwrap().component;
    assert_eq!((of(b1) == of(b3), of(b1) != part, of(b2)), (true, true, part), "{}", e.status());
    let x = |o: Id| bounds_of(&e, &EntityPath::root(o)).1.x.round();
    assert_eq!((x(b1), x(b2), x(b3)), (250.0, 200.0, 250.0));
}

#[test]
fn stretch_tool_stretches_along_the_axes_chosen() {
    // A board turned square: its own X along the world's Y (−50–0 along X, 0–200 along Y).
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) };
    let turn = glam::DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    let b = e.apply(Operation::AddPrimitive { context: root, primitive: board, placement: turn }).unwrap().unwrap().created[0];
    e.set_tool(ToolKind::Stretch);
    click(&mut e, DVec3::new(-25.0, 100.0, 18.0));
    // In its own axes (the default), its end grip, overall 300.
    click(&mut e, DVec3::new(-25.0, 200.0, 18.0));
    e.input(ToolInput::Vcb("300".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(b));
    assert!(close(lo, DVec3::new(-50.0, 0.0, 0.0)) && close(hi, DVec3::new(0.0, 300.0, 18.0)), "{lo} {hi} {}", e.status());
    // In the context's: the world's X, across its width, overall 80.
    e.set_tool_option("axes", OptionValue::Choice("context".into()));
    click(&mut e, DVec3::new(0.0, 150.0, 18.0));
    e.input(ToolInput::Vcb("80".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(b));
    assert!(close(lo, DVec3::new(-50.0, 0.0, 0.0)) && close(hi, DVec3::new(30.0, 300.0, 18.0)), "{lo} {hi} {}", e.status());
}

#[test]
fn stretch_tool_in_the_context_axes_refuses_a_part_turned_across_a_cutter() {
    // A board turned 45°: in the context's axes its box is its mesh's, and no face of it runs
    // along the world's X — refused, named.
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) };
    let turn = glam::DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_4);
    e.apply(Operation::AddPrimitive { context: root, primitive: board, placement: turn }).unwrap().unwrap();
    e.set_tool(ToolKind::Stretch);
    e.set_tool_option("axes", OptionValue::Choice("context".into()));
    // Its corners: (0, 0), (141, 141), (106, 177), (−35, 35); the grip of its box's +X face.
    click(&mut e, DVec3::new(70.0, 70.0, 18.0));
    let s = std::f64::consts::FRAC_1_SQRT_2;
    click(&mut e, DVec3::new(200.0 * s, 125.0 * s, 9.0));
    assert!(e.status().contains("traverse une coupe"), "{}", e.status());
}

#[test]
fn the_scene_index_picks_as_a_scan_does() {
    // Boxes in a grid, some stacked exactly (coincident faces: the scene's order breaks the
    // tie), some overlapping, a few gathered in an assembly copied, seen in perspective.
    let mut doc = Document::new();
    let root = doc.root();
    let mut made = Vec::new();
    for i in 0..12 {
        for j in 0..12 {
            let at = DVec3::new(i as f64 * 70.0, j as f64 * 70.0, ((i * j) % 3) as f64 * 20.0);
            let size = DVec3::new(60.0 + (i % 3) as f64 * 15.0, 60.0, 40.0 + (j % 4) as f64 * 10.0);
            let (d, out) = doc
                .apply(&Operation::AddPrimitive {
                    context: root,
                    primitive: Primitive::Box { origin: at, size },
                    placement: glam::DAffine3::IDENTITY,
                })
                .unwrap();
            doc = d;
            made.push(out.created[0]);
        }
    }
    let stacked: Vec<Id> = made.iter().copied().step_by(5).collect();
    doc = doc.apply(&Operation::Duplicate { context: root, entities: stacked, transform: glam::DAffine3::IDENTITY }).unwrap().0;
    let (d, out) = doc.apply(&Operation::MakeAssembly { context: root, entities: made[..20].to_vec(), name: None }).unwrap();
    doc = d
        .apply(&Operation::Duplicate {
            context: root,
            entities: out.created,
            transform: glam::DAffine3::from_translation(DVec3::new(0.0, 0.0, 100.0)),
        })
        .unwrap()
        .0;
    let mut e = top_editor(doc);
    e.set_parallel(false);
    e.camera.set_standard_view(
        StandardView::Iso,
        Some((DVec3::ZERO, DVec3::new(900.0, 900.0, 200.0))),
        crate::camera::ViewArea::full(e.viewport_size()),
    );
    let (mut hits, mut rays) = (0, 0);
    for x in (0..1000).step_by(23) {
        for y in (0..1000).step_by(23) {
            let ray = e.camera.ray(DVec2::new(x as f64, y as f64), e.viewport_size());
            for inferable in [false, true] {
                let found = if inferable { e.scene().pick_inferable(&ray, |_| true) } else { e.scene().pick(&ray, |_| true) };
                assert_eq!(found, e.scene().pick_by_scan(&ray, inferable), "{x} {y}");
                hits += usize::from(found.is_some());
                rays += 1;
            }
        }
    }
    assert!(hits > rays / 4 && hits < rays, "{hits} / {rays}");
}

#[test]
fn a_window_takes_an_assembly_only_whole() {
    let mut doc = Document::new();
    let root = doc.root();
    let add = |doc: &mut Document, at: DVec3| {
        let (d, out) = doc
            .apply(&Operation::AddPrimitive {
                context: root,
                primitive: Primitive::Box { origin: at, size: DVec3::splat(100.0) },
                placement: glam::DAffine3::IDENTITY,
            })
            .unwrap();
        *doc = d;
        out.created[0]
    };
    let a = add(&mut doc, DVec3::ZERO);
    let far = add(&mut doc, DVec3::new(600.0, 0.0, 0.0));
    let b = add(&mut doc, DVec3::new(200.0, 0.0, 0.0));
    let (d, out) = doc.apply(&Operation::MakeAssembly { context: root, entities: vec![b, far], name: None }).unwrap();
    doc = d;
    let asm = out.created[0];
    let mut e = top_editor(doc);
    let none = Modifiers::default();
    // Left to right around the first box and the assembly's near one: the box alone (the
    // assembly's far box is out); right to left, both.
    drag(&mut e, DVec3::new(-10.0, 150.0, 0.0), DVec3::new(310.0, -50.0, 0.0), none);
    assert_eq!(e.selection(), &[a]);
    drag(&mut e, DVec3::new(310.0, -50.0, 0.0), DVec3::new(-10.0, 150.0, 0.0), none);
    let mut taken = e.selection().to_vec();
    taken.sort();
    let mut both = vec![a, asm];
    both.sort();
    assert_eq!(taken, both);
    // Left to right around everything: both.
    drag(&mut e, DVec3::new(-10.0, 150.0, 0.0), DVec3::new(710.0, -50.0, 0.0), none);
    assert_eq!(e.selection().len(), 2);
}

#[test]
fn a_step_file_is_imported_into_the_context_in_one_step() {
    let mut e = top_editor(Document::new());
    e.import_step("assembly.step".into(), include_bytes!("../../kernel/tests/fixtures/assembly.step").to_vec());
    assert!(!e.is_importing());
    let doc = e.document();
    assert_eq!(doc.root_assembly().occurrences.len(), 1);
    assert_eq!(doc.placed_parts().len(), 2);
    // What came in is selected; one undo takes it all out.
    assert_eq!(e.selection(), &[doc.root_assembly().occurrences[0].id][..]);
    assert!(e.status().contains("assembly.step"), "{}", e.status());
    e.undo();
    assert!(e.document().root_assembly().occurrences.is_empty());
}
